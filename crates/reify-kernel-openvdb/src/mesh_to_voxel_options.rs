//! `MeshToVoxelOptions` — OpenVDB Mesh→Voxel conversion parameters and their
//! `ContentHash` producer.
//!
//! PRD §4 producer-registry table: producer crate = `reify-kernel-openvdb`.
//! The struct mirrors the two dominant precision parameters passed to
//! `openvdb::tools::meshToVolume` (PRD §8 task η).
//!
//! # ESC-3433-117 carry-forward — non-zero domain tag invariant
//!
//! `content_hash()` seeds with `ContentHash::of_str("MeshToVoxelOptions")` so
//! that `MeshToVoxelOptions::default().content_hash()` cannot equal the
//! `NO_OPTIONS` sentinel (`ContentHash(0)` at
//! `crates/reify-eval/src/realization_cache.rs:85`).  A collision would let a
//! MeshToVoxelOptions-keyed Voxel entry alias a NO_OPTIONS-keyed entry in the
//! same `ToleranceBucket`, silently returning wrong cached geometry.
//! Pinned by the unit test `default_content_hash_is_not_no_options_sentinel`.

use reify_core::ContentHash;
use reify_ir::{Mesh, VoxelResolution};

/// OpenVDB Mesh→Voxel conversion options.
///
/// Fields map directly to `openvdb::tools::meshToVolume` parameters:
/// - `voxel_size`: side length of one voxel (same units as the mesh vertices).
/// - `narrow_band`: narrow-band half-width in voxels (maps to `half_width_voxels`
///   in the FFI call convention; e.g. `3.0` is the OpenVDB default).
///
/// # No `Eq` / `Hash` derives
///
/// `f64` does not implement `Eq` or `Hash` (NaN ≠ NaN). Use
/// [`MeshToVoxelOptions::content_hash()`] for equality / caching comparisons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshToVoxelOptions {
    /// Side length of one voxel in the same units as the input mesh vertices.
    pub voxel_size: f64,

    /// Narrow-band half-width in voxels (passed as `half_width_voxels` to the
    /// FFI). OpenVDB default: `3.0`.
    pub narrow_band: f64,
}

impl Default for MeshToVoxelOptions {
    /// Returns OpenVDB `meshToVolume` defaults: `voxel_size = 0.1`,
    /// `narrow_band = 3.0` half-width voxels.
    fn default() -> Self {
        Self {
            voxel_size: 0.1,
            narrow_band: 3.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Honest-floor resolution constants (PRD §3b + §6 D7 — tunable)
// ---------------------------------------------------------------------------

/// Number of voxels along the longest bounding-box axis.
///
/// `voxel_size = h = longest_extent / VOXELS_PER_LONGEST_AXIS`.
///
/// Tunable per PRD §6 D7 ("measure first, then tune"). The value 64.0 is the
/// v0.1 conservative default — it resolves a 100 mm part at ~1.6 mm/voxel,
/// which is coarser than a final-quality mesh but correct for α/β/γ
/// thickness-DFM prototype development.
///
/// Increasing N increases memory quadratically in the interior-covering band;
/// decreasing N lowers resolution.  Both `honest_floor` tests and the δ medial
/// walk read this constant so tuning is a single-line change.
pub const VOXELS_PER_LONGEST_AXIS: f64 = 64.0;

/// Extra voxels added to the band half-width beyond the minimum needed to
/// cover the part interior.
///
/// `narrow_band = VOXELS_PER_LONGEST_AXIS / 2 + BAND_MARGIN_VOXELS`
///
/// The extra margin ensures the band covers the deepest interior point even
/// after floating-point rounding in `meshToLevelSet`'s half_width_voxels
/// parameter.  2.0 extra voxels is the PRD §6 D7 conservative default.
pub const BAND_MARGIN_VOXELS: f64 = 2.0;

/// Number of voxels required ACROSS the thinnest feature a caller asks to
/// resolve (task 6560): `voxel_size = h = min_feature / MIN_FEATURE_VOXELS_ACROSS`.
///
/// # Why 4 and not the PRD's "≈ thickness/3"
///
/// The v0.4-shells gate (`docs/prds/v0_4/structural-analysis-shells.md`) asks
/// for "≈ thickness/3 voxel size for the thinnest expected feature". At
/// `h = t/3` the half-thickness of that feature is `1.5 h` — BELOW the
/// empirically-established OpenVDB interior-signing floor of
/// "half-thickness ≥ 2 × voxel_size" documented at
/// `crates/reify-eval/tests/harness_kernel_realization/realization_read_api.rs:505-510`,
/// so the feature's interior can fail to sign negative at all. `4.0` is the
/// COARSEST value clearing that floor (`half-thickness = 2 h`), and being
/// finer than `t/3` it satisfies the gate's "resolutions sufficient for".
///
/// Tunable on the same "measure first, then tune" footing as
/// [`VOXELS_PER_LONGEST_AXIS`] (PRD §6 D7): raising it refines the grid and
/// raises cost cubically; lowering it below 4.0 re-enters the unsigned-interior
/// regime and must not be done without re-measuring that floor.
pub const MIN_FEATURE_VOXELS_ACROSS: f64 = 4.0;

/// Why a resolution request could not be turned into [`MeshToVoxelOptions`].
///
/// Shape follows [`crate::ingest::IngestError`] (`ingest.rs:85`): a plain
/// `Debug + Clone + PartialEq` enum with a hand-written [`std::fmt::Display`]
/// and a blanket [`std::error::Error`] impl, so the message is the single
/// source of truth for what the caller sees. `OpenVdbKernel` bridges it into
/// `GeometryError::OperationFailed` via that `Display`.
#[derive(Debug, Clone, PartialEq)]
pub enum VoxelResolutionError {
    /// The mesh has no usable bounding box, so no voxel size can be derived
    /// from it and none of the budget arithmetic is meaningful.
    ///
    /// Exactly the conditions under which [`MeshToVoxelOptions::honest_floor`]
    /// returns `None`: an empty `vertices` buffer, a mesh whose vertices are
    /// all coincident (zero extent on every axis), or any non-finite (NaN /
    /// Inf) vertex coordinate.
    DegenerateMesh,

    /// A caller-supplied length in the resolution request was not finite and
    /// strictly positive.
    ///
    /// Carries the offending value verbatim (including its NaN payload — the
    /// field is compared by bits in the unit tests) and the request variant it
    /// came from, so the diagnostic names both what was asked for and where.
    InvalidRequest {
        /// The offending value exactly as requested.
        requested: f64,
        /// Name of the [`VoxelResolution`] variant that carried it, e.g.
        /// `"MinFeature"`.
        variant: &'static str,
    },
}

impl std::fmt::Display for VoxelResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DegenerateMesh => write!(
                f,
                "cannot derive a voxel resolution: the mesh has no usable bounding box \
                 (empty, all vertices coincident, or a non-finite coordinate)"
            ),
            Self::InvalidRequest { requested, variant } => write!(
                f,
                "invalid VoxelResolution::{variant}({requested}): the requested length \
                 must be finite and strictly positive"
            ),
        }
    }
}

