//! Tool Guard rule engine (Feature-Spec §2.2; Contracts 3.4/3.5; Project-Plan
//! M3 Task 3.2).
//!
//! The Daemon consults this engine for every command `janus-sh` forwards.
//! Decision priority (Feature-Spec §3.5 note):
//!   1. `bash_blacklist` hit         -> BLOCK  (SUSPENDED + HITL card)
//!   2. `require_approval` hit       -> BLOCK  (SUSPENDED + HITL card)
//!   3. capability not permitted     -> BLOCK  (denied, no HITL)
//!   4. financial-class command      -> REWRITE to dry-run (Contract 3.4)
//!   5. otherwise                    -> ALLOW
//!
//! Patterns are globs (`*` -> any chars), matched as a substring of the command
//! so chained (`a && b`) and subshelled (`bash -c "..."`) forms are caught. A
//! pattern ending in `/` (e.g. `rm -rf /`) additionally requires a shell
//! separator/quote/end after the slash, so `rm -rf /tmp/x` is NOT matched while
//! `rm -rf /`, `rm -rf / && echo`, and `bash -c "rm -rf /"` are.

pub mod rules;
pub mod webhook;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use regex::Regex;

pub use rules::{AgentProfile, AgentRules};

/// Agent role used when `JANUS_AGENT` is unset or unknown.
pub const DEFAULT_AGENT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    Allow,
    Block,
    Rewrite,
}

impl VerdictKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerdictKind::Allow => "ALLOW",
            VerdictKind::Block => "BLOCK",
            VerdictKind::Rewrite => "REWRITE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub kind: VerdictKind,
    pub reason: Option<String>,
    pub rewritten_argv: Option<Vec<String>>,
    pub correlation_id: String,
    /// What triggered the verdict: `blacklist` | `require_approval` |
    /// `permissions` | `financial`. Drives whether the HITL webhook fires.
    pub cause: Option<String>,
}

/// Compiled rule engine. Reloads `agents.toml` when its mtime changes
/// (Contract 3.5 hot-reload).
pub struct Engine {
    path: PathBuf,
    state: Mutex<State>,
}

struct State {
    rules: AgentRules,
    mtime: Option<std::time::SystemTime>,
    /// agent -> (original pattern, compiled regex), per rule class.
    blacklist: HashMap<String, Vec<(String, Regex)>>,
    approval: HashMap<String, Vec<(String, Regex)>>,
    financial: HashMap<String, Vec<(String, Regex)>>,
}

impl Engine {
    /// Load + compile rules from `path`. If the file is missing or unparseable,
    /// the engine runs empty (fail-open on missing config; the Daemon logs WARN)
    /// - `janus-sh` itself is the fail-CLOSED boundary when the Daemon is down.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = Self::load_fresh(&path);
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    /// Engine with no rules (used when `agents.toml` is unreadable).
    pub fn empty() -> Self {
        Self {
            path: PathBuf::new(),
            state: Mutex::new(State {
                rules: AgentRules::default(),
                mtime: None,
                blacklist: HashMap::new(),
                approval: HashMap::new(),
                financial: HashMap::new(),
            }),
        }
    }

    fn load_fresh(path: &Path) -> State {
        let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        let rules = if path.as_os_str().is_empty() || !path.exists() {
            tracing::warn!(
                "agents.toml not found at {}; Tool Guard running empty",
                path.display()
            );
            AgentRules::default()
        } else {
            AgentRules::load(path).unwrap_or_else(|e| {
                tracing::warn!("agents.toml load failed ({e}); Tool Guard running empty");
                AgentRules::default()
            })
        };
        let mut blacklist = HashMap::new();
        let mut approval = HashMap::new();
        let mut financial = HashMap::new();
        for (name, p) in &rules.agent {
            blacklist.insert(name.clone(), compile_patterns(&p.bash_blacklist));
            approval.insert(name.clone(), compile_patterns(&p.require_approval));
            financial.insert(name.clone(), compile_patterns(&p.financial));
        }
        State {
            rules,
            mtime,
            blacklist,
            approval,
            financial,
        }
    }

