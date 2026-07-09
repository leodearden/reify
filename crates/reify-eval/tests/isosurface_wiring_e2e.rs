//! Honest-signal end-to-end build guard for the Voxel→Mesh `isosurface`
//! slice wiring fix-forward (task 5033, GAP #2).
//!
//! Self-contained sibling of downstream task δ/5002's `voxel_to_mesh_e2e.rs`
//! (commit b859d5f6ea, un-merged): that test reads
//! `examples/multi_kernel/voxel_to_mesh.ri`, which is owned by 5002 and does
//! not exist on this branch (5033 cannot depend on it — see
//! `.task/plan.json` design_decisions). This test exercises the IDENTICAL
//! compile→build path against an INLINE `.ri` source string instead, so 5033
//! self-verifies independently.
//!
//! ## Reachability (two realizations, each single-source)
//!
//! `solid = box(size, size, size)` is the operand: `compute_demanded_reprs`
//! (β, task 5000) forces a `Surface`-op operand to demand `Voxel`, so `solid`
//! should realize `BRep→Mesh` (tessellate on occt) `→Voxel` (ingest on
//! openvdb) — `produced_repr == Voxel`. GAP #2 (this test's RED signal): the
//! runtime has no post-loop conversion edge analogous to the VolumeMesh edge
//! in `engine_build.rs::execute_realization_ops` that forces a producer
//! THROUGH a demanded Voxel repr-change, so `solid` currently resolves to
//! `BRep` (design_decision 3's fallback) instead.
//!
//! `shell = isosurface(solid)` is the terminal realization: the `Stl` export
//! sink demands `Mesh`, so `shell` should realize `Voxel→Mesh` (marching
//! cubes on openvdb, task 5001/γ) — `produced_repr == Mesh`.
//!
//! ## Engine construction
//!
//! Mirrors `crates/reify-eval/tests/ensure_openvdb_kernel.rs` and task
//! 5002's `voxel_to_mesh_e2e.rs`: `Engine::with_registered_kernel`
//! (single-pick OCCT default) + `ensure_openvdb_kernel()` (idempotently adds
//! OpenVDB, leaves `default_kernel_name` == OCCT).
//!
//! ## RED assertion choice (design_decisions)
//!
//! Assertions center on the OPERAND's `produced_repr == Voxel` (the honest
//! PRD §D6 signal) and the absence of `Severity::Error` diagnostics — NOT on
//! the literal "unresolvable GeomRef::Sub('solid')" string, which is
//! registry/config-dependent (in the standard OCCT+OpenVDB registry
//! `named_steps["solid"]` is populated with a BRep handle rather than
//! absent).

// Anchor: force the linker to include the reify_kernel_occt rlib
// unconditionally so its `inventory::submit!` registration fires at binary
// startup, regardless of cfg(has_occt). Mirrors
// crates/reify-eval/tests/manifold_cross_kernel_real.rs and the OCCT/manifold
// anchors in crates/reify-cli/src/main.rs:14,20.
extern crate reify_kernel_occt as _;

// Anchor: same rationale, for reify_kernel_openvdb. Gated on `has_openvdb`
// because the whole test below only makes sense (and only compiles its
// OpenVDB-touching calls) under that cfg. Mirrors
// crates/reify-eval/tests/ensure_openvdb_kernel.rs:22-23.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

