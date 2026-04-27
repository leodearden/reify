//! Population helper for cross-reference indices.
//!
//! The [`build_cross_refs`] function lives here (not in `reify-doc`) so that
//! the `reify-doc` type crate stays compiler-free and embeddable in downstream
//! consumers that do not depend on the full compiler stack.
//!
//! The [`reify_doc::cross_refs::CrossRefs`] type is defined in `reify-doc`; this
//! module only provides the function that constructs it from compiled templates.

use reify_compiler::TopologyTemplate;
use reify_doc::cross_refs::CrossRefs;

/// Build cross-reference indices from a slice of compiled topology templates.
///
/// Both index maps use [`std::collections::BTreeMap`] for deterministic key order.
/// Inner `Vec<String>` values are sorted alphabetically and deduplicated after
/// population to guarantee a stable, canonical output regardless of input order.
pub fn build_cross_refs(templates: &[TopologyTemplate]) -> CrossRefs {
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
    use reify_doc::cross_refs::CrossRefs;
    use reify_test_support::TopologyTemplateBuilder;

    use super::build_cross_refs;

    #[test]
    fn build_cross_refs_empty_input_returns_default() {
        let result = build_cross_refs(&[]);
        assert!(result.trait_to_conformers.is_empty());
        assert!(result.entity_to_containers.is_empty());
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
    fn build_cross_refs_dedups_repeated_input_templates() {
        // Two identical Bolt templates (same name, same trait) and two identical
        // Robot templates (same name, same sub-component).  Each template name
        // must appear exactly once in the output despite appearing twice in input.
        let bolt_a = TopologyTemplateBuilder::new("Bolt").trait_bound("Rigid").build();
        let bolt_b = TopologyTemplateBuilder::new("Bolt").trait_bound("Rigid").build();
        let robot_a = TopologyTemplateBuilder::new("Robot").sub_component("arm", "Arm", vec![]).build();
        let robot_b = TopologyTemplateBuilder::new("Robot").sub_component("arm", "Arm", vec![]).build();

        let result = build_cross_refs(&[bolt_a, bolt_b, robot_a, robot_b]);

        // (a) "Bolt" must appear exactly once under "Rigid" (not twice)
        assert_eq!(
            result.trait_to_conformers["Rigid"],
            vec!["Bolt".to_string()],
            "expected a single deduplicated entry for Bolt under Rigid",
        );

        // (b) "Robot" must appear exactly once under "Arm" (not twice)
        assert_eq!(
            result.entity_to_containers["Arm"],
            vec!["Robot".to_string()],
            "expected a single deduplicated entry for Robot under Arm",
        );
    }

    #[test]
    fn build_cross_refs_input_order_independent() {
        // Combined fixture: Bolt + Spring (traits) + Robot + Arm (sub-components)
        let bolt = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .trait_bound("Fastener")
            .build();
        let spring = TopologyTemplateBuilder::new("Spring")
            .trait_bound("Rigid")
            .build();
        let robot = TopologyTemplateBuilder::new("Robot")
            .sub_component("arm", "Arm", vec![])
            .sub_component("wheel1", "Wheel", vec![])
            .sub_component("wheel2", "Wheel", vec![])
            .build();
        let arm = TopologyTemplateBuilder::new("Arm")
            .sub_component("joint", "Joint", vec![])
            .build();

        let bolt2 = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .trait_bound("Fastener")
            .build();
        let spring2 = TopologyTemplateBuilder::new("Spring")
            .trait_bound("Rigid")
            .build();
        let robot2 = TopologyTemplateBuilder::new("Robot")
            .sub_component("arm", "Arm", vec![])
            .sub_component("wheel1", "Wheel", vec![])
            .sub_component("wheel2", "Wheel", vec![])
            .build();
        let arm2 = TopologyTemplateBuilder::new("Arm")
            .sub_component("joint", "Joint", vec![])
            .build();

        let result_a = build_cross_refs(&[bolt, spring, robot, arm]);
        let result_b = build_cross_refs(&[arm2, robot2, spring2, bolt2]);

        // Full struct equality regardless of input order
        assert_eq!(result_a, result_b);

        // Forward-compat: deserializing empty JSON object must equal CrossRefs::default()
        let from_empty: CrossRefs = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(from_empty, CrossRefs::default());
    }
}
