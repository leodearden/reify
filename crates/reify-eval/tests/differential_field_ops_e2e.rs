//! End-to-end integration gate for differential field ops (PRD task θ,
//! docs/prds/v0_6/differential-field-operators.md).
//!
//! This is the G2-bearing integration gate for the whole differential-field
//! batch (tasks α–η). Its distinct value over the per-leaf tests is proving
//! Phase-1 (FEA producer-consumer dataflow) and Phase-2 (generic FD
//! construction-gap closure) coexist and evaluate correctly in ONE runnable
//! artifact through the full parse→compile→eval→ComputeNode pipeline.
//!
//! Each contract is its own test over one memoized evaluation of the example
//! ([`gate_run`]), so one regression cannot mask the others.

use std::sync::OnceLock;

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_expr::EvalContext;
use reify_expr::sampled::sample_at_point;
use reify_ir::{FieldSourceKind, SampledField, Satisfaction, Value, ValueMap};
use reify_test_support::{
    compile_source_with_stdlib, errors_only, make_simple_engine, parse_and_compile_with_stdlib,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load the combined differential-field-ops example.
///
/// Uses `include_str!` so the test binary carries the source at compile time
/// and is always in sync with the user-facing example file (single-source-of-truth
/// pattern, mirroring solve_elastic_static_e2e.rs ↔ fea_cantilever_smoke.ri).
fn diff_field_ops_source() -> &'static str {
    include_str!("../../../examples/differential_field_ops.ri")
}

/// Extract a named field from an ElasticResult value (StructureInstance or Map).
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// A named `Value::Field{Sampled}` channel of an ElasticResult value: its
/// declared domain/codomain plus the backing `SampledField`.
struct SampledChannel {
    domain: Type,
    codomain: Type,
    sf: SampledField,
}

/// Extract a named `Value::Field{Sampled}` channel from an ElasticResult value.
/// Panics if the field is absent, not a Sampled field, or the lambda is not
/// `Value::SampledField`.
fn extract_sampled_channel(result: &Value, field: &str) -> SampledChannel {
    let field_val = extract_field(result, field)
        .unwrap_or_else(|| panic!("field '{}' not found in result", field));
    match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "field '{}' source must be Sampled, got: {:?}",
                field,
                source
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => SampledChannel {
                    domain: domain_type,
                    codomain: codomain_type,
                    sf: sf.clone(),
                },
                other => panic!(
                    "field '{}' lambda must be Value::SampledField, got: {:?}",
                    field, other
                ),
            }
        }
        other => panic!("field '{}' must be Value::Field, got: {:?}", field, other),
    }
}

/// Extract the `SampledField.data` vec from a named `Value::Field{Sampled}` in
/// an ElasticResult value (see [`extract_sampled_channel`]).
fn extract_sampled_field_data(result: &Value, field: &str) -> Vec<f64> {
    extract_sampled_channel(result, field).sf.data
}

// ── shared evaluation ─────────────────────────────────────────────────────────

/// One evaluation of the example, shared by every contract test below.
///
/// `Engine::check` runs `eval` itself and returns its values and diagnostics
/// alongside the constraint results, so one `check` is the whole pipeline. The
/// engine stays inside the initializer; only what the tests read is kept.
struct GateRun {
    compiled: reify_compiler::CompiledModule,
    check: reify_eval::CheckResult,
    compute_targets: Vec<String>,
}

/// The memoized [`GateRun`]: one FEA solve per test binary, shared across
/// libtest's threads.
fn gate_run() -> &'static GateRun {
    static RUN: OnceLock<GateRun> = OnceLock::new();
    RUN.get_or_init(|| {
        let compiled = parse_and_compile_with_stdlib(diff_field_ops_source());
        let mut engine = make_simple_engine();
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        let check = engine.check(&compiled);
        let compute_targets = engine
            .eval_state()
            .expect("eval_state must be Some after check()")
            .snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, data)| data.target.clone())
            .collect();
        GateRun {
            compiled,
            check,
            compute_targets,
        }
    })
}

/// The evaluated `DifferentialFieldOps.<member>` cell.
fn cell(member: &str) -> &'static Value {
    gate_run()
        .check
        .values
        .get(&ValueCellId::new("DifferentialFieldOps", member))
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.{member} not found"))
}

/// The evaluated `DifferentialFieldOps.result` ElasticResult.
fn result_value() -> &'static Value {
    cell("result")
}

