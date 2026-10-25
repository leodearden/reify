# PRD — verify: structured plan steps and one role table (review-all S18)

**Status:** decomposed 2026-10-15 — tasks #9095–#9117 (22 leaves; ids in §10) and dark_factory #7006; review-all-reify-20261010 program, stream S18 (wave 2, spawn session). Leo ruled the open questions Q1–Q4 on 2026-10-14 and Q5 on 2026-10-15 (§12). A fable critic pass (24 findings) is folded in. gap-fill 2026-10-16: new leaves τ #9312, υ #9313, φ #9314, χ #9315; ζ #9112 and β #9097 amended (§Gap-fill amendments).
**Program:** `plans/review-all-program-reify-20261012.md`, stream **S18** (size L, risk high, mode spawn, wave 2, no hard upstream seam). Ruling: §Resolved decisions → "S18 verify.sh: structured plan steps and one role table" (all-Python target; `scripts/verify.sh` becomes a one-line exec shim; the text projections stay byte-stable; a plan-parity test gates a one-commit cutover; `run_all.sh` stays out). Cross-references are folded in from §Lead amendments, from the pass-1/pass-2 rulings R2 (S22) and Q1/Q2 (S19), and from the S21/S22/S19/S27 cross-reference lines.
**Findings closed:** 13 keys: `fk-507b17f8adc7` (high), `fk-21149d55fd41` (high), `fk-902a5ad9889e` (high), `fk-73c909381788`, `fk-03d27a868cfd`, `fk-b072e820e926`, `fk-fc5ee820d36e`, `fk-d47473e4d8d8`, `fk-6ccbfc296ee8`, `fk-7b476f26ad45`, `fk-763b633b2775`, `fk-3988a6996b82`, `fk-7bc1c1449010` (medium). None had a mechanical filing (report §Mechanical filings: all `open → this stream`). The key-index scan of all 8,890 reify tasks on 2026-10-14 found no task carrying any of them.
**Invariants established:** none promoted to this stream. Every extraction leaf names its adoption-ratchet row (program INV candidate 5) in `tests/infra/adoption-ratchet.manifest`. The new declarable surfaces (knob registry, role/profile tables) get a census row (§4 D14, G7).
**Code anchors** verified against main `1b06f93151` (2026-10-14). At that tree, `scripts/verify.sh`, `scripts/verify-pipeline-guard.sh`, `tests/infra/plan_capture_lib.sh`, `tests/infra/copy_list_preflight_lib.sh`, `scripts/lib_test_semaphore.sh`, `scripts/cpu-admit.sh`, `scripts/lib_proc_reaper.sh` and `scripts/tree-sitter-generate.sh` were byte-identical to the review's pinned tree `a030ee7528`.

Two prerequisites landed between authoring and commit (2026-10-15):
- **#7424** (verify instrumentation ledger) added the run-journal layer to `scripts/verify.sh` (+176/−25: `_vl_*`, `scripts/verify-ledger.py`). ε ports it (D10).
- **#8662** (S22 θ) put `PathFacts.inert`/`tree` and the `attribute` CLI in `scripts/reify_path_attribution.py`, and made `reify_is_inert_path` a thin per-path wrapper (#9056 measures its cost; β subsumes it).

Anchors are cited by symbol; re-locate lines at implementation time.

---

## 1. Consumer and user-observable surface (G1, G2)

`scripts/verify.sh` is the gate. Its consumers exist today; this PRD adds no seam.

- **The landing gate.**
  - `hooks/project-checks` (`exec … verify.sh all --profile debug --scope staged --include-infra`, on `main` only).
  - `hooks/pre-merge-commit` and `scripts/land.sh` (`DF_VERIFY_ROLE=merge … all --profile both --scope all`).
  - `scripts/run-offline-deep.sh` (role offline, plus the `--confirm-failed` protocol).
  - Dark-factory's task, merge and background verifies (`dark-factory-orchestrator.yaml` `test_command` / `lint_command`; `DF_VERIFY_ROLE` ∈ {task, merge, background}).
- **The fast-path oracle.** `scripts/verify-pipeline-guard.sh requires-full-gate`, consulted by dark-factory `verify.py::_verify_pipeline_guard_requires_full_gate` for config-only merge diffs.
- **The infra suite.** 50 test files assert on `--print-plan` text, 16 read verify.sh's source, 12 execute it, and 14 build sandboxes from it (§2.3).

The user is the agent or developer who changes the gate: adds a role property, a nextest flag, a plan step, a scope arm or a fixture. Today that change costs 16 role sites, a `_DEBUG/_RELEASE` variable pair, a hand-edited fixture list and a source-text shadow parser in a 1,953-line test. After this PRD it is one table row, one emitter, or nothing at all (fixture coupling is derived).

User-observable signals: the bytes and exit codes of `scripts/verify.sh` (`--print-plan`, `--print-plan=json`, `--print-scope`, `--print-role-policy`, `--print-knobs`, `--print-gates`, real runs), and the guard's verdicts.

## 2. Problem (the findings, re-verified at main)

### 2.1 The plan is shell text, and behaviour routes on its substrings

- **fk-507b17f8adc7 (high).** `PLAN` is a bash array of command strings. It has 36 append sites: 21 `add_tool` and 15 `add`, each `add` carrying a `# ld-ok:` marker.
  - The executor sends any line containing `_VERIFY_NODE_BG_PID` to a main-shell `eval`.
  - The printer annotates by the glob `*'./scripts/verify.sh psi-gate'*`.
  - `confirm_failed_run` re-parses verify.sh's own printed plan with awk between two marker comments.
  - Seven string fragments are concatenated into the nextest command. The retry composer strips one by suffix pattern.
- **fk-b072e820e926.** The gui-feature test pass is a hand copy of `emit_nextest_pass`, and the copies have diverged: it ignores `REIFY_VERIFY_NEXTEST_CONFIG`. The `gen-nextest-config.sh` lazy init is written 3 times. The sidecar `tree_oid` sed extractor appears twice (`add_test_passes`, `confirm_failed_run`).

### 2.2 Role and profile policy is scattered

- **fk-21149d55fd41 (high).** `DF_VERIFY_ROLE` is decided by role comparisons at 16 decision sites. The `merge || background` disjunction is spelled out 5 times.
  - Two cross-file exemptions compare `= "merge"`: `cpu-admit.sh::cpu_admit` and `lib_test_semaphore.sh::test_semaphore_acquire`.
  - The role matrix is asymmetric. background gets merge's profile and forced scope, the task jobserver FIFO, offline's priority, no heavy exclusion and gated admission.
- **fk-03d27a868cfd.** Every per-profile fact is a `_DEBUG/_RELEASE` variable pair: 5 internal pairs and 4 env-override pairs.
  - `emit_nextest_pass` never receives the profile. It recovers it twice with `case "$rel" in *release*`.
