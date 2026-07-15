
### ── 动静隔离架构、物理沙箱挂载与统一数据库并网指南

本部署规范书（Deploy Spec）旨在指导系统管理员或厂长在本地物理算力节点（如 Richmond Hill 车间服务器）上安全、幂等、无缝地完成 **MetaMach 2.0** 生产底座的并网通电。

本规范严格遵循 Herdr v1 插件的“动静隔离（Immutable ROOT vs. Mutable State）”规范与安全性红线，对物理目录、内存盘挂载、数据库容器及一键引导流程进行系统级定义。

## 1. 部署环境与物理依赖 (Prerequisites)

在开始部署前，必须确保宿主机满足以下物理与软件依赖：

|**依赖组件**|**最低版本要求**|**物理用途**|**验证指令**|
|---|---|---|---|
|**操作系统**|Ubuntu 22.04+ / macOS 13+|提供标准的 POSIX 兼容环境与 UDS 支持|`uname -a`|
|**Rust 工具链**|Rust 1.88+ (Edition 2024)|编译 `janus-daemon`、`herdr-janus` 与 `janus-sh`|`rustc --version`|
|**Tmux**|Tmux 3.2+|Tether 维持 PTY 会话长生不老的物理载体|`tmux -V`|
|**Docker & Compose**|Docker v24.0+ / Compose v2.20+|一键拉起并托管 Unified Postgres 数据库|`docker compose version`|
|**SOPS & Age**|SOPS v3.8+ / Age v1.1+|保证本地敏感密钥在 Git 单仓中的强加密存储|`sops --version`|

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
version: '3.8'

services:
  metamach-db:
    image: postgres:15-alpine
    container_name: metamach-postgres-db
    environment:
      POSTGRES_DB: metamach_db
      POSTGRES_USER: metamach_admin
      POSTGRES_PASSWORD: ${METAMACH_DB_PASSWORD} # 通过宿主机环境变量安全注入
    ports:
      - "127.0.0.1:5432:5432" # 仅暴露给本地环回，杜绝外网渗透
    volumes:
      - metamach_pgdata:/var/lib/postgresql/data
      - ./janus/migrations:/docker-entrypoint-initdb.d # 容器初始化时自动执行迁移
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

## 4. 物理沙箱密钥解密与挂载 (RAM Disk Decryption)

为确保金融 Blueprint（如交易账户）的 Refresh Token 绝不以明文形式残留在物理磁盘中，系统在运行时通过 `decrypt_secrets.sh` 执行内存盘挂载与阅后即焚。

### 4.1 解密与挂载脚本：`provisioning/decrypt_secrets.sh`

Bash

```
#!/usr/bin/env bash
set -euo pipefail

# 1. 声明内存盘临时路径
RAM_DISK_PATH="/dev/shm/metamach.janus"
DECRYPTED_KEY="${RAM_DISK_PATH}/hi5bot.decrypted"

# 2. 创建高安全级别的内存盘子目录
if [ ! -d "$RAM_DISK_PATH" ]; then
    mkdir -p "$RAM_DISK_PATH"
    chmod 0700 "$RAM_DISK_PATH" # 仅当前运行用户有权访问
fi

# 3. 使用 Age 私钥通过 SOPS 同步解密至内存盘
export SOPS_AGE_KEY_FILE="$HOME/.config/sops/age/keys.txt"

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
.PHONY: all bootstrap compile symlinks db-up db-down clean

# 1. 设置默认环境变量
export METAMACH_DB_PASSWORD ?= metamach_secure_pass_2026

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
	@echo "🔗 Linking agents config into Herdr Config Directory..."
	@ln -sf $$(pwd)/configs/agents.toml ~/.config/herdr/plugins/metamach.janus/agents.toml

# 4. 本地编译 Janus Core 二进制组件
compile:
	@echo "🦀 Compiling Janus Daemon, Client, and janus-sh proxy..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ Installing janus-sh helper to target bin..."
	@cp janus/target/release/janus-sh janus/target/release/target_sh

# 5. 拉起 Postgres 统一数据库容器
db-up:
	@echo "🐳 Starting Unified Postgres container..."
	@docker compose up -d
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
	@rm -rf /dev/shm/metamach.janus
```

## 6. 部署验证与联调对账 (Sanity Check)

在完成 `make bootstrap` 后，厂长必须执行以下三步物理对账，以确信车间流水线具备抵抗断电、黑客与爆库的绝对免疫力：

### 🔍 步骤 6.1：验证 `janus-sh` 物理拦截机制

在终端中，运行以下指令模拟 AI 试图非法外联或执行越权指令：

Bash

```
# 启动代理 Shell 并在交互模式下执行未经 Tool Guard 授权的命令
export SHELL=./janus/target/release/janus-sh
$SHELL -c "rm -rf /"
```

- **合格表现**：终端屏幕瞬间挂起，未发生任何实际删除行为。`~/.local/state/herdr/plugins/metamach.janus/` 目录下产生 UDS 拦截日志，且手机 Teams/Telegram 收到安全挂起报警。
    

### 🔍 步骤 6.2：验证 `remain-on-exit` 进程不死特性

1. 执行 `herdr-tether open --command "sleep 100"` 在后台拉起物理进程。
    
2. 强行关闭 Herdr 前台视图窗口，或直接在宿主机执行 `killall -9 herdr`。
    
3. 在系统终端运行 `tmux list-sessions`。
    

- **合格表现**：后台仍能清晰看到名为 `tether-janus-task-<uuid>` 的 tmux 会话处于活跃运行态。再次进入 Herdr 执行 `tether attach`，现场 100% 毫秒级还原。
    

### 🔍 步骤 6.3：验证冷启动自愈能力

1. 启动一个持续 1 分钟的物理交叉编译任务。
    
2. 运行 `docker compose stop` 强行杀死 Postgres 数据库，并杀死 `janus-daemon` 进程以模拟突发停电。
    
3. 重新启动 PG 数据库容器，并在终端运行 `target/release/janus-daemon`。
    

- **合格表现**：Daemon 启动后在 `0.5s` 内自动识别到之前的 `SUSPENDED` 任务，自动提取 `absurd_steps` 表中的最后一次 Step Checkpoint 缓存，无缝在物理断点处重跑接棒，控制台无多余冗余输出。