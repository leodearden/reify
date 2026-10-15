# run_all runner and infra-test harness — one member runner, a structured event channel, guard engines in libs

**Status:** DECOMPOSED (review-all-reify-20261010 program, stream S19; pass-1 open questions ruled by the lead 2026-10-12, see §9). Leaf task ids are backfilled into §8; μ₂ #8719 is HELD on S20 L1 #8624. gap-fill 2026-10-16: θ #8700 shrunk; new leaves υ #9303, φ #9309; D2/D8/D11 amended (§Gap-fill amendments).
**Program:** `plans/review-all-program-reify-20261012.md` → stream **S19** (agent, L/medium, wave 1). Ruling: §Resolved decisions → "S19 run_all runner and infra-test harness" (human, 2026-10-12). This PRD does not relitigate it.
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12); the review record's measurements are dated `a030ee7528` (2026-10-10). Main moves fast — cite-by-symbol; re-locate lines at implementation time. Every `tests/infra/` path this PRD touches changed only in comments between the two SHAs (`git diff a030ee7528..8e6ffcac9d -- tests/infra/run_all.sh tests/infra/test_harness_kloc_cap.sh tests/infra/test_seed_warm_lane.sh`: task 8537 cite rewrites and fc307b4d8d, comments only).
**Finding keys owned (17):** fk-acca603a9628, fk-9c87c8ca99b5, fk-1417bf006761, fk-53a9afc0d2b9, fk-34a184f51e6f, fk-35b618e9b24d, fk-4229ece8d26d, fk-3cb220f9cc94, fk-a86e30ad9ecb, fk-9c99c0223cb3, fk-2e56a7e3fcb7, fk-1c2e90be3491, fk-6213e3dfdbec, fk-75e28dab2f08, fk-6e371c1b8c6f, fk-37507523c365, fk-c796ce463c73. The record is `plans/review-all-reify-20261010.json` (statements, measurements, skeptic notes).

## 1. Goal

The next change to the infra test runner, or to any infra test that drives it, is made by an agent. Today that agent has to:

- find four execution branches in `tests/infra/run_all.sh` (6 member-spawn sites, 5 `RESULT: FAIL` recorders) and decide for each one whether a cross-cutting concern applies to it;
- keep four copies of the discovery predicate in agreement;
- keep a 3,413-line static leak analyzer (`test_slot_timeout_marker.sh` Sections D–G) and its index-aligned roster tables in step with every new suite that drives `run_all.sh`. The analyzer exists only because dark-factory classifies a slot timeout by grepping the verify log for a column-0 string;
- read 2–3k-line test files whose guard engines, fixtures and live scans are interleaved, with no lib for the engine.

This PRD makes each of those a single place, and makes classification depend on structured data instead of log text:

1. **One member runner.** `_ra_run_member` handles spawn, git-env scrub, env isolation, FD hygiene, capture, sanitize, rc and fail detail. One recorder, `_ra_record_result`, handles the `RESULT:` line, the counters and the events. Every branch becomes a *member-list producer plus a scheduling policy* that calls those two functions (fk-acca603a9628).
2. **One discovery predicate.** Every branch and the count-only probe consume `classification_discovered_set`. The H9 bucket, the subset and the H3 exclusion become filters over its output (fk-9c87c8ca99b5).
3. **A structured event channel.** `REIFY_VERIFY_EVENT_LOG` is a JSON-lines file. `run_all.sh` writes per-member start/finish records and run start/finish records into it, and the slot-timeout deadline writes a `slot_timeout` record. Tests read this channel instead of run_all's private workdir (fk-35b618e9b24d). Dark-factory's slot-timeout classifier and its FAILED/Summary readers read it instead of grepping log lines. Members never see the parent channel, so **a new infra member cannot leak a slot-timeout classification by construction**. Once dark-factory reads the channel, Sections D–G of `test_slot_timeout_marker.sh` are deleted (fk-1417bf006761).
4. **Guard engines in sourced libs.** Each test file keeps its fixtures and live scans: harness-layout guard lib (fk-53a9afc0d2b9), `wallclock_rust_guard_lib.sh` (fk-37507523c365), the cpu-governance ROW predicates in the Python instrument (fk-a86e30ad9ecb), and the PTODO hard-gate fixture helper with one schema source (fk-9c99c0223cb3).
5. **Harness uniformity:**
   - suite-wide isolation detectors run from one end-of-suite hook (fk-2e56a7e3fcb7);
   - every member is on `test_helpers.sh` (fk-3cb220f9cc94);
   - seed stubs, base fixtures and lock holders are shared (fk-1c2e90be3491, fk-75e28dab2f08);
   - source-shape and prose pins are deleted (fk-34a184f51e6f);
   - the jobserver unit oracle moves to stdlib unittest (fk-c796ce463c73).
6. **The h14 partitions** recorded for the stream's files (all `stays_whole: false` in `h14_measurements`) are carried out, except `test_verify_scope.sh` and `test_verify_semaphore_e2e.sh`, which the lead ceded to S18 (§9 R-2).

### 1.1 Consumers (G1)

| mechanism | consumer |
|---|---|
| `_ra_run_member` / `_ra_record_result` | every `run_all.sh` branch (H9 `--scope host-infra`, `REIFY_RUN_ALL_MEMBER_SUBSET`, concurrent pool phases 1/2/2.5, legacy serial); the merge gate via `scripts/verify.sh`'s `run_all.sh` plan line |
| `classification_discovered_set` as sole discovery | `run_all.sh` (all branches + `REIFY_RUN_ALL_SUBSET_COUNT_ONLY`); `test_run_all_classification.sh` (already); task #7445's native `test_*.py` discovery (becomes one change) |
| `REIFY_VERIFY_EVENT_LOG` channel | (a) `test_run_all.sh` T19/T25 (leaf ζ₁); (b) dark-factory `verify_classify.py` SEMAPHORE_TIMEOUT arm, `merge_lane/shadow.py` run_all FAILED parse, `_redact_passed_suite_blocks` (cross-repo leaf ε); (c) the S18 Python verify executor, if it reimplements slot acquisition (cross-reference §8) |
| harness-layout guard lib | `test_harness_kloc_cap.sh` live scan; `scripts/check-harness-baseline-registration.sh` (diff-scoped gate; today it sources only `harness-layout-lib.sh`, so a rule added in the test never reaches it) |
| `wallclock_rust_guard_lib.sh` | `test_no_new_wallclock_rust_deadlines.sh` (live gate) + `test_wallclock_rust_guard_lib.sh` (hermetic) |
| cpu-gov ROW predicates in `cpu_gov_instrument.py` | the live ROW verdicts in `test_cpu_load_governance.sh` (today decided by the bash `_row3_within_bound` and inline awk) |
| `_ptodo_hardgate_fixture` + one schema source | `test_reify_audit_ptodo.sh` scenarios (c)–(g), `test_lane_task_status.sh`, `crates/reify-audit/tests/common/schema.rs` |
| `test_register_finalizer` in `test_helpers.sh` | `test_seed_warm_lane.sh` (and its partition satellites) plus the five sibling warm-lane suites that hand-call `assert_no_shared_trash_litter` at EOF |
| adoption ratchet (`test_infra_adoption_ratchet.py`) | the merge gate (a `run_all.sh` member); one row per extraction in this PRD (INV candidate 5) |

## 2. Background and measured premises (G3/G6)

Measured at `a030ee7528` by the review, re-checked at `8e6ffcac9d` for this PRD:

