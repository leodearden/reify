//! Pre-pass consolidation characterization tests.
//!
//! These tests capture the order-independence contract that compile() must preserve
//! when consolidating the 4 separate declaration pre-passes into a single pass.
//! Each test verifies that declarations of different types can appear in any order
//! relative to their cross-type dependencies.

use reify_test_support::{compile_source, errors_only};

// ── Step 1: Characterization test with ALL declaration types ─────────────

/// Module with ALL declaration types (enum, function, trait, field, structure,
/// occurrence, purpose) where each type is declared BEFORE its dependencies.
/// This captures the order-independence contract the refactoring must preserve.
///
/// Ordering:
///   - function using enum match declared BEFORE the enum
///   - structure conforming to a trait declared AFTER it
///   - field referencing a function declared LATER
///   - occurrence and purpose at the end
#[test]
fn all_declaration_types_order_independent() {
    let source = r#"
        fn classify(x: Real) -> Int {
            match x {
                _ => 1
            }
        }

        field def temp : Point3 -> Scalar { source = analytical { |p| 1.0m } }

        structure S : Measurable {
            param width : Length = 80mm
            let v = classify(3.14)
        }

        trait Measurable {
            param width : Length
        }

        enum Direction { In, Out, Bidi }

        occurrence def Hole {
            param width : Length = 10mm
        }

        purpose check(subject : Structure) {
            constraint 80mm > 0mm
        }
    "#;

    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for order-independent declarations, got: {:?}",
        errors
    );

    // Verify all declaration types are compiled
    assert_eq!(module.enum_defs.len(), 1, "expected 1 enum");
    assert_eq!(module.enum_defs[0].name, "Direction");

    assert_eq!(module.functions.len(), 1, "expected 1 function");
    assert_eq!(module.functions[0].name, "classify");

    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait");
    assert_eq!(module.trait_defs[0].name, "Measurable");

    assert_eq!(module.fields.len(), 1, "expected 1 field");
    assert_eq!(module.fields[0].name, "temp");

    // Templates = structures + occurrences
    assert_eq!(
        module.templates.len(),
        2,
        "expected 2 templates (1 structure + 1 occurrence)"
    );
    assert_eq!(module.templates[0].name, "S");
    assert_eq!(module.templates[1].name, "Hole");

    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 purpose");
    assert_eq!(module.compiled_purposes[0].name, "check");

    // Verify the structure's let binding resolved the function call
    let template = &module.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");
    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "classify");
        }
        other => panic!("expected UserFunctionCall for classify, got {:?}", other),
    }
}

// ── Step 2: Function-before-enum ordering ────────────────────────────────

/// A function with a match expression on an enum variant, where the enum is
/// declared AFTER the function. Verifies that consolidating enum collection
/// and function compilation doesn't break forward references.
#[test]
fn function_before_enum_match_compiles() {
    let source = r#"
        fn to_int(d: Int) -> Int {
            match d {
                _ => 1
            }
        }

        structure S {
            let d = Direction.In
            let x = match d { In => 1, Out => 2, Bidi => 3 }
        }

        enum Direction { In, Out, Bidi }
    "#;

    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics when function is declared before enum, got: {:?}",
        errors
    );

    // The enum should be fully available
    assert_eq!(module.enum_defs.len(), 1);
    assert_eq!(module.enum_defs[0].name, "Direction");
    assert_eq!(module.enum_defs[0].variants, vec!["In", "Out", "Bidi"]);

    // The function should compile successfully
    assert_eq!(module.functions.len(), 1);
    assert_eq!(module.functions[0].name, "to_int");

    // The structure should have the match expression correctly compiled
    let template = &module.templates[0];
    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");
    let x_expr = x_cell.default_expr.as_ref().expect("let should have expr");
    match &x_expr.kind {
        reify_ir::CompiledExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 3, "expected 3 match arms for Direction");
        }
        other => panic!("expected Match expr, got {:?}", other),
    }

    // Verify enum forward reference for 'd' value
    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have 'd' value cell");
    let d_expr = d_cell.default_expr.as_ref().expect("let should have expr");
    match &d_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected Literal(Enum), got {:?}", other),
    }
}

// ── Step 3: Field-before-function ordering ───────────────────────────────

/// A field with an analytical source using a function call, where the function
/// is declared AFTER the field. Verifies that field compilation sees the
/// complete function list.
#[test]
fn field_before_function_compiles() {
    let source = r#"
        field def temp : Point3 -> Real { source = analytical { |p| scale(p) } }

        fn scale(x: Real) -> Real { x + x }
    "#;

    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics when field is declared before function, got: {:?}",
        errors
    );

    // Both should compile
    assert_eq!(module.fields.len(), 1, "expected 1 field");
    assert_eq!(module.fields[0].name, "temp");

    assert_eq!(module.functions.len(), 1, "expected 1 function");
    assert_eq!(module.functions[0].name, "scale");

    // The field's analytical source should contain a UserFunctionCall to 'scale'
    match &module.fields[0].source {
        reify_compiler::CompiledFieldSource::Analytical { expr } => {
            // The lambda body should contain a UserFunctionCall
            match &expr.kind {
                reify_ir::CompiledExprKind::Lambda { body, .. } => match &body.kind {
                    reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
                        assert_eq!(function_name, "scale");
                    }
                    other => panic!("expected UserFunctionCall in lambda body, got {:?}", other),
                },
                other => panic!("expected Lambda in analytical source, got {:?}", other),
            }
        }
        other => panic!("expected Analytical source, got: {:?}", other),
    }
}
