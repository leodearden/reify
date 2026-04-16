use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Collect all structure member names for conformance checking.
    let empty_params: HashSet<String> = HashSet::new();
    let structure_members: HashMap<String, Type> = structure
        .members
        .iter()
        .filter_map(|m| match m {
            reify_syntax::MemberDecl::Param(p) => {
                let ty = p
                    .type_expr
                    .as_ref()
                    .map(|te| {
                        resolve_type_with_aliases(&te.name, &empty_params, alias_registry)
                            .or_else(|| {
                                if enum_defs.iter().any(|e| e.name == te.name) {
                                    Some(Type::Enum(te.name.clone()))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "unresolved type in conformance check: {}",
                                        te.name
                                    ))
                                    .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                                );
                                Type::Real
                            })
                    })
                    .unwrap_or_else(|| {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "trait member '{}' has no type annotation; cannot infer type",
                                p.name
                            ))
                            .with_label(DiagnosticLabel::new(p.span, "missing type annotation")),
                        );
                        Type::Real
                    });
                Some((p.name.clone(), ty))
            }
            reify_syntax::MemberDecl::Let(l) => {
                // let bindings get their type from expression inference, not annotations.
                // Only include in structure_members when there is an explicit type annotation;
                // omitting is safe because if a trait requires this member, the conformance
                // check will report "missing required member" rather than a spurious
                // "no type annotation" error.
                let te = l.type_expr.as_ref()?;
                let ty = resolve_type_with_aliases(&te.name, &empty_params, alias_registry)
                    .or_else(|| {
                        if enum_defs.iter().any(|e| e.name == te.name) {
                            Some(Type::Enum(te.name.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "unresolved type in conformance check: {}",
                                te.name
                            ))
                            .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                        );
                        Type::Real
                    });
                Some((l.name.clone(), ty))
            }
            _ => None,
        })
        .collect();

    // Collect structure constraint labels.
    let structure_constraint_labels: HashSet<String> = structure
        .members
        .iter()
        .filter_map(|m| {
            if let reify_syntax::MemberDecl::Constraint(c) = m {
                c.label.clone()
            } else {
                None
            }
        })
        .collect();

    // Collect all requirements and defaults from all trait bounds,
    // handling refinement chains and deduplication.
    // MergeContext bundles the output accumulators (requirements, defaults) and
    // the 5 mutable tracking maps (visited, seen_names, seen_defaults,
    // seen_let_hashes, seen_let_conflict_names) so the recursive
    // collect_all_requirements signature stays within Clippy's argument-count limit.
    let mut ctx = MergeContext::new();

    for trait_bound in structure.trait_bounds {
        collect_all_requirements(
            &trait_bound.name,
            trait_registry,
            &mut ctx,
            &structure_members,
            structure.span,
            diagnostics,
        );
    }

    // Tag used when cross-checking requirements against available defaults.
    // A `param` requirement can only be satisfied by a `param` default, and a `let`
    // requirement only by a `let` default. A kind mismatch is treated the same as "no
    // default" so the user sees "missing required member" rather than a confusing
    // kind-mismatch error (the fix is the same either way: provide the member).
    //
    // See also `DefaultKindTag` (module-level) — this enum intentionally omits
    // `Constraint` because constraints are never candidates for satisfying requirements.
    #[derive(Copy, Clone, PartialEq, Eq, Hash)]
    enum AvailableDefaultKind {
        Param,
        Let,
    }

    // Build a map of available default names from ctx.defaults (non-constraint, named).
    // Used to cross-check requirements: a requirement is satisfied if the structure
    // provides the member OR if another trait in the bound set provides a matching default
    // of the SAME kind. Kind mismatches are ignored (treated as absent).
    //
    // Keyed by (name, AvailableDefaultKind) so Param and Let defaults for the same
    // member name occupy separate slots and are looked up independently. A Param default
    // can satisfy a Param requirement, and a Let default can satisfy a Let requirement,
    // without interfering with each other.
    let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = ctx.defaults
        .iter()
        .filter_map(|d| {
            let name = d.name.as_deref()?;
            let (kind, ty) = match &d.kind {
                DefaultKind::Param { cell_type, .. } => {
                    (AvailableDefaultKind::Param, cell_type.clone())
                }
                DefaultKind::Let(_) => (AvailableDefaultKind::Let, Type::Real),
                DefaultKind::Constraint(_) => return None,
            };
            Some(((name.to_string(), kind), ty))
        })
        .collect();

    // Check each requirement against structure members.
    for req in &ctx.requirements {
        match &req.kind {
            RequirementKind::Param(expected_type) | RequirementKind::Let(expected_type) => {
                // Determine which default kind can satisfy this requirement.
                let required_default_kind = match &req.kind {
                    RequirementKind::Param(_) => AvailableDefaultKind::Param,
                    RequirementKind::Let(_) => AvailableDefaultKind::Let,
                    _ => unreachable!(),
                };
                match structure_members.get(&req.name) {
                    Some(actual_type) => {
                        if !implicitly_converts_to(actual_type, expected_type) {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "type mismatch for trait member '{}': expected {}, got {}",
                                    req.name, expected_type, actual_type
                                ))
                                .with_label(DiagnosticLabel::new(structure.span, "type mismatch")),
                            );
                        }
                    }
                    None => {
                        // Check if a matching default from another trait satisfies this requirement.
                        // Only a same-kind default can satisfy: a `let` default does NOT satisfy
                        // a `param` requirement (param slots must be externally settable).
                        // The (name, kind) composite key means the lookup is already kind-filtered —
                        // no additional kind-guard is needed on the match arms.
                        //
                        // Note: `.get(&(req.name.clone(), ...))` allocates a String on every lookup
                        // because `HashMap<(String, K), V>` has no `Borrow` impl for `(&str, K)`.
                        // Requirement counts are small in practice so this is not a hot path; if it
                        // ever becomes one, switch to a two-level map `HashMap<String, HashMap<K, V>>`.
                        match available_defaults
                            .get(&(req.name.clone(), required_default_kind))
                        {
                            Some(default_type)
                                if implicitly_converts_to(default_type, expected_type) =>
                            {
                                // Same-kind default with matching type satisfies the requirement.
                            }
                            Some(default_type) => {
                                // Same-kind default but wrong type → type mismatch.
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "type mismatch for trait member '{}': \
                                         requirement expects {}, available default has {}",
                                        req.name, expected_type, default_type
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        structure.span,
                                        "type mismatch",
                                    )),
                                );
                            }
                            None => {
                                // No default of the required kind — treat as missing.
                                // A param requirement with only a let default in scope means the
                                // structure must provide a settable param slot itself.
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "missing required member '{}' (expected type: {})",
                                        req.name, expected_type
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        structure.span,
                                        "required by trait",
                                    )),
                                );
                            }
                        }
                    }
                }
            }
            RequirementKind::Sub(structure_name) => {
                let has_sub = structure.members.iter().any(|m| {
                    if let reify_syntax::MemberDecl::Sub(s) = m {
                        s.name == req.name && s.structure_name == *structure_name
                    } else {
                        false
                    }
                });
                if !has_sub {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "missing required sub-component '{}' of type '{}'",
                            req.name, structure_name
                        ))
                        .with_label(DiagnosticLabel::new(structure.span, "required by trait")),
                    );
                }
            }
        }
    }

    // Pre-register default member names in scope so their expressions can
    // reference each other (e.g., constraint x > 0 references param x from same trait).
    // register_if_absent provides the no-overwrite guarantee: first-seen type wins,
    // and the method itself is safe against cross-kind overwrites without a call-site guard.
    for default in &ctx.defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
        {
            let ty = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let(_) => Type::Real,
                DefaultKind::Constraint(_) => continue,
            };
            let _was_new = scope.register_if_absent(name, ty);
            // If _was_new is false, a prior default already registered this name
            // (first-seen type wins). Useful hook for future debug-level logging
            // to surface trait-merge conflicts where two traits supply the same
            // default name with different types.
        }
    }

    // Inject defaults for members not overridden by the structure.
    for default in &ctx.defaults {
        match &default.kind {
            DefaultKind::Param {
                cell_type,
                default_decl,
            } => {
                let name = default
                    .name
                    .as_deref()
                    .expect("DefaultKind::Param always has Some(name)");
                if !structure_members.contains_key(name) {
                    // Inject default param into value_cells
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    let default_expr = default_decl
                        .default
                        .as_ref()
                        .map(|expr| compile_expr(expr, scope, enum_defs, functions, diagnostics));

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Private,
                        cell_type: cell_type.clone(),
                        default_expr,
                        solver_hints: Vec::new(),
                        span: default.span,
                    });
                }
            }
            DefaultKind::Let(let_decl) => {
                let name = default
                    .name
                    .as_deref()
                    .expect("DefaultKind::Let always has Some(name)");
                if !structure_members.contains_key(name) {
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    let compiled_expr =
                        compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics);

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        cell_type: compiled_expr.result_type.clone(),
                        default_expr: Some(compiled_expr),
                        solver_hints: Vec::new(),
                        span: default.span,
                    });
                }
            }
            DefaultKind::Constraint(constraint_decl) => {
                let label = constraint_decl.label.as_deref();
                let already_has = label.is_some_and(|l| structure_constraint_labels.contains(l));
                if !already_has {
                    let compiled_expr = compile_expr(
                        &constraint_decl.expr,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                    );

                    let constraint_id = ConstraintNodeId {
                        entity: structure.name.to_string(),
                        index: *constraint_index,
                    };
                    *constraint_index += 1;

                    constraints.push(CompiledConstraint {
                        id: constraint_id,
                        label: constraint_decl.label.clone(),
                        expr: compiled_expr,
                        span: default.span,
                        domain: None,
                        optimized_target: None,
                    });
                }
            }
        }
    }
}

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
pub(crate) fn collect_all_requirements(
    trait_name: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    ctx: &mut MergeContext,
    structure_members: &HashMap<String, Type>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ctx.visited.insert(trait_name.to_string()) {
        return; // Already visited (diamond pattern)
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
            ctx.defaults.push(default.clone());
        } else if let Some(name) = &default.name {
            // For let bindings: use content_hash comparison to distinguish same
            // expression (dedup) vs different expression (conflict).
            if let DefaultKind::Let(let_decl) = &default.kind {
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
                                format!("conflict between '{}' and '{}'", existing_trait, trait_name),
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
                DefaultKind::Let(_) => {
                    // Unreachable: all Let defaults are handled by the early
                    // `if let DefaultKind::Let(let_decl)` block above, which always
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
            ctx.seen_defaults.insert(key, (default_type, trait_name.to_string()));
            ctx.defaults.push(default.clone());
        }
    }
}

