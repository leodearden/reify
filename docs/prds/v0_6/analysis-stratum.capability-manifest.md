# Capability manifest: `analysis-stratum`

This manifest mechanizes G3 and G6 per leaf, per `.claude/skills/prd/project.md`. It covers the PRD `docs/prds/v0_6/analysis-stratum.md` (program stream S5, pass 1, author time).

All evidence was gathered against main `b1a7be2982` on 2026-10-13. Every path the PRD builds on is byte-identical to the review's pinned tree `a030ee7528`. Bindings cite by symbol and file, never by `path:line`.

This file is the human-readable twin of `docs/prds/v0_6/analysis-stratum.capability-manifest.yaml`. That file carries the same bindings plus their `delivered_check` descriptors, and validates against dark-factory's `CapabilityManifestDoc`. **Keep the two in sync by hand.** Correcting a check later takes three writes: the `.yaml` descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

**Stamped 2026-10-14 by `commit_planning`:** α #8984, β #8986, γ #8991, δ #8988, ε #8994, ζ #8996, ι #8990, η #8997, κ₁ #8999, κ₂ #9001, λ #9002, μ #9003, ν #9004, ξ #9006, ο #9007, π #9008, ρ #9009, σ #9010. Every grep `delivered_check` was copied into its producer's `metadata.delivered_checks`. The polarity gate reported eleven `absent_overbroad` warnings, none blocking, all on move leaves that declare `files: []`. Each matched line is a `mod`/`pub mod`/`struct`/Cargo line that the move itself deletes.

## Verdict at author time

All 45 `kind: grep` checks were executed with `git grep -E` against `HEAD` and confirmed **RED** at main:

- an `expect: present` check finds nothing today;
- an `expect: absent` check finds its target today.

Four rules shaped the patterns:

- **Constructs, not bare tokens.** Each `absent` pattern is scoped to the construct that carries the defect: `const PERSISTABLE_TARGETS`, `fn is_opted_in`, `Fea\(reify_solver_elastic::FeaDiagnosticDetail\)`, `struct ModalCacheKey`, and so on. A comment that names the retired symbol does not wedge the check (overlay rule 1).
- **No vacuous check.** One candidate was dropped as vacuous: `mod stdlib_inputs` absent is GREEN at main, because S15 α creates the module.
- **No wedge-prone Cargo.toml greps.** The `faer` and `reify-fdm` checks on reify-eval's `Cargo.toml` became `manual`. `faer` stays a dev-dependency, so an absent grep would wedge.
- **No dead pathspecs.** No `present` check points at a file that does not exist yet, so `reify-audit --pattern PDCHECK` sees no dead pathspec.
- **Hardened after the fold-verifier.**
  - The upstream-branch and fold-branch checks match `if target == "…"` crate-wide, so a branch relocated by S4 η #8864 still reds.
  - `ambient-cancel-poll-gone` is crate-wide and paired with `context-cancel-slot-gone`.
  - `significance-optin-list-gone` is scoped to the string-keyed signature.
  - The three ratchet-row checks became `manual`: a bare token over `tests/infra` is satisfiable by a docstring, and the ratchet member enforces its own rows.

`manual` rows are excluded from the dispatch gate. Each one names the behavioural test or upstream edge that enforces it.

**Upstream-producer bindings.** The PRD's §10 Prereqs column is the edge source; the rows below record the load-bearing ones. Hard upstreams are recorded as `producer:#NNNN upstream` and wired at decompose as real `add_dependency` edges, never as prose ordering. They come from these streams and tasks:

- S15: κ₁ waits on every S15 code leaf that edits reify-eval's analysis files. Before that, ε waits on #8686, #8690 and #8706, ζ on #8686 and #8706, and ι on #8686 and #8690.
- S4: η #8864 comes before ε.
- S3: β #8687 comes before γ.
- S7: #8551 comes before γ.
- S13: α #8747 comes before κ₁.
- S14: #8619, #8616, #8628 and #8626.
- S6: #8837 comes before μ; #8848, #8850 and #8854 come before λ.
- #5802 and #8525.

