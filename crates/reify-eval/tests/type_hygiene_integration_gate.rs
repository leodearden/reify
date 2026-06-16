//! Type-hygiene λ integration gate — §10 boundary-test table (task #4495).
//!
//! # Purpose
//!
//! This file is the eval-layer integration gate for the entire type-hygiene
//! cluster (PRD `docs/prds/v0_6/type-hygiene.md`). It tests that all §10
//! boundary behaviours delivered by the sibling tasks α–κ compose correctly
//! in the eval layer, and that the sole new CI artefact — the green example
//! `examples/type_hygiene/type_hygiene_surface.ri` — compiles and evaluates
//! clean.
//!
//! **Row ownership:** each §10 row is tested in exactly ONE canonical file;
//! this gate is the *collected-view* for the eval-layer rows.  Compiler-layer
//! rows (1, 3, 4a, 5, 7a, 10, 11) live in
//! `crates/reify-compiler/tests/type_hygiene_integration_gate.rs`; the CLI
//! row (12) lives in `crates/reify-cli/tests/cli_type_hygiene_strict.rs`.
//!
//! # §10 boundary-test table + row→owner cross-reference
//!
//! | Row | Description                                                  | Owner (eval gate fn)                                                  |
//! |-----|--------------------------------------------------------------|-----------------------------------------------------------------------|
//! | 1   | tensor>scalar → CmpOperandKind+fixit                         | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 2   | mass>0 OK / VIOLATED                                         | `ci_example_compiles_clean_and_evaluates_green` (OK) + `row_2_mass_constraint_violated` (VIOLATED) |
//! | 3   | bare 0 vs tensor → CmpOperandKind                            | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 4a  | `x and 5` err                                                | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 4b  | Kleene runtime preserved                                     | `row_4b_kleene_runtime_preserved`                                     |
//! | 5   | TypeParam gradualism no-diag                                 | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 6   | moi(b,material.density) via let → non-Undef tensor           | `ci_example_compiles_clean_and_evaluates_green` (OCCT-gated)          |
//! | 7a  | moi(b,7850.0) → ArgTypeMismatch                              | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 7b  | moi runtime density rejection never-silent                   | `row_7b_inline_density_rejection_never_silent`                        |
//! | 8   | body_mass_props(b,Pressure) loud reject                      | `row_8_body_mass_props_pressure_as_density_warns`                     |
//! | 9   | body_mass_props(b) no-Material warn + water UNCHANGED        | `row_9_body_mass_props_no_material_warns_and_uses_water`              |
//! | 10  | scalar override of tensor-defaulted trait param → mismatch   | `reify-compiler/tests/type_hygiene_integration_gate.rs`               |
//! | 11  | compatible override conforms                                 | `reify-compiler/tests/type_hygiene_integration_gate.rs` + example     |
//! | 12  | `reify check --strict` exit both ways                        | `crates/reify-cli/tests/cli_type_hygiene_strict.rs`                  |
//! | 13  | distinct indeterminacy messages                              | `row_13_distinct_indeterminacy_messages`                              |
//! | 14  | RNEA identical post-κ                                        | `row_14_rnea_numerically_identical_post_kappa`                        |

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
use reify_ir::{ExportFormat, Satisfaction, Value};
use reify_test_support::{
    MockGeometryKernel, check_source, check_source_with_stdlib, compile_source_with_stdlib,
    errors_only, eval_source, parse_and_compile_with_stdlib,
};

// ── Path constants ────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/type_hygiene/type_hygiene_surface.ri"
);

