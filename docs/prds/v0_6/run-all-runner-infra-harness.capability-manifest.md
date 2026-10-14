# Capability manifest — run_all runner and infra-test harness (S19)

Twin of `run-all-runner-infra-harness.capability-manifest.yaml`; PRD `docs/prds/v0_6/run-all-runner-infra-harness.md`.
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12); record measurements dated `a030ee7528`. Cite-by-symbol; re-locate lines at implementation time.

Verdicts:

- `PASS` — the substrate binding held at authoring.
- `OPEN` — the binding is this leaf's own deliverable or awaits a ruling; it is not a green G3 binding.

No binding is `FAIL`.

Pre-delivery polarity was checked at `8e6ffcac9d`: every mechanical `delivered_check` evaluates NOT-delivered. `commit_planning` passed its polarity gate on 2026-10-12.

Task ids were stamped by `commit_planning` on 2026-10-12:

- μ₂ #8719 is HELD (`x_seam_wait` S20 L1 #8624) and was hand-stamped.
- ε lives in dark-factory's registry.

## α (#8678) — discovery from classification_discovered_set only; adoption ratchet member; README partition rule

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| lib-discovery-function-exists | grep:tests/infra/run-all-classification-lib.sh classification_discovered_set — defined, glob test_*.sh, skips non-files, excludes test_helpers.sh; called by run_all.sh pool branch only (re-read at 8e6ffcac9d). | PASS | — |
| inline-discovery-loops-removed | at 8e6ffcac9d run_all.sh carries three inline loops (count-only probe, member-subset branch, legacy branch) each with the test_helpers.sh skip; the legacy one is `for test_file in "$INFRA_DIR"/test_*.sh`. Delivered when no for-loop over "$INFRA_DIR"/test_*.sh remains. Accepted gap: a loop over a differently-named dir variable escapes this grep; the adoption-ratchet row inline-discovery-loop (comment-stripped) is the behavioural-lint cover. | PASS | grep `for [A-Za-z_]+ in "\$INFRA_DIR"/test_\*\.sh` absent in `tests/infra/run_all.sh` |
| pool-disable-knob-exists-for-legacy-pin | grep:tests/infra/run_all.sh REIFY_RUN_ALL_POOL_DISABLE — forces the legacy serial path, so a fixture test can compare legacy vs pool discovery. | PASS | — |
| adoption-ratchet-member | deliverable of this leaf (new stdlib-unittest member + .sh wrapper + manifest row, D9); absent at 8e6ffcac9d. | OPEN | grep `inline-discovery-loop` present in `tests/infra/adoption-ratchet.manifest` |

## β (#8681) — one member runner and one result recorder across all four run_all.sh branches

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| six-spawn-five-fail-sites-today | grep at 8e6ffcac9d: reify_git_env_scrub bash\|env spawn sites in H9, subset, pool phase 1/2/2.5, legacy = 6; non-comment `RESULT: FAIL` echoes = 5. Premise of the ratchet ceilings (1 and 1). | PASS | grep `^_ra_run_member\(\) \{` present in `tests/infra/run_all.sh` |
| recorder-exists | deliverable; D1 _ra_record_result. | OPEN | grep `^_ra_record_result\(\) \{` present in `tests/infra/run_all.sh` |
| framing-consumed-by-dark-factory | dark-factory orchestrator/verify_classify.py::_redact_passed_suite_blocks keys on `--- Running: <name> ---` / `  RESULT: (PASS\|FAIL\|SKIP) (<name>)`; merge_lane/shadow.py::_RUN_ALL_FAILED_MARKER_RE parses `FAILED <names>`. C1 keeps both byte-stable; B6 is the reify-side cover. | PASS | manual: byte-stability across four branches is a behavioural property asserted by B6, not a grep. |

