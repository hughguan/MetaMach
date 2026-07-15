

### ── 认知调度底盘、代理沙箱与耐久化工作流的系统级工程实现规格

> **EN:** Engineering Feature Spec — cognitive scheduler, agent sandbox (janus-sh), durable workflows, data contracts, and fault matrix.

## 1. 模块架构总览 (Module Map)

根据 Herdr v1 插件规格与系统的独立常驻进程设计，MetaMach 2.0 软件由以下四层核心功能组件构成。本设计说明书严格遵循动静分离（Immutable ROOT vs. Mutable State）规范，并对各特性的行为边界、数据流向和异常处理进行像素级定义。

```
+-----------------------------------------------------------------------------------------+
|                                🪐 METAMACH CORE LAYERS                                  |
+-----------------------------------------------------------------------------------------+
|  1. 🎛️ CONTROL:   janus-daemon (常驻 UDS 服务) & herdr-janus (Popup 影子终端)             |
|  2. 🛡️ SANDBOX:   janus-sh (代理 Shell) & Event-Driven Tool Guard (同步内核级卫兵)       |
|  3. 📦 WORKFLOW:  Absurd Postgres 事务引擎 & Cross-Host Tether 物理挂接 (remain-on-exit)       |
|  4. 🧠 KNOWLEDGE: Federated OpenWiki Skill & Auto-Pruning 熔炼器 (Melt DB Cache)         |
+-----------------------------------------------------------------------------------------+
```

## 2. 核心特性详细规格 (Feature Specifications)

### Feature 2.1: 单例常驻控制中枢 (Janus Daemon & Twin-Client UI)

- **特性描述**：实现一个长跑的后台守护进程 `janus-daemon` 用于集中管控状态，以及一个轻量级的 Herdr 交互外壳 `herdr-janus` 负责 Popup 弹窗渲染。
    
- **技术规格**：
    
    - `janus-daemon` 启动时在 `${HERDR_PLUGIN_STATE_DIR}/janus.sock` 绑定唯一的 Unix Domain Socket 监听。
        
    - **惰性启动自愈 (Lazy-Start)**：`herdr-janus` 被唤醒时，若检测到 `janus.sock` 不存在或连接超时，必须自动无感拉起后台守护进程。**使用 `std::process::Command::spawn()` 并显式 detach（`stdin/stdout/stderr` 重定向至 `/dev/null`、`pre_exec` 调 `setsid` 脱离控制终端）**，而非裸 `fork()`+`exec()`--后者在 macOS 上被官方明确不鼓励（见 `man fork`），且不跨平台。spawn 失败（资源不足等）时向厂长报错而非静默崩溃。
        
    - **UI 弹窗限制**：通过 `herdr-plugin.toml` 的 `placement = "popup"` 锁定 80% 宽度与 20 行高度。使用 `ratatui` 作为纯键盘 UI 渲染引擎，输入焦点自动锁定，按 `Esc` 键通过影子客户端向 Daemon 汇报后安全退栈。
        
    - **PG 不可达启动自愈**：Daemon 启动时以指数退避重试 Absurd Postgres 连接（5 次，间隔 2s）。仍不可达时进入**降级模式**：仅写本地 `fallback.db`（环形队列）、记 `WARN` 并经 Popup 通知厂长“DB 离线，运行降级”。期间所有 Step 状态变更落 `fallback.db`；检测到 PG 恢复后自动批量 Log Replay 合并至主库（与 §4 容错矩阵一致），车间不停工。
        

### Feature 2.2: 内存级代理沙箱 (janus-sh & Tool Guard)

- **特性描述**：提供一个进程外独立审计的安全门禁，在 Agent 的意图交付给 Bash 之前进行同步物理拦截，支持特种操作的 Dry-Run 重定向。
    