const PENDULUM_IDYN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/pendulum_idyn.ri"
);

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Assert `actual` is a rank-2 3×3 `MOMENT_OF_INERTIA`-dimensioned tensor
/// whose diagonal matches the analytic centroidal moments for a
/// 50 mm × 30 mm × 10 mm steel box at 7850 kg/m³ within 1e-9 kg·m²,
/// and whose off-diagonals are below 1e-9 kg·m².
///
/// Reference values (probe-5 / kernel_queries_moment_of_inertia_smoke.rs):
///   m = 7850 · 0.05 · 0.03 · 0.01 = 0.11775 kg
///   I_xx ≈ 9.8125e-6 kg·m²  (H=0.03, D=0.01)
///   I_yy ≈ 2.55125e-5 kg·m² (W=0.05, D=0.01)
///   I_zz ≈ 3.33625e-5 kg·m² (W=0.05, H=0.03)
fn assert_moi_box_analytic_tensor(actual: Option<&Value>, label: &str) {
    let w = 0.05_f64;
    let h = 0.03_f64;
    let d = 0.01_f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);
    let tol = 1e-9_f64;

    let rows = match actual {
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            rows
        }
        other => panic!(
            "{label}: expected a rank-2 Value::Tensor (3 rows × 3 cols) of \
             MOMENT_OF_INERTIA-dimensioned scalars, got: {other:?}"
        ),
    };

    fn extract(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "entry [{label}] must be Value::Scalar{{dimension: MOMENT_OF_INERTIA}}, \
                 got: {other:?}"
            ),
        }
    }

    fn row(r: &Value) -> &Vec<Value> {
        match r {
            Value::Tensor(cols) => cols,
            _ => unreachable!("already validated rank-2 shape"),
        }
    }

    let r0 = row(&rows[0]);
    let r1 = row(&rows[1]);
    let r2 = row(&rows[2]);

    let v00 = extract(&r0[0], "0,0");
    let v11 = extract(&r1[1], "1,1");
    let v22 = extract(&r2[2], "2,2");

    assert!(
        (v00 - i_xx).abs() < tol,
        "{label} I_xx=[0,0]: expected {i_xx:.3e}, got {v00:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v00 - i_xx).abs()
    );
    assert!(
        (v11 - i_yy).abs() < tol,
        "{label} I_yy=[1,1]: expected {i_yy:.3e}, got {v11:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v11 - i_yy).abs()
    );
    assert!(
        (v22 - i_zz).abs() < tol,
        "{label} I_zz=[2,2]: expected {i_zz:.3e}, got {v22:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v22 - i_zz).abs()
    );

    let off_diag = [
        (extract(&r0[1], "0,1"), "0,1"),
        (extract(&r0[2], "0,2"), "0,2"),
        (extract(&r1[0], "1,0"), "1,0"),
        (extract(&r1[2], "1,2"), "1,2"),
        (extract(&r2[0], "2,0"), "2,0"),
        (extract(&r2[1], "2,1"), "2,1"),
    ];
    for (v, lbl) in &off_diag {
        assert!(
            v.abs() < tol,
            "{label} off-diagonal [{lbl}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }
}

// ── CI-example gate (rows 2-OK + 6, OCCT-gated) ──────────────────────────────

/// §10 row 2-OK + row 6: compile the CI example `type_hygiene_surface.ri`,
/// assert zero Error-severity diagnostics, then under real OCCT assert that:
///
/// - **Row 6**: `moment_of_inertia(b, material.density)` via let-bound density
///   evaluates to a rank-2 3×3 `MOMENT_OF_INERTIA` tensor whose diagonal
///   matches the probe-5 analytic centroidal moments within 1e-9 kg·m² (same
///   reference as `kernel_queries_moment_of_inertia_smoke.rs`).
///
/// - **Row 2-OK**: the `mass > 0` bare-zero constraint is `Satisfaction::Satisfied`
///   for the `1kg` default.
///
/// RED (step-1): `examples/type_hygiene/type_hygiene_surface.ri` does not exist
/// yet → `read_to_string` panics.
/// GREEN (step-2): the example is created.
#[test]
fn ci_example_compiles_clean_and_evaluates_green() {
    // Read unconditionally: fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect(
            "examples/type_hygiene/type_hygiene_surface.ri must exist \
             (task #4495 step-2 creates it)"
        );

    // Compile with stdlib — zero Error diagnostics is the primary green signal.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "type_hygiene_surface.ri should compile with no error-severity diagnostics \
         (auto-gated by examples_smoke.rs), got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip OCCT-dependent eval assertions on runners without OCCT.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // §10 row 6: `let d = material.density; let i = moment_of_inertia(b, d)` →
    // non-Undef 3×3 MOMENT_OF_INERTIA tensor matching the probe-5 analytic values.
    let i_cell = ValueCellId::new("TypeHygieneSurface", "i");
    assert_moi_box_analytic_tensor(
        result.values.get(&i_cell),
        "TypeHygieneSurface.i (§10 row 6)",
    );

    // §10 row 2-OK: `constraint mass > 0` with `param mass = 1kg` → Satisfied.
    let mass_ok = result.constraint_results.iter().any(|e| {
        e.id.entity == "TypeHygieneSurface" && e.satisfaction == Satisfaction::Satisfied
    });
    assert!(
        mass_ok,
        "§10 row 2-OK: TypeHygieneSurface mass > 0 must be Satisfied with mass = 1kg (β \
         bare-zero coercion); constraint_results: {:?}",
        result.constraint_results
    );
}

