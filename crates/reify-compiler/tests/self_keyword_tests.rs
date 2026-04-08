//! Tests for `self` keyword resolution in structures.
//!
//! The `self` keyword refers to the enclosing entity definition or specialization
//! (spec section 8.6). `self.param_name` is equivalent to `param_name` for
//! locally-declared names; bare `self` is required when the entity itself is
//! the referent. `self` is invalid outside entity scope (fn bodies, module scope).

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
#[ignore = "requires task 153: self keyword compiler support"]
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

// ─── step-11: self.param equivalence with bare param ───

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
