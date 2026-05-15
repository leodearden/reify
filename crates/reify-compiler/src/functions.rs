use super::*;

pub(crate) fn compile_function(
    fn_def: &reify_syntax::FnDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    let empty_params = HashSet::new();
    // Resolve parameter types.
    //
    // `param_type_resolved[i]` is `true` when the i-th param's declared type resolved
    // successfully. It is used below to gate the default-type check: if the type failed
    // to resolve, the root-cause "unresolved type" diagnostic is already queued and
    // emitting a secondary FnParamDefaultTypeMismatch (against the `Type::Real` fallback)
    // would be confusing noise.
    let mut params: Vec<(String, Type)> = Vec::new();
    let mut param_type_resolved: Vec<bool> = Vec::new();
    for p in &fn_def.params {
        let (ty, resolved) = match resolve_type_expr_with_aliases(
            &p.type_expr,
            &empty_params,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        ) {
            Some(t) => (t, true),
            None => {
                diagnostics.push(
                    Diagnostic::error(format!("unresolved type: {}", p.type_expr))
                        .with_label(DiagnosticLabel::new(p.type_expr.span, "unknown type name")),
                );
                (Type::Real, false) // fallback; `resolved` flag prevents cascade in default check
            }
        };
        params.push((p.name.clone(), ty));
        param_type_resolved.push(resolved);
    }

    // Compile default expressions in a neutral scope (no params registered) so
    // defaults cannot reference sibling params — definition-time semantics.
    let neutral_scope = CompilationScope::new(&fn_def.name);
    let param_defaults: Vec<Option<CompiledExpr>> = fn_def
        .params
        .iter()
        .map(|p| {
            p.default
                .as_ref()
                .map(|d| compile_expr(d, &neutral_scope, enum_defs, functions, diagnostics))
        })
        .collect();

    // Type-check default expressions against their declared param types.
    //
    // Uses strict equality (via `fn_param_default_compatible`) matching the policy
    // in `resolve_function_overload` and `try_default_padding`'s prefix check —
    // a default value is conceptually inserted at the padded call site, so the
    // definition-site check must be at least as strict as the call-site check.
    //
    // The zip over three lockstep collections (fn_def.params, param_defaults, params)
    // makes index alignment structurally obvious: all three are built from the same
    // fn_def.params slice so they have identical length and ordering.
    //
    // The `type_ok` gate skips params whose declared type failed to resolve. The
    // root-cause "unresolved type" diagnostic is already queued; emitting a
    // secondary FnParamDefaultTypeMismatch (against the Type::Real fallback) would
    // be confusing noise — e.g. `fn f(x: Bogus = "hi")` would otherwise show both
    // "unresolved type: Bogus" AND "default type mismatch: Real vs String".
    for (((p, compiled_default), (_, param_ty)), &type_ok) in fn_def
        .params
        .iter()
        .zip(param_defaults.iter())
        .zip(params.iter())
        .zip(param_type_resolved.iter())
    {
        if !type_ok {
            continue;
        }
        // Match on both the compiled default and the syntactic default simultaneously.
        // `compiled_default.is_some() ↔ p.default.is_some()` (they are built in lockstep
        // in the param_defaults map above), so both arms are always in sync — no `.expect()`.
        if let (Some(default), Some(syntax_default)) = (compiled_default, &p.default)
            && !fn_param_default_compatible(param_ty, &default.result_type)
        {
            diagnostics.push(
                Diagnostic::error(format!(
                    "function '{}' param '{}' default type mismatch: declared param type `{}`, default expression produces `{}`",
                    fn_def.name, p.name, param_ty, default.result_type
                ))
                .with_code(DiagnosticCode::FnParamDefaultTypeMismatch)
                .with_label(DiagnosticLabel::new(
                    syntax_default.span,
                    "default expression type does not match declared param type",
                )),
            );
        }
    }

    // Resolve return type
    let return_type = match &fn_def.return_type {
        Some(te) => {
            match resolve_type_expr_with_aliases(
                te,
                &empty_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            ) {
                Some(t) => t,
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unresolved return type: {}", te))
                            .with_label(DiagnosticLabel::new(te.span, "unknown type name")),
                    );
                    Type::Real
                }
            }
        }
        None => Type::Real, // default return type
    };

    // Create a scope with function params registered
    let mut scope = CompilationScope::new(&fn_def.name);
    for (name, ty) in &params {
        scope.register(name, ty.clone());
    }

    // Compile body let bindings
    let mut compiled_lets = Vec::new();
    for let_decl in &fn_def.body.let_bindings {
        let compiled_expr =
            compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
        let let_type = compiled_expr.result_type.clone();
        // Register the let binding in scope for subsequent bindings
        scope.register(&let_decl.name, let_type);
        compiled_lets.push((let_decl.name.clone(), compiled_expr));
    }

    // Compile result expression
    let result_expr = compile_expr(
        &fn_def.body.result_expr,
        &scope,
        enum_defs,
        functions,
        diagnostics,
    );

    // Compute content hash — fold in default hashes so fn f(x:Real=1) ≠ fn f(x:Real=2).
    let content_hash = {
        let name_hash = ContentHash::of_str(&fn_def.name);
        let param_hashes = params
            .iter()
            .map(|(n, t)| ContentHash::of_str(n).combine(ContentHash::of_str(&format!("{}", t))));
        // Discriminate None and Some in disjoint hash subspaces so that absent and
        // present defaults never collide: None → tag 0x00, Some(e) → tag 0x01 ‖ e.hash.
        // Without the tag a Some(e) whose content_hash happened to equal of(&[0u8])
        // would be indistinguishable from None.
        let default_hashes = param_defaults.iter().map(|d| match d {
            Some(e) => ContentHash::of(&[1u8]).combine(e.content_hash),
            None => ContentHash::of(&[0u8]),
        });
        let body_hash = result_expr.content_hash;
        let let_hashes = compiled_lets.iter().map(|(_, e)| e.content_hash);

        let all_hashes = std::iter::once(name_hash)
            .chain(param_hashes)
            .chain(default_hashes)
            .chain(std::iter::once(body_hash))
            .chain(let_hashes);

        ContentHash::combine_all(all_hashes)
    };

    // Extract the optimized target before lowering — the extractor requires the
    // raw reify_syntax::ExprKind::StringLiteral trees, which are discarded by
    // lower_annotations. Same call shape as compile_constraint_def in defs_phase.rs.
    let opt_target = optimized_target(&fn_def.annotations);

    let annotations = lower_annotations(&fn_def.annotations, diagnostics);
    validate_annotations(&annotations, "function", diagnostics);

    Some(CompiledFunction {
        name: fn_def.name.clone(),
        is_pub: fn_def.is_pub,
        params,
        param_defaults,
        return_type,
        body: CompiledFnBody {
            let_bindings: compiled_lets,
            result_expr,
        },
        content_hash,
        annotations,
        optimized_target: opt_target,
    })
}

