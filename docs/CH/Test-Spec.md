
### ── 核心调度底盘、代理沙箱与耐久工作流的系统级质量保障方案

## 1. 测试架构与策略总览 (Testing Strategy)

为了保障 **MetaMach 2.0** 工厂的高可用性与强抗震防爆能力，本测试用例设计严格遵循以下三层质量防线：

1. **沙箱与隔离防线（Sandbox Invariants）**：验证 `janus-sh` 代理拦截和 `Tool Guard` 对高危命令、敏感密钥的 100% 同步拦截与重定向。
    
2. **耐久与自愈防线（Durable Recovery）**：模拟网络超时、服务器断电等极端物理物理故障，验证 `Tether (remain-on-exit)` 的进程保留和 `Absurd PG` 的冷启动状态对账。
    
3. **生命周期与防爆防线（Lifecycle & Storage Budget）**：验证 `Offboard` 下线熔炼、日志 16KB 截断（Size Budget）以及数据库自动降解。
    

## 2. 测试用例详细设计 (Test Cases)

### Test Suite 2.1: 单例常驻控制中枢 (Janus Daemon & Twin-Client UI)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-01-01**|**Daemon 启动**|验证控制中枢能正常单例长跑，并在退出时清理 UDS 套接字|系统干净未运行 Janus|1. 运行 `janus-daemon` 启动后台进程。<br><br>  <br><br>2. 尝试再次运行 `janus-daemon`。|1. 在 `${HERDR_PLUGIN_STATE_DIR}` 下成功生成 `janus.sock` 与 `janus.pid`。<br><br>  <br><br>2. 第二次启动报 PID 锁冲突并安全退出，不破坏原 Socket。|**Blocker**|
|**UTC-01-02**|**影子端自愈**|验证 `herdr-janus` 在 Daemon 异常死亡时能执行惰性自启|`janus-daemon` 未启动，`janus.sock` 物理文件不存在|在 Herdr 终端中按下 `prefix+j` 唤醒 Dispatcher。|1. `herdr-janus` 在后台自动 `fork` 并 `exec` 启动 `janus-daemon`。<br><br>  <br><br>2. 成功渲染出 80% 宽度 Popup 界面，连接状态显示正常。|**Critical**|
|**UTC-01-03**|**Popup 键盘锁定**|验证 UI 弹窗能完全接管键盘输入流并支持 Esc 键退栈|厂长已通过 `prefix+j` 打开 Dispatcher 界面|1. 在弹窗中使用方向键选择 Blueprint。<br><br>  <br><br>2. 按下 `Esc` 键。|1. 弹窗支持高亮切换，焦点不逃逸至后台 tiled pane。<br><br>  <br><br>2. 按 `Esc` 弹窗安全关闭，Herdr TUI 焦点平滑还原。|**Major**|

### Test Suite 2.2: 内存级代理沙箱 (janus-sh & Tool Guard)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-02-01**|**Shell 代理重定向**|验证 Tether PTY 启动时 `SHELL` 环境变量被强行替换|启动 `dev-flow` 研发流水线|在 Tether 启动的 tmux 窗格中执行 `echo $SHELL`。|终端输出为 `target/release/janus-sh`，而非系统的 `/bin/bash` 或 `/bin/sh`。|**Critical**|
|**UTC-02-02**|**同步命令拦截**|验证 `janus-sh` 能成功拦截并阻断未授权的敏感/危险指令|启动 `gatemetric` 的 `dev-flow` 任务|在 Agent 窗格中强行执行未在白名单中的命令：先建哨兵 `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`，再执行命中黑名单的 `rm -rf /tmp/metamach-test-guard-$(uuidgen)`（或系统级 `esptool.py erase_flash` 模拟）。|1. 终端指令被同步挂起。<br><br>  <br><br>2. `janus-daemon` 日志触发拦截并返回拒绝执行（Status: Blocked），原始物理 Shell 完好无损，且哨兵文件事后仍然存在（未被删除）。|**Blocker**|
|**UTC-02-03**|**金融 Dry-Run 重定向**|验证高危操作在未经审批前被强制重定向为演练模式|启动金融级产品线再平衡流程|在未授权状态下，尝试执行发单命令：`hi5bot --action execute`。|1. `janus-sh` 在 UDS 同步对账中捕获该命令。<br><br>  <br><br>2. Tool Guard 强行将入参篡改替换为 `hi5bot --action dry-run` 交付给宿主 Shell。<br><br>  <br><br>3. 物理控制台仅生成对账单 Diff，未发生实质资金划拨。|**Blocker**|

