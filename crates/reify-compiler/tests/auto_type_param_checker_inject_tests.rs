//! Parity tests: threading `&dyn ConstraintChecker` through the compiler
//! entry points is a compile-time no-op **for the cell-dependent-constraint
//! fixture** (see `CELL_DEP_SEAL_SOURCE`).
//!
//! For each new `*_checked` entry-point sibling, we assert that injecting
//! an always-indeterminate checker produces byte-identical
//! `auto_type_substitution` and diagnostics to the stub-default sibling.
//!
//! # Scope of the parity claim
//!
//! The fixture used by the four parity tests (`CELL_DEP_SEAL_SOURCE`) has
//! **no constraints** on the parameterized template (`Bearing<T: Seal>`).
//! When the constraint list is empty, every checker — stub, `AlwaysIndeterminate`,
//! or any real evaluating checker — trivially accepts all candidates.  The
//! parity tests therefore verify end-to-end **seam wiring** (the `*_checked`
//! entry points call through to the phase correctly), not constraint evaluation.
//!
//! A constant constraint (no cell references) does NOT produce the same result
//! under a real evaluating checker: `SimpleConstraintChecker` evaluates
//! `constraint 0 > 1` → `Bool(false)` → `Violated`, while the stub always
//! returns `Indeterminate`.  See `compile_with_stdlib_checked_constant_constraint_diverges`
//! for a concrete example.
//!
//! **RED state:** before step-2 implementation, the `*_checked` symbols do
//! not exist — this file fails with E0425/E0599 compile errors.

use std::fs;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_core::{ModulePath, Severity};
use reify_ir::{ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction};

/// Common fixture for the four parity tests.
///
/// `Bearing<T: Seal>` has **no constraints**, so `build_constraints_template`
/// returns an empty slice and the checker is called with an empty constraint
/// list.  All checkers trivially accept all candidates, making the parity
/// assertion sound for any checker.  The test exercises the end-to-end
/// wiring of the `*_checked` entry points rather than constraint evaluation.
const CELL_DEP_SEAL_SOURCE: &str = r#"
    trait Seal {}
    structure def GasketSeal : Seal { param d : Real = 2.0 }
    structure def Bearing<T: Seal> { param seal : T }
    structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;

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

/// A local always-violated checker.
///
/// Stands in for the constant-evaluating behavior of `SimpleConstraintChecker`
/// on constant-false expressions: `SimpleConstraintChecker` evaluates
/// `constraint 0 > 1` → `Bool(false)` → `Violated`, while
/// `AlwaysIndeterminate` / the stub return `Indeterminate`.
///
/// NOTE: `reify-constraints` is not a dev-dep of `reify-compiler`, so
/// `SimpleConstraintChecker` cannot be used directly in these integration
/// tests.  `AlwaysViolated` accurately represents what `SimpleConstraintChecker`
/// does for expressions that contain no cell references and evaluate to a
/// false boolean.
struct AlwaysViolated;

impl ConstraintChecker for AlwaysViolated {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Violated,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

/// An always-indeterminate checker that counts how many times `check()` is
/// called, using interior mutability so it works through `&dyn`.
///
/// The count proves the `*_checked` entry points actually reach the checker
/// (not just that the symbols compile). The DFS leaf visitor calls `check()`
/// once per candidate type even when the constraint list is empty, so the
/// counter increments at least once for any `auto:` resolution that finds a
/// candidate.
struct CountingIndeterminate {
    calls: std::sync::atomic::AtomicU32,
}

impl CountingIndeterminate {
    fn new() -> Self {
        Self { calls: std::sync::atomic::AtomicU32::new(0) }
    }
}

impl ConstraintChecker for CountingIndeterminate {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

/// Parse a source string with the stdlib enum seed.
fn parse_auto_source(source: &str, module_name: &str) -> reify_ast::ParsedModule {
    reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name))
}

/// Assert that `stub` and `checked` produce byte-identical auto-resolution
/// results (same `auto_type_substitution` and same diagnostics).
fn assert_parity(
    stub: &reify_compiler::CompiledModule,
    checked: &reify_compiler::CompiledModule,
    ctx: &str,
) {
    assert_eq!(
        checked.auto_type_substitution,
        stub.auto_type_substitution,
        "{ctx}: auto_type_substitution must match stub path"
    );
    assert_eq!(
        diag_tuples(checked),
        diag_tuples(stub),
        "{ctx}: diagnostics must match stub path"
    );
}

// ─── compile_with_stdlib_checked parity ───────────────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_with_stdlib_checked` must
/// produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_stdlib`.
#[test]
fn compile_with_stdlib_checked_parity() {
    let parsed = parse_auto_source(CELL_DEP_SEAL_SOURCE, "test_checker_inject");
    let stub = reify_compiler::compile_with_stdlib(&parsed);
    let checked = reify_compiler::compile_with_stdlib_checked(&parsed, &AlwaysIndeterminate);
    assert_parity(&stub, &checked, "compile_with_stdlib_checked");
}

// ─── compile_with_prelude_checked parity ──────────────────────────────────────

