# Janus vs Herdr — Responsibility Split (Final)

> **Status: 架构定稿.** 经过深入的物理测试与架构推演，Janus 与 Herdr 在 MetaMach 0.4.0 体系中的分工与边界已经彻底厘清。通过摒弃双向依赖与 Herdr 侧的 AI 屏显检测，系统彻底消除了竞争条件与 Context 污染，实现了工业级的"控制面/真理源（Janus）与显示面/HMI（Herdr）彻底解耦"。

---

## 🏗️ 一、 顶层设计哲学 (Design Invariance)

> 📌 **核心契约：**
> * **Janus** 拥有**生命、身份与安全**（Life, Workload Identity & Safety Authority）。
> * **Herdr** 拥有**视图、布局与渲染**（View, Layout & Terminal Rendering Engine）。
> * **OpenSSH** 拥有**远程传输**（Remote Transport）。
> * **tmux** 拥有**物理持久化**（Durable Execution Engine）。

```text
 ┌──────────────────────────────────────────────────────────────────────┐
 │ 🖥️ Herdr (Viewport / HMI Layer)                                      │
 │   - 纯粹的终端 UI 显示屏：Panes, Tabs, Workspaces, ANSI 色彩渲染        │
 │   - 零 AI 语义猜测，零 Context 存储，纯粹接收 Janus 上报的状态进行高亮    │
 └──────────────────────────────────▲───────────────────────────────────┘
                                    │ 单向推报 (Push-only: report-agent)
 ┌──────────────────────────────────┴───────────────────────────────────┐
 │ 🧠 Janus Core (Workload Authority & Safety Engine)                   │
 │   - 独占 Workload Identity (`JANUS_WORKLOAD_ID`)，状态机与 Context 提纯 │
 │   - 驱动 janus::tmux 物理不灭，janush 物理网闸 (30s 悬挂/Teams 合闸)     │
 │   - 基于 Absurd PG (Postgres) 管理 Durable Step Checkpoints         │
 └──────────────────────────────────────────────────────────────────────┘
```

### 架构流向（Unidirectional Dataflow）

```
[ 厂长 / Agent Planner ] ──(下发 Blueprint)──► 【 janus-daemon 】
                                                     │
                               ┌─────────────────────┴─────────────────────┐
                               ▼ (启动 Task)                               ▼ (更新状态)
                【 janus::tmux PTY Sandbox 】                     【 Absurd PG Checkpoint 】
                 - 运行 Agent / 编译器 / esptool                            │
                               │                                           │ (推送渲染指令)
                               ▼ (命令拦截)                                 ▼
                       【 janush Shim 】 ──(触发 30s 悬挂)──► 【 herdr pane report-agent 】
                                                                           │
                                                                           ▼
                                                                【 Herdr TUI Viewport 】
                                                                - Sidebar: 🔴 BLOCKED
                                                                - Render ANSI Buffer
```

**单向数据流（Unidirectional Dataflow）**，Janus 主动推报，Herdr 纯粹消费。**不存在** Herdr → Janus 的事件订阅或 Janus 轮询 Herdr 状态。

---

## 🧱 二、 核心职责与物理边界对账矩阵

| 维度 | Janus (Workload & Safety Core) | Herdr (Terminal Viewport UI) | 边界隔离机制 |
|---|---|---|---|
| **物理定位** | 后台主控 Daemon (`janus-daemon`) + 网闸 (`janush`) | 前端 Terminal Multiplexer / HMI | **进程级隔离**：Herdr 崩溃或重启 100% 不影响 Janus 正在烧录的物理任务。 |
| **工作负载身份** | **独占** `JANUS_WORKLOAD_ID` 全局唯一标识与生命周期 | 仅记录本地 Pane ID / Workspace ID | **Identity 锚定**：SSH 重连后，Janus 根据 Workload ID 将状态重新挂载给 Herdr。 |
| **持久化底座** | **独占** `janus::tmux` 物理会话 + **Absurd PG Checkpoints** | 保存本地 `session-history.json`（仅用于 View 复原） | **持久化解耦**：Herdr 看到的仅是 `tmux` 客户端，物理 PTY 活体与 Step 状态存入 Absurd PG。 |
| **Agent 状态判断** | **唯一真理源**：结合 Agent Hook、PTY 输出与 `janush` 拦截判断 | **纯状态消费者**：只读，不进行屏显正则匹配或 AI 猜测 | **单向推报**：通过 `herdr pane report-agent` 接收 Janus 推送并点亮 Sidebar 🟢/🔴。 |
| **Context 管理** | **独占**：管理 OpenWiki、MCP 图谱、Token 截断与跨 Step 清理 | **完全剥离**：零 Context 管理，仅留屏显 ANSI Terminal Buffer | **干净解耦**：避免 ANSI 色彩码和乱码污染 AI 提示词，实现 0 次重复计费。 |
| **物理安全网闸** | **独占**：`janush` 30s 悬挂、Teams 卡片合闸、Fail-Closed 熔断 | **零参与**：仅展示 `BLOCKED` 高亮，并提供厂长 Attach 直连的窗口 | **控制与显示分离**：HMI 显示屏无权绕过或决策物理安全规则。 |

