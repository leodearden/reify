//! Function compilation, split into sub-passes (task 5494 / PRD
//! `stdlib-namespace.md` D-8).
//!
//! * [`phase_fn_signatures`] runs EARLY — before any structure template exists —
//!   and resolves every `fn` declaration's signature.
//! * [`phase_fn_param_defaults`] runs next and compiles every fn's defaults
//!   that need no same-module template.  It seeds `ctx.resolution_functions`
//!   with signature stubs carrying those defaults, so the phases between it and
//!   the unified pass (`phase_traits`, `phase_fields`, `phase_constraint_defs`)
//!   can resolve — and pad — calls to same-module fns.  Omitting a default that
//!   constructs a same-module structure draws `FnDefaultNotYetAvailable` there.
//! * [`compile_function_decl`] compiles one fn's body, plus any defaults the
//!   early pass left pending.  It is driven by the unified source-order pass in
//!   `entities_phase::phase_entities`, which interleaves fn bodies with entity
//!   templates in declaration order, so a fn body constructs a same-module
//!   structure against that structure's AUTHORITATIVE [`TopologyTemplate`].
//!
//! A same-module structure declared BELOW the fn has no template yet; using it
//! draws `DiagnosticCode::StructureDeclaredAfterFunction`.

use std::collections::{HashMap, HashSet};

use reify_ast::FnDef;
use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{CompiledFunction, EnumDef};

use crate::CompiledModule;
use crate::call_table::{CallTable, PendingDefaults};
use crate::compile_builder::ctx::{CompilationCtx, signature_stub};
use crate::functions::{
    FnCompileEnv, FnParamDefaults, FnSignature, compile_fn_body, compile_fn_param_defaults,
    resolve_fn_signature,
};
use crate::name_env::{self, Definition, FunctionSignature, Origin};
use crate::types::TopologyTemplate;

/// Resolve every `fn` declaration's SIGNATURE, in source order.
///
/// The first half of the two-sub-pass split (task 5494 / PRD
/// `stdlib-namespace.md` D-8).  Signature resolution compiles no expression, so
/// it needs no template registry — which is what lets it run before any
/// structure template exists.  Template-free param defaults compile next, in
/// [`phase_fn_param_defaults`]; bodies, and defaults that construct a
/// same-module structure, compile in the unified source-order pass in
/// `phase_entities`, against the authoritative registry.
pub(crate) fn phase_fn_signatures(ctx: &mut CompilationCtx, fn_refs: &[&FnDef]) {
    for fn_def in fn_refs {
        let sig = resolve_fn_signature(
            fn_def,
            &ctx.resolution_enums,
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            &ctx.resolution_trait_names,
            &mut ctx.diagnostics,
        );
        ctx.fn_signatures.push(sig);
    }
}

/// Compile, ONCE and in source order, every fn's param defaults that need no
/// same-module structure template (PRD `stdlib-namespace.md` D-8).
///
/// Each fn's defaults are probed against what the unified pass would give them
/// minus the module's own templates: the signature stubs of the fns above it
/// (carrying the outcomes decided so far), the prelude ctor view, and every
/// module-local ctor-view name marked as declared later.  The probe's
/// diagnostics go to a scratch buffer:
///
/// * holding a `StructureDeclaredAfterFunction` or `FnDefaultNotYetAvailable`,
///   the defaults need a local template (directly, or through a fn above whose
///   defaults are pending) — `NeedLocalTemplates`, and the scratch and its
///   result are DISCARDED; the unified pass compiles them in the fn's slot;
/// * otherwise the result is exactly what the unified pass would compile —
///   `Compiled`, and the scratch is emitted, once, here.
///
/// Seeds `ctx.resolution_functions` with the resulting stubs composed with the
/// prelude as [`build_resolution_functions`] composes them, and records the
/// pending fns in `ctx.fns_with_pending_defaults`.
pub(crate) fn phase_fn_param_defaults(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
) {
    let prelude_ctor_view = name_env::ctor_template_registry(&[], prelude);
    let local_ctor_names = ctx.local_entity_names();
    let declared_callables = crate::functions::declared_callable_names(
        &ctx.declared_fn_names,
        &ctx.resolution_structure_names,
    );
    let mut table = ResolutionTable::new(prelude);
    let mut pending = PendingDefaults::default();
    for (fn_def, sig) in fn_refs.iter().zip(&ctx.fn_signatures) {
        let outcome = if fn_def.params.iter().all(|p| p.default.is_none()) {
            FnParamDefaults::Compiled(CompiledFunction::no_defaults_for(&sig.params))
        } else {
            let env = FnCompileEnv {
                enum_defs: &ctx.resolution_enums,
                calls: CallTable::with_pending(table.functions(), &pending),
                template_registry: Some(&prelude_ctor_view),
                later_declared_structure_names: Some(&local_ctor_names),
                declared_callable_names: &declared_callables,
            };
            let mut scratch = Vec::new();
            let defaults = compile_fn_param_defaults(fn_def, sig, &env, &mut scratch);
            let needs_local_templates = scratch.iter().any(|d| {
                matches!(
                    d.code,
                    Some(
                        DiagnosticCode::StructureDeclaredAfterFunction
                            | DiagnosticCode::FnDefaultNotYetAvailable
                    )
                )
            });
            if needs_local_templates {
                FnParamDefaults::NeedLocalTemplates
            } else {
                ctx.diagnostics.append(&mut scratch);
                FnParamDefaults::Compiled(defaults)
            }
        };
        if matches!(outcome, FnParamDefaults::NeedLocalTemplates) {
            let defaulted = fn_def
                .params
                .iter()
                .filter(|p| p.default.is_some())
                .map(|p| p.name.clone())
                .collect();
            pending.insert(&sig.name, &sig.params, defaulted);
        }
        table.push_user(signature_stub(sig, Some(&outcome)));
        ctx.fn_param_defaults.push(outcome);
    }
    ctx.fns_with_pending_defaults = pending;
    ctx.resolution_functions = table.into_functions();
}

