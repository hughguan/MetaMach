//! MetaMach Studio Sidecar Binary (ADR-032).
//!
//! Provides a Web Observer dashboard, REST API, and WebSocket progress stream
//! proxying queries to `janus-daemon` over UDS (`janus.sock`). Zero web dependency
//! inside core daemon.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use clap::Parser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use janus::paths;
use janus::protocol::{Request, Response as UdsResponse};
use janus::uds;

#[derive(Parser, Debug)]
#[command(
    name = "janus-studio",
    about = "MetaMach Studio Web Observer & Visual Workflow Canvas Sidecar"
)]
struct Cli {
    /// Interface IP address to bind (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Listening HTTP port (default: 8444)
    #[arg(long, default_value_t = 8444)]
    port: u16,

    /// Path to authentication token file
    #[arg(long)]
    token_file: Option<PathBuf>,

    /// Path to janus-daemon UDS socket file
    #[arg(long)]
    sock: Option<PathBuf>,

    /// Optional TLS certificate file path
    #[arg(long)]
    tls_cert: Option<PathBuf>,

    /// Optional TLS private key file path
    #[arg(long)]
    tls_key: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    sock_path: PathBuf,
    auth_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    daemon_online: bool,
    version: String,
}

#[derive(Debug, Deserialize)]
struct DispatchPayload {
    blueprint: String,
    workflow: Option<String>,
    inline_command: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GateVerdictPayload {
    approve: bool,
}

#[derive(Debug, Deserialize)]
struct SaveWorkflowPayload {
    content: String,
}

#[derive(Debug, Deserialize)]
struct BlueprintQuery {
    blueprint: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let sock_path = cli
        .sock
        .unwrap_or_else(|| paths::state_dir().join("janus.sock"));

    let token_file = cli.token_file.unwrap_or_else(|| {
        let dir = dirs_home().unwrap_or_else(|| PathBuf::from("."));
        dir.join(".metamach/studio.token")
    });

    let auth_token = ensure_auth_token(&token_file)?;

    let state = AppState {
        sock_path,
        auth_token: auth_token.clone(),
    };

    let app = axum::Router::new()
        .route("/", get(serve_index))
        .route("/styles.css", get(serve_styles))
        .route("/app.js", get(serve_app))
        .route("/api/v1/health", get(handle_health))
        .route(
            "/api/v1/workflows",
            get(handle_list_workflows).layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/api/v1/workflows/:name",
            get(handle_get_workflow).post(handle_save_workflow).layer(
                axum::middleware::from_fn_with_state(state.clone(), auth_middleware),
            ),
        )
        .route(
            "/api/v1/progress",
            get(handle_progress).layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/api/v1/dispatch",
            post(handle_dispatch).layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route(
            "/api/v1/gates/:task_id/verdict",
            post(handle_gate_verdict).layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            )),
        )
        .route("/runs/:id/stream", get(handle_ws_stream))
        .with_state(state);

    let bind = &cli.bind;
    let port = cli.port;
    let addr: SocketAddr = format!("{bind}:{port}")
        .parse()
        .context("invalid bind/port configuration")?;

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("❌ Cannot bind to {bind}:{port} — address/port is already in use.");
            eprintln!(
                "💡 Tip: Use 'janus studio --port <PORT>' to specify a custom port, or stop the process using port {port}."
            );
            std::process::exit(1);
        }
        Err(e) => return Err(e).context("failed to bind TCP listener"),
    };

    let scheme = if cli.tls_cert.is_some() && cli.tls_key.is_some() {
        "https"
    } else {
        "http"
    };

    println!("🚀 MetaMach Studio (v0.6.0) listening on {scheme}://{addr}");
    println!("🔐 Auth Token: {auth_token}");

    if cli.tls_cert.is_some() || cli.tls_key.is_some() {
        println!("🔒 TLS Configuration: Enabled");
    }

    axum::serve(listener, app).await?;

    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn ensure_auth_token(token_file: &PathBuf) -> Result<String> {
    if token_file.exists()
        && let Ok(tok) = std::fs::read_to_string(token_file)
    {
        let trimmed = tok.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    if let Some(parent) = token_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let token = Uuid::new_v4().simple().to_string();
    std::fs::write(token_file, &token)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(token_file, std::fs::Permissions::from_mode(0o600));
    }

    Ok(token)
}

async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    if let Some(token_hdr) = headers.get("x-janus-studio-token")
        && let Ok(token_str) = token_hdr.to_str()
        && token_str == state.auth_token
    {
        return Ok(next.run(request).await);
    }
    Err(StatusCode::UNAUTHORIZED)
}

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../studio_assets/index.html"))
}

