//! Per-region segmentation classifier for thin-solid medial masks (PRD task T4).
//!
//! Implements connected-component analysis on the medial mask and per-region
//! thickness/extent classification (`shell-eligible` / `tet-eligible` /
//! `mixed-component-of-body`) as specified in
//! `docs/prds/v0_4/structural-analysis-shells.md` §60–65.

use crate::medial::MedialMask;
use crate::mid_surface::MidSurfaceMesh;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

/// Caller attestation that `mask` represents a **single physical body**.
///
/// `segment_regions`'s second-pass `MixedComponentOfBody` promotion (PRD §103)
/// is **whole-mask body-scoped**: if the mask spans multiple disconnected bodies,
/// every `ShellEligible` region anywhere in the mask is promoted when *any*
/// region is `TetEligible`, regardless of whether those regions belong to the
/// same physical body.  This produces incorrect tags for multi-body inputs.
///
/// Wrapping a [`MedialMask`] in `SingleBodyMask::new(mask)` is an explicit,
/// reviewable, searchable attestation that the caller has split the mask per
/// body before invoking `segment_regions`.  No runtime validation is performed
/// — the invariant is a caller's responsibility, not a computable property
/// (a single physical body's medial mask can legitimately have multiple
/// disconnected components at voxel resolution, e.g. a barbell geometry).
///
/// # Example
///
/// ```
/// use reify_shell_extract::{MedialMask, SingleBodyMask};
///
/// let mask = MedialMask { spacing: [1.0, 1.0, 1.0], origin: [0.0, 0.0, 0.0], voxels: vec![] };
/// let single = SingleBodyMask::new(mask);
/// assert!(single.inner().voxels.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SingleBodyMask {
    mask: MedialMask,
}

impl SingleBodyMask {
    /// Affirm that `mask` represents a single physical body.
    ///
    /// This is a caller attestation — no runtime check is performed.
    /// See [`SingleBodyMask`] for the full rationale.
    pub fn new(mask: MedialMask) -> Self {
        Self { mask }
    }

    /// Return a borrow of the wrapped [`MedialMask`].
    pub fn inner(&self) -> &MedialMask {
        &self.mask
    }

    /// Unwrap and return the owned [`MedialMask`].
    pub fn into_inner(self) -> MedialMask {
        self.mask
    }
}

/// Tunable parameters for [`segment_regions`].
///
/// The default `shell_threshold = 0.2` corresponds to `L/t > 5` (PRD §63 /
/// §125 `ElasticOptions.shell_threshold`): a region whose
/// `mean_thickness / extent < 0.2` is considered shell-eligible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentationOptions {
    /// Threshold for the `mean_thickness / extent` ratio.  A region with
    /// ratio **below** this value is classified as [`RegionClassification::ShellEligible`];
    /// a region at or above is [`RegionClassification::TetEligible`].
    ///
    /// Must be strictly positive.  Default `0.2` (PRD §63 / §125).
    pub shell_threshold: f64,
}

impl Default for SegmentationOptions {
    fn default() -> Self {
        Self {
            shell_threshold: 0.2,
        }
    }
}

/// Errors returned by [`segment_regions`].
#[derive(Debug, Clone, PartialEq)]
pub enum SegmentationError {
    /// `shell_threshold` must be strictly positive.  A zero or negative
    /// threshold would classify every region as `ShellEligible` regardless
    /// of its geometry.
    InvalidThreshold {
        /// The offending threshold value supplied by the caller.
        value: f64,
    },
    /// `mesh.thickness.len()` must equal `mesh.vertices.len()`.  A mismatch
    /// indicates a caller-constructed (non-T2-produced) mesh with inconsistent
    /// parallel arrays.
    MeshLengthMismatch {
        /// Number of vertices in the mesh.
        vertices_len: usize,
        /// Number of thickness entries in the mesh.
        thickness_len: usize,
    },
}

impl std::fmt::Display for SegmentationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentationError::InvalidThreshold { value } => write!(
                f,
                "shell_threshold must be strictly positive (got {value}); \
                 a zero or negative threshold would classify every region as \
                 ShellEligible regardless of geometry"
            ),
            SegmentationError::MeshLengthMismatch {
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

impl std::error::Error for SegmentationError {}

/// Per-region classification outcome.
///
/// Derived from the `(shell-eligible / tet-eligible / mixed-component-of-body)`
/// enumeration in PRD §103.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionClassification {
    /// The region's `mean_thickness / extent` ratio is below
    /// [`SegmentationOptions::shell_threshold`].  The body is thin enough
    /// for mid-surface shell-element meshing.
    ShellEligible,
    /// The region's ratio is at or above the threshold, or the mask is empty
    /// in this region.  The body requires volumetric tet meshing.
    TetEligible,
    /// The region is locally shell-eligible, but its parent body also contains
    /// at least one tet-eligible region.  MPC tying at region interfaces
    /// (task T10) is required.
    MixedComponentOfBody,
}

/// Per-connected-component metrics and classification.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInfo {
    /// Zero-based region index, assigned by BFS order over `mask.voxels`.
    pub label: u32,
    /// Voxel indices `(i, j, k)` belonging to this connected component.
    pub voxels: Vec<[i32; 3]>,
    /// Arithmetic mean of `mesh.thickness[v]` over all mid-surface vertices
    /// `v` whose associated mask voxel belongs to this region.  `0.0` if
    /// no vertices are associated (degenerate isolated-voxel case).
    ///
    /// **Classification note**: when no vertices are associated the region is
    /// classified as [`RegionClassification::TetEligible`] regardless of the
    /// ratio, because a `0.0` thickness would otherwise yield
    /// `ratio = 0.0 < threshold` → spurious `ShellEligible`.
    pub mean_thickness: f64,
    /// Largest axis-aligned bounding-box span in world units:
    /// `max((idx_max[a] − idx_min[a]) × spacing[a])` over the three axes.
    pub extent: f64,
    /// `mean_thickness / extent`.  `f64::INFINITY` if `extent == 0`.
    pub thickness_extent_ratio: f64,
    /// Shell/tet/mixed classification for this region.
    pub classification: RegionClassification,
}

