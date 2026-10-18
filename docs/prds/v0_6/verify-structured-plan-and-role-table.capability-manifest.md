# Capability manifest — verify-structured-plan-and-role-table

PRD: `docs/prds/v0_6/verify-structured-plan-and-role-table.md`. Evidence was bound on 2026-10-14 against main `1b06f93151`.

At that tree every S18 anchor file was byte-identical to the review's pinned tree `a030ee7528`, apart from comment-only edits in `test_verify_scope.sh` and `test_compute_trampoline_registration_wired.sh` and one row added to `verify-pipeline-paths.txt`. Since then #7424 (the verify ledger layer) and #8662 (`reify_path_attribution.py`) have landed (PRD header).

How the evidence was gathered:
- **Symbol evidence:** `git grep` in the authoring session.
- **Behaviour evidence:** five read-only scout seats (verify.sh anatomy front and back, external consumers, infra-test inventory, freshness and task triage). They ran real `--print-plan` renders and reproduced the executor defects against the real `lib_proc_reaper.sh` on bash 5.2.21.
- **Delivered checks:** each was evaluated today. Every `present` check has 0 hits; every `absent` check has 1 or more.

No `.ri` fixtures: this PRD touches no language surface, so the grammar gate and the docs-truth gate do not fire.

Machine-readable twin: `verify-structured-plan-and-role-table.capability-manifest.yaml`.

## α — package, CLI, knobs, role and profile tables

- `stdlib-interpreter-on-both-verify-hosts` (G3). `/usr/bin/python3` is 3.12.3 on the dev host and on `leo-laptop` (ssh probe). The modules `tomllib`, `fcntl`, `signal` and `subprocess` are stdlib. PASS.
- `role-table-is-the-one-role-home` (signal premise). verify.sh decides the role at 16 sites, and two libs compare `= "merge"`. Delivered check: `^ROLE_TABLE` present in `scripts/reify_verify/policy.py`. PASS.
- `profile-table-replaces-variable-pairs` (signal premise). There are 5 internal and 4 env-override `_DEBUG/_RELEASE` pairs, plus 2 `*release*` recoveries. Delivered check: `^PROFILE_TABLE` present in `policy.py`. PASS.

## π — EXPECTED_CLEAN as JSON

- `expected-clean-is-inline-ts-today` (signal premise). `EXPECTED_CLEAN` is an inline TS array with about 405 entries, 145 of them prd-gate fixtures. PG-DRIFT-GUI reads it with `sed`. Delivered check: `^const EXPECTED_CLEAN = \[` absent from `reifyGrammarCorpus.test.ts`. That is the deleted construct, scoped to its declaration line, so prose naming it cannot match (overlay rule 1). PASS.

## β — scope, selectors, narrowing

- `path-facts-substrate` (G3). This leaf consumes S22 θ #8662, which landed 2026-10-15. `reify_path_attribution.py` exports `attribute(path) -> PathFacts(… inert, tree)` and an `attribute`/`closure` CLI. PASS.
- `harness-kloc-crate-set-has-an-owner`. `harness_layout_consolidatable_crates` prints the same 5 crates the literal hard-codes. Delivered check: present in `selection.py`. PASS.
- `decide-scope-is-pure`. Delivered check: `def decide_scope` present in `scope.py`. PASS.

## γ1 — plan model, renderers, parity

- `loader-path-helper-substrate` (G3). This leaf consumes S21 γ #8654, which is not on main. Covered by a hard edge. B1 proves the order `deps:snap:ambient`. PASS.
- `no-plan-line-relies-on-errexit` (G6). All 27 command lines of the widest merge render were read (`plans/merge-both-all.txt`). PASS.
- `typed-step-kind`. Delivered check: `class StepKind` present in `plan.py`. PASS.
- `plan-parity-member`. Delivered check: `class .*Parity` present in `tests/infra/test_verify_plan_parity.py`. PASS.

## γ2 — the one nextest emitter

- `one-nextest-config-path` (signal premise). There are 3 lazy-init sites and 2 placeholder copies today. Delivered check: `def nextest_config_path` present in `nextest.py`. PASS.
- `nextest-unions-multiple-filters` (G3). nextest is 0.9.136, which is why the retry composer uses a single `-E`. PASS.

## κ — `--print-gates`

- `guard-reads-verify-source-today` (signal premise). Clauses 3 and 4a read verify.sh source. Nine libs appear in clause 3 only. Delivered check: `--print-gates` present in `scripts/verify-pipeline-guard.sh`. PASS.
- `gui-vitest-workaround-row-retired` (signal premise). The #7645 workaround row is why `requires-full-gate scripts/gui-vitest-run.sh` exits 0 today (verified). Delivered check: `^scripts/gui-vitest-run\.sh$` absent from `scripts/verify-pipeline-paths.txt`. PASS.

## ε — the executor

- `semaphore-lib-has-a-standalone-cli` (G3). The main-guard runs `test_semaphore_run`. Delivered check: `--hold` present in `lib_test_semaphore.sh`. PASS.
- `cpu-admit-explicit-exemption` (Leo Q3). Delivered check: `_ca_exempt` present in `cpu-admit.sh`. PASS.
- `exit-code-collapse-reproduced` (G6). The scout reproduced it with steps exiting 75 and 124: verify exited 1 in both cases. Dark-factory branches only on 0, non-zero or negative. `jobserver-acceptance.py` counts exit 124. PASS.
- `event-log-substrate` (G3). This leaf consumes S19 γ #8685, which is not on main. Covered by a hard edge. PASS.

