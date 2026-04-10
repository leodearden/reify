use super::*;

/// Internal type alias entry — stored in the registry during compilation.
///
/// For non-parameterized aliases, `resolved_type` holds the fully-resolved `Type`.
/// For parameterized aliases, `type_params` is non-empty and `type_expr` holds the
/// original `TypeExpr` for deferred substitution at each use site.
#[derive(Debug, Clone)]
pub(crate) struct TypeAliasEntry {
    pub(crate) name: String,
    /// The resolved type for non-parameterized aliases; `None` for parameterized aliases
    /// (which require instantiation with concrete type arguments).
    pub(crate) resolved_type: Option<Type>,
    /// Type parameters for parameterized aliases (empty for simple aliases).
    pub(crate) type_params: Vec<reify_types::TypeParam>,
    /// The original type expression, stored for parameterized alias substitution.
    pub(crate) type_expr: Option<reify_syntax::TypeExpr>,
    pub(crate) is_pub: bool,
    pub(crate) span: SourceSpan,
    pub(crate) content_hash: ContentHash,
}

impl TypeAliasEntry {
    /// Convert to the public `CompiledTypeAlias` representation (no `type_expr`).
    pub(crate) fn into_compiled(self) -> CompiledTypeAlias {
        CompiledTypeAlias {
            name: self.name,
            resolved_type: self.resolved_type,
            type_params: self.type_params,
            is_pub: self.is_pub,
            span: self.span,
            content_hash: self.content_hash,
        }
    }
}

/// Registry mapping type alias names to compiled alias entries.
/// Built during the pre-pass so type resolution can check aliases.
pub(crate) struct TypeAliasRegistry {
    entries: HashMap<String, TypeAliasEntry>,
}

impl TypeAliasRegistry {
    /// Create an empty registry.
    pub(crate) fn new() -> Self {
        TypeAliasRegistry {
            entries: HashMap::new(),
        }
    }

    /// Register a type alias entry. Returns `Err(entry)` if the name is already registered.
    pub(crate) fn register(&mut self, entry: TypeAliasEntry) -> Result<(), Box<TypeAliasEntry>> {
        if self.entries.contains_key(&entry.name) {
            Err(Box::new(entry))
        } else {
            self.entries.insert(entry.name.clone(), entry);
            Ok(())
        }
    }

    /// Look up a type alias by name.
    pub(crate) fn lookup(&self, name: &str) -> Option<&TypeAliasEntry> {
        self.entries.get(name)
    }

    /// Iterate over all entries in the registry.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &TypeAliasEntry> {
        self.entries.values()
    }

    /// Consume the registry, returning all compiled entries.
    pub(crate) fn into_compiled(self) -> Vec<CompiledTypeAlias> {
        self.entries
            .into_values()
            .map(|e| e.into_compiled())
            .collect()
    }
}

impl Default for TypeAliasRegistry {
    fn default() -> Self {
        TypeAliasRegistry::new()
    }
}

/// Resolve a `TypeExpr` name to a `DimensionVector`.
///
/// Maps dimension type names to their corresponding `DimensionVector` constants.
/// Returns `None` and emits a diagnostic for unrecognized names.
pub(crate) fn resolve_dimension_type(
    type_expr: &reify_syntax::TypeExpr,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    match type_expr.name.as_str() {
        "Length" => Some(DimensionVector::LENGTH),
        "Mass" => Some(DimensionVector::MASS),
        "Time" => Some(DimensionVector::TIME),
        "Current" => Some(DimensionVector::CURRENT),
        "Temperature" => Some(DimensionVector::TEMPERATURE),
        "Angle" => Some(DimensionVector::ANGLE),
        "Area" => Some(DimensionVector::AREA),
        "Volume" => Some(DimensionVector::VOLUME),
        "Force" => Some(reify_types::dimension::FORCE),
        "Dimensionless" => Some(DimensionVector::DIMENSIONLESS),
        other => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unknown dimension type '{}': expected one of Length, Mass, Time, Current, \
                     Temperature, Angle, Area, Volume, Force, Dimensionless",
                    other
                ))
                .with_label(DiagnosticLabel::new(
                    type_expr.span,
                    "unrecognized dimension type",
                )),
            );
            None
        }
    }
}

