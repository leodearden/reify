# Capability manifest — cell-eval-transaction-closure

PRD: `docs/prds/v0_6/cell-eval-transaction-closure.md`. Machine-readable twin: `cell-eval-transaction-closure.capability-manifest.yaml` (schema: dark-factory `shared.capability_manifest.CapabilityManifestDoc`, 23 task blocks, 60 capabilities; φ and χ added by the 2026-10-16 gap-fill, `plans/gap-fill-deliberation-reify-20261016.md` §K3-D1 / §K3-D5). Task ids stamped by `commit_planning` 2026-10-13: α #8858, β #8859, ν #8860, γ #8861, δ #8862, ε #8863, η #8864, ι #8865, θ #8866, ζ #8867, κ #8868, λ #8869, μ #8870, σ #8871, τ #8872, ξ #8873, ο #8874, π #8875, ρ #8876, υ #8877, ω #8878; 2026-10-16: φ #9292, χ #9294. The milestone MS (§9) is not a leaf and has no block.

**Code anchors** verified against main `a4a8862bcc` (2026-10-13). Cite-by-symbol; re-locate lines at implementation time.

Verdicts: PASS = the binding held at authoring (read or probed this session, or reproduced by the critic or the fold-verifier), or is now ruled (OQ-1..OQ-5, `scratch/prd-results/S4.md`, re-rulings and the SECOND FOLD included); OPEN = the binding is the leaf's own measurement on a construct that does not exist yet. No FAIL rows. Every mechanical `delivered_check` is red on main today (the construct is absent, or the superseded construct is present), so none is green on the day it is written. The polarity lint reports only `absent_overbroad` warnings, which are expected before `metadata.files` is declared at decompose.

