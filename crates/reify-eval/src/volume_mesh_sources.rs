//! The body each VolumeMesh this engine produced was meshed from.
//!
//! A VolumeMesh is an extra representation for an FEA consumer, not the body's
//! geometry, so Phase-B egress (export, the tessellate walk,
//! `distance_between_placed`) reads the meshed-from body rather than the gmsh
//! VolumeMesh. Records are keyed by the full [`KernelHandle`] because handle ids
//! are kernel-local. Records span builds, as the realization cache does: a
//! cache-served VolumeMesh terminal reaches Phase-B without the VolumeMesh edge
//! running, so the record made by the build that cached it is the only source.
//! They are cleared with the cache, because after a flush or kernel reset no
//! recorded VolumeMesh can be served again and kernels may re-mint ids.

use std::collections::HashMap;

use reify_ir::KernelHandle;

/// A VolumeMesh and the body it was meshed from, as stated by the edge that
/// produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshedVolume {
    pub volume_mesh: KernelHandle,
    pub meshed_from: KernelHandle,
}

/// Every [`MeshedVolume`] recorded since the realization cache was last cleared.
#[derive(Debug, Default)]
pub(crate) struct VolumeMeshSources(HashMap<KernelHandle, KernelHandle>);

impl VolumeMeshSources {
    pub(crate) fn record(&mut self, meshed: MeshedVolume) {
        self.0.insert(meshed.volume_mesh, meshed.meshed_from);
    }

    /// The handle Phase-B egress reads for `terminal`: the body it was meshed
    /// from when `terminal` is a recorded VolumeMesh, else `terminal` itself.
    pub(crate) fn egress_body(&self, terminal: KernelHandle) -> KernelHandle {
        self.0.get(&terminal).copied().unwrap_or(terminal)
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use reify_ir::{GeometryHandleId, KernelId};

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
