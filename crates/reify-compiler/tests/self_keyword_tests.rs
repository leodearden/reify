//! Tests for `self` keyword resolution in structures.
//!
//! The `self` keyword refers to the enclosing entity definition or specialization
//! (spec section 8.6). `self.param_name` is equivalent to `param_name` for
//! locally-declared names; bare `self` is required when the entity itself is
//! the referent. `self` is invalid outside entity scope (fn bodies, module scope).
//!
//! ## Step numbering note
//!
//! Steps 1–8 and step-11 live in this file (compiler-path coverage).
//! Steps 9, 10, and 12 live in `crates/reify-eval/tests/self_keyword_eval.rs`
//! (eval-path coverage: `self_param_eval_produces_correct_value`,
//! `self_in_let_arithmetic_eval`, `self_in_constraint_eval_satisfied`).
//! There is no gap between step-8 and step-11 in this file — the apparent gap
//! is a cross-file artifact of the original task-153 plan.

use reify_types::{Severity, ValueCellId};

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_self"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );
    compiled
}

/// Helper: parse + compile source, return compiled output (may have errors).
#[allow(dead_code)]
fn compile_with_diagnostics(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_self"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ─── step-1: self.param resolves to correct ValueRef ───

#[test]
fn self_dot_param_resolves_to_value_ref() {
    // `self.x` inside a structure should resolve to the same value cell as `x`.
    let source = r#"structure S {
    param x : Scalar = 5mm
    let y = self.x
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];

    // `y` should be a value cell
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "y"),
        "should have 'y' value cell"
    );

    // `y`'s default expression should reference the `x` value cell via ValueRef
    let y_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "y")
        .expect("y value cell");
    let default_expr = y_cell
        .default_expr
        .as_ref()
        .expect("y should have default_expr");

    // The default expression should be a ValueRef pointing to ("S", "x")
    let expected_id = ValueCellId::new("S", "x");
    let refs = default_expr.collect_value_refs();
    assert!(
        refs.contains(&expected_id),
        "y's default_expr should contain ValueRef(S, x), got refs: {:?}",
        refs
    );
}

// ─── step-2: self.sub.param resolves ───

#[test]
fn self_dot_sub_dot_param_resolves() {
    // `self.bolt.d` should resolve to the sub component's member.
    let source = r#"structure Bolt {
    param d : Scalar = 10mm
}
structure S {
    sub bolt = Bolt()
    let val = self.bolt.d
}"#;
    let compiled = compile_no_errors(source);

    // Find the S template
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    // `val` should be a value cell
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "val"),
        "should have 'val' value cell"
    );

    // The default expression should reference the sub's member
    let val_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "val")
        .expect("val value cell");
    let default_expr = val_cell
        .default_expr
        .as_ref()
        .expect("val should have default_expr");
    let refs = default_expr.collect_value_refs();

    // Should reference bolt.d — the exact entity path depends on how subs are resolved,
    // but it should contain a reference to member "d" in the sub's entity path
    assert!(
        refs.iter().any(|r| r.member == "d"),
        "val's default_expr should reference member 'd' from sub bolt, got refs: {:?}",
        refs
    );
}

// ─── step-3: self in let binding ───

#[test]
fn self_in_let_binding_compiles() {
    // `self.a + 1mm` in a let binding should compile without errors.
    let source = r#"structure S {
    param a : Scalar = 3mm
    let b = self.a + 1mm
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];

    // Both value cells should exist
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "a"),
        "should have 'a' value cell"
    );
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "b"),
        "should have 'b' value cell"
    );
}

// ─── step-4: self in constraint ───

#[test]
fn self_in_constraint_compiles() {
    // `constraint self.x > 2mm` should compile without errors.
    let source = r#"structure S {
    param x : Scalar = 5mm
    constraint self.x > 2mm
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];

    // Should have at least one constraint
    assert!(
        !template.constraints.is_empty(),
        "should have at least one compiled constraint"
    );

    // The constraint expression should reference ValueCellId("S", "x")
    let expected_id = ValueCellId::new("S", "x");
    let constraint_refs: Vec<_> = template
        .constraints
        .iter()
        .flat_map(|c| c.expr.collect_value_refs())
        .collect();
    assert!(
        constraint_refs.contains(&expected_id),
        "constraint should reference S.x, got refs: {:?}",
        constraint_refs
    );
}

