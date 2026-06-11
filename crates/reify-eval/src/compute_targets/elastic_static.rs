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
//! # Determinism contract (PRD task #18)
//!
//! The `ElasticOptions.deterministic : Bool = false` field (read here via
//! `extract_execution_params`, alongside the runtime `threads` knob) selects the
//! assembly + solve execution modes inside `solve_cantilever_fea` through the
//! pure policy fn `reify_solver_elastic::resolve_execution_modes`:
//!
//! - `deterministic == true` forces **single-threaded execution with
//!   fixed-order pairwise-tree reductions** for both `AssemblyMode::Deterministic`
//!   and `SolverMode::Deterministic`, yielding **bit-stable, cross-machine
//!   reproducible** results at a ~4–8× wallclock cost.
//! - `deterministic == false` (the default) lets the resolver pick
//!   `Parallel{threads}` once the problem clears `PARALLEL_DOF_THRESHOLD`
//!   (10_000 DOFs); tiny problems (`n_dofs < threshold`) or `threads <= 1`
//!   still collapse to the Deterministic modes.
//!
//! ## Cache key & determinism
//!
//! `deterministic` (like `threads`) is **excluded from the FEA cache key** by
//! design — it changes the result bit-pattern, not its engineering value. The
//! compute-node key is composed by [`crate::compute_cache_key::compute_cache_key`],
//! whose *exclusion contract* mandates that thread count, determinism mode, and
//! any future "execution profile" flag be filtered out by the upstream
//! `options_hash` producer (`ElasticOptions::cacheable_hash`, deferred to P3.4).
//! Today that producer is not yet wired: the FEA node's `options_hash` is a
//! `ContentHash(0)` placeholder, and the per-call `ElasticOptions(...)` literal
//! is not lowered into the node's `value_inputs` (the γ-slice shallow walk in
//! `engine_eval.rs` captures only direct `ValueRef` args). So *no* `ElasticOptions`
//! field — neither `deterministic` nor `shell_force` — participates in the key
//! yet. This mirrors the mesher's treatment of `MeshingOptions.deterministic`.
//!
//! **Consequence (cache hit vs. bit-stability):** because `deterministic` is not
//! in the key, a `deterministic: true` request can be served a previously-cached
//! result that was produced by a *non-deterministic / parallel* solve. The
//! bit-stability guarantee therefore holds for a **fresh (cold-cache) solve**,
//! not necessarily for a cache hit. A consumer needing a guaranteed bit-stable
//! baseline (e.g. a golden / regression run) must evaluate on a cold cache
//! (fresh engine) rather than expect the flag to invalidate a cached result.
//!
//! ## Default-behavior change (PRD task #16)
//!
//! Before this wiring every elastostatic solve ran unconditionally in the
//! `Deterministic` assembly/solve modes. With the new default `deterministic:
//! false` and `threads: none` (→ host CPU count), any solve that clears
//! `PARALLEL_DOF_THRESHOLD` (10_000 DOFs) now runs `Parallel` and is no longer
//! bit-stable across runs/machines by default. All in-tree FEA solves use the
//! coarse cantilever mesh (≈2.5K DOFs < threshold) and so still resolve to
//! Deterministic; no existing golden/regression test depends on bit-stable
//! output for a >10K-DOF default-options solve. A large-mesh consumer that needs
//! reproducibility must pass `ElasticOptions(deterministic: true)`.
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
//! A `deterministic == true` solve **ignores any recovered warm state** and
//! starts cold (zero initial guess): a warm state produced by an earlier
//! parallel / run-varying solve would otherwise perturb the CG iteration count
//! and break the bit-stability guarantee (see the Determinism contract above).
//! It still donates a fresh `CgWarmState` back for any later non-deterministic
//! solve to reuse.
//!
//! # Cache-hit contract (§3 + significance_filter.rs)
//!
//! `significance_filter::is_opted_in("solver::elastic_static")` returns `true`,
//! registering this target in the v1 significance-filter allowlist
//! (see `significance_filter::is_opted_in`).  However, the tolerance-based output
//! significance filter (`significance_filter::significance_filter`) is **not yet
//! wired** into the live cache path — it has no production caller.  Wiring it
//! (the P3.3 freshness-walk hook that would invoke `significance_filter` via
//! `Engine::active_tolerance_for`) is deferred to task 3382.  Until then the
//! in-memory cache-hit signal relies solely on the EXACT-hash §8-η Final-gate,
//! not on tolerance equivalence.
//!
//! **Cache-hit mechanism (§8-η / §3 Final-gate):** the `evaluate_let_bindings`
//! loop in `engine_eval.rs` carries a pre-dispatch Final-gate (see the
//! `§8-η FINAL-GATE` comment banner in `engine_eval.rs`). When all inputs are
//! `Freshness::Final` and the output VC is also already `Freshness::Final` from a
//! prior `Engine::eval()`, the gate short-circuits re-dispatch and returns the
//! cached `CachedResult::Value` directly.  This is the in-memory cache-hit path
//! that prevents redundant FEA solves across successive `eval()` calls on the
//! same `CompiledModule`.
//!
//! The integration test `e2e_cantilever_second_eval_hits_cache` (step-9) verifies
//! this contract: `DISPATCH_COUNT` must equal 1 after two sequential `engine.eval()`
//! calls on the same module — the gate is the §8-η Final-gate in `engine_eval.rs`,
//! not the significance filter (which is not yet wired).
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
//! # Field-population contract (task 4084/α)
//!
//! The returned `ElasticResult` StructureInstance populates the following fields:
//!
//! - **`displacement`** — `Value::Field{source:Sampled, domain:point3<Length>,
//!   codomain:vec3<Length>}` backed by a `SampledField{kind:Regular3D}`.
//!   `data.len() == grid_count × 3`; layout is row-major x-outer/z-inner, with
//!   the 3 displacement components (dx, dy, dz) stored contiguously per grid point.
//!   Every grid point lies inside the solid (prismatic box), so all samples are finite
//!   (no NaN sentinels for the cantilever geometry).
//!
//! - **`stress`** — `Value::Field{source:Sampled, domain:point3<Length>,
//!   codomain:tensor(2,3,Pressure)}` backed by a `SampledField{kind:Regular3D}`.
//!   `data.len() == grid_count × 9`; layout is row-major x-outer/z-inner, with
//!   the 9 stress components (σ_xx,σ_xy,σ_xz, σ_yx,σ_yy,σ_yz, σ_zx,σ_zy,σ_zz)
//!   stored contiguously per grid point.  Out-of-solid grid points carry `f64::NAN`
//!   for all 9 components (the PRD §3 outside-solid sentinel).
//!
//! - **`frame`** — `Value::Undef` (tet/solid: stress is in the global Cartesian
//!   frame; no per-element local frame to report).
//!
//! - **`shell_channels`** — `Value::Undef` (solid elements have no through-thickness
//!   top/mid/bottom channels; task #4067/δ populates this on the shell path).
//!
//! - **`max_von_mises`** — `Value::Scalar{dimension:PRESSURE}` holding the
//!   ELEMENT-MAX von Mises (unchanged by α; loop over per-element stresses).
//!
//! ## Grid-resolution rule
//!
//! Grid counts = solve-mesh element counts `(nx, ny, nz)`, so grid nodes = `(nx+1, ny+1, nz+1)`.
//! Grid spans body bounds `[0,length] × [0,width] × [0,height]`.
//! `spacing[i] = (bounds_max[i] - bounds_min[i]) / counts[i]`; `axis_grids` built via
//! `linspace_inclusive`.  For a fixed `(geometry, element_order, mesh_size)`, two
//! `engine.eval()` calls produce bit-identical `bounds_min/max/spacing/axis_grids`
//! (`grids_equal` holds), which is required by `envelope_*/linear_combine` (β/ζ/η).
//!
//! ## Determinism
//!
//! Field construction uses row-major index loops only (no `HashMap` iteration, no
//! `Date`/`random`).  The §8-η Final-gate (engine_eval.rs) preserves `DISPATCH_COUNT==1`
//! across successive `eval()` calls on the same module (the gate keys on
//! `Freshness::Final` state, independent of output value shape).
//!
//! TODO: thread `StructureRegistry` through the trampoline signature (tracked
//! by task 4552) once ComputeFn/ComputeOutcome are moved into reify-ir.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector};
use reify_ir::{
    FieldSourceKind, InterpolationKind, OpaqueState, PersistentMap, SampledField, SampledGridKind,
    StructureInstanceData, StructureTypeId, Value,
};

use crate::persistent_cache::ShellChannels;
use reify_solver_elastic::{
    AnisotropicMaterial, AssemblyElement, CgIterationControl, CgSolverOptions, CgWarmState,
    ConstantField, DirichletBc, ElementOrder, FaceOrder, GridSpec, IsotropicElastic,
    OrthotropicMaterial, StressElement, TransverseIsotropicMaterial,
    apply_dirichlet_row_elimination, apply_point_load, apply_traction_load,
    assemble_global_stiffness, element_stiffness, element_stiffness_p1_with_field,
    element_stress_p1, recover_nodal_stress_p1, resample_multi_nodal_to_grid,
    resolve_execution_modes, solve_cg_with_warm_state, solve_cg_with_warm_state_progress,
    tet_volume_p1,
};

use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

// Shell-route classification + the reify-eval-side shell-solve orchestrator
// (task 3594/δ). `solve_shell_static` is referenced via its full path at the
// call site to keep the shell branch visually self-contained.
use super::shell_solve::{
    FailurePolicy, ShellForce, ShellRoute, classify_shell, is_too_thick_for_shell,
    resolve_extraction_failure,
};

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
// `u`, `coords`, `tip_nodes`, `tet_connectivity`, `nodal_stress`, and
// `nx/ny/nz` are read in `#[cfg(test)]` code; the lib-target dead_code lint
// fires because it doesn't see test-only reads.
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
    /// Tet connectivity (length n_tets = nx·ny·nz·6).
    /// Added by task 4084/α: exposed for GridSpec construction + stress assembly.
    pub tet_connectivity: Vec<[usize; 4]>,
    /// Volume-weighted nodal stress field (length n_nodes).
    /// Each entry is the recovered 3×3 Cauchy stress at that node.
    /// Added by task 4084/α: fed stride-9 row-major into resample_nodal_to_grid.
    pub nodal_stress: Vec<[[f64; 3]; 3]>,
    /// Number of element intervals along x (beam length axis).
    pub nx: usize,
    /// Number of element intervals along y (beam width axis).
    pub ny: usize,
    /// Number of element intervals along z (beam height axis).
    pub nz: usize,
}

// ── Progress-emit throttle ────────────────────────────────────────────────────

