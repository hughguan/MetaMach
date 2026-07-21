# Herdr Integration — MetaMach 0.4.0

> **Status: Implemented.** `herdr-janus` is a Herdr 0.7.3 shadow client compiled as a standalone Rust binary. The plugin manifest is at `janus/herdr-plugin.toml`; the M0-validated contract is at `docs/herdr-v1-contract.md`. Architecture decision: `docs/ADR.md` ADR-016.

---

## 1. Dependency Model

MetaMach's dependency on Herdr is a **plugin-host contract dependency** — there is no `herdr` crate in `Cargo.toml`, no code import, no SDK. The relationship is:

```
      ┌─ Herdr 0.7.3 (terminal emulator / tmux workspace) ──────────┐
      │                                                               │
      │  prefix+j                                                     │
      │  → opens overlay pane                                         │
      │  → spawns herdr-janus process                                 │
      │  → injects HERDR_PLUGIN_* env vars                            │
      │                                                               │
      │  ┌─ herdr-janus (overlay pane, ratatui TUI) ───────────────┐ │
      │  │  Tab  → toggle Dispatch ↔ Progress                      │ │
      │  │  Esc/q → exit (Herdr auto-closes overlay)               │ │
      │  │  UDS   → janus-daemon                                   │ │
      │  └─────────────────────────────────────────────────────────┘ │
      └───────────────────────────────────────────────────────────────┘
                                    │ UDS
                                    ▼
                        ┌─ janus-daemon ──────────────────────────────┐
                        │  Blueprints, Progress, GuardCheck, Dispatch │
                        └──────────────────────────────────────────────┘
```

Herdr is **optional at the daemon level** — `janus-daemon` and `janush` work standalone via `paths.rs` fallback defaults. Only `herdr-janus` (the TUI binary) strictly requires Herdr.

---

## 2. Plugin Manifest

```toml
# janus/herdr-plugin.toml
id = "metamach.janus"
name = "MetaMach Janus"
version = "0.4.1"
min_herdr_version = "0.7.3"

[[panes]]
id = "dispatcher"
title = "MetaMach Dispatcher"
placement = "overlay"
command = ["herdr-janus"]
```

### Key design decisions (ADR-016)

| Decision | Rationale |
|---|---|
| **Single pane** (`dispatcher`) | Internal `Tab` toggle handles view switching (Dispatch ↔ Progress). Two panes would be over-engineered. |
| **`placement = "overlay"`** | M0-validated Herdr 0.7.3 directive. `popup` is not a valid enum value. |
| **No `width`/`height`** | Not valid Herdr 0.7.3 manifest fields. Sizing is managed by Herdr; the ratatui app controls its own render area. |
| **No `[[keys.command]]`** | Keybinding (`prefix+j`) is configured in `~/.config/herdr/config.toml`, not the plugin manifest. |
| **No `[[actions]]`** | Pane opens via `herdr plugin pane open --entrypoint dispatcher`. |
| **No `--mode` CLI flags** | `herdr-janus` always renders a ratatui TUI. View switching is internal (`Tab`). |
| **Process exit closes overlay** | No `herdr plugin pane close` call needed — Herdr auto-closes the overlay on process exit. |

---

## 3. Injected Environment Variables

These four variables are Herdr's entire API surface that MetaMach depends on. Every `paths.rs` function reads them with a fallback for standalone (no-Herdr) operation.

| Variable | Purpose | Used by | paths.rs fallback |
|---|---|---|---|
| `HERDR_PLUGIN_ROOT` | Immutable plugin checkout (blueprints/, workflows/, bin/) | `janus-daemon` for `repo_root()` | `$PWD` |
| `HERDR_PLUGIN_CONFIG_DIR` | Mutable config (`agents.toml`) | `janus-daemon` for `agents_toml_path()` | `~/.config/herdr/plugins/config/metamach.janus` |
| `HERDR_PLUGIN_STATE_DIR` | Mutable state (`janus.sock`, `janus.pid`, `fallback.db`) | All binaries for `state_dir()` | `~/.local/state/herdr/plugins/metamach.janus` |
| `HERDR_SOCKET_PATH` | Herdr's own control socket | Not currently used by MetaMach | — |

> **Design invariant:** Every `paths.rs` function has a fallback. If Herdr is absent (standalone daemon), MetaMach binaries still work — they resolve paths under `~/.local/state/herdr/plugins/metamach.janus` using the hardcoded default.

---

## 4. Runtime Lifecycle

### 4.1 Pane open (prefix+j)

```
1. Factory Director presses prefix+j in Herdr
2. Herdr reads manifest, opens overlay pane
3. Herdr spawns: herdr-janus (with HERDR_PLUGIN_* env vars injected)
4. herdr-janus ratatui TUI starts
5. Reads paths::sock_path() → ${HERDR_PLUGIN_STATE_DIR}/janus.sock
6. Probes daemon via UDS: Ping
   ├─ Daemon online  → fetch Blueprints, render Dispatch view
   └─ Daemon offline → show "offline" status; 'r' lazy-starts daemon
7. Tab toggles Dispatch ↔ Progress views
8. Esc/q exits TUI → Herdr auto-closes overlay pane
```

