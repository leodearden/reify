//! Characterization / golden harness for `compile_geometry_op` (task #4673,
//! PRD `docs/prds/geometry-op-dispatch-registry.md` DD-4 / §9 L4).
//!
//! # Contract: byte-identical equivalence oracle for L5
//!
//! This suite snapshots the EXACT `Result<reify_ir::GeometryOp, String>` plus the
//! emitted `reify_core::Diagnostic`s produced by the CURRENT, unrefactored
//! `compile_geometry_op` for every [`reify_compiler::CompiledGeometryOp`] variant
//! × nested kind. The captured goldens are the equivalence oracle that gates the
//! highest-risk leaf L5 (the Axis-3 behavioral refactor of that function): any
//! behavioral drift introduced by L5 fails a golden compare here with a clear,
//! paste-ready diff. L5 MUST keep every golden in this file byte-identical green.
//!
//! # Coverage mechanism: compile-time-exhaustive `match`, NOT runtime iteration
//!
//! Per-kind coverage is enforced structurally: each `*_case(kind)` builder and
//! each `*_golden(kind)` lookup is an EXHAUSTIVE `match` with **no `_` arm**, so
//! adding a new variant to any kind enum in `reify-compiler` is a COMPILE error
//! (E0004) here until a golden case is added — a strictly stronger guarantee than
//! `strum::EnumIter` runtime iteration, and it touches zero `reify-compiler` src.
//! Each family carries an `ALL_*` array that the `characterize_*_family` test
//! iterates over. **`Modify` only**: the array's length is also cross-checked at
//! runtime against `reify_compiler::ModifyKind::VARIANT_COUNT` — a real tripwire
//! tied to the compiler's authoritative count. The other seven families carry
//! `ALL_*` as statically-typed `[Kind; N]` arrays; their `assert_eq!(len(), N)`
//! assertions are **tautological** (`.len()` is a compile-time constant equal to
//! the type's `N`). A new variant omitted from one of those arrays would not be
//! caught by the assertion — only the exhaustive `match` (no `_` arm) in each
//! `*_case`/`*_golden` acts as the real coverage enforcer for those families.
//!
//! # Reaching the function under test
//!
//! `compile_geometry_op` is `pub(crate)` inside the private `mod geometry_ops;`,
//! so it is reached via the cfg-gated 1:1 delegate
//! [`reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe`],
//! activated by the existing self-dev-dep
//! `reify-eval = { path = ".", features = ["test-instrumentation"] }`.
//!
//! # Snapshot determinism
//!
//! Inputs are synthetic literals built via [`lit`]; `CompiledExpr::literal`
//! attaches no span, so the `{:#?}` Debug of the produced `GeometryOp`/`Err`
//! string and the `(severity, message)` diagnostic projection are byte-stable
//! across runs. Goldens were captured via a RED→GREEN bootstrap (placeholder →
//! run on current code → paste actual).
//!
//! # Suite census (the locked oracle L5 must preserve)
//!
//! 9 `CompiledGeometryOp` variant families × 48 nested kinds, across 10 tests:
//! Primitive 7, Boolean 3, Modify 9 (+3 edges-selector branch cases), Transform
//! 5, Pattern 5 (+2 value-form branch cases), Sweep 8, Curve 6, Profile 4,
//! Surface 1 (7+3+9+5+5+8+6+4+1 = 48). The `coverage_*` test pins the
//! 9-family / 48-kind census; the per-family `characterize_*` tests plus
//! `_assert_variant_families_exhaustive` are the compile-time tripwires for a
//! newly-added variant or nested kind. L5 MUST keep all 10 tests byte-identical
//! green.

use std::collections::HashMap;

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PatternKind, PrimitiveKind,
    ProfileKind, SurfaceKind, SweepKind, TransformKind,
};
use reify_core::Diagnostic;
use reify_ir::{CompiledExpr, GeometryHandleId, GeometryOp, Value, ValueMap};

use reify_eval::geometry_op_characterization_probe::compile_geometry_op_probe;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a `CompiledExpr` literal from a constant f64 (dimensionless scalar).
///
/// Mirrors the in-module `literal_f64` helper at `geometry_ops.rs` so the
/// characterization inputs match the production unit tests' representative args.
fn lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::Real(v), reify_core::Type::dimensionless_scalar())
}

/// Build a `CompiledExpr` literal wrapping a `Value::Transform` (quaternion
/// `[w,x,y,z]` rotation + SI-metre `[tx,ty,tz]` translation).
///
/// Mirrors the in-module `transform_of` / `literal_transform` helpers (used by
/// the `compile_geometry_op_apply_transform_*` unit tests) so the ApplyTransform
/// characterization input is byte-faithful to the production reference.
fn lit_transform(q: [f64; 4], t: [f64; 3]) -> CompiledExpr {
    let v = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: q[0],
            x: q[1],
            y: q[2],
            z: q[3],
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(t[0]),
            Value::length(t[1]),
            Value::length(t[2]),
        ])),
    };
    CompiledExpr::literal(v, reify_core::Type::transform(3))
}

/// Build a `CompiledExpr` literal wrapping an arbitrary `Value`. The literal's
/// declared `Type` is inert here — `reify_expr::eval_expr` returns the embedded
/// value verbatim for a `Literal` — so this is the right tool for the synthetic
/// `edges`/`faces` selector args (e.g. an empty `Value::List`).
fn lit_raw(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, reify_core::Type::dimensionless_scalar())
}

/// A `Value::Vector` of 3 dimensionless `Real` components (accepted by the
/// production `point3_components` decoder used by `decode_axis`/`decode_plane`).
fn vec3_value(c: [f64; 3]) -> Value {
    Value::Vector(vec![Value::Real(c[0]), Value::Real(c[1]), Value::Real(c[2])])
}

/// A `Value::Axis` for the Circular pattern value-form sub-branch (decoded by
/// `decode_axis`; the direction is normalized to unit length by production).
fn axis_value(origin: [f64; 3], direction: [f64; 3]) -> Value {
    Value::Axis {
        origin: Box::new(vec3_value(origin)),
        direction: Box::new(vec3_value(direction)),
    }
}

/// A `Value::Plane` for the Mirror pattern value-form sub-branch (decoded by
/// `decode_plane`; the normal is normalized to unit length by production).
fn plane_value(origin: [f64; 3], normal: [f64; 3]) -> Value {
    Value::Plane {
        origin: Box::new(vec3_value(origin)),
        normal: Box::new(vec3_value(normal)),
    }
}

/// Build positional coordinate args (`c0`, `c1`, …) from a slice of f64. The
/// production `eval_all_args_to_f64` iterates `args` in Vec order (names are
/// inert), so this is how InterpCurve/BezierCurve/NurbsCurve/Polygon receive
/// their flat coordinate streams.
fn coord_args(coords: &[f64]) -> Vec<(String, CompiledExpr)> {
    coords
        .iter()
        .enumerate()
        .map(|(i, &v)| (format!("c{i}"), lit(v)))
        .collect()
}

/// Deterministic snapshot of a `compile_geometry_op` outcome.
///
/// `{:#?}` of the `Ok(GeometryOp)`/`Err(String)` result, followed by one
/// `[diag] <Severity> <message>` line per emitted diagnostic. The diagnostics
/// are projected to `(severity, message)` — the byte-stable, user-facing content
/// — to avoid any brittleness from `DiagnosticLabel` span formatting (the
/// in-module unit tests assert against `diag.severity` + `diag.message` for the
/// same reason).
fn snapshot(res: &Result<GeometryOp, String>, diags: &[Diagnostic]) -> String {
    let mut s = format!("{res:#?}");
    for d in diags {
        s.push_str(&format!("\n[diag] {} {:?}", d.severity.as_wire_str(), d.message));
    }
    s
}