**G3.** The only `.ri` change is ι's declaration. Its grammar-gate fixture, `tests/prd-gate/fixtures/s5_tensegrity_load_surface.ri`, uses probe names so it never collides with the stdlib declaration. It parses (`tree-sitter parse --quiet`, exit 0, session-isolated cache) and type-checks (`reify check`, exit 0) at main. `grammar_confirmed: true` holds on every leaf.

**G6.** No leaf asserts a tuned numeric constant.

- BT11 is equality against the same-day pre-change run.
- BT7 asserts an outcome (`DispatchError::Cancelled`, output cells Pending).
- BT5 and BT6 assert set relations.
- BT10 asserts that a key changes.
- BT12 asserts structure and `converged`, not a magnitude.

The D3 `.ri`-fixture workflow applies to ι only (`procedural_prd_d3_verify_workflow_is_ri_only`). Every other leaf is Rust/infra wiring, bound by grep evidence and upstream producers.

## Bindings

| leaf | capability | binding | verdict | delivered_check |
|---|---|---|---|---|
| α | `core-f64-slab` | producer:α — reify_core::persistent_cache holds only the trait at main | PASS | grep `pub fn read_f64_slab` present in `crates/reify-core/src` |
| α | `core-severity-pair` | producer:α | PASS | grep `pub fn severity_to_u8` present in `crates/reify-core/src` |
| α | `shell-extract-f64-copy-gone` | grep: private read_f64_slab copy in result.rs at main (drifted BE path + SAFETY text) | PASS | grep `fn read_f64_slab` absent in `crates/reify-shell-extract/src/result.rs` |
| α | `shell-extract-severity-copy-gone` | grep: severity_to_u8 copy in result.rs at main | PASS | grep `fn severity_to_u8` absent in `crates/reify-shell-extract/src/result.rs` |
| α | `diagnostics-wire-severity-copy-gone` | grep: severity_to_u8 copy in diagnostics_wire.rs at main | PASS | grep `fn severity_to_u8` absent in `crates/reify-eval/src/persistent_cache/diagnostics_wire.rs` |
| α | `contract-slab-definition-gone` | grep: pub fn read_f64_slab defined in the contract at main (callers stay; only the definition moves) | PASS | grep `pub fn read_f64_slab` absent in `crates/reify-compute-contract/src` |
| α | `ratchet-member-registered` | producer:α — new infra member; manifest row same diff (esc-4914-162) | PASS | grep `test_analysis_stratum_ratchet` present in `tests/infra/run-all-classification.manifest` |
| α | `persisted-round-trips-unchanged` | behavioural: ElasticResult / BucklingResultCache / ShellExtractionResult round-trip tests pass unchanged | PASS | manual: existing behavioural tests are the enforcement |
| β | `contract-owned-fea-detail` | producer:β | PASS | grep `pub enum FeaDetail` present in `crates/reify-compute-contract/src` |
| β | `dispatch-solver-variant-gone` | grep: StructuredComputeDetail::Fea wraps reify_solver_elastic::FeaDiagnosticDetail at main (construct-scoped, comment-immune) | PASS | grep `Fea\(reify_solver_elastic::FeaDiagnosticDetail\)` absent in `crates/reify-compute-contract/src/dispatch.rs` |
| β | `gui-and-persisted-detail-unchanged` | behavioural: persistent_round_trip_replays_fea_under_constrained_unanchored + gui fea_diagnostics_from_structured tests pass unchanged (on-disk mirror order unchanged) | PASS | manual: behavioural tests are the enforcement |
| γ | `engine-compute-shim-gone` | grep: pub use reify_compute_contract::{…} block in engine_compute.rs at main | PASS | grep `pub use reify_compute_contract` absent in `crates/reify-eval/src/engine_compute.rs` |
| γ | `graph-shim-gone` | grep: graph.rs re-exports CancellationHandle at main | PASS | grep `pub use reify_compute_contract::CancellationHandle` absent in `crates/reify-eval/src/graph.rs` |
| γ | `handle-tests-in-contract` | producer:γ — 0 tests in realization.rs at main | PASS | grep `fn handle_new_stores_content_hash` present in `crates/reify-compute-contract/src` |
| γ | `upstream-8687-8551` | producer:#8687 (S3 β) and #8551 (S7) upstream — they repoint two importers / delete the persistent_cache shims first | PASS | manual: dependency edges are the enforcement |
| δ | `linked-child-handle` | producer:δ | PASS | grep `pub fn child\(` present in `crates/reify-compute-contract/src/cancellation.rs` |
| δ | `ambient-cancel-poll-gone` | grep: ambient_cg_cancel_poll reads the thread-local cancel handle at main (crate-wide: #8374 may move elastic code into static_linear_solve.rs) | PASS | grep `fn ambient_cg_cancel_poll` absent in `crates/reify-eval/src` |
| δ | `context-cancel-slot-gone` | grep: the dispatch context's cancel: Option<CancellationHandle> slot (solver_progress.rs) at main | PASS | grep `cancel: Option<CancellationHandle>` absent in `crates/reify-eval/src`, `crates/reify-compute-contract/src` |
| δ | `progress-channel-in-contract` | producer:δ — solver_progress lives in reify-eval at main | PASS | grep `trait SolverProgressSink` present in `crates/reify-compute-contract/src` |
| δ | `context-extensions` | producer:δ — typed extension map for family state (D15) | PASS | grep `pub struct Extensions` present in `crates/reify-compute-contract/src` |
| δ | `BT7-gui-cancel-reaches-every-cooperative-target` | behavioural: pre-cancelled set_active_solve_cancel handle yields DispatchError::Cancelled for modal/transient/inverse-dynamics/trajectory/shell-extract (red today) and elastic_static | PASS | manual: BT7 is the enforcement; no grep can assert an outcome |
| ε | `descriptor-type` | producer:ε | PASS | grep `pub struct ComputeTargetDescriptor` present in `crates/reify-compute-contract/src` |
| ε | `persistable-allowlist-gone` | grep: const PERSISTABLE_TARGETS string allowlist at main | PASS | grep `const PERSISTABLE_TARGETS` absent in `crates/reify-eval/src/compute_persist.rs` |
| ε | `significance-optin-list-gone` | grep: fn is_opted_in(target: &str) string list at main (scoped to the string-keyed signature: unit tests named is_opted_in_* and a generic row-based helper do not match) | PASS | grep `fn is_opted_in\(target: &str` absent in `crates/reify-eval/src` |
| ε | `elastic-upstream-branch-gone` | grep: if target == "solver::elastic_static" upstream branches (2 in engine_eval at main; S4 η #8864 may move the survivor into cell_elaboration.rs) — scanned crate-wide so a relocated branch still reds | PASS | grep `if target == "solver::elastic_static"` absent in `crates/reify-eval/src` |
| ε | `fold-target-branch-gone` | grep: if target == "shell-extract::extract" fold sites (2 in run_compute_dispatch at main), scanned crate-wide | PASS | grep `if target == "shell-extract::extract"` absent in `crates/reify-eval/src` |
| ε | `target-enumerator` | producer:ε — no public enumerator over compute_registry at main | PASS | grep `pub fn registered_compute_targets` present in `crates/reify-eval/src` |
| ε | `routing-ratchet-row` | producer:ε — target-string-routing row in α's member | PASS | manual: the ratchet member is the enforcement (a bare-token grep over tests/infra is satisfiable by a docstring) |
| ε | `upstream-epsilon` | producer:#8690 #8706 #8686 #8525 #8864 upstream (§10 prereqs) | PASS | manual: dependency edges are the enforcement |
| ε | `upstream-8864` | producer:#8864 (S4 η) upstream — one CellHooks site; program cross-reference | PASS | manual: dependency edge is the enforcement |
| ε | `BT3-scratch-descriptor` | behavioural: a scratch row with persistence/significance/on_completed/demand works end to end with no reify-eval source edit | PASS | manual: BT3 is the enforcement |
| ζ | `gui-parser-gone` | grep: engine_admin::parse_shell_extraction_result (second decoder) at main | PASS | grep `fn parse_shell_extraction_result` absent in `crates/reify-eval/src/engine_admin.rs` |
| ζ | `generic-results-iterator` | producer:ζ | PASS | grep `pub fn compute_results` present in `crates/reify-eval/src` |
| ζ | `shell-decoder-ratchet-row` | producer:ζ — shell-value-decoder row in α's member | PASS | manual: the ratchet member is the enforcement |
| ζ | `BT8-cold-equals-warm` | behavioural: shell_extract_persistent_cache_round_trip asserts cold Value == warm Value (today pins regions divergence) | PASS | manual: BT8 is the enforcement |
| ζ | `BT9-gui-view` | behavioural: shell_gui_mesh_data_returns_one_entry_with_valid_fields equivalent over the new free fn | PASS | manual: BT9 is the enforcement |
| ι | `tensegrity-load-fn-declared` | producer:ι — no .ri caller of solver::tensegrity_load at main | PASS | grep `@optimized\("solver::tensegrity_load"\)` present in `crates/reify-compiler/stdlib/tensegrity.ri` |
| ι | `tensegrity-load-result-declared` | producer:ι | PASS | grep `structure def TensegrityLoadResult` present in `crates/reify-compiler/stdlib/tensegrity.ri` |
| ι | `chunk-section` | producer:ι — no analysis surface in any MCP chunk at main | PASS | grep `## Tensegrity analysis` present in `crates/reify-mcp/src/tools/chunks/stdlib.md` |
| ι | `grammar-fixture` | grammar-fixture:tests/prd-gate/fixtures/s5_tensegrity_load_surface.ri — tree-sitter parse --quiet exit 0; reify check exit 0 at main (probe names) | PASS | manual: grammar gate evidence, committed with the PRD |
| ι | `BT12-eval` | behavioural: reify eval on ι's committed fixture prints converged: true with per-node displacements | PASS | manual: BT12 is the enforcement |
| η | `ri-surface-field` | producer:η | PASS | grep `enum RiSurface` present in `crates/reify-compute-contract/src` |
| η | `lsp-hand-list-gone` | grep: const ALL_PRODUCTION_COMPUTE_TARGETS hand copy (18 of 19) at main | PASS | grep `const ALL_PRODUCTION_COMPUTE_TARGETS` absent in `crates/reify-lsp/src/diagnostics.rs` |
| η | `internal-result-register` | producer:η | PASS | grep `ENGINE_INTERNAL_RESULT_TYPES` present in `crates` |
| η | `BT5-BT6` | behavioural: identity parity and register walk; seeded failures name the target / type | PASS | manual: BT5 and BT6 are the enforcement |
| κ₁ | `workspace-member` | producer:κ₁ | PASS | grep `"crates/reify-analysis"` present in `Cargo.toml` |
| κ₁ | `result-topology-moved` | grep: compute_targets::result_topology module in reify-eval at main | PASS | grep `mod result_topology` absent in `crates/reify-eval/src/compute_targets/mod.rs` |
| κ₁ | `dag-rows` | producer:κ₁ — no reify-analysis row in the crate-DAG gate at main | PASS | grep `reify-analysis` present in `scripts/assert-crate-dag.sh` |
| κ₁ | `transitional-edge-ratchet` | producer:κ₁ — eval-imports-analysis row in α's member (only falls; π → 0) | PASS | manual: the ratchet member is the enforcement |
| κ₁ | `C5-composed-key` | behavioural: BT10 one-byte flip on either side changes the composed key; closure guard twin registered (manifest row same diff) | PASS | manual: BT10 and the twin guard are the enforcement |
| κ₁ | `upstream-S15-code-leaves` | producer:#8674 #8679 #8683 #8686 #8690 #8693 #8697 #8698 #8702 #8706 #8711 (S15) and #8747 (S13 α) upstream | PASS | manual: dependency edges are the enforcement (program order S15 → S5) |
| κ₂ | `fdm-family-moved` | grep: mod as_printed_material in reify-eval compute_targets at main | PASS | grep `mod as_printed_material` absent in `crates/reify-eval/src/compute_targets/mod.rs` |
| κ₂ | `BT11-fdm-bracket` | behavioural: reify eval examples/fdm_bracket.ri identical before and after | PASS | manual: regression floor |
| λ | `trajectory-ops-moved` | grep: pub mod trajectory_ops in reify-eval at main | PASS | grep `pub mod trajectory_ops` absent in `crates/reify-eval/src/lib.rs` |
| λ | `stdlib-dynamics-key-moved` | grep: struct InverseDynamicsCacheKey in reify-stdlib at main | PASS | grep `struct InverseDynamicsCacheKey` absent in `crates/reify-stdlib/src` |
| λ | `upstream-S6-trajectory` | producer:#8848 #8850 #8854 (S6) upstream — trajectory/trampoline.rs partitioned to the cache keys; trajectory_ops TOTS codes | PASS | manual: dependency edges are the enforcement |
| λ | `BT11-dynamics-trajectory` | behavioural: dynamics and trajectory examples identical | PASS | manual: regression floor |
| μ | `modal-ops-moved` | grep: pub mod modal_ops in reify-eval at main | PASS | grep `pub mod modal_ops` absent in `crates/reify-eval/src/lib.rs` |
| μ | `stdlib-modal-key-moved` | grep: struct ModalCacheKey in reify-stdlib at main | PASS | grep `struct ModalCacheKey` absent in `crates/reify-stdlib/src` |
| μ | `upstream-S6-modal` | producer:#8837 (S6 κ) upstream — modal_ops record-key reads rewritten | PASS | manual: dependency edge is the enforcement |
| μ | `BT11-modal` | behavioural: modal examples identical; BT7 modal rows green | PASS | manual: regression floor |
| ν | `contract-drops-solver` | grep: reify-solver-elastic normal + dev deps in the contract Cargo.toml at main | PASS | grep `^reify-solver-elastic` absent in `crates/reify-compute-contract/Cargo.toml` |
| ν | `elastic-result-out-of-contract` | grep: pub struct ElasticResult in the contract at main | PASS | grep `pub struct ElasticResult` absent in `crates/reify-compute-contract/src` |
| ν | `fea-core-moved` | grep: mod elastic_static in reify-eval compute_targets at main | PASS | grep `mod elastic_static` absent in `crates/reify-eval/src/compute_targets/mod.rs` |
| ν | `BT1-cargo-tree` | behavioural: crate-DAG row — contract normal/build closure has no solver/kernel/eval/analysis | PASS | manual: the crate-DAG gate (crates/reify-build-utils/tests/crate_dag_assertion.rs) is the enforcement |
| ξ | `tensegrity-family-moved` | grep: mod form_find in reify-eval compute_targets at main | PASS | grep `mod form_find` absent in `crates/reify-eval/src/compute_targets/mod.rs` |
| ξ | `BT11-pavilion` | behavioural: examples/tensegrity_pavilion.ri and form-find examples identical; BT12 green | PASS | manual: regression floor |
| ο | `shell-extract-moved` | grep: mod shell_extract_compute in reify-eval at main | PASS | grep `mod shell_extract_compute` absent in `crates/reify-eval/src/lib.rs` |
| ο | `BT8-BT9-after-move` | behavioural: BT8/BT9 green after the move; fea_shell_flexure.ri identical | PASS | manual: boundary tests are the enforcement |
| π | `compute-targets-module-gone` | grep: pub mod compute_targets in reify-eval at main | PASS | grep `pub mod compute_targets` absent in `crates/reify-eval/src/lib.rs` |
| π | `BT2-eval-half` | behavioural: cargo tree -p reify-eval -e normal,build has no reify-analysis / reify-fdm / faer (crate-DAG row); eval-imports-analysis = 0 | PASS | manual: the crate-DAG gate and the ratchet row are the enforcement (faer stays a dev-dependency, so no Cargo.toml grep) |
| ρ | `contract-doc-amended` | docs: §2/§4 describe CancellationHandle per-dispatch fresh and the fns HashMap registry at main | PASS | manual: prose row; asserting wording is forbidden |
| ρ | `inv-pd-2-pointer` | docs: INV-PD-2 enforcement names PVAC (and, after S15 π, C4) only | PASS | manual: prose row; pointer only |
| σ | `terminal-stamp` | PRD-close: SHIPPED header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map | PASS | manual: committed header is the signal |
