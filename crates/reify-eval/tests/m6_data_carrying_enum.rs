//! M6 data-carrying-enum end-to-end integration gate (task ζ #3946, step-3).
//!
//! Mirrors m5_integration.rs: parse → compile → eval; extract Widget.area.
//!
//! Tests:
//!   1. rect_default_area_is_200mm2 (PRIMARY §1 signal) — Rect {20mm×10mm} default
//!      → Widget.area = 0.0002 m² (si_value within 1e-12).
//!   2. circle_default_area_is_78_54mm2 — inline Circle{radius:5mm} default
//!      → Widget.area ≈ 0.00007853975 m² (π×(5mm)²).
//!   3. circle_undef_area_is_undef (D2 end-to-end) — Circle{radius:undef} default
//!      → Widget.outline = Shape::Circle (tag determined), Widget.area = Undef.

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

// ── helper ───────────────────────────────────────────────────────────────────

fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ── test 1: PRIMARY §1 signal ─────────────────────────────────────────────────

/// `reify eval examples/m6_data_carrying_enum.ri` → Widget.area ≈ 0.0002 m²
/// (20mm × 10mm = 200 mm² = 0.0002 m² SI).
///
/// The PRD §1 user-observable signal. Before step-2 (ζ fix), Widget.area = Undef.
#[test]
fn rect_default_area_is_200mm2() {
    let source = std::fs::read_to_string("../../examples/m6_data_carrying_enum.ri")
        .expect("examples/m6_data_carrying_enum.ri should exist");

    let result = eval_source(&source);

    let area_id = ValueCellId::new("Widget", "area");
    let area_val = result
        .values
        .get(&area_id)
        .unwrap_or_else(|| panic!("Widget.area not found in eval result"));

    match area_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.0002).abs() < 1e-12,
                "expected Widget.area ≈ 0.0002 m² (20mm×10mm), got {si_value} m²"
            );
        }
        other => panic!(
            "expected Value::Scalar for Widget.area, got {:?}",
            other
        ),
    }
}

// ── test 2: Circle{radius:5mm} switch-default ─────────────────────────────────

/// Inline source with `Circle { radius: 5mm }` default → area = π×(5mm)² ≈ 0.00007853975 m².
#[test]
fn circle_default_area_is_78_54mm2() {
    // Same enum/structure as m6_data_carrying_enum.ri but Circle default.
    let source = r#"
module m6_test_circle

enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point,
}

structure def Widget {
    param outline : Shape = Circle { radius: 5mm }

    let area = match outline {
        Circle { radius: r } => 3.14159 * r * r,
        Rect { width: w, height: h } => w * h,
        Point => 0mm * 0mm
    }
}
"#;

    let result = eval_source(source);

    let area_id = ValueCellId::new("Widget", "area");
    let area_val = result
        .values
        .get(&area_id)
        .unwrap_or_else(|| panic!("Widget.area not found in eval result"));

    // 3.14159 × (0.005)² = 3.14159 × 0.000025 = 0.000078539750
    let expected = 3.14159_f64 * 0.005 * 0.005;
    match area_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - expected).abs() < 1e-12,
                "expected Widget.area ≈ {expected} m² (π×(5mm)²), got {si_value} m²"
            );
        }
        other => panic!(
            "expected Value::Scalar for Widget.area, got {:?}",
            other
        ),
    }
}

// ── test 3: D2 end-to-end — Circle{radius:undef} ─────────────────────────────

/// `examples/m6_data_carrying_enum_undef.ri`: Circle {radius:undef} default.
/// The Circle arm IS selected (tag determined), but undef radius propagates →
/// Widget.area = Undef.
///
/// Also asserts Widget.outline = Shape::Circle to evidence the determined tag.
#[test]
fn circle_undef_area_is_undef() {
    let source = std::fs::read_to_string("../../examples/m6_data_carrying_enum_undef.ri")
        .expect("examples/m6_data_carrying_enum_undef.ri should exist");

    let result = eval_source(&source);

    // Widget.outline must be Shape::Circle (tag determined even with undef payload).
    let outline_id = ValueCellId::new("Widget", "outline");
    let outline_val = result
        .values
        .get(&outline_id)
        .unwrap_or_else(|| panic!("Widget.outline not found in eval result"));
    match outline_val {
        Value::Enum { variant, .. } => {
            assert_eq!(
                variant, "Circle",
                "Widget.outline should be Shape::Circle"
            );
        }
        other => panic!("expected Enum for Widget.outline, got {:?}", other),
    }

    // Widget.area must be Undef (Circle arm selected; undef radius propagates).
    let area_id = ValueCellId::new("Widget", "area");
    let area_val = result
        .values
        .get(&area_id)
        .unwrap_or_else(|| panic!("Widget.area not found in eval result"));
    assert!(
        area_val.is_undef(),
        "Widget.area should be Undef when Circle has undef radius; got {:?}",
        area_val
    );
}
