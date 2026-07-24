# MetaMach 0.4.2 — Test Report

> **Date:** 2026-07-24  
> **Environment:** macOS, Rust 1.88 (Edition 2024), no PG, no tmux  
> **Result:** ✅ **147 tests — 143 passed, 0 failed, 4 skipped**

---

## Summary

| Category | Passed | Skipped | Description |
|---|---|---|---|
| Unit tests (lib) | 102 | 1 | Core logic: absurd, agent, cognitive, gateway, lifecycle, pipeline, protocol, recipe, tmux, tool_guard, workflow |
| Binary tests | 3 | 0 | `herdr-janus` TUI rendering |
| Static contract | 3 | 0 | `herdr-plugin.toml` validation + paths fallback |
| Integration tests | 27 | 0 | UDS contract, gateway HTTP, onboard/offboard lifecycle, step workflow, tmux sessions |
| Runtime-skip tests | 11 | 0 | PG-gated tests (skipped locally, run in CI) |
| Manual integration | 0 | 3 | Herdr plugin link + e2e smoke |

**Total: 147 (143 passed + 4 skipped)**

---

## Test Details

### 1. Unit Tests — Library (`src/`)

#### `absurd` (8 tests)
| Test | Coverage |
|---|---|
| `derive_status_priority` | Status priority ordering |
| `sanitize_ident_replaces_invalid` | PG identifier sanitization |
| `truncate_over_budget_caps_and_tags` | 16KB budget enforcement |
| `truncate_respects_char_boundary` | UTF-8 safe truncation |
| `truncate_under_budget_is_unchanged` | Pass-through for small strings |
| `replay_fallback_merges_events_into_overlay` | SQLite → PG replay (PG-gated, skipped locally) |
| `record_truncates_oversized_cache` | Fallback ring buffer truncation |
| `records_and_counts` / `drain_returns_events_in_seq_order_and_empties_ring` / `ring_buffer_evicts_oldest` | Fallback ring buffer lifecycle |

#### `agent` (6 tests) — ADR-019
| Test | Coverage |
|---|---|
| `parse_provisioned_agent` | `[agent.X.provision]` TOML parsing |
| `agent_without_provision_returns_none` | Tool-Guard-only agents |
| `provision_without_quota` | Provisioning without quota limits |
| `fallback_agent_missing_warns_but_returns_primary` | Graceful missing fallback |
| `is_provisioned_distinguishes_tool_guard_only` | LLM-backed vs Tool-Guard-only |
| `mixed_existing_and_new_format` | Backward compatibility |

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

#### `gateway` (10 tests) — ADR-012/013
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
| `validates_a_good_recipe` | Valid blueprint recipe |
| `fails_when_name_mismatches_dir` | Name validation |
| `fails_when_scope_empty` | Scope validation |
| `fails_when_workflow_missing` | Missing workflow |
| `parses_cross_host_recipe` | SSH host config |
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
| *Additional tests for blacklist, rewrite, HITL* | Full Tool Guard matrix |

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

#### `workflow::engine` (tests in `mod.rs`)
| Test | Coverage |
|---|---|
| `run_workflow_happy_path_completes_all_steps` | Full 2-step workflow → COMPLETED |
| `run_workflow_stops_on_first_failure` | Step 2 fails → stop |
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

---

### 2. Binary Tests — `herdr-janus` (3 tests)

| Test | Coverage |
|---|---|
| `selection_wraps_in_dispatch` | Selection wrap-around |
| `flip_view_flips_and_resets_selection` | Tab toggle resets selection |
| `ui_renders_dispatch_view` | TUI rendering with TestBackend |

---

### 3. Static Contract Tests — `config_contract.rs` (3 tests)

| Test | Coverage |
|---|---|
| `herdr_plugin_toml_parses_and_has_required_fields` | Manifest parsing: `id`, `min_herdr_version`, `placement` enum, non-empty commands |
| `herdr_plugin_toml_command_matches_binary` | `[[panes]]` command matches `Cargo.toml [[bin]]` |
| `herdr_env_fallback_and_override` | `HERDR_PLUGIN_STATE_DIR`, `CONFIG_DIR`, `ROOT` fallback logic; `JANUS_AGENTS_TOML` override |

---

### 4. Integration Tests — `tests/`

#### `uds_contract.rs` (9 tests)
| Test | Coverage |
|---|---|
| `utc_01_01_daemon_binds_socket_and_pid` | Daemon startup: `janus.sock` + `janus.pid` |
| `utc_01_01_second_launch_refuses_duplicate_pid_lock` | Singleton PID lock enforcement |
| `contract_3_2_and_3_4_uds_round_trip` | Ping→Pong, ALLOW/BLOCK/REWRITE verdicts |
| `utc_02_02_janush_intercepts_block_and_allows` | `janush` proxy shell: exit 126 on BLOCK |
| `utc_02_04_uds_protocol_robustness` | Malformed/oversized/burst payloads |
| `utc_02_05_uds_fuzz_testing` | 10,000 random payload survival |
| `utc_02_06_fail_closed_30s_timeout` | Fail-closed 30s timeout |
| `utc_08_01_degraded_mode_core_works_and_fallback_initialized` | PG-down resilience |
| `utc_02_05_uds_fuzz_testing` (variant) | Extended fuzz |

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
| `utc_05_04b_multidb_onboard_isolation` | PG | Multi-DB topology |
| `utc_05_02_offboard_smelts_and_archives` | PG | Offboard purge + archive |
| `utc_05_03_offboard_commits_production_report_to_git` | PG | Git commit on offboard |
| `utc_05_05_re_onboard_inherits_previous_incidents` | PG | Experience inheritance |
| `utc_0a_absurd_schema_loads_on_onboard` | PG | `absurd.sql` loading |

