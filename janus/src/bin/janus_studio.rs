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
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
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

    /// Listening HTTP port (default: 8443)
    #[arg(long, default_value_t = 8443)]
    port: u16,

    /// Path to authentication token file
    #[arg(long)]
    token_file: Option<PathBuf>,

    /// Path to janus-daemon UDS socket file
    #[arg(long)]
    sock: Option<PathBuf>,
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

    let addr: SocketAddr = format!("{}:{}", cli.bind, cli.port)
        .parse()
        .context("invalid bind/port configuration")?;

    println!("🚀 MetaMach Studio (v0.6.0) listening on http://{}", addr);
    println!("🔐 Auth Token: {}", auth_token);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
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
    Html(include_str!("../../../spike/canvas-ui/index.html"))
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
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::ListWorkflows { blueprint: None };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Workflows { names }) => Ok(Json(names)),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn handle_get_workflow(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::GetWorkflow {
        blueprint: None,
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
    Json(body): Json<SaveWorkflowPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::SaveWorkflow {
        blueprint: None,
        name,
        content: body.content,
    };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Ok { message }) => Ok(Json(serde_json::json!({ "message": message }))),
        Ok(UdsResponse::Error { message }) => Ok(Json(serde_json::json!({ "error": message }))),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn handle_progress(State(state): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let req = Request::Progress { blueprint: None };
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(5)) {
        Ok(UdsResponse::Progress { active_tasks }) => Ok(Json(active_tasks)),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
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
    match uds::request_to(&state.sock_path, &req, Duration::from_secs(10)) {
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
    let mut step_count = 0;

    loop {
        interval.tick().await;
        let req = Request::Progress { blueprint: None };
        if let Ok(UdsResponse::Progress { active_tasks }) =
            uds::request_to(&state.sock_path, &req, Duration::from_millis(500))
        {
            let payload = serde_json::json!({
                "type": "SNAPSHOT",
                "run_id": id,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "active_tasks": active_tasks
            });
            if socket
                .send(Message::Text(payload.to_string()))
                .await
                .is_err()
            {
                break;
            }
        } else {
            step_count += 1;
            let mock_event = serde_json::json!({
                "type": "HEARTBEAT",
                "run_id": id,
                "seq": step_count,
                "status": "daemon_polling"
            });
            if socket
                .send(Message::Text(mock_event.to_string()))
                .await
                .is_err()
            {
                break;
            }
        }
    }
}
