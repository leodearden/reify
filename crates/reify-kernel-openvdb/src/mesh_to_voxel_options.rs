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

impl MeshToVoxelOptions {
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
    use super::{MeshToVoxelOptions, VOXELS_PER_LONGEST_AXIS};
    use reify_ir::Mesh;
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
}
