# MetaMach 0.3.0 - Factory master switch (Deployment-Spec §5.1)
#
# Native PG, no Docker. make db-init launches PG and runs the catalog migration.
# janus-daemon (M2), janush (M3), and janus::tmux (0.3.0) are picked up
# automatically by `compile` as their binaries land.

.PHONY: all bootstrap prereq symlinks compile db-init db-down db-backup db-restore db-migrate health logs uninstall clean ram-disk

# 1. Environment variables (NEVER hardcode a default password).
#    HERDR_PLUGIN_ROOT defaults to the repo checkout (the plugin source dir per
#    herdr-v1-contract §6); Herdr injects the real value when opening a pane.
HERDR_PLUGIN_ROOT ?= $(CURDIR)
HERDR_PLUGIN_STATE_DIR ?= $(HOME)/.local/state/herdr/plugins/metamach.janus
HERDR_PLUGIN_CONFIG_DIR ?= $(HOME)/.config/herdr/plugins/config/metamach.janus
# Native PG data directory (0.3.0: no Docker, PG runs directly on host).
METAMACH_DB_DIR ?= $(HOME)/.metamach/db
# Exported; if password not explicitly set, first read from Mutable State, else generate.
export METAMACH_DB_PASSWORD ?= $(shell [ -f $(HERDR_PLUGIN_STATE_DIR)/.db_password ] && cat $(HERDR_PLUGIN_STATE_DIR)/.db_password || openssl rand -hex 16)

all: bootstrap

# 2. Prerequisites check (0.3.0: pg_config, tmux, cargo — no Docker).
prereq:
	@echo "🔍 Checking prerequisites..."
	@command -v cargo >/dev/null 2>&1 || { echo "❌ cargo not found. Install Rust 1.88+ (rustup.rs)"; exit 1; }
	@echo "   ✅ cargo $(shell cargo --version | awk '{print $$2}')"
	@command -v tmux >/dev/null 2>&1 || { echo "❌ tmux not found. Install tmux 3.3+"; exit 1; }
	@echo "   ✅ tmux $(shell tmux -V | sed 's/tmux //')"
	@command -v pg_config >/dev/null 2>&1 || { echo "❌ pg_config not found. Install PostgreSQL 16+"; exit 1; }
	@echo "   ✅ pg_config $(shell pg_config --version | awk '{print $$2}')"
	@command -v pg_ctl >/dev/null 2>&1 || { echo "❌ pg_ctl not found. Install PostgreSQL 16+"; exit 1; }
	@echo "   ✅ pg_ctl available"
	@command -v initdb >/dev/null 2>&1 || { echo "❌ initdb not found. Install PostgreSQL 16+"; exit 1; }
	@echo "   ✅ initdb available"
	@echo "✅ All prerequisites satisfied."

# 3. Supreme one-command bootstrap primitive.
bootstrap: prereq symlinks compile db-init
	@echo "================================================================="
	@echo "🪐 MetaMach 0.3.0 successfully bootstrapped in Richmond Hill!"
	@echo "🔌 Run 'prefix+j' inside Herdr to open Dispatcher Console."
	@echo "================================================================="

# 4. Establish Immutable/Mutable physical directories & symlinks.
symlinks:
	@echo "📁 Creating mutable state and config directories..."
	@mkdir -p $(HERDR_PLUGIN_CONFIG_DIR)
	@mkdir -p $(HERDR_PLUGIN_STATE_DIR)
	@mkdir -p $(METAMACH_DB_DIR)
	@printf '%s' "$(METAMACH_DB_PASSWORD)" > $(HERDR_PLUGIN_STATE_DIR)/.db_password && chmod 600 $(HERDR_PLUGIN_STATE_DIR)/.db_password
	@echo "🔑 DB password persisted to $(HERDR_PLUGIN_STATE_DIR)/.db_password (chmod 600, gitignored). Save it now."
	@echo "🔗 Linking agents config into Herdr Config Directory..."
	@ln -sf $(CURDIR)/configs/agents.toml $(HERDR_PLUGIN_CONFIG_DIR)/agents.toml

# 5. Local-compile Janus binaries; install only those built in this milestone.
compile:
	@echo "🦀 Compiling Janus binaries..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ Installing built binaries to $(HERDR_PLUGIN_ROOT)/bin/..."
	@mkdir -p $(HERDR_PLUGIN_ROOT)/bin
	@for bin in janus janus-daemon herdr-janus janush; do \
		if [ -f janus/target/release/$$bin ]; then \
			cp janus/target/release/$$bin $(HERDR_PLUGIN_ROOT)/bin/$$bin; \
			echo "   installed $$bin"; \
		else \
			echo "   (skip $$bin - not built in this milestone)"; \
		fi; \
	done

