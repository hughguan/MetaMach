# Herdr Integration & Plugin Contract

> **Status:** Implemented & Validated against **Herdr 0.7.3+**.  
> Architecture Decision: `docs/ADR.md` ADR-016.  
> Manifest: `janus/herdr-plugin.toml`.

---

## 1. Overview & Architecture Dependency Model

MetaMach's dependency on Herdr is a **plugin-host contract dependency**. There is no `herdr` crate in `Cargo.toml`, no code import, and no binary link.

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

Herdr is **optional at the daemon level**. `janus-daemon` and `janush` work standalone via `janus::paths` fallback defaults. Only `herdr-janus` (the shadow TUI client) strictly requires Herdr.

---

## 2. Plugin Manifest (`herdr-plugin.toml`)

A plugin is a directory containing a `herdr-plugin.toml` manifest. The structure is flat at the top level.

```toml
# janus/herdr-plugin.toml
id = "metamach.janus"            # required, unique plugin id
name = "MetaMach Janus"          # required, human-readable
version = "0.5.0"                # required, matches Cargo.toml
min_herdr_version = "0.7.3"      # required

[[panes]]                        # declared panes
id = "dispatcher"                # required, unique within plugin
title = "MetaMach Dispatcher"    # required
placement = "overlay"            # overlay | split | tab | zoomed
command = ["herdr-janus"]        # argv array
```

### Key Design Contract Rules (ADR-016)
- **Single pane (`dispatcher`)**: Internal `Tab` toggle handles view switching (Dispatch ↔ Progress).
- **`placement = "overlay"`**: M0-validated Herdr directive. `popup` is not a valid enum value.
- **No `width`/`height`**: Sizing is managed by Herdr; the ratatui app controls its own render area.
- **No keybindings in manifest**: Keybinding (`prefix+j`) is configured in `~/.config/herdr/config.toml`.
- **Auto-closing**: Herdr auto-closes the overlay on `herdr-janus` process exit.

---

## 3. Injected Environment Variables

When Herdr opens a plugin pane, it injects these env vars into the entrypoint process:

| Environment Variable | Purpose | MetaMach Usage | Standalone Fallback (`paths.rs`) |
|---|---|---|---|
| `HERDR_PLUGIN_ROOT` | Immutable plugin checkout | `janus-daemon` `repo_root()` | `$PWD` |
| `HERDR_PLUGIN_CONFIG_DIR` | Mutable config | `agents.toml` path | `~/.config/herdr/plugins/config/metamach.janus` |
| `HERDR_PLUGIN_STATE_DIR` | Mutable state | `janus.sock`, `janus.pid`, `fallback.db` | `~/.local/state/herdr/plugins/metamach.janus` |
| `HERDR_SOCKET_PATH` | Herdr control socket API | Query agent/pane state, open panes | — |
| `HERDR_PLUGIN_ID` | `metamach.janus` | Tenant key | `metamach.janus` |
| `HERDR_PLUGIN_ENTRYPOINT_ID` | `dispatcher` | Entrypoint name | `dispatcher` |
| `HERDR_PLUGIN_CONTEXT_JSON` | Context JSON | Focused pane/workspace metadata | Empty JSON `{}` |

---

## 4. Plugin CLI & Runtime Lifecycle

### Registration & Invocation
```bash
herdr plugin link ~/metamach/janus          # register local plugin
herdr plugin list                           # verify installation & warnings
herdr plugin pane open --plugin metamach.janus --entrypoint dispatcher  # launch pane
herdr plugin unlink metamach.janus          # unregister plugin
```

### Keybinding Wire-up
In `~/.config/herdr/config.toml`:
```toml
# Wire prefix+j to MetaMach overlay
[keys]
"prefix+j" = "plugin pane open --plugin metamach.janus --entrypoint dispatcher --placement overlay"
```

---

## 5. Maintenance & Version Control

- **Version Compatibility**: `min_herdr_version = "0.7.3"` gates `plugin link`.
- **Upgrade Checklist**:
  1. Verify `herdr --version` (0.7.3+).
  2. Run `herdr plugin link ./janus`.
  3. Verify `herdr plugin list --json` has zero errors.
  4. Smoke test overlay launch (`prefix+j`).
