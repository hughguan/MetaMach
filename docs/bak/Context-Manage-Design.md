# Context Management Design — Stream Filter & Step Isolation

> **Status: ✅ ADR-018 已批准 (0.4.6).** 在现有的 16KB 截断基础上增加 ANSI 剥离和进度条折叠。Agent 局部上下文管理留给 Agent 自身处理。详见 `docs/ADR.md` ADR-018。
>
> **与实现对齐：** Phase 0b 已经提供了 `truncate_16k` 截断和 `metamach_step_meta.stdout_tail` 存储。本设计增加的是**截断之前的提纯层（Stream Filter）**。Hard Pruning（Token 阈值触发）推迟到 0.5.1+（需要 LLM 集成感知）。

---

## 1. 为什么不能完全依赖 Agent 自行管理？

单 Agent 工具（Claude Code, Codex, Pi）的上下文管理是**单 Step 维度的**。在 MetaMach 的多 Step Pipeline 中：

1. **终端脏数据过多**：串口/PTY 输出的 Flash 刷写日志（数千行进度条 `[=====>  ] 45%`）在 16KB 截断后仍然充满噪音。
2. **跨 Step 状态丢失**：Step A（编译）结束后，Step B（烧录）只需要编译产物的路径 + 校验和——不需要完整的 `cargo build` 输出。
3. **缺乏物理真理源**：Agent 自身的 Memory/Context 是易失的；Absurd PG（`metamach_step_meta`）是权威真理源。

---

## 2. MetaMach 的分层 Context 管理架构

```
+-----------------------------------------------------------------------+
| janus-daemon (MM-CORE)                                                |
|                                                                       |
|  🐘 Absurd PG ─── 权威持久化 (metamach_step_meta.stdout_tail)         |
|         │                                                             |
|         ▼ (上下文注入 / Context Invalidation)                          |
|  ┌──────────────┐     PTY Stream     ┌─────────────────────────────┐  |
|  │ Stream Filter│ ─────────────────> │ Agent (tmux sandbox)         │  |
|  │ (ANSI strip  │                    │ - 单个 Step 内部自主管理     │  |
|  │  + folding)  │                    │ - Janus 不干预微观思考       │  |
|  └──────────────┘                    └─────────────────────────────┘  |
+-----------------------------------------------------------------------+
```

### ① Janus 层：Stream Filter（0.4.7 新增）

在现有 `truncate_16k` **之前**增加一个轻量提纯层：

| 操作 | 输入 | 输出 |
|---|---|---|
| **ANSI 剥离** | `\x1b[32mCompiling...\x1b[0m` | `Compiling...` |
| **进度条折叠** | 100 行 `[=====>  ] 45%` | 1 行 `[Flash Progress: 45% → 100% SUCCESS]` |
| **重复行去重** | 50 行 `ACK ACK ACK` | 1 行 `ACK (×50)` |
| **16KB 截断** | （已存在）| 提纯后的文本，必要时带 `[Budget Exceeded]` 标签 |

实现方式：`janus/src/workflow/filter.rs`（新文件），纯函数，单元可测试。

### ② Janus 层：Step 切换时的上下文重置（Phase 0b 已实现）

* **Durable Step 隔离**：当 Pipeline 从一个 Step 流转到下一个 Step 时，Janus **不传递** Agent 的原始对话历史。
* **精简重载**：Step B 只读取 `metamach_step_meta.stdout_tail`（Step A 的 16KB 提纯输出），加上 Step B 自己的工作流定义。
* **tmux 会话隔离**：每个 Step 启动新 tmux 会话——Agent 进程从头开始，无历史污染。

### ③ Agent 节点层：局部自治（无变更）

在单个 Step 内部，Agent（Claude Code / Codex / Pi）完全自主管理其短期对话上下文。Janus 不干预。

---

## 3. Stream Filter 实现（0.4.7）

```rust
// janus/src/workflow/filter.rs（0.4.7 新增）

/// 从 PTY 原始输出中提取干净文本，用于 stdout_tail 存储。
/// 在 truncate_16k 之前调用——先提纯，再截断。
pub fn clean_pty_output(raw: &str) -> String {
    let mut out = strip_ansi_escapes(raw);
    out = collapse_progress_bars(&out);
    out = deduplicate_repeating_lines(&out);
    out
}

fn strip_ansi_escapes(s: &str) -> String { /* regex or byte-level strip */ }
fn collapse_progress_bars(s: &str) -> String { /* detect [==>  ] patterns */ }
fn deduplicate_repeating_lines(s: &str) -> String { /* 50× ACK → ACK (×50) */ }
```

集成到 `run_workflow` 中：

```rust
// workflow.rs — 在 capture_pane 之后、truncate_16k 之前
let raw_pane = backend.capture_pane(&session)?;
let cleaned = clean_pty_output(&raw_pane);
let stdout_tail = truncate_16k(&cleaned);
```

---

## 4. 与现有实现的对照

| 概念 | 当前状态 | 0.4.7 变更 |
|---|---|---|
| **16KB 截断** | `truncate_16k` 已实现（`protocol.rs`） | 无变更——Stream Filter 在其之前运行 |
| **stdout_tail 存储** | `metamach_step_meta.stdout_tail` 已实现（Phase 0b）| 无变更——存储的是提纯后的文本 |
| **Step 隔离** | 每个 Step 独立 tmux 会话（Phase 0b）| 无变更 |
| **ANSI 剥离** | 不存在 | ✅ 新增 `strip_ansi_escapes` |
| **进度条折叠** | 不存在 | ✅ 新增 `collapse_progress_bars` |
| **Hard Pruning**（Token 阈值）| 不存在 | ❌ 推迟至 0.5.1+（需要 LLM 集成感知） |

---

## 5. 0.4.7 ADR 评估

| 维度 | 评分 | 说明 |
|---|---|---|
| **价值** | 7/10 | 显著提升 HITL 卡片和 Progress 日志的可读性——纯噪音的 16KB 不如 2KB 结构化输出 |
| **实施复杂度** | 3/10 | ~100 行 Rust（纯函数，零依赖），在现有的 `capture_pane → truncate_16k` 管线中插入一个步骤 |
| **对 MM-CORE 的侵入性** | 1/10 | 新增一个文件（`workflow/filter.rs`），修改 `run_workflow` 中的一条调用——不改变任何 API 或协议 |
| **阻塞项** | Phase 0b（工作流引擎）——被阻挡但 Phase 0b 是本级提交 | |
| **测试** | 纯函数，完全可单元测试——ANSI 字符串输入，清洁文本输出，无外部依赖 | |

### Verdict

**✅ 0.4.7 的强候选。** Stream Filter 是低风险、高可测试性的纯函数层，它使已有的 16KB 截断变得更有用，不会改变任何 API 或数据库模式。Hard Pruning（Token 阈值触发）是具有更高基础设施要求的有价值后续功能——留待 0.5.1+。
