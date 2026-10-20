# Capability manifest: `trampoline-decode-and-result-build`

This manifest mechanizes G3 and G6 per leaf, per `.claude/skills/prd/project.md`. It covers the PRD `docs/prds/v0_6/trampoline-decode-and-result-build.md` (program stream S15, pass 1, author time).

All evidence was gathered against main `8e6ffcac9d` on 2026-10-12. Every path the PRD builds on is byte-identical to the review's pinned tree `a030ee7528`. Bindings cite by symbol and file, never by `path:line`.

This file is the human-readable twin of `docs/prds/v0_6/trampoline-decode-and-result-build.capability-manifest.yaml`, which carries the same bindings plus their `delivered_check` descriptors. **Keep the two in sync by hand.** A later correction to a check takes three writes: the `.yaml` descriptor, this `.md` row, and the producer task's complete `metadata.delivered_checks` list.

## Verdict at author time

Every one of the 34 `kind: grep` checks was executed with `git grep -E` against `HEAD` and confirmed **RED** at main.

- An `expect: present` check finds nothing today.
- An `expect: absent` check finds its target today.
- One check (`elastic-result-builder`) was caught GREEN during authoring. Three test-function names already contain `elastic_result_value`, so the builder was renamed `build_elastic_result` and the pattern anchored on `fn build_elastic_result\(`.

**Retro-critic sitting 2026-10-20 (R5).** Three grep checks were dropped: α's `buckling-panicking-material-reader-gone` and `modal-read-scalar-si-gone`, which #6922 and #6941 green before α runs, and γ's doc-prose `support-unknown-variant-no-fallback`. Two were re-anchored: β's `eigen-knob-fallbacks-gone` and ν's `validation-variants-moved`. The κ T11 binding is respelled with `matrix([[…Pa]])`.

No `present` check points at a file that does not yet exist; each points at an existing file or directory, so `reify-audit --pattern PDCHECK` sees no dead pathspec. `manual` rows are excluded from the dispatch gate and name the behavioural test or upstream edge that enforces them.

**Upstream-producer bindings.** Leaves α, γ, δ, η and ο build on **#6922**'s landed shape (pending, under review on `task/6922`). α and θ follow **#6941**. **#7079**, **#7080** and **#7084** depend on β, not the reverse (lead ruling OQ2 (b)). ο was held on S6 (`x_seam_wait`) until S6 released it on 2026-10-13 by wiring #8845, #8847 and #8839. These are recorded as `producer:#NNNN upstream` and wired as real `add_dependency` edges at decompose, never as prose ordering.

**G6.** No leaf asserts a tuned numeric constant.

- T3 is equality by construction: ctor-built values decode identically.
- T12's bound is one voxel spacing, the grid's own resolution, and its before-image is measured in the RED test.
- T11 is a rejection-plus-control pair (`von_mises(matrix([[100.0Pa, 0Pa, 0Pa], [0Pa, 0Pa, 0Pa], [0Pa, 0Pa, 0Pa]])) = 100 Pa` exactly; respelled by R5 M6).
- The rejection premises T6 and T11 are marked for the D3 workflow's negative-assertion probe at decompose time.

## Bindings

