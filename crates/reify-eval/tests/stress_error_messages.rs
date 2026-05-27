//! Stress tests verifying error message quality for every error class.
//!
//! Covers all error classes in the parse → compile → eval/check pipeline:
//!   - parse_error_malformed_syntax: malformed tokens → ParseError with non-empty message
//!   - compile_error_unresolved_name: unknown name → "unresolved name" diagnostic
//!   - compile_error_unknown_unit: bogus unit → "unknown unit" diagnostic
//!   - compile_error_wrong_geometry_arg_count: wrong arg count → "expects exactly N arguments"
//!   - compile_error_circular_type_alias: circular alias chain → "circular" diagnostic
//!   - compile_error_trait_not_found: missing trait → "not found" diagnostic
//!   - eval_error_geometry_kernel_failure: kernel failure → "all geometry operations failed"
//!   - eval_error_compile_geometry_op_failure: bad op ref → "failed to compile geometry operation"
//!   - constraint_violation_diagnostic: x=5mm, x>10mm → Satisfaction::Violated
//!   - all_error_classes_produce_nonempty_messages: meta-test, every diagnostic has non-empty message

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::*;
use reify_core::{ModulePath, Severity, Type};
use reify_ir::{ExportFormat, Satisfaction};

// ---------------------------------------------------------------------------
// step-25/26: parse_error_malformed_syntax
// ---------------------------------------------------------------------------

/// Verify that malformed .ri source produces at least one error (parse or compile)
/// with a non-empty, descriptive message.
///
/// Tree-sitter is error-tolerant, so `@@@` inside a structure body produces
/// an ERROR node which the lowering code reports as a ParseError.
/// Either parse errors or compile errors must be non-empty (defensive).
#[test]
fn parse_error_malformed_syntax() {
    // `@@@` inside the structure body is invalid; tree-sitter produces an ERROR node,
    // which the ts_parser lowering converts to a ParseError with "syntax error: @@@".
    let source = "structure S { @@@ }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_malformed"));
    let compiled = reify_compiler::compile(&parsed);

    // Defensive: at least one of parse errors or compile errors must be non-empty.
    // (Tree-sitter might recover silently for some inputs, in which case the
    // compiler will catch the error instead.)
    let has_any_error = !parsed.errors.is_empty()
        || compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
    assert!(
        has_any_error,
        "malformed syntax '{}' should produce at least one parse or compile error",
        source
    );

    // All parse error messages must be non-empty and descriptive.
    for e in &parsed.errors {
        assert!(
            !e.message.is_empty(),
            "parse error message must be non-empty, got empty message for source: {:?}",
            source
        );
    }

    // All compile error diagnostic messages must be non-empty.
    for d in compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
    {
        assert!(
            !d.message.is_empty(),
            "compile diagnostic message must be non-empty, got empty for source: {:?}",
            source
        );
    }
}

// ---------------------------------------------------------------------------
// step-27/28: compile_error_unresolved_name
// ---------------------------------------------------------------------------