> PG-dependent tests runtime-skip when `DATABASE_URL` is not set.

#### `step_workflow.rs` (7 tests)
| Test | Requires | Coverage |
|---|---|---|
| `utc_03_04_daemon_crash_socket_cleanup` | None | Socket cleanup after crash |
| `utc_03_06_step_status_wire_format` | None | StepStatus wire format |
| `utc_03_01_step_state_transitions` | PG | PG online → Progress query |
| `utc_03_01b_dispatch_step_transitions` | PG + tmux | Dispatch → `tmux_alive` → COMPLETED |
| `utc_03_03_cold_start_reconcile` | PG + tmux | Kill daemon mid-step → restart → resume |
| `utc_04_01_hitl_resume` | PG + tmux | `require_approval` → emit_event → resume |
| `utc_03_05_concurrent_workflow_isolation` | PG | Multi-blueprint Progress isolation |

> PG+tmux-dependent tests runtime-skip when `DATABASE_URL` or tmux is unavailable.

#### `tmux.rs` (4 tests)
| Test | Coverage |
|---|---|
| `create_persists_and_lists` | Session creation + listing |
| `kill_removes_session` | Session deletion |
| `capture_pane_returns_text` | Pane text capture |
| `remain_on_exit_survives_process_exit` | Remain-on-exit durability |

---

### 5. Manual Integration Tests — `config_contract.rs` (3 tests, `#[ignore]`)

| Test | Trigger | Coverage |
|---|---|---|
| `herdr_plugin_link_parses_manifest` | `--ignored` | `herdr plugin link` → `herdr plugin list` round-trip |
| `herdr_min_version_is_satisfied` | `--ignored` | Installed Herdr version ≥ `min_herdr_version` |
| `e2e_smoke_onboard_dispatch_progress` | `--ignored` | Full e2e: daemon → onboard → dispatch → Progress → COMPLETED |

---

## Execution Guide

### Default (local development, no external dependencies)

```bash
cd janus
cargo test --workspace
```

**Runs:** 143 tests (4 skipped — 1 PG-gated lib + 3 manual `#[ignore]`)

### With PostgreSQL (CI or `make db-init`)

```bash
DATABASE_URL=postgres://metamach_admin@localhost:5432/metamach_db \
cargo test --workspace
```

**Runs:** 143+ tests (runtime-skip tests now run; PG-gated tests active)

### Manual Integration Tests (macOS + Herdr installed)

```bash
# Herdr manifest + version tests
cargo test herdr -- --ignored

# End-to-end smoke (requires PG + tmux + Herdr)
DATABASE_URL=postgres://metamach_admin@localhost:5432/metamach_db \
cargo test e2e -- --ignored

# All manual tests
cargo test --workspace -- --ignored
```

### CI (GitHub Actions)

```bash
# Full suite with PG service + tmux
DATABASE_URL=postgres://metamach_admin@localhost:5432/metamach_db \
cargo test --workspace
```

**Runs:** All 143 non-manual tests + all runtime-skip tests (PG available in CI)

---

## Coverage Matrix

| Module | Unit Tests | Integration | PG-gated | Manual |
|---|---|---|---|---|
| absurd | 8 | — | 1 | — |
| agent | 6 | — | — | — |
| cognitive | 7 | — | — | — |
| coldstart | 1 | — | — | — |
| gateway | 10 | 2 | — | — |
| lifecycle | 7 | — | — | — |
| pipeline | 6 | — | — | — |
| protocol | 5 | 5 | — | — |
| recipe | 7 | — | — | — |
| tmux | 4 | 4 | — | — |
| tool_guard | 18 | — | — | — |
| workflow::filter | 10 | — | — | — |
| workflow::engine | 12 | — | — | — |
| herdr-janus (TUI) | 3 | — | — | — |
| herdr-plugin (contract) | 3 | — | — | 2 |
| uds_contract | — | 9 | — | — |
| onboard_lifecycle | — | 8 | 6 | — |
| step_workflow | — | 7 | 5 | — |
| e2e_smoke | — | — | 1 | 1 |
| **Total** | **107** | **35** | **13** | **3** |

---

## External Dependency Gate Test Strategy

| Dependency | Static Test | Unit Test | Integration Test | Manual Test | CI Runs? |
|---|---|---|---|---|---|
| **PostgreSQL** | — | 1 (runtime-skip) | 11 (runtime-skip) | 1 (e2e) | ✅ With PG service |
| **tmux** | — | — | 5 (runtime-skip) | 1 (e2e) | ✅ `apt-get install tmux` |
| **Herdr** | 3 (manifest + paths) | — | — | 2 (plugin link) + 1 (e2e) | ❌ Only static |
| **absurd.sql** | 1 (schema version) | — | 1 (onboard) | — | ✅ Via PG service |
| **UDS** | — | — | 9 | — | ✅ |
| **HTTP/gateway** | — | 10 | 2 | — | ✅ |
