//! Task 5194 regression lock — OCCT-free proof that a `: Rigid` body's four
//! auto-derived mass-property cells (`mass`, `centroid`, `moment_of_inertia`,
//! `moi_principal`) resolve to non-`Undef` values through `Engine::build`.
//!
//! This pins the COMPUTE contract the GUI surfacing fix depends on: the panel
//! bug (task 5194) is that `build_gui_state` sourced these cells from the
//! kernel-less `check.values` rather than the kernel-bearing build's
//! `run_post_processes` output. The GUI-side fix
//! (`gui/src-tauri/src/engine.rs::surface_geometry_derived_cells`) overlays the
//! build's `result.values` onto the panel — a fix that is only correct while
//! `build().values` actually carries the computed cells. This test guards that
//! premise deterministically, WITHOUT OCCT, via a seeded `MockGeometryKernel`.
//!
//! Determinacy-only assertions (non-`Undef`), NOT analytic magnitudes: the
//! exact-value analytic checks already live in the OCCT-gated
//! `rigid_moment_of_inertia_autoderive_smoke.rs`; the seeded mock values here are
//! arbitrary stand-ins. `moi_principal` is the strongest witness — a derived-let
//! that only the build-only `post_process_derived_lets` pass resolves.
//!
//! NOTE (task 5194 plan, step-5): this guard is expected to pass immediately. If
//! it is ever RED, the Rigid auto-derive COMPUTE path itself regressed — fix the
//! compute path, do NOT weaken this test.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, GeometryHandleId, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

/// Inline mirror of `examples/rigid_mass_props_smoke.ri`: a single-`box` `: Rigid`
/// body whose geometry realizes to a deterministic `GeometryHandleId(1)` under
/// `MockGeometryKernel` (one primitive, no booleans, no handle non-determinism).
const RIGID_MASS_SMOKE_SRC: &str = r#"
structure def RigidMassSmoke : Rigid {
    param depth : Length = 300mm
    param geometry : Solid = box(100mm, 100mm, depth)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    constraint depth > 0mm
}
"#;

/// The omitting conformer compiles clean (unconditional, OCCT-independent) and,
/// under a seeded mock kernel, `Engine::build` resolves all four auto-derived
/// mass-property cells to non-`Undef` values.
#[test]
fn rigid_mass_props_auto_derive_to_determined_via_build_occt_free() {
    // ── compile-time assertion (unconditional) ────────────────────────────────
    let compiled = parse_and_compile_with_stdlib(RIGID_MASS_SMOKE_SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "task-5194: RigidMassSmoke should compile with no error-severity \
         diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── seeded-mock build (deterministic, OCCT-free) ──────────────────────────
    // The single `box(...)` realizes to GeometryHandleId(1). Seed the geometry
    // queries the stdlib Rigid : Physical auto-derives dispatch:
    //   mass              ← Volume(1)
    //   centroid          ← Centroid(1)               (JSON-Point3 reply)
    //   moment_of_inertia ← InertiaTensor(1, ρ=7850)  (row-of-row reply)
    //   moi_principal     ← eigenvalues(moment_of_inertia)  (pure compute)
    // Diagonal diag(1,2,3) keeps the principal moments positive.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new()
        .with_volume_result(GeometryHandleId(1), Value::Real(0.003))
        .with_centroid_result(
            GeometryHandleId(1),
            Value::String("{\"x\":0.05,\"y\":0.05,\"z\":0.15}".to_string()),
        )
        .with_inertia_tensor_result(
            GeometryHandleId(1),
            7850.0,
            Value::List(vec![
                Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
                Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
                Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
            ]),
        );
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    for member in ["mass", "centroid", "moment_of_inertia", "moi_principal"] {
        let cell = ValueCellId::new("RigidMassSmoke", member);
        match result.values.get(&cell) {
            Some(Value::Undef) | None => panic!(
                "task-5194: RigidMassSmoke.{member} must be a determined (non-Undef) \
                 value in build().values — the invariant the GUI surfacing fix \
                 depends on; got {:?}",
                result.values.get(&cell)
            ),
            Some(_) => {}
        }
    }
}
