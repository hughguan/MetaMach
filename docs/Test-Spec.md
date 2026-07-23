# MetaMach 0.3.0 — Test Specification

> System-level quality assurance for the core scheduler, agent sandbox, durable workflows, degraded-mode fallback, and internalized tmux engine.

## 1. Testing Strategy

To guarantee MetaMach 0.3.0's high availability and strong anti-seismic, anti-blast capability, this test case design strictly follows four quality defense layers:

1. **Sandbox & Isolation Defense:** Validate 100% synchronous interception and redirection of high-risk commands and sensitive keys by `janush` proxy interception and Tool Guard.
2. **Durability & Self-Healing Defense:** Simulate extreme physical faults such as PG crashes and server power loss; validate `janus::tmux` (internalized) process preservation and native PG cold-start state reconciliation.
3. **Lifecycle & Anti-Bloat Defense:** Validate Offboard smelting, log 16KB truncation (Size Budget), and SQLite fallback degraded-mode ring buffer.
4. **tmux Internalization Defense:** Validate the internalized `janus::tmux` module's tmux session durability, cross-host SSH, and checkpoint-based restart.

### Severity Level Definitions

| Level | Meaning | Release Gate |
|-------|---------|-------------|
| **Blocker** | System crash, data loss, security bypass, or core workflow completely non-functional. | **Cannot ship.** Must be fixed before any release. |
| **Critical** | Major feature broken, significant degradation, or high-risk edge case with no workaround. | **Must fix before release.** Can proceed to RC only with documented risk acceptance. |
| **Major** | Feature partially impaired, non-critical error, or plausible edge case with available workaround. | **Should fix before release.** Can ship with known issue logged. |
| **Minor** | Cosmetic, UX paper-cut, or rare edge case with trivial impact. | **Fix when convenient.** No release gate impact. |

## 2. Test Cases

### Test Suite 2.1: Singleton Resident Control Hub (Janus Daemon & Twin-Client UI)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-01-01** | Daemon Startup | Validate control hub starts as singleton and cleans UDS socket on exit | Clean system, Janus not running | 1. Run `janus-daemon` to start background process. 2. Attempt to run `janus-daemon` again. | 1. `janus.sock` and `janus.pid` generated under `${HERDR_PLUGIN_STATE_DIR}`. 2. Second launch detects PID lock (stale detection: if PID is not alive, overwrite and start; if alive, report PID lock conflict and safely exit without breaking the original socket). | **Blocker** |
| **UTC-01-02** | Shadow Client Self-Healing | Validate `herdr-janus` performs lazy self-start on abnormal Daemon death | `janus-daemon` not started, `janus.sock` physically absent | Press `prefix+j` inside Herdr to wake Dispatcher. | 1. `herdr-janus` auto-launches `janus-daemon` in the background via `Command::spawn()` with detach. 2. Successfully renders the overlay Popup; connection status displays normal. | **Critical** |
| **UTC-01-03** | Popup Keyboard Lock | Validate UI popup fully captures keyboard input and supports Esc stack pop | Director has opened Dispatcher via `prefix+j` | 1. Use arrow keys to select Blueprint in popup. 2. Press `Down` 10 times (wrap test). 3. Press `Tab` 5 times (focus cycle). 4. Press `Esc`. | 1. Highlight toggles; focus never escapes to background tiled pane. 2. Highlight wraps to top. 3. Focus cycles within popup; no keystroke reaches background terminal. 4. Popup safely closes; Herdr TUI focus smoothly restores. | **Major** |
| **UTC-01-04** | Popup with Degraded Mode Banner | Validate Popup displays degraded-mode warning when PG is unreachable | PG stopped; Daemon running in degraded mode (SQLite fallback) | Press `prefix+j` inside Herdr to wake Dispatcher. | 1. Popup renders normally. 2. Status bar or banner displays "DB offline, running degraded" warning. 3. Core navigation still works (keyboard lock intact). | **Major** |