/// Evaluate a constant expression to a `f64` value.
///
/// Supports: `NumberLiteral`, `BinOp` on constant sub-expressions,
/// unary negation (`UnOp`), and `QuantityLiteral` (resolved via the registry
/// first, then the hardcoded fallback table).
///
/// Returns `None` and emits a diagnostic for non-constant expressions.
pub(crate) fn evaluate_const_expr(
    expr: &reify_syntax::Expr,
    registry: &UnitRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    match &expr.kind {
        reify_syntax::ExprKind::NumberLiteral(v) => Some(*v),
        reify_syntax::ExprKind::BinOp { op, left, right } => {
            let lhs = evaluate_const_expr(left, registry, diagnostics)?;
            let rhs = evaluate_const_expr(right, registry, diagnostics)?;
            let result = match op.as_str() {
                "+" => Some(lhs + rhs),
                "-" => Some(lhs - rhs),
                "*" => Some(lhs * rhs),
                "/" => {
                    if rhs == 0.0 {
                        diagnostics.push(
                            Diagnostic::error("division by zero in unit conversion expression")
                                .with_label(DiagnosticLabel::new(expr.span, "here")),
                        );
                        return None;
                    }
                    Some(lhs / rhs)
                }
                other => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unsupported operator '{}' in unit conversion expression",
                            other
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "here")),
                    );
                    None
                }
            };
            // Guard: reject non-finite arithmetic results (inf, NaN from overflow).
            if let Some(v) = result
                && !v.is_finite()
            {
                diagnostics.push(
                    Diagnostic::error("overflow in unit conversion expression")
                        .with_label(DiagnosticLabel::new(expr.span, "result is not finite")),
                );
                return None;
            }
            result
        }
        reify_syntax::ExprKind::UnOp { op, operand } => {
            let val = evaluate_const_expr(operand, registry, diagnostics)?;
            match op.as_str() {
                "-" => Some(-val),
                other => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unsupported unary operator '{}' in unit conversion expression",
                            other
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "here")),
                    );
                    None
                }
            }
        }
        reify_syntax::ExprKind::QuantityLiteral { value, unit } => {
            // Try registry first, then hardcoded fallback.
            if let Some(entry) = registry.lookup(unit) {
                // Affine (offset) units cannot be used in unit conversion expressions —
                // the additive offset only makes sense for runtime value expressions
                // like '25degC', not for defining conversion factors.
                if entry.offset.is_some() {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "affine (offset) unit '{}' cannot be used in unit conversion expressions; \
                             offset units are only valid in value expressions like '25{}'",
                            unit, unit
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "offset unit used in conversion")),
                    );
                    return None;
                }
                let si = value * entry.factor;
                if !si.is_finite() {
                    diagnostics.push(
                        Diagnostic::error("overflow in unit conversion expression")
                            .with_label(DiagnosticLabel::new(expr.span, "result is not finite")),
                    );
                    return None;
                }
                Some(si)
            } else if let Some((scalar_val, _dim)) = unit_to_scalar(*value, unit) {
                if let Value::Scalar { si_value, .. } = scalar_val {
                    Some(si_value)
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "internal error: unit_to_scalar returned unexpected value variant for unit '{}'; please report this",
                            unit
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "unexpected value variant")),
                    );
                    None
                }
            } else {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "unknown unit '{}' in unit conversion expression",
                        unit
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "unrecognized unit")),
                );
                None
            }
        }
        _ => {
            diagnostics.push(
                Diagnostic::error(
                    "non-constant expression in unit conversion: only numeric literals, \
                     arithmetic, and quantity literals are allowed",
                )
                .with_label(DiagnosticLabel::new(expr.span, "non-constant expression")),
            );
            None
        }
    }
}

