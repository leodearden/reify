mod annotations;
mod compile_builder;
mod conformance;
mod connect;
mod constants;
mod entity;
mod expr;
mod functions;
mod geometry;
mod geometry_boolean;
mod geometry_curve;
mod geometry_modify;
mod geometry_transform;
mod guards;
pub mod module_dag;
mod scc;
mod scope;
pub mod si_units;
pub mod stdlib_loader;
mod termination;
mod trait_requirements;
mod traits;
mod type_compat;
mod type_resolution;
mod types;
mod units;

pub use type_compat::{implicitly_converts_to, type_compatible};
pub use types::*;

// Re-export submodule items for internal cross-module access via `use super::*;`
pub(crate) use annotations::*;
pub(crate) use conformance::*;
pub(crate) use connect::*;
pub(crate) use entity::*;
pub(crate) use expr::*;
#[allow(unused_imports)]
pub(crate) use functions::*;
pub(crate) use geometry::*;
pub(crate) use geometry_boolean::*;
pub(crate) use geometry_curve::*;
pub(crate) use geometry_modify::*;
pub(crate) use geometry_transform::*;
pub(crate) use guards::*;
pub(crate) use scope::*;
#[allow(unused_imports)]
pub(crate) use termination::*;
pub(crate) use trait_requirements::*;
#[allow(unused_imports)]
pub(crate) use traits::*;
#[allow(unused_imports)]
pub(crate) use type_compat::*;
pub(crate) use type_resolution::*;
pub(crate) use units::*;
pub use units::{UnitEntry, UnitRegistry};

use std::collections::{HashMap, HashSet};

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ConstraintNodeId, ContentHash, DeterminacyPredicateKind,
    Diagnostic, DiagnosticLabel, DimensionVector, FIELD_ENTITY_PREFIX, OptimizationObjective,
    RealizationNodeId, ResolvedFunction, SelectorKind, Severity, SourceSpan, TAG_CONDITIONAL,
    TAG_FUNCTION_CALL, TAG_MATCH, TAG_USER_FUNCTION_CALL, Type, UnOp, Value, ValueCellId,
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
                && uf
                    .params
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
    compile_builder::defs_phase::phase_constraint_defs(&mut ctx, parsed, prelude, &trait_names);

    // Compile structures / occurrences and forward imports.
    compile_builder::entities_phase::phase_entities(&mut ctx, parsed, &trait_names, prelude);

    // Post-compilation pass: run deferred bound checks now that all structures
    // are compiled and available in the template registry.
    compile_builder::entities_phase::phase_pending_bound_checks(&mut ctx, prelude);

    // Post-compilation pass: detect recursive sub-component cycles,
    // validate termination conditions, and remix is_recursive into each
    // recursive template's content_hash.
    compile_builder::post_passes::phase_recursion_detection(&mut ctx);

    // Check for duplicate function signatures.
    compile_builder::post_passes::phase_dup_sig_check(&mut ctx);

    // Post-compilation pass: check field composition type compatibility.
    compile_builder::post_passes::phase_field_composition(&mut ctx);

    // Purpose compilation pass (runs after templates are populated so
    // reflective schema queries can resolve against TopologyTemplates).
    let compiled_purposes = compile_builder::post_passes::phase_purposes(&mut ctx, parsed);

    // Build a content-sensitive hash by combining the path with all compiled content.
    let content_hash = compile_builder::hash::compute_module_hash(&ctx, parsed, &compiled_purposes);

    ctx.into_compiled_module(parsed, compiled_purposes, content_hash)
}
