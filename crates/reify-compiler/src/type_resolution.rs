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
    let name = match &type_expr.kind {
        reify_syntax::TypeExprKind::Named { name, .. } => name.as_str(),
        reify_syntax::TypeExprKind::DimensionalOp { .. } => return None,
        reify_syntax::TypeExprKind::IntegerLiteral(_) => return None,
    };
    // Scan the shared table (name → dimension direction).
    if let Some((dim, _)) = reify_types::NAMED_DIMENSIONS.iter().find(|(_, n)| *n == name) {
        return Some(*dim);
    }
    // "Dimensionless" is intentionally absent from NAMED_DIMENSIONS (canonical_name returns
    // None for it), but resolve_dimension_type must still accept it.
    if name == "Dimensionless" {
        return Some(DimensionVector::DIMENSIONLESS);
    }
    // Unknown name: emit a diagnostic whose expected-names list is derived from the shared
    // table chained with "Dimensionless" so it cannot drift from NAMED_DIMENSIONS.
    // The list is exposed both in the prose message and in the structured `candidates` field;
    // downstream consumers (LSP quick-fixes, IDE tooling) should prefer the structured field
    // rather than parsing the prose.
    //
    // Build as Vec<&str> once so the prose join and the structured candidates share one
    // source of truth; with_candidates owns the &str→String conversion.
    let candidate_strs: Vec<&str> = reify_types::NAMED_DIMENSIONS
        .iter()
        .map(|(_, n)| *n)
        .chain(std::iter::once("Dimensionless"))
        .collect();
    let names_list = candidate_strs.join(", ");
    diagnostics.push(
        Diagnostic::error(format!(
            "unknown dimension type '{}': expected one of {}",
            name, names_list
        ))
        .with_label(DiagnosticLabel::new(
            type_expr.span,
            "unrecognized dimension type",
        ))
        .with_candidates(candidate_strs),
    );
    None
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
        "Solid" => Some(Type::Geometry),  // Surface-syntax alias for the geometry-handle type
        "Bool" => Some(Type::Bool),
        "Int" => Some(Type::Int),
        "Real" => Some(Type::Real),
        "String" => Some(Type::String),
        // SI base dimensions
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
        "AmountOfSubstance" => Some(Type::Scalar {
            dimension: DimensionVector::AMOUNT_OF_SUBSTANCE,
        }),
        "LuminousIntensity" => Some(Type::Scalar {
            dimension: DimensionVector::LUMINOUS_INTENSITY,
        }),
        "Angle" => Some(Type::Scalar {
            dimension: DimensionVector::ANGLE,
        }),
        "SolidAngle" => Some(Type::Scalar {
            dimension: DimensionVector::SOLID_ANGLE,
        }),
        "Money" => Some(Type::Scalar {
            dimension: DimensionVector::MONEY,
        }),
        // Geometric derived dimensions
        "Area" => Some(Type::Scalar {
            dimension: DimensionVector::AREA,
        }),
        "Volume" => Some(Type::Scalar {
            dimension: DimensionVector::VOLUME,
        }),
        // SI derived dimensions
        "Force" => Some(Type::Scalar {
            dimension: DimensionVector::FORCE,
        }),
        "Energy" => Some(Type::Scalar {
            dimension: DimensionVector::ENERGY,
        }),
        "Power" => Some(Type::Scalar {
            dimension: DimensionVector::POWER,
        }),
        "Pressure" => Some(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
        "Frequency" => Some(Type::Scalar {
            dimension: DimensionVector::FREQUENCY,
        }),
        "Voltage" => Some(Type::Scalar {
            dimension: DimensionVector::VOLTAGE,
        }),
        "Charge" => Some(Type::Scalar {
            dimension: DimensionVector::CHARGE,
        }),
        "Capacitance" => Some(Type::Scalar {
            dimension: DimensionVector::CAPACITANCE,
        }),
        "Resistance" => Some(Type::Scalar {
            dimension: DimensionVector::RESISTANCE,
        }),
        "Conductance" => Some(Type::Scalar {
            dimension: DimensionVector::CONDUCTANCE,
        }),
        "Inductance" => Some(Type::Scalar {
            dimension: DimensionVector::INDUCTANCE,
        }),
        "MagneticFlux" => Some(Type::Scalar {
            dimension: DimensionVector::MAGNETIC_FLUX,
        }),
        "MagneticFluxDensity" => Some(Type::Scalar {
            dimension: DimensionVector::MAGNETIC_FLUX_DENSITY,
        }),
        "LuminousFlux" => Some(Type::Scalar {
            dimension: DimensionVector::LUMINOUS_FLUX,
        }),
        "Illuminance" => Some(Type::Scalar {
            dimension: DimensionVector::ILLUMINANCE,
        }),
        "AbsorbedDose" => Some(Type::Scalar {
            dimension: DimensionVector::ABSORBED_DOSE,
        }),
        "AngularVelocity" => Some(Type::Scalar {
            dimension: DimensionVector::ANGULAR_VELOCITY,
        }),
        "DynamicViscosity" => Some(Type::Scalar {
            dimension: DimensionVector::DYNAMIC_VISCOSITY,
        }),
        "MomentOfInertia" => Some(Type::Scalar {
            dimension: DimensionVector::MOMENT_OF_INERTIA,
        }),
        "Density" => Some(Type::Scalar {
            dimension: DimensionVector::MASS_DENSITY,
        }),
        "Dimensionless" => Some(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
        _ => None,
    }
}

