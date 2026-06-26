//! Definition-site validation guard for pub parametric type aliases (task #4796).
//!
//! TDD steps:
//!   step-1 RED  — unknown body name (LeakName) rejected at def site
//!   step-2 impl — name-existence check in `validate_pub_parametric_alias_def_site`
//!   step-3 RED  — param bound violation (BadBound) rejected at def site
//!   step-4 impl — param-bound check added to the same validator
//!   step-5      — acceptance + no-false-positive regression

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::{ModulePath, Severity, SourceSpan};

// ── Fixtures ──────────────────────────────────────────────────────────────────

const REJECT_FIXTURE: &str =
    include_str!("fixtures/parametric_alias_def_site_reject.ri");

const OK_FIXTURE: &str =
    include_str!("fixtures/parametric_alias_def_site_ok.ri");

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Return all Error-severity diagnostics from a compiled module.
fn error_diagnostics(module: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Return the span of a TypeAliasDecl by name in a parsed module.
fn alias_span(parsed: &reify_ast::ParsedModule, name: &str) -> SourceSpan {
    parsed
        .declarations
        .iter()
        .filter_map(|d| {
            if let reify_ast::Declaration::TypeAlias(a) = d {
                Some(a)
            } else {
                None
            }
        })
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("alias '{}' not found in parsed module", name))
        .span
}

/// Return true if `label_span` overlaps with `container_span`.
/// Used to verify that a diagnostic label falls within (or at) the alias def site.
fn spans_overlap(label_span: SourceSpan, container_span: SourceSpan) -> bool {
    label_span.start < container_span.end && label_span.end > container_span.start
}

// ── step-1: RED (case a — non-exported name) ──────────────────────────────────

/// A `pub type LeakName<Q: Dimension> = Q / NotExportedThing` alias whose body
/// references `NotExportedThing` — a name that is neither a stdlib built-in nor
/// declared anywhere in scope — must produce at least one Error-severity diagnostic
/// AT THE ALIAS DEFINITION SITE after the def-site guard is implemented.
///
/// RED today: parametric alias bodies are only resolved at use sites, so
/// `NotExportedThing` is silently accepted and no def-site error is emitted.
#[test]
fn pub_parametric_alias_unknown_body_name_rejected_at_def_site() {
    // Inline source isolating the LeakName flaw (case a).
    let source = "pub type LeakName<Q: Dimension> = Q / NotExportedThing";
    let parsed = parse_with_stdlib(source, ModulePath::single("test_leak_name"));
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse without errors: {:?}",
        parsed.errors
    );

    let decl_span = alias_span(&parsed, "LeakName");

    let module = compile_with_stdlib(&parsed);
    let errors = error_diagnostics(&module);

    // (i) ≥1 Error-severity diagnostic
    assert!(
        !errors.is_empty(),
        "pub type LeakName<Q: Dimension> = Q / NotExportedThing must produce \
         at least one Error diagnostic — NotExportedThing is undefined; got none"
    );

    // (ii) At least one Error diagnostic whose label span falls within the alias
    //      declaration (def-site binding — NOT a use-site or prelude-sentinel span).
    let has_def_site_label = errors.iter().any(|e| {
        e.labels
            .iter()
            .any(|l| !l.span.is_empty() && spans_overlap(l.span, decl_span))
    });
    assert!(
        has_def_site_label,
        "at least one Error diagnostic must have a label span within the LeakName \
         alias declaration (offset range {:?}); errors: {:?}",
        decl_span,
        errors
    );
}

// ── step-3: RED (case b — param-bound violation) ──────────────────────────────

/// A `pub type BadBound<P> = Holder<P>` alias (where `Holder` requires
/// `T: Dimension` but `P` is unbounded) must produce a def-site Error diagnostic
/// citing the bound violation after the param-bound check is implemented.
///
/// RED after step-2: the name-existence check passes (Holder is a known structure,
/// P is a type param of BadBound) but no param-bound check exists yet, so the
/// mismatched bound is silently accepted.
#[test]
fn pub_parametric_alias_param_bound_violation_rejected_at_def_site() {
    let source = r#"
        structure def Holder<T: Dimension> {
            param x : Real
        }
        pub type BadBound<P> = Holder<P>
    "#;
    let parsed = parse_with_stdlib(source, ModulePath::single("test_bad_bound"));
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse without errors: {:?}",
        parsed.errors
    );

    let decl_span = alias_span(&parsed, "BadBound");

    let module = compile_with_stdlib(&parsed);
    let errors = error_diagnostics(&module);

    // (i) ≥1 Error-severity diagnostic specifically about the bound violation.
    assert!(
        !errors.is_empty(),
        "pub type BadBound<P> = Holder<P> must produce at least one Error \
         diagnostic — P is unbounded but Holder requires T: Dimension; got none"
    );

    // (ii) At least one Error diagnostic whose label span falls within the
    //      BadBound alias declaration (def-site binding).
    let has_def_site_label = errors.iter().any(|e| {
        e.labels
            .iter()
            .any(|l| !l.span.is_empty() && spans_overlap(l.span, decl_span))
    });
    assert!(
        has_def_site_label,
        "at least one Error diagnostic must have a label span within the BadBound \
         alias declaration (offset range {:?}); errors: {:?}",
        decl_span,
        errors
    );
}

