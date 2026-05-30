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

use crate::mid_surface::MidSurfaceMesh;
use crate::segmentation::{RegionClassification, SegmentationResult, SingleBodyMask};

/// Per-region routing decision: which mesher a segmented region is sent to.
///
/// Maps from [`crate::RegionClassification`]:
/// - `ShellEligible` → [`RegionMeshKind::Shell`] (thin enough to mid-surface).
/// - `MixedComponentOfBody` → [`RegionMeshKind::Shell`] (locally shell-able;
///   the body also has a tet region, so the shell is tied across the interface
///   via an MPC — see [`ShellTetInterface`]).
/// - `TetEligible` → [`RegionMeshKind::Tet`] (requires volumetric meshing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMeshKind {
    /// Region is routed to the mid-surface (T9) shell mesher.
    Shell,
    /// Region is routed to the volumetric (Gmsh) tet mesher.
    Tet,
}

/// Kernel-agnostic descriptor of one shell↔tet junction.
///
/// Carries everything `reify-eval`'s `engine_build` needs to build the MPC
/// tying rows ([`reify_solver_elastic::mpc::MpcRow::shell_tet_tying`]) without
/// `reify-shell-extract` depending on `reify-solver-elastic`. The `normal` is
/// guaranteed unit and `thickness` strictly positive by [`partition_body`], so
/// the downstream `shell_tet_tying` preconditions hold by construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellTetInterface {
    /// Label of the shell-routed region on this interface.
    pub shell_region: u32,
    /// Label of the tet-routed region on this interface.
    pub tet_region: u32,
    /// Unit outward normal of the shell mid-surface at the junction
    /// (area-weighted over the shell region's triangles). `|normal| ≈ 1`.
    pub normal: [f64; 3],
    /// Shell through-thickness at the junction (the shell region's
    /// `mean_thickness`). Strictly positive.
    pub thickness: f64,
    /// World-space location of the tie point (centroid of the shell-region
    /// voxels nearest the tet region).
    pub location: [f64; 3],
}

/// Output of [`partition_body`].
#[derive(Debug, Clone, PartialEq)]
pub struct BodyPartition {
    /// Per-region routing decision, parallel to `seg.regions` by index.
    pub region_kinds: Vec<RegionMeshKind>,
    /// Shell↔tet junctions discovered by world-space proximity.
    pub interfaces: Vec<ShellTetInterface>,
}

/// Tunable parameters for [`partition_body`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartitionOptions {
    /// Scales the characteristic length to the maximum medial-axis gap counted
    /// as a shell↔tet interface: a (shell, tet) region pair is an interface iff
    /// the minimum world-space distance between their voxel sets is below
    /// `interface_proximity_factor · characteristic_length`.
    ///
    /// Must be strictly positive. Default `2.0` (≈ a two-voxel medial-axis gap
    /// at the junction; see the module doc on why the gap exists).
    pub interface_proximity_factor: f64,
}

impl Default for PartitionOptions {
    fn default() -> Self {
        Self {
            interface_proximity_factor: 2.0,
        }
    }
}

/// Errors returned by [`partition_body`].
#[derive(Debug, Clone, PartialEq)]
pub enum PartitionError {
    /// `interface_proximity_factor` must be strictly positive. A zero or
    /// negative factor would make the proximity threshold non-positive and
    /// suppress every interface.
    InvalidProximityFactor {
        /// The offending factor supplied by the caller.
        value: f64,
    },
    /// The shell region's triangles accumulate to a ~zero area-weighted normal
    /// (all degenerate/zero-area, or no triangles map to the region), so no
    /// well-defined mid-surface normal exists for the tie.
    DegenerateInterfaceNormal {
        /// Label of the shell region whose normal could not be derived.
        shell_region: u32,
    },
    /// `mesh.thickness.len()` must equal `mesh.vertices.len()`. A mismatch
    /// indicates a caller-constructed (non-T2-produced) mesh with inconsistent
    /// parallel arrays.
    MeshLengthMismatch {
        /// Number of vertices in the mesh.
        vertices_len: usize,
        /// Number of thickness entries in the mesh.
        thickness_len: usize,
    },
}

impl std::fmt::Display for PartitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionError::InvalidProximityFactor { value } => write!(
                f,
                "interface_proximity_factor must be strictly positive (got {value}); \
                 a zero or negative factor would suppress every shell↔tet interface"
            ),
            PartitionError::DegenerateInterfaceNormal { shell_region } => write!(
                f,
                "shell region {shell_region} has a degenerate (~zero-area) \
                 area-weighted triangle normal; no mid-surface tie normal can be derived"
            ),
            PartitionError::MeshLengthMismatch {
                vertices_len,
                thickness_len,
            } => write!(
                f,
                "mesh.thickness.len() ({thickness_len}) ≠ mesh.vertices.len() ({vertices_len}); \
                 the two parallel arrays must be the same length"
            ),
        }
    }
}

impl std::error::Error for PartitionError {}