/// Real OCCT + OpenVDB engine build of an inline `isosurface(...)` module:
/// asserts the operand ("solid") realizes `Voxel` and the terminal ("shell")
/// realizes `Mesh` with a non-empty tessellated mesh.
///
/// RED (GAP #2, before step-4's runtime fix): `solid`'s `produced_repr`
/// resolves to `BRep` (design_decision 3's fallback), not `Voxel`.
#[cfg(has_openvdb)]
#[test]
fn isosurface_wiring_builds_honest_voxel_operand_and_mesh_terminal() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::Severity;
    use reify_ir::{ExportFormat, ReprKind};
    use reify_test_support::parse_and_compile_with_stdlib;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping isosurface_wiring_builds_honest_voxel_operand_and_mesh_terminal: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let source = r#"structure IsoWire {
    param size: Length = 20mm
    let solid = box(size, size, size)
    let shell = isosurface(solid)
}"#;

    let compiled = parse_and_compile_with_stdlib(source);

    // Same pairing the fixed `cmd_build` uses (and `ensure_openvdb_kernel.rs`
    // pins): single-pick OCCT default + lazily-acquired OpenVDB.
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let acquired = engine.ensure_openvdb_kernel();
    assert!(
        acquired,
        "ensure_openvdb_kernel() must return true under cfg(has_openvdb) \
         (the OpenVDB adapter must be present in the kernel registry)"
    );

    // eval() → build() — the canonical pattern (manifold_cross_kernel_real.rs).
    let _eval = engine.eval(&compiled);
    let build = engine.build(&compiled, ExportFormat::Stl);

    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "inline isosurface module must build with no error-severity \
         diagnostics (an OpenVDB-registration or dispatch failure would \
         degrade the isosurface build to an Error diagnostic); got: {errors:?}"
    );

    // ── operand (solid) == Voxel, terminal (shell) == Mesh ────────────────
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == "IsoWire")
        .collect();
    assert!(
        nodes.len() >= 2,
        "expected at least 2 realization nodes (solid operand + shell \
         terminal) for entity IsoWire; got {}: {:?}",
        nodes.len(),
        nodes
            .iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>()
    );
    nodes.sort_by_key(|(id, _)| id.index);

    let (terminal_id, terminal_node) =
        *nodes.last().expect("nodes is non-empty (asserted above)");
    assert_eq!(
        terminal_node.produced_repr,
        ReprKind::Mesh,
        "terminal realization (shell = isosurface(solid)) must record \
         produced_repr == Mesh (marching-cubes surfacing from the Voxel \
         grid); got {:?}",
        terminal_node.produced_repr
    );

    let operand_realizes_voxel = nodes[..nodes.len() - 1]
        .iter()
        .any(|(_, r)| r.produced_repr == ReprKind::Voxel);
    assert!(
        operand_realizes_voxel,
        "operand realization (solid = box(...)) must record produced_repr \
         == Voxel (β demand-seeding forces a Surface-op operand to Voxel); \
         got: {:?}",
        nodes[..nodes.len() - 1]
            .iter()
            .map(|(_, r)| r.produced_repr)
            .collect::<Vec<_>>()
    );

    // ── terminal Mesh has vertices>0 (binary — no numeric bound) ──────────
    let terminal_path = terminal_id.to_string();
    let tess = engine.tessellate_realizations(&compiled);
    let terminal_mesh = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a MeshSurface at entity_path \
                 {terminal_path:?}; got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !terminal_mesh.mesh.vertices.is_empty() && !terminal_mesh.mesh.indices.is_empty(),
        "terminal (shell) mesh must be non-empty (a closed 20mm box's narrow-band \
         SDF crosses iso=0 at its boundary, so marching cubes must yield a \
         non-empty mesh); got {} vertices, {} indices",
        terminal_mesh.mesh.vertices.len(),
        terminal_mesh.mesh.indices.len()
    );
}

/// Review fix-forward: placed-isosurface transform guard.
///
/// `walk_placed_realizations` (geometry_ops.rs) dispatches
/// `GeometryOp::ApplyTransform` to the kernel that OWNS each handle whenever
/// the composed world pose is non-identity. Before this fix,
/// `OpenVdbKernel::execute` only matched `GeometryOp::Surface` and returned
/// `Err(VOXEL_BOOL_STUB_MSG)` for `ApplyTransform`, so a PLACED isosurface
/// (mesh handle) OR a PLACED Voxel operand (grid handle) hit the `Err` arm,
/// pushed a "transform application error" diagnostic, and was dropped from
/// the surfaced output — silently. `isosurface_wiring_builds_honest_voxel_operand_and_mesh_terminal`
/// above only covers a top-level IDENTITY-pose `IsoWire`, so this path never
/// ran in CI.
///
/// Wraps `IsoWire` in a sub placed at `+100mm` on X (`PlacedAssembly`, the
/// sole root — `IsoWire` is only reachable as a sub) and asserts the placed
/// `shell` mesh survives with no error diagnostics and an honestly-displaced
/// vertex set.
///
/// RED (before the `OpenVdbKernel::execute` `ApplyTransform` fix):
/// `ApplyTransform` on the OpenVDB grid+mesh handles returns `Err`, so both
/// the diagnostics-empty and the placed-mesh-exists assertions fail.
#[cfg(has_openvdb)]
#[test]
fn isosurface_wiring_honors_placement_transform_on_shell_mesh() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::Severity;
    use reify_ir::ExportFormat;
    use reify_test_support::parse_and_compile_with_stdlib;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping isosurface_wiring_honors_placement_transform_on_shell_mesh: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let source = r#"structure IsoWire {
    param size: Length = 20mm
    let solid = box(size, size, size)
    let shell = isosurface(solid)
}

