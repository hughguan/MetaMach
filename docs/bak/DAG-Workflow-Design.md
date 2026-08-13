# DAG Workflow Design（Pipeline 层提案 — 0.4.9 演进）

 > **状态：✅ ADR-021 已批准 (0.4.9).** 详见 `docs/ADR.md` ADR-021。 
>
> **术语演进路径：**
> ```
> 0.4.0 (Phase 0b):  Blueprint → Workflow → [Steps]              ← 当前实现
> 0.5.0 (future):    Blueprint → Pipeline → [Workflow → [Steps]]  ← 本设计
> ```

---

## 🪐 术语定义（对齐 0.4.0 实现）

| 层级 | 概念 | 职责 | 0.4.0 状态 |
|---|---|---|---|
| **Blueprint** | 产品线配方 (`blueprints/<name>/janus.toml`) | 声明产品元数据：绑定一条默认 Workflow（或 Pipeline）、远程主机、OpenWiki 范围、Cognitive 配置。 | ✅ 已实现（`recipe::BlueprintRecipe`） |
| **Workflow** | 流水线 SOP (`workflows/<name>.toml`) | 扁平有序步骤列表（`[[steps]]`）+ Agent 分配。 | ✅ 已实现（`recipe::Workflow`） |
| **Pipeline** | DAG 编排层（0.5.0 新增） | 组合多个 Workflow 为带依赖编排的有向无环图（`needs:` 边），在工作流之间传递输入/输出，并按拓扑顺序执行。 | 📋 本提案 |
| **Step** | 单个执行动作 | 运行在 `janus::tmux` 沙箱中，通过 `janush` 代理执行 Shell 命令。 | ✅ 已实现（`recipe::WorkflowStep`） |

> **设计红线：** Workflow 和 Blueprint 保持其当前含义。Pipeline 不重命名任何内容——它位于 Blueprint 和 Workflow(s) 之间，作为一种新的**可选编排机制**。

---

## 🏗️ 1. 为什么需要 Pipeline？（从 0.4.0 到 0.5.0 的演进）

### 0.4.0 的扁平模型（当前）

```
gatemetric/janus.toml  →  default_workflow = "firmware-deploy"
                               │
                               ▼
                firmware-deploy.toml  →  [scout, compile, flash]
```

- ✅ 简单 — `for step in workflow.steps` 就是整个引擎
- ❌ 无复用 — 每个 Blueprint 把 `cargo build` 重写一遍
- ❌ 无法组合 — 两个工作流不能拼成一个更大的流水线

### 0.5.0 的 Pipeline 模型（提案）

```
gatemetric/janus.toml  →  pipeline = "full-release"
                               │
                               ▼
                full-release.toml（DAG）
               ┌─────────┼─────────┐
               ▼         ▼         ▼
        wf_compile   wf_audit   wf_flash
        (3 steps)    (2 steps)  (4 steps)
```

- ✅ 复用 — `wf_compile` 写一次，所有 Blueprint 共享
- ✅ 可组合 — 成熟的 Workflow 像乐高积木一样组装
- ✅ 工业隐喻 — "独立机床（Workflow）→ 组装线（Pipeline）→ 厂房（Blueprint）"

---

## ⚙️ 2. Pipeline 格式（草案）

Pipeline 是一个新的 TOML 文件类型（`pipelines/<name>.toml`），定义一个 Workflow 节点的 DAG：

```toml
# pipelines/full-release.toml

[pipeline]
name = "full-release"
description = "Full release pipeline: compile → audit → flash"

# ── Nodes: each references a workflow ──

[[nodes]]
id = "compile_stage"
workflow = "wf_compile_firmware"       # references workflows/wf_compile_firmware.toml

[[nodes]]
id = "audit_stage"
workflow = "wf_cross_audit_bin"
needs = ["compile_stage"]              # DAG edge: starts after compile_stage

[[nodes]]
id = "flash_stage"
workflow = "wf_physical_flash"
needs = ["audit_stage"]
hitl_gate = true                       # ⚠️ requires HITL approval before dispatch
```

### DAG 契约

- `needs:` 定义有向边 — `audit_stage` 在 `compile_stage` 完成之前不能开始
- 相同 `needs` 层次的节点可以**并行执行**（不同的 tmux 会话，不同的 Absurd 队列）
- 执行前进行循环检测 — 不允许出现循环
- 每个节点引用一个现有的 `Workflow`（`.toml` 中的扁平 `[[steps]]`）——Pipeline 本身不定义步骤

---

## 🛠️ 3. Workflow 单元（0.5.0 增强 — 可选）