/// Emit a `SolverProgressUpdate` on iteration 1 and every `PROGRESS_STRIDE`
/// iterations thereafter, bounding IPC overhead for non-converging solves
/// (e.g. max_iter=2000 → ≤200 sink calls rather than 2000).
///
/// Exposed `pub(crate)` so integration tests can assert cadence without
/// duplicating the constant.  Re-exported via `#[doc(hidden)] pub use` in
/// `lib.rs` for tests/ access.
pub const PROGRESS_STRIDE: usize = 10;

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
/// Returns an `ElasticResult`-shaped `Value::StructureInstance`. The populated
/// field set depends on the route (both carry `max_von_mises` (Scalar[PRESSURE]),
/// `converged` (Bool), `iterations` (Int)). **`max_von_mises` has the SAME
/// semantics on both routes — the body's peak von Mises stress** — computed as
/// the tet path's max over the solid stress field, or the shell path's max over
/// all three through-thickness channels (top/mid/bottom); it is NOT a
/// channel-specific summary (esc-3594 suggestion 4):
///   - **Tet/solid path** (task 4084/α): `displacement` + `stress` are populated
///     Regular3D Sampled `Value::Field`s; `frame` + `shell_channels` are `Undef`
///     (no per-element local frame / through-thickness data for solid elements —
///     `solver_elastic.ri` field-semantics doc, PRD DR-3 / #4067 I-3).
///   - **Shell path** (task 3594/δ, the §3b early return below): `shell_channels`
///     is a real `ShellStress` value (`shell_channels_to_value(Some(_), mid)`),
///     `stress` aliases `shell_channels.mid` (I-2), and `displacement` + `frame`
///     are `Undef` (per-element shell displacement + global-frame surfacing is
///     task θ; the local→global frames ride in `ShellChannels.frame`).
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
    let width = extract_scalar_si(&value_inputs[2]);
    let height = extract_scalar_si(&value_inputs[3]);

    // ── (3) Extract loads from value_inputs[4] (List of StructureInstances) ──
    //
    // Two load kinds are bridged here (task 4264), read in a single pass by
    // `extract_loads`:
    //
    //   PointLoad   — `force: Real` → scalar tip_force (distributed as -Z point
    //                 loads across the tip-face nodes via apply_point_load).
    //
    //   PressureLoad — `magnitude: Real, face: String, direction: String` →
    //                 face-traction assembled via apply_traction_load(f,
    //                 FaceOrder::P1Tri, …) into the same f vector.
    //                 Supported face selectors: x_min, x_max, y_min, y_max,
    //                 z_min, z_max. Unknown/empty face → silent no-op (v1).
    //
    // Both accumulate into disjoint targets and compose: a scene may mix
    // PointLoad and PressureLoad in the same LoadCase.
    let (tip_force, pressures) = extract_loads(&value_inputs[4]);

    // ── (3b) Shell-route dispatch (task 3594/δ) ──────────────────────────────
    //
    // Classify the body from `ElasticOptions.shell_force` (value_inputs[6]) and
    // the thickness/extent ratio vs `shell_threshold`. On the Shell route,
    // assemble through the MITC3 shell kernel (`solve_shell_static`) and return
    // an ElasticResult carrying a real `ShellStress` `shell_channels` EARLY —
    // before the §7a 4084/α tet resampling below. On the Tet route this `if`
    // is skipped and execution falls through to the existing solid path,
    // byte-identical to the pre-δ trampoline.
    //
    // Shells are an isotropic-material formulation in v0.4, so the branch is
    // gated on `MaterialModel::Isotropic`; an anisotropic material (or a Tet
    // classification) falls through to the solid path. The upstream
    // `shell-extract::extract` graph dependency is wired by the engine_eval
    // lowering (step-12), not here (PRD §11 OQ-2).
    let (shell_force, shell_threshold) = extract_shell_route_params(&value_inputs[6]);
    let shell_route = classify_shell(shell_force, length, width, height, shell_threshold);

    // Diagnostics accrued by the shell-route material-compatibility policy
    // (esc-3594 suggestion 3). The v0.4 MITC3 shell kernel is an ISOTROPIC
    // formulation, so a Shell classification on a non-isotropic material cannot
    // be honoured by the shell path. Rather than SILENTLY falling through to the
    // tet/solid path (which contradicts the documented `ShellForce::On` hard-
    // error intent), apply the `resolve_extraction_failure` policy: `On` aborts
    // (the user demanded a shell solve — no silent fallback), `Auto`/`Off` fall
    // back to tet with a VISIBLE warning carried to the final ComputeOutcome.
    let mut route_diagnostics: Vec<Diagnostic> = Vec::new();
    if shell_route == ShellRoute::Shell && !matches!(model, MaterialModel::Isotropic(_)) {
        let policy = resolve_extraction_failure(shell_force);
        let msg = format!(
            "shell solve requested (shell_force={shell_force:?}) but the material is \
             non-isotropic; the v0.4 MITC3 shell kernel supports isotropic materials only — {}",
            match policy {
                FailurePolicy::HardError =>
                    "aborting with no tet fallback (ShellForce::On hard-error)",
                FailurePolicy::TetFallbackWithWarning => "falling back to the tet/solid path",
            },
        );
        match policy {
            FailurePolicy::HardError => {
                return ComputeOutcome::Failed {
                    diagnostics: vec![Diagnostic::error(msg)],
                };
            }
            FailurePolicy::TetFallbackWithWarning => {
                route_diagnostics.push(Diagnostic::warning(msg));
            }
        }
    }

    // ── Too-thick dispatch-site policy (task ε #3837, PRD §7 rows 1–2) ─────────
    //
    // Gate placed AFTER `classify_shell` and BEFORE the shell-kernel branch.
    // Mirrors the non-isotropic-material policy block above (same `resolve_
    // extraction_failure` dispatch, same `route_diagnostics` vehicle).
    //
    // `is_too_thick_for_shell` shares `classify_shell`'s exact metric so the
    // route (Shell/Tet) and the too-thick flag can never contradict each other:
    //   - `On` + thin-enough  → Shell  + not-too-thick → shell solve runs.
    //   - `On` + too-thick    → Shell  + too-thick     → Hard Error (abort).
    //   - `Auto` + too-thick  → Tet    + too-thick     → Warning + tet fallback
    //     (step-8; classify_shell already routed Tet for this body, so the
    //      shell branch below is skipped even without an early return here).
    //   - `Off`               → Tet    (regardless of thickness; silent).
    // `is_too_thick_for_shell` returns `Some(ratio)` when too thick so the
    // decision and the message value come from one source — no local
    // re-derivation of `in_plane` / ratio needed (esc-3837 suggestion 4).
    if let Some(ratio) = is_too_thick_for_shell(length, width, height, shell_threshold) {
        let policy = resolve_extraction_failure(shell_force);
        match policy {
            FailurePolicy::HardError => {
                // ShellForce::On (@shell): hard-error — no tet fallback.
                // §7 message names ShellForce.Off / @solid as the opt-out.
                return ComputeOutcome::Failed {
                    diagnostics: vec![
                        Diagnostic::error(format!(
                            "body thickness/extent ratio {ratio:.2} ≥ shell_threshold {shell_threshold:.2}: \
                             body is too thick for shell solve (ratio must be < {shell_threshold:.2}). \
                             Use ElasticOptions(shell_force: ShellForce.Off) / @solid to suppress this error."
                        ))
                        .with_code(DiagnosticCode::ShellTooThick),
                    ],
                };
            }
            FailurePolicy::TetFallbackWithWarning => {
                // `ShellForce::Auto`: warn and fall through to the tet path.
                // `classify_shell` already routed Tet for Auto+too-thick (ratio ≥
                // threshold), so the shell branch below is skipped and
                // shell_channels stays Undef→None.  The warning surfaces via the
                // existing `route_diagnostics` vehicle.
                //
                // `ShellForce::Off` (@solid): SILENT — the §7 message names
                // `ShellForce.Off` / @solid as the explicit opt-out, so a body
                // solved with @solid never receives a ShellTooThick warning
                // regardless of its thickness.
                if shell_force == ShellForce::Auto {
                    route_diagnostics.push(
                        Diagnostic::warning(format!(
                            "body thickness/extent ratio {ratio:.2} ≥ shell_threshold \
                             {shell_threshold:.2}: body is too thick for shell solve \
                             (ratio must be < {shell_threshold:.2}); falling back to the \
                             tet/solid path. Use ElasticOptions(shell_force: ShellForce.Off) \
                             / @solid to suppress this warning."
                        ))
                        .with_code(DiagnosticCode::ShellTooThick),
                    );
                }
                // ShellForce::Off: no diagnostic (silent opt-out).
            }
        }
    }

    if let (ShellRoute::Shell, MaterialModel::Isotropic(iso)) = (shell_route, &model) {
        // Shell kernel takes a scalar transverse force; X/Y components are
        // ignored on the shell route (in-plane directional shell loading is
        // out of scope — task 4245 cylinder/PressureLoad exclusion).
        //
        // Warn when directional PointLoad(s) carry non-negligible in-plane
        // (X/Y) force so the silent discard is visible to the caller.
        // Threshold: XY magnitude > 1 ppm of Z magnitude (or 1 pN absolute),
        // which avoids noise from floating-point rounding near exact-zero.
        let xy_mag = (tip_force[0] * tip_force[0] + tip_force[1] * tip_force[1]).sqrt();
        let z_ref = tip_force[2].abs().max(1e-12);
        if xy_mag > z_ref * 1e-6 {
            route_diagnostics.push(Diagnostic::warning(format!(
                "PointLoad has non-negligible in-plane force components \
                 (fx={:.3e}, fy={:.3e}) on the shell route; only the \
                 transverse -Z component (fz={:.3e}) is applied. \
                 In-plane shell loading is out of scope in this release \
                 (task 4245). Use the tet/solid path for in-plane loads.",
                tip_force[0], tip_force[1], tip_force[2],
            )));
        }
        let shell_tip_force = -tip_force[2];
        let (channels, mid_field, max_von_mises, converged, iterations) =
            super::shell_solve::solve_shell_static(length, width, height, iso, shell_tip_force);

        // `shell_channels_to_value` clones `mid_field` into the ShellStress.mid
        // field, so the same field becomes BOTH `result.stress` and
        // `shell_channels.mid` — the I-2 alias (their SampledField data are
        // element-wise equal). This is the `Some(_)` arm of the 4067-shipped
        // helper; the tet path keeps using its `None`→Undef arm untouched.
        let shell_channels = shell_channels_to_value(&Some(channels), &mid_field);

        let fields: PersistentMap<String, Value> = [
            // Per-element shell displacement resampling is out of scope (task θ);
            // Undef is the accepted sentinel against the Field-typed DSL param.
            ("displacement".to_string(), Value::Undef),
            ("stress".to_string(), mid_field),
            // Per-element local→global frames are carried inside
            // `ShellChannels.frame` for the GUI populator (task θ); the top-level
            // `frame` field stays Undef.
            ("frame".to_string(), Value::Undef),
            ("shell_channels".to_string(), shell_channels),
            (
                "max_von_mises".to_string(),
                Value::Scalar {
                    si_value: max_von_mises,
                    dimension: DimensionVector::PRESSURE,
                },
            ),
            ("converged".to_string(), Value::Bool(converged)),
            ("iterations".to_string(), Value::Int(iterations as i64)),
        ]
        .into_iter()
        .collect();

        let result = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticResult".to_string(),
            version: 1,
            fields,
        }));

        // One-shot shell solve: the shell kernel runs its own cold CG, so no
        // `CgWarmState` is donated back (warm-state caching is tet-only in v0.4).
        // `route_diagnostics` carries any XY-force-on-shell warning emitted above.
        return ComputeOutcome::Completed {
            result,
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: route_diagnostics,
        };
    }

    // ── (4) Supports: non-empty list → cantilever is clamped at root ─────────
    // (We don't inspect individual FixedSupport fields; presence is sufficient.)

    // ── (5) Recover prior warm state ─────────────────────────────────────────
    // `OpaqueState` has no `Clone`, so recover via `downcast_ref` + `CgWarmState::clone`
    // (cheap — cloning Arc bumps a refcount, not the Vec payload).
    let prior_cg = prior_warm_state.and_then(|s| s.downcast_ref::<CgWarmState>().cloned());

    // ── (6) Delegate to shared FEA helper ────────────────────────────────────
    //
    // Read the thread-local dispatch context installed by `run_compute_dispatch`
    // (task 4079). When a `SolverProgressSink` or cancel handle is present, build
    // a per-iteration closure that: (a) emits a `SolverProgressUpdate` to the sink
    // (throttled — see PROGRESS_STRIDE), THEN (b) polls the externally-set cancel
    // handle and returns `Cancel` if set.  The sink has no access to the cancel
    // handle and cannot raise a cancel through `on_iteration`; the emit-then-poll
    // ordering simply ensures each iteration is reported before cancellation is
    // checked.  The cancel check is NOT throttled so interruption is responsive.
    let ctx = crate::solver_progress::current_solve_dispatch_context();
    let (ctx_sink, ctx_cancel) = match ctx {
        Some((s, c)) => (s, c),
        None => (None, None),
    };
    // Emit every PROGRESS_STRIDE iterations (and always on iter 1) to bound IPC
    // overhead — a non-converging solve with max_iter=2000 fires at most
    // ~200 emit calls rather than 2000.  The cancel poll is unaffected.
    // (`PROGRESS_STRIDE` is the module-level pub(crate) const above.)
    let mut progress_closure = |iter: usize, residual: f64| -> CgIterationControl {
        if let Some(ref sink) = ctx_sink
            && (iter == 1 || iter.is_multiple_of(PROGRESS_STRIDE))
        {
            sink.on_iteration(&crate::solver_progress::SolverProgressUpdate {
                solver_kind: "cg",
                iter: iter as u32,
                residual,
            });
        }
        if ctx_cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            CgIterationControl::Cancel
        } else {
            CgIterationControl::Continue
        }
    };
    let progress_opt: Option<&mut dyn FnMut(usize, f64) -> CgIterationControl> =
        if ctx_sink.is_some() || ctx_cancel.is_some() {
            Some(&mut progress_closure)
        } else {
            None
        };
    // Execution-mode knobs (task 2926): `ElasticOptions.deterministic` + `threads`
    // (value_inputs[6]) select the assembly/CG SolverMode inside the helper via
    // `resolve_execution_modes`. The flag is intentionally excluded from the FEA
    // cache key (the trampoline does not hash ElasticOptions).
    let (deterministic, threads_opt) = extract_execution_params(&value_inputs[6]);
    let (fea, fresh_warm) = solve_cantilever_fea(
        &model, length, width, height, tip_force, prior_cg, &pressures, deterministic, threads_opt,
        progress_opt,
    );

    // ── (6b) Cancel check ─────────────────────────────────────────────────────
    //
    // After the solve, if the cancel handle was triggered, return `Cancelled`
    // so that `run_compute_dispatch` leaves the output VC `Freshness::Pending`
    // (per compute-node-contract §2 — no bogus partial result cached).
    if ctx_cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
        return ComputeOutcome::Cancelled;
    }

    // ── (7) Build ElasticResult StructureInstance ────────────────────────────
    //
    // StructureTypeId(u32::MAX) is a synthetic sentinel for this slice.
    //   - displacement / stress: Regular3D Sampled Value::Field (task 4084/α).
    //   - frame:         Undef — tet stress is in the global Cartesian frame;
    //                    no per-element local frame exists for solid elements.
    //   - shell_channels: Undef — through-thickness top/mid/bottom is undefined
    //                    for solid elements (PRD DR-3, task #4067 I-3). The shell
    //                    path emits a real ShellStress here instead, via the §3b
    //                    early return above (task 3594/δ).
    // `cost_per_byte` is derived as 1/(warm-state size in bytes).
    let n_iters = fea.iterations as i64;
    let converged = fea.converged;
    let size_bytes = fresh_warm.estimated_size_bytes();
    // cost_per_byte: reciprocal of warm-state size — a bigger state is pricier
    // to keep. Tuners should replace this with a profiling-derived estimate.
    let cost_per_byte = if size_bytes > 0 {
        Some(1.0 / size_bytes as f64)
    } else {
        None
    };
    let new_warm_state = Some(fresh_warm.into_opaque_state());

    // ── (7a) Resample displacement + stress onto a Regular3D grid ────────────
    //
    // Grid counts = solve-mesh element counts (nx × ny × nz); grid nodes =
    // counts + 1 per axis; bounds = [0, length] × [0, width] × [0, height].
    // This mirrors the PRD §4.1 grid-metadata invariant and ensures grid points
    // coincide with FEA nodes for the prismatic box (linspace(0,L,L/nx) = node
    // coords) — enabling the Kronecker-δ accuracy proven in plan design_decision[1].
    let grid = GridSpec {
        bounds_min: [0.0, 0.0, 0.0],
        bounds_max: [length, width, height],
        counts: [fea.nx, fea.ny, fea.nz],
    };

    // Flatten nodal stress [[f64;3];3] → stride-9 row-major.
    // Layout: σ_xx,σ_xy,σ_xz, σ_yx,σ_yy,σ_yz, σ_zx,σ_zy,σ_zz per node.
    let nodal_stress_flat = super::flatten_nodal_stress(&fea.nodal_stress);

    // Single geometry pass: locate the containing tet once per grid point,
    // then interpolate both displacement (stride 3) and stress (stride 9).
    // This halves the O(grid·elems) point-location cost vs. two separate calls.
    let mut sampled = resample_multi_nodal_to_grid(
        &fea.coords,
        &fea.tet_connectivity,
        &[
            (&fea.u, 3, "displacement"), // Arc<Vec<f64>> → &[f64] via Deref
            (&nodal_stress_flat, 9, "stress"),
        ],
        &grid,
        1e-9,
    );
    debug_assert_eq!(
        sampled.len(),
        2,
        "expected 2 sampled fields (displacement + stress)"
    );
    let stress_sf = sampled.pop().unwrap(); // index 1
    let disp_sf = sampled.pop().unwrap(); // index 0

    let disp_field = super::sampled_disp_field(disp_sf);
    let stress_field = super::sampled_stress_field(stress_sf);

    let fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), disp_field),
        ("stress".to_string(), stress_field),
        ("frame".to_string(), Value::Undef),
        // task #4067 (PRD S1 / DR-3 / I-3): tet/solid results always emit
        // shell_channels=Undef (through-thickness data is undefined for solid
        // elements). The shell path emits a real ShellStress via
        // shell_channels_to_value(Some(_), mid) in the §3b early return (task 3594/δ).
        ("shell_channels".to_string(), Value::Undef),
        (
            "max_von_mises".to_string(),
            Value::Scalar {
                si_value: fea.max_von_mises,
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("converged".to_string(), Value::Bool(converged)),
        ("iterations".to_string(), Value::Int(n_iters)),
    ]
    .into_iter()
    .collect();

    let result = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticResult".to_string(),
        version: 1,
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
    // `diagnostics`   — `route_diagnostics`: empty on the normal tet path (CG
    //                   convergence failures are reflected in `converged =
    //                   Bool(false)`), or a Warning when a Shell-classified
    //                   non-isotropic body soft-fell-back to tet under
    //                   `Auto`/`Off` (esc-3594 suggestion 3).  The shell path
    //                   also carries `route_diagnostics` (XY-force warning
    //                   emitted by task-4245 esc amendment).
    ComputeOutcome::Completed {
        result,
        new_warm_state,
        cost_per_byte,
        diagnostics: route_diagnostics,
    }
}

