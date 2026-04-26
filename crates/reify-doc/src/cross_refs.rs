//! Cross-reference indices over a slice of compiled topology templates.
//!
//! This module produces *cross-template inverted indices* that answer two
//! questions needed by the `reify doc` renderer:
//!
//! 1. **trait → conformers** (`CrossRefs::trait_to_conformers`): which topology
//!    templates declare conformance to a given trait?  Used to render the
//!    "Implementations" list on a trait's documentation page.
//!
//! 2. **entity → containers** (`CrossRefs::entity_to_containers`): which topology
//!    templates include a given structure as a sub-component?  Used to render
//!    the "Used by" list on a structure's or occurrence's documentation page.
//!
//! # Relationship to `model::CrossRefs`
//!
//! The name `CrossRefs` is deliberately shared with [`crate::model::CrossRefs`],
//! but the two types are **semantically distinct**:
//!
//! - [`crate::model::CrossRefs`] holds *per-module outgoing references* (which
//!   other modules, items, or traits a given module refers to).  It is attached
//!   to each [`crate::model::ModuleDoc`] and populated by the lowering slice.
//!
//! - [`CrossRefs`] (this module) holds *cross-template inverted indices* computed
//!   over the entire compiled template set.  It is not part of `DocModel`; it is
//!   a separate artefact produced by [`build_cross_refs`] and consumed by the
//!   formatter / renderer layer.
//!
//! Both types are disambiguated by module path:
//! `reify_doc::cross_refs::CrossRefs` vs `reify_doc::model::CrossRefs`.
//! Neither is re-exported at the crate root to prevent name ambiguity at use sites.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_cross_refs_empty_input_returns_default() {
        let result = build_cross_refs(&[]);
        assert!(result.trait_to_conformers.is_empty());
        assert!(result.entity_to_containers.is_empty());
    }

    #[test]
    fn cross_refs_default_has_empty_maps() {
        let r = CrossRefs::default();
        assert!(r.trait_to_conformers.is_empty());
        assert!(r.entity_to_containers.is_empty());
    }

    #[test]
    fn cross_refs_default_serde_round_trip() {
        let original = CrossRefs::default();
        let json = serde_json::to_string(&original).expect("serialize");
        let roundtripped: CrossRefs = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, roundtripped);
    }
}