- **Branch copies.** In `run_all.sh`, `reify_git_env_scrub bash|env` spawn sites appear in the H9 branch, the subset branch, pool phase 1 (worker subshell), pool phase 2 (intra-run-serial), pool phase 2.5 (retry) and legacy: 6 sites. `RESULT: FAIL` echo sites: 5. Per-branch coverage:

  | helper | branches covering it |
  |---|---|
  | `_ra_emit_sanitized` | subset, pool |
  | `_ra_collect_fail_detail` | subset, pool |
  | `_ra_interrupt_if_worktree_gone` | pool, legacy |
  | `_h3_exclude` | pool, legacy |
  | retry | pool only |
  | content-skip | pool only |

  FD hygiene is uneven too: `9<&-` is present at the H9 and phase-1 sites and absent at phase 2, phase 2.5 and legacy. Helpers talk through globals: `_ra_skip_engine` rewrites `_h2_discovered_list` in place, and `_ra_skip_state_write` reads `failures`.
- **Discovery.** `classification_discovered_set` (in `run-all-classification-lib.sh`) is called only by the pool branch. Three inline `test_*.sh` loops with the `test_helpers.sh` exclusion remain: the count-only probe, the subset branch and the legacy branch. The lib header still says "H2 will refactor run_all.sh's discovery to consume this function directly". `_ra_skip_closure_specs` also names `test_helpers.sh`, but as a closure spec, not discovery.
- **Slot-timeout classification is text-only today.** `scripts/lib_slot_acquire.sh::slot_emit_timeout` prints `@@REIFY_SLOT_TIMEOUT@@ reason= slots= waited= disposition= lock=` at column 0 to stderr. Dark-factory `orchestrator/verify_classify.py` has two matchers: `_SLOT_TIMEOUT_SENTINEL_RE` (line-anchored) plus a three-basename allowlist of wrapper deadline lines (`lib_test_semaphore`, `cargo-test-occt-gated`, `lib_lane_x_flock`). `_has_fatal_slot_timeout_sentinel` gates on `disposition=`. `classify_failure(tool, rc, output, timed_out)` takes **only output text**. `_redact_passed_suite_blocks` keys on run_all's `--- Running: <name> ---` / `  RESULT: (PASS|FAIL|SKIP) (<name>)` framing. `merge_lane/shadow.py::_RUN_ALL_FAILED_MARKER_RE` parses run_all's `FAILED <names>` line. No `REIFY_VERIFY_EVENT_LOG` exists anywhere in either repo (`git grep` at `8e6ffcac9d`: plans only).
- **Existing, distinct event substrate.** `REIFY_SLOT_EVENT_LOG` (`slot_emit_event`) is an opt-in, space-delimited ACQUIRE/RELEASE trace consumed by the de-flake R-technique tests. It is test instrumentation, not a classification channel, and this PRD leaves it alone (§7).
- **Leak analyzer.** `test_slot_timeout_marker.sh` sections are A 329, B 114, C 95 and H 265 lines (the contract, 803 lines). D 279, E 270, F 1789 and G 1075 lines (the analyzer, 3,413) make up the rest of the 4,384 lines. Section D's own preamble names "it does not depend on a consumer-shaped rewrite staying in sync with a cross-repo regex" as a reason per-site diversion cannot be retired while dark-factory classifies text. That reason disappears only when dark-factory stops classifying text for a run that carries the channel (contract C3, rule 4).
- **Premise correction (G6) to fk-3cb220f9cc94.** The finding counts `test_slot_timeout_marker.sh` as the second member "on a private harness", by its rule `^(PASS_COUNT|PASS)=0`. At `8e6ffcac9d` that file **sources `test_helpers.sh`**. Its only `PASS=0` sits inside the `E4_RAW_EMITTER` heredoc, a deliberate pre-6353 `assert()` reproduction used as Section E's positive control. Only `test_tree_sitter_pipeline.sh` runs a private harness. The ratchet row for this lands in ο at ceiling 1, a known false match in that heredoc. η deletes Section E and lowers it to 0.
- **Members inherit nothing run_all does not inject.** `tests/infra/run-all-ambient-vars.manifest` + `test_run_all_ambient_isolation.sh` + `run_all_ambient_isolation_lib.sh::run_all_injected_env_keys` derive behaviourally what run_all injects into members. **Un**-injecting a var (the `env -u` this PRD adds) is not an injection and needs no ledger row. The ledger's own blind-spot list says the hostile loop drives the real `test_run_all.sh` (relevant to ζ₂).
- **Every file in this PRD is a verify-pipeline path.** `bash scripts/verify-pipeline-guard.sh requires-full-gate <path>` exits 0 for `tests/infra/run_all.sh`, `run-all-classification-lib.sh`, `harness-layout-lib.sh`, `test_helpers.sh`, `scripts/lib_slot_acquire.sh` and a new `tests/infra/*lib.sh`. Every leaf lands through the merge queue under the full `--scope all` gate; no leaf takes the config-only fast path.
- **Guard-engine anchors** (all re-read at `8e6ffcac9d`):
  - `harness-layout-lib.sh` defines `_harness_layout_norm_path`, `_harness_layout_row_crate` and `_harness_layout_mod_decls`, which the test reaches as private names.
  - `scripts/check-harness-baseline-registration.sh` sources only the lib.
  - `cpu_gov_instrument.py` defines `slowdown_within_bound` and `psi_below_band` with no caller outside `_selftest`, while the live verdict uses bash `_row3_within_bound`.
  - `TASKS_DB_SCHEMA` lives in `crates/reify-audit/tests/common/schema.rs`. `CREATE TABLE tasks` appears 5 times in `test_reify_audit_ptodo.sh` and once in `test_lane_task_status.sh`. Only `@@HARDGATE_C/D_PASSED@@` have a consumer (`test_reify_audit_ptodo_orphan_hardgate.sh`).
  - `slot_holder_handshake_lib.sh` (`holder_wait_for_marker`) is the shared causal-handshake primitive that `test_warm_lane_gc.sh` and `test_warm_lane_pool.sh` already adopted. `test_seed_warm_lane.sh` still carries its own `_wait_for_reader_lock` and 8 hand-copied Block Q holders.
  - `scripts/jobserver-balancer.py` defines `_transfer_burst` and `decide`.
- **Policy.** `docs/notes/infra-test-bash-to-python-migration-policy.md`:
  - Arm 1: new tests are Python (stdlib unittest + `.sh` wrapper + manifest row).
  - Arm 2: port a bash member when it is touched for a flake.
  - "Neither arm is ever a scheduled mass migration."
  - "A port is a behaviour-preserving rewrite."

## 3. Resolved design decisions

D1. **Runner shape.** The runner is two functions, both in `run_all.sh`, which stays the entry script per the h14 partition.
   - `_ra_run_member <name> <capture> <rc-file> <mode>` handles:
     - spawn through `reify_git_env_scrub`;
     - `9<&-` always;
     - `env -u REIFY_RUN_ALL_MEMBER_SUBSET -u REIFY_VERIFY_EVENT_LOG`;
     - capture;
     - rc written atomically (tmp+mv);
     - the sanitized copy;
     - fail-detail extraction;
     - `member_start`/`member_finish` events.

     `<mode>` is `capture` or `stream`, the one orthogonal output dimension (h3). `capture` defers re-emission to the report phase, and the pool uses it for block atomicity. `stream` pipes the member through the same sanitizer, line-buffered, and tees a capture for fail detail; the serial branches (H9, subset, legacy) use it so their live output keeps streaming.
   - `_ra_record_result <name> <rc> [--retried <attempt-1-capture> <attempt-1-label>]` prints the existing framing byte-for-byte (contract C1), updates `failures`/`failed_names`/`flaky_names`, and returns.
   - Retry-once, content-skip, H3 exclusion and the subset filter are scheduling or filtering policies that call these two. Skip, ledger and term helpers receive the discovered list and results as arguments, never via globals; `_ra_partial_failed_names`/`_ra_on_term` stay signal handlers over live state, per the partition record.
D2. **Discovery** is `classification_discovered_set` alone. The H9 branch is `classification_bucket host-exclusive ∩ discovered`. The subset and H3 exclusion filter its output. The count-only probe counts the same filtered output the subset branch runs. Ordering becomes the lib's `sort` order in every branch. Pinning `LC_ALL=C` inside the lib is in scope for α, so pool, subset and legacy order agree under any ambient locale.
   - *Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-4):* `classification_discovered_set` and `classification_bucket` read each member's `# run-all-bucket: pool|intra-run-serial|host-exclusive` pragma (first 20 lines) and fail closed on a missing or unknown bucket; `tests/infra/run-all-classification.manifest` is deleted. Leaf υ.
