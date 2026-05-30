//! Body partitioning: route segmented regions to shell/tet meshers and
//! identify shell↔tet interfaces (PRD task **T12**).
//!
//! Implements the routing + interface-descriptor half of
//! `docs/prds/v0_4/structural-analysis-shells.md` §124 ("mixed-region body
//! partitioning"). The T4 auto-segmenter ([`crate::segment_regions`]) labels
//! each connected component of a single body's medial mask as
//! `ShellEligible` / `TetEligible` / `MixedComponentOfBody`; this module maps
//! that classification to a per-region [`RegionMeshKind`] (shell vs. tet
//! mesher) and emits a kernel-agnostic [`ShellTetInterface`] descriptor for
//! every shell↔tet junction.
//!
//! # Why kernel-agnostic
//!
//! `reify-shell-extract` deliberately does **not** depend on
//! `reify-solver-elastic` (cycle-avoidance, task γ #3834), so the MPC tying
//! rows ([`reify_solver_elastic::mpc::MpcRow`]) cannot be produced here. This
//! module emits only the geometric tie descriptor (region pair + unit normal +
//! thickness + world location); `reify-eval`'s `engine_build` converts it to
//! `MpcRow` once it has both crates in scope. See `plan.json` design decisions.
//!
//! # Why proximity, not shared faces
//!
//! `segment_regions` builds 6-face connected components, so a shell region and
//! a tet region of one body are **disconnected** mask components (their medial
//! axes sit at different depths and do not touch). Interfaces are therefore
//! identified by world-space proximity between region voxel sets, not by shared
//! voxel faces — a shared face would have fused the two into one component.

#[cfg(test)]
mod tests {
    use crate::{
        BodyPartition, MedialMask, MidSurfaceMesh, PartitionError, PartitionOptions,
        RegionMeshKind, SegmentationResult, ShellTetInterface, SingleBodyMask, partition_body,
    };

    // ── Step 1: public-surface smoke test ─────────────────────────────────────

    /// Smoke test: all public partition types are reachable from the crate root
    /// and `partition_body` is callable.  Empty mask + empty segmentation +
    /// empty mesh → `Ok` with empty `region_kinds` and `interfaces`.
    #[test]
    fn partition_body_public_surface_is_callable_on_empty_input() {
        let mask = SingleBodyMask::new(MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        });
        let seg = SegmentationResult {
            regions: vec![],
            vertex_labels: vec![],
            triangle_labels: vec![],
        };
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let result: BodyPartition =
            partition_body(&mask, &seg, &mesh, &PartitionOptions::default())
                .expect("empty input should return Ok");
        assert!(
            result.region_kinds.is_empty(),
            "empty segmentation → no region kinds"
        );
        assert!(
            result.interfaces.is_empty(),
            "empty segmentation → no interfaces"
        );

        // Compile-probes: error and routing enums are reachable from the root.
        let _: PartitionError = PartitionError::InvalidProximityFactor { value: 0.0 };
        let _: Option<RegionMeshKind> = None;
        let _: Option<ShellTetInterface> = None;
    }
}