/// Compile ONE `fn` declaration's body, plus its param defaults when
/// [`phase_fn_param_defaults`] left them pending.
///
/// Called from the `Declaration::Function` arm of
/// `entities_phase::phase_entities`, in source order, so `user_fns_so_far` and
/// `templates_so_far` hold only what is declared ABOVE this fn:
///
/// * constructors resolve against [`name_env::ctor_template_registry`] over
///   `templates_so_far`, so a fn body may construct a same-module structure
///   declared above it and get that structure's authoritative template — the
///   same no-forward-reference rule `compile_entity` obeys for entity → entity
///   references;
/// * calls resolve against [`build_resolution_functions`] over
///   `user_fns_so_far` plus every prelude fn they do not shadow, so forward
///   references to sibling fns do not resolve.
///
/// `later_declared_structure_names` holds the module-local `structure def`s
/// with no template in `templates_so_far` yet — those declared BELOW this fn;
/// using one draws `StructureDeclaredAfterFunction` rather than a silent
/// fallback.
///
/// The ctor view is rebuilt per fn: it borrows `templates_so_far`, which the
/// caller's structure arms push to between fns.
///
/// `declared_callables` is the module's order-independent callable vocabulary
/// (task #5371), built once per pass by the caller.
///
/// Signature-level diagnostics are NOT emitted here — [`phase_fn_signatures`]
/// already emitted them exactly once — and neither are those of `Compiled`
/// defaults, which are reused as compiled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_function_decl(
    fn_def: &FnDef,
    sig: &FnSignature,
    defaults: &FnParamDefaults,
    enum_defs: &[EnumDef],
    user_fns_so_far: &[CompiledFunction],
    templates_so_far: &[TopologyTemplate],
    prelude: &[&CompiledModule],
    later_declared_structure_names: &HashSet<String>,
    declared_callables: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    let functions = build_resolution_functions(user_fns_so_far, prelude);
    let template_registry: HashMap<String, &TopologyTemplate> =
        name_env::ctor_template_registry(templates_so_far, prelude);
    let env = FnCompileEnv {
        enum_defs,
        // Never holds a pending stub: fns above are real, fns below are #5371's
        // forward references.
        calls: CallTable::complete(&functions),
        template_registry: Some(&template_registry),
        later_declared_structure_names: Some(later_declared_structure_names),
        declared_callable_names: declared_callables,
    };
    let param_defaults = match defaults {
        FnParamDefaults::Compiled(defaults) => defaults.clone(),
        FnParamDefaults::NeedLocalTemplates => {
            compile_fn_param_defaults(fn_def, sig, &env, diagnostics)
        }
    };
    compile_fn_body(fn_def, sig, param_defaults, &env, diagnostics)
}

/// The compile-time overload-dispatch table: every user function, then each
/// prelude function that wins its `(name, param types)` key under NS-P1
/// ([`name_env`]: imports before std modules, first wins). A prelude function
/// sharing a user function's key is shadowed.
pub(crate) fn build_resolution_functions(
    user: &[CompiledFunction],
    prelude: &[&CompiledModule],
) -> Vec<CompiledFunction> {
    let bindings = name_env::bind(user, prelude.iter().copied());
    let prelude_winners = bindings
        .winning_candidates()
        .iter()
        .filter(|candidate| candidate.origin != Origin::Local)
        .map(|candidate| candidate.definition.clone());
    user.iter().cloned().chain(prelude_winners).collect()
}