> **注：** 以下增强（control_flow 指令、inputs/outputs）是对当前扁平 `[[steps]]` 格式的**可选扩展**。0.5.0 的 Pipeline 可以与未修改的 0.4.0 工作流（如当前的 `dev-flow.toml`）完全兼容，通过 `truncate_16k` 在 `metamach_step_meta.stdout_tail` 中进行步骤间数据传递。只有在 Workflow 需要明确分支/循环/契约化 I/O 时，才添加以下字段。

```toml
# workflows/wf_compile_firmware.toml（增强版，兼容 0.4.0 格式）

[workflow]
name = "wf_compile_firmware"

# ── 0.5.0 optional: typed I/O contract ──
[io]
inputs = ["src_dir", "target_board"]
outputs = ["bin_path", "checksum"]

# ── steps (0.4.0 compatible) ──
[[steps]]
name = "check_env"
agent = "deployer"
command = "test -d ${io.src_dir}"
# 0.5.0 optional: on_failure hook
on_failure = "abort"

[[steps]]
name = "build"
agent = "deployer"
command = "cargo build --release --target ${io.target_board}"
# 0.5.0 optional: retry loop
retry = { max = 3, delay_secs = 2 }

[[steps]]
name = "calc_hash"
agent = "deployer"
command = "sha256sum ${io.bin_path}"
# 0.5.0 optional: capture output into io.outputs
capture = { field = "checksum", from = "stdout" }
```

> **兼容性：** 没有 `[io]`、`retry`、`on_failure` 或 `capture` 字段的 0.4.0 工作流仍然可以工作。Pipeline 引擎仅按顺序执行步骤并通过 `truncate_16k` 传递 `stdout_tail`。这些增强功能可以在多个版本中逐步引入。

---

## 🪐 4. 与 MetaMach 0.4.0 架构的契合

```
                      【 Blueprint (产品配方) 】
                         janus.toml  ──binds──► pipeline = "full-release"
                                   │
                   ┌───────────────┘
                   ▼
          【 Pipeline (DAG 编排) 】
           pipelines/full-release.toml
                   │
          ┌────────┼────────┐
          ▼        ▼        ▼
   【 Workflow 】【 Workflow 】【 Workflow 】
    (有序步骤)   (有序步骤)   (有序步骤)
          │        │        │
          ▼        ▼        ▼
      🧠 janus-daemon (DurableEngine + DurableBackend)
          │        │        │
          ▼        ▼        ▼
     janus::tmux PTY 沙箱（每个独立运行）
          │
          ▼
     🐘 Absurd PG Checkpoints (via DurableEngine trait)
```

| 层级 | 引擎动作 | 实现方式 |
|---|---|---|
| **Pipeline** | 拓扑排序 → 并行/顺序调度节点 | `janus/src/pipeline.rs`（0.5.0 新增） |
| **Workflow** | 循环执行步骤，通过 janush 运行命令，checkpoint | `janus/src/workflow.rs`（Phase 0b） |
| **单个 Step** | 创建 tmux 会话，检查 exit code，截断 stdout | `DurableBackend::create_session` + `truncate_16k` |

---

## 🏁 总结

| 概念 | 0.4.0（已实现） | 0.5.0（本提案） |
|---|---|---|
| **Blueprint** | 绑定一个 Workflow | 绑定一个 Pipeline（或一个 Workflow — 向后兼容） |
| **Workflow** | 扁平有序步骤列表 | 不变 + 可选的 control_flow/I/O 扩展 |
| **Pipeline** | 不存在 | 引用 Workflow 的 DAG 编排层，带依赖边缘和并行执行 |
| **复用** | 每个 Blueprint 从零开始写步骤 | 共享的 Workflow 库，被所有 Blueprint 引用 |
| **复杂度** | ~100 行 Rust（顺序循环） | ~500 行 Rust（拓扑排序 + 并行调度器 + 数据传递） |

### ADR 决策点

| 问题 | 建议 |
|---|---|
| **Pipeline 是否应该是 0.5.0 的优先事项？** | 是的——在 0.4.0 Phase 0b 交付工作流引擎之后再启用。Pipeline 不会阻塞 0.4.0，但它是下一个架构层级。 |
| **Workflow 增强（控制流/I/O）是否应该随 Pipeline 一起交付？** | 可以分开交付。Pipeline 可以调度未修改的 0.4.0 工作流。控制流/I/O 增强可以为 0.5.1 或 0.6.0 预留。 |
| **新格式 TOML 还是 YAML？** | TOML — 与现有的 `workflows/*.toml` 和 `janus.toml` 保持一致。在单一代码库中使用多种格式会增加不必要的认知负担。 |
| **Pipeline 文件存放在何处？** | `pipelines/<name>.toml`（与 `workflows/` 并列） — 明确区分顺序工作流和 DAG 编排。 |
