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
mod diagnostics;
mod entity;
mod expr;
mod forall_elaborate;
mod functions;
mod geometry;
mod geometry_boolean;
mod geometry_curve;
mod geometry_modify;
pub mod geometry_traits;
pub mod geometry_traits_inference;
mod geometry_transform;
mod guards;
mod ice;
mod list_helpers;
mod math_signatures;
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

pub use compile_builder::pre_pass::check_module_path_decl;
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
pub(crate) use math_signatures::*;
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
pub use units::{GEOMETRY_FUNCTION_NAMES, UnitEntry, UnitRegistry, UnitResolveError, resolve_unit_expr};

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use reify_core::{ConstraintNodeId, ContentHash, Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, FIELD_ENTITY_PREFIX, RealizationNodeId, Severity, SourceSpan, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, DeterminacyPredicateKind, ObjectiveCombination, ObjectiveSet, ObjectiveSense, ObjectiveTerm, ResolvedFunction, SelectorKind, TAG_CONDITIONAL, TAG_FUNCTION_CALL, TAG_MATCH, TAG_USER_FUNCTION_CALL, UnOp, Value};

/// Expose `validate_annotations` to integration tests without plumbing a full
/// compilation context.
///
/// # Stability
///
/// This function is intentionally named with `__` prefix to signal that it is
/// an internal test shim and **not part of the public API**. It may be removed
/// or changed at any time. Gated behind `feature = "test-support"` (or
/// `cfg(test)` for in-crate tests); not part of the released public API.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
// G-allow: task #3530 parity shim — test-support-gated (feature = "test-support"), consumed by validate_annotations parity tests during schema-delegation migration; remove when delegation is complete
pub fn __validate_annotations_for_parity_test(
    annotations: &[reify_ir::Annotation],
    context: &str,
) -> Vec<reify_core::Diagnostic> {
    let mut diagnostics = Vec::new();
    annotations::validate_annotations(annotations, context, &mut diagnostics);
    diagnostics
}

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
pub fn compile(parsed: &reify_ast::ParsedModule) -> CompiledModule {
    compile_with_prelude(parsed, &[])
}

