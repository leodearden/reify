//! Covers `phase_entities`' `FnDeclTablesMisaligned` guards: each test tampers
//! with one per-fn table before the pass, then observes what the pass did with
//! the fn that table describes. The tests are green on arrival and pinned by
//! mutation (task 9988).

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticCode, Severity, SourceSpan};

use super::phase_entities;
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::enums_phase::bind_resolution_enums;
use crate::compile_builder::functions_phase::{phase_fn_param_defaults, phase_fn_signatures};
use crate::compile_builder::names_phase::build_resolution_names;
use crate::compile_builder::pre_pass::collect_decl_refs;
use crate::compile_builder::traits_phase::phase_traits;

const TWO_FNS: &str = "fn a(x : Real) -> Real { x }\nfn b(x : Real) -> Real { x + 1.0 }\n";

/// Run the real phases over [`TWO_FNS`] with an empty prelude, applying
/// `tamper` to the context right before the unified pass.
///
/// Only the phases whose output the pass's `Declaration::Function` arm reads
/// run, in the order `compile_with_prelude_context_checked_with_config` runs
/// them. The fixture is checked well-formed before `tamper` touches it, so a
/// drifted harness fails loudly instead of passing the tamper tests vacuously.
fn run_pass_after(tamper: impl FnOnce(&mut CompilationCtx)) -> (CompilationCtx, ParsedModule) {
    let parsed = reify_syntax::parse(TWO_FNS, reify_core::ModulePath::single("fn_tables"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in fixture: {:?}",
        parsed.errors
    );

    let mut ctx = CompilationCtx::new();
    let decl_refs = collect_decl_refs(&mut ctx, &parsed);
    build_resolution_names(&mut ctx, &[], &decl_refs.trait_refs, &decl_refs.fn_refs);
    bind_resolution_enums(&mut ctx, &[]);
    phase_fn_signatures(&mut ctx, &decl_refs.fn_refs);
    phase_fn_param_defaults(&mut ctx, &[], &decl_refs.fn_refs);
    let trait_names = phase_traits(&mut ctx, &[], &decl_refs.trait_refs);
    let fixture_errors = other_errors(&ctx);
    assert!(
        fixture_errors.is_empty(),
        "fixture is ill-formed before tampering: {fixture_errors:?}"
    );

    tamper(&mut ctx);
    phase_entities(&mut ctx, &parsed, &trait_names, &[]);
    (ctx, parsed)
}

/// The span of every `FnDeclTablesMisaligned` diagnostic's label, in emission order.
fn misaligned_spans(ctx: &CompilationCtx) -> Vec<SourceSpan> {
    ctx.diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FnDeclTablesMisaligned))
        .map(|d| d.labels[0].span)
        .collect()
}

/// The errors that are not `FnDeclTablesMisaligned`.
fn other_errors(ctx: &CompilationCtx) -> Vec<&Diagnostic> {
    ctx.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.code != Some(DiagnosticCode::FnDeclTablesMisaligned))
        .collect()
}

/// The names of the module's own compiled fns, in `ctx.functions` order.
fn user_fn_names(ctx: &CompilationCtx) -> Vec<&str> {
    ctx.functions.iter().map(|f| f.name.as_str()).collect()
}

/// Whether call-table slot `idx` holds the compiled fn `name`, not its signature stub.
///
/// The two differ in `content_hash`: a stub's is `ContentHash::of_str(name)`,
/// while a compiled fn's folds its params, defaults and body.
fn slot_holds_real_fn(ctx: &CompilationCtx, idx: usize, name: &str) -> bool {
    let Some(slot) = ctx.resolution_functions.get(idx) else {
        return false;
    };
    slot.name == name
        && ctx
            .functions
            .iter()
            .any(|real| real.name == name && real.content_hash == slot.content_hash)
}

#[test]
fn untampered_tables_draw_no_misalignment_and_slot_every_real_fn() {
    let (ctx, _parsed) = run_pass_after(|_| {});

    let spans = misaligned_spans(&ctx);
    assert!(
        spans.is_empty(),
        "untampered tables drew FnDeclTablesMisaligned at {spans:?}"
    );
    let errors = other_errors(&ctx);
    assert!(
        errors.is_empty(),
        "untampered tables drew errors: {errors:?}"
    );
    assert_eq!(user_fn_names(&ctx), ["a", "b"]);
    assert!(
        slot_holds_real_fn(&ctx, 0, "a"),
        "slot 0 still holds a's signature stub"
    );
    assert!(
        slot_holds_real_fn(&ctx, 1, "b"),
        "slot 1 still holds b's signature stub"
    );
}
