// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

mod annotations;
mod arg_check;
pub mod auto_type_param;
mod compile_builder;
mod conformance;
mod connect;
mod constants;
mod entity;
mod expr;
mod forall_elaborate;
mod functions;
mod geometry;
mod geometry_boolean;
mod geometry_curve;
mod geometry_modify;
mod geometry_transform;
pub mod geometry_traits;
pub mod geometry_traits_inference;
mod guards;
mod ice;
mod list_helpers;
pub mod module_dag;
mod module_pragmas;
pub mod prelude_context;
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

pub use geometry::derive_feature_tags;
pub use prelude_context::PreludeContext;
pub use type_compat::{implicitly_converts_to, type_compatible};
pub use types::*;

// Re-export submodule items for internal cross-module access via `use super::*;`
pub(crate) use annotations::*;
pub(crate) use arg_check::*;
pub(crate) use conformance::*;
pub(crate) use connect::*;
pub(crate) use entity::*;
pub(crate) use expr::*;
#[allow(unused_imports)]
pub(crate) use forall_elaborate::*;
#[allow(unused_imports)]
pub(crate) use functions::*;
pub(crate) use geometry::*;
pub(crate) use geometry_boolean::*;
pub(crate) use geometry_curve::*;
pub(crate) use geometry_modify::*;
pub(crate) use geometry_transform::*;
pub(crate) use guards::*;
pub(crate) use ice::*;
pub(crate) use list_helpers::*;
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
pub use units::{GEOMETRY_FUNCTION_NAMES, UnitEntry, UnitRegistry};

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ConstraintNodeId, ContentHash, DeterminacyPredicateKind,
    Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, FIELD_ENTITY_PREFIX,
    OptimizationObjective, RealizationNodeId, ResolvedFunction, SelectorKind, Severity, SourceSpan,
    TAG_CONDITIONAL, TAG_FUNCTION_CALL, TAG_MATCH, TAG_USER_FUNCTION_CALL, Type, UnOp, Value,
    ValueCellId,
};

/// Compile a parsed module into a compiled module.
///
/// Performs name resolution, type checking, and expression compilation.
/// Equivalent to `compile_with_prelude(parsed, &[])`.
///
/// # Warning
///
/// This function compiles with **no prelude** — it will **not** resolve any
/// standard library definitions. In particular, the nine previously
/// hard-coded units (`mm`, `cm`, `m`, `in`, `deg`, `rad`, `kg`, `g`, `s`),
/// standard traits (`MaterialSpec`, `Physical`, etc.), and all stdlib enum
/// types are absent from the compilation environment. Source code that
/// references any of those names will produce unresolved-name diagnostics.
///
/// This entry point is intended only for **prelude-module bootstrapping**:
/// compiling a module whose output will itself be used as a prelude, at a
/// point where no compiled prelude exists yet.
///
/// For all other use cases prefer:
///
/// * [`compile_with_stdlib`] — full standard library prelude (recommended
///   default for user modules)
/// * [`compile_with_prelude`] — caller-supplied prelude modules
pub fn compile(parsed: &reify_syntax::ParsedModule) -> CompiledModule {
    compile_with_prelude(parsed, &[])
}

/// Compile a parsed module with the full standard library prelude.
///
/// This is the recommended entry point for compiling user modules with full
/// stdlib support. Delegates to [`compile_with_prelude_context`] with a
/// `&'static PreludeContext` that is built once (via [`stdlib_loader::load_stdlib_context`])
/// and shared across all calls, avoiding re-flattening stdlib enum definitions
/// on every compilation.
pub fn compile_with_stdlib(parsed: &reify_syntax::ParsedModule) -> CompiledModule {
    compile_with_prelude_context(parsed, stdlib_loader::load_stdlib_context())
}

