use super::*;

use crate::types::TopologyTemplate;

/// Push the appropriate unresolved-type diagnostic for a fn signature position.
///
/// Generic fns (non-empty `type_param_names`) emit `FnUnknownTypeParam` with a message that
/// names the generic function and clarifies the "not a declared type parameter or a known type"
/// interpretation.  Non-generic fns keep `UnresolvedType` + the legacy `"unresolved <prefix>:
/// <expr>"` message bit-for-bit (INV-6 regression pin).
///
/// `non_generic_prefix` is either `"unresolved type"` (param position) or
/// `"unresolved return type"` (return-type position); both compile_function and
/// compile_assoc_function use this helper so a future message-wording change is
/// made in exactly one place.
fn push_signature_type_error(
    diagnostics: &mut Vec<Diagnostic>,
    type_param_names: &HashSet<String>,
    type_expr: impl std::fmt::Display,
    span: reify_core::SourceSpan,
    fn_name: &str,
    non_generic_prefix: &str,
) {
    if !type_param_names.is_empty() {
        diagnostics.push(
            Diagnostic::error(format!(
                "type '{}' in the signature of generic function '{}' is not a declared type parameter or a known type",
                type_expr, fn_name
            ))
            .with_code(DiagnosticCode::FnUnknownTypeParam)
            .with_label(DiagnosticLabel::new(span, "unknown type name")),
        );
    } else {
        diagnostics.push(
            Diagnostic::error(format!("{}: {}", non_generic_prefix, type_expr))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(span, "unknown type name")),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_function(
    fn_def: &reify_ast::FnDef,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    prelude_template_registry: Option<&HashMap<String, &TopologyTemplate>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    // Build the set of declared type-parameter names so `resolve_type_expr_with_aliases`
    // can map a bare `T` → `Type::TypeParam("T")`. Mirror of entity.rs:560-563.
    let type_param_names: HashSet<String> = fn_def
        .type_params
        .iter()
        .map(|tp| tp.name.clone())
        .collect();
    // Build the subset of dimension-kinded type params (those declared with `Q: Dimension`).
    // Kept alongside `type_param_names` so the FnUnknownTypeParam gate + INV-6/INV-10
    // back-compat are bit-for-bit unchanged; the kinded resolver consults this set
    // first in the Scalar/Vector3/Point3 arms and the bare-name path (task ε).
    let dim_param_names: HashSet<String> = fn_def
        .type_params
        .iter()
        .filter(|tp| tp.bounds.iter().any(|b| b == "Dimension"))
        .map(|tp| tp.name.clone())
        .collect();
    // Resolve parameter types.
    //
    // `param_type_resolved[i]` is `true` when the i-th param's declared type resolved
    // successfully. It is used below to gate the default-type check: if the type failed
    // to resolve, the root-cause "unresolved type" diagnostic is already queued and
    // emitting a secondary FnParamDefaultTypeMismatch (against the `Type::dimensionless_scalar()` fallback)
    // would be confusing noise.
    let mut params: Vec<(String, Type)> = Vec::new();
    let mut param_type_resolved: Vec<bool> = Vec::new();
    for p in &fn_def.params {
        let (ty, resolved) = match resolve_type_expr_with_aliases_kinded(
            &p.type_expr,
            &type_param_names,
            &dim_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        ) {
            Some(t) => (t, true),
            None => {
                push_signature_type_error(
                    diagnostics,
                    &type_param_names,
                    &p.type_expr,
                    p.type_expr.span,
                    &fn_def.name,
                    "unresolved type",
                );
                (Type::Error, false) // poison; `resolved` flag still prevents the default-type cascade
            }
        };
        params.push((p.name.clone(), ty));
        param_type_resolved.push(resolved);
    }

    // Compile default expressions in a neutral scope (no params registered) so
    // defaults cannot reference sibling params and cannot recurse into the
    // enclosing function — definition-time semantics. Rationale: keeps defaults
    // pure-by-construction and order-independent.
    // See `CompiledFunction::param_defaults` in `reify-types/src/expr.rs` for the
    // field-level doc and `docs/initial-design/name-resolution-and-scoping-design-decisions.md`
    // §2.3 for the full language-design rationale.
    //
    // Thread the prelude template registry through so that defaults written as
    // `FieldFoo()` (a prelude structure-def constructor) lower to
    // CompiledExprKind::StructureInstanceCtor rather than a generic
    // FunctionCall (esc-3851-32). Same wiring applied to the body scope below.
    let mut neutral_scope = CompilationScope::new(&fn_def.name);
    if let Some(reg) = prelude_template_registry {
        neutral_scope.set_template_registry(reg);
    }
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
    // Uses strict equality (via `fn_param_default_compatible`). The definition-
    // site default-expression-vs-param-type check is strict; `try_default_padding`'s
    // PREFIX check (provided args vs leading params) uses the same trait/type-param
    // wildcard predicate as `resolve_function_overload` and is NOT strict equality.
    // A default value is conceptually inserted at the padded call site, so the
    // definition-site check must be at least as strict as the call-site check.
    //
    // The zip over three lockstep collections (fn_def.params, param_defaults, params)
    // makes index alignment structurally obvious: all three are built from the same
    // fn_def.params slice so they have identical length and ordering.
    //
    // The `type_ok` gate skips params whose declared type failed to resolve. The
    // root-cause "unresolved type" diagnostic is already queued; emitting a
    // secondary FnParamDefaultTypeMismatch (against the Type::dimensionless_scalar() fallback) would
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
            match resolve_type_expr_with_aliases_kinded(
                te,
                &type_param_names,
                &dim_param_names,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            ) {
                Some(t) => t,
                None => {
                    push_signature_type_error(
                        diagnostics,
                        &type_param_names,
                        te,
                        te.span,
                        &fn_def.name,
                        "unresolved return type",
                    );
                    Type::Error
                }
            }
        }
        None => Type::dimensionless_scalar(), // default return type
    };

    // Create a scope with function params registered.
    //
    // The template registry (passed from `phase_functions`) is threaded
    // through here so that a structure-def referenced in the fn body via
    // constructor syntax — e.g. `Widget()` — lowers to
    // `CompiledExprKind::StructureInstanceCtor` rather than a generic
    // `FunctionCall`.  The registry now includes both (a) prelude structure_defs
    // (esc-3851-32) and (b) same-module structure_defs via skeleton templates
    // built by `build_structure_def_skeleton` in `phase_functions` (task 3895).
    let mut scope = CompilationScope::new(&fn_def.name);
    if let Some(reg) = prelude_template_registry {
        scope.set_template_registry(reg);
    }
    for (name, ty) in &params {
        scope.register(name, ty.clone());
    }

    // Compile body let bindings — bodyless trait fns (body = None) are not compiled
    // here; they are deferred to task δ/ζ. Top-level Declaration::Function always
    // has Some body, so the defensive guard is effectively unreachable for them.
    let body = match &fn_def.body {
        Some(b) => b,
        None => {
            diagnostics.push(
                reify_core::Diagnostic::error(
                    "internal compiler error: compile_function called on a bodyless \
                     trait fn (body = None); this should not be reached until task δ/ζ"
                        .to_string(),
                )
                .with_label(reify_core::DiagnosticLabel::new(
                    fn_def.span,
                    "bodyless fn".to_string(),
                )),
            );
            return None;
        }
    };
    let mut compiled_lets = Vec::new();
    for let_decl in &body.let_bindings {
        let compiled_expr =
            compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
        let let_type = compiled_expr.result_type.clone();
        // Register the let binding in scope for subsequent bindings
        scope.register(&let_decl.name, let_type);
        compiled_lets.push((let_decl.name.clone(), compiled_expr));
    }

    // Compile result expression
    let result_expr = compile_expr(
        &body.result_expr,
        &scope,
        enum_defs,
        functions,
        diagnostics,
    );

    // Compute content hash — fold in default hashes so fn f(x:Real=1) ≠ fn f(x:Real=2).
    //
    // NOTE: `type_params` (bounds / defaults) are intentionally excluded from this hash.
    // Distinct generic signatures still hash differently because the param/return types are
    // already formatted as "TypeParam(T)" in `param_hashes`, capturing the type-param *names*.
    // `TypeParam.bounds` and `TypeParam.default` are unused downstream today, so omitting
    // them is currently safe. If bounds or defaults start affecting compilation (e.g. β/γ/δ),
    // fold a hash of (name + bounds + default) per TypeParam into `all_hashes` here to keep
    // content-addressing complete.
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
        doc: fn_def.doc.clone(),
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
        type_params: convert_type_params(&fn_def.type_params),
    })
}

