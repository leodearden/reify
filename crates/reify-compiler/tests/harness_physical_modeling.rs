//! Consolidated integration-test harness for the physical-modelling subsystem —
//! what a part is MADE OF (material structs, SI / imperial / compound unit resolution,
//! physical constants), HOW it is MADE (process, printer print-envelope, stackup,
//! nominal markers), and the geometry-parameter primitives that describe it
//! dimensionally (solid params, rounded primitives, half-space, vec3, datum
//! construction and projection, zone slab, curvature arg-aware typing).
//!
//! Task #5694 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-4,
//! batch 4 of 6): folds 19 former standalone `tests/*.rs` binaries into this single
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
//! (`clippy::duplicate_mod`). Its four consumers (`compound_unit_resolution_tests`,
//! `imperial_units_tests`, `physical_constants_tests`, `si_units_tests`) reach it as
//! `use crate::common[::…]` — so this unit's `external_lines` of 363 is that charge,
//! expected rather than stray.
//!
//! `m9_error_cases` and `m11_annotations_solver_hint_tests` are here BY SCOPE, not by
//! subject: their prefixes fall inside leaf CMP-4 and no better in-scope home exists.
//! Moving them to one that fits later is a correction, not a regression.
//!
//! Routing vs. the three neighbours — the line is subsystem, not filename; do NOT fold
//! any of these together:
//! - `harness_units/` pins the stdlib unit SURFACE, and is bound to keep existing by
//!   `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3.
//! - `harness_units_materials/` holds the compiler-side unit MACHINERY, plus the
//!   materials / money / cost / affine clusters swept with it.
//! - THIS root holds the unit and constant CONSUMPTION tests — code that spends them.
//! - `harness_mechanics` (this leaf's sibling) holds what the part DOES under load.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_physical_modeling/compound_unit_resolution_tests.rs"]
mod compound_unit_resolution_tests;
#[path = "harness_physical_modeling/curvature_arg_aware_typing.rs"]
mod curvature_arg_aware_typing;
#[path = "harness_physical_modeling/datum_constructor_tests.rs"]
mod datum_constructor_tests;
#[path = "harness_physical_modeling/datum_projection_tests.rs"]
mod datum_projection_tests;
#[path = "harness_physical_modeling/half_space_compile_tests.rs"]
mod half_space_compile_tests;
#[path = "harness_physical_modeling/imperial_units_tests.rs"]
mod imperial_units_tests;
#[path = "harness_physical_modeling/m11_annotations_solver_hint_tests.rs"]
mod m11_annotations_solver_hint_tests;
#[path = "harness_physical_modeling/m9_error_cases.rs"]
mod m9_error_cases;
#[path = "harness_physical_modeling/material_struct_tests.rs"]
mod material_struct_tests;
#[path = "harness_physical_modeling/nominal_marker_typing_tests.rs"]
mod nominal_marker_typing_tests;
#[path = "harness_physical_modeling/physical_constants_tests.rs"]
mod physical_constants_tests;
#[path = "harness_physical_modeling/printer_print_envelope_example_tests.rs"]
mod printer_print_envelope_example_tests;
#[path = "harness_physical_modeling/process_stdlib_compile.rs"]
mod process_stdlib_compile;
#[path = "harness_physical_modeling/rounded_primitives_tests.rs"]
mod rounded_primitives_tests;
#[path = "harness_physical_modeling/si_units_tests.rs"]
mod si_units_tests;
#[path = "harness_physical_modeling/solid_param_tests.rs"]
mod solid_param_tests;
#[path = "harness_physical_modeling/stackup_stdlib_tests.rs"]
mod stackup_stdlib_tests;
#[path = "harness_physical_modeling/vec3_type_tests.rs"]
mod vec3_type_tests;
#[path = "harness_physical_modeling/zone_slab_compile_tests.rs"]
mod zone_slab_compile_tests;
