# MetaMach 0.3.0 — Review & Audit Standards

> Architecture walkthrough and audit criteria centered on design philosophy, ultimate security, system stability, and multi-dimensional disaster recovery.

> **Safety Review Red Line:** All security review items in this specification (especially the REV-SEC-* series) **must be executed in an isolated container or dedicated test VM; strictly prohibited on production machines or hosts containing personal data.** All "destructive command" tests use safe equivalents such as `/tmp/metamach-review-*` sentinel directories; real system-level deletion is never executed.

## 1. Philosophy Alignment

Before MetaMach 0.3.0 formally connects to the grid, the Architect and Factory Director must jointly conduct an **Architecture-Level Design Review**. This specification provides a set of rigorous, quantified audit criteria to prove that the system design can 100% defend the following four design pillars even under harsh physical environments and black-box AI vulnerabilities:

```
+-----------------------------------------------------------------------------------------+
|                           METAMACH 0.3.0 REVIEW PILLARS                                  |
+-----------------------------------------------------------------------------------------+
|  1. Security    → Validate physical janush interception & /dev/shm RAM disk key protection |
|  2. Stability   → Validate remain-on-exit process guardianship & 16KB anti-bloat defense    |
|  3. Disaster    → Validate Multi-DB isolation, zero-state cold start & Fallback cache        |
|  4. Evolution   → Validate Offboard DELETE+audit archive & OpenWiki genetic inheritance      |
+-----------------------------------------------------------------------------------------+
```

## 2. Core Review Domains

### Domain 1: Workshop Security (Security Invariants Review)

This review aims to prove: **No matter how the AI hallucinates or attempts malicious privilege escalation, it absolutely cannot penetrate the sandbox to steal physical credentials, and high-risk commands cannot reach the real system shell.**

- **Metric 1.1: `/dev/shm` RAM Disk Physical Isolation Audit**
    - _Requirement:_ Validate the physical security of sensitive keys (e.g., Questrade API credentials, SSH keys) at the three stages of landing, reading, and destruction.
    - _Pass Criteria:_
        1. No plaintext decrypted `.decrypted` files are permitted on the host disk.
        2. `decrypt_secrets.sh` mount path must be confined to the `/dev/shm/` in-memory filesystem.
        3. Audit file permissions via `ls -l`; must be `0600` (only the `janus-daemon` runtime user can read/write); strictly prohibit `others` access.
        4. After `janus-daemon` process termination or task completion, the system must atomically execute `shm_unlink` or directly clear that memory block.

- **Metric 1.2: `janush` Proxy Interception & Tool Guard Reconciliation Audit**
    - _Requirement:_ Prove that interception does not rely on LLM self-restraint; interception occurs synchronously before the real physical shell.
    - _Pass Criteria:_
        1. Inspect every PTY pane launched by tmux; its `SHELL` environment variable must be forced to the absolute path `${HERDR_PLUGIN_ROOT}/bin/janush`.
        2. Audit the UDS synchronous blocking call mechanism between `janush` and `janus.sock`. When sending an interception command, `janush` must remain in a Blocked suspended state, never preemptively forking a child process.
        3. Audit the Tool Guard decision matrix: high-risk commands (e.g., unauthorized network egress, physical flash erasure, non-Dry-Run trading order execution) must 100% trigger rewrite redirection or direct error blocking before a **Correlation ID digital signature** approved by Teams/TUI is detected.

- **Metric 1.3: Fail-Closed 30s UDS Timeout Audit**
    - _Requirement:_ Prove that when the Daemon is unreachable, `janush` never lets commands through and never hangs indefinitely.
    - _Pass Criteria:_
        1. Stop `janus-daemon`; execute any command through `janush`.
        2. Within 30s, `janush` must return an error to the Agent.
        3. The command must NOT be executed; the Agent's shell must not hang.
        4. Restart Daemon; verify `janush` resumes normal operation without manual intervention.

### Domain 2: Physical System Stability (Stability & Budget Review)

This review aims to prove: **Under extreme high-frequency loads such as Agent log infinite-loop spam or database connection pool disconnection, the system will not suffer cascading physical crashes.**

- **Metric 2.1: PTY Process Anti-Deadlock & `remain-on-exit` Review**
    - _Requirement:_ Prove that on task interruption, error, or network drop, the scene is absolutely preserved and processes are not killed.
    - _Pass Criteria:_
        1. Audit `configs/tmux.conf` and `janus::tmux` invocation init parameters; must 100% configure per-session `remain-on-exit on` inside an independent tmux server (`tmux -L metamach-tmux`), with no `-g` global flag.
        2. Artificially send `kill -9 <agent_process>` to an Agent pane; that tmux window must remain in `[Exited]` suspended state, the window does not auto-close, and the console history cache (stdout) is fully retrievable via `janus tmux attach`.

