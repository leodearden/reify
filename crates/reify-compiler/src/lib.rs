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

    // Forward parse errors as diagnostics.
    compile_builder::pre_pass::forward_parse_errors(&mut ctx, parsed);

    // Validate module-level pragmas: warn on unknown names.
    compile_builder::pre_pass::validate_module_pragmas(&mut ctx, parsed);

    // Handle #no_prelude: suppress ALL prelude-dependent behavior by shadowing
    // the prelude parameter with an empty slice. This affects unit seeding,
    // trait/enum/function resolution, and constraint def imports.
    let prelude: &[&CompiledModule] = compile_builder::pre_pass::effective_prelude(parsed, prelude);

    // Consolidated pre-pass: iterate declarations once, collecting references
    // for deferred compilation and seeding the entity-namespace tracker.
    let decl_refs = compile_builder::pre_pass::collect_decl_refs(&mut ctx, parsed);

    // Compile unit declarations in source order (so later units can reference earlier ones).
    // Unit hashes are included in the module content hash. Seeds prelude units first.
    compile_builder::units_phase::phase_units(&mut ctx, prelude, &decl_refs.unit_refs);

    // Compile type alias declarations via DFS resolution with cycle detection.
    compile_builder::aliases_phase::phase_aliases(&mut ctx, &decl_refs.alias_refs);

    // Build resolution_enums: prelude enums + module-local enums.
    compile_builder::enums_phase::build_resolution_enums(&mut ctx, prelude);

    // Compile in dependency order after collecting all references:
    // 1. Functions (phase-7): compile user fns, then build ctx.resolution_functions.
    compile_builder::functions_phase::phase_functions(&mut ctx, prelude, &decl_refs.fn_refs);

    // 2. Traits (phase-8): compile traits + populate trait_names + emit
    //    deprecation warnings. Returns trait_names for downstream phases.
    let trait_names =
        compile_builder::traits_phase::phase_traits(&mut ctx, prelude, &decl_refs.trait_refs);

    // 3. Fields (need all resolution_enums + all compiled functions)
    compile_builder::fields_phase::phase_fields(&mut ctx, &decl_refs.field_refs);

    // Compile all local constraint defs in a single pass. Also emits
    // one-time shadow warnings for cross-prelude name collisions.
    compile_builder::defs_phase::phase_constraint_defs(
        &mut ctx,
        parsed,
        prelude,
        &trait_names,
    );

    // Compile structures / occurrences and forward imports.
    compile_builder::entities_phase::phase_entities(&mut ctx, parsed, &trait_names, prelude);

    // Post-compilation pass: run deferred bound checks now that all structures
    // are compiled and available in the template registry.
    compile_builder::entities_phase::phase_pending_bound_checks(&mut ctx, prelude);

    // Post-compilation pass: detect recursive sub-component cycles,
    // validate termination conditions, and remix is_recursive into each
    // recursive template's content_hash.
    compile_builder::post_passes::phase_recursion_detection(&mut ctx);

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
