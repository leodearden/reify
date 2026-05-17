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

#[cfg(test)]
mod tests {
    use super::TessellateOptions;
    use reify_types::ContentHash;

    /// `NO_OPTIONS` sentinel from `crates/reify-eval/src/realization_cache.rs:85`.
    const NO_OPTIONS_SENTINEL: ContentHash = ContentHash(0);

    /// ESC-3433-117 carry-forward: a default `TessellateOptions` must NOT hash to
    /// `ContentHash(0)` = `NO_OPTIONS`. A collision would let two semantically-
    /// distinct cache entries alias into the same `ToleranceBucket`, returning
    /// wrong geometry silently. Sealed by the domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = TessellateOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS_SENTINEL,
            "TessellateOptions::default().content_hash() must not equal NO_OPTIONS \
             (ContentHash(0)) — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"TessellateOptions\")` must \
             not itself hash to 0",
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
