//! End-to-end integration gate for differential field ops (PRD task θ,
//! docs/prds/v0_6/differential-field-operators.md).
//!
//! This is the G2-bearing integration gate for the whole differential-field
//! batch (tasks α–η). Its distinct value over the per-leaf tests is proving
//! Phase-1 (FEA producer-consumer dataflow) and Phase-2 (generic FD
//! construction-gap closure) coexist and evaluate correctly in ONE runnable
//! artifact through the full parse→compile→eval→ComputeNode pipeline.
//!
//! RED: fails to COMPILE until step-2 creates
//!   `examples/differential_field_ops.ri`  (include_str! compile error).
//! GREEN: after step-2 the test binary compiles and all assertions pass.

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_ir::{FieldSourceKind, Satisfaction, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

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

/// Extract the `SampledField.data` vec from a named `Value::Field{Sampled}` in
/// an ElasticResult value.  Panics if the field is absent, not a Sampled field,
/// or the lambda is not `Value::SampledField`.
fn extract_sampled_field_data(result: &Value, field: &str) -> Vec<f64> {
    let field_val = extract_field(result, field)
        .unwrap_or_else(|| panic!("field '{}' not found in result", field));
    match &field_val {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "field '{}' source must be Sampled, got: {:?}",
                field,
                source
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => sf.data.clone(),
                other => panic!(
                    "field '{}' lambda must be Value::SampledField, got: {:?}",
                    field, other
                ),
            }
        }
        other => panic!("field '{}' must be Value::Field, got: {:?}", field, other),
    }
}

// ── integration gate ──────────────────────────────────────────────────────────

