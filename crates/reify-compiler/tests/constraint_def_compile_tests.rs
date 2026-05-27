//! Compiler-level tests for constraint def features (task 199).
//!
//! Tests edge cases, error paths, and cross-module import for constraint defs.
//! Complements the existing constraint_inst_tests.rs from task 198.

use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
use reify_compiler::{CompiledConstraintDef, CompiledConstraintParam};
use reify_test_support::{compile_source, compile_template};
use reify_core::*;
use reify_ir::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect only error diagnostics.
fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Create a temporary project directory with `stdlib/` pre-created.
///
/// Returns `(TempDir, PathBuf)` — keep the `TempDir` alive for the test's
/// duration; the `PathBuf` is a copy of `tmp.path()` for ergonomic use.
/// The `stdlib/` subdirectory exists so that `ModuleResolver::new(&dir,
/// dir.join("stdlib"))` is robust if the resolver ever becomes strict about
/// `stdlib_root` existence.
fn fresh_project_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(dir.join("stdlib")).unwrap();
    (tmp, dir)
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
        Some("MinWall#0[0]".to_string()),
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
    for (i, expected_label) in ["Triple#0[0]", "Triple#0[1]", "Triple#0[2]"]
        .iter()
        .enumerate()
    {
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
                    assert_eq!(
                        id.member, "d",
                        "left should be ValueRef(d), got {}",
                        id.member
                    );
                }
                other => panic!("left should be ValueRef, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }

    // Second constraint: x <= hi → d <= 100mm
    // Left operand should be ValueRef(S.d), op should be Le
    match &tmpl.constraints[1].expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(*op, BinOp::Le, "second constraint should be Le (<=)");
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.member, "d",
                        "left of second constraint should be ValueRef(d), got {}",
                        id.member
                    );
                }
                other => panic!(
                    "left of second constraint should be ValueRef, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected BinOp for second constraint, got {:?}", other),
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
        .any(|c| c.label == Some("MinA#0[0]".to_string()));
    let has_min_b = tmpl
        .constraints
        .iter()
        .any(|c| c.label == Some("MinB#0[0]".to_string()));

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

    // Each instantiation gets a unique inst_idx in the label so labels don't
    // collide across instantiations of the same def (task 845).
    assert_eq!(
        tmpl.constraints[0].label,
        Some("MinWall#0[0]".to_string()),
        "constraint[0] expected label MinWall#0[0], got {:?}",
        tmpl.constraints[0].label
    );
    assert_eq!(
        tmpl.constraints[1].label,
        Some("MinWall#1[0]".to_string()),
        "constraint[1] expected label MinWall#1[0], got {:?}",
        tmpl.constraints[1].label
    );

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
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown arg 'c'"
    );
    // Error should mention 'c' (the unknown arg name) or 'unknown' — use phrase-level check.
    let found = errors
        .iter()
        .any(|d| d.message.contains("'c'") || d.message.to_lowercase().contains("unknown"));
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
/// (Renamed from `wrong_arg_count_zero`: the grammar forbids zero args,
/// so this is really the misnamed-single-arg case.)
#[test]
fn misnamed_single_arg_produces_two_errors() {
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
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        errors.len() >= 2,
        "expected >=2 errors (unknown 'y' + missing 'x'), got: {:?}",
        errors
    );
    // Use phrase-level assertions to avoid single-char false positives.
    let has_unknown_y = errors
        .iter()
        .any(|d| d.message.contains("'y'") || d.message.to_lowercase().contains("unknown"));
    let has_missing_x = errors
        .iter()
        .any(|d| d.message.contains("'x'") || d.message.to_lowercase().contains("missing"));
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
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected at least one error for missing required param 'b'"
    );
    // Error should name the missing param 'b' — use phrase-level check to avoid false positives.
    let found = errors
        .iter()
        .any(|d| d.message.contains("'b'") || d.message.to_lowercase().contains("missing"));
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
    assert!(
        errors.is_empty(),
        "expected no compile errors, got: {:?}",
        errors
    );

    assert_eq!(tmpl.constraints.len(), 1, "expected exactly 1 constraint");

    // The label "MinWall#0[0]" is the def name + predicate index.
    // When eval detects a violation, it replaces the raw ConstraintNodeId
    // with this label in the diagnostic message, so the message contains "MinWall".
    let label = tmpl.constraints[0].label.as_deref().unwrap_or("");
    assert!(
        label.contains("MinWall"),
        "constraint label should contain def name 'MinWall', got: {:?}",
        tmpl.constraints[0].label
    );
    assert_eq!(
        label, "MinWall#0[0]",
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

    let (_tmp, dir) = fresh_project_dir();

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
        Some("MinWall#0[0]".to_string()),
        "expected label MinWall[0], got {:?}",
        tmpl.constraints[0].label
    );
}