---

## 🔄 三、 状态上报管道 (Push-Only State Reporting)

由于 Herdr 无法穿透 `tmux` 的 PTY 隔离，**Janus 在状态变更时主动向 Herdr 的 API Socket 进行单向推报**：

| 事件 | Janus 动作 | Herdr 响应 |
|---|---|---|
| **Agent 开始工作** | `herdr pane report-agent <pane_id> --state working` | Sidebar 高亮 🟡 |
| **触发物理合闸/悬挂** | `herdr pane report-agent <pane_id> --state blocked` | Sidebar 高亮 🔴 + 展示 BLOCKED 横幅 |
| **Step 完成** | `herdr pane report-agent <pane_id> --state idle` | Sidebar 恢复 ⚪ |
| **Step 元数据** | `herdr pane report-metadata <pane_id> --token summary="wf_flash_m5"` | Pane 标题更新 |

### 0.4.5 实现状态

| 推报路径 | 状态 |
|---|---|
| `herdr pane report-agent` | 🔜 设计定稿，尚未实现——当前 herdr-janus 通过 UDS 轮询 `Progress` 达到近似效果 |
| `JANUS_WORKLOAD_ID` | ✅ 等效于 `JANUS_TASK_ID`（absurd 生成的 UUID），注入到每个 tmux 会话。跨 Janus/Herdr 的 Pipeline 执行标识符。 |
| 单向推报模型 | ✅ 架构已对齐——daemon 不知道 Herdr 的存在（`paths.rs` 可独立运行）。当前实现为轮询；推报模型是未来演进方向。 |

---

## 🧠 四、 Context 外科手术式管理

* **跨 Step 重置**：当 Pipeline 从 Step 1（编译）进入 Step 2（烧录）时，**Janus 强制清空 Agent 的 Context Window**——新 tmux 会话，新 Agent 进程。
* **磁盘落盘中转**：大规模编译日志或 Flash 校验数据，Janus 要求 Agent 写入 `/tmp/metamach/` 磁盘文件，仅将精简路径与 Key-Value 产物存入 Absurd PG。
* **16KB 截断**：所有 Step 输出通过 `truncate_16k` 硬截断，存储到 `metamach_step_meta.stdout_tail`。
* **Herdr 零负担**：Herdr 只管把屏显字符渲染在终端上，给厂长眼睛看，**绝对不把 Terminal Buffer 扔回给 LLM**。

---

## 🏁 五、 三大铁律

1. **Janus 是"大脑与闸门"**：拥有绝对的状态真理权、Context 主权与物理安全控制权，确保哪怕 SSH 断线、Herdr UI 挂掉，Richmond Hill 车间的硬件烧录流水线依然安全、不间断地持久运行。
2. **Herdr 是"眼睛与画板"**：剥离所有复杂且易错的 AI 状态猜测与 Context 负担，回归极致的 Terminal Multiplexer 本质，专注于多 Pane 布局、ANSI 渲染与 Handoff Attach。
3. **彻底取消双向订阅**：Janus 负责生产状态并**单向 Push**（`herdr pane report-agent`），Herdr 负责消费状态并纯粹 Display。架构达到最大程度的强健与干净。

---

## 📋 六、 0.4.5 对齐检查

| 铁律 | 实现状态 |
|---|---|
| **Janus 是大脑与闸门** | ✅ `workflow.rs` 拥有 Step 生命周期；`janush` 拥有命令拦截；`janus::gateway` 拥有 HITL 合闸 |
| **Herdr 是眼睛与画板** | ✅ `herdr-janus` 是 thin TUI 客户端——轮询 Progress，渲染 Dispatch/Progress 视图，无 AI 语义 |
| **取消双向订阅** | ✅ daemon 通过 `paths.rs` 独立于 Herdr；herdr-janus 通过 UDS 单向轮询。未来演变为 Janus → Herdr 推报模型 |
| **JANUS_WORKLOAD_ID** | ✅ `JANUS_TASK_ID`（absurd UUID）服务于此目的——注入到 tmux 会话，存储在 `metamach_step_meta` 中 |
| **Context 隔离** | ✅ 每个 Step 独立 tmux 会话；`truncate_16k` 截断；Agent 进程不跨 Step 共享历史 |
| **进程级隔离** | ✅ `janus::tmux` 会话在 Herdr 关闭后仍然存在（`remain-on-exit`）；Coldstart 从 PG 恢复 |
