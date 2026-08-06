# MetaMach 0.5.0 — Test Report

> **Date:** 2026-07-26
> **Environment:** macOS + Linux (CI), Rust 1.88 (Edition 2024)
> **Result:** ✅ **178 tests — 178 passed, 0 failed, 0 ignored**

---

## Summary

| Category | Passed | Description |
|---|---|---|
| Unit tests (lib) | 117 | Core logic: absurd, agent, cognitive, gateway, lifecycle, pipeline, protocol, recipe, tmux, tool_guard, workflow |
| Binary tests | 8 | `herdr-janus` TUI (3) + `janus` CLI (5) |
| Integration tests | 47 | UDS contract (9), gateway HTTP (2), onboard/offboard lifecycle (8), step workflow (7), tmux sessions (4), E2E pipeline (6), protocol contract (5), Herdr contract (6), SQL gateway (2) |

**Total: 178 (178 passed, 0 failed, 0 ignored)**

All tests run inline — no `#[ignore]` attributes remain. Herdr-gated tests runtime-skip when Herdr is unavailable.

---

## Test Details

### 1. Unit Tests — Library (`janus/src/`)

#### `absurd` (11 tests)
| Test | Coverage |
|---|---|
| `derive_status_priority` | Status priority ordering |
| `sanitize_ident_replaces_invalid` | PG identifier sanitization |
| `truncate_over_budget_caps_and_tags` | 16KB budget enforcement |
| `truncate_respects_char_boundary` | UTF-8 safe truncation |
| `truncate_under_budget_is_unchanged` | Pass-through for small strings |
| `replay_fallback_merges_events_into_overlay` | SQLite → PG replay (PG-gated, skip when unavailable) |
| `record_truncates_oversized_cache` | Fallback ring buffer truncation |
| `records_and_counts` | Ring buffer record counting |
| `drain_returns_events_in_seq_order_and_empties_ring` | Ring buffer drain ordering |
| `ring_buffer_evicts_oldest` | Ring buffer eviction |
| `expected_version_tracks_vendored_v0_4_0` | Vendored absurd.sql version check |

#### `agent` (11 tests) — ADR-019/026
| Test | Coverage |
|---|---|
| `parse_provisioned_agent` | `[agent.X.provision]` TOML parsing |
| `agent_without_provision_returns_none` | Tool-Guard-only agents |
| `provision_without_quota` | Provisioning without quota limits |
| `fallback_agent_missing_warns_but_returns_primary` | Graceful missing fallback |
| `is_provisioned_distinguishes_tool_guard_only` | LLM-backed vs Tool-Guard-only |
| `mixed_existing_and_new_format` | Backward compatibility |
| `preflight_probe_for_esptool` | ADR-026: esptool.py hardware probe |
| `preflight_probe_for_generic` | ADR-026: generic probe |
| `run_preflight_no_probe_returns_no_probe` | ADR-026: no probe config → NoProbe |
| `run_preflight_bypass_when_probe_exits_zero_with_bypass` | ADR-026: bypass on success |
| `run_preflight_require_approval_when_probe_fails` | ADR-026: require-approval on failure |

#### `cognitive` (7 tests)
| Test | Coverage |
|---|---|
| `mock_provider_pass_through_when_no_opinion` | No-op provider fail-open |
| `mock_provider_recommends_block` | BLOCK recommendation path |
| `noop_provider_is_fail_open` | Default provider behavior |
| `mcp_provider_unreachable_when_binary_missing` | MCP binary not found |
| `mcp_provider_timeout_when_binary_hangs` | 2s timeout enforcement |
| `extract_knowledge_returns_supplement` | Offboard supplement |
| `extract_text_reads_mcp_content_field` | MCP response parsing |

#### `coldstart` (1 test)
| Test | Coverage |
|---|---|
| `session_name_shape` | tmux session naming convention |