/// Injecting `AlwaysIndeterminate` through `compile_with_prelude_checked` must
/// produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_prelude`.
#[test]
fn compile_with_prelude_checked_parity() {
    let parsed = parse_auto_source(CELL_DEP_SEAL_SOURCE, "test_checker_inject_prelude");
    // Use empty prelude for simplicity; both paths get the same empty-prelude context.
    let prelude: &[reify_compiler::CompiledModule] = &[];
    let stub = reify_compiler::compile_with_prelude(&parsed, prelude);
    let checked =
        reify_compiler::compile_with_prelude_checked(&parsed, prelude, &AlwaysIndeterminate);
    assert_parity(&stub, &checked, "compile_with_prelude_checked");
}

// ─── compile_with_prelude_context_checked parity ──────────────────────────────

/// Injecting `CountingIndeterminate` through `compile_with_prelude_context_checked`
/// must produce byte-identical `auto_type_substitution` and diagnostics to the
/// stub-default `compile_with_prelude_context`, AND the checker must be invoked
/// at least once — proving the seam reaches the checker, not just that the
/// `*_checked` symbol resolves.
#[test]
fn compile_with_prelude_context_checked_parity() {
    let parsed = parse_auto_source(CELL_DEP_SEAL_SOURCE, "test_checker_inject_ctx");
    // Build a prelude context from an empty prelude (consistent with above).
    let prelude: Vec<reify_compiler::CompiledModule> = vec![];
    let ctx =
        reify_compiler::PreludeContext::new(&prelude.iter().collect::<Vec<_>>());
    let stub = reify_compiler::compile_with_prelude_context(&parsed, &ctx);
    let counter = CountingIndeterminate::new();
    let checked =
        reify_compiler::compile_with_prelude_context_checked(&parsed, &ctx, &counter);
    assert_parity(&stub, &checked, "compile_with_prelude_context_checked");
    assert!(
        counter.calls.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "CountingIndeterminate::check() was never called — the checker seam \
         did not reach the feasibility evaluator"
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
#[test]
fn compile_entry_with_stdlib_cfg_checked_parity() {
    let parsed = parse_auto_source(CELL_DEP_SEAL_SOURCE, "test_checker_inject_cfg");
    let dir = tempfile::TempDir::new().expect("tempdir");
    let resolver = ModuleResolver::new(dir.path(), dir.path().join("stdlib"));
    let cfg = CfgSet::default();
    let stub =
        reify_compiler::module_dag::compile_entry_with_stdlib_cfg(&parsed, &resolver, &cfg);
    let checked = reify_compiler::module_dag::compile_entry_with_stdlib_cfg_checked(
        &parsed,
        &resolver,
        &cfg,
        &AlwaysIndeterminate,
    );
    assert_parity(&stub, &checked, "compile_entry_with_stdlib_cfg_checked");
}

// ─── Constant-constraint divergence ───────────────────────────────────────────

/// Documents the live behavioral divergence between the stub and a
/// constant-evaluating checker.
///
/// The fixture adds `constraint 0 > 1` to the parameterized template
/// (`Bearing<T: Seal>`).  This constant expression contains no cell
/// references, so it does NOT evaluate to `Undef` at compile time:
///
/// - Under the **stub** (`CompileTimeIndeterminateChecker`): returns
///   `Indeterminate` regardless of the expression → `GasketSeal` is
///   feasible → `auto: Seal` resolves successfully.
/// - Under **`AlwaysViolated`** (analogous to `SimpleConstraintChecker`
///   evaluating `0 > 1` → `Bool(false)` → `Violated`): `GasketSeal` is
///   rejected → "no feasible candidates" error.
///
/// This test scopes the parity claim: the `*_checked` entry points are
/// byte-identical to the stub **only for cell-dependent constraints or when
/// the parameterized template has no constraints**.  A constant-false
/// constraint causes divergence once a real evaluating checker is injected.
#[test]
fn compile_with_stdlib_checked_constant_constraint_diverges() {
    // Bearing carries a constant-false constraint `0 > 1` — no cell references.
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> {
            param seal : T
            constraint 0 > 1
        }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let parsed = parse_auto_source(source, "test_checker_inject_const");

    // Stub path: `0 > 1` is treated as Indeterminate → GasketSeal accepted
    // → auto: Seal resolves to GasketSeal with no errors.
    let stub = reify_compiler::compile_with_stdlib(&parsed);
    let stub_errors: Vec<_> = diag_tuples(&stub)
        .into_iter()
        .filter(|(sev, _)| *sev == Severity::Error)
        .collect();
    assert!(
        stub_errors.is_empty(),
        "stub path: constant constraint treated as Indeterminate → no error; \
         got: {:?}",
        stub_errors
    );
    assert!(
        !stub.auto_type_substitution.as_slice().is_empty(),
        "stub path: auto: Seal should resolve to GasketSeal; substitution: {:?}",
        stub.auto_type_substitution.as_slice()
    );

    // Always-violated path (analogous to SimpleConstraintChecker evaluating
    // `0 > 1` → Bool(false) → Violated): GasketSeal is rejected →
    // "no feasible candidates" error, auto_type_substitution empty.
    let violated = reify_compiler::compile_with_stdlib_checked(&parsed, &AlwaysViolated);
    let violated_errors: Vec<_> = diag_tuples(&violated)
        .into_iter()
        .filter(|(sev, msg)| *sev == Severity::Error && msg.contains("no feasible candidates"))
        .collect();
    assert!(
        !violated_errors.is_empty(),
        "always-violated path: constant constraint → Violated → \
         'no feasible candidates' error expected; all diagnostics: {:?}",
        diag_tuples(&violated)
    );
    assert!(
        violated.auto_type_substitution.as_slice().is_empty(),
        "always-violated path: no auto: resolution should succeed when all \
         candidates are rejected; substitution: {:?}",
        violated.auto_type_substitution.as_slice()
    );
}

// ─── Import-compile asymmetry (architecture §4) ───────────────────────────────

/// Documents the deliberate seam: `compile_entry_with_stdlib_cfg_checked`
/// threads `checker` into the **entry module's** compile only; imported
/// modules are compiled via `dag.compile_module` → `compile_with_prelude_refs`
/// → the **stub** checker.
///
/// Regression guard: if a future refactor accidentally threads the real
/// checker into the DAG-walk import compiles, this test will catch it.
///
/// **Fixture:**
/// - `bearing.ri`: has `constraint 0 > 1` (constant-false) + `auto: Seal` slot.
///   Under the stub it resolves; under `AlwaysViolated` it fails.
/// - `assembly.ri`: imports `bearing`, no own `auto:` slot.
///
/// **Under `AlwaysViolated`:**
/// - Part A — `bearing.ri` compiled **as the entry** →
///   "no feasible candidates" error (real checker applied to entry).
/// - Part B — `assembly.ri` compiled **as the entry** →
///   `bearing.ri` is an import, compiled with the stub → its auto: resolves
///   → no "no feasible candidates" error surfaces in `assembly`'s diagnostics.
#[test]
fn compile_entry_with_stdlib_cfg_checked_import_uses_stub() {
    // bearing.ri: constant-false constraint + auto: slot.
    let bearing_source = r#"
        trait Seal {}
        pub structure def GasketSeal : Seal { param d : Real = 2.0 }
        pub structure def Bearing<T: Seal> {
            param seal : T
            constraint 0 > 1
        }
        pub structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    // assembly.ri: imports bearing, no own auto: slot.
    let assembly_source = r#"
        import bearing
        structure def TopLevel { param x : Real = 1.0 }
    "#;

    let dir = tempfile::TempDir::new().expect("tempdir");
    fs::write(dir.path().join("bearing.ri"), bearing_source).expect("write bearing.ri");
    fs::write(dir.path().join("assembly.ri"), assembly_source).expect("write assembly.ri");

    let resolver = ModuleResolver::new(dir.path(), dir.path().join("stdlib"));
    let cfg = CfgSet::default();

    // Part A: compile bearing.ri AS THE ENTRY with AlwaysViolated.
    // The entry compile path uses the real checker → constant constraint is
    // Violated → GasketSeal rejected → "no feasible candidates" error.
    let parsed_bearing =
        reify_compiler::parse_with_stdlib(bearing_source, ModulePath::single("bearing"));
    let entry_bearing = reify_compiler::module_dag::compile_entry_with_stdlib_cfg_checked(
        &parsed_bearing,
        &resolver,
        &cfg,
        &AlwaysViolated,
    );
    let entry_bearing_errors: Vec<_> = diag_tuples(&entry_bearing)
        .into_iter()
        .filter(|(sev, msg)| *sev == Severity::Error && msg.contains("no feasible candidates"))
        .collect();
    assert!(
        !entry_bearing_errors.is_empty(),
        "compiling bearing.ri AS ENTRY with AlwaysViolated must produce \
         'no feasible candidates' error; all diagnostics: {:?}",
        diag_tuples(&entry_bearing)
    );

    // Part B: compile assembly.ri AS THE ENTRY with AlwaysViolated.
    // bearing.ri is compiled as an IMPORT via dag.compile_module →
    // compile_with_prelude_refs (stub) → constant constraint is Indeterminate
    // → GasketSeal accepted → bearing's auto: resolves fine.
    // No "no feasible candidates" error should appear in assembly's diagnostics.
    let parsed_assembly =
        reify_compiler::parse_with_stdlib(assembly_source, ModulePath::single("assembly"));
    let entry_assembly = reify_compiler::module_dag::compile_entry_with_stdlib_cfg_checked(
        &parsed_assembly,
        &resolver,
        &cfg,
        &AlwaysViolated,
    );
    let import_feasible_errors: Vec<_> = diag_tuples(&entry_assembly)
        .into_iter()
        .filter(|(sev, msg)| *sev == Severity::Error && msg.contains("no feasible candidates"))
        .collect();
    assert!(
        import_feasible_errors.is_empty(),
        "when bearing.ri is compiled as an IMPORT (stub checker), AlwaysViolated \
         must not affect bearing's auto: resolution; 'no feasible candidates' must \
         NOT appear in assembly's diagnostics; got: {:?}",
        import_feasible_errors
    );
}
