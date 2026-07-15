
### ── 核心调度底盘、代理沙箱与耐久工作流的系统级质量保障方案

> **EN:** Test Spec — QA strategy and test cases for the scheduler, agent sandbox, and durable workflows.

## 1. 测试架构与策略总览 (Testing Strategy)

为了保障 **MetaMach 1.0** 工厂的高可用性与强抗震防爆能力，本测试用例设计严格遵循以下三层质量防线：

1. **沙箱与隔离防线（Sandbox Invariants）**：验证 `janus-sh` 代理拦截和 `Tool Guard` 对高危命令、敏感密钥的 100% 同步拦截与重定向。
    
2. **耐久与自愈防线（Durable Recovery）**：模拟网络超时、服务器断电等极端物理物理故障，验证 `Tether (remain-on-exit)` 的进程保留和 `Absurd Postgres` 的冷启动状态对账。
    
3. **生命周期与防爆防线（Lifecycle & Storage Budget）**：验证 `Offboard` 下线熔炼、日志 16KB 截断（Size Budget）以及数据库自动降解。
    

> **严重级别与发布门禁定义**：
> - **Blocker**：阻塞发布--该用例不通过则对应里程碑绝不可发布/并网。
> - **Critical**：必须在发布前修复；未修复不得进入下一里程碑。
> - **Major**：应予修复，可带已知问题发布，但须登记追踪并在下轮迭代关闭。

## 2. 测试用例详细设计 (Test Cases)

### Test Suite 2.1: 单例常驻控制中枢 (Janus Daemon & Twin-Client UI)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-01-01**|**Daemon 启动**|验证控制中枢能正常单例长跑，并在退出时清理 UDS 套接字|系统干净未运行 Janus|1. 运行 `janus-daemon` 启动后台进程。<br><br>  <br><br>2. 尝试再次运行 `janus-daemon`。|1. 在 `${HERDR_PLUGIN_STATE_DIR}` 下成功生成 `janus.sock` 与 `janus.pid`。<br><br>  <br><br>2. 第二次启动报 PID 锁冲突并安全退出，不破坏原 Socket。|**Blocker**|
|**UTC-01-02**|**影子端自愈**|验证 `herdr-janus` 在 Daemon 异常死亡时能执行惰性自启|`janus-daemon` 未启动，`janus.sock` 物理文件不存在|在 Herdr 终端中按下 `prefix+j` 唤醒 Dispatcher。|1. `herdr-janus` 在后台自动 `fork` 并 `exec` 启动 `janus-daemon`。<br><br>  <br><br>2. 成功渲染出 80% 宽度 Popup 界面，连接状态显示正常。|**Critical**|
|**UTC-01-03**|**Popup 键盘锁定**|验证 UI 弹窗能完全接管键盘输入流并支持 Esc 键退栈|厂长已通过 `prefix+j` 打开 Dispatcher 界面|1. 打开含 3 个蓝图选项的 Popup。<br><br>  <br><br>2. 连按 `↓` 10 次。<br><br>  <br><br>3. 连按 `Tab` 5 次。<br><br>  <br><br>4. 按下 `Esc`。|1. `↓` 10 次后高亮循环回到顶部，不逃逸。<br><br>  <br><br>2. `Tab` 5 次焦点在 Popup 内循环，不切到后台 tiled pane。<br><br>  <br><br>3. 全程无任何字符泄漏到后台终端。<br><br>  <br><br>4. `Esc` 关闭 Popup，焦点平滑还原。|**Major**|

|**UTC-01-04**|**陈旧 PID 恢复**|验证崩溃残留的陈旧 PID 文件不阻止重启|Daemon 曾崩溃，`janus.pid` 残留但进程已死|1. `kill -9` 当前 Daemon（不清理 pid）。<br><br>  <br><br>2. 重新启动 `janus-daemon`。|1. Daemon 检测 PID 已死，覆盖 `janus.pid` 并正常启动。<br><br>  <br><br>2. UDS 正常监听，无残留冲突。|**Critical**|
|**UTC-01-05**|**损坏 PID 容错**|验证 `janus.pid` 内容非法时优雅处理|`janus.pid` 内容为 `not_a_pid`|启动 `janus-daemon`。|Daemon 记 `WARN`，覆盖非法文件并正常启动，不崩溃。|**Major**|

