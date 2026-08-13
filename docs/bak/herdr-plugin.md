在 Herdr 的原生插件架构标准下，**`herdr-janus`** 作为一个纯粹的 **Herdr Plugin**，其定位完全迎合了 Herdr 的设计哲学：**“Herdr 掌控宿主 UI 界面（Panes/Popups/Keybindings），插件掌控自身的代码执行、与后台 Daemon 的通信及状态。”**

结合你提供的 Herdr 官方 Plugin 规范，`herdr-janus` 的集成方案可以做到 **100% 极简、零浸润、原生并网**。

以下是 `herdr-janus` 作为 Herdr 插件的完美落地架构与配置文件设计：

---

## 📁 一、 `herdr-janus` 插件目录结构

根据 Herdr 规范，`herdr-janus` 不需要复杂的 SDK，只需作为一个标准的本地/GitHub 插件目录存在：

```text
herdr-janus/
├── herdr-plugin.toml    # 👈 Herdr 插件标准契约清单
└── bin/
    └── herdr-janus      # 👈 编译好的 Rust 影子客户端单二进制 (TUI Visualizer)

```

---

## 📄 二、 Manifest 契约声明 (`herdr-plugin.toml`)

利用 Herdr 的 `[[actions]]`、`[[panes]]` 和 `[[keys.command]]`，我们可以把 `herdr-janus` 声明为一个支持 **全局快捷键呼出 Popup 物理合闸弹窗** 的影子客户端：

```toml
id = "com.metamach.janus"
name = "MetaMach Janus"
version = "0.4.0"
min_herdr_version = "0.7.0"
description = "Shadow TUI View and HITL Verification Gateway for MetaMach 0.4.0"
platforms = ["linux", "macos"]

# 1. 声明构建命令（发布给社区时一键 cargo build）
[[build]]
command = ["cargo", "build", "--release"]
platforms = ["linux", "macos"]

# 2. 声明一个 Transient Popup 弹窗 (用于 HITL 拦截合闸)
[[panes]]
id = "interception-popup"
title = "🚨 MetaMach Safety Interception"
platforms = ["linux", "macos"]
placement = "popup"
width = "80%"
height = "60%"
command = ["./target/release/herdr-janus", "--mode", "popup"]

# 3. 声明一个全屏 / Split 侧边栏 View (用于日常观察 MM-CORE 状态)
[[panes]]
id = "dashboard"
title = "🪐 MetaMach Dashboard"
platforms = ["linux", "macos"]
placement = "split"
command = ["./target/release/herdr-janus", "--mode", "dashboard"]

# 4. 注册 Action：手动唤醒合闸 Popup
[[actions]]
id = "trigger-approval"
title = "Open MetaMach Approval Popup"
contexts = ["workspace"]
command = ["herdr", "plugin", "pane", "open", "--plugin", "com.metamach.janus", "--entrypoint", "interception-popup"]

# 5. 绑定快捷键：按下 Prefix + m 瞬间呼出 MetaMach 物理合闸弹窗
[[keys.command]]
key = "prefix+m"
type = "plugin_action"
command = "com.metamach.janus.trigger-approval"
description = "Trigger MetaMach HITL Approval Popup"

```

---

## 🔄 三、 运行期双向通信拓扑 (Communication Flow)

Herdr 与 `herdr-janus`，以及 `janus-daemon` 之间的物理交互链路如下：

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │ Herdr Terminal Emulator (Host Surface)                                 │
 │                                                                        │
 │  按下 `prefix + m`                                                      │
 │        │                                                               │
 │        ▼                                                               │
 │  根据 manifest 拉起 Popup 窗口 (80% x 60%)                             │
 │        │                                                               │
 │        ▼                                                               │
 │  【 herdr-janus (Popup Window Process) 】                               │
 └────────┼───────────────────────────────────────────────────────────────┘
          │
          │ 1. Herdr 注入环境变量 (HERDR_SOCKET_PATH, HERDR_PLUGIN_CONFIG_DIR)
          │ 2. 向局域网/本地 UDS 发起 IPC 订阅
          ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │ 🧠 janus-daemon (MM-CORE Background Process)                           │
 │                                                                        │
 │ - 当前 `janush` 捕获到危险操作 -> 状态转为 `requires_action`          │
 │ - 广播当前 Run ID & 待批准命令                                         │
 └────────┼───────────────────────────────────────────────────────────────┘
          │
          ▼
 【 herdr-janus Popup 红字渲染现场 】
 厂长敲击键盘 `y` (Approve) / `n` (Reject)
          │
          ├─► 向 janus-daemon 发送 UDS 通电/熔断信号 -> 解冻 janush
          │
          └─► 调用 Herdr 注入的 $HERDR_BIN_PATH 关闭弹窗：
              `herdr plugin pane close` (或进程 Exit 自动关闭 Popup)

