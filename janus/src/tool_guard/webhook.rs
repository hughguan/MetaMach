//! HITL webhook card (Feature-Spec §2.4). Abstract payload + adapters.
//!
//! The Daemon constructs only an abstract [`WebhookPayload`]; each adapter
//! translates it into its native format. [`LoggingSender`] always fires (to
//! `janus.log` + a state-dir spool); [`TelegramSender`] (primary backend) POSTs
//! to the Telegram Bot API with an inline `[Resume]` keyboard when
//! `JANUS_TELEGRAM_TOKEN` + `JANUS_TELEGRAM_CHAT_ID` are configured, and no-ops
//! otherwise (the log card covers the test path until secrets are provisioned).

use std::process::Command;

use serde::Serialize;

use crate::absurd::{SIZE_BUDGET, truncate_16k};

/// Abstract HITL card (Feature-Spec §2.4: task UUID, cause, 16KB-truncated
/// scene, Resume trigger key + Correlation ID).
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub task_id: Option<i64>,
    pub execution_id: String,
    pub correlation_id: String,
    pub cause: String,
    pub command: String,
    pub reason: String,
    /// 16KB-truncated stdout scene. M3 has no captured stdout, so the
    /// intercepted command + reason stand in for the scene.
    pub scene: String,
    pub resume_key: String,
}

impl WebhookPayload {
    pub fn build(
        task_id: Option<i64>,
        execution_id: &str,
        correlation_id: &str,
        cause: &str,
        command: &str,
        reason: &str,
    ) -> Self {
        let scene_src = format!("{command}\n{reason}");
        // The scene is the only unbounded field; hard-cap at the 16KiB budget.
        let scene = if scene_src.len() > SIZE_BUDGET {
            truncate_16k(&scene_src)
        } else {
            scene_src
        };
        Self {
            task_id,
            execution_id: execution_id.to_string(),
            correlation_id: correlation_id.to_string(),
            cause: cause.to_string(),
            command: command.to_string(),
            reason: reason.to_string(),
            scene,
            resume_key: format!("metamach-resume:{correlation_id}"),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_scene_capped_to_16kib() {
        let big = "x".repeat(SIZE_BUDGET * 4);
        let p = WebhookPayload::build(Some(7), "exec", "corr", "blacklist", &big, "r");
        assert!(
            p.scene.len() <= SIZE_BUDGET,
            "scene {} > budget",
            p.scene.len()
        );
        assert!(p.scene.ends_with("[MetaMach Log Budget Exceeded]"));
    }

    #[test]
    fn resume_key_carries_correlation() {
        let p = WebhookPayload::build(None, "exec-1", "corr-9", "require_approval", "cmd", "r");
        assert_eq!(p.resume_key, "metamach-resume:corr-9");
        assert_eq!(p.correlation_id, "corr-9");
    }
}
