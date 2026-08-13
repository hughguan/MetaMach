# Observer Design — TUI Monitor Panel + Agent Planner

> **状态：✅ ADR-020 已批准 (0.4.8).** 详见 `docs/ADR.md` ADR-020。

---

## 🪐 顶层架构拓扑

```text
                               ┌────────────────────────────────────────────────────────┐
                               │          🪐 MetaMach 0.4.0+ Industrial Suite             │
                               └────────────────────────────┬───────────────────────────┘
                                                            │
    ============================ 1. CORE EXECUTION & CONTROL ============================
                                                │
                                                ▼
                                🧠 【 janus-daemon (MM-CORE) 】
                                - DurableEngine + Absurd PG
                                - Fail-Closed 30s Timeout
                                - janus::gateway (HITL ingress)
                                                │
                                               ┌┴──────────────────┐
                                               ▼                   ▼
                                【 janus::tmux 】      【 Storage & Checkpoints 】
                                - PTY Sandbox          - Absurd PG Engine
                                - Bare-metal HW        - fallback.db (SQLite)
                                               │
                                               ▼
                                   【 janush 】 (Interceptor)

    ====================== 2. REQUIRED (MANDATORY) UI SURFACE ========================
                                               │
                                               ▼
                             【 herdr-janus (enhanced Observer) 】
                             - TUI Native Monitor (Ratatui, existing)
                             - Dispatch view (existing)
                             - 🔜 Enhanced Progress view (0.4.9): HITL gate interaction
                             - 🔜 Live log stream (0.4.9): 16KB stdout_tail with ANSI strip
                             - Zero-State Transient View (SSH/Terminal Friendly)

    ====================== 3. OPTIONAL (OPT-IN) SURFACES ========================
                                               │
                                               ▼
                                   【 janus-studio 】
                                   - Optional Web Server (React Flow Canvas)
                                   - Optional Remote Web Observability
                                   - 0.6.0 candidate (see Canvas-Design.md)

```

---

## 🖥️ 一、 Observer Panel — 增强 `herdr-janus` Progress 视图（0.4.9 候选）

**核心洞察：** 不需要新的二进制文件。`herdr-janus` 已经有 Dispatch（Blueprint 选择）和 Progress（任务状态）两个视图。0.4.9 的 Observer 就是**增强的 Progress 视图**——增加 HITL 合闸交互和实时日志。

### 1. 当前状态（0.4.0 herdr-janus）

```
┌─ MetaMach Dispatcher ──────────────────────────────────────────┐
│ [Tab] Dispatch ↔ Progress                                      │
│                                                                 │
│ Progress view (current):                                        │
│   Task: gatemetric/firmware-deploy                              │
│   scout        COMPLETED                                        │
│   code         RUNNING                                          │
│   compile      PENDING                                          │
└─────────────────────────────────────────────────────────────────┘
```

数据来源：`Request::Progress { blueprint }` → `Response::Progress { active_tasks }` (UDS, 1-2s 轮询)。

### 2. 0.4.9 增强目标

```
┌─ MetaMach Observer ────────────────────────────────────────────┐
│ Blueprint: gatemetric [RUNNING]    Up: 02:45:12 | PG: 🟢       │
├──────────────────────────┬──────────────────────────────────────┤
│ 🏭 Pipeline / Workflow   │ 📜 Live Log (16KB, last COMPLETED)   │
│                          │                                      │
│  scout      [✔ COMPLETED]│ [12:00:01] Cargo build started...   │
│  code       [⏳ RUNNING ]│ [12:00:05] 16KB truncation OK        │
│  compile    [⚪ PENDING ]│                                      │
│                          │                                      │
│ ── HITL Gate ───────────│                                      │
│ 🚨 flash_serial SUSPENDED│                                      │
│ Command: esptool.py ...  │                                      │
│ Timeout: 24s             │                                      │
│ [y] Approve  [n] Reject │                                      │
├──────────────────────────┴──────────────────────────────────────┤
│ [Tab] Toggle View  [r] Refresh  [y/n] HITL  [q] Quit            │
└──────────────────────────────────────────────────────────────────┘
```

### 3. 增量变更（0.4.0 → 0.4.9）

| 变更 | 当前 | 0.4.9 | 工作量 |
|---|---|---|---|
| HITL 合闸交互 | 无——`herdr-janus` 无法审批 | `y`/`n` 键发送 `GateAction { run_id, action }` UDS 请求 → `janus::gateway` 回调路径 | ~50 行 Rust |
| 实时日志（16KB） | Progress 只显示状态 | 按 `Enter` 展开选中 Step 的 `stdout_tail`（已存在协议中） | ~30 行 Rust |
| SUSPENDED 高亮 | SUSPENDED 步骤为纯文本 | 红底高亮 + 倒计时显示 | ~20 行 Rust |
| tmux_alive 指示 | 始终为 false | Phase 0b 连接后变为真实值 | Phase 0b 依赖 |

**总工作量：~100 行 Rust，0 新依赖，0 新二进制文件。**

### 4. 适用于 0.4.9 的原因

| 因素 | 状态 |
|---|---|
| 需要 Phase 0b（工作流引擎）？ | ✅ 是的——`Progress` 数据来自于分派的工作流 |
| 需要 Pipeline DAG（0.5.0）？ | ❌ 不需要——增强的 Progress 视图可以渲染平面 `[[steps]]` |
| 需要新的二进制文件？ | ❌ 不需要——增强现有的 `herdr-janus` |
| 阻塞项？ | 仅 Phase 0b |

---

## 🎨 二、 可选项：Web Studio UI（0.6.0 候选）

另见 `docs/bak/Canvas-Design.md`。Web 可视化是可选功能——独立的 `janus-studio` 二进制文件。0.4.9 的 Observer 和 0.5.0 的 Agent Planner 在没有它的情况下仍然 100% 功能完整。

---

## 🏁 架构收益对账表

| 维度 | 旧思路（仅 Web UI） | 新架构（TUI Observer + 可选 Web Studio） | 收益 |
|---|---|---|---|
| **监控** | 需要浏览器 + Web 服务器 | TUI（增强的 herdr-janus），SSH 可用 | 离线、纯终端车间零依赖 |
| **编排** | 拖拽节点、连线、配置（手动） | Agent Planner（0.5.1，见 `Agent-Planner-Design.md`） | 对话生成 Pipeline TOML |
| **解耦** | Web 服务器崩溃影响监控 | TUI 是零状态影子客户端；Web 降级为可选 | 主控制环路零硬依赖 |
| **交付时间线** | 一个大的单一功能 | Observer（0.4.9）→ Planner（0.5.1）→ Studio（0.6.0） | 增量交付，每个步骤独立有价值 |

---

## 📋 0.4.9 ADR 评估

| 维度 | 评分 | 说明 |
|---|---|---|
| **价值** | 8/10 | HITL 从仅 Teams/Telegram 扩展到 TUI——车间中的关键安全功能 |
| **实施复杂度** | 2/10 | ~100 行 Rust 在 `herdr-janus` 中，0 个新依赖 |
| **对 MM-CORE 的侵入性** | 1/10 | 只需要 `GateAction` UDS 请求类型（~15 行协议）；gateway 回调路径已存在 |
| **阻塞项** | 中 | 需要 Phase 0b（Progress 数据 + SUSPENDED 状态）；被 Phase 0b 阻塞，不被 0.5.0 Pipeline 阻塞 |
| **整体** | **✅ 强大的 0.4.9 候选** — 高价值，低工作量，不阻塞 Pipeline DAG | |