### Test Suite 2.3: 多 Agent 跨主机耐久工作流 (Distributed Durable Workflows)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-03-01**|**Absurd 事务幂等**|验证工作流 Step 在发生多次重试时不会产生脏数据|数据库处于连接状态|调度 `gatemetric` 连续执行 3 次 `make compile` 工位。|1. Postgres 物理表中仅保留 1 条 Task 记录与最新 Step 状态。<br><br>  <br><br>2. `result_cache` JSON 数据根据最新的成功状态完成覆盖，未产生冗余记录。|**Critical**|
|**UTC-03-02**|**跨主机进程保护**|验证网络断开/SSH 重启后，物理 tmux Session 完好不灭|任务在远程 SSH 编译主机运行|1. 手动断开本地网络（或临时拔掉网线/关闭 VPN）。<br><br>  <br><br>2. 等待 10 秒后恢复网络并重新执行 `tether attach`。|1. 远程主机的编译进程没有被杀死（`remain-on-exit` 生效）。<br><br>  <br><br>2. 重新挂接后编译现场 100% 还原，数据不折损。|**Critical**|
|**UTC-03-03**|**冷启动自愈**|模拟系统物理断电，验证开机后从最后一个 Step 断点接棒|本地宿主机运行重型编译，任务状态为 `RUNNING`|1. 强行物理杀死 `postgres` 容器和 `janus-daemon`。<br><br>  <br><br>2. 重启 PG 容器（`docker compose up -d`），并直接重启 `janus-daemon` 触发冷启动自愈（**不走 `make bootstrap`**，以免全量重编译掩盖真实的冷启动代码路径）。|1. Daemon 拒绝使用 `tmux-resurrect`。<br><br>  <br><br>2. 从 Absurd PG 读出最后一次 `COMPLETED` 的 Step Checkpoint，分配全新 UUID 在物理断点处重跑接力。|**Critical**|

### Test Suite 2.4: 人工干预多端异步合闸门禁 (HITL Gate)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-04-01**|**非破坏性挂起**|验证编译中断或超权拦截时，物理现场保留，不杀进程|编译脚本故意写入一个语法错误使其失败|运行编译流水线并触发失败。|1. 数据库状态锁定为 `SUSPENDED`。<br><br>  <br><br>2. Tether 物理 tmux Session 挂起，错误现场、内存变量和控制台缓存不消失。|**Critical**|
|**UTC-04-02**|**异步双向审批**|验证移动端（Teams）接收高密卡片并执行合闸 Resume|配置了合规的外网 Teams/TG Webhook 密钥|1. 触发任务挂起。<br><br>  <br><br>2. 在手机 Teams 端阅读报错明细并点击 **`[🔄 Resume]`**。|1. Teams 秒级发出 Payload 至 Daemon 轮询端口。<br><br>  <br><br>2. Daemon 验证 Correlation ID 签名无误后，向 PTY 发送 `Ctrl+C` 释放，流水线无感接棒。|**Major**|

### Test Suite 2.5: 联邦式生命周期熔炼器 (Onboard / Offboard & Auto-Pruning)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-05-01**|**Size Budget 截断**|验证 Agent 输出死循环刷屏日志时，大 JSON 缓存不撑爆数据库|运行一个无限输出 `Hello World` 垃圾日志的脚本|调度流水线捕获该任务输出，并执行入库事务。|1. 写入 Postgres 物理表的 `result_cache` 大小被强制限制在 **16 KiB** 预算内。<br><br>  <br><br>2. 截断位置自动附加 `[MetaMach Log Budget Exceeded]` 标签，保护数据库不崩溃。|**Major**|
|**UTC-05-02**|**下线降解熔炼**|验证产品 Offboard 时，自动凝炼经验脑图并清空大 JSON 缓存|项目 `gatemetric` 开发完成，积累了 200MB 的历史 Steps 日志|运行 `janus offboard --blueprint gatemetric`。|1. 数据库自动触发存储过程 `melt_blueprint_data`，**100% 物理擦除过期的 Step JSON 大字段**。<br><br>  <br><br>2. 在 `gatemetric/openwiki/` 下自动沉淀生成 `production_report.md`，内含引脚修复、报错审计等少样本（Few-shot）进化脑图。|**Critical**|
|**UTC-05-03**|**Git 经验遗传**|验证生成的质检报告能自动增量 Commit，形成免疫自愈|熔炼器成功吐出 `production_report.md`|执行 Offboard 结算，并重新 onboard 启动下一个周期的开发。|1. 物理目录中的报告自动执行本地 `git commit` 并推送到 GitHub 远端。<br><br>  <br><br>2. 新一代 Agent 进场扫描该脑图后，在 System Prompt 中自动获得“避坑”抗体，编译一次性通过。|**Major**|
|**UTC-05-04**|**蓝图上线与租户注册**|验证 `janus onboard` 能注册租户并使产品即时可派发，且操作幂等|零产品线的干净车间，`blueprints/joyrobots/janus.toml` 已就位|1. 执行 `janus onboard --blueprint joyrobots`。<br><br>  <br><br>2. 立即连续重复执行同一指令一次。|1. `blueprints` 表新增一行 `status='ACTIVE'` 记录，Popup 派单菜单即时出现 `joyrobots`。<br><br>  <br><br>2. 重复执行不产生第二行记录（`ON CONFLICT` 幂等），菜单无重复条目。|**Blocker**|
|**UTC-05-05**|**重新上线与经验遗传**|验证 Offboard 后重新 Onboard 能回收 `production_report.md` 为免疫抗体|`gatemetric` 已 Offboard 且 `production_report.md` 内含标记串 `PIN_CONFLICT_MARKER_21`|1. 执行 `janus onboard --blueprint gatemetric` 重新上线。<br><br>  <br><br>2. 通过 Daemon 调试端点导出该蓝图 Agent 的 System Prompt。<br><br>  <br><br>3. 派发一个历史曾触发引脚冲突的工位。|1. Onboard 成功，蓝图状态回到 `ACTIVE`。<br><br>  <br><br>2. 导出的 System Prompt 的 `## Previous Incidents` 段落必须包含 `PIN_CONFLICT_MARKER_21`。<br><br>  <br><br>3. Agent 不再尝试冲突引脚配置，工位一次性通过。|**Critical**|

