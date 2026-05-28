//! `IsoMeshOptions` — Fidget SDF→Mesh iso-surface meshing parameters and
//! their `ContentHash` producer.
//!
//! PRD §4 producer-registry table: producer crate = `reify-kernel-fidget`.
//! The struct captures the two user-visible knobs for fidget-mesh iso-surface
//! extraction (PRD §8 task κ).
//!
//! # ESC-3433-117 carry-forward — non-zero domain tag invariant
//!
//! `content_hash()` seeds with `ContentHash::of_str("IsoMeshOptions")` so
//! that `IsoMeshOptions::default().content_hash()` cannot equal the
//! `NO_OPTIONS` sentinel (`ContentHash(0)` at
//! `crates/reify-eval/src/realization_cache.rs:85`). A collision would let an
//! IsoMeshOptions-keyed Mesh entry alias a NO_OPTIONS-keyed entry in the same
//! `ToleranceBucket`, silently returning wrong cached geometry.
//! Pinned by the unit test `default_content_hash_is_not_no_options_sentinel`.

use reify_core::ContentHash;

/// Fidget SDF→Mesh iso-surface meshing options.
///
/// Fields capture the two user-visible knobs for iso-surface extraction:
/// - `iso_value`: the signed-distance level-set to mesh (0.0 = exact surface;
///   non-zero values shrink/expand the surface by that distance).
/// - `target_edge_length`: the approximate maximum mesh edge length in the
///   meshing region. Controls mesh resolution — smaller values produce finer
///   meshes at higher cost.
///
/// # No `Eq` / `Hash` derives
///
/// `f64` does not implement `Eq` or `Hash` (NaN ≠ NaN). Use
/// [`IsoMeshOptions::content_hash()`] for equality / caching comparisons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsoMeshOptions {
    /// The signed-distance iso-value to mesh.
    ///
    /// `0.0` meshes the exact zero-crossings of the SDF (the solid surface).
    /// Positive values expand the surface outward; negative values shrink it
    /// inward.
    ///
    /// Default: `0.0` (exact surface).
    pub iso_value: f64,

    /// Approximate maximum mesh edge length in the meshing region.
    ///
    /// Controls the octree depth: depth ≈ ceil(log2(2·H /
    /// target_edge_length)) where H is the fixed half-extent constant.
    /// Smaller values produce finer, more accurate meshes at higher cost.
    ///
    /// Default: `0.1`.
    pub target_edge_length: f64,
}

impl Default for IsoMeshOptions {
    /// Returns the default meshing options.
    ///
    /// - `iso_value = 0.0` — mesh the exact zero-crossings (the solid surface).
    /// - `target_edge_length = 0.1` — a medium-resolution starting point.
    fn default() -> Self {
        Self {
            iso_value: 0.0,
            target_edge_length: 0.1,
        }
    }
}

impl IsoMeshOptions {
    /// Produce a [`ContentHash`] of the meshing parameters.
    ///
    /// # Wire-format invariant
    ///
    /// Encoding order is fixed and stable: domain tag →
    /// `iso_value` (little-endian bytes) →
    /// `target_edge_length` (little-endian bytes). Changing this order
    /// invalidates any persisted hash values. The little-endian byte
    /// encoding follows the convention established in
    /// `crates/reify-types/src/hash.rs:27-29` (`ContentHash::of_u64`).
    ///
    /// This hash is **bit-exact on `f64`**: `0.0` and `-0.0` are
    /// `PartialEq`-equal but produce different hashes, and two NaN values
    /// with the same bit pattern produce equal hashes despite being
    /// `PartialEq`-unequal. In practice, meshing parameters are always
    /// finite values, so callers **must not** pass `-0.0` or NaN.
    ///
    /// # ESC-3433-117 non-zero domain tag
    ///
    /// Seeded with `ContentHash::of_str("IsoMeshOptions")` so that
    /// `IsoMeshOptions::default().content_hash()` cannot equal
    /// `ContentHash(0)` — the `NO_OPTIONS` sentinel at
    /// `crates/reify-eval/src/realization_cache.rs:85`. A collision would
    /// let an IsoMeshOptions-keyed Mesh entry alias a NO_OPTIONS-keyed entry
    /// in the same `ToleranceBucket`, silently returning wrong cached geometry.
    /// Pinned by `default_content_hash_is_not_no_options_sentinel`.
    pub fn content_hash(&self) -> ContentHash {
        // STUB: returns ContentHash(0) — deliberately the NO_OPTIONS sentinel
        // so the `default_content_hash_is_not_no_options_sentinel` test fails.
        // Replaced in step-2 with the real domain-tagged hash.
        ContentHash(0)
    }
}

#[cfg(test)]
mod tests {
    use super::IsoMeshOptions;
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

    /// ESC-3433-117 carry-forward: a default `IsoMeshOptions` must NOT hash to
    /// `NO_OPTIONS` (the real sentinel from `reify-eval::realization_cache`). A
    /// collision would let two semantically-distinct cache entries alias into the
    /// same `ToleranceBucket`, returning wrong geometry silently. Sealed by the
    /// domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = IsoMeshOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS,
            "IsoMeshOptions::default().content_hash() must not equal NO_OPTIONS \
             — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"IsoMeshOptions\")` must \
             not produce the same value as reify_eval::NO_OPTIONS",
        );
    }

    /// Two options differing only in `iso_value` must produce different
    /// hashes — confirms iso_value is included in the hash input.
    #[test]
    fn iso_value_sensitivity() {
        let a = IsoMeshOptions {
            iso_value: 0.0,
            target_edge_length: 0.1,
        };
        let b = IsoMeshOptions {
            iso_value: 0.5,
            target_edge_length: 0.1,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "IsoMeshOptions with different iso_value must produce different \
             content_hash values — iso_value not hashed",
        );
    }

    /// Two options differing only in `target_edge_length` must produce different
    /// hashes — confirms target_edge_length is included in the hash input.
    #[test]
    fn target_edge_length_sensitivity() {
        let a = IsoMeshOptions {
            iso_value: 0.0,
            target_edge_length: 0.1,
        };
        let b = IsoMeshOptions {
            iso_value: 0.0,
            target_edge_length: 0.05,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "IsoMeshOptions with different target_edge_length must produce different \
             content_hash values — target_edge_length not hashed",
        );
    }

    /// Identical `IsoMeshOptions` must produce equal hashes (determinism).
    /// Confirms the hash is purely a function of the field values — no
    /// timestamp, no RNG, no pointer identity.
    #[test]
    fn identical_options_produce_equal_hashes() {
        let a = IsoMeshOptions {
            iso_value: 0.0,
            target_edge_length: 0.1,
        };
        let b = IsoMeshOptions {
            iso_value: 0.0,
            target_edge_length: 0.1,
        };
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "identical IsoMeshOptions must produce equal content_hash values \
             (hash must be deterministic)",
        );
    }
}
