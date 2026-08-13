Workload Engine Design（已对齐 M4-4.1-design.md rev 3 + Phase 0a 实现）

> **状态：✅ 已实现 / 已解决。** 本设计的核心概念（workload engine 使用 absurd + tmux + DurableBackend）已在 Phase 0b 中实现（v0.4.2）。两个待讨论项已由其他文档覆盖：Context Filter → `Context-Manage-Design.md`（0.4.6 ADR 候选）；Agent Adapter → 当前使用 `janush -c "<cmd>" + JANUS_AGENT` 方案，无需额外适配器。

---

## 🏗️ 一、 Workload Engine 的整体架构拓扑

在 MetaMach 0.4.0 中，一个 Workload (Pipeline) 不是运行在单一 Agent 进程里的，而是由 `janus-daemon` 驱动 Absurd PG 状态机（通过 `janus/src/absurd/adapter.rs` 中的 `DurableEngine` trait），控制多个独立的 `janus::tmux` PTY 沙箱顺序或并行执行。

```text
 🧠 【 janus-daemon Orchestrator 】 ──(Durable State via Absurd PG)──┐
                                                                   │
 ┌─────────────────────────────────────────────────────────────────┘
 │
 ├── Step 1: 【 Agent Sandbox (janush + tmux) 】
 │     └─► 产生 stdout ──► 存入 Absurd PG Checkpoint (via DurableEngine::set_checkpoint)
 │                                            │
 ├── (Step 交接 / 16KB truncation) ◄──────────┘ (truncate_16k via janus/src/protocol.rs)
 │     └─► 下个 Step 从 metamach_step_meta.stdout_tail 读取上一步输出
 │
 └── Step 2: 【 Agent Sandbox (janush + tmux) 】
       └─► 注入上一步 Context ──► 物理下发 CLI 命令 ──► 执行

```

> **注：** 当前 M4-4.1 Phase 0b 的引擎设计（`janus/src/workflow.rs` 尚未实现）使用 `engine.spawn_task` + `engine.claim_task` (pull-mode) + `engine.set_checkpoint`。Step 间数据传递通过 `truncate_16k` 写入 `metamach_step_meta.stdout_tail` —— 简单可工作，没有复杂的 Filter 管道。下面的 Filter 设计是**可选增强**（待 ADR 讨论）。

---

## 🔄 二、 Agent 之间的 Output 传递与 Filter 实现 [🟠 待 ADR 讨论]

Agent（如 Claude Code, Codex, Pi）的输出通常是混合了自然语言、ANSI 色彩代码和 Markdown 代码块的**非结构化文本**。直接把 Agent A 的原始 stdout 丢给 Agent B，不仅浪费 Token，还会引发指令混乱。

因此，**可以考虑在 Agent 之间实现一个 Context Filter (上下文转换网闸)** —— 但这项功能在当前 M4-4.1 计划中**不包含**。当前引擎只使用 16KB 截断。

### 1. Filter 的三级提纯管道 (The 3-Stage Filter) — 设计草案

如果被采纳为 ADR，Filter 将作为纯函数运行在 `janus-daemon` 内部步骤交接处：

```rust
// 建议路径: janus/src/workflow/filter.rs（如 ADR 采纳）

pub struct ExecutionContextFilter;

impl ExecutionContextFilter {
    /// 将 Agent A 的输出转化为 Agent B 的输入 Prompt
    pub fn transform(raw_output: &str, strategy: FilterStrategy) -> String {
        match strategy {
            FilterStrategy::CodeBlockOnly => {
                let clean_text = strip_ansi_escapes::strip_str(raw_output);
                extract_markdown_code_blocks(&clean_text)
            }
            FilterStrategy::StructuredJson => {
                extract_json_payload(&raw_output).unwrap_or_else(|| raw_output.to_string())
            }
            FilterStrategy::TruncatedTail { max_bytes } => {
                apply_16kb_budget_truncation(raw_output, max_bytes)
            }
        }
    }
}
```

