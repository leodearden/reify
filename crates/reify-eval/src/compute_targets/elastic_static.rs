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

use std::sync::Arc;

use reify_core::DimensionVector;
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    AnisotropicMaterial, AssemblyElement, AssemblyMode, CgSolverOptions, CgWarmState,
    ConstantField, DirichletBc, ElementOrder, IsotropicElastic, OrthotropicMaterial,
    SolverMode, TransverseIsotropicMaterial,
    apply_dirichlet_row_elimination, apply_point_load, assemble_global_stiffness,
    element_stiffness, element_stiffness_p1_with_field, element_stress_p1,
    solve_cg_with_warm_state,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

// ── MaterialModel ────────────────────────────────────────────────────────────

/// Dispatch tag used by `solve_cantilever_fea` to route element assembly and
/// stress recovery to the correct material path.
///
/// Isotropic: uses the legacy `element_stiffness(P1, ..)` + `element_stress_p1`
/// paths unchanged (byte-identical to the pre-δ trampoline).
///
/// Anisotropic: assembles via `element_stiffness_p1_with_field(&ConstantField{..})`
/// and recovers von Mises inline from `d_matrix_global()`.
#[allow(clippy::large_enum_variant)]
pub(crate) enum MaterialModel {
    /// Isotropic elastic material (legacy path — unchanged from pre-δ).
    Isotropic(IsotropicElastic),
    /// Homogeneous anisotropic material (orthotropic or transverse-isotropic),
    /// with its 6×6 D already rotated into the global frame.
    Anisotropic(AnisotropicMaterial),
}

// ── CantileverFeaSolve ────────────────────────────────────────────────────────

/// Outputs from `solve_cantilever_fea` exposed to callers (unit tests + trampoline).
// `u`, `coords`, and `tip_nodes` are read in `#[cfg(test)]` code; the lib-target
// dead_code lint fires because it doesn't see test-only reads.
#[allow(dead_code)]
pub(crate) struct CantileverFeaSolve {
    /// Displacement vector (length 3 × n_nodes): u[3n], u[3n+1], u[3n+2] for node n.
    /// Stored as `Arc<Vec<f64>>` to avoid copying the CgResult's shared buffer.
    pub u: Arc<Vec<f64>>,
    /// Node coordinates (length n_nodes).
    pub coords: Vec<[f64; 3]>,
    /// Indices of tip-face nodes (ix == nx) — for tip-deflection queries.
    pub tip_nodes: Vec<usize>,
    /// Maximum von Mises stress across all elements (Pa).
    pub max_von_mises: f64,
    /// True iff CG converged within max_iter.
    pub converged: bool,
    /// Number of CG iterations performed.
    pub iterations: usize,
}

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
    // ── (1) Classify material and build MaterialModel (step-6: full dispatch) ──
    //
    // Dispatch on the StructureInstance type_name.  Anisotropic conformers
    // (OrthotropicMaterial, TransverseIsotropicMaterial) are produced by γ/3779
    // (stdlib/constitutive.ri); the isotropic fallback reads youngs_modulus+
    // poisson_ratio (unchanged from the pre-δ trampoline).
    let model = classify_material(&value_inputs[0]);

    // ── (2) Extract geometry scalars (SI: metres) ─────────────────────────────
    let length = extract_scalar_si(&value_inputs[1]);
    let width  = extract_scalar_si(&value_inputs[2]);
    let height = extract_scalar_si(&value_inputs[3]);

    // ── (3) Sum tip-force magnitudes from PointLoad list ─────────────────────
    let tip_force = extract_tip_force(&value_inputs[4]);

    // ── (4) Supports: non-empty list → cantilever is clamped at root ─────────
    // (We don't inspect individual FixedSupport fields; presence is sufficient.)

    // ── (5) Recover prior warm state ─────────────────────────────────────────
    // `OpaqueState` has no `Clone`, so recover via `downcast_ref` + `CgWarmState::clone`
    // (cheap — cloning Arc bumps a refcount, not the Vec payload).
    let prior_cg = prior_warm_state.and_then(|s| s.downcast_ref::<CgWarmState>().cloned());

    // ── (6) Delegate to shared FEA helper ────────────────────────────────────
    let (fea, fresh_warm) = solve_cantilever_fea(&model, length, width, height, tip_force, prior_cg);

    // ── (7) Build ElasticResult StructureInstance ────────────────────────────
    //
    // StructureTypeId(u32::MAX) is a synthetic sentinel for this slice.
    // The three Body/Field-typed slots (displacement, stress, frame) are Undef
    // per the tet-result convention at solver_elastic.ri:280–284.
    // `cost_per_byte` is derived as 1/(warm-state size in bytes) — larger
    // warm states are more expensive per byte to retain in the pool.
    let n_iters    = fea.iterations as i64;
    let converged  = fea.converged;
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
            si_value:  fea.max_von_mises,
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

    // ── (8) Return ComputeOutcome::Completed ─────────────────────────────────
    //
    // `result`        — ElasticResult StructureInstance built above.
    //
    // `new_warm_state`— The fresh CgWarmState donated back to the cache by
    //                   `complete_compute_dispatch_atomically` (PRD §5).
    //
    // `cost_per_byte` — 1 / size_bytes of the warm state.
    //
    // `diagnostics`   — empty (CG convergence failures are reflected in
    //                   `converged = Bool(false)`).
    ComputeOutcome::Completed {
        result,
        new_warm_state,
        cost_per_byte,
        diagnostics: vec![],
    }
}