### Test Suite 2.2: In-Memory Agent Sandbox (janush & Tool Guard)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-02-01** | Shell Proxy Redirection | Validate tmux PTY `SHELL` is forcibly replaced on launch | `dev-flow` pipeline started | In tmux-launched tmux pane, execute `echo $SHELL`. | Terminal outputs the absolute path `${HERDR_PLUGIN_ROOT}/bin/janush`, not system `/bin/bash` or `/bin/sh`. | **Critical** |
| **UTC-02-02** | Sync Command Interception | Validate `janush` successfully intercepts and blocks unauthorized sensitive/dangerous commands | `gatemetric` `dev-flow` task started | In Agent pane: first create sentinel `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`, then force-execute blacklisted `rm -rf /tmp/metamach-test-guard-$(uuidgen)` (or system `esptool.py erase_flash` simulation). | 1. Terminal command synchronously suspended. 2. `janus-daemon` log triggers interception and returns rejection (Status: Blocked); original physical shell intact; sentinel file survives (not deleted). | **Blocker** |
| **UTC-02-03** | Financial Dry-Run Redirection | Validate high-risk operations forced-redirected to dry-run mode before approval | Financial-class product rebalance pipeline started | In unauthorized state, attempt to execute order command: `hi5bot --action execute`. | 1. `janush` captures the command in UDS sync. 2. Tool Guard forcibly rewrites argv to `hi5bot --action dry-run` and delivers to host shell. 3. Physical console outputs dry-run preview; no real transaction executed. | **Blocker** |
| **UTC-02-04** | UDS Protocol Robustness | Validate Daemon does not crash on malformed/unauthorized/oversized UDS payloads | Daemon running | 1. Send invalid JSON (missing field/broken UTF-8) to `janus.sock`. 2. Send 1000 requests in 1 second. 3. Send 64KB oversized payload. | 1. Invalid JSON: Daemon logs `WARN`, returns error response, does not crash. 2. High-frequency: rate-limited, no OOM. 3. Oversized: rejected (message too large). | **Critical** |
| **UTC-02-05** | UDS Fuzz Testing | Validate Daemon survives random/malicious byte sequences without crash | Daemon running | Send 10,000 fuzzed payloads (random bytes, edge-case UTF-8, boundary-length JSON) to `janus.sock`. | Daemon survives all 10,000 payloads; no crash, no OOM, no socket deadlock. Error responses are valid JSON with `verdict: "ERROR"`. | **Critical** |
| **UTC-02-06** | Fail-Closed 30s Timeout | Validate janush returns error (not hangs) when Daemon is unreachable for >30s | Daemon stopped; janush running | Execute `echo "test"` through janush. | Within 30s, janush returns an error message to the Agent; command is NOT executed (fail-closed). No indefinite hang. | **Blocker** |

### Test Suite 2.3: Durable Workflow State Machine & tmux

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-03-01** | Local Step Execution | Validate local tmux session launches and Step state transitions correctly | Daemon running, Postgres healthy | Dispatch `dev-flow` pipeline for `gatemetric` blueprint. | 1. `janus::tmux` creates tmux session. 2. `absurd_tasks` row transitions `PENDING → STARTING → RUNNING`. 3. `absurd_steps` row transitions through `PENDING → STARTING → RUNNING → COMPLETED`. | **Blocker** |
| **UTC-03-01b** | Dispatch -> Step Transitions | Validate the full step lifecycle from dispatch to completion | Daemon running, Postgres + tmux healthy | `Request::Dispatch` a 2-step workflow (Contract 3.11); poll `Request::Progress` + query the per-blueprint DB. | 1. `Response::Dispatch { task_id }` returns the absurd-minted id. 2. While a step runs, `Progress` reports `tmux_alive=true` for the task. 3. Both steps reach `COMPLETED` with `exit_code=0`. 4. `metamach_step_meta` transitions `STARTING -> RUNNING -> COMPLETED` with `started_at` set on `RUNNING`. 5. One absurd checkpoint per step (`c_<queue>`); the absurd task reaches `completed`. | **Critical** |

