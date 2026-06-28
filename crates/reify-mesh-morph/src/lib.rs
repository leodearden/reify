//! Mesh morphing classifier and engine for Reify.
//!
//! This crate provides the combined eligibility predicate for mesh morphing
//! (PRD `docs/prds/v0_3/mesh-morphing.md`, tasks #3 and #10).
//!
//! ## PRD task #5 — boundary-node correspondence + closest-point projection — boundary module
//!
//! The [`boundary`] module implements the surface-node → Dirichlet-BC
//! translation step that gates the elasticity morph (PRD task #7).
//!
//! ## PRD task #6 — Laplacian quick-pass — laplacian module
//!
//! The [`laplacian`] module implements the constrained Laplacian smoother
//! used as the cheap fast path for trivially small parameter changes —
//! surface nodes pinned to `prescribed_positions` (produced by
//! [`compute_dirichlet_bcs`]), interior nodes iteratively averaged with
//! their topological neighbours via Jacobi iteration. Engine wiring (PRD
//! task #10) selects between this smoother and the elasticity morph.
//!
//! ## PRD task #7 — linear-elasticity morph — elasticity module
//!
//! The [`elasticity`] module implements the primary morph algorithm: treat
//! the source mesh as a fictitious-elastic continuum, prescribe surface-
//! node displacements as Dirichlet BCs, and solve the linear-elastostatic
//! BVP `K · u = 0` for interior-node displacements. Composes four
//! `reify-solver-elastic` primitives (`element_stiffness`,
//! `assemble_global_stiffness`, `apply_dirichlet_row_elimination`,
//! `solve_cg`); the output mesh is `vertices_old + u`. Engine wiring (PRD
//! task #10) selects between this morph and the Laplacian quick-pass.
//!
//! ## PRD task #9 — quality check — quality module
//!
//! The [`quality`] module implements the two-tier quality-check pass that
//! runs after the morph engine produces a deformed mesh. Returns
//! [`QualityVerdict::Pass`], [`QualityVerdict::HardFail`] (element
//! inversion), or [`QualityVerdict::SoftFail`] (metric threshold breach).
//! Engine wiring (PRD task #10) maps hard/soft fail to remesh fallback.
//!
//! ## PRD task #13 — quality-threshold calibration — tests/calibration.rs
//!
//! The three quality-floor knobs on [`MorphOptions`]
//! (`quality_floor_min_scaled_jacobian`, `quality_floor_pct_below_025`,
//! `quality_aspect_ratio_factor_max`) are calibrated against two
//! procedural parametric fixtures shipped as a regression-guard suite in
//! `tests/calibration.rs`:
//!
//! - **plate hole-diameter sweep** — polar-radial grid with hex-to-6-tet
//!   decomposition; intrinsic min-scaled-J ≈ 0.022 at small steps.
//! - **bracket fillet-radius sweep** — L-bracket with parametric inner
//!   fillet; the discriminating fixture in calibration coverage — exercises
//!   both well-conditioned and near-degenerate fillet radii so the
//!   calibrated thresholds are stressed across the parameter range.
//!   Pinned by an explicit Pass/Reject verdict-mix assertion at the end of
//!   the test.
//!
//! The calibration rule: morph is rejected only when a from-scratch remesh
//! is *materially better* (> 20 % improvement on the relevant metric). This
//! is encoded as `from_scratch > MATERIALITY_FACTOR * morph` for
//! higher-is-better metrics (min scaled J) and
//! `from_scratch_max_ar_factor > MATERIALITY_FACTOR` for lower-is-better
//! metrics, where `from_scratch_max_ar_factor` is the true
//! `max(morphed_AR / from_scratch_AR)` ratio computed in
//! `tests/calibration/sweep.rs::extract_metrics` and exposed via the
//! `SweepReport` field of the same name. The live predicates live in
//! `tests/calibration/sweep.rs::sj_materially_better` and
//! `tests/calibration/sweep.rs::ar_materially_better`; the canonical
//! materiality constant lives in
//! `tests/calibration/sweep.rs::MATERIALITY_FACTOR`.
//!
//! Calibration was performed against the [`StiffnessRule::InverseVolume`]
//! production default (PRD task #8 / task 2945, shipped on main).
pub mod boundary;
pub mod diagnostics;
pub mod elasticity;
pub mod eligibility;
pub mod laplacian;
pub mod options;
pub mod quality;
pub mod stats;
pub mod types;

pub use stats::{MorphStats, record_morph_attempt, record_rejection, record_remesh, snapshot};
// Bare diagnostics re-exports. Two symbols are deliberately omitted:
//   - `snapshot` stays reachable as `diagnostics::snapshot()` to avoid
//     colliding with the `stats::snapshot` re-export above; and
//   - `MorphOutcome` is kept crate-internal — it carries no external behaviour
//     (its only consumer is the private bucket-routing `fn counter`), so it is
//     not part of the public API surface.
pub use diagnostics::{
    DiagnosticSnapshot, format_summary, record_ineligible, record_morphed, record_panicked,
    record_quality_remesh,
};
pub use boundary::{
    BoundaryAssociation, NodeAttachment, ProjectionFailure, Projector, ProjectorPayload,
    compute_dirichlet_bcs,
};
pub use elasticity::{ElasticityFailure, elasticity_morph, elasticity_morph_with_cg_opts};
pub use eligibility::{Eligibility, MorphSnapshot, Reason, morph_eligible};
pub use laplacian::{LaplacianFailure, laplacian_smooth};
pub use options::{MorphFailure, MorphOptions, StiffnessRule};
pub use quality::{QualityVerdict, quality_check};
pub use types::{BRep, InversionDetails, SoftFailDetails, SolverErrorPayload};