- **技术规格**：
    
    - **Shell 拦截代理 (janus-sh)**：Tether 启动 Pane 时，强制将底层环境变量 `SHELL` 注入重定向为 `janus-sh` 的**绝对路径**（`${HERDR_PLUGIN_ROOT}/bin/janus-sh`，由 `make bootstrap` 安装，见 Deployment-Spec §5.1）。**绝不使用 `target/release/janus-sh` 相对路径**--后者在 CWD 非仓库根、二进制未编译或窗格从其他目录启动时失效。
        
    - **同步 UDS 对账**：当 Agent 发送任意命令行字符串时，`janus-sh` 同步挂起执行，将原始 argv 数组打包发送至 `janus.sock`。
        
    - **超时与防死锁**：`janus-sh` 同步阻塞等待 Daemon 裁决，设可配置超时（默认 30s）。若 Daemon 崩溃或 UDS 断裂导致超时，按 **fail-closed** 返回错误给 Agent 且不执行命令（绝不放行），避免 Agent 的 Shell 因 Daemon 不可达而无限挂起。裁决响应格式与语义见 Contract 3.4。
        
    - **Dry-Run 重定向**：`janus-daemon` 的 Tool Guard 模块针对核心白名单命令（例如：带有修改系统级配置或执行金融扣款交易的指令）进行安全审查。在没有收到厂长在 Teams/TUI 端的数字签名授权（Correlation ID）之前，Daemon 在内存中强行篡改指令参数（如附加 `--dry-run` 标签）或返回拦截错误，绝不向下传递至真实的物理宿主 Shell。
        

### Feature 2.3: 多 Agent 跨主机耐久工作流 (Distributed Durable Workflows)

- **特性描述**：基于 Absurd Postgres 强事务引擎与 Tether 跨主机 tmux PTY，支持流水线步骤（Steps）在多工位、跨主机边界时的幂等接力与断点自愈。
    
- **技术规格**：
    
    - **Absurd 事务原语**：每个 Workflow Step 执行前，Daemon 必须在物理数据库中提交 `UPDATE` 锁死过渡态（`STARTING`）。执行成功后，将输出数据打包为 `JSONB result_cache` 一体化 Commit（状态变更为 `COMPLETED`）。
        
    - **物理不折损 (Remain-on-Exit)**：Tether 启动的本地或通过 OpenSSH BatchMode 连接的远程 tmux 会话，必须被显式注入 `remain-on-exit on`。**为避免污染厂长本机 tmux 的全局配置与其他无关会话**，Tether 使用**独立的 tmux server**（`tmux -L metamach-tether ...`），并在该 server 内 per-session 设置 `set remain-on-exit on`（不使用 `-g` 全局开关）。当跨主机交叉编译或固件烧录因退出码非零（`Exit Code != 0`）发生逻辑崩溃时，物理 PTY 窗格绝对不杀，保持错误现场。独立 server 还使灾备演练可安全执行 `tmux -L metamach-tether kill-server` 而不误杀厂长的其他会话。
        
    - **冷启动对账**：系统断电重启后，Daemon 扫描所有处于非终结态的任务，禁止使用 `tmux-resurrect`。它直接从 Postgres 中提取最后一次有效的 `COMPLETED` Checkpoint，指派全新 Tether Session UUID 无缝在物理断点处重跑接棒。
        

> **Step/Task 状态机（权威定义）**：
> ```
> PENDING -> STARTING -> RUNNING -> COMPLETED
>                        │
>                        ├──FAILED──> SUSPENDED ──(人工 metamach-resume)──> RUNNING（派发下一工位）
>                        │
>                        └──(断电/Daemon 崩溃)──> 孤儿工位：冷启动从最近 COMPLETED 接棒重跑；
>                                                 SUSPENDED 工位保持挂起并通知厂长（不盲目重跑）
> ```
> 终结态：`COMPLETED` / `FAILED`（不可恢复且未经 HITL）。冷启动对账仅从 `COMPLETED` 接棒，`RUNNING` 态视为丢失（有意的数据损失，见 §2.3 冷启动对账）。

### Feature 2.4: 人工干预多端异步合闸门禁 (HITL Gate & Notification)

- **特性描述**：在流程发生编译中断或安全超权时，触发系统级非破坏性挂起，并在外部异步通信网关上弹出高密度的对账卡片供厂长审批。
    
