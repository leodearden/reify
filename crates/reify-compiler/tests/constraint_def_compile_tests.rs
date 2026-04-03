//! Compiler-level tests for constraint def features (task 199).
//!
//! Tests edge cases, error paths, and cross-module import for constraint defs.
//! Complements the existing constraint_inst_tests.rs from task 198.

use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
use reify_compiler::*;
use reify_types::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse and compile, returning the template with the given name + all diagnostics.
fn compile_template(source: &str, name: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let diags = module.diagnostics.clone();
    let tmpl = module
        .templates
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template '{name}' in compiled module"));
    (tmpl, diags)
}

/// Collect only error diagnostics.
fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── Test 1: simple def one predicate template ────────────────────────────────

/// Compile a single-predicate constraint def, instantiate in a structure.
/// Result: exactly 1 constraint in the template.
#[test]
fn simple_def_one_predicate_template() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 0
}
structure S {
    param t: Length
    constraint MinWall(wall: t)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected exactly 1 constraint in template, got {}",
        tmpl.constraints.len()
    );

    // Label should be MinWall[0]
    assert_eq!(
        tmpl.constraints[0].label,
        Some("MinWall[0]".to_string()),
        "expected label MinWall[0], got {:?}",
        tmpl.constraints[0].label
    );
}

// ── Test 2: multi-param three args all substituted ───────────────────────────

/// Compile a 3-param constraint def. Verify all 3 param references are substituted.
#[test]
fn multi_param_three_args() {
    let source = r#"
constraint def Triple {
    param a: Length
    param b: Length
    param c: Length
    a > b
    b > c
    a > c
}
structure S {
    param x: Length
    param y: Length
    param z: Length
    constraint Triple(a: x, b: y, c: z)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // 3 predicates → 3 constraints
    assert_eq!(
        tmpl.constraints.len(),
        3,
        "expected exactly 3 constraints, got {}",
        tmpl.constraints.len()
    );

    // Each constraint should have a BinOp whose operands are ValueRefs (after substitution)
    for (i, cc) in tmpl.constraints.iter().enumerate() {
        match &cc.expr.kind {
            CompiledExprKind::BinOp { left, right, .. } => {
                assert!(
                    matches!(&left.kind, CompiledExprKind::ValueRef(_)),
                    "constraint[{i}] left should be ValueRef after substitution, got {:?}",
                    left.kind
                );
                assert!(
                    matches!(&right.kind, CompiledExprKind::ValueRef(_)),
                    "constraint[{i}] right should be ValueRef after substitution, got {:?}",
                    right.kind
                );
            }
            other => panic!("constraint[{i}] should be BinOp, got {:?}", other),
        }
    }

    // Labels: Triple[0], Triple[1], Triple[2]
    for (i, expected_label) in ["Triple[0]", "Triple[1]", "Triple[2]"].iter().enumerate() {
        assert_eq!(
            tmpl.constraints[i].label,
            Some(expected_label.to_string()),
            "expected label {expected_label}, got {:?}",
            tmpl.constraints[i].label
        );
    }
}

// ── Test 3: multiple predicates count ────────────────────────────────────────

/// 3-predicate constraint def produces exactly 3 constraints in the template.
#[test]
fn multiple_predicates_conjunction_count() {
    let source = r#"
constraint def Trio {
    param x: Length
    x > 0
    x > 1
    x > 2
}
structure S {
    param v: Length
    constraint Trio(x: v)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        3,
        "expected exactly 3 constraints for 3-predicate def, got {}",
        tmpl.constraints.len()
    );
}

// ── Test 4: named args in different order ─────────────────────────────────────

/// Providing args in reverse declaration order must produce correct substitution.
#[test]
fn named_args_different_order() {
    let source = r#"
constraint def Bounded {
    param x: Length
    param lo: Length
    param hi: Length
    x >= lo
    x <= hi
}
structure S {
    param d: Length
    constraint Bounded(hi: 100mm, lo: 5mm, x: d)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        2,
        "expected 2 constraints, got {}",
        tmpl.constraints.len()
    );

    // First constraint: x >= lo → d >= 5mm
    // Left operand should be ValueRef(S.d)
    match &tmpl.constraints[0].expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(*op, BinOp::Ge, "first constraint should be Ge (>=)");
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(id.member, "d", "left should be ValueRef(d), got {}", id.member);
                }
                other => panic!("left should be ValueRef, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

// ── Test 5: multiple defs in same module coexist ─────────────────────────────

/// Two constraint defs can coexist; each instantiation resolves to its own def.
#[test]
fn multiple_defs_same_module() {
    let source = r#"
constraint def MinA {
    param a: Length
    a > 1
}
constraint def MinB {
    param b: Length
    b > 2
}
structure S {
    param x: Length
    param y: Length
    constraint MinA(a: x)
    constraint MinB(b: y)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        2,
        "expected 2 constraints (one per def), got {}",
        tmpl.constraints.len()
    );

    // First constraint should be labeled MinA[0], second MinB[0]
    let has_min_a = tmpl
        .constraints
        .iter()
        .any(|c| c.label == Some("MinA[0]".to_string()));
    let has_min_b = tmpl
        .constraints
        .iter()
        .any(|c| c.label == Some("MinB[0]".to_string()));

    assert!(has_min_a, "expected constraint labeled MinA[0]");
    assert!(has_min_b, "expected constraint labeled MinB[0]");
}

// ── Test 6: same def multiple instantiations ─────────────────────────────────

/// One def instantiated twice in the same structure produces 2× predicates.
#[test]
fn same_def_multiple_instantiations() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 1
}
structure S {
    param t1: Length
    param t2: Length
    constraint MinWall(wall: t1)
    constraint MinWall(wall: t2)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");

    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // 1 predicate × 2 instantiations = 2 constraints
    assert_eq!(
        tmpl.constraints.len(),
        2,
        "expected 2 constraints from 2 instantiations of single-predicate def, got {}",
        tmpl.constraints.len()
    );

    // Both labeled MinWall[0] (each instantiation produces predicate index 0)
    for (i, cc) in tmpl.constraints.iter().enumerate() {
        assert_eq!(
            cc.label,
            Some("MinWall[0]".to_string()),
            "constraint[{i}] expected label MinWall[0], got {:?}",
            cc.label
        );
    }

    // But each references a different param (t1 vs t2)
    let first_param = match &tmpl.constraints[0].expr.kind {
        CompiledExprKind::BinOp { left, .. } => match &left.kind {
            CompiledExprKind::ValueRef(id) => id.member.clone(),
            other => panic!("expected ValueRef, got {:?}", other),
        },
        other => panic!("expected BinOp, got {:?}", other),
    };
    let second_param = match &tmpl.constraints[1].expr.kind {
        CompiledExprKind::BinOp { left, .. } => match &left.kind {
            CompiledExprKind::ValueRef(id) => id.member.clone(),
            other => panic!("expected ValueRef, got {:?}", other),
        },
        other => panic!("expected BinOp, got {:?}", other),
    };

    assert_ne!(
        first_param, second_param,
        "two instantiations should reference different params, both got '{first_param}'"
    );
}

// ── Test 7: too many args produces error ─────────────────────────────────────

/// Providing 3 args for a 2-param def should produce an error mentioning the
/// unknown arg name.
#[test]
fn wrong_arg_count_too_many() {
    let source = r#"
constraint def TwoParam {
    param a: Length
    param b: Length
    a > b
}
structure S {
    param x: Length
    param y: Length
    param z: Length
    constraint TwoParam(a: x, b: y, c: z)
}
"#;
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown arg 'c'"
    );
    // Error should mention 'c' (the unknown arg name) or 'unknown'
    let found = errors
        .iter()
        .any(|d| d.message.contains('c') || d.message.to_lowercase().contains("unknown"));
    assert!(
        found,
        "expected error mentioning unknown arg 'c', got: {:?}",
        errors
    );
}

