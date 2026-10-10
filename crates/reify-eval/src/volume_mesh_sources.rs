#[cfg(test)]
mod tests {
    use super::*;

    use reify_ir::{GeometryHandleId, KernelHandle, KernelId};

    fn handle(kernel: KernelId, id: u64) -> KernelHandle {
        KernelHandle {
            kernel,
            id: GeometryHandleId(id),
        }
    }

    fn sources_with_gmsh_7_meshed_from_occt_3() -> VolumeMeshSources {
        let mut sources = VolumeMeshSources::default();
        sources.record(MeshedVolume {
            volume_mesh: handle(KernelId::Gmsh, 7),
            meshed_from: handle(KernelId::Occt, 3),
        });
        sources
    }

    #[test]
    fn a_recorded_volume_mesh_resolves_to_the_body_it_was_meshed_from() {
        let sources = sources_with_gmsh_7_meshed_from_occt_3();
        assert_eq!(
            sources.egress_body(handle(KernelId::Gmsh, 7)),
            handle(KernelId::Occt, 3)
        );
    }

    #[test]
    fn an_unrecorded_handle_resolves_to_itself() {
        let sources = sources_with_gmsh_7_meshed_from_occt_3();
        assert_eq!(
            sources.egress_body(handle(KernelId::Occt, 7)),
            handle(KernelId::Occt, 7),
            "ids are kernel-local: the recorded VolumeMesh's id on another kernel \
             names an unrelated shape and must not be redirected"
        );
        assert_eq!(
            sources.egress_body(handle(KernelId::Occt, 3)),
            handle(KernelId::Occt, 3)
        );
    }

    #[test]
    fn clear_forgets_every_record() {
        let mut sources = sources_with_gmsh_7_meshed_from_occt_3();
        sources.clear();
        assert_eq!(
            sources.egress_body(handle(KernelId::Gmsh, 7)),
            handle(KernelId::Gmsh, 7)
        );
    }
}
