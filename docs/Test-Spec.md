# MetaMach 1.0 — Test Specification

> System-level quality assurance for the core scheduler, agent sandbox, and durable workflows.

## 1. Testing Strategy

To guarantee MetaMach 1.0's high availability and strong anti-seismic, anti-blast capability, this test case design strictly follows three quality defense layers:

1. **Sandbox & Isolation Defense:** Validate 100% synchronous interception and redirection of high-risk commands and sensitive keys by `janus-sh` proxy interception and Tool Guard.
2. **Durability & Self-Healing Defense:** Simulate extreme physical faults such as network timeouts and server power loss; validate `Tether (remain-on-exit)` process preservation and Absurd Postgres cold-start state reconciliation.
3. **Lifecycle & Anti-Bloat Defense:** Validate Offboard smelting, log 16KB truncation (Size Budget), and database auto-degradation.

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

### Test Suite 2.2: In-Memory Agent Sandbox (janus-sh & Tool Guard)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-02-01** | Shell Proxy Redirection | Validate Tether PTY `SHELL` is forcibly replaced on launch | `dev-flow` pipeline started | In Tether-launched tmux pane, execute `echo $SHELL`. | Terminal outputs the absolute path `${HERDR_PLUGIN_ROOT}/bin/janus-sh`, not system `/bin/bash` or `/bin/sh`. | **Critical** |
| **UTC-02-02** | Sync Command Interception | Validate `janus-sh` successfully intercepts and blocks unauthorized sensitive/dangerous commands | `gatemetric` `dev-flow` task started | In Agent pane: first create sentinel `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`, then force-execute blacklisted `rm -rf /tmp/metamach-test-guard-$(uuidgen)` (or system `esptool.py erase_flash` simulation). | 1. Terminal command synchronously suspended. 2. `janus-daemon` log triggers interception and returns rejection (Status: Blocked); original physical shell intact; sentinel file survives (not deleted). | **Blocker** |
| **UTC-02-03** | Financial Dry-Run Redirection | Validate high-risk operations forced-redirected to dry-run mode before approval | Financial-class product rebalance pipeline started | In unauthorized state, attempt to execute order command: `hi5bot --action execute`. | 1. `janus-sh` captures the command in UDS sync. 2. Tool Guard forcibly rewrites argv to `hi5bot --action dry-run` and delivers to host shell. 3. Physical console only generates reconciliation diff; no actual fund transfer occurs. | **Blocker** |
| **UTC-02-04** | UDS Protocol Robustness | Validate Daemon does not crash on malformed/unauthorized/oversized UDS payloads | Daemon running | 1. Send invalid JSON (missing field/broken UTF-8) to `janus.sock`. 2. Send 1000 requests in 1 second. 3. Send 64KB oversized payload. | 1. Invalid JSON: Daemon logs `WARN`, returns error response, does not crash. 2. High-frequency: rate-limited, no OOM. 3. Oversized: rejected (message too large). | **Critical** |
| **UTC-02-05** | Tool Guard Edge Cases | Validate allowlist pass-through, partial match, command chaining, subshell escape, env var injection | All Agent roles configured per `agents.toml` | 1. Scout runs `ls -la` (allowlist). 2. Coder runs `rm -rf /tmp/something` vs blacklisted `rm -rf /`. 3. Coder runs `rm -rf / && echo done` (chain). 4. Coder runs `bash -c "rm -rf /"` (subshell). 5. Coder runs `RM_TARGET=/ && rm -rf $RM_TARGET` (env injection). | 1. ALLOWED, executes normally. 2. `/tmp/something` deletion ALLOWED; bare `/` BLOCKED (partial match). 3. Entire chain BLOCKED. 4. Inner command detected and BLOCKED. 5. Command BLOCKED (guard detects expanded target). | **Critical** |