| Leaf | Capability | Evidence (authoring) | Verdict | delivered_check |
|---|---|---|---|---|
| α | engine-on-8730 | `tests/infra/rust_construct_ratchet.py` on `task/8730` (Row with baseline/target/STALE over the shared production view); α ← #8730, rows only | PASS | — |
| α | ratchet-member-shape-exists | S9 D12 / S27 α member shape: stdlib unittest, `.sh` wrapper, manifest row | PASS | path `tests/infra/test_cell_eval_transaction_ratchets.py` present |
| α | baselines-measured | E1 62, E2 3, E3 15 (production view), E4 8, E5 70, E6 9, E7 11 activation shapes (§10); α commits the engine's counts | PASS | — |
| β | skip-leg-is-a-string-today | `commit_cell_result_at` formats `|cache-skip=`; `cell_latest_commit_was_skip` uses `s.contains("cache-skip=")`; `Completed.outcome` placeholder; `CommitOutcome` `allow(dead_code)` | PASS | grep `CacheLegKind::Skipped` present in `cache_divergence.rs` |
| β | no-external-journal-consumer | no `EventKind`/`EvalEvent` use in gui/src-tauri, reify-cli, reify-lsp, reify-mcp | PASS | — |
| β | trace-source-move-avoids-cycle | `cell_commit` imports `journal`; `TraceSource` moves down into `journal.rs` | PASS | — |
| ν | partition-symbols-located | engine_eval h14 P1/P2/P3/P5/P7 symbol lists present at main | PASS | path `crates/reify-eval/src/solver_problem.rs` present |
| ν | upstream-moves-pending | #8687, #8645, #8735 pending | PASS | — |
| γ | write-api-is-pub | `pub fn record_evaluation*`, `pub fn put` in `cache.rs`; 13 + 1 direct production writes | PASS | grep `pub struct ValueCellSeal` present in `cache.rs` |
| γ | seal-in-cache-avoids-cycle | critic simulation: a seal in `cell_commit.rs` named by `cache.rs` → `GREW {cache, cell_commit, deps, journal, warm_pool}`; ruled B′ (seal in `cache.rs`, `mint()` pinned by E8) | PASS (ruled) | — |
| γ | compute-dispatch-writes-value-cells | `complete_compute_dispatch_atomically` and `begin_compute_dispatch` write `NodeId::Value` outputs/placeholders; callers in `run_compute_dispatch`; γ seals both and `commit_compute_outputs` commits all four legs `Recorded` (never Skip; B9) | PASS | — |
| γ | bare-snapshot-writes | 22 `snapshot.values.insert(` + 14 `snapshot_values.insert(` outside `cell_commit.rs`; rows E10/E10b; INV-EVAL-1 partial until 0 | PASS | — |
| γ | journal-helpers-move | `record_subpath_started`/`record_eval_completed` are engine_eval free fns (16 calls); move into `cell_commit.rs` | PASS | grep `fn record_subpath_started` present in `cell_commit.rs` |
| γ | pub-super-equals-pub-crate | `pub mod cache;` at the crate root; ruled B′, INV-EVAL-1 type+lint | PASS (ruled) | — |
| γ | primitive-gaps-named | `cell_commit.rs` module doc lists the determinacy-preserving and failure-path shapes as open | PASS | grep `fn reserve_preserving_determinacy_direct` absent in `crates/reify-eval/src` |
| δ | no-production-caller | `.edit_source(` callers: 14 integration-test files, `engine_admin` in-file test, engine_edit in-file tests | PASS | grep `pub fn edit_source\(` absent in `engine_edit.rs` |
| δ | warm-pool-keeps-a-donor | premise corrected: `invalidate_realization_dirty_cone` → `warm_pool.donate_with_cost` on `edit_param`'s realization eviction; δ deletes only the topology-change donation, citing MS | PASS (re-ruled) | — |
| ε | edit-param-shape | 1,938-line fn, 8 banners, 12 `let mut` | PASS | grep `struct EditTransaction` present in `engine_edit.rs` |
| η | no-containment-null-object | `struct NoContainment` in unfold.rs; 3 constructions; 0 replay-storage calls | PASS | grep `struct NoContainment` absent in `unfold.rs` |
| η | instance-scope-drops-diagnostic | probe: `cell_eval_instance_scope_runtime_diag.ri` → `Top.c.xs = undef`, exit 0 | PASS | path `crates/reify-eval/src/cell_elaboration.rs` present |
| η | param-default-drops-diagnostic | probe: `cell_eval_param_default_runtime_diag.ri` → `S.xs = undef`, exit 0; the control exits 1 with the error | PASS | — |
| η | base-builder-move-would-grow-scc | the builder names `geometry_ops`; ruled OQ-2 (B): crate-root `eval_env` free fn | PASS (ruled) | — |
| η | validate-param-override-movable | `validate_param_override` depends only on the crate root's `value_type_kind_matches` | PASS | path `crates/reify-eval/src/param_override.rs` present |
| η | r3d-r3e-on-cached-miss | R3d/R3e hits in `eval_cached`'s Param and Let miss arms (critic M2) | PASS | — |
| η | cellhooks-mint | `Engine::mint_symbolic_geometry_handle_for_cell` becomes a `CellHooks` method; `Engine::` paths are invisible to the SCC tool, so row E11 forbids `Engine` in the new modules | PASS | — |
| η | lsp-warning-change-declared | #8348 probe; empty-registry `NoRegisteredComputeTrampoline` on `eval_cached` | PASS | — |
| θ | cell-eval-ctx-type | the registry names a nonexistent `CellEvalCtx`; θ introduces it with `CellValue`, `expr_ctx`, `eval_cell_guarded` and retypes the commit value leg at every caller (fold-verify NEW 1) | PASS | grep `pub\(crate\) struct CellEvalCtx` present in `cell_eval_ctx.rs` |
| θ | boundary-is-per-site | `catch_unwind` at 2 engine_eval production sites, 0 in engine_edit; `panic_on_eval_cells` read only by the cold evaluators | PASS | grep `fn eval_cell_guarded` present in `cell_eval_ctx.rs` |
| ι | override-not-consulted-post-solve | `post_solver_re_eval_guard_cells` takes no `param_overrides` | PASS | grep `fn activate_guarded_group` present in `crates/reify-eval/src` |
| ι | guarded-param-drops-diagnostic | probe: `cell_eval_guarded_param_runtime_diag.ri` → `S.xs = undef`, exit 0 | PASS | — |
| ι | fifth-activation-spelling | `reelaborate_guarded_group` called from `edit_param`'s guard phase (critic M7) | PASS | grep `fn reelaborate_guarded_group\(` absent in `crates/reify-eval/src` |
| ζ | six-arms-today | 8 solve calls (engine_eval 6, engine_edit 2); roster in engine_edit module doc | PASS | grep `fn solve_and_write_back` present in `crates/reify-eval/src` |
| ζ | documented-divergences | skeptic: 4 of 6 step gaps recorded in code; merged-cluster divergence pinned | PASS | — |
| ζ | ordering-vs-solver-driver-parity | ruled OQ-5 (A): #6690 → ζ → #6699 | PASS (ruled) | — |
| κ | one-registered-detector | `with_builtins` registers PSD only; 5 hand-listed in eval and eval_cached; 4 eval-only | PASS | grep `fn applies_to` present in `detectors.rs` |
| κ | gui-uses-edit-check | `preview_parameter` → `.edit_check(`; committed edits go through `check()`; `edit_param` runs no post-pass | PASS | — |
| κ | dangling-symbol | no `fn detect_scope_coupling`; prose mentions remain — asserted positively (overlay rule 3) | PASS | grep `struct ScopeCouplingDetector` present in `detectors.rs` |
| κ | order-differs-by-path | eval: registry → annotation → coupling..robustness → value-scans; eval_cached: value-scans first (critic M3) | PASS | — |
| λ | counters-exist | the three `last_*` fields referenced in engine_eval, lib, engine_admin, engine_build and three test files | PASS | grep `last_param_override_type_kind_rejections` absent in `crates` |
| λ | pdiag-row | `engine_eval.rs 21` in `pdiag-baseline.txt`; target is the sum over the receiving files | PASS | — |
| μ | r4-row-pending | #8730 in progress, #8743 pending; R4 measured by ν | OPEN (μ's own measurement) | manual (R4 rows lowered in `tests/infra/test_compiler_decl_pass_ratchets.py`) |
| μ | hash-chains-are-not-unions | reify-test-support's content-hash builder interleaves members with constraints | PASS | — |
| σ | residual-sites-listed | §10 per-function list | PASS | manual (E1 equals τ's non-cell residue on the ratchet engine) |
| τ | base-builder-in-lib | `pub(crate) fn eval_ctx_with_meta<'a>` in `lib.rs`; dead `Engine::cell_eval_ctx` | PASS | grep `fn eval_ctx_with_meta<` absent in `crates/reify-eval/src` (definition-anchored; the in-file test `eval_ctx_with_meta_resolves_meta_access` would match a bare name) |
| τ | inv-eval-2-row | INV-EVAL-2 `proposed`; τ flips it with an S24-grammar pointer and the four-site lockstep | PASS | grep `^\| INV-EVAL-2 \|.*enforced\(type\+lint\)` present in `docs/invariants.md` |
| τ | geometry-ops-sites-relocated-first | S13 #8748–#8751 pending | PASS | — |
| ξ | partition-symbols-located | engine_edit h14 P1/P2/P3 symbol lists present | PASS | path `crates/reify-eval/src/graph_diff.rs` present |
| ο | partition-symbols-located | unfold h14 P1/P2 present | PASS | path `crates/reify-eval/src/unfold/instance.rs` present |
| π | partition-symbols-located | cache h14 P1/P3 present; P4 deleted by #8675 | PASS | path `crates/reify-eval/src/freshness_derive.rs` present |
| π | nodeid-move-vs-s3-oq5 | ruled OQ-4 (A); no `cache → cell_commit` edge with the seal in `cache.rs` | PASS (ruled) | path `crates/reify-eval/src/node_id.rs` present |
| ρ | hash-algo-tests-misfiled | 29 tests in persistent_cache's `mod tests`; existing home `engine_hash_tests.rs` (9 tests) | PASS | grep `fn compose_engine_version_hash_is_order_sensitive` present in `engine_hash_tests.rs` |
| ρ | shims-are-s7 | #8551 pending | PASS | — |
| υ | differential-harness-exists | `tests/common/differential.rs` included by `harness_cache` binaries; fixture sources inlined | PASS | manual |
| ω | terminal-status-stamped | overlay closed terminal vocabulary | PASS | grep `^\*\*Status:\*\* SHIPPED` present in the PRD |
| φ | replay-entry-absent | no `replay_last_eval` in `crates/reify-eval/src` at main c7bbfe2519 (gap-fill K3-D1) | PASS | grep `fn replay_last_eval` present in `crates/reify-eval/src/` |
| φ | lsp-calls-replay | reify-lsp `diagnostics.rs` is `eval_cached`'s one non-test caller, on unchanged content only, kernel-less and solver-less | PASS | grep `replay_last_eval` present in `crates/reify-lsp/src/diagnostics.rs` |
| φ | eval-cached-twin-present | `pub fn eval_cached(` (`engine_eval.rs::Engine.eval_cached`) + `dispatch_merged_cluster_solve_cached`, ~1,870 lines; 20 test files call `.eval_cached(` | PASS | grep `pub fn eval_cached\(` absent in `crates/reify-eval/src/` |
| φ | dirty-reasons-only-serve-eval-cached | `CacheStore.dirty_reasons` and its four methods are read only on the `eval_cached` path | PASS | grep `dirty_reasons` absent in `crates/reify-eval/src/cache.rs` |
| χ | no-production-caller | `set_param_and_invalidate` has no production caller since bbfbc7dd29 (#6665); callers are cache.rs tests (5), reify-eval integration tests, one reify-lsp test (gap-fill K3-D5) | PASS | grep `fn set_param_and_invalidate` absent in `crates/reify-eval/src/` |
| χ | pin-recorded-by-transaction | ε #8863's `EditTransaction` records the user pin first (gap-fill K3-D5) | PASS | — |
