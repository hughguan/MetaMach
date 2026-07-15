
### ── 以“可独立提交（Check-in）/ 物理并网”为业务单元的硅基工厂建设路线图

本计划书将 MetaMach 2.0 的研发与并网过程拆解为 **4 个核心里程碑阶段（Milestones）**。每个里程碑均以“**可编译、可独立 Commit/Check-in、100% 动静隔离合规**”的物理特性或功能模块（Feature Unit）为切分单位，并附带明确的物理验证手段，确保 Richmond Hill 车间的并网过程严丝合缝、稳步推进。

## 📅 建设路线图总览 (Milestone Timeline)

```
[Milestone 1] ──> [Milestone 2] ──> [Milestone 3] ──> [Milestone 4]
 基础设施与外壳      双生进程调度        代理沙箱与安全卫兵    生命周期与自愈
```

## 📐 Milestone 1: 基础设施并网与影子外壳 (Immutable & Base)

- **研发目标**：确立动静隔离目录，拉起 Unified Postgres 容器，跑通轻量级影子客户端 Popup 弹窗。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/herdr-plugin.toml`, `janus/src/bin/herdr_janus.rs`, `docker-compose.yml`, `Makefile`
    

### 🛠️ 任务分解 (Tasks)

#### Task 1.1: Unified Postgres 容器与 migrations 初始化 (Check-in Unit 1)

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
    

## 🧠 Milestone 2: 双生子进程 UDS 通信与调度大脑 (Daemon Core)

- **研发目标**：实现常驻后台守护进程 `janus-daemon`，建立双进程间的 UDS Socket 高速公路，实现惰性自启与单例锁。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/src/bin/janus_daemon.rs`, `janus/src/absurd/`
    

### 🛠️ 任务分解 (Tasks)

#### Task 2.1: `janus-daemon` 后台常驻服务与 UDS 握手 (Check-in Unit 3)

- **任务描述**：实现 `janus_daemon.rs` 并管理 `janus.sock` 物理套接字。
    
- **实现细节**：
    
    - Daemon 启动时在 `~/.local/state/.../janus.sock` 绑定 UDS 监听。
        
    - **单例文件锁 (PID Lock)**：在 `~/.local/state/.../janus.pid` 写入当前进程 PID。二次启动检测到该文件时，直接安全退出，防止重复抢占 UDS。
        
    - 当 UDS 收到请求时，向客户端（`herdr-janus`）发送 Mock 的 Blueprint 列表。
        

#### Task 2.2: 影子客户端 UDS 对账与“惰性启动” (Check-in Unit 4)

- **任务描述**：重构 `herdr_janus.rs`，使其连接 Daemon 交换数据，并具备无感自愈启动功能。
    
- **实现细节**：
    
    - 当厂长按下 `prefix+j` 唤醒客户端时，影子客户端首先探测 `janus.sock`。
        
    - 若探测失败，影子客户端在后台默默执行 `fork()` + `exec()` 拉起 `janus-daemon`，待其就绪后建立 UDS 连接，动态拉取产品数据渲染大盘。
        
- **UAT 物理验证**：手动物理杀死 `janus-daemon`，直接在 Herdr 内按下 `prefix+j`。弹窗应无延迟弹出，并在后台自动生成新的 `janus.pid`。
    

## 🛡️ Milestone 3: 物理沙箱、代理 Shell 与安全卫兵 (Shield Layer)

- **研发目标**：将安全门禁从 Herdr 进程外下放至 tmux 的物理边界内，实现 `janus-sh` 同步拦截和 Tool Guard 白名单过滤。
    
- **本阶段完成即可独立 Check-in 的物理目录结构**：
    
    `janus/src/tool_guard/`, `janus/target/release/janus-sh` (独立编译目标)
    

### 🛠️ 任务分解 (Tasks)

#### Task 3.1: 编译代理 Shell `janus-sh` (Check-in Unit 5)

- **任务描述**：用 Rust 实现轻量级系统命令同步代理。
    
- **实现细节**：
    
    - `janus-sh` 本身是一个极小的 CLI 程序。它被唤醒后不执行命令，而是将当前的 `argv` 数组通过 UDS 抛给 `janus-daemon`。
        
    - `janus-sh` 保持挂起（Blocked）状态，直到从 `janus-daemon` 收到核准（`ALLOW`）或篡改后（`REWRITE`）的指令再交付给真正的宿主 `/bin/sh` 执行。
        

#### Task 3.2: Tool Guard 内存规则引擎与 Teams 审批挂起 (Check-in Unit 6)

- **任务描述**：在 Daemon 中实现安全卫兵决策矩阵与非破坏性挂起。
    
- **实现细节**：
    
    - 若 Daemon 收到 `janus-sh` 抛来的命令（如未授权的网络下载、高危物理删除、金融实盘发单），比对 `configs/agents.toml` 的资质限制。
        
    - **非破坏性挂起 (Suspension)**：若为未授权高危指令，将状态标记为 `SUSPENDED`。Daemon 不杀底层 PTY，阻止 `janus-sh` 往下派发，同时通过 Teams Webhook 发送带有一键 `Resume` 的卡片。
        
- **UAT 物理验证**：在 Agent 窗格中强制运行 `rm -rf /`，终端应瞬间被同步挂起（Remain-on-Exit），手机端收到 Teams 审批卡片。
    

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
    

#### Task 4.2: 下线降解熔炼器 (Melt DB Cache) (Check-in Unit 8)

- **任务描述**：实现 `janus offboard` 指令。
    
- **实现细节**：
    
    - 下线时，自动扫描数据库，将该 Blueprint 历史上的 Steps 报错、Tool Guard 拦截日志打包。
        
    - 调用大模型将其总结为高密度的 Markdown，写入 `./blueprints/<name>/openwiki/production_report.md`。
        
    - **PG 自动降解 (Pruning)**：调用存储过程 `melt_blueprint_data`，彻底物理擦除主库对应 Steps 的 `result_cache` 大 JSON，仅保留元数据统计，实现数据库防爆收缩。
        
- **UAT 物理验证**：对积攒了大量编译日志的产品执行 `janus offboard --blueprint gatemetric`，本地成功 Commit 并推入 Git 远端一份 `production_report.md`，PG 数据库物理体积发生断崖式收缩。
    

## 🏁 交付质量门禁 (Check-in Gates)

为保证 MetaMach 2.0 仓库历史（Git History）的清爽，每次 Check-in 提交前必须通过以下 CI/CD 核验：

1. `cargo fmt --all -- --check` (100% 格式对齐)
    
2. `cargo clippy --all-targets -- -D warnings` (100% 静态安全检测无警告)
    
3. `cargo test --workspace` (100% 本地 fall-back DB 及事务单元测试通过)
    
4. 检查提交的文件，严禁将任何明文密钥、`.env` 文件、本地 `janus.sock` 误提交到 Git。