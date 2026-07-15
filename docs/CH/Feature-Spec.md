

### ── 认知调度底盘、代理沙箱与耐久化工作流的系统级工程实现规格

## 1. 模块架构总览 (Module Map)

根据 Herdr v1 插件规格与系统的独立常驻进程设计，MetaMach 2.0 软件由以下四层核心功能组件构成。本设计说明书严格遵循动静分离（Immutable ROOT vs. Mutable State）规范，并对各特性的行为边界、数据流向和异常处理进行像素级定义。

```
+-----------------------------------------------------------------------------------------+
|                                🪐 METAMACH CORE LAYERS                                  |
+-----------------------------------------------------------------------------------------+
|  1. 🎛️ CONTROL:   janus-daemon (常驻 UDS 服务) & herdr-janus (Popup 影子终端)             |
|  2. 🛡️ SANDBOX:   janus-sh (代理 Shell) & Event-Driven Tool Guard (同步内核级卫兵)       |
|  3. 📦 WORKFLOW:  Absurd PG 事务引擎 & Cross-Host Tether 物理挂接 (remain-on-exit)       |
|  4. 🧠 KNOWLEDGE: Federated OpenWiki Skill & Auto-Pruning 熔炼器 (Melt DB Cache)         |
+-----------------------------------------------------------------------------------------+
```

## 2. 核心特性详细规格 (Feature Specifications)

### Feature 2.1: 单例常驻控制中枢 (Janus Daemon & Twin-Client UI)

- **特性描述**：实现一个长跑的后台守护进程 `janus-daemon` 用于集中管控状态，以及一个轻量级的 Herdr 交互外壳 `herdr-janus` 负责 Popup 弹窗渲染。
    
- **技术规格**：
    
    - `janus-daemon` 启动时在 `${HERDR_PLUGIN_STATE_DIR}/janus.sock` 绑定唯一的 Unix Domain Socket 监听。
        
    - **惰性启动自愈 (Lazy-Start)**：`herdr-janus` 被唤醒时，若检测到 `janus.sock` 不存在或连接超时，必须自动无感 `fork` 并 `exec` 脱离终端的后台守护进程。
        
    - **UI 弹窗限制**：通过 `herdr-plugin.toml` 的 `placement = "popup"` 锁定 80% 宽度与 20 行高度。使用 `ratatui` 作为纯键盘 UI 渲染引擎，输入焦点自动锁定，按 `Esc` 键通过影子客户端向 Daemon 汇报后安全退栈。
        

### Feature 2.2: 内存级代理沙箱 (janus-sh & Tool Guard)

- **特性描述**：提供一个进程外独立审计的安全门禁，在 Agent 的意图交付给 Bash 之前进行同步物理拦截，支持特种操作的 Dry-Run 重定向。
    
- **技术规格**：
    
    - **Shell 拦截代理 (janus-sh)**：Tether 启动 Pane 时，强制将底层环境变量 `SHELL` 注入重定向为 `target/release/janus-sh`。
        
    - **同步 UDS 对账**：当 Agent 发送任意命令行字符串时，`janus-sh` 同步挂起执行，将原始 argv 数组打包发送至 `janus.sock`。
        
    - **Dry-Run 重定向**：`janus-daemon` 的 Tool Guard 模块针对核心白名单命令（例如：带有修改系统级配置或执行金融扣款交易的指令）进行安全审查。在没有收到厂长在 Teams/TUI 端的数字签名授权（Correlation ID）之前，Daemon 在内存中强行篡改指令参数（如附加 `--dry-run` 标签）或返回拦截错误，绝不向下传递至真实的物理宿主 Shell。
        

### Feature 2.3: 多 Agent 跨主机耐久工作流 (Distributed Durable Workflows)

- **特性描述**：基于 Absurd Postgres 强事务引擎与 Tether 跨主机 tmux PTY，支持流水线步骤（Steps）在多工位、跨主机边界时的幂等接力与断点自愈。
    
- **技术规格**：
    
    - **Absurd 事务原语**：每个 Workflow Step 执行前，Daemon 必须在物理数据库中提交 `UPDATE` 锁死过渡态（`STARTING`）。执行成功后，将输出数据打包为 `JSONB result_cache` 一体化 Commit（状态变更为 `COMPLETED`）。
        
    - **物理不折损 (Remain-on-Exit)**：Tether 启动的本地或通过 OpenSSH BatchMode 连接的远程 tmux 会话，必须被显式注入 `set -g remain-on-exit on`。当跨主机交叉编译或固件烧录因退出码非零（`Exit Code != 0`）发生逻辑崩溃时，物理 PTY 窗格绝对不杀，保持错误现场。
        
    - **冷启动对账**：系统断电重启后，Daemon 扫描所有处于非终结态的任务，禁止使用 `tmux-resurrect`。它直接从 Postgres 中提取最后一次有效的 `COMPLETED` Checkpoint，指派全新 Tether Session UUID 无缝在物理断点处重跑接棒。
        

