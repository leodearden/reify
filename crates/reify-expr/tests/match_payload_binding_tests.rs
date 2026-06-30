//! Match payload-binding evaluation tests (task ζ #3946, step-1).
//!
//! Pins the full eval contract for `CompiledPattern::VariantBind`:
//!   INV-2 — binder scope confined to the matched arm's body.
//!   INV-3 — arm selected by tag only (not payload determinacy).
//!   INV-4/D2 — determined tag + undef payload field → arm selected, binder = Undef.
//!   INV-5 — bare Variant / Wildcard arms are byte-for-byte unchanged.
//!
//! These are unit tests against `reify_expr::{eval_expr, EvalContext}` +
//! `reify_ir::{CompiledExpr, CompiledMatchArm, CompiledPattern, Value, ValueMap}`.

use reify_core::{DimensionVector, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, CompiledMatchArm, CompiledPattern, Value, ValueMap};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a `Value::Scalar` with LENGTH dimension from millimetres (→ metres SI).
fn mm(v: f64) -> Value {
    Value::Scalar {
        si_value: v * 0.001,
        dimension: DimensionVector::LENGTH,
    }
}

/// `Type::Scalar` for LENGTH.
fn t_length() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

/// `Type::Scalar` for AREA (m²).
fn t_area() -> Type {
    Type::Scalar {
        dimension: DimensionVector::AREA,
    }
}

/// Build a `Value::Enum` for variant "Rect" with width/height payload.
fn rect_enum(width_m: f64, height_m: f64) -> Value {
    Value::Enum {
        type_name: "Shape".to_string(),
        variant: "Rect".to_string(),
        payload: vec![
            (
                "width".to_string(),
                Value::Scalar {
                    si_value: width_m,
                    dimension: DimensionVector::LENGTH,
                },
            ),
            (
                "height".to_string(),
                Value::Scalar {
                    si_value: height_m,
                    dimension: DimensionVector::LENGTH,
                },
            ),
        ],
    }
}

/// Build a `Value::Enum` for variant "Circle" with the given radius (or Undef).
fn circle_enum(radius: Value) -> Value {
    Value::Enum {
        type_name: "Shape".to_string(),
        variant: "Circle".to_string(),
        payload: vec![("radius".to_string(), radius)],
    }
}

/// Build a `Value::Enum` for unit variant "Point" (empty payload).
fn point_enum() -> Value {
    Value::Enum {
        type_name: "Shape".to_string(),
        variant: "Point".to_string(),
        payload: vec![],
    }
}

// ── test 1: RED driver — VariantBind binds determined payload fields ──────────

/// INV-2/3: `Rect { width: w, height: h } => w * h` should return 0.02 * 0.01 = 0.0002 m².
///
/// This is the primary RED driver: today the binder cells are never populated,
/// so the body reads `w` and `h` as Undef → result is Undef. After step-2 (GREEN)
/// the binders are cracked from the payload and the body returns the product.
#[test]
fn variant_bind_binds_determined_payload_fields() {
    let w_cell = ValueCellId::new("$matcharm0.Shape", "w");
    let h_cell = ValueCellId::new("$matcharm0.Shape", "h");

    // Body: w * h  (AREA result)
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(w_cell.clone(), t_length()),
        CompiledExpr::value_ref(h_cell.clone(), t_length()),
        t_area(),
    );

    // Arm: Rect { width: w, height: h } => body
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Rect".to_string(),
            binders: vec![
                ("width".to_string(), w_cell),
                ("height".to_string(), h_cell),
            ],
        }],
        body,
    };

    // match <Rect { width: 20mm, height: 10mm }> { Rect { width: w, height: h } => w * h }
    let discriminant = CompiledExpr::literal(rect_enum(0.02, 0.01), t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], t_area());

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    match result {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (si_value - 0.0002).abs() < 1e-12,
                "expected 0.0002 m² (20mm×10mm), got {si_value}"
            );
            assert_eq!(
                dimension,
                DimensionVector::AREA,
                "result should have AREA dimension"
            );
        }
        other => panic!("expected Value::Scalar with area dimension, got {:?}", other),
    }
}