impl std::error::Error for VoxelResolutionError {}

/// Axis-aligned bounding-box extents `[dx, dy, dz]` of `mesh`, in `f64`.
///
/// Returns `None` under exactly the conditions
/// [`MeshToVoxelOptions::honest_floor`] has always rejected — it is that
/// function's original preamble, lifted verbatim so `honest_floor` and
/// [`MeshToVoxelOptions::for_resolution`] cannot drift apart on what counts as
/// a degenerate mesh:
///
/// - `vertices` is empty;
/// - any coordinate is non-finite (NaN or Inf) — rejected on the FIRST such
///   coordinate rather than skipped, because the NaN-comparison short-circuit
///   in `v < min` / `v > max` would otherwise yield a plausible-looking bbox
///   that silently ignores the offending vertex;
/// - any extent is non-finite, or the longest extent is not positive (all
///   vertices coincident).
fn bbox_extents(mesh: &Mesh) -> Option<[f64; 3]> {
    if mesh.vertices.is_empty() {
        return None;
    }

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for chunk in mesh.vertices.chunks_exact(3) {
        for axis in 0..3 {
            let v = chunk[axis];
            if !v.is_finite() {
                return None;
            }
            if v < min[axis] {
                min[axis] = v;
            }
            if v > max[axis] {
                max[axis] = v;
            }
        }
    }

    let extents = [
        (max[0] - min[0]) as f64,
        (max[1] - min[1]) as f64,
        (max[2] - min[2]) as f64,
    ];
    for &e in &extents {
        if !e.is_finite() {
            return None;
        }
    }
    if extents[0].max(extents[1]).max(extents[2]) <= 0.0 {
        return None;
    }
    Some(extents)
}

/// Reject a caller-supplied length that is not finite and strictly positive.
///
/// `variant` names the [`VoxelResolution`] arm the value came from so the
/// resulting [`VoxelResolutionError::InvalidRequest`] message points at the
/// request site rather than at the arithmetic.
fn validate_requested_length(
    value: f64,
    variant: &'static str,
) -> Result<(), VoxelResolutionError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(VoxelResolutionError::InvalidRequest {
            requested: value,
            variant,
        });
    }
    Ok(())
}

