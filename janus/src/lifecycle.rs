//! Blueprint lifecycle: Onboard + Offboard (Feature-Spec §2.5, Contracts 3.6/3.7;
//! Project-Plan Tasks 4.2/4.3).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tracing::{info, warn};

use crate::absurd::{AbsurdDb, StepTrace};
use crate::cognitive;
use crate::recipe;

// --- Offboard LLM config (configs/offboard.toml, Feature-Spec §2.5) ----------

#[derive(Debug, Clone, Deserialize)]
pub struct OffboardConfig {
    pub llm: LlmConfig,
    #[serde(default)]
    pub input: InputConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub endpoint: String,
    pub api_key_env: String,
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_input_tokens: usize,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputConfig {
    #[serde(default = "default_recent_steps")]
    pub recent_steps: usize,
    #[serde(default = "default_per_step_truncate")]
    pub per_step_truncate_bytes: usize,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            recent_steps: default_recent_steps(),
            per_step_truncate_bytes: default_per_step_truncate(),
        }
    }
}

fn default_max_tokens() -> usize {
    60000
}
fn default_timeout() -> u64 {
    120
}
fn default_recent_steps() -> usize {
    50
}
fn default_per_step_truncate() -> usize {
    16 * 1024
}

impl OffboardConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read offboard config {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        Ok(cfg)
    }
}

// --- Onboard (Task 4.3) -----------------------------------------------------

/// Result of a successful Onboard (Feature-Spec §2.5.5/§2.5.6).
#[derive(Debug, Clone)]
pub struct OnboardResult {
    pub message: String,
    /// `## Previous Incidents` few-shot lines parsed from a prior
    /// `production_report.md` (experience inheritance, UTC-05-05).
    pub previous_incidents: Vec<String>,
    pub reactivated: bool,
}

pub async fn onboard(db: &AbsurdDb, name: &str, repo_root: &Path) -> Result<OnboardResult> {
    // Step 1: recipe validation (NO database write on failure).
    let recipe = recipe::validate(name, repo_root)?;

    // Step 2: pre-ignition self-checks.
    if !db.pg_online().await {
        bail!(
            "Absurd Postgres not reachable - cannot register tenant (start it with `make db-up`)"
        );
    }
    pre_ignition_checks(&recipe);

    // Step 3: idempotent tenant registration (INSERT ... ON CONFLICT DO UPDATE).
    let reactivated = db.register_blueprint(&recipe).await?;

    // Step 5: knowledge-graph loading + experience inheritance.
    let previous_incidents = load_previous_incidents(repo_root, name);

    info!(
        %name,
        reactivated,
        incidents = previous_incidents.len(),
        "blueprint onboarded"
    );
    Ok(OnboardResult {
        message: format!(
            "blueprint `{name}` {} (workflow: {}, host: {}){}",
            if reactivated {
                "reactivated"
            } else {
                "registered"
            },
            recipe.default_workflow,
            recipe.remote_host.as_deref().unwrap_or("local"),
            if previous_incidents.is_empty() {
                String::new()
            } else {
                format!(
                    "; inherited {} previous-incident(s)",
                    previous_incidents.len()
                )
            },
        ),
        previous_incidents,
        reactivated,
    })
}

/// Best-effort pre-ignition checks (Feature-Spec §2.5.2). tmux + remote SSH are
/// WARN-only (offline Onboard first, fill target later). PG is hard-required
/// (checked by the caller).
fn pre_ignition_checks(recipe: &recipe::ValidatedRecipe) {
    if !tmux_ready() {
        warn!("pre-ignition: tmux not ready (tmux sessions need it) - continuing");
    }
    if let Some(host) = recipe.remote_host.as_deref()
        && !ssh_probe(host)
    {
        warn!("pre-ignition: remote host {host} unreachable - continuing (WARN only)");
    }
}

