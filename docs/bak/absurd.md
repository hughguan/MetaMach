在 **MetaMach 0.4.0** 的架构体系（特别是 **MM-CORE 极简双子星** 与 **Rust 单二进制** 物理底座）下，集成 **Absurd** 是一个非常优雅且极其契合的过程。

Absurd 的核心理念是：**“不需要任何独立 coordinator 进程，仅仅靠一个 `absurd.sql` 写入 Postgres 存储过程，结合 Pull 模式完成 Durable Execution。”** 这与 MetaMach 0.4.0 追求的 **“去容器化、宿主机原生、安全稳定解耦”** 的工业美学完全同频。

以下是 **MetaMach 0.4.0 与 Absurd 的硬核物理集成方案**：

---

## 🪐 集成架构拓扑 (Integration Topology)

```
                            ┌──────────────────────────────────────┐
                            │      ~/.metamach/db/ (Host PG)       │
                            │  - Managed by janus-daemon           │
                            │  - absurd.sql schema loaded          │
                            └──────────────────┬───────────────────┘
                                               │
                                 ┌─────────────┴─────────────┐
                                 │  Absurd Stored Procedures │
                                 └─────────────┬─────────────┘
                                               ▲ (Pull / Task Claims)
                                               │
                                 ┌─────────────┴─────────────┐
                                 │   janus-daemon (Rust)     │
                                 │   Absurd Durable Worker   │
                                 └─────────────┬─────────────┘
                                               │ (State Machine Step Controls)
                                               ▼
                         ┌──────────────────────────────────────────┐
                         │   janus::tmux (Physical PTY Sandbox)     │
                         │   janush (Fail-Closed 30s Interceptor)   │
                         └──────────────────────────────────────────┘

```

---

## 🛠️ 1. 物理底座与 Schema 初始化 (Database Bootstrapping)

在 0.4.0 中，`janus-daemon` 启动时直接点火宿主机 PG。我们将 Absurd 的 SQL 规范嵌入到 Daemon 的自愈点火流程中：

1. **自动 Schema 注入**：
`janus-daemon` 首次拉起 `~/.metamach/db/` 宿主机 PG 实例后，利用 Rust 内置的 `include_str!("absurd.sql")` 直接向逻辑数据库（`metamach_blueprint_<name>`）执行 Migration：
```rust
// inside janus-daemon bootstrap logic
pub async fn init_absurd_schema(pg_pool: &PgPool) -> anyhow::Result<()> {
    let absurd_sql = include_str!("../sql/absurd.sql");
    sqlx::query(absurd_sql).execute(pg_pool).await?;
    tracing::info!("Absurd durable execution engine initialized in Postgres");
    Ok(())
}

```


2. **多蓝图物理隔离 (One PG, Multi-DB)**：
每个 Blueprint 拥有独立的数据库，Absurd 的队列（Queues）和步骤状态表（Step Checkpoints）完全隔离在各自的逻辑 DB 内，互不影响锁争用。

---

## ⚙️ 2. Rust 原生 Worker 封装与 Pull 模式集成

Absurd 采用 **Pull 模式**，这与 MetaMach 的异步 Reactor 完美匹配。在 `janus-daemon` 中，我们为每个 Blueprint 拉起一个底层的 Rust Task Worker：

### 📁 任务流定义 (Durable Step Mapping)

以一个典型的 **硬件编译与固件烧录（Flash Firmware）** 蓝图任务为例：