```

---

## 🛠️ 四、 `herdr-janus` (Rust 代码) 内部对齐 Herdr 规范

在编写 `herdr-janus` 的 Rust 客户端时，充分利用 Herdr 注入的环境变量：

```rust
use std::env;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    // 1. 读取 Herdr 注入的标准环境变量
    let plugin_root = env::var("HERDR_PLUGIN_ROOT").unwrap_or_default();
    let config_dir = env::var("HERDR_PLUGIN_CONFIG_DIR").unwrap_or_default();
    let herdr_bin = env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string());

    // 2. 解析 CLI 参数 (例如 --mode popup)
    let args: Vec<String> = env::args().collect();
    let is_popup = args.contains(&"--mode".to_string());

    // 3. 连接 janus-daemon 的 UDS Socket 接收拦截日志与合闸请求
    let daemon_socket = format!("{}/.metamach/janus.sock", env::var("HOME")?);
    
    // 4. 渲染 TUI (使用 ratatui 等) 展现 16KB 日志与 y/n 提示
    let approved = render_tui_and_wait_user_input()?;

    if approved {
        send_uds_approve(&daemon_socket)?;
    } else {
        send_uds_reject(&daemon_socket)?;
    }

    // 5. 如果是在 Popup 模式下完成合闸，优雅退出，Herdr 弹窗会自动销毁解锁
    if is_popup {
        // 可选：显式调用 herdr CLI 命令保证清理
        let _ = Command::new(herdr_bin)
            .args(["plugin", "pane", "close"])
            .status();
    }

    Ok(())
}

```

---

## ⚡ 五、 本地开发与发布流 (Dev & Install Lifecycle)

### 1. Richmond Hill 本地开发调试 (Local Linking)

在 MetaMach 开发目录下，直接软链接到 Herdr 环境：

```bash
# 链接本地插件
herdr plugin link ~/metamach/crates/herdr-janus

# 查看 Action 是否生效
herdr plugin action list --plugin com.metamach.janus

# 手动测试拉起 Popup 弹窗
herdr plugin pane open --plugin com.metamach.janus --entrypoint interception-popup

```

### 2. 开源推广与社区分发 (Marketplace Distribution)

由于 Herdr 原生支持 GitHub 索引，我们可以直接将 `herdr-janus` 作为一个独立的 GitHub 仓库发布（例如 `metamach/herdr-janus`），并加上 `herdr-plugin` 标签。

全世界的极客和 Herdr 用户只需要在终端打入一行命令，就能瞬间完成 MetaMach 协同 UI 的挂载：

```bash
herdr plugin install metamach/herdr-janus