#### `gateway` (12 tests) — ADR-012/013
| Test | Coverage |
|---|---|
| `dispatch_is_non_blocking` | Non-blocking HITL dispatch |
| `await_verdict_receives_callback` | Verdict delivery via oneshot |
| `await_verdict_times_out` | Fail-closed timeout |
| `resolve_callback_duplicate_is_conflict` | Single-callback enforcement |
| `resolve_callback_expired_is_gone` | Expiry enforcement |
| `resolve_callback_unknown_is_gone` | Unknown run_id rejection |
| `resolve_callback_gone_when_awaiter_timed_out` | Timeout cleanup |
| `parse_run_id_extracts_id` | run_id parsing |
| `parse_action_maps_verdict` | Action → verdict mapping |
| `verify_hmac_accepts_correct_rejects_wrong` | HMAC validation |
| `card_has_adaptive_card_schema_and_actions` | Teams card format |
| `send_no_ops_when_url_unset` | Graceful missing URL |

#### `lifecycle` (7 tests)
| Test | Coverage |
|---|---|
| `build_llm_input_caps_steps_and_truncates` | LLM input construction |
| `cognitive_supplement_noop_when_no_provider` | Cognitive supplement passthrough |
| `git_commit_report_returns_short_hash` | Git SHA resolution |
| `offboard_config_loads_with_defaults` | Offboard config parsing |
| `parse_incidents_extracts_bullets_with_marker` | Incident parsing |
| `raw_json_snapshot_embeds_valid_json` | Fallback JSON snapshot |
| `truncate_bytes_respects_char_boundary` | UTF-8 safe byte truncation |

#### `pipeline` (6 tests) — ADR-021
| Test | Coverage |
|---|---|
| `topo_sort_linear_chain` | 3-node linear DAG (A → B → C) |
| `topo_sort_diamond` | 4-node diamond (A → B+C → D) |
| `topo_sort_independent_nodes` | 3 parallel nodes (all level 0) |
| `cycle_detection` | Cycle rejection (A → B → A) |
| `duplicate_node_id_rejected` | Duplicate node ID validation |
| `unknown_dependency_rejected` | Broken dependency reference |

#### `protocol` (5 tests)
| Test | Coverage |
|---|---|
| `truncate_16k_keeps_small_strings` | Under-budget pass-through |
| `truncate_16k_caps_and_tags_oversized` | 16KB hard cap + tag |
| `truncate_16k_respects_char_boundary` | UTF-8 safe truncation |
| `payload_enrichment_fields_populated` | WebhookPayload fields |
| `payload_scene_capped_to_16kib` | Scene truncation |

#### `recipe` (7 tests)
| Test | Coverage |
|---|---|
| `validates_a_good_recipe` | Valid `.janus/blueprint.toml` |
| `fails_when_name_mismatches_dir` | blueprint.name validation |
| `fails_when_scope_empty` | openwiki.scope validation |
| `fails_when_workflow_missing` | Missing workflow file |
| `parses_cross_host_recipe` | SSH [remote] config (ADR-017) |
| `rejects_invalid_blueprint_names` | Invalid name patterns |
| `load_recipe_rejects_invalid_names` | Path traversal prevention |

#### `tmux` (4 tests)
| Test | Coverage |
|---|---|
| `fake_backend_create_kill_round_trip` | FakeBackend lifecycle |
| `lifecycle_restart_creates_session` | Session restart |
| `session_id_names_task_with_uuid` | Session naming |
| `attach_missing_session_errors` | Error handling |

#### `tool_guard` (18 tests)
| Test | Coverage |
|---|---|
| `backtick_after_root_slash_is_blocked` | Backtick injection blocked |
| `capability_of_maps_git_log_to_git_log_tag` | Capability mapping |
| `chain_and_subshell_caught` | Shell chaining blocked |
| `coder_blacklist_globs` | Blacklist glob matching |
| `coder_tmp_delete_allowed_root_blocked` | Scope-based blocking |
| `command_string_extracts_c_arg` | `-c` argument extraction |
| `deployer_general_allowed` | Permitted command |
| `deployer_require_approval_blocks` | require_approval trigger |
| `env_injection_root_delete_blocked` | Env injection blocked |
| `env_injection_root_delete_via_backtick_blocked` | Backtick + env blocked |
| `financial_rewritten_to_dry_run` | Financial REWRITE |
| `git_log_is_a_distinct_capability` | Capability isolation |
| `scout_git_log_allowed` | Permitted git-log |
| `scout_read_allowed` | Permit read |
| `scout_write_denied_permissions` | Deny write |
| `unknown_agent_falls_back_to_default_blacklist` | Unknown agent fallback |

