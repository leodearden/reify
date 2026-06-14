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

use reify_test_support::{compile_source, parse_and_compile};
use reify_core::{Severity, ValueCellId};

/// Returns `true` if any string in `messages` contains `word` as a whole token.
///
/// Token boundaries are any character that is neither ASCII alphanumeric nor `_`.
/// Extracted from duplicated closure logic that previously appeared at four call
/// sites in this file.
fn mentions_word<'a>(mut messages: impl Iterator<Item = &'a str>, word: &str) -> bool {
    messages.any(|msg| {
        msg.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|tok| tok == word)
    })
}

/// Convenience wrapper: returns `true` if any lowercased message in `msgs` contains
/// the word `"self"` as a whole token (i.e. not as part of `myself` or `self_param`).
fn msgs_mention_self(msgs: &[String]) -> bool {
    mentions_word(msgs.iter().map(String::as_str), "self")
}

/// Asserts that `ty` is `Type::List(Box::new(Type::StructureRef(expected_name)))`.
///
/// `label` is an optional context prefix (e.g. `"via_self:"`) that is prepended
/// to failure messages to distinguish calls when the same helper is invoked
/// multiple times in one test.  Pass `""` when no label is needed.
///
/// Panics with a descriptive message if the type does not match.
/// Extracted from duplicated match-assert blocks that appeared in the three
/// collection-sub fallback tests added in task 1770.
fn assert_list_of_struct_ref(ty: &reify_core::Type, expected_name: &str, label: &str) {
    let prefix = if label.is_empty() {
        String::new()
    } else {
        format!("{} ", label)
    };
    match ty {
        reify_core::Type::List(inner) => {
            assert_eq!(
                inner.as_ref(),
                &reify_core::Type::StructureRef(expected_name.to_string()),
                "{}expected List(StructureRef({:?})), got List({:?})",
                prefix,
                expected_name,
                inner,
            );
        }
        other => panic!("{}expected List type, got: {:?}", prefix, other),
    }
}

#[test]
fn test_mentions_word() {
    // (1) exact match: the word 'self' appears as its own token
    assert!(mentions_word(
        ["unknown identifier `self`"].iter().copied(),
        "self"
    ));

    // (2) substring-only: 'self' is embedded inside 'myself' — not a whole word
    assert!(!mentions_word(["myself"].iter().copied(), "self"));

    // (3) underscore-adjacent: 'self_param' is a single token, does not match 'self'
    assert!(!mentions_word(["self_param"].iter().copied(), "self"));

    // (4) callers pre-lowercase their messages, so pass lowercased strings;
    //     'self' at start of message
    assert!(mentions_word(
        ["self is invalid here"].iter().copied(),
        "self"
    ));

    // (5) empty iterator → false
    assert!(!mentions_word(std::iter::empty(), "self"));

    // (6) word at end of message
    assert!(mentions_word(["cannot use self"].iter().copied(), "self"));

    // (7) multiple messages — only the second mentions the word
    assert!(mentions_word(
        ["unrelated error", "invalid use of self"].iter().copied(),
        "self"
    ));

    // (8) none of the messages mention the word
    assert!(!mentions_word(
        ["unrelated error", "something else entirely"]
            .iter()
            .copied(),
        "self"
    ));
}

#[test]
fn test_assert_list_of_struct_ref_valid() {
    // Calling with a List(StructureRef("Foo")) and expected name "Foo" should not panic.
    let ty = reify_core::Type::List(Box::new(reify_core::Type::StructureRef("Foo".to_string())));
    assert_list_of_struct_ref(&ty, "Foo", "");
}

#[test]
#[should_panic(expected = "expected List type")]
fn test_assert_list_of_struct_ref_non_list_panics() {
    // Calling with a non-List type should panic with "expected List type".
    let ty = reify_core::Type::Bool;
    assert_list_of_struct_ref(&ty, "Foo", "");
}