- **技术规格**：
    
    - **非破坏性挂起**：将任务状态标记为 `SUSPENDED`。此时 `janus-sh` 的命令**从未被转发至宿主 Shell**--Daemon 在 UDS 对账阶段即拒绝下发，Tether 物理 PTY 窗格保持存活但处于**空闲态**（而非卡在某个前台进程上等待 SIGINT）。Daemon 咬死该 PTY，保护内存上下文与终端历史。
        
    - **异步双向网关（Telegram 为主、Teams 为辅）**：**Telegram 为首要通知后端**（开放协议、移动端原生、Bot API + Inline Keyboard 原生实现 `[Resume]` 按钮），MS Teams 为次要适配（MessageCard 格式，API 与 Telegram 完全不同，须独立 adapter）。Daemon 仅构造一个**抽象 Webhook Payload**（任务 UUID、拦截成因、16KB 截断后的 Stdout 现场、`Resume` 触发键 + Correlation ID），再由各后端 adapter 翻译为原生格式（Telegram `sendMessage` + `inline_keyboard` / Teams Actionable MessageCard）。新增后端只需实现 adapter，不改 Daemon 主逻辑。
        
    - **远程合闸响应（重新设计，不再依赖 `Ctrl+C`）**：`SUSPENDED` 意味着命令已被拦截、窗格空闲，**没有任何进程需要被 `Ctrl+C` 释放**。正确的恢复闭环为：
        1. 厂长通过 Herdr TUI `attach` 进入该空闲窗格，**就地人工修复**（编辑代码、改写配置、修正引脚定义等）；
        2. 厂长在窗格内键入 `metamach-resume`（或在移动端点击 `Resume` 卡片回调）发出完成信号；
        3. Daemon 核对 Correlation ID 签名无误后，将状态由 `SUSPENDED` 置回 `RUNNING`，并**派发下一个工位（Step）的命令**--**绝不盲目重新执行被拦截的那条原命令**，否则会覆盖厂长刚才的就地修复。
        

### Feature 2.5: 联邦式生命周期熔炼器 (Onboard / Offboard & Auto-Pruning)

- **特性描述**：管理产品蓝图的冷热数据状态转换，在产品下线时将全生命周期的运行轨迹熔炼成冷经验沉淀，并擦除大体积缓存。
    
    > **执行前提**：`janus onboard` / `offboard` / `status` 均为经 UDS 与常驻 `janus-daemon` 通信的轻量客户端，**Daemon 必须在运行**（见 ARCH §3 CLI 架构）。Daemon 未运行时这些命令直接报错退出，**绝不绕过 Daemon 直连数据库**，以保证所有状态变更都经过 Daemon 的事务对账。
    
