use super::*;

/// Maximum allowed depth for trait refinement chains to prevent stack overflow
/// from pathologically deep but acyclic hierarchies. Realistic refinement chains
/// rarely exceed ~10 levels; 128 provides ample headroom.
pub(crate) const MAX_TRAIT_DEPTH: usize = 128;

/// Kind tag for the `seen_defaults` composite key `(name, DefaultKindTag)`.
///
/// Keeping `Param` and `Constraint` in separate slots means a Param default
/// and a Constraint default for the same member name do not interfere, and
/// cross-kind type comparisons never produce false conflict diagnostics.
///
/// `Let` defaults are **not** tracked here — they use the separate
/// `seen_let_hashes` path (content-hash dedup) and always `continue` before
/// this composite key is ever reached.
///
/// `AvailableDefaultKind` (used for requirement matching) intentionally has no
/// `Constraint` variant — constraints are never candidates for satisfying requirements.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DefaultKindTag {
    Param,
    Constraint,
}

/// Mutable tracking state threaded through `collect_all_requirements`.
///
/// Bundles the output accumulators and the 5 dedup/conflict-tracking maps so
/// the recursive function signature stays within Clippy's argument-count limit.
/// `MergeContext::new()` initialises all fields to empty; callers create one
/// instance per structure and read `requirements` / `defaults` after the loop.
///
/// `requirements` and `defaults` are `pub` because `check_trait_conformance`
/// consumes them after the collection loop. The 5 tracking maps are private —
/// only `collect_all_requirements` should mutate them.
#[derive(Default)]
pub(crate) struct MergeContext {
    /// Accumulated requirements collected across all visited traits.
    pub requirements: Vec<TraitRequirement>,
    /// Accumulated defaults collected across all visited traits.
    pub defaults: Vec<TraitDefault>,
    /// Trait names already visited — prevents double-processing diamond patterns.
    visited: HashSet<String>,
    /// Maps member name → (type, originating trait) for requirement conflict reporting.
    seen_names: HashMap<String, (Type, String)>,
    /// Composite-key dedup for Param/Constraint defaults: (name, DefaultKindTag) → (type, trait).
    seen_defaults: HashMap<(String, DefaultKindTag), (Type, String)>,
    /// Content-hash dedup for Let defaults: name → (hash, originating trait).
    seen_let_hashes: HashMap<String, (ContentHash, String)>,
    /// Let binding names that already have a conflict diagnostic (emit at most 1 per name).
    seen_let_conflict_names: HashSet<String>,
}

