
### ── 动静隔离架构、物理沙箱挂载与统一数据库并网指南

> **EN:** Deployment Spec — immutable/mutable isolation, sandbox key mounting, and Absurd Postgres bring-up.

本部署规范书（Deploy Spec）旨在指导系统管理员或厂长在本地物理算力节点（如 Richmond Hill 车间服务器）上安全、幂等、无缝地完成 **MetaMach 2.0** 生产底座的并网通电。

本规范严格遵循 Herdr v1 插件的“动静隔离（Immutable ROOT vs. Mutable State）”规范与安全性红线，对物理目录、内存盘挂载、数据库容器及一键引导流程进行系统级定义。

## 1. 部署环境与物理依赖 (Prerequisites)

在开始部署前，必须确保宿主机满足以下物理与软件依赖：

|**依赖组件**|**最低版本要求**|**物理用途**|**验证指令**|
|---|---|---|---|
|**操作系统**|Ubuntu 22.04+ / macOS 13+|提供标准的 POSIX 兼容环境与 UDS 支持|`uname -a`|
|**Rust 工具链**|Rust 1.88+ (Edition 2024)|编译 `janus-daemon`、`herdr-janus` 与 `janus-sh`|`rustc --version`|
|**Tmux**|Tmux 3.2+|Tether 维持 PTY 会话长生不老的物理载体|`tmux -V`|
|**Docker & Compose**|Docker v24.0+ / Compose v2.20+|一键拉起并托管 Absurd Postgres 数据库|`docker compose version`|
|**SOPS & Age**|SOPS v3.8+ / Age v1.1+|（可选）仅在启用加密金融蓝图密钥时必需；非金融项目可省略|`sops --version`|

## 2. 动静隔离物理目录拓扑 (Directory Mapping)

为防止 GitHub 插件更新时意外擦除厂长的本地财务数据、个性化配置与数据库凭证，必须严格实施动静隔离。部署脚本会自动创建并建立符号链接（Symlinks）：

```
[Immutable ROOT (Git Checkout)]       -->  ${HERDR_PLUGIN_ROOT} (~/.local/share/herdr/plugins/metamach.janus)
                                           ├── target/release/ (只读二进制)
                                           └── workflows/ (只读标准 SOP)

[Mutable Config (用户配置区)]        -->  ${HERDR_PLUGIN_CONFIG_DIR} (~/.config/herdr/plugins/metamach.janus)
                                           └── agents.toml (敏感密钥注入点)

[Mutable State (运行状态区)]         -->  ${HERDR_PLUGIN_STATE_DIR} (~/.local/state/herdr/plugins/metamach.janus)
                                           ├── janus.sock (UDS 套接字)
                                           ├── janus.pid (单例进程锁)
                                           └── fallback.db (本地灾备 SQLite)
```

## 3. 统一数据库并网 (Unified Database Setup)

系统采用单库多租户设计（通过 `blueprint_id` 逻辑隔离）。通过 Docker Compose 在本地拉起高性能 Postgres 实例。

### 3.1 容器编排：`docker-compose.yml`

在 `metamach/` 根目录下创建该文件：

YAML

```
services:
  metamach-db:
    image: postgres:15.8-alpine               # 锁定次版本，避免 floating tag 漂移
    container_name: metamach-postgres-db
    command: postgres -c listen_addresses=''   # 禁用 TCP 监听，仅暴露 Unix Socket，彻底杜绝网络面
    environment:
      POSTGRES_DB: metamach_db
      POSTGRES_USER: metamach_admin
      POSTGRES_PASSWORD: ${METAMACH_DB_PASSWORD} # 由 make bootstrap 随机生成并注入（见 §5.1 Makefile）
    volumes:
      - metamach_pgdata:/var/lib/postgresql/data
      - ./janus/migrations:/docker-entrypoint-initdb.d # 容器初始化时自动执行迁移
      - ${METAMACH_PG_SOCKET_DIR}:/var/run/postgresql  # Unix Socket 挂出至宿主机状态目录，宿主进程经 socket 连接
    restart: always
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U metamach_admin -d metamach_db"]
      interval: 5s
      timeout: 5s
      retries: 5

volumes:
  metamach_pgdata:
    driver: local
```

> **安全说明（对应 Deployment 评审 #2）**：已移除 `ports` 映射，Postgres 不再监听任何 TCP 端口（`listen_addresses=''`），宿主机 `janus-daemon` 经 Unix Socket 连接：连接串形如 `postgresql://metamach_admin:${METAMACH_DB_PASSWORD}@/metamach_db?host=${METAMACH_PG_SOCKET_DIR}`。即使是同机其他用户也无法经由 TCP 猜测口令连入。

