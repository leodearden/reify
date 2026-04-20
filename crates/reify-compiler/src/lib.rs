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
mod termination;
mod entity;
mod connect;
mod guards;
mod conformance;
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
    OptimizationObjective, RealizationNodeId, ResolvedFunction, SelectorKind, SourceSpan, Type,
    UnOp, Value, ValueCellId,
};

/// Format a constraint-def shadow-warning message for a name collision between two prelude modules.
///
/// `winner` is the first-imported module path string (whose definition is retained),
/// `loser` is the later-imported module path string (whose definition is silently discarded).
///
/// Exposed as `pub` so tests can build the expected string without duplicating the format
/// literal — a change to this function propagates to both production code and any test that
/// calls it, making the coupling explicit rather than hiding it behind substring assertions.
pub fn format_shadow_warning(name: &str, winner: &str, loser: &str) -> String {
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
                && !type_param_names.contains(name.as_str())
                && !enum_defs.iter().any(|e| e.name == *name)
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
    let mut imports = Vec::new();
    let mut functions = Vec::new();
    let mut fields = Vec::new();
    let mut templates = Vec::new();
    let mut diagnostics = Vec::new();

    // Forward parse errors as diagnostics
    for err in &parsed.errors {
        diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }

    // Validate module-level pragmas: warn on unknown names.
    const KNOWN_MODULE_PRAGMAS: &[&str] = &["no_prelude", "precision", "solver", "kernel", "version"];
    for pragma in &parsed.pragmas {
        if !KNOWN_MODULE_PRAGMAS.contains(&pragma.name.as_str()) {
            diagnostics.push(
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
    let mut enum_defs: Vec<reify_types::EnumDef> = Vec::new();
    let mut fn_refs: Vec<&reify_syntax::FnDef> = Vec::new();
    let mut trait_refs: Vec<&reify_syntax::TraitDecl> = Vec::new();
    let mut field_refs: Vec<&reify_syntax::FieldDef> = Vec::new();
    let mut unit_refs: Vec<&reify_syntax::UnitDecl> = Vec::new();
    let mut alias_refs: Vec<&reify_syntax::TypeAliasDecl> = Vec::new();
    // Unified entity namespace tracker (spec §4.2.1): structures, occurrences,
    // constraints, and fields all share the entity name space.
    // Maps name → (first_span, first_kind_label).
    let mut seen_entity_names: HashMap<String, (SourceSpan, &'static str)> = HashMap::new();

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Enum(e) => {
                enum_defs.push(reify_types::EnumDef {
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
                if let Some((first_span, first_kind)) = seen_entity_names.get(&field_def.name) {
                    // Duplicate entity name — emit error and skip
                    diagnostics.push(
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
                    seen_entity_names.insert(field_def.name.clone(), (field_def.span, "field"));
                    field_refs.push(field_def);
                }
            }
            reify_syntax::Declaration::Structure(structure) => {
                if let Some((first_span, first_kind)) = seen_entity_names.get(&structure.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    diagnostics.push(
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
                    seen_entity_names.insert(structure.name.clone(), (structure.span, "structure"));
                }
            }
            reify_syntax::Declaration::Occurrence(occurrence) => {
                if let Some((first_span, first_kind)) = seen_entity_names.get(&occurrence.name) {
                    // Duplicate entity name — emit error; pass 2 will skip compilation.
                    diagnostics.push(
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
                    seen_entity_names
                        .insert(occurrence.name.clone(), (occurrence.span, "occurrence"));
                }
            }
            reify_syntax::Declaration::Constraint(constraint) => {
                // Constraints reserve names in the entity namespace (spec §4.2.1)
                // even though constraint compilation is not yet implemented.
                if let Some((first_span, first_kind)) = seen_entity_names.get(&constraint.name) {
                    diagnostics.push(
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
                    seen_entity_names
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
    let mut unit_registry = UnitRegistry::new();

    // Seed prelude units into the registry so module-local code can reference them.
    // Only pub units are seeded (private units are module-internal).
    for prelude_module in prelude {
        let module_display = prelude_module.path.to_string();
        for cu in &prelude_module.units {
            if cu.is_pub {
                // Detect cross-prelude collision before overwriting: if another
                // prelude module already seeded this unit name, emit a warning.
                if let Some(existing) = unit_registry.lookup(&cu.name) {
                    let first_module: &str = existing
                        .source_module
                        .as_deref()
                        .unwrap_or("<unknown>");
                    diagnostics.push(
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
                unit_registry.seed_prelude_unit(UnitEntry {
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

    let mut compiled_units: Vec<CompiledUnit> = Vec::new();
    for unit_decl in &unit_refs {
        if let Some(entry) = compile_unit(unit_decl, &unit_registry, &mut diagnostics) {
            match unit_registry.register(entry) {
                Ok(()) => {
                    // Entry was registered; retrieve it to build CompiledUnit
                    let entry = unit_registry.lookup(&unit_decl.name).unwrap();
                    compiled_units.push(CompiledUnit {
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
                    let original = unit_registry.lookup(&dup_entry.name).unwrap();
                    match &original.source_module {
                        Some(m) if m.starts_with("std/") => {
                            // Original is a stdlib prelude unit.
                            // Emit a two-label diagnostic: primary is the user's
                            // duplicate decl; secondary is the prelude sentinel
                            // carrying provenance text.
                            diagnostics.push(
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
                            diagnostics.push(
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
                            diagnostics.push(
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
            diagnostics.push(
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
    let mut alias_registry = TypeAliasRegistry::new();
    let mut resolving = HashSet::new();
    for alias_decl in &alias_refs {
        resolve_alias_dfs(
            &alias_decl.name,
            &alias_decl_map,
            &mut alias_registry,
            &mut resolving,
            &mut diagnostics,
        );
    }

    // Build resolution_enums: prelude enums + module-local enums.
    // resolution_enums is used for type resolution during compilation;
    // only enum_defs (module-local) goes into the output CompiledModule.
    let mut resolution_enums: Vec<reify_types::EnumDef> = prelude
        .iter()
        .flat_map(|m| m.enum_defs.iter().cloned())
        .collect();
    resolution_enums.extend(enum_defs.iter().cloned());

    // Compile in dependency order after collecting all references:
    // 1. Functions (need all resolution_enums, plus prior compiled functions for self-reference)
    for fn_def in &fn_refs {
        if let Some(compiled_fn) = compile_function(
            fn_def,
            &resolution_enums,
            &functions,
            &alias_registry,
            &mut diagnostics,
        ) {
            functions.push(compiled_fn);
        }
    }

    // Build a resolution function list for compile-time overload resolution.
    // User functions appear first (shadowing priority); prelude functions with
    // distinct (name, arity, param_types) triples are appended. See
    // merge_prelude_functions() for the canonical shadow predicate.
    // `functions` (user-only) remains the output stored in CompiledModule.
    let resolution_functions: Vec<CompiledFunction> = {
        let prelude_fns: Vec<CompiledFunction> = prelude
            .iter()
            .flat_map(|m| m.functions.iter().cloned())
            .collect();
        merge_prelude_functions(&functions, &prelude_fns)
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
    let mut trait_defs = Vec::new();
    for trait_decl in &trait_refs {
        let compiled_trait = compile_trait(
            trait_decl,
            &resolution_enums,
            &alias_registry,
            &trait_names,
            &mut diagnostics,
        );
        trait_defs.push(compiled_trait);
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
    for t in &trait_defs {
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
                    &mut diagnostics,
                );
            }
        }
    }

    // 3. Fields (need all resolution_enums + all compiled functions)
    for field_def in &field_refs {
        let compiled = compile_field(
            field_def,
            &resolution_enums,
            &resolution_functions,
            &alias_registry,
            &mut diagnostics,
        );
        fields.push(compiled);
    }

    // Build a field registry so entity scopes can resolve field names.
    let field_registry: HashMap<String, &CompiledField> =
        fields.iter().map(|f| (f.name.clone(), f)).collect();

    // Compile all local constraint defs in a single pass.
    // Results are used both to populate the module output and to seed the registry.
    let constraint_defs: Vec<CompiledConstraintDef> = parsed
        .declarations
        .iter()
        .filter_map(|d| {
            if let reify_syntax::Declaration::Constraint(c) = d {
                Some(compile_constraint_def(
                    c,
                    &alias_registry,
                    &resolution_enums,
                    &trait_names,
                    &mut diagnostics,
                ))
            } else {
                None
            }
        })
        .collect();

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
                    diagnostics.push(Diagnostic::warning(format_shadow_warning(
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
    for cd in &constraint_defs {
        constraint_def_registry.insert(cd.name.clone(), cd);
    }

    let mut pending_bound_checks: Vec<PendingBoundCheck> = Vec::new();

    for decl in &parsed.declarations {
        match decl {
            reify_syntax::Declaration::Structure(structure) => {
                // Only compile the first definition; duplicates have a different
                // span than the one recorded in seen_entity_names.
                let is_first_def = seen_entity_names
                    .get(&structure.name)
                    .is_none_or(|(first_span, _)| *first_span == structure.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(structure);
                    let template = compile_entity(
                        &entity_ref,
                        EntityKind::Structure,
                        &resolution_enums,
                        &resolution_functions,
                        &trait_registry,
                        &trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &unit_registry,
                        &alias_registry,
                        &mut pending_bound_checks,
                        &mut diagnostics,
                        &templates,
                    );
                    templates.push(template);
                }
            }
            reify_syntax::Declaration::Enum(_) => {
                // Already collected in pre-pass above.
            }
            reify_syntax::Declaration::Import(import) => {
                imports.push(CompiledImport {
                    path: import.path.clone(),
                    kind: import.kind.clone(),
                    is_pub: import.is_pub,
                    span: import.span,
                });
                diagnostics.push(
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
                let is_first_def = seen_entity_names
                    .get(&occurrence.name)
                    .is_none_or(|(first_span, _)| *first_span == occurrence.span);
                if is_first_def {
                    let entity_ref = EntityDefRef::from(occurrence);
                    let template = compile_entity(
                        &entity_ref,
                        EntityKind::Occurrence,
                        &resolution_enums,
                        &resolution_functions,
                        &trait_registry,
                        &trait_names,
                        &field_registry,
                        &constraint_def_registry,
                        &unit_registry,
                        &alias_registry,
                        &mut pending_bound_checks,
                        &mut diagnostics,
                        &templates,
                    );
                    templates.push(template);
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
        let template_registry: HashMap<String, &TopologyTemplate> = templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

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
                        &mut diagnostics,
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
                        &mut diagnostics,
                        span,
                    );
                }
                PendingBoundCheck::TraitArgConformance {
                    target_name,
                    arg_name,
                    arg_type,
                    sub_name: _,
                    span,
                } => {
                    check_trait_arg_conformance(
                        &target_name,
                        &arg_name,
                        &arg_type,
                        span,
                        &template_registry,
                        &trait_registry,
                        &mut diagnostics,
                    );
                }
            }
        }
    }

    // Post-compilation pass: detect recursive sub-component cycles.
    // Build a directed reference graph from sub_components and run DFS to find cycles.
    // Tag participating templates with is_recursive=true and emit a warning diagnostic.
    let cyclic_sccs = scc::detect_recursive_structures(&mut templates, &mut diagnostics);

    // Post-compilation pass: verify recursive structures have valid termination conditions.
    // Emits errors for recursive subs without guards or with non-terminating guard heuristics.
    check_recursive_termination(&templates, &cyclic_sccs, &mut diagnostics);

    // Remix is_recursive into each recursive template's content_hash.
    // detect_recursive_structures() sets is_recursive after each template's initial
    // content_hash was computed, so without this step two templates with identical raw
    // content but different recursion status would hash identically — causing incorrect
    // incremental compilation cache hits. Non-recursive templates are untouched so
    // existing cache entries remain valid for them.
    for template in &mut templates {
        if template.is_recursive {
            template.content_hash = template.content_hash.combine(ContentHash::of(&[1u8]));
        }
    }

    // Check for duplicate function signatures: same name + same param types
    {
        let mut seen: HashMap<(String, Vec<Type>), usize> = HashMap::new();
        for (idx, f) in functions.iter().enumerate() {
            let key = (
                f.name.clone(),
                f.params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            );
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(idx);
            } else {
                diagnostics.push(Diagnostic::error(format!(
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
            fields.iter().map(|f| (f.name.as_str(), f)).collect();

        for field in &fields {
            if let CompiledFieldSource::Composed { expr } = &field.source {
                check_field_composition_types(expr, &field_registry, &mut diagnostics);
            }
        }
    }

    // Purpose compilation pass: compile after templates so reflective schema queries
    // can resolve against TopologyTemplates.
    let compiled_purposes = {
        let purpose_template_registry: HashMap<String, &TopologyTemplate> = templates
            .iter()
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .collect();

        let mut purposes = Vec::new();
        for decl in &parsed.declarations {
            if let reify_syntax::Declaration::Purpose(purpose_def) = decl {
                let compiled = compile_purpose(
                    purpose_def,
                    &resolution_enums,
                    &resolution_functions,
                    &purpose_template_registry,
                    &unit_registry,
                    &mut diagnostics,
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
        let template_hashes = templates.iter().map(|t| t.content_hash);

        // Import path hashes
        let import_hashes = imports.iter().map(|i| ContentHash::of_str(&i.path));

        // Enum def hashes
        let enum_hashes = enum_defs.iter().map(|e| {
            let mut h = ContentHash::of_str(&e.name);
            for v in &e.variants {
                h = h.combine(ContentHash::of_str(v));
            }
            h
        });

        // Function content hashes
        let function_hashes = functions.iter().map(|f: &CompiledFunction| f.content_hash);

        // Trait content hashes
        let trait_hashes = trait_defs.iter().map(|t| t.content_hash);

        // Field content hashes
        let field_hashes = fields.iter().map(|f| f.content_hash);

        // Purpose content hashes
        let purpose_hashes = compiled_purposes.iter().map(|p| p.content_hash);

        // Unit content hashes
        let unit_hashes = compiled_units.iter().map(|u| u.content_hash);

        // Type alias content hashes (sorted by name for deterministic ordering)
        let mut alias_hash_pairs: Vec<_> = alias_registry
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

    let type_aliases = alias_registry.into_compiled();

    CompiledModule {
        path: parsed.path.clone(),
        imports,
        enum_defs,
        functions,
        trait_defs,
        fields,
        compiled_purposes,
        templates,
        units: compiled_units,
        type_aliases,
        constraint_defs,
        pragmas: parsed.pragmas.clone(),
        diagnostics,
        content_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_kind_display() {
        assert_eq!(EntityKind::Structure.to_string(), "structure");
        assert_eq!(EntityKind::Occurrence.to_string(), "occurrence");
        assert_eq!(EntityKind::Structure, EntityKind::Structure);
        assert_ne!(EntityKind::Structure, EntityKind::Occurrence);
        assert_eq!(format!("{:?}", EntityKind::Structure), "Structure");
    }

    // --- Step 21: Verify new geometry function names are recognized ---

    #[test]
    fn compile_geometry_linear_pattern_recognized() {
        assert!(is_geometry_function("linear_pattern"));
    }

    #[test]
    fn compile_geometry_circular_pattern_recognized() {
        assert!(is_geometry_function("circular_pattern"));
    }

    #[test]
    fn compile_geometry_mirror_recognized() {
        assert!(is_geometry_function("mirror"));
    }

    #[test]
    fn compile_geometry_loft_recognized() {
        assert!(is_geometry_function("loft"));
    }

    #[test]
    fn compile_geometry_shell_recognized() {
        assert!(is_geometry_function("shell"));
    }

    #[test]
    fn compile_geometry_thicken_recognized() {
        assert!(is_geometry_function("thicken"));
    }

    #[test]
    fn compile_geometry_draft_recognized() {
        assert!(is_geometry_function("draft"));
    }

    // --- Verify new geometry function calls compile into realizations ---

    #[test]
    fn compile_linear_pattern_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern(w, 1, 0, 0, 4, 20)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // linear_pattern is a geometry function, so should produce a realization
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for linear_pattern call, got {}",
            template.realizations.len()
        );
        // Verify it's a Pattern op with Linear kind
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Pattern {
                    kind: PatternKind::Linear,
                    ..
                }
            ),
            "expected Pattern(Linear), got {:?}",
            op
        );
    }

    #[test]
    fn compile_mirror_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let mirrored = mirror(w, 0, 0, 0, 1, 0, 0)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_mirror"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for mirror call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Pattern {
                    kind: PatternKind::Mirror,
                    ..
                }
            ),
            "expected Pattern(Mirror), got {:?}",
            op
        );
    }

    #[test]
    fn compile_linear_pattern_2d_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern_2d(w, 1, 0, 0, 3, 20, 0, 1, 0, 4, 30)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat2d"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for linear_pattern_2d call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Pattern {
                    kind: PatternKind::Linear2D,
                    ..
                }
            ),
            "expected Pattern(Linear2D), got {:?}",
            op
        );
        // Verify correct number of named args (11: target + 10 params)
        if let CompiledGeometryOp::Pattern { args, .. } = op {
            assert_eq!(args.len(), 11, "expected 11 args, got {}", args.len());
            assert_eq!(args[0].0, "target");
            assert_eq!(args[1].0, "dx1");
            assert_eq!(args[4].0, "count1");
            assert_eq!(args[5].0, "spacing1");
            assert_eq!(args[6].0, "dx2");
            assert_eq!(args[9].0, "count2");
            assert_eq!(args[10].0, "spacing2");
        }
    }

    #[test]
    fn compile_linear_pattern_2d_wrong_arity_produces_diagnostic() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern_2d(w, 1, 0, 0, 3, 20)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat2d_err"));
        assert!(parsed.errors.is_empty());
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("linear_pattern_2d")
                    && d.message.contains("11 arguments")),
            "expected arity diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_arbitrary_pattern_produces_realization() {
        // arbitrary_pattern(target, dx1, dy1, dz1, dx2, dy2, dz2) = 7 args = target + 2 triples
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0, 0, 0, 20, 0)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for arbitrary_pattern call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Pattern {
                    kind: PatternKind::Arbitrary,
                    ..
                }
            ),
            "expected Pattern(Arbitrary), got {:?}",
            op
        );
        // Verify args: target + 6 transform coords (2 triples)
        if let CompiledGeometryOp::Pattern { args, .. } = op {
            assert_eq!(args.len(), 7, "expected 7 args, got {}", args.len());
            assert_eq!(args[0].0, "target");
            assert_eq!(args[1].0, "t0_dx");
            assert_eq!(args[2].0, "t0_dy");
            assert_eq!(args[3].0, "t0_dz");
            assert_eq!(args[4].0, "t1_dx");
        }
    }

    #[test]
    fn compile_arbitrary_pattern_too_few_args_produces_diagnostic() {
        // Needs at least 4 args (target + 1 triple)
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat_err1"));
        assert!(parsed.errors.is_empty());
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("arbitrary_pattern")),
            "expected arity diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_arbitrary_pattern_non_triple_args_produces_diagnostic() {
        // 6 args = target + 5 coords, but (6-1)%3 != 0
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0, 0, 5, 0)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat_err2"));
        assert!(parsed.errors.is_empty());
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("arbitrary_pattern")),
            "expected arity diagnostic for non-triple args, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_loft_produces_realization() {
        let source = r#"structure S {
    param r: Scalar = 10mm
    let swept = loft(r, r)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_loft"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for loft call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Sweep {
                    kind: SweepKind::Loft,
                    ..
                }
            ),
            "expected Sweep(Loft), got {:?}",
            op
        );
    }

    #[test]
    fn compile_shell_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let hollowed = shell(w, 1)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_shell"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for shell call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Modify {
                    kind: ModifyKind::Shell,
                    ..
                }
            ),
            "expected Modify(Shell), got {:?}",
            op
        );
    }

    #[test]
    fn compile_thicken_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let thickened = thicken(w, 2)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_thicken"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for thicken call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Modify {
                    kind: ModifyKind::Thicken,
                    ..
                }
            ),
            "expected Modify(Thicken), got {:?}",
            op
        );
    }

    #[test]
    fn compile_draft_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let drafted = draft(w, 0.1, w)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_draft"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for draft call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Modify {
                    kind: ModifyKind::Draft,
                    ..
                }
            ),
            "expected Modify(Draft), got {:?}",
            op
        );
    }

    #[test]
    fn compile_circular_pattern_produces_realization() {
        let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = circular_pattern(w, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_circpat"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for circular_pattern call, got {}",
            template.realizations.len()
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Pattern {
                    kind: PatternKind::Circular,
                    ..
                }
            ),
            "expected Pattern(Circular), got {:?}",
            op
        );
    }

    // --- Boolean function recognition tests (step-1) ---

    #[test]
    fn compile_geometry_union_recognized() {
        assert!(is_geometry_function("union"));
    }

    #[test]
    fn compile_geometry_intersection_recognized() {
        assert!(is_geometry_function("intersection"));
    }

    #[test]
    fn compile_geometry_difference_recognized() {
        assert!(is_geometry_function("difference"));
    }

    #[test]
    fn compile_geometry_union_all_recognized() {
        assert!(is_geometry_function("union_all"));
    }

    #[test]
    fn compile_geometry_intersection_all_recognized() {
        assert!(is_geometry_function("intersection_all"));
    }

    #[test]
    fn compile_geometry_linear_pattern_2d_recognized() {
        assert!(is_geometry_function("linear_pattern_2d"));
    }

    #[test]
    fn compile_geometry_arbitrary_pattern_recognized() {
        assert!(is_geometry_function("arbitrary_pattern"));
    }

    // --- Binary boolean op compilation tests (step-3) ---

    #[test]
    fn compile_union_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm), box(20mm, 20mm, 20mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // union(box, box) should produce 1 realization with 3 ops
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization, got {}",
            template.realizations.len()
        );
        let ops = &template.realizations[0].operations;
        assert_eq!(
            ops.len(),
            3,
            "expected 3 ops (box, box, union), got {}",
            ops.len()
        );
        assert!(
            matches!(
                ops[0],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Primitive::Box at ops[0], got {:?}",
            ops[0]
        );
        assert!(
            matches!(
                ops[1],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Primitive::Box at ops[1], got {:?}",
            ops[1]
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Union, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    // --- Nested boolean compilation test (step-11) ---

    #[test]
    fn compile_nested_boolean_produces_five_ops() {
        // union(difference(box, cylinder), sphere)
        // Expected flat ops:
        //   0: Box
        //   1: Cylinder
        //   2: Boolean{Difference, Step(0), Step(1)}
        //   3: Sphere
        //   4: Boolean{Union, Step(2), Step(3)}
        let source = r#"structure S {
    let r = union(difference(box(20mm, 20mm, 20mm), cylinder(5mm, 20mm)), sphere(10mm))
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_nested_bool"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(
            ops.len(),
            5,
            "expected 5 ops for nested boolean, got {}: {:?}",
            ops.len(),
            ops
        );
        assert!(
            matches!(
                ops[0],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "ops[0] expected Box, got {:?}",
            ops[0]
        );
        assert!(
            matches!(
                ops[1],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Cylinder,
                    ..
                }
            ),
            "ops[1] expected Cylinder, got {:?}",
            ops[1]
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Difference,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "ops[2] expected Boolean{{Difference,0,1}}, got {:?}",
            ops[2]
        );
        assert!(
            matches!(
                ops[3],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Sphere,
                    ..
                }
            ),
            "ops[3] expected Sphere, got {:?}",
            ops[3]
        );
        assert!(
            matches!(
                ops[4],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(2),
                    right: GeomRef::Step(3)
                }
            ),
            "ops[4] expected Boolean{{Union,2,3}}, got {:?}",
            ops[4]
        );
    }

    // --- Error case tests for boolean arg validation (step-9, step-10) ---

    #[test]
    fn compile_union_wrong_arity_emits_diagnostic() {
        // union(box(...)) with 1 arg should fail with arity diagnostic
        let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm))
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_union_arity"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // Should produce no realization (compilation failed)
        assert_eq!(
            template.realizations.len(),
            0,
            "expected 0 realizations for wrong-arity union, got {}",
            template.realizations.len()
        );
        // Should have a diagnostic mentioning "expects 2 arguments"
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("expects 2 arguments")),
            "expected 'expects 2 arguments' diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_union_non_geometry_arg_emits_diagnostic() {
        // union(42, box(...)) — first arg is a scalar literal, not geometry
        // The parser may reject bare number literals in function position,
        // so we use a param reference (Scalar param) which is a valid expr but not geometry.
        let source = r#"structure S {
    param w: Scalar = 10mm
    let r = union(w, box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_union_nongeom"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        // Should produce no realization (compilation failed)
        assert_eq!(
            template.realizations.len(),
            0,
            "expected 0 realizations for non-geometry arg union, got {}",
            template.realizations.len()
        );
        // Should have at least one diagnostic
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected diagnostics for non-geometry arg, got none"
        );
    }

    // --- union_all / intersection_all fold compilation tests (step-7) ---

    #[test]
    fn compile_union_all_three_args_produces_five_ops() {
        // union_all(a, b, c) → left-fold: Union(Union(a,b), c)
        // ops: Box_a, Box_b, Boolean{Union,Step(0),Step(1)}, Box_c, Boolean{Union,Step(2),Step(3)}
        let source = r#"structure S {
    let r = union_all(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union_all"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(
            ops.len(),
            5,
            "expected 5 ops for union_all(3 args), got {}: {:?}",
            ops.len(),
            ops
        );
        // ops[0]: Box
        assert!(
            matches!(
                ops[0],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Box at ops[0]"
        );
        // ops[1]: Box
        assert!(
            matches!(
                ops[1],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Box at ops[1]"
        );
        // ops[2]: Union(Step(0), Step(1))
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Union,Step(0),Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
        // ops[3]: Box
        assert!(
            matches!(
                ops[3],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Box at ops[3]"
        );
        // ops[4]: Union(Step(2), Step(3))
        assert!(
            matches!(
                ops[4],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(2),
                    right: GeomRef::Step(3)
                }
            ),
            "expected Boolean{{Union,Step(2),Step(3)}} at ops[4], got {:?}",
            ops[4]
        );
    }

    // --- difference and intersection compilation tests (step-5, step-6) ---

    #[test]
    fn compile_difference_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = difference(box(20mm, 20mm, 20mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_diff"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box, box, difference)");
        assert!(
            matches!(
                ops[0],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Box at ops[0]"
        );
        assert!(
            matches!(
                ops[1],
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            ),
            "expected Box at ops[1]"
        );
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Difference,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    #[test]
    fn compile_intersection_nested_calls_produces_three_ops() {
        let source = r#"structure S {
    let r = intersection(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_isect"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box, box, intersection)");
        assert!(
            matches!(
                ops[2],
                CompiledGeometryOp::Boolean {
                    op: BooleanOp::Intersection,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1)
                }
            ),
            "expected Boolean{{Intersection, Step(0), Step(1)}} at ops[2], got {:?}",
            ops[2]
        );
    }

    // --- Step 11: Directly test the catch-all branch in compile_geometry_call ---

    #[test]
    fn unsupported_geometry_fn_emits_diagnostic() {
        // Fabricate a FunctionCall expr with a name that is NOT in the
        // compile_geometry_call match arms (e.g., "make_cube").  This directly
        // exercises the `_ =>` catch-all branch added in step-4.
        let expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::FunctionCall {
                name: "make_cube".to_string(),
                args: vec![reify_syntax::Expr {
                    kind: reify_syntax::ExprKind::NumberLiteral(1.0),
                    span: reify_types::SourceSpan::new(0, 1),
                }],
            },
            span: reify_types::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_types::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        let geometry_lets: HashMap<&str, &reify_syntax::Expr> = HashMap::new();
        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            0,
            &geometry_lets,
            &mut HashSet::new(),
        );

        assert!(
            result.is_none(),
            "unrecognized geometry fn should return None"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("unsupported geometry function")),
            "expected 'unsupported geometry function' diagnostic, got: {:?}",
            diagnostics
        );
    }

    // --- Sweep (pipe) compiler tests (task-310 step-13) ---

    #[test]
    fn is_geometry_function_sweep() {
        assert!(is_geometry_function("sweep"));
    }

    #[test]
    fn compile_sweep_produces_sweep_kind() {
        // sweep(profile, path) = 2 args, both geometry refs
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p, p)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            1,
            "expected 1 realization for sweep call"
        );
        let op = &template.realizations[0].operations[0];
        assert!(
            matches!(
                op,
                CompiledGeometryOp::Sweep {
                    kind: SweepKind::Sweep,
                    ..
                }
            ),
            "expected Sweep(Sweep), got {:?}",
            op
        );
        // Both profile and path should be in profiles as GeomRefs
        if let CompiledGeometryOp::Sweep { profiles, .. } = op {
            assert_eq!(
                profiles.len(),
                2,
                "sweep should have 2 profiles (profile + path), got {}",
                profiles.len()
            );
            assert_eq!(profiles[0], GeomRef::Step(0));
            assert_eq!(profiles[1], GeomRef::Step(1));
        }
    }

    #[test]
    fn compile_sweep_wrong_arg_count() {
        // sweep with 1 arg (should need 2)
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep_bad"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            !compiled.diagnostics.is_empty(),
            "expected diagnostics for wrong arg count"
        );
    }

    // --- Transform compiler tests (task-377) ---

    #[test]
    fn user_function_shadowing_scale_no_realizations() {
        // A user-defined function named `scale` with matching arity (2 args)
        // should shadow the geometry built-in and produce 0 realizations.
        let source = r#"
fn scale(x: Real, factor: Real) -> Real { x * factor }

structure S {
    param p: Scalar = 5mm
    let result = scale(p, 2)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_shadow_scale"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(
            template.realizations.len(),
            0,
            "user-function shadowing: scale(p, 2) with user fn should produce 0 realizations"
        );
    }

    #[test]
    fn compile_translate_wrong_arg_count() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = translate(p, p)
}"#;
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_translate_bad"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("translate()")),
            "expected translate() arg-count diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_rotate_wrong_arg_count() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate(p, p)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_rotate_bad"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("rotate()")),
            "expected rotate() arg-count diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_scale_wrong_arg_count() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = scale(p, p, p)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_scale_bad"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("scale()")),
            "expected scale() arg-count diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_rotate_around_wrong_arg_count() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate_around(p, p, p)
}"#;
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_rotate_around_bad"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        assert!(
            compiled
                .diagnostics
                .iter()
                .any(|d| d.message.contains("rotate_around()")),
            "expected rotate_around() arg-count diagnostic, got: {:?}",
            compiled.diagnostics
        );
    }

    #[test]
    fn compile_translate_arg_ordering() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = translate(p, p, p, p)
}"#;
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_translate_args"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let op = &template.realizations[0].operations[0];
        if let CompiledGeometryOp::Transform { kind, args, .. } = op {
            assert_eq!(*kind, TransformKind::Translate);
            let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["target", "dx", "dy", "dz"]);
        } else {
            panic!("expected Transform, got {:?}", op);
        }
    }

    #[test]
    fn compile_rotate_arg_ordering() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate(p, p, p, p, p)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_rotate_args"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let op = &template.realizations[0].operations[0];
        if let CompiledGeometryOp::Transform { kind, args, .. } = op {
            assert_eq!(*kind, TransformKind::Rotate);
            let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["target", "ax", "ay", "az", "angle"]);
        } else {
            panic!("expected Transform, got {:?}", op);
        }
    }

    #[test]
    fn compile_scale_arg_ordering() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = scale(p, p)
}"#;
        let parsed =
            reify_syntax::parse(source, reify_types::ModulePath::single("test_scale_args"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let op = &template.realizations[0].operations[0];
        if let CompiledGeometryOp::Transform { kind, args, .. } = op {
            assert_eq!(*kind, TransformKind::Scale);
            let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["target", "factor"]);
        } else {
            panic!("expected Transform, got {:?}", op);
        }
    }

    #[test]
    fn compile_rotate_around_arg_ordering() {
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate_around(p, p, p, p, p, p, p, p)
}"#;
        let parsed = reify_syntax::parse(
            source,
            reify_types::ModulePath::single("test_rotate_around_args"),
        );
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        let op = &template.realizations[0].operations[0];
        if let CompiledGeometryOp::Transform { kind, args, .. } = op {
            assert_eq!(*kind, TransformKind::RotateAround);
            let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(
                names,
                vec!["target", "px", "py", "pz", "ax", "ay", "az", "angle"]
            );
        } else {
            panic!("expected Transform, got {:?}", op);
        }
    }

    // --- Bug fix tests: GeomRef::Step(0) fallback hardcoding (task-612/task-1732) ---

    #[test]
    fn sweep_non_geom_profile_fallback_uses_step_offset() {
        // sweep() where the profile arg is a literal number (not a geometry expression).
        // When step_offset=3, the profile GeomRef fallback should be Step(3), not Step(0).
        // The path arg is also a literal, so it falls back to Step(step_offset + 1) = Step(4).
        let expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::FunctionCall {
                name: "sweep".to_string(),
                args: vec![
                    reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(1.0),
                        span: reify_types::SourceSpan::new(0, 1),
                    },
                    reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(2.0),
                        span: reify_types::SourceSpan::new(0, 1),
                    },
                ],
            },
            span: reify_types::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_types::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_syntax::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            3, // step_offset = 3
            &geometry_lets,
            &mut HashSet::new(),
        );

        let ops = result.expect("sweep() should produce ops even with non-geometry args");
        let sweep_op = ops.last().expect("should have at least one op");
        match sweep_op {
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                profiles,
                ..
            } => {
                assert_eq!(profiles.len(), 2, "sweep should have 2 profiles (profile, path)");
                assert_eq!(
                    profiles[0],
                    GeomRef::Step(3),
                    "sweep profile fallback should be Step(step_offset=3), not {:?}",
                    profiles[0]
                );
            }
            other => panic!("expected Sweep(Sweep), got {:?}", other),
        }
    }

    #[test]
    fn loft_non_geom_args_fallback_uses_step_offset() {
        // loft() with 3 literal-number args (not geometry expressions).
        // When step_offset=5:
        //   - The fallback GeomRef for profile i is GeomRef::Step(5 + i) — unique per
        //     profile, preserving loft's "distinct cross-sections" semantics in the
        //     fallback (consistent with sweep()'s profile=step_offset, path=step_offset+1
        //     convention applied per profile).
        //   - The fallback is silent: no per-argument diagnostic is emitted, matching
        //     the geom_ref convention used by extrude/revolve/translate/etc.
        //   - Ops are still produced (fallback refs allow compilation to continue).
        let expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::FunctionCall {
                name: "loft".to_string(),
                args: vec![
                    reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(1.0),
                        span: reify_types::SourceSpan::new(0, 1),
                    },
                    reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(2.0),
                        span: reify_types::SourceSpan::new(0, 1),
                    },
                    reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::NumberLiteral(3.0),
                        span: reify_types::SourceSpan::new(0, 1),
                    },
                ],
            },
            span: reify_types::SourceSpan::new(0, 10),
        };
        let scope = CompilationScope::new("test");
        let enum_defs: Vec<reify_types::EnumDef> = vec![];
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let geometry_lets: HashMap<&str, &reify_syntax::Expr> = HashMap::new();

        let result = compile_geometry_call(
            &expr,
            &scope,
            &enum_defs,
            &functions,
            &mut diagnostics,
            5, // step_offset = 5
            &geometry_lets,
            &mut HashSet::new(),
        );

        // loft() with non-geometry args should still produce an op (with fallback refs)
        let ops = result.expect("loft() should produce ops even with non-geometry args");
        let loft_op = ops.last().expect("should have at least one op");
        match loft_op {
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                profiles,
                ..
            } => {
                assert_eq!(profiles.len(), 3, "loft should have 3 profiles");
                for (i, profile) in profiles.iter().enumerate() {
                    assert_eq!(
                        *profile,
                        GeomRef::Step(5 + i),
                        "loft fallback for profile {} should be Step(step_offset + {0} = {}), not {:?}",
                        i,
                        5 + i,
                        profile
                    );
                }
            }
            other => panic!("expected Sweep(Loft), got {:?}", other),
        }

        // No per-argument geometry-expression diagnostics should be emitted by the
        // loft fallback path — silent fallback matches the geom_ref convention.
        let geom_expr_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("must be a geometry expression"))
            .collect();
        assert!(
            geom_expr_diags.is_empty(),
            "expected silent fallback (no per-arg diagnostics), got: {:?}",
            geom_expr_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn loft_nested_in_union_correct_step_refs() {
        // End-to-end regression: loft nested inside union gets step_offset=1
        // (after the box op at index 0).  After the fix, loft profiles reference
        // Step(1) not Step(0).  p is a scalar param — not a geometry ref — so the
        // silent fallback fires and we can observe the corrected step index.
        let source = r#"structure S {
    param p: Scalar = 5mm
    let result = union(box(10mm, 10mm, 10mm), loft(p, p))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_loft_union"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "expected 1 realization");
        // ops layout: [0]=box, [1]=loft, [2]=Boolean(Union, Step(0), Step(1))
        let ops = &template.realizations[0].operations;
        assert_eq!(ops.len(), 3, "expected 3 ops (box + loft + union), got {}", ops.len());
        // ops[0] must be the Box primitive.
        assert!(
            matches!(&ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
            "expected ops[0] to be a Box primitive, got {:?}",
            ops[0]
        );
        // The loft op is at index 1.
        if let CompiledGeometryOp::Sweep { kind, profiles, .. } = &ops[1] {
            assert_eq!(*kind, SweepKind::Loft, "expected Loft kind at ops[1]");
            for (i, profile) in profiles.iter().enumerate() {
                assert_eq!(
                    *profile,
                    GeomRef::Step(1 + i),
                    "loft profile[{}] inside union should be Step({}) not Step(0), got {:?}",
                    i,
                    1 + i,
                    profile
                );
            }
        } else {
            panic!("expected Sweep(Loft) at ops[1], got {:?}", ops[1]);
        }
    }

    // --- compile_curve_op direct tests (step-1) ---

    fn scalar_literal(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::Real)
    }

    #[test]
    fn compile_curve_op_line_segment_direct() {
        let args: Vec<CompiledExpr> = (1..=6).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op("line_segment", args.clone(), &mut diagnostics);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_curve_op line_segment should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Curve { kind: CurveKind::LineSegment, args: op_args } => {
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["x1", "y1", "z1", "x2", "y2", "z2"]);
                assert_eq!(op_args.len(), 6);
            }
            other => panic!("expected Curve(LineSegment), got {:?}", other),
        }
    }

    #[test]
    fn compile_curve_op_wrong_arg_count() {
        let args: Vec<CompiledExpr> = (1..=3).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_curve_op("line_segment", args, &mut diagnostics);
        assert!(result.is_none(), "expected None for wrong arg count");
        assert!(!diagnostics.is_empty(), "expected diagnostic for wrong arg count");
    }

    // --- compile_transform_op direct tests (step-3) ---

    #[test]
    fn compile_transform_op_translate_direct() {
        // translate(target, dx, dy, dz) — 4 args
        let args: Vec<CompiledExpr> = (1..=4).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(0);
        let result = compile_transform_op("translate", args, target.clone(), &mut diagnostics, vec![]);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_transform_op translate should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Transform { kind: TransformKind::Translate, target: op_target, args: op_args } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "dx", "dy", "dz"]);
            }
            other => panic!("expected Transform(Translate), got {:?}", other),
        }
    }

    #[test]
    fn compile_transform_op_wrong_arg_count() {
        // translate expects 4 args; pass 2
        let args: Vec<CompiledExpr> = (1..=2).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = compile_transform_op("translate", args, GeomRef::Step(0), &mut diagnostics, vec![]);
        assert!(result.is_none(), "expected None for wrong arg count");
        assert!(!diagnostics.is_empty(), "expected diagnostic for wrong arg count");
    }

    // --- compile_modify_op direct tests (step-5) ---

    #[test]
    fn compile_modify_op_shell_direct() {
        // shell(target, thickness, face_0) — 3 args, target = GeomRef::Step(5)
        let args: Vec<CompiledExpr> = (1..=3).map(|i| scalar_literal(i as f64)).collect();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let target = GeomRef::Step(5);
        let span = SourceSpan::new(0, 0);
        let result = compile_modify_op("shell", args, target.clone(), span, &mut diagnostics, vec![]);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {:?}", diagnostics);
        let ops = result.expect("compile_modify_op shell should return Some");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify { kind: ModifyKind::Shell, target: op_target, args: op_args } => {
                assert_eq!(*op_target, target);
                let names: Vec<&str> = op_args.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(names, vec!["target", "thickness", "face_0"]);
            }
            other => panic!("expected Modify(Shell), got {:?}", other),
        }
    }

    #[test]
    fn compile_modify_op_chamfer_non_geometry_target_fallback() {
        // chamfer is registered in geometry_arg_indices() — so geom_ref(0) is used.
        // When the first arg is a scalar param (not a geometry let), the resolution
        // block finds no ops for it, so geom_ref(0) falls back to GeomRef::Step(step_offset).
        // With no sub-ops, step_offset == 0, so the target is GeomRef::Step(0).
        let source = r#"structure S {
    param target: Scalar = 5mm
    param dist: Scalar = 2mm
    let result = chamfer(target, dist)
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_chamfer_step0"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        assert!(compiled.diagnostics.is_empty(), "unexpected diagnostics: {:?}", compiled.diagnostics);
        let ops = &compiled.templates[0].realizations[0].operations;
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            CompiledGeometryOp::Modify { kind: ModifyKind::Chamfer, target: op_target, .. } => {
                // Non-geometry target → geom_ref(0) falls back to GeomRef::Step(0)
                assert_eq!(*op_target, GeomRef::Step(0),
                    "chamfer with non-geometry target should fall back to GeomRef::Step(0), got {:?}", op_target);
            }
            other => panic!("expected Modify(Chamfer), got {:?}", other),
        }
    }

    // --- compile_boolean_op regression guards (step-7) ---
    // These tests verify the full compile pipeline for boolean ops.
    // They pass before extraction (boolean code is still inline) and remain
    // as regression guards after step-8 extracts it into compile_boolean_op.

    #[test]
    fn compile_boolean_op_union_via_compile() {
        let source = r#"structure S {
    let a = union(sphere(1), cylinder(1, 2))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_bool_union"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1);
        let ops = &template.realizations[0].operations;
        // Expected: Primitive(Sphere), Primitive(Cylinder), Boolean{Union, Step(0), Step(1)}
        assert_eq!(ops.len(), 3, "expected 3 ops, got {}: {:?}", ops.len(), ops);
        assert!(matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
        assert!(matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }));
        match &ops[2] {
            CompiledGeometryOp::Boolean { op: BooleanOp::Union, left: GeomRef::Step(0), right: GeomRef::Step(1) } => {}
            other => panic!("expected Boolean{{Union, Step(0), Step(1)}}, got {:?}", other),
        }
    }

    #[test]
    fn compile_boolean_op_union_all_via_compile() {
        let source = r#"structure S {
    let a = union_all(sphere(1), sphere(2), sphere(3))
}"#;
        let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_bool_union_all"));
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let compiled = compile(&parsed);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1);
        let ops = &template.realizations[0].operations;
        // Expected left-fold: Sphere(0), Sphere(1), Boolean{Union,0,1}(2), Sphere(3), Boolean{Union,2,3}(4)
        assert_eq!(ops.len(), 5, "expected 5 ops, got {}: {:?}", ops.len(), ops);
        assert!(matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
        assert!(matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
        assert!(matches!(ops[2], CompiledGeometryOp::Boolean { op: BooleanOp::Union, .. }));
        assert!(matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
        assert!(matches!(ops[4], CompiledGeometryOp::Boolean { op: BooleanOp::Union, .. }));
    }
}
