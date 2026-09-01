//! Engine-level integration test for `body_mass_props` (RBD-β, task 3829;
//! PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
//!
//! Pins the full engine dispatch path end-to-end:
//!   parse → `compile_with_stdlib` → `Engine::build` →
//!   `engine_build.rs::post_process_body_mass_props` →
//!   `reify_eval::dynamics_ops::try_eval_body_mass_props`.
//!
//! Observable signal (kernel-INDEPENDENT, so a `MockGeometryKernel` suffices):
//! a body with NO resolvable `Material.density` passed to `body_mass_props`
//! must (a) emit exactly one `E_DynamicsNoDensity` error (the density ladder
//! has no water fallback — ambient-default-material C, task 4498) and (b) leave
//! the `mp` cell as a `MassProperties` `StructureInstance` with deferred
//! `Value::Undef` geometric fields.
//!
//! The MassProperties PSD inertia-validation hook (engine_eval.rs, task 3822)
//! classifies an `inertia == Value::Undef` field as `Skip` (no false
//! positives), so the assembled deferred instance is neither clobbered to a
//! bare `Undef` nor flagged `E_DynamicsInertiaNotPSD` — leaving exactly the one
//! `E_DynamicsNoDensity` error asserted below.

use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

/// A body with no `Material` (hence no `Material.density`) is passed to
/// `body_mass_props` — the fn-level density ladder now produces a hard
/// `E_DynamicsNoDensity` error (ambient-default-material C, task 4498). The
/// `mp` cell must still resolve to a `MassProperties` `StructureInstance`
/// with all geometric fields at `Value::Undef` (degrade shape).
#[test]
fn body_mass_props_without_material_density_errors_with_no_density() {
    let source = "structure def MassPropsBox {\n    \
        let body = box(50mm, 30mm, 10mm)\n    \
        let mp = body_mass_props(body)\n}";

    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "MassPropsBox should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: body_mass_props does not consult the kernel when no
    // density resolves (geometric fields stay Undef), so a plain mock kernel is
    // enough.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.build(&compiled, ExportFormat::Step);

    // (1) Exactly one DynamicsNoDensity error (no water fallback).
    let no_density: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsNoDensity))
        .collect();
    assert_eq!(
        no_density.len(),
        1,
        "exactly one E_DynamicsNoDensity error expected when body_mass_props has no \
         resolvable density; got {} (all diagnostics: {:#?})",
        no_density.len(),
        result.diagnostics,
    );
    assert_eq!(
        no_density[0].severity,
        Severity::Error,
        "the no-density diagnostic must be a hard Severity::Error"
    );
    // Message must name all three fixes.
    let msg = &no_density[0].message;
    assert!(
        msg.contains("explicit density argument"),
        "error message must mention 'explicit density argument' fix; got: {msg:?}"
    );
    assert!(
        msg.contains("Material"),
        "error message must mention Material density fix; got: {msg:?}"
    );
    assert!(
        msg.contains("default Material"),
        "error message must mention `default Material` ambient fix; got: {msg:?}"
    );

    // (2) The `mp` cell evaluates to a MassProperties StructureInstance. The
    // geometric fields are Undef (no density → geometry query skipped); the
    // PSD hook's Undef-inertia Skip rule keeps the instance intact.
    let cell = ValueCellId::new("MassPropsBox", "mp");
    match result.values.get(&cell) {
        Some(Value::StructureInstance(data)) => {
            assert_eq!(
                data.type_name, "MassProperties",
                "MassPropsBox.mp must be a MassProperties StructureInstance, got type_name {:?}",
                data.type_name
            );
        }
        other => panic!(
            "MassPropsBox.mp must be a MassProperties StructureInstance (geometric fields \
             deferred Undef on no-density error), got {other:?}"
        ),
    }
}