/// Compile a `UnitDecl` into a `UnitEntry`.
///
/// Resolves the dimension type, evaluates conversion and offset expressions,
/// and computes a content hash. Returns `None` if the dimension type is unknown
/// or if a conversion/offset expression fails to evaluate as a constant.
pub(crate) fn compile_unit(
    decl: &reify_syntax::UnitDecl,
    registry: &UnitRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<UnitEntry> {
    let dimension = resolve_dimension_type(&decl.dimension_type, diagnostics)?;
    let factor = if let Some(expr) = &decl.conversion {
        evaluate_const_expr(expr, registry, diagnostics)? // eval failed; diagnostic already emitted
    } else {
        1.0 // base unit with no conversion expression
    };
    // Defense-in-depth: reject zero and non-finite factors at the compile_unit level.
    // A zero factor destroys unit information (all values map to the same SI value).
    // A non-finite factor poisons all downstream computations.
    if !factor.is_finite() || factor == 0.0 {
        let msg = if factor == 0.0 {
            format!(
                "unit '{}' has zero conversion factor; factor must be finite and non-zero",
                decl.name
            )
        } else {
            format!(
                "unit '{}' has non-finite conversion factor ({}); factor must be finite and non-zero",
                decl.name, factor
            )
        };
        diagnostics.push(
            Diagnostic::error(msg).with_label(DiagnosticLabel::new(decl.span, "invalid factor")),
        );
        return None;
    }
    let offset = if let Some(expr) = &decl.offset {
        Some(evaluate_const_expr(expr, registry, diagnostics)?) // eval failed; diagnostic already emitted
    } else {
        None // non-affine unit with no offset
    };
    // Defense-in-depth: reject non-finite offset values.
    if let Some(off) = offset
        && !off.is_finite()
    {
        diagnostics.push(
            Diagnostic::error(format!(
                "unit '{}' has non-finite offset ({}); offset must be finite",
                decl.name, off
            ))
            .with_label(DiagnosticLabel::new(decl.span, "invalid offset")),
        );
        return None;
    }
    // Content hash: name + dimension bits + factor + offset
    let hash = {
        let dim_bytes: Vec<u8> = dimension
            .0
            .iter()
            .flat_map(|r| {
                let num = r.num().to_le_bytes();
                let den = r.den().to_le_bytes();
                [num[0], num[1], den[0], den[1]]
            })
            .collect();
        let mut h = ContentHash::of_str(&decl.name)
            .combine(ContentHash::of(&dim_bytes))
            .combine(ContentHash::of(&factor.to_bits().to_le_bytes()));
        if let Some(off) = offset {
            h = h.combine(ContentHash::of(&off.to_bits().to_le_bytes()));
        }
        h
    };
    Some(UnitEntry {
        name: decl.name.clone(),
        dimension,
        factor,
        offset,
        is_pub: decl.is_pub,
        span: decl.span,
        content_hash: hash,
        source_module: None,
    })
}

// --- Type resolution ---

/// Resolve a type name to a `Type`.
pub(crate) fn resolve_type_name(name: &str) -> Option<Type> {
    match name {
        "Scalar" => Some(Type::length()), // Default scalar is length-dimensioned in M1
        "Bool" => Some(Type::Bool),
        "Int" => Some(Type::Int),
        "Real" => Some(Type::Real),
        "String" => Some(Type::String),
        "Length" => Some(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        "Mass" => Some(Type::Scalar {
            dimension: DimensionVector::MASS,
        }),
        "Time" => Some(Type::Scalar {
            dimension: DimensionVector::TIME,
        }),
        "Current" => Some(Type::Scalar {
            dimension: DimensionVector::CURRENT,
        }),
        "Temperature" => Some(Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        }),
        "Angle" => Some(Type::Scalar {
            dimension: DimensionVector::ANGLE,
        }),
        "Area" => Some(Type::Scalar {
            dimension: DimensionVector::AREA,
        }),
        "Volume" => Some(Type::Scalar {
            dimension: DimensionVector::VOLUME,
        }),
        "Force" => Some(Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        }),
        "Dimensionless" => Some(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
        _ => None,
    }
}

/// Resolve a type name, also checking type parameter names.
/// Returns `Type::TypeParam(name)` if the name matches a known type parameter.
pub(crate) fn resolve_type_with_params(name: &str, type_param_names: &HashSet<String>) -> Option<Type> {
    if let Some(ty) = resolve_type_name(name) {
        return Some(ty);
    }
    if type_param_names.contains(name) {
        return Some(Type::TypeParam(name.to_string()));
    }
    None
}

/// Resolve a type name, checking builtins, type parameters, then the alias registry.
///
/// This is the primary type resolution function when aliases are available.
/// Falls through: builtins → type params → alias registry.
pub(crate) fn resolve_type_with_aliases(
    name: &str,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
) -> Option<Type> {
    if let Some(ty) = resolve_type_with_params(name, type_param_names) {
        return Some(ty);
    }
    // Check alias registry for non-parameterized aliases
    if let Some(alias_entry) = alias_registry.lookup(name)
        && let Some(ref resolved) = alias_entry.resolved_type
    {
        return Some(resolved.clone());
    }
    None
}

/// Resolve a type alias's RHS `TypeExpr` to a `Type`.
///
/// Handles three cases:
/// 1. Simple name → resolved via builtins then alias registry
/// 2. Dimensional binary op (`*`, `/`) → recursively resolve operands to
///    DimensionVectors, combine with mul/div, return `Type::Scalar { dimension }`
/// 3. Unknown → returns None
pub(crate) fn resolve_type_alias_expr(
    type_expr: &reify_syntax::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    match type_expr.name.as_str() {
        "*" | "/" => {
            // Dimensional binary operator: left OP right
            if type_expr.type_args.len() != 2 {
                return None;
            }
            let left_dim = resolve_type_alias_expr_to_dimension(
                &type_expr.type_args[0],
                alias_registry,
                diagnostics,
            )?;
            let right_dim = resolve_type_alias_expr_to_dimension(
                &type_expr.type_args[1],
                alias_registry,
                diagnostics,
            )?;
            let result_dim = if type_expr.name == "*" {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        name => {
            // Check for parameterized builtin types (List<T>, Set<T>, Map<K,V>, Option<T>)
            if !type_expr.type_args.is_empty()
                && let Some(ty) = resolve_parameterized_builtin_type(
                    name,
                    &type_expr.type_args,
                    alias_registry,
                    diagnostics,
                )
            {
                return Some(ty);
            }
            // Check for user-defined parameterized alias instantiation.
            // Use temporary diagnostics: during DFS pre-pass, type args may
            // contain unresolved type params (e.g. Container<T>) — we must not
            // emit errors for those; the alias body will be fully resolved at
            // instantiation time via resolve_type_alias_expr_with_subst.
            if !type_expr.type_args.is_empty()
                && let Some(alias_entry) = alias_registry.lookup(name)
                && !alias_entry.type_params.is_empty()
            {
                let empty = HashSet::new();
                let mut tmp_diags = Vec::new();
                if let Some(ty) = resolve_parameterized_alias(
                    alias_entry,
                    &type_expr.type_args,
                    &empty,
                    alias_registry,
                    &mut tmp_diags,
                    0,
                ) {
                    return Some(ty);
                }
                // Silently return None — deferred to instantiation time
            }
            // Simple name: check builtins, then alias registry
            let empty = HashSet::new();
            resolve_type_with_aliases(name, &empty, alias_registry)
        }
    }
}

/// Helper: resolve a TypeExpr to a DimensionVector (for dimensional algebra).
/// Returns None if the type cannot be resolved to a dimension.
pub(crate) fn resolve_type_alias_expr_to_dimension(
    type_expr: &reify_syntax::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    match type_expr.name.as_str() {
        "*" | "/" => {
            if type_expr.type_args.len() != 2 {
                return None;
            }
            let left = resolve_type_alias_expr_to_dimension(
                &type_expr.type_args[0],
                alias_registry,
                diagnostics,
            )?;
            let right = resolve_type_alias_expr_to_dimension(
                &type_expr.type_args[1],
                alias_registry,
                diagnostics,
            )?;
            Some(if type_expr.name == "*" {
                left.mul(&right)
            } else {
                left.div(&right)
            })
        }
        _ => {
            // Try resolve_dimension_type for known dimension names
            // Use a temporary diagnostics vec to avoid polluting the main one
            let mut tmp_diags = Vec::new();
            if let Some(dim) = resolve_dimension_type(type_expr, &mut tmp_diags) {
                return Some(dim);
            }
            // Check alias registry: if the alias resolves to Scalar{dim}, use that dimension
            if let Some(entry) = alias_registry.lookup(&type_expr.name)
                && let Some(Type::Scalar { dimension }) = &entry.resolved_type
            {
                return Some(*dimension);
            }
            // Fall through to error
            diagnostics.push(
                Diagnostic::error(format!(
                    "cannot resolve '{}' to a dimension type in alias expression",
                    type_expr.name
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "not a dimension type")),
            );
            None
        }
    }
}

/// Resolve a full TypeExpr at a use site, handling parameterized aliases.
///
/// Falls through: builtins → type params → non-parameterized aliases → parameterized aliases.
/// Returns None if the type cannot be resolved (caller handles "unresolved" error).
pub(crate) fn resolve_type_expr_with_aliases(
    type_expr: &reify_syntax::TypeExpr,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    // Check parameterized builtins (List<T>, Set<T>, Map<K,V>, Option<T>)
    if !type_expr.type_args.is_empty()
        && let Some(ty) = resolve_parameterized_builtin_type(
            &type_expr.name,
            &type_expr.type_args,
            alias_registry,
            diagnostics,
        )
    {
        return Some(ty);
    }

    // Simple name resolution (builtins, type params, non-parameterized aliases)
    if let Some(ty) = resolve_type_with_aliases(&type_expr.name, type_param_names, alias_registry) {
        return Some(ty);
    }

    // Check parameterized alias instantiation
    if let Some(alias_entry) = alias_registry.lookup(&type_expr.name)
        && !alias_entry.type_params.is_empty()
    {
        return resolve_parameterized_alias(
            alias_entry,
            &type_expr.type_args,
            type_param_names,
            alias_registry,
            diagnostics,
            0,
        );
    }

    None
}

/// Maximum recursion depth for parameterized alias instantiation.
/// Prevents stack overflow from recursive type aliases like `type A<T> = List<A<T>>`.
const MAX_ALIAS_INSTANTIATION_DEPTH: usize = 64;

/// Instantiate a parameterized alias by substituting type arguments.
///
/// Builds a substitution map from param names to concrete types, then
/// resolves the alias body with those substitutions applied.
pub(crate) fn resolve_parameterized_alias(
    alias_entry: &TypeAliasEntry,
    type_args: &[reify_syntax::TypeExpr],
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Option<Type> {
    if depth > MAX_ALIAS_INSTANTIATION_DEPTH {
        diagnostics.push(
            Diagnostic::error(format!(
                "type alias '{}' exceeds maximum instantiation depth (recursive type alias)",
                alias_entry.name
            ))
            .with_label(DiagnosticLabel::new(
                alias_entry.span,
                "recursive expansion",
            )),
        );
        return None;
    }
    let total_params = alias_entry.type_params.len();
    let got = type_args.len();
    let required_params = alias_entry
        .type_params
        .iter()
        .take_while(|p| p.default.is_none())
        .count();

    if got < required_params || got > total_params {
        diagnostics.push(
            Diagnostic::error(format!(
                "type alias '{}' expects {}{} type argument(s), got {}",
                alias_entry.name,
                if required_params < total_params {
                    format!("{}-", required_params)
                } else {
                    String::new()
                },
                total_params,
                got
            ))
            .with_label(DiagnosticLabel::new(alias_entry.span, "defined here")),
        );
        return None;
    }

    // Resolve each explicit type argument to a concrete Type
    let mut subst: HashMap<String, Type> = HashMap::new();
    for (param, arg_expr) in alias_entry.type_params.iter().zip(type_args) {
        let resolved =
            resolve_type_expr_with_aliases(arg_expr, type_param_names, alias_registry, diagnostics);
        if let Some(ty) = resolved {
            subst.insert(param.name.clone(), ty);
        } else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved type argument '{}' for alias '{}'",
                    arg_expr.name, alias_entry.name
                ))
                .with_label(DiagnosticLabel::new(arg_expr.span, "unknown type")),
            );
            return None;
        }
    }
    // Fill in defaults for remaining params
    for param in alias_entry.type_params.iter().skip(got) {
        if let Some(ref default_ty) = param.default {
            subst.insert(param.name.clone(), default_ty.clone());
        }
    }

    // Apply substitution to alias body
    let body = alias_entry.type_expr.as_ref()?;
    resolve_type_alias_expr_with_subst(body, alias_registry, &subst, diagnostics, depth + 1)
}