/// Re-exported so consumers can pattern-match `Reason::BijectionFailure(_)`
/// without depending on `reify-eval` directly.
pub use reify_eval::{
    BijectionFailure, CorrespondenceMap, NamingLayerErrorReason, SubShapeKind, SubShapeSide,
};
/// Re-exported so consumers of [`elasticity_morph_with_cg_opts`] can construct
/// `CgSolverOptions` without depending on `reify-solver-elastic` directly.
pub use reify_solver_elastic::CgSolverOptions;

// ── Public API ────────────────────────────────────────────────────────────────

/// Bool-only wrapper around [`morph_eligible`] per PRD task #4.
///
/// Returns `true` if both Stage A and Stage B pass (the edit is morphable),
/// `false` otherwise. The structured rejection [`Reason`] is discarded;
/// callers that need it for failure-mode visibility counters (PRD task #11)
/// should call [`morph_eligible`] directly.
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947
pub fn eligible(old_brep: BRep, new_brep: BRep) -> bool {
    // `BRep` is `Copy` (alias for `MorphSnapshot<'a>`); pass by value matches
    // `morph_eligible`'s signature directly.
    matches!(
        eligibility::morph_eligible(old_brep, new_brep),
        Eligibility::Eligible(_)
    )
}

/// Morph `old_mesh` to the shape described by `new_brep`, returning the
/// deformed [`reify_types::VolumeMesh`] on success.
///
/// ## API contract (task #4)
///
/// The function commits the full public signature. The engine logic is deferred:
///
/// | Path | Behaviour |
/// |------|-----------|
/// | Ineligible edit | Returns `Err(MorphFailure::Ineligible(reason))` immediately |
/// | Eligible edit | Returns `Err(MorphFailure::SolverError(...))` until PRD tasks #5–#9 land the engine |
///
/// ## Parameters
///
/// - `old_mesh` — the current tetrahedral mesh to deform.
/// - `old_brep` / `new_brep` — boundary-rep snapshots for eligibility and
///   boundary-node projection (PRD task #5).
/// - `options` — quality thresholds and fictitious-stiffness parameters;
///   see [`MorphOptions`].
///
/// ## Failure modes
///
/// See [`MorphFailure`] for the four-variant taxonomy. Only `Ineligible` is
/// produced by this skeleton; the remaining three variants are wired in PRD
/// tasks #7 and #9.
pub fn morph(
    old_mesh: &reify_ir::VolumeMesh,
    old_brep: BRep,
    new_brep: BRep,
    options: &MorphOptions,
) -> Result<reify_ir::VolumeMesh, MorphFailure> {
    let _ = old_mesh;
    let _ = options;
    match eligibility::morph_eligible(old_brep, new_brep) {
        Eligibility::Ineligible(reason) => Err(MorphFailure::Ineligible(reason)),
        Eligibility::Eligible(_correspondence_map) => {
            Err(MorphFailure::SolverError(SolverErrorPayload::new(
                "engine not yet implemented (PRD docs/prds/v0_3/mesh-morphing.md tasks #5-#9)",
            )))
        }
    }
}

// ── compose_morph: the real morph pipeline (task 4744 β) ───────────────────────

/// Fraction of the source mesh's bounding-box diagonal below which the cheap
/// Laplacian quick-pass is used instead of the linear-elasticity solve. Above
/// this fraction the (more robust, more expensive) elasticity morph runs.
///
/// The magnitude rule is intentionally coarse — PRD task #10 §"engine wiring"
/// leaves the Laplacian-vs-elasticity cutover tactical. Tunable.
const LAPLACIAN_DISPLACEMENT_FRACTION: f64 = 0.05;

