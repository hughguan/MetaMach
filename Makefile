# MetaMach 1.0 - Factory master switch (Deployment-Spec §5.1)
#
# M1 scope: compiles herdr-janus, brings up Absurd Postgres, wires mutable dirs.
# janus-daemon (M2) and janus-sh (M3) are picked up automatically by `compile`
# as their binaries land - no stub targets, no broken copies.

.PHONY: all bootstrap symlinks compile db-up db-down db-backup db-restore db-migrate health logs uninstall clean

# 1. Environment variables (NEVER hardcode a default password).
#    HERDR_PLUGIN_ROOT defaults to the repo checkout (the plugin source dir per
#    herdr-v1-contract §6); Herdr injects the real value when opening a pane.
HERDR_PLUGIN_ROOT ?= $(CURDIR)
HERDR_PLUGIN_STATE_DIR ?= $(HOME)/.local/state/herdr/plugins/metamach.janus
# Mutable Config dir (herdr-v1-contract §6: note the /config/ segment).
HERDR_PLUGIN_CONFIG_DIR ?= $(HOME)/.config/herdr/plugins/config/metamach.janus
# Exported so docker-compose.yml's bind mount (${METAMACH_PG_SOCKET_DIR}) agrees
# with the mkdir below - otherwise Compose falls back to its /tmp default and the
# socket lands in the wrong place (M2's daemon wouldn't find it).
export METAMACH_PG_SOCKET_DIR ?= $(HERDR_PLUGIN_STATE_DIR)/pg_socket
# Exported; if password not explicitly set, first read from Mutable State, else generate.
export METAMACH_DB_PASSWORD ?= $(shell [ -f $(HERDR_PLUGIN_STATE_DIR)/.db_password ] && cat $(HERDR_PLUGIN_STATE_DIR)/.db_password || openssl rand -hex 16)

all: bootstrap

# 2. Supreme one-command bootstrap primitive.
bootstrap: symlinks compile db-up
	@echo "================================================================="
	@echo "🪐 MetaMach 1.0 successfully bootstrapped in Richmond Hill!"
	@echo "🔌 Run 'prefix+j' inside Herdr to open Dispatcher Console."
	@echo "================================================================="

# 3. Establish Immutable/Mutable physical directories & symlinks.
symlinks:
	@echo "📁 Creating mutable state and config directories..."
	@mkdir -p $(HERDR_PLUGIN_CONFIG_DIR)
	@mkdir -p $(HERDR_PLUGIN_STATE_DIR)
	@mkdir -p $(METAMACH_PG_SOCKET_DIR)
	@printf '%s' "$(METAMACH_DB_PASSWORD)" > $(HERDR_PLUGIN_STATE_DIR)/.db_password && chmod 600 $(HERDR_PLUGIN_STATE_DIR)/.db_password
	@echo "🔑 DB password persisted to $(HERDR_PLUGIN_STATE_DIR)/.db_password (chmod 600, gitignored). Save it now."
	@echo "🔗 Linking agents config into Herdr Config Directory..."
	@ln -sf $(CURDIR)/configs/agents.toml $(HERDR_PLUGIN_CONFIG_DIR)/agents.toml

# 4. Local-compile Janus binaries; install only those built in this milestone.
compile:
	@echo "🦀 Compiling Janus binaries..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ Installing built binaries to $(HERDR_PLUGIN_ROOT)/bin/..."
	@mkdir -p $(HERDR_PLUGIN_ROOT)/bin
	@for bin in janus janus-daemon herdr-janus janus-sh; do \
		if [ -f janus/target/release/$$bin ]; then \
			cp janus/target/release/$$bin $(HERDR_PLUGIN_ROOT)/bin/$$bin; \
			echo "   installed $$bin"; \
		else \
			echo "   (skip $$bin - not built in this milestone)"; \
		fi; \
	done

# 5. Start the Absurd Postgres container (Unix Socket only, no TCP).
db-up:
	@echo "🐳 Starting Absurd Postgres container (Unix Socket only, no TCP)..."
	@mkdir -p $(METAMACH_PG_SOCKET_DIR)
	@docker compose up -d
	@echo "⏳ Waiting for container to be running..."
	@until docker compose ps metamach-db | grep -q "Up"; do sleep 0.5; done
	@echo "⏳ Waiting for database health check..."
	@docker compose exec -T metamach-db sh -c "until pg_isready -U metamach_admin -d metamach_db; do sleep 1; done"
	@echo "⚡ Database is online and migrated."

# 6. Safe shutdown; release physical resources.
db-down:
	@echo "🔌 Stopping database..."
	@docker compose down

# 7. Database backup (pg_dump to timestamped SQL file).
db-backup:
	@echo "💾 Backing up metamach_db..."
	@docker compose exec -T metamach-db pg_dump -U metamach_admin metamach_db > metamach_backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✅ Backup written to metamach_backup_*.sql"

# 8. Database restore (requires BACKUP_FILE variable).
db-restore:
	@if [ -z "$(BACKUP_FILE)" ]; then echo "❌ Usage: make db-restore BACKUP_FILE=backup.sql"; exit 1; fi
	@echo "🔄 Restoring metamach_db from $(BACKUP_FILE)..."
	@docker compose exec -T metamach-db psql -U metamach_admin -d metamach_db < $(BACKUP_FILE)
	@echo "✅ Restore complete."

# 9. Run pending migrations (idempotent: 001_init_absurd.sql uses IF NOT EXISTS).
db-migrate:
	@echo "🔄 Running pending migrations..."
	@docker compose exec -T metamach-db sh -c "for f in /docker-entrypoint-initdb.d/*.sql; do psql -U metamach_admin -d metamach_db -f \$$f; done"
	@echo "✅ Migrations complete."

# 10. Health check (DB liveness; Daemon socket lands in M2).
health:
	@echo "=== MetaMach Health Check ==="
	@docker compose exec -T metamach-db pg_isready -U metamach_admin -d metamach_db || echo "❌ DB offline"
	@test -S $(HERDR_PLUGIN_STATE_DIR)/janus.sock && echo "✅ Daemon socket alive" || echo "❌ Daemon socket missing (M2)"

# 11. Log viewing (Daemon log lands in M2).
logs:
	@tail -n 200 $(HERDR_PLUGIN_STATE_DIR)/janus.log 2>/dev/null || echo "(no janus.log yet; Daemon lands in M2)"

# 12. Full uninstall (teardown everything). The confirmation prompt runs FIRST;
#     clean is invoked from the recipe body only after the user confirms, so
#     declining leaves build artifacts and the DB container untouched.
uninstall:
	@echo "⚠️  This will DELETE all MetaMach data. Continue? [y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@$(MAKE) --no-print-directory clean
	@docker compose down -v
	@rm -rf $(HERDR_PLUGIN_CONFIG_DIR)
	@rm -rf $(HERDR_PLUGIN_STATE_DIR)
	@rm -f $(HERDR_PLUGIN_ROOT)/bin/janus-daemon $(HERDR_PLUGIN_ROOT)/bin/herdr-janus $(HERDR_PLUGIN_ROOT)/bin/janus-sh
	@echo "🗑️  MetaMach fully uninstalled."

# 13. Clean local compile cache & RAM disk.
clean:
	@echo "🧹 Cleaning cargo workspace..."
	@cd janus && cargo clean
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  Wiping RAM disk secrets at /dev/shm/metamach.janus..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi
