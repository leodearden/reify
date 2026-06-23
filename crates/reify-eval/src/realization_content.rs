// Realization projection store and Engine::project_realization_read_handle.
//
// β: GeometryHandle-arg lowering → realization_inputs + memoized Engine
// projection store.  See `docs/prds/v0_6/realization-read-api.md` §9 (task β),
// contract §3.3/§3.4, decision D2 (lazy-at-lowering).
//
// γ (Mesh→tessellate, VolumeMesh→volume_mesh()) will REPLACE the Mesh/VolumeMesh
// arms in `project_realization_read_handle` with real kernel projection + store
// insert.  δ (Sdf/Voxel→densify) has already landed (task 4510).
//
// PRD §10 OQ-2 (eviction): the store is unbounded in v1; eviction is deferred
// to a future task.  Content is immutable once keyed (realization identity is
// content-addressed), so stale entries are unreachable rather than incorrect.

use std::collections::HashMap;
use std::sync::Arc;

use reify_core::{ContentHash, Diagnostic, KernelId, RealizationNodeId};
use reify_ir::{GeometryHandleId, GeometryKernel, ReprKind};

use crate::engine_compute::{RealizationReadHandle, RealizedContent};
use crate::graph::EvaluationGraph;

/// Sentinel tessellation tolerance for the Mesh projection arm (PRD §10 OQ-1).
///
/// A [`ReprKind::Mesh`] realization is *already discrete*, so the Mesh arm's
/// `tessellate(handle, tol)` call is an identity read-back — mesh kernels ignore
/// `tol` on an already-meshed handle.  This value is therefore **functionally
/// inert** on every path that reaches it today; threading the *achieved*
/// tolerance from the build tables is deferred until a production Mesh-repr
/// realization reaches a compute target (task 4091).
///
/// The literal deliberately mirrors `Engine::DEFAULT_TESSELLATION_TOLERANCE`
/// (the nominal project default, `engine_build.rs`) so that a `tol`-honouring
/// kernel would see that default rather than a degenerate `0.0`.  It is an
/// *un-enforced* copy, **not** a compiler-checked alias: that const is private
/// to the `engine_build` module, so collapsing the two onto one `pub(crate)`
/// constant requires widening its visibility there — out of this task's lock
/// scope, so the two are left separate for now.  Drift is harmless in the
/// meantime precisely because the value is inert on the identity read-back
/// path; if a production Mesh-repr flow ever honours `tol` (task 4091), fold
/// both onto the shared `pub(crate)` const at that point.
const MESH_PROJECTION_SENTINEL_TOL: f64 = 0.0001;

// ── Projection store ─────────────────────────────────────────────────────────

/// Memoization store for realized geometry content.
///
/// Keyed by `RealizationNodeId → ContentHash → RealizedContent` (two-level map)
/// so that two dispatches over the same realization identity but *different*
/// content hashes are never conflated (e.g. after a parameter edit).
///
/// The two-level structure lets [`get`](RealizationProjectionStore::get) borrow
/// `node_id` directly (no clone) for the outer lookup.
///
/// ## Arc-clone-on-get semantics
///
/// [`get`](RealizationProjectionStore::get) returns a *cloned* `RealizedContent`
/// (which is cheap: `RealizedContent` is a thin enum over `Arc<T>`).  The Arc
/// itself is shared, so both the store and the caller observe the same heap
/// allocation — `Arc::ptr_eq` on the inner pointer holds.
///
/// ## Eviction (PRD §10 OQ-2)
///
/// The store is unbounded in v1.  Because realization identity is
/// content-addressed, a stale entry (content_hash that is no longer current for
/// a given node) is simply never looked up again — it is unreachable, not
/// incorrect.  A future task may add an LRU cap.
#[allow(dead_code)] // consumed by project_realization_read_handle / step-4
pub(crate) struct RealizationProjectionStore {
    memo: HashMap<RealizationNodeId, HashMap<ContentHash, RealizedContent>>,
}

impl RealizationProjectionStore {
    pub(crate) fn new() -> Self {
        Self {
            memo: HashMap::new(),
        }
    }

    /// Look up content by `(node_id, content_hash)`.
    ///
    /// Borrows `node_id` for the outer lookup (no clone).  Returns a cloned
    /// `RealizedContent` (cheap Arc-clone) when present, `None` on a miss.
    /// Two calls with the same key return distinct enum values pointing to the
    /// *same* inner Arc allocation.
    #[allow(dead_code)] // used in step-2 tests; dead-code silenced until step-4 wires it
    pub(crate) fn get(
        &self,
        node_id: &RealizationNodeId,
        content_hash: ContentHash,
    ) -> Option<RealizedContent> {
        self.memo.get(node_id)?.get(&content_hash).cloned()
    }

    /// Insert (or overwrite) content for `(node_id, content_hash)`.
    ///
    /// Inserts are whole-value: a partial or cancelled dispatch must not call
    /// `insert` — only fully-completed projections are stored.  This ensures the
    /// store never contains partial content (cancellation-safety §3.2-4).
    #[allow(dead_code)] // used in step-4 onwards
    pub(crate) fn insert(
        &mut self,
        node_id: RealizationNodeId,
        content_hash: ContentHash,
        content: RealizedContent,
    ) {
        self.memo
            .entry(node_id)
            .or_default()
            .insert(content_hash, content);
    }
}

// ── Engine projection method ─────────────────────────────────────────────────