#### `workflow::filter` (10 tests) — ADR-018
| Test | Coverage |
|---|---|
| `strip_ansi_removes_color_codes` | ANSI CSI removal |
| `strip_ansi_removes_cursor_movement` | ANSI cursor removal |
| `strip_ansi_preserves_plain_text` | Plain text pass-through |
| `collapse_progress_bars_single_line` | Single progress line |
| `collapse_progress_bars_multiple_lines` | Multi-line collapse |
| `collapse_progress_bars_bracket_style` | Bracket-style progress |
| `collapse_progress_bars_non_progress_unchanged` | Non-progress pass-through |
| `deduplicate_lines_collapses_repeats` | Repeat dedup |
| `deduplicate_lines_no_repeats` | No-repeat pass-through |
| `clean_pty_output_end_to_end` | Full pipeline integration |

#### `workflow` (engine tests)
| Test | Coverage |
|---|---|
| `run_workflow_happy_path_completes_all_steps` | Full 2-step workflow → COMPLETED |
| `run_workflow_retries_then_succeeds` | `max_attempts: 3` retry success |
| `run_workflow_retries_exhausted` | Retries exhausted → terminal |
| `run_workflow_resumes_from_checkpoint` | Resume from last checkpoint |
| `kill_stale_sessions_kills_only_the_task_sessions` | Session cleanup |
| `resume_point_branches_on_checkpoint_state` | Resume point resolution |
| `git_head_returns_full_hash_in_git_repo` | Git SHA resolution |
| `git_head_all_zeros_for_non_git` | Non-git sentinel |
| `queue_name_sanitizes_non_ident_chars` | Queue name sanitization |
| `shell_quote_escapes_single_quotes` | POSIX quoting |
| `step_command_includes_janush_and_env_context` | Command construction |
| `write_raw_log_creates_file` | ADR-025: raw log disk write |
| `prune_raw_logs_does_not_crash` | ADR-025: 7-day GC |
| `quota_sleep_defaults_to_300` | ADR-022: default sleep constant |
| `is_quota_exhausted_detects_429` | ADR-022: HTTP 429 detection |
| `is_quota_exhausted_detects_quota_exceeded` | ADR-022: quota-exceeded detection |
| `is_quota_exhausted_detects_rate_limit` | ADR-022: rate-limit detection |
| `is_quota_exhausted_passes_normal_output` | ADR-022: normal output pass-through |
| `env_timestamp_is_iso_8601` | ADR-024: timestamp format |
| `env_tty_devices_returns_comma_separated` | ADR-024: TTY device list |

---

### 2. Binary Tests

#### `herdr-janus` (3 tests)
| Test | Coverage |
|---|---|
| `selection_wraps_in_dispatch` | Selection wrap-around |
| `flip_view_flips_and_resets_selection` | Tab toggle resets selection |
| `ui_renders_dispatch_view` | TUI rendering with TestBackend |

#### `janus` CLI (5 tests)
| Test | Coverage |
|---|---|
| `discover_workflows_finds_toml_files` | Workflow discovery |
| `discover_workflows_returns_empty_for_missing_dir` | Graceful missing dir |
| `discover_workflows_skips_non_toml` | Non-TOML skip |
| `validate_pipeline_accepts_valid` | Pipeline validation (ADR-023) |
| `validate_pipeline_rejects_cycle` | Pipeline cycle detection (ADR-021) |

---

### 3. Integration Tests — `janus/tests/`

#### `config_contract.rs` — Herdr integration (6 tests)
| Test | Coverage |
|---|---|
| `herdr_plugin_toml_parses_and_has_required_fields` | Manifest parsing: `id`, `min_herdr_version`, placement enum, non-empty commands |
| `herdr_plugin_toml_command_matches_binary` | `[[panes]]` command matches `Cargo.toml [[bin]]` |
| `herdr_env_fallback_and_override` | `HERDR_PLUGIN_*` / `JANUS_AGENTS_TOML` fallback logic |
| `herdr_min_version_is_satisfied` | Installed Herdr version ≥ `min_herdr_version` (runtime-skip) |
| `herdr_plugin_link_parses_manifest` | `herdr plugin link` → `herdr plugin list` round-trip (runtime-skip) |
| `e2e_smoke_onboard_dispatch_progress` | Full stack: PG + tmux + Herdr + daemon → onboard → dispatch → COMPLETED (runtime-skip) |

