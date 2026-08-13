# Configurable Agents — Provisioning, Quota & Fallback

> **状态：✅ ADR-019 已批准 (0.4.7).** 详见 `docs/ADR.md` ADR-019。

---

## 📄 一、 设计原则

### 现有 `agents.toml`（Tool Guard 权限）

```toml
# 当前格式——"这个 Agent 可以做什么？"
[agent.coder]
permissions   = ["read", "write", "edit", "bash-safe", "git-commit"]
bash_safe     = true
bash_blacklist = ["rm -rf /", "> /dev/sd*"]
```

### 提议扩展（Agent Provisioning）

```toml
# 0.4.8 新增可选 section——"什么支撑这个 Agent，失败时怎么办？"
[agent.coder.provision]
adapter = "claude-code"
command = "claude --print --dangerously-skip-permissions"
system_prompt = """
You are a coding agent in the MetaMach 0.4.0 pipeline.
Output clean diffs. Never execute destructive commands.
"""
[agent.coder.provision.quota]
max_tokens_per_day = 1_000_000
max_cost_usd_per_day = 10.0
max_requests_per_hour = 50
fallback_agent = "coder_backup"       # 当额度耗尽或 API 异常时自动降级
```

> **设计红线：** `provision` 是可选 section。没有它的 Agent 仍然可以工作——现有的 Tool Guard 权限完全不变。Provisioning 只是增加了"在调度时选择哪个 LLM、如何追踪配额、以及失败后回退到哪里"的能力。

---

## 📄 二、 完整配置示例

```toml
# configs/agents.toml —— 现有权限 + 0.4.8 新增 provisioning

# ── 架构师角色（Planner）──

[agent.architect]
permissions   = ["read", "write", "bash-safe"]
bash_safe     = true

[agent.architect.provision]
adapter       = "claude-code"
command       = "claude --print --dangerously-skip-permissions"
system_prompt = """
You are the Lead Architect for MetaMach 0.4.0.
Output must strictly follow Pipeline DAG TOML syntax.
"""
[agent.architect.provision.quota]
max_tokens_per_day   = 1_000_000
max_cost_usd_per_day = 10.0
fallback_agent       = "architect_backup"

# ── 架构师备份 ──

[agent.architect_backup]
permissions   = ["read", "write", "bash-safe"]
bash_safe     = true

[agent.architect_backup.provision]
adapter = "codex"
command = "codex --model deepseek-chat"
# 无配额限制——备份可以无限使用（或使用本地模型）

# ── 核心 Coding 角色 ──

[agent.coder]
permissions     = ["read", "write", "edit", "bash-safe", "git-commit"]
bash_safe       = true
bash_blacklist  = ["rm -rf /", "> /dev/sd*"]

[agent.coder.provision]
adapter = "aider"
command = "aider --model anthropic/claude-3-5-sonnet-20241022"
[agent.coder.provision.quota]
max_requests_per_hour = 50
fallback_agent        = "coder_backup"

# ── Coding 备份（本地模型，零成本）──

[agent.coder_backup]
permissions     = ["read", "write", "edit", "bash-safe", "git-commit"]
bash_safe       = true
bash_blacklist  = ["rm -rf /", "> /dev/sd*"]

[agent.coder_backup.provision]
adapter = "aider"
command = "aider --model ollama/qwen2.5-coder:32b"
# 本地 Ollama——物理离线、零成本、无条件可用

# ── Default（后备，无 provision）──

[agent.default]
bash_safe     = true
bash_blacklist = ["rm -rf /", "> /dev/sd*", "mkfs.*"]
# 没有 provision section——Tool Guard 权限已足够，无 LLM 调度
```

---

## ⚙️ 三、 Fallback 逻辑（AgentStack）

```rust
// janus/src/agent.rs（0.4.8 新增）

pub struct AgentStack {
    agents: HashMap<String, AgentConfig>,
}

impl AgentStack {
    /// 解析 configs/agents.toml（现有 Tool Guard 字段 + 新增 provision section）。
    pub fn load(path: &Path) -> Result<Self> { ... }

    /// 为给定 agent_id 解析活跃的 Agent 配置：
    /// - 如果 primary 有配额且未超限 → 返回 primary
    /// - 如果 primary 配额已超限且存在 fallback_agent → 递归解析 fallback
    /// - 如果整个链已耗尽 → 返回 None（Pipeline SUSPENDs，等待人工干预）
    pub fn resolve(&self, agent_id: &str) -> Option<&AgentConfig> {
        let primary = self.agents.get(agent_id)?;
        if self.quota_exceeded(agent_id) {
            if let Some(ref fallback_id) = primary.provision.as_ref()
                .and_then(|p| p.quota.as_ref())
                .and_then(|q| q.fallback_agent.as_deref())
            {
                return self.resolve(fallback_id);  // 递归回退链
            }
            return None;  // 链耗尽
        }
        Some(primary)
    }
}
```