| **UTC-03-02** | Cross-Host Process Protection | Validate physical tmux Session remains intact on network disconnect/SSH restart | Task running on remote SSH compile host | 1. Programmatically drop packets to remote host (e.g., `iptables -A OUTPUT -d <remote> -j DROP`). 2. Wait 10s then restore and re-execute `janus tmux attach`. | 1. Remote host compile process not killed (`remain-on-exit` effective). 2. After re-attach, compile scene 100% restored; data lossless. | **Critical** |
| **UTC-03-03** | Cold-Start Self-Healing | Validate breakpoint resumption from the last `COMPLETED` checkpoint after a daemon crash | Daemon running, Postgres + tmux healthy | Dispatch a 2-step workflow (step 2 = `sleep`), kill `janus-daemon` mid-step-2, restart it (NOT `make bootstrap`), and let `coldstart::reconcile` resume. | 1. `coldstart::reconcile` spawns `workflow::run_workflow` for the non-terminal task. 2. The resumed run skips `COMPLETED` steps (re-reads the last checkpoint), re-runs the interrupted step in a fresh tmux session. 3. Stale tmux sessions from the crashed run are killed (no double-execution). 4. Both steps reach `COMPLETED`; the absurd task reaches `completed`. | **Critical** |
| **UTC-03-04** | Daemon Crash Recovery | Validate tmux scene survives Daemon crash during active Step; orphan step correctly handled | Step `RUNNING`; Daemon is parent of tmux session | `killall -9 janus-daemon`. | 1. tmux session survives (remain-on-exit). 2. `herdr-janus` lazy-restarts Daemon. 3. Daemon scans orphan steps; transitions orphan to `SUSPENDED`; notifies Director. | **Critical** |
| **UTC-03-05** | Concurrent Workflow Isolation | Validate multi-blueprint concurrent dispatch without cross-contamination | 2 blueprints both `ACTIVE` | Simultaneously dispatch `dev-flow` for both blueprints. | 1. 2 independent tmux sessions, 2 independent `absurd_tasks` records. 2. UDS requests correctly attributed by `task_id`; `result_cache` no cross-blueprint pollution. | **Critical** |

### Test Suite 2.4: Human-in-the-Loop Gate

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-03-06** | Optimistic Locking (target_sha) | Validate stale remote reports are discarded when HEAD advances during step execution | Blueprint repo with git history; step dispatched at SHA-A | 1. Dispatch a step at SHA-A. 2. While step is running, commit a new change so HEAD advances to SHA-B. 3. Remote report returns with `dispatch_sha` = SHA-A. | 1. Daemon detects SHA-A != SHA-B, discards the report. 2. Step marked `SUSPENDED` with `CONCURRENCY_RACE_ALERT`. 3. Auto-reschedule creates a new `absurd_tasks` row (new `task_id`) against SHA-B. 4. Old task audit trail preserved. | **Critical** |

| **UTC-04-01** | Non-Destructive Suspension | Validate physical scene preserved on compile break or privilege interception; process not killed | Compile script intentionally contains syntax error to trigger failure | Run compile pipeline; trigger failure. | 1. DB state locks to `SUSPENDED`. 2. tmux physical tmux Session suspended; error scene, memory variables, and console cache do not vanish. | **Critical** |
| **UTC-04-02** | Async Bidirectional Approval | Validate mobile (Telegram) receives high-density card and executes HITL Resume | Compliant external Telegram Webhook key configured | 1. Trigger task suspension. 2. On mobile Telegram, read error details and tap **`[Resume]`**. | 1. Telegram callback reaches Daemon polling port in seconds. 2. Daemon verifies Correlation ID signature; sends `metamach-resume` signal to pane; pipeline seamlessly hands off to next step (never blindly re-executes blocked command). | **Major** |
| **UTC-04-03** | Teams Notification & Resume | Validate Teams secondary adapter receives card and executes HITL Resume | Compliant external Teams Webhook key configured | 1. Trigger task suspension. 2. On Teams mobile, read error details and tap **`[Resume]`**. | 1. Teams callback reaches Daemon polling port. 2. Daemon verifies Correlation ID signature; pipeline resumes to next step. | **Major** |

