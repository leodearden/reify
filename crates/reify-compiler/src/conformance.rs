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
    let mut all_requirements: Vec<TraitRequirement> = Vec::new();
    let mut all_defaults: Vec<TraitDefault> = Vec::new();
    let mut visited_traits: HashSet<String> = HashSet::new();
    // Maps name → (type/hash, originating trait name) so conflict diagnostics can name
    // both traits instead of just saying "conflicting traits".
    let mut seen_requirement_names: HashMap<String, (Type, String)> = HashMap::new();
    let mut seen_default_names: HashMap<String, (Type, String)> = HashMap::new();
    let mut seen_let_hashes: HashMap<String, (ContentHash, String)> = HashMap::new();

    for trait_bound in structure.trait_bounds {
        collect_all_requirements(
            &trait_bound.name,
            trait_registry,
            &mut all_requirements,
            &mut all_defaults,
            &mut visited_traits,
            &mut seen_requirement_names,
            &mut seen_default_names,
            &mut seen_let_hashes,
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
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum AvailableDefaultKind {
        Param,
        Let,
    }

    // Build a map of available default names from all_defaults (non-constraint, named).
    // Used to cross-check requirements: a requirement is satisfied if the structure
    // provides the member OR if another trait in the bound set provides a matching default
    // of the SAME kind. Kind mismatches are ignored (treated as absent).
    let available_defaults: HashMap<String, (AvailableDefaultKind, Type)> = all_defaults
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
            Some((name.to_string(), (kind, ty)))
        })
        .collect();

    // Check each requirement against structure members.
    for req in &all_requirements {
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
                        match available_defaults.get(&req.name) {
                            Some((default_kind, default_type))
                                if *default_kind == required_default_kind
                                    && implicitly_converts_to(default_type, expected_type) =>
                            {
                                // Same-kind default with matching type satisfies the requirement.
                            }
                            Some((default_kind, default_type))
                                if *default_kind == required_default_kind =>
                            {
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
                            _ => {
                                // No default, or a default of the wrong kind — treat as missing.
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
    for default in &all_defaults {
        if let Some(name) = &default.name
            && !structure_members.contains_key(name)
        {
            let ty = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let(_) => Type::Real,
                DefaultKind::Constraint(_) => continue,
            };
            scope.register(name, ty);
        }
    }

    // Inject defaults for members not overridden by the structure.
    for default in &all_defaults {
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
                    });
                }
            }
        }
    }
}

/// Recursively collect all requirements and defaults from a trait and its refinements.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_all_requirements(
    trait_name: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    requirements: &mut Vec<TraitRequirement>,
    defaults: &mut Vec<TraitDefault>,
    visited: &mut HashSet<String>,
    // Maps member name → (type, originating trait name) for conflict reporting.
    seen_names: &mut HashMap<String, (Type, String)>,
    seen_defaults: &mut HashMap<String, (Type, String)>,
    seen_let_hashes: &mut HashMap<String, (ContentHash, String)>,
    structure_members: &HashMap<String, Type>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !visited.insert(trait_name.to_string()) {
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
            requirements,
            defaults,
            visited,
            seen_names,
            seen_defaults,
            seen_let_hashes,
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
            if let Some((existing_type, existing_trait)) = seen_names.get(&req.name) {
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
            seen_names.insert(
                req.name.clone(),
                (expected_type.clone(), trait_name.to_string()),
            );
        }

        requirements.push(req.clone());
    }

    // Collect defaults from this trait, deduplicating by name.
    for default in &compiled_trait.defaults {
        if default.name.is_none() {
            // Unnamed defaults (e.g., unlabeled constraints) — always push.
            defaults.push(default.clone());
        } else if let Some(name) = &default.name {
            // For let bindings: use content_hash comparison to distinguish same
            // expression (dedup) vs different expression (conflict).
            if let DefaultKind::Let(let_decl) = &default.kind {
                if let Some((existing_hash, existing_trait)) = seen_let_hashes.get(name.as_str()) {
                    if existing_hash != &let_decl.content_hash
                        && !structure_members.contains_key(name.as_str())
                    {
                        // Same name, different expression, not overridden → conflict.
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
                seen_let_hashes.insert(
                    name.clone(),
                    (let_decl.content_hash, trait_name.to_string()),
                );
                // Fall through to insert into seen_defaults and push.
            }

            // Extract type for dedup comparison (non-Let defaults).
            let default_type = match &default.kind {
                DefaultKind::Param { cell_type, .. } => cell_type.clone(),
                DefaultKind::Let(_) => Type::Real,
                DefaultKind::Constraint(_) => Type::Bool, // sentinel for constraint label dedup
            };

            if let Some((existing_type, existing_trait)) = seen_defaults.get(name.as_str()) {
                if existing_type != &default_type && !structure_members.contains_key(name.as_str())
                {
                    // Same name + different type + not overridden → conflict
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
                // Same name already seen → skip (deduplicate).
                continue;
            }
            seen_defaults.insert(name.clone(), (default_type, trait_name.to_string()));
            defaults.push(default.clone());
        }
    }
}