async fn serve_styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("../studio_assets/styles.css"),
    )
}

async fn serve_app() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../studio_assets/app.js"),
    )
}

async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let ping_res = uds::request_to(&state.sock_path, &Request::Ping, Duration::from_secs(2));
    let daemon_online = matches!(ping_res, Ok(UdsResponse::Pong));
    Json(HealthResponse {
        status: if daemon_online { "online" } else { "degraded" }.to_string(),
        daemon_online,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn handle_list_workflows(
    State(state): State<AppState>,
    Query(q): Query<BlueprintQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::ListWorkflows {
        blueprint: q.blueprint,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Workflows { names }) => Ok(Json(names)),
        Ok(UdsResponse::Error { message }) => Ok(Json(vec![message])),
        _ => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn handle_get_workflow(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<BlueprintQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::GetWorkflow {
        blueprint: q.blueprint,
        name: name.clone(),
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::WorkflowContent { name: _, content }) => Ok(content),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

async fn handle_save_workflow(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<BlueprintQuery>,
    Json(body): Json<SaveWorkflowPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::SaveWorkflow {
        blueprint: q.blueprint,
        name,
        content: body.content,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Ok { message }) => Ok(Json(serde_json::json!({ "message": message }))),
        Ok(UdsResponse::Error { message }) => Ok(Json(serde_json::json!({ "error": message }))),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn handle_progress(
    State(state): State<AppState>,
    Query(q): Query<BlueprintQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::Progress {
        blueprint: q.blueprint,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Progress { active_tasks }) => Ok(Json(active_tasks)),
        _ => Ok(Json(vec![])),
    }
}

async fn handle_dispatch(
    State(state): State<AppState>,
    Json(body): Json<DispatchPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::Dispatch {
        blueprint: body.blueprint,
        workflow: body.workflow,
        inline_command: body.inline_command,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(15)) {
        Ok(UdsResponse::Dispatch { task_id }) => {
            Ok(Json(serde_json::json!({ "task_id": task_id })))
        }
        Ok(UdsResponse::Error { message }) => Ok(Json(serde_json::json!({ "error": message }))),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn handle_gate_verdict(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<GateVerdictPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::GateAction {
        task_id,
        approve: body.approve,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Ok { message }) => Ok(Json(serde_json::json!({ "message": message }))),
        Ok(UdsResponse::Error { message }) => Ok(Json(serde_json::json!({ "error": message }))),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn handle_ws_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, id))
}

async fn handle_socket(mut socket: WebSocket, state: AppState, id: String) {
    let mut interval = tokio::time::interval(Duration::from_millis(1000));
    let mut last_tasks_json: Option<String> = None;
    let mut ticks = 0u64;

    // 1. Initial SNAPSHOT on connect per ADR-032 §5.C
    let req = Request::Progress { blueprint: None };
    if let Ok(UdsResponse::Progress { active_tasks }) =
        uds::request_to(&state.sock_path, &req, Duration::from_millis(1000))
    {
        let json_str = serde_json::to_string(&active_tasks).unwrap_or_default();
        let snapshot = serde_json::json!({
            "type": "SNAPSHOT",
            "run_id": id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "active_tasks": active_tasks
        });
        let _ = socket.send(Message::Text(snapshot.to_string())).await;
        last_tasks_json = Some(json_str);
    }

    // 2. Stream loop emitting DELTA on state change or HEARTBEAT
    loop {
        interval.tick().await;
        ticks += 1;
        let req = Request::Progress { blueprint: None };
        if let Ok(UdsResponse::Progress { active_tasks }) =
            uds::request_to(&state.sock_path, &req, Duration::from_millis(500))
        {
            let current_json = serde_json::to_string(&active_tasks).unwrap_or_default();
            if last_tasks_json.as_ref() != Some(&current_json) {
                let delta = serde_json::json!({
                    "type": "DELTA",
                    "run_id": id,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "active_tasks": active_tasks
                });
                if socket.send(Message::Text(delta.to_string())).await.is_err() {
                    break;
                }
                last_tasks_json = Some(current_json);
            } else if ticks.is_multiple_of(10) {
                let hb = serde_json::json!({
                    "type": "HEARTBEAT",
                    "run_id": id,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                });
                if socket.send(Message::Text(hb.to_string())).await.is_err() {
                    break;
                }
            }
        }
    }
}