- **技术规格**：
    
    - **联邦 Wiki 设计**：`configs/global_rules.md` 存放于 Immutable ROOT 中作为全局注入的 System Line。每个蓝图独占 `blueprints/<name>/openwiki/` 子目录用于存放项目专有 AST 脑图。
        
    - **Agent System Prompt 组装规范**：Daemon 按**优先级拼接**组装 Agent 的 System Prompt：`[configs/global_rules.md 全局红线]` + `[blueprints/<name>/openwiki/ 蓝图脑图]` + `[## Previous Incidents 历史避坑少样本]` + `[当前 Step 指令]`。各段独立截断以适配目标模型上下文窗口（模型与 `max_context_tokens` 在 `configs/agents.toml` 声明）；组合超限时按**逆优先级丢弃**（先丢少样本，再丢脑图，全局红线永留）。
        
    - **上线注册机制 (Onboard)**：当厂长执行 `janus onboard --blueprint <name>` 时，Daemon 按以下原子序列接管（任一关键步骤失败即整体回滚，不留半激活态）：
        
        1. **配方校验**：读取 `blueprints/<name>/janus.toml`，校验必填字段（`blueprint.name`、`blueprint.default_workflow`、`openwiki.scope`、可选 `remote`）。确认 `workflows/<default_workflow>.toml` 存在且符合工作流文件 schema。校验失败返回明确错误，不写库。
            
        2. **点火前自检**：探测 Absurd Postgres 连接可用、tmux 引擎就位。若声明了 `[remote]`，对远程 SSH 靶机执行一次 `BatchMode` 连通性探测（`-o ConnectTimeout=5`），不可达仅记录 `WARN`，不阻断上线（允许离线先上线、后补靶机）。
            
        3. **租户注册（幂等）**：执行 `INSERT INTO blueprints (name, status, default_workflow, config, openwiki_scope, remote_host, onboarded_at) VALUES (…) ON CONFLICT (name) DO UPDATE SET status='ACTIVE', onboarded_at=NOW(), offboarded_at=NULL, …`。以 `blueprint_id` 作为物理分区键完成逻辑多租户隔离。重复 Onboard 无副作用；重新上线已 `OFFBOARDED` 的蓝图将其重新激活。
            
        4. **流水线绑定**：将 `default_workflow` 持久化到蓝图元数据，并预编译校验该工作流的 Step 序列，确保派单时可即时点火。
            
        5. **脑图载入与经验遗传**：索引 `blueprints/<name>/openwiki/` 进入 OpenWiki 检索范围。**若该路径下存在 `production_report.md`（上一代 Offboard 产物）**，Daemon 优先解析其结构化区块（编译报错历史 / 引脚冲突 / Tool Guard 拦截日志 / 成功 Patch），将关键失败模式以 `## Previous Incidents` 少样本（Few-shot）形式追加进该蓝图 Agent 的 System Prompt 模板，实现跨代免疫遗传。
            
        6. **上线就绪**：事务提交，状态置 `ACTIVE`。Daemon 通过 UDS 向 `herdr-janus` 广播 `blueprint_registered` 事件，Popup 派单菜单即时刷新出现新产品。
            
    - **下线熔炼机制 (Offboard)**：当厂长执行 `janus offboard --blueprint <name>` 时：
        
        1. Daemon 扫描 Absurd 数据库，抽取该蓝图历史上所有的 Task、Step 执行轨迹与 Tool Guard 历史拦截日志。
            
        2. 调用配置化的大模型（见下文「Offboard LLM 集成规格」）将上述运行快照、引脚错误和避坑 Patch 压缩总结为高密度的 Markdown 文件，原子化写入 `./blueprints/<name>/openwiki/production_report.md`。
            
        3. **数据库防爆降解 (Melt Cache)**：调用后台 SQL 存储过程 `melt_blueprint_data('<name>')`，**整行 DELETE**（非 NULL 化）该蓝图所有 Step 的 `result_cache` JSON 大字段与 Stdout 日志行--NULL 化不释放 TOAST 物理空间，整行删除方能让 autovacuum / `VACUUM FULL` 回收。同时将一行审计元数据统计（Task ID、执行耗时）写入独立的 `absurd_audit_log` 表供后期审计。
            
        4. 通过影子客户端调用本地 Git 自动将 `production_report.md` 进行增量 Commit 与 Push，完成硅基经验的自我遗传。
            
    - **Offboard LLM 集成规格**：熔炼所用大模型为可配置外部依赖，规范如下：
        
        - **端点配置**：`configs/offboard.toml` 声明 `endpoint`、`api_key_env`（环境变量名，密钥绝不落盘）、`model`、`max_input_tokens`。
            
        - **输入预算**：仅取最近 N 条 Step（默认 N=50），每条 `result_cache` 截断至 16KB，总输入受 `max_input_tokens` 约束，超限则按时间倒序丢弃最旧条目。
            
        - **Prompt 模板**：强制结构化输出四区块--【编译报错历史】【引脚冲突细节】【Tool Guard 拦截日志】【成功通过的 Patch】。
            
        - **降级兜底**：LLM 不可用（API Key 失效、限流、air-gapped 离线）或超过 120s 超时时，**不阻塞下线**，改为写入原始 JSON 快照 `production_report.raw.json` 并记录 `WARN`。
            
        - **异步执行**：Offboard 指令立即返回「熔炼中」，LLM 总结在后台进行，报告就绪后经 UDS 事件通知 `herdr-janus` 并完成 Git 提交。
            

### Feature 2.6: 工作流进度大盘 (Workflow Monitor & Status Query)

- **特性描述**：为厂长提供在途工作流的实时可视性，回答“它还在跑吗 / 卡在第几工位 / 正常还是卡死”。以只读旁路方式聚合 Absurd Postgres 权威状态，不干扰工作流执行通道。