structure PlacedAssembly {
    sub wire : IsoWire at transform3(orient_identity(), vec3(100mm, 0mm, 0mm))
}"#;

    let compiled = parse_and_compile_with_stdlib(source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let acquired = engine.ensure_openvdb_kernel();
    assert!(
        acquired,
        "ensure_openvdb_kernel() must return true under cfg(has_openvdb) \
         (the OpenVDB adapter must be present in the kernel registry)"
    );

    let _eval = engine.eval(&compiled);
    let build = engine.build(&compiled, ExportFormat::Stl);

    let build_errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        build_errors.is_empty(),
        "placed isosurface module must build with no error-severity \
         diagnostics; got: {build_errors:?}"
    );

    // Locate the shell realization's index (last realization declared under
    // IsoWire) so its placed entity_path can be predicted — mirrors the
    // identity-pose test's `nodes.last()` pattern above.
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut iso_nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == "IsoWire")
        .collect();
    assert!(
        iso_nodes.len() >= 2,
        "expected at least 2 realization nodes (solid operand + shell \
         terminal) for entity IsoWire; got {}",
        iso_nodes.len()
    );
    iso_nodes.sort_by_key(|(id, _)| id.index);
    let (shell_id, _) = *iso_nodes
        .last()
        .expect("iso_nodes is non-empty (asserted above)");
    // `surface_subtree`'s path_prefix scheme (geometry_ops.rs): root prefix
    // is the root template's name ("PlacedAssembly"), and each sub appends
    // ".{sub_name}" — so the shell realization inside the `wire` sub surfaces
    // at "PlacedAssembly.wire#realization[{shell_index}]".
    let shell_path = format!("PlacedAssembly.wire#realization[{}]", shell_id.index);

    let tess = engine.tessellate_realizations(&compiled);
    let tess_errors: Vec<_> = tess
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        tess_errors.is_empty(),
        "placed IsoWire sub must tessellate with no error-severity \
         diagnostics (an OpenVDB ApplyTransform failure pushes a \
         'transform application error' diagnostic and drops the \
         realization); got: {tess_errors:?}"
    );

    let placed_shell = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == shell_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a placed shell MeshSurface \
                 at entity_path {shell_path:?}; got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !placed_shell.mesh.vertices.is_empty() && !placed_shell.mesh.indices.is_empty(),
        "placed shell mesh must be non-empty; got {} vertices, {} indices",
        placed_shell.mesh.vertices.len(),
        placed_shell.mesh.indices.len()
    );

    // ── honest transform-applied signal (displacement bound) ──────────────
    // box(20mm) is centered at origin (half-extent 10mm), displaced +100mm
    // by the sub translation; honest_floor's narrow band stays within
    // longest_extent/2 = 10mm of the true surface, so the placed iso=0
    // surface lies within ~±20mm of the 100mm center -> world-X ~[0.08,
    // 0.12] m. An un-transformed (origin) mesh would have min_x ~ -0.012 m
    // and mean_x ~ 0, failing both bounds below with wide margin.
    let xs: Vec<f32> = placed_shell
        .mesh
        .vertices
        .chunks_exact(3)
        .map(|v| v[0])
        .collect();
    let min_x = xs.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(
        min_x > 0.05,
        "every placed shell-mesh vertex X must exceed 0.05 m (the \
         un-transformed origin box would have min_x ~ -0.012 m); got \
         min_x = {min_x}"
    );
    let mean_x = xs.iter().sum::<f32>() / xs.len() as f32;
    assert!(
        (0.08..=0.12).contains(&mean_x),
        "mean placed shell-mesh vertex X must lie in [0.08, 0.12] m; got \
         mean_x = {mean_x}"
    );
}
