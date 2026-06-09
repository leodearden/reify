use super::*;
use std::cell::RefCell;
use std::collections::HashSet;

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
    pub(crate) type_params: Vec<reify_ir::TypeParam>,
    /// The original type expression, stored for parameterized alias substitution.
    pub(crate) type_expr: Option<reify_ast::TypeExpr>,
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

    /// Construct a `TypeAliasEntry` from a prelude `CompiledTypeAlias`.
    ///
    /// `type_expr` is set to `None` because `CompiledTypeAlias` deliberately
    /// omits the `TypeExpr` field to preserve the reify-compiler ↔ reify-syntax
    /// module boundary.  As a consequence, parameterized prelude aliases cannot
    /// be substituted at use sites; the caller (seed loop in `phase_aliases`)
    /// must skip entries with non-empty `type_params` before calling this
    /// constructor — otherwise `resolve_parameterized_alias` would find
    /// `type_expr: None` and produce an internal error.
    pub(crate) fn from_compiled_for_prelude(cta: &CompiledTypeAlias) -> TypeAliasEntry {
        TypeAliasEntry {
            name: cta.name.clone(),
            resolved_type: cta.resolved_type.clone(),
            type_params: cta.type_params.clone(),
            type_expr: None,
            is_pub: cta.is_pub,
            span: cta.span,
            content_hash: cta.content_hash,
        }
    }
}

/// Registry mapping type alias names to compiled alias entries.
/// Built during the pre-pass so type resolution can check aliases.
#[derive(Clone)]
pub(crate) struct TypeAliasRegistry {
    entries: HashMap<String, TypeAliasEntry>,
    /// Names of entries seeded from prelude modules (not user-declared).
    ///
    /// `into_compiled()` and `iter()` exclude these so the user module's
    /// exported `type_aliases` and content hash only reflect its own declarations,
    /// mirroring the units pattern (`ctx.compiled_units` only contains user-declared
    /// units, not prelude-seeded ones).
    seeded_names: HashSet<String>,
    /// Names of parametric prelude aliases that were skipped during seeding
    /// (because `CompiledTypeAlias` omits `type_expr`, parametric prelude aliases
    /// cannot be instantiated cross-module).
    ///
    /// Purely diagnostic-side metadata — excluded from `iter()`, `into_compiled()`,
    /// and content-hash computation.  Used by `resolve_type_expr_with_aliases` to
    /// emit a `Severity::Info` hint at use sites so users know the limitation is a
    /// cross-module propagation gap rather than a missing declaration.
    skipped_parametric_prelude_names: HashSet<String>,
    /// Spans on which a `Severity::Info` "parametric prelude alias not propagated"
    /// diagnostic has already been emitted in this compile pass.  Populated by
    /// [`TypeAliasRegistry::should_emit_skipped_parametric_prelude_info`] so that
    /// double-resolves of the same `TypeExpr` (e.g. when both a binding-site
    /// pre-pass and a later fixup — such as `fixup_option_none_for_let` — re-resolve
    /// the annotation) yield only one Info per span.  `RefCell` is required because
    /// `resolve_type_expr_with_aliases` takes `&TypeAliasRegistry`; switching to
    /// `&mut` would cascade through every type-resolution helper.  Mirrors the
    /// interior-mutability pattern in `RealizationLetEnv::in_flight`
    /// (conformance/mod.rs:551).
    emitted_skipped_parametric_prelude_spans: RefCell<HashSet<SourceSpan>>,
}

impl TypeAliasRegistry {
    /// Create an empty registry.
    ///
    /// A `TypeAliasRegistry` is intended for **single-pass use** — one fresh instance per
    /// compile invocation.  The `emitted_skipped_parametric_prelude_spans` dedup set grows
    /// monotonically for the lifetime of the registry; if the registry were ever reused
    /// across multiple compile passes, spans recorded in an earlier pass would silently
    /// suppress `Severity::Info` diagnostics in later passes.  If reuse is ever desired,
    /// clear `emitted_skipped_parametric_prelude_spans` (via a future `reset()` or
    /// `clear_emitted_spans()` helper) before starting the next pass.
    pub(crate) fn new() -> Self {
        TypeAliasRegistry {
            entries: HashMap::new(),
            seeded_names: HashSet::new(),
            skipped_parametric_prelude_names: HashSet::new(),
            emitted_skipped_parametric_prelude_spans: RefCell::new(HashSet::new()),
        }
    }

    /// Record that a parametric prelude alias was skipped during seeding.
    ///
    /// Called by `phase_aliases` for each prelude entry with non-empty `type_params`
    /// that is NOT shadowed by a user-module alias declaration.  Idempotent.
    pub(crate) fn mark_skipped_parametric_prelude(&mut self, name: String) {
        self.skipped_parametric_prelude_names.insert(name);
    }

    /// Return `true` if `name` is a parametric prelude alias that was skipped at seed time.
    ///
    /// Used by `resolve_type_expr_with_aliases` (transitively, via
    /// [`Self::should_emit_skipped_parametric_prelude_info`]) to decide whether to
    /// emit a `Severity::Info` hint when the name fails to resolve — signalling
    /// the cross-module propagation gap rather than leaving the user with only
    /// the generic "unresolved type" Error.
    ///
    /// This method is a pure check with no side effects.  Callers that emit the
    /// `Info` diagnostic and need span-level deduplication (so that a `TypeExpr`
    /// resolved through multiple call sites — e.g. a binding-site pre-pass plus
    /// `fixup_option_none_for_let` — yields exactly one Info per span) MUST use
    /// [`Self::should_emit_skipped_parametric_prelude_info`] instead, which records
    /// the span on first emission.
    pub(crate) fn is_skipped_parametric_prelude(&self, name: &str) -> bool {
        self.skipped_parametric_prelude_names.contains(name)
    }

    /// Decide whether to emit a `Severity::Info` "parametric prelude alias not propagated"
    /// diagnostic for `name` at `span`.  Returns `true` exactly once per `(name, span)`
    /// pair that satisfies "name is a skipped parametric prelude alias"; subsequent
    /// calls with the same span return `false`, providing span-level de-duplication
    /// across the multiple call sites of `resolve_type_expr_with_aliases`.
    ///
    /// Returns `false` (without recording the span) when `name` is not a skipped
    /// parametric prelude alias — non-skipped names cannot pollute the dedup set.
    ///
    /// Has a side effect: records `span` in the emitted-spans set on the first
    /// "true" return.  Uses interior mutability via `RefCell` because callers
    /// hold `&self`.
    pub(crate) fn should_emit_skipped_parametric_prelude_info(
        &self,
        name: &str,
        span: SourceSpan,
    ) -> bool {
        if !self.is_skipped_parametric_prelude(name) {
            return false;
        }
        self.emitted_skipped_parametric_prelude_spans
            .borrow_mut()
            .insert(span)
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

    /// Register an alias entry seeded from a prelude module (not user-declared).
    ///
    /// Like `register`, but marks the entry as prelude-seeded so `into_compiled()`
    /// and `iter()` exclude it from the user module's exported alias set and
    /// content hash.  This mirrors the units pattern: prelude-seeded entries are
    /// available for type resolution (via `lookup`) but are NOT re-exported through
    /// the user module's `type_aliases` field.
    ///
    /// Returns `Err(entry)` if the name is already registered (collision).
    pub(crate) fn register_as_prelude_seed(
        &mut self,
        entry: TypeAliasEntry,
    ) -> Result<(), Box<TypeAliasEntry>> {
        if self.entries.contains_key(&entry.name) {
            Err(Box::new(entry))
        } else {
            self.seeded_names.insert(entry.name.clone());
            self.entries.insert(entry.name.clone(), entry);
            Ok(())
        }
    }

    /// Look up a type alias by name.
    pub(crate) fn lookup(&self, name: &str) -> Option<&TypeAliasEntry> {
        self.entries.get(name)
    }

    /// Iterate over user-declared alias entries (excluding prelude-seeded entries).
    ///
    /// Used by `compute_module_hash` to ensure only user-declared aliases influence
    /// the module's content hash — changes to prelude aliases must not invalidate
    /// the content hash of user modules that don't declare or redeclare them.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &TypeAliasEntry> {
        self.entries
            .values()
            .filter(|e| !self.seeded_names.contains(&e.name))
    }