/// [`build_resolution_functions`] over a user list that grows one fn at a time.
///
/// After every [`Self::push_user`], [`Self::functions`] equals
/// `build_resolution_functions(&users_pushed_so_far, prelude)`. A user fn that
/// shadows no prelude winner's key is spliced in at the end of the user
/// prefix; only one that does triggers a rebuild, so the prelude winners are
/// cloned once rather than once per user fn.
pub(crate) struct ResolutionTable<'p> {
    prelude: &'p [&'p CompiledModule],
    prelude_keys: HashSet<FunctionSignature<'p>>,
    functions: Vec<CompiledFunction>,
    user_len: usize,
}

impl<'p> ResolutionTable<'p> {
    pub(crate) fn new(prelude: &'p [&'p CompiledModule]) -> Self {
        let prelude_keys = name_env::bind::<CompiledFunction, _>(&[], prelude.iter().copied())
            .winners()
            .map(Definition::key)
            .collect();
        ResolutionTable {
            prelude,
            prelude_keys,
            functions: build_resolution_functions(&[], prelude),
            user_len: 0,
        }
    }

    pub(crate) fn functions(&self) -> &[CompiledFunction] {
        &self.functions
    }

    pub(crate) fn push_user(&mut self, user: CompiledFunction) {
        if self.prelude_keys.contains(&user.key()) {
            self.functions.truncate(self.user_len);
            self.functions.push(user);
            self.functions = build_resolution_functions(&self.functions, self.prelude);
        } else {
            self.functions.insert(self.user_len, user);
        }
        self.user_len += 1;
    }

    pub(crate) fn into_functions(self) -> Vec<CompiledFunction> {
        self.functions
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{DiagnosticCode, ModulePath};

    use super::*;

    fn compile_src(source: &str) -> CompiledModule {
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse: {:?}",
            parsed.errors
        );
        crate::compile(&parsed)
    }

    fn codes(module: &CompiledModule) -> Vec<Option<DiagnosticCode>> {
        module.diagnostics.iter().map(|d| d.code).collect()
    }

    /// `solve(x : String, o : Real)` declares no default, so omitting `o` is an
    /// arity error even though its same-name sibling's `o` default is pending.
    const OVERLOADED_SOLVE: &str = "structure def Opts { param t : Real = 1.0 }\n\
         fn solve(x : Real, o : Opts = Opts()) -> Real { x * o.t }\n\
         fn solve(x : String, o : Real) -> Real { o }\n";

    fn assert_no_matching_overload_not_pending(module: &CompiledModule, what: &str) {
        let codes = codes(module);
        assert!(
            codes.contains(&Some(DiagnosticCode::NoMatchingOverload))
                && !codes.contains(&Some(DiagnosticCode::FnDefaultNotYetAvailable)),
            "{what}: omitting a param the matching overload never defaults is \
             NoMatchingOverload, not a pending default; got: {:#?}",
            module.diagnostics
        );
    }

    #[test]
    fn a_sibling_overloads_pending_default_is_not_blamed_below_both_fns() {
        let module = compile_src(&format!(
            "{OVERLOADED_SOLVE}structure S {{ let v = solve(\"s\") }}\n"
        ));
        assert_no_matching_overload_not_pending(&module, "structure below both fns");
    }

    #[test]
    fn a_sibling_overloads_pending_default_is_not_blamed_above_its_slot() {
        let module = compile_src(&format!(
            "{OVERLOADED_SOLVE}field def f1 : Real -> Real {{ source = analytical {{ |x| solve(\"s\") }} }}\n"
        ));
        assert_no_matching_overload_not_pending(&module, "field lambda");
    }

    fn signatures(
        functions: &[CompiledFunction],
    ) -> Vec<(String, Vec<(String, reify_core::Type)>)> {
        functions
            .iter()
            .map(|f| (f.name.clone(), f.params.clone()))
            .collect()
    }

    #[test]
    fn resolution_table_matches_build_resolution_functions_after_every_push() {
        let prelude =
            compile_src("fn kept(x : Real) -> Real { x }\nfn shadowed(x : Real) -> Real { x }\n");
        let users = compile_src(
            "fn fresh(x : Real) -> Real { x }\nfn shadowed(x : Real) -> Real { x + 1.0 }\n\
             fn kept(x : String) -> Real { 1.0 }\n",
        )
        .functions;
        let prelude_refs = [&prelude];
        let mut table = ResolutionTable::new(&prelude_refs);
        for pushed in 1..=users.len() {
            table.push_user(users[pushed - 1].clone());
            assert_eq!(
                signatures(table.functions()),
                signatures(&build_resolution_functions(&users[..pushed], &prelude_refs)),
                "after pushing {pushed} user fn(s)"
            );
        }
    }
}