### Test Suite 2.2: 内存级代理沙箱 (janus-sh & Tool Guard)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-02-01**|**Shell 代理重定向**|验证 Tether PTY 启动时 `SHELL` 环境变量被强行替换|启动 `dev-flow` 研发流水线|在 Tether 启动的 tmux 窗格中执行 `echo $SHELL`。|终端输出为 `janus-sh` 的绝对路径（`${HERDR_PLUGIN_ROOT}/bin/janus-sh`），而非系统的 `/bin/bash` 或 `/bin/sh`。|**Critical**|
|**UTC-02-02**|**同步命令拦截**|验证 `janus-sh` 能成功拦截并阻断未授权的敏感/危险指令|启动 `gatemetric` 的 `dev-flow` 任务|在 Agent 窗格中强行执行未在白名单中的命令：先建哨兵 `mkdir -p /tmp/metamach-test-guard-$(uuidgen) && echo s > /tmp/metamach-test-guard-$(uuidgen)/sentinel`，再执行命中黑名单的 `rm -rf /tmp/metamach-test-guard-$(uuidgen)`（或系统级 `esptool.py erase_flash` 模拟）。|1. 终端指令被同步挂起。<br><br>  <br><br>2. `janus-daemon` 日志触发拦截并返回拒绝执行（Status: Blocked），原始物理 Shell 完好无损，且哨兵文件事后仍然存在（未被删除）。|**Blocker**|
|**UTC-02-03**|**金融 Dry-Run 重定向**|验证高危操作在未经审批前被强制重定向为演练模式|启动金融级产品线再平衡流程|在未授权状态下，尝试执行发单命令：`hi5bot --action execute`。|1. `janus-sh` 在 UDS 同步对账中捕获该命令。<br><br>  <br><br>2. Tool Guard 强行将入参篡改替换为 `hi5bot --action dry-run` 交付给宿主 Shell。<br><br>  <br><br>3. 物理控制台仅生成对账单 Diff，未发生实质资金划拨。|**Blocker**|

|**UTC-02-04**|**UDS 协议健壮性**|验证 Daemon 对畸形/越权/超量 UDS 载荷不崩溃|Daemon 运行中|1. 向 `janus.sock` 发送非法 JSON（缺字段/坏 UTF-8）。<br><br>  <br><br>2. 1 秒内连发 1000 请求。<br><br>  <br><br>3. 发送 64KB 超大载荷。|1. 非法 JSON：Daemon 记 `WARN` 返回错误响应，不崩溃。<br><br>  <br><br>2. 高频请求：被限流，无 OOM。<br><br>  <br><br>3. 超大载荷：拒绝（消息过大）。|**Critical**|

### Test Suite 2.2b: Tool Guard 边界用例 (Edge Cases)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-02-05**|**白名单放行**|验证合规命令正常透传执行|Scout 岗位运行|执行 `ls -la`。|命令被 `ALLOW`，正常执行并返回结果。|**Major**|
|**UTC-02-06**|**部分匹配区分**|验证 `rm -rf /` 被拦而 `rm -rf ./build/` 放行|Coder 岗位运行|分别执行 `rm -rf /` 与 `rm -rf ./build/`。|前者 `BLOCK`，后者按 Coder 权限 `ALLOW`。|**Critical**|
|**UTC-02-07**|**命令链阻断**|验证 `rm -rf / && echo done` 的危险部分被拦|Coder 岗位运行|执行 `rm -rf / && echo done`。|整条链被 `BLOCK`，不部分执行 `echo`。|**Major**|
|**UTC-02-08**|**子 Shell 逃逸**|验证 `bash -c "rm -rf /"` 内层命令被识别拦截|Coder 岗位运行|执行 `bash -c "rm -rf /"`。|内层 `rm -rf /` 被识别并 `BLOCK`。|**Critical**|
|**UTC-02-09**|**环境变量展开**|验证 `RM_TARGET=/ && rm -rf $RM_TARGET` 展开后被拦|Coder 岗位运行|设 `RM_TARGET=/` 后执行 `rm -rf $RM_TARGET`。|Daemon 展开变量后命中黑名单并 `BLOCK`。|**Major**|