    /// Consume the registry, returning compiled entries for user-declared aliases only.
    ///
    /// Excludes prelude-seeded entries (registered via `register_as_prelude_seed`) so
    /// the user module's exported `type_aliases` only contains its own declarations —
    /// prelude aliases are visible for resolution but are not re-exported.
    pub(crate) fn into_compiled(self) -> Vec<CompiledTypeAlias> {
        self.entries
            .into_iter()
            .filter(|(name, _)| !self.seeded_names.contains(name))
            .map(|(_, e)| e.into_compiled())
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
    type_expr: &reify_ast::TypeExpr,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    let name = match &type_expr.kind {
        reify_ast::TypeExprKind::Named { name, .. } => name.as_str(),
        reify_ast::TypeExprKind::DimensionalOp { .. } => return None,
        reify_ast::TypeExprKind::IntegerLiteral(_) => return None,
        // Auto type-args (e.g. `auto: Seal`) cannot be resolved to a dimension;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => return None,
        // Qualified assoc-type refs (e.g. `Beam::Material`) cannot be resolved to
        // a dimension here; resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => return None,
    };
    // Scan the shared table (name → dimension direction).
    if let Some((dim, _)) = reify_core::NAMED_DIMENSIONS
        .iter()
        .find(|(_, n)| *n == name)
    {
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
    let candidate_strs: Vec<&str> = reify_core::NAMED_DIMENSIONS
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
    expr: &reify_ast::Expr,
    registry: &UnitRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<f64> {
    match &expr.kind {
        reify_ast::ExprKind::NumberLiteral { value: v, .. } => Some(*v),
        reify_ast::ExprKind::BinOp { op, left, right } => {
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
        reify_ast::ExprKind::UnOp { op, operand } => {
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
        reify_ast::ExprKind::QuantityLiteral { value, unit } => {
            // Route compound unit expressions (Mul/Div/Pow) through resolve_unit_expr,
            // which folds the factor product and dimension vector.  The bare-unit path
            // (UnitExpr::Unit(name)) is left unchanged — it handles affine units and
            // the hardcoded fallback via the existing registry lookup + unit_to_scalar.
            //
            // Dimension is intentionally discarded from Ok((factor, _dim)).  A unit
            // conversion factor is a pure scalar; the declared dimension comes from the
            // unit's `: Dim` annotation, not from the conversion expression.
            //
            // NOTE: there is no cross-check between the folded DimensionVector and the
            // declared `: Dim` annotation.  A declaration like `unit foo : Length = 1mm^2`
            // (compound yields Area, annotation says Length) is accepted silently — the
            // folded `_dim` is simply dropped.  This matches the bare-unit path (which
            // also returns only a scalar factor).  If mismatch validation is ever desired
            // it would live here: compare `_dim` against the dimension resolved from the
            // `: Dim` annotation at the call site in `compile_unit`.
            let unit = match unit {
                reify_ast::UnitExpr::Unit(name) => name,
                compound @ (reify_ast::UnitExpr::Mul(..)
                | reify_ast::UnitExpr::Div(..)
                | reify_ast::UnitExpr::Pow(..)) => {
                    match resolve_unit_expr(compound, registry, expr.span) {
                        Ok((factor, _dim)) => {
                            let si = value * factor;
                            if !si.is_finite() {
                                diagnostics.push(
                                    Diagnostic::error("overflow in unit conversion expression")
                                        .with_label(DiagnosticLabel::new(
                                            expr.span,
                                            "result is not finite",
                                        )),
                                );
                                return None;
                            }
                            return Some(si);
                        }
                        Err(e) => {
                            diagnostics.push(unit_resolve_error_to_diagnostic(&e));
                            return None;
                        }
                    }
                }
            };
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
    decl: &reify_ast::UnitDecl,
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
///
/// Named-dimension lookups delegate to `reify_types::NAMED_DIMENSIONS` (the single source of
/// truth shared with `resolve_dimension_type`). Future named-dimension additions only require a
/// one-line append to that table. `"Dimensionless"` is special-cased here — as in
/// `resolve_dimension_type` — because it is intentionally absent from `NAMED_DIMENSIONS`.
pub(crate) fn resolve_type_name(name: &str) -> Option<Type> {
    match name {
        "Scalar" => Some(Type::length()), // Default scalar is length-dimensioned in M1
        "Solid" => Some(Type::Geometry),  // Surface-syntax alias for the geometry-handle type
        "Geometry" => Some(Type::Geometry), // Canonical surface spelling of the geometry-handle type (Solid is the legacy alias)
        "DatumRef" => Some(Type::Geometry), // datum-reference handle aliases the geometry-handle type (PRD §8 Q1 / task #3116)
        // Topology-selector builtin type names (PRD §4.4 / task 4117 β).
        // Fully-qualified path required: `use super::*` brings the *reify_ir* SelectorKind
        // (Face/Point/Edge) into scope, but Type::Selector requires *reify_core::ty::SelectorKind*
        // (Face/Edge/Body). The two enums share Face/Edge variants but Body vs Point differ.
        "FaceSelector" => Some(Type::Selector(reify_core::ty::SelectorKind::Face)),
        "EdgeSelector" => Some(Type::Selector(reify_core::ty::SelectorKind::Edge)),
        "BodySelector" => Some(Type::Selector(reify_core::ty::SelectorKind::Body)),
        // Kind-agnostic selector param annotation (PRD §4.2/§11.1, task 4369/A2).
        // Bare "Selector" resolves to Type::AnySelector so a param declared as
        // `target : Selector` accepts a Selector value of ANY concrete kind
        // (Face/Edge/Body), while single-kind params (FaceSelector etc.) keep
        // exact-kind checking.  resolve_type_with_aliases inherits this arm
        // automatically since it delegates to resolve_type_name for builtin names.
        "Selector" => Some(Type::AnySelector),
        "Bool" => Some(Type::Bool),
        "Int" => Some(Type::Int),
        "Real" => Some(Type::Real),
        "String" => Some(Type::String),
        // "Dimensionless" is intentionally absent from NAMED_DIMENSIONS (canonical_name returns
        // None for it); mirror the special-case used in resolve_dimension_type.
        "Dimensionless" => Some(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
        // All named-dimension singletons: delegate to the shared NAMED_DIMENSIONS table so
        // future additions require only a one-line change there.
        _ => reify_core::NAMED_DIMENSIONS
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(dim, _)| Type::Scalar { dimension: *dim }),
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

/// Resolve a bare assoc-type name against the in-scope assoc-type map.
///
/// Called from two sites:
/// - `entity.rs`'s first-pass `Param` arm — as a fallback when
///   `resolve_type_expr_with_aliases` returns `None` and the type expression is a
///   bare `Named` (empty `type_args`).
/// - `conformance/checker.rs`'s `check_phase_resolve_structure_members` — the
///   identical fallback in the conformance member-type resolution closure.
///
/// The `assoc_type_scope` maps each declared assoc-type name to its resolved
/// concrete `Type` (structure own-binding wins over trait default; both are
/// collected before the first pass / before `check_phase_resolve_structure_members`
/// is called). The `declared_assoc_names` set contains every assoc-type name
/// declared by conformed traits, used to suppress the `UnresolvedType` cascade for
/// declared-but-unbound required types: returning `Some(Type::Error)` poisons
/// downstream checks with the compiler's standard "error type" sentinel while
/// leaving the single root-cause `TraitAssocTypeNotBound` diagnostic (emitted
/// by conformance) as the sole user-visible error. (task 3973 ιγ)
///
/// Return value:
/// - `Some(ty)`: name is in the scope — use this type.
/// - `Some(Type::Error)`: name is declared in a trait but not bound — poison
///   (anti-cascade: caller must NOT emit `UnresolvedType`).
/// - `None`: name is unrelated to assoc types — caller falls through to its
///   existing unresolved-type handling.
pub(crate) fn resolve_assoc_type_name(
    name: &str,
    assoc_type_scope: &HashMap<String, Type>,
    declared_assoc_names: &HashSet<String>,
) -> Option<Type> {
    if let Some(ty) = assoc_type_scope.get(name) {
        return Some(ty.clone());
    }
    if declared_assoc_names.contains(name) {
        // Declared-but-unbound required associated type: return the poison
        // sentinel so the caller skips `UnresolvedType` and the single
        // `TraitAssocTypeNotBound` from conformance remains the root cause.
        return Some(Type::Error);
    }
    None
}

/// Does conformed trait `trait_name` declare an associated type named `member`?
///
/// Scans the trait's `required_members` for a `RequirementKind::AssocType` with
/// the name, and its `defaults` for a `DefaultKind::AssocType` whose name
/// matches. This is the basis for both ambiguity counting (how many of a
/// structure's conformed traits declare `member`) and disambiguator validation
/// (does the qualifier trait actually declare `member`). (task 3974 ιₑ)
///
/// A trait absent from `trait_registry` answers `false` — it cannot declare the
/// member it does not define.
fn trait_declares_assoc_type(
    trait_registry: &HashMap<String, &CompiledTrait>,
    trait_name: &str,
    member: &str,
) -> bool {
    let Some(compiled) = trait_registry.get(trait_name) else {
        return false;
    };
    compiled
        .required_members
        .iter()
        .any(|r| r.name == member && matches!(r.kind, RequirementKind::AssocType(_)))
        || compiled
            .defaults
            .iter()
            .any(|d| d.name.as_deref() == Some(member) && matches!(d.kind, DefaultKind::AssocType(_)))
}

/// Resolve a qualified associated-type type-expr (`Base::Member`, or the FORK-G
/// paren-disambiguated `Base::(Trait::Member)`) to a concrete [`Type`], reading
/// iota-β's resolved associated-type table off the base structure's compiled
/// [`TopologyTemplate`]. (task 3974 ιₑ)
///
/// Caller-side fallback mirroring [`resolve_assoc_type_name`]: the generic
/// [`resolve_type_expr_with_aliases`] lacks the cross-structure
/// `template_registry` / `trait_registry`, so it keeps returning `None` for
/// `QualifiedAssoc`, and the entity.rs param `None =>` arm — which HAS the
/// registries in scope — calls this helper instead.
///
/// `base` must be a bare `Named` with no type args (the structure name). The
/// resolved `Type` comes from the single `template.assoc_types` entry keyed by
/// `member`: a structure binds each associated-type name exactly once, so every
/// valid trait qualifier resolves to the same `Type` — the qualifier is
/// disambiguation-only (matching the value-side `obj.(Trait::member)`
/// convention, FORK-G). Ambiguity is therefore a property of the trait
/// declarations (`trait_bounds` + `trait_registry`), not of the dedup-by-name
/// table.
///
/// Returns `None` when the access does not resolve; genuine-error cases push a
/// diagnostic (added incrementally across this task's steps), and the caller
/// poisons the param type to a placeholder.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_qualified_assoc_type(
    base: &reify_ast::TypeExpr,
    trait_name: Option<&str>,
    member: &str,
    span: SourceSpan,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    type_param_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    // base must be a bare structure name (`Named` with no type args).
    let reify_ast::TypeExprKind::Named {
        name: base_name,
        type_args,
    } = &base.kind
    else {
        return None;
    };
    if !type_args.is_empty() {
        return None;
    }
    let template = template_registry.get(base_name.as_str())?;

    // The conformed traits of `base` that declare an assoc type named `member`.
    let declaring_traits: Vec<&str> = template
        .trait_bounds
        .iter()
        .filter(|t| trait_declares_assoc_type(trait_registry, t, member))
        .map(String::as_str)
        .collect();

    // Bare access (`Base::Member`): resolve only when exactly one conformed
    // trait declares `member`. Two or more is genuinely ambiguous (the qualifier
    // is required); zero is handled by a later step. The `Base::(Trait::Member)`
    // disambiguator (`trait_name = Some`) is also handled by a later step.
    if trait_name.is_none() {
        match declaring_traits.len() {
            1 => {
                return template
                    .assoc_types
                    .iter()
                    .find(|a| a.type_name == member)
                    .map(|a| a.resolved.clone());
            }
            n if n >= 2 => {
                // Two or more conformed traits declare `member`: the intended
                // declaration is ambiguous. A structure binds each associated-type
                // name once, so the qualifier is disambiguation-only — point the
                // user at the FORK-G paren form `Base::(Trait::Member)`.
                let candidates = declaring_traits.join("`, `");
                diagnostics.push(
                    Diagnostic::error(format!(
                        "ambiguous associated type `{base_name}::{member}`: declared by \
                         traits `{candidates}`; qualify as `{base_name}::(<Trait>::{member})` \
                         to disambiguate"
                    ))
                    .with_code(DiagnosticCode::AmbiguousAssocType)
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!("ambiguous; use `{base_name}::(<Trait>::{member})`"),
                    )),
                );
                return None;
            }
            _ => {}
        }
    }

    let _ = type_param_names;
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
pub(crate) fn resolve_enum_type(name: &str, enum_defs: &[reify_ir::EnumDef]) -> Option<Type> {
    if enum_defs.iter().any(|e| e.name == name) {
        Some(Type::Enum(name.to_string()))
    } else {
        None
    }
}

/// Controls whether [`resolve_type_alias_expr`] propagates inner-arg
/// diagnostics from a failed [`resolve_parameterized_builtin_type`] call or
/// discards them.
///
/// - [`Propagate`][AliasInnerDiagPolicy::Propagate]: non-parametric alias body
///   (e.g. `type Bad = Scalar<NotADim>`); no later instantiation step will
///   re-emit the errors, so they must surface immediately.
/// - [`Defer`][AliasInnerDiagPolicy::Defer]: parametric alias body
///   (e.g. `type Bad<Q> = Scalar<Q>`); the alias body is re-resolved at
///   instantiation time via `resolve_type_alias_expr_with_subst`, which emits
///   inner-arg diagnostics on the substituted body.  See task #2766.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AliasInnerDiagPolicy {
    /// Propagate inner-arg errors — non-parametric alias, no instantiation step.
    Propagate,
    /// Discard inner-arg errors — parametric alias, errors re-emitted at instantiation.
    Defer,
}

/// Propagate-gate helper shared by the two inner-diagnostics check sites in
/// [`resolve_type_alias_expr`]: the builtin-parametric branch (task #2841) and
/// the user-parametric branch (task #2843).
///
/// When `policy` is [`AliasInnerDiagPolicy::Propagate`] and `tmp_diags` is
/// non-empty, the diagnostics are moved into `diagnostics` and `None` is
/// returned; the `?` at each call site then short-circuits the enclosing
/// function with `None`.  Otherwise (Defer policy, or empty tmp_diags), the
/// vector is dropped and `Some(())` is returned so execution continues.
///
/// Ownership of `tmp_diags` is taken because the vector is either consumed via
/// `extend` (Propagate path) or dropped (Defer / empty path) — the caller has
/// no use for it after this point.
fn propagate_inner_diags_if_needed(
    policy: AliasInnerDiagPolicy,
    tmp_diags: Vec<Diagnostic>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<()> {
    if policy == AliasInnerDiagPolicy::Propagate && !tmp_diags.is_empty() {
        diagnostics.extend(tmp_diags);
        None
    } else {
        Some(())
    }
}

/// Resolve a type alias's RHS `TypeExpr` to a `Type`.
///
/// Handles three cases:
/// 1. Simple name → resolved via builtins then alias registry
/// 2. Dimensional binary op (`*`, `/`) → recursively resolve operands to
///    DimensionVectors, combine with mul/div, return `Type::Scalar { dimension }`
/// 3. Unknown → returns None
///
/// When `resolve_parameterized_builtin_type` returns `None` (failed resolution),
/// `inner_diag_policy` controls whether inner-arg diagnostics are surfaced —
/// see [`AliasInnerDiagPolicy`] for the two variants and their rationale.
pub(crate) fn resolve_type_alias_expr(
    type_expr: &reify_ast::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    inner_diag_policy: AliasInnerDiagPolicy,
) -> Option<Type> {
    match &type_expr.kind {
        reify_ast::TypeExprKind::DimensionalOp { op, left, right } => {
            // Dimensional binary operator: left OP right
            let left_dim = resolve_type_alias_expr_to_dimension(left, alias_registry, diagnostics)?;
            let right_dim =
                resolve_type_alias_expr_to_dimension(right, alias_registry, diagnostics)?;
            let result_dim = if matches!(op, reify_ast::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        reify_ast::TypeExprKind::Named { name, type_args } => {
            // Check for parameterized builtin types (List<T>, Set<T>, Map<K,V>,
            // Option<T>, Scalar<Q>, Vector3<Q>, Point3<Q>, Tensor<…>, Matrix<…>).
            // Pass empty structure/trait name sets: this DFS runs before traits and
            // structures are compiled, so trait-name fallback must NOT fire here.
            //
            // Use a temporary diagnostics vector so failed-resolution inner-arg
            // errors can be propagated or discarded per inner_diag_policy —
            // see AliasInnerDiagPolicy for the full rationale.
            if !type_args.is_empty() {
                let mut tmp_diags = Vec::new();
                if let Some(ty) = resolve_parameterized_builtin_type(
                    name,
                    type_args,
                    alias_registry,
                    &mut tmp_diags,
                    &HashSet::new(),
                    &HashSet::new(),
                    &HashSet::new(),
                ) {
                    diagnostics.extend(tmp_diags);
                    return Some(ty);
                }
                // see invariant on resolve_parameterized_builtin_type: matched arms must push a diagnostic on failure (tasks #2841 / #2843).
                propagate_inner_diags_if_needed(inner_diag_policy, tmp_diags, diagnostics)?;
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
                // see AliasInnerDiagPolicy: propagate iff Propagate.
                //
                // When tmp_diags is non-empty, resolve_parameterized_alias matched the
                // user-defined alias but failed to resolve an inner type arg (e.g.
                // `type Bad = Wrapper<NotAType>` emits "unresolved type argument
                // 'NotAType' for alias 'Wrapper'").  Under Propagate (non-parametric
                // callers), surface the errors and return None so the alias entry stays
                // unresolved — falling through to the simple-name lookup below would
                // silently bind to any `resolve_type_name` default and produce a
                // wrong-type cascade at use sites (see task #2843).
                // Under Defer (parametric callers), inner-arg diagnostics about
                // unresolved type params (e.g. `T`) are expected and must be discarded;
                // substitution at use-site instantiation via resolve_type_alias_expr_with_subst
                // will resolve them correctly.
                propagate_inner_diags_if_needed(inner_diag_policy, tmp_diags, diagnostics)?;
                // Defer: silently return None — deferred to instantiation time
            }
            // Simple name: check builtins, then alias registry
            let empty = HashSet::new();
            let empty_structs = HashSet::new();
            let empty_traits = HashSet::new();
            resolve_type_with_aliases(name, &empty, alias_registry, &empty_structs, &empty_traits)
        }
        reify_ast::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            None
        }
        // Auto type-args (e.g. `auto: Seal`) cannot be resolved to a concrete type here;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => None,
        // Qualified assoc-type refs (e.g. `Beam::Material`) cannot be resolved here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => None,
    }
}

/// Helper: resolve a TypeExpr to a DimensionVector (for dimensional algebra).
/// Returns None if the type cannot be resolved to a dimension.
pub(crate) fn resolve_type_alias_expr_to_dimension(
    type_expr: &reify_ast::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    match &type_expr.kind {
        reify_ast::TypeExprKind::DimensionalOp { op, left, right } => {
            let left_dim = resolve_type_alias_expr_to_dimension(left, alias_registry, diagnostics)?;
            let right_dim =
                resolve_type_alias_expr_to_dimension(right, alias_registry, diagnostics)?;
            Some(if matches!(op, reify_ast::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            })
        }
        reify_ast::TypeExprKind::Named { name, .. } => {
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
        reify_ast::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` cannot appear as a dimension type",
                    n
                ))
                .with_label(DiagnosticLabel::new(
                    type_expr.span,
                    "expected a dimension name",
                )),
            );
            None
        }
        // Auto type-args cannot be resolved to a dimension;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => None,
        // Qualified assoc-type refs cannot be resolved to a dimension here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => None,
    }
}

/// Resolve a full TypeExpr at a use site, handling parameterized aliases.
///
/// Falls through: builtins → type params → non-parameterized aliases →
/// parameterized aliases → trait names.
/// Returns None if the type cannot be resolved (caller handles "unresolved" error).
pub(crate) fn resolve_type_expr_with_aliases(
    type_expr: &reify_ast::TypeExpr,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
) -> Option<Type> {
    let (name, type_args) = match &type_expr.kind {
        reify_ast::TypeExprKind::Named { name, type_args } => {
            (name.as_str(), type_args.as_slice())
        }
        reify_ast::TypeExprKind::DimensionalOp { .. } => return None,
        reify_ast::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            return None;
        }
        // Auto type-args cannot be resolved to a concrete type here;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => return None,
        // Qualified assoc-type refs cannot be resolved here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => return None,
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
            type_param_names,
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

    // If the name is a parametric prelude alias that was skipped at seed time,
    // emit a Severity::Info hint so the user sees the cross-module propagation
    // limitation alongside the "unresolved type" Error that the caller will emit.
    // `should_emit_skipped_parametric_prelude_info` records the span on first
    // emit and returns false for any subsequent call on the same span, providing
    // span-level dedup across multiple call sites of resolve_type_expr_with_aliases.
    if alias_registry.should_emit_skipped_parametric_prelude_info(name, type_expr.span) {
        diagnostics.push(
            Diagnostic::info(format!(
                "type '{}' is a parametric prelude alias whose cross-module propagation \
                 is not yet implemented; declare the alias in this module to use it locally",
                name
            ))
            .with_label(DiagnosticLabel::new(
                type_expr.span,
                "parametric prelude alias not propagated",
            )),
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
    type_args: &[reify_ast::TypeExpr],
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
                .with_code(DiagnosticCode::UnresolvedType)
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

/// Substitute resolved type parameters in a `Type` from a name→`Type` map.
///
/// Walks a fully-resolved `Type` and rewrites every `Type::TypeParam(name)`
/// leaf to `subst[name]` when bound, leaving unbound type-params unchanged
/// (passthrough). This is the resolved-`Type`-walk analog of the AST-expr
/// substitution in [`resolve_type_alias_expr_with_subst`] (PRD D3).
///
/// Used at generic-call sites (task 4231 β) to substitute the matched
/// function's return type once `unify` has bound the type parameters from the
/// argument types.
///
/// The `match` is intentionally exhaustive (no `_` wildcard) so that any future
/// `Type` variant forces a compile error here rather than silently passing
/// through unsubstituted — important for a recursive type walk.
pub(crate) fn substitute_type_params(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        // Type-parameter leaf: substitute when bound, else pass through.
        Type::TypeParam(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),

        // Single-inner-Type wrappers: recurse and rebuild.
        Type::List(inner) => Type::List(Box::new(substitute_type_params(inner, subst))),
        Type::Set(inner) => Type::Set(Box::new(substitute_type_params(inner, subst))),
        Type::Keyed(inner) => Type::Keyed(Box::new(substitute_type_params(inner, subst))),
        Type::Option(inner) => Type::Option(Box::new(substitute_type_params(inner, subst))),
        Type::Complex(inner) => Type::Complex(Box::new(substitute_type_params(inner, subst))),
        Type::Range(inner) => Type::Range(Box::new(substitute_type_params(inner, subst))),

        // Two-inner-Type wrappers.
        Type::Map(key, val) => Type::Map(
            Box::new(substitute_type_params(key, subst)),
            Box::new(substitute_type_params(val, subst)),
        ),
        Type::Field { domain, codomain } => Type::Field {
            domain: Box::new(substitute_type_params(domain, subst)),
            codomain: Box::new(substitute_type_params(codomain, subst)),
        },

        // Function: substitute each param + the return type.
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params
                .iter()
                .map(|p| substitute_type_params(p, subst))
                .collect(),
            return_type: Box::new(substitute_type_params(return_type, subst)),
        },

        // Quantity-bearing aggregates: recurse into the quantity slot.
        Type::Point { n, quantity } => Type::Point {
            n: *n,
            quantity: Box::new(substitute_type_params(quantity, subst)),
        },
        Type::Vector { n, quantity } => Type::Vector {
            n: *n,
            quantity: Box::new(substitute_type_params(quantity, subst)),
        },
        Type::Tensor { rank, n, quantity } => Type::Tensor {
            rank: *rank,
            n: *n,
            quantity: Box::new(substitute_type_params(quantity, subst)),
        },
        Type::Matrix { m, n, quantity } => Type::Matrix {
            m: *m,
            n: *n,
            quantity: Box::new(substitute_type_params(quantity, subst)),
        },

        // Union: substitute each arm.
        Type::Union(arms) => Type::Union(
            arms.iter()
                .map(|a| substitute_type_params(a, subst))
                .collect(),
        ),

        // All remaining leaves carry no inner `Type` to substitute.
        Type::Bool
        | Type::Int
        | Type::Real
        | Type::String
        | Type::Scalar { .. }
        | Type::Enum(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_)
        | Type::Geometry
        | Type::Orientation(_)
        | Type::Frame(_)
        | Type::Transform(_)
        | Type::AffineMap(_)
        | Type::Plane
        | Type::Axis
        | Type::BoundingBox
        | Type::Selector(_)
        | Type::AnySelector
        | Type::Error => ty.clone(),
    }
}

/// Resolve a type alias body TypeExpr with parameter substitutions applied.
///
/// Like `resolve_type_alias_expr`, but checks the substitution map first so
/// type parameters in the alias body get replaced with concrete types.
///
/// The `depth` parameter tracks alias expansion depth to prevent stack overflow
/// from recursive parameterized type aliases.
pub(crate) fn resolve_type_alias_expr_with_subst(
    type_expr: &reify_ast::TypeExpr,
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
        reify_ast::TypeExprKind::DimensionalOp { op, left, right } => {
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
            let result_dim = if matches!(op, reify_ast::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            };
            Some(Type::Scalar {
                dimension: result_dim,
            })
        }
        reify_ast::TypeExprKind::Named { name, type_args } => {
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
        reify_ast::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` is only allowed as a type argument of `Tensor` or `Matrix`",
                    n
                ))
                .with_label(DiagnosticLabel::new(type_expr.span, "integer literal not allowed in this position")),
            );
            None
        }
        // Auto type-args cannot be resolved to a concrete type here;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => None,
        // Qualified assoc-type refs cannot be resolved here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => None,
    }
}

/// Resolve a parameterized builtin type constructor (List, Set, Map, Option,
/// Tensor, Matrix, Scalar, Vector3, Point3, Field) within a type alias RHS expression.
///
/// `Field<D, C>` resolves both `D` (domain) and `C` (codomain) via
/// `resolve_type_expr_with_aliases` — the full-type resolver, **not** the
/// dimension-only resolver — because Field's domain and codomain are full Types
/// (Point3, Vector3, Tensor, structures, etc.), not bare dimensions.
///
/// Each type argument is resolved recursively via `resolve_type_expr_with_aliases`,
/// which allows inner type args to be trait names (e.g. `Option<MyTrait>`).
/// `Tensor<rank, n, q>` and `Matrix<m, n, q>` consume two integer-literal args
/// followed by a quantity type; `Scalar<Q>`, `Vector3<Q>`, and `Point3<Q>` each
/// consume one quantity type-expression.
///
/// `structure_names` and `trait_names` are threaded through so that inner args
/// can be resolved to `Type::StructureRef` / `Type::TraitObject` respectively.
/// Pass empty sets when resolving during the alias DFS pre-pass (before traits
/// and structures are compiled).
///
/// ## Invariant
///
/// Every named arm in this function that returns `None` after matching must
/// first push at least one diagnostic into `diagnostics`.  This lets callers
/// distinguish "matched but failed" from "no arm matched" via
/// `tmp_diags.is_empty()`:
///
/// - **`tmp_diags` non-empty** → a named arm matched and failed; surface the
///   diagnostics and propagate `None` so the alias stays unresolved.  Falling
///   through to a subsequent `resolve_type_name` lookup would silently bind the
///   builtin's default type and produce a wrong-type cascade at use sites
///   (see task #2841: `Scalar` default → `Type::length()`).
/// - **`tmp_diags` empty** → no named arm matched (the `_ => return None` arm
///   fired); falling through to the user-parametric alias check is safe because
///   `List`, `Set`, `Map`, `Option`, `Tensor`, `Matrix`, `Vector3`, and `Point3`
///   have no `resolve_type_name` default.
///
/// `Scalar` is the one builtin parametric with a `resolve_type_name` default
/// (`Type::length()`).  It satisfies the invariant because its failure path
/// always routes through `resolve_type_alias_expr_to_dimension`, which pushes a
/// diagnostic before returning `None` — keeping `tmp_diags` non-empty whenever
/// the `Scalar` arm matched and failed (task #2843).
///
/// The `debug_assert!` at the end of this function is forward-looking scaffolding
/// that catches any future arm that synthesises `None` directly without pushing a
/// diagnostic first.
pub(crate) fn resolve_parameterized_builtin_type(
    name: &str,
    type_args: &[reify_ast::TypeExpr],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    type_param_names: &HashSet<String>,
) -> Option<Type> {
    let pre_diag_len = diagnostics.len();
    let result = match name {
        "List" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                type_param_names,
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
                type_param_names,
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
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            let val = resolve_type_expr_with_aliases(
                &type_args[1],
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Map(Box::new(key), Box::new(val)))
        }
        "Keyed" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Keyed(Box::new(inner)))
        }
        "Option" if type_args.len() == 1 => {
            let inner = resolve_type_expr_with_aliases(
                &type_args[0],
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Option(Box::new(inner)))
        }
        "Scalar" if type_args.len() == 1 => {
            // Scalar<Q>: resolve Q to a DimensionVector and wrap.
            let dim =
                resolve_type_alias_expr_to_dimension(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::Scalar { dimension: dim })
        }
        "Vector3" if type_args.len() == 1 => {
            // Vector3<Q>: resolve Q to a DimensionVector and wrap as a 3D vector.
            let dim =
                resolve_type_alias_expr_to_dimension(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::vec3(Type::Scalar { dimension: dim }))
        }
        "Point3" if type_args.len() == 1 => {
            // Point3<Q>: resolve Q to a DimensionVector and wrap as a 3D point.
            let dim =
                resolve_type_alias_expr_to_dimension(&type_args[0], alias_registry, diagnostics)?;
            Some(Type::point3(Type::Scalar { dimension: dim }))
        }
        "Tensor" if type_args.len() == 3 => {
            // Tensor<rank, n, Q>: two integer literals + a quantity type.
            let rank =
                expect_integer_literal_type_arg(&type_args[0], "Tensor", "rank", diagnostics)?;
            let n = expect_integer_literal_type_arg(&type_args[1], "Tensor", "n", diagnostics)?;
            let quantity = resolve_type_expr_with_aliases(
                &type_args[2],
                type_param_names,
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
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::matrix(m, n, quantity))
        }
        "Field" if type_args.len() == 2 => {
            // Field<D, C>: full-type domain and codomain (Point3, Vector3, Tensor, etc.),
            // not bare dimensions. Use resolve_type_expr_with_aliases (full-type resolver)
            // rather than resolve_type_alias_expr_to_dimension. Mirrors Map's two-arg shape.
            let domain = resolve_type_expr_with_aliases(
                &type_args[0],
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            let codomain = resolve_type_expr_with_aliases(
                &type_args[1],
                type_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            )?;
            Some(Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            })
        }
        // Name did not match any known builtin parametric pattern.
        // Early-return here so the debug_assert below never fires for the
        // unmatched case: the assert only needs to hold when a named arm ran.
        _ => return None,
    };
    // Forward-looking scaffolding — see this function's doc comment (## Invariant).
    // Today no arm reaches this assert with result == None (failures short-circuit
    // via `?` or the `_ => return None` arm); this guard catches future arms that
    // synthesise None directly without first pushing a diagnostic.
    debug_assert!(
        result.is_some() || diagnostics.len() > pre_diag_len,
        "resolve_parameterized_builtin_type: arm for '{}' (arity {}) returned None \
         without pushing a diagnostic — add an explicit error before returning None \
         from any matched arm so the caller can infer match-state from diagnostics",
        name,
        type_args.len()
    );
    result
}

/// Pull an unsigned integer out of a type-arg position that requires one
/// (`Tensor<rank, n, Q>`, `Matrix<m, n, Q>`). Emits a diagnostic and returns
/// `None` when the arg is not a `TypeExprKind::IntegerLiteral`.
fn expect_integer_literal_type_arg(
    type_expr: &reify_ast::TypeExpr,
    constructor: &str,
    slot: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<usize> {
    match &type_expr.kind {
        reify_ast::TypeExprKind::IntegerLiteral(n) => Some(*n as usize),
        _ => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "`{}` expects an integer literal for `{}`, found `{}`",
                    constructor, slot, type_expr
                ))
                .with_label(DiagnosticLabel::new(
                    type_expr.span,
                    "expected integer literal",
                )),
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
///
/// Handles: `List<T>`, `Set<T>`, `Map<K,V>`, `Option<T>`, `Scalar<Q>`, `Vector3<Q>`,
/// `Point3<Q>`, `Tensor<rank,n,Q>`, `Matrix<m,n,Q>`, `Field<D,C>`.
///
/// `Field<D, C>` resolves both `D` (domain) and `C` (codomain) via
/// `resolve_type_alias_expr_with_subst` — the full-type resolver with substitutions,
/// **not** the dimension-only resolver — because Field's args are full Types.
pub(crate) fn resolve_parameterized_builtin_type_with_subst(
    name: &str,
    type_args: &[reify_ast::TypeExpr],
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
        "Keyed" if type_args.len() == 1 => {
            let inner = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::Keyed(Box::new(inner)))
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
        "Vector3" if type_args.len() == 1 => {
            // Vector3<Q>: resolve Q (with substitutions) to a DimensionVector and wrap.
            let dim = resolve_type_alias_expr_to_dim_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
            )?;
            Some(Type::vec3(Type::Scalar { dimension: dim }))
        }
        "Point3" if type_args.len() == 1 => {
            // Point3<Q>: resolve Q (with substitutions) to a DimensionVector and wrap.
            let dim = resolve_type_alias_expr_to_dim_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
            )?;
            Some(Type::point3(Type::Scalar { dimension: dim }))
        }
        "Tensor" if type_args.len() == 3 => {
            let rank =
                expect_integer_literal_type_arg(&type_args[0], "Tensor", "rank", diagnostics)?;
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
        "Field" if type_args.len() == 2 => {
            // Field<D, C>: full-type domain and codomain. Mirrors the non-subst variant
            // (resolve_parameterized_builtin_type) but threads `subst` and `depth`.
            let domain = resolve_type_alias_expr_with_subst(
                &type_args[0],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            let codomain = resolve_type_alias_expr_with_subst(
                &type_args[1],
                alias_registry,
                subst,
                diagnostics,
                depth,
            )?;
            Some(Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            })
        }
        _ => None,
    }
}

/// Helper: resolve a TypeExpr to a DimensionVector with parameter substitutions.
pub(crate) fn resolve_type_alias_expr_to_dim_with_subst(
    type_expr: &reify_ast::TypeExpr,
    alias_registry: &TypeAliasRegistry,
    subst: &HashMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DimensionVector> {
    match &type_expr.kind {
        reify_ast::TypeExprKind::DimensionalOp { op, left, right } => {
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
            Some(if matches!(op, reify_ast::DimOp::Mul) {
                left_dim.mul(&right_dim)
            } else {
                left_dim.div(&right_dim)
            })
        }
        reify_ast::TypeExprKind::Named { name, .. } => {
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
        reify_ast::TypeExprKind::IntegerLiteral(n) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "integer literal `{}` cannot appear as a dimension type",
                    n
                ))
                .with_label(DiagnosticLabel::new(
                    type_expr.span,
                    "expected a dimension name",
                )),
            );
            None
        }
        // Auto type-args cannot be resolved to a dimension;
        // resolution semantics are deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => None,
        // Qualified assoc-type refs cannot be resolved to a dimension here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => None,
    }
}

