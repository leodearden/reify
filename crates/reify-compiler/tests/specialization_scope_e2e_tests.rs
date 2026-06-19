//! End-to-end compiler tests for `sub name : StructName { body }` specialization-scope body (task 3573).
//!
//! Tests drive real `.ri` source through `reify_compiler::parse_with_stdlib` →
//! `reify_compiler::compile_with_stdlib` and assert that:
//!
//! (a) A fixture containing forbidden body members (param/port/sub) emits
//!     exactly 3 `SpecializationForbiddenDecl` Error diagnostics (PRD AC 1-3).
//! (b) A fixture containing only permitted body members (where-guarded
//!     param_assignment + let + constraint) emits zero such diagnostics
//!     (PRD AC 4-5).
//!
//! Diagnostics are filtered by `DiagnosticCode::SpecializationForbiddenDecl`
//! so that unrelated diagnostics (e.g. unresolved stub types) do not affect
//! the assertions — mirrors the convention in dep-tests
//! `specialization_scope_lowering_tests.rs` and `specialization_scope_validation_tests.rs`.

use reify_core::{DiagnosticCode, ModulePath, Severity};

/// Filter diagnostics to only those with `code == SpecializationForbiddenDecl`.
fn forbidden_decl_diagnostics(
    diagnostics: &[reify_core::Diagnostic],
) -> Vec<&reify_core::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SpecializationForbiddenDecl))
        .collect()
}

/// Fixture: `structure Base { param size : Length = 10mm }` +
/// `structure Assembly { sub motor : Base { param x : Length  port p : MechanicalPort  sub child : Base } }`
///
/// The three body members (param/port/sub) are forbidden in a specialization-scope body;
/// the validator must emit exactly 3 `SpecializationForbiddenDecl` Error diagnostics
/// (PRD AC 1-3).
///
/// Messages are asserted via `.any()` (not positional) because diagnostic ordering
/// within the body walk is an internal detail not pinned by the contract.
#[test]
fn forbidden_spec_scope_fixture_emits_three_forbidden_decl_diagnostics() {
    let source = include_str!("fixtures/specialization_scope_forbidden.ri");
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("spec_e2e_forbidden"));

    assert!(
        parsed.errors.is_empty(),
        "forbidden fixture must parse with zero errors (grammar from task 3569), got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let diags = forbidden_decl_diagnostics(&compiled.diagnostics);

    assert_eq!(
        diags.len(),
        3,
        "forbidden fixture must emit exactly 3 SpecializationForbiddenDecl diagnostics \
         (one for param x, one for port p, one for sub child); got {}: {:#?}",
        diags.len(),
        compiled.diagnostics
    );

    // All must be Error severity.
    for d in &diags {
        assert_eq!(
            d.severity,
            Severity::Error,
            "all SpecializationForbiddenDecl diagnostics must be Error severity, got: {d:#?}"
        );
    }

    // Each of the three forbidden member names must appear in at least one message.
    //
    // NOTE: these assertions couple to the single-quoted format in
    // `specialization_scope_check.rs::validate_module`:
    //   format!("'{kind}' declaration '{name}' is not permitted in a specialization scope (spec §8.7)")
    // The `Diagnostic` struct does not expose structured kind/name fields, so
    // message substring matching is the only available approach. Update these
    // strings if the diagnostic wording changes.
    let has_param_x = diags
        .iter()
        .any(|d| d.message.contains("'param'") && d.message.contains("'x'"));
    assert!(
        has_param_x,
        "expected a SpecializationForbiddenDecl diagnostic for forbidden 'param' named 'x', \
         got: {:#?}",
        diags
    );

    let has_port_p = diags
        .iter()
        .any(|d| d.message.contains("'port'") && d.message.contains("'p'"));
    assert!(
        has_port_p,
        "expected a SpecializationForbiddenDecl diagnostic for forbidden 'port' named 'p', \
         got: {:#?}",
        diags
    );

    let has_sub_child = diags
        .iter()
        .any(|d| d.message.contains("'sub'") && d.message.contains("'child'"));
    assert!(
        has_sub_child,
        "expected a SpecializationForbiddenDecl diagnostic for forbidden 'sub' named 'child', \
         got: {:#?}",
        diags
    );
}

/// Fixture: `structure Base { param shaft_diameter : Length = 8mm }` +
/// `structure Assembly { sub motor : Base { shaft_diameter = 12mm where high_torque  let m = shaft_diameter * 2  constraint shaft_diameter > 1mm } }`
///
/// All body members are permitted (where-guarded param_assignment + let + constraint);
/// the validator must emit zero `SpecializationForbiddenDecl` diagnostics (PRD AC 4-5).
#[test]
fn permitted_only_spec_scope_fixture_emits_zero_forbidden_decl_diagnostics() {
    let source = include_str!("fixtures/specialization_scope_permitted.ri");
    let parsed =
        reify_compiler::parse_with_stdlib(source, ModulePath::single("spec_e2e_permitted"));

    assert!(
        parsed.errors.is_empty(),
        "permitted fixture must parse with zero errors (grammar from task 3569), got: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let diags = forbidden_decl_diagnostics(&compiled.diagnostics);

    assert!(
        diags.is_empty(),
        "permitted-only fixture must emit zero SpecializationForbiddenDecl diagnostics, \
         got {}: {:#?}",
        diags.len(),
        diags
    );
}