// ─── step-5: bare self as entity reference ───

#[test]
fn bare_self_as_entity_reference() {
    // Bare `self` (without `.member`) should resolve to the enclosing entity
    // as a StructureRef type. `let me = self` captures the entity itself.
    let source = r#"structure S {
    param x : Scalar = 5mm
    let me = self
}"#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    // `me` should be a value cell
    assert!(
        s_template.value_cells.iter().any(|vc| vc.id.member == "me"),
        "should have 'me' value cell"
    );

    // `me`'s type should be a StructureRef pointing to S
    let me_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "me")
        .expect("me value cell");
    assert_eq!(
        me_cell.cell_type,
        reify_types::Type::StructureRef("S".to_string()),
        "bare `self` should resolve to StructureRef(\"S\")"
    );
}

// ─── step-6: self in guarded block (TreeBracket pattern) ───

#[test]
fn self_in_guarded_block() {
    // The TreeBracket pattern from spec section 8.9: `self.depth` in a guarded
    // `where` block should resolve to the enclosing entity's params.
    let source = r#"structure TreeBracket {
    param depth : Int = 3
    param width : Scalar = 10mm

    where depth > 0 {
        let child_depth = self.depth - 1
        let child_width = self.width / 2
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];

    // Should have a guarded group
    assert!(
        !template.guarded_groups.is_empty(),
        "should have at least one guarded group"
    );

    // The guarded group should contain value cells referencing self.depth and self.width
    let expected_depth = ValueCellId::new("TreeBracket", "depth");
    let expected_width = ValueCellId::new("TreeBracket", "width");

    // Guarded block members live in guarded_groups[n].members, NOT top-level value_cells.
    // (See guard_compilation.rs for the canonical pattern.)
    let group = &template.guarded_groups[0];

    // Find child_depth in guarded group and check it references self.depth
    let child_depth_cell = group
        .members
        .iter()
        .find(|m| m.id.member == "child_depth")
        .expect("child_depth in guarded group");
    let depth_refs = child_depth_cell
        .default_expr
        .as_ref()
        .expect("child_depth default_expr")
        .collect_value_refs();
    assert!(
        depth_refs.contains(&expected_depth),
        "child_depth should reference TreeBracket.depth via self, got: {:?}",
        depth_refs
    );

    // Find child_width in guarded group and check it references self.width
    let child_width_cell = group
        .members
        .iter()
        .find(|m| m.id.member == "child_width")
        .expect("child_width in guarded group");
    let width_refs = child_width_cell
        .default_expr
        .as_ref()
        .expect("child_width default_expr")
        .collect_value_refs();
    assert!(
        width_refs.contains(&expected_width),
        "child_width should reference TreeBracket.width via self, got: {:?}",
        width_refs
    );
}

// ─── step-7: self error in fn body ───

#[test]
fn self_error_in_fn_body() {
    // `self` inside a function body is invalid — functions have no enclosing entity scope.
    // The implementation may reject this at parse time or compile time; both are valid.
    // Use the same branch-on-parse-errors pattern as self_error_at_module_scope.
    let source = r#"fn f(x: Scalar) -> Scalar {
    self.x
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_self"));

    if parsed.errors.is_empty() {
        // Parsing succeeded — compiler must reject `self` in fn body
        let compiled = reify_compiler::compile(&parsed);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "expected error diagnostic for `self` in fn body"
        );
    } else {
        // Parser correctly rejects `self` in fn body — at least one error produced
        assert!(
            !parsed.errors.is_empty(),
            "expected at least one parse error for `self` in fn body"
        );
    }
}

// ─── step-8: self error at module scope ───