// ── Test 12: pub constraint def AST has is_pub = true ────────────────────────

/// Parsing a `pub constraint def` produces a ConstraintDef with `is_pub == true`.
#[test]
fn pub_constraint_def_parsed() {
    let source = "pub constraint def Positive {\n    param v: Length\n    v > 0mm\n}\n";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let constraint_def = parsed
        .declarations
        .iter()
        .find_map(|d| {
            if let reify_ast::Declaration::Constraint(c) = d {
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

// ── Test 13: type mismatch — Bool where Length expected (ignored pending task 875) ───

/// Documents the expected behavior once param-level type checking is implemented:
/// passing a Bool literal where a Length param is expected must produce at least one
/// error-level diagnostic mentioning "type" or "mismatch".
///
/// Currently ignored because the compiler defers type checking to compile_expr and
/// does not yet detect Bool-for-Length mismatches at the constraint instantiation
/// level. Remove the `#[ignore]` attribute once param-level type checking (task 875)
/// is implemented — the test will then pass without modification.
#[test]
#[ignore = "type-check gap: Bool passed where Length expected is not yet rejected (see task 875)"]
fn type_mismatch_bool_for_length() {
    let source = r#"
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    constraint MinWall(w: true)
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(
        !errors.is_empty(),
        "expected an error-level diagnostic for Bool-for-Length type mismatch, \
         got no diagnostics at all"
    );
    let has_type_or_mismatch = errors.iter().any(|d| {
        d.message.to_lowercase().contains("type") || d.message.to_lowercase().contains("mismatch")
    });
    assert!(
        has_type_or_mismatch,
        "expected error mentioning 'type' or 'mismatch' for Bool-for-Length mismatch, \
         got: {:?}",
        errors
    );
}

// ── Test 14: constraint_defs field reflects all local definitions (pub + non-pub) ──

/// Verify that `CompiledModule.constraint_defs` contains ALL local constraint defs —
/// both pub and non-pub — with correct names, `is_pub` flags, param names, and
/// predicate counts.
///
/// Covers task 878 subtest "field contents" (name/params/predicates surfaced)
/// and "non-pub visibility (local)" (non-pub defs remain reachable inside the
/// owning module's compiled output).
#[test]
fn module_constraint_defs_field_contents_reflect_definitions() {
    let source = r#"
pub constraint def PubDef {
    param t: Length
    t > 0mm
}
constraint def PrivDef {
    param x: Length
    param y: Length
    x > y
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // Both defs (pub and non-pub) should be present in constraint_defs.
    assert_eq!(
        module.constraint_defs.len(),
        2,
        "expected 2 constraint defs (1 pub + 1 non-pub), got {}",
        module.constraint_defs.len()
    );

    // --- PubDef assertions (is_pub = true, 1 param 't', 1 predicate) ---
    let pub_def: &CompiledConstraintDef = module
        .constraint_defs
        .iter()
        .find(|d| d.name == "PubDef")
        .expect("expected 'PubDef' in module.constraint_defs");

    assert!(pub_def.is_pub, "PubDef should have is_pub = true");

    assert_eq!(
        pub_def.params.len(),
        1,
        "PubDef should have 1 param, got {}",
        pub_def.params.len()
    );

    // Verify CompiledConstraintParam is the concrete type in the params Vec.
    let pub_param: &CompiledConstraintParam = &pub_def.params[0];
    assert_eq!(
        pub_param.name, "t",
        "PubDef param[0] should be named 't', got '{}'",
        pub_param.name
    );

    assert_eq!(
        pub_def.predicates.len(),
        1,
        "PubDef should have 1 predicate, got {}",
        pub_def.predicates.len()
    );

    // --- PrivDef assertions (is_pub = false, 2 params, 1 predicate) ---
    let priv_def: &CompiledConstraintDef = module
        .constraint_defs
        .iter()
        .find(|d| d.name == "PrivDef")
        .expect("expected 'PrivDef' in module.constraint_defs");

    assert!(!priv_def.is_pub, "PrivDef should have is_pub = false");

    assert_eq!(
        priv_def.params.len(),
        2,
        "PrivDef should have 2 params, got {}",
        priv_def.params.len()
    );

    assert_eq!(
        priv_def.predicates.len(),
        1,
        "PrivDef should have 1 predicate, got {}",
        priv_def.predicates.len()
    );
}

// ── Test 15: name collision from two prelude modules emits shadow warning ────────

/// Two imported modules each declare `pub constraint def MinThickness`; compiling
/// a main module that imports both must emit a Warning-severity diagnostic mentioning
/// "MinThickness" and both module paths.  The warning must be *directional*: it names
/// the first-imported module (the winner, 'a') before the later-imported module (the
/// loser, 'b'), and includes the phrase "first-imported" or "wins".  A structural
/// check distinguishes the two modules: module 'a' has 1 predicate; module 'b' has 2.
/// Exactly 1 compiled constraint in S proves module 'a' (first-import) won the registry.
///
/// Covers task 880 #1 (prelude shadow warning) and reviewer blocker on direction.
#[test]
fn cross_module_constraint_def_name_collision_emits_shadow_warning() {
    use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
    use std::fs;

    let (_tmp, dir) = fresh_project_dir();

    // Module a: defines pub MinThickness — ONE predicate (t > 0mm).
    fs::write(
        dir.join("a.ri"),
        "pub constraint def MinThickness {\n    param t: Length\n    t > 0mm\n}\n",
    )
    .unwrap();

    // Module b: also defines pub MinThickness — TWO predicates so we can tell which
    // module's def won by counting compiled constraints (a wins → 1, b wins → 2).
    fs::write(
        dir.join("b.ri"),
        "pub constraint def MinThickness {\n    param t: Length\n    t > 1mm\n    t < 1000mm\n}\n",
    )
    .unwrap();

    // Main module: imports both a and b — should trigger shadow warning
    fs::write(
        dir.join("main.ri"),
        "import a\nimport b\nstructure S {\n    param t: Length = 5mm\n    constraint MinThickness(t: t)\n}\n",
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("main", &resolver);

    assert!(
        result.is_ok(),
        "expected compilation to succeed (shadow warning, not error), got: {:?}",
        result.unwrap_err()
    );

    let compiled_main = dag
        .modules
        .get("main")
        .expect("compiled module 'main' not found");

    // No Error-level diagnostics: collision is a warning, not a hard failure.
    let errors: Vec<_> = compiled_main
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors (shadow collision is a warning), got errors: {:?}",
        errors
    );

    // The shadow warning must name both module paths AND be directional: the winner
    // ('a', first-imported) must appear before the loser ('b') in the message.
    let shadow_warnings: Vec<_> = compiled_main
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("MinThickness")
                && d.message.contains("from 'a'")
                && d.message.contains("from 'b'")
        })
        .collect();
    assert_eq!(
        shadow_warnings.len(),
        1,
        "expected exactly one shadow warning mentioning MinThickness and both module paths \
         (with 'from' prefix), got: {:?}",
        compiled_main.diagnostics
    );

    // Directional check: winner ('a', first-imported) must be named BEFORE loser ('b').
    let msg = &shadow_warnings[0].message;
    let pos_a = msg
        .find("from 'a'")
        .expect("shadow warning should contain \"from 'a'\"");
    let pos_b = msg
        .find("from 'b'")
        .expect("shadow warning should contain \"from 'b'\"");
    assert!(
        pos_a < pos_b,
        "expected winner ('a') to appear before loser ('b') in shadow warning, got: {:?}",
        msg
    );

    // Semantic-clarity check: message must include "first-imported" or "wins" so users
    // understand which definition is retained without having to guess.
    assert!(
        msg.contains("first-imported") || msg.contains("wins"),
        "shadow warning should contain 'first-imported' or 'wins' to clarify semantics, \
         got: {:?}",
        msg
    );

    // Structural first-import-wins check: 'a' has 1 predicate, 'b' has 2.
    // constraints.len() == 1 proves module 'a' won; 2 would mean 'b' incorrectly won.
    assert_eq!(
        compiled_main.templates.len(),
        1,
        "expected one structure template (S), got: {:?}",
        compiled_main.templates
    );
    let s_template = &compiled_main.templates[0];
    assert_eq!(
        s_template.constraints.len(),
        1,
        "expected 1 compiled constraint in S (module 'a' wins with 1 predicate; \
         2 would indicate module 'b' incorrectly won), got: {:?}",
        s_template.constraints
    );
    let c = &s_template.constraints[0];
    assert!(
        c.label
            .as_deref()
            .is_some_and(|l| l.starts_with("MinThickness")),
        "expected constraint label starting with 'MinThickness', got: {:?}",
        c.label
    );
}

// ── Test 16: non-pub constraint def invisible across module boundary ──────────────

/// Module A defines a non-pub constraint def; module B imports A and attempts to
/// instantiate the non-pub def — must produce an "unknown" diagnostic (the def is
/// not exported). Module A's own structure that uses the def must compile cleanly.
///
/// Covers task 878 subtest "non-pub cross-module invisibility".
#[test]
fn non_pub_constraint_def_not_instantiable_cross_module() {
    use reify_compiler::module_dag::{ModuleDag, ModuleResolver};
    use std::fs;

    let (_tmp, dir) = fresh_project_dir();

    // Module a: non-pub MinThickness (no `pub`), used internally in Wall
    fs::write(
        dir.join("a.ri"),
        concat!(
            "constraint def MinThickness {\n",
            "    param t: Length\n",
            "    t > 0mm\n",
            "}\n",
            "pub structure Wall {\n",
            "    param thickness: Length = 5mm\n",
            "    constraint MinThickness(t: thickness)\n",
            "}\n",
        ),
    )
    .unwrap();

    // Module b: imports a, tries to use the non-pub MinThickness in its own structure
    fs::write(
        dir.join("b.ri"),
        concat!(
            "import a\n",
            "structure Panel {\n",
            "    param t: Length = 3mm\n",
            "    constraint MinThickness(t: t)\n",
            "}\n",
        ),
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let _result = dag.compile_module("b", &resolver);

    // Module a should have no errors (internal use of non-pub def is valid)
    let compiled_a = dag.modules.get("a").expect("compiled module 'a' not found");
    let a_errors: Vec<_> = compiled_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        a_errors.is_empty(),
        "module a should compile cleanly (non-pub def used internally), got errors: {:?}",
        a_errors
    );

    // Module b should have an error: MinThickness is not visible across the boundary
    let compiled_b = dag.modules.get("b").expect("compiled module 'b' not found");
    let b_errors: Vec<_> = compiled_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !b_errors.is_empty(),
        "expected an error in module b: non-pub MinThickness should not be visible, \
         got no errors (diagnostics: {:?})",
        compiled_b.diagnostics
    );
    // The error must specifically say "unknown constraint definition" — not merely
    // mention "unknown" or the def name in an unrelated error. This proves the
    // resolution path (not an incidental type error or name mention) was triggered.
    let has_unknown_msg = b_errors
        .iter()
        .any(|d| d.message.contains("unknown constraint definition"));
    assert!(
        has_unknown_msg,
        "expected error containing 'unknown constraint definition', got: {:?}",
        b_errors
    );
}

// ── Test 17: generic constraint def with type-param type param compiles cleanly ──

/// A constraint def that declares a type parameter `T` and uses `T` as the type
/// of one of its params must compile without any "unknown type" error.
///
/// This characterizes the behaviour preserved by the step-2 cleanup that removes
/// the redundant `!type_param_names.contains(name)` guard from
/// `compile_constraint_def` — the `resolve_type_expr_with_aliases` call already
/// returns `Some` for any name in `type_param_names`, making the guard dead code.
/// Adding the test first ensures the cleanup cannot silently regress this contract.
#[test]
fn generic_constraint_def_with_type_param_type_compiles_cleanly() {
    let source = r#"
constraint def Aligned<T> {
    param t: T
    param w: Length
    w > 0
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);

    // No error must mention "unknown type" for the declared type parameter T.
    let unknown_type_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.message.starts_with("unknown type '"))
        .collect();
    assert!(
        unknown_type_errors.is_empty(),
        "expected no 'unknown type' errors for declared type param T, got: {:?}",
        unknown_type_errors
    );

    // Positive shape assertion: Aligned must actually be present in compiled output
    // with the correct param and type_param counts, ruling out the silent-drop hole.
    let def: &CompiledConstraintDef = module
        .constraint_defs
        .iter()
        .find(|d| d.name == "Aligned")
        .expect("Aligned constraint def must be present in module.constraint_defs");
    assert_eq!(
        def.params.len(),
        2,
        "expected Aligned to have 2 params (t, w), got {}",
        def.params.len()
    );
    assert_eq!(
        def.type_params.len(),
        1,
        "expected Aligned to have 1 type param (T), got {}",
        def.type_params.len()
    );
}

// ── Test 18: constraint def with local structure param type compiles cleanly ──

/// A constraint def whose param type is a locally-defined structure name must
/// compile without any "unknown type" error diagnostic.
///
/// Regression for the case where `compile_constraint_def` rejected structure
/// names (only builtins, type params, aliases, enums, and traits were accepted).
/// Because the resolved type is discarded at def-compile time (entity.rs only
/// reads param.name and param.default at instantiation time), accepting a
/// structure-typed param is semantically safe — no downstream type-checking
/// changes are needed.
///
/// This test MUST fail on the unpatched compiler (Wall is not in the accepted
/// categories) and pass after step-2's `structure_names` guard is added.
#[test]
fn constraint_def_with_local_structure_param_type_compiles_cleanly() {
    let source = r#"
structure Wall {
    param thickness: Length = 5mm
}
constraint def FitsWall {
    param w: Wall
    w.thickness > 0mm
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);

    // No "unknown type" error must be emitted for the structure param type.
    let unknown_type_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.starts_with("unknown type '")
                && d.message.contains("Wall")
                && d.message.contains("'w'")
        })
        .collect();
    assert!(
        unknown_type_errors.is_empty(),
        "expected no 'unknown type' error for locally-defined structure 'Wall' \
         as param type in constraint def, got: {:?}",
        unknown_type_errors
    );

    // Positive shape assertion: FitsWall must be present in compiled output.
    let def: &CompiledConstraintDef = module
        .constraint_defs
        .iter()
        .find(|d| d.name == "FitsWall")
        .expect("FitsWall constraint def must be present in module.constraint_defs");
    assert_eq!(
        def.params.len(),
        1,
        "expected FitsWall to have 1 param (w), got {}",
        def.params.len()
    );
    assert_eq!(
        def.params[0].name, "w",
        "expected FitsWall param[0] to be named 'w', got '{}'",
        def.params[0].name
    );
}