D3. **The event channel is JSON Lines with a schema version, written with an encoder, read with a parser.**
   - Writing goes through `scripts/lib_event_log.sh::event_log_emit_*`, one function per record type with positional args, plus one `event_json_string` escaper. It lives under `scripts/` because `scripts/lib_slot_acquire.sh` must not depend on `tests/infra/`.
   - Reading is never done by a bash ad-hoc parser (h12): readers use `tests/infra/verify_events.py` (stdlib `json`, `python3 -I`), which is both a CLI and an importable module, or dark-factory's own `json`.
   - The channel is production-inert when unset: zero bytes and zero forks, with the guard as the first operation, the same idiom as `slot_emit_event`.
   - Writing is non-fatal (`|| return 0`) and must never change control flow.
D4. **Member isolation by construction.** `_ra_run_member` unsets `REIFY_VERIFY_EVENT_LOG` for the member. A member's fixture `slot_acquire` deadlines, or a nested `run_all.sh` it drives, can therefore never write into the parent channel; nested runs that need a channel set their own. `run_all.sh`'s own pool-wait `slot_acquire`, which runs in the worker subshell before the member spawn, still writes to the parent channel. That is the one finite-WAIT path outside dark-factory's basename allowlist.
D5. **Dark-factory owns path allocation.** Dark-factory exports `REIFY_VERIFY_EVENT_LOG=<per-verify-invocation path>` into the verify process tree and reads the file after verify exits. `scripts/verify.sh`'s own processes receive the var untouched (S18 owns that file), but its task-scope selective-infra leaf spawns each `tests/infra` member with `env -u REIFY_VERIFY_EVENT_LOG` (#8919), so C4 holds for members run outside run_all as well as for `_ra_run_member`'s (D4). With the var unset, every behaviour on both sides is today's.
D6. **D–G deletion is gated on the dark-factory consumer, not on the channel's existence alone.** Rule 4 of contract C3 makes the channel the sole slot-timeout authority for any verify that carries a `run_start` record. Section H (the sanitizer pin) and Sections A–C (the emission contract) stay. The interim "extract D–G into a `scripts/` tool" alternative in fk-1417bf006761's proposal is **not** built, because it would be deleted a few leaves later.
D7. **Partition satellites are moves, not new authorship.** A satellite that receives existing bash blocks from an h14 partition stays bash. This is behaviour-preserving relocation, consistent with the policy note's "no scheduled mass migration". Every genuinely new assertion this PRD adds is Python (stdlib unittest + `.sh` wrapper + manifest row): the event reader's tests, the adoption ratchet, the cpu-gov instrument unit tests and the jobserver units. Leaf α records this in `tests/infra/README.md` and the policy note (S19 owns that seam). Confirmed by the lead (§9 R-3).
D8. **Every new member file lands with its `run-all-classification.manifest` row in the same diff.** Each satellite gets its own bucket on its own merits: `host-exclusive` only if it does real CPU burn or cgroup work, so splits of host-exclusive parents may rescue blocks into `pool`. The diff also repoints every tracked citation of a moved Block/Section; for example, CLAUDE.md's "`test_seed_warm_lane.sh` Block X" pointer moves to the satellite that receives Block X. Find citations with `git grep -n '<file>.*\(Block\|Section\|Test\) <id>'`.
   - *Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-4):* the "row in the same diff" rule is retired when leaf υ lands. From then on a new member carries its `# run-all-bucket:` pragma in its own header instead of a manifest row; the bucket-on-its-own-merits rule and the citation repointing stand. Leaves landing before υ still add rows, which υ's conversion moves.
D9. **Adoption ratchet (INV candidate 5).** One stdlib-unittest member, `tests/infra/test_infra_adoption_ratchet.py`, with its wrapper `test_infra_adoption_ratchet.sh`, reads a data table `tests/infra/adoption-ratchet.manifest`. Each row is `pattern-id | ERE | path | ceiling | owner leaf`. It counts code lines (whole-line `#` comments stripped, so it is a lint over code constructs, never a prose pin) and goes red when a count exceeds its ceiling. Leaf α creates the member and lands the first row. Every later extraction leaf adds its own row, set to the post-extraction count (normally 0 or 1), and names it in its done-criteria.
D10. **Ordering against the leak analyzer.** Until η deletes Sections D–G, any new suite that drives `run_all.sh` or a deadline-capable lib must be registered in `D_ROSTER` and `G_MEMBERS`/`G_SITE`/`G_SCAN`, or F1/G0 go red. So the partition of the roster member `test_run_all.sh` (ζ₂) waits for η. S18, which now owns the partition of `test_verify_semaphore_e2e.sh`, inherits the same ordering: after η. Partitions of non-roster files proceed in parallel from the start.
D11. **Harness-layout data.** The `_KLOC_WARN_KNOWN` WARN-set ratchet moves from an in-script array to a manifest beside `harness-layout-baseline.manifest`, so both ratchets use one mechanism (structured data, h12). The C1/C2 contract prose moves to `docs/contributor/harness-layout-contract.md`, and the 35 `crates/**` headers that cite `test_harness_kloc_cap.sh` as the contract's home are repointed there.
   - *Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-1, §D-K8-2):* the WARN ratchet is **deleted, not relocated**: `harness_warn_ratchet_violations`, `_warn_ratchet_rows`, `_KLOC_WARN_KNOWN` and hermetic/live Sections 4c/5d go; `tests/infra/harness-layout-warn.manifest` is not created; the WARN tier prints and never gates; the hard cap stays. The guard-lib extraction and the script partition are dropped (the cargo-native-harness-layout stream deletes most of those engines). The layout and baseline halves of the harness data, the comment-blind measure and numbered shards move to that stream; θ keeps the contract doc and the 35 header repoints.
D12. **PTODO schema single source.** A tracked `crates/reify-audit/tests/fixtures/tasks_db_schema.sql` is the one copy. `schema.rs::TASKS_DB_SCHEMA` becomes `include_str!` of it, and `_ptodo_hardgate_fixture` plus `test_lane_task_status.sh` seed from it with `sqlite3 < file`. Extracting the Rust raw string with sed would be an ad-hoc parser (h12), so the fixture helper does not do that. `test_reify_audit_ptodo_orphan_hardgate.sh` asserts **every** emitted `@@HARDGATE_[C-G]_PASSED@@` sentinel, which pins the stale-binary property for (e)–(g); the unconsumed sentinels are not dropped. Scenario (e)'s raw `"$REIFY_AUDIT_BIN"` calls go through `run_audit`.
D13. **End-of-suite hook.** `test_helpers.sh` gains `test_register_finalizer <fn>`, and `test_summary` runs the registered finalizers (in order) before it prints `Results:`. The registry is inert by default (empty array; the source-time footprint stays inert, per the file's warm-lane facility note), and no EXIT trap is added (the file's own rule). `init_isolated_lane_root` registers `assert_no_shared_trash_litter`, and `test_seed_warm_lane.sh` registers `_assert_no_shared_trash_use` and `_assert_no_bare_tmp_lanes`. The five siblings drop their hand-placed EOF call. Placement can no longer matter.

## 4. Contract (B+H — the run_all ↔ dark-factory and run_all ↔ test seam)

G5: this seam is load-bearing for merge-gate classification. It has two cross-repo consumers (dark-factory, and the S18 executor), and the mechanism count is over 8, so the PRD is B+H.