/// Resolve a type name in field context. Unlike resolve_type_name, unresolved
/// names become StructureRef (geometric domain types like Point3, Vector3)
/// but a diagnostic warning is emitted so the user knows the type was not
/// resolved from the built-in set.
pub(crate) fn resolve_field_type_name(
    name: &str,
    span: reify_types::SourceSpan,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    let empty_params = HashSet::new();
    // Field types do not currently resolve trait or structure names into
    // TraitObject/StructureRef via the unified resolver path; pass empty sets
    // so behavior is unchanged for fields.
    let empty_structs: HashSet<String> = HashSet::new();
    let empty_traits: HashSet<String> = HashSet::new();
    resolve_type_with_aliases(
        name,
        &empty_params,
        alias_registry,
        &empty_structs,
        &empty_traits,
    )
    .unwrap_or_else(|| {
        diagnostics.push(
            Diagnostic::warning(format!(
                "unresolved field type '{}', treating as structure reference",
                name
            ))
            .with_label(DiagnosticLabel::new(span, "unknown type name")),
        );
        Type::StructureRef(name.to_string())
    })
}

/// Check whether `body_ty` is compatible with the declared `codomain_ty` as an
/// analytical field codomain, incorporating the Int→Real widening coercion.
///
/// `implicitly_converts_to` is intentionally direction-sensitive and does NOT
/// include Int→Real widening (that rule lives in `type_compatible`, which is
/// symmetric by design). Field codomain checks are directional (body → declared),
/// but whole-number float literals are typed as `Int` by the expression compiler,
/// so we must also accept `Int` where `Real` is declared. Encoding this in a
/// dedicated predicate avoids repeating the widening rule at each call site —
/// a future change to widening semantics (e.g. `Int→Scalar[dimensionless]`) needs
/// updating only here.
fn field_codomain_compatible(body_ty: &Type, codomain_ty: &Type) -> bool {
    implicitly_converts_to(body_ty, codomain_ty)
        || matches!((body_ty, codomain_ty), (Type::Int, Type::Real))
}

