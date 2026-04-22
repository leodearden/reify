use super::*;

pub(crate) fn compile_function(
    fn_def: &reify_syntax::FnDef,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    let empty_params = HashSet::new();
    // Functions are compiled before the trait registry is built, so trait-name
    // resolution is not applied inside function signatures. Pass an empty set.
    let empty_traits: HashSet<String> = HashSet::new();
    // Resolve parameter types
    let mut params = Vec::new();
    for p in &fn_def.params {
        let ty = match resolve_type_expr_with_aliases(
            &p.type_expr,
            &empty_params,
            alias_registry,
            diagnostics,
            &empty_traits,
        ) {
            Some(t) => t,
            None => {
                diagnostics.push(
                    Diagnostic::error(format!("unresolved type: {}", p.type_expr))
                        .with_label(DiagnosticLabel::new(p.type_expr.span, "unknown type name")),
                );
                Type::Real // fallback
            }
        };
        params.push((p.name.clone(), ty));
    }

    // Resolve return type
    let return_type = match &fn_def.return_type {
        Some(te) => {
            match resolve_type_expr_with_aliases(
                te,
                &empty_params,
                alias_registry,
                diagnostics,
                &empty_traits,
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

    // Compute content hash
    let content_hash = {
        let name_hash = ContentHash::of_str(&fn_def.name);
        let param_hashes = params
            .iter()
            .map(|(n, t)| ContentHash::of_str(n).combine(ContentHash::of_str(&format!("{}", t))));
        let body_hash = result_expr.content_hash;
        let let_hashes = compiled_lets.iter().map(|(_, e)| e.content_hash);

        let all_hashes = std::iter::once(name_hash)
            .chain(param_hashes)
            .chain(std::iter::once(body_hash))
            .chain(let_hashes);

        ContentHash::combine_all(all_hashes)
    };

    let annotations = lower_annotations(&fn_def.annotations, diagnostics);
    validate_annotations(&annotations, "function", diagnostics);

    Some(CompiledFunction {
        name: fn_def.name.clone(),
        is_pub: fn_def.is_pub,
        params,
        return_type,
        body: CompiledFnBody {
            let_bindings: compiled_lets,
            result_expr,
        },
        content_hash,
        annotations,
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
    // Field types do not currently resolve trait names into TraitObject; pass
    // an empty trait-name set so behavior is unchanged for fields.
    let empty_traits: HashSet<String> = HashSet::new();
    resolve_type_with_aliases(name, &empty_params, alias_registry, &empty_traits).unwrap_or_else(
        || {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "unresolved field type '{}', treating as structure reference",
                    name
                ))
                .with_label(DiagnosticLabel::new(span, "unknown type name")),
            );
            Type::StructureRef(name.to_string())
        },
    )
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
    };

    // Create a scope for compiling field source expressions
    let scope = CompilationScope::new(&field_def.name);

    let source = match &field_def.source {
        reify_syntax::FieldSource::Analytical { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            CompiledFieldSource::Analytical {
                expr: compiled_expr,
            }
        }
        reify_syntax::FieldSource::Sampled { config } => {
            let compiled_config: Vec<(String, CompiledExpr)> = config
                .iter()
                .map(|(key, val_expr)| {
                    // In sampled config, bare identifiers are treated as string
                    // constants (e.g., `interpolation = linear` -> "linear").
                    let compiled = if let reify_syntax::ExprKind::Ident(name) = &val_expr.kind {
                        if scope.resolve(name).is_none() {
                            CompiledExpr::literal(Value::String(name.clone()), Type::String)
                        } else {
                            compile_expr(val_expr, &scope, enum_defs, functions, diagnostics)
                        }
                    } else {
                        compile_expr(val_expr, &scope, enum_defs, functions, diagnostics)
                    };
                    (key.clone(), compiled)
                })
                .collect();
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
        reify_syntax::FieldSource::Imported { .. } => CompiledFieldSource::Imported,
    };

    // Compute content hash
    let content_hash = {
        let name_hash = ContentHash::of_str(&field_def.name);
        let domain_hash = ContentHash::of_str(&format!("{}", domain_type));
        let codomain_hash = ContentHash::of_str(&format!("{}", codomain_type));
        let source_hash = match &source {
            CompiledFieldSource::Analytical { expr } => expr.content_hash,
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
}
