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
// (engine_build.rs:4899-4970) directly.

impl crate::Engine {
    // Method lands here in step-2 / step-4.
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
        engine.realization_handles.insert(r0.clone(), GeometryHandleId(7));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId(7),
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
            kernel_handle: GeometryHandleId::INVALID,
        };

        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "realize_solid_sdf with unresolvable subject must return None"
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
        engine
            .geometry_kernels
            .insert("occt-stub-source".to_string(), Box::new(MockGeometryKernel::new()));
        engine.default_kernel_name = Some("occt-stub-source".to_string());

        let r0 = RealizationNodeId::new("stub-solid", 0);
        engine.realization_handles.insert(r0.clone(), GeometryHandleId(42));
        let subject = GeometryHandleRef {
            realization_ref: r0,
            upstream_values_hash: [0u8; 32],
            kernel_handle: GeometryHandleId(42),
        };

        assert!(
            engine.realize_solid_sdf(subject).is_none(),
            "cfg(not(has_openvdb)) realize_solid_sdf must return None — no fabricated field"
        );
    }
}