```

---

## 🏁 总结

这个集成方式极其惊艳地体现了 **MetaMach 0.4.0 的“解耦与复用”**：

1. **零污染**：Herdr 不需要为 MetaMach 改动一行 Core 代码；`janus-daemon` 也不需要包含任何重型 UI 渲染代码。
2. **像素级契约**：`herdr-janus` 利用 Herdr 的 `placement = "popup"`，精准实现了物理拦截时的 **弹窗红字合闸**，体验完爆普通的命令行 `readline`。
3. **完全合规**：用户可编辑的配置写入 `HERDR_PLUGIN_CONFIG_DIR`，运行状态存入 `HERDR_PLUGIN_STATE_DIR`，完全符合 Herdr 0.7.0+ 的生态信任模型！


简单、直接地回答：**在 0.4.0 架构下，MetaMach 彻底重构了底层设计，上述 `herdr-tether` 的绝大部分局限（Limitations）与痛点已经被“物理级消除”或“升级为更高维度的安全机制”；同时，部分硬核的安全原则（Safety Invariants）被 MetaMach 继承并强化。**

下面是针对 `herdr-tether` 这 12 条 Limitation 在 MetaMach 0.4.0 中的**逐条物理对账与升级分析**：

---

### 🛡️ 一、 已彻底解决 / 物理规避的痛点 (Resolved & Upgraded)

#### 1. 预算与日志截断 (Bounded Inputs & Over-budget)

* **`herdr-tether` 原局限**：数据超过 Budget 直接报错（`fail with actionable errors`），拒绝截断，需要人工介入。
* **MetaMach 0.4.0 改进**：**✅ 物理级 16KB 权威截断 (16KB Flow Budget)**。在 `janush`（Shell 代理层）和 `janus-daemon` 数据库写入前置设置了双重 **16 KiB 硬性截断线**。如果 Agent 陷入死循环刷屏，系统会**自动截断并追加 `[MetaMach Log Budget Exceeded]` 标识**，既保护了 Absurd PG/SQLite 数据库不被撑爆，又保证了控制流不会因为报错中断。

#### 2. 外部 tmux 会话安全边界 (External tmux Sessions)

* **`herdr-tether` 原局限**：对发现的外部 tmux 只能 Attach，不能破坏。
* **MetaMach 0.4.0 改进**：**✅ 物理隔离与命名空间锁线 (`metamach-tmux`)**。`janus::tmux` 物理引擎严格绑定专属 Server 实例：`tmux -L metamach-tmux`。MetaMach 绝对不侵入、不探测、不接管宿主机的任何全局 tmux 会话，实现了**物理级的零污染隔离**。

#### 3. Herdr 侧边栏与 Agent 状态 Fabricating (Sidebar Limitation)

* **`herdr-tether` 原局限**：Herdr 没有原生 API 注册嵌套 Agent，Tether 只能靠修改 Pane 标题，无法欺骗状态。
* **MetaMach 0.4.0 改进**：**✅ 彻底解耦，摒弃强行伪造**。`herdr-janus` 定位为纯粹的 **Shadow View (影子 TUI)** 和 **Transient Popup 弹窗**。真正的状态机在 `janus-daemon`（Absurd PG）内部，UI 仅做无状态渲染。我们不试图在 Herdr 侧边栏中“伪造”层次结构，而是直接通过标准的 Herdr Plugin `placement = "popup"` 和 `placement = "split"` 展示真实现场。

#### 4. SSH & Include 嵌套配置解析 (SSH Config Directives)

* **`herdr-tether` 原局限**：无法递归解析 SSH `Include` 配置文件。
* **MetaMach 0.4.0 改进**：**✅ 宿主机原生透传**。MetaMach 放弃了内置重型 SSH 解析器的幻想，所有的远程逻辑通过宿主机系统原生的 `ssh` / `janush` 执行。宿主机 Shell 能解析什么，`janush` 就能拦截什么，天然完美支持所有的 SSH `Include` 指令。

---

### 🏛️ 二、 被继承并强化的硬核安全原则 (Inherited & Retained)

#### 5. 关闭视图不等于销毁进程 (Closing a View is Non-Destructive)

* **MetaMach 状态**：**✅ 绝对强化为 SIGHUP Immunity**。在 `janus::tmux` 引擎中，所有 PTY 会话设置了 `remain-on-exit on`。厂长关闭 Herdr 窗口、关闭 `herdr-janus` 弹窗、甚至断开 SSH 链接，**物理 PTY 毫秒不停，物理烧录/编译绝不中断**。销毁（Stop）必须显式通过 `janus-daemon` 或 Teams 合闸网关下发。

#### 6. 状态未知的“Fail-Closed”逻辑 (Unknown is NOT Dead)

* **MetaMach 状态**：**✅ 升级为 30 秒 Fail-Closed 超时熔断**。当宿主机网络抖动、Teams 掉线或网关未响应时，`janus-daemon` 绝不假设危险操作可以放行。30 秒内未收到明确的 `Approve` 信号，`janush` 立刻断电封线并终止进程，坚决守住物理设备的防爆红线。

#### 7. 幂等性与恢复 (Lifecycle Recovery is Idempotent)

* **MetaMach 状态**：**✅ 借助 Absurd PG 实现真正的 Durable Execution**。因为 0.4.0 将状态托管给了基于 Postgres Stored Procedures 的 Absurd 引擎，所有的步骤（Step Checkpoints）和事件挂起（Events）天然具有幂等性与 CAS (Check-And-Set) 原子锁，死机重启后能够 100% 恢复上一次的完全状态。

#### 8. 文件原子写入与 0600 权限 (Atomic Writes & File Modes)

* **MetaMach 状态**：**✅ 完美继承**。`~/.metamach/db/` 和 `fallback.db` 等敏感文件与 UDS Socket (`janus.sock`) 在创建时均严格强制使用 `0600` 物理权限（仅当前用户可读写），确保多用户宿主机上的安全性。

---

### ⚠️ 三、 MetaMach 明确保留的物理现实 (Accepted Invariants)

#### 9. Tether/Janus 不是沙箱 (Tether is NOT a Sandbox)

* **物理现实**：**完全一致**。`janush` 是一个**拦截网闸（Gatekeeper & Interceptor）**，而不是虚拟机或 Docker 沙箱。它允许 Agent 在宿主机 Bare-Metal 环境下直接访问 `/dev/ttyUSB0` 等物理硬件。如果人类通过 Teams 手动点击了 `Approve (合闸)` 放行了危险命令，该命令将在宿主机物理执行。**MetaMach 提供防爆拦截与人类确认，但不提供虚拟隔离。**

#### 10. SSH 默认策略 (SSH Policy & BatchMode)

* **物理现实**：**完全一致**。MetaMach 遵循标准的 OS 级 SSH 安全契约（`BatchMode=yes`，严格 Host-Key 校验），绝不会为了“自动化方便”而去削弱或绕过系统的 SSH 安全设置。

---

### 🏁 总结

`herdr-tether` 是一个极具启发性的**轻量级外挂插件**，但受限于“纯客户端”的身份，它不得不做出许多妥协。

而 **MetaMach 0.4.0** 通过引入 **MM-CORE 极简双子星 (`janus-daemon` + `janush`)** 与 **Absurd PG 双轨底座**，彻底从“前台 UI 插件”进化为了**宿主机原生的物理防爆机床**：

> **`herdr-tether` 解决不了的“日志撑爆、进程崩溃失联、网关卡死”问题，在 MetaMach 0.4.0 中，已经全部被 16KB 流式截断、`janus::tmux` 进程不灭、以及 Absurd 状态机在物理层面上完美击穿！**

---

## Revision History

> **2026-07-21 — English translation and cross-check.** This Chinese original was reviewed, translated, and revised. The authoritative English versions are `docs/ADR.md` ADR-016 and `docs/Herdr-Integration.md` (206 lines). 11 corrections applied:

| # | Topic | Original (ZH) | Corrected |
|---|---|---|---|
| 1 | Pane placement | `placement = "popup"` | `placement = "overlay"` (M0-validated Herdr 0.7.3 enum) |
| 2 | Pane sizing | `width = "80%"`, `height = "60%"` | Removed — not valid Herdr 0.7.3 manifest fields |
| 3 | Herdr version | `min_herdr_version = "0.7.0"` | `"0.7.3"` (validated against installed version) |
| 4 | Plugin ID | `id = "com.metamach.janus"` | `"metamach.janus"` (matching manifest + tenant key) |
| 5 | Pane count | Two panes (popup + dashboard) | One pane (`dispatcher`) — internal Tab toggle handles view switching |
| 6 | Keybinding | `[[keys.command]]` in manifest | Configured in `~/.config/herdr/config.toml` (host-level) |
| 7 | Actions | `[[actions]]` with `herdr plugin pane open` | Removed — pane opens via Herdr CLI directly |
| 8 | Socket path | `~/.metamach/janus.sock` | `HERDR_PLUGIN_STATE_DIR/janus.sock` (paths::sock_path) |
| 9 | Env var | `HERDR_BIN_PATH` | Not a documented Herdr 0.7.3 env var — removed |
| 10 | CLI modes | `--mode popup` / `--mode dashboard` | No CLI modes — always ratatui TUI with internal View enum |
| 11 | Pane close | `herdr plugin pane close` called from plugin | Process exits → Herdr auto-closes overlay |

See `docs/Herdr-Integration.md` for the full revised English version, dependency maintenance strategy, and upgrade procedure.
