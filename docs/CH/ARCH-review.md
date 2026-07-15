# MetaMach 2.0 — Architectural Design Review

> **Cross-document audit of all `docs/CH/` specifications.**
> Target: identify inconsistencies, specification gaps, execution risks, and unresolved design decisions before Milestone 1 implementation begins.

---

## Review Scope

Documents audited:

| Doc | Scope |
|-----|-------|
| `ARCH.md` | System architecture, directory topology, component interactivity |
| `PRD.md` | Product requirements & 厂长 user journey |
| `Feature-Spec.md` | Engineering feature specs, data contracts, fault matrix |
| `Project-Plan.md` | Milestone roadmap & check-in units |
| `Review-Spec.md` | Audit/review standards & sign-off criteria |
| `Test-Spec.md` | Test cases & QA strategy |
| `Deployment-Spec.md` | Physical deployment, bootstrap, directory mapping |

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Prevents implementation; must resolve before Milestone 1 |
| 🟠 **HIGH** | Breaks consistency or introduces risk; resolve before affected milestone |
| 🟡 **MEDIUM** | Ambiguity or gap that may cause rework; resolve before relevant feature |
| ⚪ **LOW** | Cosmetic / naming polish |

---

## 1. 🔴 Source Tree vs. Referenced Binaries — Critical Gaps

### 1.1 `herdr-tether` binary has no source location ✅ RESOLVED

**Affected docs:** `ARCH.md` §5, `Feature-Spec.md` §2.3, `Project-Plan.md` Task 4.1

`ARCH.md` §5 directory tree lists only two binaries under `janus/src/bin/`:

```
janus/src/bin/
├── janus_daemon.rs
└── herdr_janus.rs
```

Yet `herdr-tether` is referenced throughout as a first-class binary responsible for:
- Creating/managing tmux sessions (`remain-on-exit`)
- Cross-host SSH session orchestration
- `tether open`, `tether attach` CLI commands

**Resolution:** `herdr-tether` is an **external dependency** — separate repository at:
> https://github.com/moneycaringcoder/herdr-tether

**Remaining action:** Update `ARCH.md` §5 directory tree to document `herdr-tether` as an external dep (git submodule or cargo git dependency). Update `Deployment-Spec.md` and `Project-Plan.md` to reflect that `herdr-tether` is fetched/built from this external repo, not compiled from within `janus/`.

---

### 1.2 `janus-sh` binary has no source location

**Affected docs:** `ARCH.md` §5, `Feature-Spec.md` §2.2, `Project-Plan.md` Task 3.1

`ARCH.md` §5 shows:
```
janus/src/
├── tool_guard/          # janus-sh 代理 Shell 内存拦截
```

But `tool_guard/` is described as a module, not a binary crate. `Project-Plan.md` Task 3.1 says `janus-sh` is a "独立编译目标" (independent compilation target). `Feature-Spec.md` §2.2 says it's at `target/release/janus-sh`.

**Questions:**
- Is `janus-sh` compiled from `janus/src/bin/janus_sh.rs`?
- Is it a separate crate within the Cargo workspace?
- Why is it listed under `tool_guard/` in the tree but described as a binary?

**Recommendation:** Add `janus_sh.rs` to `janus/src/bin/` in the directory tree. Clarify whether `tool_guard/` is a library module consumed by the daemon, the proxy shell, or both.

---

### 1.3 `openwiki/` & `absurd/` — external dependency boundaries ✅ RESOLVED

**Affected docs:** `ARCH.md` §5

The tree shows:
```
openwiki/
├── bin/                    # 编译好的 OpenWiki 独立静态二进制
└── configs/
    └── global_rules.md
```

Pre-compiled binaries in a monorepo are an anti-pattern. All three referenced external components now have confirmed repos:

