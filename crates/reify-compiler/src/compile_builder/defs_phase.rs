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

use std::collections::{HashMap, HashSet};

use reify_syntax::ParsedModule;
use reify_types::{Diagnostic, DiagnosticLabel};

use crate::CompiledModule;
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
///
/// `structure_names` is the set of structure/occurrence names in scope (both
/// local and imported via the prelude). Param type names in this set suppress
/// the "unknown type" diagnostic because the resolved type is discarded at
/// def-compile time anyway — entity.rs only reads `param.name` and
/// `param.default` at instantiation time.
fn compile_constraint_def(
    c: &reify_syntax::ConstraintDef,
    alias_registry: &TypeAliasRegistry,
    enum_defs: &[reify_types::EnumDef],
    trait_names: &HashSet<String>,
    structure_names: &HashSet<String>,
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
                    structure_names,
                    trait_names,
                )
                .is_none()
                && let reify_syntax::TypeExprKind::Named { name, .. } = &te.kind
                && resolve_enum_type(name, enum_defs).is_none()
                && !structure_names.contains(name.as_str())
            {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "unknown type '{}' in param '{}' of constraint def '{}': \
                         expected a builtin scalar, type parameter, alias, enum, \
                         trait, structure, or occurrence name in scope",
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
/// that is pushed onto `ctx.constraint_defs`. Also emits the one-time
/// shadow-warning diagnostics for cross-prelude constraint-def name
/// collisions (see [`emit_constraint_def_shadow_warnings`]).
///
/// The registry that entity scopes consume is rebuilt fresh in
/// `entities_phase` without re-emitting these warnings, avoiding duplicate
/// diagnostics.
pub(crate) fn phase_constraint_defs(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
    prelude: &[&crate::CompiledModule],
    trait_names: &HashSet<String>,
) {
    // Lazily built on first Declaration::Constraint; modules with zero
    // constraint defs skip this allocation entirely.  The set contains
    // structure/occurrence names in scope: local names filtered from
    // seen_entity_names by kind, plus exported template names from every
    // prelude module.  This mirrors the trait_names building pattern in
    // phase_traits (traits_phase.rs:71-79).
    let mut structure_names: Option<HashSet<String>> = None;

    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Constraint(c) = decl {
            let names = structure_names.get_or_insert_with(|| {
                ctx.seen_entity_names
                    .iter()
                    .filter(|(_, (_, kind))| *kind == "structure" || *kind == "occurrence")
                    .map(|(name, _)| name.clone())
                    .chain(
                        prelude
                            .iter()
                            .flat_map(|m| m.templates.iter().map(|t| t.name.clone())),
                    )
                    .collect()
            });
            let compiled = compile_constraint_def(
                c,
                &ctx.alias_registry,
                &ctx.resolution_enums,
                trait_names,
                names,
                &mut ctx.diagnostics,
            );
            ctx.constraint_defs.push(compiled);
        }
    }

    emit_constraint_def_shadow_warnings(ctx, prelude);
}

/// Emit the phase-10 one-time shadow warnings for cross-prelude constraint
/// def name collisions. Two different prelude modules exporting the same
/// `pub constraint def` name trigger a warning naming the winner (first
/// imported) and loser (later imported). The first-imported definition
/// wins silently; later imports are discarded.
///
/// The entity phase rebuilds the registry fresh without re-emitting these
/// warnings — they are only reported here, at phase-10.
fn emit_constraint_def_shadow_warnings(
    ctx: &mut CompilationCtx,
    prelude: &[&crate::CompiledModule],
) {
    // Maps def name → path of the first module that contributed it.
    let mut prelude_source: HashMap<String, String> = HashMap::new();
    for m in prelude {
        let module_path_str = m.path.to_string();
        for cd in m.constraint_defs.iter().filter(|c| c.is_pub) {
            if let Some(prev_path) = prelude_source.get(&cd.name) {
                if *prev_path != module_path_str {
                    ctx.diagnostics
                        .push(Diagnostic::warning(format_shadow_warning(
                            &cd.name,
                            prev_path,
                            &module_path_str,
                        )));
                }
                // First-import wins: do not record a second source.
            } else {
                prelude_source.insert(cd.name.clone(), module_path_str.clone());
            }
        }
    }
}

