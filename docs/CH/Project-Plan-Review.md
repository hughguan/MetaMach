# Project-Plan Review — Roadmap & Milestone Deep-Dive

> **Document under review:** `docs/CH/Project-Plan.md`
> **Review lens:** Milestone sequencing, dependency analysis, check-in unit granularity, risk assessment, timeline realism

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Milestone cannot proceed as planned |
| 🟠 **HIGH** | Dependency gap or unrealistic assumption |
| 🟡 **MEDIUM** | Sequencing issue or missing detail |
| ⚪ **LOW** | Polish |

---

## 1. 🔴 Critical Dependency Gaps

### 1.1 Milestone 1 assumes Herdr v1 exists and is working

Check-in Unit 2 (M1 Task 1.2) depends on:
- Herdr v1 being installed and functional
- `herdr plugin link` command working
- `prefix+j` keybinding dispatching to the metamach plugin
- `herdr-plugin.toml` being parseable by Herdr
- `placement = "popup"` being a valid Herdr directive

**None of these are verified or documented.** If Herdr v1 is not yet available (or its plugin SDK changes), Milestone 1 is blocked at the very first user-visible task.

**Recommendation:**
- Add a **Milestone 0: Herdr v1 Integration Validation** with tasks:
  - Verify Herdr v1 installation and plugin SDK
  - Create a minimal "hello world" plugin to validate the popup contract
  - Document the exact Herdr v1 plugin API surface needed by metamach
- OR, if Herdr is part of the MetaMach project, add Herdr core tasks to Milestone 1

---

### 1.2 `herdr-tether` is a dependency of M4 but never built

Milestone 4 Task 4.1 says "Daemon 自动调用本地的 `herdr-tether`", but `herdr-tether` is:
- Not in the M1 check-in list
- Not in the M2 check-in list
- Not in the M3 check-in list
- Not compiled anywhere in the four milestones

The first time `herdr-tether` appears as a build target is... never. It materializes in M4 by magic.

**Recommendation:** Add a **Milestone 2.5 (or M3.5): Tether Engine** with tasks:
- Implement `herdr-tether` binary (tmux session management, SSH transport)
- Integrate `remain-on-exit` session creation
- UAT: `herdr-tether open --command "sleep 100"` creates a persistent tmux session

---

### 1.3 OpenWiki integration has no milestone

OpenWiki is referenced throughout the architecture as critical to the knowledge layer, but no milestone includes building or integrating OpenWiki:
- M1: infrastructure + Popup (no OpenWiki)
- M2: Daemon + UDS (no OpenWiki)
- M3: janus-sh + Tool Guard (no OpenWiki)
- M4: Tether + Offboard (Offboard writes TO OpenWiki, but OpenWiki must already exist)

**Recommendation:** Either:
- Add OpenWiki integration as a subtask in M2 (Daemon queries OpenWiki for RAG) and M4 (Offboard writes to OpenWiki)
- Or define OpenWiki as a separate project with its own milestones and declare the integration point explicitly

---

## 2. 🟠 Milestone Sequencing Issues

### 2.1 M3 (janus-sh) depends on M4 (Tether) — dependency inversion

M3 Task 3.1 says: "Tether 启动 Pane 时，强制将底层环境变量 `SHELL` 注入重定向为 `target/release/janus-sh`". This means:
- `janus-sh` is built in M3
- But Tether (which sets `SHELL=janus-sh` and creates panes) is built in M4

So in M3, `janus-sh` is compiled but cannot be **tested in its actual runtime context** (inside a Tether-managed tmux pane). The UAT says "在 Agent 窗格中强制运行 `rm -rf /`" — but there is no "Agent 窗格" until M4.

**Recommendation:** Move Tether core (tmux session management) to M3, before janus-sh integration. The "cross-host SSH" part of Tether can stay in M4, but the local tmux pane creation must be available in M3 to validate janus-sh.

New ordering:
- **M2.5: Tether Core** — local tmux session create/attach/remain-on-exit
- **M3: Shield Layer** — janus-sh + Tool Guard (tested inside Tether panes)
- **M4: Advanced** — cross-host SSH + cold start recovery + Offboard

---

### 2.2 M2 Daemon UDS test uses "Mock Blueprint list" — unrealistic validation

Task 2.1 UAT: "当 UDS 收到请求时，向客户端（herdr-janus）发送 Mock 的 Blueprint 列表". A mock list validates the UDS transport but not the database integration, which is the whole point of the Daemon. The real validation should happen against the Postgres database that was set up in M1.

**Recommendation:** By end of M2, the Daemon should query `blueprints` table from Postgres (set up in M1) and return real data. The UAT should be: "Daemon queries Postgres and returns all blueprints with status ACTIVE."

---

### 2.3 M4 Task 4.2 (Offboard) bundles three complex subsystems

Task 4.2 compresses into a single check-in unit:
1. Database scan and analysis of all historical Steps
2. LLM summarization into Markdown
3. Database melt procedure (`melt_blueprint_data`)
4. Git auto-commit and push of `production_report.md`

Each of these is a non-trivial engineering task. Bundling them into one check-in unit violates the project plan's own principle of "可独立提交/物理并网".

**Recommendation:** Split into:
- **Check-in Unit 8a:** `melt_blueprint_data` stored procedure + Daemon integration (DB-only)
- **Check-in Unit 8b:** Offboard orchestrator (scan → summarize → melt) with LLM integration
- **Check-in Unit 8c:** Git auto-commit of production_report.md