## 4. 物理沙箱密钥解密与挂载 (RAM Disk Decryption)

为确保金融 Blueprint（如交易账户）的 Refresh Token 绝不以明文形式残留在物理磁盘中，系统在运行时通过 `decrypt_secrets.sh` 执行内存盘挂载与阅后即焚。

> ⚠️ **平台注意（macOS `/dev/shm` 不可用）**：macOS 默认不存在 `/dev/shm` tmpfs，`mkdir -p /dev/shm/...` 会在根文件系统创建**普通目录**，密钥将落盘，彻底丧失内存盘安全性。故：**生产部署仅支持 Linux**；macOS 仅限开发用途，须改用 `$TMPDIR` 或 `hdiutil attach -nomount ram://2048` 创建真 RAM 盘，并明确注明“macOS 下密钥非内存态、不可用于承载真实金融凭证”。

### 4.1 解密与挂载脚本：`provisioning/decrypt_secrets.sh`

Bash

```
#!/usr/bin/env bash
set -euo pipefail

# 0. 前置依赖检查：sops / age 必须就位，否则给出明确报错而非 cryptic "command not found"
export SOPS_AGE_KEY_FILE="$HOME/.config/sops/age/keys.txt"

if ! command -v sops >/dev/null 2>&1; then
    echo "❌ sops 未安装。请先安装：macOS 'brew install sops' / Linux 'apt install sops'。"
    exit 1
fi
if ! command -v age >/dev/null 2>&1; then
    echo "❌ age 未安装。请先安装：macOS 'brew install age' / Linux 'apt install age'。"
    exit 1
fi
if [ ! -f "$SOPS_AGE_KEY_FILE" ]; then
    echo "❌ Age 私钥不存在于 $SOPS_AGE_KEY_FILE，无法解密金融凭证。"
    exit 1
fi

# 1. 声明内存盘临时路径
RAM_DISK_PATH="/dev/shm/metamach.janus"
DECRYPTED_KEY="${RAM_DISK_PATH}/hi5bot.decrypted"

# 2. 创建高安全级别的内存盘子目录
if [ ! -d "$RAM_DISK_PATH" ]; then
    mkdir -p "$RAM_DISK_PATH"
    chmod 0700 "$RAM_DISK_PATH" # 仅当前运行用户有权访问
fi

# 3. 使用 Age 私钥通过 SOPS 同步解密至内存盘
if [ -f "configs/sops/hi5bot.encrypted.json" ]; then
    echo "🔑 Decrypting financial secrets directly to RAM disk..."
    sops --decrypt --output "$DECRYPTED_KEY" configs/sops/hi5bot.encrypted.json
    chmod 0600 "$DECRYPTED_KEY" # 严格限制读写权限
    echo "✨ Secrets loaded successfully in volatile RAM."
else
    echo "⚠️ Warning: Financial secrets not found. Skipping financial vault setup."
fi
```

## 5. 一键通电引导流程 (Makefile Bootstrap)

MetaMach 2.0 提供高度简化的“一键通电并网”指令。厂长只需在根目录下执行 `make bootstrap`，系统即会自动完成环境校验、代码编译、目录建立、符号链接挂载及数据库初始化。

### 5.1 自动化部署总闸：`Makefile`

Makefile