| Component | External Repo | Status |
|-----------|--------------|--------|
| **herdr-tether** (tmux/SSH execution engine) | https://github.com/moneycaringcoder/herdr-tether | ✅ |
| **Absurd** (Postgres engine / DB layer) | https://github.com/earendil-works/absurd | ✅ |
| **OpenWiki** (RAG knowledge /联邦脑图) | https://github.com/langchain-ai/openwiki | ✅ |

**Remaining action:** Document integration strategy for all three external deps in `ARCH.md` §5 and `Deployment-Spec.md` §1:
- Git submodules? Cargo git dependencies? Pre-built binaries fetched by `make bootstrap`?
- Remove `openwiki/bin/` from the monorepo tree; OpenWiki is now an external dep, not a pre-compiled binary checked into the repo.
- Clarify: does the `openwiki/` directory in the monorepo become a pure config/skill directory (per-blueprint knowledge), with the OpenWiki engine pulled from the external repo at build time?

---

### 1.4 `configs/` directory missing from tree

**Affected docs:** `ARCH.md` §5, `Deployment-Spec.md` §2, `Feature-Spec.md` §2.5

The `ARCH.md` §5 tree has no `configs/` directory, yet multiple docs reference:
- `configs/agents.toml` — agent pool registration & permission levels (Deployment-Spec §2, Feature-Spec §2.2)
- `configs/global_rules.md` — global factory rules injected into Agent system prompts (Feature-Spec §2.5, ARCH §2.5)
- `configs/sops/hi5bot.encrypted.json` — encrypted secrets (Deployment-Spec §4.1)
- `configs/tmux.conf` — tmux configuration with `remain-on-exit` (Review-Spec §2.2)

Meanwhile, `ARCH.md` §5 puts `global_rules.md` under `openwiki/configs/`, creating a direct conflict.

**Recommendation:** Add `configs/` to the root of the directory tree containing `agents.toml`, `tmux.conf`, and `sops/`. Move `openwiki/configs/global_rules.md` to `configs/global_rules.md` and symlink or copy into OpenWiki's scope at runtime, or explicitly declare two separate config layers (global factory rules vs. OpenWiki-specific rules).

---

### 1.5 `janus/migrations/` missing from tree

**Affected docs:** `ARCH.md` §5, `Project-Plan.md` Task 1.1, `Deployment-Spec.md` §3.1

`Project-Plan.md` Task 1.1 and `Deployment-Spec.md` §3.1 reference `janus/migrations/` as a directory mounted into the Docker Postgres container at `/docker-entrypoint-initdb.d`. The `ARCH.md` §5 tree omits this directory.

**Recommendation:** Add `janus/migrations/` (containing `001_init_absurd.sql`) to the directory tree.

---

## 2. 🟠 Incomplete or Inconsistent Specifications

### 2.1 Absurd database schema is incomplete

**Affected docs:** `Feature-Spec.md` §3.1, `Project-Plan.md` Task 1.1

`Feature-Spec.md` §3.1 defines only `absurd_steps`:

```sql
CREATE TABLE absurd_steps (
    task_id INTEGER REFERENCES absurd_tasks(id) ON DELETE CASCADE,
    ...
    PRIMARY KEY (task_id, step_name)
);
```

But `Project-Plan.md` Task 1.1 mentions three tables: `blueprints`, `absurd_tasks`, `absurd_steps`. `absurd_tasks` is referenced via foreign key but never defined. `blueprints` is referenced throughout but has no schema at all.

**Recommendation:** Provide the full initial schema for all three tables (`blueprints`, `absurd_tasks`, `absurd_steps`) in `Feature-Spec.md` §3.1, including the `blueprint_id` partition key mentioned in `ARCH.md` §3.

---

### 2.2 `janus.toml` blueprint recipe format is undefined

**Affected docs:** `ARCH.md` §2.2(C), §5; `PRD.md` §2.1

`janus.toml` is described as binding:
- Default workflow
- OpenWiki index scope
- Remote SSH target IPs

But the TOML schema is never specified. Every blueprint references it; zero docs define it.

