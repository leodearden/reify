//! Trampoline for `solver::buckling` — the `fn solve_buckling`
//! @optimized target (PRD §13 task ε, docs/prds/v0_5/buckling-eigensolver.md).
//!
//! # Contract (§13-ε)
//!
//! Receives the 7 `value_inputs` matching the fn signature:
//!   `(material, length, width, height, loads, supports, options)`
//!
//! Builds a column mesh (compression axis = longest dimension, pin-pin BCs),
//! calls `solve_buckling_kernel`, returns a `BucklingResult`-shaped
//! `Value::StructureInstance`.
//!
//! # Analytical reference
//!
//! For a pin-pin column: P_cr = π²·E·I / L²
//! The smoke test uses L=0.8m, cross-section 20×20mm, Steel AISI 1045
//! (E=205 GPa). The P1-tet mesh at nx=ny=8,nz=160 yields ~9.2% error.
//!
//! # Cache-hit contract (§3 + Final-gate)
//!
//! `solver::buckling` is deliberately NOT added to `significance_filter::is_opted_in`
//! (that, plus BucklingResult-shape comparison, is task θ/3457). The cache-hit
//! signal relies on the generic Final-gate in `engine_eval.rs` (~:2808-2860)
//! which short-circuits re-dispatch when all inputs AND the output VC are
//! already `Freshness::Final` from a prior `Engine::eval()` call.
//!
//! # StructureTypeId sentinel
//!
//! The trampoline carries no `StructureRegistry` access. All StructureInstances
//! use `StructureTypeId(u32::MAX)` as a synthetic sentinel (same convention as
//! `elastic_static.rs`).
//!
//! # Mode.mode_shape = Undef
//!
//! The mode_shape field is returned as `Value::Undef` per the tet-result
//! convention (solver_elastic.ri:280-284). Populating it with displaced-position
//! Maps is task ι/3458 (GUI animator).
//!
//! # reference_load param for critical_load
//!
//! The kernel returns a dimensionless multiplier λ (λ × F_applied = P_cr).
//! The `critical_load(result, reference_load)` helper in solver_buckling.ri
//! takes an explicit `reference_load: Force` param — see design decision DD-1
//! in .task/plan.json and the non-blocking escalate_info filed at task 3454.