/// The evaluated `DifferentialFieldOps.<member>` cell as a number.
fn cell_f64(member: &str) -> f64 {
    let value = cell(member);
    value
        .as_f64()
        .unwrap_or_else(|| panic!("{member} must be numeric, got: {value:?}"))
}

/// Node count of the result's Regular3D grid (displacement is stride-3).
fn n_grid_nodes() -> usize {
    extract_sampled_field_data(result_value(), "displacement").len() / 3
}

fn max_abs(xs: &[f64]) -> f64 {
    xs.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
}

// ── example-level contracts ───────────────────────────────────────────────────

/// Parse, compile, eval and constraint checking of the example emit no
/// Error-severity diagnostic.
#[test]
fn example_evaluates_without_error_diagnostics() {
    let errors: Vec<_> = gate_run()
        .check
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );
}

/// `max` over a field reduces to its codomain's scalar (task #4629), so `dmax`
/// and `g_mag` type as Real and the example's `dmax < 1.0`, `g_mag > 0.0` and
/// `g_mag < 1.0` constraints compare scalars, not Fields. `shear_probe`, the
/// magnitude of a sampled Vector3<Angle>, types as Angle (task #6183).
#[test]
fn field_reductions_and_probes_type_as_declared() {
    let template = gate_run()
        .compiled
        .templates
        .iter()
        .find(|t| t.name == "DifferentialFieldOps")
        .expect("DifferentialFieldOps template must compile");
    let cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| panic!("cell '{member}' must exist in DifferentialFieldOps"))
            .cell_type
            .clone()
    };
    assert_eq!(
        cell_type("dmax"),
        Type::dimensionless_scalar(),
        "dmax = max(result.divergence) must type as Real (dimensionless scalar \
         codomain, task #4629)"
    );
    assert_eq!(
        cell_type("g_mag"),
        Type::dimensionless_scalar(),
        "g_mag = max(result.gradient) must type as Real (Tensor{{Real}} codomain \
         → element quantity, task #4629)"
    );
    assert_eq!(
        cell_type("shear_probe"),
        Type::angle(),
        "shear_probe = magnitude of a sampled Vector3<Angle> must type as Angle (task #6183)"
    );
}

/// The example's `@optimized` FEA call lowers to a `solver::elastic_static`
/// ComputeNode in the snapshot graph: the Phase-1 path.
#[test]
fn fea_lowers_to_elastic_static_compute_node() {
    let compute_targets = &gate_run().compute_targets;
    assert!(
        compute_targets
            .iter()
            .any(|target| target == "solver::elastic_static"),
        "expected a ComputeNode with target==\"solver::elastic_static\" in the graph; \
         found targets: {:?}",
        compute_targets
    );
}