// ── Test 8: all args have wrong names (none matching required param) ──────────

/// Providing an arg that doesn't match any param name triggers both
/// "unknown argument" AND "missing argument" errors (one for the unknown
/// arg provided, one for the required param that was never bound).
/// The grammar requires ≥1 named arg, so zero args isn't valid syntax;
/// this test covers the equivalent scenario with misnamed args.
#[test]
fn wrong_arg_count_zero() {
    let source = r#"
constraint def RequiredOne {
    param x: Length
    x > 0
}
structure S {
    param v: Length
    constraint RequiredOne(y: v)
}
"#;
    // 'y' is unknown + 'x' is missing → should produce at least two errors
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        errors.len() >= 2,
        "expected >=2 errors (unknown 'y' + missing 'x'), got: {:?}",
        errors
    );
    let has_unknown_y = errors
        .iter()
        .any(|d| d.message.contains('y') || d.message.to_lowercase().contains("unknown"));
    let has_missing_x = errors
        .iter()
        .any(|d| d.message.contains('x') || d.message.to_lowercase().contains("missing"));
    assert!(
        has_unknown_y,
        "expected error about unknown arg 'y', got: {:?}",
        errors
    );
    assert!(
        has_missing_x,
        "expected error about missing arg 'x', got: {:?}",
        errors
    );
}

// ── Test 9: one of two required params missing ────────────────────────────────

