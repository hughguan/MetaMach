# MetaMach 0.3.0 — Deployment Specification

> Immutable/Mutable separation architecture, physical sandbox mounting, and unified database grid-connection guide.

This Deploy Spec guides the system administrator or Factory Director in safely, idempotently, and seamlessly completing the grid-connection and power-on of the **MetaMach 0.3.0** production base on a local physical compute node (e.g., the Richmond Hill workshop server).

This specification strictly follows Herdr 0.7.3's **"Immutable ROOT vs. Mutable State"** separation and security red lines, providing system-level definition of physical directories, RAM disk mounting, database initialization, and the one-click bootstrap process.

## 1. Prerequisites

| Component | Minimum Version | Purpose | Verify |
|-----------|----------------|---------|--------|
| **OS** | Linux / macOS | POSIX-compatible environment & UDS support | `uname -a` |
| **Rust Toolchain** | Rust 1.88+ (Edition 2024) | Compile `janus-daemon`, `herdr-janus`, `janus-sh` | `rustc --version` |
| **Tmux** | Tmux 3.3+ | Physical carrier for Tether PTY session immortality | `tmux -V` |
| **PostgreSQL** | PG 15+ | Native host Postgres instance (no Docker); managed by `janus-daemon` on first startup | `psql --version`

| **SOPS & Age** _(optional)_ | SOPS v3.8+ / Age v1.1+ | Strong encrypted storage of local sensitive keys in Git monorepo | `sops --version` |

> **Platform Note (macOS `/dev/shm` unavailable):** macOS does not have `/dev/shm` tmpfs by default; `mkdir -p /dev/shm/...` creates a **regular directory** on the root filesystem—keys will land on disk, completely defeating RAM-disk security. Therefore: **production deployment supports Linux only**; macOS is development-only and must use `$TMPDIR` or `hdiutil attach -nomount ram://2048` to create a genuine RAM disk, with explicit notation that "keys under macOS are not memory-resident and must not carry real financial credentials."


> 💡 **Uni-Directional Stateless Deployment Pattern (Non-Normative Note for Remote Targets)**
> 
> In scenarios where the remote physical target is behind strict air-gapped network isolation and cannot host Git credentials or establish a reverse connection to the Absurd Postgres database, the following **uni-directional stateless Diff pipeline** is recommended as an implementation pattern. This is a fallback for air-gapped targets; the primary cross-host transport is the internal `janus::tether` module (bidirectional `remain-on-exit` PTY), used wherever the remote can sustain a Tether session.
>
> 1. When the local `janus-daemon` encounters a cross-host Step, it generates a full source-tree snapshot at the dispatch-pinned `target_sha` (Contract 3.1) via `git archive`, ensuring the remote receives a complete, self-contained working tree — not just an incremental patch.
> 2. The archive is projected uni-directionally through an SSH pipe onto the remote host's `/tmp/sandbox`:
>    `git archive HEAD | ssh -i /dev/shm/ssh_key user@remote "mkdir -p /tmp/sandbox && tar xf - -C /tmp/sandbox"
> 3. The remote host executes the build/test, then returns only a structured `result.json` (≤16KB) via SSH stdout to the local host for database ingestion.
> 
> This pattern keeps the remote target entirely stateless — no Git, no database, no persistent storage. All state reconciliation and audit commitments are performed locally by the Janus Daemon. This is a **recommended pattern**, not a mandatory spec contract; alternative transport mechanisms (NFS shared volumes, container volume mounts, pre-synced source trees) are equally valid as long as the remote target remains stateless.
## 2. Immutable/Mutable Physical Directory Topology

To prevent GitHub plugin updates from accidentally wiping the Factory Director's local financial data, personalized config, and database credentials, strict Immutable/Mutable separation must be enforced. The deployment scripts auto-create and establish symlinks:

```
[Immutable ROOT (Git Checkout)]       -->  ${HERDR_PLUGIN_ROOT} (plugin source checkout dir, where herdr-plugin.toml + target/ live)
                                           ├── target/release/ (read-only binaries)
                                           └── workflows/ (read-only standard SOPs)

[Mutable Config (user config zone)]  -->  ${HERDR_PLUGIN_CONFIG_DIR} (~/.config/herdr/plugins/config/metamach.janus)
                                           └── agents.toml (sensitive key injection point)

[Mutable State (runtime state zone)] -->  ${HERDR_PLUGIN_STATE_DIR} (~/.local/state/herdr/plugins/metamach.janus)
                                           ├── janus.sock (UDS socket)
                                           ├── janus.pid (singleton process lock)
                                           ├── janus.log (Daemon operational log; 10MB rotation × 5 files)
                                           └── fallback.db (local disaster recovery SQLite)