/// Output of [`segment_regions`].
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentationResult {
    /// One entry per connected component of `mask.voxels`, in BFS discovery
    /// order.
    pub regions: Vec<RegionInfo>,
    /// Per-vertex region label, parallel to `mesh.vertices`.  Entry `i` is
    /// the label of the region whose mask voxel is associated with
    /// `mesh.vertices[i]`, found by the 8-corner floor/ceil enumeration
    /// (dz outer, dy middle, dx inner — first matching corner wins).
    /// `u32::MAX` is a sentinel for vertices with no associated mask voxel
    /// (should not occur for well-formed T2 outputs) **and** for vertices
    /// whose world coordinates contain at least one non-finite component
    /// (NaN or ±Infinity); see [`segment_regions`] for the contract.
    pub vertex_labels: Vec<u32>,
    /// Per-triangle region label, parallel to `mesh.triangles`.  Derived
    /// from the first non-sentinel entry in `vertex_labels` for the
    /// triangle's three vertices (binary-MC guarantees all three share the
    /// same region on a well-formed mesh; `u32::MAX` if all three are
    /// sentinels).
    pub triangle_labels: Vec<u32>,
}

/// Compute per-region connected-component segmentation and classification
/// from a medial mask and its corresponding mid-surface mesh.
///
/// # Algorithm
///
/// 1. Validate inputs (`shell_threshold > 0`, `mesh` length consistency).
/// 2. BFS 6-face connected-component labelling of `mask.voxels`.
/// 3. Per-region metrics: axis-aligned bounding-box extent, per-vertex
///    thickness average via the 8-corner mask-voxel lookup.
/// 4. First-pass classification: `ShellEligible` if `ratio < shell_threshold`,
///    else `TetEligible`.
/// 5. Second-pass promotion: if the result contains **both** `ShellEligible`
///    and `TetEligible` regions, every `ShellEligible` is re-tagged
///    `MixedComponentOfBody` (PRD §103).
///
/// # Precondition — single body per call
///
/// The `&SingleBodyMask` parameter encodes the single-body precondition at the
/// type level. Construct a `SingleBodyMask` via `SingleBodyMask::new(mask)` as
/// an explicit caller-side attestation that `mask` represents a single physical
/// body (see [`SingleBodyMask`] for rationale). The second-pass
/// `MixedComponentOfBody` promotion (step 5) is whole-mask body-scoped, so
/// passing a multi-body mask produces incorrect tags without a diagnostic.
///
/// # Non-finite vertex coordinates
///
/// Any `mesh.vertices[i]` component that is NaN or ±Infinity causes vertex `i`
/// to be skipped entirely: `vertex_labels[i]` retains the `u32::MAX` sentinel
/// without panicking in either build mode.  This is a deliberate contract — it
/// is **not** an error.
///
/// Rationale: the internal helper [`axis_floor_ceil_unique`] applies a
/// `debug_assert!` to reject non-finite inputs during development, but in
/// release the NaN→0i32 and Inf→i32::MIN/MAX saturating casts produce absurd
/// voxel keys that may accidentally match real mask voxels near the origin.
/// The per-vertex guard upstream of the fractional-index computation eliminates
/// both failure modes.
///
/// # Errors
///
/// Returns [`SegmentationError::InvalidThreshold`] if `options.shell_threshold ≤ 0`.
/// Returns [`SegmentationError::MeshLengthMismatch`] if
/// `mesh.thickness.len() ≠ mesh.vertices.len()`.
///
/// Grid alignment between `mask` and `mesh` is **not** validated here; callers
/// are expected to pass consistent T1 + T2 outputs.
pub fn segment_regions(
    mask: &SingleBodyMask,
    mesh: &MidSurfaceMesh,
    options: &SegmentationOptions,
) -> Result<SegmentationResult, SegmentationError> {
    // Rebind to the inner MedialMask so the algorithm body is unchanged.
    let mask = mask.inner();
    // (1a) Reject invalid threshold before any other work.
    if options.shell_threshold <= 0.0 {
        return Err(SegmentationError::InvalidThreshold {
            value: options.shell_threshold,
        });
    }

    // (1b) Reject mesh with mismatched parallel arrays.
    if mesh.thickness.len() != mesh.vertices.len() {
        return Err(SegmentationError::MeshLengthMismatch {
            vertices_len: mesh.vertices.len(),
            thickness_len: mesh.thickness.len(),
        });
    }

    // (2) Short-circuit on empty mask.
    if mask.voxels.is_empty() {
        return Ok(SegmentationResult {
            regions: vec![],
            vertex_labels: vec![u32::MAX; mesh.vertices.len()],
            triangle_labels: vec![u32::MAX; mesh.triangles.len()],
        });
    }

    // Build O(1) membership lookup.
    let mask_set: FxHashSet<[i32; 3]> = mask.voxels.iter().copied().collect();

    // BFS 6-face connected-component labelling.
    let mut voxel_to_label: FxHashMap<[i32; 3], u32> = FxHashMap::default();
    let mut region_voxels: Vec<Vec<[i32; 3]>> = Vec::new();
    let mut next_label: u32 = 0;

    for &seed in &mask.voxels {
        if voxel_to_label.contains_key(&seed) {
            continue; // already labelled
        }
        // New component — BFS from seed.
        let label = next_label;
        next_label += 1;
        let mut component: Vec<[i32; 3]> = Vec::new();
        let mut queue: VecDeque<[i32; 3]> = VecDeque::new();
        voxel_to_label.insert(seed, label);
        queue.push_back(seed);
        while let Some(v) = queue.pop_front() {
            component.push(v);
            // 6-face neighbors (±1 along each axis).
            let [i, j, k] = v;
            for neighbor in [
                [i + 1, j, k],
                [i - 1, j, k],
                [i, j + 1, k],
                [i, j - 1, k],
                [i, j, k + 1],
                [i, j, k - 1],
            ] {
                if mask_set.contains(&neighbor) && !voxel_to_label.contains_key(&neighbor) {
                    voxel_to_label.insert(neighbor, label);
                    queue.push_back(neighbor);
                }
            }
        }
        region_voxels.push(component);
    }

    let num_regions = region_voxels.len();

    // (3) Per-region bounding-box extent.
    let mut extents = vec![0.0f64; num_regions];
    for (label_idx, voxels) in region_voxels.iter().enumerate() {
        let mut idx_min = voxels[0];
        let mut idx_max = voxels[0];
        for &v in voxels.iter().skip(1) {
            for a in 0..3 {
                if v[a] < idx_min[a] {
                    idx_min[a] = v[a];
                }
                if v[a] > idx_max[a] {
                    idx_max[a] = v[a];
                }
            }
        }
        let extent = (0..3usize)
            .map(|a| (idx_max[a] - idx_min[a]) as f64 * mask.spacing[a])
            .fold(0.0f64, f64::max);
        extents[label_idx] = extent;
    }

    // (4) Per-vertex region label via 8-corner candidate lookup.
    let mut vertex_labels: Vec<u32> = vec![u32::MAX; mesh.vertices.len()];
    for (vi, &world) in mesh.vertices.iter().enumerate() {
        // Guard: skip non-finite vertices — vertex_labels[vi] already holds
        // u32::MAX from the initialiser above, so continue is sufficient.
        // Without this guard, NaN casts to 0i32 (Rust 1.45+ saturating cast)
        // and can silently match a real voxel near the origin in release builds;
        // debug builds would instead panic via axis_floor_ceil_unique's
        // debug_assert!.  Mirrors the !is_finite() pattern in medial.rs.
        if !world.iter().all(|c| c.is_finite()) {
            continue;
        }
        // Fractional voxel index along each axis.
        let f: [f64; 3] = std::array::from_fn(|a| (world[a] - mask.origin[a]) / mask.spacing[a]);
        // Enumerate unique floor/ceil corner candidates (1–8 per vertex).
        // Integer-aligned axes yield one candidate; fractional axes yield two.
        'candidate: for dz in axis_floor_ceil_unique(f[2]) {
            for dy in axis_floor_ceil_unique(f[1]) {
                for dx in axis_floor_ceil_unique(f[0]) {
                    let candidate = [dx, dy, dz];
                    if let Some(&lbl) = voxel_to_label.get(&candidate) {
                        vertex_labels[vi] = lbl;
                        break 'candidate;
                    }
                }
            }
        }
    }

    // Accumulate per-region thickness sums and counts.
    let mut thickness_sum = vec![0.0f64; num_regions];
    let mut thickness_count = vec![0usize; num_regions];
    for (vi, &lbl) in vertex_labels.iter().enumerate() {
        if (lbl as usize) < num_regions {
            thickness_sum[lbl as usize] += mesh.thickness[vi];
            thickness_count[lbl as usize] += 1;
        }
    }

    // (5) First-pass classification.
    let mut regions: Vec<RegionInfo> = region_voxels
        .into_iter()
        .enumerate()
        .map(|(label_idx, voxels)| {
            let label = label_idx as u32;
            let has_vertex_data = thickness_count[label_idx] > 0;
            let mean_thickness = if has_vertex_data {
                thickness_sum[label_idx] / thickness_count[label_idx] as f64
            } else {
                // No mid-surface vertices map into this region — degenerate case
                // (isolated mask cluster with no MC-active cells).  Use 0.0 so
                // `mean_thickness` is truthful, but do NOT classify as ShellEligible:
                // a region with no thickness data is treated as TetEligible below.
                0.0
            };
            let extent = extents[label_idx];
            let thickness_extent_ratio = if extent > 0.0 {
                mean_thickness / extent
            } else {
                f64::INFINITY
            };
            // Regions with no associated vertices (`has_vertex_data == false`) are
            // conservatively classified as TetEligible: a 0.0 mean_thickness would
            // otherwise produce ratio = 0.0 < threshold → ShellEligible, which is
            // misleading when the classification is based on absent mesh data.
            let classification =
                if has_vertex_data && thickness_extent_ratio < options.shell_threshold {
                    RegionClassification::ShellEligible
                } else {
                    RegionClassification::TetEligible
                };
            RegionInfo {
                label,
                voxels,
                mean_thickness,
                extent,
                thickness_extent_ratio,
                classification,
            }
        })
        .collect();

    // (6) Second-pass: promote ShellEligible → MixedComponentOfBody if the
    // body also contains TetEligible regions.
    //
    // Rationale: PRD §103 "mixed-component-of-body" is a body-level context
    // tag — the region is locally shell-able, but MPC tying at the
    // shell/tet interface (T10) is required.  TetEligible regions retain
    // their tag in a mixed body.
    let has_shell = regions
        .iter()
        .any(|r| r.classification == RegionClassification::ShellEligible);
    let has_tet = regions
        .iter()
        .any(|r| r.classification == RegionClassification::TetEligible);
    if has_shell && has_tet {
        for r in &mut regions {
            if r.classification == RegionClassification::ShellEligible {
                r.classification = RegionClassification::MixedComponentOfBody;
            }
        }
    }

    // Build triangle labels: use the first non-sentinel vertex label.
    // On a well-formed binary-MC mesh all three vertices share the same region,
    // but if vertex 0 carries the sentinel (floating-point boundary edge case),
    // falling back to vertex 1 or 2 produces a correct label rather than
    // propagating a spurious u32::MAX sentinel to the triangle.
    let triangle_labels: Vec<u32> = mesh
        .triangles
        .iter()
        .map(|tri| {
            let lbl = tri
                .iter()
                .map(|&v| vertex_labels[v as usize])
                .find(|&l| l != u32::MAX)
                .unwrap_or(u32::MAX);
            // In debug builds, assert all non-sentinel labels agree.
            #[cfg(debug_assertions)]
            {
                let non_sentinel: Vec<u32> = tri
                    .iter()
                    .map(|&v| vertex_labels[v as usize])
                    .filter(|&l| l != u32::MAX)
                    .collect();
                if non_sentinel.len() > 1 {
                    debug_assert!(
                        non_sentinel.iter().all(|&l| l == non_sentinel[0]),
                        "triangle vertices have inconsistent region labels: {:?}",
                        non_sentinel
                    );
                }
            }
            lbl
        })
        .collect();

    Ok(SegmentationResult {
        regions,
        vertex_labels,
        triangle_labels,
    })
}