// ── §10 row 2-VIOLATED: negative mass violates mass > 0 ─────────────────────

/// §10 row 2-VIOLATED: `param mass : Scalar<Mass> = -1kg` + `constraint mass > 0`
/// → the constraint resolves `Satisfaction::Violated`.
///
/// Characterises β's polymorphic bare-zero coercion from the VIOLATED side:
/// the coercion gives `0` the `Mass` dimension, so `−1kg > 0kg` is `false` →
/// `Violated` (not `Indeterminate`). Pairs with the OK branch in
/// `ci_example_compiles_clean_and_evaluates_green` (row 2-OK).
#[test]
fn row_2_mass_constraint_violated() {
    let result = check_source_with_stdlib(
        r#"
structure def MassViolated {
    param mass : Scalar<Mass> = -1kg
    constraint mass > 0
}
"#,
    );
    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "MassViolated")
        .expect("must have a constraint entry for MassViolated");
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Violated,
        "§10 row 2-VIOLATED: mass = −1kg → mass > 0 must be Violated (β bare-zero coercion)"
    );
}

// ── §10 row 4b: Kleene runtime preserved ─────────────────────────────────────

/// §10 row 4b: Kleene 3-valued logic is preserved at runtime.
///
/// - `false and undef == false` (AND absorption — false absorbs regardless of RHS)
/// - `true or undef == true`  (OR  absorption — true  absorbs regardless of RHS)
///
/// The `undef_bool` param has no default → `Value::Undef` at eval time.
/// Characterises the α And/Or guard's runtime complement: α fires at compile
/// time for definite non-Bool operands; the Kleene evaluator handles the Undef
/// case at runtime without producing `Indeterminate` when an absorbing element
/// is present.
///
/// References `kleene_e2e.rs::kleene_e2e_and_absorption` /
/// `kleene_e2e_or_absorption` (the authoritative Kleene gate).
#[test]
fn row_4b_kleene_runtime_preserved() {
    const SRC: &str = r#"
structure def KleeneRow4b {
    param undef_bool : Bool
    let and_abs = false and undef_bool
    let or_abs  = true or undef_bool
}
"#;
    let result = eval_source(SRC);

    // false and undef == false (Kleene AND absorption)
    let and_cell = ValueCellId::new("KleeneRow4b", "and_abs");
    assert_eq!(
        result.values.get(&and_cell),
        Some(&Value::Bool(false)),
        "§10 row 4b: `false and undef` must be Bool(false) (Kleene AND absorption)"
    );

    // true or undef == true (Kleene OR absorption)
    let or_cell = ValueCellId::new("KleeneRow4b", "or_abs");
    assert_eq!(
        result.values.get(&or_cell),
        Some(&Value::Bool(true)),
        "§10 row 4b: `true or undef` must be Bool(true) (Kleene OR absorption)"
    );
}

// ── §10 row 7b: runtime density rejection never-silent ───────────────────────