/// Collect all leaf type names referenced in a TypeExpr tree.
/// For `DimensionalOp`, recurses into both operands. For `Named`, returns the name
/// followed by recursed type_args.
pub(crate) fn collect_type_expr_names(type_expr: &reify_ast::TypeExpr) -> Vec<String> {
    match &type_expr.kind {
        reify_ast::TypeExprKind::DimensionalOp { left, right, .. } => {
            collect_type_expr_names(left)
                .into_iter()
                .chain(collect_type_expr_names(right))
                .collect()
        }
        reify_ast::TypeExprKind::Named { name, type_args } => std::iter::once(name.clone())
            .chain(type_args.iter().flat_map(collect_type_expr_names))
            .collect(),
        // Integer-literal type-args contribute no type *names* to dependency graphs.
        reify_ast::TypeExprKind::IntegerLiteral(_) => Vec::new(),
        // Auto type-args contribute the bound name (e.g. `Seal` in `auto: Seal`) so that
        // trait/type-name references are preserved in dependency graphs.
        // Resolution semantics are deferred to task 3477/3558; only the name is surfaced here.
        reify_ast::TypeExprKind::Auto { bound, .. } => vec![bound.clone()],
        // Qualified assoc-type refs contribute the base names (recursed), the member name,
        // and the trait disambiguator name (if present) so that dep-graph edges are preserved.
        // Resolution is deferred to task ιₑ.
        //
        // Note: `member` and `trait_name` are not top-level type names; they are the
        // associated-type member and optional trait-disambiguation identifiers within a
        // `Base::Member` / `Base::(Trait::Member)` path.  They are included here
        // intentionally to ensure forward-compatibility dep-graph edges — the sole current
        // consumer (alias-dependency resolution) filters by `alias_decls.contains_key`, so
        // spurious entries are harmless today.  Task ιₑ will replace this placeholder with
        // proper resolved-assoc-type dep tracking and can narrow the set at that point.
        reify_ast::TypeExprKind::QualifiedAssoc { base, trait_name, member } => {
            let mut names = collect_type_expr_names(base);
            names.push(member.clone());
            if let Some(t) = trait_name {
                names.push(t.clone());
            }
            names
        }
    }
}