### Fallback 触发条件

| 条件 | 行为 |
|---|---|
| `429 Too Many Requests` | 标记 `quota_exceeded` → 切换到 fallback |
| `402 Payment Required` | 同上 |
| 当日 Token / 请求数已达上限 | 同上 |
| Fallback 也失败 | Pipeline SUSPENDs → HITL（人工选择 Agent 或充值） |

---

## 🖥️ 四、 TUI Agent 状态面板（→ 0.4.9 Observer）

> **0.4.8 不包含此 Panel。** 此 TUI 面板属于 0.4.9 的 `herdr-janus` Observer 增强（见 `Observer-Design.md`），在 Agent Stack 配置和 Phase 0b 引擎就绪后添加。

```
┌── Agents Stack Status ──────────────────────────────────────────┐
│ Active: 5 | Quota Alert: 🟢 OK | Fallback Today: 1              │
├──────────────┬─────────┬──────────┬────────────────┬────────────┤
│ AGENT        │ ROLE    │ STATUS   │ QUOTA (24H)    │ TASK       │
├──────────────┼─────────┼──────────┼────────────────┼────────────┤
│ architect    │ Planner │ 🟢 ACTIVE │ 420K/1M ($4.20)│ Idle       │
│ arch_backup  │ Planner │ ⚪ STANDBY│ 0/Unlimited    │ -          │
│ coder        │ Coder   │ 🔴 EXHAUST│ 50/50 (LIMIT!) │ -          │
│ coder_backup │ Coder   │ ⚡ RUNNING│ Unlimited      │ wf_build   │
│ auditor      │ Auditor │ 🟢 ACTIVE │ 120/Unlimited  │ HITL Gate  │
├──────────────┴─────────┴──────────┴────────────────┴────────────┤
│ 🚨 [12:30:15] coder hit rate limit (429) → switched to backup  │
└─────────────────────────────────────────────────────────────────┘
```

数据来源：`AgentStack::runtime_states` 通过 UDS 暴露（新增 `Request::AgentStatus` → `Response::AgentStatus`）。

---

## 📋 0.4.8 ADR 评估

| 维度 | 评分 | 说明 |
|---|---|---|
| **价值** | 7/10 | 定义了 Agent 配置模型——Phase 0b 引擎需要知道"用什么 Agent、配额限制、以及失败时回退到哪里"。提前设计避免引擎实现时临时决策。 |
| **实施复杂度** | 3/10 | ~150 行 Rust（配置解析 + AgentStack 结构体 + 单元测试）。扩展已有的 `rules.rs` 解析器，不替换。 |
| **对 MM-CORE 的侵入性** | 1/10 | 纯配置层——不需要 daemon 运行，不需要 PG，不需要 tmux。 |
| **阻塞项** | 无——不需要 Phase 0b。Provisioning 配置在引擎存在之前就可以定义和验证。 |
| **与现有 agents.toml 兼容性** | ✅ 向后兼容——`provision` 是可选 section，现有的 Tool Guard 条目无需修改 |

### 与现有代码的关系

| 现有 | 0.4.8 变更 |
|---|---|
| `configs/agents.toml` | 不变——现有 Tool Guard 条目继续有效。新条目可以添加 `[agent.X.provision]` section。 |
| `janus/src/tool_guard/rules.rs` | 不变——继续解析 Tool Guard 字段。新 `agent.rs` 独立解析 provision section。 |
| `janus/src/paths.rs` | 不变——`agents_toml_path()` 已存在。 |

### Verdict

**✅ 强大的 0.4.8 候选。** 此设计在被 Phase 0b 引擎实际需要之前就定义了 Agent Provisioning 模型——引擎实现时有明确的"如何调度 Agent"参考，无需临时决策。配置先行，引擎随后。工作量低（~150 行 Rust），无阻塞项，不破坏现有 `agents.toml`。
