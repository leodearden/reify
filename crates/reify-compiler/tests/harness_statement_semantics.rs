//! Consolidated integration-test harness for STATEMENT-LEVEL SEMANTICS — what a
//! statement means once its expressions type-check: quantified statements (`forall`,
//! keyed `forall`, quantifier lowering), constraint declaration and instantiation,
//! tolerancing, the stdlib surface (loader, determinacy purposes, parse-with-stdlib),
//! selector coercion and ad-hoc / nested / keyed selector resolution, lambdas and the
//! generate combinator, and string-interpolation lowering.
//!
//! Task #5695 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-5,
//! batch 5 of 6): folds 23 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count.
//!
//! Layout contract C1 — stem-named modules, why the `#[path]` on every declaration
//! below is mandatory rather than stylistic, the kLOC cap, the baseline ratchet, and
//! the by-design test-id change (no `#[test]` fn added or removed, invariant I3):
//! see `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, kept there, not
//! restated here.
//!
//! Crate-local: this harness declares the shared `common` helper ONCE, because a
//! per-member `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`). Its two consumers (`analysis_stress_fn_compile`,
//! `hoist_nested_selector_ctors`) reach it as `use crate::common[::…]` — so this unit's
//! `external_lines` charge is that one shared `tests/common/mod.rs`, expected rather
//! than stray. The line count itself is computed by `harness_layout_unit_lines`
//! (tests/infra/harness-layout-lib.sh) and deliberately not pinned here, where nothing
//! would recompute it.
//!
//! `aspect_massive_tests`, `analysis_stress_fn_compile`, `feature_datum_axis_example_tests`,
//! `display_annotation_tests` and `display_style_appearance_tests` are here BY SCOPE, not
//! by subject: their prefixes fall inside leaf CMP-5 and no better in-scope home existed.
//! None is a statement-semantics test — the first two are analysis/stress compile smoke,
//! the third a geometric-relations worked example, the last two annotations and display
//! style — so this root is a partial catch-all until they move. That move is TRACKED, not
//! merely disclosed: follow-up ticket `tkt_0RTJNNBDJAP0F0WVG8NGGR42MZ` (filed against
//! #5695) relocates all five to `harness_diagnostics_robustness` — the destination leaf
//! CMP-6 has since created, named here so the ticket is actionable without archaeology —
//! and deletes this paragraph and its sizing note with them. Its sibling
//! `tkt_0RT273RG27CPVNCQXPBHHJEQCA` (filed against #5694) is already DISCHARGED: #5696
//! routed `harness_physical_modeling`'s `m9_error_cases` and
//! `m11_annotations_solver_hint_tests` out of that root. The two were expected to land as
//! one change and did NOT: #5696 deliberately left these five alone, because they are not
//! top-level and so fall outside leaf CMP-6's sweep-up clause, and
//! because folding them would force a `#[path = "common/mod.rs"] mod common;` onto the
//! destination root (`analysis_stress_fn_compile` consumes it), charging that unit
//! `tests/common/mod.rs` as external lines it currently does not carry — a NEW charge
//! rather than a transfer, since `hoist_nested_selector_ctors` keeps that same include
//! here. This ticket therefore lands on its own. Landing it is a correction, not a
//! regression.
//!
//! SIZE THE MOVE BEFORE MAKING IT, and re-measure with `harness_layout_unit_lines`
//! (tests/infra/harness-layout-lib.sh) rather than trusting these figures — they are a
//! snapshot, in the stamped style of the `_KLOC_WARN_KNOWN` rows, not a maintained
//! number. Measured at #5696: the destination is 16063 lines across 23 members with
//! external_lines=0, i.e. 80% of C2's CAP_LINES=20000. These five add 1163 and `common`
//! adds 391, landing it near 17620 — clear of the cap, but under 400 lines short of the
//! WARN_PCT=90 advisory line at 18000, before any of those 23 members grow. A unit
//! ARRIVING at that line is RED until a `_KLOC_WARN_KNOWN` row is added in the same
//! diff, and that set is a ratchet meant to empty, so the row is a cost not a formality.
//! This ticket may therefore need to be paired with a split of the destination rather
//! than taken alone.
//!
//! Routing vs. the two sibling roots added by this same leaf — the line is subsystem, not
//! filename. `harness_type_checking` asks whether an EXPRESSION type-checks and
//! `harness_structure_declarations` whether a DECLARATION is well-formed; THIS root asks
//! what a STATEMENT does. A `forall` whose body fails to type-check is the first root's
//! business; that the `forall` lowers to the right per-element instantiation is this one's.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_statement_semantics/ad_hoc_selector_compile_tests.rs"]
mod ad_hoc_selector_compile_tests;
#[path = "harness_statement_semantics/analysis_stress_fn_compile.rs"]
mod analysis_stress_fn_compile;
#[path = "harness_statement_semantics/aspect_massive_tests.rs"]
mod aspect_massive_tests;
#[path = "harness_statement_semantics/constraint_def_compile_tests.rs"]
mod constraint_def_compile_tests;
#[path = "harness_statement_semantics/constraint_inst_tests.rs"]
mod constraint_inst_tests;
#[path = "harness_statement_semantics/display_annotation_tests.rs"]
mod display_annotation_tests;
#[path = "harness_statement_semantics/display_style_appearance_tests.rs"]
mod display_style_appearance_tests;
#[path = "harness_statement_semantics/feature_datum_axis_example_tests.rs"]
mod feature_datum_axis_example_tests;
#[path = "harness_statement_semantics/forall_statement_lower_tests.rs"]
mod forall_statement_lower_tests;
#[path = "harness_statement_semantics/forall_statement_stub_tests.rs"]
mod forall_statement_stub_tests;
#[path = "harness_statement_semantics/generate_combinator_tests.rs"]
mod generate_combinator_tests;
#[path = "harness_statement_semantics/hoist_nested_selector_ctors.rs"]
mod hoist_nested_selector_ctors;
#[path = "harness_statement_semantics/index_access_selector_coercion_tests.rs"]
mod index_access_selector_coercion_tests;
#[path = "harness_statement_semantics/keyed_forall_lower_tests.rs"]
mod keyed_forall_lower_tests;
#[path = "harness_statement_semantics/keyed_sub_resolution_tests.rs"]
mod keyed_sub_resolution_tests;
#[path = "harness_statement_semantics/lambda_compile_tests.rs"]
mod lambda_compile_tests;
#[path = "harness_statement_semantics/list_helper_selector_coercion_tests.rs"]
mod list_helper_selector_coercion_tests;
#[path = "harness_statement_semantics/parse_with_stdlib_tests.rs"]
mod parse_with_stdlib_tests;
#[path = "harness_statement_semantics/quantifier_compile_tests.rs"]
mod quantifier_compile_tests;
#[path = "harness_statement_semantics/stdlib_determinacy_purposes.rs"]
mod stdlib_determinacy_purposes;
#[path = "harness_statement_semantics/stdlib_loader_tests.rs"]
mod stdlib_loader_tests;
#[path = "harness_statement_semantics/string_interp_lowering_tests.rs"]
mod string_interp_lowering_tests;
#[path = "harness_statement_semantics/tolerancing_tests.rs"]
mod tolerancing_tests;
