//! Consolidated integration-test harness for the physical-modelling subsystem —
//! what a part is MADE OF and HOW it is MADE, beside the geometry-parameter
//! primitives that describe it dimensionally.
//!
//! - MADE OF: material structs, SI / imperial / compound unit resolution, physical
//!   constants.
//! - HOW MADE: process, printer print-envelope, stackup, nominal markers.
//! - GEOMETRY PARAMETERS swept in with them: solid params, rounded primitives,
//!   half-space, vec3, datum construction and projection, zone slab, and
//!   curvature arg-aware typing.
//!
//! Task #5694 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-4,
//! batch 4 of 6): folds 19 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is
//! added or removed (invariant I3); the test *ids* change with the layout, which is
//! the designed consequence, not a regression.
//!
//! The layout contract those declarations obey — stem-named modules, the mandatory
//! `#[path]`, the kLOC cap and the baseline ratchet — is stated ONCE, in
//! `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, and mechanically enforced
//! by that guard. Deliberately not restated here.
//!
//! Why `#[path]` is not optional in this file: a harness root is an integration-test
//! CRATE root, so a bare `mod si_units_tests;` resolves against `tests/`, not against
//! `tests/harness_physical_modeling/`. Mid-move — while the old top-level
//! `tests/<stem>.rs` still exists — that would SILENTLY bind the stale file instead
//! of failing, and the fold would look green while testing the wrong source. The
//! explicit `#[path]` makes the pre-move state a hard error.
//!
//! This unit DOES declare `common`, by design rather than by oversight: four of its
//! members (`compound_unit_resolution_tests`, `imperial_units_tests`,
//! `physical_constants_tests`, `si_units_tests`) consume `tests/common/mod.rs`, so
//! this unit's `external_lines` of 363 is expected and charged on purpose, not stray.
//! It is declared ONCE here because a per-member `mod common;` would load the same
//! source repeatedly in one compile unit (`clippy::duplicate_mod`, an error under
//! `-D warnings`, even though nextest still passes); members reach it as
//! `use crate::common[::…]`.
//!
//! TWO MEMBERS ARE HERE BY SCOPE, NOT BY SUBJECT — record this so a later reader does
//! not mistake it for an accident: `m9_error_cases.rs` and
//! `m11_annotations_solver_hint_tests.rs` land in this unit because the `m9` / `m11`
//! prefixes fall inside leaf CMP-4's declared scope and no better in-scope subsystem
//! home exists, NOT because they are physical-modelling tests. If a future leaf grows
//! a home that actually fits them, moving them out is a correction, not a regression.
//!
//! Routing vs. the three neighbours, so future unit / material tests land correctly.
//! The line is subsystem, not filename — do NOT fold any of these together:
//! - `harness_units/` pins the stdlib unit SURFACE — which symbols `stdlib/units.ri`
//!   ships and how the display-label surfaces round-trip. It is bound by
//!   `docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md` §C3 and
//!   must keep existing.
//! - `harness_units_materials/` holds the compiler-side unit MACHINERY (`UnitEntry` /
//!   `UnitRegistry`, the `unit`-declaration pre-pass, dimension resolution) plus the
//!   materials / money / cost / affine clusters it was swept with.
//! - THIS root holds the stdlib unit and constant CONSUMPTION tests — code that spends
//!   those units and constants — swept with the batch-4 prefixes.
//! - `harness_mechanics` (the sibling added by this same leaf) holds mechanics and
//!   behaviour: what the part DOES under load or motion.
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
