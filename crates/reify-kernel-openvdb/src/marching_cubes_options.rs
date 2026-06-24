//! `MarchingCubesOptions` — OpenVDB Voxel→Mesh (marching cubes) parameters
//! and their `ContentHash` producer.
//!
//! PRD §4 producer-registry table: producer crate = `reify-kernel-openvdb`.
//! The struct mirrors the two dominant parameters passed to
//! `openvdb::tools::volumeToMesh` (PRD §8 task ι).
//!
//! # ESC-3433-117 carry-forward — non-zero domain tag invariant
//!
//! `content_hash()` seeds with `ContentHash::of_str("MarchingCubesOptions")` so
//! that `MarchingCubesOptions::default().content_hash()` cannot equal the
//! `NO_OPTIONS` sentinel (`ContentHash(0)` at
//! `crates/reify-eval/src/realization_cache.rs:85`).  A collision would let a
//! MarchingCubesOptions-keyed Mesh entry alias a NO_OPTIONS-keyed entry in the
//! same `ToleranceBucket`, silently returning wrong cached geometry.
//! Pinned by the unit test `default_content_hash_is_not_no_options_sentinel`.

use reify_core::ContentHash;

/// OpenVDB Voxel→Mesh (marching cubes) conversion options.
///
/// Fields map directly to `openvdb::tools::volumeToMesh` parameters:
/// - `iso_level`: the isovalue at which to extract the surface. For a signed
///   distance field (SDF), `0.0` extracts the zero level-set (the surface).
/// - `adaptive`: when `false`, use uniform marching cubes (all quads the same
///   size); when `true`, use adaptive marching cubes (larger quads in flat
///   regions, reducing triangle count). Mapped to `adaptivity: f64`
///   (`false → 0.0` uniform, `true → 1.0` maximum adaptivity) at the FFI
///   layer.
///
/// # No `Eq` / `Hash` derives
///
/// `f64` does not implement `Eq` or `Hash` (NaN ≠ NaN). Use
/// [`MarchingCubesOptions::content_hash()`] for equality / caching comparisons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarchingCubesOptions {
    /// Isovalue at which to extract the mesh surface. For an SDF, `0.0`
    /// extracts the zero level-set (the actual surface).
    pub iso_level: f64,

    /// When `false` (default), use uniform marching cubes — all output
    /// quads/triangles are the same size. When `true`, use adaptive marching
    /// cubes — larger polygons in planar regions, reducing triangle count.
    /// Mapped to OpenVDB's `adaptivity` float parameter at the FFI layer
    /// (`false → 0.0`, `true → 1.0`).
    pub adaptive: bool,
}

impl Default for MarchingCubesOptions {
    /// Returns the simplest-correct defaults per PRD §9 Q3:
    /// - `iso_level = 0.0` — extract the zero level-set of the SDF (the surface).
    /// - `adaptive = false` — uniform marching cubes (equal-sized output quads).
    fn default() -> Self {
        Self {
            iso_level: 0.0,
            adaptive: false,
        }
    }
}

impl MarchingCubesOptions {
    /// Produce a [`ContentHash`] of the marching-cubes parameters.
    ///
    /// # Wire-format invariant
    ///
    /// Encoding order is fixed and stable: domain tag →
    /// `iso_level` (little-endian bytes) →
    /// `adaptive` (1 byte: `false → 0`, `true → 1`).
    /// Changing this order invalidates any persisted hash values.
    ///
    /// # ESC-3433-117 non-zero domain tag
    ///
    /// Seeded with `ContentHash::of_str("MarchingCubesOptions")` so that
    /// `MarchingCubesOptions::default().content_hash()` cannot equal
    /// `ContentHash(0)` — the `NO_OPTIONS` sentinel.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_str("MarchingCubesOptions")
            .combine(ContentHash::of(&self.iso_level.to_le_bytes()))
            .combine(ContentHash::of(&[self.adaptive as u8]))
    }
}

#[cfg(test)]
mod tests {
    use super::MarchingCubesOptions;
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

    /// ESC-3433-117 carry-forward: a default `MarchingCubesOptions` must NOT hash
    /// to `NO_OPTIONS` (the real sentinel from `reify-eval::realization_cache`).
    /// A collision would let two semantically-distinct cache entries alias into
    /// the same `ToleranceBucket`, returning wrong geometry silently. Sealed by
    /// the domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = MarchingCubesOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS,
            "MarchingCubesOptions::default().content_hash() must not equal NO_OPTIONS \
             — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"MarchingCubesOptions\")` must \
             not produce the same value as reify_eval::NO_OPTIONS",
        );
    }

    /// Two options differing only in `iso_level` must produce different hashes
    /// — confirms iso_level is included in the hash input.
    #[test]
    fn iso_level_sensitivity() {
        let a = MarchingCubesOptions {
            iso_level: 0.0,
            adaptive: false,
        };
        let b = MarchingCubesOptions {
            iso_level: 0.5,
            adaptive: false,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "MarchingCubesOptions with different iso_level must produce \
             different content_hash values — iso_level not hashed",
        );
    }

    /// Two options differing only in `adaptive` must produce different hashes
    /// — confirms adaptive is included in the hash input.
    #[test]
    fn adaptive_sensitivity() {
        let a = MarchingCubesOptions {
            iso_level: 0.0,
            adaptive: false,
        };
        let b = MarchingCubesOptions {
            iso_level: 0.0,
            adaptive: true,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "MarchingCubesOptions with different adaptive must produce \
             different content_hash values — adaptive not hashed",
        );
    }

    /// Identical `MarchingCubesOptions` must produce equal hashes (determinism).
    /// Confirms the hash is purely a function of the field values — no
    /// timestamp, no RNG, no pointer identity.
    #[test]
    fn identical_options_produce_equal_hashes() {
        let a = MarchingCubesOptions {
            iso_level: 0.0,
            adaptive: false,
        };
        let b = MarchingCubesOptions {
            iso_level: 0.0,
            adaptive: false,
        };
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "identical MarchingCubesOptions must produce equal content_hash values \
             (hash must be deterministic)",
        );
    }
}
