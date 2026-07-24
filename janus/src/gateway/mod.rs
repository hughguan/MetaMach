//! `janus::gateway` - payload-complete HITL Gateway (ARCH-0.4.0 §V, Contracts
//! 4.3a-c).
//!
//! Out-of-band HITL dispatch: receives an enriched [`WebhookPayload`] from
//! `tool_guard`, formats it per destination (TUI / Teams / Telegram / log), and
//! hosts a loopback HTTP listener for inbound Teams/TUI approval callbacks. The
//! gateway is **payload-complete** - it performs no DB lookups; all data needed
//! by every adapter is in the request payload. A resolved verdict is reported to
//! a [`VerdictSink`] (the daemon wires a DB-backed sink in Phase 3; tests use
//! [`LoggingVerdictSink`]).
//!
//! Threading (§5.1c): [`Gateway::dispatch`] is non-blocking - it records a
//! pending verdict and spawns a verdict thread that sends the card then blocks
//! on [`Gateway::await_verdict`]. The tmux control thread is never frozen. A
//! late callback gets `410 Gone`; the awaiter gets `Err(Timeout)` -> BLOCK.

pub mod teams;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tracing::{info, warn};
use uuid::Uuid;

use base64::Engine as _;

use crate::protocol::{GatewayVerdict, WebhookPayload, hitl_timeout_secs};

// Re-export the 0.3.0 sender adapters so the gateway assembles them as channels.
// (§VI moves TelegramSender/LoggingSender into the gateway's orbit; for 0.4.0 we
// import them from tool_guard::webhook rather than physically relocating.)
pub use crate::tool_guard::webhook::{LoggingSender, TelegramSender, WebhookSender};

pub use teams::TeamsSender;

/// Default loopback port for the Teams callback HTTP listener (§5.1b).
pub const DEFAULT_LISTEN_PORT: u16 = 8443;

/// The callback body's HMAC-SHA256 (base64) is carried in this header, per the
/// Teams Outgoing Webhook convention (`Authorization: Hmac <base64>`).
const HMAC_HEADER: &str = "authorization";

type HmacSha256 = Hmac<Sha256>;

/// HITL gateway dispatch trait (Contract 4.3c). `dispatch` is non-blocking;
/// `await_verdict` blocks the gateway's dedicated verdict thread until a callback
/// arrives or the deadline expires (fail-closed: timeout = BLOCK).
pub trait HitlGateway: Send + Sync {
    /// Dispatch a HITL card to all configured channels. Returns `Ok(())` on
    /// success; the `correlation_id` is already in `payload.correlation_id`
    /// (the gateway never mints it). Non-blocking: spawns a verdict thread and
    /// returns immediately.
    fn dispatch(&self, payload: &WebhookPayload) -> Result<(), GatewayError>;

    /// Block until a verdict arrives for the given correlation_id, or until the
    /// timeout expires (fail-closed: timeout = BLOCK). Called from the gateway's
    /// dedicated verdict thread, never from the tmux control thread.
    ///
    /// `timeout` is `Duration::from_secs(JANUS_HITL_TIMEOUT_SECS)` - the same
    /// deadline as `expires_at`. A late callback gets `410 Gone` from the HTTP
    /// listener; the awaiter gets `Err(Timeout)` -> BLOCK.
    fn await_verdict(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<GatewayVerdict, GatewayError>> + Send;
}

/// Errors specific to the HITL gateway (Contract 4.3c).
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("channel unavailable: {0}")]
    ChannelError(String),
    #[error("verdict timeout")]
    Timeout,
    #[error("invalid callback signature")]
    AuthFailed,
}

/// In-flight verdict registry entry, keyed by `correlation_id` (== Hermes `run_id`).
struct PendingVerdict {
    /// One-shot sender the HTTP listener uses to deliver the verdict. `None`
    /// once consumed (or never set if the awaiter already timed out).
    tx: Option<oneshot::Sender<GatewayVerdict>>,
    /// One-shot receiver the verdict thread awaits. Taken by `await_verdict`.
    rx: Option<oneshot::Receiver<GatewayVerdict>>,
    blueprint: String,
    task_id: Option<uuid::Uuid>,
    step: String,
    expires_at: chrono::DateTime<Utc>,
    /// `true` once a callback has resolved this entry (`409 Conflict` on duplicates).
    resolved: bool,
}

