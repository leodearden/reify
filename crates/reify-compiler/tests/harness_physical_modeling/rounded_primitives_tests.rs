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
//!   the EXACT boolean-compose op sequence: 2×Primitive(Box), 4×(Primitive(Cylinder) +
//!   Transform(Translate)), and a left-folded Boolean{Union} chain of 5 ops,
//!   15 ops total, whose LAST op is the realization root.
//! - Wrong arg count emits an Error diagnostic.
//! - `try_infer_traits_for_function_call("rounded_box", &[])` returns
//!   `Some(InferredTraits::all())`.

use reify_compiler::geometry_traits_inference::{
    InferredTraits, try_infer_traits_for_function_call,
};
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind, ProfileKind, TransformKind,
};
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

// ═══════════════════════════════════════════════════════════════════════════════
// Step-1: rounded_box — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Assert `expr` is `Mul(<magnitude>, Literal(Real(factor)))` where the PRODUCT
/// is LENGTH-dimensioned but the right-hand MULTIPLIER is a bare dimensionless
/// `Real`. Returns the factor.
///
/// The single shared shape assertion for every scaled Translate/Thicken arg in
/// this file — the corner dx/dy/dz offsets AND the negative-space pins below.
/// It pins BOTH halves of the product: the outer node must be exactly
/// `Type::length()` (units are not silently dropped by `eval.as_f64()`), and
/// the inner factor must stay a bare dimensionless `Literal(Real(..))`.
fn assert_length_scaled_by_dimensionless(expr: &CompiledExpr, label: &str) -> f64 {
    assert_eq!(
        expr.result_type,
        Type::length(),
        "{label}: the PRODUCT must be LENGTH — it already clones the magnitude's \
         result_type today; got {:?}",
        expr.result_type
    );
    match &expr.kind {
        CompiledExprKind::BinOp {
            op: BinOp::Mul,
            right,
            ..
        } => {
            assert_eq!(
                right.result_type,
                Type::dimensionless_scalar(),
                "{label}: the scale factor must stay dimensionless — a LENGTH-typed \
                 multiplier makes this Scalar{{AREA}} at eval, which the incoming \
                 eval-layer length gate rejects; got {:?}",
                right.result_type
            );
            match &right.kind {
                CompiledExprKind::Literal(Value::Real(factor)) => *factor,
                other => panic!(
                    "{label}: the scale factor must stay a bare Literal(Real(..)) — \
                     a dimensioned Scalar multiplier makes this Scalar{{AREA}} at eval, \
                     which the incoming eval-layer length gate rejects; got: {other:?}"
                ),
            }
        }
        other => {
            panic!("{label} must be a Mul(<magnitude>, <dimensionless factor>), got: {other:?}")
        }
    }
}