// ── test 2: undef payload field → arm selected, binder = Undef (D2/INV-4) ────

/// D2/INV-4 (body ignores binder): `Circle { radius: undef }` → arm IS selected
/// even though the payload field is undef. Body returns literal 7 (ignores binder).
#[test]
fn undef_payload_field_selects_arm_body_ignores_binder() {
    let r_cell = ValueCellId::new("$matcharm0.Shape", "r");

    // Body: literal Int(7) — deliberately ignores `r` to prove arm was selected.
    let body = CompiledExpr::literal(Value::Int(7), Type::Int);

    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Circle".to_string(),
            binders: vec![("radius".to_string(), r_cell)],
        }],
        body,
    };

    let discriminant =
        CompiledExpr::literal(circle_enum(Value::Undef), t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], Type::Int);

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Int(7),
        "arm should be selected despite undef payload field; body returns 7"
    );
}

/// D2/INV-4 (binder bound to Undef): body reads the binder → result is Undef.
#[test]
fn undef_payload_field_binder_is_undef() {
    let r_cell = ValueCellId::new("$matcharm0.Shape", "r");

    // Body: value_ref(r_cell) — will propagate Undef when bound to Undef.
    let body = CompiledExpr::value_ref(r_cell.clone(), t_length());

    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Circle".to_string(),
            binders: vec![("radius".to_string(), r_cell)],
        }],
        body,
    };

    let discriminant =
        CompiledExpr::literal(circle_enum(Value::Undef), t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], t_length());

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert!(
        result.is_undef(),
        "binder bound to undef payload field → body propagates Undef; got {:?}",
        result
    );
}

// ── test 3: wholly-undef discriminant → Undef (§9.2.5 unchanged) ─────────────

/// §9.2.5: a wholly-undef discriminant short-circuits to Undef before any arm is
/// tried. This is the pre-existing guard; the ζ change must not break it.
#[test]
fn wholly_undef_discriminant_yields_undef() {
    let r_cell = ValueCellId::new("$matcharm0.Shape", "r");
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Circle".to_string(),
            binders: vec![("radius".to_string(), r_cell)],
        }],
        body: CompiledExpr::literal(Value::Int(42), Type::Int),
    };

    let discriminant = CompiledExpr::literal(Value::Undef, t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], Type::Int);

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert!(
        result.is_undef(),
        "wholly-undef discriminant must yield Undef (§9.2.5); got {:?}",
        result
    );
}

// ── test 4: binder scope does not leak (INV-2) ───────────────────────────────

/// INV-2: binder cells inserted for one arm are NOT visible in the enclosing
/// `ValueMap` after eval, and a sibling arm's body cannot read them.
#[test]
fn binder_scope_does_not_leak() {
    // Encode two arms: Rect (binds w/h) and Circle (reads w_cell as foreign ref).
    let w_cell = ValueCellId::new("$matcharm0.Shape", "w");
    let h_cell = ValueCellId::new("$matcharm0.Shape", "h");
    let r_cell = ValueCellId::new("$matcharm1.Shape", "r");

    // Rect arm body: w * h (uses its own binders).
    let rect_body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(w_cell.clone(), t_length()),
        CompiledExpr::value_ref(h_cell.clone(), t_length()),
        t_area(),
    );
    let rect_arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Rect".to_string(),
            binders: vec![
                ("width".to_string(), w_cell.clone()),
                ("height".to_string(), h_cell.clone()),
            ],
        }],
        body: rect_body,
    };

    // Circle arm body: value_ref(w_cell) — tries to read the Rect arm's binder.
    // The Rect binders should NOT be in scope, so this should return Undef.
    let circle_body = CompiledExpr::value_ref(w_cell.clone(), t_length());
    let circle_arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Circle".to_string(),
            binders: vec![("radius".to_string(), r_cell)],
        }],
        body: circle_body,
    };

    // Discriminant is Circle: the Circle arm fires.
    let discriminant = CompiledExpr::literal(circle_enum(mm(5.0)), t_length());
    let match_expr =
        CompiledExpr::match_expr(discriminant, vec![rect_arm, circle_arm], t_length());

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    // The Circle arm's body reads w_cell, which belongs to the Rect binders —
    // it must NOT be in scope, so Undef is returned (scopes don't leak, INV-2).
    assert!(
        result.is_undef(),
        "Rect binder cells must not leak into the Circle arm's scope (INV-2); got {:?}",
        result
    );

    // Also verify the enclosing ValueMap is untouched.
    assert!(
        values.get(&w_cell).is_none(),
        "enclosing ValueMap must not be mutated by a match eval (INV-2)"
    );
}