/// A HITL card channel: a [`WebhookSender`] that translates a [`WebhookPayload`]
/// into a native format. The blanket impl means every `WebhookSender` (Logging,
/// Telegram, Teams) is a channel - no separate adapter boilerplate.
pub trait HitlChannel: WebhookSender {}
impl<T: WebhookSender + ?Sized> HitlChannel for T {}

/// Sink for resolved verdicts (the seam where the daemon records the outcome).
/// The gateway itself is payload-complete (no DB); the daemon supplies a
/// DB-backed sink in Phase 3. Tests use [`LoggingVerdictSink`].
pub trait VerdictSink: Send + Sync {
    fn on_verdict(
        &self,
        correlation_id: &str,
        blueprint: &str,
        task_id: Option<uuid::Uuid>,
        step: &str,
        verdict: &GatewayVerdict,
    );
}

/// Default sink: logs the resolution (audit trail).
pub struct LoggingVerdictSink;
impl VerdictSink for LoggingVerdictSink {
    fn on_verdict(
        &self,
        cid: &str,
        blueprint: &str,
        task_id: Option<uuid::Uuid>,
        step: &str,
        verdict: &GatewayVerdict,
    ) {
        info!(
            %cid, %blueprint, ?task_id, %step, ?verdict,
            "HITL verdict resolved"
        );
    }
}

/// The HITL gateway.
pub struct Gateway {
    pending: Arc<Mutex<HashMap<String, PendingVerdict>>>,
    channels: Vec<Arc<dyn HitlChannel>>,
    hmac_secret: Option<Vec<u8>>,
    listen_port: u16,
    sink: Arc<dyn VerdictSink>,
}