/// Drive the probe against `op` with the given `step_handles` and return the
/// deterministic snapshot string. `values`, `functions`, `meta_map`, and
/// `named_steps` are empty — the synthetic cases need none of them, matching the
/// in-module unit-test call shape.
fn run(op: &CompiledGeometryOp, step_handles: &[GeometryHandleId]) -> String {
    let values = ValueMap::new();
    let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    let named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let result = compile_geometry_op_probe(
        op,
        &values,
        step_handles,
        &[],
        &meta_map,
        &named_steps,
        &mut diagnostics,
    );
    snapshot(&result, &diagnostics)
}

/// Compare the probe's snapshot for `op` against `golden`.
///
/// Returns `None` on a byte-identical match, or `Some(<paste-ready block>)` on
/// drift. The block is delimited so a RED→GREEN golden bootstrap can copy the
/// `actual` verbatim into the corresponding `*_golden` arm.
///
/// As an inspection aid (NOT a bypass of the gate), when `REIFY_CHAR_DUMP_DIR`
/// is set the captured `actual` is also written to `<dir>/<label>.snap`. This is
/// purely a side-channel for diffing/blessing during a deliberate bootstrap; the
/// golden compare below is unaffected, so any behavioral drift still fails the
/// test. Inert when the env var is unset (the steady-state CI path).
#[must_use]
fn characterize(
    label: &str,
    op: &CompiledGeometryOp,
    step_handles: &[GeometryHandleId],
    golden: &str,
) -> Option<String> {
    let actual = run(op, step_handles);
    if let Ok(dir) = std::env::var("REIFY_CHAR_DUMP_DIR") {
        let safe: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(format!("{dir}/{safe}.snap"), &actual);
    }
    if actual == golden {
        None
    } else {
        Some(format!(">>>BEGIN {label}>>>\n{actual}\n<<<END {label}<<<\n"))
    }
}

/// Join any collected drift blocks into one paste-ready panic payload (empty
/// string when every case matched its golden).
fn drift_report(blocks: &[String]) -> String {
    if blocks.is_empty() {
        String::new()
    } else {
        format!(
            "\n=== CHARACTERIZATION DRIFT — paste each block into its *_golden arm ===\n\n{}",
            blocks.join("\n")
        )
    }
}

// ---------------------------------------------------------------------------
// Primitive family (7 kinds): Box/Cylinder/Sphere/Tube/Cone/Wedge/Torus
// ---------------------------------------------------------------------------

/// Every `PrimitiveKind` variant, iterated by `characterize_primitive_family`.
/// The exhaustive matches in `primitive_case`/`primitive_golden` are the sole
/// compile-time tripwire: a new `PrimitiveKind` variant is a compile error until
/// both match arms and this array are updated. The `assert_eq!(len(), 7)` in the
/// test is tautological for this statically-typed `[PrimitiveKind; 7]` array and
/// cannot independently detect a variant omitted from here; **no `VARIANT_COUNT`
/// cross-check exists** for `PrimitiveKind` (unlike `ModifyKind`).
const ALL_PRIMITIVE: [PrimitiveKind; 7] = [
    PrimitiveKind::Box,
    PrimitiveKind::Cylinder,
    PrimitiveKind::Sphere,
    PrimitiveKind::Tube,
    PrimitiveKind::Cone,
    PrimitiveKind::Wedge,
    PrimitiveKind::Torus,
];

/// Build a representative `Primitive` op for `k`, supplying each arm's required
/// `eval_arg(...)` named args (see `geometry_ops.rs` Primitive arm). EXHAUSTIVE
/// match (no `_`): a new `PrimitiveKind` is a compile error until a case exists.
fn primitive_case(k: PrimitiveKind) -> CompiledGeometryOp {
    let args = match k {
        PrimitiveKind::Box => vec![
            ("width".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.02)),
            ("depth".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Cylinder => vec![
            ("radius".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.02)),
        ],
        PrimitiveKind::Sphere => vec![("radius".to_string(), lit(0.01))],
        PrimitiveKind::Tube => vec![
            ("outer_r".to_string(), lit(0.02)),
            ("inner_r".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Cone => vec![
            ("bottom_radius".to_string(), lit(0.02)),
            ("top_radius".to_string(), lit(0.01)),
            ("height".to_string(), lit(0.03)),
        ],
        PrimitiveKind::Wedge => vec![
            ("width".to_string(), lit(0.02)),
            ("depth".to_string(), lit(0.03)),
            ("height".to_string(), lit(0.04)),
            ("top_width".to_string(), lit(0.01)),
        ],
        PrimitiveKind::Torus => vec![
            ("major_radius".to_string(), lit(0.03)),
            ("minor_radius".to_string(), lit(0.01)),
        ],
    };
    CompiledGeometryOp::Primitive { kind: k, args }
}

/// Golden snapshot per `PrimitiveKind`. EXHAUSTIVE match (no `_`): a new kind
/// without a golden is a compile error (the G2 coverage signal). Placeholders
/// (`""`) are replaced with captured actuals during the step-2 GREEN bootstrap.
fn primitive_golden(k: PrimitiveKind) -> &'static str {
    match k {
        PrimitiveKind::Box => r#"Ok(
    Box {
        width: Real(
            0.01,
        ),
        height: Real(
            0.02,
        ),
        depth: Real(
            0.03,
        ),
    },
)"#,
        PrimitiveKind::Cylinder => r#"Ok(
    Cylinder {
        radius: Real(
            0.01,
        ),
        height: Real(
            0.02,
        ),
    },
)"#,
        PrimitiveKind::Sphere => r#"Ok(
    Sphere {
        radius: Real(
            0.01,
        ),
    },
)"#,
        PrimitiveKind::Tube => r#"Ok(
    Tube {
        outer_r: Real(
            0.02,
        ),
        inner_r: Real(
            0.01,
        ),
        height: Real(
            0.03,
        ),
    },
)"#,
        PrimitiveKind::Cone => r#"Ok(
    Cone {
        bottom_radius: Real(
            0.02,
        ),
        top_radius: Real(
            0.01,
        ),
        height: Real(
            0.03,
        ),
    },
)"#,
        PrimitiveKind::Wedge => r#"Ok(
    Wedge {
        width: Real(
            0.02,
        ),
        depth: Real(
            0.03,
        ),
        height: Real(
            0.04,
        ),
        top_width: Real(
            0.01,
        ),
    },
)"#,
        PrimitiveKind::Torus => r#"Ok(
    Torus {
        major_radius: Real(
            0.03,
        ),
        minor_radius: Real(
            0.01,
        ),
    },
)"#,
    }
}