**Recommendation:** Add a `janus.toml` specification section to `Feature-Spec.md` with a complete example:

```toml
[blueprint]
name = "gatemetric"
default_workflow = "dev-flow"

[remote]
host = "192.168.1.100"
user = "builder"

[openwiki]
scope = ["mpu6050", "esp32-timers", "i2c-conflicts"]
```

---

### 2.3 Workflow `.toml` file format is undefined

**Affected docs:** `ARCH.md` §2.2(B), §5; `PRD.md` §2.2; `Feature-Spec.md` §2.3

Workflows are described conceptually (multi-step, multi-agent, cross-host) but the `.toml` file structure is never defined. Without this, Milestone 4 (Task 4.1) has no implementable contract.

**Recommendation:** Add a workflow file specification to `Feature-Spec.md`:

```toml
[workflow]
name = "dev-flow"
description = "Standard development pipeline"

[[steps]]
name = "scout"
agent = "scout"
command = "scan_and_report"
toolset = ["read", "grep", "find"]

[[steps]]
name = "code"
agent = "coder"
command = "apply_patch"
toolset = ["read", "write", "edit", "bash"]

[[steps]]
name = "cross-compile"
agent = "deployer"
command = "make cross-compile"
host = "remote"          # references [remote] from janus.toml
toolset = ["bash"]
```

---

### 2.4 `janus` CLI surface is inconsistent

**Affected docs:** `PRD.md` §4, `Feature-Spec.md` §2.5, `Test-Spec.md` §2.5

Commands referenced across docs:
- `janus offboard --blueprint <name>` (PRD §4, Feature-Spec §2.5)
- `janus-daemon` (binary, background process)
- `herdr-janus` (binary, shadow client)
- `herdr-tether` (binary, tmux manager)

Is `janus` a unified CLI wrapper that dispatches to subcommands? Or are `janus-daemon`, `herdr-janus`, and `herdr-tether` entirely separate binaries with no unifying CLI? If the former, the `janus` binary doesn't appear in the source tree. If the latter, `janus offboard` should be `janus-daemon offboard` or a separate `janus-admin` tool.

**Recommendation:** Define the CLI architecture explicitly:
- Option A: Single `janus` binary with subcommands (`janus daemon`, `janus offboard`, `janus tether open`)
- Option B: Separate binaries (`janus-daemon`, `herdr-janus`, `herdr-tether`) with `janus-daemon` accepting `offboard` as a CLI flag

Add this to `Feature-Spec.md` or `ARCH.md`.

---

### 2.5 `herdr-tether` CLI commands are inconsistent

**Affected docs:** `Feature-Spec.md` §2.3, `Test-Spec.md` §3.2, `Deployment-Spec.md` §6.2

Commands referenced:
- `herdr-tether open --command "sleep 100"` (Deployment-Spec §6.2)
- `tether attach` (Test-Spec §3.2, Deployment-Spec §6.2)
- `herdr-tether` via SSH (Feature-Spec §2.3)

Is the binary `herdr-tether` and the subcommand `open`/`attach`? Then `tether attach` should be `herdr-tether attach`.

**Recommendation:** Standardize on `herdr-tether <subcommand>` across all docs.

---

### 2.6 `production_report.md` generation — unspecified "大模型" dependency

**Affected docs:** `Feature-Spec.md` §2.5, `PRD.md` §2.1, `Test-Spec.md` UTC-05-02

The Offboard process "调用大模型将运行快照…压缩总结为高密度 Markdown". This is a critical external dependency with zero specification:
- Which model? (local? cloud API? Claude? GPT?)
- What if the model is unavailable?
- What is the prompt/template?
- How is the 16KB budget enforced on the model input?
- What about offline/air-gapped deployments?

**Recommendation:** Add a section to `Feature-Spec.md` specifying:
- The model endpoint configuration (reference `configs/agents.toml` or a new `configs/offboard.toml`)
- A fallback behavior (generate a raw JSON dump if model unavailable)
- The prompt template for summarization
- Input size limits (last N steps, capped at 16KB each)