#[test]
fn self_error_at_module_scope() {
    // `self` at module top level is invalid — there is no enclosing entity.
    // The parser rejects `self` at module scope before the compiler sees it,
    // which is the correct behavior: self is never valid outside an entity body.
    let source = r#"let x = self.y"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_self"));

    if parsed.errors.is_empty() {
        // If parsing unexpectedly succeeds, compilation must still reject it
        let compiled = reify_compiler::compile(&parsed);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "expected error diagnostic for `self` at module scope"
        );
    } else {
        // Parser correctly rejects `self` at module scope — at least one error produced
        assert!(
            !parsed.errors.is_empty(),
            "expected at least one parse error for `self` at module scope"
        );
    }
}

// ─── task-1125 step-1: self.collection_sub emits error ───

#[test]
fn self_dot_collection_sub_emits_error() {
    // `self.items` where `items` is a collection sub should emit an error,
    // not silently return StructureRef("Bolt") as if it were a single-instance sub.
    let source = r#"structure Bolt {
    param diameter : Scalar = 10mm
}
structure S {
    sub items : List<Bolt>
    let x = self.items
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items` on collection sub, got no errors"
    );
    let has_helpful_msg = errors.iter().any(|d| {
        let msg = &d.message;
        msg.contains("items")
            && (msg.contains("collection")
                || msg.contains("indexed")
                || msg.contains("index"))
    });
    assert!(
        has_helpful_msg,
        "expected error message mentioning 'items' and 'collection'/'indexed'/'index', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1125 step-3: self.collection_sub.member emits error ───

#[test]
fn self_dot_collection_sub_dot_member_emits_error() {
    // `self.items.diameter` where `items` is a collection sub should emit an error,
    // not silently return ValueRef(S.items, diameter) pointing at a nonexistent cell.
    let source = r#"structure Bolt {
    param diameter : Scalar = 10mm
}
structure S {
    sub items : List<Bolt>
    let d = self.items.diameter
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items.diameter` on collection sub, got no errors"
    );
    let has_helpful_msg = errors.iter().any(|d| {
        let msg = &d.message;
        msg.contains("items")
            && (msg.contains("collection")
                || msg.contains("indexed")
                || msg.contains("index"))
    });
    assert!(
        has_helpful_msg,
        "expected error message mentioning 'items' and 'collection'/'indexed'/'index', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1125 step-5: non-collection sub still returns StructureRef ───

#[test]
fn self_dot_non_collection_sub_still_returns_structure_ref() {
    // `self.bolt` where `bolt` is a single-instance sub should still compile cleanly
    // and produce a StructureRef("Bolt") cell — regression guard for steps 2 & 4.
    let source = r#"structure Bolt {
    param diameter : Scalar = 10mm
}
structure S {
    sub bolt = Bolt()
    let b = self.bolt
}"#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    let b_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "b")
        .expect("b value cell");
    assert_eq!(
        b_cell.cell_type,
        reify_types::Type::StructureRef("Bolt".to_string()),
        "self.bolt on a non-collection sub should resolve to StructureRef(\"Bolt\")"
    );
}

// ─── step-11: self.param equivalence with bare param ───

// NOTE: steps 9, 10, and 12 live in crates/reify-eval/tests/self_keyword_eval.rs.

#[test]
fn self_param_equivalence_with_bare_param() {
    // `self.x` and bare `x` should compile to identical ValueRef(S, x) expressions.
    let source = r#"structure S {
    param x : Scalar = 5mm
    let via_self = self.x
    let via_bare = x
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];

    let via_self_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_self")
        .expect("via_self value cell");
    let via_bare_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_bare")
        .expect("via_bare value cell");

    let self_refs = via_self_cell
        .default_expr
        .as_ref()
        .expect("via_self default_expr")
        .collect_value_refs();
    let bare_refs = via_bare_cell
        .default_expr
        .as_ref()
        .expect("via_bare default_expr")
        .collect_value_refs();

    let expected_id = ValueCellId::new("S", "x");

    // Both should reference exactly ValueCellId("S", "x")
    assert!(
        self_refs.contains(&expected_id),
        "via_self should reference S.x, got: {:?}",
        self_refs
    );
    assert!(
        bare_refs.contains(&expected_id),
        "via_bare should reference S.x, got: {:?}",
        bare_refs
    );
}