### Test Suite 2.5: Federated Lifecycle Smelter (Onboard / Offboard & Auto-Pruning)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-05-01** | Size Budget Truncation | Validate large JSON cache does not bloat DB on Agent infinite log spam | Script running infinite `Hello World` log spam | Dispatch pipeline; capture task output; execute DB insert transaction. | 1. `result_cache` written to Postgres physical table strictly capped at **16 KiB** budget. 2. Truncation point auto-appended with `[MetaMach Log Budget Exceeded]` tag; DB protected from crash. | **Major** |
| **UTC-05-02** | Offboard Degradation Smelting | Validate product Offboard auto-smelts experience knowledge graph, wipes large JSON cache, and writes audit archive | Project `gatemetric` R&D complete; accumulated 200MB historical Steps logs | Run `janus offboard --blueprint gatemetric`. | 1. Daemon orchestrates DELETE + archive sequence: **100% physically DELETEs expired Step/task rows** (not NULL-ified). 2. `absurd_audit_log` has entries for each offboarded task (DELETE + archive, not DROP DATABASE). 3. `blueprints.status = 'OFFBOARDED'` is set. 4. `result_cache` is NULL'd (TOAST space reclaimable). 5. `production_report.md` auto-deposited under `gatemetric/openwiki/` containing pin fixes, error audits, etc. as few-shot evolution knowledge graph. | **Critical** |
| **UTC-05-03** | Git Experience Inheritance | Validate generated QA report auto-incrementally commits, forming immune self-healing | Smelter successfully outputs `production_report.md` | Execute Offboard settlement, then re-onboard to start next dev cycle. | 1. Report auto-executes local `git commit` and pushes to GitHub remote. 2. Next-gen Agent scans knowledge graph on entry; System Prompt auto-acquires avoidance antibodies; compilation passes on first attempt. | **Major** |
| **UTC-05-04** | Blueprint Onboard & Tenant Registration | Validate `janus onboard` registers tenant and makes product instantly dispatchable; operation is idempotent | Clean workshop with zero product lines; `blueprints/joyrobots/janus.toml` in place | 1. Execute `janus onboard --blueprint joyrobots`. 2. Immediately consecutively repeat the same command once. | 1. Onboard calls `CREATE DATABASE metamach_blueprint_joyrobots` on the native PG instance. 2. `blueprints` table gains one `status='ACTIVE'` row; Popup dispatch menu instantly shows `joyrobots`. 3. Repeated execution produces no second row (`ON CONFLICT` idempotent); menu has no duplicate entries. | **Blocker** |
| **UTC-05-04b** | Multi-DB Onboard Isolation | Validate Onboard creates independent per-blueprint database | Clean workshop; two blueprints ready | 1. Execute `janus onboard --blueprint joyrobots`. 2. Execute `janus onboard --blueprint gatemetric`. 3. Verify database topology. | 1. `CREATE DATABASE metamach_blueprint_joyrobots` executed. 2. `CREATE DATABASE metamach_blueprint_gatemetric` executed. 3. `psql -l` shows both databases. 4. `blueprints` table in global catalog `metamach_db` has two `ACTIVE` rows. 5. Blueprint name validation rejects names > 60 chars or non-alphanumeric. | **Blocker** |

