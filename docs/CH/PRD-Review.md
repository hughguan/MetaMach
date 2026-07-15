# PRD Review — Product Requirements Deep-Dive

> **Document under review:** `docs/CH/PRD.md`
> **Review lens:** Product completeness, user journey coherence, business logic gaps, scope creep, prioritization

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Missing critical user workflow; product unusable without resolution |
| 🟠 **HIGH** | Ambiguity that will cause rework or user confusion |
| 🟡 **MEDIUM** | Unclear spec; nice-to-have gaps |
| ⚪ **LOW** | Polish / terminology |

---

## 1. 🔴 Missing Core Workflows

### 1.1 No "Onboard a new Blueprint" workflow

The PRD describes Onboarding conceptually in §2.1 ("厂长在控制台激活产品线后，系统自动在 Unified DB 数据库中为其开辟独立的逻辑租户隔离空间") but never shows HOW the厂长 actually onboards a product.

- The User Journey §4 starts with "选择产品 `gatemetric`" — implying it already exists
- What does the厂长 do on Day 1 with zero blueprints?
- What is the onboarding command? `janus onboard --blueprint joyrobots`? A TUI menu? A config file?
- What inputs does onboarding require? (workflow binding? SSH host? repo path?)

**Recommendation:** Add a "Day 0: Onboarding a new Blueprint" section to the User Journey §4, showing the full flow from `janus onboard` → configure `janus.toml` → appears in Popup menu.

---

### 1.2 No "View workflow progress" workflow

The厂长 dispatches a workflow but has no way to check its status:
- Is it running? Which step?
- Where is the progress indicator?
- How does the厂长 know if it's stuck vs. running normally?

The Popup is described as a dispatch interface, but there's no "dashboard" or "status view" in the PRD.

**Recommendation:** Add a "Workflow Monitor" view to the functional matrix, showing active tasks, current step, elapsed time, and last stdout snippet. This is a P0 feature for operational awareness.

---

### 1.3 No "View production_report.md" workflow

The Offboard section says `production_report.md` is generated. How does the厂长 read it? Where? Is it shown in the Popup? Is it just a file on disk?

**Recommendation:** Add to the Offboard flow: after generation, the Popup shows a preview with an option to `[Open in Editor]` or the path is printed for manual review.

---

## 2. 🟠 User Journey Gaps

### 2.1 User Journey conflates two separate sessions into "上午 09:16"

> "厂长正在开会，手机 Teams 收到报警卡片。厂长阅读报错上下文后，通过终端 Popup 的分屏直接 Attach 进 AI 编译现场，手动修正了 C++ 头文件的引脚定义"

This describes the厂长 simultaneously:
1. In a meeting (mobile context)
2. Using terminal Popup split-screen to attach and edit code

These are contradictory. If the厂长 is in a meeting with only a phone, they cannot "终端 Popup 的分屏直接 Attach" (that requires a laptop with Herdr). The flow should either:
- **Mobile-only path:** Approve/Reject from phone → AI retries → if still failing,厂长 returns to laptop later
- **Laptop path:**厂长 is at desk, uses TUI to attach and fix

**Recommendation:** Split into two realistic scenarios:
- **Scenario A (Mobile):**厂长 in meeting → receives card → clicks `[Approve Manual Fix]` → system replies "Waiting for manual intervention at terminal" →厂长 returns to desk → attaches, fixes, resumes
- **Scenario B (Desk):**厂长 at terminal → receives card in TUI → attaches immediately → fixes → resumes

---

### 2.2 "未来" section is too vague

> "下一次 AI 进场时，将直接读取该报告，免疫同类引脚冲突错误。"

How exactly? This is a product promise with no mechanism:
- Is the报告 injected into the Agent's system prompt?
- Is it an OpenWiki RAG query that fires automatically?
- What if the agent is a different model that doesn't understand the report format?

**Recommendation:** Add a concrete mechanism description: "On next Onboard, OpenWiki indexes `production_report.md` and injects key failure patterns into the Agent's system prompt as few-shot examples under a `## Previous Incidents` section."

---

## 3. 🟡 Functional Matrix Issues

### 3.1 Priority assignments are inconsistent with system architecture

| Feature | PRD Priority | Architectural Reality |
|---------|-------------|----------------------|
| Popup 派单控制台 | P0 | Depends on M1+M2 — user-facing tip of a deep iceberg |
| 代理沙箱拦截 | P0 | Depends on M3 — janus-sh must exist first |
| 多端合闸 HITL | P0 | Depends on M4 — requires full workflow engine |
| 物理进程保护 | P1 | Actually foundational — without this, P0 features crash on network blip |
| 下线熔炼 | P2 | Labeled "low" but is a core differentiator in the product vision |

**Issue:** Tether (P1) is foundational to the "durable" value proposition. If it fails, the entire product experience degrades. It should be P0.

**Recommendation:** Re-evaluate: Tether → P0, HITL → P1 (can ship with TUI-only approval initially), Offboard → P1 (key differentiator).

---

### 3.2 Missing "Onboard" in functional matrix

The functional matrix has Offboard (P2) but no Onboard row. Onboarding is the first action a厂长 takes — it deserves its own row with UAT criteria.

**Recommendation:** Add:

| **蓝图一键上线 (Onboard)** | 注册新产品，分配租户空间 | 执行 Onboard 后，Popup 菜单即时出现新产品，可立即派发流水线 | **高 (P0)** |

---

### 3.3 UAT criteria are unmeasurable

- "弹窗响应时间 < 100ms" — how is this measured? Cold start or warm start? With Daemon already running or lazy-starting?
- "移动端卡片推送延迟 < 2s" — depends on external webhook latency (Teams/TG). Not controllable.
- "100% 成功挂起任务" — 100% is not a testable claim.

