//! `janus-daemon` - the resident control-plane brain (Project-Plan M2 Task 2.1).
//!
//! Sole owner of state and the Absurd Postgres pool (Feature-Spec §2.1, ARCH §3).
//! Binds a UDS listener at `janus.sock`, enforces a singleton PID lock with stale
//! detection, and serves Blueprint / Progress queries to `herdr-janus` and the
//! `janus` CLI. Runs in the foreground when executed directly; lazy-started
//! detached by `herdr-janus` (stdio -> /dev/null, setsid).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tracing::{info, warn};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

use janus::absurd::AbsurdDb;
use janus::paths;
use janus::protocol::{Request, Response};
use janus::tool_guard::Engine;
use janus::{coldstart, lifecycle};
use sqlx::postgres::PgConnectOptions;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<()> {
    init_logging();
    info!("janus-daemon starting (pid {})", std::process::id());

    fs::create_dir_all(paths::state_dir()).context("create state dir")?;
    acquire_pid_lock(&paths::pid_path())?;

    let sock = paths::sock_path();
    let _ = fs::remove_file(&sock); // clear any stale socket (we hold the PID lock)
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
    info!("listening on {}", sock.display());

    let db = Arc::new(AbsurdDb::open_degraded(&paths::fallback_path())?);
    db.spawn_connect(pg_connect_options());
    info!("absurd db online: {}", db.pg_online().await);

    let repo_root = Arc::new(paths::repo_root());

    // Task 4.1: cold-start self-heal - once PG has had a moment to connect, scan
    // for non-terminal tasks and log resume plans (re-exec deferred to Task 2.4).
    {
        let db = db.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            if let Err(e) = coldstart::reconcile(&db).await {
                warn!("cold-start reconcile failed: {e}");
            }
        });
    }
    // ARCH §6.2: daily Janus GC - NULL-ify result_cache for >3-day-old completed tasks.
    {
        let db = db.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
            tick.tick().await; // first tick is immediate
            loop {
                tick.tick().await;
                match db.gc_old_caches().await {
                    Ok(n) if n > 0 => info!("janus GC: pruned {n} old step cache(s)"),
                    Ok(_) => {}
                    Err(e) => warn!("janus GC failed: {e}"),
                }
            }
        });
    }

    let agents_path = paths::agents_toml_path();
    let engine = Arc::new(Engine::load(&agents_path));
    info!("tool guard rules: {}", agents_path.display());

    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accept = listener.accept() => match accept {
                Ok((stream, _)) => {
                    let db = db.clone();
                    let engine = engine.clone();
                    let repo_root = repo_root.clone();
                    tokio::spawn(handle_conn(stream, db, engine, repo_root));
                }
                Err(e) => warn!("accept error: {e}"),
            },
            _ = tokio::signal::ctrl_c() => { info!("SIGINT, shutting down"); break; }
            _ = sigterm.recv() => { info!("SIGTERM, shutting down"); break; }
        }
    }
    cleanup(&sock, &paths::pid_path());
    Ok(())
}

/// Handle one request/response over a single connection (one line in, one out).
async fn handle_conn(
    stream: UnixStream,
    db: Arc<AbsurdDb>,
    engine: Arc<Engine>,
    repo_root: Arc<PathBuf>,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).await.is_err() {
        return;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return; // liveness probe / peer closed without sending - not an error
    }
    let resp = match serde_json::from_str::<Request>(trimmed) {
        Ok(req) => handle_request(req, db, &engine, &repo_root).await,
        Err(e) => {
            warn!("bad request ({e}): {trimmed:?}");
            Response::Error {
                message: format!("bad request: {e}"),
            }
        }
    };
    let json = serde_json::to_string(&resp)
        .unwrap_or_else(|_| r#"{"type":"error","message":"encode failed"}"#.to_string());
    let _ = write_half.write_all(json.as_bytes()).await;
    let _ = write_half.write_all(b"\n").await;
}

