// PRD §7.1 δ=4424: measure_min_wall — eval-side binding from γ's
// `realize_solid_sdf` wire to δ's `reify_shell_extract::min_wall_thickness`.
//
// Engine::measure_min_wall(&mut self, subject: GeometryHandleRef)
//   -> Option<reify_shell_extract::MinWallThickness>
//
// Calls `realize_solid_sdf(subject)?` (None on every degradation path per
// PRD §4 D5: no-openvdb-kernel / unresolvable subject / chain failure),
// derives `h = min(field.spacing)` from γ's realized SampledField, and
// delegates to `reify_shell_extract::min_wall_thickness(&sdf, h).ok()`.
//
// D5: both `None` from `realize_solid_sdf` and `Err` from
// `min_wall_thickness` map to `None` — never a fabricated number.
//
// PRD §4 D1: does NOT re-enter the dispatcher BFS / realization loop.
// The subject is already realized; this is a post-build direct recipe.
//
// Consumer: ζ=4426 (maps None/BelowResolution → Indeterminate + diagnostic).
// The realized-field happy-path honest-number e2e is deferred to η=4427
// to avoid coupling δ's tests to α's OpenVDB voxelisation defaults.

impl crate::Engine {
    /// Measure the minimum wall thickness of an already-realized BRep solid.
    ///
    /// Calls `realize_solid_sdf(subject)` to obtain the CPU-resident SampledField
    /// (None on every degradation path — no OpenVDB kernel, unresolvable subject,
    /// chain failure), derives `h = min(field.spacing)` from the realized grid's
    /// own spacing, and delegates to
    /// `reify_shell_extract::min_wall_thickness(&sdf, h)`.
    ///
    /// # Returns
    ///
    /// - `Some(Measured(t))` — min-wall `t ≥ 2·h`; conservative lower bound.
    /// - `Some(BelowResolution { raw, floor })` — `raw < 2·h`; self-describing.
    /// - `Some(NoMeasurement)` — no medial voxels found.
    /// - `None` — `realize_solid_sdf` returned `None` (D5: no fabricated number)
    ///   OR `min_wall_thickness` returned `Err` (structurally invalid field).
    ///
    /// PRD §4 D5: never panics and never fabricates a number.
    #[allow(dead_code)] // consumed by ζ=4426
    pub(crate) fn measure_min_wall(
        &mut self,
        subject: reify_ir::value::GeometryHandleRef,
    ) -> Option<reify_shell_extract::MinWallThickness> {
        // γ's BRep→Mesh→Voxel→SampledField recipe (None on degradation).
        let sdf = self.realize_solid_sdf(subject)?;

        // Derive h from the realized grid's own spacing — decouples δ's
        // honest-floor from α's OpenVDB voxel_size default (PRD §4 D decision
        // on explicit-h parameter; deferred e2e gate in η=4427).
        let h = sdf.spacing.iter().copied().fold(f64::INFINITY, f64::min);

        // Map Err (structurally invalid SDF) → None (D5).
        reify_shell_extract::min_wall_thickness(&sdf, h).ok()
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

    // ── shared fixtures for cfg-independent mock chain ────────────────────────

    /// Closed box mesh (±1.0 mm per axis, 12 triangles).
    /// Mirrors the `box_2mm` fixture in `realize_solid_sdf.rs` tests.
    fn box_2mm() -> reify_ir::Mesh {
        #[rustfmt::skip]
        let v: Vec<f32> = vec![
            -1.0, -1.0, -1.0,  1.0, -1.0, -1.0,  1.0,  1.0, -1.0, -1.0,  1.0, -1.0,
            -1.0, -1.0,  1.0,  1.0, -1.0,  1.0,  1.0,  1.0,  1.0, -1.0,  1.0,  1.0,
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

    /// Mock source kernel whose `tessellate` returns `box_2mm()`.
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
        // ingest_mesh / densify_grid_to_sampled: inherit defaults → Err.
    }

    /// Mock "openvdb" kernel: `ingest_mesh` → `Ok(handle)`;
    /// `densify_grid_to_sampled` → `Ok(invalid_sdf)` with zero spacing,
    /// which causes `min_wall_thickness` to return `Err(InvalidAxisGeometry)`.
    ///
    /// Used to test the `.ok()` Err→None arm of `measure_min_wall` (D5).
    struct DensifyInvalidFieldKernel;
    impl reify_ir::GeometryKernel for DensifyInvalidFieldKernel {
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
        /// Returns `Ok(sdf)` where `sdf` has `spacing = [0.0, 0.0, 0.0]` —
        /// a structurally-invalid field that triggers
        /// `MedialError::InvalidAxisGeometry` inside `min_wall_thickness`.
        fn densify_grid_to_sampled(
            &mut self,
            _handle: reify_ir::GeometryHandleId,
        ) -> Result<reify_ir::SampledField, reify_ir::QueryError> {
            Ok(reify_ir::SampledField {
                name: "invalid-zero-spacing".to_string(),
                kind: reify_ir::SampledGridKind::Regular3D,
                bounds_min: vec![0.0, 0.0, 0.0],
                bounds_max: vec![0.0, 0.0, 0.0],
                // INVALID: spacing = 0 → compute_medial_mask returns
                // Err(InvalidAxisGeometry) → min_wall_thickness Err → .ok() → None.
                spacing: vec![0.0, 0.0, 0.0],
                axis_grids: vec![vec![0.0], vec![0.0], vec![0.0]],
                interpolation: reify_ir::InterpolationKind::Linear,
                data: vec![0.0],
                oob_emitted: std::sync::atomic::AtomicBool::new(false),
            })
        }
    }

    // ── step-5 RED: degradation contract, cfg-independent ────────────────────
    //
    // References `engine.measure_min_wall(subject)` which does NOT exist yet —
    // compile-fails (RED) until step-6 adds the impl block.

    /// Subject + source kernel present, but no openvdb kernel registered
    /// → `realize_solid_sdf` returns `None` → `measure_min_wall` must also
    /// return `None` (D5: no fabricated number, no panic).
    /// cfg-independent (no OpenVDB needed).
    #[test]
    fn measure_min_wall_no_openvdb_kernel_returns_none() {
        let mut engine = make_engine();

        // Insert a source kernel under "occt" (tessellate returns a minimal
        // mesh — the openvdb-absence guard fires before tessellation).
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        engine.default_kernel_name = Some("occt".to_string());

        // Seed a resolvable BRep subject.
        let r0 = RealizationNodeId::new("solid-delta-1", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(7));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(7)),
        };

        // No kernel registered under openvdb_kernel_name() → realize_solid_sdf
        // returns None → measure_min_wall must propagate None (D5).
        assert!(
            engine.measure_min_wall(subject).is_none(),
            "measure_min_wall with no openvdb kernel must return None \
             (D5: no fabricated number)"
        );
    }