- **Metric 2.2: 16 KiB Storage Volume Budget Anti-Bloat Audit**
    - _Requirement:_ Verify that the system can forcibly cut off infinite-loop log spam before database insertion, preventing physical disk bloat leading to OOM crash.
    - _Pass Criteria:_
        1. Run a script that outputs infinite `Hello World` spam; the final `result_cache` JSON field written to Postgres must be strictly capped at **16 KiB**.
        2. The truncation point must carry the explicit marker `[MetaMach Log Budget Exceeded]`.
        3. The database must not crash or trigger abnormal connection pool exhaustion due to dirty oversized data.

- **Metric 2.3: tmux Internalization (janus::tmux) Audit**
    - _Requirement:_ Prove that the internalized `janus::tmux` module independently manages tmux sessions without the deprecated external `herdr-tether` plugin.
    - _Pass Criteria:_
        1. `herdr-tether` binary is not present in `bin/` or `$PATH`.
        2. `janus tmux open|attach|list` CLI subcommands function correctly.
        3. tmux sessions use `-L metamach-tmux` socket name.
        4. Session keep-alive survives popup close and terminal close (SIGHUP immunity).
        5. `LifecycleService::restart_session` correctly restores sessions from AbsurdDB checkpoints.

- **Metric 2.4: Optimistic Locking (target_sha) Audit**
    - _Requirement:_ Prove that stale remote reports cannot overwrite locally-evolved code state.
    - _Pass Criteria:_
        1. Dispatch a step with `HEAD` at SHA-A; while the step is running, commit a new change so `HEAD` advances to SHA-B.
        2. The remote report returns with `dispatch_sha = SHA-A`.
        3. Daemon detects `SHA-A != SHA-B`, discards the report, marks step `SUSPENDED`, and emits `CONCURRENCY_RACE_ALERT`.
        4. Auto-reschedule creates a new `absurd_tasks` row (new `task_id`) against `HEAD` at SHA-B.

### Domain 3: System Disaster Recovery (Extreme Disaster & Multi-Tenant Review)

This review aims to prove: **Even under catastrophic data center destruction, the system can achieve zero-state cold-start self-healing, and multi-blueprint data is absolutely isolated.**

- **Metric 3.1: Cold-Start Self-Healing Audit**
    - _Requirement:_ Prove that the Daemon can correctly handle the disposal of in-flight tasks during power loss or full cluster restart, and the system will not blindly re-execute failures.
    - _Pass Criteria:_
        1. Simulate sudden power loss by stopping native PG (`pg_ctl -D ~/.metamach/db/ stop`) and killing `janus-daemon`.
        2. Restart PG (`pg_ctl -D ~/.metamach/db/ start`) and `janus-daemon`.
        3. Within 0.5s of startup, Daemon must perform typed disposition of pre-outage tasks: `RUNNING` tasks pick up from the last `COMPLETED` Step Checkpoint; `SUSPENDED` tasks remain suspended and trigger a notification.
        4. `~/.metamach/db/` path exists with PG data intact; no `tmux-resurrect` dependency.

- **Metric 3.2: Multi-DB Tenant Isolation Audit**
    - _Requirement:_ Prove that blueprint data is physically isolated at the database level with no cross-blueprint contamination.
    - _Pass Criteria:_
        1. Onboard Blueprint A creates `CREATE DATABASE metamach_blueprint_a`; Blueprint B creates `CREATE DATABASE metamach_blueprint_b`.
        2. Tasks and steps for Blueprint A are written to `metamach_blueprint_a` only; Blueprint B to `metamach_blueprint_b` only.
        3. `blueprints` metadata resides in the global catalog `metamach_db`.
        4. Offboard does NOT call `DROP DATABASE`; per-blueprint databases are retained for forensic review.

- **Metric 3.3: SQLite Fallback & Log Replay Audit**
    - _Requirement:_ Prove that during PG unreachability, the system degrades gracefully to SQLite and replays events on PG recovery.
    - _Pass Criteria:_
        1. Stop PG; Daemon enters degraded mode; step state changes are written to `fallback.db`.
        2. Restart PG; Daemon detects recovery and triggers batch Log Replay.
        3. All `fallback_events` are merged into the correct per-blueprint database's `absurd_steps`.
        4. Ring buffer enforces bounded FIFO; oldest events are dropped when capacity is exceeded.
        5. Daemon transitions from degraded to normal mode; Popup banner updates accordingly.

### Domain 4: Lifecycle Smelting & Self-Evolution (Trace Purge & Audit Archive Review)

This review aims to prove: **The Offboard process provides a complete audit trail, correctly purges operational data while preserving forensic traceability, and feeds experience back into the knowledge graph.**

