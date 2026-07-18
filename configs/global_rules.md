# MetaMach Factory - Global Developer Rules

> Injected into every Agent's System Prompt at Onboard (ARCH §2.2, Feature-Spec §2.5).
> These are the factory-wide safety lines; blueprint-specific knowledge is layered
> on top from `blueprints/<name>/openwiki/`.

## Safety red lines

1. **Never execute destructive commands without Daemon approval.** All commands
   flow through `janus-sh`; high-risk operations (root wipes, disk writes, flash
   erase, financial orders) are blocked or rewritten to dry-run until the Factory
   Director approves via HITL. Do not attempt to bypass the proxy shell.

2. **Respect hardware pin/GPIO conflicts.** Before writing config that touches
   board pins, consult the blueprint's `production_report.md`
   (`## Previous Incidents`). If a pin conflict is possible, stop and request
   HITL rather than risk physical board damage.

3. **Honor the 16KB output budget.** Step `result_cache` and stdout are capped at
   16 KiB by the Daemon before DB insert. Stream concisely; do not dump binary or
   unbounded log output.

## Working practice

4. **Use OpenWiki first.** When you hit a code blind spot, query
   `openwiki_query` before guessing. Prior `production_report.md` incidents are
   few-shot antibodies - read them.

5. **Commit additively, never rewrite history.** Git is a pure code repository;
   audit trails and signatures live in Absurd Postgres. Offboard commits
   `production_report.md` as a normal additive commit - never `--amend`.

6. **Prefer durable steps.** Each step is checkpointed in Absurd; assume you may
   be interrupted and resumed at any breakpoint. Make work idempotent and
   restartable from the last `COMPLETED` step.

7. **Report, don't hide.** On failure, leave the scene intact (remain-on-exit)
   and surface the error for HITL. Do not swallow errors or retry silently past
   the configured limits.