impl Gateway {
    /// Construct a gateway. `channels` fire on every dispatch; `hmac_secret`
    /// (from `JANUS_TEAMS_HMAC_SECRET`) gates callback authentication - `None`
    /// skips HMAC (dev/test); `listen_port` binds the callback listener.
    pub fn new(
        channels: Vec<Arc<dyn HitlChannel>>,
        hmac_secret: Option<Vec<u8>>,
        listen_port: u16,
        sink: Arc<dyn VerdictSink>,
    ) -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            channels,
            hmac_secret,
            listen_port,
            sink,
        }
    }

    /// Bind the loopback HTTP callback listener (§5.1b) and return the bound
    /// address + the accept-loop task handle. The daemon awaits the handle;
    /// tests use the address to POST callbacks (binding port `0` yields an
    /// ephemeral port).
    pub async fn bind_listener(
        self: Arc<Self>,
    ) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>)> {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", self.listen_port).parse().unwrap();
        let listener = TcpListener::bind(addr).await?;
        let bound = listener.local_addr()?;
        info!(%bound, "HITL gateway callback listener bound");
        let me = self.clone();
        let handle = tokio::spawn(async move {
            me.accept_loop(listener).await;
        });
        Ok((bound, handle))
    }

    /// Run the accept loop forever (one spawned task per connection).
    async fn accept_loop(self: Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let me = self.clone();
                    tokio::spawn(async move {
                        me.handle_callback(stream).await;
                    });
                }
                Err(e) => warn!("gateway accept error: {e}"),
            }
        }
    }

    /// Convenience: bind + run the accept loop (the daemon's `tokio::spawn`
    /// entry point).
    pub async fn spawn_listener(self: Arc<Self>) -> std::io::Result<()> {
        let (_, handle) = self.bind_listener().await?;
        let _ = handle.await;
        Ok(())
    }

    /// Read + validate one inbound callback, returning the HTTP status to write.
    async fn handle_callback(self: Arc<Self>, mut stream: TcpStream) {
        let req = match read_request(&mut stream).await {
            Some(r) => r,
            None => {
                let _ = write_response(&mut stream, 400, "bad request").await;
                return;
            }
        };
        if req.method != "POST" {
            let _ = write_response(&mut stream, 405, "method not allowed").await;
            return;
        }
        let Some(run_id) = parse_run_id(&req.path) else {
            let _ = write_response(&mut stream, 404, "not found").await;
            return;
        };
        let status = self.resolve_callback(run_id, &req.body, req.auth.as_deref());
        let (code, msg) = status.parts();
        let _ = write_response(&mut stream, code, msg).await;
    }

    /// Resolve an inbound callback against the pending-verdict map.
    fn resolve_callback(&self, run_id: &str, body: &[u8], auth: Option<&str>) -> CallbackStatus {
        // 1. HMAC first (reject unauthenticated before revealing any state).
        if let Some(secret) = &self.hmac_secret
            && !verify_hmac(secret, body, auth)
        {
            return CallbackStatus::Unauthorized;
        }
        // 2. Parse the action body.
        let Some(action) = parse_action(body) else {
            return CallbackStatus::BadRequest;
        };
        // 3. Look up the pending entry; enforce expiry + single-callback.
        let tx = {
            let mut p = self.pending.lock().expect("pending mutex");
            let Some(entry) = p.get_mut(run_id) else {
                return CallbackStatus::Gone; // unknown / already cleaned up
            };
            if Utc::now() > entry.expires_at {
                p.remove(run_id);
                return CallbackStatus::Gone; // late callback
            }
            if entry.resolved {
                return CallbackStatus::Conflict; // duplicate approval
            }
            entry.resolved = true;
            entry.tx.take()
        };
        // 4. Deliver the verdict (outside the lock). If the receiver is gone the
        // awaiter already timed out (the race window before the spawned task
        // removes the entry) - the verdict is undeliverable, so drop the dead
        // entry and return 410 Gone rather than a false 200.
        let verdict = action_to_verdict(action);
        let delivered = match tx {
            Some(tx) => tx.send(verdict).is_ok(),
            None => false,
        };
        if !delivered {
            self.pending.lock().expect("pending mutex").remove(run_id);
            return CallbackStatus::Gone;
        }
        CallbackStatus::Resolved
    }

    /// Shared await logic: take the receiver for `cid` and block until verdict
    /// or timeout. Used by both the trait method and the dispatch-spawned task.
    async fn await_verdict_inner(
        pending: &Arc<Mutex<HashMap<String, PendingVerdict>>>,
        cid: &str,
        timeout: Duration,
    ) -> Result<GatewayVerdict, GatewayError> {
        let rx = {
            let mut p = pending.lock().expect("pending mutex");
            match p.get_mut(cid) {
                Some(e) => {
                    e.rx.take()
                        .ok_or_else(|| GatewayError::ChannelError("already awaited".into()))?
                }
                None => return Err(GatewayError::ChannelError("unknown correlation_id".into())),
            }
        };
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err(GatewayError::ChannelError("verdict sender dropped".into())),
            Err(_) => Err(GatewayError::Timeout),
        }
    }

    /// Find the correlation_id for a SUSPENDED task (TUI HITL lookup, ADR-020).
    /// Returns `None` if no pending verdict matches this task_id.
    pub fn find_correlation_by_task(&self, task_id: Uuid) -> Option<String> {
        self.pending
            .lock()
            .expect("pending mutex")
            .iter()
            .find(|(_, v)| v.task_id == Some(task_id))
            .map(|(cid, _)| cid.clone())
    }

    /// Resolve a HITL verdict from the TUI Observer (ADR-020). Skips HMAC
    /// (UDS-originated, already authenticated by the socket path).
    pub fn resolve_tui(&self, run_id: &str, approve: bool) -> CallbackStatus {
        let action = serde_json::json!({"action": if approve { "approve" } else { "reject" }});
        let action = parse_action(&serde_json::to_vec(&action).unwrap_or_default());
        let Some(action) = action else {
            return CallbackStatus::BadRequest;
        };
        let tx = {
            let mut p = self.pending.lock().expect("pending mutex");
            let Some(entry) = p.get_mut(run_id) else {
                return CallbackStatus::Gone;
            };
            if Utc::now() > entry.expires_at {
                p.remove(run_id);
                return CallbackStatus::Gone;
            }
            if entry.resolved {
                return CallbackStatus::Conflict;
            }
            entry.resolved = true;
            entry.tx.take()
        };
        let verdict = action_to_verdict(action);
        let delivered = match tx {
            Some(tx) => tx.send(verdict).is_ok(),
            None => false,
        };
        if !delivered {
            self.pending.lock().expect("pending mutex").remove(run_id);
            return CallbackStatus::Gone;
        }
        CallbackStatus::Resolved
    }
}