```
.PHONY: all bootstrap compile symlinks db-up db-down db-backup db-restore db-migrate health logs uninstall clean

# 1. 设置环境变量（严禁硬编码口令默认值）
HERDR_PLUGIN_STATE_DIR ?= ~/.local/state/herdr/plugins/metamach.janus
METAMACH_PG_SOCKET_DIR ?= $(HERDR_PLUGIN_STATE_DIR)/pg_socket
# 口令未显式设置时，优先从 Mutable State 读取；均不存在则随机生成（由 bootstrap 首次持久化）
export METAMACH_DB_PASSWORD ?= $(shell [ -f $(HERDR_PLUGIN_STATE_DIR)/.db_password ] && cat $(HERDR_PLUGIN_STATE_DIR)/.db_password || openssl rand -hex 16)

all: bootstrap

# 2. 一键通电最高控制原语
bootstrap: symlinks compile db-up
	@echo "================================================================="
	@echo "🪐 MetaMach 2.0 successfully bootstrapped in Richmond Hill!"
	@echo "🔌 Run 'prefix+j' inside Herdr to open Dispatcher Console."
	@echo "================================================================="

# 3. 建立动静隔离物理目录与符号链接
symlinks:
	@echo "📁 Creating mutable state and config directories..."
	@mkdir -p ~/.config/herdr/plugins/metamach.janus
	@mkdir -p ~/.local/state/herdr/plugins/metamach.janus
	@mkdir -p $(METAMACH_PG_SOCKET_DIR)
	@SOCK="$(HERDR_PLUGIN_STATE_DIR)/janus.sock"; len=$$(printf '%s' "$$SOCK" | wc -c | tr -d ' '); [ "$$len" -lt 100 ] || { echo "❌ janus.sock path too long ($$len chars; macOS UDS limit 104): $$SOCK"; exit 1; }
	@printf '%s' "$(METAMACH_DB_PASSWORD)" > $(HERDR_PLUGIN_STATE_DIR)/.db_password && chmod 600 $(HERDR_PLUGIN_STATE_DIR)/.db_password
	@echo "🔑 DB password persisted to $(HERDR_PLUGIN_STATE_DIR)/.db_password (chmod 600, gitignored). Save it now."
	@echo "🔗 Linking agents config into Herdr Config Directory..."
	@ln -sf $$(pwd)/configs/agents.toml ~/.config/herdr/plugins/metamach.janus/agents.toml

# 4. 本地编译 Janus Core 二进制组件
compile:
	@echo "🦀 Compiling Janus Daemon, Client, and janus-sh proxy..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ Installing binaries to absolute well-known paths..."
	@mkdir -p ${HERDR_PLUGIN_ROOT}/bin
	@cp janus/target/release/janus-sh ${HERDR_PLUGIN_ROOT}/bin/janus-sh
	@cp janus/target/release/janus-daemon ${HERDR_PLUGIN_ROOT}/bin/janus-daemon
	@cp janus/target/release/herdr-janus ${HERDR_PLUGIN_ROOT}/bin/herdr-janus

# 5. 拉起 Postgres 统一数据库容器
db-up:
	@echo "🐳 Starting Absurd Postgres container (Unix Socket only, no TCP)..."
	@mkdir -p $(METAMACH_PG_SOCKET_DIR)
	@docker compose up -d
	@echo "⏳ Waiting for container to be running..."
	@until docker compose ps metamach-db | grep -q "Up"; do sleep 0.5; done
	@echo "⏳ Waiting for database health check..."
	@docker compose exec -T metamach-db sh -c \
		"until pg_isready -U metamach_admin -d metamach_db; do sleep 1; done"
	@echo "⚡ Database is online and migrated."

# 6. 安全停机，释放物理资源
db-down:
	@echo "🔌 Stopping database..."
	@docker compose down

# 7. 清理本地编译缓存与内存盘
clean:
	@echo "🧹 Cleaning cargo workspace and unmounting RAM disk..."
	@cd janus && cargo clean
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  Wiping RAM disk secrets at /dev/shm/metamach.janus..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi

# 8. 数据库备份与恢复
db-backup:
	@echo "💾 Backing up Absurd Postgres..."
	@docker compose exec -T metamach-db pg_dump -U metamach_admin metamach_db > metamach_backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✅ Backup written to metamach_backup_*.sql"

db-restore:
	@if [ -z "$(BACKUP_FILE)" ]; then echo "❌ Usage: make db-restore BACKUP_FILE=backup.sql"; exit 1; fi
	@echo "♻️  Restoring from $(BACKUP_FILE)..."
	@docker compose exec -T metamach-db psql -U metamach_admin -d metamach_db < $(BACKUP_FILE)
	@echo "✅ Restore complete."

db-migrate:
	@echo "🔁 Applying new migrations..."
	@docker compose exec -T metamach-db sh -c 'for f in /docker-entrypoint-initdb.d/*.sql; do psql -U metamach_admin -d metamach_db -f "$$f"; done'
	@echo "✅ Migrations applied."

# 9. 健康巡检
health:
	@echo "=== MetaMach Health Check ==="
	@docker compose exec -T metamach-db pg_isready -U metamach_admin -d metamach_db >/dev/null 2>&1 && echo "✅ Absurd Postgres online" || echo "❌ Absurd Postgres offline"
	@test -S $(HERDR_PLUGIN_STATE_DIR)/janus.sock && echo "✅ Daemon socket alive" || echo "❌ Daemon socket missing"
	@test -f $(HERDR_PLUGIN_STATE_DIR)/janus.pid && kill -0 $$(cat $(HERDR_PLUGIN_STATE_DIR)/janus.pid) 2>/dev/null && echo "✅ Daemon process alive" || echo "❌ Daemon process down"

# 10. 日志查看（Daemon 日志落 ${HERDR_PLUGIN_STATE_DIR}/janus.log，按 10MB 轮转保留 5 份）
logs:
	@tail -n 200 $(HERDR_PLUGIN_STATE_DIR)/janus.log 2>/dev/null || echo "（无 janus.log；Daemon 默认写 stderr，生产建议重定向至 janus.log 并配 logrotate 10MB×5）"

# 11. 完整卸载（交互确认后删除所有数据）
uninstall: clean db-down
	@echo "⚠️  This will DELETE all MetaMach data (PG volume, state, configs, binaries). Continue? [y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@docker compose down -v
	@rm -rf ~/.config/herdr/plugins/metamach.janus ~/.local/state/herdr/plugins/metamach.janus
	@rm -f /usr/local/bin/janus-daemon /usr/local/bin/janus-sh ${HERDR_PLUGIN_ROOT}/bin/herdr-janus
	@echo "🗑️  MetaMach fully uninstalled."
```