impl MergeContext {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Recursively collect all requirements and defaults from a trait and its refinements.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_all_requirements(
    trait_name: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    ctx: &mut MergeContext,
    structure_members: &HashMap<String, Type>,
    span: SourceSpan,
    depth: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // IMPORTANT: `visited` MUST be checked (and inserted) BEFORE the depth guard.
    // In a diamond refinement pattern, a trait reachable via two paths at depth >
    // MAX_TRAIT_DEPTH would emit duplicate "too deep" diagnostics if the depth guard
    // fired first (because the visited insert never happened on the first path).
    // Visited-first ensures the second path short-circuits silently. (Task 403 fix.)
    if !ctx.visited.insert(trait_name.to_string()) {
        return; // Already visited (diamond pattern)
    }

    if depth > MAX_TRAIT_DEPTH {
        diagnostics.push(
            Diagnostic::error(format!(
                "trait refinement chain too deep (exceeded {} levels) at '{}'",
                MAX_TRAIT_DEPTH, trait_name
            ))
            .with_label(DiagnosticLabel::new(span, "trait chain too deep")),
        );
        return;
    }

    let Some(compiled_trait) = trait_registry.get(trait_name) else {
        diagnostics.push(
            Diagnostic::error(format!("unresolved trait: '{}'", trait_name))
                .with_label(DiagnosticLabel::new(span, "unknown trait")),
        );
        return;
    };

    // Walk refinement chain first (parents before children)
    for refinement in &compiled_trait.refinements {
        collect_all_requirements(
            refinement,
            trait_registry,
            ctx,
            structure_members,
            span,
            depth + 1,
            diagnostics,
        );
    }

    // Collect requirements from this trait, checking for conflicts.
    for req in &compiled_trait.required_members {
        let expected_type = match &req.kind {
            RequirementKind::Param(ty) | RequirementKind::Let(ty) => Some(ty.clone()),
            _ => None,
        };

        if let Some(expected_type) = &expected_type {
            if let Some((existing_type, existing_trait)) = ctx.seen_names.get(&req.name) {
                if existing_type != expected_type {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "conflicting trait requirements for '{}': \
                             trait '{}' requires {}, trait '{}' requires {}",
                            req.name, existing_trait, existing_type, trait_name, expected_type
                        ))
                        .with_label(DiagnosticLabel::new(
                            span,
                            format!("conflict between '{}' and '{}'", existing_trait, trait_name),
                        )),
                    );
                }
                continue; // Deduplicated
            }
            ctx.seen_names.insert(
                req.name.clone(),
                (expected_type.clone(), trait_name.to_string()),
            );
        }

        ctx.requirements.push(req.clone());
    }

    // Collect defaults from this trait, deduplicating by name.
    for default in &compiled_trait.defaults {
        if default.name.is_none() {
            // Unnamed defaults (e.g., unlabeled constraints) — always push.
            // Dedup is implicit: the `visited` set (checked above before recursing into
            // each trait) prevents re-processing the same trait, so each unnamed default
            // is encountered at most once regardless of how many paths lead to that trait.
            ctx.defaults.push(default.clone());
        } else if let Some(name) = &default.name {
            // For let bindings: use content_hash comparison to distinguish same
            // expression (dedup) vs different expression (conflict).
            if let DefaultKind::Let { let_decl, .. } = &default.kind {
                if let Some((existing_hash, existing_trait)) =
                    ctx.seen_let_hashes.get(name.as_str())
                {
                    if existing_hash != &let_decl.content_hash
                        && !structure_members.contains_key(name.as_str())
                        && !ctx.seen_let_conflict_names.contains(name.as_str())
                    {
                        // Same name, different expression, not overridden, first conflict → emit.
                        ctx.seen_let_conflict_names.insert(name.clone());
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "conflicting trait let bindings for '{}': \
                                 trait '{}' and trait '{}' provide different expressions",
                                name, existing_trait, trait_name
                            ))
                            .with_label(DiagnosticLabel::new(
                                span,
                                format!(
                                    "conflict between '{}' and '{}'",
                                    existing_trait, trait_name
                                ),
                            )),
                        );
                    }
                    // Same name already seen (same or different hash) → skip.
                    continue;
                }
                // Only record the hash when the structure does NOT override this name.
                // When overridden, conflict diagnostics are suppressed and the default
                // is filtered at injection time (~line 327), so hash-recording is
                // unnecessary.  As a side-effect, when multiple traits provide
                // `let x = <different>` and the structure overrides `x`, each trait's
                // default is pushed into `ctx.defaults` without dedup — producing
                // redundant entries.  This is harmless: the injection loop re-checks
                // `structure_members.contains_key(name)` and discards all of them.
                if !structure_members.contains_key(name.as_str()) {
                    ctx.seen_let_hashes.insert(
                        name.clone(),
                        (let_decl.content_hash, trait_name.to_string()),
                    );
                }
                // Let dedup/conflict is fully handled by seen_let_hashes.
                // Push the default and skip the seen_defaults composite-key path —
                // the Type::Real sentinel there is redundant and confusing.
                ctx.defaults.push(default.clone());
                continue;
            }

            // Extract type and kind-tag for composite-key dedup.
            // Param and Constraint each get their own (name, kind) slot so they
            // never interfere with each other's dedup or conflict detection.
            // Note: Let defaults always `continue` above and never reach this match.
            let (default_type, kind_tag) = match &default.kind {
                DefaultKind::Param { cell_type, .. } => (cell_type.clone(), DefaultKindTag::Param),
                DefaultKind::Let { .. } => {
                    // Unreachable: all Let defaults are handled by the early
                    // `if let DefaultKind::Let { let_decl, .. }` block above, which always
                    // exits via `continue`.
                    unreachable!("Let defaults must be handled by the seen_let_hashes block above")
                }
                DefaultKind::Constraint(_) => (Type::Bool, DefaultKindTag::Constraint),
            };

            // Note: `name.to_string()` allocates even on the `continue` (already-seen) path
            // because `HashMap<(String, DefaultKindTag), _>` has no `Borrow` impl for
            // `(&str, DefaultKindTag)`. Default counts per trait are tiny, so this is not a
            // hot path. To eliminate the allocation a two-level map
            // `HashMap<String, HashMap<DefaultKindTag, _>>` would allow a borrow-based outer
            // lookup, but the added complexity is not worth it at current scale.
            let key = (name.to_string(), kind_tag);
            if let Some((existing_type, existing_trait)) = ctx.seen_defaults.get(&key) {
                if existing_type != &default_type && !structure_members.contains_key(name.as_str())
                {
                    // Same (name, kind) + different type + not overridden → conflict
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "conflicting trait defaults for '{}': \
                             trait '{}' has {}, trait '{}' has {}",
                            name, existing_trait, existing_type, trait_name, default_type
                        ))
                        .with_label(DiagnosticLabel::new(
                            span,
                            format!("conflict between '{}' and '{}'", existing_trait, trait_name),
                        )),
                    );
                }
                // Same (name, kind) already seen → skip (deduplicate).
                continue;
            }
            ctx.seen_defaults
                .insert(key, (default_type, trait_name.to_string()));
            ctx.defaults.push(default.clone());
        }
    }
}

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