- **技术规格**：

    - **双视图 Popup**：`herdr-janus` 的 Popup 在原有“派单 (Dispatch)”视图基础上新增“进度 (Progress)”视图，由 `prefix+j` 唤醒后通过 `Tab` 键切换。进度视图以 `ratatui` 表格按蓝图分组渲染在途任务矩阵：所属蓝图 · 流水线 · 当前工位 · 各工位状态（`PENDING` / `STARTING` / `RUNNING` / `COMPLETED` / `SUSPENDED` / `FAILED`）· 已耗时 · 最近 Stdout 摘要（截断至 1KB）。

    - **轮询节拍**：进度视图打开期间，`herdr-janus` 以固定 1–2s 节拍经 UDS 向 Daemon 发送 `progress` 查询并重绘。关闭视图即停止轮询，零空转开销。

    - **`progress` 查询原语（Daemon 侧）**：Daemon 收到查询后执行只读 `SELECT` 聚合 `absurd_tasks JOIN absurd_steps`（过滤非终结态任务），并叠加 Tether 物理 Session 的存活信号（`tmux has-session`）。该查询走独立只读事务，**绝不占用**工作流执行的写事务通道。

    - **挂起即时高亮**：当某工位状态为 `SUSPENDED` 时，大盘在该行 1s 内高亮标红，并在行尾渲染 `[A]ttach 现场` / `[R]esume` 快捷键入口，直接复用既有 Tether attach 与 HITL resume 通路。

    - **`janus status` CLI 兜底**：无 TUI 环境（SSH / CI）下，`janus status [--blueprint <name>] [--json]` 走同一 `progress` 原语，输出纯文本或 JSON 快照，供脚本化巡检。

## 3. 系统数据交换契约 (Data Contracts)

### Contract 3.1: 核心 schema (Absurd DB)

SQL

```
-- 蓝图租户注册表 (Onboard 写入 / Offboard 置 OFFBOARDED)
CREATE TABLE blueprints (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,          -- 蓝图名，亦为逻辑租户/分区键标识
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE', -- ACTIVE | OFFBOARDED
    default_workflow VARCHAR(100) NOT NULL,     -- 绑定的默认 SOP 工作流
    config JSONB,                                -- janus.toml 原文
    openwiki_scope JSONB,                        -- [openwiki].scope 索引范围
    remote_host VARCHAR(100),                    -- [remote].host (NULL = 纯本地蓝图)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    onboarded_at TIMESTAMPTZ,                    -- 最近一次 Onboard 时间
    offboarded_at TIMESTAMPTZ                    -- 最近一次 Offboard 时间
);

-- 工作流任务表 (一次派单 = 一行 Task)
CREATE TABLE absurd_tasks (
    id SERIAL PRIMARY KEY,
    blueprint_id INTEGER NOT NULL REFERENCES blueprints(id) ON DELETE CASCADE,
    workflow_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,                 -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 核心步骤检查点表 (带 Size Budget 限制)
CREATE TABLE absurd_steps (
    task_id INTEGER REFERENCES absurd_tasks(id) ON DELETE CASCADE,
    step_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,            -- PENDING | STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    result_cache JSONB,                     -- 严格限制物理存储体积，最大 16KB 截断
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, step_name)
);
```

> **状态枚举统一**：全系统 Step/Task 状态机为 `PENDING -> STARTING -> RUNNING -> COMPLETED | FAILED | SUSPENDED`。蓝图级别状态机为 `ACTIVE <-> OFFBOARDED`（Onboard 激活 / Offboard 归档）。

### Contract 3.2: 代理 Shell 同步通信 UDS Payload (janus-sh -> daemon)

JSON

```
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "blueprint_id": "gatemetric",
  "task_id": 1042,
  "step_name": "cross_compile",
  "cwd": "/workspaces/metamach/blueprints/gatemetric/firmware",
  "argv": ["esptool.py", "--chip", "esp32", "write_flash", "0x1000", "firmware.bin"],
  "env_snapshot": {
    "USER": "factory_agent",
    "SHELL": "/target/release/janus-sh"
  }
}
```

### Contract 3.3: 工作流进度查询响应 Payload (Daemon -> herdr-janus / `janus status`)

JSON