### 2. 在 Absurd PG 中的持久化传递 (Checkpoint Handover)

利用 `DurableEngine::set_checkpoint`，Agent A 的输出（经过 16KB 截断）被保存为不可变的 JSON 数据：

```rust
// janus/src/workflow.rs 中（Phase 0b 实现）
let agent_a_raw = ctx.step("step1_claude_code", || async {
    // 通过 DurableBackend::create_session 启动 tmux 会话
    // 命令通过 janush -c "<cmd>" + JANUS_AGENT=<step.agent> 分发
}).await?;

// 当前引擎：使用 truncate_16k（已实现）
let truncated = truncate_16k(&agent_a_raw);
engine.set_checkpoint(queue, task_id, "step1_claude_code", &serde_json::json!({
    "stdout_tail": truncated
}), run_id).await?;

// 如果采纳 Filter ADR，此处可替换为 ExecutionContextFilter::transform()
```

---

## 🔌 三、 异构 Agent (Claude Code / Codex / Pi) 的适配器 [⚠️ 待 ADR 讨论]

当前 M4-4.1 设计使用**统一 env-var 派发**：Step 命令通过 `janush -c "<command>"` 执行，`JANUS_AGENT=<step.agent>` 环境变量让 Tool Guard（`agents.toml`）解析正确的 Agent 权限配置。所有 Agent 类型被 Tool Guard 统一处理——不需要 Rust 级别的适配器。

下面的 `CliAgentAdapter` trait 是一种**更结构化的替代方案**：每个 Agent 类型有专用的 Rust adapter 知道其 CLI 标志。如果采纳，它将**在 Phase 0b 引擎之上**作为一层独立的 CLI 包装器。（Adapters 不改变引擎接口，只改变传递给 `DurableBackend::create_session` 的命令字符串。）

### 1. 统一的 Agent Trait 接口 — 设计草案

```rust
// 建议路径: janus/src/agent.rs（如 ADR 采纳）

#[async_trait]
pub trait CliAgentAdapter: Send + Sync {
    /// 转换为裸机 PTY 下发的 Shell 命令字符串
    fn build_exec_command(&self, prompt: &str, context_files: &[PathBuf]) -> String;
    
    /// 从原始 PTY 刷屏输出中提取逻辑 Return Code 或错误状态
    fn parse_completion_status(&self, raw_pty_output: &str) -> AgentCompletionStatus;
}
```

### 2. 各 Agent 的具体适配器 (Adapters) — 设计草案

* **Claude Code Adapter**：
```rust
pub struct ClaudeCodeAdapter;
impl CliAgentAdapter for ClaudeCodeAdapter {
    fn build_exec_command(&self, prompt: &str, _files: &[PathBuf]) -> String {
        format!("claude --print --dangerously-skip-permissions -p '{}'", prompt.replace('\'', "'\\''"))
    }
}
```

* **Codex / Aider Adapter**：
```rust
pub struct CodexAiderAdapter;
impl CliAgentAdapter for CodexAiderAdapter {
    fn build_exec_command(&self, prompt: &str, context_files: &[PathBuf]) -> String {
        let files_str = context_files.iter().map(|f| f.to_str().unwrap()).collect::<Vec<_>>().join(" ");
        format!("aider --message '{}' --no-auto-commits {}", prompt, files_str)
    }
}
```

* **Pi / Generic Shell Adapter**：
直接透传为 Shell 命令，不添加任何 Agent 特定标志。

> **ADR 决策点：** 当前 M4-4.1 引擎使用统一的 `janush -c "<cmd>"` 分发（所有 Agent 相同）。`CliAgentAdapter` 是否增加价值取决于：(a) 不同 Agent 的 CLI 差异是否大到需要专用适配器，(b) `agents.toml` 的 Tool Guard 权限模型是否足够处理这些差异。建议 Phase 0b 先用简单方式（env-var 派发），在积累了实际使用经验后再评估是否需要 Adapter 层。