/// Resolve a type alias body TypeExpr with parameter substitutions applied.
///
/// Like `resolve_type_alias_expr`, but checks the substitution map first so
/// type parameters in the alias body get replaced with concrete types.
///
/// The `depth` parameter tracks alias expansion depth to prevent stack overflow
/// from recursive parameterized type aliases.
pub(crate) fn resolve_type_alias_expr_with_subst(
    type_expr: &reify_syntax::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    subst: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Option<Type> {
    if depth > MAX_ALIAS_INSTANTIATION_DEPTH {
        diagnostics.push(
            Diagnostic::error(format!(
                "type alias '{}' exceeds maximum instantiation depth (recursive type alias)",
                type_expr.name
            ))
            .with_label(DiagnosticLabel::new(type_expr.span, "recursive expansion")),
        );
        return None;
    }
    match type_expr.name.as_str() {
        "*" | "/" => {
            if type_expr.type_args.len() != 2 {
                return None;
            }
            let left_dim = resolve_type_alias_expr_to_dim_with_subst(
                &type_expr.type_args[0],
                alias_registry,
                subst,
                diagnostics,
            )?;
            let right_dim = resolve_type_alias_expr_to_dim_with_subst(
                &type_expr.type_args[1],
                alias_registry,
                subst,
                diagnostics,
            )?;
            let result_dim = if type_expr.name == "*" {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        name => {
            // Check substitution map first (type parameters)
            if let Some(ty) = subst.get(name) {
                return Some(ty.clone());
            }
            // Check for parameterized builtin types (List<T>, Set<T>, Map<K,V>, Option<T>)
            if !type_expr.type_args.is_empty()
                && let Some(ty) = resolve_parameterized_builtin_type_with_subst(
                    name,
                    &type_expr.type_args,
                    alias_registry,
                    subst,
                    diagnostics,
                    depth,
                )
            {
                return Some(ty);
            }
            // Check for user-defined parameterized alias instantiation
            if !type_expr.type_args.is_empty()
                && let Some(alias_entry) = alias_registry.lookup(name)
                && !alias_entry.type_params.is_empty()
            {
                // Resolve type args with current substitutions applied,
                // then build inner substitution for the target alias body
                let total_params = alias_entry.type_params.len();
                let got = type_expr.type_args.len();
                let required_params = alias_entry
                    .type_params
                    .iter()
                    .take_while(|p| p.default.is_none())
                    .count();
                if got < required_params || got > total_params {
                    return None;
                }
                let mut inner_subst: HashMap<String, Type> = HashMap::new();
                for (param, arg_expr) in alias_entry
                    .type_params
                    .iter()
                    .zip(type_expr.type_args.iter())
                {
                    let resolved = resolve_type_alias_expr_with_subst(
                        arg_expr,
                        alias_registry,
                        subst,
                        diagnostics,
                        depth,
                    )?;
                    inner_subst.insert(param.name.clone(), resolved);
                }
                for param in alias_entry.type_params.iter().skip(got) {
                    if let Some(ref default_ty) = param.default {
                        inner_subst.insert(param.name.clone(), default_ty.clone());
                    }
                }
                let body = alias_entry.type_expr.as_ref()?;
                return resolve_type_alias_expr_with_subst(
                    body,
                    alias_registry,
                    &inner_subst,
                    diagnostics,
                    depth + 1,
                );
            }
            // Then builtins + alias registry
            let empty = HashSet::new();
            resolve_type_with_aliases(name, &empty, alias_registry)
        }
    }
}

/// Resolve a parameterized builtin type constructor (List, Set, Map, Option)
/// within a type alias RHS expression.
///
/// Each type argument is resolved recursively via `resolve_type_alias_expr`.
pub(crate) fn resolve_parameterized_builtin_type(
    name: &str,
    type_args: &[reify_syntax::TypeExpr],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    match name {
        "List" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::List(Box::new(inner)))
        }
        "Set" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::Set(Box::new(inner)))
        }
        "Map" if type_args.len() == 2 => {
            let key = resolve_type_alias_expr(&type_args[0], alias_registry, diagnostics)?;
            let val = resolve_type_alias_expr(&type_args[1], alias_registry, diagnostics)?;
            Some(Type::Map(Box::new(key), Box::new(val)))
        }
        "Option" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::Option(Box::new(inner)))
        }
        _ => None,
    }
}

