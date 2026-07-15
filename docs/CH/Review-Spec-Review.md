# Review-Spec Review — Audit Criteria Meta-Review

> **Document under review:** `docs/CH/Review-Spec.md`
> **Review lens:** Audit coverage completeness, pass/fail criteria rigor, testability of review items, safety of prescribed tests

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Review item is unsafe or fundamentally unverifiable |
| 🟠 **HIGH** | Missing critical audit domain |
| 🟡 **MEDIUM** | Ambiguous pass/fail criteria |
| ⚪ **LOW** | Formatting / naming |

---

## 1. 🔴 Safety-Critical Issues

### 1.1 REV-SEC-02 prescribes live `rm -rf /` on a real system

> "强制执行 `rm -rf /`，验证 `janus-daemon` UDS 套接字是否同步拦截并锁定挂起"

This is an **extremely dangerous** audit procedure:
- If anything goes wrong (janus-sh bug, misconfiguration, Daemon not running), the command reaches the real shell
- On macOS, `rm -rf /` is blocked by System Integrity Protection (SIP), but this creates a false sense of security — the test passes but not because of MetaMach
- On Linux without `--preserve-root`, this would brick the machine
- The review spec provides no isolation requirements (VM, container, dedicated test machine)

**Recommendation:** Replace with: `rm -rf /tmp/metamach-review-test-$(uuidgen)` — create a sentinel file, attempt to delete it via a blacklisted pattern, verify the file still exists. This is equally effective at testing the guard and carries zero risk of system destruction.

**Additionally:** Add a BOLD warning at the top of the review spec: "All security review items must be executed in an isolated container or VM, never on a production machine."

---

### 1.2 REV-DIS-01 `killall -9 tmux` destroys unrelated user sessions

> "强行 `killall -9 tmux` 模拟机房断电"

`killall -9 tmux` kills ALL tmux processes on the machine, including the user's own tmux sessions that have nothing to do with MetaMach. This is destructive to the reviewer's working environment.

**Recommendation:** Use `killall -9 janus-daemon` + targeted tmux session kill: `tmux kill-session -t tether-janus-task-<uuid>`. For database simulation: `docker compose stop` (already specified).

---

## 2. 🟠 Missing Audit Domains

### 2.1 No UDS protocol security audit

The review spec audits janus-sh interception (REV-SEC-02) but never audits the UDS channel itself:
- Who can connect to `janus.sock`? File permissions?
- Is the UDS communication authenticated?
- Can a malicious local process impersonate janus-sh and send fake commands?
- Can a malicious local process impersonate the Daemon and send fake ALLOW responses?

**Recommendation:** Add **REV-SEC-03: UDS Channel Integrity**:
1. Verify `janus.sock` file permissions are `0600` (owner-only)
2. Verify that `janus-daemon` validates the peer's PID/UID before processing requests
3. Attempt to connect to `janus.sock` as a different user — must be rejected

---

### 2.2 No secret cleanup audit (post-crash)

REV-SEC-01 audits `/dev/shm` permissions during normal operation. But there's no audit for:
- What happens to `/dev/shm/*.decrypted` files after a Daemon crash? (SIGKILL — no cleanup handler runs)
- What about after a system reboot? (`/dev/shm` is cleared on reboot, but this should be verified, not assumed)
- Are there any secret remnants in swap?

**Recommendation:** Add **REV-SEC-04: Crash-Secret Hygiene**:
1. Load secrets, then `killall -9 janus-daemon`
2. Verify `/dev/shm/metamach.janus/` contents are cleared (or document that they persist until reboot — and if so, add a cleanup cron job or systemd tmpfiles.d entry)

---

### 2.3 No network isolation audit

The architecture allows agents with `allow_network = true` (deployer level) to access the network. But there's no audit for:
- Can a Scout-level agent (supposedly no network) make HTTP requests via alternative methods? (e.g., `python3 -c "import urllib"`, `/dev/tcp` in bash)
- Is network egress filtered at the process level or just the shell command level?

**Recommendation:** Add **REV-SEC-05: Network Egress Control**:
1. Run a Scout-level agent and attempt `curl http://evil.com` → must be blocked
2. Attempt `python3 -c "import urllib.request; urllib.request.urlopen('http://evil.com')"` → must be blocked
3. Document: is network control at the command-whitelist level or OS-level (iptables/nftables)?