- **Metric 4.1: Trace Purge & Audit Archive Review**
    - _Requirement:_ Prove that Offboard correctly DELETEs operational data, writes full audit traces, and retains the per-blueprint database.
    - _Pass Criteria:_
        1. Execute `janus offboard --blueprint <name>`.
        2. In the blueprint's dedicated database, `absurd_steps` and `absurd_tasks` rows are fully DELETEd (not NULL-ified, not VACUUM-dependent).
        3. In the global catalog `metamach_db`, `absurd_audit_log` has one row per offboarded task with full trace metadata (task_id, blueprint_name, workflow_name, step_count, elapsed_seconds, offboarded_at).
        4. `blueprints.status = 'OFFBOARDED'` is set; `result_cache` is NULL'd (TOAST space reclaimable).
        5. The per-blueprint database is NOT dropped — schema and audit history survive for forensic review.
        6. `production_report.md` is deposited under `blueprints/<name>/openwiki/`.

- **Metric 4.2: OpenWiki Knowledge Inheritance & Evolution Audit**
    - _Requirement:_ Prove that the Offboard-computed production report is correctly injected into the knowledge graph and that the next-generation Agent can inherit immune antibodies.
    - _Pass Criteria:_
        1. The generated `production_report.md` must contain at minimum the four structured blocks: **[Compile Error History]**, **[Pin Conflict Details]**, **[Tool Guard Synchronous Interception Logs]**, and **[Successful Patches Applied]**.
        2. On re-Onboarding the project, OpenWiki must prioritize indexing and merging that QA whitepaper.
        3. When the next-generation Agent enters and scans that knowledge graph, its System Prompt must successfully carry that immune information, and in subsequent code generation proactively avoid pin conflicts, passing compilation on the first attempt.

- **Metric 4.3: Blueprint Onboard & Tenant Registration Audit**
    - _Requirement:_ Prove that `janus onboard` can safely and idempotently convert a blueprint directory into an `ACTIVE` dispatchable product line and correctly recycle historical experience.
    - _Pass Criteria:_
        1. After executing `janus onboard --blueprint <name>`, the `blueprints` table must have exactly one `ACTIVE` row for that blueprint, and the Popup dispatch menu must instantly become visible.
        2. Onboard calls `CREATE DATABASE metamach_blueprint_<name>` for the new blueprint; on `42P04` (duplicate), treats as idempotent.
        3. Consecutively repeated Onboard must not produce duplicate rows (`ON CONFLICT` idempotent) and must not corrupt existing Task/Step data.
        4. Re-onboarding an already `OFFBOARDED` blueprint must return it to `ACTIVE` status, and if a prior `production_report.md` exists, its critical failure patterns must appear as `## Previous Incidents` few-shot in the next-generation Agent's System Prompt.

## 3. Review Sign-Off Sheet

The Factory Director and Architect must physically verify and sign off each item in the following table during reconciliation:

