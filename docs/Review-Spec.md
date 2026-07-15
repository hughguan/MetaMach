# MetaMach 1.0 — Review & Audit Standards

> Architecture walkthrough and audit criteria centered on design philosophy, ultimate security, system stability, and multi-dimensional disaster recovery.

> **Safety Review Red Line:** All security review items in this specification (especially the REV-SEC-* series) **must be executed in an isolated container or dedicated test VM; strictly prohibited on production machines or hosts containing personal data.** All "destructive command" tests use safe equivalents such as `/tmp/metamach-review-*` sentinel directories; real system-level deletion is never executed.

## 1. Philosophy Alignment

Before MetaMach 1.0 formally connects to the grid, the Architect and Factory Director must jointly conduct an **Architecture-Level Design Review**. This specification provides a set of rigorous, quantified audit criteria to prove that the system design can 100% defend the following four design pillars even under harsh physical environments and black-box AI vulnerabilities:

```
+-----------------------------------------------------------------------------------------+
|                           METAMACH 1.0 REVIEW PILLARS                                    |
+-----------------------------------------------------------------------------------------+
|  1. Security    → Validate physical janus-sh interception & /dev/shm RAM disk key protection |
|  2. Stability   → Validate remain-on-exit process guardianship & 16KB anti-bloat defense    |
|  3. Disaster    → Validate multi-tenant isolation, zero-state cold start & Fallback cache    |
|  4. Evolution   → Validate Offboard smelting QA report & OpenWiki genetic inheritance       |
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

- **Metric 1.2: `janus-sh` Proxy Interception & Tool Guard Reconciliation Audit**
    - _Requirement:_ Prove that interception does not rely on LLM self-restraint; interception occurs synchronously before the real physical shell.
    - _Pass Criteria:_
        1. Inspect every PTY pane launched by Tether; its `SHELL` environment variable must be forced to the absolute path `${HERDR_PLUGIN_ROOT}/bin/janus-sh`.
        2. Audit the UDS synchronous blocking call mechanism between `janus-sh` and `janus.sock`. When sending an interception command, `janus-sh` must remain in a Blocked suspended state, never preemptively forking a child process.
        3. Audit the Tool Guard decision matrix: high-risk commands (e.g., unauthorized network egress, physical flash erasure, non-Dry-Run trading order execution) must 100% trigger rewrite redirection or direct error blocking before a **Correlation ID digital signature** approved by Teams/TUI is detected.

### Domain 2: Physical System Stability (Stability & Budget Review)

This review aims to prove: **Under extreme high-frequency loads such as Agent log infinite-loop spam or database connection pool disconnection, the system will not suffer cascading physical crashes.**

- **Metric 2.1: PTY Process Anti-Deadlock & `remain-on-exit` Review**
    - _Requirement:_ Prove that on task interruption, error, or network drop, the scene is absolutely preserved and processes are not killed.
    - _Pass Criteria:_
        1. Audit `configs/tmux.conf` and `herdr-tether` invocation init parameters; must 100% configure per-session `remain-on-exit on` inside an independent tmux server (`tmux -L metamach-tether`), with no `-g` global flag.
        2. Artificially send `kill -9 <agent_process>` to an Agent pane; that tmux window must remain in `[Exited]` suspended state, the window does not auto-close, and the console history cache (stdout) is fully retrievable via `herdr-tether attach`.

- **Metric 2.2: 16 KiB Storage Volume Budget Anti-Bloat Audit**
    - _Requirement:_ Verify that the system can forcibly cut off infinite-loop log spam before database insertion, protecting Postgres.
    - _Pass Criteria:_
        1. When stdout streams into the write cache, `janus-daemon`'s buffer pool before the `absurd` module `INSERT` transaction must be capped at **16 KiB (16384 Bytes)** (authoritative enforcement point; `janus-sh` early-streaming truncation is an optimization only).
        2. For input exceeding the budget, the JSON field size landing in `absurd_steps.result_cache` must be strictly equal to 16KB, with the excess physically truncated and a `[MetaMach Log Budget Exceeded]` marker atomically injected at the tail.

- **Metric 2.3: Workflow Progress Visibility Audit**
    - _Requirement:_ Prove that the Factory Director has real-time, trustworthy visibility into in-flight workflows, can distinguish "running normally" from "stuck," and that queries do not interfere with the execution channel.
    - _Pass Criteria:_
        1. While the Popup Progress view is open, step status (`PENDING -> RUNNING -> COMPLETED`) latency relative to real execution must be ≤ 2s.
        2. When any step enters `SUSPENDED`, the progress dashboard must highlight that row within 1s and render `[Attach]` / `[Resume]` entries.
        3. `progress` queries must use a read-only bypass transaction: during heavy compilation (write-transaction-intensive), opening the dashboard must not cause workflow stalls or UDS blocking.
        4. In non-TUI environments, `janus status` output must be consistent with simultaneous dashboard data (same-source `progress` primitive).

- **Metric 2.4: Load & Resource Stress Audit**
    - _Requirement:_ Prove that under high-concurrency dispatch and long-running operation, the system does not deadlock or leak resources.
    - _Pass Criteria:_
        1. Dispatch 5 concurrent `dev-flow` pipelines—all must complete without deadlock.
        2. After 24 hours of continuous Daemon operation, memory usage must remain < 256MB.
        3. UDS command verdict round-trip latency (`janus-sh` -> Daemon -> response) p99 must be < 10ms under concurrent load.

### Domain 3: High Availability & Disaster Recovery (Disaster Recovery Review)

This review aims to prove: **Under disaster scenarios such as sudden power loss or physical hardware restart at the Richmond Hill workshop, the system possesses completely lossless state reconstruction and self-healing capability.**

- **Metric 3.1: Cold-Start Zero-State Self-Healing (Reconciliation) Review**
    - _Requirement:_ Prove that the system does not rely on fragile `tmux-resurrect`; it completely reconstructs and resumes at physical breakpoints via database transactions.
    - _Pass Criteria:_
        1. Prohibit configuring any `tmux-resurrect` plugin in tmux.
        2. Simulate host power-cycle restart: kill all `tmux` and `janus-daemon` processes.
        3. After re-launching the Daemon, trigger self-healing reconciliation: the Daemon must successfully retrieve the last Step in `COMPLETED` state from the `absurd_steps` physical table.
        4. The system must be able to use that Step's `result_cache` cache, auto-request a brand-new Tether Session UUID, and smoothly re-run in the background—the entire process transparent to the foreground interactive end.

- **Metric 3.2: Database Outage Fallback Mechanism Audit**
    - _Requirement:_ Verify that when the Absurd Postgres database container unexpectedly exits, workshop production does not halt.
    - _Pass Criteria:_
        1. Manually shut down the Postgres container; simulate database disconnection while `janus-daemon` is running.
        2. At this point, newly initiated Step states and transition states must seamlessly switch to writing to the local temporary **`HERDR_PLUGIN_STATE_DIR/fallback.db`** (local SQLite ring buffer).
        3. After re-launching the PG container, the Daemon must trigger synchronous replay (Log Replay), incrementally merging records from local `fallback.db` into the primary database with zero state loss.

### Domain 4: Lifecycle Smelting & Self-Evolution (Melt & Evolution Review)

This review aims to prove: **After product Offboard, the database has zero volume inflation, and accumulated debugging experience can be 100% inherited by the next generation of Agents.**

- **Metric 4.1: Logical Multi-Tenant Multi-Dimensional Cleanup Audit**
    - _Requirement:_ Verify that after executing the `melt_blueprint_data('<name>')` stored procedure, the TOAST physical tablespace occupied by the database is completely released.
    - _Pass Criteria:_
        1. Before Offboard, record the physical disk usage size of the `absurd_steps` table in Postgres.
        2. Execute `janus offboard --blueprint <name>`.
        3. After execution, all `result_cache` JSON large-field rows belonging to that Blueprint must be **entirely DELETEd** (not NULL-ified; verified via `SELECT count(*) FROM absurd_steps WHERE blueprint_id = <id> AND result_cache IS NOT NULL` returns 0).
        4. Invoke `VACUUM FULL absurd_steps`; physical disk usage must exhibit a cliff-like contraction, proving successful space reclamation.

- **Metric 4.2: Silicon Knowledge Inheritance (Few-Shot Avoidance) Audit**
    - _Requirement:_ Verify that the generated `production_report.md` possesses genuine few-shot self-healing and antibody inheritance capability.
    - _Pass Criteria:_
        1. The `production_report.md` must structurally contain the prior generation's: **[Compile Error History]**, **[Pin Conflict Details]**, **[Tool Guard Synchronous Interception Logs]**, and **[Successful Patches Applied]**.
        2. On re-Onboarding the project, OpenWiki must prioritize indexing and merging that QA whitepaper.
        3. When the next-generation Agent enters and scans that knowledge graph, its System Prompt must successfully carry that immune information, and in subsequent code generation proactively avoid pin conflicts, passing compilation on the first attempt.

- **Metric 4.3: Blueprint Onboard & Tenant Registration Audit**
    - _Requirement:_ Prove that `janus onboard` can safely and idempotently convert a blueprint directory into an `ACTIVE` dispatchable product line and correctly recycle historical experience.
    - _Pass Criteria:_
        1. After executing `janus onboard --blueprint <name>`, the `blueprints` table must have exactly one `ACTIVE` row for that blueprint, and the Popup dispatch menu must instantly become visible.
        2. Consecutively repeated Onboard must not produce duplicate rows (`ON CONFLICT` idempotent) and must not corrupt existing Task/Step data.
        3. Re-onboarding an already `OFFBOARDED` blueprint must return it to `ACTIVE` status, and if a prior `production_report.md` exists, its critical failure patterns must appear as `## Previous Incidents` few-shot in the next-generation Agent's System Prompt (verifiable via Daemon debug endpoint).