// ── step-5: Acceptance + no-false-positive regression ─────────────────────────

/// BOTH flaws in parametric_alias_def_site_reject.ri must be reported as
/// def-site Error diagnostics:
///   - LeakName: undefined/non-exported name `NotExportedThing`
///   - BadBound: unbounded param P used where Holder requires T: Dimension
///
/// This test uses the committed fixture file (include_str!) to pin the
/// full acceptance signal — the two cases must not mask each other.
#[test]
fn committed_reject_fixture_fails_with_def_site_diagnostics() {
    let parsed = parse_with_stdlib(REJECT_FIXTURE, ModulePath::single("test_reject"));
    assert!(
        parsed.errors.is_empty(),
        "reject fixture must parse without errors: {:?}",
        parsed.errors
    );

    let leak_span = alias_span(&parsed, "LeakName");
    let bad_bound_span = alias_span(&parsed, "BadBound");

    let module = compile_with_stdlib(&parsed);
    let errors = error_diagnostics(&module);

    assert!(
        !errors.is_empty(),
        "reject fixture must produce at least one Error diagnostic; got none. \
         diagnostics: {:?}",
        module.diagnostics
    );

    // Both flaws must be reported at their def sites.
    let has_leak_error = errors.iter().any(|e| {
        e.labels
            .iter()
            .any(|l| !l.span.is_empty() && spans_overlap(l.span, leak_span))
    });
    assert!(
        has_leak_error,
        "reject fixture: no Error diagnostic at the LeakName alias def site \
         (offset range {:?}); errors: {:?}",
        leak_span,
        errors
    );

    let has_bad_bound_error = errors.iter().any(|e| {
        e.labels
            .iter()
            .any(|l| !l.span.is_empty() && spans_overlap(l.span, bad_bound_span))
    });
    assert!(
        has_bad_bound_error,
        "reject fixture: no Error diagnostic at the BadBound alias def site \
         (offset range {:?}); errors: {:?}",
        bad_bound_span,
        errors
    );
}

/// Valid pub parametric aliases in parametric_alias_def_site_ok.ri must compile
/// without any new Error diagnostics from the def-site guard:
///   - `pub type Vel<Q: Dimension> = Q / Time` — mirrors stdlib Rate
///   - `pub type Wrap<U: Dimension> = Box2<U>` — U's bound satisfies Box2's T: Dimension
///
/// This pins against over-rejection by the def-site guard.
#[test]
fn valid_pub_parametric_alias_accepted() {
    let parsed = parse_with_stdlib(OK_FIXTURE, ModulePath::single("test_ok"));
    assert!(
        parsed.errors.is_empty(),
        "ok fixture must parse without errors: {:?}",
        parsed.errors
    );

    let module = compile_with_stdlib(&parsed);
    let errors = error_diagnostics(&module);

    assert!(
        errors.is_empty(),
        "valid pub parametric aliases must produce zero Error diagnostics; \
         got {} error(s): {:?}",
        errors.len(),
        errors
    );
}

/// The stdlib itself must load without any Error diagnostics introduced by
/// the def-site guard. Specifically, `pub type Rate<Q: Dimension> = Q / Time`
/// (units.ri:106) must pass the guard clean.
///
/// This test exercises the no-regression invariant for prelude aliases.
#[test]
fn stdlib_loads_clean_under_def_site_guard() {
    // A trivial user module — compile_with_stdlib loads the entire stdlib prelude.
    let source = "structure def Probe { param x : Length }";
    let parsed = parse_with_stdlib(source, ModulePath::single("test_stdlib_guard"));
    assert!(
        parsed.errors.is_empty(),
        "trivial module must parse without errors: {:?}",
        parsed.errors
    );

    let module = compile_with_stdlib(&parsed);
    let errors = error_diagnostics(&module);

    assert!(
        errors.is_empty(),
        "stdlib must load without Error diagnostics under the def-site guard; \
         got {} error(s): {:?}",
        errors.len(),
        errors
    );
}