**C1 — text framing stays byte-stable.** Every branch keeps emitting the following shapes. Dark-factory's `_redact_passed_suite_blocks`, `shadow.py` and the verify FAILED scan depend on them, and they remain the human projection after the channel lands:
- `--- Running: <name> ---` at column 0;
- `  RESULT: PASS|FAIL|SKIP (<name>)`, with the optional ` [flaky: passed on serial retry]`;
- `--- attempt 1 (concurrent pool|serial) ---` and `--- attempt 2 (serial retry) ---`;
- `--- FAILED-DETAIL: <name> ---` … `--- END FAILED-DETAIL: <name> ---`;
- `=== Summary: …` in each of its three forms;
- `=== FAILED: … ===`, `FAILED <names>`;
- the `_ra_on_term` partial forms;
- the exit code.

The only intended byte change is that member-captured `@@REIFY_SLOT_` / `@@REIFY_CLOCK_` tokens and the three wrapper-basename deadline lines are now neutralized in the H9 and legacy branches too.

**C2 — record schema (v1).** One JSON object per line, appended with a single `printf … >>` (O_APPEND). Each record is capped at 4,000 bytes; `fail_detail` entries are truncated and counted as `fail_detail_omitted`. Every record carries `"v":1`, `"ts_ns"`, `"pid"` and `"event"`.

| `event` | fields | writer |
|---|---|---|
| `run_start` | `run_id`, `scope` (`all`/`host-infra`), `branch` (`pool`/`serial`/`subset`/`host-infra`), `discovered` (int) | run_all |
| `member_start` | `member` (basename), `attempt` (1/2), `mode` (`capture`/`stream`) | `_ra_run_member` |
| `member_finish` | `member`, `attempt`, `rc` (int), `verdict` (`PASS`/`FAIL`/`SKIP`/`FLAKY_PASS`), `duration_ms`, `fail_detail` (list of strings), `fail_detail_omitted` (int) | `_ra_run_member` (rc, detail) + `_ra_record_result` (verdict) — see note |
| `slot_timeout` | `reason`, `slots`, `waited`, `disposition` (`fatal`/`soft`), `lock` (string, JSON-escaped), `emitter` (`run_all_pool` or the calling lib basename) | `slot_emit_timeout`, for every caller in the verify tree (§9 R-1) |
| `run_finish` | `discovered`, `failures`, `failed` (list), `flaky` (list), `skipped` (list), `interrupted` (bool) | run_all, normal path and `_ra_on_term` |

Note: `member_finish` is written once, by `_ra_record_result`, after the verdict is known. `_ra_run_member` passes rc and detail through its capture metadata. One writer per record keeps it SPOT.

**C3 — consumer rules (normative for both repos).**
1. A record line that fails `json.loads` is skipped and counted. A file with no `run_start` record is "channel absent".
2. Channel absent ⇒ every existing text arm is unchanged (back-compat for older reify and non-reify projects).
3. Channel present ⇒ FAILED names come from `run_finish.failed` (equal by construction to the `FAILED <names>` text).
4. Channel present ⇒ SEMAPHORE_TIMEOUT is decided only by `slot_timeout` records with `disposition == "fatal"` (plus the existing soft-only semantics). The sentinel regex and the basename allowlist are **not consulted** for that verify. This rule is what makes D–G deletable.
5. Writers never block or fail on the channel (D3).

**C4 — isolation.** No `REIFY_VERIFY_EVENT_LOG` is ever present in a member's environment (D4). The parent channel receives `slot_timeout` records only from processes that are not run_all members.

### 4.1 Boundary-test sketch (both sides)