#[test]
fn characterize_primitive_family() {
    // Tautological for [PrimitiveKind; 7] — fires only if the static-array type
    // annotation and this literal are manually out of sync. Real coverage
    // enforcement is the no-`_` match in primitive_case / primitive_golden.
    assert_eq!(ALL_PRIMITIVE.len(), 7, "ALL_PRIMITIVE size and annotation mismatch");
    let drift: Vec<String> = ALL_PRIMITIVE
        .iter()
        .filter_map(|&k| {
            characterize(&format!("primitive:{k}"), &primitive_case(k), &[], primitive_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Boolean family (3 ops): Union/Difference/Intersection
// ---------------------------------------------------------------------------

/// Step handles backing the Boolean `GeomRef::Step(0)`/`Step(1)` operands, so
/// both `left` and `right` resolve to a concrete `GeometryHandleId`.
fn boolean_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(10), GeometryHandleId(11)]
}

/// Every `BooleanOp` variant, iterated by `characterize_boolean_family`.
/// The exhaustive match in `boolean_golden` is the sole compile-time tripwire.
/// The `assert_eq!(len(), 3)` is tautological for `[BooleanOp; 3]`; no
/// `VARIANT_COUNT` cross-check exists for `BooleanOp`.
const ALL_BOOLEAN: [BooleanOp; 3] =
    [BooleanOp::Union, BooleanOp::Difference, BooleanOp::Intersection];

/// Build a `Boolean` op for `op` with both operands resolvable via
/// `boolean_step_handles` (`left = Step(0)`, `right = Step(1)`).
fn boolean_case(op: BooleanOp) -> CompiledGeometryOp {
    CompiledGeometryOp::Boolean {
        op,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    }
}

/// Golden snapshot per `BooleanOp`. EXHAUSTIVE match (no `_`): a new op without
/// a golden is a compile error. Placeholders replaced during the GREEN bootstrap.
fn boolean_golden(op: BooleanOp) -> &'static str {
    match op {
        BooleanOp::Union => r#"Ok(
    Union {
        left: GeometryHandleId(
            10,
        ),
        right: GeometryHandleId(
            11,
        ),
    },
)"#,
        BooleanOp::Difference => r#"Ok(
    Difference {
        left: GeometryHandleId(
            10,
        ),
        right: GeometryHandleId(
            11,
        ),
    },
)"#,
        BooleanOp::Intersection => r#"Ok(
    Intersection {
        left: GeometryHandleId(
            10,
        ),
        right: GeometryHandleId(
            11,
        ),
    },
)"#,
    }
}

#[test]
fn characterize_boolean_family() {
    // Tautological for [BooleanOp; 3] — see ALL_BOOLEAN doc for rationale.
    assert_eq!(ALL_BOOLEAN.len(), 3, "ALL_BOOLEAN size and annotation mismatch");
    let handles = boolean_step_handles();
    let drift: Vec<String> = ALL_BOOLEAN
        .iter()
        .filter_map(|&op| {
            characterize(&format!("boolean:{op}"), &boolean_case(op), &handles, boolean_golden(op))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Transform family (5 kinds): Translate/Rotate/Scale/RotateAround/ApplyTransform
// ---------------------------------------------------------------------------

/// Single step handle backing the Transform `target = GeomRef::Step(0)`.
fn transform_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(42)]
}

/// Every `TransformKind` variant, iterated by `characterize_transform_family`.
/// The exhaustive matches in `transform_case`/`transform_golden` are the sole
/// compile-time tripwire. The `assert_eq!(len(), 5)` is tautological for
/// `[TransformKind; 5]`; no `VARIANT_COUNT` cross-check exists for `TransformKind`.
const ALL_TRANSFORM: [TransformKind; 5] = [
    TransformKind::Translate,
    TransformKind::Rotate,
    TransformKind::Scale,
    TransformKind::RotateAround,
    TransformKind::ApplyTransform,
];

/// Build a representative `Transform` op for `k`, supplying each arm's required
/// args (see `geometry_ops.rs` Transform arm). EXHAUSTIVE match (no `_`). Args
/// mirror the in-module `compile_geometry_op_{scale,rotate_around,apply_transform}`
/// unit tests; ApplyTransform uses an identity-rotation `lit_transform`.
fn transform_case(k: TransformKind) -> CompiledGeometryOp {
    let args = match k {
        TransformKind::Translate => vec![
            ("dx".to_string(), lit(0.01)),
            ("dy".to_string(), lit(0.02)),
            ("dz".to_string(), lit(0.03)),
        ],
        TransformKind::Rotate => vec![
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("angle".to_string(), lit(1.0)),
        ],
        TransformKind::Scale => vec![("factor".to_string(), lit(2.0))],
        TransformKind::RotateAround => vec![
            ("px".to_string(), lit(0.05)),
            ("py".to_string(), lit(0.0)),
            ("pz".to_string(), lit(0.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("angle".to_string(), lit(1.0)),
        ],
        TransformKind::ApplyTransform => vec![(
            "transform".to_string(),
            lit_transform([1.0, 0.0, 0.0, 0.0], [0.01, 0.02, 0.03]),
        )],
    };
    CompiledGeometryOp::Transform {
        kind: k,
        target: GeomRef::Step(0),
        args,
    }
}

/// Golden snapshot per `TransformKind`. EXHAUSTIVE match (no `_`). Placeholders
/// replaced during the GREEN bootstrap.
fn transform_golden(k: TransformKind) -> &'static str {
    match k {
        TransformKind::Translate => r#"Ok(
    Translate {
        target: GeometryHandleId(
            42,
        ),
        dx: 0.01,
        dy: 0.02,
        dz: 0.03,
    },
)"#,
        TransformKind::Rotate => r#"Ok(
    Rotate {
        target: GeometryHandleId(
            42,
        ),
        axis: [
            0.0,
            0.0,
            1.0,
        ],
        angle_rad: 1.0,
    },
)"#,
        TransformKind::Scale => r#"Ok(
    Scale {
        target: GeometryHandleId(
            42,
        ),
        factor: 2.0,
    },
)"#,
        TransformKind::RotateAround => r#"Ok(
    RotateAround {
        target: GeometryHandleId(
            42,
        ),
        point: [
            0.05,
            0.0,
            0.0,
        ],
        axis: [
            0.0,
            0.0,
            1.0,
        ],
        angle_rad: 1.0,
    },
)"#,
        TransformKind::ApplyTransform => r#"Ok(
    ApplyTransform {
        target: GeometryHandleId(
            42,
        ),
        rotation: [
            1.0,
            0.0,
            0.0,
            0.0,
        ],
        translation: [
            0.01,
            0.02,
            0.03,
        ],
    },
)"#,
    }
}