/// NEGATIVE-SPACE PIN — the `-0.5`/`+0.5` scale factors must STAY dimensionless
/// while their enclosing `Mul` stays LENGTH.
///
/// This guards a documented drift in the units-length PRD
/// (`docs/prds/v0_6/units-length-gate-completion.md` §8 α, §4 M1 and the §2
/// anchor table), which instructs a future reader to retype two `-0.5` literals
/// in `reify-compiler/src/geometry.rs` to LENGTH. That instruction is wrong
/// twice over, and this test makes it un-implementable rather than merely
/// un-done (see esc-5742-1). Deliberately cited by binding + function name, not
/// by line number — the PRD's own line-number attribution is what went stale:
///
///  1. MIS-ATTRIBUTION — one of the two is not in `rounded_box` at all. It is
///     `minus_offset = w * (-0.5)` inside the `zone_profile` arm, feeding a
///     `Modify{Thicken}` "offset" slot. `rounded_box` has exactly ONE dz.
///  2. WRONG CLASS — both are dimensionless MULTIPLIERS, not slot-bound zeros.
///     The enclosing binop ALREADY carries the magnitude's LENGTH result_type,
///     and at eval `Scalar{LENGTH} × Real` takes the Scalar×Real arm and
///     preserves LENGTH. Retyping the factor would switch eval onto the
///     Scalar×Scalar arm and yield `Scalar{AREA}` — which the length gate this
///     task exists to satisfy would then REJECT, inverting α's purpose.
#[test]
fn rounded_box_dz_is_length_scaled_by_a_dimensionless_factor() {
    let source = r#"structure def S {
    let body = rounded_box(20mm, 10mm, 5mm, 1mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let ops = &template.realizations[0].operations;

    let mut checked = 0usize;
    for op in ops {
        if let CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            args,
            ..
        } = op
        {
            let (_, dz_expr) = args
                .iter()
                .find(|(k, _)| k == "dz")
                .expect("rounded_box corner Translate must carry a dz");
            let factor = assert_length_scaled_by_dimensionless(dz_expr, "rounded_box corner dz");
            // Bit-exact, not merely negative: the magnitude is the invariant. A drift
            // to -1.0 would push each corner cylinder a FULL height below centre
            // instead of half, and a sign-only check would stay green through it.
            assert_eq!(
                factor, -0.5,
                "rounded_box corner dz factor must be exactly -0.5 (dz = -height/2 \
                 centres the cylinder on z=0); got {factor}"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 4,
        "rounded_box must lower to 4 corner Translates; got {checked} — \
         if this changed, the pin above is no longer covering what it claims to"
    );
}

/// The `zone_profile` half of the same negative-space pin: `zone_profile`'s
/// `plus_offset`/`minus_offset` are the two literals the units-length PRD
/// (`docs/prds/v0_6/units-length-gate-completion.md` §8 α / §4 M1 / §2 anchor
/// table) actually names, while attributing them to `rounded_box`'s corner dz.
/// Both `±0.5` offsets must stay dimensionless multipliers of a LENGTH width,
/// feeding the two `Modify{Thicken}` "offset" slots.
///
/// HOME RATIONALE: `zone_profile` also has structural coverage in
/// `reify-eval/tests/zone_constructors_e2e.rs`, which would otherwise be the
/// natural neighbour. This pin lives here instead because it is one half of a
/// single indivisible claim — "the PRD's two `-0.5` literals must BOTH stay
/// dimensionless, and neither is `rounded_box`'s dz" — whose other half is
/// `rounded_box_dz_is_length_scaled_by_a_dimensionless_factor` directly above.
/// The two share `assert_length_scaled_by_dimensionless` and are only legible
/// together; splitting them across crates would leave each half looking
/// arbitrary. `reify-eval` is also outside this task's lock scope.
#[test]
fn zone_profile_offsets_are_length_scaled_by_dimensionless_factors() {
    let source = r#"structure def S {
    let z = zone_profile(box(10mm, 10mm, 10mm), 1mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let ops = &template.realizations[0].operations;

    let factors: Vec<f64> = ops
        .iter()
        .filter_map(|op| match op {
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                args,
                ..
            } => args.iter().find(|(k, _)| k == "offset"),
            _ => None,
        })
        .map(|(_, offset_expr)| {
            assert_length_scaled_by_dimensionless(offset_expr, "zone_profile thicken offset")
        })
        .collect();

    assert_eq!(
        factors.len(),
        2,
        "zone_profile must lower to two Modify{{Thicken}} ops carrying an `offset`; \
         got {} — if this changed, the pin above is no longer covering what it claims to",
        factors.len()
    );
    // ORDERED and bit-exact, not just "a +/- pair somewhere". `zone_profile` pushes
    // plus_offset (at plus_step) BEFORE minus_offset (at minus_step) and then emits
    // Boolean{Difference, left: plus_step, right: minus_step} — so the outer shell is
    // positionally the first Thicken. Swapping the two offset expressions would invert
    // outer/inner and yield an empty or inverted zone, which an unordered
    // any-positive-and-any-negative check would pass straight through. The ±0.5
    // magnitudes are the half-width contract (offset = ±w/2).
    assert_eq!(
        factors,
        vec![0.5, -0.5],
        "zone_profile's thicken offsets must be exactly [+0.5, -0.5] in emission order \
         (outer shell first, then inner — Boolean{{Difference}} depends on that order); \
         got {factors:?}"
    );
}

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
    for (i, op) in ops.iter().enumerate().take(2) {
        match op {
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

                let dx_sign = assert_length_scaled_by_dimensionless(dx_expr, &format!("corner {corner} dx"));
                assert!(
                    (dx_sign - sx).abs() < 1e-9,
                    "corner {corner}: dx sign must be {sx}, got {dx_sign}"
                );
                let dy_sign = assert_length_scaled_by_dimensionless(dy_expr, &format!("corner {corner} dy"));
                assert!(
                    (dy_sign - sy).abs() < 1e-9,
                    "corner {corner}: dy sign must be {sy}, got {dy_sign}"
                );
                let dz_sign = assert_length_scaled_by_dimensionless(dz_expr, &format!("corner {corner} dz"));
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

/// A corner_r expressed as constant arithmetic over dimensioned literals
/// (`10mm + 15mm`, folding to the same `25mm` violating value as
/// `rounded_box_corner_r_violates_min_dimension_emits_error` above) must
/// still be caught by the static constraint check — `const_length_m` folds
/// `Add`/`Sub`/`Mul` of constant operands, not just bare literals, so an
/// obviously-constant violation written with arithmetic doesn't silently
/// compile clean and defer to an opaque runtime/OCCT failure.
#[test]
fn rounded_box_corner_r_constant_arithmetic_still_caught_statically() {
    let source = r#"structure def S {
    let body = rounded_box(40mm, 30mm, 20mm, 10mm + 15mm)
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
        "expected a constant-arithmetic corner_r (10mm+15mm=25mm) to be folded and \
         still trip the 2*corner_r < min(width, depth) check, got: {messages:#?}"
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

// ═══════════════════════════════════════════════════════════════════════════════
// Step-5: rounded_rect — RED tests
// ═══════════════════════════════════════════════════════════════════════════════

/// `rounded_rect(40mm,30mm,5mm)` must lower to the 2D boolean-compose op
/// sequence: [Rect A, Rect B, (Circle,Translate)×4, Boolean(Union)×5] — 15 ops
/// total — with the LAST op (the realization root) being Boolean(Union). Each
/// corner Translate's dz must be exactly 0 (planar face, z=0).
///
/// RED: rounded_rect is unrecognised → no realization produced → assertion fails.
#[test]
fn rounded_rect_lowers_to_boolean_compose() {
    let source = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, 5mm)
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
        "rounded_rect: expected 1 realization"
    );

    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        15,
        "rounded_rect must lower to exactly 15 ops \
         [Rectangle, Rectangle, (Circle,Translate)x4, Union x5], got: {ops:#?}"
    );

    // ── ops[0..2]: two Profile(Rectangle) ────────────────────────────────────
    for (i, op) in ops.iter().enumerate().take(2) {
        match op {
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Rectangle,
                args,
            } => {
                let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(
                    keys,
                    &["width", "height"],
                    "Rectangle op[{i}] must have args [width, height], got: {keys:?}"
                );
            }
            other => panic!("op[{i}] must be Profile(Rectangle), got: {other:?}"),
        }
    }

    // Rect A (op[0])'s "height" slot (index 1) must be derived: depth - 2*corner_r.
    match &ops[0] {
        CompiledGeometryOp::Profile { args, .. } => {
            let (_, height_expr) = &args[1];
            assert_ne!(
                height_expr.result_type,
                Type::dimensionless_scalar(),
                "Rect A height-slot must be dimensioned Length, got {:?}",
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
                "Rect A height-slot must be a Sub expr (depth - 2*corner_r), got: {:?}",
                height_expr.kind
            );
        }
        _ => unreachable!(),
    }

    // Rect B (op[1])'s "width" slot (index 0) must be derived: width - 2*corner_r.
    match &ops[1] {
        CompiledGeometryOp::Profile { args, .. } => {
            let (_, width_expr) = &args[0];
            assert_ne!(
                width_expr.result_type,
                Type::dimensionless_scalar(),
                "Rect B width-slot must be dimensioned Length, got {:?}",
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
                "Rect B width-slot must be a Sub expr (width - 2*corner_r), got: {:?}",
                width_expr.kind
            );
        }
        _ => unreachable!(),
    }

    // ── ops[2..10]: 4x (Profile(Circle), Transform(Translate)) ──────────────
    let expected_signs = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
    let mut translate_steps = Vec::with_capacity(4);
    for (corner, (sx, sy)) in expected_signs.iter().enumerate() {
        let circ_idx = 2 + corner * 2;
        let trans_idx = circ_idx + 1;

        match &ops[circ_idx] {
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Circle,
                args,
            } => {
                let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(
                    keys,
                    &["radius"],
                    "corner {corner} Circle op must have args [radius], got: {keys:?}"
                );
            }
            other => {
                panic!("op[{circ_idx}] (corner {corner}) must be Profile(Circle), got: {other:?}")
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
                    GeomRef::Step(circ_idx),
                    "corner {corner} Translate must target its own Circle at Step({circ_idx}), got: {target:?}"
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

                let (_, dx_expr) = args.iter().find(|(k, _)| k == "dx").unwrap();
                let (_, dy_expr) = args.iter().find(|(k, _)| k == "dy").unwrap();
                let (_, dz_expr) = args.iter().find(|(k, _)| k == "dz").unwrap();

                let dx_sign = assert_length_scaled_by_dimensionless(dx_expr, &format!("corner {corner} dx"));
                assert!(
                    (dx_sign - sx).abs() < 1e-9,
                    "corner {corner}: dx sign must be {sx}, got {dx_sign}"
                );
                let dy_sign = assert_length_scaled_by_dimensionless(dy_expr, &format!("corner {corner} dy"));
                assert!(
                    (dy_sign - sy).abs() < 1e-9,
                    "corner {corner}: dy sign must be {sy}, got {dy_sign}"
                );

                // dz stays numerically 0 (planar, z=0) but is LENGTH-dimensioned:
                // the value is a z-offset bound into a LENGTH slot. Both halves are
                // asserted — eval dispatches on the runtime `Value` and never reads
                // `result_type`, so a result_type-only pin would pass over exactly
                // the bare-Real shape the incoming eval-layer length gate rejects.
                assert_eq!(
                    dz_expr.result_type,
                    Type::length(),
                    "corner {corner}: dz must be typed Type::length() — planar z=0 is \
                     still a LENGTH-dimensioned slot; got {:?}",
                    dz_expr.result_type
                );

                assert!(
                    matches!(
                        &dz_expr.kind,
                        CompiledExprKind::Literal(Value::Scalar { si_value, dimension })
                            if *si_value == 0.0
                                && *dimension == reify_core::DimensionVector::LENGTH
                    ),
                    "corner {corner}: dz must be Literal(Scalar{{LENGTH, 0.0}}) (planar, z=0), \
                     not Literal(Real(0.0)) — a bare Real is what the incoming eval-layer \
                     length gate rejects; got: {:?}",
                    dz_expr.kind
                );
            }
            other => panic!(
                "op[{trans_idx}] (corner {corner}) must be Transform(Translate), got: {other:?}"
            ),
        }

        translate_steps.push(trans_idx);
    }

    // ── ops[10..15]: left-folded Boolean{Union} chain ────────────────────────
    match &ops[10] {
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left,
            right,
        } => {
            assert_eq!(
                *left,
                GeomRef::Step(0),
                "first Union.left must be Rect A (Step(0))"
            );
            assert_eq!(
                *right,
                GeomRef::Step(1),
                "first Union.right must be Rect B (Step(1))"
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

/// Wrong arg count (2 or 4 args) to rounded_rect must produce an error diagnostic.
#[test]
fn rounded_rect_wrong_arg_count_emits_error() {
    let source_2 = r#"structure def S {
    let body = rounded_rect(40mm, 30mm)
}"#;
    let compiled_2 = do_compile(source_2);
    assert!(
        has_any_error(&compiled_2),
        "expected at least one error for rounded_rect with 2 args, got: {:#?}",
        compiled_2.diagnostics
    );

    let source_4 = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, 5mm, 1mm)
}"#;
    let compiled_4 = do_compile(source_4);
    assert!(
        has_any_error(&compiled_4),
        "expected at least one error for rounded_rect with 4 args, got: {:#?}",
        compiled_4.diagnostics
    );
}