```rust
// janus-daemon 内部基于 Absurd 思想组织的 Rust 状态流
pub async fn execute_physical_deploy_workflow(ctx: WorkflowContext, params: DeployParams) -> Result<()> {
    // Step 1: 编译固件 (Checkpoint 1)
    let build_artifact = ctx.step("compile_firmware", || async {
        let output = janus_tmux::exec_command("cargo build --release").await?;
        Ok(output.artifact_path)
    }).await?;

    // Step 2: 触发安全拦截 (HITL Await Event)
    // 危险操作：向物理串口 /dev/ttyUSB0 写入！
    // 此时 Task 进入 Suspend 状态，完全不占用 CPU/内存
    janus_gateway::notify_teams_interception(&params).await?;
    
    // 挂起等待 Teams / TUI 的合闸 Event (first emit wins)
    let approval = ctx.await_event(&format!("hitl.approve:{}", params.run_id)).await?;

    if !approval.is_approved {
        return Err(anyhow::anyhow!("Human operator rejected execution"));
    }

    // Step 3: 执行物理烧录 (Checkpoint 2)
    let flash_result = ctx.step("flash_esp32", || async {
        let cmd = format!("esptool.py --port {} write_flash 0x0 {}", params.port, build_artifact);
        // 通过 janush + janus::tmux 物理下发
        janus_tmux::exec_command(&cmd).await
    }).await?;

    Ok(())
}

```

---

## 🛡️ 3. 应对 PG 崩溃：SQLite Fallback 与 Absurd 的双轨接龙

Absurd 极其依赖 PG。但 MetaMach 必须满足 **“SQLite 降级生存（Degraded Mode）”** 铁律。我们通过以下方式实现 Absurd 的双轨自愈：

```
[ Normal Mode ]   janus-daemon ──► Absurd SQL (PG) ──► Step Checkpoints Saved
                         │
                   (PG Crashes / OOM)
                         │
                         ▼
[ Degraded Mode ] janus-daemon ──► fallback.db (SQLite) ──► Log Events buffered as JSON
                         │
                   (PG Restored)
                         │
                         ▼
[ Replay & Merge ] fallback.db ──► Replay to Absurd PG ──► Step Checkpoints Restored

```

1. **正常状态**：`janus-daemon` 调用 Absurd 的 PG 存储过程进行 Task 抢占与 Step 提交。
2. **PG 闪退降级**：若 PG 崩溃，Daemon 捕获 DB 异常，**切至 SQLite `fallback.db**`。当前未落盘的 `Step` 被序列化为简单的 WAL 日志存入 SQLite。此时控制面降级运行，`janush` 代理仍然保活。
3. **恢复与 Replay**：PG 重启后，Daemon 执行 Replay 逻辑，将 SQLite 缓存的日志批量写回 Absurd 的 `absurd.sql` 状态表中，完成无缝接龙。

---

## 🌐 4. 与 `mach-gateway` (Teams / Hermes) 的 Event 联动

Absurd 支持 `awaitEvent`（事件挂起），这与我们 0.4.0 的 **Microsoft Teams 远程合闸网关** 是天然的绝配：

1. **拦截触发**：当 `janush` 拦截到高危命令，`janus-daemon` 向 Absurd 注册一个挂起等待事件：`ctx.awaitEvent("hitl.approve:run_4289")`。此时 Task 在 PG 中挂起。
2. **Teams 卡片派发**：`mach-gateway` 格式化推送 Teams Adaptive Card。
3. **人类合闸回传**：厂长在 Teams 手机端点击 **Approve**。
4. **触发 Event**：`mach-gateway` 调用 Absurd 的 `emitEvent("hitl.approve:run_4289", { approved: true })`。
5. **通电唤醒**：Absurd 在 PG 内部无冲突唤醒 Task，`janus-daemon` 驱动 `janus::tmux` 解冻，下发物理烧录指令。

---

## 📊 5. 极客运维：集成 `absurdctl` 与 `habitat`

由于 Absurd 提供了现成的 CLI 和 UI 工具（`habitat`），我们可以直接将它们挂载到 MetaMach 的管理工具链中：

* **运维指令集成**：
在 MetaMach 的 `make status` 或 `metamachctl` 中直接打包调用：
```bash
# 检查当前 Blueprint 数据库中的 Absurd 任务状态
absurdctl inspect-queue -d metamach_blueprint_default

```


* **可视化 Debug (Habitat GUI)**：
在开发重型蓝图时，厂长可以可选地拉起 `habitat` Web UI，直观查看当前处于 `suspended`（等待 Teams 合闸）、`running` 或 `failed` 的 Task 时序图。

---

## 🏁 总结

将 Absurd 集成进 MetaMach 0.4.0：

