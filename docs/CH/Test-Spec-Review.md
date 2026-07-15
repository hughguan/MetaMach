# Test-Spec Review — QA & Test Coverage Deep-Dive

> **Document under review:** `docs/CH/Test-Spec.md`
> **Review lens:** Test coverage completeness, test case design quality, safety of test procedures, automation feasibility, edge case coverage

---

## Severity Legend

| Tag | Meaning |
|-----|---------|
| 🔴 **BLOCKER** | Unsafe test; critical untested scenario |
| 🟠 **HIGH** | Significant coverage gap; test design flaw |
| 🟡 **MEDIUM** | Missing test case; ambiguous expected behavior |
| ⚪ **LOW** | Naming / formatting |

---

## 1. 🔴 Critical Safety & Coverage Issues

### 1.1 UTC-02-02 prescribes `rm -rf /` as a test input

> "在 Agent 窗格中强行执行未在白名单中的命令，例如：`rm -rf /` 或系统级 `esptool.py erase_flash`"

As noted in Design-Review.md and Review-Spec-Review.md: `rm -rf /` is catastrophically dangerous as a test input. Test environments experience configuration drift, bugs, and human error. One misconfigured test run can destroy the test machine.

**Recommendation:** Replace with: `rm -rf /tmp/metamach-test-guard-$(uuidgen)` — create a unique temp directory with a sentinel file, attempt to delete it, verify sentinel still exists. Update all test cases that reference `rm -rf /`.

---

### 1.2 UTC-03-03 test procedure doesn't match Feature-Spec cold start behavior

> "重启 PG 容器，执行 `make bootstrap` 冷启动自愈线程"

Feature-Spec §2.3 says cold start recovery happens when Daemon starts (not `make bootstrap`). `make bootstrap` does compilation + symlinks + docker. Running `make bootstrap` for cold start recovery would recompile the entire project — that's not a cold start test, it's a full rebuild test.

**Recommendation:** The test should be:
1. Kill `postgres` and `janus-daemon`
2. Restart PG: `docker compose up -d`
3. Restart Daemon: `janus-daemon` (or via herdr-janus lazy-start)
4. Verify Daemon reads last COMPLETED checkpoint and resumes

Do NOT run `make bootstrap` for this test — it obscures the actual cold-start code path.

---

### 1.3 UTC-03-02 "拔掉网线" is not automatable

> "手动断开本地网络（或临时拔掉网线/关闭 VPN）"

This requires physical intervention and cannot be part of an automated test suite. For CI/CD integration, a programmatic network disruption is needed.

**Recommendation:** Use `iptables` / `pfctl` rules to drop packets to the remote host:
```bash
# Linux
iptables -A OUTPUT -d <remote_host> -j DROP
# macOS
pfctl -e -f <(echo "block drop out to <remote_host>")
```
Or use `tc qdisc` for simulated latency/loss without full disconnection.

---

## 2. 🟠 Coverage Gaps

### 2.1 Missing test: Daemon crash during active Step

All crash tests focus on infrastructure failure (PG down, network down, power loss). There's no test for:
- Daemon process crashes (segfault, OOM kill) while a Step is RUNNING
- Does herdr-janus detect the dead Daemon and lazy-restart it?
- Does the restarted Daemon recognize the orphaned Step and handle it correctly?
- Does the tmux pane survive the Daemon crash?

**Recommendation:** Add **UTC-03-04: Daemon Crash Recovery**
- Precondition: Step is RUNNING, Daemon is the parent of the tmux session
- Input: `killall -9 janus-daemon`
- Expected: tmux session survives (remain-on-exit), herdr-janus lazy-restarts Daemon, Daemon scans for orphaned steps, transitions orphan to SUSPENDED, notifies厂长

---

### 2.2 Missing test: Concurrent workflow dispatch

No test covers concurrent execution:
- What happens when 2 workflows are dispatched simultaneously?
- Do they get separate tmux sessions? Separate database transactions?
- Are there race conditions in the PID lock or UDS socket?
- Does Tool Guard correctly attribute commands to the right task?

**Recommendation:** Add **UTC-03-05: Concurrent Workflow Isolation**
- Precondition: 2 blueprints, both ACTIVE
- Input: Dispatch dev-flow for both simultaneously
- Expected: 2 separate tmux sessions created, 2 independent task records in absurd_tasks, UDS requests correctly tagged with task_id, no cross-contamination of result_cache

---

### 2.3 Missing test: Malformed UDS payload