/// Yield the unique integer voxel indices that bracket `coord` on one axis.
///
/// - If `coord` is integer-aligned (`floor == ceil`), yields a single value.
/// - If `coord` is fractional, yields `floor` then `ceil` (two distinct values).
///
/// Allocation-free: returns a chained iterator of at most two elements.
///
/// # Non-finite input
///
/// The production guard against non-finite input lives in [`segment_regions`],
/// which skips any vertex with a NaN or ±Inf world component before this helper
/// is ever called.  The `debug_assert!` below is an intentional
/// **development-time backstop**: if a future refactor introduces a call path
/// that bypasses the upstream guard, debug builds will panic loudly instead of
/// silently casting NaN to 0 (Rust 1.45+ saturating semantics).
fn axis_floor_ceil_unique(coord: f64) -> impl Iterator<Item = i32> {
    debug_assert!(
        coord.is_finite(),
        "axis_floor_ceil_unique: coord must be finite (got {coord})"
    );
    let floor = coord.floor() as i32;
    let ceil = coord.ceil() as i32;
    std::iter::once(floor).chain((floor != ceil).then_some(ceil))
}

#[cfg(test)]
mod tests {
    use super::axis_floor_ceil_unique;
    use std::collections::HashSet;

    use crate::mid_surface::{MidSurfaceOptions, extract_mid_surface};
    use crate::{
        MedialMask, MidSurfaceMesh, RegionClassification, RegionInfo, SegmentationError,
        SegmentationOptions, SegmentationResult, SingleBodyMask, segment_regions,
    };
    use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
    use std::sync::atomic::AtomicBool;

