//! End-to-end signal tests for the KIN-OFFSET authoring surface.
//!
//! - **α (task 4331)**: `revolute_pivot_offset_e2e` — translation-pivot (point3) authoring.
//! - **δ (task 4394)**: `oriented_frame3_linkage_e2e` — oriented Frame3 origin authoring.
//!
//! Original α doc:
//!
//! Loads `examples/kinematic/revolute_pivot_offset.ri`, drives it through
//! `parse_and_compile_with_stdlib → eval`, and asserts the pivot-offset
//! invariants:
//!
//! - `t0 = transform_at(j, 0rad)`: translation == (0.04, 0, 0) m (pivot offset
//!   reflected, invariant under joint angle = 0).
//! - `t1 = transform_at(j, π/3 rad)`: translation == (0.04, 0, 0) m (invariant
//!   under joint angle), rotation == R_z(π/3) = (cos π/6, 0, 0, sin π/6).
//!
//! Asserts the landed behavior: the fixture ships alongside this test and is
//! evaluated clean (no error diagnostics).

// Value::Map uses BTreeMap<Value, Value>; Value's interior-mutable SampledField
// (AtomicBool) trips clippy::mutable_key_type, but Ord/Hash on Value are by-design.
#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{
    collect_errors, make_simple_engine, parse_and_compile_with_stdlib, read_f64,
};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/revolute_pivot_offset.ri"
);

const ORIENTED_FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kinematic/spatial_linkage_oriented.ri"
);