#[test]
fn characterize_transform_family() {
    // Tautological for [TransformKind; 5] — see ALL_TRANSFORM doc for rationale.
    assert_eq!(ALL_TRANSFORM.len(), 5, "ALL_TRANSFORM size and annotation mismatch");
    let handles = transform_step_handles();
    let drift: Vec<String> = ALL_TRANSFORM
        .iter()
        .filter_map(|&k| {
            characterize(&format!("transform:{k}"), &transform_case(k), &handles, transform_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Modify family (9 kinds): Fillet/Chamfer/ChamferAsymmetric/Shell/Draft/
// Thicken/ZoneSlab/OffsetSolid/OffsetCurve
// ---------------------------------------------------------------------------

/// Single step handle backing the Modify `target = GeomRef::Step(0)`. For Draft
/// the production arm derives the neutral plane from `step_handles.last()`, so
/// this same handle also serves as the Draft plane.
fn modify_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(50)]
}

/// Every `ModifyKind` variant, iterated by `characterize_modify_family`.
/// The exhaustive matches in `modify_case`/`modify_golden` are the per-kind
/// compile-time tripwire. Additionally, `characterize_modify_family` performs a
/// **real runtime cross-check** of `ALL_MODIFY.len()` against
/// `reify_compiler::ModifyKind::VARIANT_COUNT` — the compiler's authoritative
/// count — so adding a new `ModifyKind` in `reify-compiler` without updating
/// this array fails the test at runtime even if the exhaustive matches were
/// already patched.
const ALL_MODIFY: [ModifyKind; 9] = [
    ModifyKind::Fillet,
    ModifyKind::Chamfer,
    ModifyKind::ChamferAsymmetric,
    ModifyKind::Shell,
    ModifyKind::Draft,
    ModifyKind::Thicken,
    ModifyKind::ZoneSlab,
    ModifyKind::OffsetSolid,
    ModifyKind::OffsetCurve,
];

/// The Modify kinds with a distinct 2-arg (no selector) vs 3-arg (edges
/// selector) code path. The base `modify_case` exercises the 2-arg form; the
/// `:edges` extra cases below exercise the `Some(expr)` selector branch.
const MODIFY_EDGES_VARIANTS: [ModifyKind; 3] =
    [ModifyKind::Fillet, ModifyKind::Chamfer, ModifyKind::ChamferAsymmetric];

/// Build a representative base `Modify` op for `k` (the 2-arg / no-selector form
/// for the Fillet/Chamfer/ChamferAsymmetric kinds). EXHAUSTIVE match (no `_`):
/// see `geometry_ops.rs` Modify arm for each kind's required `eval_arg` names.
fn modify_case(k: ModifyKind) -> CompiledGeometryOp {
    let args = match k {
        ModifyKind::Fillet => vec![("radius".to_string(), lit(0.005))],
        ModifyKind::Chamfer => vec![("distance".to_string(), lit(0.005))],
        ModifyKind::ChamferAsymmetric => vec![
            ("d1".to_string(), lit(0.004)),
            ("d2".to_string(), lit(0.006)),
        ],
        ModifyKind::Shell => vec![("thickness".to_string(), lit(0.002))],
        ModifyKind::Draft => vec![("angle".to_string(), lit(0.1))],
        ModifyKind::Thicken => vec![("offset".to_string(), lit(0.003))],
        ModifyKind::ZoneSlab => vec![("width".to_string(), lit(0.01))],
        ModifyKind::OffsetSolid => vec![("distance".to_string(), lit(0.002))],
        ModifyKind::OffsetCurve => vec![("distance".to_string(), lit(0.002))],
    };
    CompiledGeometryOp::Modify {
        kind: k,
        target: GeomRef::Step(0),
        args,
    }
}

/// Build the 3-arg (edges-selector) form for a `MODIFY_EDGES_VARIANTS` kind by
/// appending an `edges` arg to the base case. An empty `Value::List` drives the
/// resolver's anti-zero-edges guard (Err + `EmptyEdgeSelection` diagnostic) —
/// distinct from the base 2-arg `Ok`, characterizing both branches.
fn modify_case_with_edges(k: ModifyKind) -> CompiledGeometryOp {
    let CompiledGeometryOp::Modify { kind, target, mut args } = modify_case(k) else {
        unreachable!("modify_case always builds a Modify op");
    };
    args.push(("edges".to_string(), lit_raw(Value::List(vec![]))));
    CompiledGeometryOp::Modify { kind, target, args }
}

/// Golden snapshot per `ModifyKind` (base / 2-arg form). EXHAUSTIVE match (no
/// `_`). Placeholders replaced during the GREEN bootstrap.
fn modify_golden(k: ModifyKind) -> &'static str {
    match k {
        ModifyKind::Fillet => r#"Ok(
    Fillet {
        target: GeometryHandleId(
            50,
        ),
        edges: [],
        radius: Real(
            0.005,
        ),
    },
)"#,
        ModifyKind::Chamfer => r#"Ok(
    Chamfer {
        target: GeometryHandleId(
            50,
        ),
        edges: [],
        distance: Real(
            0.005,
        ),
    },
)"#,
        ModifyKind::ChamferAsymmetric => r#"Ok(
    ChamferAsymmetric {
        target: GeometryHandleId(
            50,
        ),
        edges: [],
        d1: Real(
            0.004,
        ),
        d2: Real(
            0.006,
        ),
    },
)"#,
        ModifyKind::Shell => r#"Ok(
    Shell {
        target: GeometryHandleId(
            50,
        ),
        thickness: Real(
            0.002,
        ),
        faces_to_remove: [],
        open_face_handles: [],
    },
)"#,
        ModifyKind::Draft => r#"Ok(
    Draft {
        target: GeometryHandleId(
            50,
        ),
        faces: [],
        angle: Real(
            0.1,
        ),
        plane: GeometryHandleId(
            50,
        ),
    },
)"#,
        ModifyKind::Thicken => r#"Ok(
    Thicken {
        target: GeometryHandleId(
            50,
        ),
        offset: Real(
            0.003,
        ),
    },
)"#,
        ModifyKind::ZoneSlab => r#"Ok(
    ZoneSlab {
        target: GeometryHandleId(
            50,
        ),
        width: Real(
            0.01,
        ),
    },
)"#,
        ModifyKind::OffsetSolid => r#"Ok(
    OffsetSolid {
        target: GeometryHandleId(
            50,
        ),
        distance: Real(
            0.002,
        ),
    },
)"#,
        ModifyKind::OffsetCurve => r#"Ok(
    OffsetCurve {
        target: GeometryHandleId(
            50,
        ),
        distance: Real(
            0.002,
        ),
        reference: None,
        direction: None,
    },
)"#,
    }
}

/// Golden snapshot for the 3-arg (edges-selector) form. Only the
/// `MODIFY_EDGES_VARIANTS` kinds reach this; the others are `unreachable!` (the
/// base-form coverage tripwire is `modify_golden`, which is exhaustive over 9).
fn modify_edges_golden(k: ModifyKind) -> &'static str {
    match k {
        ModifyKind::Fillet => r#"Err(
    "fillet: edge selector resolved to zero edges",
)
[diag] Error "fillet(solid, edges, radius): edge selector resolved to zero edges — refusing to silently fillet all edges""#,
        ModifyKind::Chamfer => r#"Err(
    "chamfer: edge selector resolved to zero edges",
)
[diag] Error "chamfer(solid, edges, distance): edge selector resolved to zero edges — refusing to silently chamfer all edges""#,
        ModifyKind::ChamferAsymmetric => r#"Err(
    "chamfer_asymmetric: edge selector resolved to zero edges",
)
[diag] Error "chamfer_asymmetric(solid, edges, d1, d2): edge selector resolved to zero edges — refusing to silently chamfer all edges""#,
        other => unreachable!("not an edges-selector Modify variant: {other}"),
    }
}

#[test]
fn characterize_modify_family() {
    // Real runtime cross-check: ModifyKind::VARIANT_COUNT is derived from
    // ModifyKind::ALL in reify-compiler (the compiler's source-of-truth), so
    // adding a new ModifyKind without updating ALL_MODIFY fails here at runtime.
    assert_eq!(
        ALL_MODIFY.len(),
        reify_compiler::ModifyKind::VARIANT_COUNT,
        "ALL_MODIFY is out of sync with ModifyKind::VARIANT_COUNT — update both together"
    );
    let handles = modify_step_handles();
    let mut drift: Vec<String> = ALL_MODIFY
        .iter()
        .filter_map(|&k| {
            characterize(&format!("modify:{k}"), &modify_case(k), &handles, modify_golden(k))
        })
        .collect();
    // EXTRA: the 3-arg (edges-selector) branch of Fillet/Chamfer/ChamferAsymmetric.
    for &k in &MODIFY_EDGES_VARIANTS {
        if let Some(d) = characterize(
            &format!("modify:{k}:edges"),
            &modify_case_with_edges(k),
            &handles,
            modify_edges_golden(k),
        ) {
            drift.push(d);
        }
    }
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Pattern family (5 kinds): Linear/Circular/Mirror/Linear2D/Arbitrary
// ---------------------------------------------------------------------------

/// Single step handle backing the Pattern `target = GeomRef::Step(0)`.
fn pattern_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(70)]
}