// ── Test 19: constraint def with prelude structure param type compiles cleanly ─

/// A constraint def whose param type is a structure exported from an imported
/// module must compile without any "unknown type" error diagnostic.
///
/// Cross-module regression: the structure_names set must include template names
/// from the prelude (imported modules), not just locally-defined structures.
///
/// This test is meant to fail before step-4 wires up the prelude-template chain
/// in `phase_constraint_defs`. Since step-2 already chains prelude templates
/// into structure_names, it should pass at step-3 commit time — acting as a
/// belt-and-suspenders confirmation that the prelude path is covered.
#[test]
fn constraint_def_with_prelude_structure_param_type_compiles_cleanly() {
    use std::fs;

    let (_tmp, dir) = fresh_project_dir();

    // Module a: exports a pub structure Wall
    fs::write(
        dir.join("a.ri"),
        "pub structure Wall {\n    param t: Length = 5mm\n}\n",
    )
    .unwrap();

    // Module b: imports a, defines a constraint def with Wall as a param type
    fs::write(
        dir.join("b.ri"),
        concat!(
            "import a\n",
            "constraint def FitsWall {\n",
            "    param w: Wall\n",
            "    w.t > 0mm\n",
            "}\n",
        ),
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("b", &resolver);

    assert!(
        result.is_ok(),
        "expected compilation to succeed, got: {:?}",
        result.unwrap_err()
    );

    let compiled_b = dag.modules.get("b").expect("compiled module 'b' not found");

    // No "unknown type" error must be emitted for the imported structure param type.
    let unknown_type_errors: Vec<_> = compiled_b
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.starts_with("unknown type '")
                && d.message.contains("Wall")
        })
        .collect();
    assert!(
        unknown_type_errors.is_empty(),
        "expected no 'unknown type' error for imported structure 'Wall' \
         as param type in constraint def, got: {:?}",
        unknown_type_errors
    );

    // Positive shape assertion: FitsWall must be present in module b's compiled output.
    let def = compiled_b
        .constraint_defs
        .iter()
        .find(|d| d.name == "FitsWall")
        .expect("FitsWall constraint def must be present in compiled module b");
    assert_eq!(
        def.params.len(),
        1,
        "expected FitsWall to have 1 param (w), got {}",
        def.params.len()
    );
}

