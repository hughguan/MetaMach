
### ── 基于 Janus Daemon 与分布式耐久会话的硅基工业级生产机床

## 1. 宏观设计理念 (Philosophical Pillars)

在分布式 AI 协同研发时代，传统的 AI 编程或 Agent 调度多属于“无状态单次调用”。面对长周期、重负载、多工位以及跨物理主机的研发场景，系统极易因网络抖动、API 熔断或上下文丢失而发生进程崩溃，导致开发心流支离破碎。

**MetaMach 2.0** 彻底颠覆了这种脆弱的拓扑。它采用 **“守护进程为主体，影子插件为外壳”** 的高凝聚松耦合架构，将整套系统解耦为 **生产要素（Agent Pool）**、**工艺流水线（Workflows）** 与 **产品蓝图（Blueprints）**：

- **大脑独立化 (Janus Daemon) ── 魂归中枢：** 核心控制流与状态转移完全由常驻后台的 **`janus-daemon`** 掌控，独占数据库连接池与事件监听网关。Herdr 端的插件仅作为轻量级影子客户端（`herdr-janus`），专门负责终端渲染与交互。
    
- **跨主机耐久性 (Cross-Host Session Durability) ── 骨肉不离：** 结合 **Tether Engine (Tmux/SSH)** 抹平网络边界。底层的物理进程現場由原生的 `remain-on-exit` tmux 会话锁死，哪怕物理网络或 SSH 中断也绝对不断线，随时挂接还原现场。
    
- **流程自愈性 (Durable Workflows & HITL) ── 韧性闭环：** 工作流状态不随单次执行成败而折损。当 AI 遇到无法攻克的阻碍（如编译报错、安全超权）时，流水线在断点处自动挂起，保留终端现场并引入**人类合闸（Human-in-the-Loop）**。修复后一键 Resume，无缝接力。
    

## 2. 核心功能规格 (Feature Specifications)

### 2.1 智能体中央行政官：Janus Daemon

**Janus Daemon** 是整个生态的认知大脑，作为系统唯一的数据读写与调度中枢独立常驻运行：

- **Absurd 事务对账：** 在每一次 Step 开始前必须在 **Absurd Postgres** 中声明过渡态（如 `STARTING` / `STOPPING`），并在成功后原子化提交 `result_cache` JSON Payload，保证流程在任何灾难重启后具备幂等自愈能力。
    
- **代理安全沙箱 (janus-sh)：** 放弃在 Herdr 进程内进行异步拦截的空想。Janus 将底层的 `SHELL` 指向定制的代理 Shell `janus-sh`。任何 Agent 试图在 tmux 中执行命令时，必须通过 UDS Socket 向 Janus Daemon 发起同步对账，通过 **Event-Driven Tool Guard** 内存级审查后方可安全执行。
    

### 2.2 软件工厂三维定制 (Three-Dimensional Customization)

#### 👥 A. 生产要素 (Agent Pool & Stack)

所有的 AI 资源与安全权限由工厂主体在全局统一注册管理：

- **凭证沙箱化：** 所有的 API Keys、SSH 密钥统一在 `/dev/shm` 内存盘挂载并解密，绝对不污染代码仓库。
    
- **岗位资质限制：** 针对不同岗位的 Agent（如扫描代码的 `Scout`、修改补丁的 `Coder`、跨端烧录的 `Deployer`），定制其专属的模型选型与 Toolset 限制（Permission Level）。
    

#### ⚙️ B. 工艺流水线 (Workflows)

通过声明式配置文件，定制高内聚、高复用的装配线：

- **多工位串联：** 声明每一个 Step 执行哪类 Agent（如 `run_agent(scout)` ➡️ `run_agent(coder)` ➡️ `run_test`）。
    
- **跨主机部署：** 支持声明物理机器环境。流水线可以在 Step 1 在本地主机调度 Agent 修改代码，在 Step 2 自动通过 Tether 将指令通过 OpenSSH 输送至远程编译服务器上跑重型编译。
    

#### 📐 C. 产品蓝图 (Blueprints)

产品线存放于 `blueprints/` 下，保持绝对的物理清爽：

- **专属配方 (janus.toml)：** 绑定默认的流水线、声明 OpenWiki 联邦脑图的索引范围、配置远程 SSH 靶机 IP。
    
