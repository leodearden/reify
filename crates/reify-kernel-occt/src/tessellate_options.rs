//! `TessellateOptions` — OCCT BRep→Mesh tessellation parameters and their
//! `ContentHash` producer.
//!
//! PRD §4 producer-registry table: producer crate = `reify-kernel-occt`.
//! The struct mirrors the two dominant precision parameters of OCCT's
//! `BRepMesh_IncrementalMesh` (PRD §9 Q-2).
//!
//! # ESC-3433-117 carry-forward — non-zero domain tag invariant
//!
//! `content_hash()` seeds with `ContentHash::of_str("TessellateOptions")` so
//! that `TessellateOptions::default().content_hash()` cannot equal the
//! `NO_OPTIONS` sentinel (`ContentHash(0)` at
//! `crates/reify-eval/src/realization_cache.rs:85`).  A collision would let a
//! TessellateOptions-keyed Mesh entry alias a NO_OPTIONS-keyed BRep entry in
//! the same `ToleranceBucket`, silently returning wrong cached geometry.
//! Pinned by the unit test `default_content_hash_is_not_no_options_sentinel`.

use reify_types::ContentHash;

/// OCCT tessellation options for the BRep→Mesh conversion stage.
///
/// Fields mirror the two dominant precision parameters of OCCT's
/// `BRepMesh_IncrementalMesh` (PRD §9 Q-2):
/// - `angular_deflection`: maximum angular difference (radians) between the
///   normals of adjacent mesh triangles. Controls curvature fidelity of
///   rounded surfaces.
/// - `linear_deflection`: maximum linear distance (metres) between a mesh
///   edge and the underlying BRep edge. Controls coarseness of straight
///   edges and flat faces.
///
/// # No `Eq` / `Hash` derives
///
/// `f64` does not implement `Eq` or `Hash` (NaN ≠ NaN). Use
/// [`TessellateOptions::content_hash()`] for equality / caching comparisons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessellateOptions {
    /// Maximum angular deflection in radians.
    ///
    /// OCCT `BRepMesh_IncrementalMesh` default: `0.5` rad (≈28.6°).
    pub angular_deflection: f64,

    /// Maximum linear deflection in metres.
    ///
    /// OCCT `BRepMesh_IncrementalMesh` default: `0.1` m.
    pub linear_deflection: f64,
}

impl Default for TessellateOptions {
    /// Returns OCCT `BRepMesh_IncrementalMesh` defaults.
    ///
    /// Per PRD §9 Q-2: `angular_deflection = 0.5` rad, `linear_deflection = 0.1` m.
    fn default() -> Self {
        Self {
            angular_deflection: 0.5,
            linear_deflection: 0.1,
        }
    }
}

impl TessellateOptions {
    /// Produce a [`ContentHash`] of the tessellation parameters.
    ///
    /// # Wire-format invariant
    ///
    /// Encoding order is fixed and stable: domain tag →
    /// `angular_deflection` (little-endian bytes) →
    /// `linear_deflection` (little-endian bytes). Changing this order
    /// invalidates any persisted hash values. The little-endian byte
    /// encoding follows the convention established in
    /// `crates/reify-types/src/hash.rs:27-29` (`ContentHash::of_u64`).
    ///
    /// This hash is **bit-exact on `f64`**: `0.0` and `-0.0` are
    /// `PartialEq`-equal but produce different hashes, and two NaN values
    /// with the same bit pattern produce equal hashes despite being
    /// `PartialEq`-unequal. In practice, OCCT deflection parameters are
    /// always positive finite values, so callers **must not** pass `-0.0`
    /// or NaN deflections.
    ///
    /// # ESC-3433-117 non-zero domain tag
    ///
    /// Seeded with `ContentHash::of_str("TessellateOptions")` so that
    /// `TessellateOptions::default().content_hash()` cannot equal
    /// `ContentHash(0)` — the `NO_OPTIONS` sentinel at
    /// `crates/reify-eval/src/realization_cache.rs:85`. A collision would
    /// let a TessellateOptions-keyed Mesh entry alias a NO_OPTIONS-keyed
    /// BRep entry in the same `ToleranceBucket`, silently returning wrong
    /// cached geometry. Pinned by `default_content_hash_is_not_no_options_sentinel`.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_str("TessellateOptions")
            .combine(ContentHash::of(&self.angular_deflection.to_le_bytes()))
            .combine(ContentHash::of(&self.linear_deflection.to_le_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::TessellateOptions;
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

    /// ESC-3433-117 carry-forward: a default `TessellateOptions` must NOT hash to
    /// `NO_OPTIONS` (the real sentinel from `reify-eval::realization_cache`). A
    /// collision would let two semantically-distinct cache entries alias into the
    /// same `ToleranceBucket`, returning wrong geometry silently. Sealed by the
    /// domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = TessellateOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS,
            "TessellateOptions::default().content_hash() must not equal NO_OPTIONS \
             — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"TessellateOptions\")` must \
             not produce the same value as reify_eval::NO_OPTIONS",
        );
    }

    /// Two options differing only in `angular_deflection` must produce different
    /// hashes — confirms angular_deflection is included in the hash input.
    #[test]
    fn angular_deflection_sensitivity() {
        let a = TessellateOptions {
            angular_deflection: 0.5,
            linear_deflection: 0.1,
        };
        let b = TessellateOptions {
            angular_deflection: 0.25,
            linear_deflection: 0.1,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "TessellateOptions with different angular_deflection must produce \
             different content_hash values — angular_deflection not hashed",
        );
    }

    /// Two options differing only in `linear_deflection` must produce different
    /// hashes — confirms linear_deflection is included in the hash input.
    #[test]
    fn linear_deflection_sensitivity() {
        let a = TessellateOptions {
            angular_deflection: 0.5,
            linear_deflection: 0.1,
        };
        let b = TessellateOptions {
            angular_deflection: 0.5,
            linear_deflection: 0.05,
        };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "TessellateOptions with different linear_deflection must produce \
             different content_hash values — linear_deflection not hashed",
        );
    }

    /// Identical `TessellateOptions` must produce equal hashes (determinism).
    /// Confirms the hash is purely a function of the field values — no
    /// timestamp, no RNG, no pointer identity.
    #[test]
    fn identical_options_produce_equal_hashes() {
        let a = TessellateOptions {
            angular_deflection: 0.5,
            linear_deflection: 0.1,
        };
        let b = TessellateOptions {
            angular_deflection: 0.5,
            linear_deflection: 0.1,
        };
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "identical TessellateOptions must produce equal content_hash values \
             (hash must be deterministic)",
        );
    }
}