/// DFS-resolve a type alias, detecting cycles via a resolving-set.
///
/// - If already in the registry → skip (already resolved).
/// - If in the resolving set → emit circular error, register with None.
/// - Otherwise: resolve dependencies first, then resolve this alias.
pub(crate) fn resolve_alias_dfs(
    name: &str,
    alias_decls: &HashMap<String, &reify_ast::TypeAliasDecl>,
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

    // Now resolve this alias (dependencies should be in the registry).
    // Use Propagate for non-parametric aliases so inner-arg errors surface
    // immediately; Defer for parametric aliases where the body re-resolves
    // at instantiation time.
    let inner_diag_policy = if decl.type_params.is_empty() {
        AliasInnerDiagPolicy::Propagate
    } else {
        AliasInnerDiagPolicy::Defer
    };
    let resolved = resolve_type_alias_expr(
        &decl.type_expr,
        alias_registry,
        diagnostics,
        inner_diag_policy,
    );
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
    decls: &[reify_ast::TypeParamDecl],
) -> Vec<reify_ir::TypeParam> {
    decls
        .iter()
        .map(|d| {
            let bounds = d
                .bounds
                .iter()
                .map(|b| reify_ir::TraitBound {
                    trait_ref: reify_ir::TraitRef {
                        name: b.clone(),
                        type_args: vec![],
                    },
                })
                .collect();
            // Resolve defaults: try builtin types first, then preserve
            // structure names as StructureRef (concrete names, not type variables).
            // DimensionalOp/IntegerLiteral/Auto cannot appear as type-parameter defaults —
            // the grammar restricts those to type_arg_list slots, so those arms are unreachable.
            // QualifiedAssoc defaults (e.g. `T = Beam::Material`) are valid grammar but
            // resolution to a concrete Type is deferred to task ιₑ; they produce None here.
            let default = d.default.as_ref().and_then(|te| match &te.kind {
                reify_ast::TypeExprKind::Named { name, .. } => {
                    Some(resolve_type_name(name).unwrap_or_else(|| Type::StructureRef(name.clone())))
                }
                reify_ast::TypeExprKind::DimensionalOp { .. } => {
                    unreachable!(
                        "dimensional operator cannot appear as a type-parameter default; \
                             the grammar restricts dimensional operators to type_arg_list slots"
                    )
                }
                reify_ast::TypeExprKind::IntegerLiteral(_) => {
                    unreachable!(
                        "integer literal cannot appear as a type-parameter default; \
                             the grammar restricts integer literals to type_arg_list slots"
                    )
                }
                reify_ast::TypeExprKind::Auto { .. } => {
                    unreachable!(
                        "auto type-arg cannot appear as a type-parameter default; \
                             the grammar restricts auto_type_arg to type_arg_list slots"
                    )
                }
                // QualifiedAssoc defaults (e.g. `structure def Foo<T = Beam::Material>`) are
                // valid grammar; resolution to a concrete Type is deferred to task ιₑ.
                reify_ast::TypeExprKind::QualifiedAssoc { .. } => None,
            });
            reify_ir::TypeParam {
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
    fn named_type_expr(name: &str) -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: name.to_string(),
                type_args: vec![],
            },
            span: reify_core::SourceSpan::new(0, 0),
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
    fn resolve_type_name_recognises_acceleration() {
        assert_eq!(
            resolve_type_name("Acceleration"),
            Some(Type::Scalar {
                dimension: DimensionVector::ACCELERATION
            })
        );
    }

    #[test]
    fn resolve_type_name_recognises_force_density() {
        assert_eq!(
            resolve_type_name("ForceDensity"),
            Some(Type::Scalar {
                dimension: DimensionVector::FORCE_DENSITY
            })
        );
    }

    #[test]
    fn resolve_dimension_type_recognises_money() {
        let te = named_type_expr("Money");
        let mut diagnostics = Vec::new();
        let result = resolve_dimension_type(&te, &mut diagnostics);
        assert_eq!(result, Some(DimensionVector::MONEY));
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostic: {:?}",
            diagnostics
        );
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
        for &(dim, name) in reify_core::NAMED_DIMENSIONS {
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

    /// Parity contract: `resolve_type_name` correctly maps every entry in
    /// `reify_types::NAMED_DIMENSIONS` and the special-cased `"Dimensionless"`.
    ///
    /// This test is written BEFORE the implementation is switched to use the shared table
    /// (step 2), so it serves as a regression-protection contract that will catch any
    /// silent divergence between the old match-arm implementation and the new table-driven
    /// one. It is expected to pass against both implementations.
    #[test]
    fn resolve_type_name_round_trips_all_named_dimensions() {
        for &(dim, name) in reify_core::NAMED_DIMENSIONS {
            assert_eq!(
                resolve_type_name(name),
                Some(Type::Scalar { dimension: dim }),
                "resolve_type_name({:?}) should return Some(Type::Scalar {{ dimension: {:?} }})",
                name,
                dim,
            );
        }
        // Special-case fallback: "Dimensionless" is intentionally absent from NAMED_DIMENSIONS
        // but must still resolve to Type::Scalar { dimension: DIMENSIONLESS }.
        assert_eq!(
            resolve_type_name("Dimensionless"),
            Some(Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS
            }),
            "resolve_type_name(\"Dimensionless\") should return Some(Type::Scalar {{ dimension: DIMENSIONLESS }})"
        );
        // Negative case: an unknown name must return None (default arm does not over-match).
        assert_eq!(
            resolve_type_name("ThisIsNotADimension"),
            None,
            "resolve_type_name(\"ThisIsNotADimension\") should return None"
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
    fn geometry_resolves_to_geometry() {
        assert_eq!(
            resolve_type_name("Geometry"),
            Some(Type::Geometry),
            "\"Geometry\" should resolve to Type::Geometry as the canonical surface spelling"
        );
    }

    #[test]
    fn resolve_enum_type_returns_some_for_matching_name() {
        let enum_defs = vec![reify_ir::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
            doc: None,
        }];
        assert_eq!(
            resolve_enum_type("Direction", &enum_defs),
            Some(Type::Enum("Direction".to_string())),
        );
    }

    #[test]
    fn resolve_enum_type_returns_none_for_non_matching_name() {
        let enum_defs = vec![reify_ir::EnumDef {
            name: "Direction".to_string(),
            variants: vec!["In".to_string(), "Out".to_string()],
            doc: None,
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

        let expected_names: std::collections::HashSet<&str> = reify_core::NAMED_DIMENSIONS
            .iter()
            .map(|(_, n)| *n)
            .chain(std::iter::once("Dimensionless"))
            .collect();

        assert_eq!(
            listed_names, expected_names,
            "diagnostic.candidates does not exactly match NAMED_DIMENSIONS + Dimensionless"
        );
    }

    // ── Selector type-name resolution (task 4117 / β) ──────────────────────────

    /// `resolve_type_name("FaceSelector")` must return `Type::Selector(Face)`.
    ///
    /// RED until step-2 adds the arm.
    #[test]
    fn resolve_type_name_recognises_face_selector() {
        assert_eq!(
            resolve_type_name("FaceSelector"),
            Some(Type::Selector(reify_core::ty::SelectorKind::Face)),
            "\"FaceSelector\" should resolve to Type::Selector(Face)"
        );
    }

    /// `resolve_type_name("EdgeSelector")` must return `Type::Selector(Edge)`.
    ///
    /// RED until step-2 adds the arm.
    #[test]
    fn resolve_type_name_recognises_edge_selector() {
        assert_eq!(
            resolve_type_name("EdgeSelector"),
            Some(Type::Selector(reify_core::ty::SelectorKind::Edge)),
            "\"EdgeSelector\" should resolve to Type::Selector(Edge)"
        );
    }

    /// `resolve_type_name("BodySelector")` must return `Type::Selector(Body)`.
    ///
    /// RED until step-2 adds the arm.
    #[test]
    fn resolve_type_name_recognises_body_selector() {
        assert_eq!(
            resolve_type_name("BodySelector"),
            Some(Type::Selector(reify_core::ty::SelectorKind::Body)),
            "\"BodySelector\" should resolve to Type::Selector(Body)"
        );
    }

    /// `resolve_type_with_aliases` must inherit the builtin selector arms so that
    /// param-annotation resolution (which calls this function) resolves selector
    /// type names without an alias entry.
    ///
    /// RED until step-2 adds the arm to `resolve_type_name`.
    #[test]
    fn resolve_type_with_aliases_inherits_face_selector() {
        let reg = TypeAliasRegistry::new();
        let result = resolve_type_with_aliases(
            "FaceSelector",
            &HashSet::new(),
            &reg,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            result,
            Some(Type::Selector(reify_core::ty::SelectorKind::Face)),
            "resolve_type_with_aliases(\"FaceSelector\", …) should return Type::Selector(Face)"
        );
    }

    /// `resolve_type_with_aliases` must inherit the Edge selector arm from the
    /// builtin resolver so param-annotation resolution works for EdgeSelector.
    #[test]
    fn resolve_type_with_aliases_inherits_edge_selector() {
        let reg = TypeAliasRegistry::new();
        let result = resolve_type_with_aliases(
            "EdgeSelector",
            &HashSet::new(),
            &reg,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            result,
            Some(Type::Selector(reify_core::ty::SelectorKind::Edge)),
            "resolve_type_with_aliases(\"EdgeSelector\", …) should return Type::Selector(Edge)"
        );
    }

    /// `resolve_type_with_aliases` must inherit the Body selector arm from the
    /// builtin resolver so param-annotation resolution works for BodySelector.
    #[test]
    fn resolve_type_with_aliases_inherits_body_selector() {
        let reg = TypeAliasRegistry::new();
        let result = resolve_type_with_aliases(
            "BodySelector",
            &HashSet::new(),
            &reg,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            result,
            Some(Type::Selector(reify_core::ty::SelectorKind::Body)),
            "resolve_type_with_aliases(\"BodySelector\", …) should return Type::Selector(Body)"
        );
    }

    // ── AnySelector type-name resolution (task 4369 / A2) ────────────────────
    //
    // The bare `Selector` spelling (no kind qualifier) must resolve to
    // `Type::AnySelector` so that param annotations like `target : Selector`
    // accept any concrete selector kind at the type-compat level.
    //
    // Tests (a) and (b) are RED until step-2 adds the resolver arm.
    // Test (c) is GREEN from pre-1's Display arm (documents the
    // resolver<->Display round-trip contract).

    /// (a) `resolve_type_name("Selector")` must return `Type::AnySelector`.
    ///
    /// RED until step-2 adds `"Selector" => Some(Type::AnySelector)` to
    /// `resolve_type_name`.
    #[test]
    fn resolve_type_name_recognises_any_selector() {
        assert_eq!(
            resolve_type_name("Selector"),
            Some(Type::AnySelector),
            "\"Selector\" should resolve to Type::AnySelector"
        );
    }

    /// (b) `resolve_type_with_aliases("Selector", …)` must return
    /// `Type::AnySelector` — it inherits the builtin arm automatically.
    ///
    /// RED until step-2 adds the arm to `resolve_type_name`.
    #[test]
    fn resolve_type_with_aliases_inherits_any_selector() {
        let reg = TypeAliasRegistry::new();
        let result = resolve_type_with_aliases(
            "Selector",
            &HashSet::new(),
            &reg,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            result,
            Some(Type::AnySelector),
            "resolve_type_with_aliases(\"Selector\", …) should return Type::AnySelector"
        );
    }

    /// (c) Display round-trip: `Type::AnySelector` formats as `"Selector"`,
    /// which is the same spelling the resolver accepts (task 4369/A2 §11.1).
    ///
    /// GREEN from pre-1's Display arm.
    #[test]
    fn any_selector_display_matches_resolver_spelling() {
        assert_eq!(
            format!("{}", Type::AnySelector),
            "Selector",
            "Type::AnySelector should display as \"Selector\" to match the resolver spelling"
        );
    }

    // ── Keyed<T> parameterized resolution (step-3 RED / task 3930 β) ──────────
    // `Keyed<Vent>` must resolve to the keyed-collection kind, distinct from the
    // `Map`/`List` resolutions of the same arg. Mirrors the List/Map resolver arms.

    #[test]
    fn resolve_parameterized_builtin_type_resolves_keyed_distinct_from_map_list() {
        let reg = TypeAliasRegistry::new();
        let structure_names: HashSet<String> = ["Vent".to_string()].into_iter().collect();
        let trait_names = HashSet::new();
        let args = [named_type_expr("Vent")];

        // Keyed<Vent> → Type::Keyed(StructureRef(Vent)), no diagnostics.
        let mut diags = Vec::new();
        let keyed = resolve_parameterized_builtin_type(
            "Keyed",
            &args,
            &reg,
            &mut diags,
            &structure_names,
            &trait_names,
            &HashSet::new(),
        );
        assert_eq!(
            keyed,
            Some(Type::Keyed(Box::new(Type::StructureRef("Vent".into())))),
            "Keyed<Vent> should resolve to the keyed-collection kind",
        );
        assert!(diags.is_empty(), "no diagnostics expected; got {:?}", diags);

        // List<Vent> resolves to the list kind — distinct from Keyed.
        let mut list_diags = Vec::new();
        let list = resolve_parameterized_builtin_type(
            "List",
            &args,
            &reg,
            &mut list_diags,
            &structure_names,
            &trait_names,
            &HashSet::new(),
        );
        assert_eq!(
            list,
            Some(Type::List(Box::new(Type::StructureRef("Vent".into())))),
        );
        assert_ne!(keyed, list, "Keyed<Vent> must be distinct from List<Vent>");

        // Distinct from a Map kind as well.
        assert_ne!(
            keyed,
            Some(Type::Map(
                Box::new(Type::String),
                Box::new(Type::StructureRef("Vent".into())),
            )),
        );
    }

    #[test]
    fn resolve_parameterized_builtin_type_with_subst_resolves_keyed_distinct_from_list() {
        let reg = TypeAliasRegistry::new();
        // The subst path is structure-name-blind by design (alias DFS runs before
        // structures are compiled — see the hardcoded empty `structure_names` in
        // `resolve_type_alias_expr_with_subst`). The inner type arg is therefore
        // supplied through the substitution map, which is exactly what this
        // resolver variant exists to exercise: `Keyed<T>` with `T := Vent`.
        let mut subst = HashMap::new();
        subst.insert("T".to_string(), Type::StructureRef("Vent".into()));
        let args = [named_type_expr("T")];

        let mut diags = Vec::new();
        let keyed = resolve_parameterized_builtin_type_with_subst(
            "Keyed",
            &args,
            &reg,
            &subst,
            &mut diags,
            0,
        );
        assert_eq!(
            keyed,
            Some(Type::Keyed(Box::new(Type::StructureRef("Vent".into())))),
            "Keyed<T>[T:=Vent] (subst path) should resolve to the keyed-collection kind",
        );
        assert!(diags.is_empty(), "no diagnostics expected; got {:?}", diags);

        let mut list_diags = Vec::new();
        let list = resolve_parameterized_builtin_type_with_subst(
            "List",
            &args,
            &reg,
            &subst,
            &mut list_diags,
            0,
        );
        assert_eq!(
            list,
            Some(Type::List(Box::new(Type::StructureRef("Vent".into())))),
        );
        assert_ne!(
            keyed, list,
            "Keyed<Vent> must be distinct from List<Vent> (subst path)",
        );
    }

    // Type resolution is position-blind: `Keyed<T>` resolves to a well-formed
    // `Type::Keyed` regardless of whether it appears in a `sub` position (its only
    // intended use) or a value position such as `param x : Keyed<Vent>`. The
    // resolver therefore emits NO diagnostic for the latter — rejecting a
    // value-position `Keyed<T>` with a clear compile error is deferred to γ/δ
    // (access-by-key resolution + structural classification own that guard).
    // Until then, `reify_eval::is_representable_cell_type` returning `false` for
    // `Type::Keyed` (engine_eval.rs, test `is_representable_cell_type_rejects_keyed`)
    // is the eval-layer backstop. This test pins the position-blindness so the
    // deferral is explicit and a future γ/δ guard has a documented anchor.
    #[test]
    fn resolve_parameterized_keyed_is_position_blind_value_guard_deferred() {
        let reg = TypeAliasRegistry::new();
        let structure_names: HashSet<String> = ["Vent".to_string()].into_iter().collect();
        let trait_names = HashSet::new();
        let args = [named_type_expr("Vent")];

        let mut diags = Vec::new();
        let keyed = resolve_parameterized_builtin_type(
            "Keyed",
            &args,
            &reg,
            &mut diags,
            &structure_names,
            &trait_names,
            &HashSet::new(),
        );
        assert_eq!(
            keyed,
            Some(Type::Keyed(Box::new(Type::StructureRef("Vent".into())))),
            "Keyed<Vent> resolves structurally even in a value position",
        );
        assert!(
            diags.is_empty(),
            "type resolution is position-blind: no value-position diagnostic is emitted \
             here (the guard is deferred to γ/δ); got {:?}",
            diags,
        );
    }

    #[test]
    fn should_emit_skipped_parametric_prelude_info_dedups_per_span() {
        let mut reg = TypeAliasRegistry::new();
        reg.mark_skipped_parametric_prelude("Vec".to_string());

        let span_a = reify_core::SourceSpan::new(10, 20);
        let span_b = reify_core::SourceSpan::new(30, 40);

        // First call with span_a → true (newly inserted).
        assert!(
            reg.should_emit_skipped_parametric_prelude_info("Vec", span_a),
            "first call on span_a should return true"
        );

        // Second call with the same span_a → false (already emitted on this span).
        assert!(
            !reg.should_emit_skipped_parametric_prelude_info("Vec", span_a),
            "second call on span_a should return false (per-span dedup)"
        );

        // Different span_b → true (dedup is per-span, not per-name).
        assert!(
            reg.should_emit_skipped_parametric_prelude_info("Vec", span_b),
            "first call on span_b should return true even though 'Vec' was already emitted on span_a"
        );

        // Name not in skipped set → false regardless of span.
        assert!(
            !reg.should_emit_skipped_parametric_prelude_info("NotSkipped", span_a),
            "non-skipped name should return false"
        );

        // Non-skipped names must NOT pollute the emitted-spans set: a fresh span
        // (50..60) passed for "NotSkipped" must not prevent "Vec" from emitting
        // on that same span.
        let span_c = reify_core::SourceSpan::new(50, 60);
        assert!(
            !reg.should_emit_skipped_parametric_prelude_info("NotSkipped", span_c),
            "non-skipped name on span_c returns false"
        );
        assert!(
            reg.should_emit_skipped_parametric_prelude_info("Vec", span_c),
            "Vec on span_c should return true — non-skipped name must not pollute emitted-spans set"
        );
    }

    /// `structure def Foo<T = Beam::Material>` parses to a `QualifiedAssoc` default.
    /// `convert_type_params` must defer gracefully (default = None) rather than
    /// panicking — resolution is deferred to task ιₑ.
    #[test]
    fn convert_type_params_qualified_assoc_default_defers_to_none() {
        let span = reify_core::SourceSpan::new(0, 0);
        let decl = reify_ast::TypeParamDecl {
            name: "T".into(),
            bounds: vec![],
            default: Some(reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::QualifiedAssoc {
                    base: Box::new(named_type_expr("Beam")),
                    trait_name: None,
                    member: "Material".into(),
                },
                span,
            }),
            span,
        };
        let result = convert_type_params(&[decl]);
        assert_eq!(result.len(), 1, "expected one TypeParam");
        assert_eq!(result[0].name, "T");
        assert_eq!(
            result[0].default, None,
            "QualifiedAssoc default must be deferred (None) until task ιₑ resolves it"
        );
    }

    // ── DatumRef resolver (task #3116) ────────────────────────────────────────

    /// Regression: `resolve_type_name("Geometry")` must return `Some(Type::Geometry)`.
    /// Already passes — used as an anchor alongside the new DatumRef test.
    #[test]
    fn resolve_type_name_recognises_geometry() {
        assert_eq!(
            resolve_type_name("Geometry"),
            Some(Type::Geometry),
            "\"Geometry\" should resolve to Type::Geometry"
        );
    }

    /// Regression: `resolve_type_name("Solid")` must return `Some(Type::Geometry)` (legacy alias).
    /// Already passes — used as an anchor alongside the new DatumRef test.
    #[test]
    fn resolve_type_name_recognises_solid_as_geometry_alias() {
        assert_eq!(
            resolve_type_name("Solid"),
            Some(Type::Geometry),
            "\"Solid\" should resolve to Type::Geometry (legacy alias)"
        );
    }

    /// RED (step-1): `resolve_type_name("DatumRef")` must return `Some(Type::Geometry)`.
    ///
    /// `DatumRef` is the datum-reference handle type used in `tolerancing.ri`.
    /// It aliases the existing geometry-handle type (PRD §8 Q1 / task #3116 D2).
    /// Fails before step-2 adds `"DatumRef" => Some(Type::Geometry)` to `resolve_type_name`.
    #[test]
    fn resolve_type_name_recognises_datum_ref_as_geometry_handle() {
        assert_eq!(
            resolve_type_name("DatumRef"),
            Some(Type::Geometry),
            "\"DatumRef\" should resolve to Type::Geometry (datum-reference handle aliases the geometry-handle type)"
        );
    }

    // ── task 4231 β: substitute_type_params (return-type substitution) ──────
    //
    // Pure-function unit tests for the resolved-`Type`-walk that rewrites
    // `Type::TypeParam` leaves from a name→Type substitution map (PRD D3).

    /// Build a substitution map from (name, Type) pairs.
    fn subst_of(pairs: &[(&str, Type)]) -> HashMap<String, Type> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn substitute_bare_type_param_bound() {
        // (a) bare TypeParam("T") with {T: Real} → Real.
        let subst = subst_of(&[("T", Type::Real)]);
        assert_eq!(
            substitute_type_params(&Type::TypeParam("T".to_string()), &subst),
            Type::Real
        );
    }

    #[test]
    fn substitute_unbound_type_param_passthrough() {
        // (b) unbound TypeParam("D") with {C: Real} → TypeParam("D") unchanged.
        let subst = subst_of(&[("C", Type::Real)]);
        assert_eq!(
            substitute_type_params(&Type::TypeParam("D".to_string()), &subst),
            Type::TypeParam("D".to_string())
        );
    }

    #[test]
    fn substitute_inside_list() {
        // (c) List(TypeParam("T")) with {T: Int} → List(Int).
        let subst = subst_of(&[("T", Type::Int)]);
        assert_eq!(
            substitute_type_params(
                &Type::List(Box::new(Type::TypeParam("T".to_string()))),
                &subst
            ),
            Type::List(Box::new(Type::Int))
        );
    }

    #[test]
    fn substitute_field_partial() {
        // (d) Field{domain: TypeParam("D"), codomain: TypeParam("C")} with
        //     {C: Real} → Field{domain: TypeParam("D"), codomain: Real}.
        //     D stays unbound (nested partial substitution).
        let subst = subst_of(&[("C", Type::Real)]);
        assert_eq!(
            substitute_type_params(
                &Type::Field {
                    domain: Box::new(Type::TypeParam("D".to_string())),
                    codomain: Box::new(Type::TypeParam("C".to_string())),
                },
                &subst
            ),
            Type::Field {
                domain: Box::new(Type::TypeParam("D".to_string())),
                codomain: Box::new(Type::Real),
            }
        );
    }

    #[test]
    fn substitute_map_both_bound() {
        // (e) Map(TypeParam("K"), TypeParam("V")) both bound → Map of concretes.
        let subst = subst_of(&[("K", Type::String), ("V", Type::Int)]);
        assert_eq!(
            substitute_type_params(
                &Type::Map(
                    Box::new(Type::TypeParam("K".to_string())),
                    Box::new(Type::TypeParam("V".to_string())),
                ),
                &subst
            ),
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn substitute_function_params_and_return() {
        // (f) Function{params:[TypeParam("T")], return_type: List(TypeParam("T"))}
        //     with {T: Real} → both positions substituted.
        let subst = subst_of(&[("T", Type::Real)]);
        assert_eq!(
            substitute_type_params(
                &Type::Function {
                    params: vec![Type::TypeParam("T".to_string())],
                    return_type: Box::new(Type::List(Box::new(Type::TypeParam("T".to_string())))),
                },
                &subst
            ),
            Type::Function {
                params: vec![Type::Real],
                return_type: Box::new(Type::List(Box::new(Type::Real))),
            }
        );
    }

    #[test]
    fn substitute_recurses_into_quantity() {
        // (g) Tensor{rank, n, quantity: TypeParam("Q")} recurses into quantity.
        let subst = subst_of(&[("Q", Type::length())]);
        assert_eq!(
            substitute_type_params(
                &Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(Type::TypeParam("Q".to_string())),
                },
                &subst
            ),
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::length()),
            }
        );
    }

    #[test]
    fn substitute_non_typeparam_leaf_identity() {
        // (h) non-typeparam leaf (Int) with empty subst → identity.
        let subst = subst_of(&[]);
        assert_eq!(substitute_type_params(&Type::Int, &subst), Type::Int);
    }
}
