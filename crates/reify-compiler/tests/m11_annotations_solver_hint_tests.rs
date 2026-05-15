//! Integration test pinning that `examples/m11_annotations.ri` exercises
//! `@solver_hint` with stdlib collection payloads after the Feature 7 block
//! is added (PRD `docs/prds/solver-hint-payloads.md`, acceptance bullet 1).
//!
//! Verifies:
//!   1. No Error-severity diagnostics.
//!   2. No Warning-severity diagnostics (per PRD acceptance "no warnings under reify check").
//!   3. `BoltedPanel.bolt_length` carries a
//!      `SolverHint { kind: DiscreteSet, collection: "standard_bolt_lengths" }`.
//!   4. `BoltedPanel.sheet_thickness` carries a
//!      `SolverHint { kind: PreferStock, collection: "standard_sheet_thicknesses" }`.

/// Absolute path to the example file, resolved at compile time from this
/// crate's manifest directory (two levels up to workspace root, then into
/// `examples/`).  Mirrors the pattern in
/// `crates/reify-eval/tests/m11_field_calculus.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m11_annotations.ri"
);

#[test]
fn m11_annotations_exercises_solver_hint_collection_payloads() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m11_annotations.ri should exist");

    // Compile with the stdlib prelude so `standard_bolt_lengths` and
    // `standard_sheet_thicknesses` resolve against `std.stock`.
    let module = reify_test_support::compile_source_with_stdlib(&source);

    // (1) No errors.
    let errors = reify_test_support::errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (2) No warnings (PRD acceptance: "producing no warnings under `reify check`").
    let warnings = reify_test_support::warnings_only(&module);
    assert!(
        warnings.is_empty(),
        "expected no Warning diagnostics, got: {:?}",
        warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Locate the BoltedPanel template by name — scoping to the specific template
    // produces clearer panic messages and guards against hints migrating to an
    // unrelated template while still passing the by-name cell assertions below.
    let bolted_panel = module
        .templates
        .iter()
        .find(|t| t.name == "BoltedPanel")
        .expect(
            "BoltedPanel template should be present in the compiled module — \
             did you forget to add the Feature 7 block to examples/m11_annotations.ri?"
        );

    // (3) BoltedPanel.bolt_length carries SolverHint { DiscreteSet, "standard_bolt_lengths" }.
    let bolt_length_cell = bolted_panel
        .value_cells
        .iter()
        .find(|c| c.id.member == "bolt_length")
        .expect("BoltedPanel should declare a `bolt_length` param");
    assert!(
        bolt_length_cell.solver_hints.iter().any(|h| {
            h.kind == reify_compiler::SolverHintKind::DiscreteSet
                && h.collection == "standard_bolt_lengths"
        }),
        "BoltedPanel.bolt_length should carry SolverHint \
         {{ kind: DiscreteSet, collection: \"standard_bolt_lengths\" }}, got: {:?}",
        bolt_length_cell.solver_hints,
    );

    // (4) BoltedPanel.sheet_thickness carries SolverHint { PreferStock, "standard_sheet_thicknesses" }.
    let sheet_thickness_cell = bolted_panel
        .value_cells
        .iter()
        .find(|c| c.id.member == "sheet_thickness")
        .expect("BoltedPanel should declare a `sheet_thickness` param");
    assert!(
        sheet_thickness_cell.solver_hints.iter().any(|h| {
            h.kind == reify_compiler::SolverHintKind::PreferStock
                && h.collection == "standard_sheet_thicknesses"
        }),
        "BoltedPanel.sheet_thickness should carry SolverHint \
         {{ kind: PreferStock, collection: \"standard_sheet_thicknesses\" }}, got: {:?}",
        sheet_thickness_cell.solver_hints,
    );
}