/// Resolve a type name, also checking type parameter names.
/// Returns `Type::TypeParam(name)` if the name matches a known type parameter.
pub(crate) fn resolve_type_with_params(
    name: &str,
    type_param_names: &HashSet<String>,
) -> Option<Type> {
    if let Some(ty) = resolve_type_name(name) {
        return Some(ty);
    }
    if type_param_names.contains(name) {
        return Some(Type::TypeParam(name.to_string()));
    }
    None
}

/// Resolve a type name, checking builtins, type parameters, the alias
/// registry, structure names, and finally trait names.
///
/// This is the primary type resolution function when aliases are available.
/// Falls through: builtins → type params → alias registry → structure names → trait names.
///
/// Structure-name resolution runs BEFORE trait-name fallback so that a name
/// bound to a concrete structure template (e.g. stdlib `Material`) resolves
/// to `Type::StructureRef`, not `Type::TraitObject`. Trait-name resolution is
/// LAST so existing sources that happened to reuse a name present in the
/// builtin/alias/generic-param/structure namespaces keep their prior
/// resolution behavior; trait names only resolve when no earlier kind matches.
pub(crate) fn resolve_type_with_aliases(
    name: &str,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
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
    // Structure-name resolution (before trait fallback): `param material : Material`
    // where `Material` is a structure def resolves to Type::StructureRef("Material").
    // This takes precedence over trait-name fallback so that the canonical
    // first-class Material struct wins over any same-named trait (task 1876).
    if structure_names.contains(name) {
        return Some(Type::StructureRef(name.to_string()));
    }
    // Trait-name fallback (last in precedence): `param m : MaterialSpec` where
    // `MaterialSpec` is a trait name resolves to Type::TraitObject("MaterialSpec").
    if trait_names.contains(name) {
        return Some(Type::TraitObject(name.to_string()));
    }
    None
}