#[test]
#[should_panic(expected = "expected List(StructureRef")]
fn test_assert_list_of_struct_ref_wrong_name_panics() {
    // Calling with List(StructureRef("Bar")) but expecting "Foo" should panic with
    // a message that includes "expected List(StructureRef".
    let ty = reify_core::Type::List(Box::new(reify_core::Type::StructureRef("Bar".into())));
    assert_list_of_struct_ref(&ty, "Foo", "");
}

#[test]
#[should_panic(expected = "ctx:")]
fn test_assert_list_of_struct_ref_label_in_panic() {
    // Calling with a non-List type and a non-empty label should produce a panic
    // message that includes the label prefix.  This exercises the `prefix` formatting
    // path (lines 48-52) together with the outer `other =>` panic branch (line 64),
    // producing "ctx: expected List type, got: Bool".
    let ty = reify_core::Type::Bool;
    assert_list_of_struct_ref(&ty, "Foo", "ctx:");
}

// ─── step-1: self.param resolves to correct ValueRef ───

#[test]
fn self_dot_param_resolves_to_value_ref() {
    // `self.x` inside a structure should resolve to the same value cell as `x`.
    let source = r#"structure S {
    param x : Length = 5mm
    let y = self.x
}"#;
    let compiled = parse_and_compile(source);
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
    param d : Length = 10mm
}
structure S {
    sub bolt = Bolt()
    let val = self.bolt.d
}"#;
    let compiled = parse_and_compile(source);

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
    param a : Length = 3mm
    let b = self.a + 1mm
}"#;
    let compiled = parse_and_compile(source);
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
    param x : Length = 5mm
    constraint self.x > 2mm
}"#;
    let compiled = parse_and_compile(source);
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
    param x : Length = 5mm
    let me = self
}"#;
    let compiled = parse_and_compile(source);
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
        reify_core::Type::StructureRef("S".to_string()),
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
    param width : Length = 10mm

    where depth > 0 {
        let child_depth = self.depth - 1
        let child_width = self.width / 2
    }
}"#;
    let compiled = parse_and_compile(source);
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
    let source = r#"fn f(x: Length) -> Scalar {
    self.x
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_self"));

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
        let msgs: Vec<String> = errors.iter().map(|d| d.message.to_lowercase()).collect();
        assert!(
            msgs_mention_self(&msgs),
            "expected a compile error mentioning `self` for `self` in fn body, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    } else {
        // Parser rejected the source — at least one parse error produced.
        // The parse error message embeds the source snippet, which includes `self`;
        // verify the error references `self` as a whole word to guard against
        // unrelated syntax regressions being mistaken for a self-rejection.
        let msgs: Vec<String> = parsed
            .errors
            .iter()
            .map(|e| e.message.to_lowercase())
            .collect();
        assert!(
            msgs_mention_self(&msgs),
            "expected a parse error mentioning `self` for `self` in fn body, got: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_self"));

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

// ─── task-1125 step-1 (updated by task-1280 step-3): self.collection_sub resolves to List<T> ───

#[test]
fn self_dot_collection_sub_resolves_to_list() {
    // `self.items` where `items` is a collection sub should resolve to a List<T> value cell,
    // mirroring bare `items`. No error should be emitted (task-1280 fix).
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let x = self.items
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let x_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("x value cell");

    let expected_id = ValueCellId::new("S", "__list_items__diameter");
    let x_refs = x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .collect_value_refs();
    assert!(
        x_refs.contains(&expected_id),
        "self.items should reference S.__list_items__diameter, got: {:?}",
        x_refs
    );

    let x_ty = &x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .result_type;
    assert!(
        matches!(x_ty, reify_core::Type::List(_)),
        "self.items should have List type, got: {:?}",
        x_ty
    );
}

// ─── task-1125 step-3: self.collection_sub.member emits error ───