* **不需要** 引入 Temporal、Inngest 等庞大的 Java/Go 服务堆栈；
* **只需要** 把 `absurd.sql` 刷入我们 `~/.metamach/db/` 宿主机 PG；
* **完全契合** MetaMach **“安全（Fail-Closed 事件挂起）、稳定（PG+SQLite 双轨）、解耦（Pull 模式）、复用（极简 SQL）”** 的四大顶层柱石！


关于维护：这是一个非常敏锐且硬核的工程维护问题。

在 MetaMach 0.4.0 的 **“单二进制、去容器化、Bare-metal 物理落盘”** 交付标准下，我们**绝不能**把 Absurd 当作一个运行时通过 `curl` 或 `git clone` 动态拉取的“外部不确定依赖（External Dynamic Dependency）”。

如果 Absurd 上游更新破坏了 API，或者 Richmond Hill 车间突然断网，动态拉取会导致系统的 Fail-Closed 安全红线瞬间崩溃。

针对如何引入依赖以及如何优雅维护 Absurd 上游更新对 MetaMach 的影响，以下是最佳工程实践方案：

---

## 📦 一、 依赖引入策略：Vendoring（代码内嵌）而非 Submodule

**结论：不要增加 GitHub Submodule，不要增加运行时 External Dep。采用 **`Vendoring`（静态代码内嵌）**。**

把 Absurd 的核心 `absurd.sql` 物理复制到 MetaMach 仓库的 `crates/janus-daemon/src/sql/absurd.sql` 中，并通过 Rust 的 `include_str!` 编译期宏直接**编译进 `janus-daemon` 的二进制文件内部**。

### 为什么这样做最符合 0.4.0 的审美？

1. **零运行时网络依赖（Zero Runtime Network Dependency）**：
在车间离线/断网环境下，`janus-daemon` 拉起宿主机 PG 后，直接在内存中读取编译期内嵌的 `absurd.sql` 字符串，微秒级完成数据库 Schema 初始化。
2. **构建确定性（Deterministic Builds）**：
`Cargo.lock` 和代码仓库完全锁定该 SQL 的 Hash 值，彻底杜绝“上游突然发版导致本地构建 break”的风险。

---

## 🛠️ 二、 维护 Absurd 上游更新的三重防爆机制

为了既能享受 Absurd 社区的升级红利，又不会让上游变更动摇 MetaMach 的稳定性，我们需要建立以下 **自动化对账与更新机制**：

### 1. 编译期 Schema 版本校验（Schema Version Locking）

Absurd 内部维护了 `absurdctl schema-version` 或内部版本表。在 `janus-daemon` 初始化点火时，必须校验当前 DB 的 Schema 版本：

```rust
// crates/janus-daemon/src/db/absurd.rs

pub const EXPECTED_ABSURD_VERSION: i32 = 4; // 锁定 MetaMach 当前兼容的 Absurd Schema 版本

pub async fn verify_and_migrate(pool: &PgPool) -> anyhow::Result<()> {
    let current_version = get_absurd_schema_version(pool).await?;
    
    if current_version < EXPECTED_ABSURD_VERSION {
        tracing::info!("Migrating Absurd schema from v{} to v{}", current_version, EXPECTED_ABSURD_VERSION);
        apply_embedded_migration(pool, current_version).await?;
    } else if current_version > EXPECTED_ABSURD_VERSION {
        anyhow::bail!("Database Absurd schema (v{}) is newer than janus-daemon binary (v{}). Please update MetaMach!", 
            current_version, EXPECTED_ABSURD_VERSION);
    }
    Ok(())
}

```

### 2. GitHub Actions 自动化上游追溯与断言测试（Upstream Watcher Pipeline）

在 MetaMach 的 GitHub 仓库中，配置一个专门的 CI Workflow（例如 `.github/workflows/upstream-absurd-check.yml`），每周自动化巡检上游 `earendil-works/absurd` 的 Release：

