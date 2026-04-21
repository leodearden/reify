use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `collect_all_requirements` collects a single `Param` requirement
    /// from a trait with no refinements.
    ///
    /// This test proves the module is reachable by the build and exercises the
    /// simplest happy path: one trait, one requirement, no diamond, no depth.
    #[test]
    fn collect_all_requirements_collects_param_from_single_trait() {
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "width".to_string(),
                kind: RequirementKind::Param(Type::Real),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("TraitA".to_string(), &trait_a);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "TraitA",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        assert_eq!(ctx.requirements.len(), 1);
        assert_eq!(ctx.requirements[0].name, "width");
        assert!(diags.is_empty(), "Expected no diagnostics, got: {:?}", diags);
    }

    /// Verify that `collect_all_requirements` deduplicates requirements in a diamond
    /// refinement pattern via the `visited` set.
    ///
    /// Diamond:
    ///   Base (has param "b")
    ///    / \
    /// Mid1 Mid2
    ///    \ /
    ///    Top
    ///
    /// Without dedup, "b" would be collected twice (once via Mid1→Base, once via
    /// Mid2→Base). The `visited.insert` check before the depth guard ensures that
    /// Base is processed exactly once, so "b" appears exactly once in `ctx.requirements`.
    #[test]
    fn collect_all_requirements_dedups_diamond_refinement() {
        let base = CompiledTrait {
            name: "Base".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "b".to_string(),
                kind: RequirementKind::Param(Type::Real),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let mid1 = CompiledTrait {
            name: "Mid1".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec!["Base".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let mid2 = CompiledTrait {
            name: "Mid2".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec!["Base".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let top = CompiledTrait {
            name: "Top".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec!["Mid1".to_string(), "Mid2".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("Base".to_string(), &base);
        trait_registry.insert("Mid1".to_string(), &mid1);
        trait_registry.insert("Mid2".to_string(), &mid2);
        trait_registry.insert("Top".to_string(), &top);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "Top",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        let b_count = ctx.requirements.iter().filter(|r| r.name == "b").count();
        assert_eq!(
            b_count, 1,
            "Expected exactly one 'b' requirement (dedup via visited), got {}",
            b_count
        );
        assert!(diags.is_empty(), "Expected no diagnostics, got: {:?}", diags);
    }
}