/// End-to-end OCCT pin test: `body_mass_props` on a 50 mm × 30 mm × 10 mm
/// steel box (7850 kg/m³) must evaluate to a `MassProperties` StructureInstance
/// whose geometric fields satisfy:
///
///   mass ≈ 7850·(0.05·0.03·0.01) = 0.11775 kg                (within 1e-9)
///   I_xx ≈ (1/12)·m·(H²+D²)  H=0.03, D=0.01                 (within 1e-9)
///   I_yy ≈ (1/12)·m·(W²+D²)  W=0.05, D=0.01                 (within 1e-9)
///   I_zz ≈ (1/12)·m·(W²+H²)  W=0.05, H=0.03                 (within 1e-9)
///   off-diagonals < 1e-9 (axis-aligned centroidal box)
///   com is a Value::Point of 3 finite LENGTH-dimensioned scalars (shape only)
///
/// Gated on `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners
/// without OCCT.  The compilation check runs unconditionally so a grammar
/// regression is caught even on OCCT-less runners.
#[test]
fn body_mass_props_box_evals_to_computed_mass_properties() {
    let source = "structure def MassPropsBox {\n    \
        let b = box(50mm, 30mm, 10mm)\n    \
        let rho = 7850kg/m^3\n    \
        let mp = body_mass_props(b, rho)\n}";

    // Validate compilation unconditionally — a grammar/signature regression
    // must fail on every runner, not just those with OCCT.
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "MassPropsBox should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip the OCCT-dependent kernel build / numeric assertions if OCCT is absent.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with a real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // The compilation and dispatch must produce no error-severity diagnostics.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "body_mass_props on real box must produce no errors; got:\n{:#?}",
        errors
    );

    // Extract the MassProperties StructureInstance from the `mp` cell.
    let cell = ValueCellId::new("MassPropsBox", "mp");
    let data = match result.values.get(&cell) {
        Some(Value::StructureInstance(d)) => d,
        other => panic!(
            "MassPropsBox.mp must be a MassProperties StructureInstance, got {other:?}\n\
             (diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        data.type_name, "MassProperties",
        "mp must be type MassProperties, got {:?}",
        data.type_name
    );

    let tol = 1e-9_f64;

    // ── mass: Value::Scalar<MASS> ─────────────────────────────────────────────
    let mass = match data.fields.get("mass").expect("mass field") {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "mass field must be MASS-dimensioned"
            );
            *si_value
        }
        other => panic!("mass field must be a Value::Scalar, got {other:?}"),
    };

    // m = ρ·V = 7850 · 0.05 · 0.03 · 0.01
    let w = 0.05_f64;
    let h = 0.03_f64;
    let d = 0.01_f64;
    let rho = 7850.0_f64;
    let expected_mass = rho * w * h * d;
    assert!(
        (mass - expected_mass).abs() < tol,
        "mass: expected {expected_mass:.6e} kg, got {mass:.6e} kg \
         (delta {:.3e}, tol {tol:.0e})",
        (mass - expected_mass).abs()
    );

    // ── com: Value::Point of 3 LENGTH-dimensioned scalars at (0, 0, 0) ──────
    // OCCT creates box(W,H,D) centered at the origin — corner at
    // (-W/2, -H/2, -D/2) — so the centroid is (0, 0, 0) in the model frame
    // (confirmed: occt_wrapper.cpp make_box uses gp_Pnt(-width/2, -height/2,
    // -depth/2) as the BRepPrimAPI_MakeBox corner). Both CenterOfMass and the
    // inertia formulas are consistent: the (1/12)m(…) formula is centroidal, so
    // the centroid location is well-defined and can be pinned at 1e-9 m.
    let com = data.fields.get("com").expect("com field");
    let comps = match com {
        Value::Point(v) => v,
        other => panic!("com field must be Value::Point, got {other:?}"),
    };
    assert_eq!(comps.len(), 3, "com must have exactly 3 components");
    for (i, comp) in comps.iter().enumerate() {
        match comp {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::LENGTH => {
                assert!(
                    (si_value - 0.0_f64).abs() < tol,
                    "com[{i}]: expected 0.0 m (box centred at origin), got {si_value:.6e} m \
                     (delta {:.3e}, tol {tol:.0e})",
                    si_value.abs()
                );
            }
            other => panic!("com[{i}] must be a LENGTH-dimensioned Scalar, got {other:?}"),
        }
    }

    // ── inertia: Value::Matrix of Value::Scalar{MOMENT_OF_INERTIA} ───────────
    let inertia_rows = match data.fields.get("inertia").expect("inertia field") {
        Value::Matrix(rows) => rows,
        other => panic!("inertia field must be Value::Matrix, got {other:?}"),
    };
    assert_eq!(inertia_rows.len(), 3, "inertia must have 3 rows");
    for r in inertia_rows {
        assert_eq!(r.len(), 3, "each inertia row must have 3 columns");
    }

    let get = |r: usize, c: usize| -> f64 {
        match &inertia_rows[r][c] {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "inertia[{r}][{c}] must be Value::Scalar{{MOMENT_OF_INERTIA}}, got {other:?}"
            ),
        }
    };

    // Analytic centroidal moments for axis-aligned box:
    //   I_xx = (1/12)·m·(H² + D²)   (rotation about x: spans H and D)
    //   I_yy = (1/12)·m·(W² + D²)   (rotation about y: spans W and D)
    //   I_zz = (1/12)·m·(W² + H²)   (rotation about z: spans W and H)
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);

    assert!(
        (get(0, 0) - i_xx).abs() < tol,
        "I_xx=[0,0]: expected {i_xx:.6e}, got {:.6e} (delta {:.3e}, tol {tol:.0e})",
        get(0, 0),
        (get(0, 0) - i_xx).abs()
    );
    assert!(
        (get(1, 1) - i_yy).abs() < tol,
        "I_yy=[1,1]: expected {i_yy:.6e}, got {:.6e} (delta {:.3e}, tol {tol:.0e})",
        get(1, 1),
        (get(1, 1) - i_yy).abs()
    );
    assert!(
        (get(2, 2) - i_zz).abs() < tol,
        "I_zz=[2,2]: expected {i_zz:.6e}, got {:.6e} (delta {:.3e}, tol {tol:.0e})",
        get(2, 2),
        (get(2, 2) - i_zz).abs()
    );

    // Off-diagonals must be zero (axis-aligned centroidal box).
    let off_diag = [
        (get(0, 1), "0,1"),
        (get(0, 2), "0,2"),
        (get(1, 0), "1,0"),
        (get(1, 2), "1,2"),
        (get(2, 0), "2,0"),
        (get(2, 1), "2,1"),
    ];
    for (v, label) in &off_diag {
        assert!(
            v.abs() < tol,
            "off-diagonal [{label}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }
}
