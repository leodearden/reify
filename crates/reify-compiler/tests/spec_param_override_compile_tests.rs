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

use reify_compiler::{find_template};
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
