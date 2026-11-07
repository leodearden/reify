// PRD §7.1 γ: realize_solid_sdf — BRep→Mesh→Voxel→SampledField post-build recipe.
//
// Turns an already-realized BRep solid into a CPU-resident queryable SDF by
// demanding a Voxel realization and driving β's BRep→Mesh→Voxel chain, then
// densifying via α.  Returns `None` on every degradation path (D5: the caller ζ
// maps `None` → self-describing `Undef` + diagnostic + `Indeterminate`, never a
// fabricated number).
//
// PRD §4 D1 — post-build direct recipe: γ does NOT re-enter the dispatcher BFS
// / realization loop and does NOT modify `demanded_reprs_for_template`.  The
// subject is already realized; γ runs the same recipe β's executor runs
// (`execute_realization_ops` Voxelize stage) directly.

/// The coarsest chordal tolerance the BRep→Mesh stage of the recipe uses, in
/// the model's own units (SI metres): the fixed value the stage used before
/// its chord followed the voxel grid (task 7502). It stays a ceiling so that a
/// coarse grid never makes any caller's tessellation coarser than it was.
const MAX_TESSELLATION_CHORD_TOLERANCE: f64 = 0.0001;

/// The chord tolerance the BRep→Mesh stage may spend per unit of voxel size.
///
/// The SDF samples are exact distances to the mesh facets, and distance is
/// 1-Lipschitz in the Hausdorff distance between surfaces, so a chord
/// deviation ε moves the zero level set by at most ε. A lattice of step h
/// locates a surface to within h/2, so ε ≤ h/2 keeps the tessellation error at
/// or below the grid's own quantisation error.
const CHORD_TOLERANCE_PER_VOXEL: f64 = 0.5;

/// Whether the caller chose `resolution`, as opposed to getting the recipe's
/// default, [`reify_ir::VoxelResolution::HonestFloor`].
///
/// This is all the recipe can see of who a failure belongs to. Under a chosen
/// resolution the request itself may be what could not be served, so the
/// failure is worth a diagnostic; the default carries no choice to blame and
/// fails as quietly as the environment failures it cannot be told apart from.
fn is_request_driven(resolution: reify_ir::VoxelResolution) -> bool {
    !matches!(resolution, reify_ir::VoxelResolution::HonestFloor)
}

/// Tessellate `brep` finely enough for the voxel grid `resolution` asks for.
///
/// The first tessellation is at [`MAX_TESSELLATION_CHORD_TOLERANCE`]. The voxel
/// size the kernel will build is then read off that mesh (the honest-floor size
/// is derived from it), and the BRep is tessellated again at that size times
/// [`CHORD_TOLERANCE_PER_VOXEL`] when this is finer than the ceiling.
///
/// A request the kernel cannot serve has no voxel size to derive a chord from,
/// so its mesh stays unrefined and the voxelizer remains the one place that
/// rejects it. A refinement that fails yields `None`, never the coarse mesh:
/// that mesh is the facet-resolving input this function exists to avoid. When
/// the resolution is [request-driven](is_request_driven) the refusal is also
/// logged, because the fine chord is the request's doing.
fn tessellate_for_voxel_grid(
    tessellator: &dyn reify_ir::GeometryKernel,
    brep: reify_ir::GeometryHandleId,
    resolution: reify_ir::VoxelResolution,
) -> Option<reify_ir::Mesh> {
    let coarse = tessellator
        .tessellate(brep, MAX_TESSELLATION_CHORD_TOLERANCE)
        .ok()?;
    let Ok(grid) = reify_kernel_openvdb::MeshToVoxelOptions::for_resolution(&coarse, resolution)
    else {
        return Some(coarse);
    };
    let chord = grid.voxel_size * CHORD_TOLERANCE_PER_VOXEL;
    if chord < MAX_TESSELLATION_CHORD_TOLERANCE {
        tessellator
            .tessellate(brep, chord)
            .inspect_err(|e| {
                if is_request_driven(resolution) {
                    tracing::warn!(
                        target: "reify_eval::realize_solid_sdf",
                        ?resolution,
                        chord,
                        error = %e,
                        "tessellation at the chord derived from the requested resolution failed"
                    );
                }
            })
            .ok()
    } else {
        Some(coarse)
    }
}

impl crate::Engine {
    /// Turn an already-realized BRep solid into a CPU-resident queryable SDF.
    ///
    /// Demands a Voxel realization by driving β's BRep→Mesh→Voxel chain, then
    /// densifying via α.  Returns `None` on every degradation path (PRD §4 D5):
    /// the caller ζ maps `None` → self-describing `Undef` + diagnostic +
    /// `Indeterminate`, never a fabricated number.
    ///
    /// PRD §4 D1 — post-build direct recipe: γ does NOT re-enter the dispatcher
    /// BFS / realization loop and does NOT modify `demanded_reprs_for_template`.
    /// The subject is already realized; γ runs the same recipe β's executor runs
    /// (`execute_realization_ops` Voxelize stage) directly.
    ///
    /// Resolution policy: [`reify_ir::VoxelResolution::HonestFloor`] — the
    /// bounding-box-derived default the recipe has always used. A caller that
    /// knows its thinnest feature should use [`Self::realize_solid_sdf_at`]
    /// with [`reify_ir::VoxelResolution::MinFeature`] instead (task 6560); at
    /// the honest floor a 1 mm feature in a 100 mm part is entirely sub-voxel.
    /// That feature is passed as a model-space length in the mesh's own units —
    /// see [`reify_ir::VoxelResolution`]'s "Units".
    ///
    /// Degradation paths → `None`: see [`Self::realize_solid_sdf_at`], of which
    /// this is the `HonestFloor` special case. There is exactly one body, so
    /// the two entry points cannot drift.
    pub(crate) fn realize_solid_sdf(
        &mut self,
        subject: reify_ir::value::GeometryHandleRef,
    ) -> Option<reify_ir::SampledField> {
        self.realize_solid_sdf_at(subject, reify_ir::VoxelResolution::HonestFloor)
    }

