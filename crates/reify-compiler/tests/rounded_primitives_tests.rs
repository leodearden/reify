//! Compiler-level lowering tests for `rounded_box` and `rounded_rect` —
//! stdlib convenience geometry constructors (task #5201).
//!
//! Both constructors lower via **boolean-union compose**, NOT curated
//! vertical-edge fillet — see the task's design-decision note: the 3-arg
//! curated `fillet(target, edges, radius)` resolves `edges` against a NAMED
//! geometry realization, but a constructor's inner box is an anonymous
//! `GeomRef::Step` sub-op with no named let, so fillet-compose cannot be
//! synthesized inside a single-expression lowering.
//!
//! Test strategy (mirrors geometry_centered_primitives_tests.rs):
//! - `rounded_box(width, depth, height, corner_r)`: compose proof — lowers to
//!   the EXACT boolean-compose op sequence: 2×Primitive(Box), 4×(Primitive(Cylinder)
//!   + Transform(Translate)), and a left-folded Boolean{Union} chain of 5 ops,
//!   15 ops total, whose LAST op is the realization root.
//! - Wrong arg count emits an Error diagnostic.
//! - `try_infer_traits_for_function_call("rounded_box", &[])` returns
//!   `Some(InferredTraits::all())`.

use reify_compiler::geometry_traits_inference::{
    InferredTraits, try_infer_traits_for_function_call,
};
use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind, TransformKind};
use reify_core::{Severity, Type};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, Value};

// ─── helpers (mirrors geometry_centered_primitives_tests.rs) ──────────────────

fn do_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_rounded"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let compiled = do_compile(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:#?}",
        errors
    );
    compiled
}

fn has_any_error(module: &reify_compiler::CompiledModule) -> bool {
    module
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
}