## γ (#8685) — REIFY_VERIFY_EVENT_LOG verify-tree JSON-lines channel (run_all records, slot_timeout record, member isolation, stdlib reader)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| channel-absent-today | git grep REIFY_VERIFY_EVENT_LOG at 8e6ffcac9d matches only plans/ (no producer, no consumer) in reify and nothing in dark-factory. | PASS | grep `REIFY_VERIFY_EVENT_LOG` present in `tests/infra/run_all.sh` |
| slot-timeout-emitter-single-site | grep:scripts/lib_slot_acquire.sh slot_emit_timeout — the one printf of @@REIFY_SLOT_TIMEOUT@@, called from slot_acquire's finite-WAIT deadline only; every wrapper (lib_test_semaphore, cargo-test-occt-gated, lib_lane_x_flock) and run_all's pool worker reach it. Every caller in the verify tree writes the record (lead ruling R-1). | PASS | grep `event_log_emit_slot_timeout` present in `scripts/lib_slot_acquire.sh` |
| member-unset-precedent | grep:tests/infra/run_all.sh `env -u REIFY_RUN_ALL_MEMBER_SUBSET` — existing per-member un-injection idiom D4 extends; un-injection is not an injection, so run-all-ambient-vars.manifest needs no row (run_all_injected_env_keys diffs injected KEYS). | PASS | grep `-u REIFY_VERIFY_EVENT_LOG` present in `tests/infra/run_all.sh` |
| stdlib-reader | deliverable; tests/infra/verify_events.py (python3 -I, stdlib json) + test_verify_events.py + wrapper + manifest row. | OPEN | grep `import json` present in `tests/infra/verify_events.py` |
| encoder-lib-registered-as-verify-pipeline | scripts/lib_event_log.sh is sourced transitively (verify.sh → lib_test_semaphore.sh → lib_slot_acquire.sh → lib_event_log.sh), so verify-pipeline-guard.sh auto-derivation misses it — same class as the existing lib_slot_acquire.sh row in scripts/verify-pipeline-paths.txt. | PASS | grep `^scripts/lib_event_log\.sh$` present in `scripts/verify-pipeline-paths.txt` |

## δ (#8689) — run_all.sh h14 partition into output, flaky-ledger and content-skip libs (args, not globals)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| partition-record | plans/review-all-reify-20261010.json h14_measurements[tests/infra/run_all.sh] — 5 groups, stays_whole false; symptoms name the globals (_ra_fail_detail, _RA_FAIL_DETAIL_MAX, _RA_INBOUND_ROLE, _RA_RUN_ID, _h2_discovered_list, failures) that must move or become arguments. | PASS | grep `^_ra_emit_sanitized\(\)` present in `tests/infra/run_all_output_lib.sh` |

## ε (dark_factory:6856) — classifier, shadow FAILED parse and passed-suite redaction read the run_all event channel
_Note:_ Filed in dark-factory's registry (dark_factory:6856); reify η (8694), Ω (8729) and gate 8612 depend on it via metadata.external_deps.

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| classifier-is-text-only-today | dark-factory orchestrator/verify_classify.py::classify_failure(tool, rc, output, timed_out) — text only; _SLOT_TIMEOUT_SENTINEL_RE + three-basename allowlist; _has_fatal_slot_timeout_sentinel reads disposition=. | PASS | manual: foreign-registry producer; its own verify asserts D1-D5. |

## ζ₁ (#8692) — test_run_all.sh T19/T25 wait on the event seam, not run_all workdir internals

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| private-workdir-coupling-today | test_run_all.sh T25 globs reify-run-all-pool.* under a private TMPDIR and reads *.rc skipping *.retry.rc (re-read at 8e6ffcac9d, Test 19/25 headers present). | PASS | grep `reify-run-all-pool\.` absent in `tests/infra/test_run_all.sh` |

## η (#8694) — delete test_slot_timeout_marker.sh Sections D-G

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| analyzer-sections-today | test_slot_timeout_marker.sh section banners at 8e6ffcac9d — D, E, F, G present (3,413 lines at a030ee7528); D_ROSTER, D_MEMBERS, G_MEMBERS arrays present. | PASS | grep `^D_ROSTER=\(` absent in `tests/infra/test_slot_timeout_marker.sh` |
| private-harness-ratchet-to-zero | premise correction — the file already sources test_helpers.sh at 8e6ffcac9d; its only PASS=0 is the E4_RAW_EMITTER heredoc (Section E positive control), deleted with Section E. | PASS | grep `E4_RAW_EMITTER` absent in `tests/infra/test_slot_timeout_marker.sh` |
| soundness-depends-on-dark-factory-rule-4 | producer:ε (external) — C3 rule 4 (channel present ⇒ text arms not consulted for SEMAPHORE_TIMEOUT). Without it, Section D's own preamble reason (cross-repo regex coupling) still holds. | OPEN | manual: cross-repo precondition; verified by ε's D1 test, not a reify grep. |

