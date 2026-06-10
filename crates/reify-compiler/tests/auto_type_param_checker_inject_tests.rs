//! Parity tests: threading `&dyn ConstraintChecker` through the compiler
//! entry points is a compile-time no-op.
//!
//! For each new `*_checked` entry-point sibling, we assert that injecting
//! an always-indeterminate checker (or the real `SimpleConstraintChecker`)
//! produces byte-identical `auto_type_substitution` and diagnostics to the
//! stub-default sibling.
//!
//! **RED state:** before step-2 implementation, the `*_checked` symbols do
//! not exist — this file fails with E0425/E0599 compile errors.
//!
//! # Fixture design
//!
//! Source uses an `auto:` sub-component whose candidate constraints reference
//! a value cell (`param d`). At compile time the `ValueMap` is empty (cells
//! are `Undef`), so the constraint evaluates to `Value::Undef` →
//! `Satisfaction::Indeterminate` under BOTH the `CompileTimeIndeterminateChecker`
//! stub AND the real `SimpleConstraintChecker`. This is the sound premise
//! documented at `auto_type_param_phase.rs:48-55`.

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_core::{ModulePath, Severity};
use reify_ir::{ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction};

/// A local always-indeterminate checker (same contract as the internal stub).
struct AlwaysIndeterminate;

impl ConstraintChecker for AlwaysIndeterminate {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

/// Extract `(severity, message)` pairs for diagnostic comparison.
/// `Diagnostic` does not derive `PartialEq`, so we compare the two scalar
/// fields that carry semantic content.
fn diag_tuples(compiled: &reify_compiler::CompiledModule) -> Vec<(Severity, String)> {
    compiled
        .diagnostics
        .iter()
        .map(|d| (d.severity, d.message.clone()))
        .collect()
}

/// Helper: parse an `auto:` source string with the stdlib enum seed.
fn parse_auto_source(source: &str) -> reify_ast::ParsedModule {
    reify_compiler::parse_with_stdlib(source, ModulePath::single("test_checker_inject"))
}

// ─── compile_with_stdlib_checked parity ───────────────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_with_stdlib_checked` must
/// produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_stdlib`.
#[test]
fn compile_with_stdlib_checked_parity() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let parsed = parse_auto_source(source);

    let stub_result = reify_compiler::compile_with_stdlib(&parsed);
    let checked_result =
        reify_compiler::compile_with_stdlib_checked(&parsed, &AlwaysIndeterminate);

    assert_eq!(
        checked_result.auto_type_substitution,
        stub_result.auto_type_substitution,
        "compile_with_stdlib_checked: auto_type_substitution must match stub path"
    );
    assert_eq!(
        diag_tuples(&checked_result),
        diag_tuples(&stub_result),
        "compile_with_stdlib_checked: diagnostics must match stub path"
    );
}

// ─── compile_with_prelude_checked parity ──────────────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_with_prelude_checked` must
/// produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_prelude`.
#[test]
fn compile_with_prelude_checked_parity() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    // Use empty prelude for simplicity; both paths get the same empty-prelude context.
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        ModulePath::single("test_checker_inject_prelude"),
    );
    let prelude: &[reify_compiler::CompiledModule] = &[];

    let stub_result = reify_compiler::compile_with_prelude(&parsed, prelude);
    let checked_result =
        reify_compiler::compile_with_prelude_checked(&parsed, prelude, &AlwaysIndeterminate);

    assert_eq!(
        checked_result.auto_type_substitution,
        stub_result.auto_type_substitution,
        "compile_with_prelude_checked: auto_type_substitution must match stub path"
    );
    assert_eq!(
        diag_tuples(&checked_result),
        diag_tuples(&stub_result),
        "compile_with_prelude_checked: diagnostics must match stub path"
    );
}

// ─── compile_with_prelude_context_checked parity ──────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_with_prelude_context_checked`
/// must produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_prelude_context`.
#[test]
fn compile_with_prelude_context_checked_parity() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let parsed = reify_compiler::parse_with_stdlib(
        source,
        ModulePath::single("test_checker_inject_ctx"),
    );

    // Build a prelude context from an empty prelude (consistent with above).
    let prelude: Vec<reify_compiler::CompiledModule> = vec![];
    let ctx = reify_compiler::PreludeContext::new(
        &prelude.iter().collect::<Vec<_>>(),
    );

    let stub_result = reify_compiler::compile_with_prelude_context(&parsed, &ctx);
    let checked_result =
        reify_compiler::compile_with_prelude_context_checked(&parsed, &ctx, &AlwaysIndeterminate);

    assert_eq!(
        checked_result.auto_type_substitution,
        stub_result.auto_type_substitution,
        "compile_with_prelude_context_checked: auto_type_substitution must match stub path"
    );
    assert_eq!(
        diag_tuples(&checked_result),
        diag_tuples(&stub_result),
        "compile_with_prelude_context_checked: diagnostics must match stub path"
    );
}

// ─── compile_entry_with_stdlib_cfg_checked parity ─────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_entry_with_stdlib_cfg_checked`
/// must produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_entry_with_stdlib_cfg`.
///
/// Uses an empty `CfgSet` and a resolver that points at a temporary directory
/// with no user modules — the source has no user imports to follow, so the DAG
/// walk is a no-op and the test exercises only the entry-compile path.
///
/// **RED state:** `compile_entry_with_stdlib_cfg_checked` does not yet exist.
#[test]
fn compile_entry_with_stdlib_cfg_checked_parity() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let parsed = reify_compiler::parse_with_stdlib(
        source,
        ModulePath::single("test_checker_inject_cfg"),
    );

    // Empty resolver: no user-import files on disk. The source has no import
    // declarations, so the DAG walk never consults the resolver.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let resolver = ModuleResolver::new(dir.path(), dir.path().join("stdlib"));
    let cfg = CfgSet::default();

    let stub_result =
        reify_compiler::module_dag::compile_entry_with_stdlib_cfg(&parsed, &resolver, &cfg);
    let checked_result = reify_compiler::module_dag::compile_entry_with_stdlib_cfg_checked(
        &parsed,
        &resolver,
        &cfg,
        &AlwaysIndeterminate,
    );

    assert_eq!(
        checked_result.auto_type_substitution,
        stub_result.auto_type_substitution,
        "compile_entry_with_stdlib_cfg_checked: auto_type_substitution must match stub path"
    );
    assert_eq!(
        diag_tuples(&checked_result),
        diag_tuples(&stub_result),
        "compile_entry_with_stdlib_cfg_checked: diagnostics must match stub path"
    );
}