---

## 🛡️ 四、 结合 MetaMach 防爆机制的 Workload 运行完整闭环

当 Agent A 输出传递给 Agent B 时，如果 Agent B 试图根据上游输出执行高危命令（例如 `rm -rf` 或物理串口烧录）：

1. **Step 1 (Agent Sandbox)**：执行代码分析 → 产生 Output → 通过 `truncate_16k` 截断并存入 `metamach_step_meta.stdout_tail`。
2. **Step 2 (Agent Sandbox)**：读取上一步输出并在 `janus::tmux` 中准备执行命令。
3. **`janush` 网闸拦截**：命令被 `janush` 物理捕获 → 发现包含敏感设备操作 → 触发 **30 秒 Fail-Closed 悬挂**。
4. **Absurd PG 挂起 (Suspend Task)**：Workload Engine 调用 `engine.await_event("hitl.approve:step_2", ...)`，在 Postgres 内部将 Pipeline 挂起，CPU 零占用。
5. **Teams/Telegram 远程合闸**：厂长在 Teams 收到 `janus::gateway` 推送的卡片（卡片内展示了经截断后的 Agent 输出与 Agent B 即将执行的指令）。
6. **通电唤醒**：点击 **Approve** → `engine.emit_event(...)` 触发 → Absurd PG 解冻 Pipeline → `janus::tmux` 物理下发命令！

---

## 🏁 总结对账

| 架构维度 | 当前实现方案 (M4-4.1 + Phase 0a) | 物理收益 |
| --- | --- | --- |
| **状态与持久化** | `DurableEngine` trait → Absurd PG (`absurd.sql`) Checkpoints | 死机重启/网络断开后，Pipeline 精确在中断的 Step 自动 Replay 恢复。 |
| **数据传递** | `truncate_16k` → `metamach_step_meta.stdout_tail`（已实现） | 16KB 硬截断，防止数据库膨胀。Filter 管道为可选增强（待 ADR）。 |
| **Agent 分发** | `janush -c "<cmd>"` + `JANUS_AGENT=<step.agent>` env var（Phase 0b） | 统一分发，Tool Guard 按 `agents.toml` 解析权限。Agent Adapter 层为可选增强（待 ADR）。 |
| **物理安全** | `janush` Interceptor + `engine.await_event` Suspension + `janus::gateway` | 链式传递过程中出现的任何高危命令，均触发 Fail-Closed 悬挂与人类合闸。 |

---

## 📋 命名对账（已修正）

| 原文（旧） | 修正后 | 原因 |
|---|---|---|
| `crates/janus-daemon/src/engine/` | `janus/src/workflow.rs` | MetaMach 是单 crate workspace，路径为 `janus/` |
| `crates/janus-daemon/src/adapters/` | `janus/src/agent.rs`（如 ADR 采纳） | 同上 |
| `janus_tmux::exec_agent()` | `DurableBackend::create_session()` | 实际 API（`janus/src/tmux/mod.rs`） |
| `janus_tmux::exec_command()` | `DurableBackend::create_session()` | 同上 |
| `mach-gateway` | `janus::gateway` | 当前代码命名（`janus/src/gateway/`） |
| `$HERDR_BIN_PATH` | `HERDR_PLUGIN_ROOT/bin/` | 不存在的 env var；真实的 Herdr 注入变量见 `docs/herdr-v1-contract.md` |

---

## 📝 待 ADR 讨论项

| 概念 | 状态 | 建议 |
|---|---|---|
| **Agent Adapter 层** (`CliAgentAdapter` trait) | ⚠️ 不包含在当前 M4-4.1 计划中 | Phase 0b 先使用统一 `janush -c` 分发；Adapter 层在积累实际使用经验后评估 |
| **Context Filter** (`ExecutionContextFilter` 三级管道) | 🟠 不包含在当前 M4-4.1 计划中 | 当前仅用 `truncate_16k`（已实现）；Filter 作为可选增强，需单独 ADR |
