use std::ops::ControlFlow;

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
/// consumes them after the collection loop. Most tracking maps are private —
/// only `collect_all_requirements` should mutate them. A few (`seen_fn_sigs`,
/// `seen_fn_default_traits`, `seen_assoc_type_reqs`, `seen_assoc_type_default_traits`)
/// are `pub` so downstream phases can read the first-seen trait attribution.
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
    /// Sub requirement names already collected — maps name → (structure_name, originating trait).
    ///
    /// When the same sub-component name appears in two traits with the same `structure_name`,
    /// the second occurrence is silently dropped (dedup, identical to `seen_names` for Param/Let).
    /// When the same name appears with a *different* `structure_name` (e.g. `sub hole = Hole`
    /// vs `sub hole = Rectangle`), a "conflicting trait sub requirements" diagnostic is emitted
    /// — analogous to the Param/Let conflict block — and the second requirement is still dropped
    /// so the checker sees exactly one entry for that sub-name.
    seen_sub_names: HashMap<String, (String, String)>,
    /// Assoc-fn signatures collected across the refinement chain, keyed by
    /// fn name → (signature, originating trait). Populated by the
    /// `RequirementKind::Fn` arm. `pub` because phase 5 reads it to name the
    /// declaring trait in the `TraitFnNotSatisfied` diagnostic (like
    /// `requirements`/`defaults`, it is a downstream-consumed output, not a
    /// purely-internal tracking map). Step-10's refinement signature-lock
    /// upgrades the first-seen insert below into a `try_dedup_or_conflict`
    /// call against this same map. (task 3939 δ)
    pub seen_fn_sigs: HashMap<String, (CompiledAssocFnSig, String)>,
    /// For default-providing assoc fns (`DefaultKind::Fn`): fn name →
    /// originating trait, recorded when the default is merged so the
    /// assoc-fn-resolution phase can key the compiled table by
    /// `(trait_name, fn_name)`. `TraitDefault` itself carries no originating
    /// trait, so this is the only place the trait is captured for defaults.
    /// First-seen wins (mirrors `seen_fn_sigs` for requirements). (task 3939 δ)
    pub seen_fn_default_traits: HashMap<String, String>,
    /// Assoc-type requirement names already collected: type name → first-seen trait.
    /// First-seen wins (dedup); identical re-declaration via diamond/refinement is
    /// silently dropped. `pub` because phase 5 reads it to name the declaring trait
    /// in the `TraitAssocTypeNotBound` diagnostic. (task 3972)
    pub seen_assoc_type_reqs: HashMap<String, String>,
    /// For default-providing assoc types (`DefaultKind::AssocType`): type name →
    /// originating trait, recorded when the default is merged so the
    /// assoc-type-resolution phase can key the compiled table by
    /// `(trait_name, type_name)`. First-seen wins. (task 3972)
    pub seen_assoc_type_default_traits: HashMap<String, String>,
    /// Conflict-detection map for assoc-type defaults: type name → (resolved Type,
    /// originating trait). Only populated when the structure does NOT override the
    /// name (mirroring the let-hash suppression pattern); when overridden, conflict
    /// diagnostics are suppressed and this map is not updated. (task 3972)
    seen_assoc_type_defaults: HashMap<String, (Type, String)>,
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
            .with_code(DiagnosticCode::TraitRefinementChainTooDeep)
            .with_label(DiagnosticLabel::new(span, "trait chain too deep")),
        );
        return;
    }

    let Some(compiled_trait) = trait_registry.get(trait_name) else {
        diagnostics.push(
            Diagnostic::error(format!("unresolved trait: '{}'", trait_name))
                .with_code(DiagnosticCode::UnresolvedTrait)
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
        match &req.kind {
            RequirementKind::Param(expected_type) | RequirementKind::Let(expected_type) => {
                if try_dedup_or_conflict(
                    &mut ctx.seen_names,
                    &req.name,
                    expected_type,
                    trait_name,
                    span,
                    |name, existing, existing_trait, new, new_trait| {
                        (
                            format!(
                                "conflicting trait requirements for '{}': \
                                 trait '{}' requires {}, trait '{}' requires {}",
                                name, existing_trait, existing, new_trait, new
                            ),
                            DiagnosticCode::ConflictingTraitRequirements,
                        )
                    },
                    diagnostics,
                )
                .is_break()
                {
                    continue;
                }
                ctx.requirements.push(req.clone());
            }
            // Assoc-type requirements: first-seen-by-name dedup (mirroring the Fn arm
            // via `seen_assoc_type_reqs`). Conflict detection (step-6) is not yet wired;
            // a second trait declaring `type X` with a DIFFERENT bound would simply be
            // dropped here (only the first-seen entry is pushed). (task 3972)
            RequirementKind::AssocType(_) => {
                if ctx.seen_assoc_type_reqs.contains_key(&req.name) {
                    continue; // already collected this assoc-type requirement (first-seen wins)
                }
                ctx.seen_assoc_type_reqs
                    .insert(req.name.clone(), trait_name.to_string());
                ctx.requirements.push(req.clone());
            }
            RequirementKind::Sub(structure_name) => {
                // Dedup Sub requirements by name, following the `seen_names` pattern for
                // Param/Let: if the same sub-component name was already collected:
                //   - Same structure_name → identical requirement, silently skip.
                //   - Different structure_name → conflicting requirements, emit a diagnostic
                //     (e.g. `sub hole = Hole` vs `sub hole = Rectangle`) then skip so the
                //     checker sees at most one entry per sub-name.
                if try_dedup_or_conflict(
                    &mut ctx.seen_sub_names,
                    &req.name,
                    structure_name,
                    trait_name,
                    span,
                    |name, existing, existing_trait, new, new_trait| {
                        (
                            format!(
                                "conflicting trait sub requirements for '{}': \
                                 trait '{}' requires sub '{}', \
                                 trait '{}' requires sub '{}'",
                                name, existing_trait, existing, new_trait, new
                            ),
                            DiagnosticCode::ConflictingTraitSubRequirements,
                        )
                    },
                    diagnostics,
                )
                .is_break()
                {
                    continue;
                }
                ctx.requirements.push(req.clone());
            }
            // Assoc-fn requirement: the refinement signature-lock (task 3939 δ).
            // A refining trait may re-declare an inherited assoc fn, but only with
            // the IDENTICAL signature — same name + different inherited signature
            // is a conflict (PRD §5.4 / §8.8 exact-match, no subtyping).
            // `CompiledAssocFnSig: PartialEq + Clone` plugs straight into the
            // generic `try_dedup_or_conflict` helper against `seen_fn_sigs`:
            //   * equal sig    → Break (dedup, requirement not re-pushed),
            //   * different sig → emit `TraitFnSignatureMismatch`, drop the copy.
            // Phase 5 still reads `seen_fn_sigs` to name the declaring trait in
            // its `TraitFnNotSatisfied` diagnostic, so first-seen (the base trait)
            // remains recorded there.
            RequirementKind::Fn(sig) => {
                if try_dedup_or_conflict(
                    &mut ctx.seen_fn_sigs,
                    &req.name,
                    sig,
                    trait_name,
                    span,
                    |name, _existing, existing_trait, _new, new_trait| {
                        (
                            format!(
                                "refining trait may not change the inherited associated-function \
                                 signature for '{}': trait '{}' and trait '{}' declare \
                                 different signatures",
                                name, existing_trait, new_trait
                            ),
                            DiagnosticCode::TraitFnSignatureMismatch,
                        )
                    },
                    diagnostics,
                )
                .is_break()
                {
                    continue;
                }
                ctx.requirements.push(req.clone());
            }
        }
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
                            .with_code(DiagnosticCode::ConflictingTraitLetBindings)
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
                // the Type::dimensionless_scalar() sentinel there is redundant and confusing.
                //
                // ds-sentinel L1 (#4646): audited — this file is NOT an offending
                // producer. It does requirement collection/dedup/conflict only and
                // performs no type-NAME resolution, so it has no "unresolved-name /
                // invalid-type-expr fallback after an error diagnostic" site to convert
                // to Type::Error. The dimensionless_scalar() refs here (this note) and in
                // the #[cfg(test)] fixtures below are not error-recovery poison sites.
                ctx.defaults.push(default.clone());
                continue;
            }

            // Assoc-fn default (task 3939 δ): record the originating trait so the
            // assoc-fn-resolution phase can key the table by (trait, fn), then push
            // so the default reaches conformance (phase 5's default-satisfies-
            // requirement check and the table-population phase both read it from
            // `ctx.defaults`). First-seen-by-name dedup keeps a single entry across
            // a diamond/refinement chain. The value-typed composite-key path below
            // does not apply to Fn defaults (a fn body has no single "default type"),
            // so it is handled here and `continue`s. Step-10 layers the refinement
            // signature-lock on top (a refining trait may override a same-name body
            // but may not change an inherited assoc-fn signature).
            if let DefaultKind::Fn(_) = &default.kind {
                if ctx
                    .seen_fn_default_traits
                    .contains_key(name.as_str())
                {
                    continue; // already collected this assoc-fn default (first-seen wins)
                }
                ctx.seen_fn_default_traits
                    .insert(name.clone(), trait_name.to_string());
                ctx.defaults.push(default.clone());
                continue;
            }

            // Assoc-type default (task 3972): record the originating trait so the
            // assoc-type-resolution phase can key the table by (trait, type_name), then
            // push so the default reaches conformance. Conflict detection via
            // `try_dedup_or_conflict` on the resolved Type: same type → dedup silently;
            // different type → emit `ConflictingTraitAssocType`. Suppression: when the
            // structure binds the name, conflict checking is skipped (mirroring the
            // seen_let_hashes suppression pattern — only the first-seen entry is pushed
            // via the seen_assoc_type_default_traits guard).
            if let DefaultKind::AssocType(ty) = &default.kind {
                if structure_members.contains_key(name.as_str()) {
                    // Structure overrides — suppress conflict, first-seen wins.
                    if ctx.seen_assoc_type_default_traits.contains_key(name.as_str()) {
                        continue;
                    }
                    ctx.seen_assoc_type_default_traits
                        .insert(name.clone(), trait_name.to_string());
                    ctx.defaults.push(default.clone());
                } else {
                    // No override — use try_dedup_or_conflict for conflict detection.
                    if try_dedup_or_conflict(
                        &mut ctx.seen_assoc_type_defaults,
                        name,
                        ty,
                        trait_name,
                        span,
                        |n, _, existing_trait, _, new_trait| {
                            (
                                format!(
                                    "conflicting trait associated type defaults for '{}': \
                                     trait '{}' and trait '{}' provide different types",
                                    n, existing_trait, new_trait
                                ),
                                DiagnosticCode::ConflictingTraitAssocType,
                            )
                        },
                        diagnostics,
                    )
                    .is_break()
                    {
                        continue; // already seen (dedup or conflict — second copy dropped)
                    }
                    // First-seen: record originating trait and push.
                    ctx.seen_assoc_type_default_traits
                        .insert(name.clone(), trait_name.to_string());
                    ctx.defaults.push(default.clone());
                }
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
                // Unreachable: all Fn defaults are handled by the early
                // `if let DefaultKind::Fn(_)` block above, which always exits via
                // `continue` (task 3939 δ).
                DefaultKind::Fn(_) => {
                    unreachable!("Fn defaults must be handled by the seen_fn_default_traits block above")
                }
                // Unreachable: all AssocType defaults are handled by the early
                // `if let DefaultKind::AssocType(_)` block (step-4), which always
                // exits via `continue`.
                DefaultKind::AssocType(_) => {
                    unreachable!("AssocType defaults must be handled by the seen_assoc_type_default_traits block")
                }
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
                        .with_code(DiagnosticCode::ConflictingTraitDefaults)
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

/// Deduplicates or reports a conflict for a single requirement/sub-requirement entry.
///
/// Looks up `name` in `seen`.
/// - **Cache miss**: inserts `(value.clone(), trait_name.to_string())` and returns `Continue(())`.
///   The caller should push the requirement.
/// - **Cache hit, equal value**: returns `Break(())` silently (deduplicated).
/// - **Cache hit, mismatched value**: invokes `conflict_builder` to obtain the
///   conflict message **and** the `DiagnosticCode` for this call site, emits a
///   `Diagnostic::error` with a uniform label `"conflict between '…' and '…'"`,
///   and returns `Break(())`. The caller should skip (do not push).
///
/// The closure returns `(message, code)` so the conflict-only data lives together
/// in one place — and is only constructed on the conflict branch. The non-conflict
/// branches (cache miss, cache hit with equal value) never invoke the closure, so
/// no placeholder code is needed at those call sites or in unit tests.
///
/// The caller pattern is:
/// ```rust,ignore
/// if try_dedup_or_conflict(&mut seen, name, value, trait_name, span, builder, diags).is_break() {
///     continue;
/// }
/// ctx.requirements.push(req.clone());
/// ```
fn try_dedup_or_conflict<V, F>(
    seen: &mut HashMap<String, (V, String)>,
    name: &str,
    value: &V,
    trait_name: &str,
    span: SourceSpan,
    conflict_builder: F,
    diagnostics: &mut Vec<Diagnostic>,
) -> ControlFlow<()>
where
    V: PartialEq + Clone,
    F: FnOnce(&str, &V, &str, &V, &str) -> (String, DiagnosticCode),
{
    if let Some((existing_value, existing_trait)) = seen.get(name) {
        if existing_value != value {
            let (msg, code) =
                conflict_builder(name, existing_value, existing_trait, value, trait_name);
            let label = format!("conflict between '{}' and '{}'", existing_trait, trait_name);
            diagnostics.push(
                Diagnostic::error(msg)
                    .with_code(code)
                    .with_label(DiagnosticLabel::new(span, label)),
            );
        }
        return ControlFlow::Break(());
    }
    seen.insert(name.to_string(), (value.clone(), trait_name.to_string()));
    ControlFlow::Continue(())
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
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "width".to_string(),
                kind: RequirementKind::Param(Type::dimensionless_scalar()),
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
        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
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
        let base = make_compiled_trait(
            "Base",
            vec![],
            vec![TraitRequirement {
                name: "b".to_string(),
                kind: RequirementKind::Param(Type::dimensionless_scalar()),
                span: SourceSpan::empty(0),
            }],
        );
        let mid1 = make_compiled_trait("Mid1", vec!["Base".to_string()], vec![]);
        let mid2 = make_compiled_trait("Mid2", vec!["Base".to_string()], vec![]);
        let top = make_compiled_trait("Top", vec!["Mid1".to_string(), "Mid2".to_string()], vec![]);

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
        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
    }

    /// Build a bodyless-assoc-fn requirement `fn <name>(self) -> <return_type>`
    /// for the refinement signature-lock tests. (task 3939 δ)
    fn assoc_fn_req(name: &str, return_type: Type) -> TraitRequirement {
        TraitRequirement {
            name: name.to_string(),
            kind: RequirementKind::Fn(CompiledAssocFnSig {
                name: name.to_string(),
                has_self: true,
                params: vec![],
                return_type,
            }),
            span: SourceSpan::empty(0),
        }
    }

    /// RED (task 3939 δ, step-9): a refining trait that CHANGES an inherited
    /// assoc-fn signature must produce exactly one `TraitFnSignatureMismatch`.
    /// Base declares `fn f(self) -> Real`; Derived (: Base) declares
    /// `fn f(self) -> Length` — same name, different inherited signature.
    ///
    /// Fails until step-10 replaces the first-seen `or_insert` in the
    /// `RequirementKind::Fn` merge arm with `try_dedup_or_conflict` (today the
    /// arm records first-seen and pushes BOTH copies, emitting zero diagnostics).
    #[test]
    fn refining_trait_changing_inherited_assoc_fn_signature_conflicts() {
        let base = make_compiled_trait("Base", vec![], vec![assoc_fn_req("f", Type::dimensionless_scalar())]);
        let derived = make_compiled_trait(
            "Derived",
            vec!["Base".to_string()],
            vec![assoc_fn_req("f", Type::length())],
        );

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("Base".to_string(), &base);
        trait_registry.insert("Derived".to_string(), &derived);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "Derived",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        let mismatch: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "a refining trait may not change an inherited assoc-fn signature — \
             expected exactly one TraitFnSignatureMismatch; got: {:?}",
            diags
        );
        assert!(
            mismatch[0].message.contains("f"),
            "the conflict diagnostic should name the fn 'f'; got: {}",
            mismatch[0].message
        );
        // The conflicting (second) requirement is dropped → a single 'f' entry.
        let f_count = ctx.requirements.iter().filter(|r| r.name == "f").count();
        assert_eq!(
            f_count, 1,
            "the conflicting requirement should be dropped, leaving one 'f'; got {}",
            f_count
        );
    }

    /// RED (task 3939 δ, step-9): a refining trait that re-declares an inherited
    /// assoc fn with the IDENTICAL signature deduplicates to a single requirement
    /// and emits zero diagnostics. Fails until step-10 (today both copies are
    /// pushed → two 'f' entries).
    #[test]
    fn refining_trait_with_identical_assoc_fn_signature_dedups() {
        let base = make_compiled_trait("Base", vec![], vec![assoc_fn_req("f", Type::dimensionless_scalar())]);
        let derived = make_compiled_trait(
            "Derived",
            vec!["Base".to_string()],
            vec![assoc_fn_req("f", Type::dimensionless_scalar())],
        );

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("Base".to_string(), &base);
        trait_registry.insert("Derived".to_string(), &derived);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "Derived",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        let f_count = ctx.requirements.iter().filter(|r| r.name == "f").count();
        assert_eq!(
            f_count, 1,
            "identical inherited assoc-fn signature should dedup to one 'f' \
             requirement; got {}",
            f_count
        );
        assert!(
            diags.is_empty(),
            "identical signatures must not conflict; got: {:?}",
            diags
        );
    }

    /// Verify that `collect_all_requirements` deduplicates `RequirementKind::Sub`
    /// requirements via `seen_sub_names` when two sibling parent traits both declare
    /// the same sub-component name and structure.
    ///
    /// Topology:
    ///   ParentA (has sub "hole = Hole")
    ///   ParentB (has sub "hole = Hole")
    ///       \   /
    ///       Child
    ///
    /// Unlike a diamond, both ParentA and ParentB are distinct nodes, so `visited` does
    /// not short-circuit either. Both run through the Sub match arm. `seen_sub_names`
    /// records ("hole", ("Hole", "ParentA")) on the first visit; the second visit
    /// (ParentB) finds "hole" already present with the same structure name and continues
    /// (deduplicated). Result: exactly one `hole` requirement, zero diagnostics.
    ///
    /// This is the canonical test for `seen_sub_names` dedup: the `visited` short-circuit
    /// does not fire (ParentA ≠ ParentB), so `seen_sub_names` alone prevents the duplicate.
    #[test]
    fn collect_all_requirements_dedups_sibling_sub_requirements() {
        let sub_hole = TraitRequirement {
            name: "hole".to_string(),
            kind: RequirementKind::Sub("Hole".to_string()),
            span: SourceSpan::empty(0),
        };
        let parent_a = make_compiled_trait("ParentA", vec![], vec![sub_hole.clone()]);
        let parent_b = make_compiled_trait("ParentB", vec![], vec![sub_hole]);
        let child = make_compiled_trait(
            "Child",
            vec!["ParentA".to_string(), "ParentB".to_string()],
            vec![],
        );

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("ParentA".to_string(), &parent_a);
        trait_registry.insert("ParentB".to_string(), &parent_b);
        trait_registry.insert("Child".to_string(), &child);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "Child",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        let hole_count = ctx.requirements.iter().filter(|r| r.name == "hole").count();
        assert_eq!(
            hole_count, 1,
            "Expected exactly one 'hole' sub-requirement (dedup via seen_sub_names), got {}",
            hole_count
        );
        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
    }

    /// Verify that two sibling parent traits declaring the same sub-component name with
    /// *different* structure names emit exactly one conflict diagnostic.
    ///
    /// Topology:
    ///   ParentA (has sub "hole = Hole")
    ///   ParentB (has sub "hole = Rectangle")   ← different structure
    ///       \   /
    ///       Child
    ///
    /// The first visit records ("hole", ("Hole", "ParentA")) in `seen_sub_names`.
    /// The second visit finds "hole" with structure "Rectangle" ≠ "Hole" and pushes a
    /// conflict diagnostic, then continues. The first-seen requirement is kept so the
    /// checker sees at most one entry per sub-name. Result: one conflict diagnostic, one
    /// `hole` requirement in `ctx.requirements`.
    #[test]
    fn collect_all_requirements_sub_conflict_emits_diagnostic() {
        let parent_a = make_compiled_trait(
            "ParentA",
            vec![],
            vec![TraitRequirement {
                name: "hole".to_string(),
                kind: RequirementKind::Sub("Hole".to_string()),
                span: SourceSpan::empty(0),
            }],
        );
        let parent_b = make_compiled_trait(
            "ParentB",
            vec![],
            vec![TraitRequirement {
                name: "hole".to_string(),
                kind: RequirementKind::Sub("Rectangle".to_string()),
                span: SourceSpan::empty(0),
            }],
        );
        let child = make_compiled_trait(
            "Child",
            vec!["ParentA".to_string(), "ParentB".to_string()],
            vec![],
        );

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("ParentA".to_string(), &parent_a);
        trait_registry.insert("ParentB".to_string(), &parent_b);
        trait_registry.insert("Child".to_string(), &child);

        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "Child",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            0,
            &mut diags,
        );

        assert_eq!(
            diags.len(),
            1,
            "Expected exactly one sub-conflict diagnostic, got: {:?}",
            diags
        );
        // The first-seen 'hole' requirement (Hole, from ParentA) is kept; the conflicting
        // one (Rectangle, from ParentB) is dropped after the diagnostic fires.
        let hole_count = ctx.requirements.iter().filter(|r| r.name == "hole").count();
        assert_eq!(
            hole_count, 1,
            "Expected one 'hole' requirement kept (first-seen wins), got {}",
            hole_count
        );
    }

    // ---- helpers for the additional branch tests below ----

    /// Minimal `CompiledTrait` fixture with no defaults, type params, annotations, or
    /// pragmas. Use this to keep test scaffolding lean; only the structurally relevant
    /// fields (`name`, `refinements`, `required_members`) need to vary per test.
    fn make_compiled_trait(
        name: &str,
        refinements: Vec<String>,
        required_members: Vec<TraitRequirement>,
    ) -> CompiledTrait {
        CompiledTrait {
            name: name.to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements,
            required_members,
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Minimal `LetDecl` fixture; only `content_hash` varies between callers.
    fn make_let_decl(name: &str, hash_val: u128) -> reify_ast::LetDecl {
        reify_ast::LetDecl {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            is_aux: false,
            type_expr: None,
            value: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 1.0,
                    is_real: false,
                },
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(hash_val),
        }
    }

    /// Minimal `ParamDecl` fixture (for `DefaultKind::Param` construction).
    fn make_param_decl(name: &str) -> reify_ast::ParamDecl {
        reify_ast::ParamDecl {
            name: name.to_string(),
            doc: None,
            is_priv: false,
            type_expr: None,
            default: None,
            where_clause: None,
            annotations: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        }
    }

    /// Minimal `ConstraintDecl` fixture (for `DefaultKind::Constraint` construction).
    fn make_constraint_decl() -> reify_ast::ConstraintDecl {
        reify_ast::ConstraintDecl {
            label: None,
            expr: reify_ast::Expr {
                kind: reify_ast::ExprKind::BoolLiteral(true),
                span: SourceSpan::empty(0),
            },
            where_clause: None,
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
        }
    }

    /// Depth guard: calling at `depth = MAX_TRAIT_DEPTH + 1` emits exactly one
    /// "too deep" diagnostic and does not panic.
    ///
    /// Simulates being at the tip of a refinement chain longer than `MAX_TRAIT_DEPTH`
    /// by invoking directly with an above-threshold depth. The `visited.insert` check
    /// fires first (correct dedup ordering), then the depth guard fires and returns
    /// after pushing one diagnostic. The registry is empty so this also confirms the
    /// depth guard short-circuits before the registry lookup.
    #[test]
    fn collect_all_requirements_depth_guard_emits_one_diagnostic() {
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let mut ctx = MergeContext::new();
        let mut diags: Vec<Diagnostic> = vec![];
        collect_all_requirements(
            "DeepTrait",
            &trait_registry,
            &mut ctx,
            &HashMap::new(),
            SourceSpan::empty(0),
            MAX_TRAIT_DEPTH + 1,
            &mut diags,
        );

        assert_eq!(
            diags.len(),
            1,
            "Expected exactly one 'too deep' diagnostic, got: {:?}",
            diags
        );
    }

    /// Let-binding conflict: two traits providing `let x` with different `content_hash`
    /// emit exactly one conflict diagnostic; the diagnostic is suppressed when
    /// `structure_members` contains the name (structure override wins).
    ///
    /// Exercises the `seen_let_hashes` dedup path and the
    /// `seen_let_conflict_names` once-per-name gate, plus the
    /// `structure_members.contains_key` suppression branch.
    #[test]
    fn collect_all_requirements_let_conflict_diagnostic_and_suppression() {
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: make_let_decl("x", 1),
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: make_let_decl("x", 2), // different hash → conflict
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let top = CompiledTrait {
            name: "Top".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec!["TraitA".to_string(), "TraitB".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("TraitA".to_string(), &trait_a);
        trait_registry.insert("TraitB".to_string(), &trait_b);
        trait_registry.insert("Top".to_string(), &top);

        // Case 1: No structure override — exactly one conflict diagnostic with
        // the typed `ConflictingTraitLetBindings` code.
        {
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
            assert_eq!(
                diags.len(),
                1,
                "Expected exactly one let-conflict diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].code,
                Some(DiagnosticCode::ConflictingTraitLetBindings),
                "Expected DiagnosticCode::ConflictingTraitLetBindings, got: {:?}",
                diags[0].code
            );
        }

        // Case 2: `structure_members` overrides "x" — hash is never recorded so
        // the second trait's `seen_let_hashes.get` returns None and no conflict fires.
        {
            let mut ctx = MergeContext::new();
            let mut diags: Vec<Diagnostic> = vec![];
            let mut structure_members: HashMap<String, Type> = HashMap::new();
            structure_members.insert("x".to_string(), Type::dimensionless_scalar());
            collect_all_requirements(
                "Top",
                &trait_registry,
                &mut ctx,
                &structure_members,
                SourceSpan::empty(0),
                0,
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "Expected no diagnostics when 'x' is overridden by structure_members, got: {:?}",
                diags
            );
        }
    }

    /// Cache-hit with mismatched value: emits exactly one `Error` diagnostic with the
    /// builder-provided message, the typed `DiagnosticCode` returned by the closure,
    /// and a uniform `"conflict between '…' and '…'"` label; returns `Break(())`;
    /// the map is unchanged.
    #[test]
    fn try_dedup_or_conflict_emits_diagnostic_on_mismatch() {
        use std::ops::ControlFlow;
        let mut map: HashMap<String, (i32, String)> = HashMap::new();
        map.insert("x".to_string(), (7_i32, "TraitA".to_string()));
        let mut diags: Vec<Diagnostic> = vec![];
        let result = try_dedup_or_conflict(
            &mut map,
            "x",
            &9_i32, // different value → conflict
            "TraitB",
            SourceSpan::empty(0),
            |name, existing, existing_trait, new, new_trait| {
                (
                    format!(
                        "BOOM '{}': {} vs {} (traits '{}' and '{}')",
                        name, existing, new, existing_trait, new_trait
                    ),
                    DiagnosticCode::ConflictingTraitRequirements,
                )
            },
            &mut diags,
        );
        assert_eq!(result, ControlFlow::Break(()));
        // Map must remain unchanged (conflict does NOT overwrite)
        assert_eq!(map.get("x"), Some(&(7_i32, "TraitA".to_string())));
        assert_eq!(
            diags.len(),
            1,
            "Expected exactly one diagnostic, got: {:?}",
            diags
        );
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(
            diags[0].code,
            Some(DiagnosticCode::ConflictingTraitRequirements)
        );
        assert_eq!(
            diags[0].message,
            "BOOM 'x': 7 vs 9 (traits 'TraitA' and 'TraitB')"
        );
        assert_eq!(diags[0].labels.len(), 1);
        assert_eq!(
            diags[0].labels[0].message,
            "conflict between 'TraitA' and 'TraitB'"
        );
    }

    /// Cache-hit with equal value: returns `Break(())` silently; the map is unchanged and
    /// no diagnostic is emitted. The conflict closure must not be invoked (guarded by
    /// `unreachable!`).
    #[test]
    fn try_dedup_or_conflict_dedups_silently_on_equal_value() {
        use std::ops::ControlFlow;
        let mut map: HashMap<String, (i32, String)> = HashMap::new();
        map.insert("x".to_string(), (7_i32, "TraitA".to_string()));
        let mut diags: Vec<Diagnostic> = vec![];
        let result = try_dedup_or_conflict(
            &mut map,
            "x",
            &7_i32, // same value as already present
            "TraitB",
            SourceSpan::empty(0),
            // No code argument needed: the closure is the sole carrier of conflict-only
            // data, and `unreachable!` proves the equal-value path never invokes it.
            |_, _, _, _, _| -> (String, DiagnosticCode) {
                unreachable!("no closure on equal-value dedup")
            },
            &mut diags,
        );
        assert_eq!(result, ControlFlow::Break(()));
        // Map must not be overwritten (still TraitA, not TraitB)
        assert_eq!(map.get("x"), Some(&(7_i32, "TraitA".to_string())));
        assert!(
            diags.is_empty(),
            "Expected no diagnostics on dedup, got: {:?}",
            diags
        );
    }

    /// Cache-miss path of `try_dedup_or_conflict`: calling with a name not in the map
    /// inserts `(value, trait_name)` and returns `Continue(())`. The conflict closure is
    /// never invoked.
    #[test]
    fn try_dedup_or_conflict_inserts_on_cache_miss() {
        use std::ops::ControlFlow;
        let mut map: HashMap<String, (i32, String)> = HashMap::new();
        let mut diags: Vec<Diagnostic> = vec![];
        let result = try_dedup_or_conflict(
            &mut map,
            "x",
            &7_i32,
            "TraitA",
            SourceSpan::empty(0),
            // No code argument needed: cache-miss path never invokes the closure.
            |_, _, _, _, _| -> (String, DiagnosticCode) { unreachable!("not on cache miss") },
            &mut diags,
        );
        assert_eq!(result, ControlFlow::Continue(()));
        assert_eq!(map.get("x"), Some(&(7_i32, "TraitA".to_string())));
        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
    }

    /// Verify that `collect_all_requirements` collects a single
    /// `RequirementKind::AssocType` requirement from a trait with no refinements.
    ///
    /// RED (step-3): fails today because the inert `RequirementKind::AssocType(_) => continue`
    /// arm drops the requirement instead of pushing it.
    #[test]
    fn collect_all_requirements_collects_assoc_type_requirement() {
        let trait_a = make_compiled_trait(
            "TraitA",
            vec![],
            vec![TraitRequirement {
                name: "Material".to_string(),
                kind: RequirementKind::AssocType(None),
                span: SourceSpan::empty(0),
            }],
        );

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

        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
        let mat_count = ctx
            .requirements
            .iter()
            .filter(|r| r.name == "Material" && matches!(r.kind, RequirementKind::AssocType(None)))
            .count();
        assert_eq!(
            mat_count, 1,
            "Expected exactly one AssocType 'Material' requirement, got {}",
            mat_count
        );
    }

    /// Diamond dedup for `RequirementKind::AssocType`: Base declares `type Material`,
    /// Mid1 and Mid2 both refine Base, Top refines Mid1+Mid2. The `visited` set
    /// ensures Base is visited exactly once → one requirement, zero diagnostics.
    ///
    /// RED (step-3): fails today because the inert arm drops AssocType requirements.
    #[test]
    fn collect_all_requirements_dedups_diamond_assoc_type_requirement() {
        let base = make_compiled_trait(
            "Base",
            vec![],
            vec![TraitRequirement {
                name: "Material".to_string(),
                kind: RequirementKind::AssocType(None),
                span: SourceSpan::empty(0),
            }],
        );
        let mid1 = make_compiled_trait("Mid1", vec!["Base".to_string()], vec![]);
        let mid2 = make_compiled_trait("Mid2", vec!["Base".to_string()], vec![]);
        let top = make_compiled_trait(
            "Top",
            vec!["Mid1".to_string(), "Mid2".to_string()],
            vec![],
        );

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

        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
        let mat_count = ctx
            .requirements
            .iter()
            .filter(|r| r.name == "Material")
            .count();
        assert_eq!(
            mat_count, 1,
            "Diamond dedup should yield exactly one 'Material' requirement, got {}",
            mat_count
        );
    }

    /// Verify that `collect_all_requirements` captures a `DefaultKind::AssocType` default
    /// into `ctx.defaults`. First-seen-by-name dedup: a second same-name default
    /// (via diamond or sibling) should be dropped.
    ///
    /// RED (step-3): fails today because the `unreachable!()` in the composite-key match
    /// panics when an AssocType default is encountered (no early if-let block exists yet).
    #[test]
    fn collect_all_requirements_collects_assoc_type_default() {
        let steel_ty = Type::StructureRef("Steel".to_string());
        let trait_with_default = CompiledTrait {
            name: "HasMaterial".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("Material".to_string()),
                kind: DefaultKind::AssocType(steel_ty.clone()),
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let top = CompiledTrait {
            name: "Top".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec!["HasMaterial".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("HasMaterial".to_string(), &trait_with_default);
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

        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
        let assoc_defaults: Vec<_> = ctx
            .defaults
            .iter()
            .filter(|d| {
                d.name.as_deref() == Some("Material")
                    && matches!(&d.kind, DefaultKind::AssocType(ty) if ty == &steel_ty)
            })
            .collect();
        assert_eq!(
            assoc_defaults.len(),
            1,
            "Expected exactly one AssocType 'Material' default with Steel type, got {}",
            assoc_defaults.len()
        );
    }

    /// Param/Constraint cross-interference: two traits each providing a named default
    /// for the same member name — one `Param`, one `Constraint` — produce no conflict
    /// diagnostic and both defaults are collected.
    ///
    /// The composite key `(name, DefaultKindTag)` gives `Param` and `Constraint`
    /// independent slots in `seen_defaults`, so they never cross-compare or conflict.
    #[test]
    fn collect_all_requirements_param_and_constraint_same_name_no_cross_interference() {
        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("y".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::dimensionless_scalar(),
                    default_decl: make_param_decl("y"),
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("y".to_string()),
                kind: DefaultKind::Constraint(make_constraint_decl()),
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let top = CompiledTrait {
            name: "Top".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec!["TraitA".to_string(), "TraitB".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("TraitA".to_string(), &trait_a);
        trait_registry.insert("TraitB".to_string(), &trait_b);
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

        assert!(
            diags.is_empty(),
            "Expected no diagnostics: Param and Constraint use separate composite-key slots, got: {:?}",
            diags
        );
        // Both defaults are independently collected (one Param, one Constraint).
        assert_eq!(
            ctx.defaults.len(),
            2,
            "Expected 2 defaults (one Param, one Constraint), got {}",
            ctx.defaults.len()
        );
    }

    /// Two traits providing `DefaultKind::AssocType` for the same name but with DIFFERENT
    /// resolved types should emit exactly one `ConflictingTraitAssocType` diagnostic.
    /// When the structure overrides the name (`structure_members` contains it), the
    /// conflict is suppressed (zero diagnostics), mirroring the let-binding suppression.
    ///
    /// Mirrors `collect_all_requirements_let_conflict_diagnostic_and_suppression`.
    ///
    /// RED (step-5): fails today because step-4 only deduplicates (first-seen wins) with
    /// no conflict detection — two different-typed defaults are silently dropped, emitting
    /// zero diagnostics when one is expected.
    #[test]
    fn collect_all_requirements_assoc_type_default_conflict_and_suppression() {
        let steel_ty = Type::StructureRef("Steel".to_string());
        let alum_ty = Type::StructureRef("Aluminum".to_string());

        let trait_a = CompiledTrait {
            name: "TraitA".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("Material".to_string()),
                kind: DefaultKind::AssocType(steel_ty.clone()),
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let trait_b = CompiledTrait {
            name: "TraitB".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("Material".to_string()),
                kind: DefaultKind::AssocType(alum_ty.clone()), // different type → conflict
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let top = CompiledTrait {
            name: "Top".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec!["TraitA".to_string(), "TraitB".to_string()],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("TraitA".to_string(), &trait_a);
        trait_registry.insert("TraitB".to_string(), &trait_b);
        trait_registry.insert("Top".to_string(), &top);

        // Case 1: No structure override — exactly one conflict diagnostic with
        // the typed `ConflictingTraitAssocType` code naming "Material".
        {
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
            assert_eq!(
                diags.len(),
                1,
                "Expected exactly one assoc-type conflict diagnostic, got: {:?}",
                diags
            );
            assert_eq!(
                diags[0].code,
                Some(DiagnosticCode::ConflictingTraitAssocType),
                "Expected DiagnosticCode::ConflictingTraitAssocType, got: {:?}",
                diags[0].code
            );
            assert!(
                diags[0].message.contains("Material"),
                "Conflict diagnostic should name 'Material'; got: {}",
                diags[0].message
            );
        }

        // Case 2: `structure_members` overrides "Material" — conflict is suppressed.
        {
            let mut ctx = MergeContext::new();
            let mut diags: Vec<Diagnostic> = vec![];
            let mut structure_members: HashMap<String, Type> = HashMap::new();
            structure_members.insert("Material".to_string(), steel_ty.clone());
            collect_all_requirements(
                "Top",
                &trait_registry,
                &mut ctx,
                &structure_members,
                SourceSpan::empty(0),
                0,
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "Expected no diagnostics when 'Material' is overridden by structure_members, got: {:?}",
                diags
            );
        }
    }
}