---

## 3. 🟠 Execution & Safety Risks

### 3.1 `janus-sh` deadlock if Daemon is unreachable

**Affected docs:** `Feature-Spec.md` §2.2

`janus-sh` blocks synchronously on UDS waiting for Daemon response. If the Daemon crashes or UDS is broken:
- Agent's shell hangs indefinitely
- No timeout is specified
- No fallback behavior is defined (fail-open? fail-closed? timeout + kill?)

**Recommendation:** Add a configurable timeout (e.g., 30s) to `janus-sh`. On timeout, the safe default is fail-closed: return an error to the agent without executing the command. Document this in `Feature-Spec.md` §2.2.

---

### 3.2 Stale PID file prevents Daemon restart after crash

**Affected docs:** `Feature-Spec.md` §2.1, `Project-Plan.md` Task 2.1

`Feature-Spec.md` §2.1: "二次启动检测到该文件时，直接安全退出". If the Daemon crashes without cleaning `janus.pid`, the PID file persists and prevents restart. No staleness detection (e.g., check if PID is still alive) is mentioned.

**Recommendation:** Add PID file staleness detection: on startup, read the PID from the file, check if a process with that PID exists (and is a janus-daemon). If not, overwrite the stale file. Document in `Feature-Spec.md` §2.1.

---

### 3.3 `rm -rf /` as a test case is dangerously specified ✅ RESOLVED

**Affected docs:** `Test-Spec.md` UTC-02-02, `Review-Spec.md` REV-SEC-02

Both documents prescribe testing the security guard by executing `rm -rf /`. Even through a proxy shell, this is a catastrophic test case:
- If `janus-sh` has a bug, the command reaches the real shell
- On macOS (which the deployment spec supports), `rm -rf /` is not protected by `--preserve-root` by default on some versions
- A safer equivalent exists: `rm -rf /tmp/metaMach_test_sentinel`

**Recommendation:** Replace all `rm -rf /` test references with a safer destructive command test, e.g., `rm -rf /tmp/metamach_test_guard_$(uuidgen)` and verify the file still exists after the blocked command. Add a prominent warning in `Test-Spec.md` that the test must be run in an isolated container/VM.

---

### 3.4 HITL resume via `Ctrl+C` is fragile

**Affected docs:** `Feature-Spec.md` §2.4

"向对应的 Tether 窗格发送 `Ctrl+C` 释放挂起，并重新点火下发原任务"

Sending `Ctrl+C` (SIGINT) assumes the suspended process is a foreground job that responds to SIGINT. But:
- The task may have been a background process
- `Ctrl+C` only affects the foreground process group
- The agent process itself, not the command, may need to be signalled
- "重新点火下发原任务" — re-executing the original task from scratch may not be appropriate if the fix was applied in-place in the suspended session

**Recommendation:** Redesign the resume mechanism:
1. When suspended, the tmux pane stays alive with the error state visible
2. Human attaches, fixes the issue in the pane
3. Human signals completion (e.g., types `metamach-resume` or clicks the button)
4. Daemon validates the fix (re-runs the check that failed) and transitions to the next step
5. Do NOT blindly re-execute the original command from scratch — it would undo the human's fix

---

### 3.5 16KB budget enforcement point is ambiguous

**Affected docs:** `Feature-Spec.md` §2.5, §4; `Test-Spec.md` UTC-05-01

`Feature-Spec.md` §4 says: "`janus-sh` 在内存中内置流式计数器...超过16KB...阻止脏数据灌入 Postgres". This implies truncation happens in `janus-sh` before sending to Daemon.

But `Feature-Spec.md` §2.5 says: "将输出数据打包为 `JSONB result_cache` 一体化 Commit" — this implies truncation at DB write time.

`Test-Spec.md` UTC-05-01 says: "写入 Postgres 物理表的 `result_cache` 大小被强制限制在 16 KiB" — this implies the check is at DB insert time.