/// Read the α fixture source, caching it via OnceLock.
fn fixture_source() -> &'static str {
    use std::sync::OnceLock;
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(FIXTURE_PATH)
            .unwrap_or_else(|e| panic!("{FIXTURE_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Read the δ oriented-linkage fixture source, caching it via OnceLock.
fn oriented_fixture_source() -> &'static str {
    use std::sync::OnceLock;
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(ORIENTED_FIXTURE_PATH)
            .unwrap_or_else(|e| panic!("{ORIENTED_FIXTURE_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Decompose a `Value::Transform` into `((w,x,y,z), [tx,ty,tz])`.
fn decompose_transform(v: &Value, label: &str) -> ((f64, f64, f64, f64), [f64; 3]) {
    let (rotation, translation) = match v {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("{label}: expected Value::Transform, got {other:?}"),
    };
    let (rw, rx, ry, rz) = match rotation {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("{label}: expected Value::Orientation, got {other:?}"),
    };
    let comps = match translation {
        Value::Vector(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Vector len=3, got {other:?}"),
    };
    (
        (rw, rx, ry, rz),
        [
            read_f64(&comps[0], &format!("{label}.t[0]")),
            read_f64(&comps[1], &format!("{label}.t[1]")),
            read_f64(&comps[2], &format!("{label}.t[2]")),
        ],
    )
}

/// Signal test: `revolute_pivot_offset.ri` produces the expected pivot-offset
/// transforms for θ = 0 and θ = π/3.
///
/// Invariants:
/// - No error-severity diagnostics.
/// - `t0.translation == (0.04, 0, 0)` m within 1e-12 (pivot offset at θ=0).
/// - `t1.translation == (0.04, 0, 0)` m within 1e-12 (pivot invariant under θ).
/// - `t1.rotation == R_z(π/3)` = `(cos(π/6), 0, 0, sin(π/6))` within 1e-12.
#[test]
fn revolute_pivot_offset_e2e() {
    let source = fixture_source();
    assert!(
        !source.is_empty(),
        "revolute_pivot_offset.ri must be non-empty"
    );

    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No error diagnostics.
    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for revolute_pivot_offset.ri, got: {errors:?}"
    );

    // Resolve bindings from the structure.  The fixture defines structure "RevolutePivotOffset".
    let get_value = |name: &str| {
        let id = ValueCellId::new("RevolutePivotOffset", name);
        result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("RevolutePivotOffset.{name} not found in eval result"))
    };

    // t0 = transform_at(j, 0rad): translation == (0.04, 0, 0) m.
    let t0 = get_value("t0");
    let (_, [t0x, t0y, t0z]) = decompose_transform(t0, "t0");
    let pivot_m = 0.04_f64; // 40 mm in SI
    assert!(
        (t0x - pivot_m).abs() < 1e-12,
        "t0.tx should be {pivot_m} m (pivot offset), got {t0x}"
    );
    assert!(t0y.abs() < 1e-12, "t0.ty should be 0, got {t0y}");
    assert!(t0z.abs() < 1e-12, "t0.tz should be 0, got {t0z}");

    // t1 = transform_at(j, π/3 rad): translation == (0.04, 0, 0) m (invariant under θ).
    let t1 = get_value("t1");
    let ((rw, rx, ry, rz), [t1x, t1y, t1z]) = decompose_transform(t1, "t1");
    assert!(
        (t1x - pivot_m).abs() < 1e-12,
        "t1.tx should be {pivot_m} m (pivot invariant under θ), got {t1x}"
    );
    assert!(t1y.abs() < 1e-12, "t1.ty should be 0, got {t1y}");
    assert!(t1z.abs() < 1e-12, "t1.tz should be 0, got {t1z}");

    // t1 rotation must be R_z(π/3) = (cos(π/6), 0, 0, sin(π/6)) up to sign.
    let theta = std::f64::consts::PI / 3.0;
    let qw = (theta / 2.0).cos(); // cos(π/6)
    let qz = (theta / 2.0).sin(); // sin(π/6)
    let matches_pos =
        (rw - qw).abs() < 1e-12 && rx.abs() < 1e-12 && ry.abs() < 1e-12 && (rz - qz).abs() < 1e-12;
    let matches_neg =
        (rw + qw).abs() < 1e-12 && rx.abs() < 1e-12 && ry.abs() < 1e-12 && (rz + qz).abs() < 1e-12;
    assert!(
        matches_pos || matches_neg,
        "t1 rotation should be R_z(π/3) ≈ ({qw},0,0,{qz}) up to sign, got ({rw},{rx},{ry},{rz})"
    );
}

/// Signal test (δ, task 4394): `spatial_linkage_oriented.ri` produces the expected
/// oriented-origin poses for both a prismatic and a revolute joint authored with
/// `frame3(point3(..mm), orient_axis_angle(vec3(..), ..rad))` as the 3rd pivot arg.
///
/// Invariants (hand-computed, tolerance 1e-12):
///
/// **t_slider** (oriented prismatic, axis=+X, origin=R_z(90°), d=0.5m):
/// - rotation = R_z(90°) = (√2/2, 0, 0, √2/2)
/// - translation = R_z(90°)·(0.5,0,0) = (0, 0.5, 0)
///
/// **t_hinge** (oriented revolute, axis=+Z, origin=frame3(point3(100mm,0,0), R_x(90°)), θ=π/2):
/// - rotation = R_x(90°)·R_z(90°) = (0.5, 0.5, -0.5, 0.5)  [Hamilton product]
/// - translation = R_x(90°)·(0,0,0) + (0.1,0,0) = (0.1, 0, 0)
#[test]
fn oriented_frame3_linkage_e2e() {
    let source = oriented_fixture_source();
    assert!(
        !source.is_empty(),
        "spatial_linkage_oriented.ri must be non-empty"
    );

    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No error diagnostics.
    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics for spatial_linkage_oriented.ri, got: {errors:?}"
    );

    // Resolve bindings from the structure.  The fixture defines structure "SpatialLinkageOriented".
    let get_value = |name: &str| {
        let id = ValueCellId::new("SpatialLinkageOriented", name);
        result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("SpatialLinkageOriented.{name} not found in eval result"))
    };

    let sq2_2 = std::f64::consts::SQRT_2 / 2.0;

    // t_slider: oriented prismatic, axis=+X, R_z(90°) origin, d=0.5m.
    // Expected: rotation=(√2/2,0,0,√2/2) up to sign, translation=(0,0.5,0).
    let t_slider = get_value("t_slider");
    let ((sw, sx, sy, sz), [stx, sty, stz]) = decompose_transform(t_slider, "t_slider");
    let s_matches_pos = (sw - sq2_2).abs() < 1e-12
        && sx.abs() < 1e-12
        && sy.abs() < 1e-12
        && (sz - sq2_2).abs() < 1e-12;
    let s_matches_neg = (sw + sq2_2).abs() < 1e-12
        && sx.abs() < 1e-12
        && sy.abs() < 1e-12
        && (sz + sq2_2).abs() < 1e-12;
    assert!(
        s_matches_pos || s_matches_neg,
        "t_slider rotation should be R_z(90°) ≈ ({sq2_2},0,0,{sq2_2}) up to sign, got ({sw},{sx},{sy},{sz})"
    );
    assert!(stx.abs() < 1e-12, "t_slider.tx should be 0, got {stx}");
    assert!(
        (sty - 0.5).abs() < 1e-12,
        "t_slider.ty should be 0.5 m (R_z(90°)·+X = +Y), got {sty}"
    );
    assert!(stz.abs() < 1e-12, "t_slider.tz should be 0, got {stz}");

    // t_hinge: oriented revolute, axis=+Z, origin=frame3(point3(100mm,0,0), R_x(90°)), θ=π/2.
    // Expected: rotation=(0.5,0.5,-0.5,0.5) up to sign, translation=(0.1,0,0).
    let t_hinge = get_value("t_hinge");
    let ((hw, hx, hy, hz), [htx, hty, htz]) = decompose_transform(t_hinge, "t_hinge");
    let h_matches_pos = (hw - 0.5).abs() < 1e-12
        && (hx - 0.5).abs() < 1e-12
        && (hy + 0.5).abs() < 1e-12
        && (hz - 0.5).abs() < 1e-12;
    let h_matches_neg = (hw + 0.5).abs() < 1e-12
        && (hx + 0.5).abs() < 1e-12
        && (hy - 0.5).abs() < 1e-12
        && (hz + 0.5).abs() < 1e-12;
    assert!(
        h_matches_pos || h_matches_neg,
        "t_hinge rotation should be (0.5,0.5,-0.5,0.5) up to sign, got ({hw},{hx},{hy},{hz})"
    );
    assert!(
        (htx - 0.1).abs() < 1e-12,
        "t_hinge.tx should be 0.1 m (100mm pivot), got {htx}"
    );
    assert!(hty.abs() < 1e-12, "t_hinge.ty should be 0, got {hty}");
    assert!(htz.abs() < 1e-12, "t_hinge.tz should be 0, got {htz}");
}