| # | side | scenario | pre | post |
|---|---|---|---|---|
| B1 | reify (γ) | fixture dir: one PASS, one FAIL (with `  FAIL: x`), one SKIP-able member; `REIFY_VERIFY_EVENT_LOG` set; pool and `REIFY_RUN_ALL_POOL_DISABLE=1` runs | private manifest, private pool lock | one `run_start` and `run_finish`; 3 `member_finish`; `run_finish.failed` == the text `FAILED` line's names; FAIL record carries `fail_detail==["  FAIL: x"]` |
| B2 | reify (γ) | member forces a real `slot_acquire` deadline (fatal) and prints a quoted sentinel | parent channel set | parent channel has **0** `slot_timeout`; re-emitted text has 0 live `@@REIFY_SLOT_` in **all four** branches |
| B3 | reify (γ) | pool lock pre-held by a released-by-test holder (`slot_holder_handshake_lib.sh`), `_H2_POOL_WAIT` tiny | parent channel set | exactly 1 `slot_timeout` with `disposition:"soft"`, `emitter:"run_all_pool"`; member still ran (soft admission) |
| B4 | reify (γ) | `lock` base containing space, `"`, `\` and a newline | channel set | `verify_events.py` round-trips the exact string |
| B5 | reify (γ) | var unset | — | no file created; `strace -f -e trace=openat` (or an fd-probe) shows no channel open; text output byte-identical to the var-set run |
| B6 | reify (β) | the same fixture run under each of the four branches | — | framing projection (lines matching C1's shapes) identical across branches except H9's member set; exit codes equal |
| B7 | reify (γ) | `lib_test_semaphore.sh` deadline outside run_all | channel set | exactly 1 `slot_timeout` `disposition:"fatal"`, `emitter:"lib_test_semaphore.sh"` |
| D1 | dark-factory (ε) | log carries a column-0 live sentinel inside a PASS block; channel present with no `slot_timeout` | — | NOT SEMAPHORE_TIMEOUT |
| D2 | dark-factory (ε) | channel present with a fatal `slot_timeout`; log text has no sentinel | — | SEMAPHORE_TIMEOUT |
| D3 | dark-factory (ε) | channel absent | — | classification identical to today's on the existing fixture corpus |
| D4 | dark-factory (ε) | channel file has a torn last line | — | torn line skipped, counted; other records honoured |
| D5 | dark-factory (ε) | `run_finish.failed` vs `shadow.py` text parse on a real captured run | — | equal |

## 5. Pre-conditions

- None upstream in the program (S19 is wave 1, "upstream seams: none").
- Leaf ε (dark-factory) must land and be deployed (green-tier is irrelevant here; this is a dark-factory code deploy) before η.
- Filed mechanical tasks #8535, #8536 and #8538 are adopted into this DAG as prerequisites (§9), not re-filed.

## 6. Cross-PRD relationship and seam owners (G4)

| seam / shared artefact | owner (program §Seam ownership) | this PRD's role |
|---|---|---|
| dark-factory's slot-timeout and Summary/FAILED consumers | **S19** | owns it; leaf ε is filed in dark-factory. Scope read here as consumers of **run_all.sh-emitted** lines (`FAILED <names>`, `=== Summary/FAILED`, run_all framing) plus the `@@REIFY_SLOT_TIMEOUT@@` family. S18's row "dark-factory consumers of @@TOKEN@@/Summary/FAILED lines" is read as **verify.sh-emitted** lines. See CROSS-REFERENCE in the hand-back |
| `run-all-classification.manifest` rows for any new wrapper | **S19** | every leaf adding a member adds its row in the same diff (D8) |
| `tests/infra/README.md` Python migration policy | **S19** | leaf α (D7) |
| `scripts/verify.sh`, `verify-pipeline-paths.txt`, `plan_capture_lib.sh` | S18 | consumer only, plus the selective-infra member unset (#8919, D5). ο moves `test_print_plan_capture_retries_on_truncation` into the existing `test_plan_capture_lib.sh` without editing the lib |
| `scripts/lib_slot_acquire.sh` | unlisted | S19 edits `slot_emit_timeout` (γ); S18's executor is a downstream consumer of C2 |
| `tests/infra/harness-layout-lib.sh` | unlisted | θ adds public names; existing public functions (`harness_layout_consolidatable_crates` etc., consumed by verify.sh and S18) keep their names and contracts |
| `scripts/lib_lane_lock.sh` | S20 | consumer. μ₂ (Block Q satellite) is filed HELD with `x_seam_wait` on S20 L1 (lane-lock API, filed concurrently). The lead releases it and wires the edge |
| `scripts/warm-lane-gc.sh` `_do_reclaim` | S20 (fk-cee9ca6b041a) | ν edits only `test_warm_lane_gc.sh` fixtures; file-lock serialization suffices, no edge |
| `crates/reify-audit/tests/common/schema.rs` | S24 area (reify-audit), not a listed seam | λ converts `TASKS_DB_SCHEMA` to `include_str!`, a one-line test-support change. Cross-reference to S24 |
| 35 `crates/**` headers citing `test_harness_kloc_cap.sh` | S23 (prose burn-down, wave 4) | θ repoints them to the docs home; S23 must not re-touch them |
| `test_verify_scope.sh`, `test_verify_semaphore_e2e.sh` | S18 (findings fk-d47473e4d8d8, fk-763b633b2775, fk-7bc1c1449010) | **ceded to S18** by the lead (§9 R-2); S19 does not partition or edit them. S18's PRD carries both h14 partitions as a cross-reference |
| task #7445 (native `test_*.py` discovery) | filed, pending | not subsumed; decompose adds #7445 → α |

## 7. Out of scope

- `REIFY_SLOT_EVENT_LOG`'s text format. It is test instrumentation with its own parsers; converging it onto C2 is a follow-up, and only if a consumer asks.
- Native `test_*.py` discovery (#7445). α makes it a one-predicate change.
- Rewriting `scripts/verify.sh` or its Summary/FAILED/@@TOKEN@@ emissions (S18).
- File:line citation burn-down across `tests/infra` (S23/S24 PCITE lane). This PRD's own moves repoint symbol and block pointers only (D8).
- Retiring dark-factory's text arms for non-reify projects. C3 rule 2 keeps them.

## 8. Decomposition plan

Labels are Greek. Real task ids are backfilled at decompose time. **Every leaf lands through the merge queue under the full gate** (§2). The *ratchet row* is the INV-candidate-5 member row the leaf adds to `adoption-ratchet.manifest`.

**α — Discovery from `classification_discovered_set` only** (#8678; medium; keys **fk-9c87c8ca99b5**; `run_all.sh`, `run-all-classification-lib.sh`, `test_run_all.sh`, `test_flake_density_report.py` docstring pointer, `tests/infra/README.md`, `docs/notes/infra-test-bash-to-python-migration-policy.md`).
- **Done:**
  - The count-only probe, the subset branch and the legacy branch consume the lib.
  - The H9 branch uses `classification_bucket host-exclusive` ∩ discovered.
  - The lib's "H2 will refactor" header and run_all's "MUST stay in sync" note are removed.
  - `LC_ALL=C` is set in the lib's sort.
  - A new test pins the legacy path (`REIFY_RUN_ALL_POOL_DISABLE=1`) and the pool path to the same `discovered` count and member set on a fixture holding a `test_*.sh` directory, a `test_helpers.sh` and a symlinked member.
  - Creates `test_infra_adoption_ratchet.py` + `.sh` wrapper + manifest row (D9) with ratchet row `inline-discovery-loop` (pattern: a `for` over `"$…"/test_*.sh` in `run_all.sh`), ceiling **0**.
  - The README and policy-note edits that record D7 (this PRD owns the README-policy seam): `## Auto-discovery` says discovery is `classification_discovered_set`, and a `## run_all event channel` stub points at this PRD's §4 (γ fills it in). They ride α so that no docs-only task pays a full merge gate.
- **Signal:** on that fixture, `bash run_all.sh <fixture>` prints the same `=== Summary: N discovered` under the pool and legacy paths, and `REIFY_RUN_ALL_SUBSET_COUNT_ONLY=1` prints N for the full subset. Re-adding an inline loop reds the ratchet.
- **Prereq:** none. α is the root because every extraction leaf adds a row to the ratchet member it creates.

**β — One member runner and one result recorder across all four branches** (#8681; high; keys **fk-acca603a9628**; `run_all.sh`, `test_run_all.sh`).
- **Done:**
  - D1 is implemented, honouring C1 byte-stability.
  - Every branch runs sanitize, fail detail, the worktree-gone check and `9<&-`.
  - `_ra_skip_engine` takes the list in and returns the kept list plus the verdict.
  - `_ra_skip_state_write` and `_ra_flaky_ledger_append` take failures/role/run-id as arguments.
  - `.rc` writes are atomic.
  - B6 is a new test.
  - Ratchet rows: `member-spawn-site` (`reify_git_env_scrub (bash|env)` in `run_all.sh`), ceiling **1**; `result-fail-recorder` (`RESULT: FAIL` echo), ceiling **1**.
- **Signal:** a fixture member printing a live slot sentinel and a `lib_lane_x_flock.sh: failed to acquire` line, run under `--scope host-infra` and under `REIFY_RUN_ALL_POOL_DISABLE=1`, shows 0 live tokens in run_all's output (today: survives in both branches). A failing member under H9 now produces a `--- FAILED-DETAIL:` block (today: none).
- **Prereq:** α.

**γ — `REIFY_VERIFY_EVENT_LOG` channel** (#8685; high; no own key — it is the mechanism fk-1417bf006761 and fk-35b618e9b24d consume; `scripts/lib_event_log.sh` (new), `scripts/lib_slot_acquire.sh`, `run_all.sh`, `tests/infra/verify_events.py` (new), `test_verify_events.py` + wrapper + manifest row (new), `scripts/verify-pipeline-paths.txt` row for `lib_event_log.sh` (transitively sourced; mirrors the `lib_slot_acquire.sh` row)).
- **Done:**
  - C2/C4 are implemented. `slot_emit_timeout` appends a `slot_timeout` record, keeping the text line unchanged.
  - B1–B5 and B7 are tests (the channel is verify-tree wide, §9 R-1).
  - `docs/notes/verify-pipeline-knobs.md` names the knob.
- **Signal:** `REIFY_VERIFY_EVENT_LOG=/tmp/x bash tests/infra/run_all.sh <fixture>; python3 -I tests/infra/verify_events.py summary /tmp/x` prints the run's discovered/failed counts, equal to the text Summary line. B2 shows 0 parent `slot_timeout` records under a deliberately deadlining member.
- **Prereq:** β.

**δ — run_all.sh h14 partition** (#8689; medium; no own key; `run_all.sh`, new `run_all_output_lib.sh` (sanitizer family + fail detail, owning `_ra_fail_detail` and `_RA_FAIL_DETAIL_MAX`), `run_all_flaky_ledger_lib.sh` (ledger + chronic check, all inputs as args), `run_all_content_skip_lib.sh` (list in, kept list + verdict out)).
- **Done:** the h14 record's 5 groups, with the term group staying in `run_all.sh`. No lib reads a `run_all.sh` global; this is checked by sourcing each lib in a test with `set -u` and calling it with only its arguments.
- **Signal:** each lib has a hermetic unit test, as Python if new assertions or bash if moved per D7. `test_run_all.sh` stays green unchanged.
- **Prereq:** β, γ.

**ε — dark-factory: read the channel (cross-repo; filed in dark-factory's registry, `external_deps` from η)** (dark_factory:6856; high; no reify key — it is the consumer half fk-1417bf006761's proposal names; dark-factory `verify.py` (allocate/export/read), `verify_classify.py`, `merge_lane/shadow.py`, `_redact_passed_suite_blocks` caller).
- **Done:** C3 rules 1–4 are implemented, with D1–D5 as dark-factory tests and the existing text arms unchanged when the channel is absent.
- **Signal:** a reify merge verify whose log carries a leaked column-0 sentinel inside a PASS block, with no channel `slot_timeout`, is not classified SEMAPHORE_TIMEOUT (D1). Dark-factory deploys via its own restart path.
- **Prereq:** γ landed on reify main (dark-factory's D-tests use γ's real `verify_events.py`-validated sample files, copied as fixtures).

**ζ₁ — test_run_all.sh T19/T25 onto the event seam** (#8692; medium; keys **fk-35b618e9b24d**; `test_run_all.sh`).
- **Done:**
  - T19 and T25 wait on `member_finish`/`run_start` records via `verify_events.py wait`, not on the TMPDIR glob `reify-run-all-pool.*`, `*.rc`/`*.retry.rc` or truncate-ordering.
  - The five private facts recorded in the finding are gone.
- **Signal:** mutating run_all's workdir prefix and `.rc` naming (a throwaway local mutant, not landed) leaves T19/T25 green, and removing `member_finish` emission reds them.
- **Prereq:** γ, #8535.

**η — Delete test_slot_timeout_marker.sh Sections D–G** (#8694; high; keys **fk-1417bf006761**; `test_slot_timeout_marker.sh`, `README.md`, the policy note's #7626 Section-F/G dialect text, CLAUDE.md "Infra tests" Section-F/G dialect sentences).
- **Done:**
  - D, E, F and G are deleted.
  - `D_ANCHOR` moves to the preamble beside `H_DF_ANCHOR`.
  - A new A-section case asserts the C2 record shape for the A9 space-bearing lock base.
  - The h14 partition of this file is satisfied by deleting groups 2–4; group 1 (A–C, H) stays whole.
  - The docs no longer instruct ports to emit a dialect for a guard that no longer exists.
  - Lowers ratchet row `private-harness` (`^(PASS|PASS_COUNT)=0` in `tests/infra/test_*.sh` excluding `test_helpers.sh`) from 1 to **0**. ο lands the row at 1; the 1 is the Section E heredoc this leaf deletes.
- **Signal:** adding a new suite that runs `run_all.sh` on a fixture with an unredirected deadlining member no longer reds anything (today: F1/G0 red), and the parent channel shows 0 `slot_timeout` records for it (B2 regression).
- **Prereq:** β, γ, ε (`external_deps`), ο.

**ζ₂ — test_run_all.sh h14 partition (6)** (#8696; medium; no own key; `test_run_all.sh` → it keeps Tests 1–8; `test_run_all_pool.sh`, `test_run_all_host_infra.sh`, `test_run_all_flaky.sh`, `test_run_all_markers.sh`, `test_run_all_subset.sh`; shared `run_all_test_preamble_lib.sh` holding `LOAD_TOLERANCE_LIB_T9` resolution and the `REIFY_RUN_ALL_FLAKY_LEDGER` isolation export).
- **Done:**
  - Manifest rows are added (D8).
  - `test_run_all_ambient_isolation.sh`'s hostile per-var loop drives every satellite, or an explicitly recorded subset with its coverage loss measured. Today it drives the real `test_run_all.sh`, so a naive split would silently drop 5/6 of its coverage.
- **Signal:** the satellites' combined assert count equals the parent's post-#8535 count, and `run_all` on the real tree lists all six.
- **Prereq:** η, ζ₁, π, #8535.

**θ — Harness-layout guard lib + h14 partition (4)** (#8700; high; keys **fk-53a9afc0d2b9**; `harness-layout-lib.sh`, new `harness-layout-guard-lib.sh`, `test_harness_kloc_cap.sh` (keeps Sections 5–10 live scans), new `test_harness_layout_guard_lib.sh` (Sections 1–4c), new `harness-layout-warn.manifest`, `scripts/check-harness-baseline-registration.sh`, `docs/contributor/harness-layout-contract.md` (new), 35 `crates/**` header pointers).
- **Done:**
  - D11 is implemented.
  - The rule functions take inputs and return values; no caller reads `_HL_*` globals.
  - The lib's helpers used by the guard get public names, and the test-local helpers lose the `_harness_layout_` prefix.
  - The diff-scoped gate sources the guard lib, so a new rule reaches it.
  - Ratchet row `harness-rule-fn-in-test` (`^harness_[a-z_]+_violations\(\)` in `test_harness_kloc_cap.sh`), ceiling **0**.
- **Signal:** a fixture adding a `#[path]`-less undeclared member reds both `test_harness_layout_guard_lib.sh` and `check-harness-baseline-registration.sh`. Today only the test sees rules added in the test.
- **Prereq:** α.
- **Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-1/§D-K8-2/§D-K8-3) — θ shrinks.** Reduced Done: `docs/contributor/harness-layout-contract.md` holds the C1/C2 contract prose; the 35 `crates/**` headers repoint to it; the WARN ratchet is deleted per amended D11. Dropped: `harness-layout-guard-lib.sh`, `test_harness_layout_guard_lib.sh`, `harness-layout-warn.manifest`, the diff-gate sourcing change and the `harness-rule-fn-in-test` ratchet row. Delivered checks re-derived: `rules-live-in-test-today` and `diff-gate-sources-guard-lib` removed, `contract-doc-home` kept, `kloc-warn-ratchet-deleted` added. Deleting the per-root accretion blocks, restated counts and escalation cites is the cargo-native stream's step A, not θ's; θ stays pointer-only. θ lands before that stream's step-A leaf. fk-53a9afc0d2b9 moves to that stream's final Python leaf when it is filed.

**ι — `wallclock_rust_guard_lib.sh` + h14 partition (3)** (#8703; medium; keys **fk-37507523c365**; new `wallclock_rust_guard_lib.sh` (the 12 engine names in the record's partition), new `test_wallclock_rust_guard_lib.sh` (Sections 1, 2, 4a–4f), `test_no_new_wallclock_rust_deadlines.sh` (Section 3 only)).
- **Done:**
  - The review-history prose (18 marker lines) moves to the commit message, with rule statements kept at the engine.
  - Ratchet row `wallclock-engine-in-test` (`^_detect_rust_wallclock_deadline\(\)` in the gate file), ceiling **0**.
- **Signal:** the live gate's verdict on main is unchanged. Mutating one engine regex reds the hermetic file and not the live file.
- **Prereq:** α.

**κ — cpu-gov ROW predicates: one home in the instrument + h14 partition (5)** (#8707; medium; keys **fk-a86e30ad9ecb**; `cpu_gov_instrument.py`, new `test_cpu_gov_instrument.py` + wrapper + manifest row, `test_cpu_load_governance.sh` (ROW cycles), new `cpu_gov_cycle_lib.sh` (`_cleanup_all`, `_ALL_MIX_PIDS`, `_row4_confine_*`), new `test_cpu_load_governance_classify.sh` (Cycle CLASSIFY), `docs/notes/cpu-load-governance-test-history.md` (header incident history)).
- **Done:**
  - Each ROW verdict calls exactly one predicate in the instrument, through a CLI verb.
  - `_row3_within_bound` and the inline ROW1 awk are deleted.
  - The ROW1-1-VACUITY asserts call the production predicate.
  - `slowdown_within_bound`'s floor semantics are decided once. The live SKIP-on-floor is preserved as an explicit outcome, not a second predicate.
  - `psi_below_band` either has a live caller or is deleted.
  - The `_read_proc_stat` re-export, its SELF-3 pin and the MIXFACTOR knob doc are deleted.
  - SELF/FIXTURE become unit tests where they need no bash helper.
  - Each satellite is reclassified on its own merits (D8).
  - Ratchet row `bash-row-predicate` (`^_row[0-9]_[a-z_]+\(\)` predicates returning a verdict in the cycles file), ceiling **0**.
- **Signal:** mutating the instrument's ROW3 bound reds `test_cpu_gov_instrument.py` and changes the live ROW3 verdict on a recorded sample. Mutating a deleted bash copy is impossible.
- **Prereq:** α.

**λ — PTODO hard-gate fixture helper, one schema source** (#8712; medium; keys **fk-9c99c0223cb3**; `test_reify_audit_ptodo.sh`, `test_lane_task_status.sh`, `test_reify_audit_ptodo_orphan_hardgate.sh`, `crates/reify-audit/tests/common/schema.rs`, new `crates/reify-audit/tests/fixtures/tasks_db_schema.sql`).
- **Done:**
  - D12 is implemented.
  - Scenarios (c)–(g) call `_ptodo_hardgate_fixture <marker-line> <cite-id> <status>`.
  - Every scenario runs through `run_audit`.
  - Ratchet row `inline-tasks-schema` (`CREATE TABLE tasks` in `tests/infra/*.sh`), ceiling **0**.
- **Signal:**
  - Adding a column to the `.sql` changes the Rust and bash fixtures together. `cargo test -p reify-audit` and both infra members stay green.
  - Deleting scenario (f)'s sentinel emission reds `orphan_hardgate` (today: silent).
- **Prereq:** α.

**μ — End-of-suite finalizers; seed-warm-lane shared fixtures and holders** (#8716; medium; keys **fk-2e56a7e3fcb7**, **fk-1c2e90be3491**; `test_helpers.sh`, `test_seed_warm_lane.sh`, the five sibling warm-lane suites).
- **Done:**
  - D13 is implemented.
  - `_make_base <tag> [sidecar-lines]` sits beside `make_isolated_lane` and replaces the 47 hand-rolled bases.
  - The 8 hand-copied Block Q holders go through `_q_hold_lane_lock`, and the suite's `_wait_for_reader_lock` is replaced by `holder_wait_for_marker`.
  - Ratchet rows:

    | row | pattern | ceiling |
    |---|---|---|
    | `seed-base-mktemp` | `_PARENT="$(mktemp -d /tmp/test-seed-` | 0 |
    | `q-holder-copy` | `^\( flock -x 9 && touch "\$Q_READY` | 0 |
    | `eof-trash-guard-call` | a bare `assert_no_shared_trash_litter` at file end in the 5 siblings | 0 |
- **Signal:** a fixture block appended after Block R that seeds a bare-`/tmp` lane now reds the suite (today: unobserved, 7–8 seed invocations past the detectors).
- **Prereq:** α, #8536.

**μ₂ — test_seed_warm_lane.sh h14 partition (9)** (#8719; medium; no own key).
- Files:
  - new `seed_warm_lane_harness_lib.sh`: stubs, `run_helper*` and `EPOCH_2020`/`STALE_EPOCH`, which move into the harness;
  - satellites `test_seed_warm_lane_guards.sh` (A,B,F,G,O), `_reseed.sh` (C,I,M,T,X + R6a), `_mtime.sh` (D,E,J,K,L,N,S,U), `_build_dir.sh` (H,P), `_lock.sh` (Q + R4), `_fd_hygiene.sh` (V), `_rerere.sh` (W);
  - `test_seed_warm_lane.sh` keeps R2/R5/R8 controls.
- **Done:** every satellite registers the D13 finalizers. Manifest rows are added and CLAUDE.md's Block-X pointer is repointed (D8).
- **Signal:** each satellite runs green standalone, and a bare-`/tmp` lane fixture placed in any satellite reds that satellite.
- **Prereq:** μ, S20 L1 (`lib_lane_lock.sh` `lane_lock_path`/`lane_lock_acquire`). μ₂ is filed HELD with `x_seam_wait = {stream: S20, contract: "lib_lane_lock.sh lane_lock_path / lane_lock_acquire (S20 L1)"}`, and the lead releases it.

**ν — test_warm_lane_gc.sh shared fixture and seed stub** (#8721; low; keys **fk-75e28dab2f08**; `test_warm_lane_gc.sh`).
- **Done:**
  - `make_gc_fixture <root>` returns repo/worktrees/base with the `target -> target.gen.1` + `.lock` layout.
  - The 12 inline stubs use `_seed_stub_body`.
  - Ratchet rows `gc-inline-gen-layout` (`touch .*target\.gen\.1\.lock`) at 1 (the helper) and `gc-inline-seed-stub` at 0.
- **Signal:** changing `_seed_stub_body`'s positional contract in one place reds or greens all 27 users together.
- **Prereq:** α, #8538 (file overlap on `test_warm_lane_gc.sh`'s one RED-until site).

**ξ — test_warm_lane_pool.sh h14 partition (4)** (#8723; medium; no own key).
- Files:
  - new `warm_lane_pool_harness_lib.sh` + `test_warm_lane_pool_harness_lib.sh` (SG/PRIV/PS-NORM/PS-RC/BH);
  - Block PG + `_mk_*_advancing_lane`/`_refresh_capture` → `test_refresh_warm_base.sh`;
  - Block FC → μ₂'s `seed_warm_lane_harness_lib.sh` stubs, retiring the divergent git/cp stubs;
  - `test_warm_lane_pool.sh` keeps the e2e core.
- **Signal:** host-exclusive time of `test_warm_lane_pool.sh` drops by the moved blocks' share, and the moved blocks run in their satellites' buckets.
- **Prereq:** α, #8538, μ₂.

**ο — test_tree_sitter_pipeline.sh onto test_helpers.sh + h14 partition (5)** (#8725; medium; keys **fk-3cb220f9cc94**).
- Files:
  - `test_tree_sitter_pipeline.sh` keeps orchestrator/hooks/verify-plan;
  - new `tree_sitter_test_lib.sh` (`run_guarded_cargo_check` tri-state adapter mapping to `assert`);
  - `test_tree_sitter_freshness.sh`, `test_tree_sitter_generate_stamp.sh`, `test_tree_sitter_build_rs.sh`;
  - `test_print_plan_capture_retries_on_truncation` → `test_plan_capture_lib.sh`.
- **Done:**
  - `declare -F | awk '/test_/'` discovery is gone; cases are called explicitly.
  - Each numbered sub-case of the 400–460-line functions is its own asserted step.
  - Ratchet row `private-harness` lands at ceiling **1**. The one match is the `E4_RAW_EMITTER` heredoc in `test_slot_timeout_marker.sh`, a fixture rather than a harness (§2 premise correction); η lowers the ceiling to 0.
- **Signal:** a forced failure of one freshness sub-case appears in run_all's `--- FAILED-DETAIL:` block with its description (today: empty), and later sub-cases still report.
- **Prereq:** α.

**π — test_test_helpers.sh: delete source-shape/prose pins + h14 partition (3); test_run_all.sh Test 6** (#8726; medium; keys **fk-34a184f51e6f**; `test_test_helpers.sh`, new `test_sync_ref_helpers.sh` (the behavioural checks from the sync-ref group), new `test_test_helpers_warm_lane.sh` (`_wl_*` group with `_robust_tmpdir` in its own preamble), `test_run_all.sh` Test 6).
- **Done:**
  - The 89 consumer source-shape checks and the 3 comment-text pins are deleted.
  - Test 6's 5 self-source asserts are deleted.
- **Signal:** rewording `sync_ref_helpers.sh`'s head -1 comment, or adding an `[ -n … ]` guard to `tests/sync_comments_test.sh`, leaves every member green, and breaking `sync_ref_helpers.sh` behaviour reds `test_sync_ref_helpers.sh`.
- **Prereq:** α.

**ρ — jobserver unit oracle to stdlib unittest; setup-dev checks out of the jobserver test** (#8727; low; keys **fk-c796ce463c73**; `test_jobserver_balancer.sh` (daemon blocks only), new `test_jobserver_balancer_units.py` + wrapper + manifest row (the 13 heredoc populations, one import), `scripts/jobserver-balancer.py` (`_transfer_burst` → public `transfer_burst` or covered through `decide`), Blocks 5/6/17/23 → `test_setup_dev_jobserver_units.sh` (moved, D7)).
- **Done:** ratchet row `jobserver-heredoc-import` (`spec_from_file_location` in `test_jobserver_balancer.sh`), ceiling **0**.
- **Signal:** renaming `transfer_burst` fails the Python unittest at import or call time (today: a string-embedded call inside bash).
- **Prereq:** α.

**σ — test_with_jcodemunch_serve.sh h14 partition (3)** (#8728; low; no own key).
- Files:
  - shared helpers (`cleanup`, `mk_tmpdir`, `require_nonempty`, `pick_free_port`, `port_is_free`, `argv_has_in_order`, `argv_lacks`, `dry_argv`) → `jcodemunch_pin_guard_lib.sh`;
  - pin/interpreter inventory and alpha-cite blocks → new `test_jcodemunch_pin_guard_lib.sh`;
  - the wrapper contract stays.
- **Signal:** each file runs green standalone, and the lib-inventory file needs nothing from the wrapper test.
- **Prereq:** α.

**υ — Run-all bucket declared in the test (`# run-all-bucket:` pragma)** (#9303; medium; keys **fk-4df7cbc5aeaf**; gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-4; `run-all-classification-lib.sh`, every `tests/infra` member header, `run-all-classification.manifest` (deleted), `test_run_all_classification.sh`, `nextest_absent_lib.sh`, `govtest_slice_reaper_lib.sh`, `scripts/check-infra-classification-manifest.sh`, `tests/infra/README.md`).
- **Done:**
  - Every discovered member carries `# run-all-bucket: pool|intra-run-serial|host-exclusive` in its first 20 lines (comment form, so the same line works in a `.py` member for #7445). The pass over the members is mechanical, derived from today's rows.
  - `classification_discovered_set` and `classification_bucket` read the pragma and fail closed on a missing or unknown bucket.
  - `run-all-classification.manifest` is deleted with its coverage / undeclared / orphaned / overlap / malformed drift checks; the rationale of the 12 non-pool rows moves into those tests' headers.
  - `nextest_absent_lib.sh` and `govtest_slice_reaper_lib.sh` repoint to the lib API.
  - `scripts/check-infra-classification-manifest.sh` becomes a pragma validator, or dies through S18's registry; no bash edit to `scripts/verify.sh` ahead of S18 ζ #9112.
  - `tests/infra/README.md` documents the one-block member header (`# run-all-bucket:` now; S18's `# guards:` line joins the same block), and amended D2/D8.
  - Subsumes #6483 (its header-count drift disappears with the manifest).
- **Signal:** deleting the pragma from one member reds discovery naming that member; `run-all-classification.manifest` no longer exists and `bash tests/infra/run_all.sh` discovers the same member set.
- **Prereq:** α (#8678, done).

**φ — One per-member timing emitter** (#9309; medium; keys **fk-83fa1aeda5f6**; gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-9; `run_all.sh`, `verify_events.py`, `test_run_all_timing.py`, `docs/notes/verify-instrumentation-ledger.md`).
- **Done:**
  - `_ra_mark` tq/ts/te timing moves onto γ's event channel, recorded in every branch including `_ra_run_serial_member` (dark-factory's merge-gate retry path).
  - `_ra_timing_member`, `_ra_timing_pool_lines` and the `REIFY_RUN_ALL_TIMING_OUT` member rows are deleted.
  - The run_all block of `docs/notes/verify-instrumentation-ledger.md` is derived from the event log, and the ledger's "Known blind spots" row for it is cleared.
  - No `scripts/verify.sh` edit (S18 owns it).
- **Signal:** a run under `--scope host-infra` (serial branch) yields one `member_finish` record per member with queued/started/ended times; today it yields no timing row.
- **Prereq:** γ (#8685). Filed as a follow-up so the in-progress γ is not re-scoped (G1).

**Adopted filed tasks (referenced, not re-filed):**
- **#8535** (fk-4229ece8d26d, collapse the 31 skip-mirror else-branches in `test_run_all.sh`) is a prerequisite of ζ₁ and ζ₂.
- **#8536** (fk-6213e3dfdbec, Block Q H1–H13 → Q1–Q13) is a prerequisite of μ.
- **#8538** (fk-6e371c1b8c6f, RED-until labels) is a prerequisite of ν and ξ.

Decompose wires these as `add_dependency` edges. It neither cancels nor re-files them.

**Ω — PRD close** (#8729; low). Depends on every leaf and the adopted tasks.
- **Done:**
  - Stamp the status `SHIPPED` with the landed ids.
  - Add the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map. §4 C1–C4 stay LIVE: `run_all.sh`, `verify_events.py` and dark-factory defer to them.
  - Apply the same header to the capability manifest.

### 8.1 DAG

```
α ─┬─ β ── γ ──┬── δ
   │           ├── ζ1 (also #8535) ───────┐
   │           └── ε(DF, external) ── η ──┼── ζ2 (also π, #8535)
   │                         ο ───────┘
   ├─ θ   ├─ ι   ├─ κ   ├─ λ   ├─ π   ├─ ρ   ├─ σ   ├─ ο
   ├─ μ (#8536) ── μ2 (S20 lane-lock leaf) ── ξ (#8538)
   └─ ν (#8538)
α ── υ (gap-fill)        γ ── φ (gap-fill)
Ω depends on all.
```

## 9. Open questions

**Lead rulings on the pass-1 design questions (2026-10-12, recorded in the program doc §Resolved decisions — pass 1 open questions):**

1. **R-1 (was OQ-1) → C.** The channel is verify-tree wide: every `slot_emit_timeout` caller writes a record. It is named `REIFY_VERIFY_EVENT_LOG` (heuristic 1), a lead amendment of the synthesis label `REIFY_RUN_ALL_EVENT_LOG`. C3 rule 4 holds, B7 is in scope, and η deletes D–G outright. S18's Python verify passes the variable through, and appends `slot_timeout` records if it reimplements slot acquisition. The stdlib reader is named `tests/infra/verify_events.py` to match.
2. **R-2 (was OQ-2) → A.** The `test_verify_scope.sh` (6) and `test_verify_semaphore_e2e.sh` (4) partitions are ceded to S18, so leaf τ is dropped and not filed. S18's PRD carries both partitions as a cross-reference.
3. **R-3 (was OQ-3) → A.** D7 is confirmed: h14 partition satellites are behaviour-preserving moves and stay bash. Only new assertions are Python.
4. The premise correction to fk-3cb220f9cc94 (§2) is accepted.

**Tactical (decide during the named leaf):**

4. Fail-detail cap per record (C2) — β/γ: default 4,000 bytes per record; `_RA_FAIL_DETAIL_MAX` lines, each truncated at 400 chars.
5. Exact satellite basenames in μ₂/ζ₂/ο/κ — the names above are defaults; any rename keeps D8's citation repointing.
6. Whether `verify_events.py wait` polls (stdlib `time.sleep` with a deadline) or uses inotify — default poll at 50 ms with a caller-supplied deadline. This is a wait, not an upper-bound assertion, so the wall-clock ratchet does not apply.

## Gap-fill amendments (2026-10-16)

- **θ #8700 shrinks** — plans/gap-fill-deliberation-reify-20261016.md §D-K8-1 (re-rules, for the harness-layout guards only, the program's S19 clause "harness guard engines moved to sourced libs"). θ keeps the contract doc `docs/contributor/harness-layout-contract.md` and the 35 header repoints, and deletes the WARN ratchet. The guard-lib extraction and the script partition are dropped. θ lands before the new `cargo-native-harness-layout` stream's step-A leaf. That stream's final Python leaf takes fk-53a9afc0d2b9 and deletes `test_harness_kloc_cap.sh` and the layout half of `harness-layout-lib.sh`; `harness_layout_consolidatable_crates` keeps its contract until S18's crate-scope leaf (§D-K8-6) moves it. Delivered checks changed (three writes).
- **D11 amended: the kLOC WARN tier is advisory** — §D-K8-2. The ratchet machinery is deleted, not relocated; the hard cap stays; the comment-blind measure and numbered shards go to the cargo-native stream.
- **Harness-root prose** — §D-K8-3 (uncontested block). Deleting the per-root accretion blocks, restated counts and escalation cites is the cargo-native stream's step A; θ stays pointer-only; S23's exclusion stays true (fk-9e9f9b36fbcc).
- **D2/D8 amended; new leaf υ #9303** — §D-K8-4. The run-all bucket is a `# run-all-bucket:` pragma in the member; the manifest and D8's same-diff row rule retire at υ; #6483 is cancelled as subsumed by υ.
- **New leaf φ #9309 after γ #8685** — §D-K8-9 (option B, the G1 default). One per-member timing emitter on the event channel; γ is not touched.