impl MeshToVoxelOptions {
    /// Derive honest-floor resolution options from the mesh bounding box.
    ///
    /// # Resolution policy (PRD §3b honest-floor)
    ///
    /// `voxel_size = h = longest_extent / VOXELS_PER_LONGEST_AXIS`
    ///
    /// The voxel size scales with the part so a 2 mm cube and a 200 mm part
    /// both get `VOXELS_PER_LONGEST_AXIS` voxels across their longest axis —
    /// a fixed `voxel_size` (e.g. the struct default 0.1) would be meaningless
    /// across unit systems.
    ///
    /// # Band-covers-interior invariant (critical for densify correctness)
    ///
    /// `narrow_band = VOXELS_PER_LONGEST_AXIS / 2 + BAND_MARGIN_VOXELS`
    ///
    /// `openvdb::tools::meshToLevelSet` builds a NARROW-BAND level set:
    /// voxels BEYOND `±narrow_band` from the surface are saturated to
    /// `±(narrow_band × voxel_size)`.  After densification deep-interior
    /// voxels read the saturated background rather than the true SDF value.
    /// Setting `narrow_band × h ≥ longest_extent/2 ≥ deepest interior point`
    /// ensures the band reaches every interior point, making φ(centre) the
    /// true geometric distance (not a saturated sentinel).  This is also
    /// required by δ's min-wall medial walk — saturated interior SDF values
    /// would produce garbage wall-thickness estimates.
    ///
    /// # Returns
    ///
    /// - `Some(opts)` for a valid mesh with at least one vertex and a
    ///   positive, finite bounding-box extent on every axis.
    /// - `None` for an empty mesh (`vertices` is empty), a mesh where all
    ///   vertices are coincident (zero extent on every axis), or a mesh
    ///   containing any non-finite (NaN or Inf) vertex coordinate.
    ///
    /// Note: a single NaN or Inf coordinate in any vertex is enough to
    /// return `None` — the function does not skip bad coordinates and
    /// compute a bbox over the remaining valid vertices.  A partial-bad
    /// mesh would yield a misleadingly-tight bbox; returning `None` forces
    /// the caller to reject or clean the mesh before voxelization.
    pub fn honest_floor(mesh: &Mesh) -> Option<Self> {
        let extents = bbox_extents(mesh)?;

        let longest = extents[0].max(extents[1]).max(extents[2]);

        let voxel_size = longest / VOXELS_PER_LONGEST_AXIS;
        // narrow_band × h ≥ longest/2 ≥ any interior point:
        //   narrow_band = N/2 + margin  →  depth = narrow_band × h
        //               = (N/2 + margin) × (longest/N)
        //               = longest/2 + margin × longest/N
        //               ≥ longest/2  ✓
        let narrow_band = VOXELS_PER_LONGEST_AXIS / 2.0 + BAND_MARGIN_VOXELS;
        Some(Self { voxel_size, narrow_band })
    }

    /// Derive conversion options from a kernel-agnostic [`VoxelResolution`]
    /// request (task 6560 — the v0.4-shells `BRep→Voxel` resolution seam).
    ///
    /// # Why this exists alongside `honest_floor`
    ///
    /// [`Self::honest_floor`] derives the voxel size from the bounding box
    /// ALONE (`longest_extent / VOXELS_PER_LONGEST_AXIS`), so it knows nothing
    /// about the features inside the part. On the shells PRD's own motivating
    /// geometry — a 1 mm flexure in a 100 mm part
    /// (`docs/prds/v0_4/structural-analysis-shells.md`, "Background") — that
    /// yields 1.5625 mm/voxel and the feature is entirely sub-voxel.
    /// `for_resolution` is the seam through which a caller that KNOWS its
    /// thinnest feature can ask for a grid that actually resolves it.
    ///
    /// # Per-variant behaviour
    ///
    /// - [`VoxelResolution::HonestFloor`] — delegated VERBATIM to
    ///   [`Self::honest_floor`], so every pre-6560 caller keeps the grid it
    ///   always got, bit-for-bit. `None` becomes
    ///   [`VoxelResolutionError::DegenerateMesh`].
    /// - [`VoxelResolution::TargetVoxelSize(h)`] — `h` is used verbatim after
    ///   validation.
    /// - [`VoxelResolution::MinFeature(t)`] — `h = t / MIN_FEATURE_VOXELS_ACROSS`.
    ///
    /// # Errors
    ///
    /// - [`VoxelResolutionError::DegenerateMesh`] — the mesh has no usable
    ///   bounding box (see [`bbox_extents`]). Checked for EVERY variant,
    ///   including the ones that do not derive `h` from the bbox, because the
    ///   band width still is.
    /// - [`VoxelResolutionError::InvalidRequest`] — the requested length was
    ///   not finite and strictly positive.
    pub fn for_resolution(
        mesh: &Mesh,
        resolution: VoxelResolution,
    ) -> Result<Self, VoxelResolutionError> {
        let voxel_size = match resolution {
            // Delegated, never re-derived: this keeps the pre-6560 behaviour
            // bit-identical by construction rather than by two implementations
            // happening to agree.
            VoxelResolution::HonestFloor => {
                return Self::honest_floor(mesh).ok_or(VoxelResolutionError::DegenerateMesh);
            }
            VoxelResolution::TargetVoxelSize(h) => {
                validate_requested_length(h, "TargetVoxelSize")?;
                h
            }
            VoxelResolution::MinFeature(t) => {
                validate_requested_length(t, "MinFeature")?;
                t / MIN_FEATURE_VOXELS_ACROSS
            }
        };

        let extents = bbox_extents(mesh).ok_or(VoxelResolutionError::DegenerateMesh)?;

        // PLACEHOLDER (task 6560, step-4): reuse honest_floor's longest-extent
        // band rule so the voxel-size arithmetic can be tested on its own.
        // Step-6 replaces this with the min-half-extent rule that makes
        // thickness-scale resolution affordable on a thin feature in a large
        // part, and adds the dense-grid budget pre-check.
        let _ = extents;
        let narrow_band = VOXELS_PER_LONGEST_AXIS / 2.0 + BAND_MARGIN_VOXELS;

        Ok(Self {
            voxel_size,
            narrow_band,
        })
    }

