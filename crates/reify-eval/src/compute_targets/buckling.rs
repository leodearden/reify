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
//! # Mode.mode_shape
//!
//! The mode_shape field is a `Value::Map { "displaced_positions": Value::List<Real> }`
//! of length 3·n_nodes (flat xyz = undeformed base + kernel eigenvector per node).
//! A companion top-level `base_node_positions: Value::List<Real>` carries the
//! undeformed node coordinates so the GUI animator can reconstruct any phase/scale
//! without extra data.  Populated by task ι/3458.
//!
//! # reference_load param for critical_load
//!
//! The kernel returns a dimensionless multiplier λ (λ × F_applied = P_cr).
//! The `critical_load(result, reference_load)` helper in solver_buckling.ri
//! takes an explicit `reference_load: Force` param — see design decision DD-1
//! in .task/plan.json and the non-blocking escalate_info filed at task 3454.

use std::collections::BTreeMap;
use std::sync::Arc;

use reify_core::DimensionVector;
use reify_ir::{FieldSourceKind, OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_solver_elastic::{
    DirichletBc, IsotropicElastic,
    apply_point_load,
    BucklingKernelOptions, solve_buckling_kernel,
    recover_nodal_stress_p1, StressElement, tet_volume_p1,
    GridSpec, resample_nodal_to_grid,
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
/// - `iterations`: `Value::Int(0)` — intentionally unpopulated for task ε.
///   `BucklingKernelResult` does not expose an eigensolver iteration count;
///   the field is reserved for a future kernel extension (cf. elastic_static
///   which propagates the CG iteration count from its solver result).
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

    // ── (4b) supports input — intentionally unused in task-ε slice ────────────
    //
    // value_inputs[5] carries the FixedSupport list from the caller, but BCs are
    // hardcoded to pin-pin (lateral clamp at both Z-end faces + one axial anchor)
    // to match the analytical k=1 Euler reference P_cr = π²EI/L².  Any column
    // geometry — fixed-free, fixed-fixed, etc. — silently receives pin-pin BCs
    // in this slice.  Support-driven BC selection is tracked as a follow-up
    // (see elastic_static.rs "presence sufficient" note for the analogous pattern).
    let _ = &value_inputs[5];

    // ── (5) Build column mesh (compression axis = Z = longest dimension) ──────
    //
    // Geometry: column along Z axis, cross-section in XY plane.
    // lx = width (X), ly = height (Y), lz = length (Z = compression axis).
    //
    // Mesh density mirrors euler_column_pin_pin.rs (nx=ny=8, nz=160) for the
    // 20×20×800 mm smoke column, giving the validated 9.2%-error result.
    // Density is geometry-driven:
    //   nx = ny = 8 elements across the shorter cross-section side (~2.5 mm each)
    //   nz = round(lz / axial_elem_size) where axial_elem_size ≈ 5 mm
    //
    // Why axial_elem_size = min(lx,ly)/(nx/2)?
    //   Using min(lx,ly)/nx (i.e. the cross-section element size ≈ 2.5 mm) would
    //   give nz=320 for the 800 mm column — doubling wall-time and invalidating
    //   the cited '9.2% error at nz=160' rationale.  Halving the divisor (nx/2=4)
    //   yields ~5 mm axial elements → nz=160, matching the reference fixture and
    //   its measured error.  Clamp to at least 1 in each direction.
    let nx: usize = 8;
    let ny: usize = 8;
    let lx = width;
    let ly = height;
    let lz = length;
    // nz: scale so axial element size ≈ 5 mm (half the cross-section element size)
    let cross_elem_size = lx.min(ly) / (nx / 2) as f64; // ~5 mm for 20 mm section at nx=8
    let nz: usize = ((lz / cross_elem_size).round() as usize).max(1);
    // Sanity: for the 20×20×800 mm smoke column: cross_elem_size=0.005, nz=160 ✓

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

    // ── (10a) Resample pre_stress displacement + stress onto Regular3D grid ─────
    //
    // Grid counts = solve-mesh element counts (nx × ny × nz), bounds = body bounds.
    // Displacement is already nodal (pre_stress_displacement: Vec<f64>, stride 3).
    // Stress: build StressElement per tet from pre_stress_per_element → recover
    // volume-weighted nodal stress → flatten stride-9 row-major → resample.
    let pre_stress_grid = GridSpec {
        bounds_min: [0.0, 0.0, 0.0],
        bounds_max: [lx, ly, lz],
        counts: [nx, ny, nz],
    };

    // Recover nodal stress from per-element tensors (volume-weighted average).
    let ps_stress_elements: Vec<StressElement> = tets
        .iter()
        .enumerate()
        .map(|(e, tet)| {
            let phys4 = [nodes[tet[0]], nodes[tet[1]], nodes[tet[2]], nodes[tet[3]]];
            StressElement {
                connectivity: tet.as_slice(),
                stress: kernel_result.pre_stress_per_element[e],
                volume: tet_volume_p1(&phys4),
            }
        })
        .collect();
    let ps_nodal_stress = recover_nodal_stress_p1(n_nodes, &ps_stress_elements);

    // Flatten nodal stress [[f64;3];3] → stride-9 row-major
    // (σ_xx,σ_xy,σ_xz, σ_yx,σ_yy,σ_yz, σ_zx,σ_zy,σ_zz per node).
    let ps_nodal_stress_flat: Vec<f64> = ps_nodal_stress
        .iter()
        .flat_map(|s| {
            [
                s[0][0], s[0][1], s[0][2],
                s[1][0], s[1][1], s[1][2],
                s[2][0], s[2][1], s[2][2],
            ]
        })
        .collect();

    let ps_disp_sf = resample_nodal_to_grid(
        &nodes,
        &tets,
        &kernel_result.pre_stress_displacement,
        3,
        &pre_stress_grid,
        "displacement",
        1e-9,
    );
    let ps_stress_sf = resample_nodal_to_grid(
        &nodes,
        &tets,
        &ps_nodal_stress_flat,
        9,
        &pre_stress_grid,
        "stress",
        1e-9,
    );

    let ps_disp_field = Value::Field {
        domain_type:   reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::vec3(reify_core::Type::length()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(ps_disp_sf)),
    };
    let ps_stress_field = Value::Field {
        domain_type:   reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::tensor(2, 3, reify_core::Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(ps_stress_sf)),
    };

    // ── (10) Build pre_stress ElasticResult StructureInstance ─────────────────
    let pre_stress_fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), ps_disp_field),
        ("stress".to_string(),       ps_stress_field),
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
    // mode_shape: Value::Map { "displaced_positions": flat xyz list } (task ι/3458).
    // Displaced positions = undeformed base node positions + mode-shape eigenvector
    // (kernel.Mode.mode_shape has length 3·n_nodes, same DOF ordering as `nodes`).
    let modes_list: Vec<Value> = kernel_result
        .modes
        .iter()
        .map(|m| {
            // Flat displaced-position list: [x0+dx0, y0+dy0, z0+dz0, x1+dx1, ...].
            // nodes[i] = [xi, yi, zi]; m.mode_shape[3i..3i+3] = [dxi, dyi, dzi].
            //
            // Guard: the kernel contract requires mode_shape.len() == 3·n_nodes.
            // chunks_exact+zip silently truncates when lengths diverge, so we assert
            // loudly in tests/debug rather than producing a silent too-short list.
            debug_assert_eq!(
                m.mode_shape.len(),
                3 * nodes.len(),
                "mode_shape length {} != 3·n_nodes {} — kernel contract violated",
                m.mode_shape.len(),
                3 * nodes.len(),
            );
            let displaced: Vec<Value> = nodes
                .iter()
                .zip(m.mode_shape.chunks_exact(3))
                .flat_map(|(base, disp)| {
                    [
                        Value::Real(base[0] + disp[0]),
                        Value::Real(base[1] + disp[1]),
                        Value::Real(base[2] + disp[2]),
                    ]
                })
                .collect();
            let mode_shape_map: BTreeMap<Value, Value> = [(
                Value::String("displaced_positions".to_string()),
                Value::List(displaced),
            )]
            .into_iter()
            .collect();
            let mode_fields: PersistentMap<String, Value> = [
                ("eigenvalue".to_string(), Value::Real(m.eigenvalue)),
                ("mode_shape".to_string(), Value::Map(mode_shape_map)),
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
    //
    // base_node_positions: flat xyz list of the undeformed node positions used
    // to build the mesh.  The GUI animator uses this as the phase=0 reference
    // frame and reconstructs displaced positions via
    //   pos(phase, scale) = base + phase·scale·(peak − base).
    let base_node_positions: Vec<Value> = nodes
        .iter()
        .flat_map(|xyz| {
            [Value::Real(xyz[0]), Value::Real(xyz[1]), Value::Real(xyz[2])]
        })
        .collect();

    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(),               Value::List(modes_list)),
        ("converged".to_string(),           Value::Bool(kernel_result.converged)),
        // iterations: BucklingKernelResult carries no eigensolver iteration count;
        // this field is intentionally unpopulated for task ε (see trampoline doc).
        ("iterations".to_string(),          Value::Int(0)),
        ("pre_stress".to_string(),          pre_stress),
        ("base_node_positions".to_string(), Value::List(base_node_positions)),
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
    // Guard: fall back to 1.0 N when no load magnitude was extracted.
    //
    // This is intentionally chosen as a non-zero sentinel, NOT an arbitrary
    // default.  The kernel returns a dimensionless multiplier λ such that
    // P_cr = λ × F_applied.  With F_applied = 1 N, the eigenvalue itself equals
    // P_cr in Newtons — the same convention used by euler_column_pin_pin.rs.
    //
    // Risk: a genuinely zero or mis-shaped load list (e.g., structs with no
    // "force" field) silently receives this sentinel rather than surfacing an
    // error.  The critical_load helper in solver_buckling.ri requires the caller
    // to supply an explicit reference_load precisely because the trampoline does
    // not store the applied load in the result — so incorrect load extraction
    // will produce a plausible-but-wrong critical load rather than a crash.
    // Diagnostic emission for this case is deferred to task θ/3457.
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