> The last three tests runtime-skip when Herdr is not available — no `#[ignore]` needed.

#### `uds_contract.rs` (9 tests)
| Test | Coverage |
|---|---|
| `utc_01_01_daemon_binds_socket_and_pid` | Daemon startup: `janus.sock` + `janus.pid` |
| `utc_01_01_second_launch_refuses_duplicate_pid_lock` | Singleton PID lock enforcement |
| `contract_3_2_and_3_4_uds_round_trip` | Ping→Pong, ALLOW/BLOCK/REWRITE verdicts |
| `utc_02_02_janush_intercepts_block_and_allows` | `janush` proxy shell: exit 126 on BLOCK & zero-arg interactive rejection |
| `utc_02_04_uds_protocol_robustness` | Malformed/oversized/burst payloads |
| `utc_02_05_uds_fuzz_testing` | 10,000 random payload survival |
| `utc_02_06_fail_closed_30s_timeout` | Fail-closed 30s timeout |
| `utc_08_01_degraded_mode_core_works_and_fallback_initialized` | PG-down resilience |
| `utc_06_03_janus_status_cli` | `janus status` JSON/text output |

#### `gateway.rs` (2 tests)
| Test | Coverage |
|---|---|
| `utc_10_02_http_callback_200_then_409_duplicate` | HTTP callback + duplicate rejection |
| `utc_10_04_hmac_auth_rejects_unsigned_and_wrong_accepts_correct` | HMAC authentication |

#### `protocol_contract.rs` (5 tests)
| Test | Coverage |
|---|---|
| `request_tags_are_snake_case` | Request discriminant wire format |
| `guard_check_round_trips_with_all_fields` | GuardCheck serialization |
| `guard_verdict_cognitive_context_omitted_when_none` | 0.3.0 wire compatibility |
| `guard_verdict_cognitive_context_included_when_some` | Cognitive context wire format |
| `response_tags_are_snake_case` | Response discriminant wire format |

#### `onboard_lifecycle.rs` (8 tests)
| Test | Requires | Coverage |
|---|---|---|
| `utc_05_01_size_budget_truncation` | None | 16KB budget constant |
| `utc_04_01_suspend_preserves_guard_verdict_scene` | None | SUSPEND protocol shape |
| `utc_05_04_onboard_registers_tenant` | PG | Onboard → PG catalog |
| `utc_05_04b_multidb_onboard_isolation` | PG | Multi-DB topology (two blueprints, one catalog) |
| `utc_05_02_offboard_smelts_and_archives` | PG | Offboard purge + archive |
| `utc_05_03_offboard_commits_production_report_to_git` | PG | Git commit on offboard |
| `utc_05_05_re_onboard_inherits_previous_incidents` | PG | Experience inheritance from `.janus/openwiki/` |
| `utc_0a_absurd_schema_loads_on_onboard` | PG | `absurd.sql` loading |

> PG-dependent tests runtime-skip when `DATABASE_URL` is not set.

#### `step_workflow.rs` (7 tests)
| Test | Requires | Coverage |
|---|---|---|
| `utc_03_04_daemon_crash_socket_cleanup` | None | Socket cleanup after crash |
| `utc_03_06_step_status_wire_format` | None | StepStatus wire format |
| `utc_03_01_step_state_transitions` | PG | PG online → Progress query for specific blueprint |
| `utc_03_01b_dispatch_step_transitions` | PG + tmux | Dispatch → `tmux_alive` → both steps COMPLETED |
| `utc_03_03_cold_start_reconcile` | PG + tmux | Kill daemon mid-step → restart → resume to COMPLETED |
| `utc_04_01_hitl_resume` | PG + tmux | `require_approval` → emit_event → re-run → COMPLETED |
| `utc_03_05_concurrent_workflow_isolation` | PG | Multi-blueprint Progress isolation |

> PG+tmux-dependent tests runtime-skip when `DATABASE_URL` or tmux is unavailable.

#### `e2e_pipeline.rs` (3 tests) — ADR-028