/// Providing 1 of 2 required args should produce an error naming the missing param.
#[test]
fn missing_required_param_with_others_present() {
    let source = r#"
constraint def TwoRequired {
    param a: Length
    param b: Length
    a > b
}
structure S {
    param x: Length
    constraint TwoRequired(a: x)
}
"#;
    let module = compile_module(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for missing required param 'b'"
    );
    // Error should name the missing param 'b'
    let found = errors
        .iter()
        .any(|d| d.message.contains('b') || d.message.to_lowercase().contains("missing"));
    assert!(
        found,
        "expected error mentioning missing param 'b', got: {:?}",
        errors
    );
}

// ── Test 10: violation diagnostic label mechanism ─────────────────────────────

/// Compile a constraint def instantiation and verify the label field in the
/// CompiledConstraint contains the def name. This is the mechanism by which
/// the eval engine produces diagnostics with the def name when the constraint
/// is violated at runtime.
///
/// Full end-to-end violation diagnostics are tested in constraint_def_eval.rs:
/// `violated_constraint_def_produces_labeled_diagnostic`.
#[test]
fn violation_diagnostic_contains_def_name() {
    let source = r#"
constraint def MinWall {
    param wall: Length
    wall > 2
}
structure S {
    param t: Length
    constraint MinWall(wall: t)
}
"#;
    let (tmpl, diags) = compile_template(source, "S");
    let errors = error_diags(&diags);
    assert!(errors.is_empty(), "expected no compile errors, got: {:?}", errors);

    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected exactly 1 constraint"
    );

    // The label "MinWall[0]" is the def name + predicate index.
    // When eval detects a violation, it replaces the raw ConstraintNodeId
    // with this label in the diagnostic message, so the message contains "MinWall".
    let label = tmpl.constraints[0].label.as_deref().unwrap_or("");
    assert!(
        label.contains("MinWall"),
        "constraint label should contain def name 'MinWall', got: {:?}",
        tmpl.constraints[0].label
    );
    assert_eq!(
        label, "MinWall[0]",
        "expected label 'MinWall[0]', got: {:?}",
        tmpl.constraints[0].label
    );
}

// ── Test 11: cross-module constraint def import ───────────────────────────────

/// A constraint def defined in module `a` can be imported into module `b`
/// and instantiated in a structure. Compilation of `b` must succeed and
/// produce a labeled constraint from the imported def.
#[test]
fn cross_module_constraint_def_import() {
    use std::fs;

    // Create a unique temp directory for this test
    let dir = std::env::temp_dir()
        .join("reify_constraint_def_test")
        .join("cross_module")
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Module a: defines a pub constraint def
    fs::write(
        dir.join("a.ri"),
        "pub constraint def MinWall {\n    param w: Length\n    w > 0mm\n}\n",
    )
    .unwrap();

    // Module b: imports a, instantiates MinWall in a structure
    fs::write(
        dir.join("b.ri"),
        "import a\nstructure S {\n    param t: Length = 5mm\n    constraint MinWall(w: t)\n}\n",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("b", &resolver);

    let _ = fs::remove_dir_all(&dir);

    assert!(
        result.is_ok(),
        "expected compilation to succeed, got: {:?}",
        result.unwrap_err()
    );

    // Get the compiled module for b
    let compiled_b = dag.modules.get("b").expect("compiled module 'b' not found");

    // Should have no error diagnostics
    let errors: Vec<_> = compiled_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors in module b, got: {:?}",
        errors
    );

    // The template S should have 1 constraint labeled MinWall[0]
    let tmpl = compiled_b
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template 'S' in module b");

    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected 1 constraint in template S, got {}",
        tmpl.constraints.len()
    );
    assert_eq!(
        tmpl.constraints[0].label,
        Some("MinWall[0]".to_string()),
        "expected label MinWall[0], got {:?}",
        tmpl.constraints[0].label
    );
}

// ── Test 12: pub constraint def AST has is_pub = true ────────────────────────

/// Parsing a `pub constraint def` produces a ConstraintDef with `is_pub == true`.
#[test]
fn pub_constraint_def_parsed() {
    let source = "pub constraint def Positive {\n    param v: Length\n    v > 0mm\n}\n";
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let constraint_def = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_syntax::Declaration::Constraint(c) = d {
                Some(c)
            } else {
                None
            }
        })
        .expect("expected a ConstraintDef declaration");

    assert!(
        constraint_def.is_pub,
        "expected is_pub == true for 'pub constraint def', got false"
    );
    assert_eq!(constraint_def.name, "Positive");
    assert_eq!(
        constraint_def.params.len(),
        1,
        "expected 1 param, got {}",
        constraint_def.params.len()
    );
}
