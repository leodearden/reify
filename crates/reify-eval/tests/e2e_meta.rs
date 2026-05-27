//! E2E tests for meta block access: parse → compile → Engine.eval().
//!
//! Exercises the full parse→compile→eval pipeline for `meta.key` expressions,
//! ensuring integration across the parser, compiler, and evaluator boundaries.

use reify_test_support::{
    assert_no_diagnostic, assert_no_error_diagnostics, make_engine, parse_and_compile,
    parse_compile_expect_err,
};
use reify_core::{Severity, ValueCellId};
use reify_ir::{Satisfaction, Value};

// ---------------------------------------------------------------------------
// --- let binding uses meta.key ---
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

    let compiled = parse_and_compile(source);

    // Sanity: Widget template should be present and tagged as a Structure.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should be present in compiled output");
    assert_eq!(
        template.entity_kind,
        reify_compiler::EntityKind::Structure,
        "expected Widget to have entity_kind == Structure"
    );

    // Eval
    let mut engine = make_engine();
    let result = engine.eval(&compiled);

    // Guard: no eval errors
    assert_no_error_diagnostics(&result.diagnostics, "eval");

    // Assert
    let desc_id = ValueCellId::new("Widget", "desc");
    assert_eq!(
        result.values.get(&desc_id),
        Some(&Value::String("A widget".to_string())),
        "Widget.desc should resolve to 'A widget' via meta.description"
    );
}

// ---------------------------------------------------------------------------
// --- multiple meta keys in one block ---
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

    let compiled = parse_and_compile(source);

    // Eval
    let mut engine = make_engine();
    let result = engine.eval(&compiled);

    // Guard: no eval errors
    assert_no_error_diagnostics(&result.diagnostics, "eval");

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
// --- meta.key on occurrence entity ---
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

    let compiled = parse_and_compile(source);

    // Sanity: the Welding template should be present and tagged as an Occurrence.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Welding")
        .expect("Welding template should be present in compiled output");
    assert_eq!(
        template.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected Welding to have entity_kind == Occurrence"
    );

    // Eval
    let mut engine = make_engine();
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
// --- child structure meta.key resolves as sub-component ---
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

    let compiled = parse_and_compile(source);

    // Assert both templates are present in compiled output.
    let _part = compiled
        .templates
        .iter()
        .find(|t| t.name == "Part")
        .expect("Part template should be present in compiled output");
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should be present in compiled output");

    // Assert Assembly's sub-component wiring: `sub part = Part()` must compile
    // to a SubComponentDecl with name "part" and structure_name "Part".
    assert_eq!(
        assembly.sub_components.len(),
        1,
        "Assembly should have exactly one sub-component"
    );
    let sub = &assembly.sub_components[0];
    assert_eq!(
        sub.name, "part",
        "sub-component binding name should be 'part'"
    );
    assert_eq!(
        sub.structure_name, "Part",
        "sub-component structure_name should be 'Part'"
    );

    // Eval
    let mut engine = make_engine();
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
// --- meta let binding propagates downstream ---
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

    let compiled = parse_and_compile(source);

    // Eval
    let mut engine = make_engine();
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
// --- meta.key string equality (match) ---
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

    let compiled = parse_and_compile(source);

    // Eval
    let mut engine = make_engine();
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
// --- meta.key string equality (mismatch) ---
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

    let compiled = parse_and_compile(source);

    // Eval
    let mut engine = make_engine();
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
// --- regression guards: missing key and no meta block ---
// ---------------------------------------------------------------------------

/// Regression guard (suggestion 8): accessing a meta key that does not exist
/// in the meta block must produce at least one compile-time Error diagnostic.
///
/// Source: `Widget` has `meta { description = "A widget" }` but the let binding
/// reads `meta.nonexistent`.  The compiler must reject this with an error.
#[test]
fn e2e_meta_access_missing_key() {
    let source = r#"
        structure def Widget {
            meta {
                description = "A widget"
            }
            let x : String = meta.nonexistent
        }
    "#;

    let compiled = parse_compile_expect_err(source, "meta block has no key");
    // Mutual-exclusion guard: the missing-key path must NOT produce the
    // no-meta-block error.  If both appear (or the wrong one appears), a future
    // compiler regression would otherwise stay hidden.
    assert_no_diagnostic(
        &compiled.diagnostics,
        Severity::Error,
        "entity has no meta block",
    );
}

/// Regression guard (suggestion 9): accessing `meta.description` on a structure
/// that has no meta block at all must produce at least one compile-time Error.
///
/// Source: `Gadget` has no meta block but the body contains `let x : String = meta.description`.
/// The compiler must reject this with an error rather than panicking or silently
/// returning a default value at eval time.
#[test]
fn e2e_meta_access_no_meta_block() {
    let source = r#"
        structure def Gadget {
            param count : Integer = 1
            let x : String = meta.description
        }
    "#;

    let compiled = parse_compile_expect_err(source, "entity has no meta block");
    // Mutual-exclusion guard: the no-meta-block path must NOT produce the
    // missing-key error.  If both appear (or the wrong one appears), a future
    // compiler regression would otherwise stay hidden.
    assert_no_diagnostic(
        &compiled.diagnostics,
        Severity::Error,
        "meta block has no key",
    );
}

// ---------------------------------------------------------------------------
// --- meta.key in constraint expression ---
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
            let tag_value : String = meta.tag
            constraint meta.tag == "valid"
        }
    "#;

    let compiled = parse_and_compile(source);

    // Check (eval + constraint evaluation) — must not panic when meta.key
    // appears in a constraint expression
    let mut engine = make_engine();
    let result = engine.check(&compiled);

    // Guard: no check-phase errors
    assert_no_error_diagnostics(&result.diagnostics, "check");

    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly one constraint result for the single \
         `constraint meta.tag == \"valid\"` declaration; \
         engine may have dropped or duplicated the MetaAccess constraint expression"
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

    // Assert the value handed to the constraint checker is Value::String("valid"),
    // not a coerced or default value — observing the resolved value on the SAME
    // compiled module that runs the constraint expression.
    let tag_value_id = ValueCellId::new("S", "tag_value");
    assert_eq!(
        result.values.get(&tag_value_id),
        Some(&Value::String("valid".to_string())),
        "S.tag_value should resolve to 'valid' via meta.tag — proves the value \
         handed to the constraint checker is Value::String(\"valid\"), not a \
         coerced or default value"
    );
}