fn tmux_ready() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Best-effort SSH connectivity probe (`-o ConnectTimeout=5 -o BatchMode`).
fn ssh_probe(host: &str) -> bool {
    std::process::Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            host,
            "true",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Parse `blueprints/<name>/openwiki/production_report.md` and extract incident
/// lines for `## Previous Incidents` few-shot injection (Feature-Spec §2.5.5,
/// UTC-05-05). Returns an empty vec if no prior report exists.
pub fn load_previous_incidents(repo_root: &Path, name: &str) -> Vec<String> {
    let path: PathBuf = repo_root
        .join("blueprints")
        .join(name)
        .join("openwiki")
        .join("production_report.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_incidents(&text)
}

/// Extract bullet lines (`- ...`) from a production report as incident few-shots.
pub fn parse_incidents(report: &str) -> Vec<String> {
    report
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("- "))
        .map(|l| l.trim_start_matches("- ").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// --- Offboard (Task 4.2) ----------------------------------------------------

#[derive(Debug, Clone)]
pub struct OffboardResult {
    pub message: String,
    pub report_path: PathBuf,
    pub purged_rows: i64,
    pub git: Option<String>,
    pub llm_used: bool,
}

pub async fn offboard(
    db: &AbsurdDb,
    name: &str,
    repo_root: &Path,
    cfg: &OffboardConfig,
) -> Result<OffboardResult> {
    if !db.pg_online().await {
        bail!("Absurd Postgres not reachable - cannot offboard (start it with `make db-up`)");
    }

    // Step 1: scan historical Step traces for this blueprint.
    let traces = db
        .scan_offboard_traces(name, cfg.input.recent_steps)
        .await?;

    // Step 2: LLM-smelt into Markdown (fallback: raw JSON snapshot). The LLM
    // call is blocking (up to timeout_seconds), so run it off the async worker.
    let (report, llm_used) = smelt_async(&traces, name, cfg).await;

    // 0.4.0 Contract 4.1: append a CognitiveProvider's knowledge artifact as a
    // supplement (never replaces the LLM smelt). Fail-open: provider errors /
    // no provider configured -> report unchanged.
    let report = cognitive_supplement(&report, name, repo_root).await;

    let openwiki_dir = repo_root.join("blueprints").join(name).join("openwiki");
    std::fs::create_dir_all(&openwiki_dir).context("create openwiki dir")?;
    let report_path = openwiki_dir.join("production_report.md");
    std::fs::write(&report_path, &report).context("write production_report.md")?;

    // Step 3: PG anti-bloat pruning - physically DELETE result_cache JSON.
    let purged_rows = db.offboard_blueprint_data(name).await?;

    // Step 4: mark the blueprint OFFBOARDED.
    db.set_blueprint_offboarded(name).await?;

    // Step 5: best-effort git commit/push of the report.
    let git = git_commit_report(repo_root, name, &report_path);

    info!(%name, purged_rows, llm_used, "blueprint offboarded");
    Ok(OffboardResult {
        message: format!(
            "offboarded `{name}` - smelted {} step trace(s) ({}), purged {} row(s)",
            traces.len(),
            if llm_used { "LLM" } else { "raw-JSON fallback" },
            purged_rows,
        ),
        report_path,
        purged_rows,
        git,
        llm_used,
    })
}

/// 0.4.0 Contract 4.1: append a CognitiveProvider's `extract_knowledge` artifact
/// to the LLM-smelt report as a supplement (never a replacement). Blueprints
/// without a `[cognitive]` section get a `NoopProvider` (empty string) -> no-op.
/// Any error (no recipe, provider failure, join error) is fail-open: the report
/// is returned unchanged.
async fn cognitive_supplement(report: &str, name: &str, repo_root: &Path) -> String {
    let provider = match recipe::load_recipe(name, repo_root) {
        Ok(r) => cognitive::load_for_blueprint(&r),
        Err(e) => {
            warn!("cognitive: load_recipe for {name} failed ({e}); no supplement");
            return report.to_string();
        }
    };
    let name_owned = name.to_string();
    let supplement =
        tokio::task::spawn_blocking(move || provider.extract_knowledge(&name_owned)).await;
    match supplement {
        Ok(Ok(text)) if !text.trim().is_empty() => {
            format!("{report}\n\n## Cognitive Provider Summary\n\n{text}\n")
        }
        Ok(Ok(_)) => report.to_string(), // empty supplement (NoopProvider)
        Ok(Err(e)) => {
            warn!("cognitive extract_knowledge for {name} failed ({e}); report unchanged");
            report.to_string()
        }
        Err(e) => {
            warn!("cognitive extract_knowledge join error for {name} ({e}); report unchanged");
            report.to_string()
        }
    }
}

async fn smelt_async(traces: &[StepTrace], name: &str, cfg: &OffboardConfig) -> (String, bool) {
    let traces = traces.to_vec();
    let name = name.to_string();
    let name_fallback = name.clone();
    let cfg = cfg.clone();
    match tokio::task::spawn_blocking(move || smelt(&traces, &name, &cfg)).await {
        Ok(pair) => pair,
        Err(e) => {
            warn!("smelt task join error: {e}");
            (raw_json_snapshot(&[], &name_fallback), false)
        }
    }
}

/// Build the LLM input + summarize. On any LLM failure (no key, offline,
/// timeout), fall back to a raw JSON snapshot (Feature-Spec §2.5 degradation).
fn smelt(traces: &[StepTrace], name: &str, cfg: &OffboardConfig) -> (String, bool) {
    let input = build_llm_input(traces, cfg);
    match llm_summarize(&input, name, cfg) {
        Ok(md) => (md, true),
        Err(e) => {
            warn!("offboard LLM smelt failed ({e}); writing raw JSON snapshot");
            (raw_json_snapshot(traces, name), false)
        }
    }
}

fn build_llm_input(traces: &[StepTrace], cfg: &OffboardConfig) -> String {
    let cap = cfg.input.per_step_truncate_bytes;
    let mut out = String::new();
    // most-recent first; discard excess beyond recent_steps (reverse-chronological).
    for t in traces.iter().rev().take(cfg.input.recent_steps) {
        let cache = t.result_cache.as_deref().unwrap_or("");
        let cache = truncate_bytes(cache, cap);
        out.push_str(&format!(
            "### task {} step `{}` [{}]\n{}\n\n",
            t.task_id, t.step_name, t.status, cache
        ));
    }
    out
}

/// Truncate at a UTF-8 char boundary <= `cap` bytes.
fn truncate_bytes(s: &str, cap: usize) -> &str {
    if s.len() <= cap {
        return s;
    }
    let mut i = cap;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

/// Call the configured LLM endpoint (OpenAI-style chat completions). Returns the
/// assistant's Markdown with the four required blocks (Feature-Spec §2.5).
fn llm_summarize(input: &str, name: &str, cfg: &OffboardConfig) -> Result<String> {
    let api_key = std::env::var(&cfg.llm.api_key_env)
        .map_err(|_| anyhow::anyhow!("LLM api key env var {} not set", cfg.llm.api_key_env))?;
    let system = format!(
        "You are the MetaMach Offboard smelter for blueprint `{name}`. Summarize the \
         execution traces into a high-density Markdown production report with exactly \
         these four sections: `## Compile Error History`, `## Pin Conflict Details`, \
         `## Tool Guard Interception Logs`, `## Successful Patches Applied`. Be terse."
    );
    let body = serde_json::json!({
        "model": cfg.llm.model,
        "max_tokens": 2048,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": input},
        ]
    });
    let resp = ureq::post(&cfg.llm.endpoint)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(cfg.llm.timeout_seconds))
        .send_json(body)
        .map_err(|e| anyhow::anyhow!("LLM HTTP request failed: {e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| anyhow::anyhow!("parse LLM response JSON: {e}"))?;
    let md = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("LLM response missing choices[0].message.content"))?
        .to_string();
    Ok(md)
}

/// Degradation fallback: a raw JSON snapshot of the traces (Feature-Spec §2.5).
fn raw_json_snapshot(traces: &[StepTrace], name: &str) -> String {
    let snapshot = serde_json::json!({
        "blueprint": name,
        "note": "LLM unavailable - raw trace snapshot (Feature-Spec §2.5 degradation)",
        "traces": traces.iter().map(|t| serde_json::json!({
            "task_id": t.task_id,
            "step_name": t.step_name,
            "status": t.status,
            "result_cache": t.result_cache,
        })).collect::<Vec<_>>(),
    });
    let body = serde_json::to_string_pretty(&snapshot).unwrap_or_default();
    format!("# {name} - Production Report (raw snapshot)\n\n```json\n{body}\n```\n")
}

/// Best-effort `git add` + `git commit` (+ `push`) of the production report
/// (Feature-Spec §2.5.4). Returns the commit short hash on success; None if git
/// is unavailable or there's nothing to commit.
fn git_commit_report(repo_root: &Path, name: &str, report: &Path) -> Option<String> {
    let rel = report
        .strip_prefix(repo_root)
        .unwrap_or(report)
        .to_string_lossy()
        .into_owned();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    if run(&["add", &rel]).is_none() {
        warn!("offboard git: `git add {rel}` failed (best-effort, skipping)");
        return None;
    }
    let msg = format!("feat(blueprint): {name} offboard production_report.md");
    // None (early return) if commit failed - e.g. nothing to commit.
    let _commit = run(&["commit", "-m", &msg])?;
    let _ = run(&["push"]); // best-effort; a missing remote is fine
    // `git commit`'s stdout is the summary line "[<branch> <hash>] <subject>",
    // not the hash alone - resolve the short hash of HEAD explicitly.
    let out = run(&["rev-parse", "--short", "HEAD"])?;
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    info!(%name, git = %hash, "offboard: committed production_report.md");
    Some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::absurd::StepTrace;
    use uuid::Uuid;

    fn trace(task: Uuid, step: &str, cache: &str) -> StepTrace {
        StepTrace {
            task_id: task,
            step_name: step.into(),
            status: "FAILED".into(),
            result_cache: Some(cache.into()),
        }
    }

    #[test]
    fn parse_incidents_extracts_bullets_with_marker() {
        let report = r#"
# Gatemetric - Production Report

## Previous Incidents

- PIN_CONFLICT_MARKER_21: GPIO 21 I2C conflict; use GPIO 22.
- ESP32 timer collision on core 1.

## Compile Error History

- `MPU6050.cpp:42` undefined reference to `Wire.begin()`.

## Successful Patches Applied

- I2C bus speed lowered to 100kHz.
"#;
        let incidents = parse_incidents(report);
        assert!(
            incidents
                .iter()
                .any(|i| i.contains("PIN_CONFLICT_MARKER_21")),
            "{incidents:?}"
        );
        assert_eq!(incidents.len(), 4);
    }

    #[test]
    fn raw_json_snapshot_embeds_valid_json() {
        let traces = vec![trace(Uuid::from_u128(7), "compile", "error: pin 21")];
        let snap = raw_json_snapshot(&traces, "gatemetric");
        // extract the ```json ... ``` block (pretty-printed, multi-line)
        let json: String = snap
            .lines()
            .skip_while(|l| !l.trim_start().starts_with("```json"))
            .skip(1) // skip the ```json fence
            .take_while(|l| !l.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["blueprint"], "gatemetric");
        assert_eq!(v["traces"][0]["step_name"], "compile");
    }

    #[test]
    fn build_llm_input_caps_steps_and_truncates() {
        let cfg = OffboardConfig {
            llm: LlmConfig {
                endpoint: "x".into(),
                api_key_env: "X".into(),
                model: "m".into(),
                max_input_tokens: 1,
                timeout_seconds: 1,
            },
            input: InputConfig {
                recent_steps: 2,
                per_step_truncate_bytes: 10,
            },
        };
        let traces: Vec<StepTrace> = (0..5)
            .map(|i| {
                trace(
                    Uuid::from_u128(i as u128),
                    &format!("s{i}"),
                    "0123456789ABCDEF",
                )
            })
            .collect();
        let input = build_llm_input(&traces, &cfg);
        // only the 2 most-recent steps (reverse), each truncated to 10 bytes.
        assert!(input.contains("s4"));
        assert!(input.contains("s3"));
        assert!(!input.contains("s2"));
        assert!(!input.contains("ABCDEF")); // truncated off
    }

    #[test]
    fn truncate_bytes_respects_char_boundary() {
        // é = 2 bytes; a 3-byte cap must not split it.
        let s = "éééé";
        let t = truncate_bytes(s, 3);
        assert!(t.chars().all(|c| c == 'é'));
        assert_eq!(t.len(), 2); // one é (2 bytes) fits, the second would exceed 3.
    }

    #[test]
    fn offboard_config_loads_with_defaults() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("offboard.toml"),
            r#"
[llm]
endpoint = "https://api.example.com/v1/chat"
api_key_env = "MY_KEY"
model = "gpt-4o-mini"
max_input_tokens = 1000
timeout_seconds = 5
[input]
recent_steps = 7
"#,
        )
        .unwrap();
        let cfg = OffboardConfig::load(&d.path().join("offboard.toml")).unwrap();
        assert_eq!(cfg.llm.model, "gpt-4o-mini");
        assert_eq!(cfg.input.recent_steps, 7);
        assert_eq!(cfg.input.per_step_truncate_bytes, 16 * 1024); // default kept
    }

    #[test]
    fn git_commit_report_returns_short_hash() {
        // `git commit`'s stdout is the "[<branch> <hash>] <subject>" summary line,
        // not the hash - git_commit_report must resolve the short hash explicitly.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let sh = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        // Skip (don't fail) if git isn't usable in this environment.
        if !sh(&["init"])
            || !sh(&["config", "user.email", "ci@example.com"])
            || !sh(&["config", "user.name", "ci"])
        {
            eprintln!("git unavailable; skipping git_commit_report_returns_short_hash");
            return;
        }
        let report = repo.join("production_report.md");
        std::fs::write(&report, "# report\n").unwrap();
        let hash =
            git_commit_report(repo, "gatemetric", &report).expect("git commit should succeed");
        assert!(!hash.is_empty(), "hash should not be empty");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "expected a short hash, got: {hash}"
        );
        // Must match `git rev-parse --short HEAD` exactly (not the summary line).
        let expect = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(hash, expect.trim());
    }
}