UTC-02-02 tests a valid but unauthorized command. There's no test for:
- Malformed JSON sent to UDS (missing fields, invalid UTF-8, oversized payload)
- UDS message from an unexpected source (wrong PID/UID)
- Rapid-fire UDS messages (DoS simulation)

**Recommendation:** Add **UTC-02-04: UDS Protocol Robustness**
- Input 1: Send invalid JSON to `janus.sock` → Daemon logs warning, returns error response, does not crash
- Input 2: Send 1000 requests in 1 second → Daemon rate-limits, does not OOM
- Input 3: Send 64KB payload → Daemon rejects (message too large)

---

### 2.4 Missing test: Edge cases in Tool Guard

The Tool Guard tests only cover two scenarios:
- UTC-02-02: Block unauthorized command
- UTC-02-03: Dry-run rewrite for financial commands

Missing:
- **Allowlist pass-through**: A Scout runs `ls -la` → must be ALLOWED and execute normally
- **Partial match**: `rm -rf /tmp/something` vs. blacklisted `rm -rf /` — the tool guard must distinguish
- **Command chaining**: `rm -rf / && echo "done"` — is only the dangerous part blocked or the whole chain?
- **Subshell escape**: `bash -c "rm -rf /"` — does the guard see the inner command?
- **Environment variable injection**: `RM_TARGET=/ && rm -rf $RM_TARGET` — does the guard expand variables?

**Recommendation:** Add **Test Suite 2.2b: Tool Guard Edge Cases** with the above scenarios.

---

### 2.5 Missing test: `production_report.md` on re-Onboard

UTC-05-03 tests that the report is committed and pushed, and that a new agent "获得避坑抗体". But the test doesn't actually verify the antibody mechanism:
- What exactly is in the agent's system prompt after re-onboard?
- Is the报告 injected as system prompt text? As RAG context? As few-shot examples?
- How do we verify the agent "免疫同类引脚冲突错误"?

**Recommendation:** Make the test verifiable:
1. Offboard → production_report.md contains specific string: "PIN_CONFLICT_MARKER_21"
2. Re-onboard the same blueprint
3. Inspect the agent's system prompt (via Daemon debug endpoint) → must contain "PIN_CONFLICT_MARKER_21"
4. Run a task that previously triggered the conflict → agent must not attempt the conflicting pin configuration

---

## 3. 🟡 Test Design Quality

### 3.1 No negative test for Daemon PID lock

UTC-01-01 tests that a second Daemon instance exits cleanly. But it doesn't test:
- What if the first Daemon crashed (stale PID)? Second instance should detect staleness and start
- What if `janus.pid` is corrupted (not a valid PID)? Should be handled gracefully

**Recommendation:** Add edge cases to UTC-01-01:
- UTC-01-01a: Start Daemon, kill -9 it (stale PID), start again → should detect stale PID and start
- UTC-01-01b: Write "not_a_pid" to janus.pid → Daemon should log warning, overwrite, and start

---

### 3.2 UTC-01-03 Popup keyboard test is too shallow

> "弹窗支持高亮切换，焦点不逃逸至后台 tiled pane"

"焦点不逃逸" is subjective and not programmatically verifiable. What constitutes "escape"?

**Recommendation:** Make test verifiable:
1. Open Popup with 3 blueprint options
2. Press `Down` 10 times → highlight must wrap to top, not escape popup
3. Press `Tab` 5 times → focus must cycle within popup
4. At no point should any keystroke cause a character to appear in the background terminal

---

### 3.3 UTC-04-02 tests only Teams, not Telegram

> "在手机 Teams 端阅读报错明细并点击 `[🔄 Resume]`"

PRD and Feature-Spec mention both Teams and Telegram. The test suite must cover both, or explicitly declare Teams as primary and Telegram as a follow-up.

**Recommendation:** Add **UTC-04-03: Telegram Notification & Resume** with equivalent steps for the Telegram bot flow. Or add a note: "Telegram flow is architecturally identical; test one channel per release, alternating."

---

### 3.4 No performance SLA tests

The test suite only covers functional correctness. Missing:
- Daemon startup time (from cold)
- UDS round-trip latency (p50, p99)
- Popup render time (from `prefix+j` to interactive)
- Database query latency for Step history

**Recommendation:** Add **Test Suite 6: Performance Benchmarks** with baseline SLAs:
- Daemon cold start: < 2s
- UDS command check round-trip: < 50ms
- Popup render (warm Daemon): < 100ms
- Step history query (1000 steps): < 500ms

---

## 4. 🟡 Test Environment Issues

### 4.1 ngrok/Cloudflare Tunnel is a fragile external dependency