// ── shell_channels_to_value ───────────────────────────────────────────────────

/// Map a `Option<ShellChannels>` + the mid-surface stress field into a DSL
/// `ShellStress` `Value::StructureInstance` (task #4067, PRD S1 / DR-1).
///
/// # Contract
///
/// - `None`   → `Value::Undef` (I-3 honest absence: tet/solid results carry no
///   through-thickness channels — PRD DR-3).
/// - `Some(ch)` → a `ShellStress`-shaped `Value::StructureInstance` with three
///   fields:
///   - `mid`    = `mid_stress.clone()` — I-2 invariant: `shell_channels.mid ==
///     `ElasticResult.stress` by construction.
///   - `top`    = `mid_stress` metadata with `data` replaced by `ch.top`.
///   - `bottom` = `mid_stress` metadata with `data` replaced by `ch.bottom`.
///
/// # Rationale for sharing mid_stress's grid
///
/// top/mid/bottom are sampled at the SAME mesh nodes (MITC3+ per-element
/// integration points share the element geometry), so reusing mid's
/// `SampledField` grid/bounds/spacing is physically correct, not a shortcut.
/// Mirrors the metadata-clone/data-swap pattern in `reify-stdlib/src/fea.rs`
/// (`out_stress_sf` construction at ~line 281).
///
/// # Defensive fallback
///
/// When `mid_stress` is not a `Value::Field { source: Sampled, lambda:
/// SampledField(_) }` (shouldn't happen on the shell path but may occur in unit
/// tests or partial results), `top` / `bottom` are built as minimal 1D
/// `SampledField` wrappers over `ch.top` / `ch.bottom` with index-based grids.
///
/// # Called by
///
/// In-file production caller: `solve_elastic_static_trampoline` (wired by done task #3594/δ).
/// Also reached via the elastic-static `ComputeFn` fn-pointer registration which
/// the orphan audit cannot trace — so this fn is permanently 0-external-caller
/// from the audit's perspective (Bucket-1 fn-pointer blind spot).
// G-allow: Bucket-1 fn-pointer ComputeFn registration blind spot; in-file production caller in `solve_elastic_static_trampoline` wired by #3594 (done); shipped by #4067 (done); permanent 0-external-caller by audit design.
pub fn shell_channels_to_value(channels: &Option<ShellChannels>, mid_stress: &Value) -> Value {
    let ch = match channels {
        None => return Value::Undef,
        Some(ch) => ch,
    };

    let top = build_channel_field(mid_stress, ch.top.clone(), "shell_channels_top");
    let bottom = build_channel_field(mid_stress, ch.bottom.clone(), "shell_channels_bottom");

    let fields: PersistentMap<String, Value> = [
        ("mid".to_string(), mid_stress.clone()),
        ("top".to_string(), top),
        ("bottom".to_string(), bottom),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ShellStress".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a `Value::Field { source: Sampled }` carrying `data`, cloning the grid
/// metadata from `template` (the mid-surface stress field) when possible.
///
/// Falls through to a 1D index-grid fallback when `template` is not a Sampled
/// field OR when `data.len()` does not match the grid's node count (product of
/// `axis_grids` lengths). The debug assertion fires in debug/test builds so the
/// mismatch is caught early; release builds silently produce a 1D field instead
/// of a malformed Sampled field that would panic downstream.
fn build_channel_field(template: &Value, data: Vec<f64>, name: &str) -> Value {
    if let Value::Field {
        domain_type,
        codomain_type,
        source: FieldSourceKind::Sampled,
        lambda,
    } = template
        && let Value::SampledField(ref sf) = **lambda
    {
        let expected_len: usize = sf.axis_grids.iter().map(|g| g.len()).product();
        debug_assert_eq!(
            data.len(),
            expected_len,
            "build_channel_field: channel data length {} != grid node count {} for '{}'",
            data.len(),
            expected_len,
            name,
        );
        if data.len() == expected_len {
            let channel_sf = SampledField {
                name: name.to_string(),
                kind: sf.kind,
                bounds_min: sf.bounds_min.clone(),
                bounds_max: sf.bounds_max.clone(),
                spacing: sf.spacing.clone(),
                axis_grids: sf.axis_grids.clone(),
                interpolation: sf.interpolation,
                data,
                oob_emitted: AtomicBool::new(false),
            };
            return Value::Field {
                domain_type: domain_type.clone(),
                codomain_type: codomain_type.clone(),
                source: FieldSourceKind::Sampled,
                lambda: Arc::new(Value::SampledField(channel_sf)),
            };
        }
    }
    // Defensive fallback: template is not a Sampled field, OR data length does
    // not match the grid's node count — wrap data in a minimal 1D index-grid
    // SampledField with Real domain/codomain.
    let n = data.len();
    let axis_grid: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let fallback_sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![n.saturating_sub(1) as f64],
        spacing: vec![1.0],
        axis_grids: vec![axis_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: reify_core::Type::Real,
        codomain_type: reify_core::Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(fallback_sf)),
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
// 9 args: the helper threads mesh geometry, tip load, pressures, CG warm-state,
// and the task-2926 execution-mode knobs (`deterministic`, `threads`) into a
// single cohesive solve; splitting them into a struct would not aid clarity.
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_cantilever_fea(
    model: &MaterialModel,
    length: f64,
    width: f64,
    height: f64,
    tip_force: [f64; 3],
    prior_cg: Option<CgWarmState>,
    pressures: &[PressureSpec],
    deterministic: bool,
    threads: Option<usize>,
    progress: Option<&mut dyn FnMut(usize, f64) -> CgIterationControl>,
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
    let ny1 = ny + 1; // 2 nodes along Y
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize { iz * ny1 * nx1 + iy * nx1 + ix };
    let node_coord = |ix: usize, iy: usize, iz: usize| -> [f64; 3] {
        [
            ix as f64 * length / nx as f64,
            iy as f64 * width / ny as f64,
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
    let mut elem_stiffness_mats: Vec<_> = Vec::with_capacity(n_tets);

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
                    node_idx(hx, hy, hz),             // c[0]: local (0,0,0)
                    node_idx(hx + 1, hy, hz),         // c[1]: local (1,0,0)
                    node_idx(hx + 1, hy + 1, hz),     // c[2]: local (1,1,0)
                    node_idx(hx, hy + 1, hz),         // c[3]: local (0,1,0)
                    node_idx(hx, hy, hz + 1),         // c[4]: local (0,0,1)
                    node_idx(hx + 1, hy, hz + 1),     // c[5]: local (1,0,1)
                    node_idx(hx + 1, hy + 1, hz + 1), // c[6]: local (1,1,1)
                    node_idx(hx, hy + 1, hz + 1),     // c[7]: local (0,1,1)
                ];
                // Six tets sharing diagonal c[0]→c[6]:
                let tets: [[usize; 4]; 6] = [
                    [c[0], c[1], c[2], c[6]], // T0: det = +dx·dy·dz
                    [c[0], c[2], c[3], c[6]], // T1: det = +dx·dy·dz
                    [c[0], c[5], c[1], c[6]], // T2: det = +dx·dy·dz (c[5]↔c[1] swap)
                    [c[0], c[3], c[7], c[6]], // T3: det = +dx·dy·dz
                    [c[0], c[4], c[5], c[6]], // T4: det = +dx·dy·dz
                    [c[0], c[7], c[4], c[6]], // T5: det = +dx·dy·dz (c[7]↔c[4] swap)
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

    // ── Execution-mode selection (task 2926) ─────────────────────────────────
    //
    // `ElasticOptions.deterministic` + `threads` resolve into the assembly and
    // CG-solve modes via the single policy fn `resolve_execution_modes`
    // (reify-solver-elastic): `deterministic ⇒ both Deterministic`; else a tiny
    // problem (`n_dofs < PARALLEL_DOF_THRESHOLD`) or `threads <= 1` also forces
    // Deterministic; otherwise both run `Parallel{threads}`. `threads` defaults
    // to the host CPU count when the caller leaves it `None`. `n_dofs = 3·n_nodes`
    // is only known here, so the resolver must be called inside this helper.
    let threads =
        threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    let n_dofs = 3 * n_nodes;
    let (assembly_mode, solver_mode) = resolve_execution_modes(deterministic, threads, n_dofs);

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

    let mut k = assemble_global_stiffness(n_nodes, &assembly_elements, assembly_mode);

    // ── Build load vector; distribute tip load to tip-face nodes ─────────────
    //
    // Tip face: all nodes at ix == nx (ny1 × nz1 = 2 × 7 = 14 nodes for
    // the 1×6 cross-section mesh). Force is distributed equally in the -Z
    // direction (height/gravity direction). Z is the bending direction.
    let mut f = vec![0.0f64; 3 * n_nodes];
    let tip_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(nx, iy, iz)))
        .collect();
    let n_tip = tip_nodes.len().max(1) as f64;
    let force_per_tip = [tip_force[0] / n_tip, tip_force[1] / n_tip, tip_force[2] / n_tip];
    for &tn in &tip_nodes {
        apply_point_load(&mut f, tn, force_per_tip);
    }

    // ── Face pressure loads (task 4264) ───────────────────────────────────────
    //
    // PressureLoad face tractions are accumulated into the same `f` vector.
    // An empty `pressures` slice is a no-op, preserving the existing tip-only path.
    assemble_box_face_pressures(&mut f, &coords, &tet_connectivity, pressures, length, width, height);

    // ── Dirichlet BCs: clamp all DOFs at root face (ix == 0) ─────────────────
    let root_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(0, iy, iz)))
        .collect();
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for &rn in &root_nodes {
        for axis in 0..3usize {
            bcs.push(DirichletBc {
                dof: 3 * rn + axis,
                value: 0.0,
            });
        }
    }
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    // ── Solve ─────────────────────────────────────────────────────────────────
    let opts = CgSolverOptions {
        tolerance: 1e-6,
        max_iter: 2000,
    };
    // Capture max_iter before `opts` is moved into the solve call (task 4366):
    // the cancel short-circuit below needs it to distinguish cooperative cancel
    // from max-iteration exhaustion without threading an extra parameter.
    let max_iter = opts.max_iter;
    // Determinism contract (task 2926): bit-stability requires a *fixed* CG
    // starting vector. CG converges to the same solution from any initial guess,
    // but the iteration count — and thus the exact bit-pattern of the converged
    // result — depends on it. A warm state carried over from an earlier solve
    // (which may have run in `Parallel`, or simply varied run-to-run) would
    // defeat the cross-run/cross-machine reproducibility guarantee. So a
    // deterministic solve discards any prior warm state and starts cold (zero
    // initial guess), trading a few extra CG iterations for reproducibility;
    // bit-stability then holds for warm- and cold-lineage solves alike.
    //
    // Task 4079: when a progress sink / cancel handle is installed, route through
    // the progress variant so the per-iteration closure (emit + cancel poll) runs;
    // otherwise take the plain no-callback path (byte-identical solve).
    let warm_start = if deterministic { None } else { prior_cg.as_ref() };
    let (cg_result, fresh_warm) = if let Some(cb) = progress {
        solve_cg_with_warm_state_progress(&k, &f, warm_start, opts, solver_mode, cb)
    } else {
        solve_cg_with_warm_state(&k, &f, warm_start, opts, solver_mode)
    };

    // ── Cancel short-circuit (task 4366) ──────────────────────────────────────
    //
    // Skip stress recovery entirely when the solve was cooperatively cancelled.
    //
    // Detection predicate: `!converged && iterations < max_iter`
    //
    // The cg_loop exit-condition contract (solver.rs:994-1045) maps to this
    // predicate as follows:
    //   - Convergence                          → converged = true       (predicate false)
    //   - max_iter exhaustion                  → iterations == max_iter  (predicate false)
    //   - Degenerate system                    → panics on p·Kp > 0     (never reaches here)
    //   - Cooperative cancel at iter < max_iter → converged = false,
    //                                             iterations < max_iter  (predicate TRUE)
    //   - Cooperative cancel at iter == max_iter → converged = false,
    //                                              iterations == max_iter (predicate false)
    //
    // The predicate is true for the overwhelmingly common cancel case.  A cancel
    // firing on the exact final iteration (iter + 1 == max_iter) makes
    // iterations == max_iter so the predicate is false — stress recovery runs
    // on partial displacements, but the §6b post-solve cancel check
    // (elastic_static.rs:~580) still returns ComputeOutcome::Cancelled so
    // correctness is preserved.  The wasted stress-recovery work is accepted for
    // this rare edge case; it does not affect the common-case latency improvement.
    //
    // The no-callback entry point (solve_cg_with_warm_state) can never cancel,
    // so it only reaches non-converged at iterations == max_iter — predicate
    // stays false there too, leaving the existing callers completely unaffected.
    //
    // On the cancelled path the trampoline's §6b post-solve cancel check
    // (elastic_static.rs:~580) returns ComputeOutcome::Cancelled and never reads
    // stress fields, so a stress-less struct is correct.
    let converged = cg_result.converged;
    let iterations = cg_result.iterations;
    if !converged && iterations < max_iter {
        return (
            CantileverFeaSolve {
                u: cg_result.into_shared_u(),
                coords,
                tip_nodes,
                max_von_mises: 0.0,
                converged,
                iterations,
                tet_connectivity,
                nodal_stress: Vec::new(),
                nx,
                ny,
                nz,
            },
            fresh_warm,
        );
    }

    // ── Stress recovery: max von Mises across all elements ────────────────────
    //
    // Isotropic: element_stress_p1 returns symmetric 3×3 Cauchy tensor;
    //   Von Mises: sqrt(½·[(σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)² + 6·(σ_xy²+σ_yz²+σ_zx²)])
    //
    // Anisotropic: mirrors the B-matrix computation inside element_stress_p1
    //   (same engineering-shear Voigt convention) but substitutes D_global for
    //   IsotropicElastic::d_matrix. Von Mises computed from σ_voigt directly.
    // ── Stress recovery: per-element tensor + element-max von Mises ──────────────
    //
    // Isotropic:   element_stress_p1  → 3×3 tensor  → scalar vM from tensor.
    // Anisotropic: element_stress_anisotropic → 3×3 tensor → scalar vM from tensor.
    //
    // Per-element tensors are collected into `stress_elements` to feed
    // recover_nodal_stress_p1 (task 4084/α). The element-max max_von_mises
    // loop is byte-identical to the pre-α code (same formula, same ordering).
    let u_disp = cg_result.u();
    let mut max_von_mises = 0.0f64;
    let mut stress_elements: Vec<[[f64; 3]; 3]> = Vec::with_capacity(tet_connectivity.len());

    for conn in &tet_connectivity {
        let phys: [[f64; 3]; 4] = [
            coords[conn[0]],
            coords[conn[1]],
            coords[conn[2]],
            coords[conn[3]],
        ];
        let u_e: [f64; 12] = [
            u_disp[3 * conn[0]],
            u_disp[3 * conn[0] + 1],
            u_disp[3 * conn[0] + 2],
            u_disp[3 * conn[1]],
            u_disp[3 * conn[1] + 1],
            u_disp[3 * conn[1] + 2],
            u_disp[3 * conn[2]],
            u_disp[3 * conn[2] + 1],
            u_disp[3 * conn[2] + 2],
            u_disp[3 * conn[3]],
            u_disp[3 * conn[3] + 1],
            u_disp[3 * conn[3] + 2],
        ];
        let sigma: [[f64; 3]; 3] = match model {
            MaterialModel::Isotropic(iso) => element_stress_p1(&phys, iso, &u_e),
            MaterialModel::Anisotropic(_) => {
                // Use the hoisted d_global (computed once above).
                element_stress_anisotropic(&phys, &aniso_precomp.as_ref().unwrap().1, &u_e)
            }
        };

        // vM from the tensor (byte-identical to the pre-α scalar calculation).
        let (sxx, syy, szz) = (sigma[0][0], sigma[1][1], sigma[2][2]);
        let (sxy, syz, szx) = (sigma[0][1], sigma[1][2], sigma[0][2]);
        let vm = f64::sqrt(
            0.5 * ((sxx - syy).powi(2)
                + (syy - szz).powi(2)
                + (szz - sxx).powi(2)
                + 6.0 * (sxy * sxy + syz * syz + szx * szx)),
        );

        stress_elements.push(sigma);
        if vm > max_von_mises {
            max_von_mises = vm;
        }
    }

    // ── Recover nodal stress field (volume-weighted averaging) ───────────────
    //
    // Build StressElement for each tet (borrows connectivity slice) and call
    // recover_nodal_stress_p1. The nodal_stress Vec is stored on CantileverFeaSolve
    // and fed stride-9 row-major to resample_nodal_to_grid by the trampoline.
    let se_refs: Vec<StressElement<'_>> = tet_connectivity
        .iter()
        .zip(stress_elements.iter())
        .map(|(conn, sigma)| {
            let phys4: [[f64; 3]; 4] = [
                coords[conn[0]],
                coords[conn[1]],
                coords[conn[2]],
                coords[conn[3]],
            ];
            StressElement {
                connectivity: conn.as_slice(),
                stress: *sigma,
                volume: tet_volume_p1(&phys4),
            }
        })
        .collect();

    let nodal_stress = recover_nodal_stress_p1(n_nodes, &se_refs);

    // `converged` and `iterations` were hoisted before the cancel short-circuit
    // above (task 4366); no re-declaration needed here.
    let fea = CantileverFeaSolve {
        u: cg_result.into_shared_u(),
        coords,
        tip_nodes,
        max_von_mises,
        converged,
        iterations,
        tet_connectivity,
        nodal_stress,
        nx,
        ny,
        nz,
    };
    (fea, fresh_warm)
}

/// Compute the full 3×3 Cauchy stress tensor for a P1 tet with a given
/// 6×6 global D matrix (anisotropic / orthotropic material path).
///
/// Mirrors `element_von_mises_anisotropic` — same Jacobian, J⁻ᵀ, B-matrix,
/// and D_global·ε_voigt multiply — but returns the symmetric 3×3 tensor
/// instead of the scalar vM.
///
/// # Voigt convention
///
/// Identical to `element_stress_p1` (result.rs):
///   σ_voigt = [σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]
///
/// Tensor layout:
///   m[0] = [σxx, σxy, σxz]
///   m[1] = [σxy, σyy, σyz]
///   m[2] = [σxz, σyz, σzz]
///
/// Added by task 4084/α: used by solve_cantilever_fea (anisotropic branch)
/// and by the step-3 vM-consistency test.
fn element_stress_anisotropic(
    phys_nodes: &[[f64; 3]; 4],
    d_global: &[[f64; 6]; 6],
    u_e: &[f64; 12],
) -> [[f64; 3]; 3] {
    // Jacobian (same as element_stress_p1 / element_von_mises_anisotropic).
    let grads_ref: [[f64; 3]; 4] = [
        [-1.0, -1.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

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

    const MIN_JACOBIAN_DET: f64 = 1.0e-30;
    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "element_stress_anisotropic: degenerate tet |det J| = {:.3e} \
         (must be > {:.3e} and finite — see PRD task #21 for the future diagnostic path)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );

    let j_inv_t = [
        [
            (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1]) / det,
            (j_mat[1][2] * j_mat[2][0] - j_mat[1][0] * j_mat[2][2]) / det,
            (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]) / det,
        ],
        [
            (j_mat[0][2] * j_mat[2][1] - j_mat[0][1] * j_mat[2][2]) / det,
            (j_mat[0][0] * j_mat[2][2] - j_mat[0][2] * j_mat[2][0]) / det,
            (j_mat[0][1] * j_mat[2][0] - j_mat[0][0] * j_mat[2][1]) / det,
        ],
        [
            (j_mat[0][1] * j_mat[1][2] - j_mat[0][2] * j_mat[1][1]) / det,
            (j_mat[0][2] * j_mat[1][0] - j_mat[0][0] * j_mat[1][2]) / det,
            (j_mat[0][0] * j_mat[1][1] - j_mat[0][1] * j_mat[1][0]) / det,
        ],
    ];

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

    // Unpack to symmetric 3×3 tensor (same layout as element_stress_p1):
    //   σ_voigt = [σxx, σyy, σzz, σxy, σyz, σxz]
    [
        [sigma_voigt[0], sigma_voigt[3], sigma_voigt[5]],
        [sigma_voigt[3], sigma_voigt[1], sigma_voigt[4]],
        [sigma_voigt[5], sigma_voigt[4], sigma_voigt[2]],
    ]
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
            let e1 = scalar_si_field(data, "e1");
            let e2 = scalar_si_field(data, "e2");
            let e3 = scalar_si_field(data, "e3");
            let g12 = scalar_si_field(data, "g12");
            let g13 = scalar_si_field(data, "g13");
            let g23 = scalar_si_field(data, "g23");
            let nu12 = real_field(data, "nu12");
            let nu13 = real_field(data, "nu13");
            let nu23 = real_field(data, "nu23");
            let law = OrthotropicMaterial {
                e1,
                e2,
                e3,
                g12,
                g13,
                g23,
                nu12,
                nu13,
                nu23,
            };
            let aniso = AnisotropicMaterial::from_law(&law, IDENTITY);
            MaterialModel::Anisotropic(aniso)
        }
        "TransverseIsotropicMaterial" => {
            let e_in_plane = scalar_si_field(data, "e_in_plane");
            let e_axial = scalar_si_field(data, "e_axial");
            let nu_in_plane = real_field(data, "nu_in_plane");
            let nu_axial = real_field(data, "nu_axial");
            let g_axial = scalar_si_field(data, "g_axial");
            let law = TransverseIsotropicMaterial {
                e_in_plane,
                e_axial,
                nu_in_plane,
                nu_axial,
                g_axial,
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
    match data.fields.get(key) {
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
    match data.fields.get(key) {
        Some(Value::Real(r)) => *r,
        other => panic!(
            "solve_elastic_static_trampoline: expected field {:?} to be \
             Value::Real, got: {:?}",
            key, other
        ),
    }
}

/// Extract `IsotropicElastic` from a `Value::StructureInstance` carrying
/// `youngs_modulus: Scalar<Pressure>` and `poisson_ratio: Real`.
fn extract_material(val: &Value) -> IsotropicElastic {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => panic!(
            "solve_elastic_static_trampoline: expected material to be \
             Value::StructureInstance, got: {:?}",
            other
        ),
    };
    let youngs_modulus = match data.fields.get("youngs_modulus") {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "solve_elastic_static_trampoline: expected youngs_modulus to be \
             Value::Scalar, got: {:?}",
            other
        ),
    };
    let poisson_ratio = match data.fields.get("poisson_ratio") {
        Some(Value::Real(r)) => *r,
        other => panic!(
            "solve_elastic_static_trampoline: expected poisson_ratio to be \
             Value::Real, got: {:?}",
            other
        ),
    };
    IsotropicElastic {
        youngs_modulus,
        poisson_ratio,
    }
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

/// Extract all load contributions from a `Value::List` of load `StructureInstance`s
/// in a **single pass**, returning `(tip_force, pressures)`.
///
/// - `tip_force` — per-axis `[f64; 3]` sum of `force * direction` over all
///   `PointLoad` items.  Direction magnitude is significant: supplying a
///   non-unit direction (e.g. `[0, 0, -2]`) silently scales the applied force
///   by `|direction|`; callers are expected to pass unit vectors.  A missing
///   or malformed `direction` defaults to `[0.0, 0.0, -1.0]` (-Z).
///   Passed as-is to `solve_cantilever_fea`.
/// - `pressures` — one `PressureSpec` per `PressureLoad` item; items of any
///   other type (e.g. `FixedSupport`) are silently skipped.
///
/// Panics with a descriptive message if `val` is not a `Value::List`.
/// A scene may mix `PointLoad` and `PressureLoad`; both accumulate into
/// the same force vector `f` via their respective kernel primitives.
fn extract_loads(val: &Value) -> ([f64; 3], Vec<PressureSpec>) {
    let items = match val {
        Value::List(v) => v,
        other => panic!(
            "solve_elastic_static_trampoline: expected Value::List for loads, got: {:?}",
            other
        ),
    };
    let mut tip_force_vec = [0.0f64; 3];
    let mut pressures = Vec::new();
    for item in items {
        if let Value::StructureInstance(data) = item {
            if data.type_name == "PointLoad" {
                if let Some(Value::Real(f)) = data.fields.get("force") {
                    let dir = match data.fields.get("direction") {
                        Some(Value::List(elems)) if elems.len() == 3 => {
                            let mut d = [0.0f64; 3];
                            for (i, e) in elems.iter().enumerate() {
                                // List<Real> elements materialize as either
                                // `Value::Real` (scene literals) or dimensionless
                                // `Value::Scalar` (structure-def default values),
                                // mirroring the `magnitude` parse below. Handle
                                // both so the default [0,0,-1] is honoured.
                                match e {
                                    Value::Real(v) => d[i] = *v,
                                    Value::Scalar { si_value, .. } => d[i] = *si_value,
                                    _ => {}
                                }
                            }
                            d
                        }
                        _ => [0.0, 0.0, -1.0],
                    };
                    for axis in 0..3 {
                        tip_force_vec[axis] += f * dir[axis];
                    }
                }
            } else if data.type_name == "PressureLoad" {
                let magnitude = match data.fields.get("magnitude") {
                    Some(Value::Real(m)) => *m,
                    Some(Value::Scalar { si_value, .. }) => *si_value,
                    _ => continue,
                };
                let face = match data.fields.get("face") {
                    Some(Value::String(s)) => s.clone(),
                    _ => continue,
                };
                let direction = match data.fields.get("direction") {
                    Some(Value::String(s)) => s.clone(),
                    _ => "normal".to_string(),
                };
                pressures.push(PressureSpec { magnitude, face, direction });
            }
        }
    }
    (tip_force_vec, pressures)
}

/// A single pressure load parsed from a `PressureLoad` StructureInstance.
///
/// Fields mirror `PressureLoad` in `solver_elastic.ri`:
/// - `magnitude` — surface pressure magnitude in Pa (SI)
/// - `face`      — face identifier: "x_min", "x_max", "y_min", "y_max", "z_min", "z_max"
/// - `direction` — "normal" (only supported value in v0.4)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PressureSpec {
    pub(crate) magnitude: f64,
    pub(crate) face: String,
    pub(crate) direction: String,
}

/// Extract all `PressureLoad` StructureInstances from a `Value::List`.
///
/// Items that are not `PressureLoad` (e.g. `PointLoad`, `FixedSupport`) are
/// silently skipped — the caller is responsible for handling them separately.
///
/// Returns an empty Vec if the list contains no `PressureLoad` items.
///
/// Production code uses the combined [`extract_loads`] (single pass over both
/// `PointLoad` and `PressureLoad`).  This function is kept for targeted unit
/// testing of the pressure-extraction path in isolation.
#[cfg(test)]
pub(crate) fn extract_pressure_loads(val: &Value) -> Vec<PressureSpec> {
    let items = match val {
        Value::List(v) => v,
        _ => return vec![],
    };
    let mut result = Vec::new();
    for item in items.iter() {
        if let Value::StructureInstance(data) = item
            && data.type_name == "PressureLoad"
        {
            let magnitude = match data.fields.get("magnitude") {
                Some(Value::Real(m)) => *m,
                Some(Value::Scalar { si_value, .. }) => *si_value,
                _ => continue,
            };
            let face = match data.fields.get("face") {
                Some(Value::String(s)) => s.clone(),
                _ => continue,
            };
            let direction = match data.fields.get("direction") {
                Some(Value::String(s)) => s.clone(),
                _ => "normal".to_string(),
            };
            result.push(PressureSpec { magnitude, face, direction });
        }
    }
    result
}

/// Map a face-name string to `(axis, at_max, extent, inward_normal)`.
///
/// Coordinates are assumed to run `[0, length] × [0, width] × [0, height]`.
/// The inward normal is the negative of the outward surface normal (pressure
/// pushes inward).  Supported names mirror the `modal_ops.rs` x_min/x_max/…
/// convention (eps=1e-9 plane predicate).  Any unrecognized or empty face name
/// returns `None` (silent no-op in v1 — validation is owned by task 4092).
fn box_face_plane(
    face: &str,
    length: f64,
    width: f64,
    height: f64,
) -> Option<(usize, bool, f64, [f64; 3])> {
    // (axis, at_max, extent, inward_normal)
    match face {
        "x_min" => Some((0, false, 0.0, [1.0, 0.0, 0.0])),
        "x_max" => Some((0, true, length, [-1.0, 0.0, 0.0])),
        "y_min" => Some((1, false, 0.0, [0.0, 1.0, 0.0])),
        "y_max" => Some((1, true, width, [0.0, -1.0, 0.0])),
        "z_min" => Some((2, false, 0.0, [0.0, 0.0, 1.0])),
        "z_max" => Some((2, true, height, [0.0, 0.0, -1.0])),
        _ => None,
    }
}

/// Collect boundary face triangles from a tetrahedral mesh on a given plane.
///
/// The plane is characterized by (`axis`, `at_max`, `extent`, `eps`):
/// - `axis`   — 0 = x, 1 = y, 2 = z
/// - `at_max` — `true`: the plane is at `extent` (upper); `false`: at 0 (lower).
/// - `extent` — physical coordinate of the plane.
/// - `eps`    — point-on-plane tolerance (1e-9 recommended).
///
/// For each tet's 4 triangular faces, a face is included if all 3 of its nodes
/// satisfy the plane predicate (`coord[axis] >= extent - eps` for at_max,
/// `coord[axis] <= eps` for lower).  A boundary face belongs to exactly one tet
/// so there is no double-counting.
fn collect_box_face_triangles(
    coords: &[[f64; 3]],
    tets: &[[usize; 4]],
    axis: usize,
    at_max: bool,
    extent: f64,
    eps: f64,
) -> Vec<[usize; 3]> {
    // Four triangular faces of a tet [a, b, c, d]:
    const FACE_IDX: [[usize; 3]; 4] = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];

    // NOTE: `eps` is an **absolute** tolerance. For the current fixtures
    // (SI-metre beams, node spacing >> 1e-9 m) this is safe. If sub-millimetre
    // or sub-micron FEA geometries are ever supported, consider scaling eps
    // relative to the relevant extent (e.g. `eps = 1e-9 * extent.max(1.0)`)
    // so that the predicate remains meaningful at very small scales.
    let on_plane = |node: usize| -> bool {
        let coord = coords[node][axis];
        if at_max { coord >= extent - eps } else { coord <= eps }
    };

    let mut result = Vec::new();
    for tet in tets {
        for fi in &FACE_IDX {
            let n0 = tet[fi[0]];
            let n1 = tet[fi[1]];
            let n2 = tet[fi[2]];
            if on_plane(n0) && on_plane(n1) && on_plane(n2) {
                result.push([n0, n1, n2]);
            }
        }
    }
    result
}