async fn handle_request(
    req: Request,
    db: Arc<AbsurdDb>,
    engine: &Engine,
    repo_root: &Path,
) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Blueprints => match db.active_blueprints().await {
            Ok(b) => Response::Blueprints { blueprints: b },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::Progress { blueprint } => match db.progress(blueprint.as_deref()).await {
            Ok(tasks) => Response::Progress {
                active_tasks: tasks,
            },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        // janush -> Daemon: synchronous Tool Guard verdict (Contract 3.2/3.4).
        Request::GuardCheck {
            execution_id,
            blueprint_id,
            task_id,
            step_name,
            argv,
            env_snapshot,
            ..
        } => {
            let verdict = engine.evaluate(&execution_id, &argv, &env_snapshot);
            // Non-destructive HITL suspension + webhook for dangerous blocks
            // (Feature-Spec §2.4). Fired in the background so the BLOCK verdict
            // returns to janush immediately (fail-closed, no PTY kill).
            if matches!(
                verdict.cause.as_deref(),
                Some("blacklist") | Some("require_approval")
            ) {
                let cmd = argv.join(" ");
                let payload = janus::tool_guard::webhook::WebhookPayload::build(
                    task_id,
                    &execution_id,
                    &verdict.correlation_id,
                    verdict.cause.as_deref().unwrap_or(""),
                    &cmd,
                    &verdict.reason.clone().unwrap_or_default(),
                    blueprint_id.as_deref().unwrap_or(""),
                    step_name.as_deref().unwrap_or(""),
                );
                let reason = verdict.reason.clone().unwrap_or_default();
                let sn = step_name;
                let tid = task_id;
                let bp = blueprint_id;
                tokio::spawn(async move {
                    if let (Some(bp), Some(tid), Some(sn)) = (bp, tid, sn.as_deref())
                        && let Err(e) = db.suspend_step(&bp, tid, sn, &reason).await
                    {
                        warn!("suspend_step failed: {e}");
                    }
                    let _ = tokio::task::spawn_blocking(move || {
                        janus::tool_guard::webhook::dispatch(&payload);
                    })
                    .await;
                });
            }
            Response::GuardVerdict {
                execution_id,
                verdict: verdict.kind.as_str().to_string(),
                reason: verdict.reason,
                rewritten_argv: verdict.rewritten_argv,
                correlation_id: verdict.correlation_id,
                cognitive_context: None,
            }
        }
        // M4 Task 4.3: Onboard (Feature-Spec §2.5).
        Request::Onboard { name } => match lifecycle::onboard(&db, &name, repo_root).await {
            Ok(r) => {
                info!(
                    %name,
                    reactivated = r.reactivated,
                    incidents = r.previous_incidents.len(),
                    "onboard via UDS"
                );
                Response::Ok { message: r.message }
            }
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        // M4 Task 4.2: Offboard (Feature-Spec §2.5).
        Request::Offboard { name } => {
            let cfg_path = repo_root.join("configs").join("offboard.toml");
            let cfg = match lifecycle::OffboardConfig::load(&cfg_path) {
                Ok(c) => c,
                Err(e) => {
                    return Response::Error {
                        message: format!("offboard config: {e}"),
                    };
                }
            };
            match lifecycle::offboard(&db, &name, repo_root, &cfg).await {
                Ok(r) => Response::Ok { message: r.message },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
    }
}

/// Singleton PID lock with stale detection (Test-Spec UTC-01-01).
fn acquire_pid_lock(pid_path: &Path) -> Result<()> {
    if let Ok(content) = fs::read_to_string(pid_path)
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        if is_pid_alive(pid) {
            bail!("janus-daemon already running (pid {pid}); refusing duplicate UDS bind");
        }
        warn!("stale pid lock (pid {pid} not alive) - overwriting");
    }
    fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}

#[cfg(unix)]
fn is_pid_alive(pid: i32) -> bool {
    // SAFETY: kill(pid, 0) performs no signal delivery; it only checks liveness.
    unsafe {
        let rc = libc::kill(pid, 0);
        if rc == 0 {
            return true;
        }
        // ESRCH = no such process (dead); EPERM = exists but not ours (alive).
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(not(unix))]
fn is_pid_alive(_pid: i32) -> bool {
    false
}

fn pg_connect_options() -> PgConnectOptions {
    let password =
        std::env::var("METAMACH_DB_PASSWORD").unwrap_or_else(|_| "metamach_dev".to_string());
    let socket = std::env::var("METAMACH_PG_SOCKET_DIR")
        .unwrap_or_else(|_| paths::pg_socket_dir().to_string_lossy().into_owned());
    PgConnectOptions::new()
        .socket(socket)
        .username("metamach_admin")
        .password(&password)
        .database("metamach_db")
}

fn cleanup(sock: &Path, pid: &Path) {
    let _ = fs::remove_file(sock);
    let _ = fs::remove_file(pid);
    info!("cleaned up socket + pid");
}

// --- logging to janus.log (Mutable State) ---------------------------------

#[derive(Clone)]
struct SharedFile(Arc<Mutex<std::fs::File>>);

struct FileWrite(Arc<Mutex<std::fs::File>>);

impl Write for FileWrite {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log mutex poisoned").write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("log mutex poisoned").flush()
    }
}

impl<'a> MakeWriter<'a> for SharedFile {
    type Writer = FileWrite;
    fn make_writer(&'a self) -> Self::Writer {
        FileWrite(self.0.clone())
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("janus=info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false);
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::log_path())
    {
        Ok(f) => {
            let maker = SharedFile(Arc::new(Mutex::new(f)));
            install_subscriber(builder.with_writer(maker).finish());
        }
        Err(e) => {
            eprintln!(
                "janus-daemon: can't open {} ({e}); logging to stderr",
                paths::log_path().display()
            );
            install_subscriber(builder.finish());
        }
    }
}

/// Install `subscriber` as the global default. If a global subscriber is already
/// set (test harness, parent process, or double-init) it cannot be replaced, so
/// logs go wherever that one directs - surface the failure so it isn't silent.
/// Generic so the file-writer and stderr-writer subscriber types both apply.
fn install_subscriber<S: tracing::Subscriber + Send + Sync + 'static>(subscriber: S) {
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!(
            "janus-daemon: could not install tracing subscriber ({e}); a global \
             subscriber is already set - logs may not reach {}",
            paths::log_path().display()
        );
    }
}