## 6. 部署验证与联调对账 (Sanity Check)

在完成 `make bootstrap` 后，厂长必须执行以下三步物理对账，以确信车间流水线具备抵抗断电、黑客与爆库的绝对免疫力：

### 🔍 步骤 6.1：验证 `janus-sh` 物理拦截机制

在终端中，运行以下指令模拟 AI 试图非法外联或执行越权指令：

Bash

```
# 先建哨兵目录与哨兵文件，再尝试用命中黑名单的命令删除它（绝不执行真实系统级删除）
SENTINEL_DIR=/tmp/metamach-deploy-guard-$(uuidgen)
mkdir -p "$SENTINEL_DIR" && echo sentinel > "$SENTINEL_DIR/sentinel"
export SHELL=./janus/target/release/janus-sh
$SHELL -c "rm -rf $SENTINEL_DIR"
test -f "$SENTINEL_DIR/sentinel" && echo "✅ 哨兵存活，命令已被拦截"
```

- **合格表现**：终端屏幕瞬间挂起，未发生任何实际删除行为，且哨兵文件事后仍然存在（证明命令被拦截、未触达真实 Shell）。`~/.local/state/herdr/plugins/metamach.janus/` 目录下产生 UDS 拦截日志，且手机 Teams/Telegram 收到安全挂起报警。
    

### 🔍 步骤 6.2：验证 `remain-on-exit` 进程不死特性

1. 执行 `herdr-tether open --command "sleep 100"` 在后台拉起物理进程。
    
2. 强行关闭 Herdr 前台视图窗口，或直接在宿主机执行 `killall -9 herdr`。
    
3. 在系统终端运行 `tmux list-sessions`。
    

- **合格表现**：后台仍能清晰看到名为 `tether-janus-task-<uuid>` 的 tmux 会话处于活跃运行态。再次进入 Herdr 执行 `herdr-tether attach`，现场 100% 毫秒级还原。
    

### 🔍 步骤 6.3：验证冷启动自愈能力

1. 启动一个持续 1 分钟的物理交叉编译任务。
    
2. 运行 `docker compose stop` 强行杀死 Postgres 数据库，并杀死 `janus-daemon` 进程以模拟突发停电。
    
3. 重新启动 PG 数据库容器，并在终端运行 `target/release/janus-daemon`。
    

- **合格表现**：Daemon 启动后在 `0.5s` 内对断电前未完结的任务分型处置：对 `RUNNING` 态任务，从 `absurd_steps` 表中最后一次 `COMPLETED` 的 Step Checkpoint 无缝接棒重跑下一工位；对 `SUSPENDED` 态任务保持挂起并通知厂长（不盲目重跑），控制台无多余冗余输出。

### 🔍 步骤 6.4：首次上线一个产品蓝图 (Onboard)

`make bootstrap` 只通电底座（数据库、二进制、符号链接），此时车间为**零产品线**状态。厂长必须显式上线一个蓝图才能派单生产：

1. 确认目标蓝图目录就位，例如 `blueprints/gatemetric/` 下含 `janus.toml`（声明 `default_workflow`、`[remote]` 靶机、`[openwiki].scope`）。
    
2. 执行上线指令：

    Bash

    ```
    janus onboard --blueprint gatemetric
    ```

3. 验证租户注册与可派发性：

    Bash

    ```
    # 蓝图已注册为 ACTIVE
    docker compose exec -T metamach-db psql -U metamach_admin -d metamach_db \
        -c "SELECT name, status, default_workflow FROM blueprints;"
    # 无 TUI 环境下巡检车间全局
    janus status
    ```

- **合格表现**：`blueprints` 表出现一行 `gatemetric` / `ACTIVE` 记录；`janus status` 输出当前在途任务（此时应为空，但命令本身返回成功，证明 `progress` 原语与 Daemon 连接正常）；在 Herdr 内 `prefix+j` 唤醒 Popup，派单菜单中已可见 `gatemetric` 并可立即派发。重复执行 `janus onboard` 不产生重复行（幂等）。