| **UTC-05-05** | Re-Onboard & Experience Inheritance | Validate re-Onboard after Offboard recycles `production_report.md` as immune antibodies | `gatemetric` has been Offboarded; `production_report.md` contains marker string `PIN_CONFLICT_MARKER_21` | 1. Execute `janus onboard --blueprint gatemetric` to re-onboard. 2. Via Daemon debug endpoint, export that blueprint's Agent System Prompt. 3. Dispatch a step that historically triggered pin conflict. | 1. Onboard succeeds; blueprint status returns to `ACTIVE`. 2. Exported System Prompt's `## Previous Incidents` section must contain `PIN_CONFLICT_MARKER_21`. 3. Agent no longer attempts conflicting pin configuration; step passes on first attempt. | **Critical** |

### Test Suite 2.6: Workflow Progress Dashboard

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-06-01** | Progress Real-Time Refresh | Validate progress dashboard reflects real step progression within 2s | Multi-step pipeline (e.g., dev-flow scout→code→cross_compile) running | 1. `prefix+j` wake Popup; `Tab` to "Progress" view. 2. Observe step state changes as execution progresses. | 1. Dashboard refreshes at 1–2s cadence; step states transition sequentially `PENDING → RUNNING → COMPLETED` with latency ≤ 2s. 2. `current_step` and elapsed time update in real-time. | **Critical** |
| **UTC-06-02** | Suspend Highlight & Resume Entry | Validate `SUSPENDED` step highlights within 1s with recovery entry | Pipeline running; artificially trigger a safety fuse (e.g., privilege violation) | Observe progress dashboard corresponding row immediately after fuse triggers. | 1. That step highlights red as `SUSPENDED` within 1s. 2. Row end renders `[A]ttach Scene` / `[R]esume` shortcut entries; attach and recovery successfully triggered. | **Major** |
| **UTC-06-03** | `janus status` CLI Output | Validate non-TUI CLI snapshot consistent with dashboard data | At least one in-flight task exists | In SSH terminal, execute `janus status` and `janus status --json`. | 1. Plain-text output lists all in-flight tasks: blueprint/step/status/elapsed. 2. `--json` output conforms to Contract 3.3 payload structure; consistent with simultaneous dashboard display. | **Major** |
| **UTC-06-04** | Multi-Blueprint Isolation Display | Validate multi-blueprint parallel dashboard groups independently with zero cross-contamination; verify per-blueprint DB isolation | `joyrobots` and `gatemetric` both `ACTIVE`, each with in-flight tasks | Simultaneously dispatch pipelines for both blueprints; open progress dashboard. | 1. Dashboard groups two independent workflows by blueprint; step states do not cross-contaminate. 2. Each workflow's `task_id` is unique; `result_cache` has no cross-blueprint pollution. 3. Onboard for Blueprint A called `CREATE DATABASE metamach_blueprint_joyrobots`; Blueprint B got a separate `CREATE DATABASE metamach_blueprint_gatemetric`. 4. Offboard does NOT call `DROP DATABASE` (uses DELETE + absurd_audit_log archive). | **Major** |

### Test Suite 2.7: Performance Benchmarks

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-07-01** | Daemon Cold Start | Baseline Daemon startup time | Clean system, Postgres healthy | Measure `janus-daemon` launch to UDS ready. | < 2s. | **Major** |
| **UTC-07-02** | UDS Verdict Latency | Baseline command check round-trip | Daemon running, single request | Measure `janush` send → Daemon verdict response. | p50 < 10ms, p99 < 50ms. | **Major** |
| **UTC-07-03** | Popup Render (Warm) | Baseline Popup render time with warm Daemon | Daemon running | Measure `prefix+j` to interactive Popup. | < 100ms. | **Major** |
| **UTC-07-04** | Step History Query | Baseline DB query for large step history | 1000 steps in DB | Measure `progress` query latency. | < 500ms. | **Major** |

