
### ── 以“可独立提交（Check-in）/ 物理并网”为业务单元的硅基工厂建设路线图

> **EN:** Project Plan — milestone roadmap (M0–M4) of independently check-in-able, network-able factory units.

本计划书将 MetaMach 1.0 的研发与并网过程拆解为 **4 个核心里程碑阶段（Milestones）**。每个里程碑均以“**可编译、可独立 Commit/Check-in、100% 动静隔离合规**”的物理特性或功能模块（Feature Unit）为切分单位，并附带明确的物理验证手段，确保 Richmond Hill 车间的并网过程严丝合缝、稳步推进。

## 📅 建设路线图总览 (Milestone Timeline)

```
[Milestone 0] ──> [Milestone 1] ──> [Milestone 2] ──> [Milestone 3] ──> [Milestone 4]
 Herdr v1 验证      基础设施与外壳      双生进程调度        代理沙箱与安全卫兵    生命周期与自愈
```

> **M0 为前置门禁**：M1 起的所有 Popup/插件任务都依赖 Herdr v1 插件 SDK 可用。M0 必须先验证该外部契约，否则 M1 Task 1.2 即被阻塞。
>
> **预估工期（粗粒度，单人/小团队，视规模调整）**：M0 ≈ 3 天 · M1 ≈ 2 周 · M2 ≈ 3 周 · M3 ≈ 3 周 · M4 ≈ 4 周。

## 🧪 Milestone 0: Herdr v1 插件契约验证 (External SDK Validation)

- **研发目标**：在投入任何 MetaMach 自研代码前，先验证 Herdr v1 插件 SDK 真实可用，消除 M1 的最大未知外部依赖。M0 不产出 MetaMach 业务代码，仅产出“契约验证证据 + 最小 PoC 插件 + Herdr v1 API 接口备忘”。

- **本阶段完成即可独立 Check-in 的物理目录结构**：

    `docs/CH/herdr-v1-contract.md`（接口备忘）, `spike/herdr-hello-plugin/`（PoC 插件，gitignore）

### 🛠️ 任务分解 (Tasks)

#### Task 0.1: Herdr v1 安装与插件 SDK 可用性验证 (Check-in Unit 0a)

- **任务描述**：安装 Herdr v1 并验证插件加载链路端到端可用。

- **实现细节**：

    - 安装 Herdr v1，执行 `herdr plugin link` 能成功挂载一个插件目录。
        
    - 验证 `prefix+j` 键绑定能派发到已挂载插件，且 `herdr-plugin.toml` 可被解析。
        
    - 验证 `placement = "popup"`、`width`、`height` 为 Herdr v1 合法指令并真实生效。
        

- **UAT 物理验证**：挂载一个空壳插件，按下 `prefix+j` 后 Herdr 真实弹出指定尺寸的 Popup 窗口；记录 Herdr v1 实际 API 表面（事件钩子、UDS 约定、生命周期回调）写入 `docs/CH/herdr-v1-contract.md`。

#### Task 0.2: 最小 Popup PoC 插件 (Check-in Unit 0b)

- **任务描述**：用 Herdr v1 SDK 实现一个“Hello World”Popup 插件，验证 MetaMach 后续所需的全部交互原语。

- **实现细节**：

    - PoC 插件能渲染一个 `ratatui` Popup，接管键盘焦点，按 `Esc` 安全退栈。
        
    - 验证 Popup 内能通过 UDS 与一个后台进程通信（为 M2 的 `herdr-janus` <-> `janus-daemon` 通路打样）。
        

- **UAT 物理验证**：PoC 插件按下 `prefix+j` 弹出、键盘焦点不逃逸、`Esc` 关闭、UDS 通信往返成功。若任何一项失败，M1 暂不启动，先与 Herdr v1 上游对齐契约。

## 📐 Milestone 1: 基础设施并网与影子外壳 (Immutable & Base)