/// Constraint violations (`corner_r <= 0`; `2*corner_r >= min(w,d)`) must emit
/// designer-readable Errors — reuses the same constraint helper as rounded_box.
#[test]
fn rounded_rect_constraint_violations_emit_errors() {
    let source_nonpositive = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, 0mm)
}"#;
    let compiled_np = do_compile(source_nonpositive);
    let messages_np: Vec<&str> = compiled_np
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages_np.iter().any(|m| m.contains("corner_r")),
        "expected an error diagnostic naming corner_r, got: {messages_np:#?}"
    );

    let source_too_big = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, 25mm)
}"#;
    let compiled_big = do_compile(source_too_big);
    let messages_big: Vec<&str> = compiled_big
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages_big
            .iter()
            .any(|m| m.contains("corner_r") && m.contains("width") && m.contains("depth")),
        "expected an error diagnostic naming the 2*corner_r < min(width, depth) \
         violation with concrete values, got: {messages_big:#?}"
    );
}

/// `try_infer_traits_for_function_call("rounded_rect", &[])` must return
/// `Some(InferredTraits::surface())`.
///
/// RED: "rounded_rect" not in the inference table → returns `None`.
#[test]
fn rounded_rect_inferred_traits_surface() {
    let result = try_infer_traits_for_function_call("rounded_rect", &[]);
    assert_eq!(
        result,
        Some(InferredTraits::surface()),
        "expected Some(InferredTraits::surface()) for \"rounded_rect\", got: {result:?}"
    );
}