### Feature 2.4: 人工干预多端异步合闸门禁 (HITL Gate & Notification)

- **特性描述**：在流程发生编译中断或安全超权时，触发系统级非破坏性挂起，并在外部异步通信网关上弹出高密度的对账卡片供厂长审批。
    
- **技术规格**：
    
    - **非破坏性挂起**：将任务状态标记为 `SUSPENDED`。Daemon 咬死当前的 Tether 物理 PTY，保护内存上下文。
        
    - **异步双向网关**：Daemon 通过外网合规 Webhook 向厂长的 MS Teams/Telegram 发送标准的 Actionable MessageCard，Payload 结构强制包含：任务 UUID、拦截成因、16KB 截断后的 Stdout 现场以及一个 `Resume` 触发键。
        
    - **远程合闸响应**：厂长点击 `Resume` 后，回调消息投射进 Daemon 的轮询端口。Daemon 核对 Correlation ID 签名无误后，驱动对应的 Tether 窗格发送 `Ctrl+C` 释放挂起，并重新点火下发原任务，实现全异步、零断线的人工接管。
        

### Feature 2.5: 联邦式生命周期熔炼器 (Onboard / Offboard & Auto-Pruning)

- **特性描述**：管理产品蓝图的冷热数据状态转换，在产品下线时将全生命周期的运行轨迹熔炼成冷经验沉淀，并擦除大体积缓存。
    
- **技术规格**：
    
    - **联邦 Wiki 设计**：`configs/global_rules.md` 存放于 Immutable ROOT 中作为全局注入的 System Line。每个蓝图独占 `blueprints/<name>/openwiki/` 子目录用于存放项目专有 AST 脑图。
        
    - **下线熔炼机制 (Offboard)**：当厂长执行 `janus offboard --blueprint <name>` 时：
        
        1. Daemon 扫描 Absurd 数据库，抽取该蓝图历史上所有的 Task、Step 执行轨迹与 Tool Guard 历史拦截日志。
            
        2. 调用大模型将上述运行快照、引脚错误和避坑 Patch 压缩总结为高密度的 Markdown 文件，原子化写入 `./blueprints/<name>/openwiki/production_report.md`。
            
        3. **数据库防爆降解 (Melt Cache)**：调用后台 SQL 存储过程 `melt_blueprint_data('<name>')`，**彻底删除该蓝图对应的 `result_cache` JSON 大字段与终端 Stdout 日志**，仅保留一行基础元数据统计（Task ID，执行耗时）供后期审计。
            
        4. 通过影子客户端调用本地 Git 自动将 `production_report.md` 进行增量 Commit 与 Push，完成硅基经验的自我遗传。
            

## 3. 系统数据交换契约 (Data Contracts)

### Contract 3.1: 步骤持久化核心 schema (Absurd DB)

SQL

```
-- 核心步骤检查点表 (带 Size Budget 限制)
CREATE TABLE absurd_steps (
    task_id INTEGER REFERENCES absurd_tasks(id) ON DELETE CASCADE,
    step_name VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,            -- STARTING | COMPLETED | SUSPENDED
    result_cache JSONB,                     -- 严格限制物理存储体积，最大 16KB 截断
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, step_name)
);
```

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

## 4. 交付质量指标与异常熔断矩阵 (UAT & Fault Matrix)

|**异常事件边界**|**底层物理行为表现**|**系统级容错与熔断收敛方案**|
|---|---|---|
|**Tether 物理网络/SSH 中断**|远程编译服务器掉线，标准 `std::process` 发生管道读写挂起。|**触发 Session 冻结**：Janus Daemon 捕获不灭的底层句柄，在 Postgres 中将 Step 标记为 `SUSPENDED`。绝对不 kill 后台 tmux 窗格。当网络恢复后，通过重新建立 SSH 管道执行 `tether attach` 秒级唤醒现场。|
|**Agent 输出幻觉，死循环刷屏日志**|终端 Stdout 字符流瞬间产生每秒数兆的垃圾文本。|**物理 Size Budget 熔断**：`janus-sh` 在内存中内置流式计数器。当单次 Step 的 Stdout 字符累积超过 **16 KiB** 阈值时，自动执行硬核截断（Truncate）并附加 `[MetaMach Log Budget Exceeded]` 标签，阻止脏数据灌入 Postgres，彻底粉碎容量爆炸隐患。|
|**Unified DB 发生连接池崩溃**|宿主机 Postgres 遭遇极端物理内存溢出或容器意外闪退。|**状态机防爆降锁**：Janus Daemon 内部设计有局部内存级 SQLite 环形备份队列（Ring Buffer）。在 PG 连接断开期间，所有的过渡态 Step 变更优先原子写入本地 `HERDR_PLUGIN_STATE_DIR/fallback.db`。当检测到主机 PG 容器恢复后，自动触发批量合并重放（Log Replay），确保车间生产不停工。|