    /// Turn an already-realized BRep solid into a CPU-resident queryable SDF at
    /// a caller-requested [`reify_ir::VoxelResolution`] (task 6560 — the
    /// v0.4-shells `BRep→Voxel` resolution seam).
    ///
    /// This is the sole implementation; [`Self::realize_solid_sdf`] is its
    /// [`reify_ir::VoxelResolution::HonestFloor`] special case.
    ///
    /// PRD §4 D1 — post-build direct recipe: γ does NOT re-enter the dispatcher
    /// BFS / realization loop and does NOT modify `demanded_reprs_for_template`.
    /// The subject is already realized; γ runs the same recipe β's executor runs
    /// (`execute_realization_ops` Voxelize stage) directly. The resolution is
    /// threaded through THIS direct recipe rather than through that executor
    /// stage because the executor keys its intermediate cache with `NO_OPTIONS`;
    /// making it options-carrying is the ESC-3433-117 aliasing hazard and needs
    /// its own cache-key work.
    ///
    /// Degradation paths → `None` (PRD §4 D5 — the caller ζ maps `None` →
    /// self-describing `Undef` + diagnostic + `Indeterminate`, never a
    /// fabricated number):
    ///  1. `subject.realization_ref` absent from `realization_handles` AND
    ///     `subject.kernel_handle == GeometryHandleId::INVALID` (resolution fails).
    ///  2. No `default_kernel_name` configured (no source kernel to tessellate).
    ///  3. No kernel registered under `openvdb_kernel_name()` — absent in stub
    ///     builds where the `cfg(any(has_openvdb, feature="stub_register"))` gate
    ///     on `inventory::submit!` is not satisfied.  This is the D5 mechanism.
    ///  4. Either tessellation (the fixed-chord pass or the refinement),
    ///     `ingest_mesh_at_resolution`, or `densify_grid_to_sampled` returns
    ///     `Err` (chain failure).
    ///  5. `resolution` is invalid (non-finite / non-positive), coarser than the
    ///     body's thinnest bounding-box extent, or implies a grid beyond the
    ///     kernel's dense-grid budget — the kernel rejects it and the `Err`
    ///     degrades here exactly like any other chain failure.
    ///
    /// Paths 1-4 are ENVIRONMENT failures, where an anonymous `None` is the
    /// honest answer. Path 5 is a CALLER failure — the request itself could not
    /// be served — so it is logged at `warn` (target
    /// `reify_eval::realize_solid_sdf`) before being dropped, keeping a rejected
    /// request distinguishable from a build with no OpenVDB in it. The two are
    /// separated by the REQUEST, which is all this layer can see: a
    /// [`reify_ir::VoxelResolution::HonestFloor`] failure carries no caller
    /// choice to blame, so its `Err` degrades silently like paths 1-4, while a
    /// request-driven one logs the error it actually got without presuming
    /// which of the two it was. That includes a refused refinement tessellation
    /// (path 4): under a request-driven resolution its chord is the request's
    /// doing.
    ///
    /// # Chord tolerance
    ///
    /// The BRep→Mesh chord follows the voxel size that
    /// [`reify_kernel_openvdb::MeshToVoxelOptions::for_resolution`] resolves
    /// `resolution` to, the policy the voxelizer itself applies, so a fine request
    /// samples geometry rather than tessellation facets. Basis and cap:
    /// [`CHORD_TOLERANCE_PER_VOXEL`], [`MAX_TESSELLATION_CHORD_TOLERANCE`].
    // The resolution-carrying entry point is reached today only through
    // `realize_solid_sdf`'s `HonestFloor` delegation; reify-shell-extract T1
    // (structural-analysis-shells.md, "Decomposition plan" → the
    // voxel-medial mid-surface extraction task) is the consumer that will
    // request a thickness-scale resolution. Deliberately NOT `#[allow(dead_code)]`:
    // the lint cannot fire while `realize_solid_sdf` has callers, and the
    // attribute would silently absorb the signal if they ever went away.
    pub(crate) fn realize_solid_sdf_at(
        &mut self,
        subject: reify_ir::value::GeometryHandleRef,
        resolution: reify_ir::VoxelResolution,
    ) -> Option<reify_ir::SampledField> {
        // ── 1. Resolve the BRep handle ──────────────────────────────────────
        // Prefer the realization_handles table (set by post_process_geometry_handle_cells
        // during build); fall back to subject.kernel_handle when it is not INVALID
        // (mirrors the `resolve_handle` pattern in engine_constraints.rs).
        let brep_id = self
            .realization_handles
            .get(&subject.realization_ref)
            .copied()
            .or_else(|| {
                let kh = subject.kernel_handle?;
                (kh != reify_ir::GeometryHandleId::INVALID).then_some(kh)
            });
        let brep_id = brep_id?;

        // ── 2. Source kernel (for tessellation) ──────────────────────────────
        // Clone to release the immutable borrow on `self` before the `get_mut`
        // calls below (mirrors the source-kernel selection pattern in measure_dfm_rules.rs).
        let source = self.default_kernel_name.clone()?;

        // ── 3. OpenVDB presence guard ─────────────────────────────────────────
        // Absence means a stub build omitted the registration (D5) or the kernel
        // was never loaded — honest None, no panic, no fabricated number.
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();
        let ovdb_present = self.geometry_kernels.get(openvdb_name);
        ovdb_present?;

        // ── 4. BRep→Mesh→Voxel→SampledField recipe (PRD §7.1 γ, §4 D1) ─────
        // γ is the first production caller to reference ReprKind::Voxel as a
        // *demanded* repr — anti-orphan production signal for the Voxel variant.
        tracing::debug!(
            target: "reify_eval::realize_solid_sdf",
            demanded = ?reify_ir::ReprKind::Voxel,
            ?brep_id,
            ?resolution,
            "realize_solid_sdf_at: demanding Voxel realization of subject solid"
        );

        // Tessellate BRep→Mesh at a chord that follows the requested grid.
        let mesh = tessellate_for_voxel_grid(
            self.geometry_kernels.get(&source)?.as_ref(),
            brep_id,
            resolution,
        )?;

        // Ingest Mesh→Voxel at the requested resolution.
        //
        // A request-driven resolution is a caller CHOICE, so an `Err` under one
        // is worth a diagnostic: it may be the request itself that could not be
        // served (malformed, too coarse, over budget), and the kernel's message
        // names the offending value. Under `HonestFloor` there is no caller
        // choice to report — the only possible causes are the same environment
        // failures the guards above degrade anonymously for — so that arm stays
        // quiet rather than blaming a resolution nobody asked for. The message
        // names the error and stops there; this layer cannot tell a rejected
        // request from a raw FFI failure and must not claim to.
        let voxel = self
            .geometry_kernels
            .get_mut(openvdb_name)?
            .ingest_mesh_at_resolution(&mesh, resolution)
            .inspect_err(|e| {
                if is_request_driven(resolution) {
                    tracing::warn!(
                        target: "reify_eval::realize_solid_sdf",
                        ?resolution,
                        error = %e,
                        "ingest_mesh_at_resolution failed for the requested resolution"
                    );
                }
            })
            .ok()?;

        // Densify Voxel→SampledField
        let field = self
            .geometry_kernels
            .get_mut(openvdb_name)?
            .densify_grid_to_sampled(voxel.id)
            .ok()?;

        Some(field)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use reify_core::RealizationNodeId;
    use reify_ir::GeometryHandleId;
    use reify_ir::value::GeometryHandleRef;
    use reify_test_support::mocks::{
        FailingMockGeometryKernel, MockConstraintChecker, MockGeometryKernel,
    };

    use crate::Engine;

    fn make_engine() -> Engine {
        Engine::new(crate::SolverPosture::SolverFree, Box::new(MockConstraintChecker::new()), None)
    }

    // ── step-1 RED: degradation contract, cfg-independent + stub ─────────────
    //
    // All three tests below reference `engine.realize_solid_sdf(subject)` which
    // does NOT exist yet — they compile-fail (RED) until step-2 wires the guards.

    /// (a) Subject + source kernel present, but no openvdb kernel registered
    /// → realize_solid_sdf must return None (absent-openvdb guard).
    #[test]
    fn realize_solid_sdf_no_openvdb_kernel_returns_none() {
        let mut engine = make_engine();

        // Insert a source kernel under "occt" (its tessellate returns a minimal
        // mesh, which is sufficient — the openvdb guard fires before tessellate).
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        engine.default_kernel_name = Some("occt".to_string());

        // Seed a resolved subject.
        let r0 = RealizationNodeId::new("solid-gamma-1", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(7));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(7)),
        };