/// `extrude(rounded_rect(...), 10mm)` must compile with NO profile-precondition
/// error (rounded_rect is a Surface), while `extrude(rounded_box(...), 10mm)`
/// DOES emit `GeometryProfileRequired` (rounded_box is a Solid).
///
/// RED (rect half) until step-6; the box half already holds today (any
/// Solid-typed nested call is rejected at a profile slot).
#[test]
fn extrude_accepts_rounded_rect_rejects_rounded_box() {
    let source_rect = r#"structure def S {
    let body = extrude(rounded_rect(40mm, 30mm, 5mm), 10mm)
}"#;
    let compiled_rect = do_compile(source_rect);
    let rect_profile_errors = compiled_rect
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(reify_core::DiagnosticCode::GeometryProfileRequired))
        .count();
    assert_eq!(
        rect_profile_errors, 0,
        "extrude(rounded_rect(...)) must NOT emit GeometryProfileRequired, got: {:#?}",
        compiled_rect.diagnostics
    );

    let source_box = r#"structure def S {
    let body = extrude(rounded_box(40mm, 30mm, 20mm, 5mm), 10mm)
}"#;
    let compiled_box = do_compile(source_box);
    let box_profile_errors = compiled_box
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(reify_core::DiagnosticCode::GeometryProfileRequired))
        .count();
    assert!(
        box_profile_errors >= 1,
        "extrude(rounded_box(...)) must emit GeometryProfileRequired (Solid at a Surface slot), got: {:#?}",
        compiled_box.diagnostics
    );
}

// ─── param-driven corner_r: runtime constraint synthesis (task #5665) ─────────

/// Compile `source`, assert it produced no error diagnostics, and return its
/// `name`d template.
///
/// The by-name lookup is `reify_test_support::compile_template`'s rather than a
/// local re-implementation; this only adds the clean-compile assertion (the same
/// one [`compile_no_errors`] makes) so a param-driven call's constraints can be
/// asserted without also asserting the module compiled. `name` is a parameter,
/// not a hardcoded `"S"`, so the helper carries over to any entity.
fn compile_template_no_errors(source: &str, name: &str) -> reify_compiler::TopologyTemplate {
    let (template, diagnostics) = reify_test_support::compile_template(source, name);
    let errors = reify_test_support::collect_errors(&diagnostics);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {errors:#?}"
    );
    template
}