/// Rewrite bare `Identifier(x)` references to conformer members into `self.x`
/// member accesses, in place, so an associated-function body's bare member refs
/// resolve as field projections on the `self` receiver (PRD §4.4, task 3941 ζ).
///
/// `members` is the conformer's member-name set; `bound` holds names that shadow
/// those members in the current lexical scope — fn params and body let-bindings
/// (seeded by the caller) plus nested binders introduced while walking
/// (lambda params, the quantifier variable, and `match` payload binders). A name
/// present in `bound` is never rewritten, so a lambda param / let / quantifier
/// var named after a member keeps its local meaning. A `self.x` written
/// explicitly is already a `MemberAccess` and is left untouched (we only rewrite
/// bare `Ident`s), so the explicit and sugared forms converge on the same node.
fn desugar_self_members(
    expr: &mut reify_ast::Expr,
    members: &HashSet<String>,
    bound: &mut HashSet<String>,
) {
    use reify_ast::ExprKind as EK;

    // `Ident` is handled before the `&mut`-borrowing match so the in-place
    // rewrite (which reassigns `expr.kind`) does not conflict with the borrow.
    if let EK::Ident(name) = &expr.kind {
        if !bound.contains(name) && members.contains(name) {
            let member = name.clone();
            let span = expr.span;
            expr.kind = EK::MemberAccess {
                object: Box::new(reify_ast::Expr {
                    kind: EK::Ident("self".to_string()),
                    span,
                }),
                member,
            };
        }
        return;
    }

    match &mut expr.kind {
        EK::BinOp { left, right, .. } => {
            desugar_self_members(left, members, bound);
            desugar_self_members(right, members, bound);
        }
        EK::UnOp { operand, .. } => desugar_self_members(operand, members, bound),
        EK::FunctionCall { args, .. } => {
            for a in args {
                desugar_self_members(a, members, bound);
            }
        }
        EK::MemberAccess { object, .. } => desugar_self_members(object, members, bound),
        EK::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            desugar_self_members(condition, members, bound);
            desugar_self_members(then_branch, members, bound);
            desugar_self_members(else_branch, members, bound);
        }
        EK::ListLiteral(items) | EK::SetLiteral(items) => {
            for it in items {
                desugar_self_members(it, members, bound);
            }
        }
        EK::MapLiteral(pairs) => {
            for (k, v) in pairs {
                desugar_self_members(k, members, bound);
                desugar_self_members(v, members, bound);
            }
        }
        EK::IndexAccess { object, index } => {
            desugar_self_members(object, members, bound);
            desugar_self_members(index, members, bound);
        }
        EK::Match { discriminant, arms } => {
            desugar_self_members(discriminant, members, bound);
            for arm in arms {
                // `Circle { radius: r }` binders shadow members in the arm body.
                let mut arm_bound = bound.clone();
                for pat in &arm.patterns {
                    if let reify_ast::MatchPattern::VariantBind { binders, .. } = pat {
                        for (_field, binder) in binders {
                            arm_bound.insert(binder.clone());
                        }
                    }
                }
                desugar_self_members(&mut arm.body, members, &mut arm_bound);
            }
        }
        EK::Lambda { params, body } => {
            let mut inner = bound.clone();
            for p in params.iter() {
                inner.insert(p.name.clone());
            }
            desugar_self_members(body, members, &mut inner);
        }
        EK::Quantifier {
            variable,
            collection,
            predicate,
            ..
        } => {
            // The collection is evaluated in the OUTER scope; the bound variable
            // shadows a member only inside the predicate.
            desugar_self_members(collection, members, bound);
            let mut inner = bound.clone();
            inner.insert(variable.clone());
            desugar_self_members(predicate, members, &mut inner);
        }
        EK::AdHocSelector { base, args, .. } => {
            desugar_self_members(base, members, bound);
            for a in args {
                desugar_self_members(a, members, bound);
            }
        }
        EK::QualifiedAccess { qualifier, .. } => desugar_self_members(qualifier, members, bound),
        EK::InstanceQualifiedAccess { object, qualified } => {
            desugar_self_members(object, members, bound);
            desugar_self_members(qualified, members, bound);
        }
        EK::Range { lower, upper, .. } => {
            if let Some(l) = lower {
                desugar_self_members(l, members, bound);
            }
            if let Some(u) = upper {
                desugar_self_members(u, members, bound);
            }
        }
        EK::TraitMethodCall { object, args, .. } => {
            desugar_self_members(object, members, bound);
            for a in args {
                desugar_self_members(a, members, bound);
            }
        }
        EK::TraitStaticCall { args, .. } => {
            for a in args {
                desugar_self_members(a, members, bound);
            }
        }
        EK::VariantConstruct { fields, .. } => {
            for (_name, value) in fields {
                desugar_self_members(value, members, bound);
            }
        }
        EK::Auto { params, .. } => {
            for (_name, value) in params {
                desugar_self_members(value, members, bound);
            }
        }
        EK::InterpolatedString(parts) => {
            for part in parts {
                if let reify_ast::StringPart::Hole(e) = part {
                    desugar_self_members(e, members, bound);
                }
            }
        }
        // Leaves with no sub-expressions, and `Ident` (handled by the early
        // return above — listed here only to keep the match exhaustive without a
        // wildcard so a future expr-bearing variant is a compile error, not a
        // silently-unwalked node).
        EK::Ident(_)
        | EK::NumberLiteral { .. }
        | EK::QuantityLiteral { .. }
        | EK::StringLiteral(_)
        | EK::BoolLiteral(_)
        | EK::EnumAccess { .. }
        | EK::Undef => {}
    }
}