### Test Suite 2.3: 多 Agent 跨主机耐久工作流 (Distributed Durable Workflows)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-03-01**|**Absurd 事务幂等**|验证工作流 Step 在发生多次重试时不会产生脏数据|数据库处于连接状态|调度 `gatemetric` 连续执行 3 次 `make compile` 工位。|1. Postgres 物理表中仅保留 1 条 Task 记录与最新 Step 状态。<br><br>  <br><br>2. `result_cache` JSON 数据根据最新的成功状态完成覆盖，未产生冗余记录。|**Critical**|
|**UTC-03-02**|**跨主机进程保护**|验证网络断开/SSH 重启后，物理 tmux Session 完好不灭|任务在远程 SSH 编译主机运行|1. 以程序化方式切断到远程主机的网络（可自动化，无需物理拔线）：Linux `iptables -A OUTPUT -d <remote_host> -j DROP`，macOS `pfctl -e -f <(echo "block drop out to <remote_host>")`。<br><br>  <br><br>2. 等待 10 秒后删除规则恢复网络（`iptables -D OUTPUT -d <remote_host> -j DROP` / `pfctl -d`）并重新执行 `herdr-tether attach`。|1. 远程主机的编译进程没有被杀死（`remain-on-exit` 生效）。<br><br>  <br><br>2. 重新挂接后编译现场 100% 还原，数据不折损。|**Critical**|
|**UTC-03-03**|**冷启动自愈**|模拟系统物理断电，验证开机后从最后一个 Step 断点接棒|本地宿主机运行重型编译，任务状态为 `RUNNING`|1. 强行物理杀死 `postgres` 容器和 `janus-daemon`。<br><br>  <br><br>2. 重启 PG 容器（`docker compose up -d`），并直接重启 `janus-daemon` 触发冷启动自愈（**不走 `make bootstrap`**，以免全量重编译掩盖真实的冷启动代码路径）。|1. Daemon 拒绝使用 `tmux-resurrect`。<br><br>  <br><br>2. 从 Absurd Postgres 读出最后一次 `COMPLETED` 的 Step Checkpoint，分配全新 UUID 在物理断点处重跑接力。|**Critical**|

|**UTC-03-04**|**Daemon 崩溃恢复**|验证 Step 运行中 Daemon 崩溃后，tmux 现场存活且孤儿工位被正确处置|Step 处于 `RUNNING`，Daemon 为 tmux 会话父进程|`killall -9 janus-daemon`。|1. tmux 会话存活（remain-on-exit）。<br><br>  <br><br>2. `herdr-janus` 惰性重启 Daemon。<br><br>  <br><br>3. Daemon 扫描孤儿工位，置 `SUSPENDED` 并通知厂长。|**Critical**|
|**UTC-03-05**|**并发工作流隔离**|验证多蓝图并发派单互不串扰|2 个蓝图均 `ACTIVE`|同时为两蓝图派发 `dev-flow`。|1. 创建 2 个独立 tmux 会话、2 条独立 `absurd_tasks` 记录。<br><br>  <br><br>2. UDS 请求正确按 `task_id` 归属，`result_cache` 无跨蓝图污染。|**Critical**|

