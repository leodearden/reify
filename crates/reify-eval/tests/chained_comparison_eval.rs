//! Chained comparison evaluation tests.
//!
//! Tests the full parse → compile → eval pipeline for chained comparisons.
//! Chained comparisons desugar at compile time into And-chains of pairwise
//! comparisons. At eval time, comparisons use eval_cmp which checks dimension
//! compatibility (returns Undef on mismatch), and And uses Kleene three-valued
//! logic (false ∧ Undef = false, true ∧ Undef = Undef).

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::eval_source;

// ── step-1: chain_range_satisfied ─────────────────────────────────────────

/// `2mm < thickness < 10mm` with thickness=5mm → Bool(true).
/// Desugars to And(Lt(2mm, 5mm), Lt(5mm, 10mm)); both true ⇒ true.
#[test]
fn chain_range_satisfied() {
    let result = eval_source(
        r#"
structure S {
    param thickness : Length = 5mm
    let result = 2mm < thickness < 10mm
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(true),
        "2mm < 5mm < 10mm should be true, got: {:?}",
        val
    );
}

// ── step-2: chain_range_violated_below ────────────────────────────────────

/// `2mm < thickness < 10mm` with thickness=1mm → Bool(false).
/// Lt(2mm, 1mm) is false; And short-circuits to false.
#[test]
fn chain_range_violated_below() {
    let result = eval_source(
        r#"
structure S {
    param thickness : Length = 1mm
    let result = 2mm < thickness < 10mm
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "2mm < 1mm < 10mm should be false (below lower bound), got: {:?}",
        val
    );
}

// ── step-3: chain_range_violated_above ────────────────────────────────────

/// `2mm < thickness < 10mm` with thickness=15mm → Bool(false).
/// Lt(2mm, 15mm) is true but Lt(15mm, 10mm) is false ⇒ And is false.
#[test]
fn chain_range_violated_above() {
    let result = eval_source(
        r#"
structure S {
    param thickness : Length = 15mm
    let result = 2mm < thickness < 10mm
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "2mm < 15mm < 10mm should be false (above upper bound), got: {:?}",
        val
    );
}

// ── step-4: chain_boundary_strict_lt_at_lower ─────────────────────────────

/// `2mm < thickness < 10mm` with thickness=2mm → Bool(false).
/// Strict `<` excludes the lower boundary: Lt(2mm, 2mm) = false.
#[test]
fn chain_boundary_strict_lt_at_lower() {
    let result = eval_source(
        r#"
structure S {
    param thickness : Length = 2mm
    let result = 2mm < thickness < 10mm
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "2mm < 2mm (strict) should be false — boundary excluded, got: {:?}",
        val
    );
}

// ── step-5: chain_boundary_strict_lt_at_upper ─────────────────────────────

/// `2mm < thickness < 10mm` with thickness=10mm → Bool(false).
/// Strict `<` excludes the upper boundary: Lt(10mm, 10mm) = false.
#[test]
fn chain_boundary_strict_lt_at_upper() {
    let result = eval_source(
        r#"
structure S {
    param thickness : Length = 10mm
    let result = 2mm < thickness < 10mm
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "10mm < 10mm (strict) should be false — upper boundary excluded, got: {:?}",
        val
    );
}

// ── step-6: three_element_chain_all_satisfied ─────────────────────────────

/// `a < b < c < d` with a=1, b=2, c=3, d=4 → Bool(true).
/// Desugars to And(And(Lt(1,2), Lt(2,3)), Lt(3,4)); all true ⇒ true.
#[test]
fn three_element_chain_all_satisfied() {
    let result = eval_source(
        r#"
structure S {
    param a : Int = 1
    param b : Int = 2
    param c : Int = 3
    param d : Int = 4
    let result = a < b < c < d
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(true),
        "1 < 2 < 3 < 4 should be true, got: {:?}",
        val
    );
}

// ── step-7: three_element_chain_middle_violated ───────────────────────────

/// `a < b < c < d` with a=1, b=5, c=3, d=10 → Bool(false).
/// Lt(5,3) is false, making the And-chain false.
#[test]
fn three_element_chain_middle_violated() {
    let result = eval_source(
        r#"
structure S {
    param a : Int = 1
    param b : Int = 5
    param c : Int = 3
    param d : Int = 10
    let result = a < b < c < d
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "1 < 5 < 3 < 10: middle 5 < 3 is false ⇒ chain is false, got: {:?}",
        val
    );
}

// ── step-8: mixed_operators_le_lt_boundary ────────────────────────────────

/// `a <= b < c` with a=5, b=5, c=10 → Bool(true).
/// Le(5,5) is true (boundary included), Lt(5,10) is true ⇒ And is true.
#[test]
fn mixed_operators_le_lt_boundary() {
    let result = eval_source(
        r#"
structure S {
    param a : Int = 5
    param b : Int = 5
    param c : Int = 10
    let result = a <= b < c
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(true),
        "5 <= 5 < 10 should be true (Le boundary included), got: {:?}",
        val
    );
}

// ── step-9: mixed_operators_le_lt_violated ────────────────────────────────

/// `a <= b < c` with a=5, b=4, c=10 → Bool(false).
/// Le(5,4) is false ⇒ And short-circuits to false.
#[test]
fn mixed_operators_le_lt_violated() {
    let result = eval_source(
        r#"
structure S {
    param a : Int = 5
    param b : Int = 4
    param c : Int = 10
    let result = a <= b < c
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Bool(false),
        "5 <= 4 is false ⇒ chain is false, got: {:?}",
        val
    );
}

// ── step-10: undef_middle_position ────────────────────────────────────────

/// `0 < x < 100` where x is an auto param (no default, no solver) → Undef.
/// Lt(0, Undef) → Undef; And(Undef, ...) → Undef (Kleene logic).
#[test]
fn undef_middle_position() {
    let result = eval_source(
        r#"
structure S {
    param x : Int = auto
    let result = 0 < x < 100
}
"#,
    );
    let id = ValueCellId::new("S", "result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'result' not found in eval result"));
    assert_eq!(
        val,
        &Value::Undef,
        "0 < Undef < 100 should be Undef (Kleene three-valued logic), got: {:?}",
        val
    );
}

// ── step-11: dimensional_mismatch_rejected_at_compile_time ─────────────────

/// `2mm < mass_param < 10mm` where mass_param has MASS dimension (5kg) — a static
/// LENGTH-vs-MASS dimensional mismatch.
///
/// BEHAVIOR CHANGE (task 4490 type-hygiene guard): this mismatch is now REJECTED at
/// COMPILE time with a `dimension mismatch in comparison` diagnostic. Previously it
/// compiled and `eval_cmp` returned `Value::Undef` at runtime (silent indeterminacy);
/// making exactly this loud at compile time is 4490's headline user-observable signal.
#[test]
fn dimensional_mismatch_rejected_at_compile_time() {
    let source = r#"
structure S {
    param mass_param : Mass = 5kg
    let result = 2mm < mass_param < 10mm
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("chain_dim_mismatch"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("dimension mismatch in comparison")),
        "expected a compile-time dimension-mismatch error for `2mm < mass_param < 10mm` \
         (Length vs Mass); got: {errors:?}"
    );
}

// ── step-12: desugaring_structure_verified ────────────────────────────────

/// Compile `2mm < thickness < 10mm` in a let binding and inspect the IR.
/// Verifies: top-level BinOp::And, left is Lt(Literal(2mm), ValueRef(thickness)),
/// right is Lt(ValueRef(thickness), Literal(10mm)), result_type is Bool.
#[test]
fn desugaring_structure_verified() {
    use reify_core::Type;
    use reify_ir::{BinOp, CompiledExprKind};

    let source = r#"
structure S {
    param thickness : Length = 5mm
    let result = 2mm < thickness < 10mm
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("chain_desugar_verify"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    let result_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "result")
        .expect("'result' value cell should exist in compiled template");

    let init = result_cell
        .default_expr
        .as_ref()
        .expect("'result' let binding should have a default_expr");

    // result_type should be Bool
    assert_eq!(
        init.result_type,
        Type::Bool,
        "chained comparison let binding should have result_type Bool, got: {:?}",
        init.result_type
    );

    // Top-level: And(Lt(Literal(2mm), ValueRef(thickness)), Lt(ValueRef(thickness), Literal(10mm)))
    match &init.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(
                *op,
                BinOp::And,
                "top-level op should be And (desugared chain), got: {:?}",
                op
            );

            // left: Lt(Literal(2mm), ValueRef(thickness))
            match &left.kind {
                CompiledExprKind::BinOp {
                    op: lop,
                    left: ll,
                    right: lr,
                } => {
                    assert_eq!(
                        *lop,
                        BinOp::Lt,
                        "left pairwise should be Lt(2mm, thickness)"
                    );
                    assert!(
                        matches!(&ll.kind, CompiledExprKind::Literal(_)),
                        "left.left should be Literal(2mm), got: {:?}",
                        ll.kind
                    );
                    assert!(
                        matches!(&lr.kind, CompiledExprKind::ValueRef(_)),
                        "left.right should be ValueRef(thickness), got: {:?}",
                        lr.kind
                    );
                }
                other => panic!("expected BinOp(Lt) for left side of And, got: {:?}", other),
            }

            // right: Lt(ValueRef(thickness), Literal(10mm))
            match &right.kind {
                CompiledExprKind::BinOp {
                    op: rop,
                    left: rl,
                    right: rr,
                } => {
                    assert_eq!(
                        *rop,
                        BinOp::Lt,
                        "right pairwise should be Lt(thickness, 10mm)"
                    );
                    assert!(
                        matches!(&rl.kind, CompiledExprKind::ValueRef(_)),
                        "right.left should be ValueRef(thickness), got: {:?}",
                        rl.kind
                    );
                    assert!(
                        matches!(&rr.kind, CompiledExprKind::Literal(_)),
                        "right.right should be Literal(10mm), got: {:?}",
                        rr.kind
                    );
                }
                other => panic!("expected BinOp(Lt) for right side of And, got: {:?}", other),
            }
        }
        other => panic!(
            "expected BinOp(And) at top level of desugared chain, got: {:?}",
            other
        ),
    }
}