/// A param-driven `corner_r` must not be silently waved through.
///
/// `validate_rounded_corner_constraint`'s static check can only fire when
/// width/depth/corner_r all fold to constants. When one of them is a param it
/// used to `return true` and record NOTHING, so `rounded_rect(40mm, 30mm,
/// corner_r)` with an oversized `corner_r` reached OCCT and failed there with
/// an opaque kernel error. The lowering must instead synthesize a constraint
/// on the enclosing template, which `Engine::check` evaluates (and the solver
/// honours) at runtime.
///
/// RED before the emission branch lands: `template.constraints` is empty for
/// the param-driven source.
#[test]
fn rounded_rect_param_driven_corner_r_emits_runtime_constraint() {
    let source = r#"structure def S {
    param corner_r: Length = 5mm
    let body = rounded_rect(40mm, 30mm, corner_r)
}"#;
    // The param-driven path must still compile clean — the point of the
    // runtime constraint is to CHECK the radius, not to false-flag it.
    let template = compile_template_no_errors(source, "S");

    assert_eq!(
        template.constraints.len(),
        1,
        "a param-driven corner_r must synthesize exactly one runtime constraint, got: {:#?}",
        template.constraints
    );
    let constraint = &template.constraints[0];
    assert_eq!(
        constraint.expr.result_type,
        Type::Bool,
        "a synthesized constraint predicate must be Bool-typed, got: {:?}",
        constraint.expr.result_type
    );
    // The label is the only text the designer sees on a violation:
    // SimpleConstraintChecker emits no span, and Engine::labeled_diagnostics
    // substitutes the label for the constraint id in the message.
    let label = constraint
        .label
        .as_deref()
        .expect("a synthesized constraint must carry a label");
    assert!(
        !label.is_empty(),
        "a synthesized constraint's label must be non-empty"
    );

    // Negative half: the all-literal VALID call is decided statically, so it
    // must NOT gain a redundant runtime constraint. The static check at
    // geometry.rs is strictly stronger there (it aborts the lowering outright),
    // so emitting one would be pure noise in `reify check` and an extra term in
    // the solver's objective.
    let source_const = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, 5mm)
}"#;
    let template_const = compile_template_no_errors(source_const, "S");
    assert!(
        template_const.constraints.is_empty(),
        "an all-constant rounded_rect call must not synthesize a runtime constraint, got: {:#?}",
        template_const.constraints
    );
}

/// A `Type::Error` `corner_r` must synthesize NOTHING.
///
/// Compilation continues past a recorded type error, so the emission branch is
/// genuinely reachable with the poison type: `rounded_rect(40mm, 30mm,
/// nonexistent)` compiles the argument to `Type::Error` and reaches the branch
/// with "unresolved name" already on the diagnostic list. A predicate built over
/// poison evaluates to `Undef`, i.e. an Indeterminate-constraint warning stacked
/// on top of the genuine error — noise at exactly the moment `reify check`'s
/// output is least readable, saying nothing the root-cause error does not.
///
/// RED before the `is_error()` short-circuit lands: one constraint, not zero.
#[test]
fn type_error_corner_r_synthesizes_no_runtime_constraint() {
    let source = r#"structure def S {
    let body = rounded_rect(40mm, 30mm, nonexistent)
}"#;
    let (template, diagnostics) = reify_test_support::compile_template(source, "S");

    // Precondition — the half that makes the skip sound: the designer is
    // already being told what is wrong. If this ever stops erroring, dropping
    // the constraint would be dropping the ONLY signal and the guard would have
    // to go with it.
    assert!(
        !reify_test_support::collect_errors(&diagnostics).is_empty(),
        "an unresolved name must be an error in its own right — the skip below \
         is only sound because this fires; got: {diagnostics:#?}"
    );

    assert!(
        template.constraints.is_empty(),
        "a Type::Error corner_r must not also synthesize a runtime constraint, got: {:#?}",
        template.constraints
    );
}

/// NEGATIVE HALF — that skip must stay keyed on `Type::Error`, NOT on "not a
/// Scalar".
///
/// A `Bool` radius reaches the same branch with its own type intact, and —
/// measured, not assumed — the module compiles CLEAN: no error and no warning
/// names it anywhere. So the synthesized constraint is the designer's only
/// signal identifying the constructor and the argument before the geometry fails
/// opaquely inside the kernel. Widening the guard to every non-Scalar type would
/// delete that signal and restore exactly the silent skip #5665 removes.
///
/// (What the constraint then DECIDES — `Indeterminate`, the predicate having
/// evaluated to `Undef` — is pinned runtime-side in
/// `reify-eval/tests/harness_geometry/rounded_corner_runtime_constraint.rs`.)
#[test]
fn wrongly_typed_but_error_free_corner_r_keeps_its_constraint() {
    let source = r#"structure def S {
    param corner_r: Bool = true
    let body = rounded_rect(40mm, 30mm, corner_r)
}"#;
    let (template, diagnostics) = reify_test_support::compile_template(source, "S");

    assert!(
        diagnostics.is_empty(),
        "premise: a Bool corner_r is reported by nothing today — if that ever \
         changes, re-derive whether this constraint is still the only signal; \
         got: {diagnostics:#?}"
    );
    assert_eq!(
        corner_labels(&template),
        vec!["rounded_rect_corner_r_valid_0"],
        "a wrongly-typed corner_r that nothing else reports must keep its \
         constraint, got: {:#?}",
        template.constraints
    );
}