impl crate::Engine {
    /// Project a single realization node into a [`RealizationReadHandle`].
    ///
    /// Looks up `node_id` in `graph.realizations` to obtain its
    /// `content_hash` and `produced_repr`, then consults
    /// `self.realization_projection_store`:
    ///
    /// * **Store hit** — returns a handle carrying `Some(content)` and an
    ///   empty diagnostics vec.
    /// * **Store miss, BRep** — returns a handle carrying `None` and **no
    ///   diagnostic** (BRep is identity-only by design; PRD §4 D1 — a None
    ///   here is expected, not a failure).
    /// * **Store miss, VolumeMesh / Mesh** (γ) — resolves the producing kernel
    ///   from `geometry_kernels` (keyed by `produced_kernel`) and the handle
    ///   from `realization_handles`, projects through `volume_mesh` /
    ///   `tessellate`, memoizes, and returns `Some(content)` with no
    ///   diagnostic.  On any resolution miss (absent handle, `produced_kernel
    ///   == None`, kernel absent, or kernel `Err`) it degrades through
    ///   [`degrade_projection`] to `None` + one `Severity::Warning` (honest
    ///   degradation §3.2-5).
    /// * **Store miss, Sdf / Voxel** — densifies the live openvdb grid via
    ///   `GeometryKernel::densify_grid_to_sampled` (δ, task 4510); returns
    ///   `Some(RealizedContent::Sdf)` + stores the content on success, or
    ///   `None` + one warning on degradation (no kernel / chain-fail).
    /// * **Absent realization** (defensive; should not occur for a live
    ///   handle) — returns a handle with `content_hash = ContentHash(0)`,
    ///   `None` content, and one warning.
    ///
    /// The `realization_ref` is contributed to `realization_inputs`
    /// **unconditionally** (even when content degrades to `None`) — the ref
    /// drives cache-key identity (PRD §3.4 / §10 OQ-4).
    pub(crate) fn project_realization_read_handle(
        &mut self,
        node_id: &RealizationNodeId,
        graph: &EvaluationGraph,
    ) -> (RealizationReadHandle, Vec<Diagnostic>) {
        match graph.realizations.get(node_id) {
            None => {
                // Defensive: a live GeometryHandle arg should always have a
                // corresponding realization node — this arm guards against
                // graph inconsistency.
                let handle = RealizationReadHandle::new(node_id.clone(), ContentHash(0), None);
                let diag = Diagnostic::warning(format!(
                    "realization {node_id}: node absent from evaluation graph; \
                     handle carries no content"
                ));
                (handle, vec![diag])
            }
            Some(node_data) => {
                let content_hash = node_data.content_hash;
                let produced_repr = node_data.produced_repr;
                let produced_kernel = node_data.produced_kernel;

                // Store hit — share the Arc without re-projecting.
                if let Some(content) = self.realization_projection_store.get(node_id, content_hash)
                {
                    let handle =
                        RealizationReadHandle::new(node_id.clone(), content_hash, Some(content));
                    return (handle, vec![]);
                }

                // Store miss — project per repr.  The VolumeMesh / Mesh arms
                // resolve `(kernel, handle)` uniformly via
                // `resolve_realization_kernel` and degrade through
                // `degrade_projection` on any miss (γ); δ owns Sdf / Voxel;
                // BRep is identity-only (PRD §4 D1).
                match produced_repr {
                    ReprKind::BRep => {
                        // Identity-only by design (PRD §4 D1): no content, no
                        // diagnostic — NOT a degradation, even with a capable
                        // kernel present.
                        let handle =
                            RealizationReadHandle::new(node_id.clone(), content_hash, None);
                        (handle, vec![])
                    }
                    ReprKind::VolumeMesh => {
                        // γ arm 1 (PRD §3.3): resolve → project → wrap into an
                        // *owned* `RealizedContent` (releasing the immutable
                        // `self` borrow `resolve_realization_kernel` holds)
                        // before `memoize_projection` takes its `&mut self`
                        // store borrow.
                        let projected: Option<RealizedContent> = self
                            .resolve_realization_kernel(node_id, produced_kernel)
                            .and_then(|(kernel, handle_id)| kernel.volume_mesh(handle_id).ok())
                            .map(|vm| RealizedContent::VolumeMesh(Arc::new(vm)));

                        match projected {
                            Some(content) => {
                                self.memoize_projection(node_id, content_hash, content)
                            }
                            None => degrade_projection(node_id, content_hash, produced_repr),
                        }
                    }
                    ReprKind::Mesh => {
                        // γ arm 2 (PRD §3.3): a Mesh realization is already
                        // discrete, so `tessellate(handle, tol)` is an identity
                        // read-back (PRD §10 OQ-1) — kernels ignore `tol` on an
                        // already-meshed handle.  Same resolve → project → wrap →
                        // memoize shape as the VolumeMesh arm.
                        let projected: Option<RealizedContent> = self
                            .resolve_realization_kernel(node_id, produced_kernel)
                            .and_then(|(kernel, handle_id)| {
                                kernel.tessellate(handle_id, MESH_PROJECTION_SENTINEL_TOL).ok()
                            })
                            .map(|mesh| RealizedContent::SurfaceMesh(Arc::new(mesh)));

                        match projected {
                            Some(content) => {
                                self.memoize_projection(node_id, content_hash, content)
                            }
                            None => degrade_projection(node_id, content_hash, produced_repr),
                        }
                    }
                    ReprKind::Sdf | ReprKind::Voxel => {
                        // δ: densify the live openvdb grid into a SampledField
                        // via GeometryKernel::densify_grid_to_sampled (reification-
                        // read-api.md §3.3 arm 3; D4 reuse of 4421's machinery).
                        //
                        // Borrow sequencing (no conflict):
                        //   1. realization_handles — read only (copy handle_id)
                        //   2. geometry_kernels    — get_mut (exclusive for densify)
                        //   3. realization_projection_store — insert on success
                        let handle_id = self.realization_handles.get(node_id).copied();

                        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();
                        let kernel_opt = self.geometry_kernels.get_mut(openvdb_name);

                        match (handle_id, kernel_opt) {
                            (Some(hid), Some(kernel)) => {
                                match kernel.densify_grid_to_sampled(hid) {
                                    Ok(field) => {
                                        let content = RealizedContent::Sdf(Arc::new(field));
                                        self.realization_projection_store.insert(
                                            node_id.clone(),
                                            content_hash,
                                            content.clone(),
                                        );
                                        let handle = RealizationReadHandle::new(
                                            node_id.clone(),
                                            content_hash,
                                            Some(content),
                                        );
                                        (handle, vec![])
                                    }
                                    Err(e) => {
                                        let handle = RealizationReadHandle::new(
                                            node_id.clone(),
                                            content_hash,
                                            None,
                                        );
                                        let diag = Diagnostic::warning(format!(
                                            "realization {node_id}: {produced_repr:?} \
                                             densify failed: {e:?}; handle carries no content"
                                        ));
                                        (handle, vec![diag])
                                    }
                                }
                            }
                            _ => {
                                // No openvdb kernel registered (cfg(not(has_openvdb))
                                // stub build or missing handle) — honest degradation.
                                let handle =
                                    RealizationReadHandle::new(node_id.clone(), content_hash, None);
                                let diag = Diagnostic::warning(format!(
                                    "realization {node_id}: {produced_repr:?} densify \
                                     unavailable (no openvdb kernel); handle carries no content"
                                ));
                                (handle, vec![diag])
                            }
                        }
                    }
                }
            }
        }
    }