---

### 2.4 No performance/stress audit

The review spec focuses on correctness in failure scenarios but never audits performance under load:
- What happens when 5 blueprints run workflows simultaneously?
- What's the Daemon's memory footprint after 24 hours?
- What's the UDS latency under concurrent janus-sh requests?

**Recommendation:** Add **REV-STB-03: Load & Resource Audit**:
1. Run 5 concurrent dev-flow pipelines — all must complete without deadlock
2. Daemon memory usage must remain under 256MB after 24 hours of continuous operation
3. UDS round-trip latency (janus-sh → Daemon → response) must remain under 10ms at p99

---

## 3. 🟡 Unverifiable or Ambiguous Criteria

### 3.1 REV-EVO-01 "大 JSON 擦除率达 100%"

"验证数据库大 JSON 擦除率达 100%" — 100% is not practically verifiable. How do you prove there isn't a single orphaned JSON blob in a TOAST table somewhere?

**Recommendation:** Change to: "所有属于目标 blueprint 的 `absurd_steps.result_cache` 字段均为 NULL。通过 `SELECT count(*) FROM absurd_steps WHERE blueprint_id = <id> AND result_cache IS NOT NULL` 验证返回 0."

---

### 3.2 REV-EVO-01 "本地成功生成 production_report.md"

"本地成功生成" is a binary check but doesn't validate quality:
- Is the report empty?
- Does it contain the required sections (编译报错历史, 引脚冲突细节, Tool Guard 拦截日志, 成功通过的 Patch)?
- Is it valid Markdown?

**Recommendation:** Add quality gates:
1. Report file size > 500 bytes (not empty)
2. Report contains all four required sections (validated by heading search)
3. Report is valid Markdown (passes `markdownlint` or equivalent)

---

### 3.3 No inter-review dependency ordering

The review items are presented as a flat checklist, but some depend on others:
- REV-SEC-02 (janus-sh block) requires REV-STB-01 (16KB budget) to be running — the Daemon must be functional
- REV-DIS-01 (cold start) requires REV-SEC-01 (/dev/shm) setup to be complete
- REV-EVO-01 (offboard) requires at least one completed workflow

**Recommendation:** Add a "Prerequisites" column to the sign-off sheet, or order the review items as a dependency graph:

```
REV-SEC-01 → REV-STB-01 → REV-SEC-02 → REV-STB-02 → REV-DIS-01 → REV-EVO-01
```

---

## 4. ⚪ Sign-off Sheet Issues

### 4.1 Handwritten signatures in a digital document

"架构师：______________________ 日期：2026年07月15日" — The sign-off sheet implies physical paper signing. For a digital-native project, consider:
- GPG-signed review tags in Git
- A `REVIEW_SIGNED.md` file with committer identities
- GitHub Issue/PR-based approval workflow

### 4.2 "新任厂长" sign-off conflates two roles

The厂长 signs off on "生产业务与合闸审批核准" but the review items are deeply technical (checking UDS socket permissions, running SQL queries). A business user (厂长) cannot meaningfully verify these. 

**Recommendation:** Split the sign-off into:
- **Technical Sign-off** (架构师 + 安全审计员): REV-SEC-01 through REV-STB-02
- **Business Sign-off** (厂长): REV-EVO-01 (report quality), UAT workflow walkthrough

---

## Summary: Review-Spec Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Replace `rm -rf /` with safe destructive test |
| 2 | 🔴 | Replace `killall -9 tmux` with targeted session kill |
| 3 | 🟠 | Add UDS channel integrity audit (REV-SEC-03) |
| 4 | 🟠 | Add crash-secret hygiene audit (REV-SEC-04) |
| 5 | 🟠 | Add network egress control audit (REV-SEC-05) |
| 6 | 🟠 | Add performance/stress audit (REV-STB-03) |
| 7 | 🟡 | Replace "100%" claims with verifiable SQL queries |
| 8 | 🟡 | Add production_report.md quality gates |
| 9 | 🟡 | Add review dependency ordering |
| 10 | ⚪ | Split sign-off into technical and business roles |