/// Verify that referencing an undefined name produces a "unresolved name" diagnostic.
#[test]
fn compile_error_unresolved_name() {
    let source = r#"structure S {
    let x = unknown_name
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_unresolved"));
    let compiled = reify_compiler::compile(&parsed);

    let error_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one compile error for unresolved name 'unknown_name', got none"
    );

    let has_unresolved_msg = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved name"));
    assert!(
        has_unresolved_msg,
        "expected diagnostic containing 'unresolved name', got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-29/30: compile_error_unknown_unit
// ---------------------------------------------------------------------------

/// Verify that a quantity literal with a bogus unit produces an "unknown unit" diagnostic.
#[test]
fn compile_error_unknown_unit() {
    let source = r#"structure S {
    param x : Length = 5quux
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_bad_unit"));
    let compiled = reify_compiler::compile(&parsed);

    let has_unknown_unit = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unknown unit"));
    assert!(
        has_unknown_unit,
        "expected diagnostic containing 'unknown unit' for bogus unit 'quux', got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-31/32: compile_error_wrong_geometry_arg_count
// ---------------------------------------------------------------------------

/// Verify that calling extrude() with the wrong number of arguments produces
/// a diagnostic containing "expects exactly 2 arguments".
#[test]
fn compile_error_wrong_geometry_arg_count() {
    // extrude() requires exactly 2 args: (profile, distance). Passing 1 arg is wrong.
    let source = r#"structure S {
    param p : Length = 5mm
    let r = extrude(p)
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_wrong_args"));
    let compiled = reify_compiler::compile(&parsed);

    let has_arg_msg = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("expects exactly 2 arguments"));
    assert!(
        has_arg_msg,
        "expected diagnostic 'expects exactly 2 arguments' for extrude(p) with 1 arg, got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-33/34: compile_error_circular_type_alias
// ---------------------------------------------------------------------------

/// Verify that a circular type alias (A = B, B = A) produces a "circular" diagnostic.
#[test]
fn compile_error_circular_type_alias() {
    // Mutual circular type alias: A → B → A.
    let source = "type A = B\ntype B = A";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_circular_alias"));
    let compiled = reify_compiler::compile(&parsed);

    let has_circular = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("circular"));
    assert!(
        has_circular,
        "expected diagnostic containing 'circular' for type A = B; type B = A, got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-35/36: compile_error_trait_not_found
// ---------------------------------------------------------------------------

/// Verify that refining from a non-existent trait produces a "not found" diagnostic.
#[test]
fn compile_error_trait_not_found() {
    let source = "structure S : NonExistentTrait { }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test_trait_missing"));
    let compiled = reify_compiler::compile(&parsed);

    // The compiler produces "unresolved trait: 'NonExistentTrait'" — check for "unresolved trait".
    let has_trait_err = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved trait") || d.message.contains("not found"));
    assert!(
        has_trait_err,
        "expected diagnostic about unknown trait 'NonExistentTrait' \
         (containing 'unresolved trait' or 'not found'), got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-37/38: eval_error_geometry_kernel_failure
// ---------------------------------------------------------------------------

/// Verify that a geometry kernel execute() failure causes:
///   - geometry_output = None
///   - at least one diagnostic containing "all geometry operations failed"
#[test]
fn eval_error_geometry_kernel_failure() {
    let e = "TestKernelFail";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_kernel_fail"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output=None when kernel always fails, got Some({} bytes)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    let has_failure_msg = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_failure_msg,
        "expected diagnostic 'all geometry operations failed' when kernel fails, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-39/40: eval_error_compile_geometry_op_failure
// ---------------------------------------------------------------------------

/// Verify that a geometry op that fails to compile (compile_geometry_op returns None)
/// produces a "failed to compile geometry operation" diagnostic.
///
/// A Boolean op referencing Step(0) and Step(1) but with no preceding primitive ops
/// causes compile_geometry_op to return None (step_handles is empty, so the
/// GeomRef::Step indices cannot be resolved).
#[test]
fn eval_error_compile_geometry_op_failure() {
    let e = "TestCompileOpFail";

    // Boolean union op referencing Step(0) and Step(1) — but no preceding ops,
    // so step_handles is empty when compile_geometry_op tries to resolve them.
    let bad_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![bad_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_bad_op"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let has_compile_fail = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("failed to compile geometry operation"));
    assert!(
        has_compile_fail,
        "expected 'failed to compile geometry operation' for Boolean op with invalid Step refs, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-41/42: constraint_violation_diagnostic
// ---------------------------------------------------------------------------

/// Verify that a violated constraint produces Satisfaction::Violated with an
/// identifiable constraint ID, so the user can locate the problem.
///
/// structure S { param x : Length = 5mm; constraint x > 10mm }
/// 5mm > 10mm is false → Violated.
#[test]
fn constraint_violation_diagnostic() {
    let source = r#"structure S {
    param x : Length = 5mm
    constraint x > 10mm
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_violation"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    // The source is syntactically valid — only the constraint is violated at runtime.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    let s_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "S")
        .collect();
    assert!(
        !s_constraints.is_empty(),
        "expected at least one constraint result for structure 'S'"
    );

    let has_violated = s_constraints
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        has_violated,
        "expected Satisfaction::Violated for x=5mm with constraint x>10mm, got: {:?}",
        s_constraints
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-43/44: all_error_classes_produce_nonempty_messages — meta-test
// ---------------------------------------------------------------------------

/// Meta-test: verify that every error-producing snippet results in at least one
/// parse or compile error with a non-empty message.
///
/// This is a safety net ensuring no error class silently produces blank messages.
#[test]
fn all_error_classes_produce_nonempty_messages() {
    let snippets: &[(&str, &str)] = &[
        ("malformed_syntax", "structure S { @@@ }"),
        (
            "unresolved_name",
            "structure S {\n    let x = unknown_name\n}",
        ),
        (
            "unknown_unit",
            "structure S {\n    param x : Length = 5quux\n}",
        ),
        (
            "wrong_arg_count",
            "structure S {\n    param p : Length = 5mm\n    let r = extrude(p)\n}",
        ),
        ("circular_alias", "type A = B\ntype B = A"),
        ("trait_not_found", "structure S : NonExistentTrait { }"),
    ];

    for (label, source) in snippets {
        let parsed = reify_syntax::parse(source, ModulePath::single("test_meta"));
        let compiled = reify_compiler::compile(&parsed);

        // Every snippet must produce at least one error (parse or compile).
        let has_any_error = !parsed.errors.is_empty()
            || compiled
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error);
        assert!(
            has_any_error,
            "[{}] expected at least one error for snippet {:?}, got none",
            label, source
        );

        // All parse error messages must be non-empty.
        for e in &parsed.errors {
            assert!(
                !e.message.is_empty(),
                "[{}] parse error message is empty (should describe the problem)",
                label
            );
        }

        // All compile error diagnostic messages must be non-empty.
        for d in compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
        {
            assert!(
                !d.message.is_empty(),
                "[{}] compile diagnostic message is empty (should describe the problem)",
                label
            );
        }
    }
}
