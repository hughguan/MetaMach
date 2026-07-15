# Feature-Spec Review — Engineering Specification Deep-Dive

> **Document under review:** `docs/CH/Feature-Spec.md`
> **Review lens:** Specification completeness, implementability, data contract rigor, edge case coverage, internal consistency

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Cannot implement feature without resolution |
| 🟠 **HIGH** | Will cause significant rework or security flaw |
| 🟡 **MEDIUM** | Ambiguity, missing edge case, or underspecification |
| ⚪ **LOW** | Naming, formatting |

---

## 1. 🔴 Implementation Blockers

### 1.1 Tool Guard decision matrix is entirely unspecified

Feature §2.2 describes Tool Guard as a "安全卫兵决策矩阵" (security guard decision matrix) that checks commands against `configs/agents.toml` permission levels. But:

- What does `configs/agents.toml` look like? No schema provided anywhere.
- What are the permission levels? (Scout / Coder / Deployer are mentioned in ARCH.md but never mapped to allowed commands)
- What constitutes a "高危命令"? Is there a static blacklist? A regex rules engine? An allowlist?
- How does Tool Guard distinguish between `rm -rf /` (always block) and `rm -rf ./build/` (may be valid for Coder)?
- Is the decision matrix hardcoded in Rust or configurable via a rules file?

**Recommendation:** Add a complete `configs/agents.toml` schema and Tool Guard rule specification:

```toml
[agent.scout]
permissions = ["read", "grep", "find", "git-log"]
allow_network = false

[agent.coder]
permissions = ["read", "write", "edit", "bash-safe", "git-commit"]
allow_network = false
bash_safe_blacklist = ["rm -rf /", "> /dev/sda", "mkfs.*", "dd if=* of=/dev/*"]

[agent.deployer]
permissions = ["read", "write", "bash-full", "ssh", "git-push"]
allow_network = true
require_approval = ["esptool.py write_flash", "make flash", "*production*"]
```

---

### 1.2 UDS protocol between janus-sh and Daemon is incomplete

Contract §3.2 defines the **request** payload (janus-sh → Daemon) but the **response** payload (Daemon → janus-sh) is never defined. What does `janus-sh` receive back?

- `ALLOW` — execute as-is? What's the wire format?
- `REWRITE` — receive modified argv? In what format?
- `BLOCK` — return an error? What error code?
- Timeout — what happens?

**Recommendation:** Add the response contract:

```json
// Daemon → janus-sh response
{
  "execution_id": "0190b2c1-...",
  "verdict": "ALLOW" | "BLOCK" | "REWRITE",
  "reason": "financial_trade_requires_approval",
  "rewritten_argv": ["hi5bot", "--action", "dry-run"],  // only present if REWRITE
  "correlation_id": "0190b2c1-..."                      // for audit trail
}
```

---

### 1.3 Workflow step execution model is undefined

Feature §2.3 says "每个 Workflow Step 执行前，Daemon 必须在物理数据库中提交 `UPDATE` 锁死过渡态（`STARTING`）". But:

- What IS a Step? Is it a shell command? An agent invocation? A sequence of agent turns?
- How does the Daemon know when a Step is "done"?
- Who monitors the Step's progress? Is there a heartbeat?
- What happens if the agent process exits but the Step should continue (multi-turn)?
- How are Step outputs captured as `result_cache`? Is it the final stdout? The agent's last message? The entire terminal buffer?

**Recommendation:** Define the Step lifecycle explicitly:

```
Step = {
    agent_type: "scout" | "coder" | "deployer",
    prompt: string,               // initial instruction to the agent
    expected_output: "diff" | "stdout" | "exit_code",
    timeout_seconds: 600,
    max_turns: 20,
    capture: "final_stdout" | "full_buffer",
}
```

Execution model: Daemon spawns agent in tmux pane → agent runs multi-turn → agent signals completion (exit code 0 or special token) → Daemon captures output → writes `result_cache` → transitions to COMPLETED.

---

## 2. 🟠 Critical Specification Gaps

### 2.1 `fork()` + `exec()` lazy-start is platform-specific

Feature §2.1 says `herdr-janus` uses `fork()` + `exec()` for lazy Daemon startup. This is a Unix-specific API:

- On macOS, `fork()` without `exec()` is explicitly discouraged by Apple (see `man fork` on macOS 13+)
- On Windows (if ever supported), `fork()` doesn't exist
- What if `fork()` fails (resource limits)?

**Recommendation:** Use `std::process::Command::spawn()` with proper detachment instead of raw `fork()`+`exec()`. This is cross-platform and doesn't carry the macOS `fork()` safety warnings. Document the spawn + detach approach.

---

### 2.2 `set -g remain-on-exit on` is a tmux server-level setting

