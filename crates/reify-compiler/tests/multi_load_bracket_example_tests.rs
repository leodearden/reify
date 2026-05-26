//! Dedicated regression tests for `examples/multi_load_bracket.ri` (task 3587).
//!
//! Pins the four task-spec leaf signals from
//! `docs/prds/v0_3/multi-load-case-fea.md` task #7, plus two additional
//! signals added by task 3647 (STANDARD_GRAVITY stdlib constant):
//!
//!   1. The file parses with zero errors.
//!   2. It compiles under the stdlib prelude with zero Error-severity diagnostics.
//!   3. The compiled module exposes a `MultiLoadBracket` structure template.
//!   4. The compiled template carries `operating`, `overload`, and `transport`
//!      value cells (pins ≥2 `LoadCase` leaf signal by compiled presence, not
//!      source-text matching), a `results` cell (pins ≥1 `MultiCaseResult`
//!      envelope), and a `width` param cell of type `Scalar<LENGTH>` (typed
//!      assertion mirroring `cost_aggregation_tests.rs:218-283`), plus
//!      source-text markers for `box(` geometry and at least one realistic-load
//!      constructor (`point_load(`, `traction_load(`, `body_force(`, or `gravity(`).
//!      Note: `pressure_load(` was removed from this list after SIR-β-load
//!      (task 3544) retired the snake_case builtin — `PressureLoad(...)` is now
//!      the valid form and uses the structure-def ctor path, not a builtin call.
//!   5. The source references `STANDARD_GRAVITY` (the std.units zero-arg pub fn)
//!      and does not contain the magic number `9.80665` inline (catches any
//!      identifier-renamed reconstruction, not just the original `let g_scalar` form).
//!
//! Mirrors the `cost_aggregation_example_compiles_under_stdlib_with_zero_errors`
//! pattern at `cost_aggregation_tests.rs:218-283`, including the typed value-cell
//! assertion (`AssemblyBOM.total_cost: Scalar<MONEY>` → here `width: Scalar<LENGTH>`).

use reify_types::{DimensionVector, ModulePath, Severity, Type};

// ─── examples/multi_load_bracket.ri compiles clean and pins leaf signals ─────