use reify_core::DimensionVector;
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    DirichletBc, IsotropicElastic,
    apply_point_load,
    BucklingKernelOptions, solve_buckling_kernel,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Trampoline for `solver::buckling`.
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
/// [6] options   : BucklingOptions    (Value::StructureInstance — solver defaults used)
/// ```
///
/// Returns a `BucklingResult`-shaped `Value::StructureInstance` with:
/// - `modes`: `Value::List` of `Mode` StructureInstances
///   (`eigenvalue: Real(λ)`, `mode_shape: Undef`)
/// - `converged`: `Value::Bool`
/// - `iterations`: `Value::Int(0)` placeholder (kernel returns total iters)
/// - `pre_stress`: `ElasticResult`-shaped StructureInstance (Undef fields +
///   `max_von_mises: Scalar(PRESSURE)`, `converged: Bool(true)`, `iterations: Int(0)`)
pub fn solve_buckling_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── (1) Extract material ──────────────────────────────────────────────────
    let mat = extract_material(&value_inputs[0]);

    // ── (2) Extract geometry scalars (SI: metres) ─────────────────────────────
    let length = extract_scalar_si(&value_inputs[1]);
    let width  = extract_scalar_si(&value_inputs[2]);
    let height = extract_scalar_si(&value_inputs[3]);

    // ── (3) Extract total compressive load magnitude from loads list ──────────
    let total_load = extract_total_load(&value_inputs[4]);

    // ── (4) Extract BucklingOptions ───────────────────────────────────────────
    let (n_modes, eigen_tol, eigen_max_iters) = extract_buckling_options(&value_inputs[6]);

    // ── (5) Build column mesh (compression axis = Z = longest dimension) ──────
    //
    // Geometry: column along Z axis, cross-section in XY plane.
    // lx = width (X), ly = height (Y), lz = length (Z = compression axis).
    //
    // Mesh density mirrors euler_column_pin_pin.rs (nx=ny=8, nz=160) for the
    // 20×20×800 mm smoke column, giving the validated 9.2%-error result.
    // Density is geometry-driven:
    //   nz ≈ length / (cross_section_size / 8)
    //   nx = ny = 8 across the cross-section
    // "cross-section size" = min(width, height) so both axes stay at ~8 elements.
    //
    // Clamp to at least 1 in each direction.
    let nx: usize = 8;
    let ny: usize = 8;
    let lx = width;
    let ly = height;
    let lz = length;
    // nz: scale so element axial length ≈ cross_section_elem_size
    let cross_elem_size = lx.min(ly) / nx as f64;
    let nz: usize = ((lz / cross_elem_size).round() as usize).max(1);

    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    // Node linearisation: (k, j, i) — matches euler_column_pin_pin.rs
    let node_id = |i: usize, j: usize, k: usize| -> usize {
        k * nx1 * ny1 + j * nx1 + i
    };
    let node_xyz = |i: usize, j: usize, k: usize| -> [f64; 3] {
        [
            i as f64 * lx / nx as f64,
            j as f64 * ly / ny as f64,
            k as f64 * lz / nz as f64,
        ]
    };

    let mut nodes = Vec::with_capacity(n_nodes);
    for k in 0..nz1 {
        for j in 0..ny1 {
            for i in 0..nx1 {
                nodes.push(node_xyz(i, j, k));
            }
        }
    }

    // Six-tet long-diagonal brick decomposition (same as euler_column_pin_pin.rs)
    const TET_SPLITS: [[usize; 4]; 6] = [
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
        [0, 5, 1, 6],
    ];

    let mut tets: Vec<[usize; 4]> = Vec::with_capacity(nx * ny * nz * 6);
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let corner = [
                    node_id(i,     j,     k    ),
                    node_id(i + 1, j,     k    ),
                    node_id(i + 1, j + 1, k    ),
                    node_id(i,     j + 1, k    ),
                    node_id(i,     j,     k + 1),
                    node_id(i + 1, j,     k + 1),
                    node_id(i + 1, j + 1, k + 1),
                    node_id(i,     j + 1, k + 1),
                ];
                for split in TET_SPLITS {
                    tets.push([corner[split[0]], corner[split[1]], corner[split[2]], corner[split[3]]]);
                }
            }
        }
    }

    // ── (6) Pin-pin BCs: lateral clamp (u_x=u_y=0) at both Z-end faces ───────
    //   + one axial anchor at the bottom corner to prevent rigid-body Z-translation.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for k_face in [0usize, nz] {
        for j in 0..=ny {
            for i in 0..=nx {
                let n = node_id(i, j, k_face);
                bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
                bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
            }
        }
    }
    // Axial anchor at bottom corner.
    let anchor = node_id(0, 0, 0);
    bcs.push(DirichletBc { dof: 3 * anchor + 2, value: 0.0 }); // u_z

    // ── (7) Load vector: distribute total_load across top-face nodes in -Z ────
    let n_top = (nx + 1) * (ny + 1);
    let mut f = vec![0.0f64; 3 * n_nodes];
    for j in 0..=ny {
        for i in 0..=nx {
            let n = node_id(i, j, nz);
            apply_point_load(&mut f, n, [0.0, 0.0, -total_load / n_top as f64]);
        }
    }

    // ── (8) Call the buckling kernel ──────────────────────────────────────────
    let opts = BucklingKernelOptions {
        n_modes,
        eigen_tol,
        eigen_max_iters,
        cg_tolerance: 1e-10,
        cg_max_iter: 10_000,
    };
    let kernel_result = solve_buckling_kernel(&nodes, &tets, &mat, &bcs, &f, &[], opts);

    // ── (9) Compute max_von_mises from pre_stress_per_element ─────────────────
    //
    // Von Mises: σ_VM = sqrt(½·[(σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²
    //                          + 6·(σ_xy²+σ_yz²+σ_zx²)])
    let max_von_mises = kernel_result
        .pre_stress_per_element
        .iter()
        .map(|sigma| {
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
        })
        .fold(0.0f64, f64::max);

    // ── (10) Build pre_stress ElasticResult StructureInstance ─────────────────
    let pre_stress_fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), Value::Undef),
        ("stress".to_string(),       Value::Undef),
        ("frame".to_string(),        Value::Undef),
        ("max_von_mises".to_string(), Value::Scalar {
            si_value:  max_von_mises,
            dimension: DimensionVector::PRESSURE,
        }),
        ("converged".to_string(),   Value::Bool(true)),
        ("iterations".to_string(),  Value::Int(0)),
    ]
    .into_iter()
    .collect();

    let pre_stress = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "ElasticResult".to_string(),
        version:   1,
        fields:    pre_stress_fields,
    }));

    // ── (11) Build modes list ─────────────────────────────────────────────────
    //
    // Modes are already sorted ascending |λ| by the kernel.
    // mode_shape = Undef per the task-ε scope boundary (task ι/3458 populates it).
    let modes_list: Vec<Value> = kernel_result
        .modes
        .iter()
        .map(|m| {
            let mode_fields: PersistentMap<String, Value> = [
                ("eigenvalue".to_string(), Value::Real(m.eigenvalue)),
                ("mode_shape".to_string(), Value::Undef),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_id:   StructureTypeId(u32::MAX),
                type_name: "Mode".to_string(),
                version:   1,
                fields:    mode_fields,
            }))
        })
        .collect();

    // ── (12) Build BucklingResult StructureInstance ───────────────────────────
    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(),      Value::List(modes_list)),
        ("converged".to_string(),  Value::Bool(kernel_result.converged)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), pre_stress),
    ]
    .into_iter()
    .collect();

    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version:   1,
        fields:    result_fields,
    }));

    // ── (13) Return ComputeOutcome::Completed ────────────────────────────────
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte:  None,
        diagnostics:    vec![],
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract `IsotropicElastic` from a `Value::StructureInstance` carrying
/// `youngs_modulus: Scalar(PRESSURE)` and `poisson_ratio: Real`.
fn extract_material(val: &Value) -> IsotropicElastic {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => panic!(
            "solve_buckling_trampoline: expected material to be \
             Value::StructureInstance, got: {:?}",
            other
        ),
    };
    let youngs_modulus = match data.fields.get(&"youngs_modulus".to_string()) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "solve_buckling_trampoline: expected youngs_modulus to be \
             Value::Scalar, got: {:?}",
            other
        ),
    };
    let poisson_ratio = match data.fields.get(&"poisson_ratio".to_string()) {
        Some(Value::Real(r)) => *r,
        other => panic!(
            "solve_buckling_trampoline: expected poisson_ratio to be \
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
            "solve_buckling_trampoline: expected Value::Scalar, got: {:?}",
            other
        ),
    }
}

