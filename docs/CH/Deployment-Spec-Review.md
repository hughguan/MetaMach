# Deployment-Spec Review — Operations & Infrastructure Deep-Dive

> **Document under review:** `docs/CH/Deployment-Spec.md`
> **Review lens:** Deployability, security hardening, failure modes, platform compatibility, operational procedures

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Deployment fails or creates security vulnerability |
| 🟠 **HIGH** | Significant operational risk or incompatibility |
| 🟡 **MEDIUM** | Missing procedure, unclear step, or unvalidated assumption |
| ⚪ **LOW** | Polish / naming |

---

## 1. 🔴 Security & Correctness Blockers

### 1.1 Hardcoded database password in Makefile

```makefile
export METAMACH_DB_PASSWORD ?= metamach_secure_pass_2026
```

This default password is:
- Committed to a public Git repository (visible in plaintext)
- Trivially guessable
- Used for a database that, per the spec, contains agent execution traces and potentially sensitive `result_cache` data

The `?=` operator means it's overridable by environment variable, but the default should never be a hardcoded secret. Many users will run `make bootstrap` without setting `METAMACH_DB_PASSWORD` and unknowingly deploy with a publicly known password.

**Recommendation:** 
1. Remove the default value entirely
2. `make bootstrap` checks if `METAMACH_DB_PASSWORD` is set; if not, generates a random 32-char password and prints it:
```makefile
METAMACH_DB_PASSWORD ?= $(shell openssl rand -hex 16)
```
3. Print the generated password ONCE at bootstrap time with a warning to save it
4. Add `METAMACH_DB_PASSWORD` to `.gitignore` mention (users may create `.env` files)

---

### 1.2 `docker-compose.yml` exposes Postgres on localhost with hardcoded credentials

```yaml
ports:
  - "127.0.0.1:5432:5432"
```

While `127.0.0.1` binding limits exposure to localhost, any local process can connect with the hardcoded (or default) credentials. On a multi-user system, another user on the same machine could access the database if they guess the port and password.

