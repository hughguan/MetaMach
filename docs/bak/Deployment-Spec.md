
### ── 动静隔离架构、物理沙箱挂载与统一数据库并网指南

> **EN:** Deployment Spec — immutable/mutable isolation, sandbox key mounting, and Absurd Postgres bring-up.

本部署规范书（Deploy Spec）旨在指导系统管理员或厂长在本地物理算力节点（如 Richmond Hill 车间服务器）上安全、幂等、无缝地完成 **MetaMach 0.3.0** 生产底座的并网通电。

本规范严格遵循 Herdr v1 插件的“动静隔离（Immutable ROOT vs. Mutable State）”规范与安全性红线，对物理目录、内存盘挂载、数据库容器及一键引导流程进行系统级定义。

## 1. 部署环境与物理依赖 (Prerequisites)

在开始部署前，必须确保宿主机满足以下物理与软件依赖：

|**依赖组件**|**最低版本要求**|**物理用途**|**验证指令**|
|---|---|---|---|
|**操作系统**|Linux / macOS|提供标准的 POSIX 兼容环境与 UDS 支持|`uname -a`|
|**Rust 工具链**|Rust 1.88+ (Edition 2024)|编译 `janus-daemon`、`herdr-janus` 与 `janus-sh`|`rustc --version`|
|**Tmux**|Tmux 3.3+|Tether 维持 PTY 会话长生不老的物理载体|`tmux -V`|
|**Docker & Compose**|Docker v24.0+ / Compose v2.20+|一键拉起并托管 Absurd Postgres 数据库|`docker compose version`|
|**SOPS & Age**|SOPS v3.8+ / Age v1.1+|（可选）仅在启用加密金融蓝图密钥时必需；非金融项目可省略|`sops --version`|


> 💡 **单向无状态部署实践建议 (Non-Normative Note for Remote Targets)**
> 
> 在远程物理靶机处于严苛单向内网隔离、且无法部署 Git 凭证或直接反向连接 Absurd PG 数据库的测试场景下，推荐采用 **单向无状态 Diff 管道（Tether Patch Pipeline）** 方案：
> 
> 1. 本地 `janus-daemon` 检索到跨端 Step 时，通过 `git archive` 以派发时固化的 `target_sha`（Contract 3.1）生成完整源码树快照，确保远程收到的是自包含的完整工作树，而非增量 patch。
> 2. 通过 Tether SSH 管道将归档单向投影至远程主机 `/tmp/sandbox`：
>    `git archive HEAD | ssh -i /dev/shm/ssh_key user@remote "mkdir -p /tmp/sandbox && tar xf - -C /tmp/sandbox"
> 3. 远程仅执行编译测试，完毕后将不超过 16KB 的结构化 `result.json` 经 SSH stdout 回传本地落库。
> 
> 该机制完全保证远程靶机在 Git、DB 级别的无状态，安全边界 100% 收拢于本地。此方案为**推荐实践**，非强制性 Spec 契约；替代方案（NFS 共享卷、容器挂载卷、预同步源码树）同等有效，只要远程靶机保持无状态。
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

One PG, Multi-DB 拓扑。宿主机运行单一原生 Postgres 15+ 实例（无 Docker），由 `janus-daemon` 直接管理。每个蓝图在 Onboard 时通过 `CREATE DATABASE metamach_blueprint_<name>` 获得独立逻辑数据库。数据持久化于 `~/.metamach/db/`。

### 3.1 原生 Postgres 引导

`janus-daemon` 在首次启动时处理 PG 生命周期：

1. **初始化集群：** 若 `~/.metamach/db/` 为空，执行 `initdb -D ~/.metamach/db/` 创建全新 PG 集群。
2. **启动服务：** 执行 `pg_ctl -D ~/.metamach/db/ -l ~/.metamach/db/pg.log start`，`listen_addresses=''`（仅 Unix Socket，无 TCP）。
3. **创建管理员角色：** `CREATE ROLE metamach_admin WITH LOGIN PASSWORD '<random>'`（密码持久化至 `~/.metamach/db/.pgpass`，chmod 600）。
4. **运行迁移：** 按顺序执行 `janus/migrations/` 中所有 `.sql` 文件。
5. **蓝图上线：** 每次 `janus onboard` 时执行 `CREATE DATABASE metamach_blueprint_<name>`。

> **连接字符串：** `postgresql://metamach_admin:<password>@/metamach_db?host=~/.metamach/db` — 仅 Unix Socket，无 TCP 暴露。

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

