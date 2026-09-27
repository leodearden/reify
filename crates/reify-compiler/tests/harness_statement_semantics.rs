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
//! #5695) relocates all five to the diagnostics / annotations harness leaf CMP-6 creates,
//! and deletes this paragraph with them. It is the sibling of #5694's
//! `tkt_0RT273RG27CPVNCQXPBHHJEQCA`, which routes `harness_physical_modeling`'s
//! `m9_error_cases` and `m11_annotations_solver_hint_tests` to that same destination; the
//! two are expected to land as one change. Landing it is a correction, not a regression.
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
