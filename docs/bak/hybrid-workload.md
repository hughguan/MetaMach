# Hybrid Workload — Remote Workloads in Unified TUI

> **Status: ✅ Implemented (v0.4.2).** ADR-017 已采纳，Phase 2 SSH transport 已交付。远程 Workload 使用同一 `TmuxBackend` + `ssh <host>` 前缀；janush ↔ daemon 连通性通过 SSH `-R` reverse tunnel 实现。远程主机仅需 tmux + janush。

---

## 🏛️ 一、 核心洞察：远程 = 本地 tmux + SSH 前缀

对 janus-daemon 来说，远程 tmux 会话和本地 tmux 会话**没有区别**。唯一的差异是 tmux CLI 调用前面加不加 `ssh <host>`：

```
# 本地
tmux -L metamach-tmux new-session -d -s <id> -c <cwd> 'janush -c "<cmd>"'

# 远程（完全相同的 tmux 命令，仅加 ssh 前缀）
ssh build-server tmux -L metamach-tmux new-session -d -s <id> -c <cwd> 'janush -c "<cmd>"'
```

所有 DurableBackend 操作（`create_session`、`poll_exit`、`capture_pane`、`kill_session`、`has_session`）都是 tmux CLI 调用——加不加 SSH 前缀，tmux 命令本身完全不变。

### 架构

```
                  🧠 janus-daemon（单实例）
                          │
              ┌───────────┼───────────┐
              ▼           ▼           ▼
         tmux ...    ssh host1    ssh host2
                     tmux ...     tmux ...
         (localhost) (remote)    (remote)
                          │
                          ▼
              🐘 Absurd PG（单实例，本地）
```

**远程主机仅需：tmux 3.3+ + janush 二进制。** 不需要 daemon，不需要 PG，不需要 Herdr。

---

## 🛠️ 二、 实现：TmuxBackend + host 前缀

无需新的 Backend 实现。`TmuxBackend` 在构造时接受一个可选的 `ssh_host`：

```rust
// janus/src/tmux/mod.rs（Phase 2 改动，~20 行）

pub struct TmuxBackend {
    tmux_cmd: String,  // "tmux" or "ssh <host> tmux"
}

impl TmuxBackend {
    pub fn new() -> Self { Self { tmux_cmd: "tmux".into() } }
    pub fn with_ssh(host: &str, user: Option<&str>) -> Self {
        let prefix = match user {
            Some(u) => format!("ssh -o BatchMode=yes -o ConnectTimeout=5 {u}@{host}"),
            None    => format!("ssh -o BatchMode=yes -o ConnectTimeout=5 {host}"),
        };
        Self { tmux_cmd: format!("{prefix} tmux") }
    }
}

impl DurableBackend for TmuxBackend {
    fn create_session(&self, id, cmd, cwd) {
        run("{} -L metamach-tmux new-session -d -s {} -c {} '{}'",
            self.tmux_cmd, id, cwd, cmd)
    }
    fn poll_exit(&self, id) {
        run("{} -L metamach-tmux display-message -p -t {} '#{{pane_dead}}:#{{pane_dead_status}}'",
            self.tmux_cmd, id)
    }
    // capture_pane, kill_session, has_session — 相同模式
}
```

### Workflow engine：无需改动

`workflow.rs` 已泛型化于 `B: DurableBackend`。Step 分发时根据 `WorkflowStep.host` 选择 Backend：

```rust
let backend: Box<dyn DurableBackend> = match step.host.as_deref() {
    None | Some("local") => Box::new(TmuxBackend::new()),
    Some(host) => Box::new(TmuxBackend::with_ssh(host, recipe.remote_user.as_deref())),
};
```

**同一个 trait，同一个实现，同一个 engine 循环。** 远程只是命令前面多了一个前缀。

---

## 🔒 三、 Tool Guard：SSH 反向隧道（零远程配置）

远程 janush 需要连接本地 janus-daemon 进行 GuardCheck，但 daemon 的 UDS socket 不在远程主机上。解决方式：daemon 在 SSH 连接中建立反向隧道（`-R`），将本地 socket 映射到远程。