/// Route a single body's segmented regions to the shell/tet meshers and
/// identify the shell↔tet interfaces between them (PRD task T12).
///
/// # Parameters
///
/// - `mask`: the [`SingleBodyMask`] whose `spacing`/`origin` define the
///   voxel→world transform used for proximity and tie-location geometry.
/// - `seg`: the T4 [`SegmentationResult`] — `regions` for routing/proximity,
///   `triangle_labels` for selecting each shell region's triangles.
/// - `mesh`: the T2/T9 [`MidSurfaceMesh`] — `vertices` supply the triangle
///   geometry used to derive interface normals.
/// - `opts`: tuning ([`PartitionOptions::interface_proximity_factor`]).
///
/// # Errors
///
/// - [`PartitionError::InvalidProximityFactor`] if
///   `opts.interface_proximity_factor ≤ 0`.
/// - [`PartitionError::MeshLengthMismatch`] if
///   `mesh.thickness.len() ≠ mesh.vertices.len()`.
/// - [`PartitionError::DegenerateInterfaceNormal`] if a shell region on an
///   interface has no well-defined area-weighted triangle normal.
pub fn partition_body(
    _mask: &SingleBodyMask,
    seg: &SegmentationResult,
    mesh: &MidSurfaceMesh,
    opts: &PartitionOptions,
) -> Result<BodyPartition, PartitionError> {
    // (1) Reject a non-positive proximity factor before any other work.
    if opts.interface_proximity_factor <= 0.0 {
        return Err(PartitionError::InvalidProximityFactor {
            value: opts.interface_proximity_factor,
        });
    }

    // (2) Reject a mesh with mismatched parallel arrays.
    if mesh.thickness.len() != mesh.vertices.len() {
        return Err(PartitionError::MeshLengthMismatch {
            vertices_len: mesh.vertices.len(),
            thickness_len: mesh.thickness.len(),
        });
    }

    // (3) Route each region to a mesher by its T4 classification. Both
    // `ShellEligible` and `MixedComponentOfBody` are locally shell-able, so
    // both map to `Shell`; the body-context distinction (MPC tying) is carried
    // by the interface set, not the per-region kind. Parallel to `seg.regions`.
    let region_kinds: Vec<RegionMeshKind> = seg
        .regions
        .iter()
        .map(|r| match r.classification {
            RegionClassification::ShellEligible | RegionClassification::MixedComponentOfBody => {
                RegionMeshKind::Shell
            }
            RegionClassification::TetEligible => RegionMeshKind::Tet,
        })
        .collect();

    // Proximity-based interface detection is layered on in step-6/8.
    Ok(BodyPartition {
        region_kinds,
        interfaces: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        BodyPartition, MedialMask, MidSurfaceMesh, PartitionError, PartitionOptions,
        RegionClassification, RegionInfo, RegionMeshKind, SegmentationResult, ShellTetInterface,
        SingleBodyMask, partition_body,
    };

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Build a [`RegionInfo`] with the given label, classification, and voxels,
    /// filling the metric fields with plausible placeholder values (the routing
    /// and proximity logic reads `classification`, `voxels`, and
    /// `mean_thickness`, not the ratio fields).
    fn region_info(
        label: u32,
        classification: RegionClassification,
        voxels: Vec<[i32; 3]>,
    ) -> RegionInfo {
        RegionInfo {
            label,
            voxels,
            mean_thickness: 1.0,
            extent: 10.0,
            thickness_extent_ratio: 0.1,
            classification,
        }
    }

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

    // ── Step 3: region routing by classification ──────────────────────────────

    /// `partition_body` maps each region's classification to a `RegionMeshKind`,
    /// parallel to `seg.regions` by index:
    ///   `ShellEligible → Shell`, `TetEligible → Tet`,
    ///   `MixedComponentOfBody → Shell`.
    #[test]
    fn partition_body_routes_regions_by_classification() {
        // Three regions placed far apart so no interface is produced (proximity
        // is exercised in step-5). Classifications cover all three variants.
        let seg = SegmentationResult {
            regions: vec![
                region_info(0, RegionClassification::ShellEligible, vec![[0, 0, 0]]),
                region_info(1, RegionClassification::TetEligible, vec![[100, 0, 0]]),
                region_info(
                    2,
                    RegionClassification::MixedComponentOfBody,
                    vec![[0, 100, 0]],
                ),
            ],
            vertex_labels: vec![],
            triangle_labels: vec![],
        };
        let mask = SingleBodyMask::new(MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[0, 0, 0], [100, 0, 0], [0, 100, 0]],
        });
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };

        let result = partition_body(&mask, &seg, &mesh, &PartitionOptions::default())
            .expect("partition_body should succeed");

        assert_eq!(
            result.region_kinds,
            vec![
                RegionMeshKind::Shell, // ShellEligible
                RegionMeshKind::Tet,   // TetEligible
                RegionMeshKind::Shell, // MixedComponentOfBody (locally shell-able)
            ],
            "region_kinds must parallel seg.regions by index: Shell/Tet/Shell"
        );
    }
}