// ── test 5a: missing payload field binds Undef (defensive branch) ────────────

/// The `unwrap_or(Value::Undef)` defensive branch: a binder field name that is
/// absent from the payload (rather than present-but-Undef) also produces Undef.
///
/// ε's E_PATTERN_UNKNOWN_FIELD / E_PATTERN_MISSING_FIELD guarantees this never
/// fires for well-typed source, but the branch must bind Undef defensively in
/// case that guarantee is ever relaxed or bypassed (e.g. by future IR surgery).
#[test]
fn missing_payload_field_binder_is_undef() {
    let r_cell = ValueCellId::new("$matcharm0.Shape", "r");

    // Binder references "radius", but the payload contains only "diameter" —
    // "radius" is entirely absent from the payload Vec.
    let missing_payload_enum = Value::Enum {
        type_name: "Shape".to_string(),
        variant: "Circle".to_string(),
        payload: vec![("diameter".to_string(), Value::Scalar {
            si_value: 0.01,
            dimension: DimensionVector::LENGTH,
        })],
    };

    // Body: value_ref(r_cell) — will be Undef when the field lookup misses.
    let body = CompiledExpr::value_ref(r_cell.clone(), t_length());
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Circle".to_string(),
            binders: vec![("radius".to_string(), r_cell)],
        }],
        body,
    };

    let discriminant = CompiledExpr::literal(missing_payload_enum, t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], t_length());

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert!(
        result.is_undef(),
        "binder for absent payload field must be Undef (defensive unwrap_or branch); got {:?}",
        result
    );
}

// ── test 5: bare Variant / Wildcard arms unchanged (INV-5) ───────────────────

/// INV-5: a unit-payload Value::Enum matched by CompiledPattern::Variant evaluates
/// its body in the current scope — byte-for-byte identical to pre-ζ behaviour.
#[test]
fn bare_variant_arm_unchanged() {
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::Variant {
            name: "Point".to_string(),
        }],
        body: CompiledExpr::literal(Value::Int(99), Type::Int),
    };

    let discriminant = CompiledExpr::literal(point_enum(), t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], Type::Int);

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert_eq!(result, Value::Int(99), "bare Variant arm should evaluate body (INV-5)");
}

/// INV-5: a Wildcard arm matches any variant and evaluates its body in the
/// current scope.
#[test]
fn wildcard_arm_unchanged() {
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::Wildcard],
        body: CompiledExpr::literal(Value::Int(0), Type::Int),
    };

    // Use a Rect discriminant — Wildcard should match it.
    let discriminant = CompiledExpr::literal(rect_enum(0.02, 0.01), t_length());
    let match_expr = CompiledExpr::match_expr(discriminant, vec![arm], Type::Int);

    let values = ValueMap::new();
    let result = eval_expr(&match_expr, &EvalContext::simple(&values));

    assert_eq!(result, Value::Int(0), "Wildcard arm should evaluate body (INV-5)");
}
