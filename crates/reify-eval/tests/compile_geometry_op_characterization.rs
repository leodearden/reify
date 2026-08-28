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
//! iterates over. Every one of the nine kind families' arrays is also
//! cross-checked at runtime against `reify_compiler::XKind::VARIANT_COUNT` — a
//! real tripwire tied to the compiler's authoritative count, since
//! `VARIANT_COUNT` is derived from `XKind::ALL` over there rather than from the
//! `[Kind; N]` annotation here. Task #5754 added the eight `VARIANT_COUNT`s that
//! were previously missing, so this used to hold for `ModifyKind` alone and now
//! holds for all nine; see `coverage_all_variant_families_and_nested_kinds` for
//! the per-family assertions, where each family's compile-time
//! `const _: () = assert!(…)` lock lives, and an honest statement of the one
//! residual gap the mechanism cannot close.
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
//! 10 `CompiledGeometryOp` variant families × 54 nested kinds, across 11 tests:
//! Primitive 8, Boolean 3, Modify 10 (+3 edges-selector branch cases), Transform
//! 7, Pattern 5 (+2 value-form branch cases), Sweep 9, Curve 6, Profile 4,
//! Surface 1, Isosurface 1 (8+3+10+7+5+9+6+4+1+1 = 54). The `coverage_*` test pins
//! the 10-family / 54-kind census; the per-family `characterize_*` tests plus
//! `_assert_variant_families_exhaustive` are the compile-time tripwires for a
//! newly-added variant or nested kind. L5 MUST keep all 11 tests byte-identical
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

/// Build a `CompiledExpr` literal from a LENGTH-dimensioned scalar (SI metres).
///
/// Mirrors the in-module `literal_length` helper at `geometry_ops.rs`. The
/// length-semantic pattern args (spacing, mirror-plane origin, circular-pattern
/// axis origin, arbitrary-pattern offsets) require a dimensioned Length after
/// tasks 5214 and 5350 — a bare `lit(..)` in those positions is now rejected at
/// eval.
fn lit_len(v: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::length(v), reify_core::Type::length())
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

/// Build a `CompiledExpr` literal wrapping a `Value::AffineMap` (dimensionless
/// row-major 3×3 `linear` + SI-metre `translation`).
///
/// Mirrors `lit_transform` so the AffineApply characterization input is
/// byte-faithful to the production `Value::AffineMap` shape (task 3963).
fn lit_affine_map(linear: [[f64; 3]; 3], translation: [f64; 3]) -> CompiledExpr {
    let v = Value::AffineMap { linear, translation };
    CompiledExpr::literal(v, reify_core::Type::affine_map(3))
}

/// Build a `CompiledExpr` literal wrapping a `Value::Vector` of 3 dimensionless
/// reals (a `vec3(..)` literal).
///
/// Mirrors the in-module `literal_vec3` / `vec3_value` helpers (used by the
/// `compile_geometry_op_scale_non_uniform_*` unit tests) so the ScaleNonUniform
/// characterization input is byte-faithful to the production reference.
fn lit_vec3(x: f64, y: f64, z: f64) -> CompiledExpr {
    let v = Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)]);
    CompiledExpr::literal(v, reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()))
}

/// Build a `CompiledExpr` literal wrapping an arbitrary `Value`. The literal's
/// declared `Type` is inert here — `reify_expr::eval_expr` returns the embedded
/// value verbatim for a `Literal` — so this is the right tool for the synthetic
/// `edges`/`faces` selector args (e.g. an empty `Value::List`).
fn lit_raw(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, reify_core::Type::dimensionless_scalar())
}

/// A `Value::Vector` of 3 dimensionless `Real` components — the shape a
/// unit-vector DIRECTION has.
///
/// Since task 5745 this is the right fixture for exactly the un-gated
/// positions of `decode_axis`/`decode_plane`: the axis DIRECTION and the plane
/// NORMAL. Their ORIGINS are LENGTH-gated and use [`point3_len`] instead — a
/// bare `Real` triple in an origin is now rejected, which is the whole point of
/// δ.
fn vec3_value(c: [f64; 3]) -> Value {
    Value::Vector(vec![Value::Real(c[0]), Value::Real(c[1]), Value::Real(c[2])])
}

/// A `Value::Point` of 3 LENGTH-dimensioned components (SI metres) — the shape
/// a dimensioned `point3(1mm, 2mm, 3mm)` origin has once `reify-stdlib` has
/// produced it, and what the `decode_axis`/`decode_plane` ORIGIN positions
/// require since task 5745.
///
/// The numeric SI values are IDENTICAL to what `vec3_value` carried before the
/// migration: the gate returns `si_value` by copy and performs no arithmetic, so
/// the captured goldens stay byte-identical across this change. Any golden churn
/// here is a defect to investigate, not to re-baseline.
fn point3_len(c: [f64; 3]) -> Value {
    Value::Point(vec![
        Value::length(c[0]),
        Value::length(c[1]),
        Value::length(c[2]),
    ])
}

/// A `Value::Axis` for the Circular pattern value-form sub-branch (decoded by
/// `decode_axis`; the direction is normalized to unit length by production).
///
/// The ORIGIN is a LENGTH `Point` and the DIRECTION a bare `Real` `Vector` — the
/// ORIGIN-vs-DIRECTION split task 5745 drew, and the shape a real
/// `axis_z(point3(10mm, 20mm, 30mm))` actually has.
fn axis_value(origin: [f64; 3], direction: [f64; 3]) -> Value {
    Value::Axis {
        origin: Box::new(point3_len(origin)),
        direction: Box::new(vec3_value(direction)),
    }
}