/// Resolve a simple name to a `Type::Enum` if it matches a declared enum; `None` otherwise.
///
/// Does NOT perform builtin/alias/trait fallback — use `resolve_type_with_aliases` first
/// and chain with `.or_else(|| resolve_enum_type(...))`.
///
/// # Hot-path note
///
/// This function performs an O(N) scan over `enum_defs` on every call.
/// In tight loops iterating over many type expressions (e.g. `check_trait_conformance`'s
/// `structure_members` filter_map), callers should instead build a `HashSet<&str>` once
/// before the loop and use `set.contains(name).then(|| Type::Enum(name.to_string()))`
/// directly — the same lookup but O(1) per call.  This helper remains the right choice
/// at callsites that resolve a single name.
pub(crate) fn resolve_enum_type(name: &str, enum_defs: &[reify_types::EnumDef]) -> Option<Type> {
    if enum_defs.iter().any(|e| e.name == name) {
        Some(Type::Enum(name.to_string()))
    } else {
        None
    }
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
    match &type_expr.kind {
        reify_syntax::TypeExprKind::DimensionalOp { op, left, right } => {
            // Dimensional binary operator: left OP right
            let left_dim = resolve_type_alias_expr_to_dimension(left, alias_registry, diagnostics)?;
            let right_dim =
                resolve_type_alias_expr_to_dimension(right, alias_registry, diagnostics)?;
            let result_dim = if matches!(op, reify_syntax::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            // Check for parameterized builtin types (List<T>, Set<T>, Map<K,V>, Option<T>).
            // Pass empty structure/trait name sets: this DFS runs before traits and
            // structures are compiled, so trait-name fallback must NOT fire here.
            if !type_args.is_empty()
                && let Some(ty) = resolve_parameterized_builtin_type(
                    name,
                    type_args,
                    alias_registry,
                    diagnostics,
                    &HashSet::new(),
                    &HashSet::new(),
                )
            {
                return Some(ty);
            }
            // Check for user-defined parameterized alias instantiation.
            // Use temporary diagnostics: during DFS pre-pass, type args may
            // contain unresolved type params (e.g. Container<T>) — we must not
            // emit errors for those; the alias body will be fully resolved at
            // instantiation time via resolve_type_alias_expr_with_subst.
            //
            // Trait-name resolution is NOT applied during alias DFS: trait
            // names aren't known until compile_trait has populated the trait
            // registry, which happens after alias resolution. Pass an empty
            // trait-name set so alias bodies resolve only against builtins
            // and the alias registry here.
            if !type_args.is_empty()
                && let Some(alias_entry) = alias_registry.lookup(name)
                && !alias_entry.type_params.is_empty()
            {
                let empty = HashSet::new();
                let empty_structs = HashSet::new();
                let empty_traits = HashSet::new();
                let mut tmp_diags = Vec::new();
                if let Some(ty) = resolve_parameterized_alias(
                    alias_entry,
                    type_args,
                    &empty,
                    alias_registry,
                    &mut tmp_diags,
                    0,
                    &empty_structs,
                    &empty_traits,
                ) {
                    return Some(ty);
                }
                // Silently return None — deferred to instantiation time
            }
            // Simple name: check builtins, then alias registry
            let empty = HashSet::new();
            let empty_structs = HashSet::new();
            let empty_traits = HashSet::new();
            resolve_type_with_aliases(name, &empty, alias_registry, &empty_structs, &empty_traits)
        }
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            None
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
    match &type_expr.kind {
        reify_syntax::TypeExprKind::DimensionalOp { op, left, right } => {
            let left_dim = resolve_type_alias_expr_to_dimension(left, alias_registry, diagnostics)?;
            let right_dim =
                resolve_type_alias_expr_to_dimension(right, alias_registry, diagnostics)?;
            Some(if matches!(op, reify_syntax::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            })
        }
        reify_syntax::TypeExprKind::Named { name, .. } => {
            // Try resolve_dimension_type for known dimension names
            // Use a temporary diagnostics vec to avoid polluting the main one
            let mut tmp_diags = Vec::new();
            if let Some(dim) = resolve_dimension_type(type_expr, &mut tmp_diags) {
                return Some(dim);
            }
            // Check alias registry: if the alias resolves to Scalar{dim}, use that dimension
            if let Some(entry) = alias_registry.lookup(name)
                && let Some(Type::Scalar { dimension }) = &entry.resolved_type
            {
                return Some(*dimension);
            }
            // Fall through to error
            diagnostics.push(
                Diagnostic::error(format!(
                    "cannot resolve '{}' to a dimension type in alias expression",
                    name
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "not a dimension type")),
            );
            None
        }
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` cannot appear as a dimension type",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "expected a dimension name")),
            );
            None
        }
    }
}

/// Resolve a full TypeExpr at a use site, handling parameterized aliases.
///
/// Falls through: builtins → type params → non-parameterized aliases →
/// parameterized aliases → trait names.
/// Returns None if the type cannot be resolved (caller handles "unresolved" error).
pub(crate) fn resolve_type_expr_with_aliases(
    type_expr: &reify_syntax::TypeExpr,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> Option<Type> {
    let (name, type_args) = match &type_expr.kind {
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            (name.as_str(), type_args.as_slice())
        }
        reify_syntax::TypeExprKind::DimensionalOp { .. } => return None,
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            return None;
        }
    };
    // Check parameterized builtins (List<T>, Set<T>, Map<K,V>, Option<T>)
    if !type_args.is_empty()
        && let Some(ty) = resolve_parameterized_builtin_type(
            name,
            type_args,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        )
    {
        return Some(ty);
    }

    // Simple name resolution (builtins, type params, non-parameterized aliases,
    // structure names, trait names).
    if let Some(ty) = resolve_type_with_aliases(
        name,
        type_param_names,
        alias_registry,
        structure_names,
        trait_names,
    ) {
        return Some(ty);
    }

    // Check parameterized alias instantiation
    if let Some(alias_entry) = alias_registry.lookup(name)
        && !alias_entry.type_params.is_empty()
    {
        return resolve_parameterized_alias(
            alias_entry,
            type_args,
            type_param_names,
            alias_registry,
            diagnostics,
            0,
            structure_names,
            trait_names,
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_parameterized_alias(
    alias_entry: &TypeAliasEntry,
    type_args: &[reify_syntax::TypeExpr],
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    depth: usize,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
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
        let resolved = resolve_type_expr_with_aliases(
            arg_expr,
            type_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        );
        if let Some(ty) = resolved {
            subst.insert(param.name.clone(), ty);
        } else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved type argument '{}' for alias '{}'",
                    arg_expr, alias_entry.name
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
                type_expr
            ))
            .with_label(DiagnosticLabel::new(type_expr.span, "recursive expansion")),
        );
        return None;
    }
    match &type_expr.kind {
        reify_syntax::TypeExprKind::DimensionalOp { op, left, right } => {
            let left_dim = resolve_type_alias_expr_to_dim_with_subst(
                left,
                alias_registry,
                subst,
                diagnostics,
            )?;
            let right_dim = resolve_type_alias_expr_to_dim_with_subst(
                right,
                alias_registry,
                subst,
                diagnostics,
            )?;
            let result_dim = if matches!(op, reify_syntax::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        reify_syntax::TypeExprKind::Named { name, type_args } => {
            // Check substitution map first (type parameters)
            if let Some(ty) = subst.get(name.as_str()) {
                return Some(ty.clone());
            }
            // Check for parameterized builtin types (List<T>, Set<T>, Map<K,V>, Option<T>).
            // Alias DFS runs before traits/structures are compiled, so we use the
            // subst-aware resolver (which is trait-blind by design).
            if !type_args.is_empty()
                && let Some(ty) = resolve_parameterized_builtin_type_with_subst(
                    name,
                    type_args,
                    alias_registry,
                    subst,
                    diagnostics,
                    depth,
                )
            {
                return Some(ty);
            }
            // Check for user-defined parameterized alias instantiation
            if !type_args.is_empty()
                && let Some(alias_entry) = alias_registry.lookup(name)
                && !alias_entry.type_params.is_empty()
            {
                // Resolve type args with current substitutions applied,
                // then build inner substitution for the target alias body
                let total_params = alias_entry.type_params.len();
                let got = type_args.len();
                let required_params = alias_entry
                    .type_params
                    .iter()
                    .take_while(|p| p.default.is_none())
                    .count();
                if got < required_params || got > total_params {
                    return None;
                }
                let mut inner_subst: HashMap<String, Type> = HashMap::new();
                for (param, arg_expr) in alias_entry.type_params.iter().zip(type_args.iter()) {
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
            // Then builtins + alias registry.
            // Trait and structure name resolution is not applied during
            // alias-body resolution under substitution: alias bodies are resolved
            // either during DFS (before traits/structures exist) or during alias
            // instantiation at a use site where the alias body itself should
            // only refer to builtins/aliases.
            let empty = HashSet::new();
            let empty_structs = HashSet::new();
            let empty_traits = HashSet::new();
            resolve_type_with_aliases(name, &empty, alias_registry, &empty_structs, &empty_traits)
        }
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            None
        }
    }
}

/// Resolve a parameterized builtin type constructor (List, Set, Map, Option,
/// Tensor, Matrix, Scalar, Vector3) within a type alias RHS expression.
///
/// Each type argument is resolved recursively via `resolve_type_expr_with_aliases`,
/// which allows inner type args to be trait names (e.g. `Option<MyTrait>`).
/// `Tensor<rank, n, q>` and `Matrix<m, n, q>` consume two integer-literal args
/// followed by a quantity type; `Scalar<Q>` and `Vector3<Q>` each consume one
/// quantity type-expression.
///
/// `structure_names` and `trait_names` are threaded through so that inner args
/// can be resolved to `Type::StructureRef` / `Type::TraitObject` respectively.
/// Pass empty sets when resolving during the alias DFS pre-pass (before traits
/// and structures are compiled).
pub(crate) fn resolve_parameterized_builtin_type(
    name: &str,
    type_args: &[reify_syntax::TypeExpr],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> Option<Type> {
    let empty_type_params = HashSet::new();
    match name {
        "List" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::List(Box::new(inner)))
        }
        "Set" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Set(Box::new(inner)))
        }
        "Map" if type_args.len() == 2 => {
            let key = resolve_type_expr_with_aliases(
                &type_args[0],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            let val = resolve_type_expr_with_aliases(
                &type_args[1],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Map(Box::new(key), Box::new(val)))
        }
        "Option" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Option(Box::new(inner)))
        }
        "Scalar" if type_args.len() == 1 => {
            // Scalar<Q>: resolve Q to a DimensionVector and wrap.
            let dim = resolve_type_alias_expr_to_dimension(
                &type_args[0],
                alias_registry,
                diagnostics,
            )?;
            Some(Type::Scalar { dimension: dim })
        }
        "Vector3" if type_args.len() == 1 => {
            // Vector3<Q>: resolve Q to a DimensionVector and wrap as a 3D vector.
            let dim = resolve_type_alias_expr_to_dimension(
                &type_args[0],
                alias_registry,
                diagnostics,
            )?;
            Some(Type::vec3(Type::Scalar { dimension: dim }))
        }
        "Tensor" if type_args.len() == 3 => {
            // Tensor<rank, n, Q>: two integer literals + a quantity type.
            let rank = expect_integer_literal_type_arg(&type_args[0], "Tensor", "rank", diagnostics)?;
            let n = expect_integer_literal_type_arg(&type_args[1], "Tensor", "n", diagnostics)?;
            let quantity = resolve_type_expr_with_aliases(
                &type_args[2],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::tensor(rank, n, quantity))
        }
        "Matrix" if type_args.len() == 3 => {
            // Matrix<m, n, Q>: two integer literals + a quantity type.
            let m = expect_integer_literal_type_arg(&type_args[0], "Matrix", "m", diagnostics)?;
            let n = expect_integer_literal_type_arg(&type_args[1], "Matrix", "n", diagnostics)?;
            let quantity = resolve_type_expr_with_aliases(
                &type_args[2],
                &empty_type_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::matrix(m, n, quantity))
        }
        _ => None,
    }
}

/// Pull an unsigned integer out of a type-arg position that requires one
/// (`Tensor<rank, n, Q>`, `Matrix<m, n, Q>`). Emits a diagnostic and returns
/// `None` when the arg is not a `TypeExprKind::IntegerLiteral`.
fn expect_integer_literal_type_arg(
    type_expr: &reify_syntax::TypeExpr,
    constructor: &str,
    slot: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    match &type_expr.kind {
        reify_syntax::TypeExprKind::IntegerLiteral(n) => Some(*n as usize),
        _ => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` expects an integer literal for `{}`, found `{}`",
                    constructor, slot, type_expr
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "expected integer literal")),
            );
            None
        }
    }
}

