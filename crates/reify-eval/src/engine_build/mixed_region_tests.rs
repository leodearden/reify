    use super::*;
    use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};
    use reify_shell_extract::{MidSurfaceMesh, ShellTetInterface};

    /// Small shell mesh: 3 vertices, 1 triangle, thickness len 3. Vertex 0 sits
    /// at the origin (the unique nearest vertex to `location = [0,0,0]`).
    fn make_shell_mesh() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![0.1, 0.1, 0.1],
        }
    }

    /// Tet mesh for interface tying: a through-thickness triple straddling the
    /// origin along +z (top z=+1, mid z=0, bot z=−1) plus a far 4th node that
    /// is excluded from the 3 nearest to `location`.
    fn make_tie_tet_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 1.0, // node 0 — top (z = +1)
                0.0, 0.0, 0.0, // node 1 — mid (z =  0, at location)
                0.0, 0.0, -1.0, // node 2 — bot (z = −1)
                9.0, 9.0, 9.0, // node 3 — far (not among the 3 nearest)
            ],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![0, 1, 2, 3],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    /// Small P1 tet mesh: 4 vertices = 1 tet (placed a unit above the shell).
    fn make_p1_tet_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 1.0, // node 0
                1.0, 0.0, 1.0, // node 1
                0.0, 1.0, 1.0, // node 2
                0.0, 0.0, 2.0, // node 3
            ],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![0, 1, 2, 3],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    // ── Step 9: unified-mesh merge (no MPCs) ─────────────────────────────────

    /// Merging a shell mesh and a tet mesh with no interfaces concatenates the
    /// node lists (shell first, tet appended as f64), emits one `Shell` element
    /// per triangle and one `Tet` element per tet (connectivity offset by the
    /// shell node count), and produces no MPC rows.
    #[test]
    fn build_mixed_region_mesh_merges_shell_then_tet_nodes_and_elements() {
        let shell = make_shell_mesh();
        let tet = make_p1_tet_mesh();
        let result = build_mixed_region_mesh(&shell, &tet, &[])
            .expect("merge with no interfaces should succeed");

        let n_shell = shell.vertices.len(); // 3
        let n_tet = tet.vertices.len() / 3; // 4
        assert_eq!(
            result.nodes.len(),
            n_shell + n_tet,
            "merged node count = shell vertices + tet vertices"
        );
        // Shell nodes preserved verbatim, first.
        assert_eq!(result.nodes[0], [0.0, 0.0, 0.0]);
        assert_eq!(result.nodes[1], [1.0, 0.0, 0.0]);
        assert_eq!(result.nodes[2], [0.0, 1.0, 0.0]);
        // Tet vertices appended (f32 → f64) after the shell nodes.
        assert_eq!(result.nodes[n_shell], [0.0, 0.0, 1.0]);
        assert_eq!(result.nodes[n_shell + 3], [0.0, 0.0, 2.0]);

        // Elements: 1 shell triangle + 1 tet.
        assert_eq!(result.elements.len(), 2, "one shell + one tet element");
        let shell_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Shell)
            .collect();
        assert_eq!(shell_elems.len(), 1, "one shell element");
        assert_eq!(
            shell_elems[0].connectivity,
            vec![0usize, 1, 2],
            "shell connectivity = triangle vertex indices"
        );
        let tet_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Tet)
            .collect();
        assert_eq!(tet_elems.len(), 1, "one tet element");
        assert_eq!(
            tet_elems[0].connectivity,
            vec![n_shell, n_shell + 1, n_shell + 2, n_shell + 3],
            "tet connectivity offset by n_shell_nodes"
        );

        // No interfaces → no MPC rows.
        assert!(result.mpc_rows.is_empty(), "no interfaces → no MPC rows");
    }

    /// Empty shell + empty tet + no interfaces → an all-empty `MixedRegionMesh`.
    #[test]
    fn build_mixed_region_mesh_on_empty_inputs_is_all_empty() {
        let empty_shell = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let empty_tet = VolumeMesh {
            vertices: vec![],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        };
        let result = build_mixed_region_mesh(&empty_shell, &empty_tet, &[])
            .expect("empty merge should succeed");
        assert!(result.nodes.is_empty(), "no nodes");
        assert!(result.elements.is_empty(), "no elements");
        assert!(result.mpc_rows.is_empty(), "no MPC rows");
    }

    // ── Step 11: interface MPC wiring (D=6 layout) ───────────────────────────

    /// One interface ties the nearest shell vertex to the resolved tet
    /// top/mid/bot triple, producing `MpcRow::shell_tet_tying`'s 6 rows under
    /// the global D=6 DOF layout (`6·node + axis`, shell nodes first, tet nodes
    /// offset by `n_shell`).
    #[test]
    fn build_mixed_region_mesh_wires_interface_mpc_rows_in_d6_layout() {
        let shell = make_shell_mesh();
        let tet = make_tie_tet_mesh();
        let n_shell = shell.vertices.len(); // 3
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let result = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&interface))
            .expect("interface wiring should succeed");

        // shell_tet_tying emits exactly 6 rows.
        assert_eq!(result.mpc_rows.len(), 6, "one interface → 6 MPC rows");

        // Resolved tie nodes (unified indices) under the fixture geometry:
        //   shell tie node = nearest shell vertex to [0,0,0] = shell node 0.
        //   tet top/mid/bot = tet locals 0/1/2 (z = +1/0/−1) → unified 3/4/5.
        let shell_n = 0usize;
        let tet_mid = n_shell + 1; // 4

        // The three displacement-matching rows: u_shell_a − u_tet_mid_a = 0,
        // pivot at the shell disp DOF (6·shell_n + a) with coeffs [1, −1] and
        // the tet-mid disp DOF (6·tet_mid + a) as the second term.
        for a in 0..3 {
            let shell_disp = 6 * shell_n + a; // 0,1,2
            let tet_mid_dof = 6 * tet_mid + a; // 24,25,26
            let row = result
                .mpc_rows
                .iter()
                .find(|r| r.dofs == vec![shell_disp, tet_mid_dof])
                .unwrap_or_else(|| {
                    panic!(
                        "missing displacement row for axis {a}: dofs [{shell_disp}, {tet_mid_dof}]"
                    )
                });
            assert_eq!(
                row.coeffs,
                vec![1.0, -1.0],
                "displacement-matching row coeffs for axis {a}"
            );
            assert_eq!(row.rhs, 0.0, "homogeneous tie (rhs = 0) for axis {a}");
        }

        // D=6 sanity: every DOF index lies in the unified 6·node space.
        let n_total = result.nodes.len(); // 7
        for row in &result.mpc_rows {
            for &d in &row.dofs {
                assert!(
                    d < 6 * n_total,
                    "DOF {d} must lie within the D=6 space (6 · {n_total} nodes)"
                );
            }
        }

        // The rotation rows must reference the shell tie node's rotation DOFs
        // (6·shell_n + 3..5) — confirming the shell side contributes rotations.
        let shell_rot: Vec<usize> = (3..6).map(|axis| 6 * shell_n + axis).collect(); // [3,4,5]
        assert!(
            result
                .mpc_rows
                .iter()
                .any(|r| r.dofs.iter().any(|d| shell_rot.contains(d))),
            "a rotation row must reference a shell rotation DOF (6·n + 3..5)"
        );
    }

    /// An interface whose tet side has no nodes cannot resolve its tie nodes →
    /// `InterfaceResolutionFailed` tagged with the interface index.
    #[test]
    fn build_mixed_region_mesh_errors_when_interface_tie_nodes_unresolvable() {
        let shell = make_shell_mesh();
        let empty_tet = VolumeMesh {
            vertices: vec![],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        };
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let err = build_mixed_region_mesh(&shell, &empty_tet, std::slice::from_ref(&interface))
            .expect_err("an interface against an empty tet mesh must fail to resolve");
        assert_eq!(
            err,
            MixedRegionError::InterfaceResolutionFailed { interface_index: 0 },
            "error must name the offending interface index"
        );
    }

    // ── Amendment: P2 chunking + additional error-path coverage ──────────────

    /// A P2 tet (10 nodes/element) is chunked by 10 and offset by `n_shell`,
    /// exercising the `ElementOrderTag::P2` branch of `nodes_per_tet` that the
    /// P1 fixtures leave uncovered. Two tets confirm both the chunk size and the
    /// per-element offset.
    #[test]
    fn build_mixed_region_mesh_chunks_p2_tet_by_ten_nodes() {
        let shell = make_shell_mesh();
        let n_shell = shell.vertices.len(); // 3
        // 20 tet vertices = two P2 tets; the positions are irrelevant to the
        // connectivity chunking under test (kept clear of any interface).
        let mut vertices = Vec::new();
        for i in 0..20 {
            vertices.push(i as f32);
            vertices.push(0.0);
            vertices.push(5.0);
        }
        let tet = VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices: (0..20u32).collect(),
                order: ElementOrderTag::P2,
            },
            normals: None,
            boundary: None,
        };

        let result = build_mixed_region_mesh(&shell, &tet, &[])
            .expect("P2 merge with no interfaces should succeed");

        assert_eq!(result.nodes.len(), n_shell + 20, "3 shell + 20 tet nodes");
        let tet_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Tet)
            .collect();
        assert_eq!(tet_elems.len(), 2, "20 P2 indices → two 10-node tets");
        assert_eq!(
            tet_elems[0].connectivity,
            (0..10).map(|m| n_shell + m).collect::<Vec<usize>>(),
            "first P2 tet: local nodes 0..10 offset by n_shell",
        );
        assert_eq!(
            tet_elems[1].connectivity,
            (10..20).map(|m| n_shell + m).collect::<Vec<usize>>(),
            "second P2 tet: local nodes 10..20 offset by n_shell",
        );
    }

    /// An interface against an empty-shell + non-empty-tet mesh cannot resolve
    /// its shell tie node (`nearest_node_index` over zero shell nodes is `None`)
    /// → `InterfaceResolutionFailed`. Complements the empty-tet case by covering
    /// the shell-side `None` branch.
    #[test]
    fn build_mixed_region_mesh_errors_when_shell_side_empty() {
        let empty_shell = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let tet = make_tie_tet_mesh();
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let err = build_mixed_region_mesh(&empty_shell, &tet, std::slice::from_ref(&interface))
            .expect_err("an interface against an empty shell mesh must fail to resolve");
        assert_eq!(
            err,
            MixedRegionError::InterfaceResolutionFailed { interface_index: 0 },
            "empty shell side → no candidate tie node",
        );
    }

    /// An interface whose geometry violates `MpcRow::shell_tet_tying`'s
    /// preconditions (non-unit `normal`, or non-positive `thickness`) is
    /// surfaced as `InvalidInterfaceGeometry` instead of panicking in the
    /// downstream assertion. Guards the seam's direct-call contract — the
    /// partition layer normally guarantees these invariants, but the seam is
    /// `pub(crate)` and reachable with an arbitrary `ShellTetInterface`.
    #[test]
    fn build_mixed_region_mesh_errors_on_invalid_interface_geometry() {
        let shell = make_shell_mesh();
        let tet = make_tie_tet_mesh();

        // Case 1: non-unit normal (|n| = 2) — would trip the unit-normal assert.
        let non_unit = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 2.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };
        let err = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&non_unit))
            .expect_err("a non-unit interface normal must be rejected");
        assert_eq!(
            err,
            MixedRegionError::InvalidInterfaceGeometry { interface_index: 0 },
            "non-unit normal → InvalidInterfaceGeometry",
        );

        // Case 2: non-positive thickness — would trip the positive-thickness assert.
        let bad_thickness = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.0,
            location: [0.0, 0.0, 0.0],
        };
        let err = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&bad_thickness))
            .expect_err("a non-positive interface thickness must be rejected");
        assert_eq!(
            err,
            MixedRegionError::InvalidInterfaceGeometry { interface_index: 0 },
            "non-positive thickness → InvalidInterfaceGeometry",
        );
    }

    // ── op_accepts_repr / classify_op_input_reprs unit tests (task 4049) ────────

    /// Pins the `(Operation, ReprKind)` input-repr classifier table for the
    /// consumer-demand backward pass (PRD §3a.4, task 4049).
    ///
    /// Asserts the following classifier contract:
    ///
    /// - Boolean* and Transform* and Pattern* accept BOTH BRep and Mesh.
    /// - Modify* (Fillet/Chamfer/Shell/Draft/Thicken) and Sweep* (8 variants)
    ///   accept BRep but NOT Mesh.
    /// - `Operation::Convert { from: ReprKind::BRep }` is classified (accepts
    ///   at least one repr).
    ///
    /// RED before step-2 impl: `op_accepts_repr` / `classify_op_input_reprs`
    /// do not exist yet.
    #[test]
    fn op_accepts_repr_classifier_table() {
        use reify_ir::{Operation, ReprKind};

        // ── Boolean* ─────────────────────────────────────────────────────────
        for bool_op in [
            Operation::BooleanUnion,
            Operation::BooleanDifference,
            Operation::BooleanIntersection,
        ] {
            assert!(
                op_accepts_repr(&bool_op, ReprKind::BRep),
                "{bool_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&bool_op, ReprKind::Mesh),
                "{bool_op:?} must accept Mesh"
            );
        }

        // ── Modify* — BRep-only consumer ─────────────────────────────────────
        for mod_op in [
            Operation::ModifyFillet,
            Operation::ModifyChamfer,
            Operation::ModifyShell,
            Operation::ModifyDraft,
            Operation::ModifyThicken,
            Operation::ModifyZoneSlab,
            Operation::ModifyOffsetSolid,
        ] {
            assert!(
                op_accepts_repr(&mod_op, ReprKind::BRep),
                "{mod_op:?} must accept BRep"
            );
            assert!(
                !op_accepts_repr(&mod_op, ReprKind::Mesh),
                "{mod_op:?} must NOT accept Mesh (BRep-only consumer)"
            );
        }

        // ── Sweep* — BRep-only consumer ──────────────────────────────────────
        for sweep_op in [
            Operation::SweepLoft,
            Operation::SweepExtrude,
            Operation::SweepRevolve,
            Operation::SweepSweep,
            Operation::SweepExtrudeSymmetric,
            Operation::SweepExtrudeInfinite,
            Operation::SweepSweepGuided,
            Operation::SweepLoftGuided,
            Operation::SweepPipe,
        ] {
            assert!(
                op_accepts_repr(&sweep_op, ReprKind::BRep),
                "{sweep_op:?} must accept BRep"
            );
            assert!(
                !op_accepts_repr(&sweep_op, ReprKind::Mesh),
                "{sweep_op:?} must NOT accept Mesh (BRep-only consumer)"
            );
        }

        // ── Transform* ───────────────────────────────────────────────────────
        for transform_op in [
            Operation::TransformTranslate,
            Operation::TransformRotate,
            Operation::TransformScale,
            Operation::TransformRotateAround,
        ] {
            assert!(
                op_accepts_repr(&transform_op, ReprKind::BRep),
                "{transform_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&transform_op, ReprKind::Mesh),
                "{transform_op:?} must accept Mesh"
            );
        }

        // ── Pattern* ─────────────────────────────────────────────────────────
        for pattern_op in [
            Operation::PatternLinear,
            Operation::PatternCircular,
            Operation::PatternMirror,
            Operation::PatternLinear2D,
            Operation::PatternArbitrary,
        ] {
            assert!(
                op_accepts_repr(&pattern_op, ReprKind::BRep),
                "{pattern_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&pattern_op, ReprKind::Mesh),
                "{pattern_op:?} must accept Mesh"
            );
        }

        // ── Convert — classified (accepts at least one repr) ─────────────────
        let convert_op = Operation::Convert {
            from: ReprKind::BRep,
        };
        assert!(
            classify_op_input_reprs(&convert_op).is_some(),
            "Convert{{from:BRep}} must be classified (Some)"
        );

        // ── Surface (isosurface / marching-cubes, task 4999) — Voxel-only ─────
        assert_eq!(
            classify_op_input_reprs(&Operation::Surface),
            Some(&[ReprKind::Voxel][..]),
            "Surface must classify as Voxel-only input (marching-cubes consumes a voxel grid)"
        );
        assert!(
            op_accepts_repr(&Operation::Surface, ReprKind::Voxel),
            "Surface must accept Voxel"
        );
        assert!(
            !op_accepts_repr(&Operation::Surface, ReprKind::Mesh),
            "Surface must NOT accept Mesh (Voxel-only consumer)"
        );
        assert!(
            !op_accepts_repr(&Operation::Surface, ReprKind::BRep),
            "Surface must NOT accept BRep (Voxel-only consumer)"
        );
    }

    /// Backward-pass tests "a" and "b" for `compute_demanded_reprs`
    /// (PRD §3a.4, task 4049).
    ///
    /// Fixture A (test a): mesh-terminal BooleanUnion → Mesh demand.
    /// One template with three named realizations:
    ///   realization "a" — Primitive Box (producer)
    ///   realization "b" — Primitive Box (producer)
    ///   realization "u" — Boolean{Union, left:Sub("a"), right:Sub("b")} (terminal)
    /// ExportFormat::Stl (mesh sink) → demand[0][2] == Mesh.
    ///
    /// Fixture B (test b): union-then-Fillet → BRep on union, Mesh on fillet.
    /// Extends fixture A by adding:
    ///   realization "f" — Modify{Fillet, target:Sub("u")} (terminal, mesh sink)
    /// demand[0][2] (union) == BRep (its consumer Fillet is BRep-only).
    /// demand[0][3] (fillet) == Mesh (terminal, mesh sink).
    ///
    /// Also asserts shape alignment with compute_demanded_tols (same
    /// [t_idx][r_idx] outer/inner lengths).
    ///
    /// RED before step-6: `compute_demanded_reprs` does not exist.
    #[test]
    fn compute_demanded_reprs_mesh_terminal_and_fillet_consumer() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        // Shared primitive op used as a leaf source.
        let prim_box = || CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };

        // ── Fixture A: single template, three realizations (a, b, u) ─────────
        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization_named("EntityA_a", 0, "a", vec![prim_box()])
            .realization_named("EntityA_b", 1, "b", vec![prim_box()])
            .realization_named(
                "EntityA_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .build();
        let module_a = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_a"))
            .template(template_a)
            .build();

        // ── Test a: mesh sink → terminal BooleanUnion demands Mesh ───────────
        let result_a = engine.compute_demanded_reprs(&module_a, ExportFormat::Stl);
        assert_eq!(
            result_a.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result_a[0].len(), 3, "template has 3 realizations");
        // demanded_tols alignment: same shape
        assert_eq!(
            result_a.len(),
            engine.compute_demanded_tols(&module_a).len(),
            "outer length must match compute_demanded_tols"
        );
        assert_eq!(
            result_a[0].len(),
            engine.compute_demanded_tols(&module_a)[0].len(),
            "inner length must match compute_demanded_tols"
        );
        assert_eq!(
            result_a[0][2],
            ReprKind::Mesh,
            "terminal BooleanUnion under Stl (mesh sink) must demand Mesh"
        );

        // ── Fixture B: extend with Fillet consuming the union ─────────────────
        let template_b = TopologyTemplateBuilder::new("EntityB")
            .realization_named("EntityB_a", 0, "a", vec![prim_box()])
            .realization_named("EntityB_b", 1, "b", vec![prim_box()])
            .realization_named(
                "EntityB_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .realization_named(
                "EntityB_f",
                3,
                "f",
                vec![CompiledGeometryOp::Modify {
                    kind: ModifyKind::Fillet,
                    target: GeomRef::Sub("u".to_string()),
                    args: vec![],
                }],
            )
            .build();
        let module_b = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_b"))
            .template(template_b)
            .build();

        // ── Test b ────────────────────────────────────────────────────────────
        let result_b = engine.compute_demanded_reprs(&module_b, ExportFormat::Stl);
        assert_eq!(
            result_b.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result_b[0].len(), 4, "template has 4 realizations");
        assert_eq!(
            result_b[0][2],
            ReprKind::BRep,
            "BooleanUnion whose consumer (Fillet) is BRep-only must demand BRep"
        );
        assert_eq!(
            result_b[0][3],
            ReprKind::Mesh,
            "terminal Fillet under Stl (mesh sink) must demand Mesh"
        );
    }

    /// Voxel demand-seeding regression test (task 5000, PRD
    /// `docs/prds/v0_3/voxel-to-mesh-surfacing.md` task β, gap 1).
    ///
    /// Fixture A (Voxel demand): one template, two named realizations:
    ///   realization "s" — Primitive Box (producer)
    ///   realization "shell" — Isosurface{grid: Sub("s")} (terminal, mesh sink)
    /// `Operation::Surface` (the isosurface builtin) is classified Voxel-only
    /// input (`classify_op_input_reprs`), so its operand "s" must demand
    /// `ReprKind::Voxel` — NOT the BRep the pre-β reverse-pass would assign
    /// (Surface does not accept Mesh, so the old code fell through to the
    /// BRep disqualifier). "shell" itself is terminal under Stl (mesh sink)
    /// → Mesh, proving Voxel demand appears ONLY at the operand realization
    /// (intra-template C-3: "only a realization consumed by a Voxel-only-
    /// input op is demanded Voxel").
    ///
    /// Fixture B (C-3 inter-graph regression guard): a byte-identical-shape
    /// sibling template where "shell" consumes "s" via `Transform{Translate}`
    /// (classified `[BRep, Mesh]`, NOT Voxel-only) instead of `Isosurface`.
    /// Asserts the demand vector contains NO `ReprKind::Voxel` anywhere and
    /// matches the classic Mesh/Mesh result — i.e. a non-isosurface graph's
    /// demand is byte-for-byte unchanged by the new Voxel arm.
    ///
    /// RED before step-2: the reverse-pass has no Voxel arm, so the isosurface
    /// operand "s" in fixture A falls to the BRep disqualifier
    /// (`op_accepts_repr(Surface, Mesh) == false`) and `result_a[0][0]` is
    /// `BRep`, not `Voxel`.
    #[test]
    fn compute_demanded_reprs_isosurface_operand_demands_voxel() {
        use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, TransformKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        let prim_box = || CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };

        // ── Fixture A: "s" (Box) → "shell" (Isosurface, terminal) ────────────
        let template_a = TopologyTemplateBuilder::new("EntityVoxA")
            .realization_named("EntityVoxA_s", 0, "s", vec![prim_box()])
            .realization_named(
                "EntityVoxA_shell",
                1,
                "shell",
                vec![CompiledGeometryOp::Isosurface {
                    grid: GeomRef::Sub("s".to_string()),
                    args: vec![],
                }],
            )
            .build();
        let module_a =
            CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_voxel_a"))
                .template(template_a)
                .build();

        let result_a = engine.compute_demanded_reprs(&module_a, ExportFormat::Stl);
        assert_eq!(
            result_a.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result_a[0].len(), 2, "template has 2 realizations");
        assert_eq!(
            result_a[0][0],
            ReprKind::Voxel,
            "Isosurface operand 's' must demand Voxel (Surface is Voxel-only input)"
        );
        assert_eq!(
            result_a[0][1],
            ReprKind::Mesh,
            "terminal Isosurface under Stl (mesh sink) must demand Mesh"
        );

        // ── Fixture B: byte-identical shape, non-Voxel-only consumer ─────────
        // "shell" consumes "s" via Transform{Translate} ([BRep, Mesh]) instead
        // of Isosurface — the C-3 regression guard: Voxel must NOT leak into
        // graphs that never use the isosurface builtin.
        let template_b = TopologyTemplateBuilder::new("EntityVoxB")
            .realization_named("EntityVoxB_s", 0, "s", vec![prim_box()])
            .realization_named(
                "EntityVoxB_shell",
                1,
                "shell",
                vec![CompiledGeometryOp::Transform {
                    kind: TransformKind::Translate,
                    target: GeomRef::Sub("s".to_string()),
                    args: vec![],
                }],
            )
            .build();
        let module_b =
            CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_voxel_b"))
                .template(template_b)
                .build();

        let result_b = engine.compute_demanded_reprs(&module_b, ExportFormat::Stl);
        assert_eq!(result_b[0].len(), 2, "template has 2 realizations");
        assert!(
            result_b[0].iter().all(|&r| r != ReprKind::Voxel),
            "non-isosurface graph must never demand Voxel (C-3): got {:?}",
            result_b[0]
        );
        assert_eq!(
            result_b[0][0],
            ReprKind::Mesh,
            "Transform-consumed producer 's' must demand classic Mesh (unchanged by the Voxel arm)"
        );
        assert_eq!(
            result_b[0][1],
            ReprKind::Mesh,
            "terminal Transform under Stl (mesh sink) must demand Mesh"
        );
    }

    /// Conservative-default test (task 4049 test "c", PRD §3a.4).
    ///
    /// Fixture: one template with two named realizations:
    ///   realization "a" — Primitive Box (producer)
    ///   realization "consumer" — Boolean{Union, left:Sub("a"), right:Sub("missing")}
    ///
    /// "missing" names no realization → unresolvable downstream reference.
    /// This exercises the PRD §3a.4 "Default-rule conservatism" trigger
    /// (downstream realization absent from graph snapshot), which is lumped with
    /// the unclassified-op trigger in the shared conservative code path.
    ///
    /// Expected: realization "a" demands BRep (conservative), and a
    /// `tracing::debug!` event is emitted naming the unresolved reference.
    ///
    /// RED before step-8: step-6 skips unresolved refs without emitting the
    /// debug log, and realization "a" is seen as terminal → incorrectly gets
    /// Mesh demand rather than BRep.
    #[test]
    fn compute_demanded_reprs_conservative_on_unresolved_sub() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CapturingSubscriberBuilder, CompiledModuleBuilder, MockConstraintChecker,
            TopologyTemplateBuilder, prime_tracing_callsite_cache,
        };

        prime_tracing_callsite_cache();

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        let prim_box = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };
        // "missing" is not the name of any realization → unresolved reference.
        let template = TopologyTemplateBuilder::new("EntityC")
            .realization_named("EntityC_a", 0, "a", vec![prim_box])
            .realization_named(
                "EntityC_consumer",
                1,
                "consumer",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("missing".to_string()),
                }],
            )
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_c"))
            .template(template)
            .build();

        let (subscriber, capture) = CapturingSubscriberBuilder::new(tracing::Level::DEBUG)
            .target_prefix("reify_eval::demanded_reprs")
            .build();

        let result = tracing::subscriber::with_default(subscriber, || {
            engine.compute_demanded_reprs(&module, ExportFormat::Stl)
        });

        // Realization "a" has an unresolved downstream consumer → conservative BRep.
        assert_eq!(
            result[0][0],
            ReprKind::BRep,
            "realization 'a' has an unresolved downstream ref 'missing'; \
             must demand BRep (conservative)"
        );

        // A debug event must have been emitted naming the unresolved reference.
        assert!(
            capture.count() >= 1,
            "expected at least one DEBUG event for the unresolved 'missing' reference; \
             got {count}",
            count = capture.count()
        );
        let msgs = capture.messages();
        assert!(
            msgs.iter().any(|m| m.contains("missing")),
            "DEBUG message must mention the unresolved reference name 'missing'; \
             messages: {msgs:?}"
        );
    }

    /// Strum-iterate completeness test (task 4049 test "d", PRD §9 Q10).
    ///
    /// Iterates ALL current `Operation` variants via `strum::IntoEnumIterator`
    /// and asserts every one has an explicit classifier entry. This is the
    /// standing forcing function: a future `Operation` variant auto-appears in
    /// `Operation::iter()` (via the `EnumIter` derive added in pre-1) and
    /// fails this test until consciously classified, making silent omission
    /// impossible.
    ///
    /// Mirrors `compute_demanded_reprs_mesh_terminal_and_fillet_consumer` for
    /// `ExportFormat::ThreeMF` (task 4286 step-5).  The terminal realization
    /// must demand `ReprKind::Mesh` for any mesh-sink format.
    ///
    /// RED before step-6: `ExportFormat::ThreeMF` does not exist.
    #[test]
    fn compute_demanded_reprs_three_mf_demands_mesh() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        let prim_box = || CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };

        // One template: Box × Box → BooleanUnion (terminal).
        let template = TopologyTemplateBuilder::new("T3mf")
            .realization_named("T3mf_a", 0, "a", vec![prim_box()])
            .realization_named("T3mf_b", 1, "b", vec![prim_box()])
            .realization_named(
                "T3mf_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_3mf"))
            .template(template)
            .build();

        let result = engine.compute_demanded_reprs(&module, ExportFormat::ThreeMF);
        assert_eq!(result.len(), 1, "one template → one outer entry");
        assert_eq!(result[0].len(), 3, "three realizations");
        assert_eq!(
            result[0][2],
            ReprKind::Mesh,
            "terminal BooleanUnion under ThreeMF (mesh sink) must demand Mesh"
        );
    }

    /// Static VolumeMesh-demand propagation (task 4743 step-8, PRD §10 OQ-1).
    ///
    /// A registered VolumeMesh-demanding `@optimized` consumer whose geometry
    /// `ValueRef` arg names a producing realization overrides that realization's
    /// demand to [`ReprKind::VolumeMesh`] inside `compute_demanded_reprs` — the
    /// module-static OQ-1 resolution (demand is computed before compute nodes
    /// dispatch, so a runtime read of `realization_inputs` is unavailable; the
    /// override rides the same consumer→producer name-match the `geometry_cell`
    /// rule uses at graph.rs:371).
    ///
    /// Fixture: structure `S` with `let body = box(...)` (named realization
    /// "body" + a `Type::Geometry` value cell "body") consumed by
    /// `let _p = vm_probe(body)` — a value cell whose `default_expr` is a
    /// `UserFunctionCall` over `body`, where `vm_probe` is
    /// `@optimized("<target>")`.
    ///
    /// Three facts pinned:
    ///   (A) WITH `test::vm-demand` registered → `body` demands VolumeMesh.
    ///   (B) Same module, NO registration → `body` stays at the Step-terminal
    ///       BRep default (the override is gated on registration).
    ///   (C) A consumer whose target is NOT registered does not override
    ///       (no false positive), even while an unrelated target is registered.
    ///
    /// RED before step-8: `demanded_reprs_for_template` ignores `value_cells`
    /// entirely, so `body` is seen as a terminal realization and demands BRep
    /// under `ExportFormat::Step` — assertion (A) fails (BRep != VolumeMesh).
    #[test]
    fn compute_demanded_reprs_volume_mesh_override_on_registered_consumer() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{ContentHash, ModulePath, Type, ValueCellId};
        use reify_ir::{
            CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, ExportFormat,
            ReprKind, Value,
        };
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        // `@optimized("<target>")` consumer fn `vm_probe(g: Geometry) -> Geometry`.
        // `optimized_target` is the @optimized target the static demand pass
        // resolves `function_name` to; `None`/unregistered → no override.
        let consumer_fn = |target: Option<&str>| {
            let params = vec![("g".to_string(), Type::Geometry)];
            CompiledFunction {
                name: "vm_probe".to_string(),
                doc: None,
                is_pub: false,
                param_defaults: CompiledFunction::no_defaults_for(&params),
                params,
                return_type: Type::Geometry,
                body: CompiledFnBody {
                    let_bindings: vec![],
                    result_expr: CompiledExpr::value_ref(
                        ValueCellId::new("vm_probe", "g"),
                        Type::Geometry,
                    ),
                },
                content_hash: ContentHash::of(b"vm_probe_fn"),
                annotations: vec![],
                optimized_target: target.map(String::from),
                type_params: vec![],
            }
        };

        // Build module: structure `S` consuming `body` via `vm_probe`, with the
        // consumer fn carrying `fn_target` as its @optimized target.
        let build_module = |fn_target: Option<&str>| {
            // `body` geometry cell default — a non-UserFunctionCall placeholder
            // (the demand pass skips it; the real box op lives on the realization).
            let body_default = CompiledExpr::literal(Value::Int(0), Type::Int);
            // `_p` consumer: vm_probe(body) where `body` is a Geometry ValueRef.
            let probe_arg =
                CompiledExpr::value_ref(ValueCellId::new("S", "body"), Type::Geometry);
            let probe_call = CompiledExpr {
                kind: CompiledExprKind::UserFunctionCall {
                    function_name: "vm_probe".to_string(),
                    args: vec![probe_arg],
                },
                result_type: Type::Geometry,
                content_hash: ContentHash::of(b"vm_probe_call"),
            };
            let template = TopologyTemplateBuilder::new("S")
                .let_binding("S", "body", Type::Geometry, body_default)
                .let_binding("S", "_p", Type::Geometry, probe_call)
                .realization_named(
                    "S",
                    0,
                    "body",
                    vec![CompiledGeometryOp::Primitive {
                        kind: PrimitiveKind::Box,
                        args: vec![],
                    }],
                )
                .build();
            CompiledModuleBuilder::new(ModulePath::single("test_vm_demand_propagation"))
                .template(template)
                .function(consumer_fn(fn_target))
                .build()
        };

        // ── (A) registered VolumeMesh-demanding consumer → body → VolumeMesh ──
        let module = build_module(Some("test::vm-demand"));
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_volume_mesh_demand("test::vm-demand");
        let result = engine.compute_demanded_reprs(&module, ExportFormat::Step);
        assert_eq!(result.len(), 1, "one template → one outer entry");
        assert_eq!(result[0].len(), 1, "structure S has one realization (body)");
        assert_eq!(
            result[0][0],
            ReprKind::VolumeMesh,
            "a registered VolumeMesh-demanding consumer over `body` must override \
             body's demand to VolumeMesh (overriding the Step-terminal BRep default)"
        );

        // ── (B) same module, NO registration → body stays at Step BRep ──
        let engine_unreg = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        let result_unreg = engine_unreg.compute_demanded_reprs(&module, ExportFormat::Step);
        assert_eq!(
            result_unreg[0][0],
            ReprKind::BRep,
            "without registration, body is a terminal realization under Step → BRep \
             (no VolumeMesh override)"
        );

        // ── (C) consumer target NOT registered → no false override ──
        let module_other = build_module(Some("test::other"));
        let mut engine_c = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine_c.register_volume_mesh_demand("test::vm-demand"); // a DIFFERENT target
        let result_c = engine_c.compute_demanded_reprs(&module_other, ExportFormat::Step);
        assert_eq!(
            result_c[0][0],
            ReprKind::BRep,
            "a consumer whose @optimized target is not registered VolumeMesh-demanding \
             must not override body's demand (no false positive)"
        );

        // ── (D) cross-template arg aliasing a local realization name → no override ──
        // A registered VolumeMesh-demanding consumer whose geometry arg is a
        // CROSS-template `ValueRef` (`Other.body`) whose bare `member` ("body")
        // coincidentally equals template S's LOCAL realization name MUST NOT
        // override S's local `body` realization. The entity-scope guard
        // (`arg_cell.entity == producing realization's entity`) rejects the
        // alias. WITHOUT the guard the bare-member `name_to_idx` match would
        // falsely promote S.body to VolumeMesh (the asymmetry the reviewer
        // flagged). Mirrors `demanded_reprs_for_template`'s conservative
        // cross-template handling.
        let cross_arg =
            CompiledExpr::value_ref(ValueCellId::new("Other", "body"), Type::Geometry);
        let cross_call = CompiledExpr {
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "vm_probe".to_string(),
                args: vec![cross_arg],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash::of(b"vm_probe_cross_call"),
        };
        let template_d = TopologyTemplateBuilder::new("S")
            .let_binding(
                "S",
                "body",
                Type::Geometry,
                CompiledExpr::literal(Value::Int(0), Type::Int),
            )
            .let_binding("S", "_p", Type::Geometry, cross_call)
            .realization_named(
                "S",
                0,
                "body",
                vec![CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    args: vec![],
                }],
            )
            .build();
        let module_d = CompiledModuleBuilder::new(ModulePath::single("test_vm_demand_cross"))
            .template(template_d)
            .function(consumer_fn(Some("test::vm-demand")))
            .build();
        let mut engine_d = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine_d.register_volume_mesh_demand("test::vm-demand");
        let result_d = engine_d.compute_demanded_reprs(&module_d, ExportFormat::Step);
        assert_eq!(
            result_d[0][0],
            ReprKind::BRep,
            "a CROSS-template geometry ValueRef (Other.body) whose bare member \
             coincidentally equals local realization `body` must NOT override S's \
             local body demand (entity-scope guard); body stays terminal BRep"
        );
    }

    /// Static VolumeMesh-demand resolves stdlib `@optimized` consumers via
    /// `self.functions`, not just `module.functions` (task 5008 GAP A,
    /// step-1).
    ///
    /// A stdlib `@optimized` geometry-consumer (e.g. `solve_elastic_static`)
    /// is never a member of a compiled user module's `functions` table —
    /// `compile_with_stdlib` keeps stdlib fns in a separate prelude, and the
    /// engine only merges prelude + module fns into
    /// `self.functions: Arc<[CompiledFunction]>` (`lib.rs:674`) during
    /// `build()` → `check()` (`merge_functions`), which runs BEFORE
    /// `compute_demanded_reprs` (`build_with_geometry_output`'s
    /// `self.check(module)` precedes its `compute_demanded_reprs` call). This
    /// test reproduces that shape directly, without a real build: the
    /// consumer fn is seeded ONLY into `engine.functions` (crate-private
    /// field; this `tests` module is a descendant of the crate-root module
    /// that declares `Engine`, so direct field assignment is visible here) —
    /// `module.functions` stays empty, exactly as it would for a real stdlib
    /// consumer at demand time.
    ///
    /// RED before step-2: `realization_indices_where`'s `vm_demanding_fns`
    /// scans only `module.functions` (empty here), so the override never
    /// fires and `body` stays at the Step-terminal `ReprKind::BRep` default
    /// instead of the expected `ReprKind::VolumeMesh`.
    #[test]
    fn compute_demanded_reprs_resolves_stdlib_optimized_consumer_via_self_functions() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{ContentHash, ModulePath, Type, ValueCellId};
        use reify_ir::{
            CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, ExportFormat,
            ReprKind, Value,
        };
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        // `@optimized("test::vm-demand")` stdlib-shaped consumer
        // `solve_probe(g: Geometry) -> Geometry` — same shape as `consumer_fn`
        // in `compute_demanded_reprs_volume_mesh_override_on_registered_consumer`
        // above, but this copy is seeded into `engine.functions`, never into
        // `module.functions`.
        let params = vec![("g".to_string(), Type::Geometry)];
        let consumer_fn = CompiledFunction {
            name: "solve_probe".to_string(),
            doc: None,
            is_pub: false,
            param_defaults: CompiledFunction::no_defaults_for(&params),
            params,
            return_type: Type::Geometry,
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::value_ref(
                    ValueCellId::new("solve_probe", "g"),
                    Type::Geometry,
                ),
            },
            content_hash: ContentHash::of(b"solve_probe_fn"),
            annotations: vec![],
            optimized_target: Some("test::vm-demand".to_string()),
            type_params: vec![],
        };

        // Structure `S`: `body` geometry cell (Primitive Box realization)
        // consumed by `_p = solve_probe(body)`. `module.functions` is left
        // EMPTY — no `.function(...)` builder call — simulating a stdlib
        // consumer that lives only in the merged `self.functions` table at
        // demand time.
        let body_default = CompiledExpr::literal(Value::Int(0), Type::Int);
        let probe_arg = CompiledExpr::value_ref(ValueCellId::new("S", "body"), Type::Geometry);
        let probe_call = CompiledExpr {
            kind: CompiledExprKind::UserFunctionCall {
                function_name: "solve_probe".to_string(),
                args: vec![probe_arg],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash::of(b"solve_probe_call"),
        };
        let template = TopologyTemplateBuilder::new("S")
            .let_binding("S", "body", Type::Geometry, body_default)
            .let_binding("S", "_p", Type::Geometry, probe_call)
            .realization_named(
                "S",
                0,
                "body",
                vec![CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    args: vec![],
                }],
            )
            .build();
        let module =
            CompiledModuleBuilder::new(ModulePath::single("test_vm_demand_self_functions"))
                .template(template)
                .build();

        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        // Seed the consumer fn ONLY into `engine.functions` — simulating the
        // post-`check()`/`eval()` merged stdlib+module function table without
        // running a real build.
        engine.functions = std::sync::Arc::from(vec![consumer_fn]);
        engine.register_volume_mesh_demand("test::vm-demand");

        let result = engine.compute_demanded_reprs(&module, ExportFormat::Step);
        assert_eq!(result.len(), 1, "one template → one outer entry");
        assert_eq!(result[0].len(), 1, "structure S has one realization (body)");
        assert_eq!(
            result[0][0],
            ReprKind::VolumeMesh,
            "a registered VolumeMesh-demanding consumer resolved via self.functions \
             (not module.functions) must override body's demand to VolumeMesh"
        );
    }

    /// RED before step-4: `PrimitiveBox/Cylinder/Sphere/Tube` and
    /// `CurveLineSegment/Arc/Helix/InterpCurve/BezierCurve/NurbsCurve`
    /// hit the `_ => None` catch-all in step-2's impl.
    #[test]
    fn classify_op_all_variants_are_classified() {
        use reify_ir::Operation;
        use strum::IntoEnumIterator;

        for op in Operation::iter() {
            assert!(
                classify_op_input_reprs(&op).is_some(),
                "Operation::{op:?} has no explicit classifier entry — \
                 classify it BRep-vs-Mesh per PRD §3a.4 (task 4049)"
            );
        }
    }