```yaml
name: Check Absurd Upstream
on:
  schedule:
    - cron: '0 0 * * 0' # 每周日运行
  workflow_dispatch:

jobs:
  check-upstream:
    runs-step:
      - uses: actions/checkout@v4
      - name: Fetch Latest Absurd Release
        run: |
          LATEST_TAG=$(curl -s https://api.github.com/repos/earendil-works/absurd/releases/latest | jq -r .tag_name)
          # 比对本地 vendored absurd.sql 的 Tag
          # 如果有新 Release，自动提交一个 Draft PR 并运行 MetaMach 的全套 Fail-Closed 集成测试！

```

### 3. Rust SDK 接口解耦隔离层 (The Absurd Abstraction Layer)

Absurd 目前提供 TS、Python 和 Go 的官方 SDK，**但 Rust SDK 还在 Bootstrap/社区阶段**。
这反而成为了 MetaMach 的优势！我们不需要直接依赖 Absurd 可能会变动的 Client 代码，而是自己在 `janus-daemon` 内部实现一层极其轻量的 Rust Adapter（直接用 `sqlx` 调用 Absurd 的 Postgres Stored Procedures）：

```
 【 janus-daemon 核心业务逻辑 】
               │
               ▼  (调用 MetaMach 定义的抽象接口)
 【 trait DurableEngine 】
               │
               ▼  (隔离层：直接封装 SQL 函数)
 【 AbsurdPgAdapter 】
   - SELECT absurd.spawn_task(...)
   - SELECT absurd.await_event(...)
               │
               ▼
 【 宿主机 Postgres DB (`absurd.sql`) 】

```

**这种隔离层的物理收益**：
即使 Absurd 更改了它的 Client SDK 函数签名，只要底层 Postgres 存储过程（Stored Procedures）的 SQL 接口保持兼容，MetaMach 的核心代码**一行都不需要改**！

---

## 🏁 总结对账清单

| 维度 | 方案 | 物理收益 |
| --- | --- | --- |
| **依赖方式** | **Vendoring**（将 `absurd.sql` 编译进 `janus-daemon`） | 离线可用，零网络依赖，单二进制打包分发。 |
| **版本控制** | **Hardcoded Version Guard**（DB 与 Binary 强对账） | 防止数据库与 Daemon 二进制版本错配导致的运行期崩溃。 |
| **上游同步** | **CI Upstream Bot**（每周定时抓取并跑单元测试） | 自动化接收社区 Bug 修复，上游 Breaking Change 提前预警。 |
| **代码解耦** | **Self-implemented `sqlx` Adapter** | 不依赖不稳定的第三方 Rust SDK，直接对接 SQL 存储过程，锁死底层契约。 |

通过这套机制，Absurd 对 MetaMach 来说就像 `sqlite3.c` 对很多 C/Rust 项目一样——它是一个**静态编译进来的、极度稳定的持久化引擎**，既享受了开源社区的演进，又绝不会给 MetaMach 的 0.4.0 生产环境带来一丝不确定性！

---

## Revision History

> **2026-07-21 — English translation and cross-check.** This Chinese original was reviewed, translated, and revised. The authoritative English version is `docs/Absurd-Integration.md` (290 lines). Key updates:

| # | Topic | Original (ZH) | Revised (EN) |
|---|---|---|---|
| 1 | Dependency model | External fetch | **Vendored** — `absurd.sql` compiled into daemon binary (ADR-015) |
| 2 | Gateway naming | `mach-gateway` | `janus::gateway` |
| 3 | Crate path | `crates/janus-daemon/` | `janus/` |
| 4 | Schema location | `src/sql/absurd.sql` | `janus/sql/absurd.sql` (v0.4.0, commit `9b77b35`) |
| 5 | CLI naming | `metamachctl` | `janus` |
| 6 | Absurd repo anatomy | Not covered | Added: what each component does, what MetaMach needs (vendored vs external vs not needed) |
| 7 | SDK rationale | Not covered | Added: why Go/Python/TS SDKs are not needed (Rust sqlx adapter) |
| 8 | Upgrade path | Not covered | Added: vendoring migration scripts on version upgrade |
| 9 | Cross-check table | Not present | 12-item cross-check against ARCH.md, ADR.md, Feature-Spec.md — all resolved |

See `docs/Absurd-Integration.md` for the full revised English version.