    fn refresh_if_changed(&self) {
        let mut guard = self.state.lock().expect("tool_guard state mutex");
        let mtime = std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| m.modified().ok());
        if mtime == guard.mtime {
            return;
        }
        let fresh = Self::load_fresh(&self.path);
        tracing::info!("agents.toml reloaded (hot-reload)");
        *guard = fresh;
    }

    /// Evaluate a command (Contract 3.2 fields) and return a Contract 3.4 verdict.
    pub fn evaluate(
        &self,
        execution_id: &str,
        argv: &[String],
        env_snapshot: &HashMap<String, String>,
    ) -> Verdict {
        self.refresh_if_changed();
        let guard = self.state.lock().expect("tool_guard state mutex");
        let cmd = command_string(argv);
        let correlation_id = execution_id.to_string();
        let agent = env_snapshot
            .get("JANUS_AGENT")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| DEFAULT_AGENT.to_string());
        let profile = guard
            .rules
            .agent
            .get(&agent)
            .or_else(|| guard.rules.agent.get(DEFAULT_AGENT));

        // 1. bash_blacklist (per-role, then the default baseline) + env-injection
        //    root-delete heuristic (UTC-02-05 case 5).
        if let Some(hit) =
            match_any(&guard.blacklist, &agent, &cmd).or_else(|| env_injection_root_delete(&cmd))
        {
            return Verdict {
                kind: VerdictKind::Block,
                reason: Some(format!("blacklisted: {hit}")),
                rewritten_argv: None,
                correlation_id,
                cause: Some("blacklist".to_string()),
            };
        }

        // 2. require_approval
        if let Some(hit) = match_any(&guard.approval, &agent, &cmd) {
            return Verdict {
                kind: VerdictKind::Block,
                reason: Some(format!("requires approval: {hit}")),
                rewritten_argv: None,
                correlation_id,
                cause: Some("require_approval".to_string()),
            };
        }

        // 3. permissions allowlist (only when a profile is resolved).
        if let Some(p) = profile
            && !permissions_allow(p, &cmd)
        {
            return Verdict {
                kind: VerdictKind::Block,
                reason: Some(format!("capability not permitted for agent '{agent}'")),
                rewritten_argv: None,
                correlation_id,
                cause: Some("permissions".to_string()),
            };
        }

        // 4. financial-class rewrite to dry-run.
        if match_any(&guard.financial, &agent, &cmd).is_some() {
            let rewritten = rewrite_dry_run(&cmd);
            let new_argv = rewritten.split_whitespace().map(String::from).collect();
            return Verdict {
                kind: VerdictKind::Rewrite,
                reason: Some("financial: forced dry-run".to_string()),
                rewritten_argv: Some(new_argv),
                correlation_id,
                cause: Some("financial".to_string()),
            };
        }

        // 5. allow
        Verdict {
            kind: VerdictKind::Allow,
            reason: None,
            rewritten_argv: None,
            correlation_id,
            cause: None,
        }
    }
}

/// Extract the command string from argv. A shell invoked as `sh -c "<cmd>"`
/// carries the real command in the arg after `-c`; otherwise join argv.
pub fn command_string(argv: &[String]) -> String {
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == "-c"
            && let Some(cmd) = it.next()
        {
            return cmd.clone();
        }
    }
    argv.join(" ")
}

fn compile_patterns(patterns: &[String]) -> Vec<(String, Regex)> {
    patterns
        .iter()
        .map(|p| {
            let re = compile_glob(p);
            (p.clone(), re)
        })
        .collect()
}