// ─── task-1127 step-1: self.param inside lambda in entity scope ───

#[test]
fn self_dot_param_inside_lambda_captures_entity_param() {
    // `self.x` inside a lambda body should be captured as ValueCellId("S", "x")
    // because the lambda inherits the entity scope via scope.clone() in the compiler.
    // The lambda's captures vec is built from all body refs minus lambda params.
    // Use `|y: Scalar|` so the addition `y + self.x` is dimensionally consistent.
    let source = r#"structure S {
    param x : Scalar = 5mm
    let f = |y: Scalar| y + self.x
}"#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    let f_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("f value cell");
    let f_expr = f_cell.default_expr.as_ref().expect("f should have default_expr");

    match &f_expr.kind {
        reify_types::CompiledExprKind::Lambda { captures, body, .. } => {
            let expected_id = ValueCellId::new("S", "x");
            assert!(
                captures.contains(&expected_id),
                "lambda captures should contain S.x (via self.x), got: {:?}",
                captures
            );
            // Also verify the lambda body references S.x (not a lambda-local shadow).
            // We call collect_value_refs on the unwrapped body (not on the Lambda node),
            // because collect_value_refs on a Lambda only returns captures, not body refs.
            let body_refs = body.collect_value_refs();
            assert!(
                body_refs.contains(&expected_id),
                "lambda body should reference S.x, got body refs: {:?}",
                body_refs
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

// ─── task-1127 step-2: bare self inside lambda in entity scope ───

#[test]
fn bare_self_inside_lambda_captures_entity_ref() {
    // bare `self` inside a lambda body should be captured as ValueCellId("S", "__self"),
    // the synthetic member name the compiler uses for bare self (expr.rs L99).
    // The lambda body's result_type should be StructureRef("S") confirming the
    // enclosing entity reference is returned.
    let source = r#"structure S {
    param x : Scalar = 5mm
    let f = |_unused| self
}"#;
    let compiled = compile_no_errors(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    let f_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("f value cell");
    let f_expr = f_cell.default_expr.as_ref().expect("f should have default_expr");

    match &f_expr.kind {
        reify_types::CompiledExprKind::Lambda { captures, body, .. } => {
            let expected_self_id = ValueCellId::new("S", "__self");
            assert!(
                captures.contains(&expected_self_id),
                "lambda captures should contain S.__self (bare self), got: {:?}",
                captures
            );
            // The lambda body is `self`, which resolves to StructureRef("S").
            assert_eq!(
                body.result_type,
                reify_types::Type::StructureRef("S".to_string()),
                "lambda body result_type should be StructureRef(\"S\") for bare self"
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

// ─── task-1127 step-3: self inside lambda in fn body is rejected ───

#[test]
fn self_inside_lambda_in_fn_body_errors() {
    // `self` inside a lambda in an fn body should be rejected — the lambda inherits
    // is_entity_scope=false from the enclosing fn scope (via scope.clone()), so
    // `self` falls through to the unresolved-name error path.
    // Mirror the dual-path pattern used by self_error_in_fn_body (step-7).
    let source = r#"fn f(x: Scalar) -> Scalar {
    let g = |y| y + self.x
    g(x)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_self"));

    if parsed.errors.is_empty() {
        // Parsing succeeded — the compiler must reject `self` inside the lambda body.
        let compiled = reify_compiler::compile(&parsed);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "expected error diagnostic for `self` inside lambda in fn body"
        );
    } else {
        // Parser correctly rejects `self` in fn body — at least one error produced.
        assert!(
            !parsed.errors.is_empty(),
            "expected at least one parse error for `self` inside lambda in fn body"
        );
    }
}
