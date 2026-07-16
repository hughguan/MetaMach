# Herdr Plugin Contract (M0 Validation)

> **Status:** Validated against installed **Herdr 0.7.3** (Homebrew) on 2026-07-15 via the `spike/herdr-hello-plugin` PoC (gitignored).
> This memo is the authoritative Herdr integration contract for MetaMach. Where the other specs previously assumed "Herdr v1", this document supersedes those assumptions with the verified 0.7.3 behavior.

## 1. Actual version vs. spec assumption

The specs reference **"Herdr v1"** throughout. The installed binary is **`herdr 0.7.3`** (`herdr --version`). The plugin model below is 0.7.3's. Plugin manifests declare a required `min_herdr_version` field, so MetaMach's plugin should set `min_herdr_version = "0.7.3"`. References to "Herdr v1" in the other `docs/*.md` should be read as "Herdr 0.7.3+".

## 2. Plugin manifest (`herdr-plugin.toml`)

A plugin is a directory containing a **`herdr-plugin.toml`** manifest. The structure is **flat at the top level** (no `[plugin]` table). Validated minimal manifest:

```toml
id = "metamach.janus"            # required, unique plugin id (also the tenant key in paths)
name = "MetaMach Janus"          # required, human-readable
version = "0.1.0"                # required
min_herdr_version = "0.7.3"      # required
# platforms = ["macos", "linux"] # optional; omit to leave undeclared (warns)

[[panes]]                        # declared panes; opened via `herdr plugin pane open --entrypoint <pane.id>`
id = "dispatcher"                # required, unique within plugin
title = "MetaMach Dispatcher"    # required
placement = "overlay"            # overlay | split | tab | zoomed
command = ["herdr-janus"]        # argv array; non-empty strings
```

Other manifest sections observed in the binary (not all exercised in M0): `[[actions]]` (id/title/contexts, invoked via `herdr plugin action invoke`), `[[event_hooks]]` (event name + hooks), `[[link_handlers]]` (pattern/action/title), and a `build` block. `[[panes]]` is the pane-entrypoint model MetaMach's popup needs.

**Spec corrections (important):**
- The spec's `placement = "popup"` is **invalid**. The real enum is `overlay | split | tab | zoomed`. MetaMach's popup = **`overlay`**.
- The spec's manifest fields `width = "80%"` / `height = 20` **do not exist** in 0.7.3. Pane sizing is managed by Herdr (overlay panes), not the manifest. Drop `width`/`height` from `herdr-plugin.toml`. (If a specific TUI size is needed, the `herdr-janus` ratatui app controls its own render area.)

## 3. Plugin lifecycle (CLI)

```
herdr plugin link <path>            # register a local plugin from its manifest dir
herdr plugin unlink <plugin_id>     # remove
herdr plugin list [--json]          # show installed (enabled, source, warnings)
herdr plugin enable|disable <id>
herdr plugin install <owner>/<repo> # install from GitHub (managed checkout)
herdr plugin config-dir <id>        # print the plugin's config directory
```

Linking re-reads the manifest on each call (idempotent update). Unknown/optional fields like `platforms` produce warnings, not errors.

## 4. Pane model (the popup mechanism)

A pane is opened non-interactively via:

```
herdr plugin pane open \
  --plugin <plugin_id> \
  --entrypoint <pane.id> \
  --placement overlay|split|tab|zoomed \
  [--workspace ID] [--target-pane PANE] [--direction right|down] \
  [--cwd PATH] [--env KEY=VALUE] [--focus|--no-focus]
```

Validated: opens an overlay pane running the pane's `command` argv, with cwd = the plugin root. The pane appears in the active workspace (or `--workspace`). `herdr plugin pane focus|close <pane_id>` manages it. (Panes running short-lived commands auto-close on exit; long-lived TUI binaries like `herdr-janus` persist.)