/// Every `PatternKind` variant, iterated by `characterize_pattern_family`.
/// The exhaustive matches in `pattern_case`/`pattern_golden` are the sole
/// compile-time tripwire. The `assert_eq!(len(), 5)` is tautological for
/// `[PatternKind; 5]`; no `VARIANT_COUNT` cross-check exists for `PatternKind`.
const ALL_PATTERN: [PatternKind; 5] = [
    PatternKind::Linear,
    PatternKind::Circular,
    PatternKind::Mirror,
    PatternKind::Linear2D,
    PatternKind::Arbitrary,
];

/// The Pattern kinds with a distinct scalar-form vs Value-form code path. The
/// base `pattern_case` exercises the scalar (back-compat) form; the `:value`
/// extra cases below exercise the `axis`/`plane` Value-form decode branch.
const PATTERN_VALUE_VARIANTS: [PatternKind; 2] = [PatternKind::Circular, PatternKind::Mirror];

/// Build a representative base `Pattern` op for `k` (the scalar/back-compat form
/// for Circular/Mirror). EXHAUSTIVE match (no `_`); see `geometry_ops.rs` Pattern
/// arm. Circular's bare numeric `angle` exercises the degrees→radians warning.
fn pattern_case(k: PatternKind) -> CompiledGeometryOp {
    let args = match k {
        PatternKind::Linear => vec![
            ("dx".to_string(), lit(1.0)),
            ("dy".to_string(), lit(0.0)),
            ("dz".to_string(), lit(0.0)),
            ("count".to_string(), lit(3.0)),
            ("spacing".to_string(), lit(0.01)),
        ],
        PatternKind::Circular => vec![
            ("ox".to_string(), lit(0.0)),
            ("oy".to_string(), lit(0.0)),
            ("oz".to_string(), lit(0.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("count".to_string(), lit(4.0)),
            ("angle".to_string(), lit(90.0)),
        ],
        PatternKind::Mirror => vec![
            ("ox".to_string(), lit(0.0)),
            ("oy".to_string(), lit(0.0)),
            ("oz".to_string(), lit(0.0)),
            ("nx".to_string(), lit(0.0)),
            ("ny".to_string(), lit(0.0)),
            ("nz".to_string(), lit(1.0)),
        ],
        PatternKind::Linear2D => vec![
            ("dx1".to_string(), lit(1.0)),
            ("dy1".to_string(), lit(0.0)),
            ("dz1".to_string(), lit(0.0)),
            ("count1".to_string(), lit(2.0)),
            ("spacing1".to_string(), lit(0.01)),
            ("dx2".to_string(), lit(0.0)),
            ("dy2".to_string(), lit(1.0)),
            ("dz2".to_string(), lit(0.0)),
            ("count2".to_string(), lit(3.0)),
            ("spacing2".to_string(), lit(0.02)),
        ],
        PatternKind::Arbitrary => vec![
            ("t0_dx".to_string(), lit(0.01)),
            ("t0_dy".to_string(), lit(0.02)),
            ("t0_dz".to_string(), lit(0.03)),
        ],
    };
    CompiledGeometryOp::Pattern {
        kind: k,
        target: GeomRef::Step(0),
        args,
    }
}

/// Build the Value-form for a `PATTERN_VALUE_VARIANTS` kind: Circular with an
/// `axis` Value::Axis, Mirror with a `plane` Value::Plane (each with a non-unit
/// direction/normal to exercise the production normalization).
fn pattern_case_value(k: PatternKind) -> CompiledGeometryOp {
    let args = match k {
        PatternKind::Circular => vec![
            ("axis".to_string(), lit_raw(axis_value([0.01, 0.02, 0.03], [0.0, 0.0, 2.0]))),
            ("count".to_string(), lit(4.0)),
            ("angle".to_string(), lit(90.0)),
        ],
        PatternKind::Mirror => vec![(
            "plane".to_string(),
            lit_raw(plane_value([0.01, 0.02, 0.03], [0.0, 0.0, 2.0])),
        )],
        other => unreachable!("not a value-form Pattern variant: {other}"),
    };
    CompiledGeometryOp::Pattern {
        kind: k,
        target: GeomRef::Step(0),
        args,
    }
}

/// Golden snapshot per `PatternKind` (base / scalar form). EXHAUSTIVE match (no
/// `_`). Placeholders replaced during the GREEN bootstrap.
fn pattern_golden(k: PatternKind) -> &'static str {
    match k {
        PatternKind::Linear => r#"Ok(
    LinearPattern {
        target: GeometryHandleId(
            70,
        ),
        direction: [
            1.0,
            0.0,
            0.0,
        ],
        count: 3,
        spacing: Real(
            0.01,
        ),
    },
)"#,
        PatternKind::Circular => include_str!("golden/pattern_circular_base.txt"),
        PatternKind::Mirror => r#"Ok(
    Mirror {
        target: GeometryHandleId(
            70,
        ),
        plane_origin: [
            0.0,
            0.0,
            0.0,
        ],
        plane_normal: [
            0.0,
            0.0,
            1.0,
        ],
    },
)"#,
        PatternKind::Linear2D => r#"Ok(
    LinearPattern2D {
        target: GeometryHandleId(
            70,
        ),
        direction1: [
            1.0,
            0.0,
            0.0,
        ],
        count1: 2,
        spacing1: Real(
            0.01,
        ),
        direction2: [
            0.0,
            1.0,
            0.0,
        ],
        count2: 3,
        spacing2: Real(
            0.02,
        ),
    },
)"#,
        PatternKind::Arbitrary => r#"Ok(
    ArbitraryPattern {
        target: GeometryHandleId(
            70,
        ),
        transforms: [
            [
                0.01,
                0.02,
                0.03,
            ],
        ],
    },
)"#,
    }
}

/// Golden snapshot for the Value-form. Only `PATTERN_VALUE_VARIANTS` reach this.
fn pattern_value_golden(k: PatternKind) -> &'static str {
    match k {
        PatternKind::Circular => include_str!("golden/pattern_circular_value.txt"),
        PatternKind::Mirror => r#"Ok(
    Mirror {
        target: GeometryHandleId(
            70,
        ),
        plane_origin: [
            0.01,
            0.02,
            0.03,
        ],
        plane_normal: [
            0.0,
            0.0,
            1.0,
        ],
    },
)"#,
        other => unreachable!("not a value-form Pattern variant: {other}"),
    }
}

#[test]
fn characterize_pattern_family() {
    // Tautological for [PatternKind; 5] — see ALL_PATTERN doc for rationale.
    assert_eq!(ALL_PATTERN.len(), 5, "ALL_PATTERN size and annotation mismatch");
    let handles = pattern_step_handles();
    let mut drift: Vec<String> = ALL_PATTERN
        .iter()
        .filter_map(|&k| {
            characterize(&format!("pattern:{k}"), &pattern_case(k), &handles, pattern_golden(k))
        })
        .collect();
    // EXTRA: the Value-form (axis/plane) sub-branch of Circular/Mirror.
    for &k in &PATTERN_VALUE_VARIANTS {
        if let Some(d) = characterize(
            &format!("pattern:{k}:value"),
            &pattern_case_value(k),
            &handles,
            pattern_value_golden(k),
        ) {
            drift.push(d);
        }
    }
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Sweep family (8 kinds): Loft/Extrude/Revolve/Sweep/ExtrudeSymmetric/
// SweepGuided/LoftGuided/Pipe
// ---------------------------------------------------------------------------

/// Step handles backing the Sweep profile/path/guide `GeomRef::Step(0..3)`.
fn sweep_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(60), GeometryHandleId(61), GeometryHandleId(62)]
}