```

> **Herdr 0.7.3 provides these dirs automatically** (validated in M0; see `docs/herdr-v1-contract.md`). When Herdr opens a plugin pane it injects `HERDR_PLUGIN_ROOT` (the plugin source checkout), `HERDR_PLUGIN_CONFIG_DIR` (`~/.config/herdr/plugins/config/<id>`), and `HERDR_PLUGIN_STATE_DIR` (`~/.local/state/herdr/plugins/<id>`) as env vars. Herdr creates the config/state dirs on first plugin run, so `make bootstrap` need not `mkdir` them; the Makefile's config-dir `ln -sf agents.toml` should target `${HERDR_PLUGIN_CONFIG_DIR}` (resolved, e.g. `~/.config/herdr/plugins/config/metamach.janus`) - to be reconciled in M1 Task 1.3 (config management).

## 3. Absurd Postgres Database Setup

One PG, Multi-DB topology. A single native Postgres 15+ instance runs on the host (no Docker), managed directly by `janus-daemon`. Each blueprint receives an independent logical database via `CREATE DATABASE metamach_blueprint_<name>` on Onboard. Data persists at `~/.metamach/db/`.

### 3.1 Native Postgres Bootstrap

`janus-daemon` handles PG lifecycle on first startup:

1. **Init cluster:** If `~/.metamach/db/` is empty, run `initdb -D ~/.metamach/db/` to create a fresh PG cluster.
2. **Start server:** Launch `pg_ctl -D ~/.metamach/db/ -l ~/.metamach/db/pg.log start` with `listen_addresses=''` (Unix socket only, no TCP).
3. **Create admin role:** `CREATE ROLE metamach_admin WITH LOGIN PASSWORD '<random>'` (password persisted to `~/.metamach/db/.pgpass`, chmod 600).
4. **Run migrations:** Execute all `.sql` files from `janus/migrations/` in order.
5. **Blueprint onboarding:** `CREATE DATABASE metamach_blueprint_<name>` on each `janus onboard`.

> **Connection string:** `postgresql://metamach_admin:<password>@/metamach_db?host=~/.metamach/db` — Unix socket only, no TCP exposure.

## 4. RAM Disk Key Decryption & Mounting

To ensure financial Blueprint (e.g., trading account) Refresh Tokens never persist in plaintext on physical disk, the system at runtime executes `decrypt_secrets.sh` for RAM disk mounting and read-once-then-burn.

### 4.1 Decryption & Mount Script: `provisioning/decrypt_secrets.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

# 0. Prerequisite checks: sops / age must be in place; clear error instead of cryptic "command not found"
export SOPS_AGE_KEY_FILE="$HOME/.config/sops/age/keys.txt"

if ! command -v sops >/dev/null 2>&1; then
    echo "❌ sops not installed. Install: macOS 'brew install sops' / Linux 'apt install sops'."
    exit 1
fi
if ! command -v age >/dev/null 2>&1; then
    echo "❌ age not installed. Install: macOS 'brew install age' / Linux 'apt install age'."
    exit 1
fi
if [ ! -f "$SOPS_AGE_KEY_FILE" ]; then
    echo "❌ Age private key not found at $SOPS_AGE_KEY_FILE; cannot decrypt financial credentials."
    exit 1
fi

# 1. Declare RAM disk temp path
RAM_DISK_PATH="/dev/shm/metamach.janus"
DECRYPTED_KEY="${RAM_DISK_PATH}/hi5bot.decrypted"

# 2. Create high-security RAM disk subdirectory
if [ ! -d "$RAM_DISK_PATH" ]; then
    mkdir -p "$RAM_DISK_PATH"
    chmod 0700 "$RAM_DISK_PATH"  # Only current runtime user has access
fi

# 3. Use Age private key to synchronously decrypt to RAM disk via SOPS
if [ -f "configs/sops/hi5bot.encrypted.json" ]; then
    echo "🔑 Decrypting financial secrets directly to RAM disk..."
    sops --decrypt --output "$DECRYPTED_KEY" configs/sops/hi5bot.encrypted.json
    chmod 0600 "$DECRYPTED_KEY"  # Strict read/write restrictions
    echo "✨ Secrets loaded successfully in volatile RAM."
else
    echo "⚠️ Warning: Financial secrets not found. Skipping financial vault setup."
fi
```

## 5. One-Command Bootstrap (Makefile)

MetaMach 0.3.0 provides a highly simplified "one-command grid-connection" instruction. The Factory Director only needs to execute `make bootstrap` in the root directory; the system auto-completes environment validation, code compilation, directory creation, symlink mounting, and native PG initialization.

### 5.1 Automation Master Switch: `Makefile`

```makefile
.PHONY: all bootstrap compile symlinks db-up db-down db-backup db-restore db-migrate health logs uninstall clean