```
Local:          janus-daemon  ←──UDS──┐
                        │             │ SSH -R reverse tunnel
                        │    ┌────────┘
                        ▼    ▼
Remote:       /tmp/mm-<host>.sock
                        │
                        ▼
              janush → 连接到 /tmp/mm-<host>.sock → 到达 daemon
```

### step_command() 改动（workflow.rs，远程 case）

```rust
fn backend_prefix(host: &str, local_sock: &Path) -> String {
    format!(
        "ssh -o BatchMode=yes -o ConnectTimeout=5 \
         -R /tmp/mm-{host}.sock:{local_sock} {host}",
        host = host,
        local_sock = local_sock.display(),
    )
}
```

远程 janush 的环境变量指向隧道 socket：

```
env HERDR_PLUGIN_STATE_DIR=/tmp \
  ssh -R /tmp/mm-build-server.sock:/run/user/1000/herdr/.../janus.sock build-server \
  tmux -L metamach-tmux new-session ... 'janush -c "make flash"'
```

janush 在远程主机上查找 `/tmp/janus.sock` → 通过 SSH 隧道回到本地 daemon → GuardCheck → ALLOW/BLOCK/REWRITE。

### 远程主机依赖

| 组件 | 用途 | 安装方式 |
|---|---|---|
| **tmux 3.3+** | 物理 PTY 会话 | 系统包管理器 |
| **janush** | 命令拦截，通过 SSH 隧道连接回 daemon | `scp` 一次，或预装在远程镜像中 |

**不需要：** janus-daemon、Absurd PG、agents.toml、Tool Guard、Herdr、janus::gateway。全部留在本地单实例中。Tool Guard 规则在本地 daemon 上运行——与本地 Workload 完全相同。

---

## 💎 四、 统一 TUI 视图

所有 Workload（本地和远程）在 `herdr-janus` Progress 视图中以相同方式显示：

```
┌─ MetaMach Observer ──────────────────────────────────────────────────┐
│ 🏭 Workloads                                                        │
│                                                                      │
│  wf_build (local)         [⏳ RUNNING ]  tmux-janus-task-...-0      │
│  wf_cross_compile (ssh)   [⏳ RUNNING ]  tmux-janus-task-...-1 🗂️   │
│  wf_flash (ssh edge)      [🔴 BLOCKED]   tmux-janus-task-...-2 🗂️   │
│  └─ HITL: esptool.py --port /dev/ttyUSB0  [y] Approve [n] Reject   │
│                                                                      │
│ 🗂️ = remote (ssh)                                                   │
└──────────────────────────────────────────────────────────────────────┘
```

唯一区别：远程 Workload 在显示中带 `🗂️` 标记和 `host` 列。其他完全相同——相同的 Progress 轮询，相同的 Task ID，相同的 `metamach_step_meta`。

---

## 📋 五、 ADR 评估（0.4.7）

| 维度 | 评分 | 说明 |
|---|---|---|
| **价值** | 7/10 | 锁定架构决策："无独立 SshTmuxBackend——远程 = tmux + SSH 前缀"。防止过度工程化。 |
| **代码变更** | 0 行 | ADR 仅为设计记录——代码属于 Phase 2。 |
| **对现有设计的影响** | 简化 | 无需分离的 Backend 实现，无需新 trait，无需新模块。比 M4-4.1-design.md 中的提案更简单。 |
| **阻塞项** | 无——ADR 先行，Phase 2 执行 | |

### 建议 ADR 条目

```
ADR-0XX: Remote Workload Model — SSH as tmux Transport + Reverse Tunnel

Decision: Remote tmux sessions use the same TmuxBackend as local
sessions with an "ssh <host>" prefix. Tool Guard connectivity
uses SSH -R reverse tunnel to map the local janus.sock to the
remote host. No separate backend, no remote daemon, no remote PG.

Rejected: Separate SshTmuxBackend type; multi-daemon model.
Rationale: All DurableBackend operations are tmux CLI calls;
ssh <host> tmux ... is syntactically identical to tmux ....
SSH -R tunnel provides janush ↔ daemon connectivity with zero
remote configuration. Remote host needs only tmux + janush.
```

**Verdict: ✅ 0.4.7 的强 ADR 候选。** 0 行代码，仅文档决策——但显著简化了 Phase 2 实现（无需分离的 Backend 类型）。