```
{
  "active_tasks": [
    {
      "task_id": 1042,
      "blueprint_id": "gatemetric",
      "workflow_name": "dev-flow",
      "status": "RUNNING",
      "started_at": "2026-07-15T09:00:00Z",
      "elapsed_seconds": 945,
      "current_step": "cross_compile",
      "tether_alive": true,
      "suspended_reason": null,
      "steps": [
        {"name": "scout",         "status": "COMPLETED"},
        {"name": "code",          "status": "COMPLETED"},
        {"name": "cross_compile", "status": "RUNNING",  "stdout_tail": "…最近 1KB 终端摘要…"}
      ]
    }
  ]
}
```

> `active_tasks` 仅包含非终结态任务（`STARTING` / `RUNNING` / `SUSPENDED`）。`stdout_tail` 为各工位最近终端输出截断至 1KB 的摘要，`tether_alive` 反映对应 Tether 物理 Session 是否存活。该 Payload 同时驱动 Popup 进度大盘渲染与 `janus status --json` 的 CLI 输出。

### Contract 3.4: 代理 Shell 同步通信 UDS 响应 (Daemon -> janus-sh)

JSON

```
{
  "execution_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a",
  "verdict": "ALLOW",                         // ALLOW | BLOCK | REWRITE
  "reason": "financial_trade_requires_approval", // 人类可读的决策成因
  "rewritten_argv": ["hi5bot", "--action", "dry-run"], // 仅当 verdict=REWRITE 时存在
  "correlation_id": "0190b2c1-7d1a-7b3c-912a-4f6c8d2e4f6a" // 审计追踪 ID；HITL 挂起时即 Resume 凭证
}
```

> **裁决语义**：`ALLOW` = 原样交付宿主 Shell 执行；`BLOCK` = `janus-sh` 向 Agent 返回非零退出码且不执行，并将 Step 置 `SUSPENDED` 触发 HITL；`REWRITE` = 以 `rewritten_argv` 替换原始 argv 后执行（如 Dry-Run 重定向）。**超时**：若 Daemon 在 `janus-sh` 同步阻塞窗口内（默认 30s）未响应，`janus-sh` 按 **fail-closed** 视为 `BLOCK` 返回错误，绝不放行（见 §2.2 deadlock 处理）。

### Contract 3.5: Agent Pool 资质与 Tool Guard 规则 schema (`configs/agents.toml`)

TOML

```
# 全局 Agent 岗位资质与 Tool Guard 决策矩阵（Daemon 启动时载入，热重载支持）

[agent.scout]
permissions      = ["read", "grep", "find", "git-log"]   # 白名单能力
allow_network    = false                                  # 禁止任何网络外联
bash_safe        = true                                   # 强制走安全 Bash 子集

[agent.coder]
permissions      = ["read", "write", "edit", "bash-safe", "git-commit"]
allow_network    = false
bash_safe        = true
bash_blacklist   = ["rm -rf /", "> /dev/sd*", "mkfs.*", "dd if=* of=/dev/*"]  # 正则黑名单

[agent.deployer]
permissions      = ["read", "write", "bash-full", "ssh", "git-push"]
allow_network    = true
require_approval = ["esptool.py write_flash", "make flash", "*production*"]  # 命中即触发 HITL 挂起
```

> **决策优先级**：Tool Guard 对每条 `janus-sh` 上报的 argv 依次判定——(1) `bash_blacklist` 命中 -> `BLOCK`；(2) `require_approval` 命中 -> `BLOCK` 并置 `SUSPENDED` 等 HITL；(3) 命令能力不在当前岗位 `permissions` 白名单 -> `BLOCK`；(4) 金融类高危指令 -> `REWRITE` 为 Dry-Run；(5) 其余 -> `ALLOW`。规则可配置（非硬编码 Rust），Daemon 通过 `configs/agents.toml` 软链接（Mutable Config 区）载入。

### Contract 3.6: 蓝图配方 schema (`blueprints/<name>/janus.toml`)

TOML

```
[blueprint]
name = "gatemetric"                # 必填，与目录名一致，亦为逻辑租户键
default_workflow = "firmware-deploy" # 必填，引用 workflows/<name>.toml

[remote]                           # 可选；纯本地蓝图（如 joyrobots）省略此段
host = "192.168.1.100"
user = "builder"

[openwiki]
scope = ["mpu6050", "esp32-timers", "i2c-conflicts"] # 该蓝图 OpenWiki 脑图索引范围
```