### Test Suite 2.8: Degraded Mode (SQLite Fallback Ring Buffer)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-08-01** | PG Crash → SQLite Fallback | Validate janush still works via SQLite ring buffer when PG is unreachable | PG stopped (`pg_ctl -D ~/.metamach/db/ stop`); Daemon running in degraded mode | 1. Dispatch a step through `janush`. 2. Verify step state is recorded. | 1. Step state changes are written to `fallback.db` (SQLite ring buffer under `${HERDR_PLUGIN_STATE_DIR}`). 2. Agent receives a warning that the system is in degraded mode. 3. Core command interception still works (Tool Guard operates in-memory). | **Critical** |
| **UTC-08-02** | PG Restored → Log Replay | Validate fallback.db events are merged into PG when PG recovers | PG was down; `fallback.db` has queued events; PG restarted | 1. Start PG (`pg_ctl -D ~/.metamach/db/ start`). 2. Daemon detects PG recovery. 3. Observe batch log replay. | 1. All events from `fallback.db` are replayed into `absurd_steps` in PG. 2. Post-replay, `fallback.db` events are marked as replayed (not duplicated). 3. Daemon transitions from degraded to normal mode. 4. Popup banner updates from "degraded" to "normal". | **Critical** |
| **UTC-08-03** | Ring Buffer Overflow | Validate oldest events are dropped when SQLite ring buffer exceeds capacity | PG down; Daemon in degraded mode; ring buffer near capacity | 1. Rapidly dispatch 1000+ steps while PG is down. 2. Verify ring buffer behavior. | 1. Ring buffer enforces bounded FIFO: oldest events are dropped when capacity is exceeded. 2. `fallback.db` file size stays within configured limit. 3. Daemon logs `WARN` when events are dropped. 4. Surviving events are still replayable after PG recovery. | **Major** |

### Test Suite 2.9: tmux Module (janus::tmux — Internalized)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-09-01** | DurableBackend::create_session | Validate tmux session creation with `-L metamach-tmux` socket name | Daemon running; tmux available | Dispatch a step that triggers `DurableBackend::create_session`. | 1. tmux session created with socket name `metamach-tmux`. 2. Session appears in `tmux -L metamach-tmux list-sessions`. 3. Session ID is a valid `TmuxSessionId`. | **Blocker** |
| **UTC-09-02** | Session Keep-Alive (SIGHUP Immunity) | Validate tmux session survives popup close / terminal close | Active tmux session via `janus::tmux` | 1. Close the Herdr popup window. 2. Kill the terminal emulator. 3. Check if tmux session survived. | 1. `tmux -L metamach-tmux list-sessions` still shows the session. 2. Session PID is still alive. 3. Re-attaching restores the scene 100%. | **Critical** |
| **UTC-09-03** | LifecycleService::restart_session | Validate session restart from AbsurdDB checkpoints after Daemon restart | Session was running; Daemon restarted; checkpoint in `absurd_steps` | 1. Restart `janus-daemon`. 2. Trigger `LifecycleService::restart_session` from checkpoint. | 1. New tmux session created with new UUID. 2. Session picks up from last `COMPLETED` step checkpoint. 3. Old session UUID is logged in audit trail. | **Critical** |
| **UTC-09-04** | Cross-Host SSH Session Creation | Validate SSH-based tmux session on remote host | Remote host reachable via SSH; SSH keys configured | Dispatch a cross-host step. | 1. `janus::tmux` creates tmux session on remote host via SSH. 2. `remain-on-exit` is enabled on remote session. 3. Remote session is visible via `ssh <remote> tmux -L metamach-tmux list-sessions`. | **Critical** |
| **UTC-09-05** | Session Status Polling | Validate `DurableBackend::inspect` returns accurate session status | Active tmux session | 1. Call `inspect(session_id)`. 2. Kill the session. 3. Call `inspect(session_id)` again. | 1. First call returns `Alive` with correct PID. 2. Second call returns `Dead`. 3. Polling latency < 5ms. | **Major** |
| **UTC-09-06** | Concurrent Session Isolation | Validate multiple sessions per blueprint are isolated | 2 blueprints `ACTIVE`; each has an in-flight tmux session | 1. Dispatch steps for both blueprints simultaneously. 2. Verify session isolation. | 1. Each blueprint gets its own tmux session. 2. Killing one session does not affect the other. 3. `inspect` correctly distinguishes sessions by `blueprint_id`. | **Critical** |