- **上线/下线机制 (On/Offboarding)：**

    - _上线 (Onboard)：_ 厂长执行 `janus onboard --blueprint <name>` 后，Daemon 按标准上线工艺接管：
        1. **配方校验**：读取并校验 `blueprints/<name>/janus.toml`，确认 `workflows/<default_workflow>.toml` 存在；
        2. **点火前自检**：探测 Absurd Postgres 可达、tmux 就位，跨主机蓝图尽力探测远程 SSH 靶机（不可达仅告警）；
        3. **租户注册**：以 `blueprint_id` 为分区键，`INSERT … ON CONFLICT DO UPDATE` 写入一行 `ACTIVE` 蓝图元数据（**幂等**，可重新激活已 `OFFBOARDED` 的蓝图）；
        4. **流水线绑定**：持久化默认 SOP 工作流绑定；
        5. **脑图载入与经验遗传**：索引 `blueprints/<name>/openwiki/`，若存在上一代 `production_report.md` 则优先索引，并将关键避坑经验以 `## Previous Incidents` 少样本注入新一代 Agent System Prompt；
        6. **上线就绪**：状态置 `ACTIVE`，产品线即时出现在 Popup 派单菜单。

    - _下线 (Offboard)：_ 提取该项目开发期间所有的 Steps 轨迹与 Tool Guard 拦截记录，自动在局部 OpenWiki 中熔炼生成一封最新的 **“质检报告与生产白皮书 (production_report.md)”**，同时彻底清空数据库中过期的 JSON 缓存大字段，实现数据库无损防爆收缩。该白皮书将在下次 Onboard 时被回收为免疫抗体，闭合“上线 → 生产 → 下线 → 再上线”的进化环路。
        

## 3. 系统架构拓扑 (System Architecture)

MetaMach 2.0 实行“大脑独立监控、影子客户端透传、物理会话挂接、数据逻辑多租户”的工业化隔离方案：

- **Control Plane (控制层)：**
    
    - **`janus-daemon` (常驻进程)：** 负责核心逻辑调度，维持与 Absurd PG 的长连接，监听外部 Teams/TG 异步消息。同时对外暴露 `progress` 查询原语：聚合 `absurd_tasks` JOIN `absurd_steps` 的实时状态与 Tether 物理 Session 存活信号，作为工作流进度大盘的唯一权威数据源。
        
    - **`herdr-janus` (影子插件)：** 被动执行。在 `herdr-plugin.toml` 中声明，专门负责拉起 `placement = "popup"` 的会话模态交互弹窗，通过 UDS Socket 向 Daemon 发送指令。Popup 内置两个视图：**派单 (Dispatch)** 与 **进度 (Progress)**，厂长可一键切换。进度视图以固定节拍（1–2s）轮询 Daemon 的 `progress` 原语渲染工作流进度大盘。
        
- **Physical Execution Plane (物理执行层)：**
    
    - **`herdr-tether` (物理引擎)：** 独立的 CLI 二进制，利用 `tmux` 的 `remain-on-exit` 特性，管理跨主机的物理 Session（格式为 `tether-janus-task-<uuid>`）。
        
- **Persistence Plane (持久化与知识层)：**
    
    - **Absurd Postgres (Unified DB)：** 单库多租户设计。用 `blueprint_id` 作为物理分区键，统一管理 Steps 状态。
        
    - **OpenWiki (共享 RAG 技能)：** 封装为 Agent 标准 Skill。Agent 遇到代码盲区时通过 `openwiki_query` 工具提出精准 RAG 检索，Janus 拦截并优先在 Postgres 级缓存中查找（Git-SHA 去重），零延迟返回精准 AST 代码片。
        

> **CLI 与二进制架构（统一入口 + 专职二进制）**：系统以统一 `janus` CLI 作为厂长与管理面的单一入口，子命令分两类：(1) **生命周期/查询子命令**--`janus onboard`、`janus offboard`、`janus status`，均为轻量客户端，经 UDS 与常驻 `janus-daemon` 通信（**Daemon 必须在运行**；`janus daemon` 子命令显式拉起它）；(2) 底层专职二进制--`janus-daemon`（常驻大脑）、`herdr-janus`（影子客户端，由 Herdr 加载）、`janus-sh`（代理 Shell，由 Tether 注入为 `SHELL`）、`herdr-tether`（物理执行引擎，外部依赖）。即 `janus offboard` 等价于“客户端经 UDS 向 Daemon 发起 offboard 指令”，而非独立直连数据库。所有 `herdr-tether` 调用统一为 `herdr-tether <subcommand>` 形式（如 `herdr-tether open`、`herdr-tether attach`），**不再使用裸 `tether <subcommand>`**。