/// Compile a trait associated function bound to a specific conforming structure
/// (the "conformer"). Sibling of [`compile_function`]; the sole difference is the
/// leading `is_self` receiver parameter, whose type is the conformer
/// `Type::StructureRef(conformer_name)` (and is registered as `self` in the body
/// scope) instead of being resolved from the sentinel `self` named type.
///
/// `self.member` bare-member sugar resolution is implemented by task ζ (#3941):
/// before the body is compiled, every bare `Identifier(x)` that names a
/// conformer member (and is not shadowed by a fn param or a body binding) is
/// rewritten to `self.x` (see [`desugar_self_members`]). The `self` receiver is
/// registered as `StructureRef(conformer)` in the body scope, so the rewritten
/// `MemberAccess` lowers to the existing `IndexAccess`-on-self node that
/// `eval_index_access` reads from the runtime `StructureInstance.fields` (PRD
/// §4.4). `conformer_members` is the member-name set to rewrite against.
///
/// δ's table-population contract is unchanged: this still returns a
/// distinguishable [`CompiledFunction`] per conformer (override vs injected
/// default differ by body content hash), and `None` for a bodyless fn (a
/// required, body = None member never reaches this path: the resolver compiles
/// the structure override or the default body, both of which carry a body).
/// Added by task 3939 δ; bare-member desugar added by task 3941 ζ.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_assoc_function(
    fn_def: &reify_ast::FnDef,
    conformer_name: &str,
    conformer_members: &HashSet<String>,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    // Mirror of compile_function: build the type-param name set for signature resolution.
    // No-op for today's non-generic assoc fns (empty fn_def.type_params → empty set).
    let type_param_names: HashSet<String> = fn_def
        .type_params
        .iter()
        .map(|tp| tp.name.clone())
        .collect();
    // Dimension-kinded subset (mirrors compile_function; empty for today's assoc fns).
    let dim_param_names: HashSet<String> = fn_def
        .type_params
        .iter()
        .filter(|tp| tp.bounds.iter().any(|b| b == "Dimension"))
        .map(|tp| tp.name.clone())
        .collect();
    let receiver_type = Type::StructureRef(conformer_name.to_string());

    // Resolve parameter types. The leading `is_self` receiver maps to the
    // conformer StructureRef rather than resolving the sentinel `self` type;
    // every other param resolves exactly as in `compile_function`.
    let mut params: Vec<(String, Type)> = Vec::new();
    for p in &fn_def.params {
        if p.is_self {
            params.push((p.name.clone(), receiver_type.clone()));
            continue;
        }
        let ty = match resolve_type_expr_with_aliases_kinded(
            &p.type_expr,
            &type_param_names,
            &dim_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        ) {
            Some(t) => t,
            None => {
                push_signature_type_error(
                    diagnostics,
                    &type_param_names,
                    &p.type_expr,
                    p.type_expr.span,
                    &fn_def.name,
                    "unresolved type",
                );
                Type::Error
            }
        };
        params.push((p.name.clone(), ty));
    }

    // Compile default expressions in a neutral scope (definition-time semantics,
    // matching `compile_function`). The `self` receiver never carries a default.
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

    // Resolve return type (defaults to Real when unannotated).
    let return_type = match &fn_def.return_type {
        Some(te) => match resolve_type_expr_with_aliases_kinded(
            te,
            &type_param_names,
            &dim_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        ) {
            Some(t) => t,
            None => {
                push_signature_type_error(
                    diagnostics,
                    &type_param_names,
                    te,
                    te.span,
                    &fn_def.name,
                    "unresolved return type",
                );
                Type::Error
            }
        },
        None => Type::dimensionless_scalar(),
    };

    // Body scope with all params (including the `self` receiver) registered so a
    // body that names `self` resolves against the conformer type.
    let mut scope = CompilationScope::new(&fn_def.name);
    for (name, ty) in &params {
        scope.register(name, ty.clone());
    }

    // Bodyless fns (required, body = None) have nothing to compile here.
    let body = fn_def.body.as_ref()?;

    // task 3941 ζ — bare-member → `self.member` desugar (PRD §4.4). Names already
    // bound in the body's lexical scope shadow conformer members and must NOT be
    // rewritten: start the bound set with the fn params (incl. the `self`
    // receiver), then add each body let-binding name AFTER its value compiles so
    // a later reference resolves to the binding, not the member. Param defaults
    // are deliberately left un-rewritten — they compile in a definition-time
    // neutral scope (PRD §13.3), matching the `neutral_scope` used above.
    let mut bound: HashSet<String> = fn_def.params.iter().map(|p| p.name.clone()).collect();

    let mut compiled_lets = Vec::new();
    for let_decl in &body.let_bindings {
        let mut value = let_decl.value.clone();
        desugar_self_members(&mut value, conformer_members, &mut bound);
        let compiled_expr = compile_expr(&value, &scope, enum_defs, functions, diagnostics);
        let let_type = compiled_expr.result_type.clone();
        scope.register(&let_decl.name, let_type);
        // The binding name shadows a same-named conformer member for subsequent
        // lets and the result expr.
        bound.insert(let_decl.name.clone());
        compiled_lets.push((let_decl.name.clone(), compiled_expr));
    }

    let mut result = body.result_expr.clone();
    desugar_self_members(&mut result, conformer_members, &mut bound);
    let result_expr = compile_expr(&result, &scope, enum_defs, functions, diagnostics);

    // Content hash — same shape as `compile_function` so that an override body
    // and an injected-default body hash differently when their bodies differ.
    //
    // NOTE: `type_params` (bounds / defaults) are intentionally excluded — see the
    // matching comment in `compile_function` for rationale. Fold them in here too
    // when bounds/defaults start affecting compilation.
    let content_hash = {
        let name_hash = ContentHash::of_str(&fn_def.name);
        let param_hashes = params
            .iter()
            .map(|(n, t)| ContentHash::of_str(n).combine(ContentHash::of_str(&format!("{}", t))));
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

    let opt_target = optimized_target(&fn_def.annotations);
    let annotations = lower_annotations(&fn_def.annotations, diagnostics);
    validate_annotations(&annotations, "function", diagnostics);

    Some(CompiledFunction {
        name: fn_def.name.clone(),
        doc: fn_def.doc.clone(),
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
        type_params: convert_type_params(&fn_def.type_params),
    })
}

