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
//! * [`SourceOrderFnEnv::compile_function_decl`] compiles one fn's body, plus
//!   any defaults the early pass left pending.  It is driven by the unified
//!   source-order pass in `entities_phase::phase_entities`, which interleaves
//!   fn bodies with entity templates in declaration order, so a fn body
//!   constructs a same-module structure against that structure's
//!   AUTHORITATIVE [`TopologyTemplate`].
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
/// module-local ctor-view name marked unavailable.  The probe's
/// diagnostics go to a scratch buffer:
///
/// * holding a `StructureDeclaredAfterFunction` or `FnDefaultNotYetAvailable`,
///   the defaults need a local template (directly, or through a fn above whose
///   defaults are pending) — `NeedLocalTemplates`, and the scratch and its
///   result are DISCARDED; the unified pass compiles them in the fn's slot;
/// * otherwise the result is exactly what the unified pass would compile —
///   `Compiled`, and the scratch is emitted, once, here.  Debug builds re-check
///   the compiled defaults, not the scratch, in
///   [`SourceOrderFnEnv::compile_function_decl`].
///
/// Seeds `ctx.resolution_functions` with the resulting stubs composed with the
/// prelude as [`build_resolution_functions`] composes them, and records the
/// pending fns in `ctx.fns_with_pending_defaults`.
pub(crate) fn phase_fn_param_defaults(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
) {
    let prelude_ctor_view = name_env::prelude_ctor_view(prelude);
    let local_ctor_names = ctx.local_entity_names();
    let declared_callables = crate::functions::declared_callable_names(
        &ctx.declared_fn_names,
        &ctx.resolution_structure_names,
    );
    let mut table = ResolutionTable::new(Vec::new(), prelude);
    let mut pending = PendingDefaults::default();
    for (fn_def, sig) in fn_refs.iter().zip(&ctx.fn_signatures) {
        let outcome = if fn_def.params.iter().all(|p| p.default.is_none()) {
            FnParamDefaults::Compiled(CompiledFunction::no_defaults_for(&sig.params))
        } else {
            let env = FnCompileEnv {
                enum_defs: &ctx.resolution_enums,
                calls: CallTable::with_pending(table.functions(), &pending),
                template_registry: Some(&prelude_ctor_view),
                unavailable_local_template_names: Some(&local_ctor_names),
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

/// The pass-invariant inputs every fn body in the unified source-order pass
/// compiles against, built once by `entities_phase::phase_entities` before its
/// declaration loop.
pub(crate) struct SourceOrderFnEnv<'a> {
    enum_defs: &'a [EnumDef],
    prelude_ctor_view: &'a HashMap<String, &'a TopologyTemplate>,
    /// The module's order-INDEPENDENT callable vocabulary (task #5371): the
    /// companion to the order-dependent `calls_above` each fn body resolves
    /// against. It changes nothing about which overload resolves; see
    /// `CompilationScope::declared_callable_names` for what it is for.
    declared_callable_names: HashSet<String>,
    local_structure_def_names: HashSet<String>,
}

impl<'a> SourceOrderFnEnv<'a> {
    pub(crate) fn new(
        enum_defs: &'a [EnumDef],
        prelude_ctor_view: &'a HashMap<String, &'a TopologyTemplate>,
        declared_callable_names: HashSet<String>,
        local_structure_def_names: HashSet<String>,
    ) -> Self {
        Self {
            enum_defs,
            prelude_ctor_view,
            declared_callable_names,
            local_structure_def_names,
        }
    }

    /// Compile ONE `fn` declaration's body, plus its param defaults when
    /// [`phase_fn_param_defaults`] left them pending.
    ///
    /// Called from the `Declaration::Function` arm of
    /// `entities_phase::phase_entities`, in source order, so `calls_above` and
    /// `templates_so_far` hold only what is declared ABOVE this fn:
    ///
    /// * constructors resolve against the prelude ctor view overlaid by
    ///   `templates_so_far` ([`name_env::overlay_local_templates`]), so a fn
    ///   body may construct a same-module structure declared above it and get
    ///   that structure's authoritative template — the same
    ///   no-forward-reference rule `compile_entity` obeys for entity → entity
    ///   references;
    /// * calls resolve against `calls_above`: the real fns declared above plus
    ///   every prelude fn they do not shadow, so forward references to sibling
    ///   fns do not resolve;
    /// * the module-local `structure def`s with no template in
    ///   `templates_so_far` — those declared BELOW this fn — draw
    ///   `StructureDeclaredAfterFunction` rather than a silent fallback.
    ///
    /// The overlay and the declared-below set are per fn because both derive
    /// from `templates_so_far`, which the caller pushes to between fns.
    ///
    /// Signature-level diagnostics are NOT emitted here — [`phase_fn_signatures`]
    /// already emitted them exactly once — and neither are those of `Compiled`
    /// defaults, which are reused as compiled (debug builds recompile them only
    /// to check that rule).
    pub(crate) fn compile_function_decl(
        &self,
        fn_def: &FnDef,
        sig: &FnSignature,
        defaults: &FnParamDefaults,
        calls_above: &ResolutionTable<'_>,
        templates_so_far: &[TopologyTemplate],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<CompiledFunction> {
        let template_registry =
            name_env::overlay_local_templates(self.prelude_ctor_view, templates_so_far);
        let declared_below = self.structure_defs_declared_below(templates_so_far);
        let env = FnCompileEnv {
            enum_defs: self.enum_defs,
            // Never holds a pending stub: fns above are real, fns below are
            // #5371's forward references.
            calls: CallTable::complete(calls_above.functions()),
            template_registry: Some(&template_registry),
            unavailable_local_template_names: Some(&declared_below),
            declared_callable_names: &self.declared_callable_names,
        };
        let param_defaults = match defaults {
            FnParamDefaults::Compiled(defaults) => {
                #[cfg(debug_assertions)]
                early_default_recheck::assert_early_defaults_match_unified(
                    fn_def, sig, defaults, &env,
                );
                defaults.clone()
            }
            FnParamDefaults::NeedLocalTemplates => {
                compile_fn_param_defaults(fn_def, sig, &env, diagnostics)
            }
        };
        compile_fn_body(fn_def, sig, param_defaults, &env, diagnostics)
    }

    /// The module-local `structure def`s with no template in `templates_so_far`.
    fn structure_defs_declared_below(
        &self,
        templates_so_far: &[TopologyTemplate],
    ) -> HashSet<String> {
        let mut declared_below = self.local_structure_def_names.clone();
        for template in templates_so_far {
            declared_below.remove(&template.name);
        }
        declared_below
    }
}

/// Debug-build recheck of the early-default commit rule.
#[cfg(debug_assertions)]
mod early_default_recheck {
    use reify_ast::FnDef;
    use reify_core::{ContentHash, Type};
    use reify_ir::CompiledExpr;

    use crate::functions::{FnCompileEnv, FnSignature, compile_fn_param_defaults};

    /// Each default slot's identity, for comparing two compilations of the same
    /// defaults: its `content_hash` (what the engine caches by) and its
    /// `result_type` (which a literal's hash leaves out).
    fn default_fingerprints(defaults: &[Option<CompiledExpr>]) -> Vec<Option<(ContentHash, Type)>> {
        defaults
            .iter()
            .map(|slot| {
                slot.as_ref()
                    .map(|expr| (expr.content_hash, expr.result_type.clone()))
            })
            .collect()
    }

    /// Debug-build recheck of the early-default commit rule (PRD
    /// `stdlib-namespace.md` D-8): panics unless recompiling `fn_def`'s defaults
    /// in the unified `env` gives `early`, the `Compiled` defaults that
    /// [`super::phase_fn_param_defaults`] committed.
    ///
    /// It checks the compiled defaults only, not the diagnostics compiling them
    /// drew: the recompile's are dropped, and nothing compares them with the
    /// ones the early pass emitted.
    pub(super) fn assert_early_defaults_match_unified(
        fn_def: &FnDef,
        sig: &FnSignature,
        early: &[Option<CompiledExpr>],
        env: &FnCompileEnv<'_>,
    ) {
        let unified = compile_fn_param_defaults(fn_def, sig, env, &mut Vec::new());
        let early_fingerprints = default_fingerprints(early);
        let unified_fingerprints = default_fingerprints(&unified);
        let slot_count = early_fingerprints.len().max(unified_fingerprints.len());
        let Some(slot) =
            (0..slot_count).find(|&i| early_fingerprints.get(i) != unified_fingerprints.get(i))
        else {
            return;
        };
        let param = match fn_def.params.get(slot) {
            Some(param) => format!("param `{}`", param.name),
            None => format!("slot {slot}, past the declared params"),
        };
        panic!(
            "early-default commit rule violated (PRD stdlib-namespace D-8): the defaults \
             `phase_fn_param_defaults` committed as `Compiled` for fn `{}` differ from the \
             unified recompile at {param}\n  early:   {}\n  unified: {}\n\
             Look for a `scope.template_registry` read in expr.rs that runs before \
             `later_declared_structure_error`.",
            fn_def.name,
            describe_slot(early_fingerprints.get(slot)),
            describe_slot(unified_fingerprints.get(slot)),
        );
    }

    /// One entry of [`default_fingerprints`], as the divergence panic prints it.
    fn describe_slot(slot: Option<&Option<(ContentHash, Type)>>) -> String {
        match slot {
            None => "no slot".to_owned(),
            Some(None) => "no default".to_owned(),
            Some(Some((hash, ty))) => format!("`{ty}` {hash}"),
        }
    }
}

/// The compile-time overload-dispatch table: every user function, then each
/// prelude function that wins its `(name, param types)` key under NS-P1
/// ([`name_env`]: imports before std modules, first wins). A prelude function
/// sharing a user function's key is shadowed.
fn build_resolution_functions(
    user: &[CompiledFunction],
    prelude: &[&CompiledModule],
) -> Vec<CompiledFunction> {
    let mut functions = user.to_vec();
    functions.extend(unshadowed_prelude_winners(user, prelude));
    functions
}

/// The prelude tail of [`build_resolution_functions`]: each prelude function
/// that wins its key under NS-P1 and shares no key with `user`.
fn unshadowed_prelude_winners(
    user: &[CompiledFunction],
    prelude: &[&CompiledModule],
) -> Vec<CompiledFunction> {
    name_env::bind(user, prelude.iter().copied())
        .winning_candidates()
        .iter()
        .filter(|candidate| candidate.origin != Origin::Local)
        .map(|candidate| candidate.definition.clone())
        .collect()
}

/// [`build_resolution_functions`] over a user list that grows one fn at a time.
///
/// [`Self::functions`] always equals
/// `build_resolution_functions(&(seed ++ users_pushed_so_far), prelude)`. A
/// user fn that shadows no prelude winner's key is spliced in at the end of
/// the user prefix; only one that does triggers a rebuild of the prelude tail,
/// so the prelude winners are cloned once rather than once per user fn, and
/// the user prefix is never re-cloned.
pub(crate) struct ResolutionTable<'p> {
    prelude: &'p [&'p CompiledModule],
    prelude_keys: HashSet<FunctionSignature<'p>>,
    functions: Vec<CompiledFunction>,
    user_len: usize,
}

impl<'p> ResolutionTable<'p> {
    pub(crate) fn new(users: Vec<CompiledFunction>, prelude: &'p [&'p CompiledModule]) -> Self {
        let prelude_keys = name_env::bind::<CompiledFunction, _>(&[], prelude.iter().copied())
            .winners()
            .map(Definition::key)
            .collect();
        ResolutionTable {
            prelude,
            prelude_keys,
            functions: build_resolution_functions(&users, prelude),
            user_len: users.len(),
        }
    }

    pub(crate) fn functions(&self) -> &[CompiledFunction] {
        &self.functions
    }

    pub(crate) fn push_user(&mut self, user: CompiledFunction) {
        if self.prelude_keys.contains(&user.key()) {
            self.functions.truncate(self.user_len);
            self.functions.push(user);
            let prelude_tail = unshadowed_prelude_winners(&self.functions, self.prelude);
            self.functions.extend(prelude_tail);
        } else {
            self.functions.insert(self.user_len, user);
        }
        self.user_len += 1;
    }

    pub(crate) fn into_functions(self) -> Vec<CompiledFunction> {
        self.functions
    }

    pub(crate) fn into_user_functions(mut self) -> Vec<CompiledFunction> {
        self.functions.truncate(self.user_len);
        self.functions
    }
}

#[cfg(test)]
mod tests {
    use reify_core::DiagnosticCode;

    use super::*;
    use crate::compile_builder::test_support::{
        compile_against_prelude, compile_src, constructed_params, fn_result_expr, parse_at,
    };

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

    /// A prelude with fns `kept` and `shadowed`, and three users: `fresh`, a
    /// `shadowed` that shadows the prelude's, and a `kept(String)` that does not.
    fn prelude_and_users() -> (CompiledModule, Vec<CompiledFunction>) {
        let prelude =
            compile_src("fn kept(x : Real) -> Real { x }\nfn shadowed(x : Real) -> Real { x }\n");
        let users = compile_src(
            "fn fresh(x : Real) -> Real { x }\nfn shadowed(x : Real) -> Real { x + 1.0 }\n\
             fn kept(x : String) -> Real { 1.0 }\n",
        )
        .functions;
        (prelude, users)
    }

    #[test]
    fn resolution_table_matches_build_resolution_functions_after_every_push() {
        let (prelude, users) = prelude_and_users();
        let prelude_refs = [&prelude];
        let mut table = ResolutionTable::new(Vec::new(), &prelude_refs);
        for pushed in 1..=users.len() {
            table.push_user(users[pushed - 1].clone());
            assert_eq!(
                signatures(table.functions()),
                signatures(&build_resolution_functions(&users[..pushed], &prelude_refs)),
                "after pushing {pushed} user fn(s)"
            );
        }
    }

    #[test]
    fn a_seeded_resolution_table_matches_build_resolution_functions_and_hands_back_its_users() {
        let (prelude, users) = prelude_and_users();
        let prelude_refs = [&prelude];
        let mut table = ResolutionTable::new(users[..1].to_vec(), &prelude_refs);
        assert_eq!(
            signatures(table.functions()),
            signatures(&build_resolution_functions(&users[..1], &prelude_refs)),
            "straight after seeding with 1 user fn"
        );
        for held in 2..=users.len() {
            table.push_user(users[held - 1].clone());
            assert_eq!(
                signatures(table.functions()),
                signatures(&build_resolution_functions(&users[..held], &prelude_refs)),
                "holding {held} user fn(s)"
            );
        }
        assert_eq!(signatures(&table.into_user_functions()), signatures(&users));
    }

    #[test]
    fn a_prelude_fn_stays_visible_to_fn_bodies_above_the_local_fn_that_shadows_it() {
        let module = compile_against_prelude(
            "fn f(x : Real) -> Real { x }\n",
            "fn g() -> Real { f(1.0) }\n\
             fn f(x : Real) -> String { \"s\" }\n\
             fn h() -> String { f(1.0) }\n",
        );
        assert_eq!(
            fn_result_expr(&module, "g").result_type,
            reify_core::Type::dimensionless_scalar(),
            "g is above the local f, so its call binds the prelude f"
        );
        assert_eq!(
            fn_result_expr(&module, "h").result_type,
            reify_core::Type::String,
            "h is below the local f, so its call binds the local f"
        );
    }

    #[test]
    fn only_a_fn_above_a_local_structure_def_is_told_it_is_declared_after() {
        const SRC: &str = "fn above() -> S { S() }\n\
             structure def S { param a : Real = 1.0 }\n\
             fn below() -> S { S() }\n";
        let module = compile_src(SRC);
        let def_start = SRC.find("structure def S").expect("fixture declares S") as u32;
        let ordering_call_sites: Vec<u32> = module
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::StructureDeclaredAfterFunction))
            .flat_map(|d| d.labels.iter().map(|label| label.span.start))
            .collect();
        assert!(
            matches!(ordering_call_sites[..], [call_site] if call_site < def_start),
            "exactly one ordering error, at the call in `above`; got: {:#?}",
            module.diagnostics
        );
        assert_eq!(
            constructed_params(fn_result_expr(&module, "below")),
            Some(vec!["a".to_string()]),
            "`below` is below S, so it constructs the local S"
        );
    }

    #[cfg(debug_assertions)]
    mod early_default_recheck_tests {
        use reify_core::Type;
        use reify_ir::CompiledExpr;

        use super::*;

        /// One defaulted param, `spoiled`, whose default the unified env
        /// compiles to the literal `1.0`.
        const EARLY_PROBE: &str = "fn early_probe(spoiled : Real = 1.0) -> Real { spoiled }\n";

        /// `source`'s one `fn` through [`SourceOrderFnEnv::compile_function_decl`],
        /// told by `early` that the early pass already compiled its defaults.
        ///
        /// The unified env is empty (no prelude, no templates, no name sets),
        /// which is what [`compile_src`] compiles a template-free module against.
        fn decl_with_early_defaults(
            source: &str,
            early: Vec<Option<CompiledExpr>>,
        ) -> Option<CompiledFunction> {
            let parsed = parse_at("test", source);
            let [reify_ast::Declaration::Function(fn_def)] = &parsed.declarations[..] else {
                panic!(
                    "fixture must declare exactly one fn: {:?}",
                    parsed.declarations
                );
            };
            let sig = resolve_fn_signature(
                fn_def,
                &[],
                &crate::type_resolution::TypeAliasRegistry::new(),
                &HashSet::new(),
                &HashSet::new(),
                &mut Vec::new(),
            );
            let prelude_view = HashMap::new();
            SourceOrderFnEnv::new(&[], &prelude_view, HashSet::new(), HashSet::new())
                .compile_function_decl(
                    fn_def,
                    &sig,
                    &FnParamDefaults::Compiled(early),
                    &ResolutionTable::new(Vec::new(), &[]),
                    &[],
                    &mut Vec::new(),
                )
        }

        /// What the unified pass compiles `source`'s defaults to.
        fn honest_defaults(source: &str) -> Vec<Option<CompiledExpr>> {
            compile_src(source).functions[0].param_defaults.clone()
        }

        #[test]
        #[should_panic(expected = "spoiled")]
        fn a_compiled_default_whose_content_differs_from_the_unified_recompile_panics() {
            let early = vec![Some(CompiledExpr::literal(
                reify_ir::Value::Real(2.0),
                Type::dimensionless_scalar(),
            ))];
            decl_with_early_defaults(EARLY_PROBE, early);
        }

        #[test]
        #[should_panic(expected = "spoiled")]
        fn a_compiled_default_whose_type_differs_from_the_unified_recompile_panics() {
            // Only `result_type` is overwritten, so the `content_hash` still
            // equals the unified recompile's: only the type comparison can fire.
            let mut early = honest_defaults(EARLY_PROBE);
            early[0]
                .as_mut()
                .expect("`spoiled` is defaulted")
                .result_type = Type::String;
            decl_with_early_defaults(EARLY_PROBE, early);
        }

        /// The control for the type-only test above: an honest default passes
        /// the debug recheck, so [`honest_defaults`] and this test's empty
        /// unified env give `spoiled` the same `content_hash`, and the type-only
        /// panic can only come from the `result_type` comparison.
        #[test]
        fn an_honest_compiled_default_passes_the_debug_recheck() {
            assert!(
                decl_with_early_defaults(EARLY_PROBE, honest_defaults(EARLY_PROBE)).is_some(),
                "an honest `Compiled` default compiles through the debug recheck"
            );
        }

        #[test]
        #[should_panic(expected = "slot 1")]
        fn an_early_default_vector_with_a_slot_past_the_declared_params_panics() {
            // Slot 0 is the honest default, so the only divergence is the extra slot 1.
            let mut early = honest_defaults(EARLY_PROBE);
            early.push(Some(CompiledExpr::literal(
                reify_ir::Value::Real(1.0),
                Type::dimensionless_scalar(),
            )));
            decl_with_early_defaults(EARLY_PROBE, early);
        }
    }
}