> Onboard 时 Daemon 严格校验 `blueprint.name`、`blueprint.default_workflow`（对应文件须存在）、`openwiki.scope`；`[remote]` 缺省即纯本地蓝图。校验失败不上线（见 §2.5 Onboard 步骤 1）。

### Contract 3.7: 工艺流水线 schema (`workflows/<name>.toml`)

TOML

```
[workflow]
name = "firmware-deploy"           # 必填，与文件名一致
description = "标准固件交叉编译与烧录线"

[[steps]]                          # 有序工位链
name = "scout"
agent = "scout"                    # 引用 configs/agents.toml 中的岗位
toolset = ["read", "grep", "find"]

[[steps]]
name = "code"
agent = "coder"
toolset = ["read", "write", "edit", "bash-safe"]

[[steps]]
name = "cross-compile"
agent = "deployer"
command = "make cross-compile"
host = "remote"                    # 引用 janus.toml [remote]；缺省=本地
toolset = ["bash-full", "ssh"]
```

> 每个 `[[steps]]` 至少声明 `name`、`agent`、`toolset`；`command` 为该工位执行的具体指令，`host` 跨主机时引用蓝图的 `[remote]`。Daemon 派单时按数组顺序串联执行，每步落一条 `absurd_steps` Checkpoint。

### Contract 3.8: 灾备环形队列 schema (`fallback.db`, SQLite)

SQL

```
-- fallback.db：PG 不可达期间承载过渡态的本地环形队列（${HERDR_PLUGIN_STATE_DIR}/fallback.db）
CREATE TABLE fallback_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    step_name TEXT NOT NULL,
    status TEXT NOT NULL,                 -- STARTING | RUNNING | COMPLETED | SUSPENDED | FAILED
    result_cache TEXT,                    -- JSON 文本，同样 16KB 截断
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_fe_task ON fallback_events(task_id);
```

> **环形队列语义**：容量上限为 **最近 1000 条或 50MB（先到先裁）**，超出即按 `seq` 最旧淘汰。PG 恢复后触发 **Log Replay**：按 `seq` 顺序合并至主库 `absurd_tasks`/`absurd_steps`，同一 `(task_id, step_name)` 以最大 `seq` 为准（last-write-wins），冲突由 `seq` 全序裁决，确保无状态丢失、无重复。

## 4. 交付质量指标与异常熔断矩阵 (UAT & Fault Matrix)

|**异常事件边界**|**底层物理行为表现**|**系统级容错与熔断收敛方案**|
|---|---|---|
|**Tether 物理网络/SSH 中断**|远程编译服务器掉线，标准 `std::process` 发生管道读写挂起。|**触发 Session 冻结**：Janus Daemon 捕获不灭的底层句柄，在 Postgres 中将 Step 标记为 `SUSPENDED`。绝对不 kill 后台 tmux 窗格。当网络恢复后，通过重新建立 SSH 管道执行 `herdr-tether attach` 秒级唤醒现场。|
|**Agent 输出幻觉，死循环刷屏日志**|终端 Stdout 字符流瞬间产生每秒数兆的垃圾文本。|**物理 Size Budget 熔断（单一权威执行点）**：`janus-sh` 在内存中内置流式计数器，对单次 Step 的 Stdout 累积超过 **16 KiB** 时**提前流式截断**（仅为优化，减少 UDS 传输量）；**权威的 16KiB 强制执行点在 `janus-daemon` 的 `absurd` 模块、`INSERT` 事务提交之前**--Daemon 落库前再次校验并硬截断、附加 `[MetaMach Log Budget Exceeded]` 标签。两道防线指向同一 16KiB 上限，DB 写入为最终闸门，确保脏数据绝不灌入 Postgres。|
|**Absurd DB 发生连接池崩溃**|宿主机 Postgres 遭遇极端物理内存溢出或容器意外闪退。|**状态机防爆降锁**：Janus Daemon 内部设计有局部内存级 SQLite 环形备份队列（Ring Buffer）。在 PG 连接断开期间，所有的过渡态 Step 变更优先原子写入本地 `HERDR_PLUGIN_STATE_DIR/fallback.db`。当检测到主机 PG 容器恢复后，自动触发批量合并重放（Log Replay），确保车间生产不停工。|