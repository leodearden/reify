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
    use super::MeshToVoxelOptions;
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

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
