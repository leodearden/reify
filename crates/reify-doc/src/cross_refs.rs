//! Cross-reference index types for the `reify doc` renderer.
//!
//! This module defines the [`CrossRefs`] struct — a pair of *cross-template
//! inverted indices* that answer two questions for the `reify doc` renderer:
//!
//! 1. **trait → conformers** (`CrossRefs::trait_to_conformers`): which topology
//!    templates declare conformance to a given trait?  Used to render the
//!    "Implementations" list on a trait's documentation page.
//!
//! 2. **entity → containers** (`CrossRefs::entity_to_containers`): which topology
//!    templates include a given structure as a sub-component?  Used to render
//!    the "Used by" list on a structure's or occurrence's documentation page.
//!
//! # Population helper
//!
//! The function that populates this struct from compiled templates lives in the
//! separate `reify-doc-build` crate (`reify_doc_build::cross_refs::build_cross_refs`)
//! to preserve this crate's serde-only embeddability — downstream consumers that
//! only need the data model do not need to pull in the full compiler stack.
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
//!   a separate artefact consumed by the formatter / renderer layer.
//!
//! Both types are disambiguated by module path:
//! `reify_doc::cross_refs::CrossRefs` vs `reify_doc::model::CrossRefs`.
//! Neither is re-exported at the crate root to prevent name ambiguity at use sites.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Cross-template inverted indices for the `reify doc` renderer.
///
/// Produced by [`build_cross_refs`] from a slice of compiled topology templates.
///
/// # Relationship to `model::CrossRefs`
///
/// This struct is **semantically distinct** from [`crate::model::CrossRefs`]:
///
/// - [`crate::model::CrossRefs`] holds *per-module outgoing references*
///   (which other modules, items, or traits a given module refers to).
///
/// - This [`CrossRefs`] holds *cross-template inverted indices* — it answers
///   "which templates conform to trait X?" and "which templates use structure Y?".
///
/// Both are disambiguated by module path; neither is re-exported at the crate root.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossRefs {
    /// Maps each trait name to the sorted, deduplicated list of template names
    /// that declare conformance to it.
    ///
    /// Built from `template.trait_bounds` across all templates.  Rendered as the
    /// "Implementations" list on a trait's documentation page.
    pub trait_to_conformers: BTreeMap<String, Vec<String>>,

    /// Maps each structure name to the sorted, deduplicated list of template names
    /// that include it as a sub-component.
    ///
    /// Built from `template.sub_components[*].structure_name` across all templates.
    /// Rendered as the "Used by" list on a structure's or occurrence's doc page.
    pub entity_to_containers: BTreeMap<String, Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