/// Compile a field declaration into a CompiledField.
pub(crate) fn compile_field(
    field_def: &reify_syntax::FieldDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledField {
    // Resolve domain and codomain types. DimensionalOp cannot appear as a field type —
    // emit exactly one diagnostic and fall back to Type::Real without forwarding a
    // sentinel "<unknown>" string to resolve_field_type_name (which would push a second
    // confusing diagnostic for the placeholder name).
    let domain_type = match &field_def.domain_type.kind {
        reify_syntax::TypeExprKind::Named { name, .. } => resolve_field_type_name(
            name.as_str(),
            field_def.domain_type.span,
            alias_registry,
            diagnostics,
        ),
        reify_syntax::TypeExprKind::DimensionalOp { .. } => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "unexpected dimensional expression",
                    )),
            );
            Type::Real
        }
        reify_syntax::TypeExprKind::IntegerLiteral(_) => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "integer literal not allowed in this position",
                    )),
            );
            Type::Real
        }
    };
    let codomain_type = match &field_def.codomain_type.kind {
        reify_syntax::TypeExprKind::Named { name, .. } => resolve_field_type_name(
            name.as_str(),
            field_def.codomain_type.span,
            alias_registry,
            diagnostics,
        ),
        reify_syntax::TypeExprKind::DimensionalOp { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "unexpected dimensional expression",
                )),
            );
            Type::Real
        }
        reify_syntax::TypeExprKind::IntegerLiteral(_) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "integer literal not allowed in this position",
                )),
            );
            Type::Real
        }
    };

    // Create a scope for compiling field source expressions
    let scope = CompilationScope::new(&field_def.name);

    let source = match &field_def.source {
        reify_syntax::FieldSource::Analytical { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            // Codomain type-check: the lambda body's inferred type must implicitly
            // convert to the declared codomain. Skip the check when either type is
            // already poisoned (anti-cascade — task-1918).
            //
            // Int→Real widening is handled by `field_codomain_compatible` so that
            // the rule is encoded in exactly one place.
            //
            // The analytical source always compiles to a Lambda. If the result is not
            // a Lambda, the expression compiler encountered an internal error and set
            // `result_type` to `Type::Error`; the debug_assert below catches any
            // regression where a non-Error, non-Lambda escapes.
            debug_assert!(
                matches!(
                    compiled_expr.kind,
                    reify_types::CompiledExprKind::Lambda { .. }
                ) || compiled_expr.result_type.is_error(),
                "analytical field source compiled to non-Lambda with non-Error result type — \
                 this indicates a compiler bug"
            );
            if let reify_types::CompiledExprKind::Lambda { body, .. } = &compiled_expr.kind {
                let body_ty = &body.result_type;
                if !body_ty.is_error()
                    && !codomain_type.is_error()
                    && !field_codomain_compatible(body_ty, &codomain_type)
                {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "field '{}' codomain mismatch: declared codomain `{}`, \
                             lambda body produces `{}`",
                            field_def.name, codomain_type, body_ty
                        ))
                        .with_code(DiagnosticCode::FieldCodomainMismatch)
                        .with_label(DiagnosticLabel::new(
                            field_def.codomain_type.span,
                            "declared codomain",
                        )),
                    );
                }
            }
            CompiledFieldSource::Analytical {
                expr: compiled_expr,
            }
        }
        reify_syntax::FieldSource::Sampled { config } => {
            // v0.2 (task 2341): walk the AST config entries and compile each value
            // expression. Runtime parsing of the resulting Values into a
            // `SampledField` is performed in `engine_eval::elaborate_field`; this
            // arm validates the shape (required keys + allowed keys + no
            // duplicates) and forwards the compiled expressions.
            //
            // Validation rules:
            //   - Accepted keys: `grid`, `bounds`, `spacing`, `interpolation`,
            //     `data`. All five are required — a missing required key
            //     produces one error per missing key, attached to the field
            //     declaration's span.
            //   - Unknown keys produce a hard error; the entry is dropped.
            //   - Duplicate keys (e.g. two `grid = ...` entries) produce a hard
            //     error; only the first occurrence is kept in the compiled
            //     config so engine_eval sees a deterministic shape.
            //
            // Design rationale (esc-2341-149, 2026-04-29 steward): the
            // originally-locked plan assumed users could write
            // `grid = RegularGrid1 { spacing = …, bounds = … }` struct-literal
            // syntax to bundle the kind tag with bounds/spacing, but Reify has
            // no anonymous struct-literal expression form and no
            // `RegularGrid*` constructor in stdlib. Resolution: surface
            // `grid`/`bounds`/`spacing` as separate top-level keys. This
            // mirrors the imported-field key=value walker pattern landed
            // earlier today (commit 06a537e36c), and keeps `grid` as an
            // explicit kind tag for diagnostic clarity.
            //
            // Error ordering matches the typical compile-time-error pattern in
            // this module: per-entry errors (unknown / duplicate) are emitted
            // as the entries are walked, and then missing-key errors are
            // emitted in a fixed order (grid, bounds, spacing, interpolation,
            // data) after the walk so that diagnostics referencing the same
            // source span are grouped together.
            const REQUIRED_KEYS: [&str; 5] = ["grid", "bounds", "spacing", "interpolation", "data"];
            let mut compiled_config: Vec<(String, reify_types::CompiledExpr)> = Vec::new();
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for (key, expr) in config {
                let key_str = key.as_str();
                let is_known = REQUIRED_KEYS.contains(&key_str);
                if !is_known {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unknown sampled-field config key: '{}'; expected grid, bounds, spacing, interpolation, or data",
                            key
                        ))
                        .with_label(DiagnosticLabel::new(
                            expr.span,
                            "unknown sampled config key",
                        )),
                    );
                    // Drop unknown-keyed entries; do not call compile_expr so
                    // unrelated unresolved-name diagnostics from the value don't
                    // cascade after the canonical "unknown key" error.
                    continue;
                }
                if !seen.insert(key_str) {
                    diagnostics.push(
                        Diagnostic::error(format!("duplicate sampled-field config key: '{}'", key))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "duplicate sampled config key",
                            )),
                    );
                    // Drop the duplicate; the first-seen entry is kept.
                    continue;
                }
                let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
                compiled_config.push((key.clone(), compiled_expr));
            }
            // Emit one error per missing required key, in declaration order
            // (grid, bounds, spacing, interpolation, data). The label points
            // at the field def span since there is no per-entry span for a
            // missing entry.
            for required in REQUIRED_KEYS {
                if !seen.contains(required) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "sampled field source is missing required key: '{}'",
                            required
                        ))
                        .with_label(DiagnosticLabel::new(
                            field_def.span,
                            "missing required sampled config key",
                        )),
                    );
                }
            }
            CompiledFieldSource::Sampled {
                config: compiled_config,
            }
        }
        reify_syntax::FieldSource::Composed { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            CompiledFieldSource::Composed {
                expr: compiled_expr,
            }
        }
        reify_syntax::FieldSource::Imported { .. } => {
            diagnostics.push(
                Diagnostic::error(
                    "imported field sources are deferred to v0.2; v0.1 supports analytical and composed only",
                )
                .with_code(DiagnosticCode::FieldImportedV02)
                .with_label(DiagnosticLabel::new(
                    field_def.span,
                    "imported field source is deferred to v0.2",
                )),
            );
            CompiledFieldSource::Imported
        }
    };

    // Compute content hash
    let content_hash = {
        let name_hash = ContentHash::of_str(&field_def.name);
        let domain_hash = ContentHash::of_str(&format!("{}", domain_type));
        let codomain_hash = ContentHash::of_str(&format!("{}", codomain_type));
        let source_hash = match &source {
            CompiledFieldSource::Analytical { expr } => expr.content_hash,
            // Iteration preserved for non-compiler construction paths:
            // `CompiledFieldBuilder::sampled` in reify-test-support may construct
            // Sampled directly with a non-empty config.  compile_field always emits
            // an empty Vec, so this reduces to ContentHash::combine_all(empty) == ContentHash(0).
            CompiledFieldSource::Sampled { config } => {
                let hashes = config
                    .iter()
                    .map(|(k, e)| ContentHash::of_str(k).combine(e.content_hash));
                ContentHash::combine_all(hashes)
            }
            CompiledFieldSource::Composed { expr } => expr.content_hash,
            CompiledFieldSource::Imported => ContentHash::of(&[0u8]),
        };
        ContentHash::combine_all([name_hash, domain_hash, codomain_hash, source_hash])
    };

    let annotations = lower_annotations(&field_def.annotations, diagnostics);
    validate_annotations(&annotations, "field", diagnostics);

    CompiledField {
        name: field_def.name.clone(),
        is_pub: field_def.is_pub,
        domain_type,
        codomain_type,
        source,
        content_hash,
        annotations,
    }
}

