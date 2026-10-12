# Capability manifest: `trampoline-decode-and-result-build`

This manifest mechanizes G3 and G6 per leaf, per `.claude/skills/prd/project.md`. It covers the PRD `docs/prds/v0_6/trampoline-decode-and-result-build.md` (program stream S15, pass 1, author time).

All evidence was gathered against main `8e6ffcac9d` on 2026-10-12. Every path the PRD builds on is byte-identical to the review's pinned tree `a030ee7528`. Bindings cite by symbol and file, never by `path:line`.

This file is the human-readable twin of `docs/prds/v0_6/trampoline-decode-and-result-build.capability-manifest.yaml`, which carries the same bindings plus their `delivered_check` descriptors. **Keep the two in sync by hand.** A later correction to a check takes three writes: the `.yaml` descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

## Verdict at author time

Every one of the 34 `kind: grep` checks was executed with `git grep -E` against `HEAD` and confirmed **RED** at main.

- An `expect: present` check finds nothing today.
- An `expect: absent` check finds its target today.
- One check (`elastic-result-builder`) was caught GREEN during authoring. Three test-function names already contain `elastic_result_value`, so the builder was renamed `build_elastic_result` and the pattern anchored on `fn build_elastic_result\(`.

No `present` check points at a file that does not yet exist; each points at an existing file or directory, so `reify-audit --pattern PDCHECK` sees no dead pathspec. `manual` rows are excluded from the dispatch gate and name the behavioural test or upstream edge that enforces them.

**Upstream-producer bindings.** Leaves α, γ, δ, η and ο build on **#6922**'s landed shape (pending, under review on `task/6922`). α and θ follow **#6941**, and β follows **#7079**, **#7080** and **#7084**. These are recorded as `producer:#NNNN upstream` and wired as real `add_dependency` edges at decompose, never as prose ordering.

**G6.** No leaf asserts a tuned numeric constant.

- T3 is equality by construction: ctor-built values decode identically.
- T12's bound is one voxel spacing, the grid's own resolution, and its before-image is measured in the RED test.
- T11 is a rejection-plus-control pair (`von_mises([[100,0,0],[0,0,0],[0,0,0]]) = 100` exactly).
- The rejection premises T6 and T11 are marked for the D3 workflow's negative-assertion probe at decompose time.

## Bindings