**Recommendation:** 
- Use Unix socket instead of TCP for local Postgres connections (remove the `ports` mapping, connect via socket)
- If TCP is required (Docker on macOS doesn't support host sockets easily), at minimum use a random port or require explicit `METAMACH_DB_PORT` configuration
- Document: for production deployments, use TLS-enabled connections and strong passwords

---

### 1.3 `decrypt_secrets.sh` has no error handling for missing `sops` or `age`

```bash
export SOPS_AGE_KEY_FILE="$HOME/.config/sops/age/keys.txt"
sops --decrypt --output "$DECRYPTED_KEY" configs/sops/hi5bot.encrypted.json
```

If `sops` is not installed, the script will fail with a cryptic "command not found" error after creating the RAM disk directory. If `$SOPS_AGE_KEY_FILE` doesn't exist, sops will fail with an unclear error.

**Recommendation:** Add prerequisite checks at the top:
```bash
if ! command -v sops &> /dev/null; then
    echo "❌ sops is not installed. Install with: brew install sops (macOS) or apt install sops (Linux)"
    exit 1
fi
if [ ! -f "$SOPS_AGE_KEY_FILE" ]; then
    echo "⚠️ Age key not found at $SOPS_AGE_KEY_FILE. Skipping secret decryption."
    exit 1
fi
```

---

### 1.4 `clean` target unconditionally deletes `/dev/shm/metamach.janus`

```makefile
clean:
	@rm -rf /dev/shm/metamach.janus
```

This is unsafe:
- If `/dev/shm/metamach.janus` is a symlink (malicious or misconfigured), `rm -rf` follows it
- No check that the directory actually contains MetaMach secrets before deletion
- No warning that decrypted secrets will be lost

**Recommendation:**
```makefile
clean:
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  Wiping RAM disk secrets..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi
	@cd janus && cargo clean
```

---

## 2. 🟠 Platform Compatibility

### 2.1 `/dev/shm` does not exist as tmpfs on macOS

The deployment spec lists macOS 13+ as supported (§1), but:

- macOS does not have `/dev/shm` by default
- On macOS, `/dev/shm` may not exist at all, or may be a regular directory on the root filesystem (not tmpfs/ramdisk)
- The `decrypt_secrets.sh` script will create a regular directory at `/dev/shm/metamach.janus` — secrets will be written TO DISK, defeating the entire purpose of RAM-disk security

**Recommendation:**
- For macOS: use `$TMPDIR` (which is per-user and may be memory-backed on some versions) or create a RAM disk via `hdiutil`:
  ```bash
  # macOS RAM disk
  RAM_DISK=$(hdiutil attach -nomount ram://2048 2>/dev/null)
  diskutil eraseVolume HFS+ "MetaMachSecrets" $RAM_DISK
  ```
- Or, for macOS development only, document that `/dev/shm` is a regular directory and secrets are NOT protected at rest — production deployments must use Linux
- **Strongly recommend:** make Linux the only supported production OS; macOS is development-only

---

### 2.2 UDS path length limits differ between macOS and Linux

`${HERDR_PLUGIN_STATE_DIR}/janus.sock` expands to:
- macOS: `/Users/<user>/.local/state/herdr/plugins/metamach.janus/janus.sock` (~80 chars)
- Linux: `/home/<user>/.local/state/herdr/plugins/metamach.janus/janus.sock` (~75 chars)

macOS has a 104-character limit for UDS paths (vs. 108 on Linux). Long usernames could approach this limit. Add a validation in the Daemon startup that the socket path is within bounds.

**Recommendation:** Add to bootstrap: validate that `janus.sock` path length < 100 chars, warn if approaching the limit.

---

### 2.3 `cp janus/target/release/janus-sh janus/target/release/target_sh` is nonsensical

```makefile
compile:
	@cp janus/target/release/janus-sh janus/target/release/target_sh
```

This copies `janus-sh` to `target_sh` in the same directory. What is `target_sh`? It's never referenced elsewhere. This looks like either:
- A debug artifact that was accidentally committed
- A placeholder for a real installation step (copy to `/usr/local/bin/` or `${HERDR_PLUGIN_ROOT}/bin/`)

**Recommendation:** Replace with a proper installation:
```makefile
	@cp janus/target/release/janus-sh /usr/local/bin/janus-sh
	@cp janus/target/release/janus-daemon /usr/local/bin/janus-daemon
	@cp janus/target/release/herdr-janus ${HERDR_PLUGIN_ROOT}/bin/herdr-janus
```

---

## 3. 🟡 Operational Gaps

### 3.1 No backup/restore procedure

The deployment spec covers initial setup and shutdown, but not:
- How to back up the Postgres database
- How to restore from backup
- How to migrate between versions (schema migrations beyond initial `001_init_absurd.sql`)
- What happens to `fallback.db` during a version upgrade

**Recommendation:** Add a §7: Backup & Migration:
```bash
# Backup
make db-backup  # → docker compose exec metamach-db pg_dump ... > backup-$(date).sql

# Restore
make db-restore BACKUP_FILE=backup-2026-07-15.sql

# Migrate (after version bump)
make db-migrate  # → runs new migrations in janus/migrations/
```

---

### 3.2 No health check or monitoring setup

After `make bootstrap`, there's no way to verify the system is healthy beyond the manual sanity checks in §6. For production operations:
- Is the Daemon running? (`janus.pid` exists + process alive)
- Is Postgres healthy? (pg_isready)
- Are there any suspended tasks needing attention?
- Disk usage of Postgres volume?

**Recommendation:** Add a `make health` target:
```makefile
health:
	@echo "=== MetaMach Health Check ==="
	@docker compose exec -T metamach-db pg_isready -U metamach_admin -d metamach_db || echo "❌ DB offline"
	@test -S ${HERDR_PLUGIN_STATE_DIR}/janus.sock && echo "✅ Daemon socket alive" || echo "❌ Daemon socket missing"
	@# ... more checks
```

---

### 3.3 No log rotation or retention policy

The Daemon produces logs, the database accumulates `result_cache` data, and `fallback.db` serves as a ring buffer. But:
- Where do Daemon logs go? (stdout? syslog? a file?)
- What is the log rotation policy?
- The 16KB budget covers Step output, but what about Daemon's own operational logs?
- `fallback.db` ring buffer size is never specified

**Recommendation:** 
- Daemon logs → `${HERDR_PLUGIN_STATE_DIR}/janus.log` with rotation at 10MB, keep 5 files
- `fallback.db` ring buffer: max 1000 entries or 50MB, whichever comes first
- Add `make logs` target to tail the Daemon log

---

### 3.4 Sanity check §6.3 uses undefined behavior expectation

> "Daemon 启动后在 `0.5s` 内自动识别到之前的 `SUSPENDED` 任务"

The Feature-Spec says cold start reads the last `COMPLETED` step, not `SUSPENDED`. These are different states:
- `COMPLETED` → the step finished successfully; next step should start
- `SUSPENDED` → HITL is pending; human intervention needed first

The sanity check says "SUSPENDED" but the Feature-Spec says "COMPLETED". Which is correct?

**Recommendation:** Align with Feature-Spec §2.3: cold start reads last `COMPLETED` checkpoint and resumes with the next step. `SUSPENDED` tasks remain suspended and notify the厂长 on Daemon restart.

---

### 3.5 No uninstall/teardown procedure

`make clean` removes build artifacts and RAM disk, and `make db-down` stops the container. But there's no procedure for:
- Complete removal of all MetaMach data (Postgres volume, state directories, symlinks, installed binaries)
- What to do if the user wants to start fresh

**Recommendation:** Add `make uninstall`:
```makefile
uninstall: clean db-down
	@echo "⚠️  This will DELETE all MetaMach data. Continue? [y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@docker compose down -v        # remove volumes
	@rm -rf ~/.config/herdr/plugins/metamach.janus
	@rm -rf ~/.local/state/herdr/plugins/metamach.janus
	@rm -f /usr/local/bin/janus-daemon /usr/local/bin/janus-sh /usr/local/bin/herdr-tether
	@echo "MetaMach fully uninstalled."
```

---

## 4. 🟡 Dependency Management

### 4.1 SOPS & Age are listed as prerequisites but usage is entirely optional

The prerequisites table lists SOPS v3.8+ and Age v1.1+ as required, but:
- `decrypt_secrets.sh` gracefully skips if the encrypted file doesn't exist
- No other part of the system requires SOPS/Age
- For a project without financial blueprints, SOPS is unused dead weight

**Recommendation:** Move SOPS & Age to "Optional Dependencies" with a note: "Required only if using encrypted secrets for financial blueprints."

---

### 4.2 No version pinning for Docker images

```yaml
image: postgres:15-alpine
```

The `15-alpine` tag is floating (it tracks the latest 15.x patch). A future Postgres 15.x update could introduce breaking changes. CI builds might pass one day and fail the next.

**Recommendation:** Pin to a specific digest or at minimum a minor version:
```yaml
image: postgres:15.8-alpine
```

---

### 4.3 `docker compose exec -T` requires the container to be running

```makefile
db-up:
	@docker compose up -d
	@docker compose exec -T metamach-db sh -c "until pg_isready..."
```

There's a race condition: `up -d` returns immediately, but the container may not have started Postgres yet. `docker compose exec` will fail if the container isn't ready. The `until pg_isready` loop handles Postgres readiness, but if the container itself isn't running yet, `exec` fails before the loop starts.

**Recommendation:** Add a wait for container to be running:
```makefile
db-up:
	@docker compose up -d
	@echo "⏳ Waiting for container..."
	@until docker compose ps metamach-db | grep -q "Up"; do sleep 0.5; done
	@echo "⏳ Waiting for database health check..."
	@docker compose exec -T metamach-db sh -c "until pg_isready -U metamach_admin -d metamach_db; do sleep 1; done"
```

---

## 5. ⚪ Minor Issues

- §3.1 `docker-compose.yml` uses `version: '3.8'` — deprecated in Docker Compose v2. Remove the `version` key.
- §5.1 Makefile uses `ln -sf $$(pwd)/configs/agents.toml` — if `configs/agents.toml` doesn't exist yet, this creates a broken symlink. Add validation.
- §6.1 sanity check says "手机 Teams/Telegram 收到安全挂起报警" — this won't work without webhook setup, which isn't covered in the deployment spec. Add a note: "Notification test requires Teams/TG webhook configuration (see configs/notifications.toml)."
- §6.2 uses `herdr-tether open` — but this binary hasn't been compiled or installed by `make bootstrap` (as noted in Project-Plan-Review, Tether is M4).

---

## Summary: Deployment-Spec Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Remove hardcoded DB password; auto-generate |
| 2 | 🔴 | Use Unix socket or random port for Postgres |
| 3 | 🔴 | Add prerequisite checks to decrypt_secrets.sh |
| 4 | 🔴 | Make `clean` target safe (check before `rm -rf`) |
| 5 | 🟠 | Address /dev/shm incompatibility on macOS |
| 6 | 🟠 | Add UDS path length validation for macOS |
| 7 | 🟠 | Fix `cp janus-sh → target_sh` (install properly) |
| 8 | 🟡 | Add backup/restore procedures |
| 9 | 🟡 | Add `make health` monitoring target |
| 10 | 🟡 | Specify log rotation and retention |
| 11 | 🟡 | Align sanity check §6.3 state (COMPLETED vs SUSPENDED) |
| 12 | 🟡 | Add `make uninstall` target |
| 13 | 🟡 | Downgrade SOPS/Age to optional dependencies |
| 14 | 🟡 | Pin Docker image to specific version |
| 15 | 🟡 | Fix race condition in `db-up` target |
| 16 | ⚪ | Remove deprecated `version` from docker-compose.yml |

> **Resolution Log (2026-07-15):**
> - **#1 🔴 (hardcoded DB password)** ✅ RESOLVED - Makefile no longer hardcodes a default; `METAMACH_DB_PASSWORD` is read from `${HERDR_PLUGIN_STATE_DIR}/.db_password` or generated via `openssl rand -hex 16`, persisted (chmod 600, gitignored), printed once at bootstrap.
> - **#2 🔴 (Postgres TCP exposure)** ✅ RESOLVED - docker-compose `ports` removed; `command: postgres -c listen_addresses=''` disables TCP; Unix Socket mounted to `${METAMACH_PG_SOCKET_DIR}`; daemon connects via socket.
> - **#3 🔴 (decrypt_secrets.sh prereq checks)** ✅ RESOLVED - Script now guards `command -v sops`, `command -v age`, and `$SOPS_AGE_KEY_FILE` existence with clear errors before any RAM-disk work.
> - **#4 🔴 (`clean` unsafe rm -rf)** ✅ RESOLVED - `clean` now guards `if [ -d /dev/shm/metamach.janus ]` before deletion, with a warning.
> - **Bonus #11 🟡 (§6.3 COMPLETED vs SUSPENDED)** ✅ RESOLVED - §6.3 aligned to Feature-Spec §2.3: `RUNNING` resumes from last `COMPLETED` checkpoint; `SUSPENDED` stays suspended + notifies.
> - **Bonus #14 🟡 (pin Docker image)** ✅ RESOLVED - image pinned to `postgres:15.8-alpine`.
> - **Bonus #16 ⚪ (deprecated `version`)** ✅ RESOLVED - `version: '3.8'` removed from docker-compose.
>
> **Round 3 (🟠 items, 2026-07-15):**
> - **#5 🟠 (macOS /dev/shm)** ✅ RESOLVED - §4 adds platform note: production Linux-only; macOS dev uses `$TMPDIR`/`hdiutil` RAM disk (secrets not memory-backed on macOS).
> - **#6 🟠 (UDS path length validation)** ✅ RESOLVED - Makefile `symlinks` validates `janus.sock` path <100 chars (macOS 104-char UDS limit).
> - **#7 🟠 (cp janus-sh -> target_sh)** ✅ RESOLVED - `compile` now installs binaries to `${HERDR_PLUGIN_ROOT}/bin/` absolute paths (also aids the absolute-`SHELL` path concern).