**Dispatch keybinding:** the spec's `prefix+j` is a Herdr keybinding (configured in `~/.config/herdr/config.toml`), not a manifest field. To wire `prefix+j` -> MetaMach popup, bind a key in `config.toml` (or via the plugin's `event_hooks`/keybinding support) that invokes `herdr plugin pane open --plugin metamach.janus --entrypoint dispatcher --placement overlay`. Exact keybinding syntax is a follow-up (read the user's `config.toml`).

## 5. Injected environment variables (validated by capture)

When Herdr opens a plugin pane, it injects these env vars into the entrypoint process:

| Env var | Value (example) | MetaMach use |
|---|---|---|
| `HERDR_PLUGIN_ID` | `metamach.janus` | tenant key |
| `HERDR_PLUGIN_ROOT` | plugin source checkout dir | read-only binaries/config (`target/release/`, `workflows/`) |
| `HERDR_PLUGIN_CONFIG_DIR` | `~/.config/herdr/plugins/config/<id>` | mutable config (`agents.toml`) |
| `HERDR_PLUGIN_STATE_DIR` | `~/.local/state/herdr/plugins/<id>` | mutable state (`janus.sock`, `janus.pid`, `fallback.db`, PG socket) |
| `HERDR_SOCKET_PATH` | `~/.config/herdr/herdr.sock` | Herdr server socket API (open/close panes, query agents) |
| `HERDR_PLUGIN_ENTRYPOINT_ID` | `dispatcher` | which pane entrypoint |
| `HERDR_PLUGIN_CONTEXT_JSON` | JSON (see below) | dispatch context |
| `HERDR_BIN_PATH` | `/opt/homebrew/bin/herdr` | invoke Herdr CLI |
| `HERDR_ENV`, `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID` | runtime ids | context |

`HERDR_PLUGIN_CONTEXT_JSON` carries rich invocation context, e.g.:
```json
{"workspace_id":"w6","workspace_label":"metamach","workspace_cwd":".../metamach",
 "tab_id":"w6:t1","focused_pane_id":"w6:p2","focused_pane_agent":"claude",
 "focused_pane_status":"blocked","invocation_source":"api","correlation_id":"plugin-pane"}
```
This lets `herdr-janus` know which agent/pane/workspace invoked it - useful for dispatch context.

## 6. Directory mapping (validated vs. spec `Deployment-Spec.md` §2)

| Spec variable | Spec path | Actual (0.7.3) | Match? |
|---|---|---|---|
| `HERDR_PLUGIN_ROOT` | `~/.local/share/herdr/plugins/<id>` | **plugin source checkout** (the linked/install dir) | ✗ - corrected |
| `HERDR_PLUGIN_CONFIG_DIR` | `~/.config/herdr/plugins/<id>` | `~/.config/herdr/plugins/config/<id>` | ✗ - extra `/config/` |
| `HERDR_PLUGIN_STATE_DIR` | `~/.local/state/herdr/plugins/<id>` | `~/.local/state/herdr/plugins/<id>` | ✅ |

**Corrections to `Deployment-Spec.md` §2:**
- `HERDR_PLUGIN_ROOT` is the plugin's source directory (where `herdr-plugin.toml` lives) - for MetaMach this is the repo checkout (or Herdr's managed checkout if installed via `herdr plugin install`). It is **not** `~/.local/share/herdr/plugins/<id>`.
- `HERDR_PLUGIN_CONFIG_DIR` = `~/.config/herdr/plugins/config/<id>` (note the extra `/config/` segment).
- `HERDR_PLUGIN_STATE_DIR` is correct as specified.

## 7. Herdr server socket API

`HERDR_SOCKET_PATH` (`~/.config/herdr/herdr.sock`) exposes a JSON-RPC-style API (protocol 16). Useful for MetaMach:
- `herdr api snapshot` - live workspace/tab/pane/agent state (workspaces, panes, agents, cwd, agent_status).
- `herdr api schema` - full request/response/event schema (223KB; defines `PluginPanePlacement`, events, etc.).
- The plugin CLI (`herdr plugin pane ...`) is itself a client of this socket.

`janus-daemon` / `herdr-janus` can use this socket to: open Tether panes for workflow steps, query agent/pane state, focus panes, etc. - complementing MetaMach's own `janus.sock` UDS.

## 8. How MetaMach maps onto this contract

- **`herdr-janus` (shadow client)** = a pane entrypoint: `[[panes]] id="dispatcher" placement="overlay" command=["herdr-janus"]`. Rendered with ratatui inside a Herdr overlay pane. Connects to `janus-daemon` via MetaMach's own `janus.sock` (in `HERDR_PLUGIN_STATE_DIR`), and may use `HERDR_SOCKET_PATH` to drive Herdr (open Tether panes, read agent context).
- **Plugin id** = `metamach.janus` (drives the CONFIG_DIR/STATE_DIR paths; matches the spec's tenant naming).
- **Dispatch** = a Herdr keybinding opens the `dispatcher` overlay pane (`prefix+j`, configured in `config.toml`).
- **Directory isolation** = Herdr's injected `HERDR_PLUGIN_ROOT/CONFIG_DIR/STATE_DIR` map exactly onto MetaMach's Immutable-ROOT / Mutable-Config / Mutable-State model. No `make bootstrap` symlink tricks needed for config/state - Herdr provides them.

## 9. M0 verdict

**Herdr 0.7.3 fully supports MetaMach's shadow-client popup architecture.** The popup is an `overlay` pane running the `herdr-janus` binary; the injected `HERDR_PLUGIN_*` env vars provide the exact ROOT/CONFIG/STATE isolation the spec needs; `HERDR_SOCKET_PATH` gives runtime control of Herdr. **Green-light M1 Task 1.2** (real `herdr_janus.rs` Popup), gated only on the keybinding-wiring follow-up below.

## 10. Follow-ups (non-blocking)
- Wire the actual `prefix+j` keybinding in `~/.config/herdr/config.toml` to open the `metamach.janus` `dispatcher` pane (read the user's config for exact syntax; or use the plugin `event_hooks`/keybinding manifest support).
- Confirm `[[actions]]` / `[[event_hooks]]` / `[[link_handlers]]` semantics if MetaMach later needs plugin-defined actions or link handling (not required for the popup).
- Environment gap: **Docker is not installed** (blocks M1 Task 1.1 - Absurd Postgres). `sops`/`age` absent (optional, financial blueprints only). Rust 1.95 + tmux present.

## 11. PoC artifact
`spike/herdr-hello-plugin/` (gitignored) - a minimal plugin with one `dispatcher` overlay pane that prints the injected `HERDR_*` env. Linked and validated: `herdr plugin link spike/herdr-hello-plugin`, then `herdr plugin pane open --plugin herdr-hello-plugin --entrypoint dispatcher --placement overlay`. Remove with `herdr plugin unlink herdr-hello-plugin`.
