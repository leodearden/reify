//! GD&T check-time legality diagnostics (task 4475 β).
//!
//! Integration tests for:
//! - C1: `Engine::enumerate_gdt_callouts` — the shared callout enumerator
//! - C2: `Engine::check` + `check_gdt_legality` — the rule-table legality pass
//!
//! Tests are added incrementally (steps 1–8); each step adds RED tests that
//! fail until the corresponding impl step makes them pass.

#[allow(unused_imports)]
use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── C1 enumerator contract (step-1 RED / step-2 GREEN) ────────────────────────

/// Parse and compile the given source with stdlib; return (module, values) pair.
fn eval_with_stdlib(source: &str) -> (reify_compiler::CompiledModule, reify_ir::ValueMap) {
    let module = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);
    (module, result.values)
}

/// C1-A: `enumerate_gdt_callouts` returns exactly the GeometricTolerance-conforming
/// instances in declaration order; a non-GDT instance is excluded.
///
/// Fixture declares (in order):
///   1. A `DimensionalTolerance` (non-GDT — does NOT conform to GeometricTolerance).
///   2. A `Flatness(material_condition: MaterialCondition.MMC, ...)` (GDT — Form family).
///
/// Expected: exactly 1 callout returned (the Flatness), in that slot; the
/// DimensionalTolerance is excluded.
#[test]
fn c1_enumerator_returns_gdt_instances_and_excludes_non_gdt() {
    const SOURCE: &str = r#"
structure def Fixture {
    let dim_tol = DimensionalTolerance(
        nominal: 10mm,
        upper_deviation: 0.05mm,
        lower_deviation: -0.05mm
    )
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    // Exactly one callout (the Flatness); DimensionalTolerance excluded.
    assert_eq!(
        callouts.len(),
        1,
        "expected exactly 1 GDT callout (Flatness); got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );

    let callout = &callouts[0];

    // type_name must be "Flatness"
    assert_eq!(
        callout.type_name, "Flatness",
        "expected type_name=Flatness, got {:?}",
        callout.type_name
    );

    // material_condition must be Some("MMC")
    assert_eq!(
        callout.material_condition.as_deref(),
        Some("MMC"),
        "expected material_condition=Some(MMC), got {:?}",
        callout.material_condition
    );

    // The instantiation span must be non-empty (not a prelude synthetic span).
    assert!(
        !callout.span.is_empty(),
        "expected non-empty instantiation span, got {:?}",
        callout.span
    );
}

/// C1-B: when a module contains no GeometricTolerance-conforming instances,
/// `enumerate_gdt_callouts` returns an empty vector.
#[test]
fn c1_enumerator_returns_empty_for_non_gdt_module() {
    const SOURCE: &str = r#"
structure def NoGdt {
    let dim_tol = DimensionalTolerance(
        nominal: 5mm,
        upper_deviation: 0.01mm,
        lower_deviation: -0.01mm
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    assert!(
        callouts.is_empty(),
        "expected empty callouts for non-GDT module, got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );
}

/// C1-C: `enumerate_gdt_callouts` returns multiple callouts in declaration order.
///
/// Fixture declares two GDT callouts: first a Flatness(RFS), then a Circularity(RFS).
/// Expected: [Flatness, Circularity] in that order.
#[test]
fn c1_enumerator_declaration_order_is_preserved() {
    const SOURCE: &str = r#"
structure def MultiGdt {
    let f = Flatness(
        tolerance_value: 0.1mm,
        feature: box(1mm, 1mm, 1mm)
    )
    let c = Circularity(
        tolerance_value: 0.05mm,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    assert_eq!(
        callouts.len(),
        2,
        "expected 2 callouts, got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );
    assert_eq!(callouts[0].type_name, "Flatness");
    assert_eq!(callouts[1].type_name, "Circularity");
    // Both default to RFS
    assert_eq!(callouts[0].material_condition.as_deref(), Some("RFS"));
    assert_eq!(callouts[1].material_condition.as_deref(), Some("RFS"));
}