        // No kernel under openvdb_kernel_name() → must return None.
        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "realize_solid_sdf with no openvdb kernel must return None"
        );
    }

    /// (b) Subject whose realization_ref is absent from realization_handles
    /// AND kernel_handle == GeometryHandleId::INVALID → resolution fails → None.
    #[test]
    fn realize_solid_sdf_unresolvable_subject_returns_none() {
        let mut engine = make_engine();

        // Set up a source kernel so the guard can't fire early on that.
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        engine.default_kernel_name = Some("occt".to_string());

        // Subject with no entry in realization_handles and INVALID kernel_handle.
        let r_absent = RealizationNodeId::new("absent-solid", 99);
        let subject = GeometryHandleRef {
            realization_ref: r_absent,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId::INVALID),
        };

        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "realize_solid_sdf with unresolvable subject must return None"
        );
    }

    // ── step-3 RED: success path + densify-Err degradation ──────────────────
    //
    // (a) is RED under step-2: the placeholder returns None, but (a) expects Some.
    // (b) is accidentally GREEN under step-2: the placeholder returns None and (b)
    //     expects None (the densify-Err path).  Step-4 fixes (a) by wiring the
    //     full tessellate→ingest→densify recipe.

    /// Closed box mesh (±1.0 mm on each axis, 12 triangles). Defined without a
    /// `cfg(has_openvdb)` gate so TessellatingBoxKernel can be used in both
    /// the cfg(has_openvdb) success test and the cfg-independent densify-Err test.
    fn box_2mm() -> reify_ir::Mesh {
        reify_kernel_openvdb::test_fixtures::box_mesh(1.0, 1.0, 1.0)
    }

    /// The `GeometryKernel` methods no fake in this module may be asked for:
    /// the recipe only tessellates and ingests, so reaching `execute`, `query`
    /// or `export` is a bug that must fail loudly. Invoke it inside the fake's
    /// `impl GeometryKernel` block.
    macro_rules! unreachable_kernel_stubs {
        () => {
            fn execute(
                &mut self,
                _op: &reify_ir::GeometryOp,
            ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
                unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
            }
            fn query(
                &self,
                _q: &reify_ir::GeometryQuery,
            ) -> Result<reify_ir::Value, reify_ir::QueryError> {
                unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
            }
            fn export(
                &self,
                _handle: reify_ir::GeometryHandleId,
                _format: reify_ir::ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), reify_ir::ExportError> {
                unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
            }
        };
    }

    /// Mock kernel whose `tessellate` returns the closed `box_2mm()` mesh.
    /// Other required methods are unreachable stubs.
    struct TessellatingBoxKernel;
    impl reify_ir::GeometryKernel for TessellatingBoxKernel {
        unreachable_kernel_stubs!();
        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            Ok(box_2mm())
        }
        // ingest_mesh: inherits default → Err (not used; openvdb kernel does ingest)
    }

    /// Mock kernel: `ingest_mesh` returns Ok(handle) so the chain reaches
    /// `densify_grid_to_sampled`, which inherits the default → Err(QueryFailed).
    /// Used under `openvdb_kernel_name()` to test the densify-Err degradation path.
    struct IngestOkDensifyFailKernel;
    impl reify_ir::GeometryKernel for IngestOkDensifyFailKernel {
        unreachable_kernel_stubs!();
        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            Ok(reify_ir::GeometryHandle {
                id: reify_ir::GeometryHandleId(42),
                repr: None,
            })
        }
        // densify_grid_to_sampled: inherits default →
        // Err(QueryError::QueryFailed("densify_grid_to_sampled not supported by this kernel"))
    }

    /// (a) SUCCESS: TessellatingBoxKernel (BRep→Mesh) + real OpenVdbKernel
    /// (Mesh→Voxel→SampledField) → Some(field) with structural + φ<0 interior.
    ///
    /// RED under step-2: the placeholder returns None; step-4 wires the full chain.
    #[cfg(has_openvdb)]
    #[test]
    fn realize_solid_sdf_realized_box_returns_sampleable_field() {
        use reify_ir::SampledGridKind;
        use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

        let mut engine = make_engine();

        // Source kernel: TessellatingBoxKernel handles BRep→Mesh stage.
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(TessellatingBoxKernel));
        engine.default_kernel_name = Some("occt".to_string());

        // OpenVDB kernel (real): Mesh→Voxel→SampledField stage.
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();
        engine
            .geometry_kernels
            .insert(openvdb_name.to_string(), Box::new(OpenVdbKernel::new()));

        // Seed a resolvable BRep subject.
        let r0 = RealizationNodeId::new("gamma-box-test", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(1));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };

        let field = engine
            .realize_solid_sdf(subject)
            .expect("realize_solid_sdf must return Some(SampledField) for a valid closed box");

        // ── Structural checks (realization-read-api.md §3.3 δ; no tolerance) ─
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
        // CPU-sampleable: φ at box centre (0,0,0) must be negative (interior).
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

    /// (b) DENSIFY-ERR: TessellatingBoxKernel + IngestOkDensifyFailKernel
    /// under openvdb_kernel_name() → chain reaches densify, gets Err → None.
    /// No panic; cfg-independent.
    #[test]
    fn realize_solid_sdf_densify_err_returns_none_no_panic() {
        let mut engine = make_engine();

        // Source kernel: TessellatingBoxKernel returns box_2mm() from tessellate.
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(TessellatingBoxKernel));
        engine.default_kernel_name = Some("occt".to_string());

        // "OpenVDB" stub: ingest_mesh → Ok(handle), densify → Err.
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();
        engine.geometry_kernels.insert(
            openvdb_name.to_string(),
            Box::new(IngestOkDensifyFailKernel),
        );

        // Seed a resolvable BRep subject.
        let r0 = RealizationNodeId::new("gamma-densify-err", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(1));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };

        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "densify Err path must return None (no panic)"
        );
    }

    /// (c) Stub build: Engine::with_registered_kernels omits openvdb from the
    /// registry (register.rs:157 cfg-gates the submit!) → no openvdb kernel in
    /// geometry_kernels → realize_solid_sdf returns None, no fabricated field.
    #[cfg(not(has_openvdb))]
    #[test]
    fn realize_solid_sdf_stub_build_returns_none_no_fabricated_field() {
        let mut engine = Engine::with_registered_kernels(crate::SolverPosture::SolverFree, Box::new(MockConstraintChecker::new()));

        // Seed a resolvable subject so the only reason for None is missing openvdb.
        // We also need a source kernel; in stub builds with_registered_kernels may
        // have one (e.g. OCCT if it is registered) or none.  Add MockGeometryKernel
        // as an explicit fallback source so the resolution + source guards pass.
        engine.geometry_kernels.insert(
            "occt-stub-source".to_string(),
            Box::new(MockGeometryKernel::new()),
        );
        engine.default_kernel_name = Some("occt-stub-source".to_string());

        let r0 = RealizationNodeId::new("stub-solid", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(42));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(42)),
        };

        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "cfg(not(has_openvdb)) realize_solid_sdf must return None — no fabricated field"
        );
    }

    // ── task 6560 step-9 RED: resolution threading through the direct recipe ──
    //
    // Every test below references `engine.realize_solid_sdf_at(subject, …)`,
    // which does NOT exist yet — they compile-fail (RED) until step-10 adds it.

    /// Shared recorder for the `(VoxelResolution, Mesh)` pairs a voxelizer is
    /// asked to ingest, in call order.
    ///
    /// `Arc<Mutex<…>>` rather than a plain field because the kernel is moved
    /// into `engine.geometry_kernels` as a `Box<dyn GeometryKernel>` and cannot
    /// be borrowed back out afterwards. Mirrors the `Arc<Mutex<Vec<f64>>>`
    /// tessellate-tolerance recorder on `MockGeometryKernel`
    /// (`reify_test_support::mocks::MockGeometryKernel::tessellate_tolerances_ref`).
    type IngestLog =
        std::sync::Arc<std::sync::Mutex<Vec<(reify_ir::VoxelResolution, reify_ir::Mesh)>>>;

    /// The requests a voxelizer was handed, in call order.
    fn requests_in(log: &IngestLog) -> Vec<reify_ir::VoxelResolution> {
        log.lock()
            .unwrap()
            .iter()
            .map(|&(request, _)| request)
            .collect()
    }

    /// Mock openvdb-slot kernel that records every `(VoxelResolution, Mesh)` it
    /// is asked to ingest and (optionally) fails the ingest.
    ///
    /// Every method the recipe must NOT call returns `Err` (or is
    /// `unreachable!()`), so a regression that reaches for the wrong entry
    /// point fails loudly rather than silently passing. In particular
    /// [`reify_ir::GeometryKernel::ingest_mesh`] is overridden to `Err` —
    /// if `realize_solid_sdf_at` ever calls the resolution-less entry point,
    /// the log stays empty and the forwarding assertions fail.
    struct RecordingVoxelizerKernel {
        log: IngestLog,
        fail_ingest: bool,
    }

    impl reify_ir::GeometryKernel for RecordingVoxelizerKernel {
        unreachable_kernel_stubs!();
        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            unreachable!() // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }
        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            Err(reify_ir::GeometryError::OperationFailed(
                "realize_solid_sdf must route through ingest_mesh_at_resolution".into(),
            ))
        }
        fn ingest_mesh_at_resolution(
            &mut self,
            mesh: &reify_ir::Mesh,
            resolution: reify_ir::VoxelResolution,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.log.lock().unwrap().push((resolution, mesh.clone()));
            if self.fail_ingest {
                return Err(reify_ir::GeometryError::OperationFailed(
                    "synthetic ingest failure".into(),
                ));
            }
            Ok(reify_ir::GeometryHandle {
                id: reify_ir::GeometryHandleId(42),
                repr: None,
            })
        }
        // densify_grid_to_sampled: inherits the default → Err, so the recipe
        // still degrades to None. These tests assert on the RECORDED
        // resolution, which is captured before that point.
    }

    /// Build an engine wired with `source` under `"occt"` and `voxelizer`
    /// under `openvdb_kernel_name()`, plus a resolvable subject.
    fn engine_with(
        source: Box<dyn reify_ir::GeometryKernel>,
        voxelizer: Box<dyn reify_ir::GeometryKernel>,
        tag: &str,
    ) -> (Engine, GeometryHandleRef) {
        let mut engine = make_engine();
        engine.geometry_kernels.insert("occt".to_string(), source);
        engine.default_kernel_name = Some("occt".to_string());
        engine.geometry_kernels.insert(
            crate::kernel_registry::openvdb_kernel_name().to_string(),
            voxelizer,
        );

        let r0 = RealizationNodeId::new(tag, 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(1));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };
        (engine, subject)
    }

    /// **Forwarding.** `realize_solid_sdf_at` must hand the requested
    /// resolution to the voxelizer verbatim, exactly once.
    #[test]
    fn realize_solid_sdf_at_forwards_the_requested_resolution() {
        let log: IngestLog = Default::default();
        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: log.clone(),
                fail_ingest: false,
            }),
            "gamma-forward-resolution",
        );

        // The overall result is None (the mock's densify inherits the default
        // Err); what this test pins is the resolution that reached the kernel.
        let _ = engine.realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0));

        assert_eq!(
            requests_in(&log),
            [reify_ir::VoxelResolution::MinFeature(1.0)],
            "realize_solid_sdf_at must call ingest_mesh_at_resolution exactly once \
             with the requested resolution"
        );
    }

    /// **Behaviour preservation.** The existing `realize_solid_sdf` entry point
    /// must forward `HonestFloor`, so every production caller of it keeps the
    /// grid it has always got.
    #[test]
    fn realize_solid_sdf_forwards_honest_floor() {
        let log: IngestLog = Default::default();
        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: log.clone(),
                fail_ingest: false,
            }),
            "gamma-honest-floor-default",
        );

        let _ = engine.realize_solid_sdf(subject);

        assert_eq!(
            requests_in(&log),
            [reify_ir::VoxelResolution::HonestFloor],
            "realize_solid_sdf must delegate with VoxelResolution::HonestFloor"
        );
    }

    /// Degradation path 1 under `_at`: unresolvable `realization_ref` AND
    /// `GeometryHandleId::INVALID` → `None`, no panic.
    #[test]
    fn realize_solid_sdf_at_unresolvable_subject_returns_none() {
        let log: IngestLog = Default::default();
        let (mut engine, _) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: log.clone(),
                fail_ingest: false,
            }),
            "gamma-at-unresolvable-seed",
        );
        let subject = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("absent-solid-at", 99),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId::INVALID),
        };

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0))
                .is_none(),
            "an unresolvable subject must degrade to None"
        );
        assert!(
            log.lock().unwrap().is_empty(),
            "the voxelizer must not be reached when the subject cannot be resolved"
        );
    }

    /// Degradation path 2 under `_at`: no `default_kernel_name` → `None`.
    #[test]
    fn realize_solid_sdf_at_no_default_kernel_returns_none() {
        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: Default::default(),
                fail_ingest: false,
            }),
            "gamma-at-no-source",
        );
        engine.default_kernel_name = None;

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0))
                .is_none(),
            "no default_kernel_name must degrade to None"
        );
    }

    /// Degradation path 3 under `_at`: no OpenVDB kernel registered → `None`.
    #[test]
    fn realize_solid_sdf_at_no_openvdb_kernel_returns_none() {
        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: Default::default(),
                fail_ingest: false,
            }),
            "gamma-at-no-openvdb",
        );
        engine
            .geometry_kernels
            .remove(crate::kernel_registry::openvdb_kernel_name());

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0))
                .is_none(),
            "a missing openvdb kernel must degrade to None"
        );
    }

    /// Degradation path 4 under `_at`: `tessellate` returns `Err` → `None`.
    ///
    /// The source kernel is `reify_test_support::mocks::FailingMockGeometryKernel`,
    /// whose `tessellate` already returns `Err(TessError::TessellationFailed(_))`
    /// — the shared utility for exactly this shape, so no local clone of it.
    #[test]
    fn realize_solid_sdf_at_tessellate_err_returns_none() {
        let log: IngestLog = Default::default();
        let (mut engine, subject) = engine_with(
            Box::new(FailingMockGeometryKernel),
            Box::new(RecordingVoxelizerKernel {
                log: log.clone(),
                fail_ingest: false,
            }),
            "gamma-at-tess-err",
        );

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0))
                .is_none(),
            "a tessellate Err must degrade to None"
        );
        assert!(
            log.lock().unwrap().is_empty(),
            "the voxelizer must not be reached when tessellation fails"
        );
    }

    /// Degradation path 5 under `_at`: `ingest_mesh_at_resolution` returns
    /// `Err` → `None`, no panic and no fabricated field.
    #[test]
    fn realize_solid_sdf_at_ingest_err_returns_none() {
        let log: IngestLog = Default::default();
        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(RecordingVoxelizerKernel {
                log: log.clone(),
                fail_ingest: true,
            }),
            "gamma-at-ingest-err",
        );

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(1.0))
                .is_none(),
            "an ingest Err must degrade to None"
        );
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "the failing ingest must have been attempted exactly once"
        );
    }

    /// An over-budget resolution must propagate as `None` — the guard's
    /// rejection is a degradation, not a panic (PRD §4 D5).
    ///
    /// Uses the REAL OpenVDB kernel so the budget guard is the one that
    /// actually fires: on the 2 mm box at h = 0.001 the implied dense grid is
    /// ≈ 4004³ ≈ 6.4e10 voxels, far beyond the 256M budget.
    #[cfg(has_openvdb)]
    #[test]
    fn realize_solid_sdf_at_over_budget_resolution_returns_none() {
        use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

        let (mut engine, subject) = engine_with(
            Box::new(TessellatingBoxKernel),
            Box::new(OpenVdbKernel::new()),
            "gamma-at-over-budget",
        );

        assert!(
            engine
                .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::TargetVoxelSize(0.001))
                .is_none(),
            "an over-budget resolution must degrade to None, not panic"
        );
    }

    // ── task 7502: the BRep→Mesh chord tolerance follows the voxel grid ──────
    //
    // The fakes below make the tessellation that REACHED the voxelizer
    // observable, not just the tolerances asked for: a recipe that tessellated
    // finely and then voxelized the coarse mesh would pass a tolerance-only
    // check.

    type ToleranceLog = std::sync::Arc<std::sync::Mutex<Vec<f64>>>;

    /// Half-extent (SI metres) of the 2 mm cube most of these tests tessellate.
    const HALF_EXTENT_OF_2MM_CUBE: f32 = 1e-3;

    /// Half-extent of a 2 m cube, whose honest-floor voxel is far coarser than
    /// the fixed chord.
    const HALF_EXTENT_OF_2M_CUBE: f32 = 1.0;

    /// The chord a mesh was tessellated at, as `ChordRecordingCubeKernel` leaves
    /// it in the mesh's `normals`. OpenVdbKernel's ingest reads only `vertices`
    /// and `indices`, so the stamp reaches the voxelizer without changing the
    /// grid it builds.
    fn chord_stamp(chord: f64) -> Option<Vec<f32>> {
        Some(vec![chord as f32])
    }

    /// Source-slot kernel whose `tessellate` records the tolerance it is asked
    /// for and returns a closed cube stamped with it ([`chord_stamp`]). With
    /// `fail_below = Some(floor)` it refuses every tolerance under `floor`.
    struct ChordRecordingCubeKernel {
        half_extent: f32,
        tolerances: ToleranceLog,
        fail_below: Option<f64>,
    }

    impl reify_ir::GeometryKernel for ChordRecordingCubeKernel {
        unreachable_kernel_stubs!();
        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.tolerances.lock().unwrap().push(tolerance);
            if self.fail_below.is_some_and(|floor| tolerance < floor) {
                return Err(reify_ir::TessError::TessellationFailed(format!(
                    "synthetic refusal: tolerance {tolerance} is below the kernel's floor"
                )));
            }
            let mut cube = reify_kernel_openvdb::test_fixtures::box_mesh(
                self.half_extent,
                self.half_extent,
                self.half_extent,
            );
            cube.normals = chord_stamp(tolerance);
            Ok(cube)
        }
    }

    /// `==` on requests, with NaN payloads compared by bit pattern so that a
    /// NaN request can be followed through the recipe.
    fn same_request(a: reify_ir::VoxelResolution, b: reify_ir::VoxelResolution) -> bool {
        use reify_ir::VoxelResolution::{HonestFloor, MinFeature, TargetVoxelSize};
        match (a, b) {
            (HonestFloor, HonestFloor) => true,
            (TargetVoxelSize(x), TargetVoxelSize(y)) | (MinFeature(x), MinFeature(y)) => {
                x.to_bits() == y.to_bits()
            }
            _ => false,
        }
    }

    /// The kernel's own answer for `request` on a cube of `half_extent`: the
    /// oracle for the voxel size the grid will be built at.
    fn kernel_policy(
        half_extent: f32,
        request: reify_ir::VoxelResolution,
    ) -> Result<reify_kernel_openvdb::MeshToVoxelOptions, reify_kernel_openvdb::VoxelResolutionError>
    {
        let cube =
            reify_kernel_openvdb::test_fixtures::box_mesh(half_extent, half_extent, half_extent);
        reify_kernel_openvdb::MeshToVoxelOptions::for_resolution(&cube, request)
    }

    /// Everything the two fakes saw during one `realize_solid_sdf_at` call.
    struct ChordRun {
        result: Option<reify_ir::SampledField>,
        tolerances: Vec<f64>,
        ingested: Vec<(reify_ir::VoxelResolution, reify_ir::Mesh)>,
    }

    impl ChordRun {
        /// The one mesh the voxelizer was handed, after checking that it was
        /// handed exactly one, with `request` intact.
        fn sole_ingested_mesh(&self, request: reify_ir::VoxelResolution) -> &reify_ir::Mesh {
            let [(ingested_request, mesh)] = self.ingested.as_slice() else {
                panic!(
                    "{request:?}: expected exactly one ingest, got {}",
                    self.ingested.len()
                );
            };
            assert!(
                same_request(*ingested_request, request),
                "{request:?}: the voxelizer must receive the request verbatim, got {ingested_request:?}"
            );
            mesh
        }
    }

    /// Run `realize_solid_sdf_at` on a fresh engine over a cube of
    /// `half_extent`, with the voxelizer a recording fake.
    fn run_chord_recipe(
        half_extent: f32,
        fail_below: Option<f64>,
        request: reify_ir::VoxelResolution,
    ) -> ChordRun {
        let tolerances = ToleranceLog::default();
        let ingested = IngestLog::default();
        let (mut engine, subject) = engine_with(
            Box::new(ChordRecordingCubeKernel {
                half_extent,
                tolerances: tolerances.clone(),
                fail_below,
            }),
            Box::new(RecordingVoxelizerKernel {
                log: ingested.clone(),
                fail_ingest: false,
            }),
            "gamma-chord-recipe",
        );

        let result = engine.realize_solid_sdf_at(subject, request);

        let tolerances = tolerances.lock().unwrap().clone();
        let ingested = ingested.lock().unwrap().clone();
        ChordRun {
            result,
            tolerances,
            ingested,
        }
    }

    /// **The derivation.** Whichever way a request names its grid, the mesh the
    /// voxelizer receives was tessellated at half the voxel size the kernel's
    /// policy resolves that request to.
    #[test]
    fn realize_solid_sdf_at_tessellates_at_half_the_voxel_size_for_every_arm() {
        use reify_ir::VoxelResolution::{HonestFloor, MinFeature, TargetVoxelSize};

        // First principles for the oracle itself: an explicit voxel size is
        // used verbatim, so the half-step chord for 1e-4 is 5e-5.
        let explicit = kernel_policy(HALF_EXTENT_OF_2MM_CUBE, TargetVoxelSize(1e-4))
            .expect("1e-4 is servable on a 2 mm cube");
        assert_eq!(explicit.voxel_size / 2.0, 1e-4 / 2.0);

        for request in [TargetVoxelSize(1e-4), MinFeature(4e-4), HonestFloor] {
            let voxel_size = kernel_policy(HALF_EXTENT_OF_2MM_CUBE, request)
                .unwrap_or_else(|e| panic!("{request:?} must be servable on a 2 mm cube: {e}"))
                .voxel_size;
            let expected = voxel_size / 2.0;

            let run = run_chord_recipe(HALF_EXTENT_OF_2MM_CUBE, None, request);

            assert_eq!(
                run.tolerances.last(),
                Some(&expected),
                "{request:?}: the last tessellation must be at half the voxel size {voxel_size}"
            );
            assert_eq!(
                run.sole_ingested_mesh(request).normals,
                chord_stamp(expected),
                "{request:?}: the voxelized mesh must be the one tessellated at {expected}"
            );
        }
    }

    /// **The ceiling.** The fixed chord is a maximum: a grid too coarse to need
    /// a finer mesh gets exactly the one tessellation it always got, including
    /// at the boundary where half the voxel size equals the ceiling.
    #[test]
    fn realize_solid_sdf_at_never_tessellates_coarser_than_the_fixed_chord() {
        use std::cmp::Ordering::{Equal, Greater};

        use reify_ir::VoxelResolution::{HonestFloor, TargetVoxelSize};

        let ceiling = super::MAX_TESSELLATION_CHORD_TOLERANCE;
        let cases = [
            // Honest-floor voxel 2/64 m: half of it is far above the ceiling.
            (HALF_EXTENT_OF_2M_CUBE, HonestFloor, Greater),
            // Half of the 2e-4 voxel is exactly the ceiling.
            (HALF_EXTENT_OF_2MM_CUBE, TargetVoxelSize(2e-4), Equal),
        ];

        for (half_extent, request, half_voxel_vs_ceiling) in cases {
            // The kernel must SERVE the request, with half a voxel where the
            // case says: an unservable request is also voxelized at the
            // ceiling, so without this the case could pass without ever
            // exercising it.
            let half_voxel = kernel_policy(half_extent, request)
                .unwrap_or_else(|e| panic!("{request:?} must be servable here: {e}"))
                .voxel_size
                / 2.0;
            assert_eq!(
                half_voxel.partial_cmp(&ceiling),
                Some(half_voxel_vs_ceiling),
                "{request:?} on a cube of half-extent {half_extent}: half the voxel size \
                 {half_voxel} must compare as {half_voxel_vs_ceiling:?} to the ceiling {ceiling}"
            );

            let run = run_chord_recipe(half_extent, None, request);

            assert_eq!(
                run.tolerances,
                [ceiling],
                "{request:?} on a cube of half-extent {half_extent}: one tessellation, at the ceiling"
            );
            assert_eq!(
                run.sole_ingested_mesh(request).normals,
                chord_stamp(ceiling),
                "{request:?} on a cube of half-extent {half_extent}: the ceiling mesh is the one voxelized"
            );
        }
    }

    /// **Unservable requests are not refined.** When the kernel's policy
    /// rejects a request there is no voxel size to derive a chord from, so the
    /// recipe hands the unrefined mesh to the voxelizer, which stays the one
    /// place that rejects it.
    ///
    /// Only the too-fine request can fail this test through a refinement: its
    /// half voxel size is far below the ceiling, so a recipe that took the
    /// voxel size from the request without asking whether the kernel can serve
    /// it would send the tessellator a runaway chord. The other two requests
    /// derive a chord at or above the ceiling (or none at all) even then.
    #[test]
    fn realize_solid_sdf_at_unservable_request_is_voxelized_unrefined() {
        use reify_ir::VoxelResolution::{MinFeature, TargetVoxelSize};

        let ceiling = super::MAX_TESSELLATION_CHORD_TOLERANCE;
        for request in [
            // Too coarse: four 1e-3 voxels do not fit the 2 mm extent.
            TargetVoxelSize(1e-3),
            // Malformed.
            MinFeature(f64::NAN),
            // Too fine: about 4000 voxels per axis is far over the dense-grid budget.
            TargetVoxelSize(1e-6),
        ] {
            assert!(
                kernel_policy(HALF_EXTENT_OF_2MM_CUBE, request).is_err(),
                "{request:?} must be unservable on a 2 mm cube for this test to mean anything"
            );

            let run = run_chord_recipe(HALF_EXTENT_OF_2MM_CUBE, None, request);

            assert_eq!(
                run.tolerances,
                [ceiling],
                "{request:?}: an unservable request must not be refined"
            );
            assert_eq!(
                run.sole_ingested_mesh(request).normals,
                chord_stamp(ceiling),
                "{request:?}: the unrefined mesh must still reach the voxelizer"
            );
        }
    }

    /// **A failed refinement degrades to `None`.** The coarse mesh is exactly
    /// the facet-resolving input the derivation exists to avoid, so it must
    /// never be voxelized as a fallback.
    #[test]
    fn realize_solid_sdf_at_refined_tessellation_failure_returns_none() {
        let ceiling = super::MAX_TESSELLATION_CHORD_TOLERANCE;
        let request = reify_ir::VoxelResolution::TargetVoxelSize(1e-4);

        let run = run_chord_recipe(HALF_EXTENT_OF_2MM_CUBE, Some(ceiling), request);

        assert_eq!(
            run.tolerances.last(),
            Some(&(1e-4 / 2.0)),
            "the refinement must have been attempted, or the None below proves nothing"
        );
        assert!(
            run.result.is_none(),
            "a refused refinement must degrade to None"
        );
        assert!(
            run.ingested.is_empty(),
            "the coarse mesh must not be voxelized as a fallback; ingested {} mesh(es)",
            run.ingested.len()
        );
    }

    /// Run `f`, capturing the `warn` events the recipe logs while it runs.
    fn capturing_recipe_warnings<T>(f: impl FnOnce() -> T) -> (T, reify_test_support::Capture) {
        let (subscriber, capture) =
            reify_test_support::CapturingSubscriberBuilder::new(tracing::Level::WARN)
                .target_prefix("reify_eval::realize_solid_sdf")
                .build();
        (tracing::subscriber::with_default(subscriber, f), capture)
    }

    /// **A refused refinement is reported to the caller who asked for it.**
    /// Under a request-driven resolution the fine chord is a consequence of
    /// the caller's request, so its refusal is logged with that request and
    /// chord, instead of degrading as anonymously as a build with no OpenVDB.
    #[test]
    fn realize_solid_sdf_at_refused_refinement_is_warned_for_a_requested_resolution() {
        let request = reify_ir::VoxelResolution::TargetVoxelSize(1e-4);

        let (run, warnings) = capturing_recipe_warnings(|| {
            run_chord_recipe(
                HALF_EXTENT_OF_2MM_CUBE,
                Some(super::MAX_TESSELLATION_CHORD_TOLERANCE),
                request,
            )
        });

        assert_eq!(
            run.tolerances.last(),
            Some(&(1e-4 / 2.0)),
            "the refinement must have been attempted, or the warning below proves nothing"
        );
        assert_eq!(
            warnings.count(),
            1,
            "a refused refinement must be logged exactly once; got {:?}",
            warnings.messages()
        );
        let events = warnings.fields_by_event();
        let fields = &events[0];
        assert_eq!(fields["resolution"], format!("{request:?}"));
        assert_eq!(fields["chord"].parse::<f64>(), Ok(1e-4 / 2.0));
        assert!(
            fields["error"].contains("synthetic refusal"),
            "the warning must carry the tessellator's own error; got {:?}",
            fields["error"]
        );
    }

    /// **A refused refinement stays silent under the default resolution.** A
    /// part small enough for the honest-floor grid to want a finer mesh was
    /// not asked for anything by the caller, so there is no choice to blame
    /// and the refusal degrades like any other environment failure.
    #[test]
    fn realize_solid_sdf_at_refused_refinement_is_silent_under_honest_floor() {
        let ceiling = super::MAX_TESSELLATION_CHORD_TOLERANCE;

        let (run, warnings) = capturing_recipe_warnings(|| {
            run_chord_recipe(
                HALF_EXTENT_OF_2MM_CUBE,
                Some(ceiling),
                reify_ir::VoxelResolution::HonestFloor,
            )
        });

        assert!(
            run.tolerances.last().is_some_and(|&chord| chord < ceiling),
            "the refinement must have been attempted, or the silence below proves nothing; \
             tessellated at {:?}",
            run.tolerances
        );
        assert!(
            run.result.is_none(),
            "a refused refinement must degrade to None"
        );
        assert_eq!(
            warnings.count(),
            0,
            "HonestFloor carries no caller choice to blame; got {:?}",
            warnings.messages()
        );
    }

    /// **End to end.** With the real OpenVDB kernel the chord must track the
    /// grid that kernel actually builds, not merely the grid the policy
    /// predicts: half the densified field's voxel spacing.
    #[cfg(has_openvdb)]
    #[test]
    fn realize_solid_sdf_at_chord_tracks_the_grid_the_kernel_builds() {
        use reify_kernel_openvdb::kernel_real::OpenVdbKernel;

        let tolerances = ToleranceLog::default();
        let (mut engine, subject) = engine_with(
            Box::new(ChordRecordingCubeKernel {
                half_extent: HALF_EXTENT_OF_2MM_CUBE,
                tolerances: tolerances.clone(),
                fail_below: None,
            }),
            Box::new(OpenVdbKernel::new()),
            "gamma-chord-tracks-grid",
        );

        let field = engine
            .realize_solid_sdf_at(subject, reify_ir::VoxelResolution::MinFeature(4e-4))
            .expect("a servable request on a closed cube must densify to a field");

        let voxel_size = field.spacing.iter().copied().fold(f64::INFINITY, f64::min);
        let chord = *tolerances
            .lock()
            .unwrap()
            .last()
            .expect("the recipe must tessellate");
        let expected = voxel_size / 2.0;
        // The densified spacing is the grid transform's voxel size, so the two
        // agree to rounding: compare within 4 ulps, not a tuned bound.
        assert!(
            (chord - expected).abs() <= 4.0 * f64::EPSILON * expected,
            "the chord {chord} must be half the densified voxel size {voxel_size}"
        );
    }
}