> "ngrok / Cloudflare Tunnel：用于映射本地 janus-daemon 端口，实现 Teams / Telegram 外网 Webhook 回调接收测试"

Relying on a third-party tunnel service for testing means:
- Tests break when ngrok changes pricing/API
- Tests require internet access (no air-gapped testing)
- Tests expose local services to the public internet during test runs

**Recommendation:** Add a local webhook receiver option: run a simple HTTP server on localhost that logs received webhooks. Tests can POST to localhost instead of going through Teams/TG. The actual Teams/TG integration is tested in a separate manual UAT phase, not in automated CI.

---

### 4.2 No containerized test environment

All tests assume a bare-metal or VM environment. For CI/CD, the entire test suite should run in a Docker Compose environment:
- `metamach-test` container with Rust, tmux, and the compiled binaries
- `metamach-db-test` container with Postgres
- Test orchestration via `docker compose -f docker-compose.test.yml up --abort-on-container-exit`

**Recommendation:** Add a `docker-compose.test.yml` and a `make test-integration` target.

---

## 5. ⚪ Minor Issues

- UTC numbering scheme: UTC-01-01, UTC-02-01, etc. is clear. Consider adding the milestone it belongs to: `UTC-M1-01`.
- Test case format uses `<br><br>  <br><br>` for line breaks in table cells — this is fragile. Consider using numbered lists or separate rows for multi-step procedures.
- "严重级别" uses Blocker/Critical/Major but doesn't define what these mean in terms of release gates. Add definitions: Blocker = cannot ship, Critical = must fix before release, Major = should fix, can ship with known issue.

---

## Summary: Test-Spec Action Items

| # | Sev | Item |
|---|-----|------|
| 1 | 🔴 | Replace `rm -rf /` in UTC-02-02 with safe equivalent |
| 2 | 🔴 | Fix UTC-03-03: use `janus-daemon` restart, not `make bootstrap` |
| 3 | 🟠 | Replace "拔掉网线" with programmatic network disruption |
| 4 | 🟠 | Add UTC-03-04: Daemon crash during active Step |
| 5 | 🟠 | Add UTC-03-05: concurrent workflow isolation |
| 6 | 🟠 | Add UTC-02-04: malformed UDS payload handling |
| 7 | 🟠 | Add Tool Guard edge case suite (allowlist, partial match, chaining, subshell) |
| 8 | 🟠 | Make UTC-05-03 knowledge inheritance verifiable |
| 9 | 🟡 | Add PID lock staleness edge cases |
| 10 | 🟡 | Make UTC-01-03 keyboard focus test verifiable |
| 11 | 🟡 | Add Telegram notification test (UTC-04-03) |
| 12 | 🟡 | Add performance benchmark suite |
| 13 | 🟡 | Replace ngrok dependency with local webhook receiver |
| 14 | 🟡 | Add containerized test environment (docker-compose.test.yml) |
| 15 | ⚪ | Define severity level release gate meanings |

> **Resolution Log (2026-07-15):**
> - **#1 🔴 (rm -rf / in UTC-02-02)** ✅ RESOLVED - UTC-02-02 now creates a `/tmp/metamach-test-guard-$(uuidgen)` sentinel dir + file, attempts a blacklisted `rm -rf` against it, and verifies the sentinel survives the block.
> - **#2 🔴 (UTC-03-03 uses make bootstrap)** ✅ RESOLVED - UTC-03-03 now restarts PG via `docker compose up -d` and the Daemon directly via `janus-daemon` (no `make bootstrap` recompile); expected output already correctly reads the last `COMPLETED` checkpoint.
>
> **Round 3 (🟠 items, 2026-07-15):**
> - **#3 🟠 (programmatic network disruption)** ✅ RESOLVED - UTC-03-02 now uses `iptables`/`pfctl` drop rules (no physical 拔网线).
> - **#4 🟠 (Daemon crash during active Step)** ✅ RESOLVED - added UTC-03-04.
> - **#5 🟠 (concurrent workflow isolation)** ✅ RESOLVED - added UTC-03-05.
> - **#6 🟠 (UDS protocol robustness)** ✅ RESOLVED - added UTC-02-04 (malformed JSON, DoS, oversized).
> - **#7 🟠 (Tool Guard edge cases)** ✅ RESOLVED - added Suite 2.2b (UTC-02-05..09: allowlist, partial match, chaining, subshell, env-var expansion).
> - **Bonus:** UTC-04-02 aligned with redesigned HITL resume (no `Ctrl+C`).