## ζ₂ (#8696) — test_run_all.sh h14 partition (6) with shared preamble lib

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| partition-record | h14_measurements[tests/infra/test_run_all.sh] — 6 groups; LOAD_TOLERANCE_LIB_T9 and the REIFY_RUN_ALL_FLAKY_LEDGER isolation export go to a shared preamble. Hostile ambient loop in test_run_all_ambient_isolation.sh drives the real test_run_all.sh (run_all_ambient_isolation_lib.sh header) — coverage must follow the split. | PASS | grep `LOAD_TOLERANCE_LIB` present in `tests/infra/run_all_test_preamble_lib.sh` |

## θ (#8700) — harness-layout guard lib; test_harness_kloc_cap.sh h14 partition (4); contract to docs

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| rules-live-in-test-today | test_harness_kloc_cap.sh defines harness_layout_violations … harness_warn_ratchet_violations (re-read at 8e6ffcac9d; only comment-cite edits since a030ee7528). scripts/check-harness-baseline-registration.sh sources only harness-layout-lib.sh. | PASS | grep `^harness_layout_violations\(\)` present in `tests/infra/harness-layout-guard-lib.sh` |
| diff-gate-sources-guard-lib | deliverable. | OPEN | grep `harness-layout-guard-lib\.sh` present in `scripts/check-harness-baseline-registration.sh` |
| contract-doc-home | deliverable; docs/contributor/ exists. | OPEN | grep `harness-layout-contract` present in `tests/infra/test_harness_kloc_cap.sh` |

## ι (#8703) — wallclock_rust_guard_lib.sh; test_no_new_wallclock_rust_deadlines.sh h14 partition (3)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| engine-reads-no-file-state | h14 record — the 12 engine functions read no top-level state (roots and baseline are arguments); downward imports only. | PASS | grep `^_detect_rust_wallclock_deadline\(\)` present in `tests/infra/wallclock_rust_guard_lib.sh` |

## κ (#8707) — cpu-gov ROW predicates single home in cpu_gov_instrument.py; test_cpu_load_governance.sh h14 partition (5)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| dead-python-predicates-live-bash-copy | cpu_gov_instrument.py defines slowdown_within_bound and psi_below_band (no caller outside _selftest); test_cpu_load_governance.sh defines _row3_within_bound used by the live ROW3 verdict (re-read at 8e6ffcac9d). | PASS | grep `^_row3_within_bound\(\)` absent in `tests/infra/test_cpu_load_governance.sh` |
| instrument-unittest | deliverable; new stdlib unittest + wrapper + manifest row. | OPEN | grep `unittest` present in `tests/infra/test_cpu_gov_instrument.py` |

## λ (#8712) — PTODO hard-gate fixture helper with one tasks-schema source

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| schema-copies-today | CREATE TABLE tasks ×5 in test_reify_audit_ptodo.sh, ×1 in test_lane_task_status.sh; canonical crates/reify-audit/tests/common/schema.rs::TASKS_DB_SCHEMA raw string (8e6ffcac9d). | PASS | grep `CREATE TABLE tasks` absent in `tests/infra/test_reify_audit_ptodo.sh`, `tests/infra/test_lane_task_status.sh` |
| rust-reads-shared-sql | deliverable (D12). | OPEN | grep `include_str!\(.*tasks_db_schema\.sql` present in `crates/reify-audit/tests/common/schema.rs` |
| every-sentinel-consumed | today only HARDGATE_C/D have a consumer (test_reify_audit_ptodo_orphan_hardgate.sh). | PASS | grep `HARDGATE_G_PASSED` present in `tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh` |

## μ (#8716) — test_helpers.sh end-of-suite finalizers; seed-warm-lane shared base fixture and lock holders

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| no-finalizer-today | test_helpers.sh test_summary prints Results and exits; no hook; the warm-lane facility deliberately installs no EXIT trap (bash traps do not stack). | PASS | grep `^test_register_finalizer\(\)` present in `tests/infra/test_helpers.sh` |
| base-constructor | deliverable. | OPEN | grep `^_make_base\(\)` present in `tests/infra/test_seed_warm_lane.sh` |
| shared-handshake-primitive-exists | tests/infra/slot_holder_handshake_lib.sh holder_wait_for_marker — already adopted by test_warm_lane_gc.sh and test_warm_lane_pool.sh. | PASS | grep `^_wait_for_reader_lock\(\)` absent in `tests/infra/test_seed_warm_lane.sh` |