## η1 — role policy replaces the shadow parsers

- `shadow-parsers-exist-today` (signal premise). The test has 8 extractors and 5 seeders, and the residual paragraph reads `NONE`. Delivered check: `^_seed_accept_role\(\)` absent from `test_nextest_slow_priority.sh`. PASS.

## η2 — structured queries

- `source-readers-inventoried` (signal premise). 16 direct source readers exist. η2 owns 11; the rest go to ι, κ, η1 and ζ. Delivered check: `class .*TestCase` present in `tests/infra/test_verify_plan_structure.py`. PASS.

## θ / θ2 / θ3 — plan capture

- θ `no-typed-reader-today`. Delivered check: `^plan_header_field\(\)` present in `plan_capture_lib.sh`. PASS.
- θ2 `single-shot-capture-today`. Delivered check: `capture_print_plan` present in the nan-safe test. PASS.
- θ3 `raw-captures-counted`. There are 35 raw captures today. Delivered check: the `raw-print-plan-capture` ratchet row with ceiling 0, present in `tests/infra/adoption-ratchet.manifest`. PASS.

## ι — `copy_verify_sandbox`

- `hand-lists-today`. There are 8 hand-copy sites in 7 files. Delivered check: `^copy_verify_sandbox\(\)` present in `copy_list_preflight_lib.sh`. PASS.

## λ — `tree-sitter-generate.sh --check`

- `no-check-mode-today`. The script accepts only `[--force]`. Delivered check: `"--check"` present in the script. PASS.

## μ / ν — contracts in verify-named members

- μ `wrapper-file-holds-verify-contracts`. Delivered check: `capture_print_plan` present in `tests/infra/test_verify_timeout_knobs.sh`. PASS.
- ν `part-b-is-a-verify-contract`. Delivered check: `features gui` present in `tests/infra/test_verify_gui_feature_test_pass.sh`. PASS.

## ζ — cutover

- `shim-replaces-body`. Delivered check: `exec /usr/bin/python3 -I -B` present in `scripts/verify.sh`. PASS.
- `fixture-lists-gone`. Delivered check: `^_RUST_COUPLED_RI_FIXTURES=` absent from `scripts/verify.sh`. PASS.
- `inert-wrapper-deleted`. Delivered check: `^reify_is_inert_path\(\)` absent from `affected-crates-lib.sh`. PASS.

## ξ / ο — partitions

- ξ `partition-record`. Delivered check: `^make_fixture\(\)` present in `tests/infra/verify_scope_fixture_lib.sh`. PASS.
- ξ `ordered-scope-greps-gone`. There are 92 ordered greps today (a real bracket class). Delivered check: the `ordered-scope-grep` ratchet row with ceiling 0, present in `tests/infra/adoption-ratchet.manifest`. PASS.
- ο `partition-record`. The file is a D_ROSTER member until #8694, which is covered by a hard edge. Delivered check: `^make_stub_bin\(\)` present in `tests/infra/verify_semaphore_e2e_lib.sh`. PASS.

## τ / υ / φ / χ — gap-fill leaves (2026-10-16, plans/gap-fill-deliberation-reify-20261016.md)

Polarity was checked at main `c7bbfe2519`. Each `absent` check has hits today; each `present` check, or `path present` check, has none.

- τ #9312 (§D-K8-5) `hand-map-is-the-selection-home-today`. `scripts/verify-pipeline-infra-tests.txt` is tracked (177 rows). Delivered check: path `scripts/verify-pipeline-infra-tests.txt` absent. PASS. `one-header-parser-in-selection`: grep `^def parse_guards_header\(` present in `scripts/reify_verify/selection.py`. OPEN.
- υ #9313 (§D-K8-6) `corpus-set-hand-declared-today`. Delivered check: grep `^RI_CORPUS_CRATES` absent in `scripts/reify_path_attribution.py`. PASS. `one-metadata-reader`: grep `^def crate_scope_classes\(` present in the same file. OPEN.
- φ #9314 (§D-K8-10 (1)) `predicates-copied-today`. Delivered check: path `crates/reify-audit/scope-predicates.toml` present. PASS.
- χ #9315 (§D-K8-10 (2)) `rationale-has-no-home-today`. Delivered check: path `docs/notes/verify-pipeline-design.md` present. PASS.

## ρ / σ / ω

- ρ `narrowing-header-multiline-today`: manual. The behavioural cover is ρ's own one-line-header test.
- σ `layer-a-only-consumer-is-verify`. Delivered check: `^reaper_run_in_pgroup\(\)` absent from `lib_proc_reaper.sh`. PASS.
- ω `terminal-stamp`. Delivered check: the anchored `**Status:** SHIPPED` header present in the PRD. PASS.

## DF — dark-factory companion

- `df-runtime-parses-no-plan-text` (G6). Dark-factory's only parser of plan text is a DF test. The sampler is already blind. The block's `external_task_id` is set at decompose.
