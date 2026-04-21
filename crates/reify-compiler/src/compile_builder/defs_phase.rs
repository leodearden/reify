//! Phase-10 constraint defs: compile every `constraint def` declaration
//! and expose the `format_shadow_warning` helper used by the downstream
//! entity-phase registry construction.
//!
//! Migrates `compile_constraint_def` and `format_shadow_warning` from
//! lib.rs (see task 2035 design decision #5): both are used only by
//! phase-10, so hoisting them here keeps single-responsibility and
//! shrinks lib.rs toward the < 300-line target.
//!
//! The phase-10 constraint_def_registry (prelude-seed with shadow
//! warnings, then local override) is rebuilt phase-locally by the
//! downstream entity phase; by design, the downstream rebuild does NOT
//! re-emit shadow warnings (avoids duplicate diagnostics).

use std::collections::HashSet;

use reify_syntax::ParsedModule;
use reify_types::{Diagnostic, DiagnosticLabel};

use crate::annotations::{
    lower_annotations, optimized_target, validate_annotations, validate_pragmas,
};
use crate::compile_builder::ctx::CompilationCtx;
use crate::type_resolution::{
    TypeAliasRegistry, convert_type_params, resolve_enum_type, resolve_type_expr_with_aliases,
};
use crate::types::{CompiledConstraintDef, CompiledConstraintParam};

/// Format a constraint-def shadow-warning message for a name collision between two prelude modules.
///
/// `winner` is the first-imported module path string (whose definition is retained),
/// `loser` is the later-imported module path string (whose definition is silently discarded).
pub(crate) fn format_shadow_warning(name: &str, winner: &str, loser: &str) -> String {
    format!(
        "constraint def '{}' from '{}' shadows '{}' from '{}' \
         (first-imported definition wins)",
        name, winner, name, loser
    )
}

/// Compile a single `constraint def` declaration into a [`CompiledConstraintDef`].
///
/// Runs annotation/pragma lowering and validation exactly once per declaration,
/// resolves param types where possible, and caches the `@optimized` target so
/// instantiation sites can read it without re-scanning annotations.
fn compile_constraint_def(
    c: &reify_syntax::ConstraintDef,
    alias_registry: &TypeAliasRegistry,
    enum_defs: &[reify_types::EnumDef],
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledConstraintDef {
    // Extract @optimized target from raw syntax annotations BEFORE lowering so the
    // raw-annotation extractor sees the original parse tree.
    let annotations_optimized_target = optimized_target(&c.annotations);

    // Lower and validate annotations/pragmas (emits diagnostics for unknown/misplaced items).
    let annotations = lower_annotations(&c.annotations, diagnostics);
    validate_annotations(&annotations, "constraint_def", diagnostics);
    validate_pragmas(&c.pragmas, "constraint_def", diagnostics);

    // Convert syntax TypeParamDecls to compiled TypeParams.
    let type_params = convert_type_params(&c.type_params);

    // Build a set of type parameter names so param type resolution can accept them.
    let type_param_names: std::collections::HashSet<String> =
        type_params.iter().map(|tp| tp.name.clone()).collect();

    // Compile each param: resolve the cell type for its diagnostic side-effect (catches
    // typoed param types at def-compile time), then keep only the name/default/span.
    // The resolved type is not stored because entity.rs only reads `param.name` and
    // `param.default` at instantiation time; storing it would be dead weight.
    let params: Vec<CompiledConstraintParam> = c
        .params
        .iter()
        .map(|param| {
            // Resolve the param type: if resolution returns None for a Named type that is
            // neither a builtin nor a declared type parameter, the name is unknown — emit
            // an error so the user sees the typo at def-compile time rather than silently
            // accepting it and getting a confusing error at the instantiation site.
            if let Some(te) = &param.type_expr
                && resolve_type_expr_with_aliases(
                    te,
                    &type_param_names,
                    alias_registry,
                    diagnostics,
                    trait_names,
                )
                .is_none()
                && let reify_syntax::TypeExprKind::Named { name, .. } = &te.kind
                && resolve_enum_type(name, enum_defs).is_none()
            {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "unknown type '{}' in param '{}' of constraint def '{}'",
                        name, param.name, c.name
                    ))
                    .with_label(DiagnosticLabel::new(te.span, "unknown type")),
                );
            }
            CompiledConstraintParam {
                name: param.name.clone(),
                default: param.default.clone(),
                span: param.span,
            }
        })
        .collect();

    CompiledConstraintDef {
        name: c.name.clone(),
        is_pub: c.is_pub,
        type_params,
        params,
        predicates: c.predicates.clone(),
        span: c.span,
        content_hash: c.content_hash,
        pragmas: c.pragmas.clone(),
        annotations,
        annotations_optimized_target,
    }
}

/// Run phase-10 (constraint defs). Iterates `parsed.declarations`, filtering
/// `Declaration::Constraint`, compiling each into a `CompiledConstraintDef`
/// that is pushed onto `ctx.constraint_defs`.
///
/// The `prelude` argument is currently unused by the phase body itself —
/// constraint-def compilation only needs the alias registry, resolution
/// enums, and trait names — but is accepted so the signature anticipates
/// the step-14 design where `entities_phase` will rebuild the registry
/// without re-emitting shadow warnings. Callers pass the `prelude` slice
/// unchanged from the orchestrator.
pub(crate) fn phase_constraint_defs(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
    _prelude: &[&crate::CompiledModule],
    trait_names: &HashSet<String>,
) {
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Constraint(c) = decl {
            let compiled = compile_constraint_def(
                c,
                &ctx.alias_registry,
                &ctx.resolution_enums,
                trait_names,
                &mut ctx.diagnostics,
            );
            ctx.constraint_defs.push(compiled);
        }
    }
}
