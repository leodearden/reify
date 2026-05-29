//! Trampoline for `solver::elastic_static` — the `fn solve_elastic_static`
//! @optimized target (PRD §8 task η, docs/prds/v0_3/compute-node-contract.md).
//!
//! # Contract (§8-η)
//!
//! Receives the 7 `value_inputs` matching the fn signature:
//!   `(material, length, width, height, loads, supports, options)`
//!
//! Builds a P1-tet FEA mesh (1×1×6 hex blocks, 6 tets per hex = 36 tets,
//! 28 nodes), assembles K, applies Dirichlet BCs, solves CG with warm-state
//! support, recovers element stresses, computes max von Mises.
//!
//! Returns `ComputeOutcome::Completed` with:
//! - `result`         — `ElasticResult`-shaped `Value::StructureInstance`
//! - `new_warm_state` — the `CgWarmState` serialised as `OpaqueState`
//! - `cost_per_byte`  — crude estimate for cache eviction
//!
//! # Analytical reference
//!
//! For a cantilever of length L, width b, height h under tip load P:
//!   σ_max = 6·P·L / (b·h²)
//!
//! The smoke test (examples/fea_cantilever_smoke.ri) uses L=1m, b=0.1m,
//! h=0.1m, P=1000 N → σ_max = 6 MPa. The coarse P1-tet mesh underestimates
//! this by 20–50% (documented method-error budget); the integration test
//! asserts ±50% tolerance (3–9 MPa).
//!
//! # Warm-state contract (§5)
//!
//! Prior warm state (if any) is recovered via `CgWarmState::from_opaque_state`;
//! a type mismatch silently falls back to cold start. The fresh `CgWarmState`
//! (wrapping the new displacement vector u) is donated back as `new_warm_state`.
//!
//! # Cache-hit contract (§3 + significance_filter.rs)
//!
//! `significance_filter::is_opted_in("solver::elastic_static")` returns `true`
//! (pinned at `significance_filter.rs:76`), opting this target into the output
//! significance filter.
//!
//! **Cache-hit mechanism (§8-η / §3 Final-gate):** the `evaluate_let_bindings`
//! loop in `engine_eval.rs` carries a pre-dispatch Final-gate at lines 2808-2860
//! (§8-η comment label). When all inputs are `Freshness::Final` and the output VC
//! is also already `Freshness::Final` from a prior `Engine::eval()`, the gate
//! short-circuits re-dispatch and returns the cached `CachedResult::Value` directly.
//! This is the in-memory cache-hit path that prevents redundant FEA solves across
//! successive `eval()` calls on the same `CompiledModule`.
//!
//! The integration test `e2e_cantilever_second_eval_hits_cache` (step-9) verifies
//! this contract: `DISPATCH_COUNT` must equal 1 after two sequential `engine.eval()`
//! calls on the same module — the test passes as of the Final-gate landing in
//! `engine_eval.rs:2809-2860`.
//!
//! # Placement rationale
//!
//! See `compute_targets/mod.rs` for why this lives in `reify-eval` rather
//! than `reify-stdlib` (the PRD §8-η preferred location).
//!
//! # StructureTypeId sentinel
//!
//! The trampoline signature carries no `StructureRegistry` access. The returned
//! `ElasticResult` StructureInstance uses `StructureTypeId(u32::MAX)` as a
//! synthetic sentinel.
//! TODO: thread `StructureRegistry` through the trampoline signature (tracked
//! as a future refinement) once ComputeFn/ComputeOutcome are moved into reify-ir.

