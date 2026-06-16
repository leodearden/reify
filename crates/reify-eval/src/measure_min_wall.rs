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
        let h = sdf
            .spacing
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);

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
            kernel_handle: GeometryHandleId(7),
        };

        // No kernel registered under openvdb_kernel_name() → realize_solid_sdf
        // returns None → measure_min_wall must propagate None (D5).
        assert!(
            engine.measure_min_wall(subject).is_none(),
            "measure_min_wall with no openvdb kernel must return None \
             (D5: no fabricated number)"
        );
    }
}
