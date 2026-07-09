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
    eprintln!("DEBUG tess.diagnostics = {:#?}", tess.diagnostics);
    eprintln!(
        "DEBUG tess.meshes paths = {:?}",
        tess.meshes.iter().map(|m| &m.entity_path).collect::<Vec<_>>()
    );
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