/// Like `resolve_parameterized_builtin_type`, but applies parameter substitutions
/// when resolving type arguments.
pub(crate) fn resolve_parameterized_builtin_type_with_subst(
    name: &str,
    type_args: &[reify_syntax::TypeExpr],
    alias_registry: &TypeAliasRegistry,
    subst: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
) -> Option<Type> {
    match name {
        "List" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::List(Box::new(inner)))
        }
        "Set" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::Set(Box::new(inner)))
        }
        "Map" if type_args.len() == 2 => {
            let key = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            let val = resolve_type_alias_expr_with_subst(
                &type_args[1],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::Map(Box::new(key), Box::new(val)))
        }
        "Option" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::Option(Box::new(inner)))
        }
        _ => None,
    }
}

/// Helper: resolve a TypeExpr to a DimensionVector with parameter substitutions.
pub(crate) fn resolve_type_alias_expr_to_dim_with_subst(
    type_expr: &reify_syntax::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    subst: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    match type_expr.name.as_str() {
        "*" | "/" => {
            if type_expr.type_args.len() != 2 {
                return None;
            }
            let left = resolve_type_alias_expr_to_dim_with_subst(
                &type_expr.type_args[0],
                alias_registry,
                subst,
                diagnostics,
            )?;
            let right = resolve_type_alias_expr_to_dim_with_subst(
                &type_expr.type_args[1],
                alias_registry,
                subst,
                diagnostics,
            )?;
            Some(if type_expr.name == "*" {
                left.mul(&right)
            } else {
                left.div(&right)
            })
        }
        name => {
            // Check substitution map (type param → concrete Type → extract dimension)
            if let Some(Type::Scalar { dimension }) = subst.get(name) {
                return Some(*dimension);
            }
            // Try resolve_dimension_type for known dimension names
            let mut tmp_diags = Vec::new();
            if let Some(dim) = resolve_dimension_type(type_expr, &mut tmp_diags) {
                return Some(dim);
            }
            // Check alias registry
            if let Some(entry) = alias_registry.lookup(name)
                && let Some(Type::Scalar { dimension }) = &entry.resolved_type
            {
                return Some(*dimension);
            }
            diagnostics.push(
                Diagnostic::error(format!(
                    "cannot resolve '{}' to a dimension type in alias expression",
                    name
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "not a dimension type")),
            );
            None
        }
    }
}