    /// Produce a [`ContentHash`] of the conversion parameters.
    ///
    /// # Wire-format invariant
    ///
    /// Encoding order is fixed and stable: domain tag →
    /// `voxel_size` (little-endian bytes) →
    /// `narrow_band` (little-endian bytes). Changing this order
    /// invalidates any persisted hash values.
    ///
    /// # ESC-3433-117 non-zero domain tag
    ///
    /// Seeded with `ContentHash::of_str("MeshToVoxelOptions")` so that
    /// `MeshToVoxelOptions::default().content_hash()` cannot equal
    /// `ContentHash(0)` — the `NO_OPTIONS` sentinel.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_str("MeshToVoxelOptions")
            .combine(ContentHash::of(&self.voxel_size.to_le_bytes()))
            .combine(ContentHash::of(&self.narrow_band.to_le_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BAND_MARGIN_VOXELS, DENSIFY_BUDGET_VOXELS, MIN_FEATURE_VOXELS_ACROSS, MeshToVoxelOptions,
        VOXELS_PER_LONGEST_AXIS, VoxelResolutionError,
    };
    use reify_ir::{Mesh, VoxelResolution};
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

    // -----------------------------------------------------------------------
    // honest_floor tests (step-1 RED)
    // -----------------------------------------------------------------------
    //
    // All tests are cfg-unconditional — honest_floor is pure arithmetic with
    // no FFI dependency, so it must compile and run in all build modes.
    //
    // Assertions use geometric inequalities rather than exact float literals
    // to avoid machine-epsilon brittleness.

    /// Helper: build a closed box mesh centred at the origin from ±half extents.
    /// 8 corner vertices, 12 outward-wound triangles.
    fn box_mesh(hx: f32, hy: f32, hz: f32) -> Mesh {
        let v: Vec<f32> = vec![
            -hx, -hy, -hz, // 0
             hx, -hy, -hz, // 1
             hx,  hy, -hz, // 2
            -hx,  hy, -hz, // 3
            -hx, -hy,  hz, // 4
             hx, -hy,  hz, // 5
             hx,  hy,  hz, // 6
            -hx,  hy,  hz, // 7
        ];
        #[rustfmt::skip]
        let i: Vec<u32> = vec![
            // Bottom (-Z)
            0, 2, 1,  0, 3, 2,
            // Top (+Z)
            4, 5, 6,  4, 6, 7,
            // Front (-Y)
            0, 1, 5,  0, 5, 4,
            // Back (+Y)
            2, 3, 7,  2, 7, 6,
            // Left (-X)
            0, 4, 7,  0, 7, 3,
            // Right (+X)
            1, 2, 6,  1, 6, 5,
        ];
        Mesh { vertices: v, indices: i, normals: None }
    }

    /// A closed 2.0-unit cube (vertices at ±1.0):
    /// - honest_floor returns Some
    /// - voxel_size > 0.0 and finite
    /// - voxel_size == 2.0 / VOXELS_PER_LONGEST_AXIS (longest extent = 2.0)
    /// - band covers the interior: narrow_band * voxel_size >= 1.0 (half of 2.0)
    #[test]
    fn honest_floor_cube_2unit() {
        let mesh = box_mesh(1.0, 1.0, 1.0); // extent 2.0 × 2.0 × 2.0
        let opts = MeshToVoxelOptions::honest_floor(&mesh)
            .expect("honest_floor must return Some for a valid closed cube");

        assert!(opts.voxel_size > 0.0, "voxel_size must be positive");
        assert!(opts.voxel_size.is_finite(), "voxel_size must be finite");

        let expected_h = 2.0 / VOXELS_PER_LONGEST_AXIS;
        assert_eq!(
            opts.voxel_size, expected_h,
            "voxel_size must equal longest_extent / VOXELS_PER_LONGEST_AXIS; \
             expected {expected_h}, got {}",
            opts.voxel_size
        );

        // Band must reach the interior: narrow_band × voxel_size >= half-extent (1.0 mm).
        let band_depth = opts.narrow_band * opts.voxel_size;
        assert!(
            band_depth >= 1.0,
            "band depth (narrow_band={} × voxel_size={}) = {} must cover \
             the interior (>= 1.0); band does NOT reach the centre",
            opts.narrow_band, opts.voxel_size, band_depth
        );
    }

    /// A non-cube box 2×4×6 units (longest axis = 6):
    /// - voxel_size == 6.0 / VOXELS_PER_LONGEST_AXIS
    /// - band covers the deepest interior (half of shortest extent = 1.0):
    ///   narrow_band * voxel_size >= 1.0
    #[test]
    fn honest_floor_non_cube_box() {
        let mesh = box_mesh(1.0, 2.0, 3.0); // extent 2 × 4 × 6
        let opts = MeshToVoxelOptions::honest_floor(&mesh)
            .expect("honest_floor must return Some for a valid non-cube box");

        let expected_h = 6.0 / VOXELS_PER_LONGEST_AXIS;
        assert_eq!(
            opts.voxel_size, expected_h,
            "voxel_size must use the longest axis (6.0); \
             expected {expected_h}, got {}",
            opts.voxel_size
        );

        // Shortest axis half-extent = 1.0; band must cover it.
        let band_depth = opts.narrow_band * opts.voxel_size;
        assert!(
            band_depth >= 1.0,
            "band depth {} must cover the shortest half-extent (1.0); \
             narrow_band={}, voxel_size={}",
            band_depth, opts.narrow_band, opts.voxel_size
        );
    }

    /// Empty mesh (no vertices) → None.
    #[test]
    fn honest_floor_empty_mesh_returns_none() {
        let mesh = Mesh { vertices: vec![], indices: vec![], normals: None };
        assert!(
            MeshToVoxelOptions::honest_floor(&mesh).is_none(),
            "honest_floor must return None for an empty mesh"
        );
    }

    /// Degenerate mesh (all vertices coincident → zero bbox extent) → None.
    #[test]
    fn honest_floor_degenerate_mesh_returns_none() {
        // All 8 "vertices" at the origin — extent is 0 on every axis.
        let v: Vec<f32> = vec![0.0_f32; 8 * 3];
        let mesh = Mesh { vertices: v, indices: vec![0, 1, 2], normals: None };
        assert!(
            MeshToVoxelOptions::honest_floor(&mesh).is_none(),
            "honest_floor must return None for a degenerate (zero-extent) mesh"
        );
    }

    /// A mesh containing a NaN vertex coordinate → None.
    ///
    /// honest_floor returns None on the FIRST non-finite coordinate encountered,
    /// rather than silently skipping it and computing a bbox over the remaining
    /// valid vertices (which would produce a plausible-looking bbox for a
    /// conceptually invalid mesh).
    #[test]
    fn honest_floor_nan_coordinate_returns_none() {
        let v: Vec<f32> = vec![
            -1.0, -1.0, -1.0,         // valid vertex
             1.0,  1.0, f32::NAN,      // NaN on z of second vertex
        ];
        let mesh = Mesh { vertices: v, indices: vec![], normals: None };
        assert!(
            MeshToVoxelOptions::honest_floor(&mesh).is_none(),
            "honest_floor must return None for a mesh containing a NaN vertex coordinate"
        );
    }

    /// A mesh containing an Inf vertex coordinate → None.
    ///
    /// Same contract as the NaN test: any non-finite coordinate rejects the mesh.
    #[test]
    fn honest_floor_inf_coordinate_returns_none() {
        let v: Vec<f32> = vec![
            -1.0, -1.0, -1.0,
             1.0,  1.0, f32::INFINITY,  // Inf on z of second vertex
        ];
        let mesh = Mesh { vertices: v, indices: vec![], normals: None };
        assert!(
            MeshToVoxelOptions::honest_floor(&mesh).is_none(),
            "honest_floor must return None for a mesh containing an Inf vertex coordinate"
        );
    }

    /// ESC-3433-117 carry-forward: a default `MeshToVoxelOptions` must NOT hash
    /// to `NO_OPTIONS` (the real sentinel from `reify-eval::realization_cache`).
    /// A collision would let two semantically-distinct cache entries alias into
    /// the same `ToleranceBucket`, returning wrong geometry silently. Sealed by
    /// the domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = MeshToVoxelOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS,
            "MeshToVoxelOptions::default().content_hash() must not equal NO_OPTIONS \
             — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"MeshToVoxelOptions\")` must \
             not produce the same value as reify_eval::NO_OPTIONS",
        );
    }

    /// Two options differing only in `voxel_size` must produce different hashes
    /// — confirms voxel_size is included in the hash input.
    #[test]
    fn voxel_size_sensitivity() {
        let a = MeshToVoxelOptions {
            voxel_size: 0.1,
            narrow_band: 3.0,
        };
        let b = MeshToVoxelOptions {
            voxel_size: 0.2,
            narrow_band: 3.0,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "MeshToVoxelOptions with different voxel_size must produce \
             different content_hash values — voxel_size not hashed",
        );
    }

    /// Two options differing only in `narrow_band` must produce different hashes
    /// — confirms narrow_band is included in the hash input.
    #[test]
    fn narrow_band_sensitivity() {
        let a = MeshToVoxelOptions {
            voxel_size: 0.1,
            narrow_band: 3.0,
        };
        let b = MeshToVoxelOptions {
            voxel_size: 0.1,
            narrow_band: 4.0,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "MeshToVoxelOptions with different narrow_band must produce \
             different content_hash values — narrow_band not hashed",
        );
    }

    /// Identical `MeshToVoxelOptions` must produce equal hashes (determinism).
    /// Confirms the hash is purely a function of the field values — no
    /// timestamp, no RNG, no pointer identity.
    #[test]
    fn identical_options_produce_equal_hashes() {
        let a = MeshToVoxelOptions {
            voxel_size: 0.1,
            narrow_band: 3.0,
        };
        let b = MeshToVoxelOptions {
            voxel_size: 0.1,
            narrow_band: 3.0,
        };
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "identical MeshToVoxelOptions must produce equal content_hash values \
             (hash must be deterministic)",
        );
    }

    // -----------------------------------------------------------------------
    // for_resolution tests (task 6560, step-3 RED)
    // -----------------------------------------------------------------------
    //
    // Pure arithmetic, no FFI: these run in stub builds too.

    /// The shells PRD's own motivating geometry (`structural-analysis-shells.md`,
    /// "Background"): a 1 mm thin feature inside a 100 mm part. Axis-aligned
    /// box, x,y ∈ [0,100], z ∈ [0,1] — the same shape as
    /// `tests/dispatcher_integration.rs:238`. Deliberately NOT origin-centred:
    /// the real-FFI probe in step-7 depends on the z ∈ [0,1] placement.
    fn plate_100x100x1() -> Mesh {
        let v: Vec<f32> = vec![
            0.0, 0.0, 0.0, // 0
            100.0, 0.0, 0.0, // 1
            100.0, 100.0, 0.0, // 2
            0.0, 100.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            100.0, 0.0, 1.0, // 5
            100.0, 100.0, 1.0, // 6
            0.0, 100.0, 1.0, // 7
        ];
        let i: Vec<u32> = vec![
            // Bottom (-Z)
            0, 2, 1, 0, 3, 2, // Top (+Z)
            4, 5, 6, 4, 6, 7, // Front (-Y)
            0, 1, 5, 0, 5, 4, // Back (+Y)
            2, 3, 7, 2, 7, 6, // Left (-X)
            0, 4, 7, 0, 7, 3, // Right (+X)
            1, 2, 6, 1, 6, 5,
        ];
        Mesh {
            vertices: v,
            indices: i,
            normals: None,
        }
    }

    /// A 4 × 4 × 0.3125 thin panel (the `thin_panel` proportions already used
    /// elsewhere in the openvdb suite), centred at the origin.
    fn thin_panel() -> Mesh {
        box_mesh(2.0, 2.0, 0.156_25)
    }

    /// `HonestFloor` must be a verbatim delegation to [`MeshToVoxelOptions::honest_floor`]
    /// — BOTH fields exactly equal, no re-derivation.
    ///
    /// This is the behaviour-preservation pin: every pre-6560 caller keeps the
    /// grid it always got, bit-for-bit.
    #[test]
    fn for_resolution_honest_floor_is_bit_identical_to_honest_floor() {
        let panel = thin_panel();
        let via_request = MeshToVoxelOptions::for_resolution(&panel, VoxelResolution::HonestFloor)
            .expect("HonestFloor on a valid panel must succeed");
        let direct = MeshToVoxelOptions::honest_floor(&panel)
            .expect("honest_floor on a valid panel must return Some");

        assert_eq!(
            via_request.voxel_size, direct.voxel_size,
            "for_resolution(HonestFloor) must delegate voxel_size verbatim"
        );
        assert_eq!(
            via_request.narrow_band, direct.narrow_band,
            "for_resolution(HonestFloor) must delegate narrow_band verbatim"
        );
    }

    /// **Pins the gate failure this task exists to close.**
    ///
    /// Today's only policy is bbox-driven: `longest_extent / 64`. On the shells
    /// PRD's own motivating part (1 mm feature in a 100 mm plate) that is
    /// 1.5625 mm/voxel — COARSER than the whole feature, so the feature is
    /// entirely sub-voxel and no amount of downstream sampling can recover it.
    #[test]
    fn honest_floor_is_sub_feature_on_the_shells_prd_motivating_plate() {
        let plate = plate_100x100x1();
        let opts = MeshToVoxelOptions::honest_floor(&plate)
            .expect("honest_floor must return Some for the plate");

        assert_eq!(
            opts.voxel_size,
            100.0 / VOXELS_PER_LONGEST_AXIS,
            "honest_floor must still derive voxel_size from the LONGEST extent (100.0)"
        );
        assert_eq!(
            opts.voxel_size, 1.5625,
            "100.0 / 64.0 is exactly representable in f64"
        );
        assert!(
            opts.voxel_size > 1.0,
            "the 1 mm feature is entirely sub-voxel under honest_floor \
             (voxel_size {} > thickness 1.0) — this is the v0.4-shells gate failure",
            opts.voxel_size
        );
    }

    /// `MinFeature(t)` resolves the feature: `voxel_size = t / MIN_FEATURE_VOXELS_ACROSS`.
    ///
    /// The second assertion is the reason `MIN_FEATURE_VOXELS_ACROSS` is 4 and
    /// not the PRD's literal "≈ thickness/3": at `h = t/3` the half-thickness
    /// is `1.5h`, BELOW the empirically-established OpenVDB interior-signing
    /// floor of "half-thickness ≥ 2 × voxel_size"
    /// (`reify-eval/tests/harness_kernel_realization/realization_read_api.rs:505-510`).
    #[test]
    fn for_resolution_min_feature_resolves_the_thin_feature() {
        let plate = plate_100x100x1();
        let opts = MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::MinFeature(1.0))
            .expect("MinFeature(1.0) on the plate must succeed");

        assert_eq!(
            opts.voxel_size,
            1.0 / MIN_FEATURE_VOXELS_ACROSS,
            "MinFeature(t) must yield t / MIN_FEATURE_VOXELS_ACROSS"
        );
        assert_eq!(opts.voxel_size, 0.25, "1.0 / 4.0 is exact in f64");
        assert!(
            opts.voxel_size * 2.0 <= 1.0 / 2.0,
            "half-thickness (0.5) must be at least 2 voxels ({}) — the documented \
             OpenVDB interior-signing floor",
            opts.voxel_size * 2.0
        );
    }

    /// `TargetVoxelSize(h)` is used verbatim.
    #[test]
    fn for_resolution_target_voxel_size_is_used_verbatim() {
        let plate = plate_100x100x1();
        let opts =
            MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::TargetVoxelSize(0.25))
                .expect("TargetVoxelSize(0.25) on the plate must succeed");
        assert_eq!(
            opts.voxel_size, 0.25,
            "TargetVoxelSize must be honoured exactly, not re-derived"
        );
    }

    /// Non-finite and non-positive requested sizes are rejected for BOTH
    /// value-carrying variants — a structured `Err`, never a silently-clamped
    /// or NaN-propagating grid.
    #[test]
    fn for_resolution_rejects_invalid_requested_sizes() {
        let plate = plate_100x100x1();
        for bad in [
            0.0_f64,
            -1.0,
            -0.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            for resolution in [
                VoxelResolution::TargetVoxelSize(bad),
                VoxelResolution::MinFeature(bad),
            ] {
                match MeshToVoxelOptions::for_resolution(&plate, resolution) {
                    Err(VoxelResolutionError::InvalidRequest { requested, .. }) => {
                        assert!(
                            requested.to_bits() == bad.to_bits(),
                            "the error must carry the offending value verbatim; \
                             requested={requested}, bad={bad}"
                        );
                    }
                    other => {
                        panic!("expected Err(InvalidRequest) for {resolution:?}; got {other:?}")
                    }
                }
            }
        }
    }

    /// A degenerate mesh is rejected for EVERY variant, matching the conditions
    /// under which `honest_floor` returns `None` (empty, all-coincident, or any
    /// non-finite coordinate).
    #[test]
    fn for_resolution_rejects_degenerate_meshes_for_every_variant() {
        let empty = Mesh {
            vertices: vec![],
            indices: vec![],
            normals: None,
        };
        let coincident = Mesh {
            vertices: vec![0.0_f32; 8 * 3],
            indices: vec![0, 1, 2],
            normals: None,
        };
        let nan = Mesh {
            vertices: vec![-1.0, -1.0, -1.0, 1.0, 1.0, f32::NAN],
            indices: vec![],
            normals: None,
        };

        for mesh in [&empty, &coincident, &nan] {
            for resolution in [
                VoxelResolution::HonestFloor,
                VoxelResolution::TargetVoxelSize(0.25),
                VoxelResolution::MinFeature(1.0),
            ] {
                let got = MeshToVoxelOptions::for_resolution(mesh, resolution);
                assert!(
                    matches!(got, Err(VoxelResolutionError::DegenerateMesh)),
                    "expected Err(DegenerateMesh) for {resolution:?} on a degenerate mesh; \
                     got {got:?}"
                );
            }
        }
    }

    /// `MIN_FEATURE_VOXELS_ACROSS` is 4.0, and is at least as fine as the
    /// shells PRD's "≈ thickness/3" — being finer satisfies the gate's
    /// "resolutions sufficient for".
    #[test]
    fn min_feature_voxels_across_is_at_least_the_prd_thickness_over_three() {
        assert_eq!(MIN_FEATURE_VOXELS_ACROSS, 4.0);
        const {
            assert!(
                MIN_FEATURE_VOXELS_ACROSS >= 3.0,
                "must be at least as fine as the shells PRD's ~ thickness/3"
            )
        };
    }

    // -----------------------------------------------------------------------
    // Narrow-band tightness + densify-budget guard (task 6560, step-5 RED)
    // -----------------------------------------------------------------------
    //
    // Pure arithmetic, no FFI: these run in stub builds too.

    /// The band must still reach every interior point at the new, much finer
    /// resolution.
    ///
    /// The bound is an IDENTITY, not a tuned number: the body is contained in
    /// its bounding box, so any interior point's distance to the BODY boundary
    /// is at most its distance to the BBOX boundary, which is at most the
    /// minimum half-extent (any path leaving the bbox must exit the body
    /// first). For the 100 × 100 × 1 plate that is 0.5.
    #[test]
    fn for_resolution_band_covers_the_interior() {
        let plate = plate_100x100x1();
        let opts = MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::MinFeature(1.0))
            .expect("MinFeature(1.0) on the plate must succeed");

        let band_depth = opts.narrow_band * opts.voxel_size;
        assert!(
            band_depth >= 0.5,
            "band depth (narrow_band={} × voxel_size={}) = {band_depth} must cover the \
             plate's minimum half-extent (0.5); a shallower band saturates the interior \
             SDF to a sentinel instead of the true distance",
            opts.narrow_band,
            opts.voxel_size
        );
    }

    /// The band is derived from the MINIMUM half-extent, not the longest extent.
    ///
    /// This is the assertion that makes thickness-scale resolution affordable
    /// on a thin feature inside a large part: at h = 0.25 on the 100 mm plate,
    /// `honest_floor`'s `longest_extent / 2` rule would demand 200+ band voxels
    /// (100/2 ÷ 0.25 = 200), whereas the containment identity needs only
    /// 0.5 / 0.25 + BAND_MARGIN_VOXELS = 4.
    #[test]
    fn for_resolution_band_is_derived_from_min_half_extent() {
        let plate = plate_100x100x1();
        let opts = MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::MinFeature(1.0))
            .expect("MinFeature(1.0) on the plate must succeed");

        assert_eq!(
            opts.narrow_band,
            0.5 / 0.25 + BAND_MARGIN_VOXELS,
            "narrow_band must be min_half_extent / voxel_size + BAND_MARGIN_VOXELS"
        );
        assert_eq!(opts.narrow_band, 4.0, "0.5/0.25 + 2.0 is exact in f64");
        assert!(
            opts.narrow_band < 100.0 / 2.0 / opts.voxel_size,
            "the min-half-extent rule must be strictly cheaper than honest_floor's \
             longest-extent rule on a thin feature in a large part"
        );
    }

    /// `honest_floor`'s own band policy is untouched by task 6560 — regression
    /// pin on the existing `VOXELS_PER_LONGEST_AXIS / 2 + BAND_MARGIN_VOXELS`
    /// constant, so every pre-existing caller and test is unchanged.
    #[test]
    fn honest_floor_band_policy_is_unchanged() {
        let panel = thin_panel();
        let opts = MeshToVoxelOptions::honest_floor(&panel)
            .expect("honest_floor must return Some for a valid panel");
        assert_eq!(
            opts.narrow_band,
            VOXELS_PER_LONGEST_AXIS / 2.0 + BAND_MARGIN_VOXELS,
            "honest_floor must keep its longest-extent band rule"
        );
        assert_eq!(opts.narrow_band, 34.0, "64.0/2.0 + 2.0");
    }

    /// The dense-grid budget is checked BEFORE any FFI work.
    ///
    /// `OpenVdbGridSource` (ingest.rs:64-79) and `SampledField`
    /// (reify-ir/src/value.rs:94-114) are both dense row-major `Vec<f64>`, and
    /// the only ceiling before this guard was the C++ `GRID_DENSIFY_MAX_VOXELS`
    /// throw inside `grid_densify_to_buffer` — i.e. AFTER a full `meshToVolume`
    /// allocation, surfaced as a mis-typed `IngestError::FileReadError`. This
    /// pre-check is pure Rust, so it also fires in stub builds.
    ///
    /// 100 / 0.001 = 1e5 per lateral axis ⇒ nx·ny ≈ 1e10 ≫ 256M.
    #[test]
    fn for_resolution_rejects_an_over_budget_request_before_any_ffi() {
        let plate = plate_100x100x1();
        match MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::TargetVoxelSize(0.001)) {
            Err(VoxelResolutionError::DensifyBudgetExceeded {
                requested_voxel_size,
                implied_voxels,
                budget,
            }) => {
                assert_eq!(
                    requested_voxel_size, 0.001,
                    "the error must name the requested voxel size verbatim"
                );
                assert_eq!(
                    budget, DENSIFY_BUDGET_VOXELS,
                    "the error must name the budget it was measured against"
                );
                assert!(
                    implied_voxels > budget,
                    "implied_voxels ({implied_voxels}) must exceed the budget ({budget})"
                );
            }
            other => panic!("expected Err(DensifyBudgetExceeded); got {other:?}"),
        }
    }

    /// The guard must not false-positive on the very request this task exists
    /// to enable: `MinFeature(1.0)` on the 100 mm plate implies a densify bbox
    /// of roughly 408 × 408 × 12 ≈ 2.0M voxels — three orders of magnitude
    /// under the 256M budget.
    #[test]
    fn for_resolution_budget_guard_does_not_false_positive() {
        let plate = plate_100x100x1();
        assert!(
            MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::MinFeature(1.0)).is_ok(),
            "the thickness-scale request the shells gate needs must stay within budget"
        );
    }

    /// The Rust-side budget mirrors the C++ `GRID_DENSIFY_MAX_VOXELS`
    /// (cpp/openvdb_wrapper.h:146): 256M voxels ≈ 1 GiB at 4 bytes/float.
    #[test]
    fn densify_budget_matches_the_cpp_ceiling() {
        assert_eq!(DENSIFY_BUDGET_VOXELS, 256 * 1024 * 1024);
    }

    /// A request whose per-axis counts would overflow `i64` under naive
    /// multiplication must return the budget `Err` — never panic, never wrap
    /// to a small positive product that sneaks past the cap.
    ///
    /// At h = 1e-12 on the plate the lateral axes are ~1e14 voxels each, so
    /// nx·ny ≈ 1e28 — far outside `i64`. The check compares against
    /// `budget / next` BEFORE multiplying, exactly as openvdb_wrapper.cpp:377-393
    /// does, so the wrap can never happen.
    #[test]
    fn for_resolution_budget_guard_is_overflow_safe() {
        let plate = plate_100x100x1();
        match MeshToVoxelOptions::for_resolution(&plate, VoxelResolution::TargetVoxelSize(1e-12)) {
            Err(VoxelResolutionError::DensifyBudgetExceeded {
                implied_voxels,
                budget,
                ..
            }) => {
                assert!(
                    implied_voxels > budget,
                    "an overflowing request must still report an over-budget count, \
                     not a wrapped small positive one; got {implied_voxels}"
                );
            }
            other => panic!("expected Err(DensifyBudgetExceeded) for 1e-12; got {other:?}"),
        }
    }
}