### 4.2 Plugin registration

```bash
herdr plugin link ~/metamach/janus          # register from local manifest dir
herdr plugin list                           # verify (enabled, source, warnings)
herdr plugin pane open --plugin metamach.janus --entrypoint dispatcher  # manual test
```

### 4.3 Plugin uninstall

```bash
herdr plugin unlink metamach.janus          # remove registration
# Compiled binaries remain at janus/target/release/ — unaffected
```

---

## 5. Herdr Version Compatibility

### 5.1 Version lock

The manifest field `min_herdr_version = "0.7.3"` declares the minimum compatible Herdr version. Herdr validates this on `plugin link` — a Herdr older than 0.7.3 will reject the manifest.

### 5.2 Contract stability assessment

| Herdr change | Impact | Likelihood | Mitigation |
|---|---|---|---|
| `placement` enum rename | Manifest parse error; pane won't open | Low | M0 validated 0.7.3 enum (`overlay | split | tab | zoomed`) |
| Env var names change | `paths.rs` resolves to wrong directories | Very low | These are Herdr's stable plugin contract |
| New required manifest fields | Warning or error on `plugin link` | Medium | Test `herdr plugin link` after upgrade |
| `min_herdr_version` parsing change | MetaMach's `"0.7.3"` rejected | Low | Semver parsing is stable |
| Pane lifecycle change | Process spawn/cleanup behavior differs | Low | Process exit semantics are fundamental |

### 5.3 Upgrade procedure

When a new Herdr version is released:

```bash
# 1. Check version
herdr --version

# 2. Re-validate manifest
herdr plugin unlink metamach.janus 2>/dev/null
herdr plugin link ~/metamach/janus

# 3. Check for warnings
herdr plugin list --json | jq '.[] | select(.id == "metamach.janus")'

# 4. Smoke test
herdr plugin pane open --plugin metamach.janus --entrypoint dispatcher

# 5. If all clear, optionally bump min_herdr_version
# (only if new features are used; staying on 0.7.3 is fine for compatibility)
```

---

## 6. Maintenance Checklist

### Daily / per-session
- `herdr-janus` lazy-starts the daemon if socket absent — no manual intervention needed

### Per Herdr upgrade
- [ ] Run `herdr plugin link ~/metamach/janus`
- [ ] Check for manifest warnings
- [ ] Smoke test pane open
- [ ] Update `min_herdr_version` if new features are adopted

### Per MetaMach release
- [ ] Verify `herdr-plugin.toml` version matches `janus/Cargo.toml`
- [ ] Verify `min_herdr_version` is still correct against installed Herdr
- [ ] Test `make symlinks` → `herdr plugin link` → `prefix+j` end-to-end

### CI considerations
- Herdr is not installed on GitHub Actions Ubuntu runners
- Current CI tests binaries standalone (via UDS directly) — this is intentional
- A Herdr smoke test would require installing the Herdr binary on the CI runner (brew not available, no apt package)
- **Decision:** manual smoke test on macOS development host; CI covers binary correctness only

---

## 7. Related Documents

| Document | Relationship |
|---|---|
| `docs/herdr-v1-contract.md` | **M0-validated** Herdr 0.7.3 plugin contract — authoritative for all integration details |
| `docs/ADR.md` ADR-016 | Architecture decision: single-pane design, corrected manifest, 11 fixes from original Chinese design doc |
| `janus/herdr-plugin.toml` | Live manifest — source of truth for Herdr integration |
| `janus/src/paths.rs` | Env var resolution with Herdr/standalone fallbacks |
| `janus/src/bin/herdr_janus.rs` | Shadow client TUI — ratatui rendering, Tab toggle, UDS client |
| `Makefile` | `compile` builds `herdr-janus`; `symlinks` installs to `HERDR_PLUGIN_ROOT/bin/` |
| `docs/bak/herdr-plugin.md` | Original Chinese design doc (gitignored backup) — superseded by ADR-016 |

---

## 8. Design Principles (inherited from herdr-tether analysis)

MetaMach 0.4.0's Herdr integration resolves the key limitations of the former `herdr-tether` external plugin:

| Principle | herdr-tether Limitation | MetaMach 0.4.0 |
|---|---|---|
| **SIGHUP Immunity** | Closing TUI could kill tmux sessions | `janus::tmux` is daemon-owned; sessions survive frontend destruction |
| **Fail-Closed** | Unknown state assumed safe | 30s timeout → BLOCK; never lets through on uncertainty |
| **16KB Budget** | Over-budget = fatal error | Dual-defense: janush streaming + daemon pre-insert truncation |
| **tmux Isolation** | Could attach to external sessions | Strict `tmux -L metamach-tmux` isolation |
| **Idempotent Recovery** | File-based state | Absurd PG checkpoints; cold-start from last COMPLETED step |
| **File Security** | Varies | All runtime files enforce `0600` permissions |
| **SSH Policy** | Could not parse SSH Include | Host-native `ssh` binary; inherits all system config resolution |
| **Not a Sandbox** | Tether was not a sandbox | janush is a gatekeeper — approved commands execute bare-metal |