/// Apply face-pressure tractions from `pressures` into the global force vector `f`.
///
/// For each `PressureSpec`:
/// 1. Resolve the face name via `box_face_plane` (unrecognized face → skip).
/// 2. Collect boundary triangles on that plane via `collect_box_face_triangles`.
/// 3. For each triangle, call `apply_traction_load(f, FaceOrder::P1Tri, …)`.
///
/// The traction vector is `magnitude · inward_normal`.  Only `"normal"` direction
/// is supported in v1; other direction strings are treated as `"normal"`.
/// Accumulates additively into `f` — composable with the existing tip point loads.
///
/// **Performance note:** the full tet mesh is scanned once per `PressureSpec`
/// (O(|pressures| × n_tets)).  For the current fixtures (≤ 2 specs, small meshes)
/// this is negligible.  If scenes with many distinct pressure faces become common,
/// a future optimisation could collect boundary triangles per (axis, at_max) key
/// in a single O(n_tets) pass and then index specs by their resolved face.
fn assemble_box_face_pressures(
    f: &mut [f64],
    coords: &[[f64; 3]],
    tets: &[[usize; 4]],
    pressures: &[PressureSpec],
    length: f64,
    width: f64,
    height: f64,
) {
    for spec in pressures {
        let Some((axis, at_max, extent, inward_normal)) =
            box_face_plane(&spec.face, length, width, height)
        else {
            continue; // unrecognized/empty face → silent no-op
        };
        let traction = [
            spec.magnitude * inward_normal[0],
            spec.magnitude * inward_normal[1],
            spec.magnitude * inward_normal[2],
        ];
        let tris = collect_box_face_triangles(coords, tets, axis, at_max, extent, 1e-9);
        for tri in &tris {
            let tri_phys = [coords[tri[0]], coords[tri[1]], coords[tri[2]]];
            apply_traction_load(f, FaceOrder::P1Tri, tri, &tri_phys, traction);
        }
    }
}