/// Every `SweepKind` variant, iterated by `characterize_sweep_family`.
/// The exhaustive matches in `sweep_case`/`sweep_golden` are the sole
/// compile-time tripwire. The `assert_eq!(len(), 8)` is tautological for
/// `[SweepKind; 8]`; no `VARIANT_COUNT` cross-check exists for `SweepKind`.
const ALL_SWEEP: [SweepKind; 8] = [
    SweepKind::Loft,
    SweepKind::Extrude,
    SweepKind::Revolve,
    SweepKind::Sweep,
    SweepKind::ExtrudeSymmetric,
    SweepKind::SweepGuided,
    SweepKind::LoftGuided,
    SweepKind::Pipe,
];

/// Build a representative `Sweep` op for `k`, supplying the profile/path/guide
/// `GeomRef`s (resolvable via `sweep_step_handles`) and each arm's args.
/// EXHAUSTIVE match (no `_`); see `geometry_ops.rs` Sweep arm. Distances/angles
/// clear the degeneracy floors so each case yields a clean Ok.
fn sweep_case(k: SweepKind) -> CompiledGeometryOp {
    let (profiles, args): (Vec<GeomRef>, Vec<(String, CompiledExpr)>) = match k {
        SweepKind::Loft => (vec![GeomRef::Step(0), GeomRef::Step(1)], vec![]),
        SweepKind::Extrude => (
            vec![GeomRef::Step(0)],
            vec![("distance".to_string(), lit(0.02))],
        ),
        SweepKind::Revolve => (
            vec![GeomRef::Step(0)],
            vec![
                ("ax".to_string(), lit(0.0)),
                ("ay".to_string(), lit(0.0)),
                ("az".to_string(), lit(1.0)),
                ("angle".to_string(), lit(1.0)),
                ("ox".to_string(), lit(0.0)),
                ("oy".to_string(), lit(0.0)),
                ("oz".to_string(), lit(0.0)),
            ],
        ),
        SweepKind::Sweep => (vec![GeomRef::Step(0), GeomRef::Step(1)], vec![]),
        SweepKind::ExtrudeSymmetric => (
            vec![GeomRef::Step(0)],
            vec![("distance".to_string(), lit(0.02))],
        ),
        SweepKind::SweepGuided => (
            vec![GeomRef::Step(0), GeomRef::Step(1), GeomRef::Step(2)],
            vec![],
        ),
        SweepKind::LoftGuided => (
            vec![GeomRef::Step(0), GeomRef::Step(1), GeomRef::Step(2)],
            vec![],
        ),
        SweepKind::Pipe => (
            vec![GeomRef::Step(0)],
            vec![("radius".to_string(), lit(0.005))],
        ),
    };
    CompiledGeometryOp::Sweep {
        kind: k,
        profiles,
        args,
    }
}

/// Golden snapshot per `SweepKind`. EXHAUSTIVE match (no `_`). Placeholders
/// replaced during the GREEN bootstrap.
fn sweep_golden(k: SweepKind) -> &'static str {
    match k {
        SweepKind::Loft => r#"Ok(
    Loft {
        profiles: [
            GeometryHandleId(
                60,
            ),
            GeometryHandleId(
                61,
            ),
        ],
    },
)"#,
        SweepKind::Extrude => r#"Ok(
    Extrude {
        profile: GeometryHandleId(
            60,
        ),
        distance: Real(
            0.02,
        ),
    },
)"#,
        SweepKind::Revolve => r#"Ok(
    Revolve {
        profile: GeometryHandleId(
            60,
        ),
        axis_origin: [
            0.0,
            0.0,
            0.0,
        ],
        axis_dir: [
            0.0,
            0.0,
            1.0,
        ],
        angle_rad: 1.0,
    },
)"#,
        SweepKind::Sweep => r#"Ok(
    Sweep {
        profile: GeometryHandleId(
            60,
        ),
        path: GeometryHandleId(
            61,
        ),
    },
)"#,
        SweepKind::ExtrudeSymmetric => r#"Ok(
    ExtrudeSymmetric {
        profile: GeometryHandleId(
            60,
        ),
        distance: Real(
            0.02,
        ),
    },
)"#,
        SweepKind::SweepGuided => r#"Ok(
    SweepGuided {
        profile: GeometryHandleId(
            60,
        ),
        path: GeometryHandleId(
            61,
        ),
        guide: GeometryHandleId(
            62,
        ),
    },
)"#,
        SweepKind::LoftGuided => r#"Ok(
    LoftGuided {
        profiles: [
            GeometryHandleId(
                60,
            ),
            GeometryHandleId(
                61,
            ),
        ],
        guides: [
            GeometryHandleId(
                62,
            ),
        ],
    },
)"#,
        SweepKind::Pipe => r#"Ok(
    Pipe {
        path: GeometryHandleId(
            60,
        ),
        radius: Real(
            0.005,
        ),
    },
)"#,
    }
}