// ── solve_cantilever_fea ──────────────────────────────────────────────────────

/// Core FEA solve for the cantilever fixture used by `solve_elastic_static_trampoline`
/// and the unit tests.
///
/// Builds a `nx×1×nz` Freudenthal hex-split mesh (6 P1-tets per hex), assembles K,
/// applies Dirichlet BCs, solves CG, recovers max von Mises.
///
/// # Material dispatch
/// - `MaterialModel::Isotropic(iso)` — uses `element_stiffness(P1, ..)` and
///   `element_stress_p1` (byte-identical to the pre-δ trampoline).
/// - `MaterialModel::Anisotropic(aniso)` — assembles via
///   `element_stiffness_p1_with_field(&ConstantField{material: aniso})` (PRD C4
///   per-element centroid sampling) and recovers von Mises inline from
///   `aniso.d_matrix_global()`.
///
/// Returns `(CantileverFeaSolve, CgWarmState)`.
pub(crate) fn solve_cantilever_fea(
    model: &MaterialModel,
    length: f64,
    width: f64,
    height: f64,
    tip_force: f64,
    prior_cg: Option<CgWarmState>,
) -> (CantileverFeaSolve, CgWarmState) {
    // ── Mesh ──────────────────────────────────────────────────────────────────
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
    // FIX: scale nx ∝ nz × (L/h) so that δ_x ≈ δ_z (near-cubic elements
    // in the bending plane). For L=1m, h=0.1m, nz=6: nx = 6×10 = 60.
    // Near-cubic Freudenthal tets have minimal shear locking.
    //
    // ny=1: bending is about Y, so a single element in the Y direction is
    // sufficient.
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

    // ── Per-element stiffness matrices ────────────────────────────────────────
    //
    // Freudenthal decomposition of each hex (c[0]..c[7]) into 6 tets.
    // Node ordering for each tet is chosen to give a positive Jacobian
    // determinant (right-handed orientation).
    let n_tets = nx * ny * nz * 6;
    let mut tet_connectivity: Vec<[usize; 4]> = Vec::with_capacity(n_tets);
    let mut elem_stiffness_mats: Vec<_>        = Vec::with_capacity(n_tets);

    // Hoist per-element-constant anisotropic quantities out of the element loops.
    //
    // For `MaterialModel::Anisotropic`, both the `ConstantField` (used in the
    // stiffness loop) and `d_matrix_global()` (used in the stress-recovery loop)
    // are identical for every element: the material is homogeneous and the frame
    // is the identity, so `rotate_voigt` runs once here instead of O(n_tets) times.
    //
    // For `MaterialModel::Isotropic` this tuple is `None` and incurs no cost.
    let aniso_precomp: Option<(ConstantField, [[f64; 6]; 6])> =
        if let MaterialModel::Anisotropic(a) = model {
            Some((ConstantField { material: *a }, a.d_matrix_global()))
        } else {
            None
        };

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
                    let phys4: [[f64; 3]; 4] = [phys[0], phys[1], phys[2], phys[3]];
                    let k_e = match model {
                        MaterialModel::Isotropic(iso) => {
                            element_stiffness(ElementOrder::P1, &phys, iso)
                        }
                        MaterialModel::Anisotropic(_) => {
                            // Use the hoisted ConstantField (computed once above).
                            element_stiffness_p1_with_field(
                                &phys4,
                                &aniso_precomp.as_ref().unwrap().0,
                            )
                        }
                    };
                    tet_connectivity.push(conn);
                    elem_stiffness_mats.push(k_e);
                }
            }
        }
    }

    // ── Assemble global stiffness matrix ──────────────────────────────────────
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

    // ── Build load vector; distribute tip load to tip-face nodes ─────────────
    //
    // Tip face: all nodes at ix == nx (ny1 × nz1 = 2 × 7 = 14 nodes for
    // the 1×6 cross-section mesh). Force is distributed equally in the -Z
    // direction (height/gravity direction). Z is the bending direction.
    let mut f = vec![0.0f64; 3 * n_nodes];
    let tip_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(nx, iy, iz)))
        .collect();
    let n_tip = tip_nodes.len().max(1);
    let force_per_tip = tip_force / n_tip as f64;
    for &tn in &tip_nodes {
        apply_point_load(&mut f, tn, [0.0, 0.0, -force_per_tip]);
    }

    // ── Dirichlet BCs: clamp all DOFs at root face (ix == 0) ─────────────────
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

    // ── Solve ─────────────────────────────────────────────────────────────────
    let opts = CgSolverOptions { tolerance: 1e-6, max_iter: 2000 };
    let (cg_result, fresh_warm) = solve_cg_with_warm_state(
        &k,
        &f,
        prior_cg.as_ref(),
        opts,
        SolverMode::Deterministic,
    );

    // ── Stress recovery: max von Mises across all elements ────────────────────
    //
    // Isotropic: element_stress_p1 returns symmetric 3×3 Cauchy tensor;
    //   Von Mises: sqrt(½·[(σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)² + 6·(σ_xy²+σ_yz²+σ_zx²)])
    //
    // Anisotropic: mirrors the B-matrix computation inside element_stress_p1
    //   (same engineering-shear Voigt convention) but substitutes D_global for
    //   IsotropicElastic::d_matrix. Von Mises computed from σ_voigt directly.
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
        let vm = match model {
            MaterialModel::Isotropic(iso) => {
                let sigma = element_stress_p1(&phys, iso, &u_e);
                let sxx = sigma[0][0];
                let syy = sigma[1][1];
                let szz = sigma[2][2];
                let sxy = sigma[0][1];
                let syz = sigma[1][2];
                let szx = sigma[0][2];
                f64::sqrt(0.5 * (
                    (sxx - syy).powi(2)
                    + (syy - szz).powi(2)
                    + (szz - sxx).powi(2)
                    + 6.0 * (sxy * sxy + syz * syz + szx * szx)
                ))
            }
            MaterialModel::Anisotropic(_) => {
                // Use the hoisted d_global (computed once above).
                element_von_mises_anisotropic(
                    &phys,
                    &aniso_precomp.as_ref().unwrap().1,
                    &u_e,
                )
            }
        };
        if vm > max_von_mises {
            max_von_mises = vm;
        }
    }

    let fea = CantileverFeaSolve {
        u: cg_result.u,
        coords,
        tip_nodes,
        max_von_mises,
        converged: cg_result.converged,
        iterations: cg_result.iterations,
    };
    (fea, fresh_warm)
}

