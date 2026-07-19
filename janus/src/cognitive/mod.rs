//! `janus::cognitive` - Cognitive Provider SPI (ARCH-0.4.0 §III/§IV, Contracts
//! 4.1/4.2).
//!
//! The daemon never loads OpenWiki AST/graph data into its own heap. Instead it
//! queries an external provider through a narrow trait. Providers are opt-in
//! (configured per blueprint in `janus.toml` under `[cognitive]`); blueprints
//! without that section get a [`NoopProvider`] (fail-open - the existing Tool
//! Guard rule engine remains the sole gate).
//!
//! Invariants (§III):
//! - **Cannot block the tmux session.** `validate_command` runs in the Tool
//!   Guard verdict path with a hard 2s timeout; on timeout the daemon proceeds
//!   with the standard verdict (advisory, not gating).
//! - **Cannot read database state.** The provider receives only `argv` + `cwd`
//!   + `blueprint` name.
//! - **Opt-in per blueprint** via `[cognitive.codebase_memory]` in `janus.toml`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// Contract 4.1 - Cognitive Provider SPI. Implementations communicate via local
/// IPC (MCP over stdio for the codebase-memory adapter). The daemon holds at
/// most one active provider per blueprint.
pub trait CognitiveProvider: Send + Sync {
    /// Validate whether a command is consistent with the blueprint's domain
    /// constraints. Returns `None` when the provider has no opinion
    /// (pass-through); returns `Some(reason)` to recommend a BLOCK verdict with
    /// a human-readable explanation.
    fn validate_command(
        &self,
        blueprint: &str,
        argv: &[String],
        cwd: Option<&str>,
    ) -> Result<Option<String>, CognitiveError>;

    /// On Offboard, produce a condensed knowledge artifact for the blueprint.
    /// The returned string is written to `production_report.md` **in addition
    /// to** (not replacing) the existing LLM smelt output (a supplement).
    fn extract_knowledge(&self, blueprint: &str) -> Result<String, CognitiveError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CognitiveError {
    #[error("provider not reachable: {0}")]
    Unreachable(String),
    #[error("query timeout")]
    Timeout,
}

/// Hard timeout for the advisory `validate_command` check (§III invariant).
const VALIDATE_TIMEOUT: Duration = Duration::from_secs(2);

/// Fail-open default: no opinion on commands, empty knowledge artifact. Used
/// when a blueprint has no `[cognitive]` section.
pub struct NoopProvider;

impl CognitiveProvider for NoopProvider {
    fn validate_command(
        &self,
        _: &str,
        _: &[String],
        _: Option<&str>,
    ) -> Result<Option<String>, CognitiveError> {
        Ok(None)
    }

    fn extract_knowledge(&self, _: &str) -> Result<String, CognitiveError> {
        Ok(String::new())
    }
}

/// codebase-memory-mcp adapter (Contract 4.2). Speaks MCP JSON-RPC 2.0 over
/// stdio to an external `codebase-memory-mcp` child process. The provider is
/// spawned per call (0.4.0 simplification; a persistent lazy-spawned process
/// with an `initialize` handshake is a follow-on once the server contract is
/// pinned). All calls are bounded by a timeout; a late/missing response is
/// `CognitiveError::Timeout` (advisory pass-through).
pub struct McpProvider {
    command: String,
    args: Vec<String>,
    /// Transport timeout for `extract_knowledge` (default 5s). `validate_command`
    /// always uses [`VALIDATE_TIMEOUT`] (2s) per §III.
    timeout_secs: u64,
}

impl McpProvider {
    pub fn new(command: String, args: Vec<String>, timeout_secs: u64) -> Self {
        Self {
            command,
            args,
            timeout_secs,
        }
    }

    /// One JSON-RPC `tools/call` round-trip with a timeout. Returns the
    /// `result` field of the response (MCP `content[0].text` lives under it).
    fn call(&self, tool: &str, args: Value, timeout: Duration) -> Result<Value, CognitiveError> {
        let child = Command::new(&self.command)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CognitiveError::Unreachable(format!("spawn `{}`: {e}", self.command)))?;
        let mut guard = ChildGuard(child);

        // Send the request.
        if let Some(stdin) = guard.0.stdin.as_mut() {
            let req = serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": tool, "arguments": args}
            });
            let line = format!("{req}\n");
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| CognitiveError::Unreachable(format!("write: {e}")))?;
            stdin
                .flush()
                .map_err(|e| CognitiveError::Unreachable(format!("flush: {e}")))?;
        }
        // Drop stdin to signal the server (per-call model); the guard kills the
        // child on return regardless.
        drop(guard.0.stdin.take());

        let stdout = guard
            .0
            .stdout
            .take()
            .ok_or_else(|| CognitiveError::Unreachable("no stdout".into()))?;

        // Read one response line with a timeout (reader thread + channel). On
        // timeout the guard kills the child, unblocking the reader.
        let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let res = reader
                .read_line(&mut line)
                .map(|_| line)
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        let line = match rx.recv_timeout(timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => return Err(CognitiveError::Unreachable(format!("read: {e}"))),
            Err(_) => return Err(CognitiveError::Timeout),
        };
        let v: Value = serde_json::from_str(line.trim())
            .map_err(|e| CognitiveError::Unreachable(format!("parse response: {e}")))?;
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