/// Compile a parsed module with the full standard library prelude.
///
/// This is the recommended entry point for compiling user modules with full
/// stdlib support. Delegates to [`compile_with_prelude_context`] with a
/// `&'static PreludeContext` that is built once (via [`stdlib_loader::load_stdlib_context`])
/// and shared across all calls, avoiding re-flattening stdlib enum definitions
/// on every compilation.
pub fn compile_with_stdlib(parsed: &reify_ast::ParsedModule) -> CompiledModule {
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
    module_path: reify_core::ModulePath,
) -> reify_ast::ParsedModule {
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
    parsed: &reify_ast::ParsedModule,
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
///
/// **Cross-phase note:** the silent first-wins policy here is the third variant in a set
/// of three divergent cross-prelude collision policies; see `prelude_context` §
/// "Cross-prelude collision policy" for the full comparison (units = last-wins/warns;
/// aliases = first-wins/warns; functions = first-wins/silent).
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

/// Merge is_pub prelude purposes into the user module's compiled_purposes.
///
/// Appends each prelude pub purpose whose name is not already present in
/// `user_purposes` (first-wins shadow: a user-defined purpose overrides any
/// stdlib purpose of the same name).
///
/// This propagates standard purposes (e.g. `simulation_ready`, `design_review`
/// from `std.determinacy.purposes`) into every user module compiled against the
/// stdlib, without requiring an explicit import — matching the global-prelude
/// model used for functions, units, and type aliases.
///
/// Only `is_pub` purposes are merged; private prelude purposes remain scoped to
/// their declaring module.
///
/// `std.determinacy.purposes` is registered LAST in `stdlib_loader::load_stdlib`
/// so that no other stdlib module inherits standard purposes during intra-stdlib
/// sequential compilation — stdlib-internal compiled_purposes counts and content
/// hashes remain stable. Only user modules (compiled against the full stdlib) gain
/// the standard purposes. (task-4016 ζ)
pub fn merge_prelude_purposes(
    user_purposes: Vec<CompiledPurpose>,
    prelude_refs: &[&CompiledModule],
) -> Vec<CompiledPurpose> {
    let mut result = user_purposes;
    for module in prelude_refs {
        for p in &module.compiled_purposes {
            if p.is_pub && !result.iter().any(|up| up.name == p.name) {
                result.push(p.clone());
            }
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
    parsed: &reify_ast::ParsedModule,
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
        // See `prelude_context` § "Cross-prelude collision policy" for the full cross-phase
        // comparison (units = last-wins/warns; aliases = first-wins/warns; functions =
        // first-wins/silent).
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
    compile_builder::aliases_phase::phase_aliases(
        &mut compile_ctx,
        prelude_aliases,
        &decl_refs.alias_refs,
    );

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

    compile_builder::names_phase::build_resolution_names(
        &mut compile_ctx,
        prelude_refs,
        &decl_refs.trait_refs,
    );

    compile_builder::functions_phase::phase_functions(
        &mut compile_ctx,
        prelude_refs,
        &decl_refs.fn_refs,
        &decl_refs.structure_refs,
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

    // Resolve `auto:` / `auto(free):` type-args raised by `phase_entities`
    // BEFORE the bound-check pass: the resolver rewrites placeholder slots to
    // concrete `Type::StructureRef`, so `phase_pending_bound_checks` sees the
    // resolved candidate rather than the synthetic `__auto_<bound>` placeholder.
    compile_builder::auto_type_param_phase::phase_auto_type_param_resolution(
        &mut compile_ctx,
        prelude_refs,
    );

    // Resolve deferred sub-instance-override `auto` / `auto(free)` cells raised
    // for forward-declared child structures (task 3806, step 10).  Runs after
    // `phase_auto_type_param_resolution` (all `type_args` placeholders are
    // concrete) and before `phase_pending_bound_checks` (same template-registry
    // composition; consistent ordering with other post-passes).
    compile_builder::entities_phase::phase_sub_override_autos(&mut compile_ctx, prelude_refs);

    compile_builder::entities_phase::phase_pending_bound_checks(&mut compile_ctx, prelude_refs);

    // Function-call-argument trait conformance post-pass (task-4081).
    // Runs immediately after phase_pending_bound_checks using the same
    // template+trait registry composition. Walks ALL CompiledExpr-bearing fields of
    // every entity template (value cells, constraints, objective, realizations,
    // ports, guarded groups, match-arm guards, sub-components, forall bodies) plus
    // all function bodies for UserFunctionCall nodes, validating each trait-carrying
    // param against its arg via check_fn_arg_conformance. See the
    // phase_fn_arg_conformance / for_each_template_root_expr doc-comments for the
    // exact root set and the documented residual (connections, compiled_purposes).
    compile_builder::entities_phase::phase_fn_arg_conformance(&mut compile_ctx, prelude_refs);

    compile_builder::post_passes::phase_recursion_detection(&mut compile_ctx);
    compile_builder::post_passes::phase_dup_sig_check(&mut compile_ctx);
    compile_builder::post_passes::phase_field_composition(&mut compile_ctx);
    compile_builder::post_passes::phase_augment_composed_captures(&mut compile_ctx);

    let compiled_purposes = compile_builder::post_passes::phase_purposes(&mut compile_ctx, parsed);
    // Merge is_pub prelude purposes (e.g. simulation_ready/design_review from
    // std.determinacy.purposes) into the user module's compiled_purposes.
    // Respects #no_prelude via the prelude_refs.is_empty() guard — when the
    // pragma is active, prelude_refs is already &[] so no merge occurs.
    let compiled_purposes = if prelude_refs.is_empty() {
        compiled_purposes
    } else {
        merge_prelude_purposes(compiled_purposes, prelude_refs)
    };
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
    parsed: &reify_ast::ParsedModule,
    prelude: &[&CompiledModule],
) -> CompiledModule {
    let ctx = PreludeContext::new(prelude);
    compile_with_prelude_context(parsed, &ctx)
}
