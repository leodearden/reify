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
    /// Degradation paths → `None`:
    ///  1. `subject.realization_ref` absent from `realization_handles` AND
    ///     `subject.kernel_handle == GeometryHandleId::INVALID` (resolution fails).
    ///  2. No `default_kernel_name` configured (no source kernel to tessellate).
    ///  3. No kernel registered under `openvdb_kernel_name()` — absent in stub
    ///     builds where the `cfg(any(has_openvdb, feature="stub_register"))` gate
    ///     on `inventory::submit!` is not satisfied.  This is the D5 mechanism.
    ///  4. `tessellate`, `ingest_mesh`, or `densify_grid_to_sampled` returns
    ///     `Err` (chain failure).
    #[allow(dead_code)] // consumed by δ=4424, ε=4425, ζ=4426 (future tasks)
    pub(crate) fn realize_solid_sdf(
        &mut self,
        subject: reify_ir::value::GeometryHandleRef,
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
                // TODO(#4652): step-8 converts None to genuine decline; for now
                // treat None like INVALID (no None producer until eval-mint in step-4).
                let kh = subject
                    .kernel_handle
                    .unwrap_or(reify_ir::GeometryHandleId::INVALID);
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
            "realize_solid_sdf: demanding Voxel realization of subject solid"
        );

        // Tessellate BRep→Mesh
        let mesh = self
            .geometry_kernels
            .get(&source)?
            .tessellate(brep_id, 0.0001)
            .ok()?;

        // Ingest Mesh→Voxel
        let voxel = self
            .geometry_kernels
            .get_mut(openvdb_name)?
            .ingest_mesh(&mesh)
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
    use reify_test_support::mocks::{MockConstraintChecker, MockGeometryKernel};

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
}