| leaf | capability | binding | verdict | delivered_check |
|---|---|---|---|---|
| α | `stdlib-inputs-module-declared` | producer:α — new compute_targets::stdlib_inputs module; absent at main | PASS | grep `mod stdlib_inputs` present in `crates/reify-eval/src/compute_targets/mod.rs` |
| α | `compute-input-malformed-code` | producer:α — one new DiagnosticCode for non-dimension decode failures; DiagnosticCode is #[non_exhaustive] (additive) | PASS | grep `ComputeInputMalformed` present in `crates/reify-core/src/diagnostics.rs` |
| α | `modal-material-reader-gone` | grep: fn extract_isotropic_material in modal_ops.rs at main (missing field -> 0.0) | PASS | grep `fn extract_isotropic_material` absent in `crates/reify-eval/src/modal_ops.rs` |
| α | `ratchet-member-registered` | producer:μ #8711 (rows member on #8730's rust_construct_ratchet engine, R5 M2; no bespoke engine) — tests/infra/test_trampoline_decode_ratchet.{py,sh}; manifest row lands in the creating diff (esc-4914-162); α adds its rows | PASS | grep `test_trampoline_decode_ratchet` present in `tests/infra/run-all-classification.manifest` |
| α | `upstream-6922-shape` | producer:#6922 upstream (pending, under review on task/6922) — fea_inputs.rs / fea_value_shape.rs / FieldRejection / failed_outcome that α generalizes (R5 M3); re-measure after it lands. R5 polarity: modal-read-scalar-si-gone and buckling-panicking-material-reader-gone dropped (greened by #6941/#6922 before α) | PASS | manual: upstream task shape; the dependency edge is the enforcement |
| α | `one-policy-T1-T2` | behavioural: T1/T2 — hand-built malformed / wrong-dimension ElasticMaterial gives the same coded Failed from all four trampolines (Rust test below the compiler, esc-7372-3 shape) | PASS | manual: behavioural boundary test; no grep can assert one failure policy |
| β | `eigen-knob-fallbacks-gone` | grep: extract_eigen_knobs restates ModalOptions defaults (10, 1e-9, 200) at main (R5 polarity: anchored on `fn extract_eigen_knobs\(` so the two #[cfg(test)] extract_eigen_knobs_* fns do not match) | PASS | grep `fn extract_eigen_knobs\(` absent in `crates/reify-eval/src/modal_ops.rs` |
| β | `buckling-option-fallbacks-gone` | grep: extract_buckling_options restates BucklingOptions defaults at main | PASS | grep `fn extract_buckling_options` absent in `crates/reify-eval/src/compute_targets/buckling.rs` |
| β | `shell-threshold-local-gone` | grep: shell_threshold = 0.2 restated as a Rust local in elastic_static at main | PASS | grep `let mut shell_threshold = 0\.2` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| β | `ctor-fills-defaults` | substrate: CompiledExprKind::StructureInstanceCtor.defaults fills every non-supplied param (reify_ir::expr); no production code hand-builds an options instance (all type_name: "ElasticOptions" constructions are #[cfg(test)]). R5 B1: absent is hand-built-only; Undef is ctor-reachable (eval_structure_instance_ctor keeps an Undef slot), so Undef takes the per-position policy | PASS | manual: substrate fact true at main; D1's premise |
| β | `pdrop-builds-on-beta` | consumer:#7079/#7080/#7084 downstream (lead ruling OQ2 (b), D2) — they depend on β; β honors nothing new and decodes today's read set | PASS | manual: downstream dependency edges are the enforcement |
| γ | `modal-string-support-decode-gone` | grep: enum DeclaredSupport (type_name string decode) in modal_ops.rs at main | PASS | grep `enum DeclaredSupport` absent in `crates/reify-eval/src/modal_ops.rs` |
| γ | `pressure-direction-typed` | producer:γ — PressureSpec.direction: String parsed and never read at main | PASS | grep `enum PressureDirection` present in `crates/reify-eval/src` |
| γ | `pressure-direction-string-field-gone` | grep: pub(crate) direction: String on PressureSpec at main | PASS | grep `pub\(crate\) direction: String` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs` |
| γ | `T6-unknown-direction-rejected` | behavioural: PressureLoad(direction: "tangential") -> reify eval exit 1 with a coded Error (negative-assertion: probe at decompose that it exits 0 today) | PASS | manual: rejection must be observed firing by the D3 workflow, not grepped |
| δ | `registry-free-constructor` | producer:δ — absent at main | PASS | grep `pub fn registry_free` present in `crates/reify-ir/src` (widened from `value.rs` 2026-10-14 by the S10 decompose session: S10 α₄ splits value.rs into `value/`) |
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
| θ | `gui-copy-gone` | grep: gui engine.rs scalar_to_f64 at main (path widened 2026-10-14: S25 δ #8940 moves the copy into engine/mechanism.rs and engine/tensegrity.rs) | PASS | grep `fn scalar_to_f64` absent in `gui/src-tauri/src` |
| ι | `truncation-note-coded` | producer:ι — 24 prefixes, 3 coded at main | PASS | grep `MechanismModalNModesTruncated` present in `crates/reify-core/src/diagnostics.rs` |
| ι | `T14-code-identity` | behavioural: every modal diagnostic test asserts DiagnosticCode identity; I_ note at Severity::Info | PASS | manual: multi-line emission site; behavioural tests are the enforcement |
| κ | `shared-symmetry-predicate` | producer:κ — the tolerance closure is copied 3x in analysis.rs at main | PASS | grep `fn is_symmetric_3x3` present in `crates/reify-stdlib/src/analysis.rs` |
| κ | `asymmetry-code` | producer:κ | PASS | grep `StressTensorAsymmetric` present in `crates/reify-core/src/diagnostics.rs` |
| κ | `T11-reachable-from-ri` | grammar/semantic (R5 M6 respelled): von_mises(matrix([[1.0Pa, 2.0Pa, 0Pa], [0Pa, 0Pa, 0Pa], [0Pa, 0Pa, 0Pa]])) panics in debug and returns the upper-triangle answer in release today; control matrix([[100.0Pa, 0Pa, 0Pa], ...]) -> 100 Pa. The bare nested list is not a Matrix argument (Undef via OpContractViolation, exit 0) and gets a coded diagnostic from κ's classifier | PASS | manual: rejection must be observed firing by the D3 workflow |
| λ | `one-failed-point` | behavioural: inline ComputeOutcome::Failed constructions in shell_extract_compute_fn fall from 13 to 1 (C6 inline-failed row) | PASS | manual: count assertion lives in the C6 ratchet row, not a grep descriptor |
| μ | `flat-index-accessor` | producer:μ — reify-ir states only 'Flat row-major data' at main | PASS | grep `pub fn flat_index_3d` present in `crates/reify-ir/src` (widened from `value.rs` 2026-10-14 by the S10 decompose session: S10 α₄ splits value.rs into `value/`) |
| μ | `medial-walk-cycle-broken` | grep: walk_direction imports medial::{normalize3, sample_at_index} while medial imports walk_direction at main | PASS | grep `use crate::medial::\{normalize3, sample_at_index\}` absent in `crates/reify-shell-extract/src/walk_direction.rs` |
| μ | `T12-slab-in-plane` | behavioural: mid-surface of build_slab_sdf(H) lies in z=0 within one voxel spacing; before-image measured in the RED test | PASS | manual: relational numeric bound (G6): one voxel spacing is the grid's own resolution |
| ν | `validation-variants-moved` | grep: DataLengthMismatch exists only in medial.rs (MedialError) at main (R5 polarity: anchored on the variant declaration so a doc mention does not satisfy it) | PASS | grep `^\s*DataLengthMismatch\b` present in `crates/reify-shell-extract/src/grid_validation.rs` |
| ν | `T13-no-panic` | behavioural: short data buffer / NaN spacing into extract_mid_surface returns a typed error instead of an index-out-of-bounds panic | PASS | manual: behavioural boundary test |
| ξ | `same-grid-predicate` | producer:ξ | PASS | grep `pub fn same_grid` present in `crates/reify-ir/src` (widened from `value.rs` 2026-10-14 by the S10 decompose session: S10 α₄ splits value.rs into `value/`) |
| ξ | `grids-equal-gone` | grep: fea::grids_equal duplicates grid_metadata_eq at main | PASS | grep `fn grids_equal` absent in `crates/reify-stdlib/src` |
| ξ | `fea-partitioned` | producer:ξ — fea.rs is one 6,617-line file at main | PASS | grep `mod envelope` present in `crates/reify-stdlib/src` |
| ο | `partition-shape` | h14: seven measured groups; preconditions wired by the S6 session 2026-10-13: S6 ρ #8845 (MechanismTree, fk-38811e6f7300), σ #8847 (dynamics on Joint), λ #8839 (bindings move); released by S6, no longer held | PASS | manual: structural; the seat's partition table and the no-new-pub(super) rule are reviewed, not grepped |
| π | `inv-pd-2-row` | docs: design-invariants.md INV-PD-2 enforcement names only reify-audit --pattern PVAC at main | PASS | manual: prose row; asserting wording is forbidden (assert behaviour, not prose) |
| ρ | `chunk-signatures-compile` | docs-truth: every documented signature compiles in a smoke .ri (overlay docs-truth gate item 1) | PASS | manual: docs-truth acceptance is a smoke compile plus findability review |
| σ | `terminal-stamp` | PRD-close: SHIPPED header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map | PASS | manual: committed header is the signal |
| ζ₀ | `degenerate-modal-builder-gone` | grep: fn degenerate_modal_result in modal_ops.rs at main (fabricated norms 0.0, boundary_conditions [], modes [] on the 9 E_ error paths, returned as Completed) | PASS | grep `fn degenerate_modal_result` absent in `crates` |
| ζ₀ | `degenerate-history-builder-gone` | grep: fn degenerate_displacement_history in modal_ops.rs at main (transient error paths returned as Completed) | PASS | grep `fn degenerate_displacement_history` absent in `crates` |
| ζ₀ | `completed-constructor` | producer:ζ₀ — reify-compute-contract has no ComputeOutcome constructor at main (S5 owns the crate per G4: cross-reference) | PASS | grep `pub fn completed\(` present in `crates/reify-compute-contract/src` |
| ζ₀ | `modal-error-paths-failed` | behavioural: each of the nine modal E_ mnemonics' scenarios yields ComputeOutcome::Failed (cell Undef via mark_failed), never Completed beside a value | PASS | manual: behavioural tests in the existing modal binaries are the enforcement |
| τ | `multi-case-builder` | producer:τ — solve_multi_case_trampoline returns Value::Map{cases} at main; no builder for the declared MultiCaseResult | PASS | grep `fn build_multi_case_result\(` present in `crates` |
| τ | `multi-case-buckling-builder` | producer:τ — solve_buckling_multi_case_trampoline returns Value::Map{cases} at main; no builder for the declared MultiCaseBucklingResult | PASS | grep `fn build_multi_case_buckling_result\(` present in `crates` |
| τ | `multi-case-kind` | producer:τ — detect_multi_case_result matches any Map whose cases value is a non-empty Map at main | PASS | grep `enum MultiCaseKind` present in `crates` |
| τ | `upstream-multi-case` | producer:#8999 #8686 #8690 #9219 #8943 upstream (G1 follow-on after S5 κ₁) | PASS | manual: dependency edges are the enforcement |
| υ | `slice-settings-default-literals-gone` | grep: fdm_slice::read_slice_settings restates FDMProcess .ri defaults with unwrap_or literals at main (layer height 0.2 mm and others). R5 polarity: also matches a comment (fdm_slice.rs:343) and, after #6922, infill_density.unwrap_or(0.2); υ re-anchors this check on the slice-settings fields it removes before landing (three writes) | PASS | grep `unwrap_or\(0\.2\)` absent in `crates/reify-eval/src/compute_targets/fdm_slice.rs` |
| υ | `decoded-param-read-fdm` | behavioural: C6 decoded-param-read rows for FDMProcess, AsPrintedOptions and FDMCouponOverride at 0; both rungs Failed with a coded diagnostic on a missing declared field | PASS | manual: the C6 ratchet rows and behavioural tests are the enforcement |
| υ | `upstream-6922` | producer:#6922 #8674 upstream (#6922 in progress on task/6922 rewrites the three FDM files; re-measure after it lands) | PASS | manual: dependency edges are the enforcement |
| φ | `stdlib-tuple-decoder-gone` | grep: reify-stdlib tensegrity.rs extract_index_tuples is a second Tensegrity decoder (Option, silent Undef) at main | PASS | grep `fn extract_index_tuples` absent in `crates/reify-stdlib/src/tensegrity.rs` |
| φ | `tensegrity-topology-decoder` | producer:φ — no shared Tensegrity topology decoder at main | PASS | grep `struct TensegrityTopology` present in `crates` |
| φ | `one-parse-two-policies` | behavioural: a malformed member index gives the located E_FormFindInfeasible on solve and Undef in the viewport, from the same decoder; tensegrity golden and viewport e2e unchanged | PASS | manual: behavioural tests are the enforcement |
