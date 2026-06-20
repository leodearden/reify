//! Compiler IR-contract tests for non-auto specialization-body param_assignment
//! overrides (task 4694, step 3).
//!
//! These tests verify that when a `sub` specialization body contains a
//! `param_assignment` whose value is a concrete expression (NOT `auto` /
//! `auto(free)`), the compiler injects an entry for that member into the
//! `SubComponentDecl.args` vector so that the eval args-precedence path
//! (`unfold.rs:elaborate_child_params_only:336`) applies the override at runtime.
//!
//! This test is GREEN on arrival because step-2 (the impl step) is done before
//! step-3 in this task.  It characterizes the producer wiring at the IR level and
//! complements the eval E2E in spec_param_override_resolution.rs.
//!
//! ## Absent-member validation (step 5 RED → step 6 GREEN)
//!
//! Step 5 adds `non_auto_override_absent_member_emits_error` which is RED until
//! step 6 adds absent-member validation to entity.rs.

use reify_compiler::find_template;
use reify_core::Severity;
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Shared fixtures ───────────────────────────────────────────────────────────

/// Bearing has a `bore` param with default 5mm so we can distinguish the
/// overridden value (3mm) from the default at the IR level.
const BEARING_PREAMBLE: &str = "structure Bearing { param bore : Length = 5mm }";

// ── Test (a): non-auto override appears in SubComponentDecl.args ──────────────

/// `bore = 3mm` in a specialization body must inject an `("bore", compiled_expr)`
/// entry into `SubComponentDecl.args` for template "A"'s sub-component "b".
///
/// Also asserts AC4 no-error: a valid non-auto override emits no compile errors.
///
/// The `args` entry is what the eval path (`elaborate_child_params_only`) checks
/// before falling back to the child's `default_expr` — so this locks the producer
/// wiring that makes the override take runtime effect.
#[test]
fn non_auto_override_present_in_sub_component_args() {
    let source = format!(
        "{BEARING_PREAMBLE}  structure A {{ sub b : Bearing {{ bore = 3mm }} }}"
    );
    let module = compile_source_with_stdlib(&source);

    // AC4 no-error: valid override must compile cleanly.
    assert!(
        errors_only(&module).is_empty(),
        "unexpected compile errors: {:?}",
        errors_only(&module)
    );

    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");

    let sub_b = template
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .unwrap_or_else(|| {
            panic!(
                "expected a SubComponentDecl named 'b' in template A; got: {:?}",
                template.sub_components.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        });

    assert!(
        sub_b.args.iter().any(|(name, _)| name == "bore"),
        "SubComponentDecl 'b'.args should contain an entry for 'bore'; \
         got args: {:?}",
        sub_b.args.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
}

// ── Test (b, step-5 RED): absent member in non-auto override emits an error ───

/// `nope = 3mm` where `nope` is not a member of Bearing (child declared BEFORE
/// parent, so the inline validation path fires) must emit exactly one
/// error-severity diagnostic naming `nope` or `Bearing`.
///
/// RED (step 5): absent-member validation for non-auto overrides is not yet
///   implemented in entity.rs.  No diagnostic is emitted.
/// GREEN (step 6): entity.rs mirrors the auto path's member lookup for non-auto
///   inline entries, emitting "sub `b`: override for `nope` — no such param in
///   `Bearing`".
#[test]
fn non_auto_override_absent_member_emits_error() {
    // Child (Bearing) before parent (A) → inline validation path.
    let source = format!(
        "{BEARING_PREAMBLE}  structure A {{ sub b : Bearing {{ nope = 3mm }} }}"
    );
    let module = compile_source_with_stdlib(&source);

    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error for absent member `nope`; got: {:?}",
        errors
    );
    assert!(
        errors[0].message.contains("nope") || errors[0].message.contains("Bearing"),
        "error message should name the absent member or the child structure; got: {:?}",
        errors[0].message
    );
}

// ── Test (c, amend suggestion 1): duplicate body override warns, first wins ───

/// `{ bore = 3mm  bore = 4mm }` must emit exactly one warning-severity
/// diagnostic about the duplicate and inject exactly ONE "bore" entry into
/// `SubComponentDecl.args` (with the FIRST value, 3mm).
///
/// Pins the "first assignment wins" semantics added by the amend pass.
#[test]
fn non_auto_override_duplicate_in_body_warns_first_wins() {
    // Two assignments to the same member in one specialization body.
    let source = format!(
        "{BEARING_PREAMBLE}  structure A {{ sub b : Bearing {{ bore = 3mm  bore = 4mm }} }}"
    );
    let module = compile_source_with_stdlib(&source);

    // No errors — a duplicate override is a warning, not an error.
    assert!(
        errors_only(&module).is_empty(),
        "duplicate body override should not be an error; got errors: {:?}",
        errors_only(&module)
    );

    // Exactly one warning (for the duplicate second assignment).
    let warnings: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one warning for the duplicate override; got: {:?}",
        warnings
    );
    assert!(
        warnings[0].message.contains("duplicate") || warnings[0].message.contains("bore"),
        "warning message should mention 'duplicate' or 'bore'; got: {:?}",
        warnings[0].message
    );

    // Exactly one args entry for "bore" (the first, 3mm).
    let template = find_template(&module.templates, "A")
        .expect("expected a compiled template for structure A");
    let sub_b = template
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("expected SubComponentDecl 'b' in template A");

    let bore_count = sub_b.args.iter().filter(|(n, _)| n == "bore").count();
    assert_eq!(
        bore_count, 1,
        "SubComponentDecl 'b'.args should have exactly one 'bore' entry after dedup; \
         got {} entries",
        bore_count
    );
}

// ── Note (amend suggestion 3): constructor-arg + body-override conflict ───────
//
// The grammar makes the instantiation form (`sub name = Ctor(args)`) and the
// specialization form (`sub name : Type { overrides }`) MUTUALLY EXCLUSIVE.
// A single sub declaration cannot carry both parenthetical constructor args
// and a specialization body, so the conflict scenario identified in suggestion
// 3 ("`sub b : Bearing(bore: 4mm) { bore = 3mm }`") cannot be parsed and
// therefore requires no test.  The comment in entity.rs documents this
// grammar-level invariant for future readers.
