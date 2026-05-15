//! Integration test pinning that `examples/m11_annotations.ri` exercises
//! `@solver_hint` with stdlib collection payloads after the Feature 7 block
//! is added (PRD `docs/prds/solver-hint-payloads.md`, acceptance bullet 1).
//!
//! Verifies:
//!   1. No Error-severity diagnostics.
//!   2. No Warning-severity diagnostics (per PRD acceptance "no warnings under reify check").
//!   3. At least one `ValueCellDecl` across all templates carries a
//!      `SolverHint { kind: DiscreteSet, collection: "standard_bolt_lengths" }`.
//!   4. At least one `ValueCellDecl` across all templates carries a
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
    // unrelated template while still passing the broad at-least-one assertions.
    let bolted_panel = module
        .templates
        .iter()
        .find(|t| t.name == "BoltedPanel")
        .expect(
            "BoltedPanel template should be present in the compiled module — \
             did you forget to add the Feature 7 block to examples/m11_annotations.ri?"
        );

    // (3) BoltedPanel has a cell with DiscreteSet + standard_bolt_lengths.
    let has_bolt_lengths = bolted_panel.value_cells.iter().any(|cell| {
        cell.solver_hints.iter().any(|h| {
            h.kind == reify_compiler::SolverHintKind::DiscreteSet
                && h.collection == "standard_bolt_lengths"
        })
    });
    assert!(
        has_bolt_lengths,
        "expected BoltedPanel to have a ValueCellDecl with SolverHint \
         {{ kind: DiscreteSet, collection: \"standard_bolt_lengths\" }}"
    );

    // (4) BoltedPanel has a cell with PreferStock + standard_sheet_thicknesses.
    let has_sheet_thicknesses = bolted_panel.value_cells.iter().any(|cell| {
        cell.solver_hints.iter().any(|h| {
            h.kind == reify_compiler::SolverHintKind::PreferStock
                && h.collection == "standard_sheet_thicknesses"
        })
    });
    assert!(
        has_sheet_thicknesses,
        "expected BoltedPanel to have a ValueCellDecl with SolverHint \
         {{ kind: PreferStock, collection: \"standard_sheet_thicknesses\" }}"
    );

    // Guard: the two hints must live on distinct cells.  A regression where both
    // hints are mistakenly attached to the same cell (and the other cell loses its
    // hint) would still satisfy each any() assertion above in isolation, so we
    // explicitly reject that configuration here.
    assert!(
        !bolted_panel.value_cells.iter().any(|cell| {
            let has_discrete = cell.solver_hints.iter().any(|h| {
                h.kind == reify_compiler::SolverHintKind::DiscreteSet
                    && h.collection == "standard_bolt_lengths"
            });
            let has_prefer = cell.solver_hints.iter().any(|h| {
                h.kind == reify_compiler::SolverHintKind::PreferStock
                    && h.collection == "standard_sheet_thicknesses"
            });
            has_discrete && has_prefer
        }),
        "DiscreteSet(standard_bolt_lengths) and PreferStock(standard_sheet_thicknesses) \
         must be on distinct ValueCellDecl entries, not both on the same cell"
    );
}