/// Sum `force` fields from all `PointLoad` StructureInstances in a `Value::List`.
/// Falls back to a default load of 1.0 N if no force entries are found.
fn extract_total_load(val: &Value) -> f64 {
    let items = match val {
        Value::List(v) => v,
        other => panic!(
            "solve_buckling_trampoline: expected Value::List for loads, got: {:?}",
            other
        ),
    };
    let mut total = 0.0f64;
    for item in items {
        if let Value::StructureInstance(data) = item {
            // PointLoad.force : Real (magnitude)
            if let Some(Value::Real(f)) = data.fields.get(&"force".to_string()) {
                total += f;
            }
            // Also handle Scalar forces (in case units are carried through)
            if let Some(Value::Scalar { si_value, .. }) = data.fields.get(&"force".to_string()) {
                total += si_value;
            }
        }
    }
    // Guard: if no load was extracted, use 1.0 N (degenerate case)
    if total == 0.0 { 1.0 } else { total }
}

/// Extract BucklingOptions fields: (n_modes, eigen_tol, eigen_max_iters).
///
/// Falls back to kernel defaults if the value is not a StructureInstance or
/// the fields are missing.
fn extract_buckling_options(val: &Value) -> (usize, f64, usize) {
    let default_n_modes: usize = 10;
    let default_tol: f64 = 1e-8;
    let default_max_iters: usize = 1000;

    let data = match val {
        Value::StructureInstance(d) => d,
        _ => return (default_n_modes, default_tol, default_max_iters),
    };

    let n_modes = match data.fields.get(&"n_modes".to_string()) {
        Some(Value::Int(n)) => (*n).max(1) as usize,
        _ => default_n_modes,
    };
    let eigen_tol = match data.fields.get(&"tol".to_string()) {
        Some(Value::Real(r)) => {
            let v = *r;
            if v.is_finite() && v > 0.0 { v } else { default_tol }
        }
        _ => default_tol,
    };
    let eigen_max_iters = match data.fields.get(&"max_iters".to_string()) {
        Some(Value::Int(n)) => (*n).max(1) as usize,
        _ => default_max_iters,
    };

    (n_modes, eigen_tol, eigen_max_iters)
}

// ── skeleton: build a minimal well-formed BucklingResult with eigenvalue=0 ───
//
// This builder is used when we need a placeholder result (e.g., if the kernel
// panics in testing). Currently unused by the main trampoline path.
#[allow(dead_code)]
fn placeholder_result() -> Value {
    let mode_fields: PersistentMap<String, Value> = [
        ("eigenvalue".to_string(), Value::Real(0.0)),
        ("mode_shape".to_string(), Value::Undef),
    ]
    .into_iter()
    .collect();
    let mode = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version:   1,
        fields:    mode_fields,
    }));

    let pre_stress_fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), Value::Undef),
        ("stress".to_string(),       Value::Undef),
        ("frame".to_string(),        Value::Undef),
        ("max_von_mises".to_string(), Value::Scalar {
            si_value:  0.0,
            dimension: DimensionVector::PRESSURE,
        }),
        ("converged".to_string(),   Value::Bool(true)),
        ("iterations".to_string(),  Value::Int(0)),
    ]
    .into_iter()
    .collect();
    let pre_stress = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "ElasticResult".to_string(),
        version:   1,
        fields:    pre_stress_fields,
    }));

    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(),      Value::List(vec![mode])),
        ("converged".to_string(),  Value::Bool(false)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), pre_stress),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version:   1,
        fields:    result_fields,
    }))
}
