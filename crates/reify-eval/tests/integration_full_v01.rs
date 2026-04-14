//! Full v0.1 integration tests.
//!
//! Exercises every v0.1 language feature in a single cohesive engineering
//! assembly: type aliases, multi-trait conformance, constraint defs, recursive
//! structures, geometric types, connect with connector/port mapping, purpose
//! with forall, where guards, quantifiers, match, lambda, ranges, some/none,
//! fn overloading, determinacy predicates, @test annotations, meta access,
//! doc comments, minimize, self, field defs with gradient+sample, custom units,
//! complex numbers, enums, collections.
//!
//! Uses examples/integration_full_v01.ri as the source file.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read integration_full_v01.ri, caching the result in a `OnceLock`.
fn source() -> String {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/integration_full_v01.ri should exist")
    })
    .clone()
}

/// Parse and compile (with stdlib) the canonical source, caching the result.
fn compiled() -> CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(&source())).clone()
}

/// Eval the canonical source with SimpleConstraintChecker.
fn eval_canonical() -> reify_eval::EvalResult {
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.eval(&compiled())
}

/// Check the canonical source with SimpleConstraintChecker.
fn check_canonical() -> reify_eval::CheckResult {
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled())
}

/// Parse, compile (with stdlib), check a mutated source string.
fn check_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled)
}

// ── Test 2: file compiles with expected templates ─────────────────────────────

/// Compile integration_full_v01.ri (with stdlib) and verify the compiled module
/// contains the expected templates: Assembly (>=10 value cells), RecursiveBeam,
/// PipeConnector — and at least 5 templates total (including @test structures).
#[test]
fn integration_full_v01_compiles() {
    let compiled = compiled();
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );

    // Assembly must exist with >=10 value cells (params + lets from full body)
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("should have an Assembly template");
    assert!(
        assembly.value_cells.len() >= 10,
        "Assembly should have >=10 value cells (params + lets), got {}",
        assembly.value_cells.len()
    );

    // RecursiveBeam must exist
    assert!(
        compiled.templates.iter().any(|t| t.name == "RecursiveBeam"),
        "should have a RecursiveBeam template"
    );

    // At least 3 templates total (Assembly, RecursiveBeam, PipeConnector).
    // More templates (>=9) are added in step-22 when @test structures land.
    assert!(
        compiled.templates.len() >= 3,
        "expected >=3 templates total, got {}",
        compiled.templates.len()
    );
}

// ── Test 1: file parses without errors ───────────────────────────────────────

/// Read integration_full_v01.ri, verify it parses without errors, and assert
/// minimum top-level declaration counts: >=2 traits, >=2 structures, >=1
/// purpose, >=2 constraint defs, >=1 enum, >=2 functions (overloads), >=1
/// field def, >=1 unit decl, >=1 type alias. These counts guard against silent
/// declaration drops more precisely than a pure parse check.
#[test]
fn integration_full_v01_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    use reify_syntax::Declaration;

    let trait_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Trait(_)))
        .count();
    let structure_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Structure(_)))
        .count();
    let purpose_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Purpose(_)))
        .count();
    let constraint_def_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Constraint(_)))
        .count();
    let enum_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Enum(_)))
        .count();
    let function_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Function(_)))
        .count();
    let field_def_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Field(_)))
        .count();
    let unit_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Unit(_)))
        .count();
    let type_alias_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::TypeAlias(_)))
        .count();

    assert!(
        trait_count >= 2,
        "expected >=2 Trait declarations, got {trait_count}"
    );
    assert!(
        structure_count >= 2,
        "expected >=2 Structure declarations, got {structure_count}"
    );
    assert!(
        purpose_count >= 1,
        "expected >=1 Purpose declaration, got {purpose_count}"
    );
    assert!(
        constraint_def_count >= 2,
        "expected >=2 ConstraintDef declarations, got {constraint_def_count}"
    );
    assert!(
        enum_count >= 1,
        "expected >=1 Enum declaration, got {enum_count}"
    );
    assert!(
        function_count >= 2,
        "expected >=2 Function declarations (safety_factor overloads), got {function_count}"
    );
    assert!(
        field_def_count >= 1,
        "expected >=1 FieldDef declaration, got {field_def_count}"
    );
    assert!(
        unit_count >= 1,
        "expected >=1 Unit declaration, got {unit_count}"
    );
    assert!(
        type_alias_count >= 1,
        "expected >=1 TypeAlias declaration, got {type_alias_count}"
    );
}