## 3. Review Sign-Off Sheet

The Factory Director and Architect must physically verify and sign off each item in the following table during reconciliation:

| Review ID | Audit Item | Verification Method | Status | Risk |
|---|---|---|---|---|
| **REV-SEC-01** | `/dev/shm` Permission Isolation | Run `stat /dev/shm/*.decrypted` on the host; verify permissions `0600` and owner is the daemon user. | `[ ]` Verified | **Critical (Red)** |
| **REV-SEC-02** | `janus-sh` Privilege Blocking | Create sentinel `mkdir -p /tmp/metamach-review-$(uuidgen) && echo s > /tmp/metamach-review-$(uuidgen)/sentinel`; via Agent pane, force-execute blacklisted `rm -rf /tmp/metamach-review-*`; verify Daemon UDS synchronously intercepts and locks, and sentinel file survives. | `[ ]` Verified | **Critical (Red)** |
| **REV-SEC-03** | UDS Channel Integrity | `stat janus.sock` verify permissions `0600`; connection attempt as different user must be rejected; verify Daemon validates peer PID/UID. | `[ ]` Verified | **High (Orange)** |
| **REV-SEC-04** | Post-Crash Key Hygiene | Load keys, then `kill -9 janus-daemon`; audit `/dev/shm/*.decrypted` cleanup or tmpfiles rule; after reboot, `/dev/shm` is empty. | `[ ]` Verified | **High (Orange)** |
| **REV-SEC-05** | Network Egress Control | Scout-level Agent attempts `curl`/`python3 urllib`/`/dev/tcp` egress—all blocked; document control layer. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-STB-01** | 16KB Size Budget | Run `cat /dev/urandom` spam; verify JSON written to Postgres is forcibly truncated with Budget marker. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-STB-03** | Load & Resource Stress | 5 concurrent `dev-flow` complete without deadlock; 24h Daemon memory < 256MB; UDS verdict p99 < 10ms. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-DIS-01** | Cold-Start Zero-State Self-Healing | `killall -9 janus-daemon` and kill only MetaMach tmux sessions (`for s in $(tmux list-sessions -F '#{session_name}' | grep '^tether-janus-'); do tmux kill-session -t "$s"; done`); DB via `docker compose stop` to simulate power loss; verify smooth breakpoint resumption after re-launch. | `[ ]` Verified | **High (Orange)** |
| **REV-EVO-01** | Offboard Degradation & Experience Inheritance | Execute `janus offboard`; verify database large JSON wipe rate reaches 100% (all rows DELETEd, not NULL-ified), and `production_report.md` successfully generated locally. | `[ ]` Verified | **Medium (Yellow)** |
| **REV-OPS-01** | Workflow Progress Visibility | Dispatch multi-step workflow; open progress dashboard; verify step states refresh ≤2s, `SUSPENDED` highlights ≤1s, and `janus status` output consistent with dashboard (same source). | `[ ]` Verified | **High (Orange)** |
| **REV-EVO-02** | Blueprint Onboard & Tenant Registration | Execute `janus onboard`; verify `blueprints` table has exactly one `ACTIVE` row, idempotent with no duplicates, and re-Onboard recycles `production_report.md` into System Prompt. | `[ ]` Verified | **High (Orange)** |

## 4. UAT Final Approval

This specification is physically verified by the **MetaMach 1.0 Architect** and the **Factory Director (End User)**. Sign-off is executed digitally via GPG-signed Git tag (`git tag -s v1.0-review-approved`) or GitHub PR approval workflow.

Once sign-off is complete, the Richmond Hill workshop's distributed silicon leviathan formally ignites and connects to the grid.

- **Architect (System Stability & Security Endorsement):** GPG-signed tag or PR approval
- **Factory Director (Production Business & Grid-Connection Approval):** GPG-signed tag or PR approval