# 1. Environment variables
HERDR_PLUGIN_STATE_DIR ?= ~/.local/state/herdr/plugins/metamach.janus
METAMACH_DB_DIR ?= ~/.metamach/db
export METAMACH_DB_PASSWORD ?= $(shell [ -f $(METAMACH_DB_DIR)/.pgpass ] && cat $(METAMACH_DB_DIR)/.pgpass || openssl rand -hex 16)

all: bootstrap

# 2. Supreme one-command bootstrap primitive
bootstrap: symlinks compile db-up
	@echo "================================================================="
	@echo "🪐 MetaMach 0.3.0 successfully bootstrapped!"
	@echo "🔌 Run 'prefix+j' inside Herdr to open Dispatcher Console."
	@echo "================================================================="

# 3. Establish Immutable/Mutable physical directories & symlinks
symlinks:
	@echo "📁 Creating mutable state and config directories..."
	@mkdir -p ~/.config/herdr/plugins/metamach.janus
	@mkdir -p ~/.local/state/herdr/plugins/metamach.janus
	@mkdir -p $(METAMACH_DB_DIR)
	@printf '%s' "$(METAMACH_DB_PASSWORD)" > $(METAMACH_DB_DIR)/.pgpass && chmod 600 $(METAMACH_DB_DIR)/.pgpass
	@echo "🔑 DB password persisted to $(METAMACH_DB_DIR)/.pgpass (chmod 600)."
	@echo "🔗 Linking agents config into Herdr Config Directory..."
	@ln -sf $$(pwd)/configs/agents.toml ~/.config/herdr/plugins/metamach.janus/agents.toml

# 4. Local compile Janus Core binary components
compile:
	@echo "🦀 Compiling Janus Daemon, Client, and janus-sh proxy..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ Installing binaries to ${HERDR_PLUGIN_ROOT}/bin/..."
	@mkdir -p ${HERDR_PLUGIN_ROOT}/bin
	@cp janus/target/release/janus-daemon ${HERDR_PLUGIN_ROOT}/bin/janus-daemon
	@cp janus/target/release/herdr-janus ${HERDR_PLUGIN_ROOT}/bin/herdr-janus
	@cp janus/target/release/janus-sh ${HERDR_PLUGIN_ROOT}/bin/janus-sh