These are three different enforcement points. Which one is canonical?

**Recommendation:** Define a single enforcement point: `janus-daemon` should be the gatekeeper (before DB insert). `janus-sh` can do early streaming truncation as an optimization, but the authoritative 16KB enforcement must be in the Daemon's `absurd` module before the `INSERT` transaction. Document this explicitly.

---

## 4. 🟡 Specification Gaps & Ambiguities

### 4.1 Notification backends: Teams vs. Telegram — which is primary?

**Affected docs:** `ARCH.md` §3, §4; `PRD.md` §2.4, §4; `Feature-Spec.md` §2.4; `Test-Spec.md` UTC-04-02

Three different treatments:
- `ARCH.md` §3: "监听外部 Teams/TG 异步消息" — both equally
- `PRD.md` §2.4: "MS Teams / Telegram 手机端" — both
- `Feature-Spec.md` §2.4: "MS Teams/Telegram 发送标准的 Actionable MessageCard" — Teams MessageCard format, but Telegram uses a completely different API
- `Test-Spec.md` UTC-04-02: only tests Teams

MessageCard is a Microsoft-specific format. Telegram uses its own Bot API with inline keyboards.

**Recommendation:** Choose a primary notification backend (recommend: Telegram for simplicity, open protocol, and mobile-native experience). MS Teams can be a secondary integration. Define the webhook payload format abstractly and then specify the concrete adapters. Update `Test-Spec.md` to include Telegram test cases.

---

### 4.2 `SHELL=target/release/janus-sh` — relative path fragility

**Affected docs:** `Feature-Spec.md` §2.2, `Test-Spec.md` UTC-02-01

Setting `SHELL` to a relative path `target/release/janus-sh` will break if:
- CWD is not the repo root
- The binary hasn't been compiled yet
- The pane is started from a different directory

**Recommendation:** Always use an absolute path, resolved from `${HERDR_PLUGIN_ROOT}` or the compiled binary's installed location. The bootstrap process (`make bootstrap`) should install `janus-sh` to a well-known absolute path (e.g., `/usr/local/bin/janus-sh` or `${HERDR_PLUGIN_ROOT}/target/release/janus-sh`).

---

### 4.3 Herdr v1 plugin contract is undefined

**Affected docs:** `ARCH.md` §5, `Feature-Spec.md` §2.1, `Deployment-Spec.md` §2

`herdr-plugin.toml` is referenced with fields `placement`, `width`, `height`. But:
- What is Herdr v1? Is it an existing product with its own documentation?
- What other fields does `herdr-plugin.toml` support?
- What is the UDS protocol between Herdr and the plugin?
- What events does Herdr dispatch to the plugin?

Without the Herdr contract, the `herdr-janus` client's interface is underspecified.

**Recommendation:** Either:
- Link to external Herdr v1 plugin SDK documentation
- If Herdr is part of MetaMach, add its specification to `docs/`
- At minimum, provide the full `herdr-plugin.toml` schema in `Feature-Spec.md`

---

### 4.4 Docker Compose `version` field is deprecated

**Affected docs:** `Deployment-Spec.md` §3.1

```yaml
version: '3.8'
```

The `version` field has been deprecated since Docker Compose v2. It generates a warning on modern installations.

**Recommendation:** Remove the `version` top-level key from `docker-compose.yml`.

---

### 4.5 No `janus.toml` for `joyrobots` blueprint example

**Affected docs:** `ARCH.md` §5

The `gatemetric/` blueprint has `janus.toml` explicitly shown in the prose ("专属生产配方 (绑定 firmware-deploy，配置 SSH 编译靶机)"), but `joyrobots/` just has `janus.toml` listed with no description.

**Recommendation:** Provide minimal example `janus.toml` files for both blueprints, or at minimum document what fields differ between a local-only blueprint (`joyrobots`) and a cross-host blueprint (`gatemetric`).

---

