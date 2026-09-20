//! Runtime eval tests for DFM constraint defs in std.process (task 4274, γ).
//!
//! Verifies that the new `Manufacturable`, `BendManufacturable`,
//! `DrawManufacturable`, and `DraftManufacturable` constraint defs produce
//! definite `Satisfaction::Violated` / `Satisfaction::Satisfied` results when
//! evaluated against pure-scalar process conformers (no geometry kernel needed).
//!
//! Geometry-backed rules (`FitsBuildVolume`, `FeatureManufacturable` over a
//! `Subtracting` conformer with a `Solid` param) are covered at compile-clean
//! level only in `crates/reify-compiler/tests/process_stdlib_compile.rs` — the
//! no-kernel `make_simple_engine()` cannot realize `bounding_box(solid)`.

use reify_eval::{CheckResult, ConstraintCheckEntry};
use reify_ir::Satisfaction;
use reify_test_support::check_source_with_stdlib;

// ─── test helper ──────────────────────────────────────────────────────────────

/// Find a constraint-check entry in `result` by entity name and label string.
///
/// Panics with a diagnostic listing all entries if not found, using a
/// consistent message format across all tests in this module.
fn find_constraint_result<'a>(
    result: &'a CheckResult,
    entity: &str,
    label: &str,
) -> &'a ConstraintCheckEntry {
    result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == entity && e.label == Some(label.to_string()))
        .unwrap_or_else(|| {
            panic!(
                "expected {entity} {label}; got: {:?}",
                result.constraint_results
            )
        })
}

// ─── step-1: universal Manufacturable — violated / satisfied ──────────────────

/// `UnderSpecWall` applies `Manufacturable(measured: 0.5mm, capability: 1mm)`.
/// Since `0.5mm >= 1mm` is false, the single predicate must report Violated.
///
/// `InSpecWall` applies `Manufacturable(measured: 2mm, capability: 1mm)`.
/// Since `2mm >= 1mm` is true, the single predicate must report Satisfied.
///
/// Pure scalars — no geometry kernel is needed.
///
/// RED: `Manufacturable` does not exist in `process.ri` yet → compile error →
/// `check_source_with_stdlib` panics at the `parse_and_compile_with_stdlib`
/// assertion (`"compile errors: [...]"`).
#[test]
fn manufacturable_violated_and_satisfied() {
    let source = r#"
import std.process

structure def UnderSpecWall {
    param wall     : Length = 0.5mm
    param min_feat : Length = 1mm
    constraint Manufacturable(measured: wall, capability: min_feat)
}

structure def InSpecWall {
    param wall     : Length = 2mm
    param min_feat : Length = 1mm
    constraint Manufacturable(measured: wall, capability: min_feat)
}
"#;

    let result = check_source_with_stdlib(source);

    // UnderSpecWall: 0.5mm >= 1mm → false → Violated
    assert_eq!(
        find_constraint_result(&result, "UnderSpecWall", "Manufacturable#0[0]").satisfaction,
        Satisfaction::Violated,
        "UnderSpecWall: 0.5mm >= 1mm should be Violated"
    );

    // InSpecWall: 2mm >= 1mm → true → Satisfied
    assert_eq!(
        find_constraint_result(&result, "InSpecWall", "Manufacturable#0[0]").satisfaction,
        Satisfaction::Satisfied,
        "InSpecWall: 2mm >= 1mm should be Satisfied"
    );
}

// ─── step-3: Forming scalar family — BendManufacturable / DrawManufacturable /
//             DraftManufacturable  ────────────────────────────────────────────

/// `StampedPanel` is a fully-scalar `Forming` conformer (no `Solid` params).
/// `UnderSpecPart` binds `proc = StampedPanel()` and applies the three Forming
/// constraint defs with values that should each produce Violated:
///   - bend_radius 1mm  < proc.min_bend_radius 2mm  → BendManufacturable Violated
///   - draw_depth  80mm > proc.max_draw_depth  50mm  → DrawManufacturable Violated
///   - draft       1deg < proc.draft_angle     3deg  → DraftManufacturable Violated
///
/// `InSpecPart` applies the same defs with in-spec values (5mm, 10mm, 5deg):
///   - bend_radius 5mm  >= 2mm  → BendManufacturable Satisfied
///   - draw_depth  10mm <= 50mm → DrawManufacturable Satisfied
///   - draft       5deg >= 3deg → DraftManufacturable Satisfied
///
/// RED: `BendManufacturable`, `DrawManufacturable`, `DraftManufacturable` do
/// not exist in `process.ri` yet → compile error → panic.
#[test]
fn forming_family_violated_and_satisfied() {
    let source = r#"
import std.process

structure def StampedPanel : Forming {
    param duration       : Time   = 10min
    param cost           : Money  = 5USD
    param min_bend_radius : Length = 2mm
    param max_draw_depth : Length = 50mm
    param draft_angle    : Angle  = 3deg
}

structure def UnderSpecPart {
    let proc         = StampedPanel()
    param bend_radius : Length = 1mm
    param draw_depth  : Length = 80mm
    param draft       : Angle  = 1deg
    constraint BendManufacturable(proc: proc, bend_radius: bend_radius)
    constraint DrawManufacturable(proc: proc, draw_depth: draw_depth)
    constraint DraftManufacturable(proc: proc, draft: draft)
}

structure def InSpecPart {
    let proc         = StampedPanel()
    param bend_radius : Length = 5mm
    param draw_depth  : Length = 10mm
    param draft       : Angle  = 5deg
    constraint BendManufacturable(proc: proc, bend_radius: bend_radius)
    constraint DrawManufacturable(proc: proc, draw_depth: draw_depth)
    constraint DraftManufacturable(proc: proc, draft: draft)
}
"#;

    let result = check_source_with_stdlib(source);

    // UnderSpecPart violations
    assert_eq!(
        find_constraint_result(&result, "UnderSpecPart", "BendManufacturable#0[0]").satisfaction,
        Satisfaction::Violated,
        "UnderSpecPart: 1mm >= 2mm should be Violated"
    );
    assert_eq!(
        find_constraint_result(&result, "UnderSpecPart", "DrawManufacturable#0[0]").satisfaction,
        Satisfaction::Violated,
        "UnderSpecPart: 80mm <= 50mm should be Violated"
    );
    assert_eq!(
        find_constraint_result(&result, "UnderSpecPart", "DraftManufacturable#0[0]").satisfaction,
        Satisfaction::Violated,
        "UnderSpecPart: 1deg >= 3deg should be Violated"
    );

    // InSpecPart satisfactions
    assert_eq!(
        find_constraint_result(&result, "InSpecPart", "BendManufacturable#0[0]").satisfaction,
        Satisfaction::Satisfied,
        "InSpecPart: 5mm >= 2mm should be Satisfied"
    );
    assert_eq!(
        find_constraint_result(&result, "InSpecPart", "DrawManufacturable#0[0]").satisfaction,
        Satisfaction::Satisfied,
        "InSpecPart: 10mm <= 50mm should be Satisfied"
    );
    assert_eq!(
        find_constraint_result(&result, "InSpecPart", "DraftManufacturable#0[0]").satisfaction,
        Satisfaction::Satisfied,
        "InSpecPart: 5deg >= 3deg should be Satisfied"
    );
}