/// Several param-driven rounded-corner calls in one entity must each get their
/// OWN constraint, with a label that names its constructor and distinguishes
/// it from its siblings.
///
/// The label is the only channel that reaches the designer — the emitted
/// diagnostic carries no span — so a shared or constructor-agnostic label
/// leaves "which of these three calls is wrong?" unanswerable.
///
/// RED before the label lands: all three are the same fixed placeholder.
#[test]
fn multiple_param_driven_rounded_calls_get_distinct_self_identifying_labels() {
    let source = r#"structure def S {
    param corner_r: Length = 5mm
    let plate = rounded_rect(40mm, 30mm, corner_r)
    let block = rounded_box(40mm, 30mm, 20mm, corner_r)
    let shim = rounded_rect(60mm, 50mm, corner_r)
}"#;
    let template = compile_template_no_errors(source, "S");

    assert_eq!(
        template.constraints.len(),
        3,
        "each param-driven rounded-corner call needs its own constraint, got: {:#?}",
        template.constraints
    );

    let labels: Vec<&str> = template
        .constraints
        .iter()
        .map(|c| {
            c.label
                .as_deref()
                .expect("a synthesized constraint must carry a label")
        })
        .collect();

    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(
        unique.len(),
        3,
        "labels must be pairwise distinct so a violation identifies WHICH call, got: {labels:?}"
    );

    // Each label names the constructor it came from: two rounded_rect, one
    // rounded_box.
    assert_eq!(
        labels.iter().filter(|l| l.contains("rounded_rect")).count(),
        2,
        "both rounded_rect calls must name their constructor, got: {labels:?}"
    );
    assert_eq!(
        labels.iter().filter(|l| l.contains("rounded_box")).count(),
        1,
        "the rounded_box call must name its constructor, got: {labels:?}"
    );
}

/// The labels of one constraint vec, in emission order.
///
/// Shared by `corner_labels` (the entity's flat, unguarded list) and by the
/// guarded-arm assertions below (a `CompiledGuardedGroup`'s `constraints` /
/// `else_constraints`), so both express the same "a synthesized constraint must
/// carry a label" expectation in one place.
fn group_arm_labels(constraints: &[reify_compiler::CompiledConstraint]) -> Vec<String> {
    constraints
        .iter()
        .map(|c| {
            c.label
                .as_deref()
                .expect("a synthesized constraint must carry a label")
                .to_string()
        })
        .collect()
}

/// The corner-radius labels of a compiled template, in emission order.
fn corner_labels(template: &reify_compiler::TopologyTemplate) -> Vec<String> {
    group_arm_labels(&template.constraints)
}

/// ONE source-level rounded-corner call must synthesize exactly ONE constraint,
/// however many times its geometry let is later reused.
///
/// `compile_geometry_call_inner` re-inlines a geometry let's initializer on
/// EVERY reference of its name in geometry-argument position (reached from
/// `resolve_boolean_arg`), threading the sink through with `reborrow()` — so
/// without a guard each re-inline re-runs the rounded-corner arm and pushes
/// another byte-identical copy, growing N+1 with the number of reuses.
///
/// Two designer-visible symptoms, both wrong: an oversized `corner_r` reports
/// several Violated constraints carrying several different indices for a single
/// offending call (so the label's index actively misleads about WHICH call is
/// at fault), and the solver sees the same predicate's residual several times
/// over, over-weighting that one piece of geometry.
///
/// RED before the dedup lands: 2 for one reuse, 4 for the deeper chain.
#[test]
fn geometry_let_reused_as_boolean_arg_emits_one_constraint() {
    let source = r#"structure def S {
    param corner_r: Length = 5mm
    let plate = rounded_box(40mm, 30mm, 20mm, corner_r)
    let cut = difference(plate, box(10mm, 10mm, 10mm))
}"#;
    let template = compile_template_no_errors(source, "S");
    assert_eq!(
        corner_labels(&template),
        vec!["rounded_box_corner_r_valid_0"],
        "one rounded_box call reused as a boolean arg must synthesize exactly \
         one constraint, got: {:#?}",
        template.constraints
    );

    // A deeper reuse chain: `plate` is consumed twice more. A one-shot flag on
    // the arm would not be enough — the growth is per-reuse, so this must pin
    // 1 as well, not merely "fewer than before".
    let source_chain = r#"structure def S {
    param corner_r: Length = 5mm
    let plate = rounded_box(40mm, 30mm, 20mm, corner_r)
    let a = difference(plate, box(10mm, 10mm, 10mm))
    let b = union(a, plate)
}"#;
    let template_chain = compile_template_no_errors(source_chain, "S");
    assert_eq!(
        corner_labels(&template_chain),
        vec!["rounded_box_corner_r_valid_0"],
        "reusing the same geometry let three times must still synthesize one \
         constraint, got: {:#?}",
        template_chain.constraints
    );
}