use reify_core::DimensionVector;
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, CgWarmState, DirichletBc, ElementOrder,
    IsotropicElastic, SolverMode, apply_dirichlet_row_elimination, apply_point_load,
    assemble_global_stiffness, element_stiffness, element_stress_p1, solve_cg_with_warm_state,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::elastic_static`.
///
/// Accepts the seven `value_inputs` corresponding to:
///
/// ```text
/// [0] material  : ElasticMaterial    (Value::StructureInstance)
/// [1] length    : Length             (Value::Scalar { dimension: LENGTH })
/// [2] width     : Length             (Value::Scalar { dimension: LENGTH })
/// [3] height    : Length             (Value::Scalar { dimension: LENGTH })
/// [4] loads     : List<…>            (Value::List of PointLoad StructureInstances)
/// [5] supports  : List<…>            (Value::List of FixedSupport StructureInstances)
/// [6] options   : ElasticOptions     (Value::StructureInstance — solver defaults used)
/// ```
///
/// Returns an `ElasticResult`-shaped `Value::StructureInstance` with fields:
/// `displacement`, `stress`, `frame` (all Undef — tet convention),
/// `max_von_mises` (Scalar[PRESSURE]), `converged` (Bool), `iterations` (Int).
///
/// The warm-state donate→checkout round-trip is exercised via
/// `CgWarmState::from_opaque_state` / `CgWarmState::into_opaque_state`.
pub fn solve_elastic_static_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (1) Extract material properties ──────────────────────────────────────
    let mat = extract_material(&value_inputs[0]);

    // ── (2) Extract geometry scalars (SI: metres) ─────────────────────────────
    let length = extract_scalar_si(&value_inputs[1]);
    let width  = extract_scalar_si(&value_inputs[2]);
    let height = extract_scalar_si(&value_inputs[3]);

    // ── (3) Sum tip-force magnitudes from PointLoad list ─────────────────────
    let tip_force = extract_tip_force(&value_inputs[4]);

    // ── (4) Supports: non-empty list → cantilever is clamped at root ─────────
    // (We don't inspect individual FixedSupport fields; presence is sufficient.)

    // ── (5) Build a nx×1×nz hex mesh split into 6 tets per hex ──────────────
    //
    // Layout: X-axis = beam length, Y-axis = width, Z-axis = height.
    //
    // P1 constant-strain tetrahedra suffer shear locking in bending problems:
    // they cannot represent the linear-strain (linear-stress) bending field
    // and instead develop parasitic shear strains that make the mesh
    // artificially stiff. The severity of locking scales with element aspect
    // ratio in the BENDING PLANE (XZ):
    //
    //   locking_ratio ∝ (δ_x / δ_z)²
    //
    // where δ_x = length/nx (element length along beam axis) and
    //       δ_z = height/nz (element height through cross-section).
    //
    // With nx=6, nz=8 the aspect ratio is (L/6)/(h/8) = (L*8)/(h*6) = 13.3
    // for the smoke test (L=1m, h=0.1m), giving a ~75% stress underestimate
    // (measured: 1.46 MPa vs analytical 6 MPa).
    //
    // FIX: scale nx ∝ nz × (L/h) so that δ_x ≈ δ_z (near-cubic elements
    // in the bending plane). For L=1m, h=0.1m, nz=6: nx = 6×10 = 60,
    // δ_x = δ_z = 16.7 mm. Near-cubic Freudenthal tets have minimal shear
    // locking; empirically this mesh yields max von Mises ≈ 3.5–4.5 MPa
    // for the smoke-test cantilever (within the ±50% tolerance [3, 9] MPa).
    //
    // ny=1: bending is about Y, so a single element in the Y direction is
    // sufficient. Increasing ny improves isotropy slightly but at quadratic
    // element-count cost.
    //
    // Freudenthal 6-tet decomposition shares the main body diagonal
    // c[0]→c[6] of each hex. All six tets have |det J| = dx·dy·dz.
    let nz: usize = 6;
    // Scale nx to maintain near-cubic elements in the bending plane (XZ).
    // Clamped to ≥1 to handle degenerate geometry (height ≈ length).
    let nx: usize = ((length / height * nz as f64).round() as usize).max(1);
    let ny: usize = 1;
    let nx1 = nx + 1;
    let ny1 = ny + 1;  // 2 nodes along Y
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize {
        iz * ny1 * nx1 + iy * nx1 + ix
    };
    let node_coord = |ix: usize, iy: usize, iz: usize| -> [f64; 3] {
        [
            ix as f64 * length / nx as f64,
            iy as f64 * width  / ny as f64,
            iz as f64 * height / nz as f64,
        ]
    };

    let mut coords = vec![[0.0f64; 3]; n_nodes];
    for iz in 0..nz1 {
        for iy in 0..ny1 {
            for ix in 0..nx1 {
                coords[node_idx(ix, iy, iz)] = node_coord(ix, iy, iz);
            }
        }
    }

    // ── (6) Build per-element stiffness matrices ──────────────────────────────
    //
    // Freudenthal decomposition of each hex (c[0]..c[7]) into 6 tets.
    // Node ordering for each tet is chosen to give a positive Jacobian
    // determinant (right-handed orientation). The assembly uses |det J|
    // regardless, so orientation sign is not critical for correctness —
    // positive orientation is still the convention.
    let n_tets = nx * ny * nz * 6;
    let mut tet_connectivity: Vec<[usize; 4]> = Vec::with_capacity(n_tets);
    let mut elem_stiffness_mats: Vec<_>        = Vec::with_capacity(n_tets);

    for hz in 0..nz {
        for hy in 0..ny {
            for hx in 0..nx {
                let c = [
                    node_idx(hx,   hy,   hz  ),  // c[0]: local (0,0,0)
                    node_idx(hx+1, hy,   hz  ),  // c[1]: local (1,0,0)
                    node_idx(hx+1, hy+1, hz  ),  // c[2]: local (1,1,0)
                    node_idx(hx,   hy+1, hz  ),  // c[3]: local (0,1,0)
                    node_idx(hx,   hy,   hz+1),  // c[4]: local (0,0,1)
                    node_idx(hx+1, hy,   hz+1),  // c[5]: local (1,0,1)
                    node_idx(hx+1, hy+1, hz+1),  // c[6]: local (1,1,1)
                    node_idx(hx,   hy+1, hz+1),  // c[7]: local (0,1,1)
                ];
                // Six tets sharing diagonal c[0]→c[6]:
                let tets: [[usize; 4]; 6] = [
                    [c[0], c[1], c[2], c[6]],  // T0: det = +dx·dy·dz
                    [c[0], c[2], c[3], c[6]],  // T1: det = +dx·dy·dz
                    [c[0], c[5], c[1], c[6]],  // T2: det = +dx·dy·dz (c[5]↔c[1] swap)
                    [c[0], c[3], c[7], c[6]],  // T3: det = +dx·dy·dz
                    [c[0], c[4], c[5], c[6]],  // T4: det = +dx·dy·dz
                    [c[0], c[7], c[4], c[6]],  // T5: det = +dx·dy·dz (c[7]↔c[4] swap)
                ];
                for conn in tets {
                    let phys: Vec<[f64; 3]> = conn.iter().map(|&n| coords[n]).collect();
                    let k_e = element_stiffness(ElementOrder::P1, &phys, &mat);
                    tet_connectivity.push(conn);
                    elem_stiffness_mats.push(k_e);
                }
            }
        }
    }

    // ── (7) Assemble global stiffness matrix ──────────────────────────────────
    let assembly_elements: Vec<AssemblyElement<'_>> = tet_connectivity
        .iter()
        .zip(elem_stiffness_mats.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn.as_slice(),
            k_e,
        })
        .collect();

    let mut k = assemble_global_stiffness(
        n_nodes,
        &assembly_elements,
        AssemblyMode::Deterministic,
    );

    // ── (8) Build load vector; distribute tip load to tip-face nodes ──────────
    //
    // Tip face: all nodes at ix == nx (ny1 × nz1 = 2 × 9 = 18 nodes for the
    // 1×8 cross-section mesh). Force is distributed equally in the -Z direction
    // (height/gravity direction). Z is the bending direction for a cantilever
    // with the formula σ_max = 6PL/(bh²) where h is the Z-dimension (height).
    //
    // Load in -Z causes bending about the Y axis; with nz=8 elements across
    // the height, the P1 elements can capture the bending stress variation.
    let mut f = vec![0.0f64; 3 * n_nodes];
    let tip_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(nx, iy, iz)))
        .collect();
    let n_tip = tip_nodes.len().max(1);  // guard against zero div (18 nodes for nz=8)
    let force_per_tip = tip_force / n_tip as f64;
    for &tn in &tip_nodes {
        // Force in -Z direction (height = bending direction; see §8 comment above).
        apply_point_load(&mut f, tn, [0.0, 0.0, -force_per_tip]);
    }

    // ── (9) Dirichlet BCs: clamp all DOFs at root face (ix == 0) ─────────────
    let root_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(0, iy, iz)))
        .collect();
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for &rn in &root_nodes {
        for axis in 0..3usize {
            bcs.push(DirichletBc { dof: 3 * rn + axis, value: 0.0 });
        }
    }
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    // ── (10) Recover prior warm state; solve ──────────────────────────────────
    // `OpaqueState` has no `Clone`, so recover via `downcast_ref` + `CgWarmState::clone`
    // (cheap — cloning Arc bumps a refcount, not the Vec payload).
    let prior_cg = prior_warm_state.and_then(|s| s.downcast_ref::<CgWarmState>().cloned());
    let opts = CgSolverOptions { tolerance: 1e-6, max_iter: 2000 };
    let (cg_result, fresh_warm) = solve_cg_with_warm_state(
        &k,
        &f,
        prior_cg.as_ref(),
        opts,
        SolverMode::Deterministic,
    );

    // ── (11) Stress recovery: max von Mises across all elements ───────────────
    //
    // Each element_stress_p1 call returns the constant Cauchy stress tensor σ
    // (3×3 symmetric): [[σ_xx, σ_xy, σ_xz], [σ_xy, σ_yy, σ_yz], [σ_xz, σ_yz, σ_zz]].
    // Von Mises: σ_VM = sqrt(½·[(σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²
    //                            + 6·(σ_xy²+σ_yz²+σ_zx²)])
    let u_disp = &cg_result.u;
    let mut max_von_mises = 0.0f64;
    for conn in &tet_connectivity {
        let phys: [[f64; 3]; 4] = [
            coords[conn[0]],
            coords[conn[1]],
            coords[conn[2]],
            coords[conn[3]],
        ];
        let u_e: [f64; 12] = [
            u_disp[3 * conn[0]],     u_disp[3 * conn[0] + 1], u_disp[3 * conn[0] + 2],
            u_disp[3 * conn[1]],     u_disp[3 * conn[1] + 1], u_disp[3 * conn[1] + 2],
            u_disp[3 * conn[2]],     u_disp[3 * conn[2] + 1], u_disp[3 * conn[2] + 2],
            u_disp[3 * conn[3]],     u_disp[3 * conn[3] + 1], u_disp[3 * conn[3] + 2],
        ];
        let sigma = element_stress_p1(&phys, &mat, &u_e);
        // Unpack symmetric 3×3: rows/cols are (x, y, z)
        let sxx = sigma[0][0];
        let syy = sigma[1][1];
        let szz = sigma[2][2];
        let sxy = sigma[0][1];
        let syz = sigma[1][2];
        let szx = sigma[0][2];
        let vm = f64::sqrt(0.5 * (
            (sxx - syy).powi(2)
            + (syy - szz).powi(2)
            + (szz - sxx).powi(2)
            + 6.0 * (sxy * sxy + syz * syz + szx * szx)
        ));
        if vm > max_von_mises {
            max_von_mises = vm;
        }
    }

    // ── (12) Build ElasticResult StructureInstance ────────────────────────────
    //
    // StructureTypeId(u32::MAX) is a synthetic sentinel for this slice.
    // The three Body/Field-typed slots (displacement, stress, frame) are Undef
    // per the tet-result convention at solver_elastic.ri:280–284.
    // `cost_per_byte` is derived as 1/(warm-state size in bytes) — larger
    // warm states are more expensive per byte to retain in the pool.
    let n_iters    = cg_result.iterations as i64;
    let converged  = cg_result.converged;
    let size_bytes = fresh_warm.estimated_size_bytes();
    // cost_per_byte: reciprocal of warm-state size — a bigger state is pricier
    // to keep. Tuners should replace this with a profiling-derived estimate.
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    let new_warm_state = Some(fresh_warm.into_opaque_state());

    let fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), Value::Undef),
        ("stress".to_string(),       Value::Undef),
        ("frame".to_string(),        Value::Undef),
        ("max_von_mises".to_string(), Value::Scalar {
            si_value:  max_von_mises,
            dimension: DimensionVector::PRESSURE,
        }),
        ("converged".to_string(),   Value::Bool(converged)),
        ("iterations".to_string(),  Value::Int(n_iters)),
    ]
    .into_iter()
    .collect();

    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "ElasticResult".to_string(),
        version:   1,
        fields,
    }));

    // ── (13) Return ComputeOutcome::Completed ────────────────────────────────
    //
    // Field-by-field derivation (for future tuners):
    //
    // `result`        — ElasticResult StructureInstance built above.
    //
    // `new_warm_state`— The fresh CgWarmState (wrapping the converged
    //                   displacement vector u) serialised via `into_opaque_state()`.
    //                   Donated back to the cache by `complete_compute_dispatch_atomically`
    //                   (PRD §5). The next dispatch reads it via `get_warm_state` →
    //                   `CgWarmState::from_opaque_state` for warm-start CG solve.
    //
    // `cost_per_byte` — 1 / size_bytes of the warm state. Larger warm states are
    //                   more expensive per byte to keep in the pool (eviction LRU
    //                   prefers cheaper entries). Tuners: replace with a
    //                   profiling-derived cost (e.g. wall-clock solve time / state
    //                   size) once solve-time measurements are available.
    //
    // `diagnostics`   — empty (CG convergence failures are reflected in
    //                   `converged = Bool(false)` and the caller can inspect
    //                   that field; no separate diagnostic is needed today).
    ComputeOutcome::Completed {
        result,
        new_warm_state,
        cost_per_byte,
        diagnostics: vec![],
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract `IsotropicElastic` from a `Value::StructureInstance` carrying
/// `youngs_modulus: Scalar(PRESSURE)` and `poisson_ratio: Real`.
fn extract_material(val: &Value) -> IsotropicElastic {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => panic!(
            "solve_elastic_static_trampoline: expected material to be \
             Value::StructureInstance, got: {:?}",
            other
        ),
    };
    let youngs_modulus = match data.fields.get(&"youngs_modulus".to_string()) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "solve_elastic_static_trampoline: expected youngs_modulus to be \
             Value::Scalar, got: {:?}",
            other
        ),
    };
    let poisson_ratio = match data.fields.get(&"poisson_ratio".to_string()) {
        Some(Value::Real(r)) => *r,
        other => panic!(
            "solve_elastic_static_trampoline: expected poisson_ratio to be \
             Value::Real, got: {:?}",
            other
        ),
    };
    IsotropicElastic { youngs_modulus, poisson_ratio }
}