MetaMach 0.3.0 提供高度简化的"一键通电并网"指令。厂长只需在根目录下执行 `make bootstrap`，系统即会自动完成环境校验、代码编译、目录建立、符号链接挂载及原生 PG 初始化。

### 5.1 自动化部署总闸：`Makefile`

```makefile
.PHONY: all bootstrap compile symlinks db-up db-down db-backup db-restore db-migrate health logs uninstall clean

# 1. 环境变量
HERDR_PLUGIN_STATE_DIR ?= ~/.local/state/herdr/plugins/metamach.janus
METAMACH_DB_DIR ?= ~/.metamach/db
export METAMACH_DB_PASSWORD ?= $(shell [ -f $(METAMACH_DB_DIR)/.pgpass ] && cat $(METAMACH_DB_DIR)/.pgpass || openssl rand -hex 16)

all: bootstrap

# 2. 一键通电原语
bootstrap: symlinks compile db-up
	@echo "================================================================="
	@echo "🪐 MetaMach 0.3.0 并网通电成功！"
	@echo "🔌 在 Herdr 内按下 prefix+j 打开调度控制台。"
	@echo "================================================================="

# 3. 建立动静隔离物理目录与符号链接
symlinks:
	@echo "📁 创建可变状态与配置目录..."
	@mkdir -p ~/.config/herdr/plugins/metamach.janus
	@mkdir -p ~/.local/state/herdr/plugins/metamach.janus
	@mkdir -p $(METAMACH_DB_DIR)
	@printf '%s' "$(METAMACH_DB_PASSWORD)" > $(METAMACH_DB_DIR)/.pgpass && chmod 600 $(METAMACH_DB_DIR)/.pgpass
	@echo "🔑 数据库密码已持久化至 $(METAMACH_DB_DIR)/.pgpass (chmod 600)。"
	@echo "🔗 链接 agents 配置到 Herdr 配置目录..."
	@ln -sf $$(pwd)/configs/agents.toml ~/.config/herdr/plugins/metamach.janus/agents.toml

# 4. 本地编译 Janus 核心二进制组件
compile:
	@echo "🦀 编译 Janus Daemon、Client 与 janus-sh 代理..."
	@cd janus && cargo build --release --locked
	@echo "🛡️ 安装二进制至 ${HERDR_PLUGIN_ROOT}/bin/..."
	@mkdir -p ${HERDR_PLUGIN_ROOT}/bin
	@cp janus/target/release/janus-daemon ${HERDR_PLUGIN_ROOT}/bin/janus-daemon
	@cp janus/target/release/herdr-janus ${HERDR_PLUGIN_ROOT}/bin/herdr-janus
	@cp janus/target/release/janus-sh ${HERDR_PLUGIN_ROOT}/bin/janus-sh

# 5. 初始化原生 Postgres（无 Docker）
db-up:
	@echo "🐘 初始化原生 Postgres 至 $(METAMACH_DB_DIR)..."
	@if [ ! -f $(METAMACH_DB_DIR)/PG_VERSION ]; then \
		echo "  → 执行 initdb..."; \
		initdb -D $(METAMACH_DB_DIR) -U metamach_admin --auth-local=trust; \
	fi
	@echo "  → 启动 PG 服务（仅 Unix Socket，无 TCP）..."
	@pg_ctl -D $(METAMACH_DB_DIR) -l $(METAMACH_DB_DIR)/pg.log start 2>/dev/null || true
	@echo "  → 设置管理员密码..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d postgres -c "ALTER ROLE metamach_admin WITH PASSWORD '$(METAMACH_DB_PASSWORD)';" 2>/dev/null || true
	@echo "  → 创建 metamach_db..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d postgres -c "CREATE DATABASE metamach_db;" 2>/dev/null || true
	@echo "  → 运行迁移..."
	@for f in janus/migrations/*.sql; do psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f $$f; done
	@echo "⚡ 原生 Postgres 已就绪于 $(METAMACH_DB_DIR)。"

# 6. 安全关闭数据库
db-down:
	@echo "🔌 停止 Postgres..."
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || true

# 7. 数据库备份（pg_dump 至时间戳 SQL 文件）
db-backup:
	@echo "💾 备份 metamach_db..."
	@pg_dump -h $(METAMACH_DB_DIR) -U metamach_admin metamach_db > metamach_backup_$$(date +%Y%m%d_%H%M%S).sql
	@echo "✅ 备份完成。"

# 8. 数据库恢复（需 BACKUP_FILE 变量）
db-restore:
	@if [ -z "$(BACKUP_FILE)" ]; then echo "❌ 用法: make db-restore BACKUP_FILE=backup.sql"; exit 1; fi
	@echo "🔄 恢复 metamach_db 从 $(BACKUP_FILE)..."
	@psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db < $(BACKUP_FILE)
	@echo "✅ 恢复完成。"

# 9. 运行待处理迁移
db-migrate:
	@echo "🔄 运行待处理迁移..."
	@for f in janus/migrations/*.sql; do psql -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db -f $$f; done
	@echo "✅ 迁移完成。"

# 10. 健康检查
health:
	@echo "=== MetaMach 健康检查 ==="
	@pg_isready -h $(METAMACH_DB_DIR) -U metamach_admin -d metamach_db || echo "❌ DB 离线"
	@test -S $(HERDR_PLUGIN_STATE_DIR)/janus.sock && echo "✅ Daemon Socket 存活" || echo "❌ Daemon Socket 缺失"

# 11. 日志查看
logs:
	@tail -n 200 $(HERDR_PLUGIN_STATE_DIR)/janus.log 2>/dev/null || echo "(无 janus.log；Daemon 默认输出至 stderr)"

# 12. 完全卸载
uninstall:
	@echo "⚠️  此操作将删除所有 MetaMach 数据。继续？[y/N]" && read -r REPLY && [ "$$REPLY" = "y" ]
	@pg_ctl -D $(METAMACH_DB_DIR) stop 2>/dev/null || true
	@rm -rf $(METAMACH_DB_DIR)
	@rm -rf ~/.config/herdr/plugins/metamach.janus
	@rm -rf ~/.local/state/herdr/plugins/metamach.janus
	@echo "🗑️  MetaMach 已完全卸载。"

# 13. 清理本地编译缓存
clean:
	@echo "🧹 清理 cargo 工作区..."
	@cd janus && cargo clean
	@if [ -d /dev/shm/metamach.janus ]; then \
		echo "⚠️  清除 RAM 盘密钥于 /dev/shm/metamach.janus..."; \
		rm -rf /dev/shm/metamach.janus; \
	fi
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
export SHELL=${HERDR_PLUGIN_ROOT}/bin/janus-sh
$SHELL -c "rm -rf $SENTINEL_DIR"
test -f "$SENTINEL_DIR/sentinel" && echo "✅ 哨兵存活，命令已被拦截"
```