## 4. 组件交互细节 (Component Interactivity)

下面以 `gatemetric`（BMX 姿态评估系统）在跨越本地与远程编译服务器的 **标准研发装配线（dev-flow）** 部署为例，展示组件之间令人屏息的精密咬合。

### 🔄 研发装配线执行时序流 (Sequence of Execution)

Code snippet

```
sequenceDiagram
    autonumber
    actor Human as 厂长 (Human)
    participant Client as herdr-janus (影子插件)
    participant Daemon as janus-daemon (大脑)
    participant Absurd as Absurd PG (状态库)
    participant Guard as Tool Guard (janus-sh)
    participant Tether as herdr-tether (物理执行)
    participant OW as OpenWiki (共享知识)
    participant Teams as MS Teams / TUI (审批门)

    Human->>Client: 按下 prefix+j 唤醒 Dispatcher 弹窗 (popup)
    Client->>Daemon: 通过 UDS 请求 ACTIVE 产品线与配置
    Daemon-->>Client: 返回 gatemetric 属性与可用 workflows
    Human->>Client: 选定 gatemetric 派发 dev-flow 流水线
    Client->>Daemon: 发送指令，关闭 Popup

    Note over Daemon, Absurd: 【第一阶段：Scouting 扫描与记忆加载】
    Daemon->>Absurd: 事务校验：SELECT status FROM blueprints WHERE id = 'gatemetric'
    Absurd-->>Daemon: 返回 ACTIVE 状态
    Daemon->>OW: 唤醒 OpenWiki RAG 技能：载入上一代 production_report.md 的避坑经验
    OW-->>Daemon: 注入 Coder Agent System Prompt (避开硬件 I2C 引脚冲突)

    Note over Daemon, Tether: 【第二阶段：本地 Coder 工位搬砖】
    Daemon->>Tether: 创建本地 tmux 会话: "tether-janus-step1-uuid"
    Tether->>Tether: 强制开启 remain-on-exit
    Daemon->>Tether: 运行 Coder Agent 注入修改滤波算法补丁
    Tether-->>Daemon: 写入完成，输出 Git Diff 文件

    Note over Daemon, Tether: 【第三阶段：跨主机编译工位 (Remote SSH)】
    Daemon->>Absurd: 写入 Step 1 Checkpoint: result_cache = Git Diff JSON (哈希去重)
    Daemon->>Tether: 唤醒远程编译服务器 Session: "tether-janus-step2-uuid" via OpenSSH BatchMode
    Daemon->>Tether: 从 Absurd 读取 Diff JSON 注入远端 shell，执行 "make cross-compile"
    Tether->>Tether: 远端交叉编译失败，硬件引脚配置缺失 (Exit Code != 0)
    Tether-->>Daemon: 捕获报错现场 (自动限制在 16KB 预算内，防止爆库)

    Note over Guard, Teams: 【第四阶段：HITL 安全熔断与人工接管】
    Daemon->>Absurd: 锁死状态为 SUSPENDED (非破坏性挂起，远程 tmux Session 绝对不杀)
    Daemon->>Teams: 发送报警卡片：[编译失败！引脚 21 冲突。点击一键 Resume 或 TUI 登录调试]
    Human->>Tether: 通过 Herdr TUI attach 进那个未死的远程 tmux 现场，手动改写 C++ 头文件
    Human->>Teams: 在 Teams 手机端点击 [🔄 Resume Workflow]

    Note over Daemon, Absurd: 【第五阶段：下线熔炼与经验进化】
    Teams-->>Daemon: 接收到恢复指令
    Daemon->>Tether: 驱动 Tether 远程重新 run "make cross-compile" 
    Tether-->>Daemon: 编译通过，质检成功！
    Daemon->>Daemon: janus offboard --blueprint gatemetric
    Daemon->>Absurd: 执行 melt_blueprint_data()，清除运行大 JSON，数据库 footprint 瞬间收缩
    Daemon->>OW: 将本次引脚冲突的修复方案写入 blueprints/gatemetric/openwiki/production_report.md
    Daemon-->>Human: 进化归档成功。下一次 Agent 进场时将直接拥有该免疫抗体！
```

