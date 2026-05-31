//! `VolumeMeshOptions` — Gmsh volume-mesh solver parameters and their
//! `ContentHash` producer.
//!
//! PRD §4 producer-registry table: producer crate = `reify-kernel-gmsh`.
//! The struct carries the two behavioral discriminators that select the tet-vs-swept
//! meshing path and partition the `RealizationCache` options-hash dimension.
//!
//! # ESC-3433-117 carry-forward — non-zero domain tag invariant
//!
//! `content_hash()` seeds with `ContentHash::of_str("VolumeMeshOptions")` so
//! that `VolumeMeshOptions::default().content_hash()` cannot equal the
//! `NO_OPTIONS` sentinel (`ContentHash(0)` at
//! `crates/reify-eval/src/realization_cache.rs:85`).  A collision would let a
//! VolumeMeshOptions-keyed VolumeMesh entry alias a NO_OPTIONS-keyed entry in
//! the same `ToleranceBucket`, silently returning wrong cached geometry.
//! Pinned by the unit test `default_content_hash_is_not_no_options_sentinel`.

use reify_core::ContentHash;

/// Gmsh volume-mesh solver options for the surface→volume meshing stage.
///
/// Fields are the two behavioral discriminators that select the tet-vs-swept
/// meshing path (M-024 regression: `force_tet={true,false}` must yield distinct
/// cache slots):
/// - `force_tet`: forces all-tetrahedral meshing regardless of classifier output.
/// - `require_hex_wedge`: requires hex/wedge elements; mutually exclusive with
///   `force_tet` (the DSL enforces `constraint !(force_tet && require_hex_wedge)`
///   upstream; the struct hashes whatever it is given).
///
/// Both fields default to `false` — the classifier-driven path; the escape
/// hatches are opt-in.
///
/// `gmsh_2d` and `sweep_step` from the PRD §4 table are `FnOnce` closures
/// (execution mechanics, not hashable identity) and are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeMeshOptions {
    /// Force all-tetrahedral meshing, bypassing the classifier.
    pub force_tet: bool,

    /// Require hex/wedge elements; mutually exclusive with `force_tet`.
    pub require_hex_wedge: bool,
}

impl VolumeMeshOptions {
    /// Produce a [`ContentHash`] of the volume-mesh solver options.
    ///
    /// # Wire-format invariant
    ///
    /// Encoding order is fixed and stable: domain tag →
    /// `force_tet` byte → `require_hex_wedge` byte.
    /// Changing this order invalidates any persisted hash values.
    ///
    /// # ESC-3433-117 non-zero domain tag
    ///
    /// Seeded with `ContentHash::of_str("VolumeMeshOptions")` so that
    /// `VolumeMeshOptions::default().content_hash()` cannot equal
    /// `ContentHash(0)` — the `NO_OPTIONS` sentinel at
    /// `crates/reify-eval/src/realization_cache.rs:85`. A collision would
    /// let a VolumeMeshOptions-keyed VolumeMesh entry alias a NO_OPTIONS-keyed
    /// entry in the same `ToleranceBucket`, silently returning wrong
    /// cached geometry. Pinned by `default_content_hash_is_not_no_options_sentinel`.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_str("VolumeMeshOptions")
            .combine(ContentHash::of(&[self.force_tet as u8]))
            .combine(ContentHash::of(&[self.require_hex_wedge as u8]))
    }
}

#[cfg(test)]
mod tests {
    use super::VolumeMeshOptions;
    // Import the authoritative sentinel — not a hand-copied literal — so this
    // test fails loudly if reify_eval::NO_OPTIONS ever drifts (ESC-3433-117).
    use reify_eval::NO_OPTIONS;

    /// ESC-3433-117 carry-forward: a default `VolumeMeshOptions` must NOT hash to
    /// `NO_OPTIONS` (the real sentinel from `reify-eval::realization_cache`). A
    /// collision would let two semantically-distinct cache entries alias into the
    /// same `ToleranceBucket`, returning wrong geometry silently. Sealed by the
    /// domain-tag seed in `content_hash()`.
    #[test]
    fn default_content_hash_is_not_no_options_sentinel() {
        let hash = VolumeMeshOptions::default().content_hash();
        assert_ne!(
            hash,
            NO_OPTIONS,
            "VolumeMeshOptions::default().content_hash() must not equal NO_OPTIONS \
             — ESC-3433-117 non-zero domain tag invariant violated; \
             the domain-tag seed `ContentHash::of_str(\"VolumeMeshOptions\")` must \
             not produce the same value as reify_eval::NO_OPTIONS",
        );
    }

    /// Two options differing only in `force_tet` must produce different hashes
    /// (M-024 discriminator — two volume-mesh solves differing only in force_tet
    /// MUST get distinct cache slots).
    #[test]
    fn force_tet_sensitivity() {
        let a = VolumeMeshOptions { force_tet: true, require_hex_wedge: false };
        let b = VolumeMeshOptions { force_tet: false, require_hex_wedge: false };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "VolumeMeshOptions with different force_tet must produce different \
             content_hash values — force_tet not hashed",
        );
    }

    /// Two options differing only in `require_hex_wedge` must produce different
    /// hashes — confirms require_hex_wedge is included in the hash input.
    #[test]
    fn require_hex_wedge_sensitivity() {
        let a = VolumeMeshOptions { force_tet: false, require_hex_wedge: true };
        let b = VolumeMeshOptions { force_tet: false, require_hex_wedge: false };
        assert_ne!(
            a.content_hash(),
            b.content_hash(),
            "VolumeMeshOptions with different require_hex_wedge must produce different \
             content_hash values — require_hex_wedge not hashed",
        );
    }

    /// Identical `VolumeMeshOptions` must produce equal hashes (determinism).
    /// Confirms the hash is purely a function of the field values — no
    /// timestamp, no RNG, no pointer identity.
    #[test]
    fn identical_options_produce_equal_hashes() {
        let a = VolumeMeshOptions { force_tet: true, require_hex_wedge: false };
        let b = VolumeMeshOptions { force_tet: true, require_hex_wedge: false };
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "identical VolumeMeshOptions must produce equal content_hash values \
             (hash must be deterministic)",
        );
    }
}