/// Like `resolve_parameterized_builtin_type`, but applies parameter substitutions
/// when resolving type arguments.
///
/// Called during alias DFS (before structures and traits are compiled), so inner type
/// args are resolved via `resolve_type_alias_expr_with_subst` — which is trait-blind by
/// design. There is no `structure_names`/`trait_names` parameter here; the plain
/// alias-DFS resolver is correct for this context.
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
        "Scalar" if type_args.len() == 1 => {
            let dim = resolve_type_alias_expr_to_dim_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
            )?;
            Some(Type::Scalar { dimension: dim })
        }
        "Tensor" if type_args.len() == 3 => {
            let rank = expect_integer_literal_type_arg(&type_args[0], "Tensor", "rank", diagnostics)?;
            let n = expect_integer_literal_type_arg(&type_args[1], "Tensor", "n", diagnostics)?;
            let quantity = resolve_type_alias_expr_with_subst(
                &type_args[2],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::tensor(rank, n, quantity))
        }
        "Matrix" if type_args.len() == 3 => {
            let m = expect_integer_literal_type_arg(&type_args[0], "Matrix", "m", diagnostics)?;
            let n = expect_integer_literal_type_arg(&type_args[1], "Matrix", "n", diagnostics)?;
            let quantity = resolve_type_alias_expr_with_subst(
                &type_args[2],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::matrix(m, n, quantity))
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
    match &type_expr.kind {
        reify_syntax::TypeExprKind::DimensionalOp { op, left, right } => {
            let left_dim = resolve_type_alias_expr_to_dim_with_subst(
                left,
                alias_registry,
                subst,
                diagnostics,
            )?;
            let right_dim = resolve_type_alias_expr_to_dim_with_subst(
                right,
                alias_registry,
                subst,
                diagnostics,
            )?;
            Some(if matches!(op, reify_syntax::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            })
        }
        reify_syntax::TypeExprKind::Named { name, .. } => {
            // Check substitution map (type param → concrete Type → extract dimension)
            if let Some(Type::Scalar { dimension }) = subst.get(name.as_str()) {
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
        reify_syntax::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` cannot appear as a dimension type",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "expected a dimension name")),
            );
            None
        }
    }
}

/// Collect all leaf type names referenced in a TypeExpr tree.
/// For `DimensionalOp`, recurses into both operands. For `Named`, returns the name
/// followed by recursed type_args.
pub(crate) fn collect_type_expr_names(type_expr: &reify_syntax::TypeExpr) -> Vec<String> {
    match &type_expr.kind {
        reify_syntax::TypeExprKind::DimensionalOp { left, right, .. } => {
            collect_type_expr_names(left)
                .into_iter()
                .chain(collect_type_expr_names(right))
                .collect()
        }
        reify_syntax::TypeExprKind::Named { name, type_args } => std::iter::once(name.clone())
            .chain(type_args.iter().flat_map(collect_type_expr_names))
            .collect(),
        // Integer-literal type-args contribute no type *names* to dependency graphs.
        reify_syntax::TypeExprKind::IntegerLiteral(_) => Vec::new(),
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
pub(crate) fn convert_type_params(
    decls: &[reify_syntax::TypeParamDecl],
) -> Vec<reify_types::TypeParam> {
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
            // DimensionalOp cannot appear as a type-parameter default — the grammar
            // only allows Named nodes in that position, so this arm is unreachable.
            let default = d.default.as_ref().map(|te| match &te.kind {
                reify_syntax::TypeExprKind::Named { name, .. } => {
                    resolve_type_name(name).unwrap_or_else(|| Type::StructureRef(name.clone()))
                }
                reify_syntax::TypeExprKind::DimensionalOp { .. } => {
                    unreachable!(
                        "dimensional operator cannot appear as a type-parameter default; \
                             the parser only emits Named nodes for type-param defaults"
                    )
                }
                reify_syntax::TypeExprKind::IntegerLiteral(_) => {
                    unreachable!(
                        "integer literal cannot appear as a type-parameter default; \
                             the grammar restricts integer literals to type_arg_list slots"
                    )
                }
            });
            reify_types::TypeParam {
                name: d.name.clone(),
                bounds,
                default,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal helper: build a Named TypeExpr with a synthetic zero span.
    fn named_type_expr(name: &str) -> reify_syntax::TypeExpr {
        reify_syntax::TypeExpr {
            kind: reify_syntax::TypeExprKind::Named {
                name: name.to_string(),
                type_args: vec![],
            },
            span: reify_types::SourceSpan::new(0, 0),
        }
    }

    #[test]
    fn resolve_type_name_recognises_money() {
        assert_eq!(
            resolve_type_name("Money"),
            Some(Type::Scalar {
                dimension: DimensionVector::MONEY
            })
        );
    }

    #[test]
    fn resolve_type_name_recognises_moment_of_inertia() {
        assert_eq!(
            resolve_type_name("MomentOfInertia"),
            Some(Type::Scalar {
                dimension: DimensionVector::MOMENT_OF_INERTIA
            })
        );
    }

    #[test]
    fn resolve_type_name_recognises_density_as_mass_density() {
        // User-facing "Density" resolves to the kg·m⁻³ mass-density singleton,
        // NOT to MAGNETIC_FLUX_DENSITY. The Rust constant is renamed to
        // MASS_DENSITY to make this distinction unambiguous at the source level.
        assert_eq!(
            resolve_type_name("Density"),
            Some(Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY
            })
        );
        assert_ne!(
            resolve_type_name("Density"),
            Some(Type::Scalar {
                dimension: DimensionVector::MAGNETIC_FLUX_DENSITY
            })
        );
    }

    #[test]
    fn resolve_dimension_type_recognises_money() {
        let te = named_type_expr("Money");
        let mut diagnostics = Vec::new();
        let result = resolve_dimension_type(&te, &mut diagnostics);
        assert_eq!(result, Some(DimensionVector::MONEY));
        assert!(diagnostics.is_empty(), "unexpected diagnostic: {:?}", diagnostics);
    }

    #[test]
    fn resolve_dimension_type_unknown_lists_money_in_error() {
        let te = named_type_expr("Foo");
        let mut diagnostics = Vec::new();
        let _ = resolve_dimension_type(&te, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.contains("Money"),
            "error message should list 'Money' in the expected list; got: {}",
            diagnostics[0].message
        );
    }

    /// Parity contract: `resolve_dimension_type` correctly maps every entry in
    /// `reify_types::NAMED_DIMENSIONS` and the special-cased `"Dimensionless"`.
    ///
    /// This test is written BEFORE the implementation is switched to use the shared table
    /// (step 4), so it serves as a regression-protection contract that will catch any
    /// silent divergence between the old match-arm implementation and the new table-driven
    /// one. It is expected to pass against both implementations.
    #[test]
    fn resolve_dimension_type_round_trips_all_named_dimensions() {
        for &(dim, name) in reify_types::NAMED_DIMENSIONS {
            let te = named_type_expr(name);
            let mut diagnostics = Vec::new();
            let result = resolve_dimension_type(&te, &mut diagnostics);
            assert_eq!(
                result,
                Some(dim),
                "resolve_dimension_type({:?}) should return Some({:?})",
                name,
                dim,
            );
            assert!(
                diagnostics.is_empty(),
                "resolve_dimension_type({:?}) should produce no diagnostics; got: {:?}",
                name,
                diagnostics,
            );
        }
        // Special-case fallback: "Dimensionless" is intentionally absent from NAMED_DIMENSIONS
        // but must still resolve to DimensionVector::DIMENSIONLESS.
        let te = named_type_expr("Dimensionless");
        let mut diagnostics = Vec::new();
        let result = resolve_dimension_type(&te, &mut diagnostics);
        assert_eq!(
            result,
            Some(DimensionVector::DIMENSIONLESS),
            "resolve_dimension_type(\"Dimensionless\") should return Some(DIMENSIONLESS)"
        );
        assert!(
            diagnostics.is_empty(),
            "resolve_dimension_type(\"Dimensionless\") should produce no diagnostics; got: {:?}",
            diagnostics,
        );
    }

    #[test]
    fn solid_resolves_to_geometry() {
        assert_eq!(
            resolve_type_name("Solid"),
            Some(Type::Geometry),
            "\"Solid\" should resolve to Type::Geometry as a surface-syntax alias"
        );
    }

    #[test]
    fn resolve_enum_type_returns_some_for_matching_name() {
        let enum_defs = vec![reify_types::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
        }];
        assert_eq!(
            resolve_enum_type("Direction", &enum_defs),
            Some(Type::Enum("Direction".to_string())),
        );
    }

    #[test]
    fn resolve_enum_type_returns_none_for_non_matching_name() {
        let enum_defs = vec![reify_types::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
        }];
        assert_eq!(resolve_enum_type("Missing", &enum_defs), None);
    }

    #[test]
    fn resolve_enum_type_returns_none_for_empty_slice() {
        assert_eq!(resolve_enum_type("Direction", &[]), None);
    }

    /// Regression lock: the unknown-name diagnostic for `resolve_dimension_type` must expose
    /// every name from `reify_types::NAMED_DIMENSIONS` plus `"Dimensionless"` as a structured
    /// `candidates` field, with no extras or omissions.
    ///
    /// The `candidates` field is a machine-readable `Vec<String>` asserted via exact
    /// set-membership — decoupled from the human-readable prose. A future reword of the message
    /// (e.g. `"expected one of: A, B"` or `"valid names are A, B"`) cannot silently bypass this
    /// assertion because it no longer parses prose at all.
    ///
    /// The prose message wording is not part of this test's contract — the structured
    /// `candidates` field is.
    #[test]
    fn resolve_dimension_type_unknown_diagnostic_lists_all_named_dimensions() {
        let te = named_type_expr("Foo");
        let mut diagnostics = Vec::new();
        let _ = resolve_dimension_type(&te, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        let diag = &diagnostics[0];

        // Structural assertion: the candidate set carries the names list as a
        // machine-readable Vec<String>, decoupled from the human-readable prose.
        let listed_names: std::collections::HashSet<&str> =
            diag.candidates.iter().map(String::as_str).collect();

        let expected_names: std::collections::HashSet<&str> = reify_types::NAMED_DIMENSIONS
            .iter()
            .map(|(_, n)| *n)
            .chain(std::iter::once("Dimensionless"))
            .collect();

        assert_eq!(
            listed_names,
            expected_names,
            "diagnostic.candidates does not exactly match NAMED_DIMENSIONS + Dimensionless"
        );
    }
}