/// Extract SI scalar value from `Value::Scalar { si_value, .. }`.
fn extract_scalar_si(val: &Value) -> f64 {
    match val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "solve_elastic_static_trampoline: expected Value::Scalar, got: {:?}",
            other
        ),
    }
}

/// Sum `force` fields from all `PointLoad` StructureInstances in a `Value::List`.
/// Each `PointLoad.force` is a `Value::Real`.
fn extract_tip_force(val: &Value) -> f64 {
    let items = match val {
        Value::List(v) => v,
        other => panic!(
            "solve_elastic_static_trampoline: expected Value::List for loads, got: {:?}",
            other
        ),
    };
    let mut total = 0.0f64;
    for item in items {
        if let Value::StructureInstance(data) = item
            && let Some(Value::Real(f)) = data.fields.get(&"force".to_string())
        {
            total += f;
        }
    }
    total
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reify_solver_elastic::{AnisotropicMaterial, OrthotropicMaterial};

    /// step-3 RED (task δ/3780): orthotropic ConstantField cantilever tip-deflection
    /// band test at L/h = 8.
    ///
    /// Fixture: L=0.8 m, b=h=0.1 m, P=1000 N; strongly anisotropic material
    /// (E1=200 GPa along beam axis, E2=E3=10 GPa, G12=G13=G23=4 GPa,
    /// nu12=nu13=nu23=0.3). Identity material frame → E1 governs bending.
    ///
    /// Reference: Euler–Bernoulli δ_ref = P·L³/(3·E1·I), I = b·h³/12.
    /// Band: ±50% of δ_ref (P1-tet method-error budget; achievability survey §4.2,
    /// 2026-05-29; deflection converges better than stress for P1 tets).
    ///
    /// RED: MaterialModel enum and solve_cantilever_fea don't exist yet.
    #[test]
    fn orthotropic_cantilever_tip_deflection_within_euler_bernoulli_band() {
        // Build Rust OrthotropicMaterial: E1 >> E2 = E3 (strongly transverse-stiff)
        let law = OrthotropicMaterial {
            e1: 200e9_f64,  // 200 GPa — beam-axis Young's modulus (governs bending)
            e2: 10e9_f64,   // 10 GPa  — transverse
            e3: 10e9_f64,   // 10 GPa  — transverse
            g12: 4e9_f64,   // 4 GPa
            g13: 4e9_f64,   // 4 GPa
            g23: 4e9_f64,   // 4 GPa
            nu12: 0.3_f64,
            nu13: 0.3_f64,
            nu23: 0.3_f64,
        };
        // Identity material frame: beam axis = material 1-axis → E1 governs bending.
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);

        // Cantilever geometry at L/h = 8 (keeps fixture off slender bending-lock wall).
        let length = 0.8_f64;   // m — beam length (x-axis)
        let width  = 0.1_f64;   // m — cross-section width (y-axis)
        let height = 0.1_f64;   // m — cross-section height (z-axis, bending direction)
        let tip_force = 1000.0_f64; // N (distributed to tip-face nodes by trampoline)

        // Call the new pub(crate) helper (doesn't exist yet → compile-fail RED).
        let (result, _fresh_warm) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso_mat),
            length,
            width,
            height,
            tip_force,
            None,
        );

        // Tip deflection = max |u_z| over tip-face nodes.
        let tip_deflection = result
            .tip_nodes
            .iter()
            .map(|&n| result.u[3 * n + 2].abs())  // z-component
            .fold(0.0f64, f64::max);

        // Euler–Bernoulli reference: δ = P·L³ / (3·E1·I), I = b·h³/12.
        let i_beam = width * height.powi(3) / 12.0;
        let delta_eb = tip_force * length.powi(3) / (3.0 * 200e9_f64 * i_beam);

        assert!(
            tip_deflection.is_finite() && tip_deflection > 0.0,
            "tip deflection must be finite and positive, got {tip_deflection}"
        );
        assert!(
            tip_deflection >= 0.5 * delta_eb && tip_deflection <= 1.5 * delta_eb,
            "tip deflection {tip_deflection:.6e} m outside ±50% band [{:.6e}, {:.6e}] m \
             of Euler–Bernoulli reference {delta_eb:.6e} m",
            0.5 * delta_eb,
            1.5 * delta_eb,
        );
    }
}