/// Assert `expr` is a dimensioned (non-dimensionless) `Mul(<magnitude>,
/// Literal(Real(sign)))` and return the `sign` factor — the shared shape used
/// by the dz/dx/dy Translate-arg assertions below (mirrors the dz check in
/// `cylinder_centered_lowers_to_cylinder_plus_translate`).
fn assert_signed_mul(expr: &CompiledExpr, label: &str) -> f64 {
    assert_ne!(
        expr.result_type,
        Type::dimensionless_scalar(),
        "{label} must be typed as a dimensioned Scalar (Length), not bare Real — \
         units would be silently dropped by eval.as_f64(); got {:?}",
        expr.result_type
    );
    match &expr.kind {
        CompiledExprKind::BinOp {
            op: BinOp::Mul,
            right,
            ..
        } => match &right.kind {
            CompiledExprKind::Literal(Value::Real(factor)) => *factor,
            other => panic!("{label} Mul right operand must be a Real literal, got: {other:?}"),
        },
        other => panic!("{label} expression must be a Mul(<magnitude>, <sign literal>), got: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step-1: rounded_box — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// `rounded_box(40mm,30mm,20mm,5mm)` must lower to the boolean-compose op
/// sequence: [Box A, Box B, (Cylinder,Translate)×4, Boolean(Union)×5] — 15 ops
/// total — with the LAST op (the realization root) being Boolean(Union).
///
/// RED: rounded_box is unrecognised → no realization produced → assertion fails.
#[test]
fn rounded_box_lowers_to_boolean_compose() {
    let source = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 5mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    assert_eq!(
        template.realizations.len(),
        1,
        "rounded_box: expected 1 realization"
    );

    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        15,
        "rounded_box must lower to exactly 15 ops \
         [Box, Box, (Cylinder,Translate)x4, Union x5], got: {ops:#?}"
    );

    // ── ops[0..2]: two Primitive(Box) ────────────────────────────────────────
    for i in 0..2 {
        match &ops[i] {
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args,
            } => {
                let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(
                    keys,
                    &["width", "height", "depth"],
                    "Box op[{i}] must have args [width, height, depth], got: {keys:?}"
                );
            }
            other => panic!("op[{i}] must be Primitive(Box), got: {other:?}"),
        }
    }

    // Box A (op[0])'s "height" slot (index 1) must be a derived (dimensioned)
    // expression — depth - 2*corner_r — NOT a bare pass-through of `depth`.
    match &ops[0] {
        CompiledGeometryOp::Primitive { args, .. } => {
            let (_, height_expr) = &args[1];
            assert_ne!(
                height_expr.result_type,
                Type::dimensionless_scalar(),
                "Box A height-slot must be dimensioned Length, got {:?}",
                height_expr.result_type
            );
            assert!(
                matches!(
                    height_expr.kind,
                    CompiledExprKind::BinOp {
                        op: BinOp::Sub,
                        ..
                    }
                ),
                "Box A height-slot must be a Sub expr (depth - 2*corner_r), got: {:?}",
                height_expr.kind
            );
        }
        _ => unreachable!(),
    }

    // Box B (op[1])'s "width" slot (index 0) must similarly be derived —
    // width - 2*corner_r.
    match &ops[1] {
        CompiledGeometryOp::Primitive { args, .. } => {
            let (_, width_expr) = &args[0];
            assert_ne!(
                width_expr.result_type,
                Type::dimensionless_scalar(),
                "Box B width-slot must be dimensioned Length, got {:?}",
                width_expr.result_type
            );
            assert!(
                matches!(
                    width_expr.kind,
                    CompiledExprKind::BinOp {
                        op: BinOp::Sub,
                        ..
                    }
                ),
                "Box B width-slot must be a Sub expr (width - 2*corner_r), got: {:?}",
                width_expr.kind
            );
        }
        _ => unreachable!(),
    }

    // ── ops[2..10]: 4x (Primitive(Cylinder), Transform(Translate)) ──────────
    // one pair per corner, expected (dx_sign, dy_sign): (+,+), (+,-), (-,+), (-,-)
    let expected_signs = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
    let mut translate_steps = Vec::with_capacity(4);
    for (corner, (sx, sy)) in expected_signs.iter().enumerate() {
        let cyl_idx = 2 + corner * 2;
        let trans_idx = cyl_idx + 1;

        match &ops[cyl_idx] {
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                args,
            } => {
                let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(
                    keys,
                    &["radius", "height"],
                    "corner {corner} Cylinder op must have args [radius, height], got: {keys:?}"
                );
            }
            other => {
                panic!("op[{cyl_idx}] (corner {corner}) must be Primitive(Cylinder), got: {other:?}")
            }
        }

        match &ops[trans_idx] {
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target,
                args,
            } => {
                assert_eq!(
                    *target,
                    GeomRef::Step(cyl_idx),
                    "corner {corner} Translate must target its own Cylinder at Step({cyl_idx}), got: {target:?}"
                );
                let arg_keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
                assert!(
                    arg_keys.contains(&"dx"),
                    "corner {corner}: Translate args must contain \"dx\", got: {arg_keys:?}"
                );
                assert!(
                    arg_keys.contains(&"dy"),
                    "corner {corner}: Translate args must contain \"dy\", got: {arg_keys:?}"
                );
                assert!(
                    arg_keys.contains(&"dz"),
                    "corner {corner}: Translate args must contain \"dz\", got: {arg_keys:?}"
                );
                assert!(
                    !arg_keys.contains(&"target"),
                    "corner {corner}: Translate args must NOT contain \"target\", got: {arg_keys:?}"
                );

                let (_, dx_expr) = args.iter().find(|(k, _)| k == "dx").unwrap();
                let (_, dy_expr) = args.iter().find(|(k, _)| k == "dy").unwrap();
                let (_, dz_expr) = args.iter().find(|(k, _)| k == "dz").unwrap();

                let dx_sign = assert_signed_mul(dx_expr, &format!("corner {corner} dx"));
                assert!(
                    (dx_sign - sx).abs() < 1e-9,
                    "corner {corner}: dx sign must be {sx}, got {dx_sign}"
                );
                let dy_sign = assert_signed_mul(dy_expr, &format!("corner {corner} dy"));
                assert!(
                    (dy_sign - sy).abs() < 1e-9,
                    "corner {corner}: dy sign must be {sy}, got {dy_sign}"
                );
                let dz_sign = assert_signed_mul(dz_expr, &format!("corner {corner} dz"));
                assert!(
                    dz_sign < 0.0,
                    "corner {corner}: dz Mul factor must be negative (shift down by height/2), got {dz_sign}"
                );
            }
            other => {
                panic!("op[{trans_idx}] (corner {corner}) must be Transform(Translate), got: {other:?}")
            }
        }

        translate_steps.push(trans_idx);
    }

    // ── ops[10..15]: left-folded Boolean{Union} chain ────────────────────────
    // ops[10] = Union(Step(0), Step(1))  — Box A ∪ Box B
    // ops[11..15][k] = Union(Step(prev), Step(translate_steps[k])) for k=0..4
    match &ops[10] {
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left,
            right,
        } => {
            assert_eq!(
                *left,
                GeomRef::Step(0),
                "first Union.left must be Box A (Step(0))"
            );
            assert_eq!(
                *right,
                GeomRef::Step(1),
                "first Union.right must be Box B (Step(1))"
            );
        }
        other => panic!("op[10] must be Boolean(Union), got: {other:?}"),
    }
    let mut prev_step = 10usize;
    for (k, &tstep) in translate_steps.iter().enumerate() {
        let idx = 11 + k;
        match &ops[idx] {
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left,
                right,
            } => {
                assert_eq!(
                    *left,
                    GeomRef::Step(prev_step),
                    "op[{idx}] Union.left must chain from previous Union at Step({prev_step})"
                );
                assert_eq!(
                    *right,
                    GeomRef::Step(tstep),
                    "op[{idx}] Union.right must be corner {k}'s Translate at Step({tstep})"
                );
            }
            other => panic!("op[{idx}] must be Boolean(Union), got: {other:?}"),
        }
        prev_step = idx;
    }

    // The LAST op is the realization root, and must be the final Union.
    assert!(
        matches!(
            ops.last().unwrap(),
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                ..
            }
        ),
        "last op (realization root) must be Boolean(Union), got: {:#?}",
        ops.last()
    );
}

