//! End-to-end integration tests for meta block access.
//!
//! Tests the complete pipeline: parse → compile → Engine.eval() → check values.
//! Covers 8 scenarios: basic structure, occurrence, error cases (nonexistent key,
//! no meta block), sub-structure, multiple keys, and string equality comparisons.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity, Value, ValueCellId};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse source and compile, returning the `CompiledModule`.
/// Panics if there are parse errors.
fn compile_source(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse → compile → eval. Panics on parse errors or compile errors.
/// Returns the `(CompiledModule, EvalResult)` pair.
fn parse_compile_eval(
    source: &str,
) -> (reify_compiler::CompiledModule, reify_eval::EvalResult) {
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    (compiled, result)
}

// ---------------------------------------------------------------------------
// step-1: E2E meta access on structure
// ---------------------------------------------------------------------------

/// Full pipeline: structure with meta block, let binding accessing meta.
/// Verifies the meta value is resolved to Value::String at eval time.
#[test]
fn e2e_meta_access_on_structure() {
    let source = r#"
structure def Bracket {
    meta { description = "A bracket" }
    let label : String = meta.description
}
"#;
    let (_, result) = parse_compile_eval(source);
    let id = ValueCellId::new("Bracket", "label");
    assert_eq!(
        result.values.get(&id),
        Some(&Value::String("A bracket".to_string())),
        "expected label == \"A bracket\""
    );
}

// ---------------------------------------------------------------------------
// step-2: E2E meta access on occurrence
// ---------------------------------------------------------------------------

/// Full pipeline: occurrence def with meta, let binding accessing meta.
/// Verifies entity_kind == Occurrence and meta value resolves correctly.
#[test]
fn e2e_meta_access_on_occurrence() {
    let source = r#"
occurrence def Welding {
    meta { process = "MIG" }
    let label : String = meta.process
}
"#;
    let (compiled, result) = parse_compile_eval(source);
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(
        compiled.templates[0].entity_kind,
        reify_compiler::EntityKind::Occurrence
    );
    let id = ValueCellId::new("Welding", "label");
    assert_eq!(
        result.values.get(&id),
        Some(&Value::String("MIG".to_string())),
        "expected label == \"MIG\""
    );
}

// ---------------------------------------------------------------------------
// step-3: E2E nonexistent key — compile-time error
// ---------------------------------------------------------------------------

/// Full pipeline: accessing a nonexistent meta key produces a compile-time
/// error containing "no key".
#[test]
fn e2e_meta_nonexistent_key_error() {
    let source = r#"
structure def X {
    meta { a = "1" }
    let x : String = meta.nonexistent
}
"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one compile error");
    assert!(
        errors.iter().any(|d| d.message.contains("no key")),
        "expected 'no key' in error messages, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-4: E2E no meta block — compile-time error
// ---------------------------------------------------------------------------

/// Full pipeline: accessing meta when there is no meta block produces a
/// compile-time error containing "no meta block".
#[test]
fn e2e_meta_no_meta_block_error() {
    let source = r#"
structure def X {
    param width : Length = 10mm
    let x : String = meta.foo
}
"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one compile error");
    assert!(
        errors.iter().any(|d| d.message.contains("no meta block")),
        "expected 'no meta block' in error messages, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// step-5: E2E meta on sub-structure
// ---------------------------------------------------------------------------

/// Full pipeline: Part with meta + Assembly containing sub part = Part().
/// Verifies that Part's own let binding (meta.material) resolves to "steel"
/// when both templates are compiled and evaluated together.
#[test]
fn e2e_meta_on_sub_structure() {
    let source = r#"
structure def Part {
    meta { material = "steel" }
    let label : String = meta.material
    param size : Length = 10mm
}
structure def Assembly {
    sub part = Part()
}
"#;
    let (_, result) = parse_compile_eval(source);
    let id = ValueCellId::new("Part", "label");
    assert_eq!(
        result.values.get(&id),
        Some(&Value::String("steel".to_string())),
        "expected Part.label == \"steel\""
    );
}

// ---------------------------------------------------------------------------
// step-6: E2E multiple meta keys in let bindings
// ---------------------------------------------------------------------------

/// Full pipeline: Widget with three meta keys, each accessed by a separate
/// let binding — all three resolve to their expected string values.
#[test]
fn e2e_meta_value_in_let_binding() {
    let source = r#"
structure def Widget {
    meta {
        author = "Team A",
        version = "2.0",
        material = "steel"
    }
    let a : String = meta.author
    let v : String = meta.version
    let m : String = meta.material
}
"#;
    let (_, result) = parse_compile_eval(source);
    assert_eq!(
        result.values.get(&ValueCellId::new("Widget", "a")),
        Some(&Value::String("Team A".to_string())),
        "expected a == \"Team A\""
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("Widget", "v")),
        Some(&Value::String("2.0".to_string())),
        "expected v == \"2.0\""
    );
    assert_eq!(
        result.values.get(&ValueCellId::new("Widget", "m")),
        Some(&Value::String("steel".to_string())),
        "expected m == \"steel\""
    );
}

// ---------------------------------------------------------------------------
// step-7: E2E meta string comparison — equal
// ---------------------------------------------------------------------------

/// Full pipeline: `meta.grade == "A2"` evaluates to Value::Bool(true).
#[test]
fn e2e_meta_string_comparison_equal() {
    let source = r#"
structure def Bolt {
    meta { grade = "A2" }
    let matches : Bool = meta.grade == "A2"
}
"#;
    let (_, result) = parse_compile_eval(source);
    let id = ValueCellId::new("Bolt", "matches");
    assert_eq!(
        result.values.get(&id),
        Some(&Value::Bool(true)),
        "expected matches == true when grade == \"A2\""
    );
}

// ---------------------------------------------------------------------------
// step-8: E2E meta string comparison — not equal
// ---------------------------------------------------------------------------

/// Full pipeline: `meta.grade == "A4"` evaluates to Value::Bool(false)
/// when the meta value is "A2".
#[test]
fn e2e_meta_string_comparison_not_equal() {
    let source = r#"
structure def Bolt {
    meta { grade = "A2" }
    let matches : Bool = meta.grade == "A4"
}
"#;
    let (_, result) = parse_compile_eval(source);
    let id = ValueCellId::new("Bolt", "matches");
    assert_eq!(
        result.values.get(&id),
        Some(&Value::Bool(false)),
        "expected matches == false when grade is \"A2\" not \"A4\""
    );
}