/// NEGATIVE HALF — two genuinely DISTINCT source calls must keep their two
/// constraints even when their argument text is character-for-character
/// identical.
///
/// This guards against an over-broad dedup. The two calls below carry DIFFERENT
/// spans but the SAME `expr.content_hash` — the predicates really are
/// structurally identical — so a dedup keyed on content alone would silently
/// collapse them into one and drop a real check on the second call.
///
/// Passes today; must keep passing.
#[test]
fn distinct_rounded_calls_with_identical_args_keep_separate_constraints() {
    let source = r#"structure def S {
    param corner_r: Length = 5mm
    let a = rounded_box(40mm, 30mm, 20mm, corner_r)
    let b = rounded_box(40mm, 30mm, 20mm, corner_r)
}"#;
    let template = compile_template_no_errors(source, "S");
    let labels = corner_labels(&template);
    assert_eq!(
        labels.len(),
        2,
        "two distinct source calls each need their own constraint even with \
         identical argument text, got: {:#?}",
        template.constraints
    );
    let unique: std::collections::HashSet<&str> =
        labels.iter().map(String::as_str).collect();
    assert_eq!(
        unique.len(),
        2,
        "the two constraints must stay distinguishable by label, got: {labels:?}"
    );
}

/// NEGATIVE HALF — a rounded-corner call inside a GUARDED geometry let must
/// still get its constraint, which is why the re-inline path must be
/// deduplicated rather than muted.
///
/// A guarded geometry let emits NO realization of its own (entity.rs's
/// third-pass `GuardedGroup` arm documents that as a separate, unimplemented
/// feature) while `collect_geometry_exprs` DOES recurse into guarded groups when
/// building the re-inline map. So for this shape the re-inline is the ONLY
/// emission path: suppressing it would drop the constraint entirely and
/// reintroduce exactly the silent skip task #5665 exists to remove.
///
/// ALSO the no-regression half of the guarded-arm routing below: this shape
/// reaches the sink ONLY via the TOP-LEVEL `cut` realization's re-inline path,
/// and `cut` is realized unconditionally — so its predicate is correctly
/// UNGUARDED and must stay on the flat `template.constraints`. A naive "any
/// rounded call under a `where` goes into that arm" fix would wrongly move it.
///
/// Passes today; must keep passing.
#[test]
fn guarded_geometry_let_reused_as_boolean_arg_still_emits_one_constraint() {
    let source = r#"structure def S {
    param active: Bool = true
    param corner_r: Length = 5mm
    where active {
        let plate = rounded_box(40mm, 30mm, 20mm, corner_r)
    }
    let cut = difference(plate, box(10mm, 10mm, 10mm))
}"#;
    let template = compile_template_no_errors(source, "S");
    assert_eq!(
        corner_labels(&template),
        vec!["rounded_box_corner_r_valid_0"],
        "a guarded rounded_box let reaches the sink only via the re-inline \
         path — it must emit exactly one constraint, not zero and not two, \
         got: {:#?}",
        template.constraints
    );
}

/// A rounded-corner call inside a `where`/`else` group must file its constraint
/// into THAT arm of the enclosing `CompiledGuardedGroup` — not onto the
/// entity's flat, UNGUARDED `template.constraints`.
///
/// The two arms of a `where`/`else` are mutually exclusive: at most one of
/// `plate` and `plate2` is ever realized. Filing both predicates on the flat
/// list makes BOTH enforced unconditionally, so at least one of them always
/// describes geometry that was never lowered — a `Violated` verdict naming a
/// constructor the design never used, flipping `reify check`'s exit code on a
/// design that is in fact valid.
///
/// The routing target is not invented for this fix: `CompiledGuardedGroup`
/// already carries per-arm `constraints` / `else_constraints`, which
/// `collect_active_constraints` gates on the group's `guard_value_cell`. This
/// only puts the synthesized predicate where a hand-written one would go.
///
/// RED before the arm routing lands: both constraints sit on the flat list and
/// both guarded arms are empty.
#[test]
fn guarded_geometry_constraint_lands_in_its_own_arm() {
    let source = r#"structure def S {
    param active: Bool = false
    param corner_r: Length = 5mm
    where active {
        param plate: Solid = rounded_box(40mm, 30mm, 20mm, corner_r)
    } else {
        param plate2: Solid = rounded_rect(60mm, 50mm, corner_r)
    }
}"#;
    let template = compile_template_no_errors(source, "S");

    assert_eq!(
        corner_labels(&template),
        Vec::<String>::new(),
        "a guarded call's predicate must NOT land on the entity's unguarded \
         constraint list — that enforces a dead arm's geometry, got: {:#?}",
        template.constraints
    );

    assert_eq!(
        template.guarded_groups.len(),
        1,
        "fixture must compile to exactly one guarded group, got: {:#?}",
        template.guarded_groups
    );
    let group = &template.guarded_groups[0];

    let where_labels = group_arm_labels(&group.constraints);
    assert_eq!(
        where_labels.len(),
        1,
        "the `where` arm must hold exactly its own rounded_box predicate, \
         got: {where_labels:?}"
    );
    assert!(
        where_labels[0].starts_with("rounded_box_corner_r_valid_"),
        "the `where` arm's constraint must be the rounded_box one, got: \
         {where_labels:?}"
    );

    let else_labels = group_arm_labels(&group.else_constraints);
    assert_eq!(
        else_labels.len(),
        1,
        "the `else` arm must hold exactly its own rounded_rect predicate, \
         got: {else_labels:?}"
    );
    assert!(
        else_labels[0].starts_with("rounded_rect_corner_r_valid_"),
        "the `else` arm's constraint must be the rounded_rect one, got: \
         {else_labels:?}"
    );
}