---

## 3. 🟡 Missing Milestones & Tasks

### 3.1 No configuration management milestone

`configs/agents.toml`, `configs/tmux.conf`, `configs/global_rules.md`, and `janus.toml` appear across all milestones but no task creates or validates them. They are assumed to exist.

**Recommendation:** Add to M1: "Create and validate all configuration file templates with schema validation."

---

### 3.2 No CI/CD pipeline task

The check-in gates (§🏁) list `cargo fmt`, `cargo clippy`, and `cargo test` as requirements, but no milestone includes setting up GitHub Actions (despite `.github/workflows/build-janus.yml` appearing in the ARCH.md directory tree).

**Recommendation:** Add to M1: "Set up GitHub Actions workflow for fmt + clippy + test on push to main."

---

### 3.3 No integration testing milestone

All UAT validations are manual ("physical verification"). There's no milestone for automated integration tests. The Test-Spec.md defines test cases but the project plan never allocates time to implement them.

**Recommendation:** Add an **M3.5: Integration Test Suite** milestone:
- Implement UTC-01 through UTC-05 as automated test scripts
- Add to CI: `cargo test --integration` runs against docker-compose test environment

---

### 3.4 No documentation milestone

README.md, ARCH.md, PRD.md etc. all exist as design docs, but there's no task for operational documentation (runbooks, troubleshooting guides, API docs for the UDS protocol).

**Recommendation:** Add to M4 or as a parallel track: "Write operator runbook and UDS protocol documentation."

---

## 4. 🟡 Risk Assessment

### 4.1 Single-point-of-failure: one developer assumption

The plan assumes sequential execution by a single developer or small team. There's no parallelization strategy. If M2 (Daemon) is blocked, M3 (janus-sh) cannot proceed in parallel even though janus-sh could theoretically be developed against a mock Daemon interface.

**Recommendation:** Define the UDS protocol contract (request/response JSON schema) as a M1 deliverable. This allows M2 (Daemon server) and M3 (janus-sh client) to be developed in parallel against the shared contract.

---

### 4.2 No time estimates

The milestones have no timeboxes. Without estimates, it's impossible to:
- Assess if the plan is realistic
- Track progress against schedule
- Identify schedule slips early

**Recommendation:** Add rough timeboxes: M1 (2 weeks), M2 (3 weeks), M3 (3 weeks), M4 (4 weeks). Adjust based on team size.

---

### 4.3 No rollback or revert strategy

Each check-in unit is described as "可独立提交" but there's no mention of how to revert if a check-in breaks previous functionality. With sequential milestones and tightly coupled components, a regression in M4 could break M1 functionality.

**Recommendation:** Add to check-in gates: "All existing UAT validations from previous milestones must still pass."

---

## 5. ⚪ Minor Issues

- M1 check-in list says `janus/src/bin/herdr_janus.rs` but the directory tree in ARCH.md also shows `janus_daemon.rs`. This is correct (herdr_janus is M1, janus_daemon is M2).
- "双生子进程" (twin process) terminology is used for M2 but the actual architecture has 4+ binaries. Consider "多进程通信架构".
- "Check-in Unit" numbering (1-8) is clear but not linked to Git tags or releases. Consider tagging each check-in: `v0.1.0-m1-checkin1`, etc.

---

## Summary: Project-Plan Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Add M0: Herdr v1 integration validation |
| 2 | 🔴 | Add Tether (herdr-tether) build task before M4 |
| 3 | 🔴 | Add OpenWiki integration tasks |
| 4 | 🟠 | Move local Tether to M3 (dependency of janus-sh testing) |
| 5 | 🟠 | M2 UAT: query real Postgres, not mock data |
| 6 | 🟠 | Split M4 Task 4.2 (Offboard) into 3 check-in units |
| 7 | 🟡 | Add configuration management task to M1 |
| 8 | 🟡 | Add CI/CD setup task to M1 |
| 9 | 🟡 | Add automated integration test milestone |
| 10 | 🟡 | Add documentation milestone |
| 11 | 🟡 | Define UDS protocol contract in M1 for parallel development |
| 12 | 🟡 | Add time estimates to milestones |
| 13 | 🟡 | Add regression check to check-in gates |

> **Resolution Log (2026-07-15):**
> - **#1 🔴 (M0: Herdr v1 validation)** ✅ RESOLVED - Added Milestone 0 (Task 0.1 SDK validation + Task 0.2 Popup PoC) before M1; timeline updated; M1 Task 1.2 no longer assumes Herdr blindly.
> - **#2 🔴 (herdr-tether fetch/build task)** ✅ RESOLVED - Added M2 Task 2.4 (Check-in Unit 4c): `make bootstrap` pulls/builds herdr-tether, UAT `herdr-tether open` persistent session.
> - **#3 🔴 (OpenWiki integration tasks)** ✅ RESOLVED - Added M2 Task 2.5 (Check-in Unit 4d): pull/build OpenWiki + Daemon `openwiki_query` RAG, index isolation UAT; closes loop with M4 Offboard write-back.
> - **#5 🟠 (M2 UAT real Postgres, not mock)** ✅ RESOLVED - M2 Task 2.1 UAT now queries real `blueprints` (status ACTIVE) from Postgres.
> - **#11 🟡 (UDS protocol contract for parallel dev)** ✅ RESOLVED - UDS request/response contracts defined in Feature-Spec §3.2/3.4; enables parallel Daemon/janus-sh development.
