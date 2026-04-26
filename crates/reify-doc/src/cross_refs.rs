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

/// Build cross-reference indices from a slice of compiled topology templates.
///
/// Both index maps use [`BTreeMap`] for deterministic key order.  Inner
/// `Vec<String>` values are sorted alphabetically and deduplicated after
/// population to guarantee a stable, canonical output regardless of input order.
pub fn build_cross_refs(_templates: &[reify_compiler::TopologyTemplate]) -> CrossRefs {
    CrossRefs::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use reify_test_support::TopologyTemplateBuilder;

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

    #[test]
    fn build_cross_refs_multi_trait_conformance() {
        let bolt = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .trait_bound("Fastener")
            .build();
        let spring = TopologyTemplateBuilder::new("Spring")
            .trait_bound("Rigid")
            .build();

        let result = build_cross_refs(&[bolt, spring]);

        // (a) BTreeMap key order: alphabetical
        let keys: Vec<&str> = result.trait_to_conformers.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["Fastener", "Rigid"]);

        // (b) Fastener has only Bolt
        assert_eq!(
            result.trait_to_conformers["Fastener"],
            vec!["Bolt".to_string()]
        );

        // (c) Rigid has Bolt and Spring, sorted alphabetically
        assert_eq!(
            result.trait_to_conformers["Rigid"],
            vec!["Bolt".to_string(), "Spring".to_string()]
        );

        // (d) no sub-components → entity_to_containers is empty
        assert!(result.entity_to_containers.is_empty());

        // (e) serde round-trip
        let json = serde_json::to_string(&result).expect("serialize");
        let roundtripped: CrossRefs = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, roundtripped);
    }
}
