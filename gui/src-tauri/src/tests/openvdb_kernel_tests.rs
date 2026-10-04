//! The GUI's production boot path (`EngineSession::with_registered_kernel`)
//! must hold the OpenVDB adapter, so a design whose plan names openvdb (an
//! `isosurface`) realizes in the viewport instead of degrading to the
//! "target kernel 'openvdb' not present" dispatch error (task #6963).

#[cfg(has_openvdb)]
use crate::engine::EngineSession;
#[cfg(has_openvdb)]
use reify_constraints::SimpleConstraintChecker;

#[cfg(has_openvdb)]
fn production_session() -> EngineSession {
    EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker))
}

#[cfg(has_openvdb)]
#[test]
fn production_session_holds_the_openvdb_kernel() {
    let name = reify_core::KernelId::OpenVdb.as_registry_name();

    let registry = reify_eval::kernel_registry::registry();
    assert!(
        registry.contains_key(name),
        "LINK defect, not the boot defect: the OpenVDB adapter is not in this \
         binary's kernel registry under cfg(has_openvdb); registry keys: {:?}",
        registry.keys().collect::<Vec<_>>()
    );

    let session = production_session();
    let loaded: Vec<&str> = session.engine().registered_kernel_names().collect();
    assert!(
        loaded.contains(&name),
        "the production EngineSession must hold the {name:?} kernel; \
         loaded kernels: {loaded:?}"
    );
}

#[cfg(all(feature = "gui", has_openvdb))]
#[test]
fn production_session_realizes_isosurface_shell() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping production_session_realizes_isosurface_shell: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let fixture = format!(
        "{}/../../examples/multi_kernel/voxel_to_mesh.ri",
        super::gui_crate_manifest_dir()
    );
    let source = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("failed to read isosurface fixture {fixture}: {e}"));

    let mut session = production_session();
    let state = session
        .load_from_source(&source, "voxel_to_mesh")
        .expect("voxel_to_mesh.ri must load through the production session");

    let errors: Vec<_> = state
        .tessellation_diagnostics
        .iter()
        .chain(&state.compile_diagnostics)
        .filter(|d| d.severity == "Error")
        .collect();
    assert!(
        errors.is_empty(),
        "voxel_to_mesh.ri must load with no Error diagnostic; got: {errors:?}"
    );

    let shell_path = session
        .compiled_for_test()
        .expect("compiled module must be set after load_from_source")
        .templates
        .iter()
        .find(|t| t.name == "VoxelToMesh")
        .expect("VoxelToMesh template must exist")
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("shell"))
        .expect("VoxelToMesh must have a realization named 'shell'")
        .id
        .to_string();

    let shell_mesh = state
        .meshes
        .iter()
        .find(|m| m.entity_path == shell_path)
        .unwrap_or_else(|| {
            panic!(
                "the isosurface shell {shell_path:?} must surface as a mesh; \
                 mesh entity_paths: {:?}",
                state
                    .meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !shell_mesh.vertices.is_empty() && !shell_mesh.indices.is_empty(),
        "the isosurface shell mesh must be non-empty; got {} vertex floats, {} indices",
        shell_mesh.vertices.len(),
        shell_mesh.indices.len()
    );
}