/// Compose the landed morph primitives into the full morph pipeline used at the
/// engine seam (task 4744 β / PRD `docs/prds/v0_3/mesh-morphing.md` task #10).
///
/// Distinct from the [`morph`] skeleton: this fn additionally takes the source
/// mesh's [`BoundaryAssociation`] (from the 4092 attributed VolumeMesh producer)
/// and a `&dyn GeometryKernel` for the **new** BRep, so it can actually project
/// boundary nodes and deform the mesh. Pipeline:
///
/// 1. [`morph_eligible`] — Stage A + Stage B → [`CorrespondenceMap`].
/// 2. [`compute_dirichlet_bcs`] over a [`KernelProjector`] — project each
///    boundary node onto its mapped new-BRep entity (cycle-free: names only
///    `reify_ir::GeometryKernel`).
/// 3. Displacement-magnitude rule → [`laplacian_smooth`] (small) or
///    [`elasticity_morph`] (large). The solve is wrapped in
///    [`std::panic::catch_unwind`] so a solver panic is recorded
///    ([`record_panicked`]) and degraded to a structured failure rather than
///    unwinding through the engine dispatch.
/// 4. [`quality_check`] — on [`QualityVerdict::Pass`], [`record_morphed`] and
///    return the deformed mesh.
///
/// Connectivity is preserved by construction (both solvers deform vertices in
/// place and clone `tet_indices`).
///
/// The eligibility-reject and quality-reject failure arms return a structured
/// [`MorphFailure`]; their diagnostic counters (`record_ineligible` /
/// `record_quality_remesh`) are wired by task 4744 step-10.
// G-allow: mesh-morph public API — §3.2 realization-kind dispatch producer; consumer is the morph arm at the VolumeMesh dispatch (task #4744 steps 16/18, engine_build.rs + register_morph_producer)
pub fn compose_morph(
    source_mesh: &reify_ir::VolumeMesh,
    boundary: &BoundaryAssociation,
    old_brep: BRep,
    new_brep: BRep,
    kernel: &dyn reify_ir::GeometryKernel,
    options: &MorphOptions,
) -> Result<reify_ir::VolumeMesh, MorphFailure> {
    // 1. Stage A + Stage B eligibility → correspondence map. On reject, record
    //    the matching ineligible bucket (structural / bijection / naming).
    let correspondence = match eligibility::morph_eligible(old_brep, new_brep) {
        Eligibility::Eligible(map) => map,
        Eligibility::Ineligible(reason) => {
            record_ineligible(&reason);
            return Err(MorphFailure::Ineligible(reason));
        }
    };

    // 2. Project boundary nodes onto the NEW BRep through the kernel. The
    //    KernelProjector names only reify_ir::GeometryKernel — the cycle-free
    //    seam that lets this crate project without a reify-kernel-occt dep.
    let projector = boundary::KernelProjector(kernel);
    let prescribed = compute_dirichlet_bcs(source_mesh, boundary, &correspondence, &projector)
        .map_err(|failure| {
            MorphFailure::SolverError(SolverErrorPayload::new(format!(
                "boundary-node projection failed: {failure:?}"
            )))
        })?;

    // 3. Displacement-magnitude rule: small → Laplacian quick-pass; large →
    //    elasticity. Wrap the solve in catch_unwind so a solver panic becomes a
    //    recorded, honest fallback rather than unwinding the engine dispatch.
    let use_laplacian = displacement_is_small(source_mesh, &prescribed);
    let solve = std::panic::AssertUnwindSafe(|| {
        if use_laplacian {
            laplacian_smooth(source_mesh, &prescribed, options.laplacian_iterations)
                .map_err(|e| SolverErrorPayload::new(format!("laplacian morph failed: {e:?}")))
        } else {
            elasticity_morph(source_mesh, &prescribed, options)
                .map_err(|e| SolverErrorPayload::new(format!("elasticity morph failed: {e:?}")))
        }
    });
    let morphed = match std::panic::catch_unwind(solve) {
        Ok(Ok(mesh)) => mesh,
        Ok(Err(payload)) => return Err(MorphFailure::SolverError(payload)),
        Err(panic) => {
            let detail = panic_detail(panic.as_ref());
            record_panicked(&detail);
            return Err(MorphFailure::SolverError(SolverErrorPayload::new(format!(
                "morph solver panicked: {detail}"
            ))));
        }
    };

    // 4. Quality gate (note: quality_check takes the morphed mesh first, the
    //    source second). On Pass: record the morph and return the deformed mesh.
    //    On any fail verdict: record the matching hard/soft remesh bucket exactly
    //    once and return the structured failure so the caller remeshes.
    let verdict = quality_check(&morphed, source_mesh, options);
    if matches!(verdict, QualityVerdict::Pass) {
        record_morphed();
        return Ok(morphed);
    }
    record_quality_remesh(&verdict);
    match verdict {
        QualityVerdict::HardFail(details) => Err(MorphFailure::QualityHardFail(details)),
        QualityVerdict::SoftFail(details) => Err(MorphFailure::QualitySoftFail(details)),
        QualityVerdict::Pass => unreachable!("Pass returns early above"),
    }
}

/// True if the maximum prescribed boundary displacement is small relative to the
/// source mesh's bounding-box diagonal — the [`LAPLACIAN_DISPLACEMENT_FRACTION`]
/// cutover between the Laplacian quick-pass and the elasticity solve.
fn displacement_is_small(
    mesh: &reify_ir::VolumeMesh,
    prescribed: &[(u32, [f64; 3])],
) -> bool {
    let mut max_disp = 0.0_f64;
    for (idx, pos) in prescribed {
        if let Some(old) = mesh.vertex_f64(*idx) {
            let d = [pos[0] - old[0], pos[1] - old[1], pos[2] - old[2]];
            let mag = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            max_disp = max_disp.max(mag);
        }
    }
    let scale = bbox_diagonal(mesh).max(1e-12);
    max_disp <= LAPLACIAN_DISPLACEMENT_FRACTION * scale
}

