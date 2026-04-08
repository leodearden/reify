//! E2E tests for meta block access: parse → compile → Engine.eval().
//!
//! Exercises the full parse→compile→eval pipeline for `meta.key` expressions,
//! ensuring integration across the parser, compiler, and evaluator boundaries.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

// ---------------------------------------------------------------------------
// step-13: E2E — let binding using meta.key resolves to Value::String
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with meta block + let binding using `meta.key`,
/// compile, eval, assert the let binding resolves to the expected string.
#[test]
fn e2e_meta_access_let_binding() {
    let source = r#"
        structure def Widget {
            meta {
                description = "A widget"
            }
            let desc : String = meta.description
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let desc_id = ValueCellId::new("Widget", "desc");
    assert_eq!(
        result.values.get(&desc_id),
        Some(&Value::String("A widget".to_string())),
        "Widget.desc should resolve to 'A widget' via meta.description"
    );
}

// ---------------------------------------------------------------------------
// step-15: E2E — multiple meta keys in one block
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with two meta keys, two let bindings each reading
/// a different key.  Both should resolve to their respective string values.
#[test]
fn e2e_meta_access_multiple_keys() {
    let source = r#"
        structure def Gear {
            meta {
                name = "Gear",
                version = "2.0"
            }
            let n : String = meta.name
            let v : String = meta.version
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert both keys
    let n_id = ValueCellId::new("Gear", "n");
    let v_id = ValueCellId::new("Gear", "v");

    assert_eq!(
        result.values.get(&n_id),
        Some(&Value::String("Gear".to_string())),
        "Gear.n should resolve to 'Gear' via meta.name"
    );
    assert_eq!(
        result.values.get(&v_id),
        Some(&Value::String("2.0".to_string())),
        "Gear.v should resolve to '2.0' via meta.version"
    );
}

// ---------------------------------------------------------------------------
// task-213: E2E — meta.key on an `occurrence def` entity
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with an `occurrence def` that has a meta block
/// and a let binding reading `meta.key`. Compile, eval, assert the let
/// resolves to the expected string. This exercises the occurrence code path
/// alongside the already-tested structure path, confirming meta access works
/// uniformly across entity kinds.
#[test]
fn e2e_meta_access_on_occurrence() {
    let source = r#"
        occurrence def Welding {
            meta {
                method = "TIG"
            }
            let label : String = meta.method
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Sanity: the compiled template should be tagged as an Occurrence.
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(
        compiled.templates[0].entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected occurrence entity kind"
    );

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let label_id = ValueCellId::new("Welding", "label");
    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("TIG".to_string())),
        "Welding.label should resolve to 'TIG' via meta.method on an occurrence"
    );
}

// ---------------------------------------------------------------------------
// task-213 step-1/2: E2E — meta access on a `structure def` entity (Structure kind)
// ---------------------------------------------------------------------------

/// Full pipeline: parse a `structure def` with a meta block + let binding, compile
/// (assert no errors, assert entity_kind == Structure), eval, assert the let
/// binding resolves to the expected string.  Explicitly verifies the Structure
/// entity-kind path (as opposed to the Occurrence path in the existing test).
#[test]
fn e2e_meta_access_on_structure_resolves() {
    let source = r#"
        structure def Bracket {
            meta {
                part_number = "BR-001"
            }
            let pn : String = meta.part_number
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Sanity: the compiled template should be tagged as a Structure.
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(
        compiled.templates[0].entity_kind,
        reify_compiler::EntityKind::Structure,
        "expected structure entity kind"
    );

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let pn_id = ValueCellId::new("Bracket", "pn");
    assert_eq!(
        result.values.get(&pn_id),
        Some(&Value::String("BR-001".to_string())),
        "Bracket.pn should resolve to 'BR-001' via meta.part_number"
    );
}

// ---------------------------------------------------------------------------
// task-213 step-3/4: E2E — nonexistent meta key produces compile error
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with `meta.nonexistent` where only `a` is defined.
/// After compile, diagnostics should contain at least one Error whose message
/// includes "no key".  No eval step — the error is a compile-time rejection.
#[test]
fn e2e_meta_nonexistent_key_error() {
    let source = r#"
        structure def S {
            meta {
                a = "1"
            }
            let x : String = meta.nonexistent
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile — expect an error diagnostic
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one compile error");
    assert!(
        errors.iter().any(|d| d.message.contains("no key")),
        "expected 'no key' error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// task-213 step-5/6: E2E — meta access without a meta block produces error
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with `meta.foo` on a structure that has no meta block.
/// After compile, diagnostics should contain at least one Error whose message
/// includes "no meta block".
#[test]
fn e2e_meta_no_meta_block_error() {
    let source = r#"
        structure def S {
            param width : Length = 10mm
            let x : String = meta.foo
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile — expect an error diagnostic
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected at least one compile error");
    assert!(
        errors.iter().any(|d| d.message.contains("no meta block")),
        "expected 'no meta block' error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// task-213 step-7/8: E2E — child structure's meta.key resolves when used as sub-component
// ---------------------------------------------------------------------------

/// Full pipeline: two structures — `Part` with a meta block + let binding, and
/// `Assembly` that instantiates `Part` as a sub-component.  Parse→compile (assert
/// no errors)→eval, assert Part.label == Value::String("steel").  Verifies that
/// a child template's meta_map entry is built correctly when the entity is
/// referenced as a sub-component in another structure.
#[test]
fn e2e_meta_sub_structure_child_meta() {
    let source = r#"
        structure def Part {
            meta {
                material = "steel"
            }
            let label : String = meta.material
            param size : Length = 10mm
        }

        structure def Assembly {
            sub part = Part()
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert Part's own label resolves
    let label_id = ValueCellId::new("Part", "label");
    assert_eq!(
        result.values.get(&label_id),
        Some(&Value::String("steel".to_string())),
        "Part.label should resolve to 'steel' via meta.material"
    );
}

// ---------------------------------------------------------------------------
// task-213 step-9/10: E2E — meta value stored in let binding propagates downstream
// ---------------------------------------------------------------------------

/// Full pipeline: `v` holds meta.version, and `is_v2` compares `v == "2"`.
/// Both bindings must resolve correctly, proving that a meta-derived let binding
/// propagates as a usable value to subsequent expressions.
#[test]
fn e2e_meta_let_binding_downstream() {
    let source = r#"
        structure def S {
            meta {
                version = "2"
            }
            let v : String = meta.version
            let is_v2 : Bool = v == "2"
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let v_id = ValueCellId::new("S", "v");
    let is_v2_id = ValueCellId::new("S", "is_v2");

    assert_eq!(
        result.values.get(&v_id),
        Some(&Value::String("2".to_string())),
        "S.v should resolve to '2' via meta.version"
    );
    assert_eq!(
        result.values.get(&is_v2_id),
        Some(&Value::Bool(true)),
        "S.is_v2 should be true when v == '2'"
    );
}

// ---------------------------------------------------------------------------
// task-213 step-11/12: E2E — meta.key string equality (matching case → true)
// ---------------------------------------------------------------------------

/// Full pipeline: `matches` is set to `meta.tag == "valid"`.  When tag IS "valid"
/// the expression must evaluate to Value::Bool(true).
#[test]
fn e2e_meta_string_eq_match() {
    let source = r#"
        structure def S {
            meta {
                tag = "valid"
            }
            let matches : Bool = meta.tag == "valid"
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let matches_id = ValueCellId::new("S", "matches");
    assert_eq!(
        result.values.get(&matches_id),
        Some(&Value::Bool(true)),
        "S.matches should be true when meta.tag == 'valid'"
    );
}

// ---------------------------------------------------------------------------
// task-213 step-13/14: E2E — meta.key string equality (non-matching case → false)
// ---------------------------------------------------------------------------

/// Full pipeline: `mismatch` is set to `meta.tag == "invalid"`.  When tag IS "valid"
/// (and NOT "invalid") the expression must evaluate to Value::Bool(false).
#[test]
fn e2e_meta_string_eq_mismatch() {
    let source = r#"
        structure def S {
            meta {
                tag = "valid"
            }
            let mismatch : Bool = meta.tag == "invalid"
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert
    let mismatch_id = ValueCellId::new("S", "mismatch");
    assert_eq!(
        result.values.get(&mismatch_id),
        Some(&Value::Bool(false)),
        "S.mismatch should be false when meta.tag ('valid') != 'invalid'"
    );
}

// ---------------------------------------------------------------------------
// step-17: E2E — meta.key in a constraint expression
// ---------------------------------------------------------------------------

/// Full pipeline: parse source with `constraint meta.tag == "valid"`.
/// The constraint expression contains a MetaAccess node; eval should not panic,
/// and the constraint result should be Satisfied (MockConstraintChecker default).
#[test]
fn e2e_meta_access_in_constraint() {
    let source = r#"
        structure def S {
            meta {
                tag = "valid"
            }
            constraint meta.tag == "valid"
        }
    "#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Check (eval + constraint evaluation) — must not panic when meta.key
    // appears in a constraint expression
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // Assert constraint_results is non-empty so the loop below is not vacuous.
    // If the engine silently drops the constraint expression, this will fail.
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint result, got zero \
         (engine may have dropped the MetaAccess constraint expression)"
    );

    // Assert no constraint violations
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