/// Compile a glob (`*` -> `.*`) into an unanchored regex. A pattern ending in
/// `/` gains a trailing boundary lookahead so `rm -rf /` does not match
/// `rm -rf /tmp/x` (but does match `rm -rf / && ...`, `bash -c "rm -rf /"`).
fn compile_glob(pattern: &str) -> Regex {
    let mut s = String::with_capacity(pattern.len() + 16);
    for c in pattern.chars() {
        match c {
            '*' => s.push_str(".*"),
            c if "\\.+?()[]{}|^$".contains(c) => {
                s.push('\\');
                s.push(c);
            }
            c => s.push(c),
        }
    }
    if pattern.ends_with('/') {
        // boundary: whitespace, quote, end, or a shell separator. Consuming
        // (not lookahead) - the `regex` crate has no lookarounds; for a boolean
        // is_match consuming the separator is equivalent.
        s.push_str(r#"(?:[\s"'&|;<>()]|$)"#);
    }
    Regex::new(&s).unwrap_or_else(|_| Regex::new(r"$^").expect("never-match regex"))
}

/// First matching pattern (by original text) for `agent`, falling back to the
/// `default` profile so the baseline blacklist applies to every role.
fn match_any(
    map: &HashMap<String, Vec<(String, Regex)>>,
    agent: &str,
    cmd: &str,
) -> Option<String> {
    let check = |name: &str| {
        map.get(name).and_then(|v| {
            v.iter()
                .find(|(_, re)| re.is_match(cmd))
                .map(|(p, _)| p.clone())
        })
    };
    check(agent).or_else(|| check(DEFAULT_AGENT))
}

/// UTC-02-05 case 5: `RM_TARGET=/ && rm -rf $RM_TARGET` - detect a var assigned
/// to `/` whose `$VAR` is then `rm -rf`-ed. Best-effort (the `regex` crate has
/// no backreferences, so this is a two-step scan).
fn env_injection_root_delete(cmd: &str) -> Option<String> {
    let re_assign = Regex::new(r#"([A-Za-z_][A-Za-z0-9_]*)=/(?:[\s&|;"']|$)"#).ok()?;
    let re_rm = Regex::new(r"rm\s+-rf\s+").ok()?;
    if !re_rm.is_match(cmd) {
        return None;
    }
    for cap in re_assign.captures_iter(cmd) {
        let var = &cap[1];
        let use_re = Regex::new(&format!(r"rm\s+-rf\s+.*\$[{{]?{var}[}}]?")).ok()?;
        if use_re.is_match(cmd) {
            return Some(format!("rm -rf / (via ${var})"));
        }
    }
    None
}

/// Infer a capability tag from the command's leading tokens, for the permissions
/// allowlist check. `git <sub>` is refined (`git commit` -> `git-commit`).
fn capability_of(cmd: &str) -> String {
    let mut toks = cmd.split_whitespace();
    let first = toks.next().unwrap_or("");
    let second = toks.next().unwrap_or("");
    match first {
        "ls" | "cat" | "head" | "tail" | "grep" | "find" | "pwd" | "echo" | "wc" | "tree"
        | "less" | "more" => "read".to_string(),
        "git" => match second {
            "log" | "show" | "status" | "diff" => "read".to_string(),
            "commit" => "git-commit".to_string(),
            "push" => "git-push".to_string(),
            _ => "git".to_string(),
        },
        "rm" | "mv" | "cp" | "mkdir" | "touch" | "chmod" | "chown" | "tee" => "write".to_string(),
        "ssh" | "scp" | "rsync" => "ssh".to_string(),
        _ => "other".to_string(),
    }
}

/// Step 3: is the command's capability permitted for this role?
fn permissions_allow(profile: &AgentProfile, cmd: &str) -> bool {
    if profile.permissions.iter().any(|p| p == "bash-full") {
        return true;
    }
    let cap = capability_of(cmd);
    if profile.permissions.iter().any(|p| p == &cap) {
        return true;
    }
    // bash_safe admits general (non-network) commands; network cmds need `ssh`.
    if (profile.bash_safe || profile.permissions.iter().any(|p| p == "bash-safe")) && cap != "ssh" {
        return true;
    }
    false
}

/// Contract 3.4 dry-run rewrite: `--action execute` -> `--action dry-run`,
/// else append ` --dry-run`.
fn rewrite_dry_run(cmd: &str) -> String {
    if cmd.contains("--action execute") {
        cmd.replace("--action execute", "--action dry-run")
    } else if cmd.contains(" execute") {
        cmd.replacen(" execute", " dry-run", 1)
    } else {
        format!("{cmd} --dry-run")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn engine() -> (Engine, NamedTempFile) {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"
[agent.scout]
permissions   = ["read", "grep", "find", "git-log"]
allow_network = false
bash_safe     = false

[agent.coder]
permissions    = ["read", "write", "edit", "bash-safe", "git-commit"]
allow_network  = false
bash_safe      = true
bash_blacklist = ["rm -rf /", "> /dev/sd*", "mkfs.*", "dd if=* of=/dev/*"]

[agent.deployer]
permissions      = ["read", "write", "bash-full", "ssh", "git-push"]
allow_network    = true
require_approval = ["esptool.py write_flash", "make flash", "*production*"]
financial        = ["hi5bot --action execute"]

[agent.default]
bash_safe      = true
bash_blacklist = ["rm -rf /", "> /dev/sd*", "mkfs.*", "dd if=* of=/dev/*"]
"#
        )
        .unwrap();
        let path = f.path().to_path_buf();
        (Engine::load(&path), f)
    }

    fn env(agent: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("JANUS_AGENT".to_string(), agent.to_string());
        m
    }

    fn eval(e: &Engine, agent: &str, cmd: &str) -> Verdict {
        e.evaluate("exec-1", &["-c".to_string(), cmd.to_string()], &env(agent))
    }

    #[test]
    fn scout_read_allowed() {
        let (e, _f) = engine();
        let v = eval(&e, "scout", "ls -la");
        assert_eq!(v.kind, VerdictKind::Allow);
    }

    #[test]
    fn scout_write_denied_permissions() {
        let (e, _f) = engine();
        let v = eval(&e, "scout", "rm /tmp/junk");
        assert_eq!(v.kind, VerdictKind::Block);
        assert_eq!(v.cause.as_deref(), Some("permissions"));
    }

    #[test]
    fn coder_tmp_delete_allowed_root_blocked() {
        let (e, _f) = engine();
        assert_eq!(
            eval(&e, "coder", "rm -rf /tmp/something").kind,
            VerdictKind::Allow
        );
        let v = eval(&e, "coder", "rm -rf /");
        assert_eq!(v.kind, VerdictKind::Block);
        assert_eq!(v.cause.as_deref(), Some("blacklist"));
    }

    #[test]
    fn coder_blacklist_globs() {
        let (e, _f) = engine();
        assert_eq!(
            eval(&e, "coder", "dd if=img of=/dev/sda").kind,
            VerdictKind::Block
        );
        assert_eq!(
            eval(&e, "coder", "mkfs.ext4 /dev/sda").kind,
            VerdictKind::Block
        );
        assert_eq!(
            eval(&e, "coder", "echo hi > /dev/sda").kind,
            VerdictKind::Block
        );
    }

    #[test]
    fn chain_and_subshell_caught() {
        let (e, _f) = engine();
        assert_eq!(
            eval(&e, "coder", "rm -rf / && echo done").kind,
            VerdictKind::Block
        );
        assert_eq!(
            eval(&e, "coder", "bash -c \"rm -rf /\"").kind,
            VerdictKind::Block
        );
    }

    #[test]
    fn env_injection_root_delete_blocked() {
        let (e, _f) = engine();
        let v = eval(&e, "coder", "RM_TARGET=/ && rm -rf $RM_TARGET");
        assert_eq!(v.kind, VerdictKind::Block);
        assert_eq!(v.cause.as_deref(), Some("blacklist"));
    }

    #[test]
    fn deployer_require_approval_blocks() {
        let (e, _f) = engine();
        let v = eval(&e, "deployer", "esptool.py write_flash 0x1000 fw.bin");
        assert_eq!(v.kind, VerdictKind::Block);
        assert_eq!(v.cause.as_deref(), Some("require_approval"));
        assert_eq!(eval(&e, "deployer", "make flash").kind, VerdictKind::Block);
        assert_eq!(
            eval(&e, "deployer", "run-production-deploy").kind,
            VerdictKind::Block
        );
    }

    #[test]
    fn deployer_general_allowed() {
        let (e, _f) = engine();
        assert_eq!(
            eval(&e, "deployer", "ssh builder@host 'ls'").kind,
            VerdictKind::Allow
        );
        assert_eq!(
            eval(&e, "deployer", "git push origin main").kind,
            VerdictKind::Allow
        );
    }

    #[test]
    fn financial_rewritten_to_dry_run() {
        let (e, _f) = engine();
        let v = eval(&e, "deployer", "hi5bot --action execute");
        assert_eq!(v.kind, VerdictKind::Rewrite);
        assert_eq!(v.cause.as_deref(), Some("financial"));
        let argv = v.rewritten_argv.unwrap();
        assert_eq!(argv, vec!["hi5bot", "--action", "dry-run"]);
    }

    #[test]
    fn unknown_agent_falls_back_to_default_blacklist() {
        let (e, _f) = engine();
        let v = eval(&e, "stranger", "rm -rf /");
        assert_eq!(v.kind, VerdictKind::Block);
        assert_eq!(v.cause.as_deref(), Some("blacklist"));
    }

    #[test]
    fn command_string_extracts_c_arg() {
        assert_eq!(command_string(&["-c".into(), "ls -la".into()]), "ls -la");
        assert_eq!(
            command_string(&["script.sh".into(), "arg".into()]),
            "script.sh arg"
        );
    }
}
