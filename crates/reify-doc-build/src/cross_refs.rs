//! Population helper for cross-reference indices.
//!
//! The [`build_cross_refs`] function lives here (not in `reify-doc`) so that
//! the `reify-doc` type crate stays compiler-free and embeddable in downstream
//! consumers that do not depend on the full compiler stack.
//!
//! The [`reify_doc::cross_refs::CrossRefs`] type is defined in `reify-doc`; this
//! module only provides the function that constructs it from compiled templates.

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