/// Collect the set of field cell IDs (`__field.<name>`) referenced by a
/// composed field's compiled expression.
///
/// Walks `expr` via `CompiledExpr::walk` (the canonical exhaustive traversal
/// in reify-types/src/expr.rs:298), and for every `FunctionCall` whose
/// `function.name` matches a key in `field_registry`, emits
/// `ValueCellId::new(FIELD_ENTITY_PREFIX, name)`. Results are deduplicated
/// via an interim `HashSet`, then returned as a `Vec` in arbitrary order.
///
/// Self-references (a composed field calling its own name) are NOT filtered
/// here; the caller in `phase_augment_composed_captures` excludes the outer
/// field from the registry it passes in, so this helper never sees a
/// self-referential FunctionCall.
///
/// Used by `phase_augment_composed_captures` (post-pass) to seed each
/// composed lambda's `captures` Vec with the field cell IDs it transitively
/// reads — so that `extract_dependency_trace` surfaces field-to-field deps
/// via the existing `Lambda { captures, .. }` arm of `collect_value_refs_inner`.
pub(crate) fn collect_composed_field_dependencies(
    expr: &CompiledExpr,
    field_registry: &HashMap<&str, &CompiledField>,
) -> Vec<ValueCellId> {
    let mut seen: HashSet<ValueCellId> = HashSet::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::FunctionCall { function, .. } = &node.kind
            && field_registry.contains_key(function.name.as_str())
        {
            seen.insert(ValueCellId::new(FIELD_ENTITY_PREFIX, &function.name));
        }
    });
    seen.into_iter().collect()
}