**Recommendation:** Use measurable, reproducible criteria:
- "Popup renders within 100ms when Daemon is already running (warm path)"
- "Lazy-start path (Daemon not running): Popup renders within 3s"
- "Mobile card push: webhook POST completes within 500ms (local measurement); end-to-end delivery depends on external service"
- "≥ 99.9% of unauthorized commands blocked in test suite (N=10,000)"

---

## 4. 🟡 Business Logic Gaps

### 4.1 What happens when a workflow fails without triggering HITL?

The PRD describes two states: success (continues) and HITL-triggered (suspends). But what about:
- Transient network error (SSH timeout)?
- Out-of-disk error?
- Agent produces valid but incorrect code that passes compilation but fails tests?

The fault matrix in Feature-Spec.md covers some of these, but the PRD should describe the user-visible behavior: "Workflow pauses, error card sent to厂长 with [Retry] / [Skip Step] / [Abort] options."

**Recommendation:** Add a "Workflow error handling" section describing the three failure modes from the厂长's perspective: transient (auto-retry 3x), blocking (HITL card), fatal (abort with report).

---

### 4.2 No multi-蓝图 management view

What if the厂长 has 5 active blueprints? How do they manage them? The Popup is described as selecting one blueprint and dispatching. Is there a list view? A status overview?

**Recommendation:** Add a "Factory Dashboard" concept showing all blueprints with status indicators (idle/running/suspended/completed).

---

## 5. ⚪ Terminology Polish

- "厂长" (Factory Director) is used 25+ times. Consider defining once and using consistently — some paragraphs use "您" (you) while others use "厂长". Pick one narrative voice.
- "Richmond Hill 车间" (Richmond Hill workshop) is charming world-building but may confuse readers who think it's a real physical requirement. Add a note: "Richmond Hill represents any deployment environment."

---

## Summary: PRD Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Add "Onboard a new Blueprint" workflow |
| 2 | 🔴 | Add "View workflow progress" feature to matrix |
| 3 | 🟠 | Fix contradictory mobile/laptop scenario in User Journey §4 |
| 4 | 🟠 | Add concrete mechanism for "knowledge inheritance" |
| 5 | 🟡 | Re-prioritize functional matrix (Tether → P0) |
| 6 | 🟡 | Add Onboard row to functional matrix |
| 7 | 🟡 | Make UAT criteria measurable |
| 8 | 🟡 | Add workflow error handling from厂长 perspective |
| 9 | 🟡 | Add multi-blueprint dashboard concept |
| 10 | ⚪ | Normalize厂长/您 narrative voice |

> **Resolution Log (2026-07-15):** The following action items have been addressed across the spec docs:
> - **#1 🔴 (Onboard workflow)** ✅ RESOLVED — PRD §2.1 (6-step Onboard) + §4 (Day 0 journey); ARCH §2.2(C); Feature-Spec §2.5 (Onboard spec); Project-Plan Task 4.3; Test-Spec UTC-05-04 / UTC-05-05; Review-Spec 指标 4.3 + REV-EVO-02; Deployment-Spec §6.4.
> - **#2 🔴 (View workflow progress in matrix)** ✅ RESOLVED — PRD §2.5 (Workflow Monitor) + §3 matrix row; ARCH §3 (progress UDS primitive + second popup mode) + §4 (progress query sequence); Feature-Spec §2.6 + Contract 3.3; Project-Plan Task 2.3; Test-Spec Suite 2.6 (UTC-06-01..04); Review-Spec 指标 2.3 + REV-OPS-01.
> - **#4 🟠 (knowledge inheritance mechanism)** ✅ RESOLVED — concrete `## Previous Incidents` few-shot injection on re-Onboard, specified in PRD §2.1/§4, ARCH §2.2(C), Feature-Spec §2.5; verified by Test-Spec UTC-05-05.
> - **#6 🟡 (Onboard row in matrix)** ✅ RESOLVED — Onboard row (P0) added to PRD §3.
> - **#9 🟡 (multi-blueprint dashboard)** ✅ RESOLVED — PRD §2.5 (多蓝图并列运营), Feature-Spec §2.6, Test-Spec UTC-06-04.
>
> Items #5, #7, #8, #10 remain open (🟡/⚪, not requested).
>
> **Round 3 (🟠 items, 2026-07-15):**
> - **#3 🟠 (contradictory mobile/laptop scenario)** ✅ RESOLVED - PRD §4 step 4 split into Path A (mobile-only: mark-for-manual-fix -> return to desk -> attach -> `metamach-resume`) and Path B (desk: attach immediately -> resume), aligned with the redesigned HITL resume.
>
> **Round 4 (🟡 items, 2026-07-15):**
> - **#7 🟡 (measurable UAT)** ✅ RESOLVED - PRD §3 matrix UAT cells now measurable (warm/lazy-start render times, ≥99.9% block rate N=10k, webhook POST ≤500ms local).
> - **#8 🟡 (workflow error handling)** ✅ RESOLVED - PRD §2.6 adds three failure modes (transient auto-retry 3x, blocking HITL card, fatal abort + partial report).
> - **#5 🟡 (reprioritize matrix)** ⏸️ DEFERRED - subjective product-priority call (Tether→P0 etc.), not a spec gap; left for product decision.
>
> **Round 5 (⚪ Low, 2026-07-15):**
> - **#10 ⚪ (厂长/您 narrative voice)** ✅ RESOLVED - PRD normalized to third-person "厂长" throughout (Director's Note + User Journey intro).