    /// Memoize a freshly-projected `content` and build its `Some(content)`
    /// handle — the shared success tail of the content-bearing projection arms
    /// (VolumeMesh / Mesh).
    ///
    /// Each arm computes an `Option<RealizedContent>` (resolve → kernel call →
    /// wrap), then matches: `Some` → this helper, `None` →
    /// [`degrade_projection`].  Because the arm produces an *owned*
    /// `RealizedContent` first, the immutable `self` borrow held by
    /// `resolve_realization_kernel` is released before this `&mut self` store
    /// borrow — keeping the disjoint-borrow sequencing in one place.
    ///
    /// The insert is whole-value (never partial — §3.2-4) and a successful
    /// projection emits zero diagnostics.
    fn memoize_projection(
        &mut self,
        node_id: &RealizationNodeId,
        content_hash: ContentHash,
        content: RealizedContent,
    ) -> (RealizationReadHandle, Vec<Diagnostic>) {
        self.realization_projection_store
            .insert(node_id.clone(), content_hash, content.clone());
        let handle = RealizationReadHandle::new(node_id.clone(), content_hash, Some(content));
        (handle, vec![])
    }

    /// Resolve the `(kernel, handle)` pair a content-bearing projection arm
    /// (VolumeMesh / Mesh) needs, or `None` when any link is missing.
    ///
    /// Returns `None` — funnelling the caller to [`degrade_projection`] — when:
    /// * `node_id` has no entry in `realization_handles` (handle never recorded),
    /// * `produced_kernel == None` (no terminal kernel recorded for the node), or
    /// * the named kernel is absent from `geometry_kernels`.
    ///
    /// The returned `&dyn GeometryKernel` borrows `self.geometry_kernels`, so the
    /// caller MUST consume it into an *owned* projection result (e.g.
    /// `.volume_mesh(handle).ok()`) — releasing this shared borrow — before
    /// taking the `&mut self.realization_projection_store` memoization borrow.
    pub(crate) fn resolve_realization_kernel(
        &self,
        node_id: &RealizationNodeId,
        produced_kernel: Option<KernelId>,
    ) -> Option<(&dyn GeometryKernel, GeometryHandleId)> {
        let handle = self.realization_handles.get(node_id).copied()?;
        // Try the canonical registry name first (e.g. "occt" when using
        // `Engine::with_registered_kernels`).  Fall back to the engine's
        // `default_kernel_name` for the `Engine::new(checker, Some(k))` path,
        // where the kernel is stored under `DEFAULT_KERNEL_NAME`
        // (`"__reify_eval_default_kernel"`) rather than `"occt"`.
        let kernel = produced_kernel
            .and_then(|k| self.geometry_kernels.get(k.as_registry_name()))
            .or_else(|| {
                let name = self
                    .default_kernel_name
                    .as_deref()
                    .unwrap_or(crate::Engine::DEFAULT_KERNEL_NAME);
                self.geometry_kernels.get(name)
            })?;
        Some((&**kernel, handle))
    }
}