impl CognitiveProvider for McpProvider {
    fn validate_command(
        &self,
        blueprint: &str,
        argv: &[String],
        cwd: Option<&str>,
    ) -> Result<Option<String>, CognitiveError> {
        let args = serde_json::json!({
            "blueprint": blueprint,
            "argv": argv,
            "cwd": cwd,
        });
        let result = self.call("validate_command", args, VALIDATE_TIMEOUT)?;
        Ok(extract_text(&result).filter(|t| !t.is_empty()))
    }

    fn extract_knowledge(&self, blueprint: &str) -> Result<String, CognitiveError> {
        let args = serde_json::json!({ "blueprint": blueprint });
        let timeout = Duration::from_secs(self.timeout_secs.max(1));
        let result = self.call("extract_knowledge", args, timeout)?;
        Ok(extract_text(&result).unwrap_or_default())
    }
}

/// Pull the `content[0].text` field out of an MCP `tools/call` result.
fn extract_text(result: &Value) -> Option<String> {
    result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .map(String::from)
}

/// Load the cognitive provider for a blueprint recipe. Returns a [`NoopProvider`]
/// when no `[cognitive]` section is configured (fail-open).
pub fn load_for_blueprint(recipe: &crate::recipe::BlueprintRecipe) -> Box<dyn CognitiveProvider> {
    if let Some(cog) = &recipe.cognitive
        && let Some(cm) = &cog.codebase_memory
    {
        Box::new(McpProvider::new(
            cm.command.clone(),
            cm.args.clone(),
            cm.timeout_secs,
        ))
    } else {
        Box::new(NoopProvider)
    }
}

/// Kill + reap the child on drop so a timed-out or errored call never leaks the
/// external process.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock provider that returns a canned BLOCK reason (UTC-10-06) or hangs
    /// (UTC-10-07, via `hang`).
    struct MockProvider {
        reason: Option<String>,
        knowledge: String,
    }
    impl CognitiveProvider for MockProvider {
        fn validate_command(
            &self,
            _: &str,
            _: &[String],
            _: Option<&str>,
        ) -> Result<Option<String>, CognitiveError> {
            Ok(self.reason.clone())
        }
        fn extract_knowledge(&self, _: &str) -> Result<String, CognitiveError> {
            Ok(self.knowledge.clone())
        }
    }

    #[test]
    fn noop_provider_is_fail_open() {
        let p = NoopProvider;
        assert_eq!(
            p.validate_command("bp", &["ls".into()], None).unwrap(),
            None
        );
        assert_eq!(p.extract_knowledge("bp").unwrap(), "");
    }

    #[test]
    fn mock_provider_recommends_block() {
        // UTC-10-06: a provider with an opinion returns Some(reason) -> the
        // daemon turns this into a BLOCK verdict + cognitive_context.
        let p = MockProvider {
            reason: Some("pin conflict: GPIO 21 I2C".into()),
            knowledge: String::new(),
        };
        let r = p
            .validate_command("gatemetric", &["make".into()], None)
            .unwrap();
        assert_eq!(r.as_deref(), Some("pin conflict: GPIO 21 I2C"));
    }

    #[test]
    fn mock_provider_pass_through_when_no_opinion() {
        // UTC-10-07 (pass-through side): None -> standard Tool Guard verdict.
        let p = MockProvider {
            reason: None,
            knowledge: String::new(),
        };
        assert_eq!(
            p.validate_command("bp", &["ls".into()], None).unwrap(),
            None
        );
    }

    #[test]
    fn extract_knowledge_returns_supplement() {
        // UTC-10-08 (provider side): extract_knowledge returns a non-empty
        // artifact the daemon appends to the LLM smelt report.
        let p = MockProvider {
            reason: None,
            knowledge: "## Cognitive Summary\nGPIO 21 I2C conflict observed.".into(),
        };
        let k = p.extract_knowledge("gatemetric").unwrap();
        assert!(k.contains("Cognitive Summary"));
    }

    #[test]
    fn mcp_provider_unreachable_when_binary_missing() {
        // A non-existent command -> spawn fails -> Unreachable (not a panic, not
        // a hang). The advisory check degrades to pass-through in the daemon.
        let p = McpProvider::new("definitely-not-a-real-binary-xyz-12345".into(), vec![], 5);
        let r = p.validate_command("bp", &["ls".into()], None);
        assert!(
            matches!(r, Err(CognitiveError::Unreachable(_))),
            "got {r:?}"
        );
    }

    #[test]
    fn mcp_provider_timeout_when_binary_hangs() {
        // `sleep 30` spawns but never writes to stdout -> the 2s
        // validate_command deadline fires -> Timeout. (sleep is ubiquitous on
        // macOS/Linux.) The child is killed by the ChildGuard on return.
        let p = McpProvider::new("sleep".into(), vec!["30".into()], 5);
        let r = p.validate_command("bp", &["ls".into()], None);
        match r {
            Err(CognitiveError::Timeout) => {}
            Err(CognitiveError::Unreachable(_)) => {} // sleep not on PATH
            other => panic!("expected Timeout/Unreachable, got {other:?}"),
        }
    }

    #[test]
    fn extract_text_reads_mcp_content_field() {
        let v = serde_json::json!({
            "content": [{"type": "text", "text": "pin conflict"}]
        });
        assert_eq!(extract_text(&v).as_deref(), Some("pin conflict"));

        // Missing content -> None.
        assert_eq!(extract_text(&serde_json::json!({})), None);
    }
}