/// Every `constraint` statement in the example evaluates to Satisfied.
#[test]
fn all_eleven_constraints_are_satisfied() {
    let constraint_results = &gate_run().check.constraint_results;
    assert_eq!(
        constraint_results.len(),
        11,
        "expected exactly 11 constraint results (matching the 11 `constraint` \
         statements in differential_field_ops.ri), got {} — a regression may \
         have silently dropped constraint registration or evaluation",
        constraint_results.len()
    );
    for entry in constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Phase 1: FEA derivative channels ──────────────────────────────────────────

/// `result.divergence` is a Sampled `Point3<Length> → Real` field with one
/// finite value per grid node, and it satisfies the cross-field trace identity
/// div[k] = (1−2ν)/E · tr(σ)[k] against `result.stress`, as
/// `crates/reify-eval-fea-tests/tests/solve_elastic_static_e2e.rs::e2e_cantilever_divergence_field_contract`
/// also pins on the same cantilever fixture.
#[test]
fn divergence_is_dimensionless_and_matches_stress_trace() {
    let divergence = extract_sampled_channel(result_value(), "divergence");
    assert_eq!(
        divergence.domain,
        Type::point3(Type::length()),
        "divergence domain must be Point3<Length>"
    );
    assert_eq!(
        divergence.codomain,
        Type::dimensionless_scalar(),
        "divergence codomain must be Real (dimensionless_scalar)"
    );
    let div_data = &divergence.sf.data;
    for (k, &d) in div_data.iter().enumerate() {
        assert!(
            d.is_finite(),
            "divergence data[{}] = {} is not finite",
            k,
            d
        );
    }
    let n_grid_nodes = n_grid_nodes();
    assert_eq!(
        div_data.len(),
        n_grid_nodes,
        "divergence data.len() = {} but displacement grid has {} nodes",
        div_data.len(),
        n_grid_nodes
    );

    // Steel_AISI_1045: E = 205e9 Pa, ν = 0.29. Both fields are recovered and
    // resampled with the same linear weights, so the identity holds to
    // floating-point accumulation.
    let e_pa = 205e9_f64;
    let nu = 0.29_f64;
    let factor = (1.0 - 2.0 * nu) / e_pa;

    let stress_data = extract_sampled_field_data(result_value(), "stress");
    assert_eq!(
        stress_data.len(),
        n_grid_nodes * 9,
        "stress data must have 9 components per grid node"
    );
    for k in 0..n_grid_nodes {
        let tr_sigma = stress_data[9 * k] + stress_data[9 * k + 4] + stress_data[9 * k + 8];
        let expected_div = factor * tr_sigma;
        let got_div = div_data[k];
        // Mixed tolerance: 1e-6 relative + 1e-12 absolute floor.  A pure-
        // relative tolerance with a 1e-18 floor produces an effective 1e-24
        // absolute floor at neutral-axis nodes where expected_div≈0, but
        // got_div (recovered via a different path — divergence of displacement,
        // not from stress) can carry ~1e-15..1e-18 of independent FP noise,
        // which would fail that 1e-24 check.
        assert!(
            (got_div - expected_div).abs() < 1e-6 * expected_div.abs() + 1e-12,
            "trace identity violated at k={}: div={:e}, (1-2ν)/E·tr(σ)={:e}, abs-err={:e}",
            k,
            got_div,
            expected_div,
            (got_div - expected_div).abs(),
        );
    }
    let max_div = max_abs(div_data);
    assert!(
        max_div > 1e-12,
        "max|div| = {:e} is effectively zero — no divergence signal",
        max_div
    );
}

/// `g_mag = max(result.gradient)` is finite, positive under load, and below the
/// small-strain bound of 1 that the rotation and shear degree bounds rest on.
#[test]
fn gradient_magnitude_is_positive_and_small_strain() {
    let g_mag = cell_f64("g_mag");
    assert!(
        g_mag.is_finite() && g_mag > 0.0,
        "g_mag = max(result.gradient) must be finite and > 0 under non-zero load; got {}",
        g_mag
    );
    assert!(
        g_mag < 1.0,
        "g_mag = {} must be < 1.0 (small-strain engineering bound)",
        g_mag
    );
}

// ── Phase 2: generic FD operators ─────────────────────────────────────────────

/// The generic FD operators are exact on polynomials: `lap_max` is
/// max(laplacian(x²)) = 2 and `grad_max` is max(gradient(3x+2)) = 3. Unit tests
/// `laplacian_1d_quadratic_exact` and `gradient_1d_affine_exact` in reify-expr's
/// `sampled_fd.rs` prove both to 1e-12, so 1e-9 here is slack, not a guess.
#[test]
fn phase2_fd_operators_are_exact_on_polynomials() {
    let lap_max = cell_f64("lap_max");
    assert!(
        (lap_max - 2.0).abs() < 1e-9,
        "lap_max = max(laplacian(quadratic)) = {} expected ≈ 2.0 (exact on quadratics, tol=1e-9)",
        lap_max
    );

    let grad_max = cell_f64("grad_max");
    assert!(
        (grad_max - 3.0).abs() < 1e-9,
        "grad_max = max(gradient(linear)) = {} expected ≈ 3.0 (exact on linears, tol=1e-9)",
        grad_max
    );
}

// ── ruling #6164: curl stays dimensionless, rotation is the crossing ─────────

/// HALF 1 of ruling #6164: `result.curl` is dimensionless BY DECISION, not by
/// default. ∇×u is Length/Length, so the derivative algebra stays
/// quotient-pure and `result.curl` stays type-identical to
/// `curl(result.displacement)`; a retype to an angle codomain fails here.
#[test]
fn curl_stays_dimensionless_vector3_real() {
    let curl = extract_sampled_channel(result_value(), "curl");
    assert_eq!(
        curl.domain,
        Type::point3(Type::length()),
        "curl domain must be Point3<Length>"
    );
    assert_eq!(
        curl.codomain,
        Type::vec3(Type::dimensionless_scalar()),
        "curl codomain must be Vector3<Real> — DIMENSIONLESS BY DECISION \
         (ruling #6164 HALF 1). If this assertion is failing because someone \
         retyped curl to Vector3<Angle>, that is a revert of #6164, not a fix: \
         the radian belongs at the `rotation` crossing, not in the derivative \
         algebra."
    );

    let n_grid_nodes = n_grid_nodes();
    assert_eq!(
        curl.sf.data.len(),
        3 * n_grid_nodes,
        "curl data must have 3 components per grid node ({} nodes)",
        n_grid_nodes
    );
    for (k, &c) in curl.sf.data.iter().enumerate() {
        assert!(c.is_finite(), "curl data[{}] = {} is not finite", k, c);
    }
}

/// HALF 2 of ruling #6164: `result.rotation` = ∇×u / 2 is the crossing where
/// the radian enters. Against `result.curl` it has the same domain and grid,
/// exactly half the data, and a DIFFERENT codomain quantity.
#[test]
fn rotation_is_curl_halved_and_typed_angle() {
    let curl = extract_sampled_channel(result_value(), "curl");
    let rotation = extract_sampled_channel(result_value(), "rotation");
    assert_eq!(
        rotation.domain,
        Type::point3(Type::length()),
        "rotation domain must be Point3<Length>"
    );
    assert_eq!(
        rotation.codomain,
        Type::vec3(Type::angle()),
        "rotation codomain must be Vector3<Angle> — the designated crossing \
         (ruling #6164 HALF 2)"
    );
    assert_eq!(
        rotation.sf.data.len(),
        curl.sf.data.len(),
        "rotation must share curl's grid exactly ({} vs {} components)",
        rotation.sf.data.len(),
        curl.sf.data.len()
    );

    // An exactness claim, not a tolerance: IEEE-754 division by 2.0 only
    // decrements the exponent, so it is exact for every normal operand, and
    // halving reaches the subnormal range only below ~1e-308, far under
    // physical strain magnitudes. Do NOT soften this to a tolerance.
    for (i, (&r, &c)) in rotation.sf.data.iter().zip(curl.sf.data.iter()).enumerate() {
        assert_eq!(
            r,
            c / 2.0,
            "rotation[{}] = {:e} must be EXACTLY curl[{}]/2 = {:e} (0 ULP)",
            i,
            r,
            i,
            c / 2.0
        );
    }

    // Whole-field degree bound over the raw SampledField buffer, which .ri
    // cannot reach. It is implied by the example's validated
    // `constraint g_mag < 1.0` (g_mag = max‖∇u‖ < 1), since each component is
    //   |ω_i| = |(∇×u)_i| / 2 = |∂u_j/∂x_k − ∂u_k/∂x_j| / 2 ≤ ‖∇u‖ < 1 rad.
    // A tighter bound would need a measurement recorded here as its basis.
    let max_rot_rad = max_abs(&rotation.sf.data);
    assert!(
        max_rot_rad.is_finite(),
        "max|rotation| must be finite, got {}",
        max_rot_rad
    );
    assert!(
        max_rot_rad > 0.0,
        "max|rotation| = {:e} is effectively zero — no rotation signal under load",
        max_rot_rad
    );
    let max_rot_deg = max_rot_rad.to_degrees();
    assert!(
        max_rot_deg < 1.0_f64.to_degrees(),
        "max|rotation| = {}° must be below the small-strain bound of {}° \
         (= 1 rad), which follows from the validated g_mag < 1.0 via \
         |ω| = |∇×u|/2 ≤ ‖∇u‖",
        max_rot_deg,
        1.0_f64.to_degrees()
    );
}

/// The example's `rot_probe = rotation_probe(sample(result.rotation, ..))`,
/// where `fn rotation_probe(v: Vector3<Angle>) -> Angle`, is a real ANGLE
/// scalar: the radian survives a `Vector3<Angle>` user-function parameter in
/// both directions.
///
/// An out-of-bounds sample returns `Value::Undef`, which passes the
/// no-Error-diagnostics check and would leave the `.ri` pin asserting nothing,
/// so a hollow Undef fails here.
#[test]
fn rot_probe_is_a_finite_angle_scalar() {
    let max_rot_rad = max_abs(&extract_sampled_field_data(result_value(), "rotation"));
    match cell("rot_probe") {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "rot_probe = rotation_probe(sample(result.rotation, ..)) must be \
                 ANGLE-dimensioned — the radian must survive the Vector3<Angle> \
                 call boundary in both directions (ruling #6164)"
            );
            assert!(
                si_value.is_finite() && *si_value > 0.0,
                "rot_probe = {} must be finite and > 0 — a zero or non-finite \
                 value means the sample point fell outside the cantilever bounds \
                 and the .ri pin is hollow",
                si_value
            );
            // The probe is magnitude() of ONE sampled node's rotation vector,
            // so it cannot exceed sqrt(3) × the grid's max |component|.
            assert!(
                *si_value <= max_rot_rad * 3.0_f64.sqrt() + 1e-12,
                "rot_probe = {} exceeds sqrt(3)·max|rotation component| = {} — \
                 the probe is not sampling the same field",
                si_value,
                max_rot_rad * 3.0_f64.sqrt()
            );
        }
        Value::Undef => panic!(
            "rot_probe is Value::Undef — the sample point is outside the \
             cantilever bounds, which silently hollows out the .ri crossing pin"
        ),
        other => panic!("rot_probe must be a Scalar[ANGLE], got: {:?}", other),
    }
}