| leaf | capability | binding | verdict | delivered_check |
|---|---|---|---|---|
| α | `stdlib-inputs-module-declared` | producer:α — new compute_targets::stdlib_inputs module; absent at main | PASS | grep `mod stdlib_inputs` present in `crates/reify-eval/src/compute_targets/mod.rs` |
| α | `compute-input-malformed-code` | producer:α — one new DiagnosticCode for non-dimension decode failures; DiagnosticCode is #[non_exhaustive] (additive) | PASS | grep `ComputeInputMalformed` present in `crates/reify-core/src/diagnostics.rs` |
| α | `modal-read-scalar-si-gone` | grep: fn read_scalar_si defined in modal_ops.rs at main (maps non-numeric to 0.0, dimension-blind) | PASS | grep `fn read_scalar_si` absent in `crates/reify-eval/src/modal_ops.rs` |
| α | `modal-material-reader-gone` | grep: fn extract_isotropic_material in modal_ops.rs at main (missing field -> 0.0) | PASS | grep `fn extract_isotropic_material` absent in `crates/reify-eval/src/modal_ops.rs` |
| α | `buckling-panicking-material-reader-gone` | grep: buckling::extract_material panics on malformed input at main; #6922 may already re-home it, either way the buckling-local reader is gone after α | PASS | grep `fn extract_material` absent in `crates/reify-eval/src/compute_targets/buckling.rs` |
| α | `ratchet-member-registered` | producer:α — tests/infra/test_trampoline_decode_ratchet.{py,sh}; manifest row lands same diff (esc-4914-162) | PASS | grep `test_trampoline_decode_ratchet` present in `tests/infra/run-all-classification.manifest` |
| α | `upstream-6922-shape` | producer:#6922 upstream (pending, under review on task/6922) — fea_inputs.rs / fea_value_shape.rs that α generalizes; re-measure after it lands | PASS | manual: upstream task shape; the dependency edge is the enforcement |
| α | `one-policy-T1-T2` | behavioural: T1/T2 — hand-built malformed / wrong-dimension ElasticMaterial gives the same coded Failed from all four trampolines (Rust test below the compiler, esc-7372-3 shape) | PASS | manual: behavioural boundary test; no grep can assert one failure policy |
| β | `eigen-knob-fallbacks-gone` | grep: extract_eigen_knobs restates ModalOptions defaults (10, 1e-9, 200) at main | PASS | grep `fn extract_eigen_knobs` absent in `crates/reify-eval/src/modal_ops.rs` |
| β | `buckling-option-fallbacks-gone` | grep: extract_buckling_options restates BucklingOptions defaults at main | PASS | grep `fn extract_buckling_options` absent in `crates/reify-eval/src/compute_targets/buckling.rs` |
| β | `shell-threshold-local-gone` | grep: shell_threshold = 0.2 restated as a Rust local in elastic_static at main | PASS | grep `let mut shell_threshold = 0\.2` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| β | `ctor-fills-defaults` | substrate: CompiledExprKind::StructureInstanceCtor.defaults fills every non-supplied param (reify_ir::expr); no production code hand-builds an options instance (all type_name: "ElasticOptions" constructions are #[cfg(test)]) | PASS | manual: substrate fact true at main; D1's premise |
| β | `pdrop-mechanism-upstream` | producer:#7079 upstream (pending) — C1 declaration mechanism; #7080/#7084 honors land first (D2) | PASS | manual: upstream dependency; edges are the enforcement |
| γ | `modal-string-support-decode-gone` | grep: enum DeclaredSupport (type_name string decode) in modal_ops.rs at main | PASS | grep `enum DeclaredSupport` absent in `crates/reify-eval/src/modal_ops.rs` |
| γ | `pressure-direction-typed` | producer:γ — PressureSpec.direction: String parsed and never read at main | PASS | grep `enum PressureDirection` present in `crates/reify-eval/src` |
| γ | `pressure-direction-string-field-gone` | grep: pub(crate) direction: String on PressureSpec at main | PASS | grep `pub\(crate\) direction: String` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| γ | `T6-unknown-direction-rejected` | behavioural: PressureLoad(direction: "tangential") -> reify eval exit 1 with a coded Error (negative-assertion: probe at decompose that it exits 0 today) | PASS | manual: rejection must be observed firing by the D3 workflow, not grepped |
| δ | `registry-free-constructor` | producer:δ — absent at main | PASS | grep `pub fn registry_free` present in `crates/reify-ir/src/value.rs` |
| δ | `private-consts-gone` | grep: 5 production const REGISTRY_FREE_TYPE_ID copies at main | PASS | grep `const REGISTRY_FREE_TYPE_ID` absent in `crates/reify-eval/src/appearance.rs`, `crates/reify-eval/src/compute_targets/as_printed_material.rs`, `crates/reify-eval/src/dynamics_ops.rs`, `crates/reify-stdlib/src/analysis.rs`, `crates/reify-stdlib/src/dynamics/eval.rs` |
| δ | `trait-object-arm-by-name` | grep: value_type_kind_matches TraitObject arm reads r.meta(data.type_id) at main (ctor instances carry the StructureTypeId(0) placeholder) | PASS | grep `r\.meta\(data\.type_id\)` absent in `crates/reify-eval/src/lib.rs` |
| ε | `warm-started-declared` | grep: structure def ElasticResult declares 15 params and no warm_started at main | PASS | grep `param warm_started : Bool` present in `crates/reify-compiler/stdlib/solver_elastic.ri` |
| ε | `elastic-result-builder` | producer:ε | PASS | grep `fn build_elastic_result\(` present in `crates/reify-eval/src` |
| ε | `field-layout-substrate` | substrate: StructureMeta.field_layout populated by populate_structure_registry at Engine::new (engine_new_populates_structure_registry_from_prelude) | PASS | manual: substrate fact true at main |
| ζ | `modal-result-builder` | producer:ζ | PASS | grep `fn build_modal_result\(` present in `crates/reify-eval/src` |
| ζ | `mechanism-builder-collapsed` | grep: mechanism_modal_result_value is a third hand-spelled ModalResult builder at main | PASS | grep `fn mechanism_modal_result_value` absent in `crates/reify-eval/src/modal_ops.rs` |
| ζ | `stale-6-fields-docstrings` | grep: two production docstrings claim '(6 fields' at main (scoped to the parenthesised production form; the test-module prose 'only had 6 fields' is not matched — rule 4) | PASS | grep `\(6 fields` absent in `crates/reify-eval/src/modal_ops.rs` |
| ζ | `topology-owner-live` | owner:#7097 (pending) — the undeclared-write list entry's owner | PASS | manual: liveness is checked by the C4 test's owner list, and by PVAC once #7102 lands |
| η | `eval-minter-gone` | grep: dynamics_ops::assemble_mass_properties is a second MassProperties minter at main | PASS | grep `fn assemble_mass_properties` absent in `crates/reify-eval/src/dynamics_ops.rs` |
| η | `eval-inertia-parser-gone` | grep: dynamics_psd::inertia_3x3_from_value is a second inertia parser at main | PASS | grep `pub fn inertia_3x3_from_value` absent in `crates/reify-eval/src/dynamics_psd.rs` |
| θ | `appearance-copy-gone` | grep: color_cell_f64 at main | PASS | grep `fn color_cell_f64` absent in `crates/reify-eval/src/appearance.rs` |
| θ | `bom-copy-gone` | grep: bom_report::value_as_f64 at main | PASS | grep `fn value_as_f64` absent in `crates/reify-eval/src/bom_report.rs` |
| θ | `gui-copy-gone` | grep: gui engine.rs scalar_to_f64 at main | PASS | grep `fn scalar_to_f64` absent in `gui/src-tauri/src/engine.rs` |
| ι | `truncation-note-coded` | producer:ι — 24 prefixes, 3 coded at main | PASS | grep `MechanismModalNModesTruncated` present in `crates/reify-core/src/diagnostics.rs` |
| ι | `T14-code-identity` | behavioural: every modal diagnostic test asserts DiagnosticCode identity; I_ note at Severity::Info | PASS | manual: multi-line emission site; behavioural tests are the enforcement |
| κ | `shared-symmetry-predicate` | producer:κ — the tolerance closure is copied 3x in analysis.rs at main | PASS | grep `fn is_symmetric_3x3` present in `crates/reify-stdlib/src/analysis.rs` |
| κ | `asymmetry-code` | producer:κ | PASS | grep `StressTensorAsymmetric` present in `crates/reify-core/src/diagnostics.rs` |
| κ | `T11-reachable-from-ri` | grammar/semantic: von_mises([[..]]) nested-list literal is the shipped spelling (examples/fields_analysis.ri header, fea_stress_reductions_smoke); asymmetric fixture panics in debug today — probe at decompose with reify eval | PASS | manual: rejection must be observed firing by the D3 workflow |
| λ | `one-failed-point` | behavioural: inline ComputeOutcome::Failed constructions in shell_extract_compute_fn fall from 13 to 1 (C6 inline-failed row) | PASS | manual: count assertion lives in the C6 ratchet row, not a grep descriptor |
| μ | `flat-index-accessor` | producer:μ — reify-ir states only 'Flat row-major data' at main | PASS | grep `pub fn flat_index_3d` present in `crates/reify-ir/src/value.rs` |
| μ | `medial-walk-cycle-broken` | grep: walk_direction imports medial::{normalize3, sample_at_index} while medial imports walk_direction at main | PASS | grep `use crate::medial::\{normalize3, sample_at_index\}` absent in `crates/reify-shell-extract/src/walk_direction.rs` |
| μ | `T12-slab-in-plane` | behavioural: mid-surface of build_slab_sdf(H) lies in z=0 within one voxel spacing; before-image measured in the RED test | PASS | manual: relational numeric bound (G6): one voxel spacing is the grid's own resolution |
| ν | `validation-variants-moved` | grep: DataLengthMismatch exists only in medial.rs (MedialError) at main | PASS | grep `DataLengthMismatch` present in `crates/reify-shell-extract/src/grid_validation.rs` |
| ν | `T13-no-panic` | behavioural: short data buffer / NaN spacing into extract_mid_surface returns a typed error instead of an index-out-of-bounds panic | PASS | manual: behavioural boundary test |
| ξ | `same-grid-predicate` | producer:ξ | PASS | grep `pub fn same_grid` present in `crates/reify-ir/src/value.rs` |
| ξ | `grids-equal-gone` | grep: fea::grids_equal duplicates grid_metadata_eq at main | PASS | grep `fn grids_equal` absent in `crates/reify-stdlib/src` |
| ξ | `fea-partitioned` | producer:ξ — fea.rs is one 6,617-line file at main | PASS | grep `mod envelope` present in `crates/reify-stdlib/src` |
| ο | `partition-shape` | h14: seven measured groups; precondition S6's MechanismTree extraction (fk-38811e6f7300) | PASS | manual: structural; the seat's partition table and the no-new-pub(super) rule are reviewed, not grepped |
| π | `inv-pd-2-row` | docs: design-invariants.md INV-PD-2 enforcement names only reify-audit --pattern PVAC at main | PASS | manual: prose row; asserting wording is forbidden (assert behaviour, not prose) |
| ρ | `chunk-signatures-compile` | docs-truth: every documented signature compiles in a smoke .ri (overlay docs-truth gate item 1) | PASS | manual: docs-truth acceptance is a smoke compile plus findability review |
| σ | `terminal-stamp` | PRD-close: SHIPPED header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map | PASS | manual: committed header is the signal |