/// Parse a source string with the stdlib's prelude enum names pre-seeded
/// into the parser's `EnumAccess` disambiguation set.
///
/// Companion to [`compile_with_stdlib`]: when the produced `ParsedModule`
/// will be fed to `compile_with_stdlib`, prefer this entry over the bare
/// [`reify_syntax::parse`] so that `Type.Variant` references to stdlib
/// enums (e.g. `CorrosionClass.C5`) lower to `ExprKind::EnumAccess` rather
/// than `ExprKind::MemberAccess`.
///
/// Reads the cached `&'static PreludeContext` from
/// [`stdlib_loader::load_stdlib_context`] (no fresh stdlib compile),
/// memoizes its flattened enum-name list in a process-global `OnceLock`,
/// and delegates to [`reify_syntax::parse_with_prelude_enums`].  This keeps
/// hot edit-loop callers (LSP recompiles per keystroke, GUI engine
/// reloads) from re-collecting the same `&'static str`s on every parse.
/// The parser accepts `&[&'static str]` and stores borrows rather than
/// allocating owned `String`s, so the memoised slice flows through with
/// zero per-call heap allocation.
pub fn parse_with_stdlib(
    source: &str,
    module_path: reify_types::ModulePath,
) -> reify_syntax::ParsedModule {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let names: &Vec<&'static str> =
        NAMES.get_or_init(|| stdlib_loader::load_stdlib_context().enum_names().collect());
    reify_syntax::parse_with_prelude_enums(source, module_path, names)
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
/// negligible, but crate-internal callers in a hot loop should use
/// `compile_with_prelude_refs` directly to avoid repeated allocation.
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

/// Compile a parsed module using a pre-built [`PreludeContext`].
///
/// Like [`compile_with_prelude`] but skips re-flattening prelude enum
/// definitions on every call: the flattening is done once at
/// [`PreludeContext`] construction time, so callers that compile many
/// user modules against the same prelude (e.g. `compile_with_stdlib`) pay
/// the allocation cost only once.
///
/// This is also the single phase-orchestration body shared with
/// [`compile_with_prelude_refs`] (which builds an ad-hoc [`PreludeContext`]
/// and delegates here). Keeping a single orchestrator guarantees the two
/// paths stay in sync across future phase additions.
///
/// For the stdlib hot-path, prefer [`compile_with_stdlib`] which already
/// delegates to a `&'static PreludeContext` after the refactor in step-9.
///
/// # Parity
///
/// Produces output identical to `compile_with_prelude(parsed, prelude)` for
/// any prelude whose [`PreludeContext::from_slice`] was built from that
/// same `prelude` slice.
pub fn compile_with_prelude_context(
    parsed: &reify_syntax::ParsedModule,
    ctx: &PreludeContext,
) -> CompiledModule {
    let mut compile_ctx = compile_builder::ctx::CompilationCtx::new();

    compile_builder::pre_pass::forward_parse_errors(&mut compile_ctx, parsed);
    compile_builder::pre_pass::validate_module_pragmas(&mut compile_ctx, parsed);
    compile_builder::dot_chain_lint::lint_module(parsed, &mut compile_ctx.diagnostics);
    compile_builder::shadow_lint::lint_module(parsed, &mut compile_ctx.diagnostics);
    compile_builder::specialization_scope_check::validate_module(
        parsed,
        &mut compile_ctx.diagnostics,
    );

    // Respect #no_prelude: if the pragma is present, treat as empty prelude.
    let prelude_refs: &[&CompiledModule] =
        compile_builder::pre_pass::effective_prelude(parsed, ctx.modules());

    let decl_refs = compile_builder::pre_pass::collect_decl_refs(&mut compile_ctx, parsed);

    compile_builder::units_phase::phase_units(&mut compile_ctx, prelude_refs, &decl_refs.unit_refs);
    // Mirror the resolution_enums gate (lib.rs:270-277): when prelude_refs is
    // empty (#no_prelude pragma or empty prelude), pass &[] so prelude aliases
    // are not seeded — consistent with how units, enums, traits, and functions
    // suppress prelude contribution when the pragma is active.
    let prelude_aliases = if prelude_refs.is_empty() {
        &[][..]
    } else {
        // Emit cross-prelude pub alias collision warnings detected at PreludeContext
        // construction time. Mirroring units_phase's 'last-wins' warning for cross-prelude
        // unit collisions, but first-wins here (PreludeContext::new deduplicates eagerly).
        for (alias_name, first_module, second_module) in ctx.pub_alias_collision_warnings() {
            compile_ctx.diagnostics.push(
                Diagnostic::warning(format!(
                    "prelude pub alias '{}' declared in both '{}' and '{}'; first-wins",
                    alias_name, first_module, second_module
                ))
                .with_label(DiagnosticLabel::new(
                    SourceSpan::prelude(),
                    "cross-prelude collision",
                )),
            );
        }
        ctx.pub_aliases()
    };
    compile_builder::aliases_phase::phase_aliases(&mut compile_ctx, prelude_aliases, &decl_refs.alias_refs);

    // Use the pre-built resolution_enums from the context instead of
    // re-flattening the prelude modules on every call.
    // `prelude_refs.is_empty()` covers two cases:
    //   (a) #no_prelude pragma is active — suppress prelude enum contribution.
    //   (b) caller passed an empty prelude — ctx.resolution_enums() is also
    //       empty by construction, so &[] and ctx.resolution_enums() are equivalent.
    if prelude_refs.is_empty() {
        compile_builder::enums_phase::build_resolution_enums_from_cache(&mut compile_ctx, &[]);
    } else {
        compile_builder::enums_phase::build_resolution_enums_from_cache(
            &mut compile_ctx,
            ctx.resolution_enums(),
        );
    }

    compile_builder::functions_phase::phase_functions(
        &mut compile_ctx,
        prelude_refs,
        &decl_refs.fn_refs,
    );

    let trait_names = compile_builder::traits_phase::phase_traits(
        &mut compile_ctx,
        prelude_refs,
        &decl_refs.trait_refs,
    );

    compile_builder::fields_phase::phase_fields(&mut compile_ctx, &decl_refs.field_refs);

    compile_builder::defs_phase::phase_constraint_defs(
        &mut compile_ctx,
        parsed,
        prelude_refs,
        &trait_names,
    );

    compile_builder::entities_phase::phase_entities(
        &mut compile_ctx,
        parsed,
        &trait_names,
        prelude_refs,
    );

    compile_builder::entities_phase::phase_pending_bound_checks(&mut compile_ctx, prelude_refs);

    compile_builder::post_passes::phase_recursion_detection(&mut compile_ctx);
    compile_builder::post_passes::phase_dup_sig_check(&mut compile_ctx);
    compile_builder::post_passes::phase_field_composition(&mut compile_ctx);
    compile_builder::post_passes::phase_augment_composed_captures(&mut compile_ctx);

    let compiled_purposes = compile_builder::post_passes::phase_purposes(&mut compile_ctx, parsed);
    let content_hash =
        compile_builder::hash::compute_module_hash(&compile_ctx, parsed, &compiled_purposes);

    let mut module = compile_ctx.into_compiled_module(parsed, compiled_purposes, content_hash);
    module_pragmas::apply_module_pragmas(parsed, &mut module);
    module
}

/// Compile a parsed module with prelude definitions provided as references.
///
/// This is the inner implementation used by the module DAG to avoid cloning
/// already-compiled modules. The `prelude` slice contains references to
/// compiled modules whose exported definitions (units, traits, enums,
/// constraint defs) are visible during compilation.
///
/// Builds an ad-hoc [`PreludeContext`] from `prelude` and delegates to
/// [`compile_with_prelude_context`], so both paths share a single phase
/// orchestrator. The one-shot context allocation is negligible for the
/// non-stdlib path; the stdlib hot-path bypasses this function entirely via
/// [`compile_with_stdlib`].
///
/// External callers should use [`compile_with_prelude`] instead.
pub(crate) fn compile_with_prelude_refs(
    parsed: &reify_syntax::ParsedModule,
    prelude: &[&CompiledModule],
) -> CompiledModule {
    let ctx = PreludeContext::new(prelude);
    compile_with_prelude_context(parsed, &ctx)
}
