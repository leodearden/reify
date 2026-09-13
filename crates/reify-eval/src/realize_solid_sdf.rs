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

/// Chordal tolerance handed to the source kernel's `tessellate` for the
/// BRep→Mesh stage of the recipe, in the model's own units (SI metres).
///
/// A fixed value, independent of the caller's [`reify_ir::VoxelResolution`] —
/// see the "Known limitation" section on [`crate::Engine::realize_solid_sdf_at`]
/// for why it cannot be derived here and what closing that needs.
const TESSELLATION_CHORD_TOLERANCE: f64 = 0.0001;

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
    /// That feature is passed as a MODEL-SPACE length in the mesh's own units
    /// (SI metres, per `reify_ir::Mesh::vertices`), so the 1 mm feature is
    /// `MinFeature(0.001)` — see `VoxelResolution`'s "Units" section, which
    /// spells out why `MinFeature(1.0)` is not it.
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
    ///  4. `tessellate`, `ingest_mesh_at_resolution`, or
    ///     `densify_grid_to_sampled` returns `Err` (chain failure).
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
    /// [`reify_ir::VoxelResolution::HonestFloor`] ingest carries no caller
    /// choice to blame, so its `Err` degrades silently like paths 1-4, while a
    /// request-driven ingest logs the error it actually got without presuming
    /// which of the two it was.
    ///
    /// # Known limitation — the tessellation tolerance is not derived from `resolution`
    ///
    /// The BRep→Mesh stage uses a fixed [`TESSELLATION_CHORD_TOLERANCE`],
    /// independent of `resolution`, and the voxel grid can only resolve detail
    /// the mesh already carries. At the shells working point in real model
    /// space (a 0.1 m part, `MinFeature(0.001)` ⇒ h = 2.5e-4 m) that tolerance
    /// is already ≈ 40 % of a voxel, and a finer request resolves tessellation
    /// FACETS rather than geometry — silently, since nothing errors.
    ///
    /// Deriving it here is not possible as the crates are layered: it needs the
    /// voxel size the request resolves to, and that mapping is KERNEL policy
    /// (`MeshToVoxelOptions::for_resolution`). `reify-eval` cannot name
    /// `reify-kernel-openvdb` — the adapter → eval dependency direction is
    /// deliberately inverted (see that crate's `Cargo.toml` dev-dep rationale)
    /// — so duplicating the policy here would be a SPOT violation across a
    /// crate boundary. Closing it needs a kernel-side seam reporting that voxel
    /// size; filed as a follow-up of task 6560, "Derive the BRep→Mesh chord
    /// tolerance from the VoxelResolution request".
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

        // Tessellate BRep→Mesh. The tolerance does not track `resolution` — see
        // this method's "Known limitation".
        let mesh = self
            .geometry_kernels
            .get(&source)?
            .tessellate(brep_id, TESSELLATION_CHORD_TOLERANCE)
            .ok()?;

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
                if !matches!(resolution, reify_ir::VoxelResolution::HonestFloor) {
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
        Engine::new(Box::new(MockConstraintChecker::new()), None)
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

    /// Closed box mesh (±1.0 mm on each axis, 12 triangles).
    /// Same fixture as realization_content.rs::box_2mm; defined without a
    /// `cfg(has_openvdb)` gate so TessellatingBoxKernel can be used in both
    /// the cfg(has_openvdb) success test and the cfg-independent densify-Err test.
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

    /// Mock kernel whose `tessellate` returns the closed `box_2mm()` mesh.
    /// Other required methods are unreachable stubs.
    struct TessellatingBoxKernel;
    impl reify_ir::GeometryKernel for TessellatingBoxKernel {
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
        let mut engine = Engine::with_registered_kernels(Box::new(MockConstraintChecker::new()));

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

    /// Shared recorder for the `VoxelResolution` values a kernel is handed.
    ///
    /// `Arc<Mutex<…>>` rather than a plain field because the kernel is moved
    /// into `engine.geometry_kernels` as a `Box<dyn GeometryKernel>` and cannot
    /// be borrowed back out afterwards. Mirrors the `Arc<Mutex<Vec<f64>>>`
    /// tessellate-tolerance recorder on `MockGeometryKernel`
    /// (`reify_test_support::mocks::MockGeometryKernel::tessellate_tolerances_ref`).
    type ResolutionLog = std::sync::Arc<std::sync::Mutex<Vec<reify_ir::VoxelResolution>>>;

    /// Mock openvdb-slot kernel that records every `VoxelResolution` it is
    /// handed and (optionally) fails the ingest.
    ///
    /// Every method the recipe must NOT call returns `Err` (or is
    /// `unreachable!()`), so a regression that reaches for the wrong entry
    /// point fails loudly rather than silently passing. In particular
    /// [`reify_ir::GeometryKernel::ingest_mesh`] is overridden to `Err` —
    /// if `realize_solid_sdf_at` ever calls the resolution-less entry point,
    /// the log stays empty and the forwarding assertions fail.
    struct RecordingVoxelizerKernel {
        log: ResolutionLog,
        fail_ingest: bool,
    }

    impl reify_ir::GeometryKernel for RecordingVoxelizerKernel {
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
            _mesh: &reify_ir::Mesh,
            resolution: reify_ir::VoxelResolution,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.log.lock().unwrap().push(resolution);
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
        let log: ResolutionLog = Default::default();
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
            log.lock().unwrap().as_slice(),
            &[reify_ir::VoxelResolution::MinFeature(1.0)],
            "realize_solid_sdf_at must call ingest_mesh_at_resolution exactly once \
             with the requested resolution"
        );
    }

    /// **Behaviour preservation.** The existing `realize_solid_sdf` entry point
    /// must forward `HonestFloor`, pinning that `measure_thickness_pair`
    /// (engine_constraints.rs:1551) and `measure_min_feature`
    /// (measure_min_feature.rs:46) keep the grid they have always got.
    #[test]
    fn realize_solid_sdf_forwards_honest_floor() {
        let log: ResolutionLog = Default::default();
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
            log.lock().unwrap().as_slice(),
            &[reify_ir::VoxelResolution::HonestFloor],
            "realize_solid_sdf must delegate with VoxelResolution::HonestFloor"
        );
    }

    /// Degradation path 1 under `_at`: unresolvable `realization_ref` AND
    /// `GeometryHandleId::INVALID` → `None`, no panic.
    #[test]
    fn realize_solid_sdf_at_unresolvable_subject_returns_none() {
        let log: ResolutionLog = Default::default();
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
        let log: ResolutionLog = Default::default();
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
        let log: ResolutionLog = Default::default();
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
}
