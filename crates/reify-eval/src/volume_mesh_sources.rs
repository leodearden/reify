//! The body each VolumeMesh this engine produced was meshed from.
//!
//! A VolumeMesh is an extra representation for an FEA consumer, not the body's
//! geometry, so Phase-B egress (export, the tessellate walk,
//! `distance_between_placed`) reads the meshed-from body rather than the gmsh
//! VolumeMesh. Records are keyed by the full [`KernelHandle`] because handle ids
//! are kernel-local. Records span builds: a cache-served VolumeMesh terminal
//! reaches Phase-B without the VolumeMesh edge running, so the record made by
//! the build that cached it is the only source.
//!
//! Records are cleared only by a full flush (`clear_realization_cache`,
//! `reset_geometry_for_reload`).

use std::collections::HashMap;

use reify_ir::{GeometryHandleId, KernelHandle, KernelId};

/// A VolumeMesh and the body it was meshed from, as stated by the edge that
/// produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshedVolume {
    pub volume_mesh: KernelHandle,
    pub meshed_from: KernelHandle,
}

impl MeshedVolume {
    /// Push the gmsh VolumeMesh `id` onto a realization's `step_handles` as its
    /// new terminal, paired with the body it was meshed from. Every VolumeMesh
    /// edge stores its terminal through this, so none pushes one unpaired.
    #[must_use]
    pub(crate) fn push_gmsh_terminal(
        step_handles: &mut Vec<KernelHandle>,
        id: GeometryHandleId,
        meshed_from: KernelHandle,
    ) -> Self {
        let volume_mesh = KernelHandle {
            kernel: KernelId::Gmsh,
            id,
        };
        step_handles.push(volume_mesh);
        Self {
            volume_mesh,
            meshed_from,
        }
    }
}

/// Every [`MeshedVolume`] recorded since the realization cache was last fully
/// cleared.
#[derive(Debug, Default)]
pub(crate) struct VolumeMeshSources(HashMap<KernelHandle, KernelHandle>);

impl VolumeMeshSources {
    fn record(&mut self, meshed: MeshedVolume) {
        self.0.insert(meshed.volume_mesh, meshed.meshed_from);
    }

    /// The handle Phase-B egress reads for `terminal`: the body it was meshed
    /// from when `terminal` is a recorded VolumeMesh, else `terminal` itself.
    fn egress_body(&self, terminal: KernelHandle) -> KernelHandle {
        self.0.get(&terminal).copied().unwrap_or(terminal)
    }

    /// Record the VolumeMesh edge's pair when it ran, then resolve the
    /// realization's Phase-B terminal: the egress body of its last handle, or
    /// `None` when `realization_handles` (its own slice of `step_handles`) is
    /// empty. A cache-served run, where the edge did not run, resolves through
    /// the pair the cold build recorded, so both resolve to the same body.
    pub(crate) fn record_and_resolve_terminal(
        &mut self,
        realization_handles: &[KernelHandle],
        volume_mesh: Option<MeshedVolume>,
    ) -> Option<KernelHandle> {
        if let Some(meshed) = volume_mesh {
            self.record(meshed);
        }
        realization_handles
            .last()
            .map(|&last| self.egress_body(last))
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn push_gmsh_terminal_appends_the_gmsh_handle_and_pairs_it_with_its_source() {
        let body = handle(KernelId::Occt, 3);
        let mut step_handles = vec![handle(KernelId::Occt, 1), body];
        let meshed = MeshedVolume::push_gmsh_terminal(&mut step_handles, GeometryHandleId(7), body);
        assert_eq!(
            step_handles,
            vec![handle(KernelId::Occt, 1), body, handle(KernelId::Gmsh, 7)]
        );
        assert_eq!(
            meshed,
            MeshedVolume {
                volume_mesh: handle(KernelId::Gmsh, 7),
                meshed_from: body,
            }
        );
    }

    #[test]
    fn a_cold_volume_mesh_terminal_resolves_to_the_body_it_was_meshed_from() {
        let body = handle(KernelId::Occt, 3);
        let mut step_handles = vec![body];
        let meshed = MeshedVolume::push_gmsh_terminal(&mut step_handles, GeometryHandleId(7), body);
        let mut sources = VolumeMeshSources::default();
        assert_eq!(
            sources.record_and_resolve_terminal(&step_handles, Some(meshed)),
            Some(body)
        );
    }

    #[test]
    fn a_cache_served_volume_mesh_terminal_resolves_through_the_cold_record() {
        let mut sources = sources_with_gmsh_7_meshed_from_occt_3();
        assert_eq!(
            sources.record_and_resolve_terminal(&[handle(KernelId::Gmsh, 7)], None),
            Some(handle(KernelId::Occt, 3)),
            "the cache-served run did not run the VolumeMesh edge; it must still \
             resolve to the body the cold build meshed from"
        );
    }

    #[test]
    fn a_non_volume_mesh_terminal_resolves_to_its_last_handle() {
        let mut sources = sources_with_gmsh_7_meshed_from_occt_3();
        assert_eq!(
            sources.record_and_resolve_terminal(
                &[handle(KernelId::Occt, 1), handle(KernelId::Manifold, 2)],
                None
            ),
            Some(handle(KernelId::Manifold, 2))
        );
    }

    #[test]
    fn an_empty_realization_resolves_to_none_but_still_records_its_pair() {
        let mut sources = VolumeMeshSources::default();
        let meshed = MeshedVolume {
            volume_mesh: handle(KernelId::Gmsh, 7),
            meshed_from: handle(KernelId::Occt, 3),
        };
        assert_eq!(sources.record_and_resolve_terminal(&[], Some(meshed)), None);
        assert_eq!(
            sources.egress_body(handle(KernelId::Gmsh, 7)),
            handle(KernelId::Occt, 3)
        );
    }
}