/// Bounding-box diagonal length of a [`reify_ir::VolumeMesh`]'s flat XYZ vertex
/// buffer (0.0 for an empty/degenerate buffer).
fn bbox_diagonal(mesh: &reify_ir::VolumeMesh) -> f64 {
    if mesh.vertices.len() < 3 {
        return 0.0;
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for c in mesh.vertices.chunks_exact(3) {
        for k in 0..3 {
            min[k] = min[k].min(c[k]);
            max[k] = max[k].max(c[k]);
        }
    }
    let dx = (max[0] - min[0]) as f64;
    let dy = (max[1] - min[1]) as f64;
    let dz = (max[2] - min[2]) as f64;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Best-effort extraction of a panic message from a caught panic payload.
fn panic_detail(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

// ── register_morph_producer: install the engine seam (task 4744 β) ─────────────

/// Install the mesh-morph producer hook on `engine`.
///
/// Mirrors `reify_eval::compute_targets::register_compute_fns`: called once at
/// Engine construction (by `reify-cli` for production, by the `reify-eval`
/// morph-arm e2e for tests) to install the single [`reify_eval::MorphProducer`]
/// the VolumeMesh realization dispatch probes before remeshing. The installed
/// producer wraps the request's new-BRep kernel in a [`KernelProjector`] (via
/// [`compose_morph`]) and drives the full morph composition.
///
/// # Panics
///
/// Panics if a producer is already registered — the single-install discipline of
/// [`reify_eval::Engine::register_morph_producer`] (mirrors `register_compute_fn`).
// G-allow: mesh-morph engine-seam installer — §4.2/D3 producer registration (mirrors register_compute_fns); consumers: reify-cli production registration (task #4744 step-22) + the reify-eval morph-arm e2e (task #4744 step-19/20)
pub fn register_morph_producer(engine: &mut reify_eval::Engine) {
    engine.register_morph_producer(Box::new(MeshMorphProducer));
}

/// The production [`reify_eval::MorphProducer`] implementation.
///
/// A stateless unit struct (uses `MorphOptions::default()` per call): its
/// [`try_morph`][reify_eval::MorphProducer::try_morph] re-assembles the
/// borrowing `reify_eval::BRepSnapshot`s into this crate's [`BRep`] alias, runs
/// [`compose_morph`] (which wraps the kernel in a [`KernelProjector`]), and
/// renders the structured [`MorphFailure`] into the engine-side
/// [`reify_eval::MorphResult`] string variants.
///
/// `Send + Sync` is satisfied trivially (no fields) — the trait bound that lets
/// the boxed producer live on the shared [`reify_eval::Engine`].
struct MeshMorphProducer;

impl reify_eval::MorphProducer for MeshMorphProducer {
    fn try_morph(&self, ctx: reify_eval::MorphRequest<'_>) -> reify_eval::MorphResult {
        use reify_eval::MorphResult;

        let options = MorphOptions::default();
        match compose_morph(
            ctx.source,
            ctx.boundary,
            brep_from_snapshot(ctx.old_brep),
            brep_from_snapshot(ctx.new_brep),
            ctx.kernel,
            &options,
        ) {
            Ok(mesh) => MorphResult::Ok(mesh),
            // The structured reify-mesh-morph reason/verdict types cannot be
            // named across the cycle boundary, so render them to text — the
            // engine decision helper only matches the variant + logs the string.
            Err(MorphFailure::Ineligible(reason)) => MorphResult::Ineligible(format!("{reason:?}")),
            Err(MorphFailure::QualityHardFail(details)) => {
                MorphResult::QualityReject(format!("quality hard fail: {details:?}"))
            }
            Err(MorphFailure::QualitySoftFail(details)) => {
                MorphResult::QualityReject(format!("quality soft fail: {details:?}"))
            }
            Err(MorphFailure::SolverError(payload)) => {
                MorphResult::SolverError(payload.message().to_string())
            }
        }
    }
}

/// Re-assemble a borrowing `reify_eval::BRepSnapshot` into this crate's [`BRep`]
/// alias (`eligibility::MorphSnapshot`).
///
/// The two are field-identical (both name only `reify_eval`/`reify_ir` types),
/// but the engine cannot name `MorphSnapshot` across the cycle boundary, so it
/// hands the constituents over as a `BRepSnapshot` and this crate re-wraps them
/// before calling [`compose_morph`]. Lifetime-transparent: the returned `BRep`
/// borrows exactly as long as `snap`.
fn brep_from_snapshot(snap: reify_eval::BRepSnapshot<'_>) -> BRep<'_> {
    BRep {
        graph: snap.graph,
        values: snap.values,
        topology_attributes: snap.topology_attributes,
        faces: snap.faces,
        edges: snap.edges,
        vertices: snap.vertices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_eval::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
    use reify_core::{ContentHash, RealizationNodeId, Type, ValueCellId};
    use reify_ir::{CapKind, FeatureId, GeometryHandleId, ReprKind, Role, TopologyAttribute, TopologyAttributeTable, Value, ValueMap};

    // ── Test fixture helpers (mirrored from eligibility::tests) ───────────────

    fn graph_with_cell(id: &ValueCellId, cell_type: Type) -> EvaluationGraph {
        let mut g = EvaluationGraph::default();
        g.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type,
                default_expr: None,
                content_hash: ContentHash::of_str(&format!("{id}")),
            },
        );
        g
    }

    fn diverged_graph(base: &EvaluationGraph) -> EvaluationGraph {
        let mut g = base.clone();
        let rnid = RealizationNodeId::new("Extra", 0);
        g.realizations.insert(
            rnid.clone(),
            RealizationNodeData { geometry_cell: None,
                id: rnid,
                operations: Vec::new(),
                content_hash: ContentHash::of_str("diverge"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
                input_cone_hash: None,
            },
        );
        g
    }

    fn make_brep<'a>(
        graph: &'a EvaluationGraph,
        values: &'a ValueMap,
        table: &'a TopologyAttributeTable,
    ) -> BRep<'a> {
        BRep {
            graph,
            values,
            topology_attributes: table,
            faces: &[],
            edges: &[],
            vertices: &[],
        }
    }

    fn empty_mesh() -> reify_ir::VolumeMesh {
        reify_ir::VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: reify_ir::ElementOrderTag::P1,
            normals: None,
            boundary: None,
        }
    }

    // ── Stage-B fixture helpers (mirrored from eligibility::tests) ────────────

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    fn feat() -> FeatureId {
        FeatureId::realization("Feature", 0)
    }

    fn attr(role: Role, local_index: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: feat(),
            role,
            local_index,
            user_label: None,
            mod_history: Vec::new(),
        }
    }

    // ── Step-5: eligible() contract ───────────────────────────────────────────

    #[test]
    fn eligible_returns_true_when_morph_eligible_yields_eligible() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        assert!(eligible(old_brep, new_brep));
    }

    #[test]
    fn eligible_returns_false_on_stage_a_structural_change() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = diverged_graph(&old_graph);
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        assert!(!eligible(old_brep, new_brep));
    }

    // ── Step-7/amendment: morph() Ineligible and Eligible paths ─────────────

    #[test]
    fn morph_returns_solver_error_on_eligible_path() {
        // Verifies the Eligible arm of morph() returns SolverError (not panics)
        // until the engine lands in PRD tasks #5–#9.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(
            matches!(result, Err(MorphFailure::SolverError(_))),
            "eligible path should return SolverError (unimplemented), got: {result:?}"
        );
    }

    #[test]
    fn morph_returns_ineligible_failure_on_stage_a_structural_change() {
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = diverged_graph(&old_graph);
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));
        let table = TopologyAttributeTable::default();
        let old_brep = make_brep(&old_graph, &values, &table);
        let new_brep = make_brep(&new_graph, &values, &table);
        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(matches!(
            result,
            Err(MorphFailure::Ineligible(Reason::StructuralChange))
        ));
    }

    // ── Step-31: lib re-exports make boundary module public surface accessible ─

    // Compile fence: verifies each name from the boundary module is accessible
    // from the crate root, and pins the compute_dirichlet_bcs signature.
    // Follows the `const _: fn() = || { ... }` discipline in eligibility.rs —
    // no runtime assertions, just type-check guarantees.
    const _: fn() = || {
        use crate::{
            BoundaryAssociation, NodeAttachment, ProjectionFailure, Projector, ProjectorPayload,
            compute_dirichlet_bcs,
        };
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &BoundaryAssociation,
            &reify_eval::CorrespondenceMap,
            &dyn Projector,
        ) -> Result<Vec<(u32, [f64; 3])>, ProjectionFailure> = compute_dirichlet_bcs;
        // Type mentions for names not in _fn_ref; avoids unused-import warnings.
        let _: Option<NodeAttachment> = None;
        let _: Option<ProjectorPayload> = None;
    };

    // ── Step-18: lib re-exports make laplacian module public surface accessible ─

    // Compile fence: verifies LaplacianFailure variants and laplacian_smooth
    // are accessible from the crate root, and pins the laplacian_smooth
    // signature. Same discipline as the boundary fence above — fails to
    // compile if a re-export drops or the public signature drifts.
    const _: fn() = || {
        use crate::{LaplacianFailure, laplacian_smooth};
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            u32,
        ) -> Result<reify_ir::VolumeMesh, LaplacianFailure> = laplacian_smooth;
        // Variant mentions force the enum's variant set into the fence — adding
        // or removing a variant under the same names elsewhere would still
        // require these constructors to compile.
        let _: LaplacianFailure = LaplacianFailure::InvalidNodeIndex(0u32);
        let _: LaplacianFailure =
            LaplacianFailure::UnsupportedElementOrder(reify_ir::ElementOrderTag::P2);
    };

    // ── Step-17: lib re-exports make elasticity module public surface accessible ─

    // Compile fence: verifies ElasticityFailure variants and elasticity_morph
    // are accessible from the crate root, and pins the elasticity_morph
    // signature. Same discipline as the boundary, laplacian, and quality
    // fences above — fails to compile if a re-export drops, the public
    // signature drifts, or a variant is renamed.
    const _: fn() = || {
        use crate::{
            CgSolverOptions, ElasticityFailure, elasticity_morph, elasticity_morph_with_cg_opts,
        };
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            &MorphOptions,
        ) -> Result<reify_ir::VolumeMesh, ElasticityFailure> = elasticity_morph;
        #[allow(clippy::type_complexity)]
        // pinning the full public signature is the point of the fence
        let _fn_with_opts: fn(
            &reify_ir::VolumeMesh,
            &[(u32, [f64; 3])],
            &MorphOptions,
            CgSolverOptions,
        )
            -> Result<reify_ir::VolumeMesh, ElasticityFailure> = elasticity_morph_with_cg_opts;
        let _ = _fn_with_opts;
        // Variant mentions force the enum's variant set into the fence —
        // adding or removing a variant under the same names elsewhere would
        // still require these constructors to compile.
        let _: ElasticityFailure = ElasticityFailure::InvalidNodeIndex(0u32);
        let _: ElasticityFailure =
            ElasticityFailure::UnsupportedElementOrder(reify_ir::ElementOrderTag::P2);
        let _: ElasticityFailure = ElasticityFailure::SolverNotConverged { iterations: 0 };
        let _: ElasticityFailure = ElasticityFailure::InvalidTetIndex(0u32);
        let _: ElasticityFailure = ElasticityFailure::NoElementsForPrescribedDisplacements;
        let _: ElasticityFailure = ElasticityFailure::MalformedTetIndices { len: 0 };
    };

    // ── Step-12: lib re-exports make quality module public surface accessible ──

    // Compile fence: verifies quality_check and QualityVerdict are accessible
    // from the crate root, pins the quality_check signature, and exhaustively
    // mentions all three QualityVerdict variant constructors.
    // Same discipline as the boundary and laplacian fences above.
    const _: fn() = || {
        use crate::{QualityVerdict, quality_check};
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            &reify_ir::VolumeMesh,
            &MorphOptions,
        ) -> QualityVerdict = quality_check;
        // Variant mentions — exhaustive constructor coverage:
        let _: QualityVerdict = QualityVerdict::Pass;
        let _: QualityVerdict = QualityVerdict::HardFail(crate::types::InversionDetails {
            element_index: 0,
            jacobian: -0.5,
        });
        let _: QualityVerdict = QualityVerdict::SoftFail(crate::types::SoftFailDetails {
            min_scaled_jacobian: None,
            pct_below_025: None,
            max_aspect_ratio_factor: None,
            degenerate_morphed_element: None,
        });
    };

    // ── task 2945: lib re-export + variant fence for StiffnessRule ───────────

    // Compile fence: verifies StiffnessRule and all three variants are accessible
    // from the crate root. Adding, removing, or renaming a variant or dropping the
    // re-export breaks compilation immediately.
    const _: fn() = || {
        use crate::StiffnessRule;
        let _: StiffnessRule = StiffnessRule::Uniform;
        let _: StiffnessRule = StiffnessRule::InverseVolume;
        let _: StiffnessRule = StiffnessRule::InverseEdgeLengthSquared;
    };

    // ── task 2948: lib re-export fence for the diagnostics surface ───────────

    // Compile fence: verifies the diagnostics surface is re-exported from the
    // crate root. Two symbols are intentionally absent from the bare re-export
    // (and therefore from this fence): `snapshot`, reached via the
    // `diagnostics::` module path to avoid colliding with the existing
    // `stats::snapshot` re-export; and `MorphOutcome`, kept crate-internal
    // (internal bucket-routing only). Dropping any bare re-export breaks
    // compilation immediately.
    const _: fn() = || {
        use crate::{
            DiagnosticSnapshot, format_summary, record_ineligible, record_morphed, record_panicked,
            record_quality_remesh,
        };
        let _: fn() = record_morphed;
        let _: fn(&crate::QualityVerdict) = record_quality_remesh;
        let _: fn(&crate::Reason) = record_ineligible;
        let _: fn(&str) = record_panicked;
        let _: fn(&DiagnosticSnapshot) -> String = format_summary;
        // `snapshot` via the module path, not a bare re-export (avoids the
        // collision with `stats::snapshot`).
        let _: fn() -> DiagnosticSnapshot = crate::diagnostics::snapshot;
    };

    // ── Step 1 (task 3153): pin the by-value `eligible` signature ────────────

    // Compile fence: fails to compile until `eligible` takes `BRep` by value.
    // Mirrors the boundary/laplacian/quality fence discipline above.
    #[allow(unused)]
    const _: fn() = || {
        let _fn_ref: fn(BRep, BRep) -> bool = eligible;
        let _ = _fn_ref;
    };

    // ── Step 3 (task 3153): pin the by-value `morph` signature ───────────────

    // Compile fence: fails to compile until `morph` takes `BRep` by value.
    // `old_mesh` and `options` remain `&`-bound (not `Copy`).
    #[allow(unused)]
    const _: fn() = || {
        let _fn_ref: fn(
            &reify_ir::VolumeMesh,
            BRep,
            BRep,
            &MorphOptions,
        ) -> Result<reify_ir::VolumeMesh, MorphFailure> = morph;
        let _ = _fn_ref;
    };

    // ── Steps 1-2 (task 3142): Stage-B regression guards ─────────────────────

    #[test]
    fn morph_returns_ineligible_bijection_failure_on_stage_b_count_mismatch() {
        // Regression guard: morph() must project Stage-B CountMismatch into
        // MorphFailure::Ineligible(Reason::BijectionFailure(_)), not SolverError
        // or panic.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        // old: 1 face with Cap(Top); new: 2 faces Cap(Top)+Cap(Bottom).
        // Stage A passes (identical graphs); Stage B rejects on CountMismatch.
        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));

        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 1));

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20), h(21)],
            edges: &[],
            vertices: &[],
        };

        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(
            matches!(
                result,
                Err(MorphFailure::Ineligible(Reason::BijectionFailure(_)))
            ),
            "Stage-B CountMismatch should project to MorphFailure::Ineligible(BijectionFailure), got: {result:?}"
        );
    }

    #[test]
    fn morph_returns_ineligible_naming_layer_error_on_stage_b_imported_geometry() {
        // Regression guard: morph() must project Stage-B NamingLayerError::Imported
        // into MorphFailure::Ineligible(Reason::NamingLayerError {..}), not into
        // Reason::BijectionFailure or SolverError.
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        // Empty tables + non-empty face slices → Stage B surfaces
        // BijectionFailure::NamingLayerError { kind: Face, reason: Imported },
        // which morph_eligible projects to top-level Reason::NamingLayerError.
        let old_table = TopologyAttributeTable::default();
        let new_table = TopologyAttributeTable::default();

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        let mesh = empty_mesh();
        let options = MorphOptions::default();
        let result = morph(&mesh, old_brep, new_brep, &options);
        assert!(
            matches!(
                result,
                Err(MorphFailure::Ineligible(Reason::NamingLayerError {
                    kind: SubShapeKind::Face,
                    reason: NamingLayerErrorReason::Imported,
                }))
            ),
            "Stage-B NamingLayerError::Imported should project to \
             MorphFailure::Ineligible(Reason::NamingLayerError), got: {result:?}"
        );
    }

    // ── Step-7 (task 4744 β): compose_morph success path ─────────────────────

    /// Tiny shape-preserving shift (in metres) applied by [`ShiftingKernel`] —
    /// small relative to the unit tet's bbox diagonal (≈1.73 m) so the morph is
    /// routed through the Laplacian quick-pass.
    const SHIFT_EPS: f32 = 1.0e-4;

    /// A stub `GeometryKernel` whose `closest_point_on_shape` shifts the queried
    /// point by `+SHIFT_EPS` in x — a tiny, shape-preserving displacement. The
    /// four required methods are unused stubs; `vertex_point` is left at the
    /// trait default (this fixture has no `OnVertex` attachments).
    struct ShiftingKernel;

    impl reify_ir::GeometryKernel for ShiftingKernel {
        fn execute(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            Err(reify_ir::GeometryError::OperationFailed("unused".into()))
        }
        fn query(&self, _q: &reify_ir::GeometryQuery) -> Result<Value, reify_ir::QueryError> {
            Err(reify_ir::QueryError::QueryFailed("unused".into()))
        }
        fn export(
            &self,
            _h: GeometryHandleId,
            _f: reify_ir::ExportFormat,
            _w: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            Err(reify_ir::ExportError::FormatError("unused".into()))
        }
        fn tessellate(
            &self,
            _h: GeometryHandleId,
            _t: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            Err(reify_ir::TessError::TessellationFailed("unused".into()))
        }
        fn closest_point_on_shape(
            &self,
            _handle: GeometryHandleId,
            point: [f64; 3],
        ) -> Result<[f64; 3], reify_ir::QueryError> {
            Ok([point[0] + SHIFT_EPS as f64, point[1], point[2]])
        }
    }

    /// A single well-shaped P1 tet at the unit corner — positive volume, good
    /// quality, so a tiny translation passes `quality_check`.
    fn single_tet_mesh() -> reify_ir::VolumeMesh {
        reify_ir::VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: reify_ir::ElementOrderTag::P1,
            normals: None,
            boundary: None,
        }
    }

    #[test]
    fn compose_morph_eligible_small_displacement_preserves_connectivity_and_records_morphed() {
        diagnostics::reset_for_test();

        // Eligible old/new BRep: identical graphs (Stage A passes) + one
        // matching Cap(Top) face each (Stage B yields face_to_face {h(10):h(20)}).
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        // All four nodes attached to the (matching) face → all prescribed, so the
        // Laplacian quick-pass deforms by the tiny +x shift with no free interior
        // nodes (a shape-preserving translation: connectivity-identical, quality-safe).
        let mut boundary = BoundaryAssociation::default();
        for n in 0..4u32 {
            boundary.associate(n, NodeAttachment::OnFace(h(10)));
        }

        let source = single_tet_mesh();
        let kernel = ShiftingKernel;
        let options = MorphOptions::default();

        let morphed = compose_morph(&source, &boundary, old_brep, new_brep, &kernel, &options)
            .expect("eligible small-displacement morph should succeed");

        // Connectivity preserved: identical tet_indices (the defining property of morph).
        assert_eq!(
            morphed.tet_indices, source.tet_indices,
            "morph must preserve connectivity (same tet_indices)"
        );
        // Deformed: vertices moved by the prescribed +x shift.
        assert_ne!(
            morphed.vertices, source.vertices,
            "morph must deform the vertices"
        );
        // Diagnostics: exactly one successful morph recorded.
        assert_eq!(
            diagnostics::snapshot().morphed,
            1,
            "compose_morph must record exactly one morphed outcome"
        );
    }

    // ── Step-9 (task 4744 β): compose_morph failure arms record one counter ───

    #[test]
    fn compose_morph_stage_b_count_mismatch_returns_ineligible_and_records_bijection_bucket() {
        diagnostics::reset_for_test();

        // old 1 face Cap(Top); new 2 faces Cap(Top)+Cap(Bottom) → Stage-B
        // CountMismatch → Ineligible(BijectionFailure).
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));
        new_table.record(h(21), attr(Role::Cap(CapKind::Bottom), 1));

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20), h(21)],
            edges: &[],
            vertices: &[],
        };

        let mut boundary = BoundaryAssociation::default();
        boundary.associate(0, NodeAttachment::OnFace(h(10)));

        let source = single_tet_mesh();
        let kernel = ShiftingKernel;
        let options = MorphOptions::default();
        let result = compose_morph(&source, &boundary, old_brep, new_brep, &kernel, &options);

        assert!(
            matches!(
                result,
                Err(MorphFailure::Ineligible(Reason::BijectionFailure(_)))
            ),
            "Stage-B count mismatch must be Ineligible(BijectionFailure), got: {result:?}"
        );
        // The matching diagnostic bucket is incremented exactly once.
        assert_eq!(
            diagnostics::snapshot().ineligible_bijection_failure, 1,
            "compose_morph must record the bijection-failure ineligible bucket"
        );
        assert_eq!(
            diagnostics::snapshot().morphed, 0,
            "an ineligible edit must not record a morph"
        );
    }

    #[test]
    fn compose_morph_quality_soft_fail_returns_quality_failure_and_records_soft_bucket() {
        diagnostics::reset_for_test();

        // Same eligible setup as the success path (one matching Cap(Top) face).
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));

        let old_brep = BRep {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &[h(10)],
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRep {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &[h(20)],
            edges: &[],
            vertices: &[],
        };

        let mut boundary = BoundaryAssociation::default();
        for n in 0..4u32 {
            boundary.associate(n, NodeAttachment::OnFace(h(10)));
        }

        let source = single_tet_mesh();
        let kernel = ShiftingKernel;
        // An impossibly high scaled-Jacobian floor (> the valid [-1, 1] range)
        // makes every morphed element trip the soft-fail metric → guaranteed
        // QualityVerdict::SoftFail, independent of the deformation magnitude.
        let options = MorphOptions {
            quality_floor_min_scaled_jacobian: 2.0,
            ..MorphOptions::default()
        };
        let result = compose_morph(&source, &boundary, old_brep, new_brep, &kernel, &options);

        assert!(
            matches!(result, Err(MorphFailure::QualitySoftFail(_))),
            "an impossibly-high SJ floor must force a quality soft-fail, got: {result:?}"
        );
        // The soft-fail remesh bucket is incremented exactly once.
        assert_eq!(
            diagnostics::snapshot().remeshed_quality_soft_fail, 1,
            "compose_morph must record the soft-fail remesh bucket"
        );
        assert_eq!(
            diagnostics::snapshot().morphed, 0,
            "a quality reject must not record a morph"
        );
    }

    // ── Step-17 (task 4744 β): register_morph_producer installs a working producer ─

    /// `register_morph_producer` installs a [`reify_eval::MorphProducer`] on the
    /// engine such that [`reify_eval::Engine::morph_producer`] is `Some`, and a
    /// trivial eligible [`reify_eval::MorphRequest`] routed through it returns
    /// `Ok` with connectivity preserved. This pins the cross-crate seam: the
    /// installed impl wraps `req.kernel` in a [`KernelProjector`] and drives the
    /// step-8 [`compose_morph`] composition.
    ///
    /// Mirrors the `compose_morph` success-path fixture (one matching `Cap(Top)`
    /// face each, the `ShiftingKernel`'s tiny `+x` shift, the single unit tet),
    /// but built as `reify_eval::BRepSnapshot`s — the `MorphRequest` snapshot
    /// type — rather than the `reify_mesh_morph::BRep` alias.
    ///
    /// No process-global counter assertion here (the counter increment is
    /// covered by the same-crate `compose_morph` success test and the reify-eval
    /// e2e at step-19): this test pins only the registration + dispatch wiring.
    #[test]
    fn register_morph_producer_installs_producer_that_morphs_eligible_request() {
        use reify_eval::{BRepSnapshot, Engine, MorphRequest, MorphResult};
        use reify_test_support::mocks::MockConstraintChecker;

        // Eligible old/new BRep: identical graphs (Stage A passes) + one matching
        // Cap(Top) face each (Stage B yields face_to_face {h(10):h(20)}).
        let id = ValueCellId::new("Part", "width");
        let old_graph = graph_with_cell(&id, Type::length());
        let new_graph = old_graph.clone();
        let mut values = ValueMap::new();
        values.insert(id, Value::length(0.05));

        let mut old_table = TopologyAttributeTable::default();
        old_table.record(h(10), attr(Role::Cap(CapKind::Top), 0));
        let mut new_table = TopologyAttributeTable::default();
        new_table.record(h(20), attr(Role::Cap(CapKind::Top), 0));

        let old_faces = [h(10)];
        let new_faces = [h(20)];
        let old_brep = BRepSnapshot {
            graph: &old_graph,
            values: &values,
            topology_attributes: &old_table,
            faces: &old_faces,
            edges: &[],
            vertices: &[],
        };
        let new_brep = BRepSnapshot {
            graph: &new_graph,
            values: &values,
            topology_attributes: &new_table,
            faces: &new_faces,
            edges: &[],
            vertices: &[],
        };

        // All four nodes on the matching face → all prescribed → the Laplacian
        // quick-pass applies the tiny +x shift (a shape-preserving translation).
        let mut boundary = BoundaryAssociation::default();
        for n in 0..4u32 {
            boundary.associate(n, NodeAttachment::OnFace(h(10)));
        }

        let source = single_tet_mesh();
        let kernel = ShiftingKernel;

        // The seam under test: register_morph_producer installs the producer.
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        crate::register_morph_producer(&mut engine);

        let producer = engine
            .morph_producer()
            .expect("register_morph_producer must install a MorphProducer");

        let request = MorphRequest {
            source: &source,
            boundary: &boundary,
            old_brep,
            new_brep,
            kernel: &kernel,
        };

        match producer.try_morph(request) {
            MorphResult::Ok(mesh) => {
                // Connectivity preserved: the producer wraps req.kernel in a
                // KernelProjector and runs compose_morph, which deforms vertices
                // in place and clones tet_indices by construction.
                assert_eq!(
                    mesh.tet_indices, source.tet_indices,
                    "morph must preserve connectivity (same tet_indices)"
                );
                assert_ne!(
                    mesh.vertices, source.vertices,
                    "morph must deform the vertices"
                );
            }
            other => {
                panic!("expected MorphResult::Ok for an eligible request, got: {other:?}")
            }
        }
    }
}
