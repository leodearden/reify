pub mod module_dag;
mod scc;
pub mod si_units;
pub mod stdlib_loader;
mod types;
mod units;
mod type_resolution;
mod type_compat;
mod scope;
mod expr;
mod traits;
mod annotations;
mod compile_builder;
mod termination;
mod entity;
mod connect;
mod guards;
mod conformance;
mod trait_requirements;
mod functions;
mod geometry;
mod geometry_boolean;
mod geometry_transform;
mod geometry_modify;
mod geometry_curve;
mod constants;

pub use types::*;
pub use type_compat::{implicitly_converts_to, type_compatible};

// Re-export submodule items for internal cross-module access via `use super::*;`
pub use units::{UnitEntry, UnitRegistry};
pub(crate) use units::*;
pub(crate) use type_resolution::*;
#[allow(unused_imports)]
pub(crate) use type_compat::*;
pub(crate) use scope::*;
pub(crate) use expr::*;
pub(crate) use traits::*;
pub(crate) use annotations::*;
pub(crate) use termination::*;
pub(crate) use entity::*;
pub(crate) use connect::*;
pub(crate) use guards::*;
pub(crate) use conformance::*;
pub(crate) use trait_requirements::*;
pub(crate) use functions::*;
pub(crate) use geometry::*;
pub(crate) use geometry_boolean::*;
pub(crate) use geometry_transform::*;
pub(crate) use geometry_modify::*;
pub(crate) use geometry_curve::*;

use std::collections::{HashMap, HashSet};

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ConstraintNodeId, ContentHash,
    DeterminacyPredicateKind, Diagnostic, DiagnosticLabel, DimensionVector, FIELD_ENTITY_PREFIX,
    OptimizationObjective, RealizationNodeId, ResolvedFunction, SelectorKind, Severity,
    SourceSpan, TAG_CONDITIONAL, TAG_FUNCTION_CALL, TAG_MATCH, TAG_USER_FUNCTION_CALL, Type,
    UnOp, Value, ValueCellId,
};