### Test Suite 2.3: Multi-Agent Cross-Host Durable Workflows

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-03-01** | Absurd Transaction Idempotency | Validate workflow Step produces no dirty data on multiple retries | DB connected | Dispatch `gatemetric` `make compile` station 3 consecutive times. | 1. Postgres physical table retains only 1 Task record with latest Step state. 2. `result_cache` JSON overwritten with latest success state; no redundant records. | **Critical** |
| **UTC-03-02** | Cross-Host Process Protection | Validate physical tmux Session remains intact on network disconnect/SSH restart | Task running on remote SSH compile host | 1. Programmatically drop packets to remote host (e.g., `iptables -A OUTPUT -d <remote> -j DROP`). 2. Wait 10s then restore and re-execute `herdr-tether attach`. | 1. Remote host compile process not killed (`remain-on-exit` effective). 2. After re-attach, compile scene 100% restored; data lossless. | **Critical** |
| **UTC-03-03** | Cold-Start Self-Healing | Simulate physical power loss; validate breakpoint resumption from last Step | Local host running heavy compile; task state `RUNNING` | 1. Force-kill `postgres` container and `janus-daemon`. 2. Restart PG container (`docker compose up -d`) and directly restart `janus-daemon` to trigger cold-start self-healing (**do NOT run `make bootstrap`**—full recompilation masks the real cold-start code path). | 1. Daemon rejects `tmux-resurrect`. 2. Reads last `COMPLETED` Step Checkpoint from Absurd Postgres; assigns new UUID; resumes at physical breakpoint. | **Critical** |
| **UTC-03-04** | Daemon Crash Recovery | Validate tmux scene survives Daemon crash during active Step; orphan step correctly handled | Step `RUNNING`; Daemon is parent of tmux session | `killall -9 janus-daemon`. | 1. tmux session survives (remain-on-exit). 2. `herdr-janus` lazy-restarts Daemon. 3. Daemon scans orphan steps; transitions orphan to `SUSPENDED`; notifies Director. | **Critical** |
| **UTC-03-05** | Concurrent Workflow Isolation | Validate multi-blueprint concurrent dispatch without cross-contamination | 2 blueprints both `ACTIVE` | Simultaneously dispatch `dev-flow` for both blueprints. | 1. 2 independent tmux sessions, 2 independent `absurd_tasks` records. 2. UDS requests correctly attributed by `task_id`; `result_cache` no cross-blueprint pollution. | **Critical** |

### Test Suite 2.4: Human-in-the-Loop Gate

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-04-01** | Non-Destructive Suspension | Validate physical scene preserved on compile break or privilege interception; process not killed | Compile script intentionally contains syntax error to trigger failure | Run compile pipeline; trigger failure. | 1. DB state locks to `SUSPENDED`. 2. Tether physical tmux Session suspended; error scene, memory variables, and console cache do not vanish. | **Critical** |
| **UTC-04-02** | Async Bidirectional Approval | Validate mobile (Telegram) receives high-density card and executes HITL Resume | Compliant external Telegram Webhook key configured | 1. Trigger task suspension. 2. On mobile Telegram, read error details and tap **`[Resume]`**. | 1. Telegram callback reaches Daemon polling port in seconds. 2. Daemon verifies Correlation ID signature; sends `metamach-resume` signal to pane; pipeline seamlessly hands off to next step (never blindly re-executes blocked command). | **Major** |
| **UTC-04-03** | Teams Notification & Resume | Validate Teams secondary adapter receives card and executes HITL Resume | Compliant external Teams Webhook key configured | 1. Trigger task suspension. 2. On Teams mobile, read error details and tap **`[Resume]`**. | 1. Teams callback reaches Daemon polling port. 2. Daemon verifies Correlation ID signature; pipeline resumes to next step. | **Major** |

### Test Suite 2.5: Federated Lifecycle Smelter (Onboard / Offboard & Auto-Pruning)

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-05-01** | Size Budget Truncation | Validate large JSON cache does not bloat DB on Agent infinite log spam | Script running infinite `Hello World` log spam | Dispatch pipeline; capture task output; execute DB insert transaction. | 1. `result_cache` written to Postgres physical table strictly capped at **16 KiB** budget. 2. Truncation point auto-appended with `[MetaMach Log Budget Exceeded]` tag; DB protected from crash. | **Major** |
| **UTC-05-02** | Offboard Degradation Smelting | Validate product Offboard auto-smelts experience knowledge graph and wipes large JSON cache | Project `gatemetric` R&D complete; accumulated 200MB historical Steps logs | Run `janus offboard --blueprint gatemetric`. | 1. DB auto-triggers `melt_blueprint_data`: **100% physically DELETEs expired Step JSON rows** (not NULL-ified). 2. `production_report.md` auto-deposited under `gatemetric/openwiki/` containing pin fixes, error audits, etc. as few-shot evolution knowledge graph. | **Critical** |
| **UTC-05-03** | Git Experience Inheritance | Validate generated QA report auto-incrementally commits, forming immune self-healing | Smelter successfully outputs `production_report.md` | Execute Offboard settlement, then re-onboard to start next dev cycle. | 1. Report auto-executes local `git commit` and pushes to GitHub remote. 2. Next-gen Agent scans knowledge graph on entry; System Prompt auto-acquires avoidance antibodies; compilation passes on first attempt. | **Major** |
| **UTC-05-04** | Blueprint Onboard & Tenant Registration | Validate `janus onboard` registers tenant and makes product instantly dispatchable; operation is idempotent | Clean workshop with zero product lines; `blueprints/joyrobots/janus.toml` in place | 1. Execute `janus onboard --blueprint joyrobots`. 2. Immediately consecutively repeat the same command once. | 1. `blueprints` table gains one `status='ACTIVE'` row; Popup dispatch menu instantly shows `joyrobots`. 2. Repeated execution produces no second row (`ON CONFLICT` idempotent); menu has no duplicate entries. | **Blocker** |
| **UTC-05-05** | Re-Onboard & Experience Inheritance | Validate re-Onboard after Offboard recycles `production_report.md` as immune antibodies | `gatemetric` has been Offboarded; `production_report.md` contains marker string `PIN_CONFLICT_MARKER_21` | 1. Execute `janus onboard --blueprint gatemetric` to re-onboard. 2. Via Daemon debug endpoint, export that blueprint's Agent System Prompt. 3. Dispatch a step that historically triggered pin conflict. | 1. Onboard succeeds; blueprint status returns to `ACTIVE`. 2. Exported System Prompt's `## Previous Incidents` section must contain `PIN_CONFLICT_MARKER_21`. 3. Agent no longer attempts conflicting pin configuration; step passes on first attempt. | **Critical** |