/// Honest-degradation tail (PRD §3.2-5) shared by the content-bearing
/// projection arms (VolumeMesh / Mesh) and the not-yet-wired Sdf / Voxel arm.
///
/// Returns a handle carrying `None` content plus exactly one
/// `Severity::Warning` — it never panics and never partially memoizes.  Routing
/// every resolution miss (absent handle, `produced_kernel == None`, kernel
/// absent from `geometry_kernels`, or the kernel call returning `Err`) through
/// this single tail keeps the arms uniform.  BRep does **not** call this — it is
/// identity-only (None without a diagnostic; PRD §4 D1).
fn degrade_projection(
    node_id: &RealizationNodeId,
    content_hash: ContentHash,
    produced_repr: ReprKind,
) -> (RealizationReadHandle, Vec<Diagnostic>) {
    let handle = RealizationReadHandle::new(node_id.clone(), content_hash, None);
    let diag = Diagnostic::warning(format!(
        "realization {node_id}: {produced_repr:?} content projection unavailable \
         (producing kernel unresolved or returned no content); handle carries no \
         content"
    ));
    (handle, vec![diag])
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use reify_core::{ContentHash, KernelId, RealizationNodeId};
    use reify_ir::{ElementOrderTag, GeometryHandleId, GeometryKernel, Mesh, ReprKind, VolumeMesh};
    use reify_test_support::mocks::{
        FailingMockGeometryKernel, MockConstraintChecker, MockGeometryKernel,
    };

    use super::RealizationProjectionStore;
    use crate::Engine;
    use crate::engine_compute::RealizedContent;
    use crate::graph::{EvaluationGraph, RealizationNodeData};

    /// Build an `Engine` (via the test-only `with_test_kernels_and_registry`
    /// seam) with a single geometry kernel injected under `name`. The
    /// capability registry is empty — the realization-read projection resolves
    /// kernels from `geometry_kernels` keyed by `produced_kernel`, not from the
    /// dispatch registry.
    fn engine_with_kernel(name: &str, kernel: Box<dyn GeometryKernel>) -> Engine {
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(name.to_string(), kernel);
        Engine::with_test_kernels_and_registry(
            Box::new(MockConstraintChecker::new()),
            kernels,
            BTreeMap::new(),
            Some(name.to_string()),
        )
    }

    /// Canonical single-P1-tet [`VolumeMesh`] fixture for the content-arm tests.
    fn make_volume_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // v0
                1.0, 0.0, 0.0, // v1
                0.0, 1.0, 0.0, // v2
                0.0, 0.0, 1.0, // v3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    /// Seed a kernel-backed realization: insert the `RealizationNodeData` with
    /// `produced_kernel` set AND register the engine-side `realization_handles`
    /// entry, so the projection can resolve `(kernel, handle)`. This is the
    /// content-arm analogue of [`seed_realization`], which leaves
    /// `produced_kernel = None` and registers no handle (the degradation setup).
    fn seed_kernel_realization(
        engine: &mut Engine,
        graph: &mut EvaluationGraph,
        node_id: RealizationNodeId,
        content_hash: ContentHash,
        produced_repr: ReprKind,
        produced_kernel: KernelId,
        handle: GeometryHandleId,
    ) {
        let data = RealizationNodeData {
            id: node_id.clone(),
            operations: vec![],
            content_hash,
            produced_repr,
            geometry_cell: None,
            produced_kernel: Some(produced_kernel),
        };
        graph.realizations.insert(node_id.clone(), data);
        engine.realization_handles.insert(node_id, handle);
    }

    fn make_engine() -> Engine {
        Engine::new(Box::new(MockConstraintChecker::new()), None)
    }

    fn make_mesh() -> Arc<Mesh> {
        Arc::new(Mesh {
            vertices: vec![0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0_u32, 1, 2],
            normals: Some(vec![0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
        })
    }

    // ── RealizationProjectionStore tests ────────────────────────────────────

    // step-1 (RED): these tests fail to compile until step-2 implements
    // get/insert.

    #[test]
    fn store_hit_returns_arc_ptr_eq_content() {
        let mut store = RealizationProjectionStore::new();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-content-1");
        let mesh = make_mesh();
        let content = RealizedContent::SurfaceMesh(Arc::clone(&mesh));

        store.insert(r0.clone(), h, content);

        let retrieved = store.get(&r0, h).expect("should be a hit");
        match retrieved {
            RealizedContent::SurfaceMesh(got) => {
                assert!(
                    Arc::ptr_eq(&got, &mesh),
                    "get must return the same Arc (ptr_eq), not a deep copy"
                );
            }
            _ => panic!("expected SurfaceMesh"),
        }
    }

    #[test]
    fn store_miss_on_different_content_hash() {
        let mut store = RealizationProjectionStore::new();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("hash-A");
        let h2 = ContentHash::of_str("hash-B");
        let content = RealizedContent::SurfaceMesh(make_mesh());
        store.insert(r0.clone(), h, content);

        assert!(
            store.get(&r0, h2).is_none(),
            "different ContentHash must be a miss"
        );
    }

    #[test]
    fn store_miss_on_different_node_id() {
        let mut store = RealizationProjectionStore::new();
        let r0 = RealizationNodeId::new("E", 0);
        let r1 = RealizationNodeId::new("E", 1);
        let h = ContentHash::of_str("shared-hash");
        let content = RealizedContent::SurfaceMesh(make_mesh());
        store.insert(r0.clone(), h, content);

        assert!(
            store.get(&r1, h).is_none(),
            "different RealizationNodeId must be a miss"
        );
    }

    // ── Engine::project_realization_read_handle tests ───────────────────────

    // step-3 (RED): these tests fail to compile until step-4 implements
    // project_realization_read_handle.

    fn seed_realization(
        graph: &mut EvaluationGraph,
        node_id: RealizationNodeId,
        content_hash: ContentHash,
        produced_repr: ReprKind,
    ) {
        let data = RealizationNodeData {
            id: node_id.clone(),
            operations: vec![],
            content_hash,
            produced_repr,
            geometry_cell: None,
            produced_kernel: None,
        };
        graph.realizations.insert(node_id, data);
    }

    #[test]
    fn project_brep_returns_none_content_no_diagnostic() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("brep-content");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::BRep);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert_eq!(handle.node_id, r0);
        assert_eq!(handle.content_hash, h);
        assert!(handle.content().is_none(), "BRep must carry no content");
        assert!(diags.is_empty(), "BRep must emit no diagnostic");
    }

    #[test]
    fn project_mesh_returns_none_content_with_warning() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-h");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Mesh);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(handle.content().is_none());
        assert_eq!(diags.len(), 1, "Mesh repr must emit exactly one warning");
    }

    #[test]
    fn project_volume_mesh_returns_none_content_with_warning() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-h");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::VolumeMesh);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);
        assert!(handle.content().is_none());
        assert_eq!(
            diags.len(),
            1,
            "VolumeMesh repr must emit exactly one warning"
        );
    }

    // ── γ: VolumeMesh content arm (real kernel projection) ──────────────────

    /// A VolumeMesh realization with `produced_kernel` set and a content-
    /// capable kernel injected projects to `Some(RealizedContent::VolumeMesh)`
    /// carrying the kernel's mesh — `element_order` preserved, P1 connectivity
    /// (`tet_indices.len() % 4 == 0`), >0 tets — and emits no diagnostic.
    #[test]
    fn project_volume_mesh_returns_some_content() {
        let mock = MockGeometryKernel::new().with_volume_mesh_output(make_volume_mesh());
        let mut engine = engine_with_kernel("gmsh", Box::new(mock));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-content");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::VolumeMesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            diags.is_empty(),
            "successful VolumeMesh projection must emit no diagnostic; got {diags:?}"
        );
        match handle.content() {
            Some(RealizedContent::VolumeMesh(vm)) => {
                assert_eq!(
                    vm.element_order,
                    ElementOrderTag::P1,
                    "element_order must be preserved through projection"
                );
                assert_eq!(
                    vm.tet_indices.len() % 4,
                    0,
                    "P1 tet_indices must be a multiple of 4; got len {}",
                    vm.tet_indices.len()
                );
                assert!(
                    vm.tet_indices.len() / 4 > 0,
                    "projected mesh must carry at least one tet"
                );
            }
            other => panic!("expected Some(RealizedContent::VolumeMesh), got {other:?}"),
        }
    }

    /// A second projection of the same realization shares the memoized `Arc`
    /// (store hit) and emits no diagnostic.
    #[test]
    fn project_volume_mesh_memoizes() {
        let mock = MockGeometryKernel::new().with_volume_mesh_output(make_volume_mesh());
        let mut engine = engine_with_kernel("gmsh", Box::new(mock));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-content");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::VolumeMesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle1, _) = engine.project_realization_read_handle(&r0, &graph);
        let (handle2, diags2) = engine.project_realization_read_handle(&r0, &graph);

        assert!(diags2.is_empty(), "memoized hit must emit no diagnostic");
        let arc1 = match handle1.content() {
            Some(RealizedContent::VolumeMesh(a)) => Arc::clone(a),
            other => panic!("expected Some(VolumeMesh) on first call, got {other:?}"),
        };
        let arc2 = match handle2.content() {
            Some(RealizedContent::VolumeMesh(a)) => Arc::clone(a),
            other => panic!("expected Some(VolumeMesh) on second call, got {other:?}"),
        };
        assert!(
            Arc::ptr_eq(&arc1, &arc2),
            "the second projection must share the memoized Arc (store hit)"
        );
    }

    // ── γ: Mesh content arm (real kernel tessellate read-back) ──────────────

    /// A Mesh realization with `produced_kernel` set and a content-capable
    /// kernel injected projects to `Some(RealizedContent::SurfaceMesh)` carrying
    /// the kernel's tessellated mesh (identity read-back, PRD §10 OQ-1), with
    /// zero diagnostics. Supersedes `project_mesh_returns_none_content_with_warning`
    /// for the kernel-present case (that test keeps the no-kernel degradation path).
    #[test]
    fn project_mesh_returns_surface_mesh_via_tessellate() {
        let mut engine = engine_with_kernel("gmsh", Box::new(MockGeometryKernel::new()));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-content");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::Mesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            diags.is_empty(),
            "successful Mesh projection must emit no diagnostic; got {diags:?}"
        );
        match handle.content() {
            Some(RealizedContent::SurfaceMesh(mesh)) => {
                assert_eq!(
                    mesh.indices.len(),
                    3,
                    "projected mesh must carry the kernel's one-triangle tessellation"
                );
                assert_eq!(
                    mesh.vertices.len(),
                    9,
                    "projected mesh must carry 3 vertices (9 floats)"
                );
            }
            other => panic!("expected Some(RealizedContent::SurfaceMesh), got {other:?}"),
        }
    }

    /// A second projection of the same Mesh realization shares the memoized
    /// `Arc` (store hit) and emits no diagnostic.
    #[test]
    fn project_mesh_memoizes() {
        let mut engine = engine_with_kernel("gmsh", Box::new(MockGeometryKernel::new()));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-content");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::Mesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle1, _) = engine.project_realization_read_handle(&r0, &graph);
        let (handle2, diags2) = engine.project_realization_read_handle(&r0, &graph);

        assert!(diags2.is_empty(), "memoized hit must emit no diagnostic");
        let arc1 = match handle1.content() {
            Some(RealizedContent::SurfaceMesh(a)) => Arc::clone(a),
            other => panic!("expected Some(SurfaceMesh) on first call, got {other:?}"),
        };
        let arc2 = match handle2.content() {
            Some(RealizedContent::SurfaceMesh(a)) => Arc::clone(a),
            other => panic!("expected Some(SurfaceMesh) on second call, got {other:?}"),
        };
        assert!(
            Arc::ptr_eq(&arc1, &arc2),
            "the second projection must share the memoized Arc (store hit)"
        );
    }

    #[test]
    fn project_sdf_returns_none_content_with_warning() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("sdf-h");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Sdf);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);
        assert!(handle.content().is_none());
        assert_eq!(diags.len(), 1, "Sdf repr must emit exactly one warning");
    }

    #[test]
    fn project_voxel_returns_none_content_with_warning() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("voxel-h");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);
        assert!(handle.content().is_none());
        assert_eq!(diags.len(), 1, "Voxel repr must emit exactly one warning");
    }

    #[test]
    fn project_store_hit_returns_some_content_no_diagnostic() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-h");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Mesh);

        // Pre-seed the store with a RealizedContent.
        let mesh = make_mesh();
        let content = RealizedContent::SurfaceMesh(Arc::clone(&mesh));
        engine
            .realization_projection_store
            .insert(r0.clone(), h, content);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(diags.is_empty(), "store hit must emit no diagnostic");
        match handle.content() {
            Some(RealizedContent::SurfaceMesh(got)) => {
                assert!(
                    Arc::ptr_eq(got, &mesh),
                    "store hit must return the same Arc"
                );
            }
            _ => panic!("expected Some(SurfaceMesh)"),
        }
    }

    #[test]
    fn project_absent_node_returns_zero_hash_none_with_warning() {
        let mut engine = make_engine();
        let graph = EvaluationGraph::default(); // empty — no realizations
        let r0 = RealizationNodeId::new("absent", 99);

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert_eq!(handle.node_id, r0);
        assert_eq!(handle.content_hash, ContentHash(0));
        assert!(handle.content().is_none());
        assert_eq!(diags.len(), 1, "absent realization must emit one warning");
    }

    // ── γ: degradation matrix (honest None + no panic, §3.2-5) ──────────────
    //
    // The content arms (steps 6/8) already funnel *every* non-Ok resolution —
    // kernel Err, `produced_kernel == None`, kernel absent from
    // `geometry_kernels` — through the same `None => None + one warning`
    // degradation tail, and BRep is identity-only.  These tests pin that
    // matrix explicitly as regression locks (each is also a no-panic probe).

    /// (a) A VolumeMesh realization whose producing kernel's `volume_mesh`
    /// returns `Err` (FailingMockGeometryKernel inherits the trait default-Err)
    /// degrades to `None` + exactly one warning, no panic.
    #[test]
    fn project_volume_mesh_kernel_err_degrades_to_none_with_warning() {
        let mut engine = engine_with_kernel("gmsh", Box::new(FailingMockGeometryKernel));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-fail");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::VolumeMesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(handle.content().is_none(), "kernel Err must degrade to None content");
        assert_eq!(diags.len(), 1, "kernel Err must emit exactly one warning");
    }

    /// (a′) The Mesh-arm analogue of (a): a Mesh realization whose producing
    /// kernel's `tessellate` returns `Err` (FailingMockGeometryKernel — which
    /// is exactly what the real GmshKernel also does) degrades to `None` +
    /// exactly one warning, no panic.  The Mesh arm degrades via a *different*
    /// kernel call (`tessellate(handle, tol).ok()`) than the VolumeMesh arm
    /// (`volume_mesh(handle).ok()`), so this locks the `tessellate`-Err branch
    /// symmetric with `project_volume_mesh_kernel_err_degrades_to_none_with_warning`.
    #[test]
    fn project_mesh_kernel_err_degrades_to_none_with_warning() {
        let mut engine = engine_with_kernel("gmsh", Box::new(FailingMockGeometryKernel));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("mesh-fail");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::Mesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            handle.content().is_none(),
            "tessellate Err must degrade to None content"
        );
        assert_eq!(diags.len(), 1, "tessellate Err must emit exactly one warning");
    }

    /// (b) A content-bearing realization whose `produced_kernel` is `None`
    /// (no terminal kernel recorded) degrades to `None` + one warning even when
    /// a handle and a capable kernel are present — isolating `produced_kernel
    /// == None` as the sole degradation cause.
    #[test]
    fn project_none_produced_kernel_degrades_to_none_with_warning() {
        let mock = MockGeometryKernel::new().with_volume_mesh_output(make_volume_mesh());
        let mut engine = engine_with_kernel("gmsh", Box::new(mock));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-nokernel");
        // seed_realization leaves produced_kernel = None; insert a handle so the
        // ONLY missing link is the produced_kernel.
        seed_realization(&mut graph, r0.clone(), h, ReprKind::VolumeMesh);
        engine.realization_handles.insert(r0.clone(), GeometryHandleId(1));

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(handle.content().is_none(), "None produced_kernel must degrade to None");
        assert_eq!(diags.len(), 1, "None produced_kernel must emit exactly one warning");
    }

    /// (c) A realization whose `produced_kernel` names a kernel absent from
    /// `geometry_kernels` (here: an engine with an empty kernel map) degrades to
    /// `None` + one warning, no panic.
    #[test]
    fn project_absent_kernel_degrades_to_none_with_warning() {
        let mut engine = make_engine(); // empty geometry_kernels map
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("vmesh-absent-kernel");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::VolumeMesh,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(handle.content().is_none(), "absent kernel must degrade to None");
        assert_eq!(diags.len(), 1, "absent kernel must emit exactly one warning");
    }

    /// (d) A BRep realization is identity-only (PRD §4 D1): even with a capable
    /// kernel and a handle present, it returns `None` and NO diagnostic.
    #[test]
    fn project_brep_with_kernel_present_returns_none_no_diagnostic() {
        let mock = MockGeometryKernel::new().with_volume_mesh_output(make_volume_mesh());
        let mut engine = engine_with_kernel("gmsh", Box::new(mock));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("E", 0);
        let h = ContentHash::of_str("brep-with-kernel");
        seed_kernel_realization(
            &mut engine,
            &mut graph,
            r0.clone(),
            h,
            ReprKind::BRep,
            KernelId::Gmsh,
            GeometryHandleId(1),
        );

        let (handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(handle.content().is_none(), "BRep is identity-only: no content");
        assert!(
            diags.is_empty(),
            "BRep must emit NO diagnostic even with a kernel present"
        );
    }

    // ── δ Sdf/Voxel densify projection tests ────────────────────────────────
    //
    // step-7 RED: success + memoization fail (arm returns None+warning today).
    // Degradation tests should already PASS (None+1 diag from both old & new arm).

    /// Closed box mesh (±1.0 mm on each axis, 12 triangles).
    /// Same fixture as `ingest_mesh_densify_tests::box_2mm`.
    #[cfg(has_openvdb)]
    fn box_2mm() -> reify_ir::Mesh {
        let v: Vec<f32> = vec![
            -1.0, -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, -1.0, -1.0, 1.0,
            1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
        ];
        #[rustfmt::skip]
        let i: Vec<u32> = vec![
            0,2,1, 0,3,2,  4,5,6, 4,6,7,  0,1,5, 0,5,4,
            2,3,7, 2,7,6,  0,4,7, 0,7,3,  1,2,6, 1,6,5,
        ];
        reify_ir::Mesh {
            vertices: v,
            indices: i,
            normals: None,
        }
    }

    /// δ SUCCESS: `project_realization_read_handle` on a Voxel node backed by
    /// a live ingested box mesh returns `Some(RealizedContent::Sdf(...))` with
    /// structural integrity checks.
    ///
    /// RED: current arm returns `None + 1 warning`; step-8 replaces it with
    /// the densify projection.
    ///
    /// Uses make_engine() + manually inserts OpenVdbKernel to avoid invoking
    /// the `unreachable!()` factories of the cfg(test) synthetic kernels that
    /// `Engine::with_registered_kernels` would also instantiate.
    #[cfg(has_openvdb)]
    #[test]
    fn project_voxel_with_openvdb_kernel_returns_sampled_field() {
        use reify_ir::{GeometryKernel, SampledGridKind};
        use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

        // Use make_engine() to avoid hitting cfg(test) synthetic kernel
        // factories (they are unreachable!()), then insert the real openvdb
        // kernel directly.
        let mut engine = make_engine();
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();

        // Ingest the closed box into the live openvdb kernel instance.
        let mesh = box_2mm();
        let mut openvdb = OpenVdbKernel::new();
        let handle = openvdb
            .ingest_mesh(&mesh)
            .expect("ingest_mesh must succeed for a valid closed box");
        engine
            .geometry_kernels
            .insert(openvdb_name.to_string(), Box::new(openvdb));

        // Seed realization graph + handles.
        let r0 = RealizationNodeId::new("voxel-delta-test", 0);
        let h = ContentHash::of_str("box-voxel-hash");
        let mut graph = EvaluationGraph::default();
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);
        engine.realization_handles.insert(r0.clone(), handle.id);

        let (read_handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        // Success path: Some(SampledField) + no diagnostic.
        assert!(
            diags.is_empty(),
            "Voxel/openvdb success path must emit no diagnostic; got: {diags:?}"
        );
        let field = read_handle
            .sdf()
            .expect("Voxel projection must return Some(SampledField) via sdf()");

        // Structural checks (realization-read-api.md §3.3 δ; no numeric tolerance).
        assert_eq!(
            field.kind,
            SampledGridKind::Regular3D,
            "kind must be Regular3D"
        );
        assert_eq!(
            field.spacing.len(),
            3,
            "spacing must have 3 entries for Regular3D"
        );
        for (i, &s) in field.spacing.iter().enumerate() {
            assert!(
                s > 0.0 && s.is_finite(),
                "spacing[{i}] = {s} must be positive and finite"
            );
        }
        // Bounds must cover the box extents (±1.0 mm on each axis).
        for i in 0..3 {
            assert!(
                field.bounds_min[i] <= -1.0,
                "bounds_min[{i}] = {} must be ≤ -1.0 (box half-extent)",
                field.bounds_min[i]
            );
            assert!(
                field.bounds_max[i] >= 1.0,
                "bounds_max[{i}] = {} must be ≥ 1.0 (box half-extent)",
                field.bounds_max[i]
            );
        }
        // Data must be non-empty and finite.
        assert!(
            !field.data.is_empty(),
            "densified field data must not be empty"
        );
        assert!(
            field.data.iter().all(|v| v.is_finite()),
            "all SampledField data values must be finite"
        );
        // CPU-sampleable: interpolate at the box centre (0,0,0) → finite value.
        let phi = reify_expr::interp::interpolate_3d(
            reify_expr::interp::InterpolationMethod::Linear,
            &field.axis_grids[0],
            &field.axis_grids[1],
            &field.axis_grids[2],
            &field.data,
            (0.0, 0.0, 0.0),
        )
        .value;
        assert!(phi.is_finite(), "SDF at (0,0,0) must be finite; got {phi}");
        assert!(
            phi < 0.0,
            "SDF at box centre must be negative (interior); got {phi}"
        );
    }

    /// δ MEMOIZATION: two projections of the same (node, content_hash) return
    /// `Arc::ptr_eq` content — the second call is a store hit.
    ///
    /// RED: current arm returns `None + 1 warning` on every call (no insert).
    ///
    /// Uses make_engine() + manually inserts OpenVdbKernel (same rationale as
    /// `project_voxel_with_openvdb_kernel_returns_sampled_field`).
    #[cfg(has_openvdb)]
    #[test]
    fn project_voxel_memoized_returns_ptr_eq_arc() {
        use reify_ir::GeometryKernel;
        use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

        let mut engine = make_engine();
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();

        let mesh = box_2mm();
        let mut openvdb = OpenVdbKernel::new();
        let handle = openvdb
            .ingest_mesh(&mesh)
            .expect("ingest_mesh must succeed");
        engine
            .geometry_kernels
            .insert(openvdb_name.to_string(), Box::new(openvdb));

        let r0 = RealizationNodeId::new("memo-test", 0);
        let h = ContentHash::of_str("memo-hash");
        let mut graph = EvaluationGraph::default();
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);
        engine.realization_handles.insert(r0.clone(), handle.id);

        let (h1, diags1) = engine.project_realization_read_handle(&r0, &graph);
        assert!(
            diags1.is_empty(),
            "first projection must emit no diagnostic"
        );
        let arc1 = match h1.content() {
            Some(RealizedContent::Sdf(a)) => std::sync::Arc::clone(a),
            other => panic!("first projection must return Some(Sdf); got {other:?}"),
        };

        let (h2, diags2) = engine.project_realization_read_handle(&r0, &graph);
        assert!(
            diags2.is_empty(),
            "second projection (store hit) must emit no diagnostic"
        );
        let arc2 = match h2.content() {
            Some(RealizedContent::Sdf(a)) => std::sync::Arc::clone(a),
            other => panic!("second projection must return Some(Sdf); got {other:?}"),
        };

        assert!(
            std::sync::Arc::ptr_eq(&arc1, &arc2),
            "two projections of the same (node, content_hash) must return Arc::ptr_eq \
             content (store hit path)"
        );
    }

    /// δ DEGRADATION (no-kernel engine): when no openvdb kernel is registered
    /// (β's `make_engine`), projecting a Voxel node returns `None + 1 diag`.
    #[test]
    fn project_voxel_no_openvdb_kernel_returns_none_with_one_diagnostic() {
        let mut engine = make_engine(); // no geometry kernel at all
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("voxel-degrade", 0);
        let h = ContentHash::of_str("voxel-degrade-hash");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);
        // No entry in realization_handles — kernel lookup will fail anyway.

        let (read_handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            read_handle.sdf().is_none(),
            "Voxel + no openvdb kernel must return None content"
        );
        assert_eq!(
            diags.len(),
            1,
            "Voxel + no openvdb kernel must emit exactly one diagnostic; got {diags:?}"
        );
    }

    /// δ DEGRADATION: Sdf node with no openvdb kernel returns `None + 1 diag`.
    #[test]
    fn project_sdf_no_openvdb_kernel_returns_none_with_one_diagnostic() {
        let mut engine = make_engine();
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("sdf-degrade", 0);
        let h = ContentHash::of_str("sdf-degrade-hash");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Sdf);

        let (read_handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            read_handle.sdf().is_none(),
            "Sdf + no openvdb kernel must return None content"
        );
        assert_eq!(
            diags.len(),
            1,
            "Sdf + no openvdb kernel must emit exactly one diagnostic; got {diags:?}"
        );
    }

    /// Minimal `GeometryKernel` stub that inherits the default
    /// `densify_grid_to_sampled` (returns `Err(QueryFailed)`).
    /// All other required methods are unreachable in the densify test.
    struct DensifyAlwaysFailKernel;
    impl reify_ir::GeometryKernel for DensifyAlwaysFailKernel {
        fn execute(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        fn query(
            &self,
            _q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        fn export(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _format: reify_ir::ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            unimplemented!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        // densify_grid_to_sampled: inherits default →
        // Err(QueryError::QueryFailed("densify_grid_to_sampled not supported by this kernel"))
    }

    /// δ DEGRADATION (densify Err): kernel registered but `densify_grid_to_sampled`
    /// returns `Err(QueryFailed)` — arm must produce `None + 1 diag`, not panic.
    #[test]
    fn project_voxel_densify_err_returns_none_with_one_diagnostic() {
        let mut engine = make_engine();
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();

        // Register a stub under the openvdb name with no densify override; its
        // default densify_grid_to_sampled always returns Err(QueryFailed).
        engine
            .geometry_kernels
            .insert(openvdb_name.to_string(), Box::new(DensifyAlwaysFailKernel));

        let r0 = RealizationNodeId::new("densify-err", 0);
        let h = ContentHash::of_str("densify-err-hash");
        // A fake handle id so (Some(hid), Some(kernel)) matches and we reach
        // the densify call; DensifyAlwaysFailKernel doesn't hold any handles,
        // but its densify returns Err before accessing them.
        let fake_id = reify_ir::GeometryHandleId(99);
        let mut graph = EvaluationGraph::default();
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);
        engine.realization_handles.insert(r0.clone(), fake_id);

        let (read_handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            read_handle.sdf().is_none(),
            "densify Err arm must return None content; got {:?}",
            read_handle.content()
        );
        assert_eq!(
            diags.len(),
            1,
            "densify Err arm must emit exactly one diagnostic; got {diags:?}"
        );
    }

    /// δ DEGRADATION (cfg(not(has_openvdb)) stub build): the Voxel/Sdf arm
    /// must return `None + 1 diag` even when `with_registered_kernels` is used
    /// (openvdb is not registered in stub builds so the kernel lookup fails).
    #[cfg(not(has_openvdb))]
    #[test]
    fn project_voxel_stub_build_returns_none_no_fabricated_field() {
        use reify_test_support::mocks::MockConstraintChecker;

        let mut engine = Engine::with_registered_kernels(Box::new(MockConstraintChecker::new()));
        let mut graph = EvaluationGraph::default();
        let r0 = RealizationNodeId::new("stub-voxel", 0);
        let h = ContentHash::of_str("stub-hash");
        seed_realization(&mut graph, r0.clone(), h, ReprKind::Voxel);

        let (read_handle, diags) = engine.project_realization_read_handle(&r0, &graph);

        assert!(
            read_handle.sdf().is_none(),
            "cfg(not(has_openvdb)) Voxel projection must return None — no fabricated field"
        );
        assert_eq!(
            diags.len(),
            1,
            "cfg(not(has_openvdb)) Voxel projection must emit exactly 1 diagnostic; got {diags:?}"
        );
    }
}