/// Extract `(ShellForce, shell_threshold)` from the `ElasticOptions`
/// `Value::StructureInstance` at `value_inputs[6]` for shell-route classification
/// (task 3594/δ).
///
/// - `shell_force` is a `Value::Enum { type_name: "ShellForce", variant }`
///   (`Off` / `Auto` / `On`); any unknown variant is treated as `Auto`.
/// - `shell_threshold` is a dimensionless `Value::Real` (a `Value::Scalar` is
///   also accepted, but ONLY when it is `DIMENSIONLESS`; a scalar carrying a
///   real dimension — e.g. PRESSURE — is treated as an upstream type error and
///   ignored, falling back to the `0.2` default, per esc-3594 suggestion 2).
///
/// A missing options instance or missing/garbled fields fall back to the stdlib
/// defaults (`ShellForce::Auto`, `0.2`), so a bare `ElasticOptions()` classifies
/// exactly as `solver_elastic.ri` declares.
///
/// `pub(crate)` so the `@optimized`→ComputeNode lowering in `engine_eval.rs`
/// (task 3594/δ step-12) reuses the *exact* same options-parse + classification
/// helpers this trampoline uses — the graph wiring (upstream shell-extract node)
/// and the trampoline's own Shell/Tet routing must always agree.
pub(crate) fn extract_shell_route_params(options: &Value) -> (ShellForce, f64) {
    // stdlib defaults (solver_elastic.ri:173,176).
    let mut shell_force = ShellForce::Auto;
    let mut shell_threshold = 0.2_f64;
    if let Value::StructureInstance(data) = options {
        if let Some(Value::Enum { variant, .. }) = data.fields.get("shell_force") {
            shell_force = match variant.as_str() {
                "Off" => ShellForce::Off,
                "On" => ShellForce::On,
                _ => ShellForce::Auto, // "Auto" or any unknown variant
            };
        }
        match data.fields.get("shell_threshold") {
            Some(Value::Real(r)) => shell_threshold = *r,
            // `shell_threshold` is a dimensionless ratio: only accept a
            // `Value::Scalar` that is actually DIMENSIONLESS. A scalar carrying a
            // real dimension (e.g. PRESSURE) is an upstream type error — ignore
            // it and keep the default rather than silently consuming a
            // mis-dimensioned magnitude as the ratio (esc-3594 suggestion 2).
            Some(Value::Scalar {
                si_value,
                dimension,
            }) if dimension.is_dimensionless() => shell_threshold = *si_value,
            _ => {}
        }
    }
    (shell_force, shell_threshold)
}