#[test]
fn characterize_sweep_family() {
    // Tautological for [SweepKind; 8] — see ALL_SWEEP doc for rationale.
    assert_eq!(ALL_SWEEP.len(), 8, "ALL_SWEEP size and annotation mismatch");
    let handles = sweep_step_handles();
    let drift: Vec<String> = ALL_SWEEP
        .iter()
        .filter_map(|&k| {
            characterize(&format!("sweep:{k}"), &sweep_case(k), &handles, sweep_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Curve family (6 kinds): LineSegment/Arc/Helix/InterpCurve/BezierCurve/NurbsCurve
// ---------------------------------------------------------------------------

/// Every `CurveKind` variant, iterated by `characterize_curve_family`.
/// The exhaustive matches in `curve_case`/`curve_golden` are the sole
/// compile-time tripwire. The `assert_eq!(len(), 6)` is tautological for
/// `[CurveKind; 6]`; no `VARIANT_COUNT` cross-check exists for `CurveKind`.
const ALL_CURVE: [CurveKind; 6] = [
    CurveKind::LineSegment,
    CurveKind::Arc,
    CurveKind::Helix,
    CurveKind::InterpCurve,
    CurveKind::BezierCurve,
    CurveKind::NurbsCurve,
];

/// Build a representative `Curve` op for `k` (no target / no step handles).
/// EXHAUSTIVE match (no `_`); see `geometry_ops.rs` Curve arm. The Interp/Bezier
/// coords are flat triples; NurbsCurve uses the positional
/// `degree, n_points, poles…, weights…, knots…` layout (a minimal valid
/// degree-1 / 2-point curve).
fn curve_case(k: CurveKind) -> CompiledGeometryOp {
    let args = match k {
        CurveKind::LineSegment => vec![
            ("x1".to_string(), lit(0.0)),
            ("y1".to_string(), lit(0.0)),
            ("z1".to_string(), lit(0.0)),
            ("x2".to_string(), lit(0.01)),
            ("y2".to_string(), lit(0.02)),
            ("z2".to_string(), lit(0.03)),
        ],
        CurveKind::Arc => vec![
            ("cx".to_string(), lit(0.0)),
            ("cy".to_string(), lit(0.0)),
            ("cz".to_string(), lit(0.0)),
            ("radius".to_string(), lit(0.01)),
            ("start_angle".to_string(), lit(0.0)),
            ("end_angle".to_string(), lit(1.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
        ],
        CurveKind::Helix => vec![
            ("radius".to_string(), lit(0.01)),
            ("pitch".to_string(), lit(0.005)),
            ("height".to_string(), lit(0.05)),
        ],
        // 2 points → 6 coords.
        CurveKind::InterpCurve => coord_args(&[0.0, 0.0, 0.0, 0.01, 0.02, 0.03]),
        // 3 control points → 9 coords.
        CurveKind::BezierCurve => coord_args(&[0.0, 0.0, 0.0, 0.01, 0.01, 0.0, 0.02, 0.0, 0.0]),
        // degree=1, n_points=2, poles(2×3), weights(2), knots(n+deg+1=4).
        CurveKind::NurbsCurve => coord_args(&[
            1.0, 2.0, // degree, n_points
            0.0, 0.0, 0.0, 0.01, 0.0, 0.0, // poles
            1.0, 1.0, // weights
            0.0, 0.0, 1.0, 1.0, // knots
        ]),
    };
    CompiledGeometryOp::Curve { kind: k, args }
}

/// Golden snapshot per `CurveKind`. EXHAUSTIVE match (no `_`). Placeholders
/// replaced during the GREEN bootstrap.
fn curve_golden(k: CurveKind) -> &'static str {
    match k {
        CurveKind::LineSegment => r#"Ok(
    LineSegment {
        x1: 0.0,
        y1: 0.0,
        z1: 0.0,
        x2: 0.01,
        y2: 0.02,
        z2: 0.03,
    },
)"#,
        CurveKind::Arc => r#"Ok(
    Arc {
        center: [
            0.0,
            0.0,
            0.0,
        ],
        radius: 0.01,
        start_angle: 0.0,
        end_angle: 1.0,
        axis: [
            0.0,
            0.0,
            1.0,
        ],
    },
)"#,
        CurveKind::Helix => r#"Ok(
    Helix {
        radius: 0.01,
        pitch: 0.005,
        height: 0.05,
    },
)"#,
        CurveKind::InterpCurve => r#"Ok(
    InterpCurve {
        points: [
            [
                0.0,
                0.0,
                0.0,
            ],
            [
                0.01,
                0.02,
                0.03,
            ],
        ],
    },
)"#,
        CurveKind::BezierCurve => r#"Ok(
    BezierCurve {
        control_points: [
            [
                0.0,
                0.0,
                0.0,
            ],
            [
                0.01,
                0.01,
                0.0,
            ],
            [
                0.02,
                0.0,
                0.0,
            ],
        ],
    },
)"#,
        CurveKind::NurbsCurve => r#"Ok(
    NurbsCurve {
        control_points: [
            [
                0.0,
                0.0,
                0.0,
            ],
            [
                0.01,
                0.0,
                0.0,
            ],
        ],
        weights: [
            1.0,
            1.0,
        ],
        knots: [
            0.0,
            0.0,
            1.0,
            1.0,
        ],
        degree: 1,
    },
)"#,
    }
}

#[test]
fn characterize_curve_family() {
    // Tautological for [CurveKind; 6] — see ALL_CURVE doc for rationale.
    assert_eq!(ALL_CURVE.len(), 6, "ALL_CURVE size and annotation mismatch");
    let drift: Vec<String> = ALL_CURVE
        .iter()
        .filter_map(|&k| {
            characterize(&format!("curve:{k}"), &curve_case(k), &[], curve_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Profile family (4 kinds): Rectangle/Circle/Polygon/Ellipse
// ---------------------------------------------------------------------------

/// Every `ProfileKind` variant, iterated by `characterize_profile_family`.
/// The exhaustive matches in `profile_case`/`profile_golden` are the sole
/// compile-time tripwire. The `assert_eq!(len(), 4)` is tautological for
/// `[ProfileKind; 4]`; no `VARIANT_COUNT` cross-check exists for `ProfileKind`.
const ALL_PROFILE: [ProfileKind; 4] = [
    ProfileKind::Rectangle,
    ProfileKind::Circle,
    ProfileKind::Polygon,
    ProfileKind::Ellipse,
];

/// Build a representative `Profile` op for `k` (no target / no step handles).
/// EXHAUSTIVE match (no `_`); see `geometry_ops.rs` Profile arm. Rectangle/
/// Circle/Ellipse take named `Value` args; Polygon takes flat coordinate pairs.
fn profile_case(k: ProfileKind) -> CompiledGeometryOp {
    let args = match k {
        ProfileKind::Rectangle => vec![
            ("width".to_string(), lit(0.02)),
            ("height".to_string(), lit(0.03)),
        ],
        ProfileKind::Circle => vec![("radius".to_string(), lit(0.01))],
        // 3 points → 6 coords (chunks of 2).
        ProfileKind::Polygon => coord_args(&[0.0, 0.0, 0.01, 0.0, 0.005, 0.01]),
        ProfileKind::Ellipse => vec![
            ("semi_major".to_string(), lit(0.02)),
            ("semi_minor".to_string(), lit(0.01)),
        ],
    };
    CompiledGeometryOp::Profile { kind: k, args }
}

/// Golden snapshot per `ProfileKind`. EXHAUSTIVE match (no `_`). Placeholders
/// replaced during the GREEN bootstrap.
fn profile_golden(k: ProfileKind) -> &'static str {
    match k {
        ProfileKind::Rectangle => r#"Ok(
    RectangleProfile {
        width: Real(
            0.02,
        ),
        height: Real(
            0.03,
        ),
    },
)"#,
        ProfileKind::Circle => r#"Ok(
    CircleProfile {
        radius: Real(
            0.01,
        ),
    },
)"#,
        ProfileKind::Polygon => r#"Ok(
    PolygonProfile {
        points: [
            [
                0.0,
                0.0,
            ],
            [
                0.01,
                0.0,
            ],
            [
                0.005,
                0.01,
            ],
        ],
    },
)"#,
        ProfileKind::Ellipse => r#"Ok(
    EllipseProfile {
        semi_major: Real(
            0.02,
        ),
        semi_minor: Real(
            0.01,
        ),
    },
)"#,
    }
}