### Test Suite 2.6: 工作流进度大盘 (Workflow Monitor)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-06-01**|**进度实时刷新**|验证进度大盘在 2s 内反映工位真实推进|一个多工位流水线（如 `dev-flow` 的 scout→code→cross_compile）正在运行|1. `prefix+j` 唤醒 Popup，`Tab` 切换到“进度”视图。<br><br>  <br><br>2. 观察工位状态随执行推进的变化。|1. 大盘按 1–2s 节拍刷新，工位状态依次变为 `PENDING → RUNNING → COMPLETED`，延迟 ≤ 2s。<br><br>  <br><br>2. `current_step` 与耗时字段随真实执行实时更新。|**Critical**|
|**UTC-06-02**|**挂起高亮与 Resume 入口**|验证 `SUSPENDED` 工位 1s 内高亮并附带恢复入口|流水线运行中，人为触发一次安全熔断（如越权指令）|触发熔断后立即观察进度大盘对应行。|1. 该工位在 1s 内高亮标红为 `SUSPENDED`。<br><br>  <br><br>2. 行尾渲染 `[A]ttach 现场` / `[R]esume` 快捷键入口，可成功 attach 或触发恢复。|**Major**|
|**UTC-06-03**|**`janus status` CLI 输出**|验证无 TUI 环境下 CLI 快照与大盘数据一致|至少一个在途任务存在|在 SSH 终端执行 `janus status` 与 `janus status --json`。|1. 纯文本输出列出所有在途任务的蓝图/工位/状态/耗时。<br><br>  <br><br>2. `--json` 输出符合 Contract 3.3 Payload 结构，与同时刻大盘显示一致。|**Major**|
|**UTC-06-04**|**多蓝图隔离展示**|验证多蓝图并行时大盘按蓝图独立分组、互不串扰|`joyrobots` 与 `gatemetric` 均为 `ACTIVE` 且各自有在途任务|同时为两个蓝图派发流水线，打开进度大盘。|1. 大盘按蓝图分组列出两条独立工作流，各自的工位状态互不串扰。<br><br>  <br><br>2. 每条工作流的 `task_id` 唯一，`result_cache` 无跨蓝图污染。|**Major**|

## 3. 测试环境配置与自动化并网验证 (Testing Environment)

### 3.1 自动化测试依赖工具

为执行上述系统级测试，车间宿主机必须安装并报备以下物理工具：

- **Docker & Compose (v2.20+)**：用于一键拉起 Unified Postgres 测试容器。
    
- **Rust (v1.88+)**：用于本地锁版本编译测试二进制。
    
- **Tmux (v3.2+)**：Tether 维持会话不灭的底层依赖。
    
- **ngrok / Cloudflare Tunnel**：用于映射本地 `janus-daemon` 端口，实现 Teams / Telegram 外网 Webhook 回调接收测试。
    

### 3.2 动静分离存储目录验证（UAT 物理对账）

在测试开始前，必须验证并确认以下三个物理路径无状态交叉污染：

- **只读根目录（Immutable ROOT）**：安装路径，存放静态配置与 OpenWiki `global_rules.md`。
    
    Bash
    
    ```
    # 验证该目录下不存在任何 sqlite.db, .env 等可写文件
    find ${HERDR_PLUGIN_ROOT} -name "*.db" -o -name ".env"
    ```
    
- **动态配置目录（Mutable Config）**：
    
    Bash
    
    ```
    # 验证敏感密钥、agents.toml 被软链接在此处
    ls -l ${HERDR_PLUGIN_CONFIG_DIR}/agents.toml
    ```
    
- **耐久状态目录（Mutable State）**：
    
    Bash
    
    ```
    # 验证本地 fallback.db、缓存 UDS janus.sock 在此处读写
    ls -l ${HERDR_PLUGIN_STATE_DIR}/janus.sock
    ```
    

按照本说明书执行完全部的 UAT 物理对账和压力故障测试后，您的 **MetaMach 2.0** 将具备真正意义上的“物理防爆、进程不灭、安全合规、自我进化”的企业级质量背书！