#[test]
fn self_dot_collection_sub_dot_member_emits_error() {
    // `self.items.diameter` where `items` is a collection sub should emit an error,
    // not silently return ValueRef(S.items, diameter) pointing at a nonexistent cell.
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let d = self.items.diameter
}"#;
    let compiled = compile_source(source);
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
            && (msg.contains("collection") || msg.contains("indexed") || msg.contains("index"))
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
    param diameter : Length = 10mm
}
structure S {
    sub bolt = Bolt()
    let b = self.bolt
}"#;
    let compiled = parse_and_compile(source);
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
        reify_core::Type::StructureRef("Bolt".to_string()),
        "self.bolt on a non-collection sub should resolve to StructureRef(\"Bolt\")"
    );
}

// ─── step-11: self.param equivalence with bare param ───

// NOTE: steps 9, 10, and 12 live in crates/reify-eval/tests/self_keyword_eval.rs.

#[test]
fn self_param_equivalence_with_bare_param() {
    // `self.x` and bare `x` should compile to identical ValueRef(S, x) expressions.
    let source = r#"structure S {
    param x : Length = 5mm
    let via_self = self.x
    let via_bare = x
}"#;
    let compiled = parse_and_compile(source);
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
    // Use `|y: Length|` so the addition `y + self.x` is dimensionally consistent.
    let source = r#"structure S {
    param x : Length = 5mm
    let f = |y: Length| y + self.x
}"#;
    let compiled = parse_and_compile(source);
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
    let f_expr = f_cell
        .default_expr
        .as_ref()
        .expect("f should have default_expr");

    match &f_expr.kind {
        reify_ir::CompiledExprKind::Lambda { captures, body, .. } => {
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
    param x : Length = 5mm
    let f = |_unused| self
}"#;
    let compiled = parse_and_compile(source);
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
    let f_expr = f_cell
        .default_expr
        .as_ref()
        .expect("f should have default_expr");

    match &f_expr.kind {
        reify_ir::CompiledExprKind::Lambda { captures, body, .. } => {
            let expected_self_id = ValueCellId::new("S", "__self");
            assert!(
                captures.contains(&expected_self_id),
                "lambda captures should contain S.__self (bare self), got: {:?}",
                captures
            );
            // The lambda body is `self`, which resolves to StructureRef("S").
            assert_eq!(
                body.result_type,
                reify_core::Type::StructureRef("S".to_string()),
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
    let source = r#"fn f(x: Length) -> Scalar {
    let g = |y| y + self.x
    g(x)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_self"));

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
        let msgs: Vec<String> = errors.iter().map(|d| d.message.to_lowercase()).collect();
        assert!(
            msgs_mention_self(&msgs),
            "expected a compile error mentioning `self` for `self` inside lambda in fn body, got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    } else {
        // Parser rejected the source — at least one parse error produced.
        // The parse error message embeds the source snippet, which includes `self`;
        // verify the error references `self` as a whole word to guard against
        // unrelated syntax regressions being mistaken for a self-rejection.
        let msgs: Vec<String> = parsed
            .errors
            .iter()
            .map(|e| e.message.to_lowercase())
            .collect();
        assert!(
            msgs_mention_self(&msgs),
            "expected a parse error mentioning `self` for `self` inside lambda in fn body, got: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
}

// ─── task-1280 step-1: self.collection_sub equivalence with bare collection sub ───

#[test]
fn self_dot_collection_sub_equivalence_with_bare() {
    // `self.bolts` and bare `bolts` (a collection sub) should compile to the same
    // ValueRef, mirroring how `self.param` ≡ bare `param` (step-11).
    // Both should resolve to S.__list_bolts__diameter with type List<T>.
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub bolts : List<Bolt>
    let via_self = self.bolts
    let via_bare = bolts
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let via_self_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_self")
        .expect("via_self value cell");
    let via_bare_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_bare")
        .expect("via_bare value cell");

    let expected_id = ValueCellId::new("S", "__list_bolts__diameter");

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

    assert!(
        self_refs.contains(&expected_id),
        "via_self should reference S.__list_bolts__diameter, got: {:?}",
        self_refs
    );
    assert!(
        bare_refs.contains(&expected_id),
        "via_bare should reference S.__list_bolts__diameter, got: {:?}",
        bare_refs
    );

    // Both should resolve to List type
    let self_ty = &via_self_cell
        .default_expr
        .as_ref()
        .expect("via_self default_expr")
        .result_type;
    let bare_ty = &via_bare_cell
        .default_expr
        .as_ref()
        .expect("via_bare default_expr")
        .result_type;

    assert!(
        matches!(self_ty, reify_core::Type::List(_)),
        "via_self should have List type, got: {:?}",
        self_ty
    );
    assert!(
        matches!(bare_ty, reify_core::Type::List(_)),
        "via_bare should have List type, got: {:?}",
        bare_ty
    );
    assert_eq!(
        self_ty, bare_ty,
        "via_self and via_bare should have identical result types"
    );
}

// ─── task-1280 amend: lexicographic first-member selection with multiple params ───

#[test]
fn self_dot_collection_sub_picks_lexicographic_first_member() {
    // `Bolt` has two params: `diameter` and `length`.
    // Lexicographically "diameter" < "length" (d < l), so the resolver must pick
    // `diameter` as the representative member — both via `self.bolts` and bare `bolts`.
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
    param length : Length = 50mm
}
structure S {
    sub bolts : List<Bolt>
    let via_self = self.bolts
    let via_bare = bolts
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let via_self_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_self")
        .expect("via_self value cell");
    let via_bare_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_bare")
        .expect("via_bare value cell");

    // Must resolve to diameter (lexicographically first), not length.
    let expected_id = ValueCellId::new("S", "__list_bolts__diameter");
    let wrong_id = ValueCellId::new("S", "__list_bolts__length");

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

    assert!(
        self_refs.contains(&expected_id),
        "via_self should reference __list_bolts__diameter (lexicographic first), got: {:?}",
        self_refs
    );
    assert!(
        !self_refs.contains(&wrong_id),
        "via_self must NOT reference __list_bolts__length, got: {:?}",
        self_refs
    );
    assert!(
        bare_refs.contains(&expected_id),
        "via_bare should reference __list_bolts__diameter (lexicographic first), got: {:?}",
        bare_refs
    );
    assert!(
        !bare_refs.contains(&wrong_id),
        "via_bare must NOT reference __list_bolts__length, got: {:?}",
        bare_refs
    );
}

// ─── task-1770 steps: fallback path tests ───
// step-1: empty-params structure (sub_member_types["parts"] = Some(empty BTreeMap))
// step-3: forward reference      (sub_member_types.get("bolts") = None)
// step-4: self/bare equivalence on fallback path

// ─── task-1770 step-1: fallback path — empty-params structure ───

#[test]
fn collection_sub_fallback_empty_structure_cell_id_and_type() {
    // `structure Empty {}` has no params, so sub_member_types["parts"] is Some(empty BTreeMap).
    // resolve_collection_sub_to_list falls back to the coarse path and should produce:
    //   (a) cell ID  : S.__list_parts  (not __list_parts__<member>, since there is no member)
    //   (b) type     : List(StructureRef("Empty"))  ← structure TYPE name, not field name
    //
    // Before the fix this test FAILS because the fallback produces StructureRef("parts")
    // (the field name) instead of StructureRef("Empty") (the structure type name).
    let source = r#"structure Empty {}
structure S {
    sub parts : List<Empty>
    let x = self.parts
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let x_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("x value cell");

    // (a) cell ID must be S.__list_parts (fallback coarse ID)
    let expected_id = ValueCellId::new("S", "__list_parts");
    let x_refs = x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .collect_value_refs();
    assert!(
        x_refs.contains(&expected_id),
        "self.parts should reference S.__list_parts (fallback coarse ID), got: {:?}",
        x_refs
    );

    // (b) type must be List(StructureRef("Empty")) — the structure type name, not the field name
    let x_ty = &x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .result_type;
    assert_list_of_struct_ref(x_ty, "Empty", "");
}

// ─── task-1770 step-3: fallback path — forward reference (sub_member_types returns None) ───

#[test]
fn collection_sub_fallback_forward_ref_uses_type_name() {
    // S is declared BEFORE Bolt.  When the compiler processes S, Bolt's template has not
    // yet been compiled, so sub_member_types.get("bolts") returns None — triggering the
    // outer-None branch of the fallback (different from step-1's empty-BTreeMap branch).
    // The fix should make both branches produce List(StructureRef("Bolt")), not
    // List(StructureRef("bolts")).
    //
    // Assertions:
    //   (a) cell ID : S.__list_bolts  (coarse fallback ID)
    //   (b) type    : List(StructureRef("Bolt"))  — structure TYPE name
    let source = r#"structure S {
    sub bolts : List<Bolt>
    let x = self.bolts
}
structure Bolt {
    param diameter : Length = 10mm
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let x_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("x value cell");

    // (a) cell ID must be S.__list_bolts (coarse fallback — Bolt was not yet compiled)
    let expected_id = ValueCellId::new("S", "__list_bolts");
    let x_refs = x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .collect_value_refs();
    assert!(
        x_refs.contains(&expected_id),
        "forward-ref fallback should reference S.__list_bolts, got: {:?}",
        x_refs
    );

    // (b) type must be List(StructureRef("Bolt")) — the structure type name
    let x_ty = &x_cell
        .default_expr
        .as_ref()
        .expect("x default_expr")
        .result_type;
    assert_list_of_struct_ref(x_ty, "Bolt", "");
}

// ─── task-1770 step-4: self/bare equivalence on the fallback path ───

#[test]
fn collection_sub_fallback_equivalence_self_and_bare() {
    // Extends the equivalence pattern from self_dot_collection_sub_equivalence_with_bare
    // to the fallback scenario: `structure Empty {}` (no params) triggers the fallback.
    // Both `self.parts` and bare `parts` should resolve to the same cell and type.
    //
    // Assertions:
    //   (a) both via_self and via_bare reference S.__list_parts (same coarse ID)
    //   (b) both have type List(StructureRef("Empty"))
    let source = r#"structure Empty {}
structure S {
    sub parts : List<Empty>
    let via_self = self.parts
    let via_bare = parts
}"#;
    let compiled = parse_and_compile(source);
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");

    let via_self_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_self")
        .expect("via_self value cell");
    let via_bare_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "via_bare")
        .expect("via_bare value cell");

    // (a) Both should reference S.__list_parts
    let expected_id = ValueCellId::new("S", "__list_parts");

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

    assert!(
        self_refs.contains(&expected_id),
        "via_self (fallback) should reference S.__list_parts, got: {:?}",
        self_refs
    );
    assert!(
        bare_refs.contains(&expected_id),
        "via_bare (fallback) should reference S.__list_parts, got: {:?}",
        bare_refs
    );

    // (b) Both should have type List(StructureRef("Empty"))
    let self_ty = &via_self_cell
        .default_expr
        .as_ref()
        .expect("via_self default_expr")
        .result_type;
    let bare_ty = &via_bare_cell
        .default_expr
        .as_ref()
        .expect("via_bare default_expr")
        .result_type;

    assert_list_of_struct_ref(self_ty, "Empty", "via_self:");
    assert_list_of_struct_ref(bare_ty, "Empty", "via_bare:");
}

// ─── task-1281 step-3: self.collection_sub.member error uses correct fallback type ───

#[test]
fn self_dot_collection_sub_member_error_has_correct_fallback_type() {
    // `self.items.diameter` where `items` is List<Bolt> should emit an error AND
    // have a cell type matching Bolt's diameter type (Scalar with length dimension),
    // NOT Type::dimensionless_scalar().
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let d = self.items.diameter
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items.diameter` on collection sub"
    );

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    let d_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("d value cell");

    // The fallback type should be exactly Scalar{LENGTH} — diameter is declared as `Scalar = 10mm`.
    // (Type::Scalar { dimension: DimensionVector::LENGTH })
    assert_eq!(
        d_cell.cell_type,
        reify_core::Type::Scalar {
            dimension: reify_core::DimensionVector::LENGTH
        },
        "self.items.diameter error fallback should be Scalar{{LENGTH}}, got {:?}",
        d_cell.cell_type
    );
}

// ─── task-1281 step-5: self.collection_sub aggregation recommends drop self ───

#[test]
fn self_dot_collection_sub_aggregation_recommends_drop_self() {
    // `self.items.count` should emit an error recommending `items.count`
    // (drop self.), NOT `items[i].count` (the per-instance recommendation).
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let c = self.items.count
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items.count` on collection sub"
    );

    // (a) an error diagnostic exists — checked above

    // (b) error message should contain 'items.count' (correct aggregation recommendation)
    let has_items_count = errors.iter().any(|d| d.message.contains("items.count"));
    assert!(
        has_items_count,
        "expected error message containing 'items.count', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) error message should NOT contain 'items[i].count' (misleading per-instance recommendation)
    let has_indexed = errors.iter().any(|d| d.message.contains("items[i].count"));
    assert!(
        !has_indexed,
        "error message should not recommend 'items[i].count' for aggregation, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1281 step-7: no cascading diagnostics after type fallback fixes ───

#[test]
fn self_dot_collection_sub_no_cascading_diagnostics() {
    // `self.items.diameter` used in a constraint should produce exactly 1 error
    // (the collection sub error), not additional type-mismatch diagnostics from
    // d being Type::dimensionless_scalar() and then being compared with 5mm (Scalar{Length}).
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let d = self.items.diameter
    constraint d > 5mm
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error (collection sub access), got {}: {:?}",
        errors.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1281 amend: self.collection_sub.sum also recommends drop self ───

#[test]
fn self_dot_collection_sub_sum_aggregation_recommends_drop_self() {
    // `self.items.sum` should emit an error recommending `items.sum`
    // (drop self.), NOT `items[i].sum` (the per-instance recommendation).
    // This guards against partial regressions in the aggregation-member list
    // (e.g. if a new aggregation is added only to one branch).
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let s = self.items.sum
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items.sum` on collection sub"
    );

    // error message should contain 'items.sum' (correct aggregation recommendation)
    let has_items_sum = errors.iter().any(|d| d.message.contains("items.sum"));
    assert!(
        has_items_sum,
        "expected error message containing 'items.sum', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // error message should NOT contain 'items[i].sum' (misleading per-instance recommendation)
    let has_indexed = errors.iter().any(|d| d.message.contains("items[i].sum"));
    assert!(
        !has_indexed,
        "error message should not recommend 'items[i].sum' for aggregation, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1281 review S2: aggregation no-cascading-diagnostics ───

#[test]
fn self_dot_collection_sub_aggregation_no_cascading_diagnostics() {
    // `self.items.count` used in a constraint should produce exactly 1 error
    // (the collection sub aggregation error), not additional type-mismatch
    // diagnostics from the fallback type being wrong (e.g. Real vs Int).
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let c = self.items.count
    constraint c > 0
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error (collection sub aggregation access), got {}: {:?}",
        errors.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── task-1281 review S3: unknown member on collection sub ───

#[test]
fn self_dot_collection_sub_unknown_member_error() {
    // `self.items.nonexistent` where `nonexistent` is neither an aggregation method
    // nor a field of the element struct should emit an "unknown member" error,
    // NOT suggest indexed access to a field that doesn't exist.
    let source = r#"structure Bolt {
    param diameter : Length = 10mm
}
structure S {
    sub items : List<Bolt>
    let x = self.items.nonexistent
}"#;
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for `self.items.nonexistent`"
    );

    let has_unknown_member = errors.iter().any(|d| d.message.contains("unknown member"));
    assert!(
        has_unknown_member,
        "expected 'unknown member' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Should NOT suggest indexed access for a member that doesn't exist
    let has_indexed = errors.iter().any(|d| d.message.contains("items[i]"));
    assert!(
        !has_indexed,
        "should not suggest indexed access for unknown member, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
