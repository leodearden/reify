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