### Test Suite 2.4: 人工干预多端异步合闸门禁 (HITL Gate)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-04-01**|**非破坏性挂起**|验证编译中断或超权拦截时，物理现场保留，不杀进程|编译脚本故意写入一个语法错误使其失败|运行编译流水线并触发失败。|1. 数据库状态锁定为 `SUSPENDED`。<br><br>  <br><br>2. Tether 物理 tmux Session 挂起，错误现场、内存变量和控制台缓存不消失。|**Critical**|
|**UTC-04-02**|**异步双向审批**|验证移动端（Teams）接收高密卡片并执行合闸 Resume|配置了合规的外网 Teams/TG Webhook 密钥|1. 触发任务挂起。<br><br>  <br><br>2. 在手机 Teams 端阅读报错明细并点击 **`[🔄 Resume]`**。|1. Teams 秒级发出 Payload 至 Daemon 轮询端口。<br><br>  <br><br>2. Daemon 验证 Correlation ID 签名无误后，将状态由 `SUSPENDED` 置回 `RUNNING` 并派发下一工位（不发 `Ctrl+C`、不重跑被拦命令），流水线无感接棒。|**Major**|

|**UTC-04-03**|**Telegram 通知与合闸**|验证 Telegram Bot 通道接收卡片并 Resume（与 Teams 架构等价；每轮发布交替测一通道）|配置了 Telegram Bot Token 与 chat_id|1. 触发任务挂起。<br><br>  <br><br>2. 在 Telegram 端阅读卡片并点击 Inline Keyboard 的 **`[🔄 Resume]`**。|1. Telegram Bot 秒级发出 `sendMessage` + `inline_keyboard`。<br><br>  <br><br>2. 回调经 Webhook 投射至 Daemon，验证 Correlation ID 后派发下一工位。|**Major**|

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

### Test Suite 2.7: 性能与压测基准 (Performance Benchmarks)

|**用例编号**|**功能模块**|**测试目的**|**前置条件**|**测试输入与物理步骤**|**预期输出与物理表现**|**严重级别**|
|---|---|---|---|---|---|---|
|**UTC-07-01**|**Daemon 冷启动**|验证 Daemon 冷启动时延|系统干净未运行|启动 `janus-daemon`，测量至 `janus.sock` 就绪。|冷启动 ≤ 2s。|**Major**|
|**UTC-07-02**|**UDS 往返时延**|验证命令裁决往返 p50/p99|Daemon 运行中|发送 1000 次 `janus-sh` 裁决请求，统计往返时延。|p50 ≤ 10ms，p99 ≤ 50ms。|**Major**|
|**UTC-07-03**|**Popup 渲染时延**|验证 warm path 弹窗渲染|Daemon 已运行|按下 `prefix+j`，测量至 Popup 可交互。|≤ 100ms。|**Major**|
|**UTC-07-04**|**Step 历史查询**|验证大历史下的查询时延|1000 条 Step 记录|`janus status` 查询在途/历史任务。|查询 ≤ 500ms。|**Major**|

## 3. 测试环境配置与自动化并网验证 (Testing Environment)

### 3.1 自动化测试依赖工具

为执行上述系统级测试，车间宿主机必须安装并报备以下物理工具：

- **Docker & Compose (v2.20+)**：用于一键拉起 Absurd Postgres 测试容器。
    
- **Rust (v1.88+)**：用于本地锁版本编译测试二进制。
    
- **Tmux (v3.2+)**：Tether 维持会话不灭的底层依赖。
    
- **ngrok / Cloudflare Tunnel**：用于映射本地 `janus-daemon` 端口，实现 Teams / Telegram 外网 Webhook 回调接收测试。**自动化 CI 优先使用本地 Webhook 接收器**（在 localhost 起一个简单 HTTP server 记录收到的 webhook，测试直接 POST 到 localhost），避免依赖第三方隧道与公网暴露；真实 Teams/TG 集成在单独的人工 UAT 阶段验证。
    
- **容器化测试环境**：CI 中整套用例须跑在 `docker-compose.test.yml` 内（`metamach-test` 容器含 Rust/tmux/编译产物 + `metamach-db-test` Postgres），通过 `make test-integration`（`docker compose -f docker-compose.test.yml up --abort-on-container-exit`）编排，确保无裸金属/宿主机状态依赖。
    

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
    

按照本说明书执行完全部的 UAT 物理对账和压力故障测试后，您的 **MetaMach 1.0** 将具备真正意义上的“物理防爆、进程不灭、安全合规、自我进化”的企业级质量背书！