// ── Test 19b: non-pub prelude structure param type compiles cleanly ───────────

/// Companion to test 19 (`constraint_def_with_prelude_structure_param_type_compiles_cleanly`).
///
/// Verifies that a non-`pub` structure exported by a prelude module is still
/// accepted as a constraint-def param type.  `structure_names` is built from
/// `prelude[i].templates` without filtering by visibility, so the non-pub
/// structure from module `a` is included when compiling module `b`.
///
/// This test documents current behavior explicitly: the "in scope" predicate
/// for constraint-def param types is visibility-agnostic at the prelude level.
/// If that policy changes in the future, this test should be updated to match.
#[test]
fn constraint_def_with_nonpub_prelude_structure_param_type_compiles_cleanly() {
    use std::fs;

    let (_tmp, dir) = fresh_project_dir();

    // Module a: non-pub structure Wall (no `pub` keyword).
    fs::write(
        dir.join("a.ri"),
        "structure Wall {\n    param t: Length = 5mm\n}\n",
    )
    .unwrap();

    // Module b: imports a, defines a constraint def with Wall as a param type.
    fs::write(
        dir.join("b.ri"),
        concat!(
            "import a\n",
            "constraint def FitsWall {\n",
            "    param w: Wall\n",
            "    w.t > 0mm\n",
            "}\n",
        ),
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, dir.join("stdlib"));
    let mut dag = ModuleDag::new();
    let result = dag.compile_module("b", &resolver);

    assert!(
        result.is_ok(),
        "expected compilation to succeed, got: {:?}",
        result.unwrap_err()
    );

    let compiled_b = dag.modules.get("b").expect("compiled module 'b' not found");

    // A non-pub structure in the prelude is still in scope for constraint-def
    // param types — structure_names is built from m.templates without a
    // visibility filter, so no "unknown type" error should fire.
    let unknown_type_errors: Vec<_> = compiled_b
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.starts_with("unknown type '")
                && d.message.contains("Wall")
        })
        .collect();
    assert!(
        unknown_type_errors.is_empty(),
        "expected no 'unknown type' error for non-pub prelude structure 'Wall' \
         as param type in constraint def, got: {:?}",
        unknown_type_errors
    );

    // Positive shape assertion: FitsWall must be present in module b's compiled output.
    let def = compiled_b
        .constraint_defs
        .iter()
        .find(|d| d.name == "FitsWall")
        .expect("FitsWall constraint def must be present in compiled module b");
    assert_eq!(
        def.params.len(),
        1,
        "expected FitsWall to have 1 param (w), got {}",
        def.params.len()
    );
}