- **研发目标**：确立动静隔离目录，拉起 Absurd Postgres 容器，跑通轻量级影子客户端 Popup 弹窗。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/herdr-plugin.toml`, `janus/src/bin/herdr_janus.rs`, `docker-compose.yml`, `Makefile`
    

### 🛠️ 任务分解 (Tasks)

#### Task 1.1: Absurd Postgres 容器与 migrations 初始化 (Check-in Unit 1)

- **任务描述**：编写并提交 `docker-compose.yml` 及 `janus/migrations/`。
    
- **实现细节**：
    
    - 在 Postgres 中建立 `metamach_db`，并编写 `001_init_absurd.sql` 初始化表结构（`blueprints`, `absurd_tasks`, `absurd_steps`）。
        
    - 配置容器在启动时自动挂载并执行 migrations 脚本。
        
- **UAT 物理验证**：运行 `docker compose up -d`，登录容器内执行 `\dt` 应该能看到初始化完成的所有数据库物理表。
    

#### Task 1.2: 影子外壳 Popup 弹窗与 TUI 渲染 (Check-in Unit 2)

- **任务描述**：编写 `herdr-plugin.toml` 并实现 `herdr_janus.rs` 影子客户端。
    
- **实现细节**：
    
    - 在插件配置中声明 `placement = "popup"`, `width = "80%"`, `height = 20`。
        
    - 在 `herdr_janus.rs` 中利用 `ratatui` 渲染一个静态的“生产排班大盘”交互界面。焦点自动锁定，按 `Esc` 退出。
        
- **UAT 物理验证**：执行 `herdr plugin link` 挂载插件，在 Herdr 中按下 `prefix+j`，屏幕中央应流畅弹出 80% 宽度的浮动 Popup。
    

#### Task 1.3: 配置模板创建与 schema 校验 (Check-in Unit 2b)

- **任务描述**：创建并校验全部配置文件模板，消除“配置被假设存在”的隐患。
    
- **实现细节**：
    
    - 创建 `configs/agents.toml`（Contract 3.5）、`configs/tmux.conf`（per-session remain-on-exit）、`configs/global_rules.md`、`blueprints/*/janus.toml`（Contract 3.6）、`workflows/*.toml`（Contract 3.7）模板。
        
    - 编写 schema 校验脚本；`make bootstrap` 在 `ln -sf` 前校验源文件存在，避免 broken symlink。
        
- **UAT 物理验证**：`make bootstrap` 后各配置软链接有效、schema 校验通过、无 broken symlink。
    

#### Task 1.4: GitHub Actions CI 流水线 (Check-in Unit 2c)

- **任务描述**：配置 `.github/workflows/build-janus.yml`，push/PR 到 main 时自动跑 fmt + clippy + test。
    
- **实现细节**：
    
    - CI 步骤 = `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test --workspace`；缓存 cargo registry/target 加速。
        
- **UAT 物理验证**：PR 触发 CI 全绿方可合并；缓存命中率可见。
    

## 🧠 Milestone 2: 双生子进程 UDS 通信与调度大脑 (Daemon Core)

- **研发目标**：实现常驻后台守护进程 `janus-daemon`，建立双进程间的 UDS Socket 高速公路，实现惰性自启与单例锁。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/src/bin/janus_daemon.rs`, `janus/src/absurd/`
    

### 🛠️ 任务分解 (Tasks)

#### Task 2.1: `janus-daemon` 后台常驻服务与 UDS 握手 (Check-in Unit 3)

- **任务描述**：实现 `janus_daemon.rs` 并管理 `janus.sock` 物理套接字。
    
- **实现细节**：
    
    - Daemon 启动时在 `~/.local/state/.../janus.sock` 绑定 UDS 监听。
        
    - **单例文件锁 (PID Lock) + 陈旧检测**：在 `~/.local/state/.../janus.pid` 写入当前进程 PID。二次启动检测到该文件时，**先读取其中的 PID 并校验该进程是否仍存活且确为 `janus-daemon`**：若进程已不存在（崩溃后残留的陈旧 PID 文件），则覆盖该文件并正常启动；若 PID 仍存活，则安全退出，防止重复抢占 UDS。文件内容非法（非数字 PID）时记 `WARN` 并覆盖。
        
    - 当 UDS 收到请求时，Daemon 查询 Absurd Postgres 的 `blueprints` 表，向客户端（`herdr-janus`）返回所有 `status = 'ACTIVE'` 的真实蓝图列表（不再使用 Mock 数据；M1 已建表，可由 migration 种子或 M4 的 `janus onboard` 写入）。
        

#### Task 2.2: 影子客户端 UDS 对账与“惰性启动” (Check-in Unit 4)

- **任务描述**：重构 `herdr_janus.rs`，使其连接 Daemon 交换数据，并具备无感自愈启动功能。
    
- **实现细节**：
    
    - 当厂长按下 `prefix+j` 唤醒客户端时，影子客户端首先探测 `janus.sock`。
        
    - 若探测失败，影子客户端在后台默默以 `std::process::Command::spawn()` 拉起 `janus-daemon` 并 detach（`setsid` 脱离控制终端、标准流重定向至 `/dev/null`，见 Feature-Spec §2.1），待其就绪后建立 UDS 连接，动态拉取产品数据渲染大盘。
        
- **UAT 物理验证**：手动物理杀死 `janus-daemon`，直接在 Herdr 内按下 `prefix+j`。弹窗应无延迟弹出，并在后台自动生成新的 `janus.pid`。
    

#### Task 2.3: 工作流进度查询与大盘渲染 (Check-in Unit 4b)

- **任务描述**：在 Daemon 侧实现只读 `progress` 查询原语，在 `herdr-janus` 侧为 Popup 新增“进度 (Progress)”视图与 `janus status` CLI。

- **实现细节**：
    
    - Daemon 暴露 `progress` UDS 查询：以只读事务聚合 `absurd_tasks JOIN absurd_steps`（过滤非终结态），叠加 Tether `tmux has-session` 存活信号，返回 Contract 3.3 定义的 Payload。该查询走独立只读通道，不与工作流写事务争用。
        
    - `herdr-janus` Popup 新增 Progress 视图，`Tab` 键在“派单 / 进度”间切换；进度视图以 1–2s 节拍轮询 `progress` 并用 `ratatui` 表格按蓝图分组渲染。`SUSPENDED` 工位高亮并附 `[A]ttach` / `[R]esume` 入口。
        
    - 实现 `janus status [--blueprint <name>] [--json]` CLI，复用同一 `progress` 原语输出纯文本/JSON 快照。
        
- **UAT 物理验证**：派发一个多工位流水线后切换到进度视图，工位状态应在 2s 内随真实执行推进（`PENDING -> RUNNING -> COMPLETED`）；人为触发 `SUSPENDED` 后该行 1s 内高亮。SSH 环境执行 `janus status` 应输出与大盘一致的在途任务快照。

> 注：本任务在 M2 即落地查询与渲染骨架，其所读取的 Task/Step 真实数据随 M3（janus-sh 工位执行）、M4（跨主机工作流）逐步丰满；M2 阶段可用 migration 种子任务验证渲染。

#### Task 2.4: Tether Engine 外部依赖拉取与本地会话验证 (Check-in Unit 4c)

- **任务描述**：将外部依赖 `herdr-tether`（https://github.com/moneycaringcoder/herdr-tether）纳入 `make bootstrap` 拉取/构建流程，并验证本地 tmux 会话原语可用，为 M3（janus-sh 在 Tether 窗格内测试）与 M4（跨主机）提前就位。

- **实现细节**：
    
    - 在 `make bootstrap` 中增加 `tether` 目标：通过 git submodule 或 cargo git 依赖拉取 `herdr-tether` 源码并 `cargo build --release`，产物安装到 `${HERDR_PLUGIN_ROOT}/bin/herdr-tether`。
        
    - 验证 `herdr-tether open --command "sleep 100"` 能在独立 tmux server（`tmux -L metamach-tether`）中创建持久会话，并 per-session 注入 `remain-on-exit on`（不用 `-g` 全局开关，不污染厂长本机 tmux）。
        
    - 验证 `herdr-tether attach` 能秒级挂接并存留现场。
        

- **UAT 物理验证**：`make bootstrap` 后 `herdr-tether open --command "sleep 100"` 拉起会话；强行关闭前台视图，`tmux list-sessions` 仍见 `tether-janus-*` 会话活跃；`herdr-tether attach` 现场毫秒级还原。

#### Task 2.5: OpenWiki 外部依赖拉取与 RAG 查询验证 (Check-in Unit 4d)

- **任务描述**：将外部依赖 OpenWiki（https://github.com/langchain-ai/openwiki）纳入构建流程，并打通 Daemon -> OpenWiki 的 RAG 查询链路，为 M4 Offboard 写回与 Agent 进场检索提前就位。

- **实现细节**：
    
    - `make bootstrap` 增加 `openwiki` 目标：拉取/构建 OpenWiki 引擎，配置 `blueprints/<name>/openwiki/` 与全局 `configs/global_rules.md` 的索引范围。
        
    - Daemon 实现 `openwiki_query` 旁路：Agent 遇代码盲区时发起 RAG 检索，Daemon 优先命中 Absurd Postgres 级缓存（Git-SHA 去重），未命中再查 OpenWiki 引擎。
        
    - 验证索引范围隔离：不同蓝图的局部脑图互不串扰。
        

- **UAT 物理验证**：为一个蓝图索引其 `openwiki/` 后，`openwiki_query` 能返回精准 AST 片段；跨蓝图查询结果不串扰。Offboard 写回 `production_report.md` 后，重新索引能被检索命中（与 M4 Task 4.2/4.3 闭环）。

## 🛡️ Milestone 3: 物理沙箱、代理 Shell 与安全卫兵 (Shield Layer)

- **研发目标**：将安全门禁从 Herdr 进程外下放至 tmux 的物理边界内，实现 `janus-sh` 同步拦截和 Tool Guard 白名单过滤。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/src/tool_guard/`, `janus/target/release/janus-sh` (独立编译目标)
    

### 🛠️ 任务分解 (Tasks)

#### Task 3.1: 编译代理 Shell `janus-sh` (Check-in Unit 5)

- **任务描述**：用 Rust 实现轻量级系统命令同步代理。
    
- **实现细节**：
    
    - `janus-sh` 本身是一个极小的 CLI 程序。它被唤醒后不执行命令，而是将当前的 `argv` 数组通过 UDS 抛给 `janus-daemon`。
        
    - `janus-sh` 保持挂起（Blocked）状态，直到从 `janus-daemon` 收到 `ALLOW`/`REWRITE`/`BLOCK` 裁决（见 Contract 3.4）再决定交付宿主 `/bin/sh` 执行或返回错误。
        
    - **依赖就位（解决依赖倒置）**：本任务的 UAT 须在真实 Tether 窗格内验证 `SHELL=janus-sh`。该依赖由 **M2 Task 2.4（Tether Engine）** 提前并网、M2 末即就位，故 M3 不再出现「janus-sh 已编译却无 Agent 窗格可测」的依赖倒置。
        
- **UAT 物理验证**：在 Tether 拉起的窗格中 `echo $SHELL` 应为 `janus-sh`；经 `janus-sh` 执行命令时同步阻塞于 UDS 对账，收到 `ALLOW`/`REWRITE` 方交付宿主 Shell，`BLOCK` 则返回错误且不执行。
    

#### Task 3.2: Tool Guard 内存规则引擎与 Teams 审批挂起 (Check-in Unit 6)

- **任务描述**：在 Daemon 中实现安全卫兵决策矩阵与非破坏性挂起。
    
- **实现细节**：
    
    - 若 Daemon 收到 `janus-sh` 抛来的命令（如未授权的网络下载、高危物理删除、金融实盘发单），比对 `configs/agents.toml` 的资质限制。
        
    - **非破坏性挂起 (Suspension)**：若为未授权高危指令，将状态标记为 `SUSPENDED`。Daemon 不杀底层 PTY，阻止 `janus-sh` 往下派发，同时通过 Teams Webhook 发送带有一键 `Resume` 的卡片。
        
- **UAT 物理验证**：在 Agent 窗格中先建哨兵 `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`，再强制运行命中黑名单的 `rm -rf /tmp/metamach-test-guard-$(uuidgen)`，终端应瞬间被同步挂起（Remain-on-Exit）、哨兵存活，手机端收到 Teams 审批卡片。
    

## 📈 Milestone 4: 跨主机耐久、冷自愈与下线熔炼 (Advanced & Prune)

- **研发目标**：并网 Tether 跨主机 tmux、实现冷启动零状态自愈（放弃 tmux-resurrect）和下线（Offboard）降解熔炼。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `workflows/`, `blueprints/` (产品配方), `janus/src/bin/janus_daemon.rs` (补充 GC / Onboard / Offboard 子模块)
    

### 🛠️ 任务分解 (Tasks)

#### Task 4.1: 跨主机 Tether tmux 驱动与冷启动自愈 (Check-in Unit 7)

- **任务描述**：实现跨主机的 SOP 会话驱动，以及断电开机自愈。
    
- **实现细节**：
    
    - 在 Workflow 执行到下一个 Step 时，若声明了远程编译服务器，Daemon 自动调用本地的 `herdr-tether` 通过 SSH 将 Payload 环境变量注入远程。
        
    - **冷启动对账**：Daemon 启动后检索 Absurd Postgres，若有未完结任务（`RUNNING`），直接读取最后一次 `COMPLETED` 的 `result_cache` JSON。重新指派新的 Tether Session UUID 并在后台重新跑任务，断点处无缝接龙。
        
- **UAT 物理验证**：运行重型编译时人为 `docker compose stop` PG 并杀死 Daemon。重启后 Daemon 应在 0.5s 内从数据库 Step 缓存中重构现场无感接棒。
    

#### Task 4.2a: 数据库降解存储过程 (melt_blueprint_data) (Check-in Unit 8a)

- **任务描述**：实现 `melt_blueprint_data('<name>')` SQL 存储过程 + Daemon 集成（纯 DB 层，无外部依赖，可独立 Check-in）。
    
- **实现细节**：
    
    - 存储过程 **整行 DELETE** 该蓝图所有 Steps 的 `result_cache` JSON 大字段与 Stdout 日志（整行删除而非 NULL，以便释放 TOAST 空间），仅保留一行审计元数据统计（Task ID、执行耗时）写入独立的 `absurd_audit_log` 表。
        
    - Daemon `offboard` 路径调用该过程；调用后可 `VACUUM FULL` 回收磁盘。
        

- **UAT 物理验证**：执行 `melt_blueprint_data('gatemetric')` 后，该蓝图 `absurd_steps.result_cache` 行已删除/均为 NULL，`VACUUM FULL` 后磁盘占用断崖式收缩。
    

#### Task 4.2b: Offboard 编排器与 LLM 熔炼 (Check-in Unit 8b)

- **任务描述**：实现 `janus offboard` 编排：扫描历史 Steps -> LLM 总结 -> 写 `production_report.md`。
    
- **实现细节**：
    
    - 扫描数据库，抽取该蓝图历史 Steps 报错、Tool Guard 拦截日志打包。
        
    - 按 Feature-Spec §2.5「Offboard LLM 集成规格」调用配置化大模型（`configs/offboard.toml`），总结为高密度 Markdown 写入 `./blueprints/<name>/openwiki/production_report.md`；LLM 不可用或超时降级写 `production_report.raw.json`。
        
    - 编排完成后触发 Task 4.2a 的 melt 与 Task 4.2c 的 Git 提交。
        

- **UAT 物理验证**：对积攒大量编译日志的产品执行 `janus offboard --blueprint gatemetric`，本地生成结构化 `production_report.md`（含四区块）；LLM 不可用时回退为 `.raw.json`。
    

#### Task 4.2c: 质检报告 Git 自动遗传 (Check-in Unit 8c)

- **任务描述**：将 `production_report.md` 增量 Commit 并 Push 至 Git 远端。
    
- **实现细节**：
    
    - 通过影子客户端调用本地 Git 对 `production_report.md` 做增量 Commit 与 Push，完成硅基经验自我遗传。
        
    - Push 失败不阻塞下线，记 `WARN` 并保留本地提交待后续重试。
        

- **UAT 物理验证**：Offboard 后本地成功 `git commit` 并推入 Git 远端一份 `production_report.md`。
    

#### Task 4.3: 蓝图上线与租户注册 (Onboard) (Check-in Unit 8d)

- **任务描述**：实现 `janus onboard --blueprint <name>` 指令，补齐与 Offboard 对称的上线侧生命周期闭环。

- **实现细节**：
    
    - 读取并校验 `blueprints/<name>/janus.toml`（必填字段 + `workflows/<default_workflow>.toml` 存在性），校验失败明确报错且不写库。
        
    - 执行点火前自检：Absurd Postgres 可达、tmux 就位；跨主机蓝图对 `[remote].host` 做尽力 SSH 连通性探测（不可达仅 `WARN`）。
        
    - **幂等租户注册**：`INSERT … ON CONFLICT (name) DO UPDATE` 写入 `blueprints` 行（`status='ACTIVE'`、`config`、`openwiki_scope`、`remote_host`、`onboarded_at`）。重新上线已 `OFFBOARDED` 蓝图即重新激活。
        
    - **脑图载入与经验遗传**：索引 `blueprints/<name>/openwiki/`；若存在上一代 `production_report.md`，解析其结构化区块并以 `## Previous Incidents` 少样本注入该蓝图 Agent System Prompt 模板。
        
    - 上线就绪后通过 UDS 广播 `blueprint_registered` 事件，Popup 派单菜单即时刷新。
        
- **UAT 物理验证**：对零产品线的干净车间执行 `janus onboard --blueprint joyrobots`，`blueprints` 表出现一行 `ACTIVE` 记录且 Popup 菜单即时出现该产品；重复执行无副作用（幂等）。对一个已 Offboard 的蓝图重新 Onboard，验证其 `production_report.md` 被回收进新一代 Agent 的 System Prompt。

#### Task 4.4: 自动化集成测试套件与运维文档 (Check-in Unit 8e)

- **任务描述**：将 Test-Spec 的 UTC-01..07 自动化为脚本，并补齐运维 runbook 与 UDS 协议文档。
    
- **实现细节**：
    
    - 实现 `docker-compose.test.yml`（`metamach-test` + `metamach-db-test`）与 `make test-integration`，将全部 UTC 跑在容器内、无裸金属依赖。
        
    - 编写 `docs/CH/runbook.md`（启停 / 备份恢复 / 常见故障排查）与 `docs/CH/uds-protocol.md`（UDS 请求/响应契约，引用 Contract 3.2/3.4）。
        
- **UAT 物理验证**：`make test-integration` 全绿；runbook 覆盖启停、备份恢复、降级模式处置。
    

## 🏁 交付质量门禁 (Check-in Gates)

为保证 MetaMach 1.0 仓库历史（Git History）的清爽，每次 Check-in 提交前必须通过以下 CI/CD 核验：

1. `cargo fmt --all -- --check` (100% 格式对齐)
    
2. `cargo clippy --all-targets -- -D warnings` (100% 静态安全检测无警告)
    
3. `cargo test --workspace` (100% 本地 fall-back DB 及事务单元测试通过)
    
4. 检查提交的文件，严禁将任何明文密钥、`.env` 文件、本地 `janus.sock` 误提交到 Git。
    
5. **回归核验**：所有先前里程碑的 UAT 物理验证仍须通过--新 Check-in 不得破坏既有功能（防止 M4 回归击穿 M1）。