/// Format a constraint-def shadow-warning message for a name collision between two prelude modules.
///
/// `winner` is the first-imported module path string (whose definition is retained),
/// `loser` is the later-imported module path string (whose definition is silently discarded).
fn format_shadow_warning(name: &str, winner: &str, loser: &str) -> String {
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
    let annotations_optimized_target = crate::annotations::optimized_target(&c.annotations);

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

/// Compile a parsed module into a compiled module.
///
/// Performs name resolution, type checking, and expression compilation.
/// Equivalent to `compile_with_prelude(parsed, &[])`.
pub fn compile(parsed: &reify_syntax::ParsedModule) -> CompiledModule {
    compile_with_prelude(parsed, &[])
}

/// Compile a parsed module with the full standard library prelude.
///
/// This is the recommended entry point for compiling user modules with full
/// stdlib support. Equivalent to `compile_with_prelude(parsed, stdlib_loader::load_stdlib())`.
pub fn compile_with_stdlib(parsed: &reify_syntax::ParsedModule) -> CompiledModule {
    compile_with_prelude(parsed, stdlib_loader::load_stdlib())
}

/// Compile a parsed module with prelude definitions available for resolution.
///
/// Prelude modules provide trait definitions, enum definitions, and functions
/// that are visible to the user module during compilation. The output
/// `CompiledModule` contains only the user's own definitions — prelude
/// definitions are used as context but not duplicated in the output.
///
/// This is a thin wrapper around [`compile_with_prelude_refs`] that accepts
/// owned `CompiledModule` slices for external callers. Internal code should
/// prefer `compile_with_prelude_refs` to avoid cloning.
///
/// **Performance note:** this wrapper allocates a `Vec<&CompiledModule>` on
/// every call. For the typical call site with a small prelude this is
/// negligible, but callers in a hot loop should use `compile_with_prelude_refs`
/// (currently `pub(crate)`) directly to avoid repeated allocation.
pub fn compile_with_prelude(
    parsed: &reify_syntax::ParsedModule,
    prelude: &[CompiledModule],
) -> CompiledModule {
    let refs: Vec<&CompiledModule> = prelude.iter().collect();
    compile_with_prelude_refs(parsed, &refs)
}

/// Merge user functions with prelude functions, applying the canonical shadowing rule:
/// prelude functions whose `(name, arity, param_types)` triple matches any user function
/// are excluded; all others are appended. The result has user functions first, preserving
/// first-match-wins dispatch order for any resolver that iterates linearly.
///
/// First-wins dedup also applies prelude-vs-prelude: the shadow check runs against
/// `result` (the accumulating output), not `user` alone, so when two stdlib modules
/// both declare the same `(name, arity, param_types)` triple only the first-seen
/// entry survives. This mirrors stdlib load order (sequential in `stdlib_loader::load_stdlib`)
/// and silently drops duplicates without a diagnostic — intentional for now; if a future
/// stdlib PR introduces an accidental collision it will be invisible, so rely on
/// stdlib-level review rather than this function for duplicate detection.
///
/// This is the single-source shadow predicate for Reify function tables. It is used by
/// [`compile_with_prelude_refs`] to build the compile-time overload-resolution table.
/// `reify_eval::Engine` uses an unfiltered append that is dispatch-equivalent under
/// first-match-wins semantics (shadowed prelude entries are unreachable at dispatch time),
/// but the dedup logic for the filtered case lives here.
pub fn merge_prelude_functions(
    user: &[CompiledFunction],
    prelude: &[CompiledFunction],
) -> Vec<CompiledFunction> {
    let mut result = user.to_vec();
    for f in prelude {
        let shadowed = result.iter().any(|uf| {
            uf.name == f.name
                && uf.params.len() == f.params.len()
                && uf.params
                    .iter()
                    .zip(f.params.iter())
                    .all(|((_, ut), (_, ft))| ut == ft)
        });
        if !shadowed {
            result.push(f.clone());
        }
    }
    result
}

/// Compile a parsed module with prelude definitions provided as references.
///
/// This is the inner implementation used by the module DAG to avoid cloning
/// already-compiled modules. The `prelude` slice contains references to
/// compiled modules whose exported definitions (units, traits, enums,
/// constraint defs) are visible during compilation.
///
/// External callers should use [`compile_with_prelude`] instead.
pub(crate) fn compile_with_prelude_refs(
    parsed: &reify_syntax::ParsedModule,
    prelude: &[&CompiledModule],
) -> CompiledModule {
    // All durable mutable state owned by the phases lives on ctx. Phase-local
    // ref collections (fn_refs, trait_refs, etc.) stay as locals because they
    // borrow from `parsed`.
    let mut ctx = compile_builder::ctx::CompilationCtx::new();

    // Forward parse errors as diagnostics
    for err in &parsed.errors {
        ctx.diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }

    // Validate module-level pragmas: warn on unknown names.
    const KNOWN_MODULE_PRAGMAS: &[&str] = &["no_prelude", "precision", "solver", "kernel", "version"];
    for pragma in &parsed.pragmas {
        if !KNOWN_MODULE_PRAGMAS.contains(&pragma.name.as_str()) {
            ctx.diagnostics.push(
                Diagnostic::warning(format!("unknown pragma #{}", pragma.name))
                    .with_label(DiagnosticLabel::new(pragma.span, "unknown pragma")),
            );
        }
    }

    // Handle #no_prelude: suppress ALL prelude-dependent behavior by shadowing
    // the prelude parameter with an empty slice. This affects unit seeding,
    // trait/enum/function resolution, and constraint def imports.
    let has_no_prelude = parsed.pragmas.iter().any(|p| p.name == "no_prelude");
    let prelude: &[&CompiledModule] = if has_no_prelude { &[] } else { prelude };

    // Consolidated pre-pass: iterate declarations once, collecting references
    // for deferred compilation. This replaces 4 separate loops (enum, function,
    // trait, field) with a single match dispatch.
    let mut fn_refs: Vec<&reify_syntax::FnDef> = Vec::new();
    let mut trait_refs: Vec<&reify_syntax::TraitDecl> = Vec::new();
    let mut field_refs: Vec<&reify_syntax::FieldDef> = Vec::new();
    let mut unit_refs: Vec<&reify_syntax::UnitDecl> = Vec::new();
    let mut alias_refs: Vec<&reify_syntax::TypeAliasDecl> = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Enum(e) => {
                ctx.enum_defs.push(reify_types::EnumDef {
                    name: e.name.clone(),
                    variants: e.variants.clone(),
                });
            }
            reify_syntax::Declaration::Function(fn_def) => {
                fn_refs.push(fn_def);
            }
            reify_syntax::Declaration::Trait(trait_decl) => {
                trait_refs.push(trait_decl);
            }
            reify_syntax::Declaration::Field(field_def) => {
                if let Some((first_span, first_kind)) = ctx.seen_entity_names.get(&field_def.name) {
                    // Duplicate entity name — emit error and skip
                    ctx.diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            field_def.name
                        ))
                        .with_label(DiagnosticLabel::new(field_def.span, "field defined here"))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    ctx.seen_entity_names
                        .insert(field_def.name.clone(), (field_def.span, "field"));
                    field_refs.push(field_def);
                }
            }
            reify_syntax::Declaration::Structure(structure) => {
                if let Some((first_span, first_kind)) = ctx.seen_entity_names.get(&structure.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    ctx.diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            structure.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            structure.span,
                            "structure defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    ctx.seen_entity_names
                        .insert(structure.name.clone(), (structure.span, "structure"));
                }
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                if let Some((first_span, first_kind)) = ctx.seen_entity_names.get(&occurrence.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    ctx.diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            occurrence.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            occurrence.span,
                            "occurrence defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    ctx.seen_entity_names
                        .insert(occurrence.name.clone(), (occurrence.span, "occurrence"));
                }
            }
            reify_syntax::Declaration::Constraint(constraint) => {
                // Constraints reserve names in the entity namespace (spec §4.2.1)
                // even though constraint compilation is not yet implemented.
                if let Some((first_span, first_kind)) = ctx.seen_entity_names.get(&constraint.name) {
                    ctx.diagnostics.push(
                        Diagnostic::error(format!(
                            "duplicate entity definition '{}'",
                            constraint.name
                        ))
                        .with_label(DiagnosticLabel::new(
                            constraint.span,
                            "constraint defined here",
                        ))
                        .with_label(DiagnosticLabel::new(
                            *first_span,
                            format!("first defined as {} here", first_kind),
                        )),
                    );
                } else {
                    ctx.seen_entity_names
                        .insert(constraint.name.clone(), (constraint.span, "constraint"));
                }
            }
            reify_syntax::Declaration::Unit(unit_decl) => {
                unit_refs.push(unit_decl);
            }
            reify_syntax::Declaration::TypeAlias(alias_decl) => {
                alias_refs.push(alias_decl);
            }
            // Import, Purpose handled in pass 2 / purpose pass
            _ => {}
        }
    }

    // Compile unit declarations in source order (so later units can reference earlier ones).
    // Unit hashes are included in the module content hash.

    // Seed prelude units into the registry so module-local code can reference them.
    // Only pub units are seeded (private units are module-internal).
    for prelude_module in prelude {
        let module_display = prelude_module.path.to_string();
        for cu in &prelude_module.units {
            if cu.is_pub {
                // Detect cross-prelude collision before overwriting: if another
                // prelude module already seeded this unit name, emit a warning.
                if let Some(existing) = ctx.unit_registry.lookup(&cu.name) {
                    let first_module: &str = existing
                        .source_module
                        .as_deref()
                        .unwrap_or("<unknown>");
                    ctx.diagnostics.push(
                        Diagnostic::warning(format!(
                            "prelude unit '{}' declared in both '{}' and '{}'; last-wins",
                            cu.name, first_module, module_display
                        ))
                        .with_label(DiagnosticLabel::new(
                            SourceSpan::prelude(),
                            "cross-prelude collision",
                        )),
                    );
                }
                ctx.unit_registry.seed_prelude_unit(UnitEntry {
                    name: cu.name.clone(),
                    dimension: cu.dimension,
                    factor: cu.factor,
                    offset: cu.offset,
                    is_pub: cu.is_pub,
                    span: SourceSpan::prelude(),
                    content_hash: cu.content_hash,
                    source_module: Some(module_display.clone()),
                });
            }
        }
    }

    for unit_decl in &unit_refs {
        if let Some(entry) = compile_unit(unit_decl, &ctx.unit_registry, &mut ctx.diagnostics) {
            match ctx.unit_registry.register(entry) {
                Ok(()) => {
                    // Entry was registered; retrieve it to build CompiledUnit
                    let entry = ctx.unit_registry.lookup(&unit_decl.name).unwrap();
                    ctx.compiled_units.push(CompiledUnit {
                        name: entry.name.clone(),
                        is_pub: entry.is_pub,
                        dimension: entry.dimension,
                        factor: entry.factor,
                        offset: entry.offset,
                        content_hash: entry.content_hash,
                    });
                }
                Err(dup_entry) => {
                    // Duplicate unit name — find the original entry to determine provenance.
                    let original = ctx.unit_registry.lookup(&dup_entry.name).unwrap();
                    match &original.source_module {
                        Some(m) if m.starts_with("std/") => {
                            // Original is a stdlib prelude unit.
                            // Emit a two-label diagnostic: primary is the user's
                            // duplicate decl; secondary is the prelude sentinel
                            // carrying provenance text.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}' — already defined in stdlib prelude",
                                    dup_entry.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    "duplicate of stdlib unit",
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    "defined in stdlib prelude",
                                )),
                            );
                        }
                        Some(m) => {
                            // Original was seeded from a user module — name that module.
                            // Emit a two-label diagnostic: primary is the user's
                            // duplicate decl; secondary is the prelude sentinel
                            // carrying provenance text.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}' — already defined in module '{}'",
                                    dup_entry.name, m
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    format!("duplicate of unit from '{}'", m),
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    format!("defined in module '{}' prelude", m),
                                )),
                            );
                        }
                        None => {
                            // Module-local duplicate — show both source locations.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}'",
                                    dup_entry.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    "duplicate declared here",
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    "first declared here",
                                )),
                            );
                        }
                    }
                }
            }
        }
    }

    // Compile type alias declarations via DFS resolution with cycle detection.
    // Build a lookup map of all alias declarations, detecting duplicates.
    let mut alias_decl_map: HashMap<String, &reify_syntax::TypeAliasDecl> = HashMap::new();
    for alias_decl in &alias_refs {
        if let Some(first) = alias_decl_map.get(&alias_decl.name) {
            ctx.diagnostics.push(
                Diagnostic::error(format!(
                    "duplicate type alias declaration '{}'",
                    alias_decl.name
                ))
                .with_label(DiagnosticLabel::new(
                    alias_decl.span,
                    "duplicate declared here",
                ))
                .with_label(DiagnosticLabel::new(first.span, "first declared here")),
            );
        } else {
            alias_decl_map.insert(alias_decl.name.clone(), alias_decl);
        }
    }

    // DFS-resolve each alias with cycle detection via resolving-set.
    let mut resolving = HashSet::new();
    for alias_decl in &alias_refs {
        resolve_alias_dfs(
            &alias_decl.name,
            &alias_decl_map,
            &mut ctx.alias_registry,
            &mut resolving,
            &mut ctx.diagnostics,
        );
    }

    // Build resolution_enums: prelude enums + module-local enums.
    // resolution_enums is used for type resolution during compilation;
    // only enum_defs (module-local) goes into the output CompiledModule.
    ctx.resolution_enums = prelude
        .iter()
        .flat_map(|m| m.enum_defs.iter().cloned())
        .collect();
    ctx.resolution_enums.extend(ctx.enum_defs.iter().cloned());

    // Compile in dependency order after collecting all references:
    // 1. Functions (need all resolution_enums, plus prior compiled functions for self-reference)
    for fn_def in &fn_refs {
        if let Some(compiled_fn) = compile_function(
            fn_def,
            &ctx.resolution_enums,
            &ctx.functions,
            &ctx.alias_registry,
            &mut ctx.diagnostics,
        ) {
            ctx.functions.push(compiled_fn);
        }
    }

    // Build a resolution function list for compile-time overload resolution.
    // User functions appear first (shadowing priority); prelude functions with
    // distinct (name, arity, param_types) triples are appended. See
    // merge_prelude_functions() for the canonical shadow predicate.
    // `functions` (user-only) remains the output stored in CompiledModule.
    ctx.resolution_functions = {
        let prelude_fns: Vec<CompiledFunction> = prelude
            .iter()
            .flat_map(|m| m.functions.iter().cloned())
            .collect();
        merge_prelude_functions(&ctx.functions, &prelude_fns)
    };

    // Build the set of trait names known at compile time so the type resolver
    // can resolve `param m : Material` (trait name) to Type::TraitObject(...).
    //
    // Collected from local trait declarations (syntax) and prelude trait defs
    // (already compiled) BEFORE `compile_trait` runs, so trait members whose
    // types reference other traits can resolve their siblings. Trait-name
    // resolution is last in precedence (builtins → type params → alias → trait)
    // so existing name-reuse stays backward compatible.
    let trait_names: HashSet<String> = trait_refs
        .iter()
        .map(|t| t.name.clone())
        .chain(
            prelude
                .iter()
                .flat_map(|m| m.trait_defs.iter().map(|t| t.name.clone())),
        )
        .collect();

    // 2. Traits (depend on resolution_enums for enum type resolution in params)
    for trait_decl in &trait_refs {
        let compiled_trait = compile_trait(
            trait_decl,
            &ctx.resolution_enums,
            &ctx.alias_registry,
            &trait_names,
            &mut ctx.diagnostics,
        );
        ctx.trait_defs.push(compiled_trait);
    }

    // Build trait registry for conformance checking.
    // Start with prelude traits, then add module-local traits (module overrides prelude on collision).
    let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
    // Collect prelude trait references. We need to hold the prelude trait_defs
    // in scope so trait_registry can borrow from them.
    let prelude_trait_defs: Vec<&CompiledTrait> =
        prelude.iter().flat_map(|m| m.trait_defs.iter()).collect();
    for t in &prelude_trait_defs {
        trait_registry.insert(t.name.clone(), t);
    }
    // Module-local traits override prelude on name collision
    for t in &ctx.trait_defs {
        trait_registry.insert(t.name.clone(), t);
    }

    // Deprecation check: warn when a trait refinement references a @deprecated parent trait.
    // TraitDecl.refinements is Vec<String> without individual spans; use the child trait's span.
    for trait_decl in &trait_refs {
        for refinement_name in &trait_decl.refinements {
            if let Some(parent_trait) = trait_registry.get(refinement_name.as_str())
                && let Some(msg) = deprecation_message(&parent_trait.annotations)
            {
                emit_deprecation_warning(
                    "trait",
                    refinement_name,
                    &msg,
                    trait_decl.span,
                    &mut ctx.diagnostics,
                );
            }
        }
    }

    // 3. Fields (need all resolution_enums + all compiled functions)
    for field_def in &field_refs {
        let compiled = compile_field(
            field_def,
            &ctx.resolution_enums,
            &ctx.resolution_functions,
            &ctx.alias_registry,
            &mut ctx.diagnostics,
        );
        ctx.fields.push(compiled);
    }

    // Build a field registry so entity scopes can resolve field names.
    let field_registry: HashMap<String, &CompiledField> =
        ctx.fields.iter().map(|f| (f.name.clone(), f)).collect();

    // Compile all local constraint defs in a single pass.
    // Results are used both to populate the module output and to seed the registry.
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Constraint(c) = decl {
            let compiled = compile_constraint_def(
                c,
                &ctx.alias_registry,
                &ctx.resolution_enums,
                &trait_names,
                &mut ctx.diagnostics,
            );
            ctx.constraint_defs.push(compiled);
        }
    }

    // Build a constraint def registry so entity scopes can resolve constraint instantiations.
    // Prelude defs (pub-only, from imported modules) are seeded first; local defs override.
    // Shadow detection: warn when two different prelude modules export the same def name.
    let mut constraint_def_registry: HashMap<String, &CompiledConstraintDef> = HashMap::new();
    // Maps def name → path of the first module that contributed it (for shadow warnings).
    let mut prelude_source: HashMap<String, String> = HashMap::new();
    for m in prelude {
        let module_path_str = m.path.to_string();
        for cd in m.constraint_defs.iter().filter(|c| c.is_pub) {
            if let Some(prev_path) = prelude_source.get(&cd.name) {
                if *prev_path != module_path_str {
                    // Two different imported modules export the same constraint def name.
                    // The first-imported module wins; emit a warning that names the winner
                    // (prev_path) before the loser (module_path_str) so users know which
                    // import is retained and which is silently discarded.
                    ctx.diagnostics.push(Diagnostic::warning(format_shadow_warning(
                        &cd.name,
                        prev_path,
                        &module_path_str,
                    )));
                }
                // First-import wins: do not overwrite the existing registry entry.
            } else {
                prelude_source.insert(cd.name.clone(), module_path_str.clone());
                constraint_def_registry.insert(cd.name.clone(), cd);
            }
        }
    }
    // Local defs override prelude defs silently (by design: local always wins).
    for cd in &ctx.constraint_defs {
        constraint_def_registry.insert(cd.name.clone(), cd);
    }

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                // Only compile the first definition; duplicates have a different
                // span than the one recorded in seen_entity_names.
                let is_first_def = ctx
                    .seen_entity_names
                    .get(&structure.name)
                    .is_none_or(|(first_span, _)| *first_span == structure.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(structure);
                    let template = compile_entity(
                        &entity_ref,
                        EntityKind::Structure,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        &trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.diagnostics,
                        &ctx.templates,
                    );
                    ctx.templates.push(template);
                }
            }
            reify_syntax::Declaration::Enum(_) => {
                // Already collected in pre-pass above.
            }
            reify_syntax::Declaration::Import(import) => {
                ctx.imports.push(CompiledImport {
                    path: import.path.clone(),
                    kind: import.kind.clone(),
                    is_pub: import.is_pub,
                    span: import.span,
                });
                ctx.diagnostics.push(
                    Diagnostic::warning(format!(
                        "import \"{}\" noted; module resolution not yet implemented",
                        import.path
                    ))
                    .with_label(DiagnosticLabel::new(import.span, "import")),
                );
            }
            reify_syntax::Declaration::Function(_) => {
                // Already compiled in pre-pass above.
            }
            reify_syntax::Declaration::Trait(_) => {
                // Already compiled in trait pre-pass above.
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                // Only compile the first definition; duplicates have a different
                // span than the one recorded in seen_entity_names.
                let is_first_def = ctx
                    .seen_entity_names
                    .get(&occurrence.name)
                    .is_none_or(|(first_span, _)| *first_span == occurrence.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(occurrence);
                    let template = compile_entity(
                        &entity_ref,
                        EntityKind::Occurrence,
                        &ctx.resolution_enums,
                        &ctx.resolution_functions,
                        &trait_registry,
                        &trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &ctx.unit_registry,
                        &ctx.alias_registry,
                        &mut ctx.pending_bound_checks,
                        &mut ctx.diagnostics,
                        &ctx.templates,
                    );
                    ctx.templates.push(template);
                }
            }
            reify_syntax::Declaration::Field(_) => {
                // Already compiled in field pre-pass above.
            }
            reify_syntax::Declaration::Purpose(_) => {
                // Compiled in dedicated purpose pass below.
            }
            reify_syntax::Declaration::Constraint(_) => {
                // Already compiled by the constraint_defs pre-pass above;
                // annotation/pragma validation ran there too.
            }
            reify_syntax::Declaration::Unit(_) => {
                // Already compiled in unit pre-pass above.
            }
            reify_syntax::Declaration::TypeAlias(_) => {
                // Already compiled in type alias pre-pass above.
            }
        }
    }

    // Post-compilation pass: run deferred bound checks now that all structures
    // are compiled and available in the template registry.
    {
        let template_registry: HashMap<String, &TopologyTemplate> = ctx
            .templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

        let pending_bound_checks = std::mem::take(&mut ctx.pending_bound_checks);
        for check in pending_bound_checks {
            match check {
                PendingBoundCheck::SubComponent {
                    type_args,
                    target_name,
                    span,
                } => {
                    // Resolve type_params from the template registry now that
                    // all structures are compiled.
                    let type_params =
                        if let Some(target) = template_registry.get(target_name.as_str()) {
                            if target.type_params.is_empty() {
                                continue; // target has no type params, nothing to check
                            }
                            &target.type_params
                        } else {
                            // Target structure not found — skip (may be an external/unknown structure)
                            continue;
                        };

                    check_type_param_bounds(
                        type_params,
                        &type_args,
                        &target_name,
                        &template_registry,
                        &trait_registry,
                        &mut ctx.diagnostics,
                        span,
                    );
                }
                PendingBoundCheck::TraitConformance {
                    type_params,
                    type_args,
                    target_name,
                    span,
                } => {
                    check_type_param_bounds(
                        &type_params,
                        &type_args,
                        &target_name,
                        &template_registry,
                        &trait_registry,
                        &mut ctx.diagnostics,
                        span,
                    );
                }
                PendingBoundCheck::TraitArgConformance {
                    target_name,
                    arg_name,
                    arg_type,
                    arg_call_name,
                    span,
                } => {
                    check_trait_arg_conformance(
                        &target_name,
                        &arg_name,
                        &arg_type,
                        arg_call_name.as_deref(),
                        span,
                        &template_registry,
                        &trait_registry,
                        &mut ctx.diagnostics,
                    );
                }
            }
        }
    }

    // Post-compilation pass: detect recursive sub-component cycles.
    // Build a directed reference graph from sub_components and run DFS to find cycles.
    // Tag participating templates with is_recursive=true and emit a warning diagnostic.
    let cyclic_sccs = scc::detect_recursive_structures(&mut ctx.templates, &mut ctx.diagnostics);

    // Post-compilation pass: verify recursive structures have valid termination conditions.
    // Emits errors for recursive subs without guards or with non-terminating guard heuristics.
    check_recursive_termination(&ctx.templates, &cyclic_sccs, &mut ctx.diagnostics);

    // Remix is_recursive into each recursive template's content_hash.
    // detect_recursive_structures() sets is_recursive after each template's initial
    // content_hash was computed, so without this step two templates with identical raw
    // content but different recursion status would hash identically — causing incorrect
    // incremental compilation cache hits. Non-recursive templates are untouched so
    // existing cache entries remain valid for them.
    for template in &mut ctx.templates {
        if template.is_recursive {
            template.content_hash = template.content_hash.combine(ContentHash::of(&[1u8]));
        }
    }

    // Check for duplicate function signatures: same name + same param types
    {
        let mut seen: HashMap<(String, Vec<Type>), usize> = HashMap::new();
        for (idx, f) in ctx.functions.iter().enumerate() {
            let key = (
                f.name.clone(),
                f.params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            );
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(idx);
            } else {
                ctx.diagnostics.push(Diagnostic::error(format!(
                    "duplicate function signature: {}({})",
                    f.name,
                    f.params
                        .iter()
                        .map(|(_, t)| format!("{}", t))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }
    }

    // Post-compilation pass: check field composition type compatibility.
    // For composed fields, if the body references other fields, verify that
    // the codomain of the inner field matches the domain of the outer field.
    {
        let field_registry: HashMap<&str, &CompiledField> =
            ctx.fields.iter().map(|f| (f.name.as_str(), f)).collect();

        for field in &ctx.fields {
            if let CompiledFieldSource::Composed { expr } = &field.source {
                check_field_composition_types(expr, &field_registry, &mut ctx.diagnostics);
            }
        }
    }

    // Purpose compilation pass: compile after templates so reflective schema queries
    // can resolve against TopologyTemplates.
    let compiled_purposes = {
        let purpose_template_registry: HashMap<String, &TopologyTemplate> = ctx
            .templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

        let mut purposes = Vec::new();
        for decl in &parsed.declarations {
            if let reify_syntax::Declaration::Purpose(purpose_def) = decl {
                let compiled = compile_purpose(
                    purpose_def,
                    &ctx.resolution_enums,
                    &ctx.resolution_functions,
                    &purpose_template_registry,
                    &ctx.unit_registry,
                    &mut ctx.diagnostics,
                );
                purposes.push(compiled);
            }
        }
        purposes
    };

    // Build a content-sensitive hash by combining the path with all compiled content.
    let content_hash = {
        let path_hash = ContentHash::of_str(&format!("{}", parsed.path));

        // Template content hashes
        let template_hashes = ctx.templates.iter().map(|t| t.content_hash);

        // Import path hashes
        let import_hashes = ctx.imports.iter().map(|i| ContentHash::of_str(&i.path));

        // Enum def hashes
        let enum_hashes = ctx.enum_defs.iter().map(|e| {
            let mut h = ContentHash::of_str(&e.name);
            for v in &e.variants {
                h = h.combine(ContentHash::of_str(v));
            }
            h
        });

        // Function content hashes
        let function_hashes = ctx.functions.iter().map(|f: &CompiledFunction| f.content_hash);

        // Trait content hashes
        let trait_hashes = ctx.trait_defs.iter().map(|t| t.content_hash);

        // Field content hashes
        let field_hashes = ctx.fields.iter().map(|f| f.content_hash);

        // Purpose content hashes
        let purpose_hashes = compiled_purposes.iter().map(|p| p.content_hash);

        // Unit content hashes
        let unit_hashes = ctx.compiled_units.iter().map(|u| u.content_hash);

        // Type alias content hashes (sorted by name for deterministic ordering)
        let mut alias_hash_pairs: Vec<_> = ctx
            .alias_registry
            .iter()
            .map(|a| (a.name.clone(), a.content_hash))
            .collect();
        alias_hash_pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let alias_hashes = alias_hash_pairs.into_iter().map(|(_, h)| h);

        let all_hashes = std::iter::once(path_hash)
            .chain(template_hashes)
            .chain(import_hashes)
            .chain(enum_hashes)
            .chain(function_hashes)
            .chain(trait_hashes)
            .chain(field_hashes)
            .chain(purpose_hashes)
            .chain(unit_hashes)
            .chain(alias_hashes);

        ContentHash::combine_all(all_hashes)
    };

    let type_aliases = ctx.alias_registry.into_compiled();

    CompiledModule {
        path: parsed.path.clone(),
        imports: ctx.imports,
        enum_defs: ctx.enum_defs,
        functions: ctx.functions,
        trait_defs: ctx.trait_defs,
        fields: ctx.fields,
        compiled_purposes,
        templates: ctx.templates,
        units: ctx.compiled_units,
        type_aliases,
        constraint_defs: ctx.constraint_defs,
        pragmas: parsed.pragmas.clone(),
        diagnostics: ctx.diagnostics,
        content_hash,
    }
}
