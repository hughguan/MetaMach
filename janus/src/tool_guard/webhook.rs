//! HITL webhook card adapters (Feature-Spec §2.4). The abstract
//! [`WebhookPayload`] now lives in [`crate::protocol`] (0.4.0 §5.4, enriched for
//! the Hermes Run API); this module keeps the sender adapters + the dispatch
//! entrypoint. 0.4.0 Phase 3 will repoint `dispatch` at [`crate::gateway`]; for
//! now it fires the local senders as in 0.3.0.
//!
//! [`LoggingSender`] always fires (to `janus.log` + a state-dir spool);
//! [`TelegramSender`] (primary backend) POSTs to the Telegram Bot API with an
//! inline `[Resume]` keyboard when `JANUS_TELEGRAM_TOKEN` +
//! `JANUS_TELEGRAM_CHAT_ID` are configured, and no-ops otherwise (the log card
//! covers the test path until secrets are provisioned).

pub use crate::protocol::WebhookPayload;

use std::process::Command;

pub trait WebhookSender: Send + Sync {
    fn send(&self, payload: &WebhookPayload);
}

/// Always-on adapter: writes the card to `janus.log` (via tracing) and appends a
/// line to `<state_dir>/webhook_cards.log` for audit.
pub struct LoggingSender {
    spool: std::path::PathBuf,
}

impl LoggingSender {
    pub fn new(spool: std::path::PathBuf) -> Self {
        Self { spool }
    }
}

impl WebhookSender for LoggingSender {
    fn send(&self, p: &WebhookPayload) {
        tracing::warn!(
            cause = %p.cause,
            command = %p.command,
            reason = %p.reason,
            correlation = %p.correlation_id,
            "HITL suspension card"
        );
        let line = format!(
            "[{}] cause={} cmd={:?} reason={} resume={}\n",
            p.correlation_id, p.cause, p.command, p.reason, p.resume_key
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.spool)
        {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Telegram Bot API adapter (primary). Fires only when both
/// `JANUS_TELEGRAM_TOKEN` and `JANUS_TELEGRAM_CHAT_ID` are set; otherwise no-ops
/// (the [`LoggingSender`] already recorded the card). Uses `curl` (ubiquitous
/// on macOS/Linux) so no TLS/network crate is pulled into the build.
pub struct TelegramSender;

impl WebhookSender for TelegramSender {
    fn send(&self, p: &WebhookPayload) {
        let (token, chat) = match (
            std::env::var("JANUS_TELEGRAM_TOKEN"),
            std::env::var("JANUS_TELEGRAM_CHAT_ID"),
        ) {
            (Ok(t), Ok(c)) if !t.is_empty() && !c.is_empty() => (t, c),
            _ => return, // secrets absent - LoggingSender covers the test/audit path
        };
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let text = format!(
            "🛑 MetaMach HITL\nCause: {cause}\nCmd: {cmd}\nReason: {reason}\nCorrelation: {cid}",
            cause = p.cause,
            cmd = p.command,
            reason = p.reason,
            cid = p.correlation_id
        );
        let body = serde_json::json!({
            "chat_id": chat,
            "text": text,
            "reply_markup": {
                "inline_keyboard": [[{"text": "[Resume]", "callback_data": p.resume_key}]]
            }
        })
        .to_string();
        match Command::new("curl")
            .args([
                "-s",
                "-S",
                "--max-time",
                "10",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
                "-d",
                &body,
                &url,
            ])
            .output()
        {
            Ok(o) if o.status.success() => {
                tracing::info!(correlation = %p.correlation_id, "Telegram HITL card sent");
            }
            Ok(o) => tracing::warn!(
                stderr = %String::from_utf8_lossy(&o.stderr),
                "Telegram send non-zero"
            ),
            Err(e) => tracing::warn!(error = %e, "Telegram send failed (curl missing?)"),
        }
    }
}

/// Dispatch a card through all configured adapters. Blocking (curl may run) -
/// callers should invoke from `spawn_blocking`.
pub fn dispatch(payload: &WebhookPayload) {
    LoggingSender::new(crate::paths::state_dir().join("webhook_cards.log")).send(payload);
    TelegramSender.send(payload);
}
