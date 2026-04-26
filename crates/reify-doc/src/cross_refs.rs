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
pub fn build_cross_refs(templates: &[reify_compiler::TopologyTemplate]) -> CrossRefs {
    let mut result = CrossRefs::default();

    for template in templates {
        // trait → conformers index
        for trait_name in &template.trait_bounds {
            result
                .trait_to_conformers
                .entry(trait_name.clone())
                .or_default()
                .push(template.name.clone());
        }

        // entity → containers index
        for sub in &template.sub_components {
            result
                .entity_to_containers
                .entry(sub.structure_name.clone())
                .or_default()
                .push(template.name.clone());
        }
    }

    // Post-process: sort + dedup inner vecs for deterministic output.
    for conformers in result.trait_to_conformers.values_mut() {
        conformers.sort();
        conformers.dedup();
    }
    for containers in result.entity_to_containers.values_mut() {
        containers.sort();
        containers.dedup();
    }

    result
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
    fn build_cross_refs_nested_sub_components_with_dedup() {
        let robot = TopologyTemplateBuilder::new("Robot")
            .sub_component("arm", "Arm", vec![])
            .sub_component("wheel1", "Wheel", vec![])
            .sub_component("wheel2", "Wheel", vec![])
            .build();
        let arm = TopologyTemplateBuilder::new("Arm")
            .sub_component("joint", "Joint", vec![])
            .build();

        let result = build_cross_refs(&[robot, arm]);

        // (a) BTreeMap key order: alphabetical
        let keys: Vec<&str> = result.entity_to_containers.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["Arm", "Joint", "Wheel"]);

        // (b) Arm is contained by Robot
        assert_eq!(
            result.entity_to_containers["Arm"],
            vec!["Robot".to_string()]
        );

        // (c) Joint is contained by Arm (nested, non-root template)
        assert_eq!(
            result.entity_to_containers["Joint"],
            vec!["Arm".to_string()]
        );

        // (d) Wheel appears once despite two instances (wheel1, wheel2)
        assert_eq!(
            result.entity_to_containers["Wheel"],
            vec!["Robot".to_string()]
        );

        // (e) no trait bounds → trait_to_conformers is empty
        assert!(result.trait_to_conformers.is_empty());

        // (f) serde round-trip
        let json = serde_json::to_string(&result).expect("serialize");
        let roundtripped: CrossRefs = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, roundtripped);
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