# 6. Initialize native Postgres (0.3.0: no Docker).
#    make db-init launches PG + catalog migration; per-blueprint migrations run on janus onboard.
db-init:
	@echo "🐘 Initializing native Postgres at $(METAMACH_DB_DIR)..."
	@if [ ! -f $(METAMACH_DB_DIR)/PG_VERSION ]; then \
		echo "   Running initdb..."; \
		initdb -D $(METAMACH_DB_DIR) --username=metamach_admin --auth=trust; \
		echo "   Starting PG..."; \
		pg_ctl -D $(METAMACH_DB_DIR) -l $(METAMACH_DB_DIR)/pg.log start; \
		echo "   Creating metamach_db..."; \
		createdb -h $(METAMACH_DB_DIR) -U metamach_admin metamach_db; \
		echo "   Running catalog migration (001_catalog.sql only)..."; \
		psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f janus/migrations/001_catalog.sql; \
	else \
		echo "   PG already initialized at $(METAMACH_DB_DIR)."; \
		pg_ctl -D $(METAMACH_DB_DIR) -l $(METAMACH_DB_DIR)/pg.log start 2>/dev/null || echo "   (already running)"; \
	fi
	@echo "⏳ Waiting for database health check..."
	@until pg_isready -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db; do sleep 0.5; done
	@echo "⚡ Database is online and migrated."

# 7. Safe shutdown; release physical resources.
db-down:
	@echo "🔌 Stopping database..."
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || echo "   (already stopped)"

# 8. Database backup (pg_dump to timestamped SQL file).
db-backup:
	@echo "💾 Backing up metamach_db..."
	@pg_dump -h $(METAMACH_DB_DIR) -U metamach_admin metamach_db > metamach_backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✅ Backup written to metamach_backup_*.sql"

# 9. Database restore (requires BACKUP_FILE variable).
db-restore:
	@if [ -z "$(BACKUP_FILE)" ]; then echo "❌ Usage: make db-restore BACKUP_FILE=backup.sql"; exit 1; fi
	@echo "🔄 Restoring metamach_db from $(BACKUP_FILE)..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db < $(BACKUP_FILE)
	@echo "✅ Restore complete."

# 10. Run catalog migration (idempotent: 001_catalog.sql uses IF NOT EXISTS).
db-migrate:
	@echo "🔄 Running catalog migration (001_catalog.sql)..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f janus/migrations/001_catalog.sql
	@echo "✅ Catalog migration complete. Per-blueprint migrations (002_blueprint.sql) run on janus onboard."

# 11. Health check (native PG liveness; Daemon socket).
health:
	@echo "=== MetaMach Health Check ==="
	@pg_isready -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db || echo "❌ DB offline"
	@test -S $(HERDR_PLUGIN_STATE_DIR)/janus.sock && echo "✅ Daemon socket alive" || echo "❌ Daemon socket missing"

# 12. Log viewing (Daemon log).
logs:
	@tail -n 200 $(HERDR_PLUGIN_STATE_DIR)/janus.log 2>/dev/null || echo "(no janus.log; Daemon defaults to stderr)"

# 13. macOS RAM disk workaround (macOS has no /dev/shm).
ram-disk:
	@echo "💾 Creating macOS RAM disk for secret storage..."
	@if [ "$$(uname -s)" = "Darwin" ]; then \
		SIZE_MB=64; \
		DEV=$$(hdiutil attach -nomount ram://$$((SIZE_MB * 2048)) 2>/dev/null | tail -1 | tr -d ' '); \
		if [ -n "$$DEV" ]; then \
			newfs_hfs -v metamach_ramdisk $$DEV >/dev/null 2>&1; \
			mkdir -p /tmp/metamach_ramdisk; \
			mount -t hfs $$DEV /tmp/metamach_ramdisk; \
			echo "   ✅ $$SIZE_MB MB RAM disk mounted at /tmp/metamach_ramdisk"; \
			echo "   ⚠️  macOS: keys are memory-resident but /dev/shm semantics differ from Linux."; \
			echo "   ⚠️  Do not use for production credentials."; \
		else \
			echo "   ❌ Failed to create RAM disk."; \
		fi; \
	else \
		echo "   ✅ Linux: /dev/shm is available natively. No RAM disk needed."; \
	fi

# 14. Full uninstall (teardown everything). The confirmation prompt runs FIRST;
#     clean is invoked from the recipe body only after the user confirms.
uninstall:
	@echo "⚠️  This will DELETE all MetaMach data. Continue? [y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@$(MAKE) --no-print-directory clean
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || true
	@rm -rf $(METAMACH_DB_DIR)
	@rm -rf $(HERDR_PLUGIN_CONFIG_DIR)
	@rm -rf $(HERDR_PLUGIN_STATE_DIR)
	@rm -f $(HERDR_PLUGIN_ROOT)/bin/janus-daemon $(HERDR_PLUGIN_ROOT)/bin/herdr-janus $(HERDR_PLUGIN_ROOT)/bin/janush $(HERDR_PLUGIN_ROOT)/bin/janus
	@echo "🗑️  MetaMach fully uninstalled."

# 15. Clean local compile cache.
clean:
	@echo "🧹 Cleaning cargo workspace..."
	@cd janus && cargo clean
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  Wiping RAM disk secrets at /dev/shm/metamach.janus..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi
	@if [ -d /tmp/metamach_ramdisk ]; then \
		echo "⚠️  Unmounting macOS RAM disk at /tmp/metamach_ramdisk..."; \
		umount /tmp/metamach_ramdisk 2>/dev/null || true; \
	fi