#[test]
fn characterize_profile_family() {
    // Tautological for [ProfileKind; 4] — see ALL_PROFILE doc for rationale.
    assert_eq!(ALL_PROFILE.len(), 4, "ALL_PROFILE size and annotation mismatch");
    let drift: Vec<String> = ALL_PROFILE
        .iter()
        .filter_map(|&k| {
            characterize(&format!("profile:{k}"), &profile_case(k), &[], profile_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Surface family (1 kind): NurbsSurface
// ---------------------------------------------------------------------------

/// All `SurfaceKind` variants iterated by `characterize_surface_family`.
///
/// NOTE: The eval lowering for Surface is a transient stub (task #4191 step-6).
/// The golden below reflects the stub error; it will be updated in step-10
/// when the real nested-grid decode is implemented.
const ALL_SURFACE: [SurfaceKind; 1] = [SurfaceKind::Nurbs];

/// Build a representative `Surface` op for `k` (no kernel step needed).
/// EXHAUSTIVE match (no `_`); mirrors production arg shape for each kind.
fn surface_case(k: SurfaceKind) -> CompiledGeometryOp {
    let args = match k {
        SurfaceKind::Nurbs => {
            // Minimal 2×2 bilinear patch (degree 1×1, clamped knots).
            let pt = |x, y, z| {
                Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
            };
            vec![
                (
                    "control_points".to_string(),
                    lit_raw(Value::List(vec![
                        Value::List(vec![pt(0.0, 0.0, 0.0), pt(0.0, 0.01, 0.0)]),
                        Value::List(vec![pt(0.01, 0.0, 0.0), pt(0.01, 0.01, 0.005)]),
                    ])),
                ),
                (
                    "weights".to_string(),
                    lit_raw(Value::List(vec![
                        Value::List(vec![Value::Real(1.0), Value::Real(1.0)]),
                        Value::List(vec![Value::Real(1.0), Value::Real(1.0)]),
                    ])),
                ),
                (
                    "u_knots".to_string(),
                    lit_raw(Value::List(vec![
                        Value::Real(0.0),
                        Value::Real(0.0),
                        Value::Real(1.0),
                        Value::Real(1.0),
                    ])),
                ),
                (
                    "v_knots".to_string(),
                    lit_raw(Value::List(vec![
                        Value::Real(0.0),
                        Value::Real(0.0),
                        Value::Real(1.0),
                        Value::Real(1.0),
                    ])),
                ),
                ("u_degree".to_string(), lit_raw(Value::Int(1))),
                ("v_degree".to_string(), lit_raw(Value::Int(1))),
            ]
        }
    };
    CompiledGeometryOp::Surface { kind: k, args }
}

/// Golden snapshot per `SurfaceKind`. EXHAUSTIVE match (no `_`).
///
/// NOTE: The golden reflects the transient stub eval lowering (step-6 of task
/// #4191). It will be updated in step-10 when the real decode is wired.
fn surface_golden(k: SurfaceKind) -> &'static str {
    match k {
        SurfaceKind::Nurbs => r#"Err(
    "nurbs_surface eval lowering not yet implemented",
)"#,
    }
}

#[test]
fn characterize_surface_family() {
    // Tautological for [SurfaceKind; 1] — see ALL_SURFACE doc for rationale.
    assert_eq!(ALL_SURFACE.len(), 1, "ALL_SURFACE size and annotation mismatch");
    let drift: Vec<String> = ALL_SURFACE
        .iter()
        .filter_map(|&k| {
            characterize(&format!("surface:{k}"), &surface_case(k), &[], surface_golden(k))
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Coverage (the G2 user-observable signal)
// ---------------------------------------------------------------------------

/// Compile-time exhaustiveness guard over the 9 `CompiledGeometryOp` VARIANT
/// FAMILIES. This `match` has **no `_` arm**, so adding a 10th variant to
/// `reify_compiler::CompiledGeometryOp` is a COMPILE error (E0004) here until a
/// characterization family is wired up for it. This is the variant-level half of
/// the G2 coverage signal; the per-kind half is each family's `*_case`/`*_golden`
/// exhaustive match (a new nested kind is likewise a compile error). The function
/// is never called — its body is the assertion, enforced at type-check time.
#[allow(dead_code)]
fn _assert_variant_families_exhaustive(op: &CompiledGeometryOp) {
    match op {
        CompiledGeometryOp::Primitive { .. } => {}
        CompiledGeometryOp::Boolean { .. } => {}
        CompiledGeometryOp::Modify { .. } => {}
        CompiledGeometryOp::Transform { .. } => {}
        CompiledGeometryOp::Pattern { .. } => {}
        CompiledGeometryOp::Sweep { .. } => {}
        CompiledGeometryOp::Curve { .. } => {}
        CompiledGeometryOp::Profile { .. } => {}
        CompiledGeometryOp::Surface { .. } => {}
    }
}

/// Runtime census cross-check for the 8-family / 47-nested-kind oracle.
///
/// # Coverage protection model (be precise — this matters for L5)
///
/// **Primary tripwire (compile-time):** each `*_case`/`*_golden` exhaustive
/// `match` (no `_` arm) — adding a new nested kind is a compile error until the
/// match arm and a golden exist. This is the real coverage enforcer for all
/// families.
///
/// **Secondary tripwire (runtime, Modify only):** `ALL_MODIFY.len()` is
/// cross-checked against `reify_compiler::ModifyKind::VARIANT_COUNT`, so a new
/// `ModifyKind` added to the compiler that is also reflected in `ModifyKind::ALL`
/// (and therefore increments `VARIANT_COUNT`) fails this test even if the array
/// here hasn't been updated yet.
///
/// **No secondary tripwire (the other 8 families):** `ALL_PRIMITIVE`, `ALL_BOOLEAN`,
/// `ALL_TRANSFORM`, `ALL_PATTERN`, `ALL_SWEEP`, `ALL_CURVE`, `ALL_PROFILE`, and
/// `ALL_SURFACE` are statically-typed `[Kind; N]` arrays; their `len()` assertions
/// below are **tautological** (`.len()` equals the static `N`). A developer who
/// patches the exhaustive match arms but forgets to add the new variant to `ALL_*`
/// will not be caught by these assertions — the new variant's golden will simply
/// never be exercised. When `VARIANT_COUNT` equivalents become available for these
/// enums in `reify-compiler`, add the same cross-check as Modify here. Census:
/// 7 + 3 + 9 + 5 + 5 + 8 + 6 + 4 + 1 = 48.
#[test]
fn coverage_all_variant_families_and_nested_kinds() {
    // Per-family array widths. For Primitive/Boolean/Transform/Pattern/Sweep/
    // Curve/Profile/Surface these are tautological checks (the static [Kind; N]
    // type makes .len() a compile-time constant equal to N). They're kept to
    // document the expected census and catch any manual desync between the literal
    // and the type annotation; but they cannot detect a variant omitted from ALL_*.
    // Modify's separate VARIANT_COUNT assert below IS a real runtime tripwire.
    assert_eq!(ALL_PRIMITIVE.len(), 7, "ALL_PRIMITIVE census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_BOOLEAN.len(), 3, "ALL_BOOLEAN census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_MODIFY.len(), 9, "ALL_MODIFY census");
    assert_eq!(ALL_TRANSFORM.len(), 5, "ALL_TRANSFORM census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_PATTERN.len(), 5, "ALL_PATTERN census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_SWEEP.len(), 8, "ALL_SWEEP census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_CURVE.len(), 6, "ALL_CURVE census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_PROFILE.len(), 4, "ALL_PROFILE census (tautological — real tripwire is exhaustive match)");
    assert_eq!(ALL_SURFACE.len(), 1, "ALL_SURFACE census (tautological — real tripwire is exhaustive match)");

    // Modify: real runtime cross-check against the compiler's source-of-truth.
    assert_eq!(
        ALL_MODIFY.len(),
        reify_compiler::ModifyKind::VARIANT_COUNT,
        "ALL_MODIFY is out of sync with ModifyKind::VARIANT_COUNT — update both together"
    );

    // Exactly 9 CompiledGeometryOp variant families are represented (matches the
    // no-`_` guard in `_assert_variant_families_exhaustive`). This array's own
    // .len() == 9 is also tautological (hardcoded 9 entries), but the
    // _assert_variant_families_exhaustive match is the real compile-time guard.
    let family_widths = [
        ALL_PRIMITIVE.len(),
        ALL_BOOLEAN.len(),
        ALL_MODIFY.len(),
        ALL_TRANSFORM.len(),
        ALL_PATTERN.len(),
        ALL_SWEEP.len(),
        ALL_CURVE.len(),
        ALL_PROFILE.len(),
        ALL_SURFACE.len(),
    ];
    assert_eq!(family_widths.len(), 9, "CompiledGeometryOp variant family count");

    // Total nested-kind census across all families. Because the per-family widths
    // are tautological for statically-typed arrays (except Modify), this sum also
    // cannot independently detect a variant omitted from ALL_*; it documents the
    // expected census and catches any manual size change not reflected here.
    let total: usize = family_widths.iter().sum();
    assert_eq!(total, 48, "total nested-kind census; update if any ALL_* array is resized");
}