### Test Suite 2.10: HITL Gateway & Cognitive Provider (0.4.0)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-10-01** | Gateway Dispatch (Non-Blocking) | Validate `gateway::dispatch()` returns immediately without blocking the tmux control thread | Daemon running; gateway configured with LoggingSender | 1. Trigger a HITL suspension via `require_approval` command. 2. Measure time from `dispatch()` call to control-loop resume. | 1. `dispatch()` returns `Ok(())` within 5ms. 2. Control loop continues processing other requests immediately. 3. tmux session is not paused or frozen. | **Blocker** |
| **UTC-10-02** | Gateway HTTP Listener (Callback Ingress) | Validate the loopback HTTP listener accepts Teams callback POSTs | Gateway running on `127.0.0.1:8443`; pending verdict in map | 1. POST a valid callback to `/v1/runs/{run_id}/actions` with `action: "approve"`. 2. POST a duplicate callback for the same `run_id`. | 1. First callback returns `200 OK`; verdict thread receives `GatewayVerdict::Approve`. 2. Duplicate callback returns `409 Conflict`. | **Blocker** |
| **UTC-10-03** | Verdict Timeout (Fail-Closed) | Validate gateway returns timeout error when no callback arrives within deadline | Pending verdict with 5s timeout; no callback sent | 1. Call `await_verdict(correlation_id, Duration::from_secs(5))`. 2. Wait for timeout. | 1. Returns `Err(GatewayError::Timeout)` after 5s. 2. Verdict is BLOCK (fail-closed). 3. Pending-verdict map entry is cleaned up. | **Critical** |
| **UTC-10-04** | HMAC Authentication | Validate gateway rejects unsigned or incorrectly-signed callbacks | Gateway configured with HMAC secret; pending verdict | 1. POST a callback with no HMAC header. 2. POST a callback with wrong HMAC signature. 3. POST a correctly-signed callback. | 1. No-HMAC: returns `401 Unauthorized`. 2. Wrong HMAC: returns `401 Unauthorized`. 3. Correct HMAC: returns `200 OK`. | **Critical** |
| **UTC-10-05** | Teams Adaptive Card Format | Validate `TeamsSender` produces valid Adaptive Card JSON | HITL triggered; TeamsSender adapter active | 1. Inspect the JSON payload sent to the Teams webhook URL. | 1. JSON contains `type: "message"`, `attachments[0].contentType: "application/vnd.microsoft.card.adaptive"`. 2. Actions array has `Approve`, `Reject`, `Override` buttons. 3. Each action URL targets `/v1/runs/{run_id}/actions`. | **Major** |
| **UTC-10-06** | Cognitive Provider Validate (Advisory) | Validate `CognitiveProvider::validate_command` returns BLOCK recommendation without blocking tmux | CognitiveProvider configured for blueprint; `validate_command` returns `Some("pin conflict")` | 1. Dispatch a command that triggers the cognitive check. 2. Measure tmux session responsiveness during check. | 1. Tool Guard receives `Some("pin conflict")` and issues BLOCK. 2. Check completes within 2s (hard timeout). 3. tmux session is never paused during the check. | **Critical** |
| **UTC-10-07** | Cognitive Provider Timeout (Pass-Through) | Validate timeout on cognitive check does not block the verdict | CognitiveProvider configured; `validate_command` hangs | 1. Dispatch a command while the provider is hung. 2. Wait for the 2s timeout. | 1. Tool Guard proceeds with standard verdict (no cognitive input). 2. `WARN` log entry records the timeout. 3. No BLOCK is issued due to the timeout alone. | **Major** |
| **UTC-10-08** | CognitiveProvider extract_knowledge (Supplement) | Validate `extract_knowledge` output is appended to LLM smelt | Blueprint ready for Offboard; CognitiveProvider configured | 1. Execute `janus offboard --blueprint <name>`. 2. Inspect `production_report.md`. | 1. `production_report.md` contains both the LLM smelt output (Feature-Spec §2.5) and the cognitive provider's artifact. 2. Provider artifact is appended after the LLM sections. 3. If provider fails, LLM smelt still completes (provider is a supplement). | **Major** |
| **UTC-10-09** | WebhookPayload Enrichment | Validate enriched payload carries all Hermes fields | HITL triggered; gateway dispatch called | 1. Inspect the `WebhookPayload` passed to `gateway::dispatch()`. | 1. `correlation_id` is non-empty. 2. `blueprint`, `step`, `stdout_tail`, `expires_at` are all populated. 3. `stdout_tail == scene` (always equal at construction). | **Major** |
| **UTC-10-10** | expires_at Expiry (410 Gone) | Validate callback after expiry is rejected | Pending verdict with `expires_at` in the past | 1. POST a callback to `/v1/runs/{run_id}/actions` after `expires_at`. | 1. Returns `410 Gone`. 2. Verdict is not applied. 3. Pending-verdict map entry is cleaned up. | **Major** |

