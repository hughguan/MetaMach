# Physical PTY Execution Engine Contract (`janus::tmux`)

> **Status:** Implemented & Internalized.  
> Architecture Decision: `docs/ADR.md` ADR-017, ADR-029.  
> Source Module: `janus/src/tmux/mod.rs` (`janus::tmux`).  
> Isolated Server Socket: `tmux -L metamach-tmux`.

---

## 1. Overview & Architecture Dependency Model

`janus::tmux` is MetaMach's physical execution engine. Formerly an external plugin (`herdr-tether`), it has been **internalized as a native Rust module** within the `janus` workspace.

```
┌─ janus-daemon (Control Plane) ─────────────────────────────────────────┐
│                                                                        │
│  workflow::run_workflow                                                │
│    │                                                                   │
│    ├─► TmuxBackend::create_session(id, janush_command, cwd)            │
│    │     │                                                             │
│    │     ▼                                                             │
│    │   tmux -L metamach-tmux new-session -d -s tmux-janus-task-<uuid>  │
│    │   tmux -L metamach-tmux set-option remain-on-exit on              │
│    │   tmux -L metamach-tmux respawn-pane -k "janush -c '<command>'"   │
│    │                                                                   │
│    ├─► TmuxBackend::poll_exit(id) ───┐ (step completion detection)     │
│    │                                 │                                 │
│    └─► TmuxBackend::capture_pane(id) ┴─► PTY Stream Filter             │
│                                           │ (stdout_tail 16 KiB)       │
│                                           ▼                            │
│                                   Absurd Postgres                      │
└────────────────────────────────────────────────────────────────────────┘
```

### Key Invariants
1. **Isolated Server (`tmux -L metamach-tmux`)**: Physical sessions run on a dedicated socket (`TMUX_SOCKET`), never polluting the developer's personal tmux server.
2. **Physical Non-Destruction (`remain-on-exit on`)**: Sessions survive process exit, SSH drops, or daemon restarts (ARCH §6.1). Pane completion is detected via `display-message '#{pane_dead}: #{pane_dead_status}'`.
3. **Fail-Closed PTY Interception (`janush`)**: Workload commands are wrapped in `janush -c "<command>"` so every agent execution is synchronously reconciled against Tool Guard before execution.

---

## 2. Rust `DurableBackend` Trait Surface

The PTY execution interface is governed by `janus::tmux::DurableBackend` (`janus/src/tmux/mod.rs`):

| Method | Tmux Command / Mechanism | Purpose |
|---|---|---|
| `create_session(id, command, cwd)` | `new-session -d -s <id>`<br>`set-option -t <id> remain-on-exit on`<br>`respawn-pane -t <id> -k <cmd>` | Spawn a durable, PTY-isolated workload session with race-free `remain-on-exit` ordering. |
| `poll_exit(id)` | `display-message -p -t <id> '#{pane_dead}:#{pane_dead_status}'` | Query whether the workload has terminated and retrieve its exit code (`Some(code)`). |
| `capture_pane(id)` | `capture-pane -p -t <id> -S -200` | Capture stdout/stderr pane lines for HITL scene previews and raw log harvesting. |
| `has_session(id)` | `has-session -t <id>` | Query session liveness on the isolated tmux server. |
| `list_sessions()` | `list-sessions -F '#{session_name}'` | Enumerate all MetaMach active session targets. |
| `kill_session(id)` | `kill-session -t <id>` | Destroy a session (GC / abort / offboard purge). |
| `attach(id)` | `attach-session -t <id>` | Connect the foreground terminal interactively (TUI attach). |

---

## 3. Session Naming & Identity Isolation

- **Session Target (`SessionId`)**: Newtyped wrapper to prevent confusion between Absurd `task_id` UUIDs and physical tmux targets.
- **Prefix Standard**: `SESSION_PREFIX` = `tmux-janus-task-`.
- **Target Format**: `tmux-janus-task-<uuid>` (e.g., `tmux-janus-task-01912a3b-4c5d-7e8f-9a0b-1c2d3e4f5a6b`).
- **Environment Override**: `JANUS_TMUX_SOCKET` can override `metamach-tmux` for isolated test sandboxes.

---

## 4. Cross-Host Execution & SSH Tunneling (ADR-017 / ADR-029)

`TmuxBackend::with_ssh(host, user)` enables key-based execution across remote physical hosts without altering the `DurableBackend` trait surface.

### SSH Command Prefix
When `is_remote() == true`, all tmux control commands are transparently wrapped:
```bash
ssh -o BatchMode=yes \
    -o ConnectTimeout=5 \
    -o StrictHostKeyChecking=accept-new \
    [-l <user>] \
    <host> \
    tmux -L metamach-tmux <args...>
```

### Reverse Tunneling for Proxy Shell (`janush`)
Remote PTY workloads invoke `janush`, which requires socket access back to `janus-daemon`:
- `BackendFactory` orchestrates an SSH reverse tunnel (`-R <remote_socket>:<local_socket>`).
- Remote `janush` forwards tool execution requests over the reverse tunnel to the local `janus-daemon`.

---

## 5. PTY Log Filtering & Output Tail Harvesting

Physical pane outputs are captured and cleaned via `janus::workflow::filter`:
1. **ANSI Removal**: Strips terminal escape codes and cursor movement sequences.
2. **Progress Bar Collapsing**: Collapses continuous terminal spinner / progress bar redrawn lines.
3. **Repeated Line Deduplication**: Suppresses noisy repetitive output while preserving log semantics.
4. **16 KiB Hard Truncation**: Enforces `SIZE_BUDGET` (16 KiB) on `stdout_tail` before saving into Absurd Postgres overlay rows.

---

## 6. Testing & Simulation Strategy

- **Runtime-Skip Pattern**: Integration tests checking physical PTY behavior require a real `tmux 3.3+` server. If `tmux` is absent from `PATH`, tests skip gracefully (`eprintln!("skip: tmux not installed")`).
- **`FakeTmuxBackend`**: In-memory `DurableBackend` mock provided in `janus/src/tmux/mod.rs` for isolated daemon unit tests without invoking real tmux processes.