### 4.6 Makefile `METAMACH_DB_PASSWORD` hardcoded default

**Affected docs:** `Deployment-Spec.md` §5.1

```makefile
export METAMACH_DB_PASSWORD ?= metamach_secure_pass_2026
```

Hardcoding a default database password in a public repository is a security concern. While it's overridable via environment variable, the default should either be randomly generated at bootstrap time or require explicit user input.

**Recommendation:** Remove the default. If `METAMACH_DB_PASSWORD` is not set, `make bootstrap` should either:
- Generate a random password and print it
- Prompt the user to set it
- Fail with a clear error message

---

### 4.7 `janus on/offboard` vs. `janus-daemon` — when is Daemon running?

**Affected docs:** `PRD.md` §4, `Feature-Spec.md` §2.5

`PRD.md` §4 says the厂长 runs `janus offboard --blueprint gatemetric` from the console. But:
- Is the Daemon required to be running for this command?
- If so, is it a client command sent through `herdr-janus` → UDS → Daemon?
- If the Daemon handles it directly, is it a CLI flag to `janus-daemon`?
- If the Daemon is NOT running, can offboard work directly against the database?

**Recommendation:** Clarify the execution model: `janus offboard` is a command sent from `herdr-janus` (or a standalone `janus` CLI) to the Daemon via UDS. The Daemon must be running. Document this dependency.

---

### 4.8 Cold start recovery: "re-run from last COMPLETED step" vs. "resume SUSPENDED"

**Affected docs:** `Feature-Spec.md` §2.3, `Test-Spec.md` UTC-03-03

`Feature-Spec.md` §2.3 says on cold start: "提取最后一次有效的 `COMPLETED` Checkpoint，指派全新 Tether Session UUID 无缝在物理断点处重跑接棒"

This means the system re-runs the NEXT step after the last COMPLETED one. But what about a step that was `RUNNING` when power was lost? It's silently abandoned — the system starts from the last COMPLETED state. This is correct behavior but should be documented as intentional data loss of the in-flight step.

Meanwhile, `SUSPENDED` steps (HITL) are handled differently — they wait for human intervention.

**Recommendation:** Add a state machine diagram to `Feature-Spec.md` §2.3 showing all state transitions:
```
PENDING → STARTING → RUNNING → COMPLETED
                            ↘ FAILED → SUSPENDED → (human resume) → RUNNING
                            ↘ (power loss) → lost, restart from last COMPLETED
```

---

### 4.9 `fallback.db` SQLite schema is undefined

**Affected docs:** `Feature-Spec.md` §4

The fault matrix mentions a SQLite ring buffer at `${HERDR_PLUGIN_STATE_DIR}/fallback.db` but:
- What tables does it contain? (mirror of `absurd_steps`?)
- What is the ring buffer size limit?
- How does conflict resolution work during log replay?

**Recommendation:** Add the `fallback.db` schema and ring buffer semantics to `Feature-Spec.md` §4.

---

### 4.10 macOS support vs. Linux-specific features

**Affected docs:** `Deployment-Spec.md` §1

The deployment spec lists "macOS 13+" as supported, but:
- `/dev/shm` on macOS is not a tmpfs by default — it may be a symlink to `/private/tmp` or not exist
- `chmod 0700 /dev/shm/...` may not work as expected on macOS
- `tmux` behavior differs slightly (e.g., `remain-on-exit` may behave differently)
- UDS paths on macOS have length limits (104 chars) shorter than Linux (108 chars)

**Recommendation:** Either:
- Add macOS-specific notes for `/dev/shm` usage (use a different RAM disk approach)
- Restrict to Linux-only for production, macOS for development only
- Test the `/dev/shm` assumptions on macOS and document workarounds

---

## 5. ⚪ Naming & Polish

### 5.1 Inconsistent database naming

