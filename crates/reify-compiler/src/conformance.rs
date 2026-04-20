use super::*;

/// Maximum allowed depth for trait refinement chains to prevent stack overflow
/// from pathologically deep but acyclic hierarchies. Realistic refinement chains
/// rarely exceed ~10 levels; 128 provides ample headroom.
const MAX_TRAIT_DEPTH: usize = 128;

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    trait_names: &HashSet<String>,
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
                            resolve_type_with_aliases(
                                name,
                                &empty_params,
                                alias_registry,
                                trait_names,
                            )
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
                    let ty = resolve_type_with_aliases(
                        name,
                        &empty_params,
                        alias_registry,
                        trait_names,
                    )
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
            0,
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
    // default name.  Populated by Pass 2 below and drained by the injection
    // loop (task 1834 step-9) to avoid double-compilation of the same
    // expression.  Also consumed by the `available_defaults` builder
    // (task 1834 step-8) so unannotated let defaults contribute their
    // *inferred* type to requirement-matching, instead of the previous
    // `Type::Real` fallback.
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
    // TWO-PASS PRE-REGISTER DESIGN (task 1834 amendment — reviewer_comprehensive
    // behavior_regression fix):
    //   Pass 1 — register every *annotated* default (Param + Let with
    //     `Some(cell_type)`) into the scope.  No expression compilation
    //     happens here, so ordering within `ctx.defaults` does not matter
    //     for the annotated types made visible to Pass 2.
    //   Pass 2 — for each *unannotated* Let (`cell_type: None`), compile
    //     the expression against the fully-populated annotated scope from
    //     Pass 1, cache the compiled_expr in `inferred_let_exprs`, and
    //     register the inferred `result_type`.
    //
    // The split restores the pre-1834 tolerance for forward references to
    // any *annotated* member: before this amendment, Pass 1+2 were a single
    // pass that walked `ctx.defaults` in source order, so an unannotated
    // `let a = b + 1mm` appearing before `let b : Length = 2mm` would compile
    // against a scope that did not yet contain `b` — a silent regression
    // vs. the pre-1834 code, which registered every annotated type up front.
    // Both passes run BEFORE `available_defaults` is built so Pass 2's
    // inference results feed the requirement-matching lookup below.
    //
    // DESIGN LIMITATION (acknowledged simplification): Pass 2 still walks
    // `ctx.defaults` in source order.  Two *unannotated* lets that
    // forward-reference each other — e.g., `let a = b + 1mm` where
    // `let b = 5mm` is *also* unannotated — will fail inference for the
    // forward-referencing binding (`b` is not in scope when `a`'s
    // expression is compiled), yielding an `unresolved name` diagnostic.
    // Annotating *either* binding unblocks the case because annotated
    // types are registered by Pass 1.  A topological ordering pass over
    // unannotated lets would remove the limitation entirely but is out of
    // scope for task 1834 ("documenting as intentional simplification").
    let mut inferred_let_exprs: HashMap<String, CompiledExpr> = HashMap::new();
    // Unannotated-let defaults whose scope slot was already claimed by an annotated
    // type in Pass 1.  Pass 2 records names here and skips the `inferred_let_exprs`
    // insert so the injection loop does not emit a duplicate Let cell alongside the
    // Param/annotated-Let cell that will already be injected for the same name.
    // The injection loop uses this set to distinguish a deliberate skip from drift.
    let mut pass2_skipped: HashSet<String> = HashSet::new();

    // Shared conflict logger for `register_if_absent` Occupied returns.  Captures
    // `&structure.name` from the enclosing scope so both Pass 1 and Pass 2 call
    // sites stay structurally identical — no drift risk if the message or fields
    // ever change.
    let log_conflict = |name: &str, ignored_ty: Type| {
        tracing::debug!(
            target: "reify_compiler::conformance",
            name = %name,
            entity = %structure.name,
            ignored_ty = ?ignored_ty,
            "trait-merge conflict: second default with same name ignored; first-seen type wins"
        );
    };

    // Pass 1: register all *annotated* defaults (Param, Let-with-annotation).
    // Unannotated lets and constraints are deferred to Pass 2 / injection.
    // register_if_absent provides the no-overwrite guarantee: first-seen type
    // wins, and the method itself is safe against cross-kind overwrites
    // without a call-site guard.
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
                // Deferred to Pass 2 — needs Pass 1's scope to compile against.
                DefaultKind::Let { cell_type: None, .. } => continue,
                DefaultKind::Constraint(_) => continue,
            };
            // First-seen type wins. `ty` is moved into `register_if_absent`; on
            // the cold Occupied (conflict) path the method hands it back via
            // `Some(ignored_ty)` for the debug emission, so no clone is needed on
            // the hot Vacant insertion path.
            if let Some(ignored_ty) = scope.register_if_absent(name, ty) {
                log_conflict(name, ignored_ty);
            }
        }
    }

    // Pass 2: compile each *unannotated* Let's expression against the
    // fully-populated annotated scope from Pass 1 and register its inferred
    // type.  When `register_if_absent` finds the scope slot already claimed
    // (Pass 1 registered an annotated Param or Let), the compiled expression
    // is discarded and the name is recorded in `pass2_skipped` so the
    // injection loop skips Let-cell injection — preventing a duplicate
    // (entity, member) cell alongside the annotated-type injection.  When the
    // slot is vacant, the expression is cached in `inferred_let_exprs` for
    // reuse by the injection loop (avoids double compilation) and by
    // `available_defaults` (so requirement-matching uses the inferred type
    // instead of the old `Type::Real` fallback).
    for default in &ctx.defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
            && let DefaultKind::Let {
                cell_type: None,
                let_decl,
            } = &default.kind
        {
            let compiled_expr =
                compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics);
            let inferred_ty = compiled_expr.result_type.clone();
            if let Some(ignored_ty) = scope.register_if_absent(name, inferred_ty) {
                log_conflict(name, ignored_ty);
                // Scope slot already claimed by an annotated type (Pass 1).
                // Record in pass2_skipped so the injection loop skips Let-cell
                // injection for this name and avoids duplicate (entity, member) cells.
                pass2_skipped.insert(name.to_string());
            } else {
                inferred_let_exprs.insert(name.clone(), compiled_expr);
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
    //
    // Names in `pass2_skipped` are explicitly excluded from the Let arm (Option B,
    // task 1951): those are names where Pass 1 claimed the scope slot with an annotated
    // Param or Let, so the injection loop (lines ~520-527) already `continue`s past
    // them — no Let cell is ever injected for such a name. Advertising a phantom
    // `(name, Let)` entry would break the invariant "only one default satisfies a
    // given (name, kind)": a `RequirementKind::Let` lookup against the phantom entry
    // would kind-match and emit a spurious "requirement expects <T>, available default
    // has Real" type-mismatch instead of the clearer "missing required member" diagnostic.
    // Excluding pass2_skipped names makes the advertisement builder symmetric with the
    // injection loop.
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
                    // Do not advertise a phantom Let entry for names that Pass 2
                    // recorded in pass2_skipped: the injection loop will not emit
                    // a Let cell for those names, so advertising one here would
                    // violate the "one default per (name, kind)" invariant.
                    if pass2_skipped.contains(name) {
                        return None;
                    }
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
                    // branches: unannotated lets populate the cache unless Pass 2
                    // found the scope slot already claimed (recorded in `pass2_skipped`);
                    // annotated lets never use the cache.
                    //
                    // Cache miss handling: two reasons a `None` arm miss can occur:
                    //   (a) Deliberate skip (`pass2_skipped.contains(name)`): Pass 2
                    //       found an annotated type claiming the scope slot and did not
                    //       cache the expression.  Silent `continue` — the Param/
                    //       annotated-Let default will inject its own cell for this name.
                    //   (b) Unexpected drift: a refactor decoupled the pre-register
                    //       guard from the injection guard or changed the cache key.
                    //       `debug_assert!(false, …)` fires in dev/test; the error
                    //       diagnostic fires in release rather than silently recompiling
                    //       (which would risk duplicating diagnostics already pushed by
                    //       Pass 2 for the same AST node).
                    let compiled_expr = match cell_type {
                        Some(_) => {
                            compile_expr(&let_decl.value, scope, enum_defs, functions, diagnostics)
                        }
                        None => {
                            match inferred_let_exprs.remove(name) {
                                Some(ce) => ce,
                                None => {
                                    if pass2_skipped.contains(name) {
                                        // Deliberate skip: Pass 2 found an annotated
                                        // type already occupying the scope slot and
                                        // did not cache this expression (see `pass2_skipped`
                                        // above).  The Param/annotated-Let default will
                                        // inject its own cell; skip Let injection here
                                        // to prevent duplicate (entity, member) cells.
                                        continue;
                                    }
                                    // Unexpected: pre-register guard and injection guard
                                    // have diverged, or the cache key changed.
                                    debug_assert!(
                                        false,
                                        "unannotated let '{}' has no cached compiled expression \
                                         and is not in pass2_skipped — drift between the \
                                         pre-register guard and the injection guard in conformance.rs",
                                        name
                                    );
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

/// Verify that a compiled arg value's type conforms to the declared param type
/// in the target structure when the declared type is `Type::TraitObject(trait_name)`.
///
/// `arg_call_name` carries the callee name when the arg expression was any
/// `FunctionCall` (e.g. `Steel()` or `Steel(density: 1.0)` → `Some("Steel")`).
/// The expression compiler can default to `Type::Real` for unknown calls; if
/// `arg_call_name` is a known structure in the template registry we promote the
/// arg type to `StructureRef(name)` for the conformance check.
///
/// Conformance strategy (step-6 verified):
/// - `Type::StructureRef` args: uses `satisfies_trait_bound` to walk the structure's declared
///   trait bounds, following refinement chains transitively (e.g. `Rigid : Physical : Material`
///   satisfies a `Material` param).
/// - `Type::TraitObject` args: uses `trait_satisfies` to check equality-or-refinement between
///   the arg trait and the required trait.
///
/// Skips silently when:
/// - The target template is not found (external/unknown structure).
/// - The arg name is not found in the target's value cells (positional arg or error).
/// - The declared param type is not `Type::TraitObject` (no call-site type-check is performed in the compiler today for non-trait params).
/// - The arg_type is `Type::Error` (anti-cascade: treat as pass-through).
///
/// Emits at most one diagnostic per call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_arg_conformance(
    target_name: &str,
    arg_name: &str,
    arg_type: &Type,
    arg_call_name: Option<&str>,
    span: SourceSpan,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(arg_type, Type::Error) {
        return;
    }

    // Look up the target template — skip if not found (external/forward-ref miss).
    let Some(target) = template_registry.get(target_name) else {
        return;
    };

    // Find the declared param cell for this arg name.
    let Some(cell) = target.value_cells.iter().find(|vc| vc.id.member == arg_name) else {
        return; // Arg name not found — skip (positional arg or existing error).
    };

    // Only act when the param's declared type is a trait object.
    // TODO(follow-up): handle Option<TraitObject> and collection-typed trait params —
    // wrapping a trait type in Option or a collection currently bypasses call-site
    // conformance silently (known gap, not forgotten).
    let Type::TraitObject(required_trait) = &cell.cell_type else {
        return; // Non-trait param — no call-site type-check is performed in the compiler today.
    };

    // When the compiled arg_type defaulted to Type::Real from a FunctionCall
    // expression and the callee is a known structure template, promote to
    // StructureRef so the conformance check can walk the structure's trait bounds.
    let promoted: Option<Type> = if matches!(arg_type, Type::Real) {
        arg_call_name
            .filter(|call_name| template_registry.contains_key(*call_name))
            .map(|call_name| Type::StructureRef(call_name.to_string()))
    } else {
        None
    };
    let effective_arg_type = promoted.as_ref().unwrap_or(arg_type);

    // Check conformance based on effective_arg_type.
    match effective_arg_type {
        Type::StructureRef(struct_name) => {
            // Look up the arg's structure template and walk its trait bounds.
            let Some(arg_template) = template_registry.get(struct_name.as_str()) else {
                return; // Arg structure not compiled yet — skip.
            };
            if !satisfies_trait_bound(
                &arg_template.trait_bounds,
                required_trait,
                trait_registry,
            ) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        struct_name, required_trait, arg_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!(
                            "type '{}' does not conform to trait '{}'",
                            struct_name, required_trait
                        ),
                    )),
                );
            }
        }
        Type::TraitObject(arg_trait_name) => {
            // Trait-object arg: check that arg_trait refines (or equals) required_trait.
            let mut visited = HashSet::new();
            if !trait_satisfies(arg_trait_name, required_trait, trait_registry, &mut visited) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        arg_trait_name, required_trait, arg_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!(
                            "trait '{}' does not refine trait '{}'",
                            arg_trait_name, required_trait
                        ),
                    )),
                );
            }
        }
        _ => {
            // Anti-cascade: when arg_type is Type::Real and arg_call_name is Some
            // but the callee was not in the template registry (so promotion returned
            // None), an "undefined function" diagnostic already fired for that
            // unknown call.  Emitting "type 'real' does not conform to trait 'X'"
            // here would be misleading — Real is the expression compiler's fallback
            // for unresolved calls, not the author's intended type.  Suppress.
            if matches!(arg_type, Type::Real) && arg_call_name.is_some() {
                return;
            }
            // Neither StructureRef nor TraitObject — cannot conform to a trait.
            // The original arg_type is used in the message (not the effective type,
            // which equals arg_type here since promotion didn't apply).
            diagnostics.push(
                Diagnostic::error(format!(
                    "type '{}' does not conform to trait '{}' required by param '{}'",
                    arg_type, required_trait, arg_name
                ))
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("expected a type conforming to trait '{}'", required_trait),
                )),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit test for the Option B fix (task 1951).
    ///
    /// This test exercises the code path the integration-level
    /// `phantom_let_advertisement_contract_for_future_parser_extension` test in
    /// `trait_merge_tests.rs` CANNOT reach: it hand-builds a `RequirementKind::Let`
    /// requirement (not parseable from reify source today — see
    /// `let_type_disambiguation_tests.rs:470-497` and esc-1951-6) and verifies that
    /// the Option B guard in `available_defaults` suppresses the phantom
    /// `(name, Let) -> Type::Real` entry for names recorded in `pass2_skipped`.
    ///
    /// ## Scenario
    ///
    /// - **TraitX**: requires `let x : Length` (hand-built `RequirementKind::Let` — not
    ///   parser-reachable today)
    /// - **TraitY**: provides `param x : Length` — Pass 1 claims the scope slot for "x"
    /// - **TraitZ**: provides `let x = 5.5` (unannotated; `cell_type: None`) — Pass 2
    ///   sees the slot already claimed and records "x" in `pass2_skipped`
    /// - **Structure S : TraitX + TraitY + TraitZ { }** — no member override
    ///
    /// ## Expected behavior (post-fix)
    ///
    /// The `pass2_skipped.contains(name)` guard in the `DefaultKind::Let` arm of
    /// `available_defaults` returns `None` before reaching the `Type::Real` fallback.
    /// The `RequirementKind::Let` lookup for "x" finds no entry → the `None` arm fires →
    /// correct "missing required member" diagnostic (not the spurious "available default
    /// has Real" phantom type-mismatch).
    ///
    /// ## Pre-fix behavior (should NOT happen after fix)
    ///
    /// Without the guard, `available_defaults` contained `("x", Let) -> Type::Real`.
    /// The lookup found it, `implicitly_converts_to(Real, Length)` was false, and a
    /// spurious "requirement expects …, available default has Real" diagnostic was emitted.
    ///
    /// This test **FAILS on the unpatched code** and **PASSES after the Option B fix**.
    #[test]
    fn option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name() {
        // --- Build CompiledTrait fixtures ---

        // TraitX: requires `let x : Length` (hand-built — not parser-reachable)
        let trait_x = CompiledTrait {
            name: "TraitX".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "x".to_string(),
                kind: RequirementKind::Let(Type::length()),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitY: `param x : Length` — no default expression needed.
        // Pass 1 registers "x" → Type::length() in the scope.
        let trait_y = CompiledTrait {
            name: "TraitY".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Param {
                    cell_type: Type::length(),
                    default_decl: reify_syntax::ParamDecl {
                        name: "x".to_string(),
                        doc: None,
                        type_expr: None,
                        default: None, // no default expression
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // TraitZ: `let x = 5.5` (unannotated; cell_type: None).
        // Pass 2 compiles NumberLiteral(5.5) → Type::Real, finds "x" already in scope,
        // and records "x" in pass2_skipped (no inferred_let_exprs cache entry).
        let trait_z = CompiledTrait {
            name: "TraitZ".to_string(),
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some("x".to_string()),
                kind: DefaultKind::Let {
                    cell_type: None,
                    let_decl: reify_syntax::LetDecl {
                        name: "x".to_string(),
                        doc: None,
                        is_pub: false,
                        type_expr: None,
                        value: reify_syntax::Expr {
                            kind: reify_syntax::ExprKind::NumberLiteral(5.5),
                            span: SourceSpan::empty(0),
                        },
                        where_clause: None,
                        annotations: vec![],
                        span: SourceSpan::empty(0),
                        content_hash: ContentHash(0),
                    },
                },
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure S : TraitX + TraitY + TraitZ { } — no member overrides
        let structure_def = reify_syntax::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![
                reify_syntax::TraitBoundRef {
                    name: "TraitX".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitY".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
                reify_syntax::TraitBoundRef {
                    name: "TraitZ".to_string(),
                    type_args: vec![],
                    span: SourceSpan::empty(0),
                },
            ],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let entity_ref = EntityDefRef::from(&structure_def);

        let trait_registry: HashMap<String, &CompiledTrait> = [
            ("TraitX".to_string(), &trait_x),
            ("TraitY".to_string(), &trait_y),
            ("TraitZ".to_string(), &trait_z),
        ]
        .into_iter()
        .collect();

        let mut scope = CompilationScope::new("S");
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index = 0u32;
        let enum_defs: &[reify_types::EnumDef] = &[];
        let functions: &[CompiledFunction] = &[];
        let alias_registry = TypeAliasRegistry::new();
        let trait_names: HashSet<String> = trait_registry.keys().cloned().collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];

        check_trait_conformance(
            &entity_ref,
            &trait_registry,
            &trait_names,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            &alias_registry,
            &mut diagnostics,
        );

        // --- Assertion 1: no phantom type-mismatch diagnostic ---
        // Pre-fix: `available_defaults` had `("x", Let) -> Real`; the
        // RequirementKind::Let lookup found it, `implicitly_converts_to(Real, Length)` was
        // false, and a spurious "requirement expects …, available default has Real"
        // diagnostic was emitted.
        // Post-fix: no phantom entry → this filter collects nothing.
        let phantom_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("available default")
                    && d.message.contains("Real")
                    && d.message.contains('x')
            })
            .collect();
        assert!(
            phantom_diags.is_empty(),
            "Option B fix violated: phantom `(x, Let) -> Type::Real` advertisement caused \
             a spurious type-mismatch diagnostic. Expected no phantom diagnostic. Got: {:?}",
            phantom_diags
        );

        // --- Assertion 2: correct "missing required member" diagnostic IS present ---
        // With the phantom entry absent, the None arm of the available_defaults lookup
        // fires and emits the correct "missing required member" diagnostic.
        let missing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("missing required member") && d.message.contains("x")
            })
            .collect();
        assert_eq!(
            missing_diags.len(),
            1,
            "Expected exactly one 'missing required member' diagnostic for 'x' (Option B fix). \
             Got: {:?}",
            diagnostics
        );
    }
}