/// Wrong arg count (3 or 5 args) to rounded_box must produce an error diagnostic.
#[test]
fn rounded_box_wrong_arg_count_emits_error() {
    let source_3 = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm)
}"#;
    let compiled_3 = do_compile(source_3);
    assert!(
        has_any_error(&compiled_3),
        "expected at least one error for rounded_box with 3 args, got: {:#?}",
        compiled_3.diagnostics
    );

    let source_5 = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 5mm, 1mm)
}"#;
    let compiled_5 = do_compile(source_5);
    assert!(
        has_any_error(&compiled_5),
        "expected at least one error for rounded_box with 5 args, got: {:#?}",
        compiled_5.diagnostics
    );
}

/// `try_infer_traits_for_function_call("rounded_box", &[])` must return
/// `Some(InferredTraits::all())` — proves the dispatch arm is wired.
///
/// RED: "rounded_box" not in the inference table → returns `None`.
#[test]
fn rounded_box_inferred_traits_all() {
    let result = try_infer_traits_for_function_call("rounded_box", &[]);
    assert_eq!(
        result,
        Some(InferredTraits::all()),
        "expected Some(InferredTraits::all()) for \"rounded_box\", got: {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step-3: rounded_box constraint contract — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// `corner_r` not `> 0` (here: `0mm`) must emit a designer-readable Error
/// diagnostic naming `corner_r`.
///
/// RED until step-4 adds the compile-time constraint check — currently a
/// zero corner_r silently lowers (degenerate zero-radius corner cylinders)
/// with no diagnostic.
#[test]
fn rounded_box_corner_r_not_positive_emits_error() {
    let source = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 0mm)
}"#;
    let compiled = do_compile(source);
    let messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("corner_r")),
        "expected an error diagnostic naming corner_r, got: {messages:#?}"
    );
}

/// `2*corner_r >= min(width, depth)` (here: `2*25mm=50mm >= min(40mm,30mm)=30mm`)
/// must emit a designer-readable Error naming the concrete offending values.
///
/// RED until step-4.
#[test]
fn rounded_box_corner_r_violates_min_dimension_emits_error() {
    let source = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 25mm)
}"#;
    let compiled = do_compile(source);
    let messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("corner_r") && m.contains("width") && m.contains("depth")),
        "expected an error diagnostic naming the 2*corner_r < min(width, depth) \
         violation with concrete values, got: {messages:#?}"
    );
}

/// Valid constant args (satisfying `corner_r > 0` and
/// `2*corner_r < min(width, depth)`) must compile with zero error diagnostics.
#[test]
fn rounded_box_valid_constant_args_compile_clean() {
    let source = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 5mm)
}"#;
    compile_no_errors(source);
}

/// A param-driven (non-constant) `corner_r` cannot be checked statically and
/// must NOT be false-flagged — even when its default value would numerically
/// violate the constraint (here `r`'s default `25mm` would fail against
/// `width=40mm, depth=30mm`, same numbers as the violating case above).
#[test]
fn rounded_box_param_driven_corner_r_skips_static_check() {
    let source = r#"structure def S {
    param r: Length = 25mm
    let body = rounded_box(40mm, 30mm, 20mm, r)
}"#;
    compile_no_errors(source);
}
