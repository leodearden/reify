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
                        // Extract name from Named variant; DimensionalOp can't be a param type.
                        let name_opt = match &te.kind {
                            reify_syntax::TypeExprKind::Named { name, .. } => Some(name.as_str()),
                            reify_syntax::TypeExprKind::DimensionalOp { .. } => None,
                        };
                        if let Some(name) = name_opt {
                            resolve_type_with_aliases(name, &empty_params, alias_registry)
                                .or_else(|| {
                                    if enum_defs.iter().any(|e| e.name == name) {
                                        Some(Type::Enum(name.to_string()))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "unresolved type in conformance check: {}",
                                            name
                                        ))
                                        .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                                    );
                                    Type::Real
                                })
                        } else {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unresolved type in conformance check: {}",
                                    te
                                ))
                                .with_label(DiagnosticLabel::new(te.span, "unexpected dimensional expression")),
                            );
                            Type::Real
                        }
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
                // Extract name from Named variant; DimensionalOp can't be a let type annotation.
                let name_opt = match &te.kind {
                    reify_syntax::TypeExprKind::Named { name, .. } => Some(name.as_str()),
                    reify_syntax::TypeExprKind::DimensionalOp { .. } => None,
                };
                if let Some(name) = name_opt {
                    let ty = resolve_type_with_aliases(name, &empty_params, alias_registry)
                        .or_else(|| {
                            if enum_defs.iter().any(|e| e.name == name) {
                                Some(Type::Enum(name.to_string()))
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unresolved type in conformance check: {}",
                                    name
                                ))
                                .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                            );
                            Type::Real
                        });
                    Some((l.name.clone(), ty))
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unresolved type in conformance check: {}",
                            te
                        ))
                        .with_label(DiagnosticLabel::new(te.span, "unexpected dimensional expression")),
                    );
                    Some((l.name.clone(), Type::Real))
                }
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

    // Cache of compiled expressions for unannotated let defaults, keyed by
    // default name.  Populated by the pre-register pass below and drained by
    // the injection loop (task 1834 step-9) to avoid double-compilation of
    // the same expression.  Also consumed by the `available_defaults` builder
    // (task 1834 step-8) so unannotated let defaults contribute their *inferred*
    // type to requirement-matching, instead of the previous `Type::Real` fallback.
    //
    // INVARIANTS that make the name-only key safe (no `AvailableDefaultKind`
    // discriminator, unlike `available_defaults` below):
    //   1. Only `DefaultKind::Let { cell_type: None }` inserts into this cache —
    //      no cross-kind writes, so a `Param`-named `x` and a `Let`-named `x`
    //      never collide on this map.
    //   2. Only the `DefaultKind::Let` arm of the injection loop reads from
    //      this cache — reads are kind-guarded by the enclosing match, so
    //      the lookup cannot be satisfied by a non-Let entry.
    //   3. `collect_all_requirements` deduplicates defaults by (name, kind)
    //      across the trait-bound set, so at most one unannotated-let default
    //      with a given name reaches this loop.
    //
    // If any of these ever drift, key the cache on
    // `(String, AvailableDefaultKind)` to match `available_defaults` for
    // symmetry and explicit kind discrimination.
    //
    // DESIGN LIMITATION (acknowledged simplification): type inference for
    // unannotated lets proceeds in `ctx.defaults` iteration order.  Two
    // unannotated lets that forward-reference each other — e.g., `let a = b + 1mm`
    // where `let b = 5mm` is *also* unannotated and appears *after* `a` in
    // iteration order — will fail inference for the forward-referencing
    // binding (`b` is not in scope when `a`'s expression is compiled), yielding
    // a diagnostic from `compile_expr`.  Adding an annotation to the
    // *forward-referencing* binding (the one that needs its referent in scope)
    // unblocks the case, because the annotated branch above skips `compile_expr`
    // in the pre-register pass — by the time the injection loop compiles that
    // binding's expression, the referent has already been registered.
    // Annotating only the *forward-referenced* binding does NOT help here,
    // because the pre-register pass walks `ctx.defaults` in iteration order and
    // the forward-referencing binding's `compile_expr` still fires before the
    // referenced binding's match arm runs.  A two-pass variant — first register
    // all annotated defaults/params, then compile unannotated-let expressions —
    // would make either-side annotation sufficient, but is out of scope for
    // task 1834 ("documenting as intentional simplification").
    let mut inferred_let_exprs: HashMap<String, CompiledExpr> = HashMap::new();

    // Pre-register default member names in scope so their expressions can
    // reference each other (e.g., constraint x > 0 references param x from same trait).
    // register_if_absent provides the no-overwrite guarantee: first-seen type wins,
    // and the method itself is safe against cross-kind overwrites without a call-site guard.
    //
    // Two branches for Let defaults:
    //   - annotated (cell_type: Some(ty))   → register the annotation directly,
    //   - unannotated (cell_type: None)     → compile the expression in the
    //     partial scope, register the inferred `result_type`, and cache the
    //     compiled_expr in `inferred_let_exprs` for the injection loop.
    //
    // This pass runs BEFORE `available_defaults` is built so unannotated-let
    // inference results feed the requirement-matching lookup below.
    for default in &ctx.defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
        {
            let ty = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let {
                    cell_type: Some(annotation_ty),
                    ..
                } => annotation_ty.clone(),
                DefaultKind::Let {
                    cell_type: None,
                    let_decl,
                } => {
                    // Unannotated let: infer the type from the expression,
                    // compiled in the partial scope visible so far (structure
                    // members + already-registered defaults).  Cache the
                    // compiled_expr so the injection loop can reuse it.
                    let compiled_expr =
                        compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics);
                    let inferred_ty = compiled_expr.result_type.clone();
                    inferred_let_exprs.insert(name.clone(), compiled_expr);
                    inferred_ty
                }
                DefaultKind::Constraint(_) => continue,
            };
            // `ty` is cloned here so we retain the value for the debug event on
            // the cold conflict path (`!was_new`). `register_if_absent` consumes its
            // argument, so we cannot borrow `ty` after the call without the clone.
            // This is a compile-time-only path; the clone cost is negligible.
            let was_new = scope.register_if_absent(name, ty.clone());
            // First-seen type wins. When was_new is false a prior default already
            // owns this name — the incoming type is silently dropped. Emit a debug
            // event so trait-merge conflicts are observable at runtime.
            if !was_new {
                tracing::debug!(
                    target: "reify_compiler::conformance",
                    name = %name,
                    entity = %structure.name,
                    ignored_ty = ?ty,
                    "trait-merge conflict: second default with same name ignored; first-seen type wins"
                );
            }
        }
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
    //
    // For unannotated let defaults (`cell_type: None`), the advertised type comes from
    // `inferred_let_exprs` populated by the pre-register pass above — see task 1834 step-8.
    // The final `Type::Real` fallback is reached only when inference itself failed
    // (the expression errored out and left no cached result), and matches the pre-fix
    // behavior as a safety net for that pathological case.
    let available_defaults: HashMap<(String, AvailableDefaultKind), Type> = ctx
        .defaults
        .iter()
        .filter_map(|d| {
            let name = d.name.as_deref()?;
            let (kind, ty) = match &d.kind {
                DefaultKind::Param { cell_type, .. } => {
                    (AvailableDefaultKind::Param, cell_type.clone())
                }
                DefaultKind::Let { cell_type, .. } => {
                    let resolved = cell_type.clone().unwrap_or_else(|| {
                        inferred_let_exprs
                            .get(name)
                            .map(|e| e.result_type.clone())
                            .unwrap_or(Type::Real)
                    });
                    (AvailableDefaultKind::Let, resolved)
                }
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
                        match available_defaults.get(&(req.name.clone(), required_default_kind)) {
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
                                    .with_label(
                                        DiagnosticLabel::new(structure.span, "type mismatch"),
                                    ),
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
                                    .with_label(
                                        DiagnosticLabel::new(structure.span, "required by trait"),
                                    ),
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
            DefaultKind::Let {
                cell_type,
                let_decl,
            } => {
                let name = default
                    .name
                    .as_deref()
                    .expect("DefaultKind::Let always has Some(name)");
                if !structure_members.contains_key(name) {
                    let cell_id = ValueCellId {
                        entity: structure.name.to_string(),
                        member: name.to_string(),
                    };

                    // Reuse the compiled_expr cached by the pre-register/inference
                    // pass (task 1834 step-9) to avoid a second compilation of the
                    // same expression.  The dispatch mirrors the pre-register
                    // branches: unannotated lets always populate the cache there,
                    // annotated lets never do.
                    //
                    // Cache miss handling (task 1834 reviewer_comprehensive
                    // robustness fix): a cache miss on the `None` arm means the
                    // pre-register pass did not compile this expression, which
                    // would only happen if a future refactor decoupled the
                    // pre-register guard (`!structure_members.contains_key(name)`)
                    // from the injection guard or changed the cache key (today
                    // name-only, keyed on the invariants documented beside the
                    // cache declaration above).  Emit a single
                    // internal-consistency diagnostic and skip injection rather
                    // than silently recompiling — a recompile would risk
                    // duplicating diagnostics that the pre-register pass already
                    // pushed for the same AST node if the invariants ever drift
                    // such that both passes run for one name.  `debug_assert!`
                    // still trips in dev/test so the drift is caught early.
                    let compiled_expr = match cell_type {
                        Some(_) => {
                            compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics)
                        }
                        None => {
                            debug_assert!(
                                inferred_let_exprs.contains_key(name),
                                "unannotated let '{}' should have been cached by the \
                                 pre-register pass (Pass 2) — cache miss indicates a \
                                 drift between the pre-register guard and the injection guard",
                                name
                            );
                            match inferred_let_exprs.remove(name) {
                                Some(ce) => ce,
                                None => {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "internal error: compiled expression for unannotated \
                                             trait let '{}' was not cached by the pre-register \
                                             pass; this indicates a drift between the pre-register \
                                             and injection guards in conformance.rs",
                                            name
                                        ))
                                        .with_label(
                                            DiagnosticLabel::new(
                                                default.span,
                                                "internal consistency",
                                            ),
                                        ),
                                    );
                                    // Skip injection for this default rather than
                                    // silently recompiling (see comment block above).
                                    continue;
                                }
                            }
                        }
                    };

                    // Cross-check the expression type against the let's annotation.
                    // The annotation captures user intent; any drift here is an error.
                    //
                    // Use `type_compatible` (not `implicitly_converts_to`) so the check
                    // honors Int→Real widening — `let x : Real = 42.0` parses the
                    // expression as `Int` (parser quirk on whole-number `.0` literals,
                    // expr.rs:102-109) and the annotation captures the user's `Real`
                    // intent.  `type_compatible` is the same widening relation applied
                    // throughout type checking (type_compat.rs:81), so accepting it here
                    // matches the rest of the compiler instead of being stricter at this
                    // one site.  See task 1834 esc-1834-58 for the trade-off; the
                    // requirement-vs-member sites at lines 268/293 keep the stricter
                    // `implicitly_converts_to` because they compare two annotated types
                    // (no Int-literal source).
                    if let Some(annotation_ty) = cell_type
                        && !type_compatible(annotation_ty, &compiled_expr.result_type)
                    {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "type mismatch for trait let '{}': annotation expects {}, expression evaluates to {}",
                                name, annotation_ty, compiled_expr.result_type
                            ))
                            .with_label(DiagnosticLabel::new(default.span, "type mismatch")),
                        );
                    }

                    // Annotation is authoritative on the injected cell when present
                    // (matches the scope pre-registration at ~line 167 which also
                    // prefers the annotation over the inferred expression type).
                    // Fall back to the inferred expression type only when there
                    // is no annotation.
                    let injected_cell_type = cell_type
                        .clone()
                        .unwrap_or_else(|| compiled_expr.result_type.clone());

                    value_cells.push(ValueCellDecl {
                        id: cell_id,
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        cell_type: injected_cell_type,
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
