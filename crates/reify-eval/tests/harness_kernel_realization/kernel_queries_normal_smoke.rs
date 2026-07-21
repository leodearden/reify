//! End-to-end smoke test for `normal(Surface, Point3<Length>) -> Vector3<Dimensionless>`
//! (task 3615, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-ζ).
//!
//! The fixture `examples/kernel_queries/normal_smoke.ri` contains:
//!
//! ```ri
//! structure def NormalSmoke {
//!     let solid = box(10mm, 10mm, 10mm)
//!     let pt    = point3(0mm, 0mm, 5mm)
//!     let n     = normal(solid, pt)
//! }
//! ```
//!
//! Two assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — `normal_smoke.ri` parses + typechecks and
//!    the `NormalSmoke.n` cell's compile-time type is `Type::vec3(Type::dimensionless_scalar())`
//!    (dimensionless Vector3), proving the `units.rs` GEOMETRY_QUERY_NAMES
//!    registration and `geometry_query_result_type` arm wire the cell type
//!    through the compiler.  At engine runtime the `solid` arg is a
//!    `TopAbs_SOLID` handle — `resolve_geometry_handle_arg` resolves it but
//!    `surface_normal_at_point` in C++ rejects non-FACE shapes, so the cell
//!    evaluates to `Value::Undef`.  This is expected pre-Phase-3 (KGQ-ρ) and
//!    does not regress.
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    spawn a real `OcctKernelHandle`, build `box(10mm, 10mm, 10mm)`, extract
//!    faces, and call
//!    `kernel.query(&GeometryQuery::FaceNormalAt { handle, px, py, pz })`
//!    directly on a box face to prove the `OcctKernel::query()` dispatch →
//!    `surface_normal_at_point` FFI chain is live from the eval test harness.
//!    The numeric [0, 0, 1] pin for the +Z face is in
//!    `face_differential_integration.rs::
//!     geometry_query_face_normal_at_on_top_face_of_box_encodes_z_normal`
//!    (step-1).
//!
//! The `Normal` helper dispatch arm (`TopologySelectorHelper::Normal`,
//! `dispatch_normal_vector3`) and its four mock-kernel unit tests (step-5/6)
//! together prove the `try_eval_topology_selector` path; this smoke test pins
//! the compile-type contract and OCCT FFI reachability.
//!
//! Modelled on `kernel_queries_contains.rs` (same Surface + Point3 arg shape)
//! and `kernel_queries_angle_smoke.rs` (CARGO_MANIFEST_DIR path + type
//! assertion pattern).

use reify_core::Type;
use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const NORMAL_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/normal_smoke.ri"
);

/// Pins the user-observable signal for KGQ-ζ:
///
/// - `NormalSmoke.n` must have compile-time type `Type::vec3(Type::dimensionless_scalar())`,
///   verifying the `units.rs` registration wires the cell type correctly.
/// - `GeometryQuery::FaceNormalAt` through the real OCCT kernel returns
///   `Ok(Value::String(_))`, confirming the `OcctKernel::query()` dispatch
///   and `surface_normal_at_point` FFI chain are live.
///
/// Skips the OCCT-backed assertion cleanly when OCCT is not available.
#[test]
fn normal_smoke_compiles_as_vec3_real_and_face_normal_at_ffis() {
    // ── assertion 1: fixture exists, compiles, NormalSmoke.n typed Vec3<Real> ──

    // Read the fixture unconditionally so a missing file fails even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(NORMAL_SMOKE_PATH)
        .expect("examples/kernel_queries/normal_smoke.ri should exist (task 3615 step-8)");

    // Validate fixture compilation unconditionally — a grammar or type-system
    // regression (e.g. `normal` signature change) should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/normal_smoke.ri should compile with no \
         error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Locate the NormalSmoke topology template.
    let tmpl = compiled
        .templates
        .iter()
        .find(|t| t.name == "NormalSmoke")
        .expect("NormalSmoke structure should exist in normal_smoke.ri");

    // The `n = normal(solid, pt)` cell must typecheck as Vector3<Dimensionless>.
    // Set by `geometry_query_result_type("normal") → Type::vec3(Type::dimensionless_scalar())` in
    // crates/reify-compiler/src/units.rs (KGQ-ζ, task 3615 step-4).
    let n_cell = tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "n")
        .expect("NormalSmoke.n value cell should exist");

    assert_eq!(
        n_cell.cell_type,
        Type::vec3(Type::dimensionless_scalar()),
        "NormalSmoke.n should have type Type::vec3(Type::dimensionless_scalar()) \
         (dimensionless Vector3, KGQ-ζ registration), got: {:?}",
        n_cell.cell_type
    );

    // ── assertion 2: real-OCCT FaceNormalAt query chain is live (gated) ────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // Build box(10mm × 10mm × 10mm) centred at origin.
    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("box(10mm, 10mm, 10mm) should build successfully");

    // Extract the 6 faces.
    let faces = kernel
        .extract_faces(box_handle.id)
        .expect("extract_faces should succeed on a valid box");
    assert_eq!(
        faces.len(),
        6,
        "box(10mm, 10mm, 10mm) should have exactly 6 faces"
    );

    // Drive GeometryQuery::FaceNormalAt through the real kernel on each face.
    // `ValueOfUV` projects any 3D point to the nearest (u,v) on the face's
    // underlying surface — this is well-defined for all 6 planar faces of a
    // box regardless of which specific face is tested.
    // Probe point: (0, 0, 5mm = 0.005 m) = +Z face midpoint.  For the +Z
    // face this is an exact on-surface point (plane z = +0.005 m); for all
    // other faces it is an off-surface point that ValueOfUV projects to the
    // nearest UV coordinates.  Either way the query always returns
    // Ok(Value::String(json-point3)) for a valid planar face handle.
    //
    // Numeric pin (x≈0, y≈0, z≈1 for the +Z face within 1e-9) is in step-1:
    //   reify-kernel-occt/tests/face_differential_integration.rs::
    //     geometry_query_face_normal_at_on_top_face_of_box_encodes_z_normal
    for &face in &faces {
        let result = kernel.query(&GeometryQuery::FaceNormalAt {
            handle: face,
            px: 0.0,
            py: 0.0,
            pz: 0.005,
        });

        assert!(
            result.is_ok(),
            "GeometryQuery::FaceNormalAt on a box face should succeed \
             (ValueOfUV projects any point to nearest UV), got: {:?}",
            result
        );

        match result.unwrap() {
            Value::String(_json) => {
                // JSON-Point3 wire format confirmed for this face.
            }
            other => panic!(
                "GeometryQuery::FaceNormalAt should return Value::String(json-point3), \
                 got: {:?}",
                other
            ),
        }
    }
}
