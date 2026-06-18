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
//! Each family additionally carries an `ALL_*` array + a count assertion (Modify
//! cross-checks against `reify_compiler::ModifyKind::VARIANT_COUNT`).
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

use std::collections::HashMap;

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind, TransformKind,
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

/// Every `PrimitiveKind` variant. Count-asserted below; the exhaustive matches
/// in `primitive_case`/`primitive_golden` are the per-kind compile-time tripwire.
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
    assert_eq!(ALL_PRIMITIVE.len(), 7, "PrimitiveKind variant count drifted");
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

/// Every `BooleanOp` variant. Count-asserted below; the exhaustive match in
/// `boolean_golden` is the per-op compile-time tripwire.
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
    assert_eq!(ALL_BOOLEAN.len(), 3, "BooleanOp variant count drifted");
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

/// Every `TransformKind` variant. Count-asserted below; the exhaustive matches
/// in `transform_case`/`transform_golden` are the per-kind compile-time tripwire.
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
    assert_eq!(ALL_TRANSFORM.len(), 5, "TransformKind variant count drifted");
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

/// Every `ModifyKind` variant. Count-asserted below against the literal 9 AND
/// `ModifyKind::VARIANT_COUNT`; the exhaustive matches in `modify_case`/
/// `modify_golden` are the per-kind compile-time tripwire.
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
        ModifyKind::Fillet => "",
        ModifyKind::Chamfer => "",
        ModifyKind::ChamferAsymmetric => "",
        ModifyKind::Shell => "",
        ModifyKind::Draft => "",
        ModifyKind::Thicken => "",
        ModifyKind::ZoneSlab => "",
        ModifyKind::OffsetSolid => "",
        ModifyKind::OffsetCurve => "",
    }
}

/// Golden snapshot for the 3-arg (edges-selector) form. Only the
/// `MODIFY_EDGES_VARIANTS` kinds reach this; the others are `unreachable!` (the
/// base-form coverage tripwire is `modify_golden`, which is exhaustive over 9).
fn modify_edges_golden(k: ModifyKind) -> &'static str {
    match k {
        ModifyKind::Fillet => "",
        ModifyKind::Chamfer => "",
        ModifyKind::ChamferAsymmetric => "",
        other => unreachable!("not an edges-selector Modify variant: {other}"),
    }
}

#[test]
fn characterize_modify_family() {
    assert_eq!(ALL_MODIFY.len(), 9, "ModifyKind variant count drifted");
    assert_eq!(
        ALL_MODIFY.len(),
        reify_compiler::ModifyKind::VARIANT_COUNT,
        "ModifyKind::VARIANT_COUNT drifted from ALL_MODIFY"
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