## 3. Testing Environment

### 3.1 Automated Test Dependencies

- **PostgreSQL 16+ (host-native):** Native PG instance managed by `janus-daemon`. No Docker required. Data stored at `~/.metamach/db/`.
- **Rust (v1.88+):** Local locked-version compile test binaries.
- **Tmux (v3.3+):** Underlying dependency for `janus::tmux` session immortality and integration tests.
- **SQLite (bundled via rusqlite):** Degraded-mode fallback ring buffer; no external dependency.
- **Local webhook receiver:** A simple HTTP server on localhost for notification callback testing (replaces ngrok/Cloudflare Tunnel for CI; actual Teams/TG integration tested in separate manual UAT phase, not automated CI).

### 3.2 Immutable/Mutable Directory Verification

Before testing begins, verify the following physical paths have no state cross-contamination:

- **Immutable ROOT:** Installation path; holds static config and OpenWiki `global_rules.md`.
    ```bash
    find ${HERDR_PLUGIN_ROOT} -name "*.db" -o -name ".env"
    # Must return empty (fallback.db lives in Mutable State, not ROOT)
    ```
- **Mutable Config:**
    ```bash
    ls -l ${HERDR_PLUGIN_CONFIG_DIR}/agents.toml
    # Must show symlink
    ```
- **Mutable State:**
    ```bash
    ls -l ${HERDR_PLUGIN_STATE_DIR}/janus.sock
    # Must exist when Daemon is running
    ls -l ${HERDR_PLUGIN_STATE_DIR}/fallback.db
    # May exist if Daemon ever entered degraded mode
    ```
- **Native PG Data:**
    ```bash
    ls -l ~/.metamach/db/PG_VERSION
    # Must exist after `make db-init`
    ```

### 3.3 Test Gating & Dependencies

| Test | Depends On | Notes |
|------|-----------|-------|
| UTC-03-02 (Cross-Host Process Protection) | UTC-09-xx (tmux Module) | Blocked until janus::tmux tests pass |
| UTC-09-04 (Cross-Host SSH) | SSH credentials | Use `#[ignore = "requires SSH credentials"]` in CI |
| UTC-07-xx (Benchmarks) | criterion harness | Deferred to P2; no 0.3.0 gate impact |

After executing all UAT physical reconciliation and stress fault tests per this specification, your **MetaMach 0.3.0** will possess truly "physically anti-blast, process-immortal, security-compliant, self-evolving, degraded-mode resilient" enterprise-grade quality endorsement.