## μ₂ (#8719) — test_seed_warm_lane.sh h14 partition (9)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| partition-record | h14_measurements[tests/infra/test_seed_warm_lane.sh] — 9 groups; EPOCH_2020 into harness; R4 moves with Q, R6a with I. | PASS | grep `EPOCH_2020` present in `tests/infra/seed_warm_lane_harness_lib.sh` |
| s20-lane-lock-surface | producer:S20 lane-lock leaf (lib_lane_lock.sh lane_lock_path / lane_lock_acquire) — same wave, real dependency edge at decompose. | OPEN | manual: cross-PRD producer id unknown until S20 decomposes. |

## ν (#8721) — test_warm_lane_gc.sh make_gc_fixture and _seed_stub_body adoption

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| helper-exists-inline-copies-remain | _seed_stub_body defined in test_warm_lane_gc.sh; 12 of 20 STUB_EOF heredocs are identical inline copies (record measurement, reproduced by skeptic). | PASS | grep `^make_gc_fixture\(\)` present in `tests/infra/test_warm_lane_gc.sh` |

## ξ (#8723) — test_warm_lane_pool.sh h14 partition (4)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| partition-record | h14_measurements[tests/infra/test_warm_lane_pool.sh] — harness lib, PG → test_refresh_warm_base.sh (exists), FC stub farm, e2e core. | PASS | grep `^detect_substrate\(\)` present in `tests/infra/warm_lane_pool_harness_lib.sh` |

## ο (#8725) — test_tree_sitter_pipeline.sh onto test_helpers.sh; h14 partition (5)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| private-harness-and-substring-discovery-today | test_tree_sitter_pipeline.sh defines PASS_COUNT/FAIL_COUNT and discovers cases via declare -F \| awk '/test_/'; run_all.sh _ra_collect_fail_detail ERE matches neither 'ASSERTION FAILED' nor 'RUN … FAIL'. | PASS | grep `declare -F \\| awk` absent in `tests/infra/test_tree_sitter_pipeline.sh` |
| plan-capture-test-home-exists | tests/infra/test_plan_capture_lib.sh exists (S18 owns plan_capture_lib.sh; this leaf only moves a test into its existing test file). | PASS | grep `retries_on_truncation` present in `tests/infra/test_plan_capture_lib.sh` |

## π (#8726) — delete test_test_helpers.sh source-shape and prose pins; h14 partition (3); test_run_all.sh Test 6

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| consumer-loop-today | test_test_helpers.sh lines ~622-1372 hold 89 check() calls over CONSUMERS + sync_comments_test.sh + sync_ref_helpers.sh (record; partition symbols include the CONSUMERS loop). | PASS | grep `^CONSUMERS=` absent in `tests/infra/test_test_helpers.sh` |
| sync-ref-own-test | deliverable; tests/infra/sync_ref_helpers.sh exists, no own test today. | OPEN | grep `sync_ref_helpers\.sh` present in `tests/infra/test_sync_ref_helpers.sh` |

## ρ (#8727) — jobserver unit oracle to stdlib unittest; setup-dev checks moved

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| importlib-heredocs-today | test_jobserver_balancer.sh — 13 spec_from_file_location heredocs, 4 mod._transfer_burst calls; scripts/jobserver-balancer.py defines _transfer_burst and decide. | PASS | grep `spec_from_file_location` absent in `tests/infra/test_jobserver_balancer.sh` |

## σ (#8728) — test_with_jcodemunch_serve.sh h14 partition (3)

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| lib-already-sourced | test_with_jcodemunch_serve.sh already sources tests/infra/jcodemunch_pin_guard_lib.sh (h14 record). | PASS | grep `^require_nonempty\(\)` present in `tests/infra/jcodemunch_pin_guard_lib.sh` |

## Ω (#8729) — PRD close — terminal stamp and freeze header

| capability | binding | verdict | delivered_check |
|---|---|---|---|
| terminal-stamp | deliverable; overlay "PRD terminal status" three-value vocabulary. | OPEN | grep `Status:\*\* SHIPPED` present in `docs/prds/v0_6/run-all-runner-infra-harness.md` |