/// §10 row 7b: ε (task 4492) evaluate-then-accept — when a non-Density value
/// reaches `moment_of_inertia` at runtime via the `resolve_density_arg` path,
/// the rejection is NEVER a silent `Value::Undef`.  The eval engine emits a
/// `Severity::Warning` diagnostic containing:
///   - `"expects Density"` (the expected type)
///   - `"7850kg/m^3"` (the migration hint from `arg_acceptance::density_spec`)
///
/// This test exercises the `Acceptance::Rejected` arm of `resolve_density_arg`
/// (geometry_ops.rs) by passing a bare-Real `7850.0` (dimensionless, not Density)
/// to `moment_of_inertia`.  ζ (task 4493) also catches this at compile time
/// (`ArgTypeMismatch`); the engine proceeds to eval and emits the runtime
/// Warning too — demonstrating that BOTH layers report the error, with neither
/// being silent.
#[test]
fn row_7b_inline_density_rejection_never_silent() {
    const SRC: &str = r#"
structure def MoiRejection {
    let b = box(50mm, 30mm, 10mm)
    let i = moment_of_inertia(b, 7850.0)
}
"#;
    // compile_source_with_stdlib: ζ emits ArgTypeMismatch at compile time.
    // We tolerate that compile-time error (both layers report it) and proceed
    // to eval to verify the runtime Warning path (ε never-silent).
    let compiled = compile_source_with_stdlib(SRC);

    // Build with MockGeometryKernel so density-arg resolution runs (OCCT-independent).
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // The runtime Warning must contain "expects Density" AND the "7850kg/m^3" hint.
    let density_warn = result.diagnostics.iter().find(|d| {
        d.message.contains("expects Density") && d.message.contains("7850kg/m^3")
    });
    assert!(
        density_warn.is_some(),
        "§10 row 7b: inline density rejection must emit a Warning containing 'expects Density' \
         and '7850kg/m^3' hint (ε never-silent; arg_acceptance.rs:54); \
         build diagnostics: {:#?}",
        result.diagnostics
    );
}

// ── §10 row 8: body_mass_props Pressure-as-density is a loud reject ──────────

/// §10 row 8: δ (task 4491) — `body_mass_props(b, <Pressure value>)` emits at
/// least one `Severity::Warning` containing `"expects Density"` AND `"Pressure"`.
///
/// A `Pressure` value (e.g. 101325 Pa) has the wrong dimension for the density
/// argument; δ converts the silent Undef fall-through into a loud diagnostic.
/// Kernel-independent: the density ladder runs before any geometry query.
///
/// References `dynamics_ops.rs:1534` (the δ rejection arm in
/// `resolve_body_density`).
#[test]
fn row_8_body_mass_props_pressure_as_density_warns() {
    const SRC: &str = r#"
structure def BmpPressure {
    let b = box(50mm, 30mm, 10mm)
    let p = 101325Pa
    let mp = body_mass_props(b, p)
}
"#;
    let compiled = parse_and_compile_with_stdlib(SRC);
    // Compile may or may not flag the pressure arg — we care about the runtime Warning.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let pressure_warn = result.diagnostics.iter().find(|d| {
        d.message.contains("expects Density") && d.message.contains("Pressure")
    });
    assert!(
        pressure_warn.is_some(),
        "§10 row 8: body_mass_props with a Pressure density-arg must emit a Warning \
         containing 'expects Density' and 'Pressure' (δ loud reject, dynamics_ops.rs:1534); \
         build diagnostics: {:#?}",
        result.diagnostics
    );
    // The warning must be Severity::Warning (not Error — computation degrades gracefully).
    assert_eq!(
        pressure_warn.unwrap().severity,
        Severity::Warning,
        "§10 row 8: the Pressure-as-density rejection diagnostic must be Severity::Warning"
    );
}

// ── §10 row 9: body_mass_props no-Material → DynamicsDefaultDensity + water ──

/// §10 row 9: `body_mass_props(b)` on a body with no Material emits EXACTLY ONE
/// `DiagnosticCode::DynamicsDefaultDensity` warning and falls back to the 1000 kg/m³
/// water default.  This interim behaviour is UNCHANGED post-κ (decision 9).
///
/// Characterises the δ density ladder's water-fallback arm (dynamics_ops.rs),
/// verifying the one-warning invariant.  Kernel-independent.
///
/// References `dynamics_body_mass_props.rs::body_mass_props_without_material_density_warns_and_assembles_mass_properties`.
#[test]
fn row_9_body_mass_props_no_material_warns_and_uses_water() {
    const SRC: &str = r#"
structure def BmpNoMaterial {
    let body = box(50mm, 30mm, 10mm)
    let mp = body_mass_props(body)
}
"#;
    let compiled = parse_and_compile_with_stdlib(SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "BmpNoMaterial should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // §10 row 9 invariant: EXACTLY ONE DynamicsDefaultDensity warning.
    let default_density_warns: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsDefaultDensity))
        .collect();
    assert_eq!(
        default_density_warns.len(),
        1,
        "§10 row 9: body_mass_props(b) with no Material must emit exactly one \
         W_DynamicsDefaultDensity warning (water fallback, UNCHANGED interim); \
         got {} (all diagnostics: {:#?})",
        default_density_warns.len(),
        result.diagnostics,
    );
    assert_eq!(
        default_density_warns[0].severity,
        Severity::Warning,
        "§10 row 9: DynamicsDefaultDensity must be Severity::Warning"
    );

    // The mp cell must resolve to a MassProperties StructureInstance.
    let cell = ValueCellId::new("BmpNoMaterial", "mp");
    match result.values.get(&cell) {
        Some(Value::StructureInstance(data)) => {
            assert_eq!(
                data.type_name, "MassProperties",
                "BmpNoMaterial.mp must be a MassProperties StructureInstance, got {:?}",
                data.type_name
            );
        }
        other => panic!(
            "§10 row 9: BmpNoMaterial.mp must be a MassProperties StructureInstance \
             (geometric fields may be Undef), got {other:?}"
        ),
    }
}