/// Check field composition types in a composed field expression.
///
/// Uses `CompiledExpr::walk` to traverse all 12+ expression variants,
/// looking for nested field calls like `f2(f1(p))`. For each such nesting,
/// verifies that the inner field's codomain matches the outer field's domain.
pub(crate) fn check_field_composition_types(
    expr: &CompiledExpr,
    field_registry: &HashMap<&str, &CompiledField>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut errors = Vec::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::FunctionCall { function, args } = &node.kind {
            // If this function call references a known field
            if let Some(outer_field) = field_registry.get(function.name.as_str()) {
                // Check if any argument is also a field call
                for arg in args {
                    if let CompiledExprKind::FunctionCall { function: inner_fn, .. } = &arg.kind
                        && let Some(inner_field) = field_registry.get(inner_fn.name.as_str())
                    {
                        // inner_field's codomain should implicitly convert to outer_field's domain
                        if !implicitly_converts_to(&inner_field.codomain_type, &outer_field.domain_type) {
                            errors.push(
                                Diagnostic::error(format!(
                                    "field composition type mismatch: codomain of '{}' ({}) does not match domain of '{}' ({})",
                                    inner_field.name, inner_field.codomain_type,
                                    outer_field.name, outer_field.domain_type
                                )),
                            );
                        }
                    }
                }
            }
        }
    });
    diagnostics.extend(errors);
}