/// Build a combined constraint-def registry: prelude pub defs first
/// (first-imported wins on cross-prelude name collisions), then local defs
/// override on name collision with prelude.
///
/// Borrows from `local` (module-local constraint defs in ctx) and every prelude
/// module's `constraint_defs`. Non-pub prelude defs are excluded — only pub
/// constraint defs are exported. All local constraint defs (pub or not) are
/// inserted; non-pub local defs are only reachable within the current module
/// but still shadow prelude defs of the same name.
///
/// Shadow warnings for cross-prelude name collisions are NOT emitted here —
/// they were already emitted once in [`phase_constraint_defs`] via
/// [`emit_constraint_def_shadow_warnings`]. The downstream entity-phase
/// rebuild must not re-emit them to avoid duplicate diagnostics.
///
/// Asymmetry with `build_trait_registry` (design decision #2 task 2080):
/// uses `.entry().or_insert()` for prelude (preserving first-imported-wins
/// policy pinned by `cross_module_constraint_def_name_collision_emits_shadow_warning`)
/// and `.insert()` for local (allowing local override of any prelude def).
pub(crate) fn build_constraint_def_registry<'a>(
    local: &'a [CompiledConstraintDef],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a CompiledConstraintDef> {
    let mut registry: HashMap<String, &'a CompiledConstraintDef> = HashMap::new();
    for m in prelude {
        for cd in m.constraint_defs.iter().filter(|c| c.is_pub) {
            registry.entry(cd.name.clone()).or_insert(cd);
        }
    }
    for cd in local {
        registry.insert(cd.name.clone(), cd);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::build_constraint_def_registry;
    use crate::CompiledModule;
    use crate::types::CompiledConstraintDef;
    use reify_types::{ContentHash, ModulePath, SourceSpan};

    fn mk_cd(name: &str, is_pub: bool, span: SourceSpan) -> CompiledConstraintDef {
        CompiledConstraintDef {
            name: name.to_string(),
            is_pub,
            type_params: vec![],
            params: vec![],
            predicates: vec![],
            span,
            content_hash: ContentHash::of_str(""),
            pragmas: vec![],
            annotations: vec![],
            annotations_optimized_target: None,
        }
    }

    fn mk_module(path: &str, cds: Vec<CompiledConstraintDef>) -> CompiledModule {
        CompiledModule {
            path: ModulePath::single(path),
            imports: vec![],
            enum_defs: vec![],
            functions: vec![],
            trait_defs: vec![],
            fields: vec![],
            compiled_purposes: vec![],
            templates: vec![],
            units: vec![],
            type_aliases: vec![],
            constraint_defs: cds,
            pragmas: vec![],
            default_tolerance: None,
            declared_version: None,
            solver_pragma: None,
            kernel_pragma: None,
            diagnostics: vec![],
            content_hash: ContentHash::of_str(""),
        }
    }

    /// Covers the three key invariants of `build_constraint_def_registry`:
    ///
    /// 1. First-imported prelude wins: on cross-prelude name collision, the
    ///    first module in the slice retains its definition.
    /// 2. Local overrides prelude: a local def with the same name beats any
    ///    prelude def regardless of insertion order.
    /// 3. Non-pub prelude defs are excluded from the registry.
    #[test]
    fn build_constraint_def_registry_first_imported_prelude_wins_and_local_overrides() {
        let span_a = SourceSpan::new(1, 1);
        let span_b = SourceSpan::new(2, 2);
        let span_local = SourceSpan::new(3, 3);

        // Module 'a': pub MinThickness + non-pub Hidden.
        let m_a = mk_module(
            "a",
            vec![
                mk_cd("MinThickness", true, span_a),
                mk_cd("Hidden", false, SourceSpan::new(0, 0)),
            ],
        );
        // Module 'b': pub MinThickness (second-imported — should lose to 'a').
        let m_b = mk_module("b", vec![mk_cd("MinThickness", true, span_b)]);

        // Local: WallWidth (no collision) + MinThickness override.
        let local = vec![
            mk_cd("WallWidth", false, SourceSpan::new(4, 4)),
            mk_cd("MinThickness", false, span_local),
        ];

        // With local defs: local MinThickness override wins; WallWidth added; Hidden excluded.
        let registry = build_constraint_def_registry(&local, &[&m_a, &m_b]);
        assert!(
            registry.contains_key("MinThickness"),
            "MinThickness should be in registry"
        );
        assert!(
            registry.contains_key("WallWidth"),
            "WallWidth should be in registry"
        );
        assert!(
            !registry.contains_key("Hidden"),
            "non-pub Hidden should be excluded from registry"
        );
        // ptr::eq is stronger than span equality: it pins the exact stored reference.
        assert!(
            std::ptr::eq(registry["MinThickness"], &local[1]),
            "local MinThickness should override prelude (ptr identity to local[1])"
        );

        // Without local defs: first-imported prelude 'a' wins over 'b'.
        let registry_no_local = build_constraint_def_registry(&[], &[&m_a, &m_b]);
        assert!(
            std::ptr::eq(registry_no_local["MinThickness"], &m_a.constraint_defs[0]),
            "first-imported prelude 'a' should win over 'b' (ptr identity to m_a.constraint_defs[0])"
        );

        // Empty-prelude baseline: local defs populate the registry without panicking.
        let local_only = build_constraint_def_registry(&local, &[]);
        assert!(
            local_only.contains_key("MinThickness"),
            "local MinThickness should be present with empty prelude"
        );
        assert!(
            local_only.contains_key("WallWidth"),
            "local WallWidth should be present with empty prelude"
        );
    }
}