impl HitlGateway for Gateway {
    fn dispatch(&self, payload: &WebhookPayload) -> Result<(), GatewayError> {
        let cid = payload.correlation_id.clone();
        let secs = hitl_timeout_secs();
        let expires_at = Utc::now() + chrono::Duration::seconds(secs);
        let (tx, rx) = oneshot::channel();
        {
            let mut p = self.pending.lock().expect("pending mutex");
            p.insert(
                cid.clone(),
                PendingVerdict {
                    tx: Some(tx),
                    rx: Some(rx),
                    blueprint: payload.blueprint.clone(),
                    task_id: payload.task_id,
                    step: payload.step.clone(),
                    expires_at,
                    resolved: false,
                },
            );
        }
        // Spawn the verdict thread: send the card, await the verdict, report it.
        let pending = self.pending.clone();
        let sink = self.sink.clone();
        let channels = self.channels.clone();
        let cid_task = cid.clone();
        let payload_task = payload.clone();
        let timeout = Duration::from_secs(secs as u64);
        tokio::spawn(async move {
            // 1. Fire the card to every channel (blocking HTTP -> spawn_blocking).
            let ch = channels;
            let p = payload_task;
            let _ = tokio::task::spawn_blocking(move || {
                for c in &ch {
                    c.send(&p);
                }
            })
            .await;
            // 2. Await the verdict (blocks this task, not the tmux control thread).
            let blueprint = {
                let p = pending.lock().expect("pending mutex");
                p.get(&cid_task)
                    .map(|e| (e.blueprint.clone(), e.task_id, e.step.clone()))
            };
            match Self::await_verdict_inner(&pending, &cid_task, timeout).await {
                Ok(v) => {
                    if let Some((blueprint, task_id, step)) = blueprint {
                        sink.on_verdict(&cid_task, &blueprint, task_id, &step, &v);
                    }
                }
                Err(e) => {
                    warn!(cid = %cid_task, error = %e, "HITL verdict timed out / errored");
                    // Remove the dead entry: prevents a slow leak (no callback
                    // ever arrived) and ensures a late callback returns 410 Gone
                    // rather than a false 200 (the awaiter already gave up and
                    // dropped its receiver).
                    pending.lock().expect("pending mutex").remove(&cid_task);
                }
            }
        });
        Ok(())
    }

    fn await_verdict(
        &self,
        correlation_id: &str,
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<GatewayVerdict, GatewayError>> + Send {
        Self::await_verdict_inner(&self.pending, correlation_id, timeout)
    }
}

// --- HTTP parsing (minimal: one POST per connection, Content-Length bodies) ---

struct HttpRequest {
    method: String,
    path: String,
    auth: Option<String>,
    body: Vec<u8>,
}

/// Read one HTTP/1.1 request (headers up to `\r\n\r\n`, then Content-Length body).
async fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut header_buf: Vec<u8> = Vec::new();
    let mut one = [0u8; 1];
    loop {
        if stream.read(&mut one).await.ok()? == 0 {
            return None;
        }
        header_buf.push(one[0]);
        if header_buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if header_buf.len() > 8192 {
            return None; // header too large
        }
    }
    let header_str = std::str::from_utf8(&header_buf).ok()?;
    let mut lines = header_str.split("\r\n");
    let req_line = lines.next()?;
    let mut parts = req_line.split(' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut content_length = 0usize;
    let mut auth = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            if k == "content-length" {
                content_length = v.parse().unwrap_or(0);
            } else if k == HMAC_HEADER {
                auth = Some(v.to_string());
            }
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).await.ok()?;
    }
    Some(HttpRequest {
        method,
        path,
        auth,
        body,
    })
}