- **合格表现**：终端屏幕瞬间挂起，未发生任何实际删除行为，且哨兵文件事后仍然存在（证明命令被拦截、未触达真实 Shell）。`~/.local/state/herdr/plugins/metamach.janus/` 目录下产生 UDS 拦截日志，且手机 Teams/Telegram 收到安全挂起报警。
    

### 🔍 步骤 6.2：验证 `remain-on-exit` 进程不死特性

1. 执行 `janus tether open --command "sleep 100"` 通过内置 Tether 模块在后台拉起物理进程。
    
2. 强行关闭 Herdr 前台视图窗口，或直接在宿主机执行 `killall -9 herdr`。
    
3. 在系统终端运行 `tmux list-sessions`。
    

- **合格表现**：后台仍能清晰看到名为 `tether-janus-task-<uuid>` 的 tmux 会话处于活跃运行态。再次进入 Herdr 执行 `janus tether attach`，现场 100% 毫秒级还原。
    

### 🔍 步骤 6.3：验证冷启动自愈能力

1. 启动一个持续 1 分钟的物理交叉编译任务。
    
2. 运行 `pg_ctl -D ~/.metamach/db/ stop` 强行停止 Postgres，并杀死 `janus-daemon` 进程以模拟突发停电。
    
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
    psql -h ~/.metamach/db/ -U metamach_admin -d metamach_db \
        -c "SELECT name, status, default_workflow FROM blueprints;"
    # 无 TUI 环境下巡检车间全局
    janus status
    ```

- **合格表现**：`blueprints` 表出现一行 `gatemetric` / `ACTIVE` 记录；`janus status` 输出当前在途任务（此时应为空，但命令本身返回成功，证明 `progress` 原语与 Daemon 连接正常）；在 Herdr 内 `prefix+j` 唤醒 Popup，派单菜单中已可见 `gatemetric` 并可立即派发。重复执行 `janus onboard` 不产生重复行（幂等）。