Feature §2.3 says Tether injects `set -g remain-on-exit on`. The `-g` flag sets this **globally** for all sessions on the tmux server. If multiple blueprints share a tmux server, this is fine (it's the desired behavior). But if the user has their own tmux sessions, this modifies their global config.

**Recommendation:** Use per-session setting: `set remain-on-exit on` (without `-g`) or use `-t <target-session>` to scope it. Better: start a separate tmux server with `-L metamach-tether` to avoid polluting the user's tmux environment.

---

### 2.3 `Ctrl+C` resume mechanism is fundamentally flawed

As noted in Design-Review.md §3.4, but worth reiterating here as a spec-level issue:

Feature §2.4: "驱动对应的 Tether 窗格发送 `Ctrl+C` 释放挂起" — this assumes the suspended process is stuck on a foreground operation waiting for SIGINT. But `janus-sh` blocked the command BEFORE it reached the shell. There is nothing to `Ctrl+C` — the pane is idle, not stuck.

**Recommendation:** Rewrite the resume mechanism:
1. `SUSPENDED` means the Daemon refused to forward the command
2. The tmux pane is alive but idle (waiting for the next command)
3. Human attaches to the pane, manually fixes the issue (edits code, changes config)
4. Human signals completion via a special command (e.g., `metamach-resume` typed in the pane, or the Teams button callback)
5. Daemon receives the signal, transitions from `SUSPENDED` → `RUNNING`, and dispatches the **next** command (not a blind re-execute of the blocked one)

---

### 2.4 Offboard LLM call has no specification

Feature §2.5: "调用大模型将上述运行快照...压缩总结为高密度的 Markdown" — zero detail:

- Which model endpoint? Configured where?
- What is the prompt?
- What if the API key is invalid or rate-limited?
- What if the accumulated Step history exceeds the model's context window?
- Is this synchronous (blocking offboard for minutes) or async?
- What about air-gapped deployments with no LLM access?

**Recommendation:** Specify:
- Model configuration in `configs/offboard.toml` with endpoint, API key env var, model name
- Prompt template with max input tokens
- Fallback behavior: if LLM unavailable, write a raw JSON dump as `production_report.raw.json`
- Timeout: 120s, after which fallback is used
- Async execution: Offboard returns immediately, LLM summarization runs in background, report appears when ready

---

## 3. 🟡 Underspecified Areas

### 3.1 No error handling for Daemon database connection failure at startup

What happens if `janus-daemon` starts but Postgres is unreachable? The spec says Daemon "独占数据库连接池" but doesn't cover initialization failure. Does it:
- Retry with exponential backoff?
- Start in degraded mode with only `fallback.db`?
- Exit with an error?

**Recommendation:** Add startup behavior: retry PG connection 5 times with 2s intervals, then start in degraded mode (fallback.db only), log warning, and notify厂长. When PG becomes available, replay fallback.db.

---

### 3.2 No specification for agent prompt construction

The system injects OpenWiki knowledge and global rules into agent system prompts, but:
- How is the prompt assembled? (template? concatenation?)
- What's the priority: global_rules > blueprint-specific > task-specific?
- How is the 16KB budget applied to prompts (not just outputs)?
- What if the combined prompt exceeds the model's context limit?

**Recommendation:** Add a prompt assembly spec: `[global_rules.md] + [blueprint openwiki/] + [step prompt]`, with each section truncated to fit within model context window (model-specific, configured in agents.toml).

---

### 3.3 `blueprint_id` filtering is mentioned but never implemented in schema

Feature §3.1 shows `absurd_steps` with `task_id → absurd_tasks(id)`. But ARCH.md says `blueprint_id` is the partition key. Where does `blueprint_id` live?

- Is it in `absurd_tasks`? (likely — a task belongs to a blueprint)
- But `absurd_tasks` schema is never defined
- Queries like "get all steps for blueprint X" require JOIN through `absurd_tasks`

**Recommendation:** Complete the schema:

```sql
CREATE TABLE blueprints (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    config JSONB,           -- janus.toml contents
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE absurd_tasks (
    id SERIAL PRIMARY KEY,
    blueprint_id INTEGER REFERENCES blueprints(id) ON DELETE CASCADE,
    workflow_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,  -- PENDING | RUNNING | COMPLETED | SUSPENDED | FAILED
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);
```

---

### 3.4 `melt_blueprint_data` stored procedure behavior is vague

Feature §2.5: "彻底删除该蓝图对应的 `result_cache` JSON 大字段与终端 Stdout 日志，仅保留一行基础元数据统计". But:

- Is `result_cache` set to `NULL` or is the row deleted?
- "仅保留一行基础元数据统计" — one row per task? Per blueprint? What fields?
- If `result_cache` is NULLed but the row remains, `VACUUM FULL` won't reclaim TOAST space (NULLs don't free TOAST)

**Recommendation:** The procedure should DELETE rows entirely (not NULLify) for steps belonging to offboarded blueprints, and INSERT a single summary row into a separate `absurd_audit_log` table. This allows TOAST reclamation via normal autovacuum.

---

## 4. ⚪ Minor Issues

### 4.1 Inconsistent status values

Feature §3.1 schema says valid statuses: `STARTING | COMPLETED | SUSPENDED`. But other docs reference `RUNNING`, `FAILED`, `PENDING`. The schema comment should list the complete enum.

**Recommendation:** Full status enum: `PENDING → STARTING → RUNNING → COMPLETED | FAILED | SUSPENDED`.

### 4.2 Typos

- "物理物理故障" appears in Test-Spec.md (duplicate word) — not in this doc, but worth noting
- §4 fault matrix: "防爆降锁" → should probably be "防爆降级" (degradation, not lock reduction)

---

## Summary: Feature-Spec Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Define `configs/agents.toml` schema + Tool Guard rule engine |
| 2 | 🔴 | Define UDS response payload (Daemon → janus-sh) |
| 3 | 🔴 | Define Step execution model, lifecycle, and output capture |
| 4 | 🟠 | Replace `fork()`+`exec()` with `Command::spawn()` |
| 5 | 🟠 | Scope `remain-on-exit` per-session, use separate tmux server |
| 6 | 🟠 | Redesign HITL resume mechanism (no blind `Ctrl+C`) |
| 7 | 🟠 | Specify Offboard LLM integration (endpoint, prompt, fallback, timeout) |
| 8 | 🟡 | Add Daemon startup behavior for PG unavailable |
| 9 | 🟡 | Specify agent prompt assembly and context budget |
| 10 | 🟡 | Complete database schema (blueprints + absurd_tasks) |
| 11 | 🟡 | Clarify melt_blueprint_data DELETE vs NULL behavior |
| 12 | ⚪ | Normalize status enum across all docs |
