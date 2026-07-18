//! 0.4.0 HITL gateway integration tests (Test-Spec UTC-10-02, UTC-10-04).
//!
//! Spins up the gateway's loopback HTTP callback listener on an ephemeral port
//! and POSTs Teams-style approval callbacks to exercise the ingress path:
//! 200 on resolve / 409 on duplicate (UTC-10-02), and HMAC 401/401/200
//! (UTC-10-04). The pure-logic paths (dispatch non-blocking = UTC-10-01,
//! await_verdict timeout = UTC-10-03, Adaptive Card schema = UTC-10-05, payload
//! enrichment = UTC-10-09, 410-Gone logic = UTC-10-10, parse helpers) are
//! covered by the unit tests in `src/gateway/mod.rs` + `teams.rs`; the cognitive
//! provider behavior (UTC-10-06/07/08) by `src/cognitive/mod.rs` + a lifecycle
//! test.

use std::sync::Arc;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use janus::gateway::{Gateway, HitlGateway, LoggingVerdictSink};
use janus::protocol::WebhookPayload;
use janus::tool_guard::webhook::WebhookSender;

type HmacSha256 = Hmac<Sha256>;

/// A no-op channel (these tests assert on HTTP responses, not card delivery).
struct NoopChannel;
impl WebhookSender for NoopChannel {
    fn send(&self, _: &WebhookPayload) {}
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

/// POST a callback to `/v1/runs/{cid}/actions` and return the HTTP status code.
async fn post(addr: &std::net::SocketAddr, cid: &str, body: &str, auth: Option<&str>) -> u16 {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let auth_header = match auth {
        Some(a) => format!("\r\nAuthorization: {a}"),
        None => String::new(),
    };
    let req = format!(
        "POST /v1/runs/{cid}/actions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len}{auth_header}\r\n\r\n{body}",
        len = body.len(),
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 256];
    let n = stream.read(&mut buf).await.unwrap();
    let resp = std::str::from_utf8(&buf[..n]).unwrap();
    // Parse "HTTP/1.1 200 OK" -> 200.
    resp.split(' ').nth(1).unwrap().parse().unwrap()
}

/// Base64-encoded HMAC-SHA256 of `body` keyed by `secret` (Teams convention).
fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).unwrap();
    mac.update(body);
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

#[tokio::test]
async fn utc_10_02_http_callback_200_then_409_duplicate() {
    // UTC-10-02: a valid callback resolves (200); a duplicate is rejected (409).
    let gw = Arc::new(Gateway::new(
        vec![Arc::new(NoopChannel)],
        None, // no HMAC for this case
        0,    // ephemeral port
        Arc::new(LoggingVerdictSink),
    ));
    let (addr, _handle) = gw.clone().bind_listener().await.unwrap();

    // dispatch creates the pending-verdict entry (correlation_id == run_id).
    gw.dispatch(&payload("cid-02")).unwrap();

    let body = r#"{"action":"approve","approved_by":"teams"}"#;
    // First callback resolves the verdict -> 200.
    assert_eq!(post(&addr, "cid-02", body, None).await, 200);
    // Duplicate callback for the same run_id -> 409 Conflict.
    assert_eq!(post(&addr, "cid-02", body, None).await, 409);
}

#[tokio::test]
async fn utc_10_04_hmac_auth_rejects_unsigned_and_wrong_accepts_correct() {
    // UTC-10-04: no-HMAC -> 401, wrong-HMAC -> 401, correct-HMAC -> 200.
    let secret = b"shared-secret".to_vec();
    let gw = Arc::new(Gateway::new(
        vec![Arc::new(NoopChannel)],
        Some(secret.clone()),
        0,
        Arc::new(LoggingVerdictSink),
    ));
    let (addr, _handle) = gw.clone().bind_listener().await.unwrap();
    gw.dispatch(&payload("cid-04")).unwrap();

    let body = r#"{"action":"approve","approved_by":"teams"}"#;
    // No Authorization header -> 401.
    assert_eq!(post(&addr, "cid-04", body, None).await, 401);
    // Wrong signature -> 401.
    assert_eq!(
        post(&addr, "cid-04", body, Some("Hmac dG90YWxseS13cm9uZw==")).await,
        401
    );
    // Correct signature -> 200.
    let tag = sign(&secret, body.as_bytes());
    assert_eq!(
        post(&addr, "cid-04", body, Some(&format!("Hmac {tag}"))).await,
        200
    );
}