/// Compute von Mises stress for a P1 tet with a given 6×6 global D matrix.
///
/// Mirrors the B-matrix construction in `element_stress_p1` (same engineering-shear
/// Voigt convention: ε = [ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]) but substitutes
/// `d_global` for `IsotropicElastic::d_matrix`. Used by the anisotropic branch.
///
/// Von Mises: sqrt(½·[(σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²+6·(σ_xy²+σ_yz²+σ_zx²)])
fn element_von_mises_anisotropic(
    phys_nodes: &[[f64; 3]; 4],
    d_global: &[[f64; 6]; 6],
    u_e: &[f64; 12],
) -> f64 {
    // Jacobian (same as element_stress_p1).
    // Use a fixed reference point — gradients are constant for P1.
    // Reference gradients of the P1 shape functions at (1/4, 1/4, 1/4):
    //   N1 = 1-ξ-η-ζ, N2 = ξ, N3 = η, N4 = ζ
    //   ∇ξ = [[-1,-1,-1],[1,0,0],[0,1,0],[0,0,1]]
    let grads_ref: [[f64; 3]; 4] = [
        [-1.0, -1.0, -1.0],
        [ 1.0,  0.0,  0.0],
        [ 0.0,  1.0,  0.0],
        [ 0.0,  0.0,  1.0],
    ];

    // J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j]
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..4 {
        for i in 0..3 {
            for j in 0..3 {
                j_mat[i][j] += phys_nodes[k][i] * grads_ref[k][j];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);

    // Degenerate-element guard — mirrors `element_stress_p1` (result.rs:94-100).
    // `det.is_normal()` catches ±0, ±∞, NaN, and subnormals; the absolute-value
    // floor matches `reify_solver_elastic::math::MIN_JACOBIAN_DET` (1e-30).
    // A degenerate tet with |det J| at or below this threshold would produce
    // NaN/Inf stress via the division in the J⁻ᵀ computation below.
    const MIN_JACOBIAN_DET: f64 = 1.0e-30;
    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "element_von_mises_anisotropic: degenerate tet |det J| = {:.3e} \
         (must be > {:.3e} and finite — see PRD task #21 for the future diagnostic path)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );

    // J⁻ᵀ via cofactor / det (same formula as element_stress_p1 → inverse_transpose_3x3)
    let j_inv_t = [
        [
            (j_mat[1][1]*j_mat[2][2] - j_mat[1][2]*j_mat[2][1]) / det,
            (j_mat[1][2]*j_mat[2][0] - j_mat[1][0]*j_mat[2][2]) / det,
            (j_mat[1][0]*j_mat[2][1] - j_mat[1][1]*j_mat[2][0]) / det,
        ],
        [
            (j_mat[0][2]*j_mat[2][1] - j_mat[0][1]*j_mat[2][2]) / det,
            (j_mat[0][0]*j_mat[2][2] - j_mat[0][2]*j_mat[2][0]) / det,
            (j_mat[0][1]*j_mat[2][0] - j_mat[0][0]*j_mat[2][1]) / det,
        ],
        [
            (j_mat[0][1]*j_mat[1][2] - j_mat[0][2]*j_mat[1][1]) / det,
            (j_mat[0][2]*j_mat[1][0] - j_mat[0][0]*j_mat[1][2]) / det,
            (j_mat[0][0]*j_mat[1][1] - j_mat[0][1]*j_mat[1][0]) / det,
        ],
    ];

    // Physical gradients: ∇x N_i = J⁻ᵀ · ∇ξ N_i
    let mut grads_phys = [[0.0_f64; 3]; 4];
    for i in 0..4 {
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..3 {
                s += j_inv_t[r][c] * grads_ref[i][c];
            }
            grads_phys[i][r] = s;
        }
    }

    // Build B and compute ε_voigt = B · u_e in one fused loop.
    // Convention matches element_stress_p1 (rows 0-5: ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz)
    let mut eps = [0.0_f64; 6];
    for i in 0..4 {
        let (gx, gy, gz) = (grads_phys[i][0], grads_phys[i][1], grads_phys[i][2]);
        let (ux, uy, uz) = (u_e[3 * i], u_e[3 * i + 1], u_e[3 * i + 2]);
        eps[0] += gx * ux;
        eps[1] += gy * uy;
        eps[2] += gz * uz;
        eps[3] += gy * ux + gx * uy;
        eps[4] += gz * uy + gy * uz;
        eps[5] += gz * ux + gx * uz;
    }

    // σ_voigt = D_global · ε_voigt
    let mut sigma_voigt = [0.0_f64; 6];
    for i in 0..6 {
        let mut s = 0.0;
        for j in 0..6 {
            s += d_global[i][j] * eps[j];
        }
        sigma_voigt[i] = s;
    }

    // σ_voigt = [σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]
    let (sxx, syy, szz, sxy, syz, szx) = (
        sigma_voigt[0], sigma_voigt[1], sigma_voigt[2],
        sigma_voigt[3], sigma_voigt[4], sigma_voigt[5],
    );
    f64::sqrt(0.5 * (
        (sxx - syy).powi(2)
        + (syy - szz).powi(2)
        + (szz - sxx).powi(2)
        + 6.0 * (sxy * sxy + syz * syz + szx * szx)
    ))
}


// ── helpers ───────────────────────────────────────────────────────────────────

/// Classify a material `Value::StructureInstance` as `MaterialModel::Isotropic`
/// or `MaterialModel::Anisotropic` by inspecting its `type_name`.
///
/// Dispatch table (δ/3780 step-6):
/// - `"OrthotropicMaterial"` → read 9 constants (e1..e3, g12..g23, nu12..nu23)
///   → `Rust OrthotropicMaterial` → `AnisotropicMaterial::from_law(&law, I₃)` → Anisotropic.
/// - `"TransverseIsotropicMaterial"` → read 5 constants → same.
/// - else → `extract_material` (reads `youngs_modulus` + `poisson_ratio`) → Isotropic.
///
/// Identity material frame `I₃` is used for the homogeneous `ConstitutiveLaw`
/// surface (axis-aligned cantilever, beam axis = material 1-axis → E1 governs
/// bending). Per-element frames arrive with the `Field` surface in ε/3787.
fn classify_material(val: &Value) -> MaterialModel {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => panic!(
            "solve_elastic_static_trampoline: expected material to be \
             Value::StructureInstance, got: {:?}",
            other
        ),
    };
    // Identity material frame: global axes = material principal axes.
    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    match data.type_name.as_str() {
        "OrthotropicMaterial" => {
            let e1   = scalar_si_field(data, "e1");
            let e2   = scalar_si_field(data, "e2");
            let e3   = scalar_si_field(data, "e3");
            let g12  = scalar_si_field(data, "g12");
            let g13  = scalar_si_field(data, "g13");
            let g23  = scalar_si_field(data, "g23");
            let nu12 = real_field(data, "nu12");
            let nu13 = real_field(data, "nu13");
            let nu23 = real_field(data, "nu23");
            let law  = OrthotropicMaterial { e1, e2, e3, g12, g13, g23, nu12, nu13, nu23 };
            let aniso = AnisotropicMaterial::from_law(&law, IDENTITY);
            MaterialModel::Anisotropic(aniso)
        }
        "TransverseIsotropicMaterial" => {
            let e_in_plane  = scalar_si_field(data, "e_in_plane");
            let e_axial     = scalar_si_field(data, "e_axial");
            let nu_in_plane = real_field(data, "nu_in_plane");
            let nu_axial    = real_field(data, "nu_axial");
            let g_axial     = scalar_si_field(data, "g_axial");
            let law = TransverseIsotropicMaterial {
                e_in_plane, e_axial, nu_in_plane, nu_axial, g_axial,
            };
            let aniso = AnisotropicMaterial::from_law(&law, IDENTITY);
            MaterialModel::Anisotropic(aniso)
        }
        _ => {
            // Isotropic fallback: reads youngs_modulus + poisson_ratio (unchanged
            // from the pre-δ trampoline).
            MaterialModel::Isotropic(extract_material(val))
        }
    }
}