    // ── Err→None path: min_wall_thickness Err maps to None (D5) ─────────────
    //
    // Tests the `.ok()` branch: realize_solid_sdf returns Some(sdf) but
    // min_wall_thickness(sdf, h) returns Err (structurally-invalid field) →
    // measure_min_wall must return None (D5: no fabricated number, no panic).
    // cfg-independent.

    /// `realize_solid_sdf` returns `Some(sdf)` (chain succeeds through ingest)
    /// but the returned `SampledField` has zero spacing — structurally invalid,
    /// causing `min_wall_thickness` to return `Err(InvalidAxisGeometry)`.
    /// The `.ok()` in `measure_min_wall` maps that `Err` to `None` (D5).
    #[test]
    fn measure_min_wall_invalid_field_err_maps_to_none() {
        let mut engine = make_engine();

        // Source kernel: tessellate returns box_2mm() mesh.
        engine
            .geometry_kernels
            .insert("occt".to_string(), Box::new(TessellatingBoxKernel));
        engine.default_kernel_name = Some("occt".to_string());

        // "OpenVDB" stub: ingest_mesh → Ok(handle), densify → Ok(invalid_sdf).
        // The invalid sdf (spacing=0) makes min_wall_thickness return Err.
        let openvdb_name = crate::kernel_registry::openvdb_kernel_name();
        engine.geometry_kernels.insert(
            openvdb_name.to_string(),
            Box::new(DensifyInvalidFieldKernel),
        );

        // Seed a resolvable BRep subject.
        let r0 = RealizationNodeId::new("delta-invalid-field", 0);
        engine
            .realization_handles
            .insert(r0.clone(), GeometryHandleId(1));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(GeometryHandleId(1)),
        };

        // realize_solid_sdf → Some(invalid_sdf); min_wall_thickness → Err;
        // .ok() → None (D5: Err path mapped to None, no panic).
        assert!(
            engine.measure_min_wall(subject).is_none(),
            "measure_min_wall with structurally-invalid SDF (zero spacing) must \
             return None — Err from min_wall_thickness maps to None via .ok() (D5)"
        );
    }
}