/// Resolve a type name in field context. Unlike resolve_type_name, unresolved
/// names become StructureRef (geometric domain types like Point3, Vector3)
/// but a diagnostic warning is emitted so the user knows the type was not
/// resolved from the built-in set.
pub(crate) fn resolve_field_type_name(
    name: &str,
    span: reify_core::SourceSpan,
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
        || (matches!(body_ty, Type::Int)
            && matches!(codomain_ty, Type::Scalar { dimension } if dimension.is_dimensionless()))
}

/// Compile a field declaration into a CompiledField.
pub(crate) fn compile_field(
    field_def: &reify_ast::FieldDef,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledField {
    // Resolve domain and codomain types. A structurally-invalid type-expr (e.g.
    // DimensionalOp) cannot appear as a field type — emit exactly one diagnostic and
    // fall back to Type::Error (poison) without forwarding a sentinel "<unknown>"
    // string to resolve_field_type_name (which would push a second confusing
    // diagnostic for the placeholder name). Poison engages the anti-cascade guards so
    // the root-cause diagnostic stands alone (ds-sentinel L1, task #4646). Exception:
    // the Function/arrow arms keep Type::dimensionless_scalar() — the arrow type
    // resolves fine, it is merely disallowed in this position (see those arms below).
    let domain_type = match &field_def.domain_type.kind {
        reify_ast::TypeExprKind::Named { name, .. } => resolve_field_type_name(
            name.as_str(),
            field_def.domain_type.span,
            alias_registry,
            diagnostics,
        ),
        reify_ast::TypeExprKind::DimensionalOp { .. } => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_code(DiagnosticCode::UnresolvedType)
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "unexpected dimensional expression",
                    )),
            );
            Type::Error
        }
        reify_ast::TypeExprKind::IntegerLiteral(_) => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_code(DiagnosticCode::UnresolvedType)
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "integer literal not allowed in this position",
                    )),
            );
            Type::Error
        }
        // Auto type-args cannot appear as a field domain type; resolution deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_code(DiagnosticCode::UnresolvedType)
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "auto type-arg not allowed in this position",
                    )),
            );
            Type::Error
        }
        // Qualified assoc-type refs cannot appear as a field domain type here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => {
            diagnostics.push(
                Diagnostic::error(format!("unresolved field type: {}", field_def.domain_type))
                    .with_code(DiagnosticCode::UnresolvedType)
                    .with_label(DiagnosticLabel::new(
                        field_def.domain_type.span,
                        "associated type not yet resolved in this position",
                    )),
            );
            Type::Error
        }
        // A function / arrow type `(T) -> U` (task 4595) cannot be a field domain type.
        // The arrow type resolves fine — it is simply disallowed in this position —
        // so the top-line message says "not allowed" rather than "unresolved" (the
        // shared UnresolvedType code is retained for audit-coverage; message prose is
        // not a structured contract — see unresolved_diagnostic_code_audit_tests.rs).
        reify_ast::TypeExprKind::Function { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "function type not allowed as a field domain type: {}",
                    field_def.domain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.domain_type.span,
                    "function type not allowed in this position",
                )),
            );
            // ds-sentinel:allow PRD §3 KEEP (esc-4646-3): the arrow type resolves fine —
            // it is disallowed in field-domain position, not an unknown name — so the
            // source expr still type-checks against Real. Converting to Type::Error here
            // would poison the source-expr result in a way that misrepresents the error.
            Type::dimensionless_scalar()
        }
    };
    let codomain_type = match &field_def.codomain_type.kind {
        reify_ast::TypeExprKind::Named { name, .. } => resolve_field_type_name(
            name.as_str(),
            field_def.codomain_type.span,
            alias_registry,
            diagnostics,
        ),
        reify_ast::TypeExprKind::DimensionalOp { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "unexpected dimensional expression",
                )),
            );
            Type::Error
        }
        reify_ast::TypeExprKind::IntegerLiteral(_) => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "integer literal not allowed in this position",
                )),
            );
            Type::Error
        }
        // Auto type-args cannot appear as a field codomain type; resolution deferred to task 3477/3558.
        reify_ast::TypeExprKind::Auto { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "auto type-arg not allowed in this position",
                )),
            );
            Type::Error
        }
        // Qualified assoc-type refs cannot appear as a field codomain type here;
        // resolution deferred to task ιₑ.
        reify_ast::TypeExprKind::QualifiedAssoc { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved field type: {}",
                    field_def.codomain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "associated type not yet resolved in this position",
                )),
            );
            Type::Error
        }
        // A function / arrow type `(T) -> U` (task 4595) cannot be a field codomain type.
        // As with the domain arm above, the arrow type resolves fine — it is simply
        // disallowed here — so the message says "not allowed" rather than "unresolved".
        reify_ast::TypeExprKind::Function { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "function type not allowed as a field codomain type: {}",
                    field_def.codomain_type
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    field_def.codomain_type.span,
                    "function type not allowed in this position",
                )),
            );
            // PRD §3 KEEP (esc-4646-3): same rationale as the domain arm above — arrow type
            // is disallowed in field-codomain position, not an unknown name.
            Type::dimensionless_scalar() // ds-sentinel:allow PRD §3 KEEP (esc-4646-3)
        }
    };

    // Create a scope for compiling field source expressions
    let scope = CompilationScope::new(&field_def.name);

    let source = match &field_def.source {
        reify_ast::FieldSource::Analytical { expr } => {
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
                    reify_ir::CompiledExprKind::Lambda { .. }
                ) || compiled_expr.result_type.is_error(),
                "analytical field source compiled to non-Lambda with non-Error result type — \
                 this indicates a compiler bug"
            );
            if let reify_ir::CompiledExprKind::Lambda { body, .. } = &compiled_expr.kind {
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
        reify_ast::FieldSource::Sampled { config } => {
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
            let mut compiled_config: Vec<(String, reify_ir::CompiledExpr)> = Vec::new();
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
        reify_ast::FieldSource::Composed { expr } => {
            let compiled_expr = compile_expr(expr, &scope, enum_defs, functions, diagnostics);
            CompiledFieldSource::Composed {
                expr: compiled_expr,
            }
        }
        reify_ast::FieldSource::Imported { path, format, grid } => {
            // Validate required keys: path, format, grid.
            if path.is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "imported field source is missing required key: 'path'",
                    )
                    .with_label(DiagnosticLabel::new(
                        field_def.span,
                        "missing required imported config key",
                    )),
                );
            }
            if format.is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "imported field source is missing required key: 'format'",
                    )
                    .with_label(DiagnosticLabel::new(
                        field_def.span,
                        "missing required imported config key",
                    )),
                );
            }
            if grid.is_none() {
                diagnostics.push(
                    Diagnostic::error(
                        "imported field source is missing required key: 'grid'",
                    )
                    .with_label(DiagnosticLabel::new(
                        field_def.span,
                        "missing required imported config key",
                    )),
                );
            }
            // Validate format value: only "OpenVDB" is supported in v0.2.
            if let Some(fmt) = format.as_deref()
                && fmt != "OpenVDB"
            {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "unsupported imported field format '{}': only 'OpenVDB' is supported",
                        fmt
                    ))
                    .with_label(DiagnosticLabel::new(
                        field_def.span,
                        "unsupported format for imported field source",
                    )),
                );
            }
            // Note: the struct is populated with whatever was parsed even when the
            // validation diagnostics above were emitted.  Eval relies on (Some path,
            // Some grid) and treats anything else as Undef; compile errors are the
            // user-visible signal for missing or invalid keys.
            CompiledFieldSource::Imported {
                path: path.clone(),
                format: format.clone(),
                grid: grid.clone(),
            }
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
            CompiledFieldSource::Imported { path, format, grid } => {
                let ph = path.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                let fh = format.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                let gh = grid.as_deref().map(ContentHash::of_str).unwrap_or(ContentHash(0));
                ContentHash::combine_all([ph, fh, gh])
            }
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
            source: CompiledFieldSource::Imported { path: None, format: None, grid: None },
            content_hash: ContentHash(0),
            annotations: vec![],
        }
    }

    /// Build a composed expression representing `outer_name(inner_name(dummy_literal))`.
    ///
    /// The dummy literal is typed `Real` to match the `domain_type` of the inner
    /// field (`Type::dimensionless_scalar()`) in all current test cases. `check_field_composition_types`
    /// only validates inter-function wiring (inner.codomain → outer.domain) and does
    /// not check argument types against the inner field's domain, so the dummy type
    /// currently has no effect on test outcomes. It is kept consistent with the inner
    /// domain to avoid spurious failures if argument-type checking is added later.
    fn make_composition_expr(outer_name: &str, inner_name: &str) -> CompiledExpr {
        let dummy = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let inner_call = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: inner_name.to_string(),
                    qualified_name: inner_name.to_string(),
                },
                args: vec![dummy],
            },
            result_type: Type::dimensionless_scalar(),
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
            result_type: Type::dimensionless_scalar(),
            content_hash: ContentHash(0),
        }
    }

    /// inner codomain = Vector<3,Real>, outer domain = Tensor<1,3,Real>.
    /// Rule 1a applies (Vector<N,Q> → Tensor<1,N,Q>): zero diagnostics.
    /// Pins the producer→consumer wiring: inner.codomain is checked as FROM,
    /// outer.domain as TO.
    #[test]
    fn field_composition_allows_vector_to_tensor1() {
        let inner = make_field("inner", Type::dimensionless_scalar(), Type::vec3(Type::dimensionless_scalar()));
        let outer = make_field("outer", Type::tensor(1, 3, Type::dimensionless_scalar()), Type::dimensionless_scalar());
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
        let inner = make_field("inner", Type::dimensionless_scalar(), Type::matrix(3, 3, Type::dimensionless_scalar()));
        let outer = make_field("outer", Type::tensor(2, 3, Type::dimensionless_scalar()), Type::dimensionless_scalar());
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
        let inner = make_field("inner", Type::dimensionless_scalar(), Type::tensor(2, 3, Type::dimensionless_scalar()));
        let outer = make_field("outer", Type::matrix(3, 3, Type::dimensionless_scalar()), Type::dimensionless_scalar());
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
        let inner = make_field("inner", Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let outer = make_field("outer", Type::dimensionless_scalar(), Type::dimensionless_scalar());
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
        let outer = make_field("outer", Type::dimensionless_scalar(), Type::dimensionless_scalar());
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
        let inner = make_field("inner", Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let outer = make_field("outer", Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let body = make_composition_expr("outer", "inner");
        let lambda_expr = CompiledExpr {
            kind: CompiledExprKind::Lambda {
                params: vec![("p".to_string(), Some(Type::dimensionless_scalar()))],
                param_ids: vec![ValueCellId::new("$lambda0", "p")],
                body: Box::new(body),
                captures: vec![],
            },
            result_type: Type::dimensionless_scalar(),
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

    // ── ds-sentinel L1 (task #4646): producer poison for the pub(crate)-only
    //    assoc-fn sites and the PARSE-UNREACHABLE Tier-2 field type-expr arms. ──
    //
    // The Reify parser only yields `TypeExprKind::Named` in annotation positions,
    // so the DimOp/IntLit/Auto/QualAssoc field arms cannot be reached via
    // compile_source; they are exercised here by direct AST construction. Each
    // test asserts `.is_error()` on the RESOLVED TYPE the producer returns — the
    // precise effect of the L1 fix (dimensionless == not-error pre-fix ->
    // Error == is-error post-fix), genuinely RED before the conversion.

    fn ds_span() -> reify_core::SourceSpan {
        reify_core::SourceSpan::new(0, 0)
    }

    fn ds_named(name: &str) -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: name.to_string(),
                type_args: vec![],
            },
            span: ds_span(),
        }
    }

    fn ds_dim_op() -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::DimensionalOp {
                op: reify_ast::DimOp::Mul,
                left: Box::new(ds_named("Force")),
                right: Box::new(ds_named("Length")),
            },
            span: ds_span(),
        }
    }

    fn ds_int_lit() -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::IntegerLiteral(5),
            span: ds_span(),
        }
    }

    fn ds_auto() -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Auto {
                free: false,
                bound: "Dimension".to_string(),
            },
            span: ds_span(),
        }
    }

    fn ds_qual_assoc() -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::QualifiedAssoc {
                base: Box::new(ds_named("Beam")),
                trait_name: None,
                member: "Bogus".to_string(),
            },
            span: ds_span(),
        }
    }

    /// An arrow / function type `(Length) -> Length`. Unlike the
    /// DimOp/IntLit/Auto/QualAssoc arms, this RESOLVES fine — it is merely
    /// DISALLOWED as a field domain/codomain, so per the esc-4646-3
    /// ratification it deliberately KEEPS Type::dimensionless_scalar() (NOT
    /// poison). Used to pin that the Function arms stay non-error.
    fn ds_function_type() -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Function {
                params: vec![ds_named("Length")],
                return_type: Box::new(ds_named("Length")),
            },
            span: ds_span(),
        }
    }

    /// Build a `reify_ast::FieldDef` with an `Imported` source so only the
    /// domain/codomain type-expr resolution is under test (no analytical-lambda
    /// codomain-check noise).
    fn ds_field(
        domain_type: reify_ast::TypeExpr,
        codomain_type: reify_ast::TypeExpr,
    ) -> reify_ast::FieldDef {
        reify_ast::FieldDef {
            name: "fld".to_string(),
            is_pub: false,
            domain_type,
            codomain_type,
            source: reify_ast::FieldSource::Imported {
                path: None,
                format: None,
                grid: None,
            },
            span: ds_span(),
            content_hash: reify_core::ContentHash::of_str("fld"),
            annotations: vec![],
        }
    }

    // step-3: compile_assoc_function — unresolved param-type NAME (site :393)
    // and unresolved return-type NAME (site :433) must each resolve to Type::Error.
    #[test]
    fn ds_l1_assoc_fn_unresolved_names_are_error() {
        let fn_def = reify_ast::FnDef {
            name: "m".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params: vec![
                reify_ast::FnParam {
                    name: "self".to_string(),
                    is_self: true,
                    type_expr: ds_named("self"),
                    default: None,
                    span: ds_span(),
                },
                reify_ast::FnParam {
                    name: "x".to_string(),
                    is_self: false,
                    type_expr: ds_named("Bogus"),
                    default: None,
                    span: ds_span(),
                },
            ],
            // A body is required: compile_assoc_function returns None for a bodyless fn.
            return_type: Some(ds_named("Bogus")),
            body: Some(reify_ast::FnBody {
                let_bindings: vec![],
                result_expr: reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value: 0.0,
                        is_real: true,
                    },
                    span: ds_span(),
                },
            }),
            span: ds_span(),
            content_hash: reify_core::ContentHash::of_str("m"),
            annotations: vec![],
        };
        let mut diags: Vec<Diagnostic> = Vec::new();
        let compiled = compile_assoc_function(
            &fn_def,
            "Conformer",
            &HashSet::new(),
            &[],
            &[],
            &TypeAliasRegistry::new(),
            &HashSet::new(),
            &HashSet::new(),
            &mut diags,
        )
        .expect("compile_assoc_function should return Some for a fn with a body");

        let (_, x_ty) = compiled
            .params
            .iter()
            .find(|(n, _)| n == "x")
            .expect("assoc fn should have a non-self param x");
        assert!(
            x_ty.is_error(),
            "unresolved assoc-fn param type `Bogus` must be Type::Error, got: {:?}",
            x_ty
        );
        assert!(
            compiled.return_type.is_error(),
            "unresolved assoc-fn return type `Bogus` must be Type::Error, got: {:?}",
            compiled.return_type
        );
    }

    // step-5: compile_field domain — DimOp (:589), IntegerLiteral (:600),
    // Auto (:612), QualifiedAssoc (:625) must each resolve domain_type to Type::Error.
    #[test]
    fn ds_l1_field_domain_invalid_type_exprs_are_error() {
        for domain in [ds_dim_op(), ds_int_lit(), ds_auto(), ds_qual_assoc()] {
            let field = ds_field(domain.clone(), ds_named("Length"));
            let mut diags: Vec<Diagnostic> = Vec::new();
            let compiled = compile_field(&field, &[], &[], &TypeAliasRegistry::new(), &mut diags);
            assert!(
                compiled.domain_type.is_error(),
                "field domain {:?} must resolve to Type::Error, got: {:?}",
                domain.kind,
                compiled.domain_type
            );
            // Pin compile_field's "emit exactly one diagnostic ... without
            // forwarding a sentinel <unknown>" contract (header comment): the
            // invalid domain arm must push EXACTLY ONE UnresolvedType. A
            // regression that forwarded a sentinel name to
            // resolve_field_type_name would push a SECOND type diagnostic
            // (caught here) and return a StructureRef (caught by is_error
            // above). The Imported-source stub (None path/format/grid) emits
            // its own unrelated "missing required key" diagnostics, so we count
            // by the structured UnresolvedType code rather than the raw vec len.
            let unresolved = diags
                .iter()
                .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
                .count();
            assert_eq!(
                unresolved, 1,
                "field domain {:?} must emit exactly one UnresolvedType diagnostic, got: {:?}",
                domain.kind, diags
            );
        }
    }

    // step-7: compile_field codomain — DimOp (:666), IntegerLiteral (:680),
    // Auto (:695), QualifiedAssoc (:711) must each resolve codomain_type to Type::Error.
    #[test]
    fn ds_l1_field_codomain_invalid_type_exprs_are_error() {
        for codomain in [ds_dim_op(), ds_int_lit(), ds_auto(), ds_qual_assoc()] {
            let field = ds_field(ds_named("Length"), codomain.clone());
            let mut diags: Vec<Diagnostic> = Vec::new();
            let compiled = compile_field(&field, &[], &[], &TypeAliasRegistry::new(), &mut diags);
            assert!(
                compiled.codomain_type.is_error(),
                "field codomain {:?} must resolve to Type::Error, got: {:?}",
                codomain.kind,
                compiled.codomain_type
            );
            // Pin the "exactly one diagnostic" contract (see the domain test):
            // the invalid codomain arm must push EXACTLY ONE UnresolvedType.
            // Counted by structured code to stay immune to the Imported-source
            // stub's unrelated "missing required key" diagnostics; a
            // sentinel-name forward re-introducing a second confusing diagnostic
            // is caught here, and the resulting StructureRef by is_error above.
            let unresolved = diags
                .iter()
                .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
                .count();
            assert_eq!(
                unresolved, 1,
                "field codomain {:?} must emit exactly one UnresolvedType diagnostic, got: {:?}",
                codomain.kind, diags
            );
        }
    }

    // Amendment (reviewer_comprehensive, esc-4646-3 KEEP pin): the Function /
    // arrow field arms (compile_field :636 domain, :720 codomain) DELIBERATELY
    // keep Type::dimensionless_scalar() rather than poison — the arrow type
    // resolves fine, it is merely DISALLOWED in this position (NOT a resolution
    // failure). A future contributor who "completes" the poison conversion by
    // also poisoning these arms (the out-of-scope follow-up tracked by the LIVE
    // task #4657, filed from esc-4646-36) would trip THIS test, forcing the
    // intended review rather than silently closing the gap. Pins the deliberate
    // exception until #4657 lands — the guard points at a live, non-terminal
    // successor, so it cannot outlive its rationale.
    #[test]
    fn ds_l1_field_arrow_arms_stay_dimensionless_not_poison() {
        // Arrow as field DOMAIN (codomain a valid Named type).
        let field = ds_field(ds_function_type(), ds_named("Length"));
        let mut diags: Vec<Diagnostic> = Vec::new();
        let compiled = compile_field(&field, &[], &[], &TypeAliasRegistry::new(), &mut diags);
        assert!(
            !compiled.domain_type.is_error(),
            "arrow field domain must NOT be poison (deliberate esc-4646-3 KEEP; poison conversion tracked by #4657), got: {:?}",
            compiled.domain_type
        );
        assert_eq!(
            compiled.domain_type,
            Type::dimensionless_scalar(),
            "arrow field domain must stay dimensionless_scalar, got: {:?}",
            compiled.domain_type
        );

        // Arrow as field CODOMAIN (domain a valid Named type).
        let field = ds_field(ds_named("Length"), ds_function_type());
        let mut diags: Vec<Diagnostic> = Vec::new();
        let compiled = compile_field(&field, &[], &[], &TypeAliasRegistry::new(), &mut diags);
        assert!(
            !compiled.codomain_type.is_error(),
            "arrow field codomain must NOT be poison (deliberate esc-4646-3 KEEP; poison conversion tracked by #4657), got: {:?}",
            compiled.codomain_type
        );
        assert_eq!(
            compiled.codomain_type,
            Type::dimensionless_scalar(),
            "arrow field codomain must stay dimensionless_scalar, got: {:?}",
            compiled.codomain_type
        );
    }
}