/// Full end-to-end integration gate for the differential-field-ops batch.
///
/// Asserts (in order):
///   (a) No Error-severity diagnostics — parse + compile + eval are clean.
///   (b) A ComputeNode with `target == "solver::elastic_static"` exists in the
///       snapshot graph (Phase-1 FEA path lowered via `@optimized`).
///   (c) All `.ri` `constraint` statements evaluate to `Satisfaction::Satisfied`
///       via `engine.check`.
///   (d) PHASE 1 — `result.divergence` is a `Value::Field{source:Sampled}`
///       with `codomain_type == Type::dimensionless_scalar()` and all-finite
///       data; the exact cross-field trace identity
///         div[k] = (1−2ν)/E · tr(σ)[k]
///       holds to rel-tol 1e-6 (proven-GREEN on the same cantilever fixture in
///       α/solve_elastic_static_e2e.rs:814-875; reused verbatim here).
///       Also asserts `DifferentialFieldOps.g_mag` is finite and > 0 (γ
///       magnitude signal, non-trivial under load) and < 1 (small-strain bound).
///   (d2) PHASE 1 / HALF 1 of ruling #6164 — `result.curl` is a
///       `Value::Field{source:Sampled}` with `domain == Point3<Length>`,
///       `codomain == Vector3<Real>` (DIMENSIONLESS — decided, not defaulted),
///       all-finite data, and `len() == 3 * n_grid_nodes`.
///   (d3) PHASE 1 / HALF 2 of ruling #6164 — `result.rotation` is a
///       `Value::Field{source:Sampled}` with `codomain == Vector3<Angle>` and
///       `rotation[i] == curl[i] / 2` bit-exactly (0 ULP), plus the harness-side
///       degree comparison against the small-strain bound implied by the
///       already-validated `constraint g_mag < 1.0`.
///   (e) PHASE 2 — `DifferentialFieldOps.lap_max` ≈ 2.0 within 1e-9
///       (max of laplacian(f) where f(x)=x²; exact on quadratics).
///       `DifferentialFieldOps.grad_max` ≈ 3.0 within 1e-9
///       (max of gradient(g) where g(x)=3x+2; exact on linears).
///       Proven by δ's laplacian_1d_quadratic_exact (1e-12 on 5-node grid).
#[test]
fn differential_field_ops_integration_gate() {
    let source = diff_field_ops_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (a) No Error-severity diagnostics ────────────────────────────────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // ── (a2) Compile-time type pins: dmax and g_mag must type as Real ────────
    //
    // After W1 (max(field)→codomain scalar, task #4629), `max(result.divergence)`
    // types as Real (divergence codomain is dimensionless Scalar → scalar_or_real →
    // Real) and `max(result.gradient)` types as Real (Tensor{Real} codomain →
    // element quantity Real).  These pins confirm the W1 typing is correct BEFORE
    // W3 (step-6) removes the Field/StructureRef comparison guard deferral, so the
    // constraints `dmax < 1.0`, `g_mag > 0.0`, `g_mag < 1.0` compare Real scalars
    // (not Fields) and remain compile-clean after the deferral is lifted.
    let diff_tmpl = compiled
        .templates
        .iter()
        .find(|t| t.name == "DifferentialFieldOps")
        .expect("DifferentialFieldOps template must compile");

    let dmax_type = diff_tmpl
        .value_cells
        .iter()
        .find(|c| c.id.member == "dmax")
        .expect("cell 'dmax' must exist in DifferentialFieldOps")
        .cell_type
        .clone();
    assert_eq!(
        dmax_type,
        Type::dimensionless_scalar(),
        "dmax = max(result.divergence) must type as Real after W1 field-reduction typing \
         (task #4629); got {:?}",
        dmax_type
    );

    let g_mag_type = diff_tmpl
        .value_cells
        .iter()
        .find(|c| c.id.member == "g_mag")
        .expect("cell 'g_mag' must exist in DifferentialFieldOps")
        .cell_type
        .clone();
    assert_eq!(
        g_mag_type,
        Type::dimensionless_scalar(),
        "g_mag = max(result.gradient) must type as Real after W1 field-reduction typing \
         (Tensor{{Real}} codomain → element quantity → Real, task #4629); got {:?}",
        g_mag_type
    );

    // ── (b) ComputeNode with target == "solver::elastic_static" ──────────────
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::elastic_static");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"solver::elastic_static\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // ── (c) All constraint_results == Satisfied ───────────────────────────────
    // Re-evaluate via engine.check to obtain constraint satisfaction results.
    // engine.check() triggers a second evaluation pass because the constraint
    // satisfaction layer requires a fresh traversal with check-mode semantics
    // separate from the value-extraction eval above (engine.eval builds
    // eval_state / the snapshot graph; engine.check drives the
    // constraint-checker overlay).  The double-solve cost is accepted here
    // because both passes are required by the API contract: neither returns
    // the other's results.
    let check_result = engine.check(&compiled);
    assert_eq!(
        check_result.constraint_results.len(),
        10,
        "expected exactly 10 constraint results (matching the 10 `constraint` \
         statements in differential_field_ops.ri), got {} — a regression may \
         have silently dropped constraint registration or evaluation",
        check_result.constraint_results.len()
    );
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }

    // ── (d) Phase 1 — divergence field contract + trace identity ─────────────

    let result_cell = ValueCellId::new("DifferentialFieldOps", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.result not found"));

    // Divergence must be Value::Field{source:Sampled, codomain:Real}
    let div_val = extract_field(result_val, "divergence")
        .unwrap_or_else(|| panic!("field 'divergence' not found in DifferentialFieldOps.result"));

    let (div_domain, div_codomain) = match &div_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            ..
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "divergence source must be Sampled, got: {:?}",
                source
            );
            (domain_type.clone(), codomain_type.clone())
        }
        other => panic!(
            "DifferentialFieldOps.result.divergence must be Value::Field, got: {:?}",
            other
        ),
    };
    assert_eq!(
        div_domain,
        Type::point3(Type::length()),
        "divergence domain must be Point3<Length>"
    );
    assert_eq!(
        div_codomain,
        Type::dimensionless_scalar(),
        "divergence codomain must be Real (dimensionless_scalar)"
    );

    // All data finite
    let div_data = extract_sampled_field_data(result_val, "divergence");
    for (k, &d) in div_data.iter().enumerate() {
        assert!(
            d.is_finite(),
            "divergence data[{}] = {} is not finite",
            k,
            d
        );
    }

    let disp_data = extract_sampled_field_data(result_val, "displacement");
    let n_grid_nodes = disp_data.len() / 3;
    assert_eq!(
        div_data.len(),
        n_grid_nodes,
        "divergence data.len() = {} but displacement grid has {} nodes",
        div_data.len(),
        n_grid_nodes
    );

    // ── Exact cross-field trace identity: div[k] = (1−2ν)/E · tr(σ)[k] ──────
    //
    // Steel_AISI_1045: E = 205e9 Pa, ν = 0.29.
    // Both fields recovered and resampled with the same linear weights →
    // identity holds to floating-point accumulation (rel-tol 1e-6 generous).
    // Proven GREEN on the same cantilever fixture in solve_elastic_static_e2e.rs:814-875.
    // Reused verbatim here as the Phase-1 engineering-quantity signal (PRD §θ).
    let e_pa = 205e9_f64;
    let nu = 0.29_f64;
    let factor = (1.0 - 2.0 * nu) / e_pa;

    let stress_data = extract_sampled_field_data(result_val, "stress");
    assert_eq!(
        stress_data.len(),
        n_grid_nodes * 9,
        "stress data must have 9 components per grid node"
    );

    let mut max_div = 0.0_f64;
    let mut max_tr_sigma = 0.0_f64;
    for k in 0..n_grid_nodes {
        let tr_sigma = stress_data[9 * k] + stress_data[9 * k + 4] + stress_data[9 * k + 8];
        let expected_div = factor * tr_sigma;
        let got_div = div_data[k];
        // Mixed tolerance: 1e-6 relative + 1e-12 absolute floor.  A pure-
        // relative tolerance with a 1e-18 floor produces an effective 1e-24
        // absolute floor at neutral-axis nodes where expected_div≈0, but
        // got_div (recovered via a different path — divergence of displacement,
        // not from stress) can carry ~1e-15..1e-18 of independent FP noise,
        // which would fail that 1e-24 check.  Proven GREEN today; the floor
        // makes it robust to future mesh/grid changes.
        assert!(
            (got_div - expected_div).abs() < 1e-6 * expected_div.abs() + 1e-12,
            "trace identity violated at k={}: div={:e}, (1-2ν)/E·tr(σ)={:e}, abs-err={:e}",
            k,
            got_div,
            expected_div,
            (got_div - expected_div).abs(),
        );
        if got_div.abs() > max_div {
            max_div = got_div.abs();
        }
        if tr_sigma.abs() > max_tr_sigma {
            max_tr_sigma = tr_sigma.abs();
        }
    }
    assert!(
        max_div > 1e-12,
        "max|div| = {:e} is effectively zero — no divergence signal",
        max_div
    );

    // ── g_mag = max(result.gradient): finite, >0 under load, <1 small-strain ─
    let g_mag_cell = ValueCellId::new("DifferentialFieldOps", "g_mag");
    let g_mag_val = eval_result
        .values
        .get(&g_mag_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.g_mag not found"));
    let g_mag = g_mag_val
        .as_f64()
        .unwrap_or_else(|| panic!("g_mag must be numeric, got: {:?}", g_mag_val));
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

    // ── (d2) HALF 1 of ruling #6164 — `result.curl` stays DIMENSIONLESS ──────
    //
    // This pin closes a verified gap: before #6164 there were ZERO curl pins in
    // this gate.  It characterizes already-shipped behaviour and is GREEN on
    // arrival — its value is that a future retype of `ElasticResult.curl` to an
    // angle-typed codomain now fails LOUDLY here.  That is HALF 1's whole point:
    // curl is dimensionless BY DECISION, not by default.  ∇×u is Length/Length,
    // the derivative algebra stays quotient-pure, and `result.curl` must remain
    // type-identical to `curl(result.displacement)`.
    let curl_val = extract_field(result_val, "curl")
        .unwrap_or_else(|| panic!("field 'curl' not found in DifferentialFieldOps.result"));
    let (curl_domain, curl_codomain) = match &curl_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            ..
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "curl source must be Sampled, got: {:?}",
                source
            );
            (domain_type.clone(), codomain_type.clone())
        }
        other => panic!(
            "DifferentialFieldOps.result.curl must be Value::Field, got: {:?}",
            other
        ),
    };
    assert_eq!(
        curl_domain,
        Type::point3(Type::length()),
        "curl domain must be Point3<Length>"
    );
    assert_eq!(
        curl_codomain,
        Type::vec3(Type::dimensionless_scalar()),
        "curl codomain must be Vector3<Real> — DIMENSIONLESS BY DECISION \
         (ruling #6164 HALF 1). If this assertion is failing because someone \
         retyped curl to Vector3<Angle>, that is a revert of #6164, not a fix: \
         the radian belongs at the `rotation` crossing below, not in the \
         derivative algebra."
    );

    let curl_data = extract_sampled_field_data(result_val, "curl");
    assert_eq!(
        curl_data.len(),
        3 * n_grid_nodes,
        "curl data must have 3 components per grid node ({} nodes)",
        n_grid_nodes
    );
    for (k, &c) in curl_data.iter().enumerate() {
        assert!(c.is_finite(), "curl data[{}] = {} is not finite", k, c);
    }

    // ── (d3) HALF 2 of ruling #6164 — `result.rotation` IS the crossing ──────
    //
    // `rotation` = ∇×u / 2 is the designated crossing where the radian enters
    // explicitly.  Note the asymmetry against (d2) directly above: same domain,
    // same grid, same node count, componentwise exactly half the data — and a
    // DIFFERENT codomain quantity.  That contrast is the entire ruling.
    let rot_val = extract_field(result_val, "rotation")
        .unwrap_or_else(|| panic!("field 'rotation' not found in DifferentialFieldOps.result"));
    let (rot_domain, rot_codomain) = match &rot_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            ..
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "rotation source must be Sampled, got: {:?}",
                source
            );
            (domain_type.clone(), codomain_type.clone())
        }
        other => panic!(
            "DifferentialFieldOps.result.rotation must be Value::Field, got: {:?}",
            other
        ),
    };
    assert_eq!(
        rot_domain,
        Type::point3(Type::length()),
        "rotation domain must be Point3<Length>"
    );
    assert_eq!(
        rot_codomain,
        Type::vec3(Type::angle()),
        "rotation codomain must be Vector3<Angle> — the designated crossing \
         (ruling #6164 HALF 2)"
    );

    let rot_data = extract_sampled_field_data(result_val, "rotation");
    assert_eq!(
        rot_data.len(),
        curl_data.len(),
        "rotation must share curl's grid exactly ({} vs {} components)",
        rot_data.len(),
        curl_data.len()
    );

    // BIT-EXACT ×½ identity, asserted at 0 ULP.
    //
    // G6 numeric-premise discipline (the example file states this convention
    // itself, above its `g_mag` bound): this is not a guessed tolerance, it is
    // an exactness claim.  IEEE-754 division by 2.0 only decrements the
    // exponent, so it is exact for every normal operand; subnormal
    // underflow is unreachable at
    // physical strain magnitudes (|∇×u| here is ~1e-3, and halving reaches the
    // subnormal range only below ~1e-308).  Do NOT soften this to a tolerance.
    for (i, (&r, &c)) in rot_data.iter().zip(curl_data.iter()).enumerate() {
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

    // Whole-FIELD degree comparison.  It lives here because it reduces over the
    // raw SampledField buffer, which .ri cannot reach — NOT for want of a
    // degree comparison in-language.  The example now also carries
    // `constraint rot_probe > 0.001deg` / `< 0.5deg` at its single probe point:
    // `magnitude` of a sampled Vector3<Angle> is an Angle SCALAR, and a
    // scalar/`deg`-literal comparison type-checks and evaluates.  What remains
    // dead is Vector3 COMPONENT access (`.x`, `v[0]` and `norm(v) < 5deg` were
    // all probed dead), which a per-component in-language assertion would need.
    //
    // The bound is NOT invented.  It is RIGOROUSLY IMPLIED by the example's
    // already-validated `constraint g_mag < 1.0`, asserted directly above:
    // g_mag = max‖∇u‖ < 1, and each rotation component is
    //   |ω_i| = |(∇×u)_i| / 2 = |∂u_j/∂x_k − ∂u_k/∂x_j| / 2 ≤ ‖∇u‖ < 1 rad.
    // 1 rad ≈ 57.29578°, so the assertion compares against
    // `1.0_f64.to_degrees()` rather than a hand-picked degree constant.
    //
    // A TIGHTER bound would need a MEASUREMENT, and per the example's own G6
    // rule the measured value would have to be recorded here as its basis.
    // None is asserted, so none is claimed.
    let max_rot_rad = rot_data.iter().fold(0.0_f64, |m, r| m.max(r.abs()));
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

    // ── (d4) The .ri-side crossing signal is REAL, not silently Undef ────────
    //
    // `examples/differential_field_ops.ri` binds
    //   let rot_probe = rotation_probe(sample(result.rotation, point3(500mm, 50mm, 50mm)))
    // where `fn rotation_probe(v: Vector3<Angle>) -> Angle`.  That call boundary
    // is the whole user-observable signal: it proves `Vector3<Angle>` is accepted
    // as a user-function PARAMETER type, the one position in the capability chain
    // no existing stdlib or example code exercises.
    //
    // This assertion is load-bearing rather than decorative: an out-of-bounds
    // sample returns `Value::Undef` (`sample_at_point`'s bounds-reject path), and a
    // hollow Undef would sail past the no-Error-diagnostics check at (a) above,
    // leaving the .ri pin asserting nothing.  Pinning the ANGLE dimension here
    // also confirms the rad tag survives the round trip out through the call
    // boundary, not just into it.
    let rot_probe_cell = ValueCellId::new("DifferentialFieldOps", "rot_probe");
    let rot_probe_val = eval_result
        .values
        .get(&rot_probe_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.rot_probe not found"));
    match rot_probe_val {
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
            // Cross-check against the field data read directly above: the probe
            // is magnitude() of ONE sampled node's rotation vector, so it cannot
            // exceed the max |component| times sqrt(3) over the whole grid.
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

    // No `orient_exp(rotation)` assertion sits beside (d3)/(d4) on purpose:
    // `orient_exp`'s dimension gate in `reify-stdlib`'s `orientation` module
    // still returns `Value::Undef` for an ANGLE argument until ruling #6080
    // lands, so such an assertion could only pin `Undef`. #6080 owns that gate
    // and this channel needs no edit when it lands.

    // ── (e) Phase 2 — exact polynomial fixture assertions ────────────────────
    //
    // laplacian_1d_quadratic_exact (sampled_fd.rs) proves max(laplacian(x²)) = 2.0
    // to 1e-12 on a 5-node Regular1D grid with spacing 1.0; tolerance here is 1e-9.
    // gradient_1d_affine_exact proves the first-difference is exact on linears;
    // max(gradient(3x+2)) = 3.0 to 1e-12; tolerance here is 1e-9.

    let lap_max_cell = ValueCellId::new("DifferentialFieldOps", "lap_max");
    let lap_max_val = eval_result
        .values
        .get(&lap_max_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.lap_max not found"));
    let lap_max = lap_max_val
        .as_f64()
        .unwrap_or_else(|| panic!("lap_max must be numeric, got: {:?}", lap_max_val));
    assert!(
        (lap_max - 2.0).abs() < 1e-9,
        "lap_max = max(laplacian(quadratic)) = {} expected ≈ 2.0 (exact on quadratics, tol=1e-9)",
        lap_max
    );

    let grad_max_cell = ValueCellId::new("DifferentialFieldOps", "grad_max");
    let grad_max_val = eval_result
        .values
        .get(&grad_max_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.grad_max not found"));
    let grad_max = grad_max_val
        .as_f64()
        .unwrap_or_else(|| panic!("grad_max must be numeric, got: {:?}", grad_max_val));
    assert!(
        (grad_max - 3.0).abs() < 1e-9,
        "grad_max = max(gradient(linear)) = {} expected ≈ 3.0 (exact on linears, tol=1e-9)",
        grad_max
    );
}