// ── Test 20: unknown type error lists acceptable categories ──────────────────

/// A genuinely unknown param type must still produce an error, AND the error
/// message must list acceptable categories (builtin, type parameter, alias,
/// enum, trait, structure) so the user knows what IS accepted.
///
/// This test asserts:
///   (i)  an Error-severity diagnostic is emitted whose message starts with
///        "unknown type '" and contains the bogus name.
///   (ii) the message mentions at least two of the accepted categories by
///        lowercase substring — uses an "at least two" check to avoid
///        over-pinning specific wording.
///
/// Fails on the unpatched compiler because the current message is
/// `"unknown type '{}' in param '{}' of constraint def '{}'"` with no
/// category listing.
///
/// Pins both 'structure' and 'occurrence' because `structure_names` accepts
/// entities of either kind, and the diagnostic must name both so users know
/// what is actually valid.
#[test]
fn constraint_def_unknown_type_error_lists_acceptable_categories() {
    let source = r#"
constraint def Foo {
    param x: GenuinelyUnknownTypeName
    x > 0
}
"#;
    let module = compile_source(source);
    let errors = error_diags(&module.diagnostics);

    // (i) The error must still be emitted and name the bogus type.
    let unknown_type_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.starts_with("unknown type '")
                && d.message.contains("GenuinelyUnknownTypeName")
        })
        .collect();
    assert_eq!(
        unknown_type_errors.len(),
        1,
        "expected exactly one 'unknown type' error for 'GenuinelyUnknownTypeName', got: {:?}",
        errors
    );

    // (ii) The message must explicitly mention 'structure' — the new category
    // that this patch adds to the whitelist.  Pinning this single keyword is
    // more faithful to the change than a loose "any 2 of 6" threshold: it
    // guards against a regression that keeps other category words but drops
    // the one this change introduced.
    let msg = unknown_type_errors[0].message.to_lowercase();
    assert!(
        msg.contains("structure"),
        "expected error message to mention 'structure' as an accepted category \
         (the new category added by this patch), got: {:?}",
        unknown_type_errors[0].message
    );

    // (iii) The message must also mention 'occurrence' — structure_names is
    // built from entities whose kind is either "structure" OR "occurrence",
    // so the diagnostic must name both to correctly describe what is accepted.
    assert!(
        msg.contains("occurrence"),
        "expected error message to mention 'occurrence' as an accepted category \
         (structure_names accepts both structure AND occurrence names), got: {:?}",
        unknown_type_errors[0].message
    );
}

// ── Test 21 ──────────────────────────────────────────────────────────────────

/// Verifies that a module containing structure and occurrence declarations but
/// zero `constraint def` declarations compiles without errors and produces an
/// empty `constraint_defs` output.
#[test]
fn module_with_no_constraint_defs_compiles_cleanly() {
    let source = r#"
structure MyStruct {
    param x: Real
}

occurrence def MyOccurrence {
    param y: Real
}
"#;
    let module = compile_source(source);

    // (i) No error diagnostics — the compiler must not crash or emit errors
    //     when there are zero constraint defs.
    assert!(
        error_diags(&module.diagnostics).is_empty(),
        "expected no errors for a module with no constraint defs, got: {:?}",
        module.diagnostics
    );

    // (ii) constraint_defs must be empty — nothing to compile.
    assert!(
        module.constraint_defs.is_empty(),
        "expected empty constraint_defs for a module with no constraint def \
         declarations, got: {:?}",
        module.constraint_defs
    );
}