### Test Suite 2.6: Workflow Progress Dashboard

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-06-01** | Progress Real-Time Refresh | Validate progress dashboard reflects real step progression within 2s | Multi-step pipeline (e.g., dev-flow scout→code→cross_compile) running | 1. `prefix+j` wake Popup; `Tab` to "Progress" view. 2. Observe step state changes as execution progresses. | 1. Dashboard refreshes at 1–2s cadence; step states transition sequentially `PENDING → RUNNING → COMPLETED` with latency ≤ 2s. 2. `current_step` and elapsed time update in real-time. | **Critical** |
| **UTC-06-02** | Suspend Highlight & Resume Entry | Validate `SUSPENDED` step highlights within 1s with recovery entry | Pipeline running; artificially trigger a safety fuse (e.g., privilege violation) | Observe progress dashboard corresponding row immediately after fuse triggers. | 1. That step highlights red as `SUSPENDED` within 1s. 2. Row end renders `[A]ttach Scene` / `[R]esume` shortcut entries; attach and recovery successfully triggered. | **Major** |
| **UTC-06-03** | `janus status` CLI Output | Validate non-TUI CLI snapshot consistent with dashboard data | At least one in-flight task exists | In SSH terminal, execute `janus status` and `janus status --json`. | 1. Plain-text output lists all in-flight tasks: blueprint/step/status/elapsed. 2. `--json` output conforms to Contract 3.3 payload structure; consistent with simultaneous dashboard display. | **Major** |
| **UTC-06-04** | Multi-Blueprint Isolation Display | Validate multi-blueprint parallel dashboard groups independently with zero cross-contamination | `joyrobots` and `gatemetric` both `ACTIVE`, each with in-flight tasks | Simultaneously dispatch pipelines for both blueprints; open progress dashboard. | 1. Dashboard groups two independent workflows by blueprint; step states do not cross-contaminate. 2. Each workflow's `task_id` is unique; `result_cache` has no cross-blueprint pollution. | **Major** |

### Test Suite 2.7: Performance Benchmarks

| ID | Module | Purpose | Precondition | Steps | Expected | Severity |
|----|--------|--------|-------------|-------|----------|----------|
| **UTC-07-01** | Daemon Cold Start | Baseline Daemon startup time | Clean system, Postgres healthy | Measure `janus-daemon` launch to UDS ready. | < 2s. | **Major** |
| **UTC-07-02** | UDS Verdict Latency | Baseline command check round-trip | Daemon running, single request | Measure `janus-sh` send → Daemon verdict response. | p50 < 10ms, p99 < 50ms. | **Major** |
| **UTC-07-03** | Popup Render (Warm) | Baseline Popup render time with warm Daemon | Daemon running | Measure `prefix+j` to interactive Popup. | < 100ms. | **Major** |
| **UTC-07-04** | Step History Query | Baseline DB query for large step history | 1000 steps in DB | Measure `progress` query latency. | < 500ms. | **Major** |

## 3. Testing Environment

### 3.1 Automated Test Dependencies

- **Docker & Compose (v2.20+):** One-click start Absurd Postgres test container.
- **Rust (v1.88+):** Local locked-version compile test binaries.
- **Tmux (v3.2+):** Underlying dependency for Tether session immortality.
- **Local webhook receiver:** A simple HTTP server on localhost for notification callback testing (replaces ngrok/Cloudflare Tunnel for CI; actual Teams/TG integration tested in separate manual UAT phase, not automated CI).

### 3.2 Immutable/Mutable Directory Verification

Before testing begins, verify the following three physical paths have no state cross-contamination:

- **Immutable ROOT:** Installation path; holds static config and OpenWiki `global_rules.md`.
    ```bash
    find ${HERDR_PLUGIN_ROOT} -name "*.db" -o -name ".env"
    # Must return empty
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
    ```

After executing all UAT physical reconciliation and stress fault tests per this specification, your **MetaMach 1.0** will possess truly "physically anti-blast, process-immortal, security-compliant, self-evolving" enterprise-grade quality endorsement.