// ── §10 row 13: distinct indeterminacy messages ───────────────────────────────

/// §10 row 13: ι (task 4489) — two distinct indeterminacy messages for two
/// distinct root causes:
///
/// - **"undefined inputs:"** — one or more leaf `ValueCellId`s in the constraint
///   expression resolved to `Value::Undef` (e.g. a param with no default).
///   The message names the specific Undef cells.
///
/// - **"operator undefined for these operand kinds"** — all leaf cells are
///   DEFINED but the operator itself cannot produce a result for those value
///   kinds (e.g. a dimension mismatch or operator-not-applicable combination).
///
/// Both branches produce `Satisfaction::Indeterminate` with a
/// `DiagnosticCode::ConstraintIndeterminate` Warning; only the message text
/// differs.
///
/// Characterises the `classify_undef` discriminant in
/// `reify-constraints/src/lib.rs:63–185`.
#[test]
fn row_13_distinct_indeterminacy_messages() {
    // ── Branch 1: undefined inputs ─────────────────────────────────────────────
    // `param x : Scalar<Mass>` has no default → Undef at check time.
    // classify_undef sees the leaf cell as Undef → "undefined inputs".
    {
        let result = check_source_with_stdlib(
            r#"
structure def UndefinedInput {
    param x : Scalar<Mass>
    constraint x > 0
}
"#,
        );
        let entry = result
            .constraint_results
            .iter()
            .find(|e| e.id.entity == "UndefinedInput")
            .expect("must have a constraint entry for UndefinedInput");
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Indeterminate,
            "§10 row 13 (undefined-inputs branch): must be Indeterminate"
        );

        // The ConstraintIndeterminate diagnostic must say "undefined inputs" and name the cell.
        let msg = result
            .diagnostics
            .iter()
            .find(|d| {
                d.code == Some(DiagnosticCode::ConstraintIndeterminate)
                    && d.message.contains("UndefinedInput")
            })
            .map(|d| d.message.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "§10 row 13: expected ConstraintIndeterminate diagnostic for UndefinedInput; \
                     all diagnostics: {:#?}",
                    result.diagnostics
                )
            });
        assert!(
            msg.contains("undefined inputs"),
            "§10 row 13 (branch 1): message must contain 'undefined inputs'; got: {msg}"
        );
    }

    // ── Branch 2: operator undefined (division by zero at runtime) ───────────
    // `param x : Scalar<Length> = 1m` and `param k : Int = 0` are DEFINED.
    // `x / k` → Undef at runtime (division by zero); `x > Undef` → Undef so
    // the constraint is Undef but with DEFINED leaves.
    // classify_undef sees defined leaves → "operator undefined for these operand kinds".
    //
    // NOTE: We use division-by-zero rather than a cross-dimension comparison
    // because α (task 4490) now fires DimensionMismatch at compile time even
    // without stdlib, which would prevent reaching the runtime classify_undef
    // branch via `check_source`.  Division by zero is a purely runtime failure
    // (the compiler cannot statically detect that `k`'s default is 0), so the
    // constraint reaches the evaluator with defined leaves and returns Undef.
    // See `reify_constraints::tests::division_by_zero_no_panic` for the
    // equivalent lower-level characterisation.
    {
        let result = check_source(
            r#"
structure def OperatorUndef {
    param x : Scalar<Length> = 1m
    param k : Int = 0
    constraint x > x / k
}
"#,
        );
        let entry = result
            .constraint_results
            .iter()
            .find(|e| e.id.entity == "OperatorUndef")
            .expect("must have a constraint entry for OperatorUndef");
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Indeterminate,
            "§10 row 13 (operator-undefined branch): must be Indeterminate"
        );

        let msg = result
            .diagnostics
            .iter()
            .find(|d| {
                d.code == Some(DiagnosticCode::ConstraintIndeterminate)
                    && d.message.contains("OperatorUndef")
            })
            .map(|d| d.message.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "§10 row 13: expected ConstraintIndeterminate diagnostic for OperatorUndef; \
                     all diagnostics: {:#?}",
                    result.diagnostics
                )
            });
        assert!(
            msg.contains("operator undefined"),
            "§10 row 13 (branch 2): message must contain 'operator undefined for these operand \
             kinds'; got: {msg}"
        );
        assert!(
            !msg.contains("undefined inputs"),
            "§10 row 13 (branch 2): message must NOT say 'undefined inputs' (both leaves are \
             DEFINED — it's a division-by-zero, not a missing-value); got: {msg}"
        );
    }
}