# 5. Initialize native Postgres (no Docker)
db-up:
	@echo "🐘 Initializing native Postgres at $(METAMACH_DB_DIR)..."
	@if [ ! -f $(METAMACH_DB_DIR)/PG_VERSION ]; then \
		echo "  → Running initdb..."; \
		initdb -D $(METAMACH_DB_DIR) -U metamach_admin --auth-local=trust; \
	fi
	@echo "  → Starting PG server (Unix socket only, no TCP)..."
	@pg_ctl -D $(METAMACH_DB_DIR) -l $(METAMACH_DB_DIR)/pg.log start 2>/dev/null || true
	@echo "  → Setting admin password..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d postgres -c "ALTER ROLE metamach_admin WITH PASSWORD '$(METAMACH_DB_PASSWORD)';" 2>/dev/null || true
	@echo "  → Creating metamach_db..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d postgres -c "CREATE DATABASE metamach_db;" 2>/dev/null || true
	@echo "  → Running migrations..."
	@for f in janus/migrations/*.sql; do psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f $$f; done
	@echo "⚡ Native Postgres online at $(METAMACH_DB_DIR)."

# 6. Safe shutdown
db-down:
	@echo "🔌 Stopping Postgres..."
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || true

# 7. Database backup
db-backup:
	@echo "💾 Backing up metamach_db..."
	@pg_dump -h $(METAMACH_DB_DIR) -U metamach_admin metamach_db > metamach_backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✅ Backup complete."

# 8. Database restore
db-restore:
	@if [ -z "$(BACKUP_FILE)" ]; then echo "❌ Usage: make db-restore BACKUP_FILE=backup.sql"; exit 1; fi
	@echo "🔄 Restoring metamach_db from $(BACKUP_FILE)..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db < $(BACKUP_FILE)
	@echo "✅ Restore complete."

# 9. Run pending migrations
db-migrate:
	@echo "🔄 Running pending migrations..."
	@for f in janus/migrations/*.sql; do psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f $$f; done
	@echo "✅ Migrations complete."

# 10. Health check
health:
	@echo "=== MetaMach Health Check ==="
	@pg_isready -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db || echo "❌ DB offline"
	@test -S $(HERDR_PLUGIN_STATE_DIR)/janus.sock && echo "✅ Daemon socket alive" || echo "❌ Daemon socket missing"

# 11. Log viewing
logs:
	@tail -n 200 $(HERDR_PLUGIN_STATE_DIR)/janus.log 2>/dev/null || echo "(no janus.log; Daemon defaults to stderr)"

# 12. Full uninstall
uninstall:
	@echo "⚠️  This will DELETE all MetaMach data. Continue? [y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || true
	@rm -rf $(METAMACH_DB_DIR)
	@rm -rf ~/.config/herdr/plugins/metamach.janus
	@rm -rf ~/.local/state/herdr/plugins/metamach.janus
	@echo "🗑️  MetaMach fully uninstalled."

# 13. Clean local compile cache
clean:
	@echo "🧹 Cleaning cargo workspace..."
	@cd janus && cargo clean
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  Wiping RAM disk secrets at /dev/shm/metamach.janus..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi
```

## 6. Deployment Verification & Sanity Check

After completing `make bootstrap`, the Factory Director must execute the following physical reconciliation steps to confirm the workshop pipeline possesses absolute immunity to power loss, intrusion, and database bloat.

### Step 6.1: Verify `janus-sh` Physical Interception

```bash
# Create sentinel dir & file, then attempt to delete it with a blacklisted command (NEVER real system-level delete)
SENTINEL_DIR=/tmp/metamach-deploy-guard-$(uuidgen)
mkdir -p "$SENTINEL_DIR" && echo sentinel > "$SENTINEL_DIR/sentinel"
export SHELL=./janus/target/release/janus-sh
$SHELL -c "rm -rf $SENTINEL_DIR"
test -f "$SENTINEL_DIR/sentinel" && echo "✅ Sentinel survived; command was intercepted"
```

- **Pass:** Terminal instantly suspends; no actual deletion occurs; sentinel file still exists afterward (proving the command was intercepted and never reached the real shell). Interception log appears under `~/.local/state/herdr/plugins/metamach.janus/`; phone Telegram/Teams receives security suspension alert.

### Step 6.2: Verify `remain-on-exit` Process Immortality

1. Execute `janus tether open --command "sleep 100"` to launch a background physical process via the internal Tether module.
2. Force-close the Herdr foreground view window, or directly execute `killall -9 herdr` on the host.
3. Run `tmux list-sessions` in a system terminal.

- **Pass:** The background still clearly shows a tmux session named `tether-janus-task-<uuid>` in active running state. Re-enter Herdr and execute `janus tether attach`; scene restores 100% in milliseconds.

### Step 6.3: Verify Cold-Start Self-Healing

1. Start a physical cross-compilation task lasting approximately 1 minute.
2. Run `pg_ctl -D ~/.metamach/db/ stop` to forcibly stop Postgres, and kill the `janus-daemon` process to simulate a sudden power outage.
3. Restart the PG database container, and run `target/release/janus-daemon` in a terminal.

- **Pass:** Within `0.5s` of startup, the Daemon performs a typed disposition of pre-outage unfinished tasks: for `RUNNING`-state tasks, it picks up from the last `COMPLETED` Step Checkpoint in the `absurd_steps` table and seamlessly resumes the next station; for `SUSPENDED`-state tasks, it keeps them suspended and notifies the Factory Director (never blindly re-runs). Console has no extraneous redundant output.

### Step 6.4: First Blueprint Onboard

`make bootstrap` only powers on the base (database, binaries, symlinks); at this point the workshop is in a **zero product line** state. The Factory Director must explicitly onboard a blueprint before dispatching production:

1. Confirm the target blueprint directory is in place, e.g., `blueprints/gatemetric/` contains `janus.toml` (declaring `default_workflow`, `[remote]` target, `[openwiki].scope`).
2. Execute the onboard command:
    ```bash
    janus onboard --blueprint gatemetric
    ```
3. Verify tenant registration and dispatchability:
    ```bash
    # Blueprint registered as ACTIVE
    psql -h ~/.metamach/db/ -U metamach_admin -d metamach_db \
        -c "SELECT name, status, default_workflow FROM blueprints;"
    # Inspect workshop status headlessly
    janus status
    ```

- **Pass:** The `blueprints` table shows one `gatemetric` / `ACTIVE` row; `janus status` outputs current in-flight tasks (should be empty at this point, but the command itself returns success, proving the `progress` primitive and Daemon connection are normal); inside Herdr, `prefix+j` wakes the Popup and the dispatch menu already shows `gatemetric` ready for immediate dispatch. Repeated execution of `janus onboard` produces no duplicate row (idempotent).