| Review ID | Audit Item | Verification Method | Status | Risk |
|---|---|---|---|---|
| **REV-SEC-01** | `/dev/shm` Permission Isolation | Run `stat /dev/shm/*.decrypted` on the host; verify permissions `0600` and owner is the daemon user. | `[ ]` Verified | **Critical (Red)** |
| **REV-SEC-02** | `janush` Privilege Blocking | Create sentinel `mkdir -p /tmp/metamach-review-$(uuidgen) && echo s > /tmp/metamach-review-$(uuidgen)/sentinel`; via Agent pane, force-execute blacklisted `rm -rf /tmp/metamach-review-*`; verify Daemon UDS synchronously intercepts and locks, and sentinel file survives. | `[ ]` Verified | **Critical (Red)** |
| **REV-SEC-03** | UDS Channel Integrity | `stat janus.sock` verify permissions `0600`; connection attempt as different user must be rejected; verify Daemon validates peer PID/UID. | `[ ]` Verified | **High (Orange)** |
| **REV-SEC-04** | Post-Crash Key Hygiene | Load keys, then `kill -9 janus-daemon`; audit `/dev/shm/*.decrypted` cleanup or tmpfiles rule; after reboot, `/dev/shm` is empty. | `[ ]` Verified | **High (Orange)** |
| **REV-SEC-05** | Network Egress Control | Scout-level Agent attempts `curl`/`python3 urllib`/`/dev/tcp` egress — all blocked; document control layer. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-SEC-06** | Fail-Closed 30s UDS Timeout | Stop `janus-daemon`; execute any command through `janush`; verify error returned within 30s, command NOT executed, no indefinite hang. | `[ ]` Verified | **Critical (Red)** |
| **REV-STB-01** | 16KB Size Budget | Run `cat /dev/urandom` spam; verify JSON written to Postgres is forcibly truncated with Budget marker. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-STB-02** | tmux Internalization (janus::tmux) | Verify `herdr-tether` binary absent; `janus tmux open|attach|list` functional; tmux uses `-L metamach-tmux`; session survives popup close. | `[ ]` Verified | **Critical (Red)** |
| **REV-STB-03** | Load & Resource Stress | 5 concurrent `dev-flow` complete without deadlock; 24h Daemon memory < 256MB; UDS verdict p99 < 10ms. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-STB-04** | Optimistic Locking (target_sha) | Dispatch step at SHA-A; commit new change → SHA-B; verify stale report discarded, `CONCURRENCY_RACE_ALERT` emitted, auto-reschedule creates new task_id. | `[ ]` Verified | **High (Orange)** |
| **REV-DIS-01** | Cold-Start Zero-State Self-Healing | `killall -9 janus-daemon` and kill only MetaMach tmux sessions; stop PG via `pg_ctl -D ~/.metamach/db/ stop` to simulate power loss; restart PG and Daemon; verify smooth breakpoint resumption. | `[ ]` Verified | **High (Orange)** |
| **REV-DIS-02** | Multi-DB Tenant Isolation | Onboard two blueprints; verify `CREATE DATABASE metamach_blueprint_<name>` for each; verify task/step data in separate databases; Offboard does NOT call DROP DATABASE. | `[ ]` Verified | **High (Orange)** |
| **REV-DIS-03** | SQLite Fallback & Log Replay | Stop PG; verify degraded mode writes to `fallback.db`; restart PG; verify Log Replay merges all events; verify ring buffer FIFO eviction under overflow. | `[ ]` Verified | **High (Orange)** |
| **REV-EVO-01** | Offboard Trace Purge & Audit Archive | Execute `janus offboard`; verify `absurd_steps`/`absurd_tasks` rows fully DELETEd (not NULL-ified); `absurd_audit_log` has one row per task; per-blueprint DB retained; `production_report.md` generated. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-EVO-02** | Blueprint Onboard & Tenant Registration | Execute `janus onboard`; verify `blueprints` table has exactly one `ACTIVE` row, idempotent with no duplicates, `CREATE DATABASE` called, and re-Onboard recycles `production_report.md` into System Prompt. | `[ ]` Verified | **High (Orange)** |
| **REV-OPS-01** | Workflow Progress Visibility | Dispatch multi-step workflow; open progress dashboard; verify step states refresh ≤2s, `SUSPENDED` highlights ≤1s, and `janus status` output consistent with dashboard (same source). | `[ ]` Verified | **High (Orange)** |

| **REV-GW-01** | Gateway HTTP Callback Ingress | POST valid + invalid (no-HMAC, wrong-HMAC, duplicate) callbacks to `127.0.0.1:8443/v1/runs/{id}/actions`; verify 200/401/409 responses. | `[ ]` Verified | **Critical (Red)** |
| **REV-GW-02** | Verdict Timeout (Fail-Closed) | Set `JANUS_HITL_TIMEOUT_SECS=5`; trigger HITL; do not send callback; verify `Err(Timeout)` returned and verdict defaults to BLOCK. | `[ ]` Verified | **Critical (Red)** |
| **REV-GW-03** | Non-Blocking Dispatch | Trigger HITL; measure control-loop resume time; verify `dispatch()` returns < 5ms and tmux session is never paused. | `[ ]` Verified | **High (Orange)** |
| **REV-GW-04** | expires_at Expiry (410 Gone) | Set `expires_at` to 1s in the past; POST callback; verify `410 Gone` and verdict not applied. | `[ ]` Verified | **High (Orange)** |
| **REV-GW-05** | Teams Adaptive Card Format | Trigger HITL with TeamsSender; capture outbound JSON; verify Adaptive Card schema with Approve/Reject/Override actions. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-COG-01** | Cognitive Provider Advisory Check | Configure provider returning `Some("pin conflict")`; dispatch matching command; verify BLOCK verdict with cognitive reason. | `[ ]` Verified | **High (Orange)** |
| **REV-COG-02** | Cognitive Provider Timeout (Pass-Through) | Configure provider that hangs; dispatch command; verify 2s timeout, WARN log, standard verdict proceeds. | `[ ]` Verified | **High (Orange)** |
| **REV-COG-03** | extract_knowledge Supplement | Execute Offboard with provider active; verify `production_report.md` contains both LLM smelt + provider artifact; verify provider failure does not block Offboard. | `[ ]` Verified | **Medium (Yellow)** |

## 4. UAT Final Approval

This specification is physically verified by the **MetaMach 0.3.0 Architect** and the **Factory Director (End User)**. Sign-off is executed digitally via GPG-signed Git tag (`git tag -s v0.3.0-review-approved`) or GitHub PR approval workflow.

Once sign-off is complete, the Richmond Hill workshop's distributed silicon leviathan formally ignites and connects to the grid.

- **Architect (System Stability & Security Endorsement):** GPG-signed tag or PR approval
- **Factory Director (Production Business & Grid-Connection Approval):** GPG-signed tag or PR approval
