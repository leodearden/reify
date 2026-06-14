//! Compiler-level lowering tests for `box_centered` and `cylinder_centered`
//! — the Phase 2 centred-primitive constructors from
//! `docs/prds/geometry-primitive-constructors.md` task ε.
//!
//! Test strategy (mirrors solid_param_tests.rs + let_scope_tests.rs):
//! - `box_centered`: alias proof — lowers to the IDENTICAL Primitive(Box) op
//!   as `box`; wrong arg count emits error.
//! - `cylinder_centered`: compose proof — lowers to EXACTLY [Primitive(Cylinder),
//!   Transform(Translate)] with the Translate targeting the Cylinder; wrong
//!   arg count emits error.
//! - Both: `try_infer_traits_for_function_call` returns `Some(InferredTraits::all())`.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, TransformKind};
use reify_compiler::geometry_traits_inference::{InferredTraits, try_infer_traits_for_function_call};
use reify_core::{Severity, Type};
use reify_ir::{BinOp, CompiledExprKind, Value};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn do_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_centered"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
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
    module.diagnostics.iter().any(|d| d.severity == Severity::Error)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step-1: box_centered — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// `box_centered(10mm,20mm,30mm)` must lower to EXACTLY ONE op:
/// `Primitive{kind:Box, args:[("width",...),("height",...),("depth",...)]}` —
/// the IDENTICAL op as `box(10mm,20mm,30mm)` (op-identity alias proof).
///
/// RED: box_centered is unrecognised → no realization produced → assertion fails.
/// GREEN: after implementation both realizations have one Primitive(Box) with matching arg keys.
#[test]
fn box_centered_lowering_matches_box() {
    let source_box = r#"structure def S {
    let body = box(10mm, 20mm, 30mm)
}"#;
    let source_centered = r#"structure def S {
    let body = box_centered(10mm, 20mm, 30mm)
}"#;

    let compiled_box = compile_no_errors(source_box);
    let compiled_centered = compile_no_errors(source_centered);

    let template_box = compiled_box
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found in box source");
    let template_centered = compiled_centered
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found in box_centered source");

    assert_eq!(
        template_box.realizations.len(),
        1,
        "box: expected 1 realization"
    );
    assert_eq!(
        template_centered.realizations.len(),
        1,
        "box_centered: expected 1 realization (alias of box)"
    );

    let ops_box = &template_box.realizations[0].operations;
    let ops_centered = &template_centered.realizations[0].operations;

    // box_centered is an alias → must lower to a SINGLE op
    assert_eq!(
        ops_centered.len(),
        1,
        "box_centered must lower to exactly 1 op (Primitive(Box)), got: {:#?}",
        ops_centered
    );

    // Both must be Primitive(Box) with identical arg-key ordering
    match (&ops_box[0], &ops_centered[0]) {
        (
            CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, args: args_box },
            CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, args: args_centered },
        ) => {
            let keys_box: Vec<&str> = args_box.iter().map(|(k, _)| k.as_str()).collect();
            let keys_centered: Vec<&str> =
                args_centered.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys_box,
                keys_centered,
                "arg keys differ between box and box_centered: box={keys_box:?}, centered={keys_centered:?}"
            );
            // Keys must be width / height / depth in that order
            assert_eq!(keys_centered, &["width", "height", "depth"]);
        }
        (box_op, centered_op) => {
            panic!(
                "expected Primitive(Box) for both;\n  box:     {box_op:?}\n  centered:{centered_op:?}"
            );
        }
    }
}

/// Wrong arg count (2 args to box_centered) must produce at least one error diagnostic.
///
/// RED state:  box_centered not recognised → "unknown function" or similar error.
/// GREEN state: box_centered arm fires → arg-count error "expected 3, got 2".
/// Either way an error is emitted, so this test is green in both states — it is
/// included to pin the post-implementation behaviour (correct error message exists).
#[test]
fn box_centered_wrong_arg_count_emits_error() {
    let source = r#"structure def S {
    let body = box_centered(10mm, 20mm)
}"#;
    let compiled = do_compile(source);
    assert!(
        has_any_error(&compiled),
        "expected at least one error for box_centered with 2 args, got: {:#?}",
        compiled.diagnostics
    );
}

