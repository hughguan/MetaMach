//! `TeamsSender` - Microsoft Teams Adaptive Card adapter (ARCH-0.4.0 §5.2,
//! Contract 4.3b). Translates an enriched [`WebhookPayload`] into an Adaptive
//! Card with `Approve` / `Reject` / `Override` actions and POSTs it to the
//! configured Teams Incoming Webhook URL. No-ops when the URL is unset - the
//! [`super::LoggingSender`] already recorded the card for audit.
//!
//! Outbound transport is `ureq` (already a dependency); inbound callback HMAC
//! validation lives in [`super`] (the loopback HTTP listener). The daemon never
//! handles TLS or a public port - a tunnel/reverse proxy fronts the listener
//! (§5.1b).

use crate::protocol::WebhookPayload;
use crate::tool_guard::webhook::WebhookSender;

/// Teams Incoming Webhook URL (outbound card destination).
const TEAMS_URL_ENV: &str = "JANUS_TEAMS_WEBHOOK_URL";
/// Public base URL the card's action buttons POST back to. In production this is
/// the tunnel/reverse-proxy fronting the gateway's loopback listener (§5.1b).
const TEAMS_CALLBACK_BASE_ENV: &str = "JANUS_TEAMS_CALLBACK_URL";

/// Default callback base (the gateway's own loopback listener).
const DEFAULT_CALLBACK_BASE: &str = "http://127.0.0.1:8443";

pub struct TeamsSender;

impl WebhookSender for TeamsSender {
    fn send(&self, p: &WebhookPayload) {
        let Some(url) = std::env::var(TEAMS_URL_ENV).ok().filter(|s| !s.is_empty()) else {
            return; // not configured - LoggingSender covers the audit path
        };
        let callback_base = std::env::var(TEAMS_CALLBACK_BASE_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_CALLBACK_BASE.to_string());
        let card = build_card(p, &callback_base);
        match ureq::post(&url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(10))
            .send_string(&card)
        {
            Ok(_) => tracing::info!(correlation = %p.correlation_id, "Teams HITL card sent"),
            Err(e) => tracing::warn!(
                correlation = %p.correlation_id,
                error = %e,
                "Teams HITL send failed"
            ),
        }
    }
}

/// Build the Adaptive Card JSON (Contract 4.3b). The three action buttons POST
/// `approve` / `reject` / `override` to `/v1/runs/{run_id}/actions` on the
/// callback base URL. `stdout_tail` is already 16 KiB-capped by
/// [`WebhookPayload::build`].
fn build_card(p: &WebhookPayload, callback_base: &str) -> String {
    let base = callback_base.trim_end_matches('/');
    let cb = format!("{base}/v1/runs/{}/actions", p.correlation_id);
    serde_json::json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [
                    {"type": "TextBlock", "text": format!("🛑 HITL: {}", p.cause), "weight": "Bolder", "size": "Medium"},
                    {"type": "TextBlock", "text": format!("Blueprint: {}", p.blueprint), "isSubtle": true},
                    {"type": "TextBlock", "text": format!("Step: {}", p.step), "isSubtle": true},
                    {"type": "TextBlock", "text": format!("Command: {}", p.command), "wrap": true},
                    {"type": "TextBlock", "text": format!("{}\n{}", p.reason, p.stdout_tail), "wrap": true, "isSubtle": true},
                    {"type": "TextBlock", "text": format!("Expires: {}", p.expires_at), "isSubtle": true, "spacing": "Small"}
                ],
                "actions": [
                    {"type": "Action.Http", "title": "Approve", "method": "POST", "url": &cb,
                     "body": r#"{"action":"approve","approved_by":"teams"}"#},
                    {"type": "Action.Http", "title": "Reject", "method": "POST", "url": &cb,
                     "body": r#"{"action":"reject","approved_by":"teams"}"#},
                    {"type": "Action.Http", "title": "Override", "method": "POST", "url": &cb,
                     "body": r#"{"action":"override","override_command":"","approved_by":"teams"}"#}
                ]
            }
        }]
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> WebhookPayload {
        WebhookPayload::build(
            None,
            "exec-1",
            "corr-9",
            "require_approval",
            "make flash",
            "needs approval",
            "gatemetric",
            "flash",
        )
    }

    #[test]
    fn card_has_adaptive_card_schema_and_actions() {
        // UTC-10-05: valid Adaptive Card JSON with Approve/Reject/Override
        // actions, each targeting /v1/runs/{run_id}/actions.
        let card = build_card(&payload(), "http://127.0.0.1:8443");
        let v: serde_json::Value = serde_json::from_str(&card).unwrap();
        assert_eq!(v["type"], "message");
        assert_eq!(
            v["attachments"][0]["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
        let content = &v["attachments"][0]["content"];
        assert_eq!(content["type"], "AdaptiveCard");
        let actions = content["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        let titles: Vec<&str> = actions
            .iter()
            .map(|a| a["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, vec!["Approve", "Reject", "Override"]);
        for a in actions {
            assert_eq!(a["method"], "POST");
            assert!(
                a["url"]
                    .as_str()
                    .unwrap()
                    .ends_with("/v1/runs/corr-9/actions"),
                "url: {}",
                a["url"]
            );
        }
        // Title carries the cause; body carries blueprint + step.
        assert!(
            content["body"][0]["text"]
                .as_str()
                .unwrap()
                .contains("require_approval")
        );
        assert!(
            content["body"][1]["text"]
                .as_str()
                .unwrap()
                .contains("gatemetric")
        );
    }

    #[test]
    fn send_no_ops_when_url_unset() {
        // No panic / clean return when Teams is not configured.
        // SAFETY: env mutation is not thread-safe; this test is the sole toucher
        // of TEAMS_URL_ENV in the suite.
        unsafe { std::env::remove_var(TEAMS_URL_ENV) };
        TeamsSender.send(&payload());
    }
}