    // ── Test helpers (mirrored from mid_surface.rs) ───────────────────────────

    /// Build an analytic-slab Regular3D SampledField:
    /// `φ(x,y,z) = |z| - half_thickness_voxels` on an N×N×N grid
    /// centred at the origin with unit spacing.
    fn slab_sdf_3d(half_thickness_voxels: f64, voxel_count: usize) -> SampledField {
        let n = voxel_count;
        let spacing: f64 = 1.0;
        let half_extent = (n as f64 - 1.0) / 2.0;
        let bounds_min = -half_extent;
        let bounds_max = half_extent;
        let axis_grid: Vec<f64> = (0..n)
            .map(|idx| bounds_min + (idx as f64) * spacing)
            .collect();
        let mut data = Vec::with_capacity(n * n * n);
        for &_x in &axis_grid {
            for &_y in &axis_grid {
                for &z in &axis_grid {
                    data.push(z.abs() - half_thickness_voxels);
                }
            }
        }
        SampledField {
            name: format!("slab-{half_thickness_voxels}-{voxel_count}"),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![bounds_min, bounds_min, bounds_min],
            bounds_max: vec![bounds_max, bounds_max, bounds_max],
            spacing: vec![spacing, spacing, spacing],
            axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build the centerline MedialMask for a z-slab on an N×N×N grid:
    /// every voxel `(i, j, center_k)` for `i, j ∈ 0..n`.
    fn centerline_mask(n: usize, sdf: &SampledField) -> MedialMask {
        let center_k = (n as i32 - 1) / 2;
        let voxels: Vec<[i32; 3]> = (0..n as i32)
            .flat_map(|i| (0..n as i32).map(move |j| [i, j, center_k]))
            .collect();
        MedialMask {
            spacing: [sdf.spacing[0], sdf.spacing[1], sdf.spacing[2]],
            origin: [sdf.bounds_min[0], sdf.bounds_min[1], sdf.bounds_min[2]],
            voxels,
        }
    }

    // ── Step 1: public-surface smoke test ────────────────────────────────────

    /// Smoke test: all public types are reachable from the crate root and
    /// `segment_regions` is callable.  Empty mask + empty mesh → `Ok` with
    /// all output vecs empty.
    #[test]
    fn segment_regions_public_surface_is_callable_on_empty_mask() {
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let result: SegmentationResult = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("empty mask + empty mesh should return Ok");
        assert!(result.regions.is_empty(), "empty mask → no regions");
        assert!(
            result.vertex_labels.is_empty(),
            "empty mesh → no vertex labels"
        );
        assert!(
            result.triangle_labels.is_empty(),
            "empty mesh → no triangle labels"
        );
        // Compile-probes: error type and subtypes are reachable.
        let _: SegmentationError = SegmentationError::InvalidThreshold { value: 0.0 };
        let _: Option<RegionInfo> = None;
        let _: Option<RegionClassification> = None;
    }

    // ── Step 3: threshold validation ─────────────────────────────────────────

    /// `segment_regions` rejects zero or negative `shell_threshold`.
    #[test]
    fn segment_regions_rejects_zero_or_negative_threshold() {
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![],
        };
        let mesh = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        assert_eq!(
            segment_regions(
                &SingleBodyMask::new(mask.clone()),
                &mesh,
                &SegmentationOptions {
                    shell_threshold: 0.0
                }
            ),
            Err(SegmentationError::InvalidThreshold { value: 0.0 }),
            "zero threshold must be rejected"
        );
        assert_eq!(
            segment_regions(
                &SingleBodyMask::new(mask.clone()),
                &mesh,
                &SegmentationOptions {
                    shell_threshold: -0.1
                }
            ),
            Err(SegmentationError::InvalidThreshold { value: -0.1 }),
            "negative threshold must be rejected"
        );
    }

    // ── Step 5: mesh-length mismatch validation ───────────────────────────────

    /// `segment_regions` rejects a mesh where `thickness.len() ≠ vertices.len()`.
    #[test]
    fn segment_regions_rejects_mesh_length_mismatch() {
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[0, 0, 0]],
        };
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![],
            thickness: vec![1.0, 2.0], // length 2 ≠ vertices length 3
        };
        assert_eq!(
            segment_regions(
                &SingleBodyMask::new(mask),
                &mesh,
                &SegmentationOptions::default()
            ),
            Err(SegmentationError::MeshLengthMismatch {
                vertices_len: 3,
                thickness_len: 2
            })
        );
    }

    // ── Step 7: single-slab CC ────────────────────────────────────────────────

    /// Single slab → 1 connected component containing all mask voxels.
    #[test]
    fn segment_regions_on_single_slab_yields_one_region_with_all_mask_voxels() {
        let n = 17;
        let sdf = slab_sdf_3d(1.0, n);
        let mask = centerline_mask(n, &sdf);
        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("T2 extraction should succeed");

        let result = segment_regions(
            &SingleBodyMask::new(mask.clone()),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed on slab");

        assert_eq!(result.regions.len(), 1, "one connected component");
        assert_eq!(result.regions[0].label, 0);
        assert_eq!(
            result.regions[0].voxels.len(),
            mask.voxels.len(),
            "region contains all mask voxels"
        );
        // Exact set equality.
        let region_set: HashSet<[i32; 3]> = result.regions[0].voxels.iter().copied().collect();
        let mask_set: HashSet<[i32; 3]> = mask.voxels.iter().copied().collect();
        assert_eq!(
            region_set, mask_set,
            "region voxel set equals mask voxel set"
        );
    }

    // ── Step 9: two disjoint slabs → two CCs ─────────────────────────────────

    /// Two disconnected voxel clusters → two distinct regions.
    #[test]
    fn segment_regions_on_two_disjoint_slabs_yields_two_regions() {
        // Cluster A: z-plane at k=4, all (i,j) in 0..8 on a 16³ logical grid.
        // Cluster B: z-plane at k=12, all (i,j) in 0..8.
        // The two planes are separated by 7 voxels — face-disconnected.
        let spacing = [1.0f64; 3];
        let origin = [0.0f64; 3];
        let cluster_a: Vec<[i32; 3]> = (0..8i32)
            .flat_map(|i| (0..8i32).map(move |j| [i, j, 4]))
            .collect();
        let cluster_b: Vec<[i32; 3]> = (0..8i32)
            .flat_map(|i| (0..8i32).map(move |j| [i, j, 12]))
            .collect();
        let mut voxels = cluster_a.clone();
        voxels.extend_from_slice(&cluster_b);
        let mask = MedialMask {
            spacing,
            origin,
            voxels,
        };

        // Minimal mesh consistent with validation (length-3 thickness for 3 vertices).
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 4.0], [1.0, 0.0, 4.0], [0.0, 0.0, 12.0]],
            triangles: vec![],
            thickness: vec![1.0, 1.0, 1.0],
        };

        let result = segment_regions(
            &SingleBodyMask::new(mask.clone()),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed");

        assert_eq!(
            result.regions.len(),
            2,
            "two disjoint clusters → two regions"
        );
        // Labels are in {0, 1}.
        let labels: HashSet<u32> = result.regions.iter().map(|r| r.label).collect();
        assert_eq!(labels, HashSet::from([0, 1]));

        // Identify regions by representative voxel (not by positional index) so
        // the test is robust to BFS order.
        let region_a = result
            .regions
            .iter()
            .find(|r| {
                r.voxels
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    .contains(&[0i32, 0, 4])
            })
            .expect("region containing cluster-A representative voxel [0,0,4] must exist");
        let region_b = result
            .regions
            .iter()
            .find(|r| {
                r.voxels
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    .contains(&[0i32, 0, 12])
            })
            .expect("region containing cluster-B representative voxel [0,0,12] must exist");

        // Each cluster is an 8×8 z-plane → 64 voxels.
        assert_eq!(
            region_a.voxels.len(),
            64,
            "cluster A must have 64 voxels (8×8 z-plane)"
        );
        assert_eq!(
            region_b.voxels.len(),
            64,
            "cluster B must have 64 voxels (8×8 z-plane)"
        );

        // In-plane bounding-box extent = max(i-span, j-span, k-span).
        // i-span = 7, j-span = 7, k-span = 0 (single z-plane) → extent = 7.0.
        assert!(
            (region_a.extent - 7.0).abs() < 0.01,
            "cluster A extent ≈ 7.0 (got {})",
            region_a.extent
        );
        assert!(
            (region_b.extent - 7.0).abs() < 0.01,
            "cluster B extent ≈ 7.0 (got {})",
            region_b.extent
        );

        // Voxel sets are disjoint and their union equals the mask.
        let set_a: HashSet<[i32; 3]> = region_a.voxels.iter().copied().collect();
        let set_b: HashSet<[i32; 3]> = region_b.voxels.iter().copied().collect();
        assert!(set_a.is_disjoint(&set_b), "regions must be disjoint");
        let union: HashSet<[i32; 3]> = set_a.union(&set_b).copied().collect();
        let mask_set: HashSet<[i32; 3]> = mask.voxels.iter().copied().collect();
        assert_eq!(union, mask_set, "union of region voxels equals mask");
    }

    // ── Step 11: slab metrics + vertex/triangle labels ────────────────────────

    /// Single slab → ShellEligible with correct thickness, extent, and labels.
    #[test]
    fn segment_regions_on_thin_slab_classifies_as_shell_eligible_with_correct_metrics() {
        let n = 17;
        let sdf = slab_sdf_3d(1.0, n);
        let mask = centerline_mask(n, &sdf);
        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("T2 extraction should succeed");

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed");

        let region = &result.regions[0];
        assert_eq!(
            region.classification,
            RegionClassification::ShellEligible,
            "slab with ratio ≈ 0.125 < 0.2 is ShellEligible"
        );
        assert!(
            (region.mean_thickness - 2.0).abs() < 0.5,
            "mean_thickness ≈ 2.0 (got {})",
            region.mean_thickness
        );
        assert!(
            (region.extent - 16.0).abs() < 0.5,
            "extent ≈ 16.0 (n-1 voxels × unit spacing) (got {})",
            region.extent
        );
        assert!(
            (region.thickness_extent_ratio - 0.125).abs() < 0.02,
            "ratio ≈ 0.125 (got {})",
            region.thickness_extent_ratio
        );

        // Vertex and triangle labels: same length as mesh, all 0 (single region).
        assert_eq!(result.vertex_labels.len(), mesh.vertices.len());
        assert_eq!(result.triangle_labels.len(), mesh.triangles.len());
        assert!(
            result.vertex_labels.iter().all(|&l| l == 0),
            "all vertices belong to region 0"
        );
        assert!(
            result.triangle_labels.iter().all(|&l| l == 0),
            "all triangles belong to region 0"
        );
    }

    // ── Step 13: cube cluster → TetEligible ──────────────────────────────────

    /// 3×3×3 cube cluster with high thickness → TetEligible.
    #[test]
    fn segment_regions_on_cube_cluster_classifies_as_tet_eligible() {
        let voxels: Vec<[i32; 3]> = (0..3i32)
            .flat_map(|i| (0..3i32).flat_map(move |j| (0..3i32).map(move |k| [i, j, k])))
            .collect();
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels,
        };
        // One vertex at cube centroid with high thickness.
        let mesh = MidSurfaceMesh {
            vertices: vec![[1.0, 1.0, 1.0]],
            triangles: vec![],
            thickness: vec![3.0],
        };

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed");

        assert_eq!(result.regions.len(), 1);
        let region = &result.regions[0];
        assert!(
            (region.mean_thickness - 3.0).abs() < 0.01,
            "mean_thickness ≈ 3.0 (got {})",
            region.mean_thickness
        );
        assert!(
            (region.extent - 2.0).abs() < 0.01,
            "extent = (3-1)×1 = 2.0 (got {})",
            region.extent
        );
        assert!(
            (region.thickness_extent_ratio - 1.5).abs() < 0.01,
            "ratio ≈ 1.5 (got {})",
            region.thickness_extent_ratio
        );
        assert_eq!(
            region.classification,
            RegionClassification::TetEligible,
            "ratio 1.5 >> 0.2 threshold → TetEligible"
        );
    }

    // ── Thickness averaging with multiple vertices per region ────────────────

    /// Multiple mesh vertices mapping into the same region must be averaged
    /// correctly.  Thicknesses (1.0, 2.0, 3.0) → mean 2.0.
    ///
    /// This guards against regressions where the accumulator divides by the
    /// wrong denominator (e.g. `thickness_count.len()` instead of
    /// `thickness_count[label_idx]`).
    #[test]
    fn segment_regions_averages_per_region_thickness_over_multiple_vertices() {
        // Three collinear voxels at y=0, z=0 with x ∈ {0, 1, 2} → one connected
        // component (face-adjacent along x).
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[0, 0, 0], [1, 0, 0], [2, 0, 0]],
        };
        // Three mesh vertices, one per voxel centroid, with non-uniform thicknesses.
        let mesh = MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            triangles: vec![],
            thickness: vec![1.0, 2.0, 3.0],
        };

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed");

        assert_eq!(result.regions.len(), 1, "collinear voxels → one component");
        let region = &result.regions[0];
        assert!(
            (region.mean_thickness - 2.0).abs() < 1e-10,
            "mean of (1.0, 2.0, 3.0) = 2.0 (got {})",
            region.mean_thickness
        );
        // extent = max(x-span=2, y-span=0, z-span=0) × spacing 1.0 = 2.0
        assert!(
            (region.extent - 2.0).abs() < 1e-10,
            "extent ≈ 2.0 (got {})",
            region.extent
        );
        // ratio = 2.0 / 2.0 = 1.0 > 0.2 → TetEligible
        assert_eq!(
            region.classification,
            RegionClassification::TetEligible,
            "ratio 1.0 > 0.2 → TetEligible"
        );
    }

    // ── Step 15: mixed body → ShellEligible promoted to MixedComponentOfBody ──

    /// Two disjoint regions (one shell-able, one tet-able) → shell region
    /// promoted to MixedComponentOfBody.
    #[test]
    fn segment_regions_on_mixed_body_promotes_shell_regions_to_mixed_component() {
        // Component A: z-plane slab-style centerline at k=2 for i,j in 0..16.
        // Locally shell-able (mean_thickness ≈ 2.0, extent = 15.0, ratio ≈ 0.133).
        let slab_voxels: Vec<[i32; 3]> = (0..16i32)
            .flat_map(|i| (0..16i32).map(move |j| [i, j, 2]))
            .collect();

        // Component B: 3×3×3 cube offset well away from A (i,j,k in 20..23).
        let cube_voxels: Vec<[i32; 3]> = (20..23i32)
            .flat_map(|i| (20..23i32).flat_map(move |j| (20..23i32).map(move |k| [i, j, k])))
            .collect();

        let mut voxels = slab_voxels.clone();
        voxels.extend_from_slice(&cube_voxels);
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels,
        };

        // One representative vertex per component.
        // Slab vertex: centroid of slab plane (7.5, 7.5, 2.0) — we use (7.0, 7.0, 2.0)
        // which maps to voxel [7, 7, 2] ∈ slab_voxels.  Thickness ≈ 2.0.
        // Cube vertex: centroid (21.0, 21.0, 21.0).  Thickness ≈ 3.0.
        let mesh = MidSurfaceMesh {
            vertices: vec![[7.0, 7.0, 2.0], [21.0, 21.0, 21.0]],
            triangles: vec![],
            thickness: vec![2.0, 3.0],
        };

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions should succeed");

        assert_eq!(result.regions.len(), 2);

        // Identify which region is the slab and which is the cube.
        let slab_region = result
            .regions
            .iter()
            .find(|r| r.voxels.contains(&[0, 0, 2]))
            .expect("slab region must exist");
        let cube_region = result
            .regions
            .iter()
            .find(|r| r.voxels.contains(&[20, 20, 20]))
            .expect("cube region must exist");

        // Slab: locally shell-able but promoted to MixedComponentOfBody.
        assert!(
            slab_region.thickness_extent_ratio < 0.2,
            "slab ratio ({}) should be < 0.2 without promotion",
            slab_region.thickness_extent_ratio
        );
        assert_eq!(
            slab_region.classification,
            RegionClassification::MixedComponentOfBody,
            "shell-eligible slab in mixed body → MixedComponentOfBody"
        );

        // Cube: TetEligible (not promoted).
        assert_eq!(
            cube_region.classification,
            RegionClassification::TetEligible,
            "tet-eligible cube in mixed body stays TetEligible"
        );
    }

    // ── Step 17: threshold flip ───────────────────────────────────────────────

    /// Tightening shell_threshold flips the slab from ShellEligible to TetEligible.
    #[test]
    fn tightening_shell_threshold_flips_slab_classification_to_tet_eligible() {
        let n = 17;
        let sdf = slab_sdf_3d(1.0, n);
        let mask = centerline_mask(n, &sdf);
        let mesh = extract_mid_surface(&sdf, &mask, &MidSurfaceOptions::default())
            .expect("T2 extraction should succeed");

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions {
                shell_threshold: 0.001,
            },
        )
        .expect("segment_regions should succeed");

        assert_eq!(result.regions.len(), 1);
        assert_eq!(
            result.regions[0].classification,
            RegionClassification::TetEligible,
            "ratio ≈ 0.125 > 0.001 threshold → TetEligible"
        );
    }

    // ── Task 3136: axis_floor_ceil_unique dedup ──────────────────────────────

    /// `axis_floor_ceil_unique` must yield a single value for integer-aligned
    /// coordinates and two distinct values for fractional coordinates.
    #[test]
    fn axis_floor_ceil_unique_dedups_integer_coords() {
        assert_eq!(
            axis_floor_ceil_unique(0.0).collect::<Vec<_>>(),
            vec![0],
            "0.0 is integer-aligned → single yield"
        );
        assert_eq!(
            axis_floor_ceil_unique(-3.0).collect::<Vec<_>>(),
            vec![-3],
            "-3.0 is integer-aligned → single yield"
        );
        assert_eq!(
            axis_floor_ceil_unique(0.5).collect::<Vec<_>>(),
            vec![0, 1],
            "0.5 is fractional → floor=0, ceil=1"
        );
        assert_eq!(
            axis_floor_ceil_unique(-1.5).collect::<Vec<_>>(),
            vec![-2, -1],
            "-1.5 is fractional → floor=-2, ceil=-1"
        );
    }

    /// The triple-nested loop over `axis_floor_ceil_unique` must visit the
    /// correct number of unique candidates: 1 for all-integer coords, up to
    /// 8 for all-fractional coords.
    #[test]
    fn segment_regions_corner_enumeration_visits_each_unique_candidate_once() {
        fn count_candidates(f: [f64; 3]) -> usize {
            let mut n = 0usize;
            for _dz in axis_floor_ceil_unique(f[2]) {
                for _dy in axis_floor_ceil_unique(f[1]) {
                    for _dx in axis_floor_ceil_unique(f[0]) {
                        n += 1;
                    }
                }
            }
            n
        }

        assert_eq!(
            count_candidates([0.0, 0.0, 0.0]),
            1,
            "all-integer → 1 candidate"
        );
        assert_eq!(
            count_candidates([0.5, 0.0, 0.0]),
            2,
            "one-fractional → 2 candidates"
        );
        assert_eq!(
            count_candidates([0.5, 0.5, 0.0]),
            4,
            "two-fractional → 4 candidates"
        );
        assert_eq!(
            count_candidates([0.5, 0.5, 0.5]),
            8,
            "all-fractional → 8 candidates"
        );
    }

    /// Regression: a vertex whose world position maps to an integer voxel index
    /// (floor == ceil on every axis) must still be correctly labeled.
    /// This exercises the dedup path's single-candidate case.
    #[test]
    fn segment_regions_correctly_labels_vertex_at_integer_voxel_position() {
        // Single voxel at index [3, 4, 5].
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[3, 4, 5]],
        };
        // One vertex whose fractional index is exactly [3.0, 4.0, 5.0].
        // With origin=[0,0,0] and spacing=[1,1,1]:
        //   f[a] = (world[a] - origin[a]) / spacing[a] = world[a].
        let mesh = MidSurfaceMesh {
            vertices: vec![[3.0, 4.0, 5.0]],
            triangles: vec![],
            thickness: vec![1.0],
        };
        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("single voxel + single vertex → Ok");
        assert_eq!(
            result.vertex_labels,
            vec![0],
            "vertex at integer voxel position must be labeled with the only region (0)"
        );
    }

    // ── Task 3149: non-finite vertex coords → sentinel label ─────────────────

    /// Guard: vertices with non-finite world coordinates must receive the
    /// `u32::MAX` sentinel label, not a silently-wrong region label.
    ///
    /// ## Why this matters
    ///
    /// In Rust 1.45+, `f64::NAN as i32 == 0`.  Without an explicit finite
    /// check, a NaN-component vertex computes fractional indices that cast
    /// to 0, which can accidentally match a real voxel at index `[0, 0, 0]`
    /// and silently receive a non-sentinel label.  In debug builds the
    /// helper's `debug_assert!` catches this first, producing a panic
    /// instead.  Either way the pre-fix behaviour is wrong.
    ///
    /// Post-fix: every non-finite vertex is short-circuited before the
    /// fractional-index computation; `vertex_labels[vi]` retains its
    /// `u32::MAX` initialiser without any explicit re-assignment.
    #[test]
    fn segment_regions_assigns_sentinel_label_to_non_finite_vertex_coords() {
        // Single-voxel mask at [0, 0, 0] so that the NaN→0i32 saturating cast
        // would accidentally hit a real voxel pre-fix.
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[0, 0, 0]],
        };
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let neg_inf = f64::NEG_INFINITY;
        let mesh = MidSurfaceMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],     // 0: finite → must label 0
                [nan, 0.0, 0.0],     // 1: NaN x-component
                [0.0, nan, 0.0],     // 2: NaN y-component
                [0.0, 0.0, nan],     // 3: NaN z-component
                [inf, neg_inf, nan], // 4: all non-finite
            ],
            // tri 0: all three vertices non-finite → triangle must receive u32::MAX
            // tri 1: one finite vertex (0) + two non-finite (1, 2) → inherits label 0
            triangles: vec![[1, 2, 3], [0, 1, 2]],
            thickness: vec![1.0; 5],
        };

        let result = segment_regions(
            &SingleBodyMask::new(mask),
            &mesh,
            &SegmentationOptions::default(),
        )
        .expect("segment_regions must not error or panic on non-finite vertex coords");

        // ── vertex labels ────────────────────────────────────────────────────
        assert_eq!(
            result.vertex_labels[0], 0,
            "finite vertex [0.0, 0.0, 0.0] must be labelled with region 0"
        );
        assert_eq!(
            result.vertex_labels[1],
            u32::MAX,
            "vertex with NaN x-component must receive sentinel u32::MAX \
             (not a spurious 0 due to NaN→0i32 cast)"
        );
        assert_eq!(
            result.vertex_labels[2],
            u32::MAX,
            "vertex with NaN y-component must receive sentinel u32::MAX"
        );
        assert_eq!(
            result.vertex_labels[3],
            u32::MAX,
            "vertex with NaN z-component must receive sentinel u32::MAX"
        );
        assert_eq!(
            result.vertex_labels[4],
            u32::MAX,
            "vertex with all-non-finite coords (±Inf, NaN) must receive sentinel u32::MAX"
        );

        // ── triangle labels ──────────────────────────────────────────────────
        // A triangle whose every vertex is non-finite (all sentinel) must itself
        // receive the u32::MAX sentinel — the `find(non-sentinel)` in the
        // triangle-label builder returns None.
        assert_eq!(
            result.triangle_labels[0],
            u32::MAX,
            "triangle [1, 2, 3] — all non-finite vertices — must propagate u32::MAX sentinel"
        );
        // A triangle with at least one finite vertex must inherit that vertex's
        // region label, even when the others carry the sentinel.
        assert_eq!(
            result.triangle_labels[1], 0,
            "triangle [0, 1, 2] — finite vertex 0 (label 0) + two non-finite — must label 0"
        );
    }

    // ── Step 19: defaults pin ────────────────────────────────────────────────

    /// Pin SegmentationOptions default values against accidental drift.
    #[test]
    fn segmentation_options_defaults_pin_empirical_constants() {
        // Destructure-without-`..` ensures the test fails to compile if any
        // field is added, removed, or renamed — catching drift at compile time.
        let SegmentationOptions { shell_threshold } = SegmentationOptions::default();
        assert_eq!(
            shell_threshold, 0.2,
            "shell_threshold default must match PRD ElasticOptions.shell_threshold (L/t > 5)"
        );
    }

    // ── Task 3137: SingleBodyMask newtype wrapper ────────────────────────────

    /// `SingleBodyMask::new` + `inner()` round-trips the wrapped `MedialMask`.
    #[test]
    fn single_body_mask_wraps_medial_mask_via_new_and_inner() {
        let mask = MedialMask {
            spacing: [1.0, 1.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            voxels: vec![[1, 2, 3]],
        };
        assert_eq!(SingleBodyMask::new(mask.clone()).inner(), &mask);
    }
}
