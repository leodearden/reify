//! Tests for the std.determinacy.purposes stdlib module (task-4016 ζ).
//!
//! The module ships two pub purposes:
//!   - `simulation_ready(subject : Structure)` — checks that all geometric params
//!     are determined (PRD §5 body); geometry-undef → Violated per esc-4016-163.
//!   - `design_review(subject : Structure)` — checks that all params are
//!     constrained (solver variables); any determined param → Violated.
//!
//! Step coverage:
//!   step-1: module presence + compile (simulation_ready shape)
//!   step-3: producer wholesale-merge boundary test
//!   step-5: design_review presence

use reify_compiler::stdlib_loader;
use reify_core::ModulePath;

// ── step-1: module presence + clean compile + simulation_ready shape ──────────

/// The stdlib contains a module with path "std.determinacy.purposes".
///
/// RED until determinacy_purposes.ri is registered in stdlib_loader.rs.
#[test]
fn std_determinacy_purposes_module_exists() {
    let modules = stdlib_loader::load_stdlib();
    let found = modules
        .iter()
        .any(|m| format!("{}", m.path) == "std/determinacy/purposes");
    assert!(
        found,
        "expected stdlib module 'std.determinacy.purposes' (path 'std/determinacy/purposes') \
         to be present; found paths: {:?}",
        modules.iter().map(|m| format!("{}", m.path)).collect::<Vec<_>>()
    );
}

/// std.determinacy.purposes compiles with zero Error-severity diagnostics.
///
/// RED until the module registers and compiles cleanly.
#[test]
fn std_determinacy_purposes_has_no_errors() {
    let modules = stdlib_loader::load_stdlib();
    let module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/determinacy/purposes")
        .expect("std.determinacy.purposes module should exist (see std_determinacy_purposes_module_exists)");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "std.determinacy.purposes should have zero Error-severity diagnostics, got: {:?}",
        errors
    );
}

/// std.determinacy.purposes exposes a pub purpose named "simulation_ready"
/// with exactly one param whose entity_kind is "Structure".
///
/// RED until the module is registered and contains the correct declaration.
#[test]
fn std_determinacy_purposes_has_simulation_ready() {
    let modules = stdlib_loader::load_stdlib();
    let module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/determinacy/purposes")
        .expect("std.determinacy.purposes module should exist");

    let purpose = module
        .compiled_purposes
        .iter()
        .find(|p| p.name == "simulation_ready");

    assert!(
        purpose.is_some(),
        "std.determinacy.purposes should contain a purpose named 'simulation_ready'; \
         found purposes: {:?}",
        module
            .compiled_purposes
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    let purpose = purpose.unwrap();
    assert!(
        purpose.is_pub,
        "simulation_ready must be pub so it can be merged into user modules"
    );
    assert_eq!(
        purpose.params.len(),
        1,
        "simulation_ready should have exactly 1 param, got: {:?}",
        purpose.params
    );
    assert_eq!(
        purpose.params[0].entity_kind,
        "Structure",
        "simulation_ready param must have entity_kind 'Structure'"
    );
}

// ── step-3: producer wholesale-merge boundary test ────────────────────────────
//
// These tests are added in step-3 but live in this file for cohesion.
// They fail RED in step-3 (merge not yet wired), GREEN after step-4 impl.

/// compile_with_stdlib of a minimal user source (no purpose, no import)
/// yields a module whose compiled_purposes contains "simulation_ready".
///
/// This tests that merge_prelude_purposes propagates the stdlib pub purpose
/// into every user module compiled against the stdlib.
///
/// RED until merge_prelude_purposes is wired into compile_with_prelude_context.
#[test]
fn compile_with_stdlib_merges_simulation_ready_into_user_module() {
    let source = r#"
structure Part {
    param width : Length = 80mm
}
"#;
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let has_sim_ready = compiled
        .compiled_purposes
        .iter()
        .any(|p| p.name == "simulation_ready");
    assert!(
        has_sim_ready,
        "compile_with_stdlib output should contain 'simulation_ready' purpose merged \
         from std.determinacy.purposes; found: {:?}",
        compiled
            .compiled_purposes
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );
}

/// The minimal-prelude path (parse_and_compile / compile_source) does NOT
/// inject simulation_ready — pinning the merge boundary so the purpose is
/// only available when the full stdlib is the prelude.
///
/// Uses reify_test_support::parse_and_compile (compile with empty prelude).
#[test]
fn minimal_prelude_does_not_inject_simulation_ready() {
    let source = r#"
structure Part {
    param width : Length = 80mm
}
"#;
    // compile_source uses the empty-prelude path (no stdlib).
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);

    let has_sim_ready = compiled
        .compiled_purposes
        .iter()
        .any(|p| p.name == "simulation_ready");
    assert!(
        !has_sim_ready,
        "minimal-prelude compile should NOT inject 'simulation_ready'; \
         this purpose is only available via the stdlib prelude. \
         Found purposes: {:?}",
        compiled
            .compiled_purposes
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );
}

