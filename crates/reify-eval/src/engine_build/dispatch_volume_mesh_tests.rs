    use super::*;
    use reify_ir::{ElementOrderTag, GeometryError, VolumeConnectivity, VolumeMesh};
    use reify_solver_elastic::{
        Mesh2d, Mesh2dError, Mesh2dReport, SweepError, SweepParams, SweptMesh3d,
    };

    fn make_empty_volume_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    fn make_swept_mesh(layers: usize) -> SweptMesh3d {
        use reify_solver_elastic::SweptConnectivity;
        SweptMesh3d {
            vertices: vec![],
            connectivity: SweptConnectivity::Wedge { indices: vec![] },
            layers,
        }
    }

    fn make_mesh2d_report() -> Mesh2dReport {
        Mesh2dReport {
            mesh: Mesh2d::Triangle {
                vertices: vec![],
                indices: vec![],
            },
            recombine_attempted: false,
            recombine_quality_ok: true,
        }
    }

    fn extrude_kind() -> crate::sweep_classifier::SweptKind {
        use reify_ir::Value;
        crate::sweep_classifier::SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        }
    }

    // ── Step-1: compile-time surface pin ─────────────────────────────────

    /// Compile-time surface pin: names `VolumeMeshOutcome::Tet` and `::Swept`,
    /// and verifies `dispatch_volume_mesh`'s generic signature including the
    /// new `ops`/`handles` slice parameters.  A rename, signature drift, or
    /// removal of either variant breaks compilation before any behavioural test.
    // ── Step-3: force_tet short-circuit ──────────────────────────────────

    #[test]
    fn dispatch_force_tet_always_calls_tet_path_regardless_of_swept_kind() {
        let kind = extrude_kind();
        let result = dispatch_volume_mesh(
            Some(&kind),
            true, // force_tet
            true, // require_hex_wedge (should be ignored when force_tet)
            &[],  // ops — not reached before force_tet short-circuit
            &[],  // handles
            |_swept| unreachable!("gmsh_2d must not be called when force_tet=true"),
            |_params, _mesh| unreachable!("sweep_step must not be called when force_tet=true"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result, Ok(VolumeMeshOutcome::Tet(_))),
            "force_tet=true must return Tet regardless of swept_kind; got {result:?}"
        );
    }

    // ── Step-5: None swept_kind + !require_hex_wedge → tet fallback ──────

    #[test]
    fn dispatch_no_swept_kind_returns_tet_when_not_require_hex_wedge() {
        let result = dispatch_volume_mesh(
            None,
            false, // force_tet
            false, // require_hex_wedge
            &[],   // ops
            &[],   // handles
            |_swept| unreachable!("gmsh_2d must not be called when swept_kind=None"),
            |_params, _mesh| unreachable!("sweep_step must not be called when swept_kind=None"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result, Ok(VolumeMeshOutcome::Tet(_))),
            "swept_kind=None + require_hex_wedge=false must return Tet; got {result:?}"
        );
    }

    // ── Step-7: None swept_kind + require_hex_wedge → error ──────────────

    #[test]
    fn dispatch_no_swept_kind_errors_when_require_hex_wedge() {
        let result = dispatch_volume_mesh(
            None,
            false, // force_tet
            true,  // require_hex_wedge
            &[],   // ops
            &[],   // handles
            |_swept| unreachable!("gmsh_2d must not be called"),
            |_params, _mesh| unreachable!("sweep_step must not be called"),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("body not swept"),
                    "error message must contain \"body not swept\"; got: {msg}"
                );
            }
            other => panic!("expected Err(OperationFailed(\"body not swept\")), got {other:?}"),
        }
    }

    // ── Step-9: Some(swept) happy path → Swept ───────────────────────────
    // Also asserts that the SweepParams delivered to sweep_step match the
    // SweptKind's fields — pins the params contract that dispatch advertises.

    #[test]
    fn dispatch_swept_kind_happy_path_returns_swept_and_pins_sweep_params() {
        let kind = extrude_kind(); // Extrude { axis: [0,0,1], length: Value::length(0.01) }
        let result = dispatch_volume_mesh(
            Some(&kind),
            false, // force_tet
            false, // require_hex_wedge
            &[],   // ops — Extrude arm does not need them
            &[],   // handles
            |_swept| Ok(make_mesh2d_report()),
            |params, _mesh| {
                // Assert that the SweepParams delivered to sweep_step are correct
                // for the Extrude arm (axis forwarded verbatim, length = 0.01).
                match params {
                    SweepParams::Extrude { axis, length } => {
                        assert_eq!(*axis, [0.0, 0.0, 1.0], "axis must be [0,0,1]");
                        assert!(
                            (length - 0.01).abs() < 1e-12,
                            "length must be 0.01; got {length}"
                        );
                    }
                    other => panic!("expected SweepParams::Extrude, got {other:?}"),
                }
                Ok(make_swept_mesh(2))
            },
            || unreachable!("tet_path must not be called on the swept happy path"),
        );
        match result {
            Ok(VolumeMeshOutcome::Swept(mesh3d)) => {
                assert_eq!(
                    mesh3d.layers, 2,
                    "swept mesh must have the layers returned by sweep_step"
                );
            }
            other => panic!("expected Ok(Swept(mesh3d)) with layers=2, got {other:?}"),
        }
    }

    // ── Step-11: swept failure + !require_hex_wedge → tet fallback ───────

    #[test]
    fn dispatch_swept_failure_falls_back_to_tet_when_not_require_hex_wedge() {
        let kind = extrude_kind();

        // Subcase A: gmsh_2d fails
        let result_a = dispatch_volume_mesh(
            Some(&kind),
            false,
            false,
            &[],
            &[], // ops, handles
            |_swept| Err(Mesh2dError::DegenerateBoundary),
            |_params, _mesh| unreachable!("sweep_step must not be called when gmsh_2d fails"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result_a, Ok(VolumeMeshOutcome::Tet(_))),
            "gmsh_2d failure + require_hex_wedge=false must fall back to Tet; got {result_a:?}"
        );

        // Subcase B: sweep_step fails
        let result_b = dispatch_volume_mesh(
            Some(&kind),
            false,
            false,
            &[],
            &[], // ops, handles
            |_swept| Ok(make_mesh2d_report()),
            |_params, _mesh| Err(SweepError::DegenerateAxis),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result_b, Ok(VolumeMeshOutcome::Tet(_))),
            "sweep_step failure + require_hex_wedge=false must fall back to Tet; got {result_b:?}"
        );
    }

    // ── Step-13: swept failure + require_hex_wedge → error ───────────────

    #[test]
    fn dispatch_swept_failure_errors_when_require_hex_wedge() {
        let kind = extrude_kind();

        // Subcase A: gmsh_2d fails
        let result_a = dispatch_volume_mesh(
            Some(&kind),
            false,
            true, // require_hex_wedge
            &[],
            &[], // ops, handles
            |_swept| Err(Mesh2dError::DegenerateBoundary),
            |_params, _mesh| unreachable!("sweep_step must not be called when gmsh_2d fails"),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result_a {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("swept hex/wedge path failed"),
                    "subcase A error must contain \"swept hex/wedge path failed\"; got: {msg}"
                );
            }
            other => panic!("subcase A: expected Err(OperationFailed), got {other:?}"),
        }

        // Subcase B: sweep_step fails
        let result_b = dispatch_volume_mesh(
            Some(&kind),
            false,
            true, // require_hex_wedge
            &[],
            &[], // ops, handles
            |_swept| Ok(make_mesh2d_report()),
            |_params, _mesh| Err(SweepError::DegenerateMagnitude),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result_b {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("swept hex/wedge path failed"),
                    "subcase B error must contain \"swept hex/wedge path failed\"; got: {msg}"
                );
            }
            other => panic!("subcase B: expected Err(OperationFailed), got {other:?}"),
        }
    }

    #[allow(dead_code, unreachable_code)]
    fn _surface_pin() {
        // Name both variants — a rename or variant removal breaks compilation.
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Tet(todo!()); // ptodo:allow exhaustiveness/stub arm - not tracked debt
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Swept(todo!()); // ptodo:allow exhaustiveness/stub arm - not tracked debt
        // Verify the full signature including the new ops/handles slice parameters
        // via function-item to function-pointer coercion.
        type DispatchVolumeMeshFn = fn(
            Option<&SweptKind>,
            bool,
            bool,
            &[GeometryOp],
            &[GeometryHandleId],
            fn(&SweptKind) -> Result<Mesh2dReport, Mesh2dError>,
            fn(&SweepParams, &Mesh2d) -> Result<SweptMesh3d, SweepError>,
            fn() -> Result<VolumeMesh, GeometryError>,
        ) -> Result<VolumeMeshOutcome, GeometryError>;
        let _: DispatchVolumeMeshFn = dispatch_volume_mesh::<_, _, _>;
    }