/// A `Value::Plane` for the Mirror pattern value-form sub-branch (decoded by
/// `decode_plane`; the normal is normalized to unit length by production).
///
/// The ORIGIN is a LENGTH `Point` and the NORMAL a bare `Real` `Vector` — the
/// ORIGIN-vs-DIRECTION split task 5745 drew, and the shape a real
/// `plane_yz(10mm)` actually has.
fn plane_value(origin: [f64; 3], normal: [f64; 3]) -> Value {
    Value::Plane {
        origin: Box::new(point3_len(origin)),
        normal: Box::new(vec3_value(normal)),
    }
}

/// Build positional LENGTH coordinate args (`c0`, `c1`, …) from a slice of SI
/// metres. The production reader iterates `args` in Vec order (names are
/// inert), so this is how a variadic builtin receives its flat coordinate
/// stream.
///
/// Every position minted here is DIMENSIONED, because this helper's only
/// wholesale user is Polygon: every `polygon` argument is a LENGTH-gated 2-D
/// vertex coordinate in the XY plane (task 5661), at every arity, with no
/// dimensionless neighbour to leave bare. The curve arms are written out
/// explicitly below instead of calling this: InterpCurve/BezierCurve are also
/// wholesale-gated but interleave nothing, while NurbsCurve gates ONLY its pole
/// span and so cannot be swapped wholesale at all.
fn coord_args(coords: &[f64]) -> Vec<(String, CompiledExpr)> {
    coords
        .iter()
        .enumerate()
        .map(|(i, &v)| (format!("c{i}"), lit_len(v)))
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
// Primitive family (8 kinds): Box/Cylinder/Sphere/Tube/Cone/Wedge/Torus/HalfSpace
// ---------------------------------------------------------------------------

/// Every `PrimitiveKind` variant, iterated by `characterize_primitive_family`.
/// The exhaustive matches in `primitive_case`/`primitive_golden` are the primary
/// compile-time tripwire: a new `PrimitiveKind` variant is a compile error until
/// both match arms and this array are updated. This array's width is also
/// cross-checked against `PrimitiveKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_PRIMITIVE` in `reify-eval/src/geometry_ops/tests.rs`.
const ALL_PRIMITIVE: [PrimitiveKind; 8] = [
    PrimitiveKind::Box,
    PrimitiveKind::Cylinder,
    PrimitiveKind::Sphere,
    PrimitiveKind::Tube,
    PrimitiveKind::Cone,
    PrimitiveKind::Wedge,
    PrimitiveKind::Torus,
    PrimitiveKind::HalfSpace,
];

/// Build a representative `Primitive` op for `k`, supplying each arm's required
/// `eval_arg(...)` named args (see `geometry_ops.rs` Primitive arm). EXHAUSTIVE
/// match (no `_`): a new `PrimitiveKind` is a compile error until a case exists.
fn primitive_case(k: PrimitiveKind) -> CompiledGeometryOp {
    let args = match k {
        PrimitiveKind::Box => vec![
            ("width".to_string(), lit_len(0.01)),
            ("height".to_string(), lit_len(0.02)),
            ("depth".to_string(), lit_len(0.03)),
        ],
        PrimitiveKind::Cylinder => vec![
            ("radius".to_string(), lit_len(0.01)),
            ("height".to_string(), lit_len(0.02)),
        ],
        PrimitiveKind::Sphere => vec![("radius".to_string(), lit_len(0.01))],
        PrimitiveKind::Tube => vec![
            ("outer_r".to_string(), lit_len(0.02)),
            ("inner_r".to_string(), lit_len(0.01)),
            ("height".to_string(), lit_len(0.03)),
        ],
        PrimitiveKind::Cone => vec![
            ("bottom_radius".to_string(), lit_len(0.02)),
            ("top_radius".to_string(), lit_len(0.01)),
            ("height".to_string(), lit_len(0.03)),
        ],
        PrimitiveKind::Wedge => vec![
            ("width".to_string(), lit_len(0.02)),
            ("depth".to_string(), lit_len(0.03)),
            ("height".to_string(), lit_len(0.04)),
            ("top_width".to_string(), lit_len(0.01)),
        ],
        PrimitiveKind::Torus => vec![
            ("major_radius".to_string(), lit_len(0.03)),
            ("minor_radius".to_string(), lit_len(0.01)),
        ],
        // MIXED, and deliberately so: `(px, py, pz)` is a point on the boundary
        // plane and is gated, but `(nx, ny, nz)` is a DIMENSIONLESS unit normal
        // and stays a bare `lit(..)`. Swapping the normal to `lit_len` here
        // would hide an over-broad gate rather than characterize the real one —
        // `examples/half_space.ri` writes exactly this shape.
        PrimitiveKind::HalfSpace => vec![
            ("px".to_string(), lit_len(0.0)),
            ("py".to_string(), lit_len(0.0)),
            ("pz".to_string(), lit_len(0.0)),
            ("nx".to_string(), lit(0.0)),
            ("ny".to_string(), lit(0.0)),
            ("nz".to_string(), lit(1.0)),
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
        width: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        depth: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Cylinder => r#"Ok(
    Cylinder {
        radius: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Sphere => r#"Ok(
    Sphere {
        radius: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Tube => r#"Ok(
    Tube {
        outer_r: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        inner_r: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Cone => r#"Ok(
    Cone {
        bottom_radius: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        top_radius: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Wedge => r#"Ok(
    Wedge {
        width: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        depth: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.04,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        top_width: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::Torus => r#"Ok(
    Torus {
        major_radius: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        minor_radius: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        PrimitiveKind::HalfSpace => r#"Ok(
    HalfSpace {
        px: Scalar {
            si_value: 0.0,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        py: Scalar {
            si_value: 0.0,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        pz: Scalar {
            si_value: 0.0,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        nx: Real(
            0.0,
        ),
        ny: Real(
            0.0,
        ),
        nz: Real(
            1.0,
        ),
    },
)"#,
    }
}

#[test]
fn characterize_primitive_family() {
    // Tautological for [PrimitiveKind; 8] — fires only if the static-array type
    // annotation and this literal are manually out of sync. Real coverage
    // enforcement is the no-`_` match in primitive_case / primitive_golden.
    assert_eq!(ALL_PRIMITIVE.len(), 8, "ALL_PRIMITIVE size and annotation mismatch");
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
/// The exhaustive match in `boolean_golden` is the primary compile-time
/// tripwire. This array's width is also cross-checked against
/// `BooleanOp::VARIANT_COUNT` in `coverage_all_variant_families_and_nested_kinds`
/// and locked at compile time immediately below.
const ALL_BOOLEAN: [BooleanOp; 3] =
    [BooleanOp::Union, BooleanOp::Difference, BooleanOp::Intersection];

/// Compile-time registry lock for the Boolean family (task #5754).
///
/// `BooleanOp` has no production `*_COMPILERS` fn-table (booleans dispatch by
/// inline match), so `ALL_BOOLEAN` here is the whole registry surface for this
/// family — which is why its lock lives in this file rather than beside the
/// other seven in `geometry_ops/tests.rs`.
const _: () = assert!(
    ALL_BOOLEAN.len() == BooleanOp::VARIANT_COUNT,
    "ALL_BOOLEAN / BooleanOp::VARIANT_COUNT mismatch — a variant was added without \
     registering it; extend ALL_BOOLEAN and BooleanOp::ALL together"
);

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
// Transform family (7 kinds): Translate/Rotate/Scale/RotateAround/ApplyTransform/
// AffineApply/ScaleNonUniform
// ---------------------------------------------------------------------------

/// Single step handle backing the Transform `target = GeomRef::Step(0)`.
fn transform_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(42)]
}

/// Every `TransformKind` variant, iterated by `characterize_transform_family`.
/// The exhaustive matches in `transform_case`/`transform_golden` are the sole
/// primary compile-time tripwire. This array's width is also cross-checked
/// against `TransformKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_TRANSFORM` in `reify-eval/src/geometry_ops/tests.rs`.
const ALL_TRANSFORM: [TransformKind; 7] = [
    TransformKind::Translate,
    TransformKind::Rotate,
    TransformKind::Scale,
    TransformKind::RotateAround,
    TransformKind::ApplyTransform,
    TransformKind::AffineApply,
    TransformKind::ScaleNonUniform,
];

/// Build a representative `Transform` op for `k`, supplying each arm's required
/// args (see `geometry_ops.rs` Transform arm). EXHAUSTIVE match (no `_`). Args
/// mirror the in-module `compile_geometry_op_{scale,rotate_around,apply_transform}`
/// unit tests; ApplyTransform uses an identity-rotation `lit_transform`.
fn transform_case(k: TransformKind) -> CompiledGeometryOp {
    let args = match k {
        // Translation components are LENGTH-semantic (task 5623) — `lit_len`.
        // The golden below is unchanged: `Value::length(0.01).as_f64()` and
        // `Value::Real(0.01).as_f64()` are both `0.01`.
        TransformKind::Translate => vec![
            ("dx".to_string(), lit_len(0.01)),
            ("dy".to_string(), lit_len(0.02)),
            ("dz".to_string(), lit_len(0.03)),
        ],
        TransformKind::Rotate => vec![
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("angle".to_string(), lit(1.0)),
        ],
        TransformKind::Scale => vec![("factor".to_string(), lit(2.0))],
        // Only the PIVOT is LENGTH-semantic (task 5623); ax/ay/az/angle stay
        // on `lit`. Golden unchanged.
        TransformKind::RotateAround => vec![
            ("px".to_string(), lit_len(0.05)),
            ("py".to_string(), lit_len(0.0)),
            ("pz".to_string(), lit_len(0.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("angle".to_string(), lit(1.0)),
        ],
        TransformKind::ApplyTransform => vec![(
            "transform".to_string(),
            lit_transform([1.0, 0.0, 0.0, 0.0], [0.01, 0.02, 0.03]),
        )],
        TransformKind::AffineApply => vec![(
            "map".to_string(),
            lit_affine_map(
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
                [0.01, 0.02, 0.03],
            ),
        )],
        TransformKind::ScaleNonUniform => {
            vec![("factors".to_string(), lit_vec3(2.0, 1.0, 0.5))]
        }
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
        TransformKind::AffineApply => r#"Ok(
    AffineApply {
        target: GeometryHandleId(
            42,
        ),
        linear: [
            [
                1.0,
                0.0,
                0.0,
            ],
            [
                0.0,
                1.0,
                0.0,
            ],
            [
                0.0,
                0.0,
                2.0,
            ],
        ],
        translation: [
            0.01,
            0.02,
            0.03,
        ],
    },
)"#,
        TransformKind::ScaleNonUniform => r#"Ok(
    ScaleNonUniform {
        target: GeometryHandleId(
            42,
        ),
        sx: 2.0,
        sy: 1.0,
        sz: 0.5,
    },
)"#,
    }
}

#[test]
fn characterize_transform_family() {
    // Tautological for [TransformKind; 7] — see ALL_TRANSFORM doc for rationale.
    assert_eq!(ALL_TRANSFORM.len(), 7, "ALL_TRANSFORM size and annotation mismatch");
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
const ALL_MODIFY: [ModifyKind; 10] = [
    ModifyKind::Fillet,
    ModifyKind::Chamfer,
    ModifyKind::ChamferAsymmetric,
    ModifyKind::Shell,
    ModifyKind::Draft,
    ModifyKind::Thicken,
    ModifyKind::ZoneSlab,
    ModifyKind::OffsetSolid,
    ModifyKind::OffsetSurface,
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
        ModifyKind::Fillet => vec![("radius".to_string(), lit_len(0.005))],
        ModifyKind::Chamfer => vec![("distance".to_string(), lit_len(0.005))],
        ModifyKind::ChamferAsymmetric => vec![
            ("d1".to_string(), lit_len(0.004)),
            ("d2".to_string(), lit_len(0.006)),
        ],
        ModifyKind::Shell => vec![("thickness".to_string(), lit_len(0.002))],
        // `Draft`'s `angle` stays BARE: it is an ANGLE position owned by
        // `docs/prds/v0_6/angle-units-surface-convergence.md`, not by this leaf.
        ModifyKind::Draft => vec![("angle".to_string(), lit(0.1))],
        ModifyKind::Thicken => vec![("offset".to_string(), lit_len(0.003))],
        ModifyKind::ZoneSlab => vec![("width".to_string(), lit_len(0.01))],
        ModifyKind::OffsetSolid => vec![("distance".to_string(), lit_len(0.002))],
        ModifyKind::OffsetSurface => vec![("distance".to_string(), lit_len(0.002))],
        ModifyKind::OffsetCurve => vec![("distance".to_string(), lit_len(0.002))],
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
        radius: Scalar {
            si_value: 0.005,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::Chamfer => r#"Ok(
    Chamfer {
        target: GeometryHandleId(
            50,
        ),
        edges: [],
        distance: Scalar {
            si_value: 0.005,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::ChamferAsymmetric => r#"Ok(
    ChamferAsymmetric {
        target: GeometryHandleId(
            50,
        ),
        edges: [],
        d1: Scalar {
            si_value: 0.004,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        d2: Scalar {
            si_value: 0.006,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::Shell => r#"Ok(
    Shell {
        target: GeometryHandleId(
            50,
        ),
        thickness: Scalar {
            si_value: 0.002,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
        offset: Scalar {
            si_value: 0.003,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::ZoneSlab => r#"Ok(
    ZoneSlab {
        target: GeometryHandleId(
            50,
        ),
        width: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::OffsetSolid => r#"Ok(
    OffsetSolid {
        target: GeometryHandleId(
            50,
        ),
        distance: Scalar {
            si_value: 0.002,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::OffsetSurface => r#"Ok(
    OffsetSurface {
        target: GeometryHandleId(
            50,
        ),
        distance: Scalar {
            si_value: 0.002,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ModifyKind::OffsetCurve => r#"Ok(
    OffsetCurve {
        target: GeometryHandleId(
            50,
        ),
        distance: Scalar {
            si_value: 0.002,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
/// primary compile-time tripwire. This array's width is also cross-checked
/// against `PatternKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_PATTERN` in `reify-eval/src/geometry_ops/tests.rs`.
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
            // spacing is length-semantic → dimensioned Length (task 5214).
            ("spacing".to_string(), lit_len(0.01)),
        ],
        PatternKind::Circular => vec![
            // Axis ORIGIN is length-semantic → dimensioned Length (task 5350);
            // axis DIRECTION stays a bare dimensionless unit vector.
            ("ox".to_string(), lit_len(0.0)),
            ("oy".to_string(), lit_len(0.0)),
            ("oz".to_string(), lit_len(0.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
            ("count".to_string(), lit(4.0)),
            ("angle".to_string(), lit(90.0)),
        ],
        PatternKind::Mirror => vec![
            // Plane ORIGIN is length-semantic → dimensioned Length (task 5214);
            // plane NORMAL stays a bare dimensionless unit vector.
            ("ox".to_string(), lit_len(0.0)),
            ("oy".to_string(), lit_len(0.0)),
            ("oz".to_string(), lit_len(0.0)),
            ("nx".to_string(), lit(0.0)),
            ("ny".to_string(), lit(0.0)),
            ("nz".to_string(), lit(1.0)),
        ],
        PatternKind::Linear2D => vec![
            ("dx1".to_string(), lit(1.0)),
            ("dy1".to_string(), lit(0.0)),
            ("dz1".to_string(), lit(0.0)),
            ("count1".to_string(), lit(2.0)),
            // spacings are length-semantic → dimensioned Length (task 5214).
            ("spacing1".to_string(), lit_len(0.01)),
            ("dx2".to_string(), lit(0.0)),
            ("dy2".to_string(), lit(1.0)),
            ("dz2".to_string(), lit(0.0)),
            ("count2".to_string(), lit(3.0)),
            ("spacing2".to_string(), lit_len(0.02)),
        ],
        PatternKind::Arbitrary => vec![
            // Offsets are translations (length-semantic) → dimensioned Length.
            ("t0_dx".to_string(), lit_len(0.01)),
            ("t0_dy".to_string(), lit_len(0.02)),
            ("t0_dz".to_string(), lit_len(0.03)),
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
        PatternKind::Linear => include_str!("golden/pattern_linear_base.txt"),
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
        PatternKind::Linear2D => include_str!("golden/pattern_linear2d_base.txt"),
        PatternKind::Arbitrary => r#"Ok(
    ArbitraryPattern {
        target: GeometryHandleId(
            70,
        ),
        transforms: [
            (
                [
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                ],
                [
                    0.01,
                    0.02,
                    0.03,
                ],
            ),
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
/// primary compile-time tripwire. This array's width is also cross-checked
/// against `SweepKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_SWEEP` in `reify-eval/src/geometry_ops/tests.rs` — where that
/// lock caught a real hole (that array had silently omitted `ExtrudeInfinite`).
const ALL_SWEEP: [SweepKind; 9] = [
    SweepKind::Loft,
    SweepKind::Extrude,
    SweepKind::Revolve,
    SweepKind::Sweep,
    SweepKind::ExtrudeSymmetric,
    SweepKind::SweepGuided,
    SweepKind::LoftGuided,
    SweepKind::Pipe,
    SweepKind::ExtrudeInfinite,
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
            vec![("distance".to_string(), lit_len(0.02))],
        ),
        SweepKind::Revolve => (
            vec![GeomRef::Step(0)],
            vec![
                ("ax".to_string(), lit(0.0)),
                ("ay".to_string(), lit(0.0)),
                ("az".to_string(), lit(1.0)),
                ("angle".to_string(), lit(1.0)),
                // Only the axis ORIGIN is LENGTH-semantic (task 5623);
                // ax/ay/az/angle stay on `lit`. Golden unchanged.
                ("ox".to_string(), lit_len(0.0)),
                ("oy".to_string(), lit_len(0.0)),
                ("oz".to_string(), lit_len(0.0)),
            ],
        ),
        SweepKind::Sweep => (vec![GeomRef::Step(0), GeomRef::Step(1)], vec![]),
        SweepKind::ExtrudeSymmetric => (
            vec![GeomRef::Step(0)],
            vec![("distance".to_string(), lit_len(0.02))],
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
            vec![("radius".to_string(), lit_len(0.005))],
        ),
        SweepKind::ExtrudeInfinite => (
            vec![GeomRef::Step(0)],
            vec![
                // `dx`/`dy`/`dz` are a dimensionless DIRECTION, not a length —
                // they stay on `lit` (task 5744 boundary). Golden unchanged.
                ("dx".to_string(), lit(0.0)),
                ("dy".to_string(), lit(0.0)),
                ("dz".to_string(), lit(1.0)),
                ("direction".to_string(), lit_raw(Value::String("positive".into()))),
            ],
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
        distance: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
        distance: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
        radius: Scalar {
            si_value: 0.005,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        SweepKind::ExtrudeInfinite => r#"Ok(
    ExtrudeInfinite {
        profile: GeometryHandleId(
            60,
        ),
        axis: [
            0.0,
            0.0,
            1.0,
        ],
        both: false,
    },
)"#,
    }
}

#[test]
fn characterize_sweep_family() {
    // Tautological for [SweepKind; 9] — see ALL_SWEEP doc for rationale.
    assert_eq!(ALL_SWEEP.len(), 9, "ALL_SWEEP size and annotation mismatch");
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
/// primary compile-time tripwire. This array's width is also cross-checked
/// against `CurveKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_CURVE` in `reify-eval/src/geometry_ops/tests.rs`.
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
        // Both endpoints are LENGTH-gated (task 5623). The golden below is
        // unchanged: `Value::length(0.01).as_f64() == Value::Real(0.01).as_f64()`.
        CurveKind::LineSegment => vec![
            ("x1".to_string(), lit_len(0.0)),
            ("y1".to_string(), lit_len(0.0)),
            ("z1".to_string(), lit_len(0.0)),
            ("x2".to_string(), lit_len(0.01)),
            ("y2".to_string(), lit_len(0.02)),
            ("z2".to_string(), lit_len(0.03)),
        ],
        // Centre and radius are LENGTH-gated (task 5623); the two angles and the
        // ax/ay/az unit vector stay deliberately bare. Golden below unchanged.
        CurveKind::Arc => vec![
            ("cx".to_string(), lit_len(0.0)),
            ("cy".to_string(), lit_len(0.0)),
            ("cz".to_string(), lit_len(0.0)),
            ("radius".to_string(), lit_len(0.01)),
            ("start_angle".to_string(), lit(0.0)),
            ("end_angle".to_string(), lit(1.0)),
            ("ax".to_string(), lit(0.0)),
            ("ay".to_string(), lit(0.0)),
            ("az".to_string(), lit(1.0)),
        ],
        // radius / pitch / height are all LENGTH-gated (task 5623); `pitch` is a
        // rise per turn, not an angle. Golden below unchanged.
        CurveKind::Helix => vec![
            ("radius".to_string(), lit_len(0.01)),
            ("pitch".to_string(), lit_len(0.005)),
            ("height".to_string(), lit_len(0.05)),
        ],
        // 2 points → 6 coords. EVERY position is a point coordinate and so is
        // LENGTH-gated (task 5658). The golden below is unchanged:
        // `Value::length(0.01).as_f64() == Value::Real(0.01).as_f64()`.
        CurveKind::InterpCurve => vec![
            ("c0".to_string(), lit_len(0.0)),
            ("c1".to_string(), lit_len(0.0)),
            ("c2".to_string(), lit_len(0.0)),
            ("c3".to_string(), lit_len(0.01)),
            ("c4".to_string(), lit_len(0.02)),
            ("c5".to_string(), lit_len(0.03)),
        ],
        // 3 control points → 9 coords. EVERY position is a control-point
        // coordinate and so is LENGTH-gated (task 5658). Golden unchanged.
        CurveKind::BezierCurve => vec![
            ("c0".to_string(), lit_len(0.0)),
            ("c1".to_string(), lit_len(0.0)),
            ("c2".to_string(), lit_len(0.0)),
            ("c3".to_string(), lit_len(0.01)),
            ("c4".to_string(), lit_len(0.01)),
            ("c5".to_string(), lit_len(0.0)),
            ("c6".to_string(), lit_len(0.02)),
            ("c7".to_string(), lit_len(0.0)),
            ("c8".to_string(), lit_len(0.0)),
        ],
        // degree=1, n_points=2, poles(2×3), weights(2), knots(n+deg+1=4).
        // ONLY the pole span is LENGTH-gated (task 5658) — `coord_args` cannot
        // be swapped wholesale here. `degree` is a polynomial degree (a count),
        // `n_points` is a count, the weights are rational blending factors and
        // the knots are parameter-space values, so all eight stay deliberately
        // BARE. Golden unchanged, same `as_f64` identity as above.
        CurveKind::NurbsCurve => vec![
            ("c0".to_string(), lit(1.0)),      // degree
            ("c1".to_string(), lit(2.0)),      // n_points
            ("c2".to_string(), lit_len(0.0)),  // pole 1 x
            ("c3".to_string(), lit_len(0.0)),  // pole 1 y
            ("c4".to_string(), lit_len(0.0)),  // pole 1 z
            ("c5".to_string(), lit_len(0.01)), // pole 2 x
            ("c6".to_string(), lit_len(0.0)),  // pole 2 y
            ("c7".to_string(), lit_len(0.0)),  // pole 2 z
            ("c8".to_string(), lit(1.0)),      // weight 1
            ("c9".to_string(), lit(1.0)),      // weight 2
            ("c10".to_string(), lit(0.0)),     // knot 1
            ("c11".to_string(), lit(0.0)),     // knot 2
            ("c12".to_string(), lit(1.0)),     // knot 3
            ("c13".to_string(), lit(1.0)),     // knot 4
        ],
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
/// primary compile-time tripwire. This array's width is also cross-checked
/// against `ProfileKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds`, and locked at compile time
/// beside `ALL_PROFILE` in `reify-eval/src/geometry_ops/tests.rs`.
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
            ("width".to_string(), lit_len(0.02)),
            ("height".to_string(), lit_len(0.03)),
        ],
        ProfileKind::Circle => vec![("radius".to_string(), lit_len(0.01))],
        // 3 points → 6 coords (chunks of 2). EVERY position is a vertex
        // coordinate in the XY plane and so is LENGTH-gated (task 5661), which
        // is why `coord_args` mints dimensioned literals. The golden below is
        // unchanged: `Value::length(0.01).as_f64() == Value::Real(0.01).as_f64()`.
        ProfileKind::Polygon => coord_args(&[0.0, 0.0, 0.01, 0.0, 0.005, 0.01]),
        ProfileKind::Ellipse => vec![
            ("semi_major".to_string(), lit_len(0.02)),
            ("semi_minor".to_string(), lit_len(0.01)),
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
        width: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        height: Scalar {
            si_value: 0.03,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
    },
)"#,
        ProfileKind::Circle => r#"Ok(
    CircleProfile {
        radius: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
        semi_major: Scalar {
            si_value: 0.02,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
        semi_minor: Scalar {
            si_value: 0.01,
            dimension: DimensionVector(
                [
                    Rational {
                        num: 1,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                    Rational {
                        num: 0,
                        den: 1,
                    },
                ],
            ),
        },
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
/// NOTE: The eval lowering for Surface is the real nested-grid decode wired in
/// task #4191 step-10; the golden below reflects the decoded `NurbsSurface` IR
/// node (not the earlier step-6 stub error).
///
/// This array's width is cross-checked against `SurfaceKind::VARIANT_COUNT` in
/// `coverage_all_variant_families_and_nested_kinds` and locked at compile time
/// immediately below.
const ALL_SURFACE: [SurfaceKind; 1] = [SurfaceKind::Nurbs];

/// Compile-time registry lock for the Surface family (task #5754).
///
/// Like `BooleanOp`, `SurfaceKind` has no production `*_COMPILERS` fn-table
/// (surfaces dispatch by inline match), so `ALL_SURFACE` here is the whole
/// registry surface for this family.
const _: () = assert!(
    ALL_SURFACE.len() == SurfaceKind::VARIANT_COUNT,
    "ALL_SURFACE / SurfaceKind::VARIANT_COUNT mismatch — a variant was added without \
     registering it; extend ALL_SURFACE and SurfaceKind::ALL together"
);

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
/// NOTE: The golden reflects the real nested-grid decode wired in step-10 of
/// task #4191 — the probe lowers the `Surface` op to a `GeometryOp::NurbsSurface`
/// IR node carrying the decoded control-point/weight grids and u/v knots+degrees.
fn surface_golden(k: SurfaceKind) -> &'static str {
    match k {
        SurfaceKind::Nurbs => r#"Ok(
    NurbsSurface {
        control_points: [
            [
                [
                    0.0,
                    0.0,
                    0.0,
                ],
                [
                    0.0,
                    0.01,
                    0.0,
                ],
            ],
            [
                [
                    0.01,
                    0.0,
                    0.0,
                ],
                [
                    0.01,
                    0.01,
                    0.005,
                ],
            ],
        ],
        weights: [
            [
                1.0,
                1.0,
            ],
            [
                1.0,
                1.0,
            ],
        ],
        u_knots: [
            0.0,
            0.0,
            1.0,
            1.0,
        ],
        v_knots: [
            0.0,
            0.0,
            1.0,
            1.0,
        ],
        u_degree: 1,
        v_degree: 1,
    },
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
// Isosurface family (1 kind): marching-cubes extraction from a Voxel grid
// ---------------------------------------------------------------------------

/// `Isosurface` (task #4999) has no nested "kind" enum — unlike `Surface`
/// (`SurfaceKind`), the variant itself is the sole representative case.
/// `ALL_ISOSURFACE` is a trivial 1-element marker array so
/// `characterize_isosurface_family` follows the same
/// `.iter().filter_map(characterize(...))` shape as every other family.
const ALL_ISOSURFACE: [(); 1] = [()];

/// Single step handle backing the Isosurface `grid = GeomRef::Step(0)` operand.
fn isosurface_step_handles() -> Vec<GeometryHandleId> {
    vec![GeometryHandleId(90)]
}

/// Build the representative `Isosurface` op for `_k`. EXHAUSTIVE match is
/// unnecessary (no nested kind), but the `grid` + `iso`/`adaptive` shape
/// mirrors the step-3 RED unit-test inputs (`crates/reify-eval/src/geometry_ops.rs`)
/// so the golden reflects the real Voxel-grid decode: a resolvable
/// `grid = GeomRef::Step(0)` operand plus named `iso: 5mm` / `adaptive: true`
/// args. `iso`/`adaptive` are wrapped via `lit_raw` (the literal's declared
/// `Type` is inert — see `lit_raw`'s doc).
fn isosurface_case(_k: ()) -> CompiledGeometryOp {
    CompiledGeometryOp::Isosurface {
        grid: GeomRef::Step(0),
        args: vec![
            ("iso".to_string(), lit_raw(Value::length(0.005))),
            ("adaptive".to_string(), lit_raw(Value::Bool(true))),
        ],
    }
}

/// Golden snapshot for the sole Isosurface case. The probe lowers
/// `CompiledGeometryOp::Isosurface` to `GeometryOp::Surface { grid, iso_level,
/// adaptive }` — `iso: 5mm` decodes to `0.005` SI metres via the same
/// Length→f64 path as every other Length-typed geometry arg; `adaptive: true`
/// decodes directly to `bool`.
fn isosurface_golden(_k: ()) -> &'static str {
    r#"Ok(
    Surface {
        grid: GeometryHandleId(
            90,
        ),
        iso_level: 0.005,
        adaptive: true,
    },
)"#
}

#[test]
fn characterize_isosurface_family() {
    // Tautological for [(); 1] — see ALL_ISOSURFACE doc for rationale.
    assert_eq!(ALL_ISOSURFACE.len(), 1, "ALL_ISOSURFACE size and annotation mismatch");
    let handles = isosurface_step_handles();
    let drift: Vec<String> = ALL_ISOSURFACE
        .iter()
        .filter_map(|&k| {
            characterize(
                &format!("isosurface:{k:?}"),
                &isosurface_case(k),
                &handles,
                isosurface_golden(k),
            )
        })
        .collect();
    assert!(drift.is_empty(), "{}", drift_report(&drift));
}

// ---------------------------------------------------------------------------
// Coverage (the G2 user-observable signal)
// ---------------------------------------------------------------------------

/// Compile-time exhaustiveness guard over the 10 `CompiledGeometryOp` VARIANT
/// FAMILIES. This `match` has **no `_` arm**, so adding an 11th variant to
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
        CompiledGeometryOp::Isosurface { .. } => {}
    }
}

/// The nine-kind-family census as documented in the doc block of
/// [`coverage_all_variant_families_and_nested_kinds`] below.
///
/// Kept as a named constant so the prose and the assertion cannot drift apart
/// silently: the test compares this against the sum of the nine
/// `VARIANT_COUNT`s, so if the doc arithmetic is wrong the test fails.
const DOCUMENTED_KIND_FAMILY_CENSUS: usize = 53;

/// Runtime census cross-check for the 10-family / 53-nested-kind oracle
/// (nine kind families totalling 52, plus the Isosurface marker family).
///
/// # Coverage protection model (be precise — this matters for L5)
///
/// **Primary tripwire (compile-time):** each `*_case`/`*_golden` exhaustive
/// `match` (no `_` arm) — adding a new nested kind is a compile error until the
/// match arm and a golden exist. This is the real coverage enforcer for all
/// families.
///
/// **Secondary tripwire (runtime, all nine kind families):** each `ALL_*.len()`
/// is cross-checked against that family's `reify_compiler::XKind::VARIANT_COUNT`,
/// so a new variant added to the compiler that is also reflected in `XKind::ALL`
/// (and therefore increments `VARIANT_COUNT`) fails this test even if the array
/// here hasn't been updated yet. Task #5754 added the eight missing
/// `VARIANT_COUNT`s, so what was previously true of `ModifyKind` alone now holds
/// for `PrimitiveKind`, `BooleanOp`, `TransformKind`, `PatternKind`, `SweepKind`,
/// `CurveKind`, `ProfileKind` and `SurfaceKind` as well. These assertions are no
/// longer tautological: `VARIANT_COUNT` is derived from the compiler's own
/// `XKind::ALL`, not from the static `[Kind; N]` annotation here.
///
/// **Where each family's compile-time lock lives:** the same `ALL_*`/
/// `VARIANT_COUNT` pairs are additionally locked with
/// `const _: () = assert!(…)`, which fires at `cargo check` before any test
/// runs. Seven of them sit beside their tables in
/// `reify-eval/src/geometry_ops/tests.rs::compile_geometry_op_registry_completeness`;
/// Boolean's and Surface's sit beside `ALL_BOOLEAN`/`ALL_SURFACE` in this file,
/// because those two families have no production `*_COMPILERS` fn-table (they
/// dispatch by inline match) and so have no table over there to attach to.
///
/// **Residual gap (stated honestly, do not over-read the guarantee):** because
/// `VARIANT_COUNT` is defined as `XKind::ALL.len()`, both sides move together —
/// a variant added to the enum but never added to `XKind::ALL` is invisible to
/// any assertion built on `VARIANT_COUNT`. Closing that needs enum-variant
/// reflection, which stable Rust lacks (`std::mem::variant_count` is
/// nightly-only). What closes it in practice is each family's exhaustive
/// `Display` match in `reify-compiler`, plus the exhaustive `*_case`/`*_golden`
/// matches here — both fail `cargo check` naming the family the instant a
/// variant is added, forcing the author through `ALL`. `VARIANT_COUNT` then
/// catches the unregistered registry row.
///
/// Census: 8 + 3 + 10 + 7 + 5 + 9 + 6 + 4 + 1 = 53 across the nine kind families
/// (54 including the `ALL_ISOSURFACE` marker family).
#[test]
fn coverage_all_variant_families_and_nested_kinds() {
    // Per-family array widths, each cross-checked against the compiler's
    // authoritative `VARIANT_COUNT` (task #5754). These are NO LONGER
    // tautological: `XKind::VARIANT_COUNT` is derived from `XKind::ALL` in
    // reify-compiler, so a variant added there but omitted from the `ALL_*`
    // array here fails this assertion. (The same pairs are additionally locked
    // at compile time — seven beside their tables in `geometry_ops/tests.rs`,
    // and Boolean/Surface beside theirs above.)
    assert_eq!(ALL_PRIMITIVE.len(), PrimitiveKind::VARIANT_COUNT, "ALL_PRIMITIVE is out of sync with PrimitiveKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_BOOLEAN.len(), BooleanOp::VARIANT_COUNT, "ALL_BOOLEAN is out of sync with BooleanOp::VARIANT_COUNT — update both together");
    assert_eq!(ALL_MODIFY.len(), 10, "ALL_MODIFY census");
    assert_eq!(ALL_TRANSFORM.len(), TransformKind::VARIANT_COUNT, "ALL_TRANSFORM is out of sync with TransformKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_PATTERN.len(), PatternKind::VARIANT_COUNT, "ALL_PATTERN is out of sync with PatternKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_SWEEP.len(), SweepKind::VARIANT_COUNT, "ALL_SWEEP is out of sync with SweepKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_CURVE.len(), CurveKind::VARIANT_COUNT, "ALL_CURVE is out of sync with CurveKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_PROFILE.len(), ProfileKind::VARIANT_COUNT, "ALL_PROFILE is out of sync with ProfileKind::VARIANT_COUNT — update both together");
    assert_eq!(ALL_SURFACE.len(), SurfaceKind::VARIANT_COUNT, "ALL_SURFACE is out of sync with SurfaceKind::VARIANT_COUNT — update both together");
    // Isosurface is a marker family with no nested kind enum, so it has no
    // VARIANT_COUNT to cross-check against; this one stays tautological.
    assert_eq!(ALL_ISOSURFACE.len(), 1, "ALL_ISOSURFACE census (tautological — real tripwire is exhaustive match)");

    // Modify: real runtime cross-check against the compiler's source-of-truth.
    assert_eq!(
        ALL_MODIFY.len(),
        reify_compiler::ModifyKind::VARIANT_COUNT,
        "ALL_MODIFY is out of sync with ModifyKind::VARIANT_COUNT — update both together"
    );

    // Exactly 10 CompiledGeometryOp variant families are represented (matches the
    // no-`_` guard in `_assert_variant_families_exhaustive`). This array's own
    // .len() == 10 is also tautological (hardcoded 10 entries), but the
    // _assert_variant_families_exhaustive match is the real compile-time guard.
    // Widths taken from the compiler's VARIANT_COUNTs, NOT from the local arrays
    // (task #5754) — so this roll-up is anchored to reify-compiler's source of
    // truth rather than re-deriving itself from the very tables it is auditing.
    // The nine per-family assertions above are what tie the local arrays to it.
    let kind_family_counts = [
        PrimitiveKind::VARIANT_COUNT,
        BooleanOp::VARIANT_COUNT,
        ModifyKind::VARIANT_COUNT,
        TransformKind::VARIANT_COUNT,
        PatternKind::VARIANT_COUNT,
        SweepKind::VARIANT_COUNT,
        CurveKind::VARIANT_COUNT,
        ProfileKind::VARIANT_COUNT,
        SurfaceKind::VARIANT_COUNT,
    ];
    let family_widths = [
        kind_family_counts[0],
        kind_family_counts[1],
        kind_family_counts[2],
        kind_family_counts[3],
        kind_family_counts[4],
        kind_family_counts[5],
        kind_family_counts[6],
        kind_family_counts[7],
        kind_family_counts[8],
        ALL_ISOSURFACE.len(),
    ];
    assert_eq!(family_widths.len(), 10, "CompiledGeometryOp variant family count");

    // The nine kind families' census, derived from VARIANT_COUNT, must match the
    // figure documented in this test's doc block above. A doc/data mismatch here
    // means the narrative has drifted from the compiler and must be corrected.
    let kind_family_total: usize = kind_family_counts.iter().sum();
    assert_eq!(
        kind_family_total, DOCUMENTED_KIND_FAMILY_CENSUS,
        "the nine-family VARIANT_COUNT census disagrees with the figure documented on \
         coverage_all_variant_families_and_nested_kinds — fix the doc block, not this assert"
    );

    // Total nested-kind census across all ten families (the nine kind families
    // plus the Isosurface marker).
    let total: usize = family_widths.iter().sum();
    assert_eq!(total, 54, "total nested-kind census; update if any ALL_* array is resized");
}