- **fk-902a5ad9889e (high).** `test_nextest_slow_priority.sh` derives role → profile → wall semantics from verify.sh's SOURCE TEXT.
  - It uses 8 extractors and splits the accepted role set out of an error-message string.
  - It sed-mutates copies of verify.sh with 5 seeders.
  - Its residual allowlist is read from a comment paragraph in `.config/nextest.toml`. That paragraph now reads `ACCEPTED RESIDUAL — NONE` (#7552), so the comment grammar has zero production rows.

### 2.3 Facts copied from their owners; contracts held by grep

- **fk-73c909381788.** `_RUST_COUPLED_RI_FIXTURES` (40) and `_GUI_COUPLED_RI_FIXTURES` (145) copy facts that live in the tracked `*.rs` and in `reifyGrammarCorpus.test.ts` `EXPECTED_CLEAN`.
  - The two lists were edited in 19 commits since 2026-07-06, and 30 open tasks each owe a registration edit.
  - The comment saying the lists "NEST EXACTLY" is false: 6 Rust-coupled names are absent from the GUI list.
  - `select_harness_kloc_guard` hard-codes a 5-crate literal that `harness-layout-lib.sh::harness_layout_consolidatable_crates` owns.
  - Residual copies kept here, each with its drift guard: `select_cheap_ptodo_gate` vs `ptodo.rs::is_swept_ext` (PT-DRIFT); `pdiag_swept_path` vs `pdiag.rs::is_swept_path` (PDIAG-DRIFT); `pdoccover_input_path` vs `pdoccover.rs` (PDOCCOVER-DRIFT). The hook path has no built `reify-audit` binary to ask instead (§9).
- **fk-fc5ee820d36e.** `verify-pipeline-guard.sh` rebuilds the load-bearing set from 6 routes.
  - Two routes read verify.sh SOURCE: clause 3, `source "$SCRIPT_DIR/…"` lines (nine libs are found by this clause only), and clause 4a, `add`/`add_tool` statements.
  - One route regex-scans a `--print-plan` run (4b). By its own comment, 4b buys nothing today.
  - After a shim, clauses 3 and 4a go silent and the guard **fails open** for those nine libs.
- **fk-d47473e4d8d8.** The scope decision reaches tests only as a prose comment line, and the field order is a contract held by comments.
  - The ordered `RUN_RUST=[01] RUN_GUI=[01]` substring is grepped 89 times in `test_verify_scope.sh`, plus 3 elsewhere.
  - `plan_capture_lib.sh` has no typed field reader. All ten of its functions are ERE matchers.
- **fk-763b633b2775.** The sandbox copy list exists at 8 build sites in 7 files.
  - `copy_list_preflight_lib.sh::derive_source_closure` only asserts the hand lists, never produces them.
  - It cannot see Python imports, so after a shim it passes **vacuously**.
- **fk-7b476f26ad45.** `test_nan_safe_ordering_guard_wired.sh` captures the plan single-shot.
  - 35 of the 50 plan-text tests use raw captures.
  - Its sections hD–hH, about 700 lines, re-test the shared lexer through one consumer.
- **fk-6ccbfc296ee8 / fk-3988a6996b82.** verify.sh rendering contracts live in the wrong members.
  - `test_occt_flock_gate.sh` lines ~129–1000 belong to the gate, not the OCCT wrapper. Its Tests 5 and 7 grep comments only.
  - Part B of `test_compute_trampoline_registration_wired.sh`, about 600 lines, also belongs to the gate.
- **fk-7bc1c1449010.** `test_verify_semaphore_e2e.sh::_ts_ready_check` re-implements `tree-sitter-generate.sh::_stamp_is_current` with 3 of its 6 conjuncts. `tree-sitter-generate.sh` has no `--check` mode.

### 2.4 Defects reproduced in today's executor (scouts, 2026-10-14; Leo Q2)

1. A failing plan step exits **1** whatever its code: 75 and 124 become 1. The cause is errexit inside the reaper's backgrounded `eval`. `scripts/jobserver-acceptance.py` counts step exit 124 and assumes propagation.
2. The offline confirm wrapper `( cmd; _confirm_rc=$?; …record…; exit $_confirm_rc )` never runs its recorder on a red pass, for the same reason.
3. The background sweep ledger records `pass`/`exit_code:0` for failed runs. The cause: the node lane's replacement EXIT trap resets `$?` before `_verify_cleanup` reads it.
4. The MERGE_HEAD force-to-all is evaluated against the caller's cwd before `cd "$REPO_ROOT"`. From a cwd other than the main checkout root, it silently does not fire.
5. A missing option value (`--profile` with no value) dies in bash's `${2:?}` with exit 1, not 64.
6. `--help` is `sed -n '2,80p'` of the script header, and it ends mid-sentence.
7. The `# narrowing —` header embeds the newline-separated crate lists, so it spans N physical lines without `#`.

## 3. Premise checks run at authoring (G3, G6)

1. **Interpreter.** `/usr/bin/python3` is 3.12.3 on both the dev host and the remote runner `leo-laptop` (ssh probe 2026-10-14).
   - `tomllib`, `fcntl`, `signal` and `subprocess` are stdlib.
   - Dark-factory strips the venv from PATH and injects `PYTHONUNBUFFERED=1`. Hooks inject neither, so the shim names `/usr/bin/python3` explicitly and the executor flushes explicitly.
2. **Dark-factory never parses `--print-plan` text at runtime.** This corrects the ruling's premise that DF migrates "to the JSON form" (G6).
   - The only DF parser of plan text is the DF test `orchestrator/tests/test_verify_role_integration.py::_command_lines`.
   - DF's runtime contract with verify is: argv and env in; one merged stdout+stderr stream scanned line by line; the exit code as 0 / non-zero / negative signal; and three files on disk.
   - Those files are `target/reify-verify-attempt.json`, the confirm id list on stdout, and nothing else verify writes.
   - The other `--print-plan` text parsers are reify's own: guard clause 4b, `confirm_failed_run`, and the infra tests.
3. **"Summary/FAILED lines"** (ruling wording) are run_all's, not verify's (G6).
   - verify.sh's own status lines are `verify.sh: + <line>`, `verify.sh: FAILED (exit N): <line>`, `verify.sh: all checks passed (action=… profile=… scope=…).` and `verify.sh: nothing to verify (…) — no commands in plan.`
   - The `=== Summary:` and `FAILED <names>` lines come from `tests/infra/run_all.sh`, relayed through the plan step that runs it. That is S19's seam, and stays untouched here.
4. **"The 67 infra tests that parse verify.sh's source text"** (ruling wording) is 16 test files that read the source directly. Three more do so indirectly through `assert_source_closure_copied` (G6).
   - 67 is the count of `tests/infra/test_*` files that contain the literal `scripts/verify.sh`.
   - The production parser of verify.sh's source is `verify-pipeline-guard.sh` (§2.3).
5. **Plan lines do not rely on errexit mid-line** (all 27 command lines of the widest merge plan were read).
   - Every line is one command, an `&&` chain, an `if … fi`, an explicit `|| exit $?` loop, or `export …; cmd`.
   - So running each step as its own `bash -c '<line>'` with no shell options yields the status the line's author intended. That is what fixes defects 1 and 2 (§2.4).
6. **The semaphore lib already has a standalone CLI** (`lib_test_semaphore.sh CMD…` → `test_semaphore_run`). A `--hold` arm is a small addition that leaves all slot logic in bash.
7. **Upstream substrate** (hard dependency edges, §11).
   - Not yet on main:
     - `scripts/lib_native_ld_path.sh` cargo-target class (S21 γ #8654);
     - `REIFY_VERIFY_EVENT_LOG` and the `slot_timeout` record that `lib_slot_acquire.sh::slot_emit_timeout` writes for every caller (S19 γ #8685).
   - On main since 2026-10-15: `scripts/reify_path_attribution.py` (S22 θ #8662), with `attribute(path) -> PathFacts(is_global, seed_crates, unioned_crates, inert, tree)` and the `attribute [--format tsv|json]` / `closure` CLI.
8. **nextest** is 0.9.136. It UNIONs multiple `-E` filters, which is why the retry composer folds the gate and retry expressions into one `-E` (kept).

## 4. Design (G5: B + H — load-bearing gate, cross-repo consumer, more than 8 mechanisms)

### D1 — One Python package, an exec shim, layered modules

- `scripts/verify.sh` becomes:
  ```bash
  #!/usr/bin/env bash
  exec /usr/bin/python3 -I -B -u "$(dirname -- "${BASH_SOURCE[0]}")/verify.py" "$@"
  ```
  - `exec` keeps verify the process DF signals.
  - `-I` stops an ambient `PYTHONPATH` or user site from changing the gate. It also discards `PYTHONUNBUFFERED`, which is why the shim passes `-u` (D10 stream order).
  - `-B` writes no `__pycache__` into worktrees, which DF's mtime-based no-progress detector and git-cleanliness checks would see.
  - The path resolves from the shim's own location, never from cwd or any `REIFY_*` variable: the guard runs it under `env -u` of every `REIFY_*/DF_*`.
  - The shim stays an executable regular file at `scripts/verify.sh`. DF's `reify_checkout` marker, the hooks, the yaml and the plan lines' `./scripts/verify.sh psi-gate|compile-gate` all keep their path.
  - The shim is the ruled external entry point, not a heuristic-13 re-export shim: external consumers name the path.
- `scripts/verify.py` is the entry point. It puts its own directory on `sys.path`, the repo convention, because `-I` implies `-P`. It then calls `reify_verify.cli.main`.
- Package `scripts/reify_verify/` contains these modules. Imports go only downward: no module imports `cli`, and none imports a peer above it.

| module | purpose | partition of verify.sh it replaces (h14 record) |
|---|---|---|
| `knobs` | knob registry: name, default, parser, one-line meaning; `_resolve_timeout_knob` semantics | header knob prose, `_resolve_timeout_knob` |
| `policy` | `RolePolicy` and `ProfilePolicy` tables (D3, D4) | the 16 role sites, the profile pairs |
| `environment` | the process environment for plan steps (D9), as a frozen `StepEnvironment` | `apply_env`, `_jobserver_owner_live` |
| `scope` | changed paths per scope, `ScopeDecision` (D5), coupled fixture sets (D6) | `decide_scope`, `is_occt_crate`, the two lists |
| `selection` | infra selectors over a `ScopeDecision` | `select_*`, `pdiag_*`, `pdoccover_*` |
| `narrowing` | closure, narrowing, `closure_reaches_reify_gui`, `RUN_GUI_VITEST` | narrowing block |
| `plan` | `Step`, `StepKind`, nextest fragment records, `Plan` | `PLAN`, `add`, `add_tool`, `_LD_SCRUB` |
| `nextest` | the one nextest-pass emitter, filter composition, `nextest_config_path` | `emit_nextest_pass`, gui-feature pass |
| `build` | typed emitters in phase order; release-delta decision; retry marker | `build_plan`, `add_test_passes`, `_derive_merge_delta` |
| `render` | the text and JSON projections, shared by printer and executor (D7, D8) | print loop |
| `execute` | the executor (D10) | execute loop, traps, `_verify_cleanup`, ledger |
| `confirm` | `--confirm-failed` as a direct call over the test-region steps | `confirm_failed_run`, `_confirm_subset_pass` |
| `admission` | `psi-gate` / `compile-gate` actions → `cpu_admit` with explicit parameters | `psi_gate`, `compile_gate` |
| `queries` | `--print-gates`, `--print-scope`, `--print-role-policy`, `--print-knobs` | (new) |
| `bashcall` | the D15 `BashCall` and `DataFile` registries | the `source` lines and inline lib calls |
| `cli` | argv parsing (order-independent, today's messages), usage text, action dispatch | arg loop, `usage` |

No module exceeds ~800 lines. Each module is understandable alone (heuristic 13). Functions take explicit records and return records: a frozen `Invocation` (parsed argv + role + profile + scope) feeds `ScopeDecision`, which feeds `Plan`. There are no module globals (heuristics 5, 7, 8).

### D2 — A plan step is a typed record; kinds are dispatch, the text is a projection

```text
StepKind = CARGO | TOOL | BUILTIN | MAIN_SHELL | SENTINEL
Step(kind, phase, command: <typed command record>, gate_path: str|None,
     guard_file: str|None, annotation: tuple[str, ...])
```

- **The kinds.**
  - `CARGO` reaches cargo and runs with the plan loader path.
  - `TOOL` is any non-cargo program; its render carries the `_LD_SCRUB` prefix.
  - `BUILTIN` is a shell builtin with no loader decision: the `RELEASE-PASS` echo and the `#4624` binary-presence assertion.
  - `MAIN_SHELL` is the node-lane start/join pair.
  - `SENTINEL` is semaphore acquire/release.
- **Kind replaces the conventions.** It replaces the `add`/`add_tool` choice and the 15 `# ld-ok:` markers. Mixed lines (shell + cargo, e.g. the gui-feature check) are `CARGO`, as today.
- **Commands are typed.** A cargo command carries subcommand, selector (`Workspace | Packages(tuple)`), `release`, wall, priority class, a `NextestFlags` record, config file, confirm wrapper and the trailing-FD close.
  - `NextestFlags` holds the heavy filter (`exclude | only | none`), `no_fail_fast`, `test_threads` and the retry subset.
  - The retry subset is composed as a typed filter expression and rendered once. No suffix strips.
- **`render.render_step(step) -> str`** is the only function that produces step text.
  - The printer prints it.
  - The executor echoes it (`verify.sh: + …`) and runs it.
  - The confirm path runs exactly the test-region steps it selects from the `Plan`. No re-invocation, no awk, no marker parsing (the C24 self-consumer disappears).
- **Executor dispatch is by `kind`.** No substring of step text routes anything.
- **`MAIN_SHELL` steps are executed structurally** (D10) and rendered byte-identically for the projection. They are the only steps whose text is not what runs, and the JSON record says so (`"executes": "structural"`).

### D3 — One role table

`policy.ROLE_TABLE: dict[Role, RolePolicy]`. `Role` is an enum parsed once from `DF_VERIFY_ROLE`: empty → `task`; unknown → today's message and exit 64. `RolePolicy` has these named fields, each read by exactly the consumer that used to compare the role:

| field | task | merge | offline | background |
|---|---|---|---|---|
| `default_profile` | debug | both | release | both |
| `prio_class` (CARGO_PRIO incl. degraded-tool warnings) | `nice -n 15 ionice -c 2 -n 7 ` | `nice -n 5 ` | `nice -n 19 ionice -c3 ` | `nice -n 19 ionice -c3 ` |
| `force_scope_all` | no | yes | no | yes |
| `jobserver` | task FIFO | merge FIFO | none | task FIFO |
| `heavy` | exclude-eligible (knob) | exclude-eligible (knob) | only (+`--run-ignored all`) | none (runs all) |
| `fail_fast` | yes | yes | no (`--no-fail-fast`) | yes |
| `records_confirm_manifest` | no | no | yes | no |
| `release_wall_default` | 90m | 90m | 13h | 90m |
| `release_selector` | release-sensitive set | release-sensitive set | `--workspace` | release-sensitive set |
| `infra_tier` | selective | wholesale (run_all pool + pre-builds) | selective | wholesale |
| `attempt_sidecar` | no | yes (unless `failed_only` retry) | no | no |
| `release_delta_skip_eligible` | no | yes | no | no |
| `sweep_ledger` | no | no | no | yes |
| `gui_feature_test_pass` | yes | yes | no | yes |
| `admission_exempt` | no | yes | no | no |

- The values are today's, read from the code (scout A1 §3.2, A2 §2).
- `records_confirm_manifest` is a property of its own. Today the build-time gate for the confirm wrapper is `_OFFLINE_NO_FAIL_FAST` being non-empty, which couples two properties (heuristic 3).
- **`admission_exempt` is passed, never exported (Leo Q3).**
  - `admission` invokes `cpu_admit` with an explicit `_ca_exempt=0|1` parameter beside the other `_ca_*` parameters it already passes. The parameters become explicit assignments in the one `bash -c` that sources `cpu-admit.sh`, instead of bash dynamic scope.
  - The executor's semaphore holder (D10) receives `--admission-exempt 0|1`.
  - The two libs' standalone paths (`scripts/agent-bin/cargo`, `scripts/cpu-governed-exec.sh`, manual CLI) keep `DF_VERIFY_ROLE = merge` as their external contract when the explicit parameter is absent.
  - A behavioural parity test (B5) pins each lib's standalone exemption per role to `admission_exempt`.
  - No ambient flag reaches `run_all.sh`'s task-role subtree or nextest processes.
- `--print-role-policy` prints one JSON line per role: every field, plus the walls resolved under the current env knobs. This replaces the ruling's working name `--print-role-walls`: it prints the whole policy, not only walls (heuristic 1).

### D4 — One profile table

`policy.PROFILE_TABLE: dict[Profile, ProfilePolicy]` with fields:
- `cargo_release_flag`;
- `wall_knob` and default (the role's `release_wall_default` overrides the release default);
- `narrowable` (debug yes, release never);
- `confirm_manifest` / `confirm_sidecar` (default path plus env override name);
- `retry_filter_env` (per-profile name, falling back to the base name).

`nextest.emit_pass(profile: ProfilePolicy, …)` receives the record. No consumer re-derives the profile from a flag string. `PROFILES` comes from `--profile` (or the role default) as a tuple of `Profile`, debug before release.

### D5 — The scope decision is a record

```text
ScopeDecision(run_rust, run_gui, run_occt_gate, run_gui_vitest, gui_path_signal,
              changed_paths: tuple[str, ...], rename_sources: tuple[str, ...])
```

- **Changed paths per scope are gathered exactly as today.**
  - The same git commands run under the hook's own `GIT_*` env, never scrubbed (consumer C18).
  - The branch diff is against `merge-base main HEAD`.
  - A staged primary-diff failure is still fail-open. A rename-source failure still fails wide.
  - *Lead amendment 2026-10-15 (program review-all-reify-20261010; hotspot-survey finding fk-39f88c1738b6, task #9170):* the staged primary diff **fails wide** on a non-zero git status like its three siblings (verify-scope contract C5); #9170 implements it in `gather_changed` after β #9097. The "still fail-open" clause above is superseded.
  - The `.task/` filter stays.
- **`decide_scope(changed, rename_sources, facts)` is pure.**
  - It reads `reify_path_attribution.attribute` over the whole list in one in-process batch (S22 R2; consumer of #8662).
  - Its arm order is re-expressed over `PathFacts`. Two verify-policy arms stay ahead of the fact map, as S22 ruled: the `tests/prd-gate/fixtures/*.ri` coupled-set arm (D6) and the `docs/gui-event-channels.md` arm.
  - A third policy arm is planned elsewhere. spec-conformance's `docs/reify-language-spec.md` escape from the docs no-heavy arm is #6766/#6909's, held behind ζ (Q5), so it lands directly in `scope.py` as one more policy arm ahead of the fact map.
  - Every arm that sets the OCCT gate today keeps it unconditionally (global paths, `tree-sitter-reify/`, Rust-coupled fixtures, `examples/*.ri`, the non-inert catch-all). Only the `crates/*` arm conditions it on the crate being in `occt_declared_set` (critic K19). B1's S22-B14 scenarios pin this.
- **It prints the record.**
  - The `# scope decision — …` and `# narrowing — …` header lines stay byte-identical.
  - `--print-plan=json` carries the record as a `scope` object (D7).
- `affected-crates-lib.sh::reify_is_inert_path` is deleted at cutover (ζ), as S22 θ anticipates.

### D6 — Coupled fixture sets become derived data (Leo Q1)

- **The Rust set is derived at plan time with PG-DRIFT's own predicate.**
  - The predicate: `git grep -h -E 'tests/prd-gate/fixtures/[A-Za-z0-9_.-]+\.ri' -- '*.rs'` minus lines carrying `pg-drift:allow`, basename-extracted.
  - It runs lazily: only when a changed path is a prd-gate fixture.
  - No Rust list exists.
- **The GUI set comes from a data file.**
  - `EXPECTED_CLEAN` moves out of `gui/src/__tests__/reifyGrammarCorpus.test.ts` into `gui/src/__tests__/reifyGrammarCorpus.expected-clean.json`, a JSON array of repo-relative paths.
  - The vitest ledger reads it, and so does the planner, taking the `tests/prd-gate/fixtures/` entries.
  - The rest of the corpus-drift ledger moves into `reifyGrammarCorpusLedger.test.ts` in the same leaf, per #7775.
  - The JSON file is registered in `scripts/verify-pipeline-paths.txt`. It is config-only data that changes scope decisions.
- **The drift guards keep guarding the bash lists until ζ, then become behavioural tests of the planner** (fold-verifier V1).
  - Inside the window, bash `verify.sh` still classifies by its lists. Fixture-registration tasks still register there, so PG-DRIFT (the bash list equals the `.rs`-derived set) and PG-DRIFT-GUI (the bash list covers the JSON pins) stay unchanged in kind. π only switches PG-DRIFT-GUI's source to the JSON.
  - β adds the behavioural tests of the derivation as a Python member running in B1's shared clone:
    - every `.rs`-referenced fixture stages to `RUN_RUST=1`;
    - every JSON-pinned fixture stages to `RUN_GUI=1`;
    - an unreferenced, unpinned fixture stages to no heavy checks.
  - At ζ the bash lists disappear. PG-DRIFT/PG-DRIFT-GUI are then rewritten as behavioural tests of the planner through the shim, and the β member's scenarios fold into them.
  - PG-DRIFT-DIR stays throughout: it forbids an unreviewed directory walk that would defeat per-file derivation.
- **The "nest exactly" claim is dropped.** The two sets are independent and nothing needs them to nest.
- **`select_harness_kloc_guard`'s crate set** comes from one `bash -c` call of `harness_layout_consolidatable_crates`. S19 θ #8700 keeps that public name.

### D7 — Machine-readable plan: `--print-plan=json`

`--print-plan` (text) is unchanged. `--print-plan=json` prints JSON lines with `"v": 1`:
- `header` (action, profile, profiles, scope, include_infra, nextest, role);
- `scope` (the D5 record);
- `narrowing` (narrow_active, affected[], closure[], from_diff);
- `heavy_partition`;
- `env` (the ENV_LINES, structured: name, value, or a reason when left unset);
- one `step` per step: index, kind, phase, gate_path, guard_file, scrub, executes, typed fields, `render` = the exact text line;
- `retry_scope` when the `failed_only` marker would print.

Tests and dark-factory read fields. `render` lets a consumer migrate one field at a time. Readers use stdlib `json`, never bash.

### D8 — Byte-stable projections (the frozen contract until its consumers move)

Byte-identical to today, except the one plan divergence recorded in §12 and the exit codes per D10/D11:
- **`--print-plan` stdout**: retry marker; header; NOTE; scope; narrowing, including its multi-line form until ρ; heavy partition; environment block; commands; the empty-plan comment; region comments; the psi-gate annotation.
- **verify's own stderr lines** (§3 item 3), the `verify.sh: ERROR — …` / `WARNING — …` / `retry refused: …` diagnostics, with the em dash.
- The retry marker on stdout.
- `target/reify-verify-attempt.json` (keys `tree_oid`, `profiles` as a space-joined string, `timestamp`).
- The background ledger line format.
- **`--confirm-failed`**: stdout is the bare id list; the single refusal line `verify.sh: ERROR — confirm refused: …` goes to real stderr; everything else goes to the private log. DF's `_VERIFY_USAGE_MARKER_RE` rejects any `Usage:`/`Options:` line, so the usage text keeps those two header lines.
- **Unchanged by construction**, because the bash libs still emit them: `@@REIFY_CLOCK_*@@`, `@@REIFY_SLOT_TIMEOUT@@`, `lib_test_semaphore.sh: …` and the `cpu_admit` lines.

**Listed divergences** (critic K16, K17). Each is asserted positively in B1 or B2, and none is read by a consumer:
- A malformed timeout knob warns once, where offline today warns twice.
- Knob warnings and missing-lib errors fire after argv parsing, so `--help` works with a bad knob.
- In `--confirm-failed` mode, every diagnostic reaches the private log, including the knob warnings that today leak to the real stderr. The order that guarantees it (fold-verifier V11) is: parse argv; validate, with any refusal on the real stderr; `dup2` the private log over fd 2; then resolve knobs.
- The five new query flags (`--print-plan=json`, `--print-scope`, `--print-role-policy`, `--print-knobs`, `--print-gates`) are accepted, where bash exits 64 on them (fold-verifier V12).
- The holder's pid replaces verify's in `@@REIFY_CLOCK_STOP@@ pid=` and in `REIFY_SLOT_EVENT_LOG` rows.
- The D11 usage text and the defect 4–6 fixes.

New lines must never start with `FAILED `, end in ` FAILED`, begin `error: could not find` or `flock:`, or contain `tree-sitter generate`. Those patterns re-route DF's failure classifier (C2, C7). An internal Python error prints one line, `verify.sh: ERROR — internal: <type>: <msg>`, and exits 1. The traceback goes to stderr only under `REIFY_VERIFY_DEBUG=1`, because a traceback is a STRONG failure marker in DF's excerpts.

### D9 — The step environment

- `environment.build(invocation, policy)` returns a frozen `StepEnvironment`: the env mapping every step gets, plus the ENV_LINES rendering.
- It sources `$HOME/.cargo/env` through one `bash -c`, captures `env -0` afterwards, and adopts the whole delta, not only `PATH` (critic K24). It sets `RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`, and the jobserver `CARGO_MAKEFLAGS` after the owner-liveness probe.
- It removes `BASH_ENV` and `ENV` (critic K14). Each step is now a fresh non-interactive bash, which would read an ambient `BASH_ENV` before every step; today's in-process `eval` never did. No gate path sets either variable (`git grep -w BASH_ENV` over `scripts/`, `hooks/`, the yaml and `tests/infra/` is empty, 2026-10-14).
- It captures `REIFY_AMBIENT_LD_LIBRARY_PATH`, then computes the plan `LD_LIBRARY_PATH` from **`scripts/lib_native_ld_path.sh`'s `cargo-target` class**. That is one `bash -c` call, a consumer of S21 γ #8654. Parity proves it reproduces today's `deps:snap:ambient` order.
- `DF_VERIFY_ROLE` is exported normalised (`gui/vitest.config.ts` reads it). `REIFY_VERIFY_EVENT_LOG` is passed through untouched (S19 Q1 / D5).
- The S21 loader-prepend ratchet's verify.sh exemption goes from 2 to 0 at cutover. No Python code prepends a loader dir.

### D10 — The executor

- **CARGO / TOOL / BUILTIN steps.** Each runs as the argv `["bash", "-c", render]`, never a re-quoted string, with **no shell options**, in its own session (`start_new_session=True`). Its context:
  - env = `StepEnvironment`; cwd = repo root;
  - stdin, stdout and stderr inherited at the fd level;
  - FDs above 2 closed, so the `9<&-` suffix in the text is a harmless no-op.

  Before spawning, the executor writes `verify.sh: + <render>` with one `os.write` to fd 2. A non-zero status prints `verify.sh: FAILED (exit N): <render>` and exits **N** (defect 1 fixed; the documented contract; `jobserver-acceptance.py`'s assumption).
- **Stream order** (critic K13; fold-verifier V10). The contract is the flush rule: every stdout write is flushed before any fd-2 write or spawn. Its mechanism is the first statement of `cli.main`, `sys.stdout.reconfigure(write_through=True)`, so the rule holds for every invocation, including the pre-cutover `python3 -I -B scripts/verify.py` runs. The shim's `-u` is belt and braces, because `-I` discards `PYTHONUNBUFFERED`. B6 asserts, on one merged capture, that the retry marker precedes the first `verify.sh: +` line.
- **MAIN_SHELL.** The node-lane start launches the lane's command (scrub included) in a session the executor owns. The join waits for it and reports a failure with the join step's render, as today. `$_VERIFY_NODE_BG_PID` and the trap text exist only in the projection.
- **SENTINEL.** Acquire spawns a **semaphore holder**: `bash scripts/lib_test_semaphore.sh --hold --admission-exempt 0|1`.
  - The new arm acquires through the unchanged `test_semaphore_acquire` and writes `held` to its stdout. Its stdout is a pipe the executor reads for that single line, so `held` never reaches verify's stdout.
  - It then blocks reading stdin, a pipe whose write end is non-inheritable. On EOF it calls `test_semaphore_release` and exits 0. Its stderr is inherited, so the lib's lines and clock markers reach the stream unchanged.
  - The release must run even under the main-guard's `set -euo pipefail` (fold-verifier V13). The arm installs an EXIT trap calling `test_semaphore_release` immediately after a successful acquire, and reads with `read -r _ || true`. A SIGKILLed verify therefore still yields the `REIFY_SLOT_EVENT_LOG` RELEASE row (B6 asserts it).
  - Acquire failure exits with the lib's code (75/64/1). The executor then prints `verify.sh: FAILED (exit N): test-run semaphore acquire` and exits N.
  - Release closes the holder's stdin and waits for it. If verify dies, the holder sees EOF and releases.
  - Slot logic, clock-stop markers and `slot_timeout` records stay in the bash lib. The executor reimplements no slot acquisition, so it appends no records (S19 Q1 satisfied by construction).
  - The holder's pid replaces verify's in `@@REIFY_CLOCK_STOP@@ pid=` and in `REIFY_SLOT_EVENT_LOG` rows. Both are unparsed; listed in D8.
- **Teardown** (critic K7, K15). A step's session id is its leader's pid. Teardown enumerates every live process whose session id (`/proc/<pid>/stat` field 6) equals a live step's or the lane's, and signals each process group in it. GNU `timeout` `setpgid`s its own group inside the step's session, so killing only the leader's group would miss it.
  - Every session gets TERM at once, lane first; then `REIFY_REAPER_GRACE_SECS` (default 10); then KILL. `REIFY_PROC_REAPER_DISABLE` is honoured.
  - Today's `reaper_kill_pgroup` has the same `timeout` hole, so this is a fix, not a regression.
  - Dark-factory's cgroup kill stays the backstop: its 5 s grace is shorter than ours. Since the lane leaves verify's own process group, DF's `killpg` reaches npm only through this handler or the cgroup.
- **Signals.** TERM/INT/HUP handlers run teardown, release the holder, write the ledger with the signal's code, remove the generated nextest config, and exit **143/130/129**. An untrapped Python exit (−15) would flip DF's classification to INFRA_KILL (C1).
- **Ledger and cleanup.** The background ledger records the true exit code (defect 3 fixed). Cleanup runs once, on every exit path.
- **The instrumentation ledger (#7424, landed 2026-10-15).** The executor reproduces the `_vl_*` run-journal layer exactly:
  - when it activates: execute mode only, a non-empty plan, `REIFY_VERIFY_RUN_ID` unset, the harness-ancestor refusal, `REIFY_VERIFY_LEDGER_DIR`;
  - the `plan.nul` it writes;
  - the per-step journal events and snapshots;
  - the finalize call that reports a failure on verify's stderr.

  It talks to `scripts/verify-ledger.py` through that script's CLI, as today. Feeding the typed `phase` to the ledger instead of letting `verify-ledger.py` re-parse step text is #9035's change, held behind ζ (§11).
- **Confirm.** `--confirm-failed` is a separate, non-fatal runner (critic K1). It never routes through `run_step`'s exit-on-failure.
  - After validation it saves the real stdout and stderr on two non-inheritable fds, then `dup2`s the private log over fd 2. Diagnostics from that point on reach the log, as today.
  - For each profile in `PROFILES` it builds its own `Invocation(action=test, profile=P, scope=all, include_infra=0)`, under the confirm env overrides today's child receives (`REIFY_VERIFY_RETRY_SCOPE=failed_only`, the manifest as both filter files, the tree OID, the scratch sidecar/manifest paths, `REIFY_VERIFY_NEXTEST_CONFIG`). It builds that plan in-process; there is no re-invocation and no awk.
  - It runs only the `CARGO` steps between the region sentinels. Sentinels are not executed, and the psi/compile gates precede ACQUIRE, so they are outside the region.
  - Each step runs through `run_step_logged`: the child's fd 1 and fd 2 both go to the private log, the status is recorded and never fatal, and there is no `verify.sh: +` echo. This reproduces `( eval "$_line" ) >>"$_CONFIRM_LOG" 2>&1 || true`.
  - The JUnit extract and the id list follow. The ids are printed on the saved real stdout through the same `grep . | sort -u` subprocess pipeline as today, so ordering stays in the caller's locale.
  - The single refusal line goes to the saved real stderr. Exit codes are 0 / 100 / 64 as today.
  - The confirm wrapper's recorder now runs on a red pass (defect 2 fixed).
  - The sidecar `tree_oid` reader is one function, shared with the retry precompute.
- **Admission.** `psi-gate` / `compile-gate` resolve their knobs in `knobs`, then call `cpu_admit` (D3). They stay execute-only and ignore `--print-plan`. They run before `cd`, MERGE_HEAD and environment, as today.
- **MERGE_HEAD** is resolved against the repo root (defect 4 fixed). Hooks already run at the root, so production behaviour is unchanged.

### D11 — CLI

- Order-independent tokens, the same actions and flags, and today's message strings.
- A missing option value prints `verify.sh: ERROR — --profile requires a value` and exits **64** (defect 5).
- Usage text is data in `cli`. It is complete (defect 6), keeps the `Usage:` and `Options:` header lines, and on errors goes to stderr after the ERROR line, as today.
- New flags: `--print-plan=json`, `--print-scope`, `--print-role-policy`, `--print-knobs`, `--print-gates`. Each takes the same validation path, and each is added to the exit-64 unknown-argument tests' expectations.
- `--print-scope` prints one JSON line holding the D5 `ScopeDecision` and the narrowing record, using the same serialiser `--print-plan=json`'s `scope` and `narrowing` records use. β delivers it (critic K2). Scope tests can read a decision without building a plan.

### D12 — `--print-gates`, and the guard reads it

- `queries.print_gates()` prints one repo-relative path per line, sorted unique. The list covers:
  - every `gate_path` an emitter can produce, **declared by the emitter registry** (each emitter declares its gate paths statically, so no branch is missed);
  - every planner input: the package's own modules, `scripts/verify.py` and `scripts/reify_path_attribution.py`, plus the data files and bash libs derived from the D15 `DataFile` and `BashCall` registries (never a hand list).
- B3 checks completeness over the parity corpus: every rendered step's `gate_path` is listed.
- `verify-pipeline-guard.sh` pre-cutover (κ; critic K11):
  - It gains `_print_gates_cmd`, defaulting to `/usr/bin/python3 -I -B "$SCRIPT_DIR/verify.py" --print-gates`, run under the same scrubbed env and timeout as 4b. Bash `verify.sh` has no `--print-gates` until ζ.
  - `REIFY_VERIFY_PIPELINE_GUARD_PRINT_GATES` overrides it for fixtures, with a script that prints paths.
  - `REIFY_VERIFY_PIPELINE_GUARD_VERIFY_SH` keeps serving clauses 3/4a until ζ deletes them.
  - Clause 4b is deleted at κ. It is the only route that reads the bash plan, and from γ1 on B1 binds the bash plan to the Python plan, so the `--print-gates` route covers it inside the window.
  - The `scripts/gui-vitest-run.sh` workaround row and its comment block leave `verify-pipeline-paths.txt` (#7645): the derivation names the path itself.
- **`--print-gates` fails closed** (fold-verifier V6). This route never inherits clause 4b's fail-soft semantics. If `_print_gates_cmd` exits non-zero, times out, or prints nothing, the guard reports requires-full-gate (exit 0) for every path and logs `verify-pipeline-guard.sh: --print-gates failed (<reason>) — requiring the full gate` to stderr. Before ζ it is unioned with clauses 3/4a; after ζ it is the derivation, and a broken planner must never make its own inputs fast-path-safe.
- At cutover (ζ) it deletes clauses 3 and 4a and the `_SH_PATH_ERE` extractor. `_print_gates_cmd` is then the derivation, with the three manifests and the infra glob unchanged.
- It moves its measurement prose to this PRD (§2.3) and the commit history.
- S22 R2's `verify-pipeline-paths.txt` row for `scripts/reify_path_attribution.py` is met by derivation: it is a listed planner input. A `.py` diff never takes DF's config-only fast path anyway.

### D13 — Port window, parity and cutover

- **The port window opens when the plan-parity member lands (γ1)** and closes at cutover (ζ).
  - Leo Q5 and its follow-up: every open task that changes what `scripts/verify.sh` plans or runs waits behind ζ through a hard dependency edge, and is implemented once, in Python. §11 lists them. Each carries a one-line note naming the window.
  - The S18 chain up to and including ζ is pinned in the scheduler, so the window stays short.
  - Fixture-registration tasks are not held. Their only verify.sh touch is a list entry; the D6 derivation reproduces it, and bash PG-DRIFT keeps guarding the lists through the window (D6).
  - Exceptions, ruled by Leo in the follow-up: #7424 is in flight (19 commits) and cannot be parked cheaply mid-branch. #8919 is a one-line `env -u` that S19's dark-factory ε needs before it turns rule 4 on, so holding it would delay ε by the whole S18 chain at no port saving. Both become prerequisites of γ1, and the port carries their behaviour.
  - Any change that still lands inside the window must make the same change in `scripts/reify_verify/` in the same diff. The parity member reds otherwise, and its failure message names this rule and the module to edit (heuristic 10, informatively).
- **Plan parity (B1).** Old (`bash scripts/verify.sh`) and new (`python3 -I -B scripts/verify.py`) are compared on `--print-plan` stdout, stderr diagnostics and exit code.
  - **Where it runs** (critic K9, K10; fold-verifier V2). Both sides run in one shared clone of **the tree under test**, built in three steps:
    1. `git clone --shared` of the checkout into a scratch dir, with a local `main` ref.
    2. Apply `git diff --binary HEAD`, the tracked worktree against HEAD, and commit it in the clone. This is what makes a pre-merge-commit hook, a staged pre-commit and any uncommitted verify test the candidate rather than HEAD. Never use `GIT_INDEX_FILE` against the shared index.
    3. Changed-path scenarios stage or commit scratch changes on top, with `git -c core.hooksPath=/dev/null` and the git env scrubbed.
  - Both sides therefore read the identical full tree: the real `*.rs` the D6 derivation greps, the real GUI-pins JSON, every lib and data file. No copy list exists, and B1 does not depend on ι.
  - A B1 self-test makes an uncommitted edit to a `render` function and asserts it shows up as a divergence.
  - **Budget and sharding** (critic K20). The member is sharded by action group into one member per group. The `scripts/verify.sh  tests/infra/test_verify_*.sh` infra-map row selects each member at task tier whenever verify.sh or the package changes. That is deliberate: the window rule needs it there. Each member must stay inside the selective-infra leaf's 10-minute wall with margin; γ1 measures it and shards further if not.
  - The corpus has two parts:
  - **The full grid.** Every action {test, lint, typecheck, all} × role {task, merge, offline, background} × profile {debug, release, both, implicit} × scope {all, staged, branch}, with `--include-infra`, in the clone.
  - **Targeted scenarios.**
    - The `test_verify_scope.sh` changed-path fixtures. This is S22's B14.
    - `REIFY_GATE_EXCLUDE_HEAVY=1`, `REIFY_RELEASE_DELTA_SKIP=1` with an override, a `failed_only` retry with filter files, `REIFY_GUI_RETRY_SPECS` valid and invalid, `--test-threads`, `--narrow` and `REIFY_AFFECTED_CRATES_OVERRIDE`.
    - nextest absent (PATH farm) and nextest present-but-failing.
  - It is a Python unittest member. Its expected divergences are exactly D8's listed divergences and §12's one plan divergence, each asserted positively; any other difference reds.
- **Executor parity (B2).** Both implementations run on stub `cargo`/`npm` PATH farms, covering pass and fail at every step kind, holder contention, gate subcommands, TERM mid-step, `--confirm-failed` with stub JUnit, and the background ledger.
  - Compared: the projected stderr/stdout lines (pids and timestamps normalised) and the exit code.
  - The expected divergences are D10's fixes to defects 1–4 plus D8's executor-side listed divergences: confirm-mode diagnostics to the log, the single knob warning, the holder pid (normalised). Each is asserted positively (fold-verifier V17).
- **Cutover (ζ) is one commit under the full gate.**
  - The shim replaces the bash body.
  - B1 and B2 retire with the old implementation.
  - The guard and sandbox inventories switch to `--print-gates`.
  - The two bash libs' explicit-parameter paths (ε) become the only path verify uses.
  - Everything the bash body owned is deleted.

### D14 — Tests follow the infra-test policy

- **New assertions are Python** (stdlib `unittest`, a `.sh` wrapper, a `run-all-classification.manifest` row).
- **Infra-map rows** (critic K18). Each `scripts/reify_verify/*.py` module and `scripts/verify.py` get the same `tests/infra/test_verify_*.sh` glob row that `scripts/verify.sh` has today, plus their specific rows. Task-tier selection must not narrow when logic moves out of verify.sh.
  - *Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-5):* this hand-row clause stays in force through the port window and **ends at leaf τ**. τ replaces `scripts/verify-pipeline-infra-tests.txt` with a `# guards:` header in each `tests/infra` member, which `reify_verify.selection` reads at plan time.
- **Moved bash blocks stay bash** (S19 R-3 precedent, D7 there).
- **The 16 source-reading files.** Their source greps are replaced by Python structured-query tests over `--print-plan=json`, `--print-role-policy`, `--print-knobs` and executor behaviour. They are never replaced by imports of package internals (stance: tests drive public seams).
- **Hard-coded pass counts.** `test_verify_nextest_absent_suites.sh` hard-codes minimum pass counts for eight sibling suites (fold-verifier V9): compile_gate, semaphore_wiring, offline_partition, semaphore_e2e, verify_scope, failfast_order, occt_gated_scope and retry_subset. A leaf that changes the assertion count of one of them updates its count in the same diff.
- **Census row** (G7, design-invariants census, row format owned by S24):
  - Surface: "verify knob registry and role/profile policy fields".
  - Gate: `test_verify_policy.py` — every field appears in `--print-role-policy`, and every knob is documented in `docs/notes/verify-pipeline-knobs.md`. Consumption itself is convention-only.
  - Status: shipped with α.

### D15 — Bash seams are typed calls, and `--print-gates` derives from them (critic K8)

Every bash library function the planner or executor needs is one `BashCall` record (lib path, function, argv) in a registry. It is invoked as one `bash -c 'source "$1"; …' _ <lib>` fork, with `BASH_ENV` and `ENV` removed from its environment, as for steps and the holder (fold-verifier V14).

Two inputs feed `--print-gates`' "bash libs invoked" and "data files read":
- the registry;
- the `DataFile` records (each data file a module reads).

There is never a hand list, so a call the planner makes cannot be missing from the guard's set (B3 checks this). In-process reimplementation is allowed only for verify's own logic, never for a fact a lib owns (heuristic 11).

| seam | form | leaf |
|---|---|---|
| `heavy-test-filter-lib.sh` → `REIFY_HEAVY_NEXTEST_FILTER` (the 8-atom literal) | `BashCall` (source, print the exported value) | γ1 |
| `lib_git_env_scrub.sh::reify_git_env_scrub_prefix` (the `env -u GIT_DIR …` prefix inside the selective-infra step) | `BashCall` | γ1 |
| `affected-crates-lib.sh::affected_crates` (cargo metadata, closure, `sort -u` in the caller's locale, its stderr fall-back line) | `BashCall`, one call over the whole path list (it also keeps serving pre-commit and #8662) | β |
| `occt-scope-lib.sh::occt_declared_set`; `release-scope-lib.sh::release_declared_set`, `release_delta_requires_pass` | `BashCall` | β (occt), γ2 (release) |
| `harness-layout-lib.sh::harness_layout_consolidatable_crates` | `BashCall` | β |
| `lib_native_ld_path.sh` cargo-target class; `$HOME/.cargo/env` | `BashCall` | γ1 (environment) |
| `lib_main_checkout.sh::reify_main_checkout` (ledger path) | `BashCall` | ε |
| `cpu-admit.sh::cpu_admit` with explicit `_ca_*` parameters | `BashCall` | ε |
| `lib_test_semaphore.sh --hold` | holder subprocess (D10) | ε |
| `gen-nextest-config.sh`, `confirm-failed-manifest.sh`, `tree-sitter-*.sh` and the other gate scripts | script subprocess / plan step `gate_path` | γ2, ε |
| nextest availability probe (`cargo nextest --version`, 3 retries, `REIFY_NEXTEST_PROBE_RETRY_SLEEP`, the exact ERROR line) | in-process (verify's own logic, same messages) | γ1 |
| `REIFY_INFRA_SUITE_ACTIVE` re-entrancy guard on the wholesale and selective infra phases | in-process (knob in `knobs`) | γ1 |

## 5. Boundary tests (H)

| # | seam | test | asserts |
|---|---|---|---|
| B1 | old ↔ new planner | `tests/infra/test_verify_plan_parity.py` (γ1, γ2; retired at ζ) | byte-identical `--print-plan` stdout/stderr and exit code over the D13 corpus |
| B2 | old ↔ new executor | `tests/infra/test_verify_executor_parity.py` (ε; retired at ζ) | identical projected lines and exit codes, except the four asserted fixes |
| B3 | planner ↔ guard | `test_verify_pipeline_guard.sh` + `test_verify_print_gates.py` (κ) | every corpus step's `gate_path` and every `BashCall`/`DataFile` input ∈ `--print-gates`; `requires-full-gate` exits 0 for each listed path; a fixture printer's extra gate is reported; a failing, timing-out or empty printer makes `requires-full-gate <any path>` exit 0 (fail closed) |
| B4 | inventory ↔ sandbox | `test_copy_list_preflight.sh` (ι, ζ) | a `copy_verify_sandbox` sandbox renders the same `--print-plan` as the repo for a fixed scenario; a missing inventory file reds by name |
| B5 | role table ↔ bash libs | `test_verify_policy.py` (ε) | for each role, standalone `cpu_admit` bypass and `test_semaphore_acquire` bypass ⇔ `admission_exempt`; verify's explicit parameter overrides the role |
| B6 | verify ↔ dark-factory stream | `test_verify_executor_contract.py` (ε) | TERM/INT/HUP → 143/130/129 after cleanup, with no process left whose session id was a step's (a `timeout`-wrapped step included); no new line matches the D8 forbidden patterns; confirm stdout purity: a stub nextest that prints per-test lines and `Summary` and exits 100 yields a bare id list and exit 100 (K1); the retry marker precedes the first `verify.sh: +` line in one merged capture (K13); the holder's `held` never reaches stdout; sidecar keys; `REIFY_VERIFY_EVENT_LOG` reaches a step unmodified and the selective-infra leaf unsets it (#8919) |
| B7 | coupled sets ↔ scope | until ζ: PG-DRIFT / PG-DRIFT-GUI in `test_verify_scope.sh` guard the bash lists (π switches PG-DRIFT-GUI's source to the JSON); β's `test_verify_scope_record.py` tests the Python derivation in B1's shared clone; at ζ: PG-DRIFT/-GUI become behavioural tests of the planner | the bash list equals the `.rs`-derived set and covers the JSON pins (window); the derived Rust set and the JSON pins classify as D6 says; vitest and the planner read the same JSON |

## 6. Cross-PRD relationship and seam ownership (G4)

| seam | owner | this PRD |
|---|---|---|
| `scripts/verify-pipeline-paths.txt` and the config-only fast path | **S18** | registers the GUI-pins JSON (π); the conditional `crates/reify-build-utils/native-deps.toml` row at cutover, if #8634 has landed and the guard does not already require the full gate for it (ζ); the guard reads `--print-gates` (κ, ζ) |
| dark-factory consumers of verify-emitted lines and `DF_VERIFY_ROLE` | **S18** | D8 contract; the DF companion task (§11) migrates DF's one text-plan parser (a test) to `--print-plan=json` |
| `tests/infra/plan_capture_lib.sh` | **S18** | typed readers (θ); the lib is never edited by S19 (S19 ο adds a case to its self-test only) |
| `scripts/reify_path_attribution.py` | S22 (#8662) | consumer: `decide_scope` reads `attribute` in one batch; S18 deletes `reify_is_inert_path` (ζ) |
| `scripts/lib_native_ld_path.sh` | S21 (#8654) | consumer: `environment` reads the `cargo-target` class; the loader ratchet exemption goes 2 → 0 (ζ) |
| `REIFY_VERIFY_EVENT_LOG`, `lib_slot_acquire.sh::slot_emit_timeout` | S19 (#8685) | passes the var through; reimplements no slot acquisition (D10) |
| `tests/infra/run_all.sh`, run_all-emitted Summary/FAILED lines, `run-all-classification.manifest` format | S19 | not edited; new members add rows in today's format |
| `test_slot_timeout_marker.sh` D_ROSTER/G_MEMBERS | S19 (#8694 deletes D–G) | `test_verify_semaphore_e2e.sh` is a roster member, so its partition (ο) waits for #8694 |
| `tests/infra/harness-layout-lib.sh` public names | S19 (#8700 keeps them) | consumer of `harness_layout_consolidatable_crates` |
| `scripts/verify-pipeline-infra-tests.txt` main.rs row | S27 φ #8776 (held) | file-lock contention only |
| `docs/legibility/design-invariants.md` row format | S24 | adds one census row (α) |
| `scripts/cpu-admit.sh`, `scripts/lib_test_semaphore.sh` | unlisted | ε adds the explicit-exemption parameter and the `--hold` arm; standalone contracts unchanged |
| `scripts/lib_proc_reaper.sh` | unlisted | Layer A moves into the executor; σ retires the bash Layer-A functions; `reap-orphans` (Layer B) unchanged |
| `scripts/tree-sitter-generate.sh` | unlisted | λ adds `--check` |
| `gui/src/__tests__/reifyGrammarCorpus.test.ts` | unlisted (S26 owns generated wire types, not this file) | π moves the ledger and `EXPECTED_CLEAN` |

## 7. Pre-conditions

- These land before the leaves that port their behaviour (§11 edges); the port carries their behaviour and does not race them:
  - #8654 (S21 γ);
  - #8662 (S22 θ, after #7491);
  - #8685 (S19 γ);
  - #8919 (the selected-infra leaf unsets `REIFY_VERIFY_EVENT_LOG`, in bash);
  - #7424 (in-flight verify instrumentation).
- #8476 (in flight; restructures `test_verify_scope.sh`) lands before π, ι and ξ edit that file.
- #8694 (S19 η) lands before ο.
- Every other open task that changes `scripts/verify.sh` behaviour waits behind ζ (D13, Leo Q5; §11 lists them).

## 8. Out of scope

- `tests/infra/run_all.sh` and every run_all-emitted line (S19).
- Dark-factory runtime classifiers (they keep reading the byte-stable stream).
- Retiring the text `--print-plan`. It stays the human projection, and the tests that pin it may migrate to fields over time.
- Asking `reify-audit` for its swept-extension and path predicates at plan time. There is no built binary on the hook path; the three guarded copies stay (§2.3) — follow-up candidate, not filed.
- The retry-spec file consolidation of DF's six retry env vars (DF candidate D-g in the scout record), and the per-test result file (D-e).

## 9. Follow-up candidates (recorded, not filed)

- A single `reify-audit --print-scope-predicates` data export, so `selection` can drop its three guarded copies once a cheap build path exists.
  - *Filed 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-10 (1)) in a cheaper form as leaf φ:* one data file `crates/reify-audit/scope-predicates.toml` that reify-audit `include_str!`s and `selection` reads as a D15 `DataFile`. It needs no binary.
- A typed retry-spec file replacing DF's six retry env vars (DF D-g) and a structured `--confirm-failed --format=json` (DF D-f).

## 10. Decomposition plan

Every leaf carries `metadata.x_finding_run = review-all-reify-20261010`, `metadata.source = review-all`, `priority` = severity, and file-level `metadata.files`.

Every leaf touches verify-pipeline files, so each pays the full gate per landing. π's `gui/src/__tests__` edits are not verify-pipeline files, but its `verify-pipeline-paths.txt` row and PG-DRIFT-GUI edit are.

"Python member" = stdlib unittest + `.sh` wrapper + `run-all-classification.manifest` row + `verify-pipeline-infra-tests.txt` rows (D14).

- **α #9095 — Package skeleton, CLI, knob registry, role and profile tables** · high · keys `fk-21149d55fd41`, `fk-03d27a868cfd`.
  - Scope: `scripts/verify.py`; `scripts/reify_verify/{__init__,cli,knobs,policy,queries}.py` (`--print-role-policy`, `--print-knobs`); usage text as data (D11); `docs/notes/verify-pipeline-knobs.md` gains every registry knob.
  - Tests: Python member `test_verify_policy.py`, which compares CLI validation messages and exit codes against bash for the whole argument-error corpus (the defect-5 divergence asserted). Plus the census row (D14).
  - Not wired: `scripts/verify.sh` is unchanged.
  - Signal: `python3 -I -B scripts/verify.py --print-role-policy` prints the D3 table, whose rows equal what today's plans show per role. `--help` ends at the last option.
- **π #9096 — EXPECTED_CLEAN becomes JSON; corpus ledger in its own file** · medium · no own key; subsumes #7775.
  - Scope: `gui/src/__tests__/reifyGrammarCorpus.expected-clean.json`, `reifyGrammarCorpusLedger.test.ts`; `reifyGrammarCorpus.test.ts` loses the ledger; PG-DRIFT-GUI reads the JSON through `python3 -I` (no sed over TS); `verify-pipeline-paths.txt` row for the JSON.
  - Signal: `scripts/gui-test.sh -- src/__tests__/reifyGrammarCorpusLedger.test.ts` green with the same 405 pins; PG-DRIFT-GUI green.
  - Depends on #8476.
- **β #9097 — Scope decision, selectors, narrowing** · medium · key `fk-73c909381788`.
  - Scope: `scope`, `selection` and `narrowing` modules (D5, D6), and `--print-scope` (D11).
  - D15 `BashCall`s: `affected_crates` (one call over the path list), `occt_declared_set`, `harness_layout_consolidatable_crates`.
  - A Python member `test_verify_scope_record.py` tests the derivation behaviourally (B7) in B1's shared clone of the tree under test, through `--print-scope`. Scenarios cover an `.rs`-referenced fixture, a JSON-pinned fixture, an unreferenced one, and a `pg-drift:allow` control (critic K9).
  - PG-DRIFT/PG-DRIFT-GUI in `test_verify_scope.sh` are NOT rewritten here: they keep guarding the bash lists until ζ (fold-verifier V1).
  - Subsumes #9056: one batched `attribute` call instead of one per catch-all path. Its other half, the deletion of `reify_is_inert_path`, is ζ's.
  - Prose references to the two list names in tracked docs repointed to D6 (C §6.8 inventory); the `crates/**` doc comments move at ζ.
  - Signal: in B1's shared clone of the tree under test, staging an `.rs`-referenced prd-gate fixture, `python3 -I -B scripts/verify.py all --scope staged --print-scope` prints `"run_rust": 1` with no list edit. An unreferenced, unpinned fixture prints `"run_rust": 0, "run_gui": 0`.
  - Depends on α, π, #8662.
- **γ1 #9098 — Typed plan model, renderers, non-test emitters, plan parity** · high · key `fk-507b17f8adc7`.
  - Scope: `plan`, `render`, `build` (all phases except the test region), and `environment` (D9).
  - D15 `BashCall`s: `lib_native_ld_path.sh`, `$HOME/.cargo/env`, the heavy filter, the git-env scrub prefix. The nextest probe and the `REIFY_INFRA_SUITE_ACTIVE` guard are in-process.
  - Delivers `--print-plan` and `--print-plan=json` for actions lint and typecheck, with every header and environment line.
  - Tests: Python member B1 `test_verify_plan_parity.py`, sharded by action group, running in a shared clone of the tree under test (D13). It covers the grid for lint/typecheck plus all header and environment lines. **Opens the port window.**
  - Signal: B1 green on every lint/typecheck scenario, and its message names the window rule when it reds.
  - Depends on α, β, #8654, #8919, #7424.
- **γ2 #9099 — Test region: the one nextest emitter** · medium · key `fk-b072e820e926`.
  - Scope: `nextest` (profile record, typed filter composition, `nextest_config_path` — override / print placeholder / memoised generate — the gui-feature pass built from the same emitter, the confirm wrapper); `build` test region (semaphore sentinels, release-delta decision through the `release_declared_set` / `release_delta_requires_pass` `BashCall`s, attempt sidecar, retry marker); B1 extended to test/all.
  - Signal: B1 green across the full D13 corpus. One `gen-nextest-config.sh` call site in the package. The gui-feature pass honours `REIFY_VERIFY_NEXTEST_CONFIG` (the divergence today's hand copy carries is fixed, with that scenario as B1's single listed plan divergence).
  - Depends on γ1.
- **κ #9100 — `--print-gates` and the guard reads it** · medium · key `fk-fc5ee820d36e`; subsumes #7645.
  - Scope: `queries.print_gates` (D12): emitter-declared gate paths plus the planner inputs derived from the D15 registries.
  - Guard changes, all per D12:
    - `verify-pipeline-guard.sh` gains `_print_gates_cmd` and the `REIFY_VERIFY_PIPELINE_GUARD_PRINT_GATES` fixture override;
    - it drops clause 4b;
    - the `scripts/gui-vitest-run.sh` workaround row and its comment leave `verify-pipeline-paths.txt` (#7645);
    - the guard's measurement prose moves out.
  - Fixtures in `test_verify_pipeline_guard.sh`:
    - It deletes only the 4b fixtures (`_NOFORK_VERIFY`, `_LIBLESS_VERIFY_E`) and keeps `_SYNTH_VERIFY`, which tests clause 3 until ζ (fold-verifier V3).
    - It adds a fixture gate-printer driven through `REIFY_VERIFY_PIPELINE_GUARD_PRINT_GATES`: an extra printed gate is reported; a printer that exits 1, times out or prints nothing makes `requires-full-gate <any path>` exit 0 (D12 fail-closed, V6).
  - New Python member `tests/infra/test_verify_print_gates.py` (fold-verifier V8): every step `gate_path` in the B1 corpus is ⊆ `--print-gates`, and so is every `BashCall`/`DataFile` input.
  - Signal: with the workaround row gone, `bash scripts/verify-pipeline-guard.sh requires-full-gate scripts/gui-vitest-run.sh` still exits 0 (via `--print-gates`), and `--list` names that path exactly once. `requires-full-gate scripts/reify_verify/plan.py` exits 0.
  - Depends on γ2.
- **ε #9101 — The executor** · high · no own key (the executor half of fk-507b17f8adc7's dispatch-on-kind).
  - Scope: `execute` (kind dispatch, session-wide teardown, holder, signals, ledger via the `reify_main_checkout` `BashCall`), `confirm` (the separate non-fatal runner), `admission` (D10). Plus `lib_test_semaphore.sh --hold --admission-exempt` and `cpu_admit`'s explicit `_ca_exempt`.
  - Tests: executor parity B2, contract B6, role-lib parity B5.
  - Carries #7424's step-timing hook and #8919's unset (already ported by γ1/γ2 where they are plan text). `REIFY_VERIFY_EVENT_LOG` passes through.
  - Signal (critic K3):
    - The PATH farm (fold-verifier V18): every stubbed command exits 0 except `cargo nextest run`, which exits 124; `REIFY_NEXTEST_PROBE_RETRY_SLEEP=0`; a private semaphore lock dir (`REIFY_TEST_SEMAPHORE_LOCK`); and admission off (`REIFY_PSI_GATE_DISABLE=1`, `REIFY_COMPILE_GATE_DISABLE=1`).
    - On that farm, `python3 -I -B scripts/verify.py test --scope all` exits 124, and its last stderr line is `verify.sh: FAILED (exit 124): …`. Today's bash exits 1 on the same farm.
    - A run TERM'd during a `timeout`-wrapped step exits 143, and no process whose session id was that step's survives.
  - Depends on γ2, #8685, #7424.
- **η1 #9103 — Role policy replaces the slow-priority shadow parsers** · high · key `fk-902a5ad9889e`.
  - Scope: new Python member `test_verify_role_policy.py`. K/L compare `--print-role-policy` rows with `.config/nextest.toml` read by `tomllib`. The 8 extractors, 5 seeders and the residual comment grammar (`_residual_role_documented`, `RESIDUAL_PAR_ANCHOR`) are deleted from `test_nextest_slow_priority.sh`. `test_occt_flock_gate.sh` T10's two `_resolve_timeout_knob` greps move to the same member.
  - Signal: `grep -c '_for_file()' tests/infra/test_nextest_slow_priority.sh` drops by 8. Changing a role's wall in `policy` reds the member with the role named.
  - Depends on γ2.
- **η2 #9104 — The remaining source-reading greps become structured queries** · medium · no own key.
  - Scope: new Python member `test_verify_plan_structure.py` replaces the source greps in:
    - `test_verify_ld_library_path_scope.sh` (the `ld-ok` census → each `CARGO` step unscrubbed / `TOOL` step scrubbed; export-between-build-and-exec → executor ordering);
    - `test_verify_semaphore_wiring.sh`, `test_verify_test_threads.sh` 6d, `test_cargo_incremental_lane_decision.sh`, `test_jobserver_balancer.sh`, `test_cpu_admit.sh` W1, `test_verify_compile_gate.sh`;
    - `test_proc_reaper.sh` Part 4 (→ executor SIGTERM behaviour);
    - `test_gui_vitest_rpc_hardening.sh`, `test_verify_admission_knob_parity.sh` (→ `--print-knobs`);
    - `test_verify_release_delta_skip.sh` (function extraction → a JSON `release_delta` field).
    (`test_verify_gui_feature_check.sh`'s hand-rolled preflight goes to ι; `test_verify_pipeline_guard.sh` to κ; `test_occt_flock_gate.sh` T10 to η1; `test_verify_scope.sh` B-KLOC to ζ.)
  - Updates `test_verify_nextest_absent_suites.sh` counts in the same diff.
  - Signal: each listed file's source-reading construct is gone and has a named case in `test_verify_plan_structure.py`; mutating the corresponding planner behaviour (e.g. making one `TOOL` step unscrubbed) reds that case.
  - Depends on γ2, ε.
- **θ #9105 — Typed plan field readers in plan_capture_lib** · medium · key `fk-d47473e4d8d8`; subsumes #7647.
  - Scope: `plan_header_field <dump> <line-kind> <NAME>` (order-independent key=value token reader for the `# verify.sh plan`, `# scope decision`, `# narrowing` and `# heavy partition` lines — one parser at the boundary) and `plan_step_index <dump> <ERE>` for ordering claims. Adopted in `test_nextest_absent_lib.sh` and `test_tree_sitter_pipeline.sh` (`ts_offset_of` → step indices). The six stale line cites become symbol cites.
  - Ratchet row `ordered-scope-grep | RUN_RUST=[01] RUN_GUI=[01] | tests/infra/test_*.sh | <post-θ count> | θ`. The ERE uses a real bracket class: the escaped form matches nothing (fold-verifier V5). The count is 92 at authoring; ξ lowers it to 0.
  - Signal: inserting a new field at the front of the scope header in a fixture dump leaves every θ-adopted assertion green.
  - No S18 dependency: the readers parse today's header text (critic K21).
- **θ2 #9106 — One capture form; nan_safe through it** · medium · key `fk-7b476f26ad45`; subsumes #8440.
  - Scope: `test_nan_safe_ordering_guard_wired.sh` captures through `capture_print_plan` + `plan_capture_complete` with positive completeness checks; hD–hH trimmed to a wiring smoke (the lexer matrix stays in `test_rust_production_view_lib.py`). Ratchet row `raw-print-plan-capture | --print-plan[^|]*[|] *grep | tests/infra/test_*.sh | <post-θ2 count> | θ2`.
  - Signal: an empty capture now reds the file (mutation-validated).
- **θ3 #9107 — Remaining raw captures** · low · no own key.
  - Scope: the other raw `--print-plan |` captures move to `capture_print_plan`; ratchet ceiling → 0.
  - Signal: the ratchet row reads 0.
  - Depends on θ2.
- **ι #9108 — `copy_verify_sandbox`** · medium · key `fk-763b633b2775`; subsumes #7002 and #7785.
  - Scope: `copy_list_preflight_lib.sh::copy_verify_sandbox <dest>` copies `derive_source_closure` plus a declared data-file list (occt/release crate lists, the infra map, `gen-nextest-config.sh`, `.config/nextest.toml`, the GUI-pins JSON) and asserts.
  - Adopted at every sandbox-building hand-copy site: `test_verify_scope.sh` ×2, `test_verify_throughput.sh`, `test_scope_boundary.sh`, `test_verify_release_delta_skip.sh`, `test_run_all_tiering.sh`, `test_verify_gui_feature_check.sh`, and ν's `_make_plan_fixture`.
  - Not adopted: `test_verify_pipeline_guard.sh`'s `_SYNTH_VERIFY`, a deliberate clause-3 fixture that ζ deletes (fold-verifier V3).
  - The #7785 behaviour change (the infra map now present in that fixture) is analysed and its b10/b13/b15 expectations updated.
  - Ratchet row `verify-sandbox-hand-copy | cp "\$REPO_ROOT/scripts/verify\.sh" | tests/infra/test_*.sh | <post-ι count = the remaining _SYNTH_VERIFY line> | ι`. ζ lowers it to 0.
  - Signal: the ratchet reads its post-ι ceiling, with only `_SYNTH_VERIFY` left; adding a `source` line to a lib verify sources reds no sandbox suite.
  - Depends on #8476, ν, κ (κ deletes the 4b fixtures first).
- **λ #9109 — `tree-sitter-generate.sh --check`** · medium · key `fk-7bc1c1449010`.
  - Scope: a read-only `--check` (exit 0 iff `_stamp_is_current`, no lock, no write); `test_verify_semaphore_e2e.sh::ensure_tree_sitter_ready` calls it; the e2e fake fixtures are built by running the real script against a fixture grammar; stale line cites dropped.
  - Ratchet row `ts-stamp-predicate-copy | grammar_hash\.stamp | tests/infra/*semaphore_e2e*.sh | 0 | λ`. It is scoped to the e2e file and ο's later partitions (named `*semaphore_e2e*`). `test_tree_sitter_pipeline.sh` legitimately tests the generate script itself, and it is S19's (fold-verifier V4).
  - Signal: a lane whose `parser.c` is non-empty but unattested reads "stale" to both.
- **μ #9110 — `test_occt_flock_gate.sh` split** · medium · key `fk-6ccbfc296ee8`; subsumes #7581.
  - Scope: Tests 10–13/17/17b and T1–T18-BG move to `tests/infra/test_verify_timeout_knobs.sh` (bash, moved), with one merge-role capture helper carrying the `REIFY_RELEASE_DELTA_SKIP` pin (#7581); `occt_plan_grep_or_dump` moves into `plan_capture_lib.sh` as `plan_grep_or_dump`; Tests 5 and 7a (comment-only greps) deleted; `scripts/occt-touching-crates.txt` header corrected (the wrapper is no longer invoked by verify).
  - Signal: `test_occt_flock_gate.sh` contains no `--print-plan`; the new member is green.
  - Depends on η1.
- **ν #9111 — Gui-feature test-pass contracts in their own member** · medium · key `fk-3988a6996b82`.
  - Scope: Part B of `test_compute_trampoline_registration_wired.sh` → `tests/infra/test_verify_gui_feature_test_pass.sh` (bash, moved, one extra capture); Part B's false "today only COMPILE-checks" header fixed; cites by symbol.
  - Signal: the trampoline member's header and content describe INV-FEA-1 only; the new member is green.
- **ζ #9112 — Cutover** · high · no own key.
  - Scope: the D1 shim replaces the bash body in one commit under the full gate. B1/B2 retired. Guard clauses 3/4a and the extractor deleted. `affected-crates-lib.sh::reify_is_inert_path` deleted. The loader ratchet's verify.sh exemption removed (2 → 0).
  - `copy_verify_sandbox`'s inventory becomes `--print-gates`' planner inputs. It applies `derive_source_closure` over each bash lib listed there, so transitively sourced libs (`lib_slot_acquire.sh`, `lib_clock_stop.sh`, `lib_event_log.sh`) are copied too (critic K23).
  - Also in the same commit:
    - The conditional `native-deps.toml` row (§6).
    - The `test_verify_scope.sh` VS-* and `test_run_all_tiering.sh` mapped-artifact sentinel scenarios repointed from the shim to a planner module.
    - B-KLOC-driftguard deleted (nothing left to drift).
    - PG-DRIFT / PG-DRIFT-GUI rewritten as behavioural tests of the planner through the shim, with β's `test_verify_scope_record.py` scenarios folded in. The bash lists they guarded are gone (D6, fold-verifier V1).
    - `test_verify_pipeline_guard.sh`'s `_SYNTH_VERIFY` clause-3 fixture deleted with clause 3; the `verify-sandbox-hand-copy` ratchet lowered to 0.
    - Docs repointed: `docs/notes/orphaned-test-binary-reaper.md` Layer A, `docs/notes/verify-pipeline-knobs.md`, the `/prd` overlay `.claude/skills/prd/project.md` "Landing PRD artifacts" re-baselining text, `tests/prd-gate/README.md`, and the `crates/**` doc comments naming the two list variables.
  - Signal:
    - `scripts/verify.sh` is 2 lines.
    - A docs-only `git commit` on main runs the hook path in seconds through the Python planner.
    - `DF_VERIFY_ROLE=merge scripts/verify.sh all --profile both --scope all --print-plan` equals the last bash render byte for byte, apart from the listed divergences.
    - `requires-full-gate scripts/lib_test_semaphore.sh` still exits 0.
  - Depends on γ1, γ2, ε, η1, η2, ι, κ, #8654, #8662.
  - *Amended 2026-10-16 (plans/gap-fill-deliberation-reify-20261016.md §D-K8-10 (2), G1 sentence-sized form):* ζ's commit message records how each long comment block of `scripts/verify.sh` is classified: enforced-by-type (deleted), persistent rationale, or reviewer history (dropped). Moving the persistent rationale to `docs/notes/verify-pipeline-design.md` is leaf χ, after ζ.
- **ξ #9113 — `test_verify_scope.sh` h14 partition (6)** · medium · no own key; subsumes #8486.
  - Scope: the record's six partitions over a `verify_scope_fixture_lib.sh` that also holds the PG-DRIFT/PG-DRIFT-GUI derivations GV-6 reads. #8486's heuristic-13 obstacle is fixed: the `plan_for*` helpers take the fixture as a parameter instead of reading scenario globals. The 89 ordered scope greps → `plan_header_field`; ratchet `ordered-scope-grep` → 0.
  - Signal: no file over 1,500 lines; same assertion total (the `test_verify_nextest_absent_suites.sh` S-row updated).
  - Depends on ζ, θ, ι, #8476.
- **ο #9114 — `test_verify_semaphore_e2e.sh` h14 partition (4)** · medium · no own key.
  - Scope: the record's four partitions, named `test_verify_semaphore_e2e_<group>.sh` with the lib `verify_semaphore_e2e_lib.sh`, so λ's `*semaphore_e2e*` ratchet path keeps covering them. The tree-sitter group collapses into λ's `--check` call instead of moving. D_ROSTER/G_MEMBERS no longer apply after #8694.
  - Signal: no partition reaches back into the parent's globals; the suite's pass count is preserved.
  - Depends on λ, #8694.
- **ρ #9115 — Single-line narrowing header** · low · no own key.
  - Scope: the `# narrowing —` header joins its lists with commas (defect 7); `plan_count_noncomment_lines` consumers and θ's reader updated in the same diff.
  - Signal: a narrowed `--print-plan` has exactly one `# narrowing —` line and no bare continuation lines.
  - Depends on ζ.
- **σ #9116 — Retire the bash Layer-A reaper functions** · low · no own key.
  - Scope: `reaper_run_in_pgroup`, `reaper_teardown` and `reaper_kill_pgroup` have no consumer after ζ, so they are deleted with their `test_proc_reaper.sh` unit sections. `reap-orphans` (Layer B) stays.
  - Signal: `git grep -n 'reaper_run_in_pgroup' -- scripts tests` returns nothing.
  - Depends on ζ.
- **τ #9312 — Tests declare what they guard (`# guards:` header)** · medium · key `fk-a4abc5559858` (gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-5).
  - Scope: a `# guards: <path-or-glob> ...` header in each `tests/infra` member (or `GUARDS = [...]` in a `.py` member), in the same header block as S19 υ's `# run-all-bucket:` pragma. One parser in `reify_verify.selection` reads it at plan time, with fnmatch matching as today. One structural Python member checks that every member has a header or an explicit `guards: none`. Deleted: `scripts/verify-pipeline-infra-tests.txt` (177 rows), the `is-registered` anti-drift sweep in `verify-pipeline-guard.sh`, and the fixtures that copy the map. D14's hand-row clause ends here.
  - Signal: a new `tests/infra` member without a `# guards:` line reds the structural member; editing an artifact a member declares selects that member at task tier, with no map row.
  - Depends on ζ. Held there because bash `select_infra_tests` reads the txt until cutover.
- **υ #9313 — Crate scope classes declared in the crate** · medium · key `fk-9911cc28f75c` (weakened) (gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-6).
  - Scope: each crate's Cargo.toml gets `[package.metadata.reify-scope]` with boolean `corpus_reader`, `release_sensitive` and `harness_consolidatable`. `crate_scope_classes(cargo_metadata)` in `scripts/reify_path_attribution.py` reads it from the metadata call `affected_crates` already makes. `attribute()`, the release-declared set and the kloc set read it.
  - Deleted, with their drift tests: `RI_CORPUS_CRATES`, `scripts/release-sensitive-crates.txt` and the lib's crate literal. `scripts/occt-touching-crates.txt` stays until the leaf shows the `occt_touching_set` closure is equal to it.
  - One structural test checks that every declared key names a workspace member. β keeps its `BashCall` shape; this leaf replaces what those BashCalls read.
  - The finding record is corrected: `dark-factory-orchestrator.yaml` lines 215-216 are a comment pointing at the txt, not a third copy.
  - Signal: marking a crate `corpus_reader = true` in its own Cargo.toml is the whole edit that puts it in the corpus scope class.
  - Depends on β.
- **φ #9314 — reify-audit scope predicates as one data file** · low · key `fk-b1c017e8b73a` (gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-10 (1)).
  - Scope: `crates/reify-audit/scope-predicates.toml` holds the swept-extension / PDIAG / PDOCCOVER path predicates. ptodo, pdiag and pdoccover `include_str!` it, and `selection` reads it as a D15 `DataFile`. A Rust test pins file ≡ predicates. The three guarded copies and their PT/PDIAG/PDOCCOVER drift tests are deleted.
  - S24 seam note: the Rust half is in S24's reify-audit area.
  - Signal: adding a swept extension is one edit to the toml.
  - Depends on ζ.
- **χ #9315 — verify-pipeline rationale gets a home** · low · key `fk-9662894f1522` (gap-fill 2026-10-16, plans/gap-fill-deliberation-reify-20261016.md §D-K8-10 (2)).
  - Scope: write `docs/notes/verify-pipeline-design.md` from the blocks ζ's commit message classified as persistent rationale (read the pre-cutover text with `git show <ζ>^:scripts/verify.sh`). Each section carries a symbol pointer into `scripts/reify_verify/`. Link it from `docs/notes/verify-pipeline-knobs.md`.
  - Signal: every block ζ classified as persistent rationale has a section there.
  - Depends on ζ.
- **ω #9117 — PRD close** · low · no own key.
  - Scope: terminal stamp, ID backfill, AS-AUTHORED freeze of this PRD and its manifest. Clears any S18 priority override still set (§11 pins).
  - Signal: the committed header.
  - Depends on every leaf above.
- **DF dark_factory #7006 — dark-factory companion** · low (sidecar label `DF`, `external_task_id: dark_factory:7006`).
  - Scope: `orchestrator/tests/test_verify_role_integration.py` reads `--print-plan=json` (`step.kind`, typed `prio_class`) instead of the text grammar; `sampler/metrics.py::_is_verify_sh` matches the entry script in `cmdline` (already blind today); DF comments citing `verify.sh:NNNN` repointed to symbols.
  - Signal: that DF test passes against a reify checkout at ζ or later. Its `scripts/verify.sh --print-plan=json test` invocation goes through the shim; its `bash <script>` PermissionError fallback keeps working.
  - External dependency on reify ζ (critic K4). The test invokes the shim path with action `test`, which needs γ2's test region and ζ's shim.

## 11. Dependency edges (for decompose)

- **Intra-batch:**
  - α → β, γ1; π → β; β → γ1; γ1 → γ2;
  - γ2 → κ, ε, η1, η2; ε → η2;
  - ν → ι; κ → ι; η1 → μ; θ2 → θ3; λ → ο;
  - {γ1, γ2, ε, η1, η2, ι, κ} → ζ;
  - ζ → ξ, ρ, σ; θ → ξ; ι → ξ;
  - every leaf → ω.
  - γ2 → η1 stays, although η1 needs only α's table: deleting the bash wall parsers before B1 covers the test region would leave the bash walls unguarded (critic K21 considered).
- **Upstream:** #8662 → β, ζ; #8654 → γ1, ζ; #8919 → γ1; #7424 → γ1, ε; #8685 → ε; #8476 → π, ι, ξ; #8694 → ο.
- **Held behind ζ (Leo Q5 and follow-up)**, each with a one-line window note. Re-scanned at filing over task text and files across every open task (fold-verifier V7):
  - **Behaviour changes** (scout D's RETARGET set minus the two exceptions, plus the re-scan's #7605, #9035, #9061; conservatively #6432 and #6573, whose verify.sh wiring is conditional): #5576, #5841, #5853, #6077, #6199, #6432, #6573, #6766, #6909, #6964, #7335, #7428, #7429, #7474, #7515, #7516, #7605, #7797, #7815, #8180, #8235, #8292, #8350, #8803, #8804, #9035, #9061.
  - **Tasks that edit or decide what S18 deletes** (critic K12). Each note says which half is moot:
    - #8540: its verify.sh half — line cites and the `--help` range — is moot after ζ; α fixes `--help`. Its sibling-script cites stay its own work.
    - #8921: its verify.sh comment half is deleted by ζ; its `test_verify_scope.sh` half stays.
    - #8804: the note records Q1 (derivation); any outcome is a Python change.
  - **Cascade made visible.** Holding these also holds their dependents: #6766 (7), #6909 (5), #6964 (5), #8292 meshing-service (3), and one each for #7335, #7797, #7429 and #8540 (#8073, a warm-lane cite fix).
  - **Not held:**
    - #7424 and #8919, the two exceptions (D13), are γ1 prerequisites.
    - The ~106 fixture-registration tasks are not held: the D6 derivation reproduces their list entry, and bash PG-DRIFT guards it until ζ. They get the Q4 note.
    - #8567 and #8591 are comment-only cite fixes in files ν/ο move; file locks serialise them.
- **Subsumed at filing:** #7775 → π, #7645 → κ, #7647 → θ, #8440 → θ2, #7002 and #7785 → ι, #7581 → μ, #8486 → ξ, #9056 → β (+ ζ).
- **Gap-fill edges (2026-10-16):** ζ → τ, φ, χ; β → υ. None of these leaves becomes a prerequisite of a #8612 closure task (G1).
- **Pinned (Leo Q5):** the S18 chain up to and including ζ — α, π, β, γ1, γ2, κ, ε, η1, η2, ι, ν, ζ — plus its open upstream prerequisites #8654, #8685, #8919 and #8476 (#7491, #8662 and #7424 have landed). Each pin is `pinned: true`, checked with `get_pin_queue`. ω's close clears any S18 override still set.
- **Gate:** #8612 depends on the high leaves α, γ1, ε, η1, ζ.
- **Dark-factory companion:** `metadata.external_deps` on reify ζ.

## 12. Rulings (this session)

- **Q1 (Leo, 2026-10-14) → derive Rust, JSON for GUI.** D6: no Rust list exists. `EXPECTED_CLEAN` becomes a JSON file that vitest and the planner both read, and PG-DRIFT/-GUI become behavioural tests.
- **Q2 (Leo) → fix defects 1–6 in the port, keep every plan byte.** D10/D11 fix them. Defect 7 (the multi-line narrowing header) keeps its bytes through cutover and is fixed by ρ.
- **Q3 (Leo) → pass the admission exemption, don't export it.** D3: explicit parameters at verify's two call sites; the libs' standalone `DF_VERIFY_ROLE = merge` stays as their external contract, pinned by B5.
- **Q4 (Leo) → yes to both cross-task writes in pass 2.** The decompose session widens each delivered check that names `scripts/verify.sh`, with the three writes: task metadata, the owning PRD's `.yaml` and its `.md`. Program-stream PRDs go to the lead. It also appends a one-line note to each open fixture-registration task.
- **Q5 (Leo, 2026-10-15) → hold everything behind ζ, and pin the chain up to and including ζ.** §11 lists the held tasks and the pins.
- **Q5 follow-up (Leo, 2026-10-15) → behaviour changes only.**
  - Held: the open tasks that change what verify.sh plans or runs.
  - Not held: the ~106 fixture-registration tasks (Q4 note instead; bash PG-DRIFT guards them until ζ).
  - Exceptions: #7424 (in flight) and #8919 (S19's ordering) stay γ1 prerequisites.
  - The fold-verifier caught that the first rationale given for the exceptions ("a cycle") was false; the reasons recorded in D13 are the real ones.
- **Q3's consequence for the report's "Deletable" list.** The "2 cross-file exemptions" are retained by ruling as the libs' standalone contract, not deleted. What is deleted is verify's own re-testing of the role (critic K22).
- **The one plan-byte divergence (lead decision under the S18 ruling, not Q2).** γ2 builds the gui-feature pass from the one nextest emitter, which is fk-b072e820e926's ruled remedy. That pass therefore honours `REIFY_VERIFY_NEXTEST_CONFIG`, which today's hand copy ignores. The plan text differs only when that variable is set and the role is not offline. Today the only setters are tests and the offline confirm child, which has no gui-feature pass. B1 lists the scenario as its single plan divergence.
- **Recorded premise corrections (G6, §3):**
  - DF runtime never parses plan text.
  - "Summary/FAILED" are run_all's lines.
  - "67 tests" is 16 direct source readers plus 3 indirect.
  - The residual allowlist is empty.
  - The fixture lists do not nest.
  - `--print-role-walls` is named `--print-role-policy` (heuristic 1).

## 13. Open questions (tactical, implementation-time)

- The exact module boundary between `build` and `nextest` if `build` grows past ~800 lines: split by phase group, never by reaching back into `build`.
- B1's per-shard wall time: γ1 measures it; if a shard exceeds the selective-infra leaf's 10-minute wall with margin, it shards further. Never sample the grid.
- Critic record: a fable/high critic seat reviewed the assembled draft on 2026-10-14 (24 findings: 1 blocker, 11 major, 12 minor). All 24 are folded in; the `critic K<n>` tags in the text mark where.
- The per-step timing hook's shape once #7424 lands (ε ports whatever #7424 ships).

## Gap-fill amendments (2026-10-16)

- **New leaf τ #9312, after ζ #9112; D14's infra-map row clause ends at τ.** plans/gap-fill-deliberation-reify-20261016.md §D-K8-5.
- **New leaf υ #9313, after β #9097.** §D-K8-6. Crate scope classes move into `[package.metadata.reify-scope]`, read once from cargo metadata; RI_CORPUS_CRATES, `release-sensitive-crates.txt` and the kloc literal are deleted. `harness_layout_consolidatable_crates` keeps its contract until this leaf moves it (S19 θ / cargo-native stream cross-reference).
- **New leaf φ #9314, after ζ: the §9 follow-up candidate is filed in data-file form.** §D-K8-10 (1). The S24 seam note covers `crates/reify-audit`.
- **ζ #9112 amended, sentence-sized (G1).** §D-K8-10 (2). The commit message records the classification of verify.sh's long comment blocks. **New leaf χ #9315 after ζ** moves the persistent rationale to `docs/notes/verify-pipeline-design.md`.
- **β #9097 description hygiene.** Uncontested block. It now reads "staged primary diff fails wide (D5 amended 2026-10-15; implemented by #9170)".
- **D-K8-10 (5).** A standalone low task after β retires B13 and `direct_crates_per_path` and subsumes #7766. It is not a leaf of this PRD.
