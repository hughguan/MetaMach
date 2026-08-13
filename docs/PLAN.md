# MetaMach 0.7.0-candidate — Project Plan & Execution History

> **Scope:** Milestone roadmap (M0–M6) and execution plan covering all physical check-in units, verification gates, and milestone achievements from 0.1.0 through 0.7.0-candidate.  
> **Status:** Fully Implemented & CI-Green.

---

## Milestone Roadmap Summary

```
[M0: Herdr Contract] ──► [M1: Infra & TUI] ──► [M2: Daemon & Absurd] ──► [M3: Tool Guard] ──► [M4: Lifecycle & Self-Heal] ──► [M5: Integration & CI]
  Herdr 0.7.3 Validated    Native PG / TUI         UDS & Multi-DB Fanout      Fail-Closed Interceptor     Init/Offboard + Cold-Start     178 Tests & Blocking CI
```

---

## Milestone Breakdown & Check-in Units

### Milestone 0: Herdr 0.7.3 Plugin Contract Validation — ✅ Completed
- **Task 0.1**: Verified Herdr 0.7.3 CLI (`herdr plugin link`), manifest schema (`herdr-plugin.toml`), overlay pane placement (`placement = "overlay"`), and injected environment variables (`HERDR_PLUGIN_ROOT`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_STATE_DIR`).
- **Task 0.2**: Validated `herdr-janus` ratatui overlay rendering and UDS client socket handling.

### Milestone 1: Infrastructure Grid-Connection & Shadow TUI — ✅ Completed
- **Task 1.1**: Host-native PostgreSQL cluster setup (`make db-init`), catalog schema (`001_catalog.sql`), and blueprint migration overlay.
- **Task 1.2**: `herdr-janus` shadow TUI client with dual views (Dispatch view and Progress view, toggled via `Tab`).

### Milestone 2: `janus-daemon` Resident Brain & Absurd PG Engine — ✅ Completed
- **Task 2.1**: `janus-daemon` resident background service, UDS listener (`janus.sock`), and PID locking (`janus.pid` with stale PID detection).
- **Task 2.2**: Shadow client lazy-start protocol (spawns daemon detached if socket is offline).
- **Task 2.3**: Absurd PostgreSQL durable engine adapter (`janus::absurd::AbsurdPgAdapter`).
- **Task 2.4**: F1 Multi-DB fanout architecture (catalog DB `metamach_db` + dedicated per-blueprint DB `metamach_blueprint_<name>`).

### Milestone 3: `janush` Proxy Shell & Tool Guard Security Layer — ✅ Completed
- **Task 3.1**: `janush` proxy shell binary (interception layer injected into tmux PTY sessions).
- **Task 3.2**: `janus::tool_guard` rule engine (`ALLOW` / `BLOCK` / `REWRITE`), 30s fail-closed timeout (exit code 126).
- **Task 3.3**: `janus::tmux` physical session engine (isolated `tmux -L metamach-tmux` server, `remain-on-exit` panes).

### Milestone 4: Lifecycle Management, Cold-Start & HITL Gateway — ✅ Completed
- **Task 4.1**: `janus init` scaffold and blueprint validation / registration.
- **Task 4.2**: `janus offboard` data smelting (`melt_blueprint_data`), git report generation (`production_report.md`).
- **Task 4.3**: Cold-start reconciliation (`janus::coldstart` auto-resumes interrupted tasks from last `COMPLETED` checkpoint).
- **Task 4.4**: `janus::gateway` HITL Gateway (Teams Adaptive Cards, HMAC-SHA256 verification, non-blocking verdict loop).
- **Task 4.5**: Cross-host SSH reverse tunnel transport (`janus::tmux` remote execution over SSH `-R`).

### Milestone 5: Integration Test Suite & CI Hardening — ✅ Completed
- **Task 5.1**: Comprehensive test suite (178 tests across 8 integration files and unit tests).
- **Task 5.2**: GitHub Actions CI matrix (`ubuntu-24.04` full PG/tmux/Herdr tests + `macos-latest` build tests).
- **Task 5.3**: Pre-push hook (`scripts/pre-push`) with docs-only diff detection (`paths-ignore`).

---

## 0.4.0 & 0.5.0 Feature Extensions

- **0.4.0 Delta**: Cognitive Provider SPI (`janus::cognitive` MCP plugins), ANSI stream filter, hardware pre-flight probes.
- **0.5.0 Delta (ADR-029 & ADR-031)**: Project-based templates under `.janus/`, unified Workflow DSL (combining Linear steps and DAG node graphs under `.janus/workflows/`), shape-driven dispatch in `janus-daemon`.