// ── amendment: first-wins shadow contract ─────────────────────────────────────
//
// Exercises the behavioral contract in merge_prelude_purposes: if a user
// module declares a purpose with the same name as a stdlib purpose, the user's
// declaration wins (first-wins shadow) and the stdlib one is NOT appended.
// The surviving purpose must have a user-source declaration_span (NOT the
// prelude sentinel SourceSpan::prelude()), proving the user-declared purpose
// is the one in the merged list.
//
// If the guard (!result.iter().any(|up| up.name == p.name)) is changed to
// last-wins or always-append, this test fails — ensuring the shadow contract
// cannot silently regress.

/// A user purpose named "simulation_ready" shadows the stdlib one: after
/// compile_with_stdlib the module has exactly ONE "simulation_ready" purpose,
/// and its declaration_span is the user's (NOT SourceSpan::prelude()).
#[test]
fn user_purpose_shadows_stdlib_simulation_ready() {
    // User source: declares its own simulation_ready before the stdlib merge.
    // The purpose body must be syntactically valid; it uses the same reflective
    // quantifier as the stdlib one but over .params (simpler body, same shape).
    let source = r#"
purpose simulation_ready(subject : Structure) {
    constraint forall p in subject.params: determined(p)
}
structure Part {
    param width : Length = 80mm
}
"#;
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single("test_shadow"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in user source: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Exactly one "simulation_ready" — the stdlib duplicate must be suppressed.
    let sim_ready_purposes: Vec<_> = compiled
        .compiled_purposes
        .iter()
        .filter(|p| p.name == "simulation_ready")
        .collect();
    assert_eq!(
        sim_ready_purposes.len(),
        1,
        "expected exactly 1 'simulation_ready' purpose (user shadows stdlib); \
         found {}: {:?}",
        sim_ready_purposes.len(),
        sim_ready_purposes.iter().map(|p| &p.name).collect::<Vec<_>>()
    );

    // The surviving purpose must be the user's, NOT the prelude-merged one.
    // SourceSpan::prelude() has start == end == u32::MAX; user spans are smaller.
    let survivor = sim_ready_purposes[0];
    assert!(
        !survivor.declaration_span.is_prelude(),
        "user-declared 'simulation_ready' should have a user source span, \
         not SourceSpan::prelude() — the stdlib copy must have been suppressed. \
         Got declaration_span: {:?}",
        survivor.declaration_span
    );
}

// ── step-5: design_review presence ───────────────────────────────────────────
//
// Tests added in step-5. RED until design_review is added to determinacy_purposes.ri.

/// std.determinacy.purposes exposes a pub purpose named "design_review"
/// with exactly one param whose entity_kind is "Structure".
///
/// RED until design_review is added to determinacy_purposes.ri.
#[test]
fn std_determinacy_purposes_has_design_review() {
    let modules = stdlib_loader::load_stdlib();
    let module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/determinacy/purposes")
        .expect("std.determinacy.purposes module should exist");

    let purpose = module
        .compiled_purposes
        .iter()
        .find(|p| p.name == "design_review");

    assert!(
        purpose.is_some(),
        "std.determinacy.purposes should contain a purpose named 'design_review'; \
         found purposes: {:?}",
        module
            .compiled_purposes
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    let purpose = purpose.unwrap();
    assert!(
        purpose.is_pub,
        "design_review must be pub so it can be merged into user modules"
    );
    assert_eq!(
        purpose.params.len(),
        1,
        "design_review should have exactly 1 param, got: {:?}",
        purpose.params
    );
    assert_eq!(
        purpose.params[0].entity_kind,
        "Structure",
        "design_review param must have entity_kind 'Structure'"
    );
}