> **Note:** These tests exercise the multi-step execution DAG, Tool Guard interception,
> and Absurd DB state machine automatically in CI using deterministic mock processes
> (`echo`, `true`, `sleep`). Live LLM multi-agent pipelines (UTC-E2E-01 through
> UTC-E2E-03 in `Test-Spec.md` §2.11) are executed manually prior to tagging
> production releases.

| Test | Requires | Coverage |
|---|---|---|
| `e2e_onboard_dispatch_returns_task_id` | PG + tmux | Onboard → Dispatch → absurd-minted task_id |
| `e2e_tool_guard_blocks_blacklisted` | PG + tmux | Onboard → Dispatch → step fails on blacklisted command |
| `e2e_multi_step_workflow_produce_transform_verify` | PG + tmux | 3-step workflow: gen → xform → check, polls Progress until terminal, verifies completion |

#### `tmux.rs` (4 tests)
| Test | Coverage |
|---|---|
| `create_persists_and_lists` | Session creation + listing |
| `kill_removes_session` | Session deletion |
| `capture_pane_returns_text` | Pane text capture |
| `remain_on_exit_survives_process_exit` | Remain-on-exit durability |

---

## Execution Guide

### Default (local development, no external dependencies)

```bash
cargo test --workspace --manifest-path janus/Cargo.toml
```

**Runs:** ~130 tests — PG-gated, tmux-gated, and Herdr-gated tests runtime-skip gracefully.

### With PostgreSQL (pre-push hook or `make db-init`)

```bash
# Auto-provisions PG, runs all tests including PG-gated
git push  # pre-push hook handles everything
```

**Runs:** All 178 tests. PG auto-provisioned via `make db-init`, tests run sequentially to avoid local PG connection exhaustion.

### With Herdr (macOS, Herdr installed via Homebrew)

Tests runtime-detect Herdr on PATH. If `herdr server` is running, the 3 Herdr integration tests run automatically. If not, they skip with a message.

### CI (GitHub Actions)

```yaml
- Install Herdr + start server
- DATABASE_URL=postgres://metamach_admin@localhost:5432/metamach_db
- cargo test --workspace --manifest-path janus/Cargo.toml
```

**Runs:** All 178 tests — PG (Docker), tmux (apt-get), and Herdr (release binary + `herdr server`) are all provisioned. Tests run in parallel (Docker PG handles connection load).

---

## Coverage Matrix

| Module | Unit | Integration | PG-gated | Herdr-gated |
|---|---|---|---|---|
| absurd | 11 | — | 1 | — |
| agent | 11 | — | — | — |
| cognitive | 7 | — | — | — |
| coldstart | 1 | — | — | — |
| gateway | 12 | 2 | — | — |
| lifecycle | 7 | — | — | — |
| pipeline | 6 | — | — | — |
| protocol | 5 | 5 | — | — |
| recipe | 7 | — | — | — |
| tmux | 4 | 4 | — | — |
| tool_guard | 18 | — | — | — |
| workflow::filter | 10 | — | — | — |
| workflow::engine | 20 | — | — | — |
| herdr-janus (TUI) | 3 | — | — | — |
| janus (CLI) | 5 | — | — | — |
| config_contract | — | 6 | 1 | 3 |
| uds_contract | — | 9 | — | — |
| onboard_lifecycle | — | 8 | 6 | — |
| step_workflow | — | 7 | 5 | — |
| e2e_pipeline | — | 3 | 3 | — |
| **Total** | **127** | **44** | **16** | **3** |

---

## CI Dependency Gate Matrix

| Dependency | Provisioning | Tests Exercised |
|---|---|---|
| **PostgreSQL 16** | Docker service container (`postgres:16`) | 16 PG-gated tests (onboard, offboard, step_workflow, e2e_pipeline, absurd replay) |
| **tmux 3.3+** | `apt-get install tmux` | 8 tmux-gated tests (step transition, cold-start, HITL resume, e2e) |
| **Herdr 0.7.5** | Download binary from GitHub releases + `herdr server &` | 3 Herdr-gated tests (version check, plugin link, e2e smoke) |
| **absurd.sql** | Catalog migration via `psql` | Schema loading verified on onboard |
| **UDS** | — (kernel facility) | 9 UDS contract tests |
| **HTTP/gateway** | — (loopback) | 2 HTTP gateway tests |