- "Absurd Postgres" (`ARCH.md`, `Feature-Spec.md`)
- "Absurd PG" (`ARCH.md`, `Test-Spec.md`)
- "Unified DB" (`ARCH.md`, `Deployment-Spec.md`)
- "Unified Postgres" (`Project-Plan.md`)
- "PG" (`Review-Spec.md`, `Test-Spec.md`)

**Recommendation:** Standardize on "Absurd Postgres" (formal) and "Absurd DB" (shorthand) across all docs.

---

### 5.2 "metamach_db" vs. "metamach" database name

- `Deployment-Spec.md` §3.1: `POSTGRES_DB: metamach_db`
- `Deployment-Spec.md` §5.1: `-d metamach_db`
- But no other doc references this name directly

**Recommendation:** Consistent naming is fine. Just confirm `metamach_db` is canonical.

---

### 5.3 Mixed language in document titles

- `ARCH.md`: "基于 Janus Daemon 与分布式耐久会话的硅基工业级生产机床" (Chinese)
- `PRD.md`: "面向新任厂长的硅基工业级生产调度中枢业务指南" (Chinese)
- `README.md`: English

**Recommendation:** Since README is English and the project targets an international audience, consider keeping document body content language-consistent. Title language can follow the body language.

---

## 6. Summary: Pre-Milestone-1 Action Items