/// The canonical example file `examples/multi_load_bracket.ri` must parse,
/// compile under the stdlib prelude with zero Error diagnostics, expose a
/// `MultiLoadBracket` template, and contain the four leaf-signal API markers
/// mandated by the multi-load-case FEA PRD task #7.
///
/// Path resolution uses `CARGO_MANIFEST_DIR` so it works in any worktree.
#[test]
fn multi_load_bracket_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/multi_load_bracket.ri"
    );

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/multi_load_bracket.ri — check CARGO_MANIFEST_DIR resolution",
    );

    // ── Parse ─────────────────────────────────────────────────────────────────

    let parsed = reify_syntax::parse(&src, ModulePath::single("multi_load_bracket"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in multi_load_bracket.ri: {:?}",
        parsed.errors
    );

    // ── Compile ───────────────────────────────────────────────────────────────

    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling multi_load_bracket.ri under stdlib, got:\n{:#?}",
        errors
    );

    // ── Template presence ────────────────────────────────────────────────────

    let multi_load_bracket = module
        .templates
        .iter()
        .find(|t| t.name == "MultiLoadBracket")
        .unwrap_or_else(|| {
            panic!(
                "MultiLoadBracket template should be present in compiled multi_load_bracket.ri; \
                 found templates: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    // ── Value-cell and content-marker assertions (leaf-signal pinning) ────────
    //
    // The first three signals are checked against the compiled template's
    // value_cells rather than raw source text, so they cannot be satisfied by
    // comment text that happens to match the pattern.  The latter two use source-
    // text markers for API entry points that are not ambiguous with comments in
    // the example file.

    // Leaf signal: '≥2 LoadCase instances' — three named cells must compile.
    // Checking compiled value_cells is stronger than `src.matches("LoadCase(")`:
    // a comment mentioning LoadCase cannot satisfy this assertion.
    for cell_name in &["operating", "overload", "transport"] {
        assert!(
            multi_load_bracket
                .value_cells
                .iter()
                .any(|c| c.id.member == *cell_name),
            "leaf signal '≥2 LoadCase instances': expected MultiLoadBracket to carry a \
             '{}' value cell (compiled from a LoadCase constructor); found cells: {:?}",
            cell_name,
            multi_load_bracket
                .value_cells
                .iter()
                .map(|c| &c.id.member)
                .collect::<Vec<_>>()
        );
    }

    // Leaf signal: '≥1 MultiCaseResult envelope' — a 'results' cell must compile.
    // Checking value_cells avoids false positives from the comment that mentions
    // `MultiCaseResult(...)` in the example's engine-wiring note.
    assert!(
        multi_load_bracket
            .value_cells
            .iter()
            .any(|c| c.id.member == "results"),
        "leaf signal '≥1 MultiCaseResult envelope': expected MultiLoadBracket to carry a \
         'results' value cell; found cells: {:?}",
        multi_load_bracket
            .value_cells
            .iter()
            .map(|c| &c.id.member)
            .collect::<Vec<_>>()
    );

    // Typed cell assertion (mirrors cost_aggregation_tests.rs:218-283 pattern,
    // which asserts AssemblyBOM.total_cost: Scalar<MONEY>).  The 'width' param
    // carries an explicit `Length` annotation, so the compiler round-trip from
    // `param width : Length` → `Scalar<LENGTH>` is verifiable without type
    // inference ambiguity.
    let width_cell = multi_load_bracket
        .value_cells
        .iter()
        .find(|c| c.id.member == "width")
        .unwrap_or_else(|| {
            panic!(
                "MultiLoadBracket should carry a 'width' value cell; found cells: {:?}",
                multi_load_bracket
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        width_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "MultiLoadBracket.width should have type Scalar<LENGTH>, got {:?}",
        width_cell.cell_type
    );

    // Source-text markers for the remaining leaf signals.  These patterns do
    // not appear inside comments in the example file, so substring matching
    // is unambiguous here.
    assert!(
        src.contains("box("),
        "leaf signal 'plausible bracket geometry': expected src to contain 'box(' \
         (parametric box geometry for the bracket body)"
    );

    // `pressure_load(` removed from the OR-list after SIR-β-load (task 3544
    // step-7): the snake_case builtin is retired; `PressureLoad(...)` is the
    // valid form but uses the structure-def ctor path, not a builtin-style
    // `name(` call. The example file uses `point_load(` so the test stays green.
    let has_realistic_load = src.contains("point_load(")
        || src.contains("traction_load(")
        || src.contains("body_force(")
        || src.contains("gravity(");
    assert!(
        has_realistic_load,
        "leaf signal 'realistic loads': expected src to contain at least one of \
         point_load(, traction_load(, body_force(, gravity( \
         but none found"
    );

    // Task 3647 leaf signals: stdlib gravity constant in use; inline magic-number removed.
    // The positive STANDARD_GRAVITY pin verifies the stdlib symbol is consumed.
    // The negative 9.80665 pin is stronger than checking for a specific identifier
    // (e.g. `let g_scalar`): it catches any inline reconstruction regardless of the
    // binding name chosen, directly encoding 'no inline magic-number for gravity'.
    // Note: the comment in multi_load_bracket.ri deliberately does not repeat the
    // numeric value so this assertion remains unambiguous.
    assert!(
        src.contains("STANDARD_GRAVITY"),
        "leaf signal 'stdlib gravity constant in use': expected src to reference \
         STANDARD_GRAVITY (the std.units zero-arg pub fn) in the gravity load construction"
    );
    assert!(
        !src.contains("9.80665"),
        "leaf signal 'no inline gravity magic-number': expected src NOT to contain \
         the literal 9.80665 — gravity must be expressed via STANDARD_GRAVITY() rather \
         than reconstructed inline (catches any binding name, not just `let g_scalar`)"
    );
}