/// Extract the `run_id` from `/v1/runs/{run_id}/actions`.
fn parse_run_id(path: &str) -> Option<&str> {
    let p = path.split('?').next()?; // drop any query string
    let p = p.strip_prefix("/v1/runs/")?;
    let id = p.strip_suffix("/actions")?;
    if id.is_empty() { None } else { Some(id) }
}

#[derive(Debug)]
enum HitlAction {
    Approve,
    Reject,
    Override(String),
}

/// Parse the callback JSON body: `{"action":"approve|reject|override"[,"override_command":"..."]}`.
fn parse_action(body: &[u8]) -> Option<HitlAction> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let action = v.get("action")?.as_str()?;
    match action {
        "approve" => Some(HitlAction::Approve),
        "reject" => Some(HitlAction::Reject),
        "override" => {
            let cmd = v
                .get("override_command")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            Some(HitlAction::Override(cmd.to_string()))
        }
        _ => None,
    }
}

fn action_to_verdict(action: HitlAction) -> GatewayVerdict {
    match action {
        HitlAction::Approve => GatewayVerdict::Approve,
        HitlAction::Reject => GatewayVerdict::Reject,
        HitlAction::Override(cmd) => GatewayVerdict::Override {
            rewritten_argv: cmd.split_whitespace().map(String::from).collect(),
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CallbackStatus {
    Resolved,
    Conflict,
    Gone,
    Unauthorized,
    BadRequest,
}

impl CallbackStatus {
    pub fn parts(self) -> (u16, &'static str) {
        match self {
            CallbackStatus::Resolved => (200, "ok"),
            CallbackStatus::Conflict => (409, "conflict"),
            CallbackStatus::Gone => (410, "gone"),
            CallbackStatus::Unauthorized => (401, "unauthorized"),
            CallbackStatus::BadRequest => (400, "bad request"),
        }
    }
}

/// Verify the callback HMAC-SHA256 (base64 in the `Authorization` header) in
/// constant time via `Mac::verify_slice`. `None` auth -> fail.
fn verify_hmac(secret: &[u8], body: &[u8], auth: Option<&str>) -> bool {
    let Some(auth) = auth else {
        return false;
    };
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    // Teams sends `Authorization: Hmac <base64>`; accept the bare base64 too.
    let given_b64 = auth.trim().strip_prefix("Hmac ").unwrap_or(auth).trim();
    let Ok(given) = base64::engine::general_purpose::STANDARD.decode(given_b64) else {
        return false;
    };
    mac.verify_slice(&given).is_ok()
}

/// Write a minimal HTTP/1.1 JSON response and close the connection.
async fn write_response(stream: &mut TcpStream, code: u16, msg: &str) -> std::io::Result<()> {
    let body = format!("{{\"status\":\"{msg}\"}}");
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        reason = http_reason(code),
        len = body.len()
    );
    stream.write_all(resp.as_bytes()).await
}

fn http_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A channel that counts sends (for dispatch non-blocking tests).
    struct CountingChannel {
        count: Arc<AtomicU32>,
    }
    impl WebhookSender for CountingChannel {
        fn send(&self, _: &WebhookPayload) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn gw(channels: Vec<Arc<dyn HitlChannel>>) -> Gateway {
        Gateway::new(channels, None, 0, Arc::new(LoggingVerdictSink))
    }

    fn payload(cid: &str) -> WebhookPayload {
        WebhookPayload::build(
            Some(uuid::Uuid::nil()),
            "exec",
            cid,
            "require_approval",
            "make flash",
            "needs approval",
            "gatemetric",
            "flash",
        )
    }

    #[tokio::test]
    async fn dispatch_is_non_blocking() {
        // UTC-10-01: dispatch returns immediately; the card send happens on a
        // spawned task, so the tmux control thread is never frozen. (The spawned
        // task's long await is cancelled when the test runtime drops.)
        let count = Arc::new(AtomicU32::new(0));
        let ch: Arc<dyn HitlChannel> = Arc::new(CountingChannel {
            count: count.clone(),
        });
        let gw = gw(vec![ch]);
        let start = std::time::Instant::now();
        let r = gw.dispatch(&payload("cid-01"));
        let elapsed = start.elapsed();
        assert!(r.is_ok(), "dispatch should return Ok");
        assert!(
            elapsed.as_millis() < 50,
            "dispatch returned in {elapsed:?} (must be non-blocking)",
        );
        // The pending entry exists immediately.
        assert!(gw.pending.lock().unwrap().contains_key("cid-01"));
    }

    #[tokio::test]
    async fn await_verdict_times_out() {
        // UTC-10-03: no callback arrives -> Err(Timeout) after the deadline
        // (fail-closed BLOCK).
        let gw = gw(vec![]);
        // Insert a pending entry with no sender -> the receiver never resolves.
        let (tx, rx) = oneshot::channel();
        gw.pending.lock().unwrap().insert(
            "cid-03".into(),
            PendingVerdict {
                tx: Some(tx),
                rx: Some(rx),
                blueprint: "bp".into(),
                task_id: None,
                step: "s".into(),
                expires_at: Utc::now() + chrono::Duration::seconds(3600),
                resolved: false,
            },
        );
        let r = gw.await_verdict("cid-03", Duration::from_millis(100)).await;
        assert!(matches!(r, Err(GatewayError::Timeout)), "got {r:?}");
    }

    #[tokio::test]
    async fn await_verdict_receives_callback() {
        // The HTTP listener path: a tx.send delivers the verdict to the awaiter.
        let gw = gw(vec![]);
        let (tx, rx) = oneshot::channel();
        gw.pending.lock().unwrap().insert(
            "cid-recv".into(),
            PendingVerdict {
                tx: Some(tx),
                rx: Some(rx),
                blueprint: "bp".into(),
                task_id: None,
                step: "s".into(),
                expires_at: Utc::now() + chrono::Duration::seconds(3600),
                resolved: false,
            },
        );
        // Schedule the verdict delivery.
        let pending = gw.pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let tx = pending
                .lock()
                .unwrap()
                .get_mut("cid-recv")
                .and_then(|e| e.tx.take());
            if let Some(tx) = tx {
                let _ = tx.send(GatewayVerdict::Approve);
            }
        });
        let r = gw.await_verdict("cid-recv", Duration::from_secs(2)).await;
        assert!(matches!(r, Ok(GatewayVerdict::Approve)), "got {r:?}");
    }

    #[test]
    fn parse_run_id_extracts_id() {
        assert_eq!(parse_run_id("/v1/runs/abc-123/actions"), Some("abc-123"));
        assert_eq!(parse_run_id("/v1/runs/abc/actions?x=1"), Some("abc"));
        assert_eq!(parse_run_id("/v1/runs//actions"), None);
        assert_eq!(parse_run_id("/other"), None);
    }

    #[test]
    fn parse_action_maps_verdict() {
        let a = parse_action(br#"{"action":"approve"}"#).unwrap();
        assert!(matches!(action_to_verdict(a), GatewayVerdict::Approve));
        let a = parse_action(br#"{"action":"reject"}"#).unwrap();
        assert!(matches!(action_to_verdict(a), GatewayVerdict::Reject));
        let a =
            parse_action(br#"{"action":"override","override_command":"make dry-run"}"#).unwrap();
        match action_to_verdict(a) {
            GatewayVerdict::Override { rewritten_argv } => {
                assert_eq!(rewritten_argv, vec!["make", "dry-run"]);
            }
            other => panic!("expected Override, got {other:?}"),
        }
        assert!(parse_action(br#"{"action":"bogus"}"#).is_none());
        assert!(parse_action(b"not json").is_none());
    }

    #[test]
    fn verify_hmac_accepts_correct_rejects_wrong() {
        let secret = b"shared-secret";
        let body = br#"{"action":"approve"}"#;
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(body);
        let tag = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        // Correct signature (with the Teams "Hmac " prefix).
        assert!(verify_hmac(secret, body, Some(&format!("Hmac {tag}"))));
        // Bare base64 also accepted.
        assert!(verify_hmac(secret, body, Some(&tag)));
        // Wrong signature.
        assert!(!verify_hmac(secret, body, Some("dG90YWxseS13cm9uZw==")));
        // Missing auth.
        assert!(!verify_hmac(secret, body, None));
        // Tampered body.
        assert!(!verify_hmac(secret, br#"{"action":"reject"}"#, Some(&tag)));
    }

    #[test]
    fn resolve_callback_duplicate_is_conflict() {
        // No HMAC secret (test_default) -> auth skipped. A resolved entry -> 409.
        let gw = gw(vec![]);
        let (tx, rx) = oneshot::channel();
        gw.pending.lock().unwrap().insert(
            "cid-dup".into(),
            PendingVerdict {
                tx: Some(tx),
                rx: Some(rx),
                blueprint: "bp".into(),
                task_id: None,
                step: "s".into(),
                expires_at: Utc::now() + chrono::Duration::seconds(3600),
                resolved: false,
            },
        );
        let body = br#"{"action":"approve"}"#;
        assert!(matches!(
            gw.resolve_callback("cid-dup", body, None),
            CallbackStatus::Resolved
        ));
        // Second callback for the same run_id -> 409.
        assert!(matches!(
            gw.resolve_callback("cid-dup", body, None),
            CallbackStatus::Conflict
        ));
    }

    #[test]
    fn resolve_callback_expired_is_gone() {
        let gw = gw(vec![]);
        let (tx, rx) = oneshot::channel();
        gw.pending.lock().unwrap().insert(
            "cid-exp".into(),
            PendingVerdict {
                tx: Some(tx),
                rx: Some(rx),
                blueprint: "bp".into(),
                task_id: None,
                step: "s".into(),
                expires_at: Utc::now() - chrono::Duration::seconds(1), // already expired
                resolved: false,
            },
        );
        assert!(matches!(
            gw.resolve_callback("cid-exp", br#"{"action":"approve"}"#, None),
            CallbackStatus::Gone
        ));
        // Entry cleaned up.
        assert!(!gw.pending.lock().unwrap().contains_key("cid-exp"));
    }

    #[test]
    fn resolve_callback_unknown_is_gone() {
        let gw = gw(vec![]);
        assert!(matches!(
            gw.resolve_callback("never-existed", br#"{"action":"approve"}"#, None),
            CallbackStatus::Gone
        ));
    }

    #[test]
    fn resolve_callback_gone_when_awaiter_timed_out() {
        // P1 race-fix: the awaiter took rx + timed out (rx dropped) but the entry
        // still lingers (the spawned task hasn't removed it yet). A late callback
        // must NOT return a false 200 - `tx.send` fails (receiver gone), so the
        // undeliverable verdict is treated as 410 Gone and the dead entry is
        // removed.
        let gw = gw(vec![]);
        let (tx, rx) = oneshot::channel();
        gw.pending.lock().unwrap().insert(
            "cid-race".into(),
            PendingVerdict {
                tx: Some(tx),
                rx: Some(rx),
                blueprint: "bp".into(),
                task_id: None,
                step: "s".into(),
                expires_at: Utc::now() + chrono::Duration::seconds(3600),
                resolved: false,
            },
        );
        // Simulate the awaiter taking + dropping rx (timeout).
        let rx = gw
            .pending
            .lock()
            .unwrap()
            .get_mut("cid-race")
            .unwrap()
            .rx
            .take()
            .unwrap();
        drop(rx);
        // Late callback: tx.send fails (receiver gone) -> Gone, not Resolved.
        assert!(matches!(
            gw.resolve_callback("cid-race", br#"{"action":"approve"}"#, None),
            CallbackStatus::Gone
        ));
        // Dead entry removed (no leak, no false-200 on a further duplicate).
        assert!(!gw.pending.lock().unwrap().contains_key("cid-race"));
    }
}