| # | Severity | Item | Affected Milestone |
|---|----------|------|---------------------|
| 1 | ~ | ~~Add `herdr-tether` source location to tree~~ → external: [herdr-tether](https://github.com/moneycaringcoder/herdr-tether) | M1 (document dep) |
| 2 | 🔴 | Add `janus-sh` binary source to tree | M3 (shield) |
| 3 | 🔴 | Add `configs/` directory to tree | M1 (infrastructure) |
| 4 | 🔴 | Add `janus/migrations/` to tree | M1 (infrastructure) |
| 5 | ~ | ~~Resolve `openwiki/` source~~ → [langchain-ai/openwiki](https://github.com/langchain-ai/openwiki) | M1 (document dep) |
| 6 | 🟠 | Define complete database schema (3 tables) | M1 (infrastructure) |
| 7 | 🟠 | Define `janus.toml` schema | M1 (infrastructure) |
| 8 | 🟠 | Define workflow `.toml` schema | M2 (daemon) |
| 9 | 🟠 | Define CLI architecture (janus vs. janus-daemon) | M2 (daemon) |
| 10 | 🟠 | Fix `herdr-tether` CLI command naming | M4 (advanced) |
| 11 | 🟠 | Add janus-sh timeout/deadlock handling | M3 (shield) |
| 12 | 🟠 | Add PID file staleness detection | M2 (daemon) |
| 13 | ✅ | ~~Replace `rm -rf /` test with safe equivalent~~ RESOLVED -> Review-Spec REV-SEC-02 + Test-Spec UTC-02-02 now use `/tmp/metamach-*` sentinel (see Review-Spec-Review #1, Test-Spec-Review #1) | M3 (shield) |
| 14 | 🟠 | Redesign HITL resume mechanism | M4 (advanced) |
| 15 | 🟠 | Clarify 16KB enforcement point | M4 (advanced) |
| 16 | 🟠 | Specify Offboard LLM dependency | M4 (advanced) |
| 17 | 🟡 | Resolve Teams vs. Telegram notification priority | M4 (advanced) |
| 18 | 🟡 | Use absolute path for `SHELL` env var | M3 (shield) |
| 19 | 🟡 | Document Herdr v1 plugin contract | M1 (infrastructure) |
| 20 | ✅ | ~~Remove deprecated Docker Compose `version` field~~ RESOLVED -> removed from docker-compose.yml (Deployment-Spec-Review #16) | M1 (infrastructure) |
| 21 | ✅ | ~~Remove hardcoded DB password default~~ RESOLVED -> Deployment-Spec Makefile generates random password (`openssl rand`), persists to Mutable State (chmod 600, gitignored); see Deployment-Spec-Review #1 | M1 (infrastructure) |
| 22 | 🟡 | Add state machine diagram | M2 (daemon) |
| 23 | 🟡 | Define `fallback.db` schema | M4 (advanced) |
| 24 | 🟡 | Address macOS `/dev/shm` compatibility | M1 (infrastructure) |
| 25 | ⚪ | Standardize database naming | All |
| 26 | ⚪ | Confirm `metamach_db` as canonical DB name | M1 |
| 27 | ✅ | **Add "Onboard a new Blueprint" workflow** (from PRD-Review #1) — RESOLVED across PRD §2.1/§4, ARCH §2.2(C), Feature-Spec §2.5, Project-Plan Task 4.3, Test-Spec UTC-05-04/05, Review-Spec 指标 4.3, Deployment-Spec §6.4 | M4 (lifecycle) |
| 28 | ✅ | **Add "View workflow progress" feature to matrix** (from PRD-Review #2) — RESOLVED across PRD §2.5/§3, ARCH §3/§4, Feature-Spec §2.6 + Contract 3.3, Project-Plan Task 2.3, Test-Spec Suite 2.6, Review-Spec 指标 2.3 | M2 (daemon) |

---

## Review Sign-Off

> **Round 3 (🟠 items, 2026-07-15):** The following §6 🟠 action items are now RESOLVED across source docs:
> - **#7 (janus.toml schema)** -> Feature-Spec Contract 3.6.
> - **#8 (workflow .toml schema)** -> Feature-Spec Contract 3.7.
> - **#9 (CLI architecture)** -> ARCH §3 CLI 架构 note (unified `janus` CLI + dedicated binaries).
> - **#10 (herdr-tether CLI naming)** -> standardized `herdr-tether <subcommand>` across all docs.
> - **#11 (janus-sh timeout/deadlock)** -> Feature-Spec §2.2 + Contract 3.4 (30s fail-closed).
> - **#12 (PID staleness)** -> Project-Plan Task 2.1 (stale-PID detection).
> - **#14 (HITL resume redesign)** -> Feature-Spec §2.4 (no `Ctrl+C`; `metamach-resume` + next step).
> - **#15 (16KB enforcement point)** -> Feature-Spec §4 (Daemon `absurd` module before `INSERT` = authoritative).
> - **#16 (Offboard LLM spec)** -> Feature-Spec §2.5 (`configs/offboard.toml`, prompt, fallback, timeout, async).
> Remaining open: #17 (Teams/Telegram 🟡) and other 🟡/⚪ items not in scope.

| Role | Name | Date |
|------|------|------|
| Architect | ___________ | 2026-07-15 |
| Factory Director | ___________ | 2026-07-15 |

> **Round 4 (🟡 items, 2026-07-15):** Remaining §6 🟡/⚪ items now RESOLVED:
> - **#4.1 / #17 (Teams vs Telegram)** -> Telegram primary, Teams secondary, abstract payload + adapters (Feature-Spec §2.4).
> - **#4.2 (SHELL absolute path)** -> Feature-Spec §2.2 uses `${HERDR_PLUGIN_ROOT}/bin/janus-sh`.
> - **#4.5 (joyrobots janus.toml)** -> Contract 3.6 notes `[remote]` optional for local-only blueprints.
> - **#4.7 (offboard requires Daemon)** -> ARCH §3 CLI note + Feature-Spec §2.5 execution-prerequisite note.
> - **#4.8 / #22 (state machine)** -> Feature-Spec §2.3 state-machine diagram.
> - **#4.9 / #23 (fallback.db schema)** -> Feature-Spec Contract 3.8.
> - **#5.1 (DB naming)** -> standardized to 'Absurd Postgres' / 'Absurd DB' across all source docs.
> - **#5.3 (CN/EN titles)** -> English subtitles added to all 7 spec docs.
> Remaining open: **#5.2** (metamach_db canonical - already consistent), **PRD-Review #5** (matrix reprioritization - deferred as a product-priority call, not a spec gap).