// ── §10 row 14: RNEA numerically identical post-κ ───────────────────────────

/// §10 row 14: RNEA (`inverse_dynamics_at_snapshot`) is numerically IDENTICAL
/// after κ (task 4494) changed `param inertia` to
/// `param inertia : Matrix<3,3,MomentOfInertia>` — a type-only change that
/// must not perturb the SI values.
///
/// Drives `examples/dynamics/pendulum_idyn.ri` through
/// `parse_and_compile_with_stdlib` + `Engine::build` under `MockGeometryKernel`
/// (kernel-INDEPENDENT: `inverse_dynamics` reads mass from the body's
/// MassProperties solid) and asserts the actuator torque equals
/// `m·g·L·sin(30°) = 1·9.81·0.1·0.5 = 0.4905 N·m` within 1 µN·m.
///
/// Reference value validated at the pure-Rust core by
/// `reify_stdlib::dynamics::rnea::single_pendulum_static_gravity_torque`
/// and end-to-end by `rigid_body_dynamics_e2e.rs::pendulum_idyn_static_gravity_torque_is_0_4905`.
#[test]
fn row_14_rnea_numerically_identical_post_kappa() {
    let source = std::fs::read_to_string(PENDULUM_IDYN_PATH)
        .expect("examples/dynamics/pendulum_idyn.ri must exist (authored prior to task #4495)");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "pendulum_idyn.ri should compile with no error-severity diagnostics post-κ, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: inverse_dynamics reads mass from MassProperties solid.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Extract PendulumIdyn.forces.
    let cell = reify_core::ValueCellId::new("PendulumIdyn", "forces");
    let forces = match result.values.get(&cell) {
        Some(Value::List(f)) => f,
        other => panic!(
            "§10 row 14: PendulumIdyn.forces must be a List<JointForce>, \
             got {other:?}\n(all diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(forces.len(), 1, "§10 row 14: one revolute joint ⇒ one JointForce");

    // Extract ScalarTorque.magnitude.
    let value = field(&forces[0], "JointForce", "value");
    let torque = num(field(value, "ScalarTorque", "magnitude"));

    let expected = 0.4905_f64; // m·g·L·sin(30°) = 1·9.81·0.1·0.5
    let tol = 1e-6_f64; // 1 µN·m
    assert!(
        (torque - expected).abs() < tol,
        "§10 row 14: RNEA torque post-κ must be {expected} N·m ± {tol:.0e}, \
         got {torque:.7} (κ is a type-only change; SI values must be identical)"
    );
}

// ── Field-extraction helpers (row 14) ────────────────────────────────────────

fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("§10 row 14: expected a numeric Value, got {other:?}"),
    }
}

fn field<'a>(v: &'a Value, type_name: &str, member: &str) -> &'a Value {
    match v {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, type_name,
                "§10 row 14: expected a {type_name} instance, got type_name {}",
                data.type_name
            );
            data.fields
                .get(member)
                .unwrap_or_else(|| panic!("§10 row 14: {type_name} missing field `{member}`"))
        }
        other => panic!("§10 row 14: expected a {type_name} StructureInstance, got {other:?}"),
    }
}