/// Collect all leaf type names referenced in a TypeExpr tree.
/// For binary ops (`*`, `/`), recurses into operands. Otherwise returns the name.
pub(crate) fn collect_type_expr_names(type_expr: &reify_syntax::TypeExpr) -> Vec<String> {
    match type_expr.name.as_str() {
        "*" | "/" => type_expr
            .type_args
            .iter()
            .flat_map(collect_type_expr_names)
            .collect(),
        name => std::iter::once(name.to_string())
            .chain(type_expr.type_args.iter().flat_map(collect_type_expr_names))
            .collect(),
    }
}

/// DFS-resolve a type alias, detecting cycles via a resolving-set.
///
/// - If already in the registry → skip (already resolved).
/// - If in the resolving set → emit circular error, register with None.
/// - Otherwise: resolve dependencies first, then resolve this alias.
pub(crate) fn resolve_alias_dfs(
    name: &str,
    alias_decls: &HashMap<String, &reify_syntax::TypeAliasDecl>,
    alias_registry: &mut TypeAliasRegistry,
    resolving: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Already resolved (or registered as cycle-error placeholder)
    if alias_registry.lookup(name).is_some() {
        return;
    }
    // Not a declared alias
    let Some(decl) = alias_decls.get(name) else {
        return;
    };
    // Cycle detected: name is already being resolved up the call stack
    if !resolving.insert(name.to_string()) {
        diagnostics.push(
            Diagnostic::error(format!("circular type alias '{}'", name))
                .with_label(DiagnosticLabel::new(decl.span, "forms a cycle")),
        );
        // Register placeholder to prevent re-processing
        let type_params = convert_type_params(&decl.type_params);
        let entry = TypeAliasEntry {
            name: name.to_string(),
            resolved_type: None,
            type_params,
            type_expr: Some(decl.type_expr.clone()),
            is_pub: decl.is_pub,
            span: decl.span,
            content_hash: decl.content_hash,
        };
        let _ = alias_registry.register(entry);
        return;
    }

    // Resolve dependencies first (only those that are aliases)
    let dep_names = collect_type_expr_names(&decl.type_expr);
    for dep in &dep_names {
        if alias_decls.contains_key(dep.as_str()) {
            resolve_alias_dfs(dep, alias_decls, alias_registry, resolving, diagnostics);
        }
    }

    // Now resolve this alias (dependencies should be in the registry)
    let resolved = resolve_type_alias_expr(&decl.type_expr, alias_registry, diagnostics);
    let type_params = convert_type_params(&decl.type_params);
    let entry = TypeAliasEntry {
        name: name.to_string(),
        resolved_type: resolved,
        type_params,
        type_expr: Some(decl.type_expr.clone()),
        is_pub: decl.is_pub,
        span: decl.span,
        content_hash: decl.content_hash,
    };
    // May fail if cycle detection already registered this name — that's OK
    let _ = alias_registry.register(entry);

    resolving.remove(name);
}

/// Convert parsed TypeParamDecl to compiled TypeParam structs.
pub(crate) fn convert_type_params(decls: &[reify_syntax::TypeParamDecl]) -> Vec<reify_types::TypeParam> {
    decls
        .iter()
        .map(|d| {
            let bounds = d
                .bounds
                .iter()
                .map(|b| reify_types::TraitBound {
                    trait_ref: reify_types::TraitRef {
                        name: b.clone(),
                        type_args: vec![],
                    },
                })
                .collect();
            // Resolve defaults: try builtin types first, then preserve
            // structure names as StructureRef (concrete names, not type variables).
            let default = d.default.as_ref().map(|te| {
                resolve_type_name(&te.name).unwrap_or_else(|| Type::StructureRef(te.name.clone()))
            });
            reify_types::TypeParam {
                name: d.name.clone(),
                bounds,
                default,
            }
        })
        .collect()
}