#[cfg(test)]
mod tests {
    //! Unit tests for `check_field_composition_types` wiring direction.
    //!
    //! `check_field_composition_types` is `pub(crate)` so these tests must live
    //! inside the crate. They pin the producer→consumer direction (inner.codomain
    //! as FROM, outer.domain as TO) that a future refactor could silently reverse.
    //!
    //! Covers suggestion #16 (field-composition portion) from task 231.
    use super::*;

    /// Build a minimal `CompiledField` for testing.
    /// Only `name`, `domain_type`, and `codomain_type` are semantically relevant
    /// to `check_field_composition_types`; `source` is always `Imported`.
    fn make_field(name: &str, domain_type: Type, codomain_type: Type) -> CompiledField {
        CompiledField {
            name: name.to_string(),
            is_pub: false,
            domain_type,
            codomain_type,
            source: CompiledFieldSource::Imported,
            content_hash: ContentHash(0),
            annotations: vec![],
        }
    }

    /// Build a composed expression representing `outer_name(inner_name(dummy_literal))`.
    ///
    /// The dummy literal is typed `Real` to match the `domain_type` of the inner
    /// field (`Type::Real`) in all current test cases. `check_field_composition_types`
    /// only validates inter-function wiring (inner.codomain → outer.domain) and does
    /// not check argument types against the inner field's domain, so the dummy type
    /// currently has no effect on test outcomes. It is kept consistent with the inner
    /// domain to avoid spurious failures if argument-type checking is added later.
    fn make_composition_expr(outer_name: &str, inner_name: &str) -> CompiledExpr {
        let dummy = CompiledExpr::literal(Value::Real(0.0), Type::Real);
        let inner_call = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: inner_name.to_string(),
                    qualified_name: inner_name.to_string(),
                },
                args: vec![dummy],
            },
            result_type: Type::Real,
            content_hash: ContentHash(0),
        };
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: outer_name.to_string(),
                    qualified_name: outer_name.to_string(),
                },
                args: vec![inner_call],
            },
            result_type: Type::Real,
            content_hash: ContentHash(0),
        }
    }

    /// inner codomain = Vector<3,Real>, outer domain = Tensor<1,3,Real>.
    /// Rule 1a applies (Vector<N,Q> → Tensor<1,N,Q>): zero diagnostics.
    /// Pins the producer→consumer wiring: inner.codomain is checked as FROM,
    /// outer.domain as TO.
    #[test]
    fn field_composition_allows_vector_to_tensor1() {
        let inner = make_field("inner", Type::Real, Type::vec3(Type::Real));
        let outer = make_field("outer", Type::tensor(1, 3, Type::Real), Type::Real);
        let expr = make_composition_expr("outer", "inner");
        let mut registry = HashMap::new();
        registry.insert("inner", &inner);
        registry.insert("outer", &outer);
        let mut diagnostics = Vec::new();
        check_field_composition_types(&expr, &registry, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "Vector<3,Real>→Tensor<1,3,Real> composition should produce zero diagnostics (Rule 1a)"
        );
    }

    /// inner codomain = Matrix<3,3,Real>, outer domain = Tensor<2,3,Real>.
    /// Rule 3 is one-way (Tensor<2>→Matrix, NOT Matrix→Tensor<2>): one diagnostic.
    #[test]
    fn field_composition_rejects_matrix_to_tensor2() {
        let inner = make_field("inner", Type::Real, Type::matrix(3, 3, Type::Real));
        let outer = make_field("outer", Type::tensor(2, 3, Type::Real), Type::Real);
        let expr = make_composition_expr("outer", "inner");
        let mut registry = HashMap::new();
        registry.insert("inner", &inner);
        registry.insert("outer", &outer);
        let mut diagnostics = Vec::new();
        check_field_composition_types(&expr, &registry, &mut diagnostics);
        assert_eq!(
            diagnostics.len(),
            1,
            "Matrix<3,3,Real>→Tensor<2,3,Real> should produce one diagnostic (Rule 3 is one-way)"
        );
        assert!(
            diagnostics[0].message.contains("codomain of 'inner'"),
            "Expected \"codomain of 'inner'\" (producer wiring) in diagnostic; got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("domain of 'outer'"),
            "Expected \"domain of 'outer'\" (consumer wiring) in diagnostic; got: {}",
            diagnostics[0].message
        );
    }

    /// inner codomain = Tensor<2,3,Real>, outer domain = Matrix<3,3,Real>.
    /// Rule 3 applies (Tensor<2,N,Q> → Matrix<N,N,Q>): zero diagnostics.
    #[test]
    fn field_composition_allows_tensor2_to_matrix() {
        let inner = make_field("inner", Type::Real, Type::tensor(2, 3, Type::Real));
        let outer = make_field("outer", Type::matrix(3, 3, Type::Real), Type::Real);
        let expr = make_composition_expr("outer", "inner");
        let mut registry = HashMap::new();
        registry.insert("inner", &inner);
        registry.insert("outer", &outer);
        let mut diagnostics = Vec::new();
        check_field_composition_types(&expr, &registry, &mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "Tensor<2,3,Real>→Matrix<3,3,Real> composition should produce zero diagnostics (Rule 3)"
        );
    }

    // ── Task 2343 step-1: collect_composed_field_dependencies extracts ────────
    //   field-name FunctionCall references from a composed lambda body.
    //
    // Pins the contract used by `phase_augment_composed_captures` to seed the
    // composed lambda's `captures` Vec with the field cell IDs it transitively
    // reads — so that `extract_dependency_trace(composed_expr)` surfaces those
    // deps via the existing `Lambda { captures, .. }` arm of
    // `collect_value_refs_inner` in reify-types/src/expr.rs.

    /// Synthetic composed-style expr `outer(inner(dummy))` and a registry
    /// containing both `inner` and `outer` as fields: helper returns both
    /// their `__field.<name>` cell IDs (deduplicated, order-independent).
    #[test]
    fn collect_composed_field_dependencies_finds_both_field_refs() {
        let inner = make_field("inner", Type::Real, Type::Real);
        let outer = make_field("outer", Type::Real, Type::Real);
        let expr = make_composition_expr("outer", "inner");
        let mut registry: HashMap<&str, &CompiledField> = HashMap::new();
        registry.insert("inner", &inner);
        registry.insert("outer", &outer);

        let deps = collect_composed_field_dependencies(&expr, &registry);

        let inner_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "inner");
        let outer_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "outer");
        assert_eq!(
            deps.len(),
            2,
            "expected exactly 2 field deps (inner, outer), got: {:?}",
            deps
        );
        assert!(
            deps.contains(&inner_id),
            "deps should contain __field.inner, got: {:?}",
            deps
        );
        assert!(
            deps.contains(&outer_id),
            "deps should contain __field.outer, got: {:?}",
            deps
        );
    }

    /// Repeated FunctionCall to the same registered field deduplicates to a
    /// single entry. Pins the HashSet-based dedup contract.
    #[test]
    fn collect_composed_field_dependencies_deduplicates_repeated_refs() {
        // Build `outer(outer(dummy))` — a self-nested call with the same
        // outer name appearing twice. Even when the inner call resolves to
        // the same field, the helper emits a single dep entry.
        let outer = make_field("outer", Type::Real, Type::Real);
        let expr = make_composition_expr("outer", "outer");
        let mut registry: HashMap<&str, &CompiledField> = HashMap::new();
        registry.insert("outer", &outer);

        let deps = collect_composed_field_dependencies(&expr, &registry);

        let outer_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "outer");
        assert_eq!(
            deps.len(),
            1,
            "duplicate FunctionCall(outer) refs should dedupe to 1, got: {:?}",
            deps
        );
        assert!(
            deps.contains(&outer_id),
            "deps should contain __field.outer, got: {:?}",
            deps
        );
    }

    /// FunctionCall whose name is NOT in the registry produces no dep.
    /// Distinguishes field-call references from ordinary stdlib/user-fn calls.
    #[test]
    fn collect_composed_field_dependencies_ignores_non_field_calls() {
        let expr = make_composition_expr("sin", "cos"); // neither is a field
        let registry: HashMap<&str, &CompiledField> = HashMap::new();
        let deps = collect_composed_field_dependencies(&expr, &registry);
        assert!(
            deps.is_empty(),
            "non-field FunctionCalls should produce no deps, got: {:?}",
            deps
        );
    }

    /// Lambda-rooted variant of the basic dep-discovery test. Production
    /// callers always pass a `composed { |p| ... }` lambda — the bare
    /// FunctionCall used by the other unit tests doesn't exercise the
    /// `expr.walk` Lambda-body recursion path. Without this test, a future
    /// refactor that stopped descending into Lambda bodies in `walk` would
    /// silently regress field-dep collection but leave the unit tests green
    /// (only the integration test in `field_compile_tests.rs` would fail).
    #[test]
    fn collect_composed_field_dependencies_walks_lambda_body() {
        let inner = make_field("inner", Type::Real, Type::Real);
        let outer = make_field("outer", Type::Real, Type::Real);
        let body = make_composition_expr("outer", "inner");
        let lambda_expr = CompiledExpr {
            kind: CompiledExprKind::Lambda {
                params: vec![("p".to_string(), Some(Type::Real))],
                param_ids: vec![ValueCellId::new("$lambda0", "p")],
                body: Box::new(body),
                captures: vec![],
            },
            result_type: Type::Real,
            content_hash: ContentHash(0),
        };
        let mut registry: HashMap<&str, &CompiledField> = HashMap::new();
        registry.insert("inner", &inner);
        registry.insert("outer", &outer);

        let deps = collect_composed_field_dependencies(&lambda_expr, &registry);

        assert_eq!(
            deps.len(),
            2,
            "Lambda-rooted expr: expected 2 field deps via body recursion, got: {:?}",
            deps
        );
        assert!(deps.contains(&ValueCellId::new(FIELD_ENTITY_PREFIX, "inner")));
        assert!(deps.contains(&ValueCellId::new(FIELD_ENTITY_PREFIX, "outer")));
    }
}