/// Read a `Value::Scalar { si_value, .. }` field from a StructureInstance.
fn scalar_si_field(data: &StructureInstanceData, key: &str) -> f64 {
    match data.fields.get(&key.to_string()) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "solve_elastic_static_trampoline: expected field {:?} to be \
             Value::Scalar, got: {:?}",
            key, other
        ),
    }
}

/// Read a `Value::Real` field from a StructureInstance.
fn real_field(data: &StructureInstanceData, key: &str) -> f64 {
    match data.fields.get(&key.to_string()) {
        Some(Value::Real(r)) => *r,
        other => panic!(
            "solve_elastic_static_trampoline: expected field {:?} to be \
             Value::Real, got: {:?}",
            key, other
        ),
    }
}

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

    /// Amendment (test_coverage): pin `element_von_mises_anisotropic` against the
    /// analytic bending-stress reference for the same orthotropic fixture.
    ///
    /// The analytic peak bending stress for a cantilever is:
    ///   σ_max = 6·P·L / (b·h²)
    /// This is material-independent (pure-equilibrium Euler–Bernoulli result).
    /// For the fixture: 6×1000×0.8 / (0.1×0.01) = 4.8 MPa.
    ///
    /// The ±50% band is the same P1-tet method-error budget already documented
    /// for the isotropic stress test (solve_elastic_static_e2e.rs:231) and mirrors
    /// the reviewer's suggestion to add a stress-magnitude assertion that would
    /// catch regressions in the D_global·ε multiply, eps ordering, or Voigt-index
    /// unpacking inside `element_von_mises_anisotropic`.
    #[test]
    fn orthotropic_cantilever_max_von_mises_within_stress_band() {
        // Same orthotropic fixture as the deflection test.
        let law = OrthotropicMaterial {
            e1: 200e9_f64, e2: 10e9_f64, e3: 10e9_f64,
            g12: 4e9_f64,  g13: 4e9_f64, g23: 4e9_f64,
            nu12: 0.3_f64, nu13: 0.3_f64, nu23: 0.3_f64,
        };
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);

        let length    = 0.8_f64;
        let width     = 0.1_f64;
        let height    = 0.1_f64;
        let tip_force = 1000.0_f64;

        let (result, _) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso_mat),
            length, width, height, tip_force, None,
        );

        // Analytic σ_max = 6·P·L / (b·h²) — independent of material stiffness.
        let sigma_analytic = 6.0 * tip_force * length / (width * height * height);
        let vm = result.max_von_mises;

        assert!(
            vm.is_finite() && vm > 0.0,
            "max_von_mises must be finite and positive, got {vm}"
        );
        // ±50% P1-tet method-error band (same budget as isotropic stress e2e).
        assert!(
            vm >= 0.5 * sigma_analytic && vm <= 1.5 * sigma_analytic,
            "max_von_mises {vm:.4e} Pa outside ±50% band [{:.4e}, {:.4e}] Pa \
             of analytic σ_max {sigma_analytic:.4e} Pa",
            0.5 * sigma_analytic,
            1.5 * sigma_analytic,
        );
    }
}