### 📊 工作流进度查询时序 (Progress Query)

进度大盘的交互极为轻量，与上述重型装配时序解耦，由厂长在 Popup 内主动触发：

```
sequenceDiagram
    autonumber
    actor Human as 厂长 (Human)
    participant Client as herdr-janus (影子插件)
    participant Daemon as janus-daemon (大脑)
    participant Absurd as Absurd PG (状态库)

    Human->>Client: prefix+j 唤醒 Popup，切换至“进度”视图
    loop 每 1–2s 轮询
        Client->>Daemon: UDS 发送 progress 查询
        Daemon->>Absurd: SELECT absurd_tasks JOIN absurd_steps WHERE status IN (非终结态)
        Absurd-->>Daemon: 返回在途任务及其工位状态
        Daemon-->>Client: 返回进度 Payload（工位状态/当前工位/耗时/Stdout 摘要）
    end
    Client-->>Human: 渲染进度大盘；SUSPENDED 工位高亮并附 Resume 入口
```

> 该查询为**只读旁路**：不占用工作流执行的事务通道，不影响在跑工位，仅读取 Absurd PG 中的权威状态。无 TUI 环境下，`janus status` CLI 走同一 `progress` 原语输出纯文本/JSON 快照。

## 5. GitHub 终极 Monorepo 目录结构

为了完全遵循 Herdr v1 插件的 **“动静分离（Immutable ROOT vs. Mutable State）”** 物理隔离防线，整个 `metamach` 仓库采用如下组织拓扑：

Plaintext

```
metamach/ (GitHub 唯一主单仓 - 硅基工厂大本营)
├── .github/
│   └── workflows/
│       └── build-janus.yml     # 自动化 CI: 跨平台编译 janus 二进制
│
├── Makefile                    # 工厂总闸 (make bootstrap, make run-gatemetric, etc.)
├── docker-compose.yml          # 一键拉起 Unified Postgres 容器
├── README.md                   # 工厂操作守则与安全防爆白皮书
├── .gitignore                  # 严格过滤本地临时沙箱、PG 物理数据目录及本地 State
│
│   # ====================================================================
│   # 1. 🛡️ JANUS CORE (工厂最高控制大脑与影子客户端)
│   # ====================================================================
├── janus/
│   ├── Cargo.toml              # Rust Workspace 配置
│   ├── herdr-plugin.toml       # 🔌 Herdr v1 插件契约声明 (声明 Popup 弹窗及 event 挂钩)
│   ├── migrations/             # Postgres 初始化迁移脚本
│   │   └── 001_init_absurd.sql
│   └── src/
│       ├── bin/
│       │   ├── janus_daemon.rs # 🪐 独立运行的后台常驻守护进程 (编译为 target/release/janus-daemon)
│       │   ├── herdr_janus.rs  # 🔌 极轻量级 Herdr 影子客户端 (编译为 target/release/herdr-janus)
│       │   └── janus_sh.rs     # 🛡️ 代理 Shell (编译为 target/release/janus-sh)
│       │
│       ├── tool_guard/         # janus-sh 代理 Shell 内存拦截与白名单过滤逻辑
│       ├── absurd/             # 独占的 sqlx Postgres 连接池、对账与 GC 事务
│       └── tui/                # 影子客户端载入的 80% 宽度 Popup 键盘交互界面 (Ratatui)
│
│   # ====================================================================
│   # 2. 💾 CONFIG & EXTERNAL DEPENDENCIES (配置与外部依赖)
│   # ====================================================================
├── configs/                    # 全局静态配置
│   ├── agents.toml             # Agent Pool 注册、权限白名单
│   ├── tmux.conf               # Tether tmux 初始化配置 (remain-on-exit)
│   └── global_rules.md         # 工厂全局开发者守则 (Agent 进场必读的安全生产红线)
│
├── openwiki/                   # 🔗 External: langchain-ai/openwiki — RAG 知识联邦引擎
│   └── configs/                # OpenWiki 引擎配置 (二进制由外部仓库构建)
│
├── workflows/                  # ⚙️ 统一管理的工艺流水线标准 SOP
│   ├── dev-flow.toml           # 💻 标准研发线
│   ├── debug-flow.toml         # 🐞 诊断除错线
│   └── firmware-deploy.toml    # 📦 物理交叉编译与烧录线
│
│   # ====================================================================
│   # 3. 📐 BLUEPRINTS (产品线 / 目标开发项目)
│   # ====================================================================
├── blueprints/
│   │
│   ├── joyrobots/              # 🤖 JoyRobots (模块化教育机器人平台)
│   │   ├── janus.toml          # 专属生产配方 (绑定 dev-flow)
│   │   ├── src/                # 纯粹的项目源码
│   │   └── openwiki/           # 局部脑图 (Spike Prime API)
│   │
│   └── gatemetric/             # 📉 GateMetric (BMX 姿态评估系统)
│       ├── janus.toml          # 专属生产配方 (绑定 firmware-deploy，配置 SSH 编译靶机)
│       ├── firmware/           # ESP32 滤波 C++/Arduino 源码
│       ├── 3d-enclosure/       # Bambu Lab X1C 专用的传感器外壳 CAD/STL 图纸
│       └── openwiki/           # 局部脑图 (MPU6050 时序依赖与 production_report 免疫白皮书)
│
│   # ====================================================================
│   # 4. 🛠️ PROVISIONING (大终端自动化维护与沙箱挂载)
│   # ====================================================================
└── provisioning/
    ├── bootstrap.sh            # 零依赖一键部署脚本 (软链接配置目录，启动后台 PG 容器)
    └── init-user-db.sh         # Postgres 初始化角色、权限与 metamach_db 数据库脚本

# ═══════════════════════════════════════════════════════════════════════
# 🔗 EXTERNAL DEPENDENCIES (独立仓库，由 make bootstrap 拉取/构建)
# ═══════════════════════════════════════════════════════════════════════
# herdr-tether → https://github.com/moneycaringcoder/herdr-tether
#    物理执行引擎：tmux 会话管理、remain-on-exit、跨主机 SSH
# absurd       → https://github.com/earendil-works/absurd
#    Absurd Postgres 引擎：事务对账、连接池、melt_blueprint_data 存储过程
# openwiki     → https://github.com/langchain-ai/openwiki
#    联邦知识库 RAG 引擎：共享技能检索、production_report 索引
```