/// A rounded-corner call inside a NESTED `where`/`else` must land in the INNER
/// group's arm, not in the outer one's.
///
/// `emit_guarded_geometry_realizations` recurses into nested groups, so the arm
/// polarity has to be re-derived at each level rather than inherited: an inner
/// `else` member reached from an outer `where` belongs to the inner group's
/// `else_constraints`. The guard CELL is already correct at any depth —
/// `resolve_guard` yields the innermost group a member was registered under —
/// so only the polarity is at stake here.
///
/// Groups are located by `guard_value_cell`, never by vec position: measured,
/// `guarded_groups` is populated in POST-order, so the nested `__guard_1` group
/// sits at index 0 and the outer `__guard_0` at index 1. An index-based
/// assertion would silently assert the opposite of what it reads.
///
/// RED before the polarity threading lands: the recursive `GuardedGroup` arm
/// forwards the caller's polarity, so `inner` and `inner2` both land on
/// whichever arm the outer call was given.
#[test]
fn nested_guarded_geometry_constraint_lands_in_the_inner_arm() {
    let source = r#"structure def S {
    param active: Bool = true
    param corner_r: Length = 5mm
    where active {
        param plate: Solid = rounded_box(40mm, 30mm, 20mm, corner_r)
        where corner_r > 1mm {
            param inner: Solid = rounded_rect(70mm, 55mm, corner_r)
        } else {
            param inner2: Solid = rounded_rect(80mm, 65mm, corner_r)
        }
    } else {
        param plate2: Solid = rounded_rect(60mm, 50mm, corner_r)
    }
}"#;
    let template = compile_template_no_errors(source, "S");

    assert_eq!(
        corner_labels(&template),
        Vec::<String>::new(),
        "no guarded call's predicate may land on the entity's unguarded list, \
         got: {:#?}",
        template.constraints
    );
    assert_eq!(
        template.guarded_groups.len(),
        2,
        "fixture must compile to two guarded groups, got: {:#?}",
        template.guarded_groups
    );

    // Locate each group by its guard cell, NOT by index — the vec is in
    // post-order, so the nested group comes first.
    let find = |cell: &str| {
        template
            .guarded_groups
            .iter()
            .find(|g| g.guard_value_cell.member == cell)
            .unwrap_or_else(|| {
                panic!(
                    "no guarded group for {cell}; cells were: {:?}",
                    template
                        .guarded_groups
                        .iter()
                        .map(|g| g.guard_value_cell.member.clone())
                        .collect::<Vec<_>>()
                )
            })
    };
    let outer = find("__guard_0");
    let inner = find("__guard_1");

    // The OUTER group holds only its own directly-declared pair.
    assert_eq!(
        group_arm_labels(&outer.constraints),
        vec!["rounded_box_corner_r_valid_0"],
        "the outer `where` arm must hold only `plate`'s predicate, got: {:#?}",
        outer.constraints
    );
    let outer_else = group_arm_labels(&outer.else_constraints);
    assert_eq!(
        outer_else.len(),
        1,
        "the outer `else` arm must hold only `plate2`'s predicate, got: \
         {outer_else:?}"
    );
    assert!(
        outer_else[0].starts_with("rounded_rect_corner_r_valid_"),
        "the outer `else` arm's constraint must be `plate2`'s rounded_rect one, \
         got: {outer_else:?}"
    );

    // The INNER group holds the nested pair, one per polarity.
    let inner_where = group_arm_labels(&inner.constraints);
    assert_eq!(
        inner_where.len(),
        1,
        "the inner `where` arm must hold exactly `inner`'s predicate, got: \
         {inner_where:?}"
    );
    assert!(
        inner_where[0].starts_with("rounded_rect_corner_r_valid_"),
        "the inner `where` arm's constraint must be a rounded_rect one, got: \
         {inner_where:?}"
    );
    let inner_else = group_arm_labels(&inner.else_constraints);
    assert_eq!(
        inner_else.len(),
        1,
        "the inner `else` arm must hold exactly `inner2`'s predicate, got: \
         {inner_else:?}"
    );
    assert!(
        inner_else[0].starts_with("rounded_rect_corner_r_valid_"),
        "the inner `else` arm's constraint must be a rounded_rect one, got: \
         {inner_else:?}"
    );

    // All four predicates accounted for, each exactly once.
    let total = outer.constraints.len()
        + outer.else_constraints.len()
        + inner.constraints.len()
        + inner.else_constraints.len();
    assert_eq!(
        total, 4,
        "each of the four rounded calls must synthesize exactly one constraint \
         into exactly one arm, got {total}"
    );
}