// ── task #6183 σ: the `shear_angles` channel ──────────────────────────────────

/// `result.shear_angles` holds the Voigt engineering shears (γ_yz, γ_zx, γ_xy),
/// a named Vector3<Angle> crossing that is exactly 2 × the symmetric
/// off-diagonals of `result.gradient`, which itself stays Tensor<2,3,Real>
/// (INV-AD-3). The example's `shear_probe` cell is
/// `magnitude(sample(result.shear_angles, point3(500mm, 50mm, 50mm)))`.
#[test]
fn shear_angles_project_gradient_as_angle() {
    let shear = extract_sampled_channel(result_value(), "shear_angles");
    let grad = extract_sampled_channel(result_value(), "gradient");
    assert_eq!(
        shear.domain,
        Type::point3(Type::length()),
        "shear_angles domain must be Point3<Length>"
    );
    assert_eq!(
        shear.codomain,
        Type::vec3(Type::angle()),
        "shear_angles codomain must be Vector3<Angle> — a named crossing (task #6183)"
    );
    assert_eq!(
        grad.codomain,
        Type::tensor(2, 3, Type::dimensionless_scalar()),
        "gradient codomain must STAY Tensor<2,3,Real> — INV-AD-3: never retype the \
         tensor; angle readings are extracted by named channels like shear_angles"
    );

    let g = &grad.sf.data;
    let gamma = &shear.sf.data;
    assert_eq!(g.len() % 9, 0, "gradient must be stride-9");
    let n_nodes = g.len() / 9;
    assert_eq!(
        gamma.len(),
        3 * n_nodes,
        "shear_angles must carry 3 components per gradient node ({n_nodes} nodes)"
    );
    for (k, &c) in gamma.iter().enumerate() {
        assert!(c.is_finite(), "shear_angles data[{k}] = {c} is not finite");
    }

    // γ = 2·ε_offdiag at 0 ULP: γ_ij and ε_ij·2 are the same IEEE addition of
    // the same operands (commutative bitwise), and ×0.5 / ×2 are exact for
    // normal operands. Voigt order: component c pairs with (i, j).
    const VOIGT_PAIRS: [(usize, usize); 3] = [(1, 2), (2, 0), (0, 1)];
    for k in 0..n_nodes {
        for (c, &(i, j)) in VOIGT_PAIRS.iter().enumerate() {
            let eps_ij = 0.5 * (g[9 * k + 3 * i + j] + g[9 * k + 3 * j + i]);
            assert_eq!(
                gamma[3 * k + c],
                2.0 * eps_ij,
                "shear_angles[{k}][{c}] must be EXACTLY 2·ε_{i}{j} of result.gradient (0 ULP)"
            );
        }
    }

    // Whole-field degree bound, RIGOROUSLY implied by the validated
    // `constraint g_mag < 1.0` (g_mag = max‖∇u‖_F over the same slab):
    //   |γ_ij| = |g_ij + g_ji| ≤ √2·√(g_ij² + g_ji²) ≤ √2·‖∇u‖_F < √2 rad.
    let max_gamma_rad = max_abs(gamma);
    assert!(
        max_gamma_rad > 0.0,
        "max|shear_angles| is zero — no shear signal under load"
    );
    assert!(
        max_gamma_rad.to_degrees() < 2f64.sqrt().to_degrees(),
        "max|shear_angles| = {}° must be below √2 rad = {}°, implied by g_mag < 1.0",
        max_gamma_rad.to_degrees(),
        2f64.sqrt().to_degrees()
    );

    // Harness-side sampling through the production sampler at the example's
    // probe point. Interpolation is linear, so it commutes with the linear
    // projection up to rounding (≤~16 ULP of the operands).
    let length = |m: f64| Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    };
    let probe = Value::Point(vec![length(0.5), length(0.05), length(0.05)]);
    let empty = ValueMap::new();
    let ctx = EvalContext::simple(&empty);
    let sampled_shear = sample_at_point(&shear.sf, &probe, &shear.codomain, &ctx);
    let sampled_grad = sample_at_point(&grad.sf, &probe, &grad.codomain, &ctx);
    let Value::Vector(shear_components) = &sampled_shear else {
        panic!("sampled shear_angles must be a Value::Vector, got {sampled_shear:?}")
    };
    let Value::Vector(grad_components) = &sampled_grad else {
        panic!("sampled gradient must be a Value::Vector, got {sampled_grad:?}")
    };
    assert_eq!(shear_components.len(), 3, "Voigt shear arity");
    assert_eq!(grad_components.len(), 9, "gradient arity");
    let grad_at = |r: usize, c: usize| {
        grad_components[3 * r + c]
            .as_f64()
            .unwrap_or_else(|| panic!("sampled gradient[{r}][{c}] must be numeric"))
    };
    let mut sampled_gamma_rad = [0.0_f64; 3];
    for (c, &(i, j)) in VOIGT_PAIRS.iter().enumerate() {
        let Value::Scalar {
            si_value,
            dimension,
        } = &shear_components[c]
        else {
            panic!(
                "sampled shear component {c} must be a Scalar, got {:?}",
                shear_components[c]
            )
        };
        assert_eq!(
            *dimension,
            DimensionVector::ANGLE,
            "sampled shear component {c} must be ANGLE-dimensioned"
        );
        let (g_ij, g_ji) = (grad_at(i, j), grad_at(j, i));
        assert!(
            (si_value - (g_ij + g_ji)).abs() <= 1e-12 * (g_ij.abs() + g_ji.abs()) + 1e-18,
            "sampled γ component {c} = {si_value:e} must equal sampled g_{i}{j} + g_{j}{i} = {:e}",
            g_ij + g_ji
        );
        sampled_gamma_rad[c] = *si_value;
    }

    // The .ri probe cell: a real ANGLE scalar equal to the harness magnitude.
    let expected_probe = sampled_gamma_rad.iter().map(|c| c * c).sum::<f64>().sqrt();
    match cell("shear_probe") {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "shear_probe must be ANGLE-dimensioned (task #6183)"
            );
            assert!(
                si_value.is_finite() && *si_value > 0.0,
                "shear_probe = {si_value} must be finite and > 0"
            );
            assert!(
                (si_value - expected_probe).abs() <= 1e-12 * expected_probe,
                "shear_probe = {si_value:e} must equal the harness-sampled ‖γ‖ = {expected_probe:e}"
            );
        }
        Value::Undef => panic!(
            "shear_probe is Value::Undef — the sample point is outside the cantilever \
             bounds, which silently hollows out the .ri shear-limit pin"
        ),
        other => panic!("shear_probe must be a Scalar[ANGLE], got: {other:?}"),
    }
}

/// Non-vacuity control for the example's shear-limit fn: its `Vector3<Angle>`
/// parameter must REJECT wrong-quantity sampled vectors, so the typed pass of
/// `sample(result.shear_angles, ..)` is a real check. Compile-only.
#[test]
fn shear_limit_fn_rejects_non_angle_sampled_vectors() {
    let source = diff_field_ops_source();
    let needle = "sample(result.shear_angles,";
    assert_eq!(
        source.matches(needle).count(),
        1,
        "precondition: the example must sample result.shear_angles exactly once"
    );
    // curl is Vector3<Real> (bare dimensionless); displacement is Vector3<Length>.
    for wrong in ["result.curl", "result.displacement"] {
        let mutated = source.replace(needle, &format!("sample({wrong},"));
        let compiled = compile_source_with_stdlib(&mutated);
        let errors = errors_only(&compiled);
        assert!(
            errors.iter().any(|d| d.message.contains("shear_magnitude")),
            "passing sample({wrong}, ..) to shear_magnitude(Vector3<Angle>) must be \
             rejected with a diagnostic naming the fn; got errors: {errors:?}"
        );
    }
}