/// `try_infer_traits_for_function_call("box_centered", &[])` must return
/// `Some(InferredTraits::all())` — proves the dispatch arm is wired.
///
/// RED: "box_centered" not in the inference table → returns `None`.
#[test]
fn box_centered_inferred_traits_all() {
    let result = try_infer_traits_for_function_call("box_centered", &[]);
    assert_eq!(
        result,
        Some(InferredTraits::all()),
        "expected Some(InferredTraits::all()) for \"box_centered\", got: {result:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step-3: cylinder_centered — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// `cylinder_centered(5mm,20mm)` must lower to EXACTLY TWO ops:
///   [0] = Primitive{kind:Cylinder, args:[("radius",...),("height",...)]}
///   [1] = Transform{kind:Translate, target:GeomRef::Step(<index of op 0>),
///                   args containing dx,dy,dz but NOT "target"}
///
/// RED: cylinder_centered unrecognised → no realization → assertion fails.
#[test]
fn cylinder_centered_lowers_to_cylinder_plus_translate() {
    let source = r#"structure def S {
    let body = cylinder_centered(5mm, 20mm)
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
        "cylinder_centered: expected 1 realization"
    );

    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        2,
        "cylinder_centered must lower to exactly 2 ops [Primitive(Cylinder), Transform(Translate)], got: {ops:#?}"
    );

    // op[0]: Primitive(Cylinder) with radius, height
    match &ops[0] {
        CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, args } => {
            let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                &["radius", "height"],
                "Cylinder op must have args [radius, height], got: {keys:?}"
            );
        }
        other => panic!("op[0] must be Primitive(Cylinder), got: {other:?}"),
    }

    // The Cylinder lands at the flat index equal to the realization's step_offset.
    // Inside a single-realization structure the first realization starts at step 0,
    // so the Cylinder is at Step(0) and the Translate's target must be Step(0).
    // (More precisely: the Cylinder is at index 0 within this realization's ops, and
    // step_offset for the first realization is 0.)
    let cylinder_step = 0usize; // first op in this realization

    // op[1]: Transform(Translate) targeting the Cylinder
    match &ops[1] {
        CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target,
            args,
        } => {
            assert_eq!(
                *target,
                GeomRef::Step(cylinder_step),
                "Translate must target the Cylinder at Step({cylinder_step}), got: {target:?}"
            );
            let arg_keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
            // args must contain dx, dy, dz — NOT a "target" key (eval reads target from the GeomRef field)
            assert!(
                arg_keys.contains(&"dx"),
                "Translate args must contain \"dx\", got: {arg_keys:?}"
            );
            assert!(
                arg_keys.contains(&"dy"),
                "Translate args must contain \"dy\", got: {arg_keys:?}"
            );
            assert!(
                arg_keys.contains(&"dz"),
                "Translate args must contain \"dz\", got: {arg_keys:?}"
            );
            assert!(
                !arg_keys.contains(&"target"),
                "Translate args must NOT contain \"target\" (target is the GeomRef field), got: {arg_keys:?}"
            );

            // ── dz expression: must be dimensioned and encode a negative offset ─────
            //
            // The critical correctness constraint for cylinder_centered:
            // dz = -(height / 2) shifts the OCCT base-at-origin cylinder
            // (z ∈ [0, h]) down so the centroid lands at z = 0.
            //
            // Two bugs would silently pass without this check:
            //   (a) dz typed as bare Real (Type::dimensionless_scalar()) — eval.as_f64() would
            //       return the dimensionless 0.5 instead of the SI-metres value,
            //       shifting by 0.5m rather than height/2.
            //   (b) A positive factor — the cylinder would shift UP instead of down.
            let (_, dz_expr) = args
                .iter()
                .find(|(k, _)| k == "dz")
                .expect("already asserted dz key is present");

            assert_ne!(
                dz_expr.result_type,
                Type::dimensionless_scalar(),
                "dz must be typed as a dimensioned Scalar (Length), not bare Real — \
                 units would be silently dropped by eval.as_f64(); got {:?}",
                dz_expr.result_type
            );

            match &dz_expr.kind {
                CompiledExprKind::BinOp { op: BinOp::Mul, right, .. } => {
                    match &right.kind {
                        CompiledExprKind::Literal(Value::Real(factor)) => {
                            assert!(
                                *factor < 0.0,
                                "dz Mul factor must be negative (shift cylinder down by height/2), \
                                 got factor={factor}"
                            );
                        }
                        other => panic!(
                            "dz Mul right operand must be a Real literal, got: {other:?}"
                        ),
                    }
                }
                other => panic!(
                    "dz expression must be Mul(height, -0.5) — \
                     alternative forms (UnOp::Neg / BinOp::Div) are not currently \
                     emitted but would be accepted if the implementation changes; \
                     got: {other:?}"
                ),
            }
        }
        other => panic!("op[1] must be Transform(Translate), got: {other:?}"),
    }
}

/// Wrong arg count (3 args to cylinder_centered) must produce an error diagnostic.
#[test]
fn cylinder_centered_wrong_arg_count_emits_error() {
    let source = r#"structure def S {
    let body = cylinder_centered(5mm, 20mm, 10mm)
}"#;
    let compiled = do_compile(source);
    assert!(
        has_any_error(&compiled),
        "expected at least one error for cylinder_centered with 3 args, got: {:#?}",
        compiled.diagnostics
    );
}

/// `try_infer_traits_for_function_call("cylinder_centered", &[])` must return
/// `Some(InferredTraits::all())`.
///
/// RED: "cylinder_centered" not in the inference table → returns `None`.
#[test]
fn cylinder_centered_inferred_traits_all() {
    let result = try_infer_traits_for_function_call("cylinder_centered", &[]);
    assert_eq!(
        result,
        Some(InferredTraits::all()),
        "expected Some(InferredTraits::all()) for \"cylinder_centered\", got: {result:?}"
    );
}

/// Sanity: `cylinder_centered` with correct arg count produces exactly ONE realization
/// whose last op is the realization root (the Translate).
#[test]
fn cylinder_centered_realization_root_is_translate() {
    let source = r#"structure def S {
    param r: Length = 5mm
    param h: Length = 20mm
    let body = cylinder_centered(r, h)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;
    assert_eq!(ops.len(), 2, "expected [Cylinder, Translate]");

    // The last op (the realization root) must be the Translate
    assert!(
        matches!(
            ops.last().unwrap(),
            CompiledGeometryOp::Transform { kind: TransformKind::Translate, .. }
        ),
        "last op (realization root) must be Transform(Translate), got: {:#?}",
        ops.last()
    );
}