/// Read the execution-mode knobs from an `ElasticOptions`-shaped `Value`
/// (`value_inputs[6]`), mirroring [`extract_shell_route_params`]' missing-field
/// fallback discipline (task 2926).
///
/// Returns `(deterministic, threads)`:
/// - `deterministic` — `ElasticOptions.deterministic` (`Value::Bool`). When
///   `true`, the FEA assembly + CG solve are forced single-threaded with
///   fixed-order pairwise-tree reductions for bit-stable, cross-machine results
///   (PRD task #18). Defaults to `false` (stdlib `solver_elastic.ri:193`).
/// - `threads` — `ElasticOptions.threads : Option<Int>`. `none` (the default)
///   materialises at runtime as `Value::Option(None)` → `None`, and the caller
///   then resolves it to the host CPU count; an explicit value arrives as
///   `Value::Option(Some(Value::Int))`, while a bare `Value::Int` is also
///   accepted defensively. Only a positive count is honoured — `0`, negative,
///   or a mis-typed cell falls back to the `none`/auto default.
///
/// A non-`StructureInstance` (or one missing both fields) yields the stdlib
/// defaults `(false, None)`.
pub(crate) fn extract_execution_params(options: &Value) -> (bool, Option<usize>) {
    let mut deterministic = false;
    let mut threads: Option<usize> = None;
    if let Value::StructureInstance(data) = options {
        if let Some(Value::Bool(b)) = data.fields.get("deterministic") {
            deterministic = *b;
        }
        // Unwrap the runtime `Option<Int>` representation, tolerating a bare Int.
        let threads_value = match data.fields.get("threads") {
            Some(Value::Option(inner)) => inner.as_deref(),
            other => other,
        };
        if let Some(Value::Int(n)) = threads_value {
            // `usize::try_from` rejects negatives; `filter` rejects 0 → auto.
            threads = usize::try_from(*n).ok().filter(|&n| n > 0);
        }
    }
    (deterministic, threads)
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reify_solver_elastic::{AnisotropicMaterial, OrthotropicMaterial};

    // ── task 4264: PressureLoad bridge ────────────────────────────────────────

    /// step-1 RED (task 4264): extract_pressure_loads reads PressureLoad items
    /// and ignores PointLoad items in the same list.
    ///
    /// Fixture: a Value::List containing one PressureLoad and one PointLoad.
    /// Expected: extract_pressure_loads returns exactly one PressureSpec whose
    /// fields match the PressureLoad input; the PointLoad is silently ignored.
    ///
    /// RED: PressureSpec and extract_pressure_loads don't exist yet.
    #[test]
    fn extract_pressure_loads_reads_pressure_and_ignores_point_load() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        // Build a PressureLoad StructureInstance
        let pressure_fields: PersistentMap<String, Value> = [
            ("magnitude".to_string(), Value::Real(1.0e6)),
            ("face".to_string(), Value::String("x_max".to_string())),
            ("direction".to_string(), Value::String("normal".to_string())),
        ]
        .into_iter()
        .collect();
        let pressure_load = Value::StructureInstance(Box::new(StructureInstanceData {
            type_name: "PressureLoad".to_string(),
            type_id: StructureTypeId(u32::MAX),
            version: 0,
            fields: pressure_fields,
        }));

        // Build a PointLoad StructureInstance (should be ignored)
        let point_fields: PersistentMap<String, Value> =
            [("force".to_string(), Value::Real(500.0))].into_iter().collect();
        let point_load = Value::StructureInstance(Box::new(StructureInstanceData {
            type_name: "PointLoad".to_string(),
            type_id: StructureTypeId(u32::MAX),
            version: 0,
            fields: point_fields,
        }));

        let loads = Value::List(vec![pressure_load, point_load]);

        let specs = extract_pressure_loads(&loads);

        assert_eq!(specs.len(), 1, "expected exactly 1 PressureSpec, got {}", specs.len());
        assert_eq!(specs[0].magnitude, 1.0e6);
        assert_eq!(specs[0].face, "x_max");
        assert_eq!(specs[0].direction, "normal");

        // Also assert that a bare empty list → empty Vec.
        let empty_specs = extract_pressure_loads(&Value::List(vec![]));
        assert!(empty_specs.is_empty(), "empty list should return empty Vec");
    }

    /// step-9 RED (task 2926): extract_execution_params reads `deterministic`
    /// (Value::Bool, default false) and `threads` (Option<Int>, default None)
    /// from an ElasticOptions-shaped StructureInstance, mirroring
    /// `extract_shell_route_params`' missing-field fallback discipline.
    ///
    /// `threads : Option<Int>` materialises at runtime as a `Value::Option`, so
    /// the helper must unwrap `Value::Option(Some(Value::Int))` in addition to
    /// accepting a bare `Value::Int`. Missing fields (or a non-StructureInstance)
    /// fall back to the stdlib defaults `(deterministic = false, threads = none)`.
    ///
    /// RED: `extract_execution_params` does not exist yet → compile-fail.
    #[test]
    fn extract_execution_params_reads_deterministic_and_threads() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        // Wrap a field map as an ElasticOptions-shaped StructureInstance.
        let make_options = |fields: PersistentMap<String, Value>| {
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_name: "ElasticOptions".to_string(),
                type_id: StructureTypeId(u32::MAX),
                version: 0,
                fields,
            }))
        };

        // (a) deterministic = true, threads = 4 (bare Int) → (true, Some(4)).
        let opts_a = make_options(
            [
                ("deterministic".to_string(), Value::Bool(true)),
                ("threads".to_string(), Value::Int(4)),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(extract_execution_params(&opts_a), (true, Some(4)));

        // (b) deterministic = false, threads stored in the real runtime
        //     Option<Int> representation `Value::Option(Some(Int(8)))` → (false, Some(8)).
        let opts_b = make_options(
            [
                ("deterministic".to_string(), Value::Bool(false)),
                ("threads".to_string(), Value::Option(Some(Box::new(Value::Int(8))))),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(extract_execution_params(&opts_b), (false, Some(8)));

        // (c) threads = none (`Value::Option(None)`), `deterministic` field absent
        //     → both fall back to defaults (false, None).
        let opts_c =
            make_options([("threads".to_string(), Value::Option(None))].into_iter().collect());
        assert_eq!(extract_execution_params(&opts_c), (false, None));

        // (d) instance carrying only an unrelated field (neither `deterministic`
        //     nor `threads` present) → defaults (false, None).
        let opts_d = make_options(
            [("require_hex_wedge".to_string(), Value::Bool(false))].into_iter().collect(),
        );
        assert_eq!(extract_execution_params(&opts_d), (false, None));

        // (e) non-StructureInstance → defaults (false, None).
        assert_eq!(extract_execution_params(&Value::Real(1.0)), (false, None));
    }

    /// step-3 RED (task 4264): box_face_pressure_conserves_resultant.
    ///
    /// Build a unit-cube [0,1]^3 mesh with 8 corner nodes and the standard
    /// 6-tet Freudenthal connectivity (same hex split as solve_cantilever_fea).
    /// Verify:
    ///   (a) collect_box_face_triangles for x_max returns triangles whose
    ///       total area equals 1.0 (±1e-9).
    ///   (b) assemble_box_face_pressures with magnitude=1.0 on x_max yields
    ///       global resultant Σf = (-1, 0, 0) within abs 1e-9 (pressure is
    ///       inward on x_max → -x; Σf = -p·A·x̂).
    ///
    /// Achievability basis: kernel apply_traction_load already proves
    /// Σf = face_area·traction (neumann.rs conservation tests + Lamé test 4113);
    /// a tiling of the face sums to total area exactly.
    ///
    /// RED: collect_box_face_triangles / assemble_box_face_pressures / box_face_plane
    ///      do not exist yet.
    #[test]
    fn box_face_pressure_conserves_resultant() {
        // Unit-cube mesh: 8 corner nodes of [0,1]^3.
        let coords: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];
        // Freudenthal 6-tet split — identical to the single-hex case in
        // solve_cantilever_fea (c[0..7] = nodes 0..7 for hx=hy=hz=0).
        let tets: Vec<[usize; 4]> = vec![
            [0, 1, 2, 6], // T0
            [0, 2, 3, 6], // T1
            [0, 5, 1, 6], // T2
            [0, 3, 7, 6], // T3
            [0, 4, 5, 6], // T4
            [0, 7, 4, 6], // T5
        ];

        // (a) Collect triangles on the x_max face (axis=0, at_max=true, extent=1.0).
        let tris = collect_box_face_triangles(&coords, &tets, 0, true, 1.0, 1e-9);
        assert!(!tris.is_empty(), "x_max should have boundary triangles");

        // Sum triangle areas; each triangle area = ½|cross(ab, ac)|.
        let total_area: f64 = tris
            .iter()
            .map(|tri| {
                let a = coords[tri[0]];
                let b = coords[tri[1]];
                let c = coords[tri[2]];
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let cross = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                0.5 * (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt()
            })
            .sum();
        assert!(
            (total_area - 1.0).abs() < 1e-9,
            "x_max face area should be 1.0 for unit cube, got {total_area}"
        );

        // (b) Assemble pressure loads; check global resultant Σf = (-1, 0, 0).
        let mut f = vec![0.0_f64; 3 * coords.len()];
        let specs = [PressureSpec {
            magnitude: 1.0,
            face: "x_max".to_string(),
            direction: "normal".to_string(),
        }];
        assemble_box_face_pressures(&mut f, &coords, &tets, &specs, 1.0, 1.0, 1.0);

        let (sum_x, sum_y, sum_z) =
            f.chunks_exact(3).fold((0.0_f64, 0.0_f64, 0.0_f64), |(sx, sy, sz), dof| {
                (sx + dof[0], sy + dof[1], sz + dof[2])
            });
        assert!(
            (sum_x - (-1.0)).abs() < 1e-9,
            "Σfx should be -1.0 (inward on x_max), got {sum_x}"
        );
        assert!(sum_y.abs() < 1e-9, "Σfy should be 0.0, got {sum_y}");
        assert!(sum_z.abs() < 1e-9, "Σfz should be 0.0, got {sum_z}");
    }

    /// step-5 RED (task 4264): solve_cantilever_fea_with_x_max_pressure_compresses_inward.
    ///
    /// Geometry: length=1.0 m, width=0.1 m, height=0.1 m, isotropic steel
    /// (E=200 GPa, ν=0.3). tip_force=0.0, pressures=[PressureSpec{magnitude:1e6,
    /// face:"x_max", direction:"normal"}] — root face x=0 is auto-clamped.
    ///
    /// Expected (sign-only, no tight magnitude band):
    /// - result.converged == true
    /// - mean of result.u[3*n+0] over tip_nodes < 0  (inward -x displacement)
    /// - result.max_von_mises is finite and > 0
    ///
    /// RED: solve_cantilever_fea does not yet accept a `pressures` parameter.
    #[test]
    fn solve_cantilever_fea_with_x_max_pressure_compresses_inward() {
        let iso = IsotropicElastic { youngs_modulus: 200e9_f64, poisson_ratio: 0.3_f64 };
        let model = MaterialModel::Isotropic(iso);
        let length = 1.0_f64;
        let width = 0.1_f64;
        let height = 0.1_f64;
        let pressures = [PressureSpec {
            magnitude: 1.0e6,
            face: "x_max".to_string(),
            direction: "normal".to_string(),
        }];

        let (result, _warm) =
            solve_cantilever_fea(
                &model, length, width, height, [0.0, 0.0, 0.0], None, &pressures, true, None, None,
            );

        assert!(result.converged, "FEA must converge under x_max pressure");

        // Mean x-displacement over tip nodes must be negative (inward on -x).
        let tip_ux: f64 = result
            .tip_nodes
            .iter()
            .map(|&n| result.u[3 * n])
            .sum::<f64>()
            / result.tip_nodes.len().max(1) as f64;
        assert!(
            tip_ux < 0.0,
            "mean tip u_x should be < 0 (inward) under x_max pressure, got {tip_ux}"
        );

        assert!(
            result.max_von_mises.is_finite() && result.max_von_mises > 0.0,
            "max_von_mises must be finite > 0, got {}",
            result.max_von_mises
        );
    }

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
            e1: 200e9_f64, // 200 GPa — beam-axis Young's modulus (governs bending)
            e2: 10e9_f64,  // 10 GPa  — transverse
            e3: 10e9_f64,  // 10 GPa  — transverse
            g12: 4e9_f64,  // 4 GPa
            g13: 4e9_f64,  // 4 GPa
            g23: 4e9_f64,  // 4 GPa
            nu12: 0.3_f64,
            nu13: 0.3_f64,
            nu23: 0.3_f64,
        };
        // Identity material frame: beam axis = material 1-axis → E1 governs bending.
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);

        // Cantilever geometry at L/h = 8 (keeps fixture off slender bending-lock wall).
        let length = 0.8_f64; // m — beam length (x-axis)
        let width = 0.1_f64; // m — cross-section width (y-axis)
        let height = 0.1_f64; // m — cross-section height (z-axis, bending direction)
        let tip_force = 1000.0_f64; // N — scalar kept for Euler–Bernoulli reference below

        // Call the new pub(crate) helper (doesn't exist yet → compile-fail RED).
        let (result, _fresh_warm) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso_mat),
            length,
            width,
            height,
            [0.0, 0.0, -tip_force],
            None,
            &[],
            true,
            None,
            None,
        );

        // Tip deflection = max |u_z| over tip-face nodes.
        let tip_deflection = result
            .tip_nodes
            .iter()
            .map(|&n| result.u[3 * n + 2].abs()) // z-component
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

    /// Row 3 (ε/3781 step-3): constant-field lift of an IsotropicElastic must
    /// produce an ElasticResult identical to the native isotropic path.
    ///
    /// # Rationale
    ///
    /// β/3778 C4 guarantees that `element_stiffness_p1_with_field(&ConstantField{..})`
    /// for an identity-frame isotropic lift is bitwise identical to the legacy
    /// `element_stiffness(P1, ..)`. Since the same mesh, same f, and same
    /// deterministic CG are used, the displacement vectors u must also be
    /// bitwise identical.
    ///
    /// # Thresholds
    ///
    /// - **iterations**: exact equality (`assert_eq!`). Contingent on β/3778 C4
    ///   bitwise-identity: identical K + deterministic preconditioned CG ⇒
    ///   identical convergence path. If C4 is ever softened to a numerical
    ///   tolerance, this assertion must be relaxed accordingly.
    /// - **displacement u**: 1e-12 relative tolerance. The expected diff is 0.0
    ///   ULPs (bit-identity propagates through CG), but a tolerance-based guard
    ///   is used over `assert_eq!` as defensive style.
    /// - **max_von_mises**: 1e-9 relative tolerance, reflecting the fact that
    ///   the two stress-recovery code paths (`element_stress_p1` vs
    ///   `element_von_mises_anisotropic`) share the same u but compute stress
    ///   via different numerical sequences; 1e-9 is the expected agreement band
    ///   for an identity-frame isotropic lift.
    ///
    /// This proves the C4 isotropic-lift equivalence flows end-to-end through
    /// `solve_cantilever_fea` to the consumer `CantileverFeaSolve`.
    #[test]
    fn constant_field_lift_matches_isotropic_elastic_result() {
        // Same geometry/load as the sibling orthotropic tests.
        let length = 0.8_f64;
        let width = 0.1_f64;
        let height = 0.1_f64;
        let tip_force = 1000.0_f64;

        let iso = IsotropicElastic {
            youngs_modulus: 200e9,
            poisson_ratio: 0.3,
        };
        let identity: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso = AnisotropicMaterial::from_law(&iso, identity);

        // Solve with the native isotropic path.
        let (iso_result, _) = solve_cantilever_fea(
            &MaterialModel::Isotropic(iso),
            length,
            width,
            height,
            [0.0, 0.0, -tip_force],
            None,
            &[],
            true,
            None,
            None,
        );
        // Solve with the anisotropic identity-frame lift path.
        let (aniso_result, _) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso),
            length,
            width,
            height,
            [0.0, 0.0, -tip_force],
            None,
            &[],
            true,
            None,
            None,
        );

        // Both must converge.
        assert!(iso_result.converged, "isotropic solve must converge");
        assert!(aniso_result.converged, "anisotropic solve must converge");

        // CG iteration count must be identical: same K (bit-identical per β/3778
        // C4 guarantee), same f, same deterministic preconditioner ⇒ same
        // convergence path.
        //
        // NOTE: this exact-equality assertion is contingent on the β/3778 C4
        // bitwise-identity guarantee holding all the way through assembly.  If
        // that guarantee is ever softened to a numerical tolerance, the two
        // solves may converge in a different number of steps and this assertion
        // must be relaxed to a tolerance-based comparison.
        assert_eq!(
            iso_result.iterations, aniso_result.iterations,
            "iso and identity-frame aniso must require identical CG iterations \
             (same K + same f + deterministic CG ⇒ identical convergence path; \
             contingent on β/3778 C4 bitwise-identity guarantee)",
        );

        // Displacement vectors must agree component-wise.
        //
        // The underlying guarantee is bitwise identity (β/3778 C4 ⇒ identical K
        // + deterministic CG ⇒ bit-equal u), so the 1e-12 relative tolerance is
        // a defensive guard rather than a numerically tight bound — the actual
        // diff is expected to be 0.0 ULPs.  A tolerance-based assertion is used
        // here rather than component-wise assert_eq! because floating-point
        // equality assertions are considered fragile style even when the
        // theoretical guarantee is exact; the 1e-12 budget leaves no practical
        // room for divergence.
        assert_eq!(
            iso_result.u.len(),
            aniso_result.u.len(),
            "displacement vectors must have the same length",
        );
        for i in 0..iso_result.u.len() {
            let tol = 1e-12 * iso_result.u[i].abs().max(1.0);
            let diff = (aniso_result.u[i] - iso_result.u[i]).abs();
            assert!(
                diff < tol,
                "displacement at i={i}: |u_aniso−u_iso|={diff:.3e} ≥ tol={tol:.3e} \
                 (u_iso={:.3e}, u_aniso={:.3e})",
                iso_result.u[i],
                aniso_result.u[i],
            );
        }

        // max_von_mises must agree to 1e-9 relative: the two stress-recovery
        // code paths (element_stress_p1 vs element_von_mises_anisotropic)
        // compute the same physical quantity for an identity-frame isotropic lift.
        let vm_iso = iso_result.max_von_mises;
        let vm_aniso = aniso_result.max_von_mises;
        assert!(
            vm_iso > 0.0,
            "isotropic max_von_mises must be positive (got {vm_iso})",
        );
        let vm_tol = 1e-9 * vm_iso.abs().max(1.0);
        assert!(
            (vm_aniso - vm_iso).abs() < vm_tol,
            "max_von_mises: iso={vm_iso:.4e} Pa, aniso={vm_aniso:.4e} Pa, \
             |diff|={:.3e}, tol={vm_tol:.3e}",
            (vm_aniso - vm_iso).abs(),
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
            e1: 200e9_f64,
            e2: 10e9_f64,
            e3: 10e9_f64,
            g12: 4e9_f64,
            g13: 4e9_f64,
            g23: 4e9_f64,
            nu12: 0.3_f64,
            nu13: 0.3_f64,
            nu23: 0.3_f64,
        };
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);

        let length = 0.8_f64;
        let width = 0.1_f64;
        let height = 0.1_f64;
        let tip_force = 1000.0_f64; // scalar kept for analytic-sigma reference below

        let (result, _) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso_mat),
            length,
            width,
            height,
            [0.0, 0.0, -tip_force],
            None,
            &[],
            true,
            None,
            None,
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

    // ── step-3 RED (task α/4084): element_stress_anisotropic + extended CantileverFeaSolve ──

    /// element_stress_anisotropic vM must match element_von_mises_anisotropic
    /// (same D_global·ε computation, just different output shape).
    ///
    /// Uses a single-element unit tet with the orthotropic fixture; asserts
    /// that the vM derived from the 3×3 tensor agrees to ≤1e-9 rel with the
    /// scalar returned by element_von_mises_anisotropic.
    ///
    /// Compile-fails until step-4 adds element_stress_anisotropic.
    #[test]
    fn element_stress_anisotropic_vm_matches_anisotropic() {
        let law = OrthotropicMaterial {
            e1: 200e9_f64,
            e2: 10e9_f64,
            e3: 10e9_f64,
            g12: 4e9_f64,
            g13: 4e9_f64,
            g23: 4e9_f64,
            nu12: 0.3_f64,
            nu13: 0.3_f64,
            nu23: 0.3_f64,
        };
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);
        let d_global = aniso_mat.d_matrix_global();

        // Unit tet: nodes at (0,0,0),(1,0,0),(0,1,0),(0,0,1) — deterministic
        let phys: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];

        // Non-zero displacement vector (non-degenerate stress state)
        let u_e: [f64; 12] = [
            0.0, 0.0, 0.0, 1e-4, 0.0, 0.0, 0.0, 1e-4, 0.0, 0.0, 0.0, 1e-4,
        ];

        // ── Part A: non-degeneracy guard (orthotropic fixture) ───────────────────
        // Verifies that element_stress_anisotropic returns a non-zero, finite
        // tensor for a non-trivial D matrix and displacement field.
        // NOTE: the pre-α test compared this vM against element_von_mises_anisotropic,
        // but after the step-4 refactor that function is a thin wrapper that calls
        // element_stress_anisotropic internally — making the comparison tautological.
        // Only the finite/positive guard remains meaningful here.
        let sigma = element_stress_anisotropic(&phys, &d_global, &u_e);
        let (sxx, syy, szz) = (sigma[0][0], sigma[1][1], sigma[2][2]);
        let (sxy, syz, szx) = (sigma[0][1], sigma[1][2], sigma[0][2]);
        let vm_from_tensor = f64::sqrt(
            0.5 * ((sxx - syy).powi(2)
                + (syy - szz).powi(2)
                + (szz - sxx).powi(2)
                + 6.0 * (sxy * sxy + syz * syz + szx * szx)),
        );
        assert!(
            vm_from_tensor.is_finite() && vm_from_tensor > 0.0,
            "vM from tensor must be finite and positive (orthotropic fixture); got {vm_from_tensor}"
        );

        // ── Part B: isotropic oracle — element_stress_anisotropic vs element_stress_p1 ─
        // For an isotropic D matrix, element_stress_anisotropic (6×6 D·ε path) and
        // element_stress_p1 (independent implementation using Lamé parameters) must
        // produce bit-identical 3×3 tensors.  This is a genuine inter-implementation
        // consistency check — not a comparison of the same code path to itself.
        let e_iso = 200e9_f64;
        let nu_iso = 0.3_f64;
        let g_iso = e_iso / (2.0 * (1.0 + nu_iso));
        let iso_orth = OrthotropicMaterial {
            e1: e_iso,
            e2: e_iso,
            e3: e_iso,
            g12: g_iso,
            g13: g_iso,
            g23: g_iso,
            nu12: nu_iso,
            nu13: nu_iso,
            nu23: nu_iso,
        };
        let iso_aniso = AnisotropicMaterial::from_law(&iso_orth, identity);
        let d_iso = iso_aniso.d_matrix_global();
        let iso_mat = IsotropicElastic {
            youngs_modulus: e_iso,
            poisson_ratio: nu_iso,
        };

        let sigma_aniso = element_stress_anisotropic(&phys, &d_iso, &u_e);
        let sigma_p1 = element_stress_p1(&phys, &iso_mat, &u_e);

        for r in 0..3 {
            for c in 0..3 {
                let a = sigma_aniso[r][c];
                let b = sigma_p1[r][c];
                let tol = 1e-9 * b.abs().max(1.0);
                assert!(
                    (a - b).abs() <= tol,
                    "isotropic oracle mismatch σ[{r}][{c}]: \
                     element_stress_anisotropic={a:.6e}, element_stress_p1={b:.6e}, \
                     diff={:.3e} > tol={tol:.3e}",
                    (a - b).abs(),
                );
            }
        }
    }

    /// Extended CantileverFeaSolve exposes tet_connectivity, nodal_stress, nx/ny/nz.
    ///
    /// Uses the orthotropic fixture (length=0.8, height=0.1 → nz=6, nx=48, ny=1).
    /// Compile-fails until step-4 adds these fields to CantileverFeaSolve.
    #[test]
    fn cantilever_fea_solve_extended_fields() {
        let law = OrthotropicMaterial {
            e1: 200e9_f64,
            e2: 10e9_f64,
            e3: 10e9_f64,
            g12: 4e9_f64,
            g13: 4e9_f64,
            g23: 4e9_f64,
            nu12: 0.3_f64,
            nu13: 0.3_f64,
            nu23: 0.3_f64,
        };
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let aniso_mat = AnisotropicMaterial::from_law(&law, identity);

        let length = 0.8_f64;
        let width = 0.1_f64;
        let height = 0.1_f64;

        let (fea, _) = solve_cantilever_fea(
            &MaterialModel::Anisotropic(aniso_mat),
            length,
            width,
            height,
            [0.0, 0.0, -1000.0],
            None,
            &[],
            true,
            None,
            None,
        );

        // Expected mesh counts: nz=6, nx=round(0.8/0.1*6)=48, ny=1
        let nz_exp = 6usize;
        let nx_exp = ((length / height * nz_exp as f64).round() as usize).max(1);
        let ny_exp = 1usize;

        assert_eq!(fea.nz, nz_exp, "nz");
        assert_eq!(fea.ny, ny_exp, "ny");
        assert_eq!(fea.nx, nx_exp, "nx");

        let expected_n_tets = nx_exp * ny_exp * nz_exp * 6;
        assert_eq!(
            fea.tet_connectivity.len(),
            expected_n_tets,
            "tet_connectivity.len() should be n_tets={expected_n_tets}"
        );

        let expected_n_nodes = (nx_exp + 1) * (ny_exp + 1) * (nz_exp + 1);
        assert_eq!(
            fea.nodal_stress.len(),
            expected_n_nodes,
            "nodal_stress.len() should be n_nodes={expected_n_nodes}"
        );
    }

    // ── step-9 RED (task δ/3594): shell-route trampoline contract ─────────────
    //
    // Drive `solve_elastic_static_trampoline` directly with hand-built
    // value_inputs and pin BOTH routing branches:
    //
    //   (1) shell case (shell_force=On): the ElasticResult must carry a real
    //       `ShellStress` `shell_channels` (NOT Undef) with finite top/mid/bottom,
    //       a populated `stress` field whose SampledField data bit-equals
    //       `shell_channels.mid` (the I-2 alias — compared as extracted data Vecs,
    //       NOT whole-Value PartialEq, mirroring solve_elastic_static_e2e.rs), and
    //       a max-over-elements top-channel von Mises within the one-OOM band
    //       [3e7, 3e9] Pa around σ=6PL/(bh²)=3e8.
    //
    //   (2) tet no-regression case (shell_force=Off): the task 4084/α baseline is
    //       preserved — `shell_channels` and `frame` stay Undef, but `stress` is a
    //       POPULATED Regular3D Sampled Field (4084/α populates displacement+stress
    //       for tets; this is NOT Undef).
    //
    // RED: the current trampoline ignores shell_force and always runs the tet
    // path, so it always emits shell_channels=Undef — branch (1) fails until
    // step-10 adds the shell route. Branch (2) already holds today.
    //
    // (All of Value / FieldSourceKind / PersistentMap / SampledGridKind /
    // StructureInstanceData / StructureTypeId / DimensionVector are already in
    // scope via the `use super::*` at the top of this test module, so no extra
    // `use` here.)

    /// Steel-like isotropic material StructureInstance. `classify_material` falls
    /// through to `MaterialModel::Isotropic` for any non-Orthotropic /
    /// non-TransverseIsotropic `type_name` (reads youngs_modulus + poisson_ratio).
    fn shell9_make_isotropic_material(youngs: f64, poisson: f64) -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "youngs_modulus".to_string(),
                Value::Scalar {
                    si_value: youngs,
                    dimension: DimensionVector::PRESSURE,
                },
            ),
            ("poisson_ratio".to_string(), Value::Real(poisson)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "IsotropicElastic".to_string(),
            version: 1,
            fields,
        }))
    }

    /// `Value::Scalar` geometry length (SI metres).
    fn shell9_make_len(m: f64) -> Value {
        Value::Scalar {
            si_value: m,
            dimension: DimensionVector::LENGTH,
        }
    }

    /// `Value::List` with one `PointLoad { force: Real }` (trampoline sums force).
    fn shell9_make_point_loads(force_n: f64) -> Value {
        let fields: PersistentMap<String, Value> = [("force".to_string(), Value::Real(force_n))]
            .into_iter()
            .collect();
        Value::List(vec![Value::StructureInstance(Box::new(
            StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "PointLoad".to_string(),
                version: 1,
                fields,
            },
        ))])
    }

    /// `Value::List` with one `FixedSupport` (fields not inspected; presence clamps).
    fn shell9_make_supports() -> Value {
        Value::List(vec![Value::StructureInstance(Box::new(
            StructureInstanceData {
                type_id: StructureTypeId(u32::MAX),
                type_name: "FixedSupport".to_string(),
                version: 1,
                fields: [].into_iter().collect(),
            },
        ))])
    }

    /// `ElasticOptions` with the given `ShellForce` variant + default
    /// `shell_threshold = 0.2`. `shell_force` is a `Value::Enum`.
    fn shell9_make_options(shell_force_variant: &str) -> Value {
        let fields: PersistentMap<String, Value> = [
            (
                "shell_force".to_string(),
                Value::Enum {
                    type_name: "ShellForce".to_string(),
                    variant: shell_force_variant.to_string(),
                },
            ),
            ("shell_threshold".to_string(), Value::Real(0.2)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticOptions".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Extract the `SampledField.data` vec from a `Value::Field { Sampled }`.
    fn shell9_field_data(v: &Value) -> Vec<f64> {
        match v {
            Value::Field { lambda, .. } => match lambda.as_ref() {
                Value::SampledField(sf) => sf.data.clone(),
                other => panic!("field lambda must be Value::SampledField, got {other:?}"),
            },
            other => panic!("expected Value::Field, got {other:?}"),
        }
    }

    /// von Mises of a row-major 3×3 stress window
    /// (`[σxx,σxy,σxz, σyx,σyy,σyz, σzx,σzy,σzz]`).
    fn shell9_vm9(w: &[f64]) -> f64 {
        let (sxx, syy, szz) = (w[0], w[4], w[8]);
        let (sxy, syz, szx) = (w[1], w[5], w[6]);
        (0.5 * ((sxx - syy).powi(2)
            + (syy - szz).powi(2)
            + (szz - sxx).powi(2)
            + 6.0 * (sxy * sxy + syz * syz + szx * szx)))
            .sqrt()
    }

    /// Unwrap a `ComputeOutcome::Completed` into the ElasticResult field map.
    fn shell9_result_fields(outcome: ComputeOutcome) -> PersistentMap<String, Value> {
        match outcome {
            ComputeOutcome::Completed { result, .. } => match result {
                Value::StructureInstance(d) => {
                    assert_eq!(
                        d.type_name.as_str(),
                        "ElasticResult",
                        "trampoline must return an ElasticResult StructureInstance"
                    );
                    d.fields
                }
                other => panic!("expected ElasticResult StructureInstance, got {other:?}"),
            },
            other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
        }
    }

    /// (1) shell route: shell_force=On + thin steel flexure → real ShellStress
    /// shell_channels, I-2 stress alias, in-band top von Mises.
    ///
    /// RED today: the trampoline always emits shell_channels=Undef.
    #[test]
    fn shell_route_trampoline_populates_shell_channels() {
        // Fixture: 50mm × 10mm × 1mm steel flexure, 10 N tip load.
        let value_inputs = [
            shell9_make_isotropic_material(205e9, 0.29),
            shell9_make_len(0.05),
            shell9_make_len(0.01),
            shell9_make_len(0.001),
            shell9_make_point_loads(10.0),
            shell9_make_supports(),
            shell9_make_options("On"),
        ];

        let cancellation = CancellationHandle::new();
        let outcome =
            solve_elastic_static_trampoline(&value_inputs, &[], &Value::Undef, None, &cancellation);
        let fields = shell9_result_fields(outcome);

        // shell_channels must be a "ShellStress" StructureInstance (NOT Undef).
        let sc = fields
            .get("shell_channels")
            .expect("ElasticResult must carry a shell_channels field");
        let sc_data = match sc {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name.as_str(),
                    "ShellStress",
                    "shell_channels must be a ShellStress instance on the shell route"
                );
                d
            }
            other => panic!(
                "shell_channels must be a ShellStress StructureInstance on the shell route, \
                 got {other:?} (shell route not wired — RED until step-10)"
            ),
        };

        let top = shell9_field_data(
            sc_data
                .fields
                .get("top")
                .expect("ShellStress.top"),
        );
        let mid = shell9_field_data(
            sc_data
                .fields
                .get("mid")
                .expect("ShellStress.mid"),
        );
        let bottom = shell9_field_data(
            sc_data
                .fields
                .get("bottom")
                .expect("ShellStress.bottom"),
        );
        assert!(
            !top.is_empty() && top.iter().all(|x| x.is_finite()),
            "top channel must be non-empty and all-finite"
        );
        assert!(
            !mid.is_empty() && mid.iter().all(|x| x.is_finite()),
            "mid channel must be non-empty and all-finite"
        );
        assert!(
            !bottom.is_empty() && bottom.iter().all(|x| x.is_finite()),
            "bottom channel must be non-empty and all-finite"
        );

        // stress must be populated and bit-equal shell_channels.mid (I-2 alias).
        let stress = fields
            .get("stress")
            .expect("ElasticResult must carry a stress field");
        assert!(
            !matches!(stress, Value::Undef),
            "stress must be a populated field on the shell route (I-2 alias source)"
        );
        assert_eq!(
            shell9_field_data(stress),
            mid,
            "I-2 alias: result.stress data must equal shell_channels.mid data element-wise"
        );

        // max-over-elements top-channel von Mises within the one-OOM band.
        assert_eq!(
            top.len() % 9,
            0,
            "top must hold a row-major 3×3 per element (len % 9 == 0)"
        );
        let max_vm = top.chunks_exact(9).map(shell9_vm9).fold(0.0_f64, f64::max);
        assert!(
            max_vm.is_finite() && max_vm > 0.0,
            "max top von Mises must be finite and > 0, got {max_vm}"
        );
        assert!(
            (3e7..=3e9).contains(&max_vm),
            "max top von Mises {max_vm:.4e} Pa outside one-OOM band [3e7, 3e9] Pa \
             around σ=6PL/(bh²)=3e8"
        );
    }

    /// (2) tet route no-regression vs task 4084/α: shell_force=Off keeps
    /// shell_channels + frame Undef, but stress is a POPULATED Regular3D Sampled
    /// Field. Holds today and must keep holding after step-10.
    #[test]
    fn tet_route_trampoline_preserves_4084_baseline() {
        // shell_force=Off forces the tet path regardless of geometry; a 0.1m cube
        // (ratio 1.0) would classify Tet under Auto anyway. Small mesh (nx=6).
        let value_inputs = [
            shell9_make_isotropic_material(205e9, 0.29),
            shell9_make_len(0.1),
            shell9_make_len(0.1),
            shell9_make_len(0.1),
            shell9_make_point_loads(1000.0),
            shell9_make_supports(),
            shell9_make_options("Off"),
        ];

        let cancellation = CancellationHandle::new();
        let outcome =
            solve_elastic_static_trampoline(&value_inputs, &[], &Value::Undef, None, &cancellation);
        let fields = shell9_result_fields(outcome);

        // 4084/α tet baseline: shell_channels + frame remain Undef.
        assert!(
            matches!(
                fields.get("shell_channels"),
                Some(Value::Undef)
            ),
            "tet path must keep shell_channels=Undef (4084/α baseline)"
        );
        assert!(
            matches!(fields.get("frame"), Some(Value::Undef)),
            "tet path must keep frame=Undef (4084/α baseline)"
        );

        // BUT stress is a populated Regular3D Sampled Field (4084/α, NOT Undef).
        let stress = fields
            .get("stress")
            .expect("ElasticResult must carry a stress field");
        match stress {
            Value::Field { source, lambda, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "tet stress must be a Sampled field source"
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => {
                        assert!(
                            matches!(sf.kind, SampledGridKind::Regular3D),
                            "tet stress grid must be Regular3D (4084/α), got {:?}",
                            sf.kind
                        );
                        assert!(
                            !sf.data.is_empty(),
                            "tet stress data must be populated (4084/α — NOT Undef)"
                        );
                    }
                    other => panic!("tet stress lambda must be Value::SampledField, got {other:?}"),
                }
            }
            other => panic!(
                "tet stress must be a populated Value::Field (4084/α baseline), got {other:?}"
            ),
        }
    }

    // ── task 4245 — directional PointLoad ──────────────────────────────────────

    /// step-3 RED (task 4245): `extract_loads` must return a per-axis [f64;3]
    /// force vector instead of a scalar f64.
    ///
    /// Cases:
    ///   (a) PointLoad{force:1000, direction:[0,-1,0]}  → [0,-1000,0]
    ///   (b) PointLoad{force:500}  (no direction field) → [0,0,-500] (default -Z)
    ///   (c) two orthogonal loads: [0,0,-1]*1000 + [0,-1,0]*500 → [0,-500,-1000]
    ///
    /// RED: `extract_loads` currently returns `(f64, Vec<PressureSpec>)`;
    /// destructuring as `([fx,fy,fz], _)` is a compile-fail until step-4.
    #[test]
    fn extract_loads_accumulates_per_axis_force_vector() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        fn make_point_load_with_dir(force: f64, dir: [f64; 3]) -> Value {
            let fields: PersistentMap<String, Value> = [
                ("force".to_string(), Value::Real(force)),
                (
                    "direction".to_string(),
                    Value::List(vec![Value::Real(dir[0]), Value::Real(dir[1]), Value::Real(dir[2])]),
                ),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_name: "PointLoad".to_string(),
                type_id: StructureTypeId(u32::MAX),
                version: 0,
                fields,
            }))
        }

        fn make_point_load_no_dir(force: f64) -> Value {
            let fields: PersistentMap<String, Value> =
                [("force".to_string(), Value::Real(force))].into_iter().collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_name: "PointLoad".to_string(),
                type_id: StructureTypeId(u32::MAX),
                version: 0,
                fields,
            }))
        }

        // (a) explicit direction [0,-1,0] with force 1000 → [0,-1000,0]
        let loads_a = Value::List(vec![make_point_load_with_dir(1000.0, [0.0, -1.0, 0.0])]);
        let ([fx, fy, fz], _) = extract_loads(&loads_a);
        assert!((fx).abs() < 1e-9, "(a) expected fx≈0, got {fx}");
        assert!((fy - (-1000.0)).abs() < 1e-9, "(a) expected fy=-1000, got {fy}");
        assert!((fz).abs() < 1e-9, "(a) expected fz≈0, got {fz}");

        // (b) no direction field → default [0,0,-1]; force 500 → [0,0,-500]
        let loads_b = Value::List(vec![make_point_load_no_dir(500.0)]);
        let ([fx, fy, fz], _) = extract_loads(&loads_b);
        assert!((fx).abs() < 1e-9, "(b) expected fx≈0, got {fx}");
        assert!((fy).abs() < 1e-9, "(b) expected fy≈0, got {fy}");
        assert!((fz - (-500.0)).abs() < 1e-9, "(b) expected fz=-500, got {fz}");

        // (c) two orthogonal loads: [0,0,-1]*1000 + [0,-1,0]*500 → [0,-500,-1000]
        let loads_c = Value::List(vec![
            make_point_load_with_dir(1000.0, [0.0, 0.0, -1.0]),
            make_point_load_with_dir(500.0, [0.0, -1.0, 0.0]),
        ]);
        let ([fx, fy, fz], _) = extract_loads(&loads_c);
        assert!((fx).abs() < 1e-9, "(c) expected fx≈0, got {fx}");
        assert!((fy - (-500.0)).abs() < 1e-9, "(c) expected fy=-500, got {fy}");
        assert!((fz - (-1000.0)).abs() < 1e-9, "(c) expected fz=-1000, got {fz}");
    }

    /// amendment (task 4245 esc): `extract_loads` direction elements carried as
    /// `Value::Scalar` (e.g. structure-def defaults materialising as dimensionless
    /// scalars) are handled by the `Value::Scalar { si_value, .. }` branch.
    ///
    /// This test covers suggestion-3 from the code-review: the Scalar branch was
    /// claimed as the path for structure-def defaults, but had no dedicated
    /// regression test.  A PointLoad whose direction list contains `Value::Scalar`
    /// elements must behave identically to one with `Value::Real` elements.
    #[test]
    fn extract_loads_direction_scalar_elements_handled() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        // Build a PointLoad where direction elements are Value::Scalar
        // (dimensionless), mirroring structure-def default materialisation.
        let dir_scalars: Vec<Value> = [-0.0_f64, -1.0_f64, 0.0_f64]
            .iter()
            .map(|&v| Value::Scalar { si_value: v, dimension: DimensionVector::DIMENSIONLESS })
            .collect();
        let fields: PersistentMap<String, Value> = [
            ("force".to_string(), Value::Real(800.0)),
            ("direction".to_string(), Value::List(dir_scalars)),
        ]
        .into_iter()
        .collect();
        let point_load = Value::StructureInstance(Box::new(StructureInstanceData {
            type_name: "PointLoad".to_string(),
            type_id: StructureTypeId(u32::MAX),
            version: 0,
            fields,
        }));

        let loads = Value::List(vec![point_load]);
        let ([fx, fy, fz], _) = extract_loads(&loads);
        // force=800, direction=[0,-1,0] → tip_force_vec=[0,-800,0]
        assert!((fx).abs() < 1e-9, "expected fx≈0, got {fx}");
        assert!((fy - (-800.0)).abs() < 1e-9, "expected fy=-800, got {fy}");
        assert!((fz).abs() < 1e-9, "expected fz≈0, got {fz}");
    }

    /// amendment (task 4245 esc): malformed `direction` values silently fall back
    /// to `[0, 0, -1]` — this is the intentional forward-compatibility contract
    /// (design_decision[4] in plan.json).  This test pins the contract so the
    /// silent-default behaviour is a deliberate, regression-tested choice rather
    /// than dead code.
    ///
    /// Cases:
    ///   (a) direction list has only 2 elements (length ≠ 3) → fallback to -Z.
    ///   (b) direction field is a `Value::String` (entirely wrong type, e.g. typo
    ///       in a Rust-constructed test fixture) → fallback to -Z.
    #[test]
    fn extract_loads_malformed_direction_defaults_to_neg_z() {
        use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

        fn make_point_load(force: f64, direction: Value) -> Value {
            let fields: PersistentMap<String, Value> = [
                ("force".to_string(), Value::Real(force)),
                ("direction".to_string(), direction),
            ]
            .into_iter()
            .collect();
            Value::StructureInstance(Box::new(StructureInstanceData {
                type_name: "PointLoad".to_string(),
                type_id: StructureTypeId(u32::MAX),
                version: 0,
                fields,
            }))
        }

        // (a) too-short list [0.0, -1.0] → fallback to [0,0,-1]; force=300
        let short_dir = Value::List(vec![Value::Real(0.0), Value::Real(-1.0)]);
        let loads_a = Value::List(vec![make_point_load(300.0, short_dir)]);
        let ([fx, fy, fz], _) = extract_loads(&loads_a);
        assert!((fx).abs() < 1e-9, "(a) fx: expected 0, got {fx}");
        assert!((fy).abs() < 1e-9, "(a) fy: expected 0, got {fy}");
        assert!(
            (fz - (-300.0)).abs() < 1e-9,
            "(a) fz: expected -300 (default -Z fallback), got {fz}"
        );

        // (b) direction is a String (entirely wrong type) → fallback to [0,0,-1]
        let str_dir = Value::String("neg_z".to_string());
        let loads_b = Value::List(vec![make_point_load(400.0, str_dir)]);
        let ([fx, fy, fz], _) = extract_loads(&loads_b);
        assert!((fx).abs() < 1e-9, "(b) fx: expected 0, got {fx}");
        assert!((fy).abs() < 1e-9, "(b) fy: expected 0, got {fy}");
        assert!(
            (fz - (-400.0)).abs() < 1e-9,
            "(b) fz: expected -400 (default -Z fallback), got {fz}"
        );
    }

    // ── task 4366: cancel short-circuit + cadence ─────────────────────────────

    /// step-1 RED (task 4366): when the progress closure returns
    /// `CgIterationControl::Cancel` on its first call, `solve_cantilever_fea`
    /// must NOT run the stress-recovery loop: `nodal_stress` must be empty and
    /// `max_von_mises` must be 0.0.
    ///
    /// Contrast case: the same fixture solved with `None` progress (no cancel)
    /// must converge, populate `nodal_stress`, and have `max_von_mises > 0`.
    ///
    /// RED on base: the cancel branch does not exist yet — stress recovery
    /// always runs, so `nodal_stress.is_empty()` fails.
    #[test]
    fn solve_cantilever_fea_cancelled_skips_stress_recovery() {
        let iso = IsotropicElastic { youngs_modulus: 200e9_f64, poisson_ratio: 0.3_f64 };
        let model = MaterialModel::Isotropic(iso);
        let length = 1.0_f64;
        let width = 0.1_f64;
        let height = 0.1_f64;
        let tip_force = [0.0_f64, 0.0, -1000.0];

        // ── Case 1: cancel on first iteration ──────────────────────────────────
        let mut cancelled = false;
        let (fea_cancelled, _) = solve_cantilever_fea(
            &model,
            length,
            width,
            height,
            tip_force,
            None,
            &[],
            true,
            None,
            Some(&mut |_iter: usize, _residual: f64| -> CgIterationControl {
                cancelled = true;
                CgIterationControl::Cancel
            }),
        );

        assert!(cancelled, "progress closure must have been invoked at least once");
        assert!(
            !fea_cancelled.converged,
            "cancelled solve must report converged=false"
        );
        assert!(
            fea_cancelled.iterations >= 1,
            "cancelled solve must report ≥1 iteration, got {}",
            fea_cancelled.iterations
        );
        assert!(
            fea_cancelled.nodal_stress.is_empty(),
            "cancelled solve must skip stress recovery — nodal_stress must be empty, \
             got {} entries",
            fea_cancelled.nodal_stress.len()
        );
        assert_eq!(
            fea_cancelled.max_von_mises,
            0.0,
            "cancelled solve must skip stress recovery — max_von_mises must be 0.0"
        );

        // ── Case 2: no cancel (None progress) → full stress recovery ───────────
        let (fea_full, _) = solve_cantilever_fea(
            &model,
            length,
            width,
            height,
            tip_force,
            None,
            &[],
            true,
            None,
            None,
        );

        assert!(fea_full.converged, "uncancelled solve must converge");
        assert!(
            !fea_full.nodal_stress.is_empty(),
            "uncancelled solve must populate nodal_stress"
        );
        assert!(
            fea_full.max_von_mises > 0.0,
            "uncancelled solve must have max_von_mises > 0, got {}",
            fea_full.max_von_mises
        );
    }

    /// step-3 RED (task 4245): `solve_cantilever_fea` honours a -Y tip_force.
    ///
    /// tip_force = [0.0, -1000.0, 0.0] on a 1 m × 0.1 m × 0.1 m beam.
    /// Expected (sign/dominance only — ny=1 mesh is coarse in Y):
    ///   - result.converged
    ///   - mean tip u_y < 0   (load applied in −Y)
    ///   - |mean u_y| > 2 × |mean u_z|  (direction is honoured, not hardcoded -Z)
    ///
    /// RED: `solve_cantilever_fea` currently takes `tip_force: f64`; passing
    /// `[f64;3]` is a compile-fail until step-4.
    #[test]
    fn solve_cantilever_fea_directional_y_load() {
        let iso = IsotropicElastic { youngs_modulus: 200e9, poisson_ratio: 0.3 };
        let model = MaterialModel::Isotropic(iso);

        let (result, _) =
            solve_cantilever_fea(
                &model, 1.0, 0.1, 0.1, [0.0, -1000.0, 0.0], None, &[], true, None, None,
            );

        assert!(result.converged, "directional Y-load solve must converge");

        let n = result.tip_nodes.len().max(1) as f64;
        let mean_uy: f64 =
            result.tip_nodes.iter().map(|&nd| result.u[3 * nd + 1]).sum::<f64>() / n;
        let mean_uz: f64 =
            result.tip_nodes.iter().map(|&nd| result.u[3 * nd + 2]).sum::<f64>() / n;

        assert!(mean_uy < 0.0, "tip mean u_y must be < 0 under −Y load, got {mean_uy}");
        assert!(
            mean_uy.abs() > mean_uz.abs() * 2.0,
            "−Y load must dominate: |u_y|={:.4e} must be > 2×|u_z|={:.4e}",
            mean_uy.abs(),
            mean_uz.abs(),
        );
    }
}