> ⚠️ **关于外部依赖与动态配置的说明：**
> - `herdr-tether`、`absurd`、`openwiki` 均为独立外部仓库，不在本 monorepo 中编译。`make bootstrap` 负责拉取/构建/链接这些依赖。
> - 运行时 Mutable 配置（如 `agents.toml`）必须被软链接放置在 根据官方 Spec 规范，运行时的 Mutable 配置（如 `agents.toml`）必须被软链接放置在 **`${HERDR_PLUGIN_CONFIG_DIR}`**（即 `~/.config/herdr/plugins/metamach.janus`）中； 所有的事务日志、缓存的 SQLite 及临时 Socket 文件必须存放在 **`${HERDR_PLUGIN_STATE_DIR}`**（即 `~/.local/state/herdr/plugins/metamach.janus`）下。这样可以彻底确保 GitHub 在更新插件源码时，**绝不意外抹除任何本地财务或开发状态数据**。

## 6. 极致的防震与防爆安全设计 (Resilience Invariants)

1. **物理不折损 ── tmux Remain-on-Exit：** 所有由 Tether 托管的物理 Session 均注入 `remain-on-exit on`。当 AI 运行发生段错误或语法错误退出时，终端现场被 100% 完整保留。绝对不杀物理进程，防止开发上下文随风而逝。
    
2. **容量防爆 ── 16KB Budget & SQL Pruning：** Unified Database 绝不无节制膨胀。所有 Step Checkpoint 的大 JSON 缓存、终端 Stdout 抓取超过 16KB 强力截断。每日定时执行 `Janus GC` 事务，自动清理 3 天前已结束的所有 Blueprint 缓存字段。
    
3. **安全零越权 ── 物理 janus-sh 拦截：** 不依赖 AI 去自我控制。所有的高危命令在到达底层 Bash 前，必须经过 `janus-sh` 在 UDS 管道中对 Janus Daemon 发起同步对账。未检测到 Teams/TUI 审批通过前，强制在内存中重写或拒绝执行。
    
4. **无状态冷启动 ── 绝对抛弃 tmux-resurrect：** 状态的唯一权威源是 Postgres。当机房重启后，系统直接从数据库中读取最后一个 Completed Step 的 JSON 缓存，重新指派全新的 Tether Session UUID 瞬时拉起，在物理断电处无缝接棒。