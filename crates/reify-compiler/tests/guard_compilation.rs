//! Guard compilation tests.
//!
//! Tests for compiling where-clauses and guarded blocks into
//! CompiledGuardedGroup entries in TopologyTemplate.

use reify_test_support::{assert_no_diagnostics, compile_first_template};
use reify_core::*;
use reify_ir::*;

/// Characterization test: `Chain` inside a `where {}` block is silently ignored
/// by `compile_guarded_members` — no diagnostic of any severity is emitted.
///
/// Top-level ports `a` and `b` are declared so the parser can resolve the port
/// identifiers in `chain a -> b`. The chain statement lives inside the guarded
/// block and is silently dropped.
#[test]
fn chain_in_block_guard_silently_ignored() {
    let source = r#"
trait T { param d : Length }
structure def S {
    param active : Bool = true
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    where active {
        chain a -> b
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    assert_no_diagnostics(
        &diagnostics,
        "chain in block guard (should be silently dropped)",
    );
    // Positive assertion: the chain inside the guarded block must NOT desugar
    // into a top-level connection — it should be silently dropped.
    assert!(
        template.connections.is_empty(),
        "expected no compiled connections — chain inside a guarded block should be \
         silently dropped, not desugared to top-level connections, got: {:?}",
        template
            .connections
            .iter()
            .map(|c| format!("{} -> {}", c.left_port, c.right_port))
            .collect::<Vec<_>>()
    );
    // Verify that the surrounding structure compiled correctly: ports a and b
    // (declared at the top level) should still appear in the compiled template.
    // If this assertion fails, the whole template failed to compile rather than
    // just the chain being dropped — which would make the connections.is_empty()
    // assertion trivially true for the wrong reason.
    assert_eq!(
        template.ports.len(),
        2,
        "ports a and b should still compile — only the chain should be dropped"
    );
    // Verify the guarded group itself exists (the `where active {}` block should
    // always produce a group) but contains no chain-derived members.  A chain
    // does not lower to a value cell, so members and constraints inside the group
    // must both be empty after the silent drop.
    assert_eq!(
        template.guarded_groups.len(),
        1,
        "expected exactly one guarded group for the `where active {{}}` block"
    );
    let group = &template.guarded_groups[0];
    assert!(
        group.members.is_empty(),
        "chain inside guarded block should produce no compiled members in the guard, \
         got: {:?}",
        group
            .members
            .iter()
            .map(|m| m.id.member.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        group.constraints.is_empty(),
        "chain inside guarded block should produce no compiled constraints in the guard, \
         got: {:?}",
        group.constraints
    );
}

/// Parse `param x : Scalar = 5mm where active` — the per-declaration where clause
/// should compile into a CompiledGuardedGroup with x as a guarded member.
#[test]
fn compile_param_with_where_clause() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // x should NOT be in top-level value_cells (it's guarded)
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "guarded param x should not be in top-level value_cells"
    );

    // active should be in top-level value_cells
    assert!(
        template
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "active"),
        "unguarded param active should be in top-level value_cells"
    );

    // Should have 1 guarded group
    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    // Guard value cell should be in structure_controlling
    assert!(
        template
            .structure_controlling
            .contains(&group.guard_value_cell),
        "guard_value_cell should be in structure_controlling"
    );

    // Members should contain x
    assert_eq!(group.members.len(), 1, "expected 1 member in guarded group");
    assert_eq!(group.members[0].id.member, "x");

    // No else members
    assert!(group.else_members.is_empty(), "expected no else members");
}

/// Block guard: `where active { param x .. param y .. constraint x > 2mm }`
/// should compile into one guarded group with 2 member value cells and 1 constraint.
#[test]
fn compile_block_guard() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        param y : Scalar = 10mm
        constraint x > 2mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // active should be in top-level value_cells
    assert!(
        template
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "active"),
        "unguarded param active should be in top-level value_cells"
    );

    // x, y should NOT be in top-level value_cells
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "guarded param x should not be in top-level value_cells"
    );
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "y"),
        "guarded param y should not be in top-level value_cells"
    );

    // Should have 1 guarded group from the block guard
    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    // 2 members (x, y)
    assert_eq!(
        group.members.len(),
        2,
        "expected 2 members in guarded group"
    );
    let member_names: Vec<_> = group.members.iter().map(|m| m.id.member.as_str()).collect();
    assert!(member_names.contains(&"x"), "expected member x");
    assert!(member_names.contains(&"y"), "expected member y");

    // 1 constraint
    assert_eq!(
        group.constraints.len(),
        1,
        "expected 1 constraint in guarded group"
    );

    // No top-level constraints (all guarded)
    assert!(
        template.constraints.is_empty(),
        "expected no top-level constraints"
    );

    // Guard value cell in structure_controlling
    assert!(
        template
            .structure_controlling
            .contains(&group.guard_value_cell),
        "guard_value_cell should be in structure_controlling"
    );
}

/// Nested guards: `where a { where b { param x : Scalar = 1mm } }`
/// should produce 2 guarded groups. The inner guard_expr should be
/// AND(ValueRef(outer_guard), ValueRef(b)).
#[test]
fn compile_nested_guards() {
    let source = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        where b {
            param x : Scalar = 1mm
        }
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // a, b in top-level; x should not be
    assert!(template.value_cells.iter().any(|vc| vc.id.member == "a"));
    assert!(template.value_cells.iter().any(|vc| vc.id.member == "b"));
    assert!(!template.value_cells.iter().any(|vc| vc.id.member == "x"));

    // Should have 2 guarded groups (one per nesting level)
    assert_eq!(
        template.guarded_groups.len(),
        2,
        "expected 2 guarded groups (outer + inner)"
    );

    // Find the inner group (the one with x as a member)
    let inner = template
        .guarded_groups
        .iter()
        .find(|g| g.members.iter().any(|m| m.id.member == "x"))
        .expect("expected inner group with member x");

    // Inner guard_expr should be BinOp::And
    assert!(
        matches!(
            &inner.guard_expr.kind,
            CompiledExprKind::BinOp { op: BinOp::And, .. }
        ),
        "inner guard_expr should be AND conjunction, got {:?}",
        inner.guard_expr.kind
    );
}

/// Else block: `where cond { param a } else { param b }`
/// should have members=[a] and else_members=[b] in the same guarded group.
#[test]
fn compile_else_block() {
    let source = r#"
structure S {
    param cond : Bool = true
    where cond {
        param a : Scalar = 1mm
    } else {
        param b : Scalar = 2mm
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Only 'cond' in top-level value_cells
    assert_eq!(
        template.value_cells.len(),
        1,
        "expected only 'cond' in top-level"
    );
    assert_eq!(template.value_cells[0].id.member, "cond");

    // 1 guarded group
    assert_eq!(template.guarded_groups.len(), 1);
    let group = &template.guarded_groups[0];

    // members=[a], else_members=[b]
    assert_eq!(group.members.len(), 1);
    assert_eq!(group.members[0].id.member, "a");

    assert_eq!(group.else_members.len(), 1);
    assert_eq!(group.else_members[0].id.member, "b");

    // Same guard_value_cell
    assert!(
        template
            .structure_controlling
            .contains(&group.guard_value_cell)
    );
}

/// Reference safety: unguarded `let y = x` referencing guarded `param x` should
/// produce a diagnostic error about unsafe/unguarded reference.
#[test]
fn reference_safety_unguarded_to_guarded_error() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
    let y = x
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Should contain a diagnostic about unguarded reference
    let guard_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unguarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        !guard_errors.is_empty(),
        "expected diagnostic about unguarded reference to guarded cell, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Reference safety: within the same guard block, references are safe.
#[test]
fn reference_safety_same_guard_ok() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        let y = x
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No reference safety errors
    let guard_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unguarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        guard_errors.is_empty(),
        "should not have reference safety errors for same-guard references: {:?}",
        guard_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Content hash must include guarded member default expressions, not just
/// guard_expr hashes. Two structures identical except for a guarded member's
/// default expression (5mm vs 10mm) must produce different content hashes.
#[test]
fn content_hash_includes_guarded_member_exprs() {
    let source_a = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
}
"#;
    let source_b = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 10mm where active
}
"#;

    let (template_a, diag_a) = compile_first_template(source_a);
    let (template_b, diag_b) = compile_first_template(source_b);

    // No errors
    assert!(diag_a.iter().all(|d| d.severity != Severity::Error));
    assert!(diag_b.iter().all(|d| d.severity != Severity::Error));

    // Content hashes should differ since guarded member defaults differ
    assert_ne!(
        template_a.content_hash, template_b.content_hash,
        "content_hash should differ when guarded member default expressions differ"
    );
}

/// Content hash must also differ when a guarded constraint expression changes.
#[test]
fn content_hash_includes_guarded_constraint_exprs() {
    let source_a = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        constraint x > 2mm
    }
}
"#;
    let source_b = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Scalar = 5mm
        constraint x > 3mm
    }
}
"#;

    let (template_a, diag_a) = compile_first_template(source_a);
    let (template_b, diag_b) = compile_first_template(source_b);

    assert!(diag_a.iter().all(|d| d.severity != Severity::Error));
    assert!(diag_b.iter().all(|d| d.severity != Severity::Error));

    assert_ne!(
        template_a.content_hash, template_b.content_hash,
        "content_hash should differ when guarded constraint expressions differ"
    );
}

/// Reference safety: an else_member referencing a cell guarded under a different
/// guard should produce a cross-guard diagnostic. Currently the cross-guard check
/// only iterates group.members, missing else_members.
#[test]
fn reference_safety_else_member_cross_guard() {
    let source = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        param x : Scalar = 5mm
    }
    where b {
    } else {
        let y = x
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Should contain a diagnostic about cross-guard or differently-guarded reference
    let guard_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("differently-guarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        !guard_warnings.is_empty(),
        "expected diagnostic about cross-guard reference from else_member y to guarded cell x, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Sub declaration inside a `where {}` block should emit a 'not yet supported' error.
///
/// Before the fix, `compile_guarded_members` silently dropped Sub declarations via
/// a `_ => {}` catch-all. This test asserts that exactly one error diagnostic is
/// emitted with a message containing 'not yet supported' and 'sub'.
#[test]
fn sub_in_block_guard_emits_unsupported_error() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        sub child = S()
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.to_lowercase().contains("not yet supported")
                && d.message.to_lowercase().contains("sub")
        })
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one 'not yet supported' error for sub in block guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Minimize declaration inside a `where {}` block should emit a 'not yet supported' error.
///
/// Before the fix, `compile_guarded_members` silently dropped Minimize declarations via
/// a `_ => {}` catch-all. This test asserts that exactly one error diagnostic is
/// emitted with a message containing 'not yet supported' and 'minimize'.
#[test]
fn minimize_in_block_guard_emits_unsupported_error() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm
    where active {
        minimize x
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.to_lowercase().contains("not yet supported")
                && d.message.to_lowercase().contains("minimize")
        })
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one 'not yet supported' error for minimize in block guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Maximize declaration inside a `where {}` block should emit a 'not yet supported' error.
///
/// Before the fix, `compile_guarded_members` silently dropped Maximize declarations via
/// a `_ => {}` catch-all. This test asserts that exactly one error diagnostic is
/// emitted with a message containing 'not yet supported' and 'maximize'.
#[test]
fn maximize_in_block_guard_emits_unsupported_error() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm
    where active {
        maximize x
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.to_lowercase().contains("not yet supported")
                && d.message.to_lowercase().contains("maximize")
        })
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "expected exactly one 'not yet supported' error for maximize in block guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Characterization test for task 1312: `ConstraintInst` inside a `where {}` block is
/// silently ignored by `compile_guarded_members` — no "not yet supported" or ICE
/// diagnostic is emitted.
///
/// This test pins the current silent-drop behavior so the wildcard→explicit-arm refactor
/// in `compile_guarded_members` is verified behavior-preserving. The real compile-time
/// guarantee (a future new `MemberDecl` variant must be explicitly handled) is provided
/// by Rust's exhaustiveness checker after the `_ => {}` wildcard is replaced by explicit
/// variant names.
#[test]
fn constraint_inst_in_block_guard_silently_ignored() {
    let source = r#"
constraint def NonNeg {
    param x : Scalar
    x >= 0
}
structure S {
    param active : Bool = true
    param y : Scalar = 5mm
    where active {
        constraint NonNeg(x: y)
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Assert no Error-severity diagnostics at all — `ConstraintInst` inside a
    // `where {}` block is silently dropped, so the full absence of errors is the
    // precise characterization of "silently ignored".  This also avoids the
    // fragile substring check for "ice" which could false-positive on words like
    // "service" or "notice" in future diagnostic messages.
    assert_no_diagnostics(
        &diagnostics,
        "constraint inst in block guard (should be silently dropped)",
    );
}

/// Characterization test: `Port` inside a `where {}` block is silently ignored by
/// `compile_guarded_members` — no error diagnostic is emitted.
///
/// This pins the current silent-drop behavior so future refactors (replacing the
/// wildcard arm with explicit variant names) are verified behavior-preserving.
/// The exhaustiveness guarantee (a future new `MemberDecl` variant must be
/// explicitly handled) is provided by Rust's exhaustiveness checker.
#[test]
fn port_in_block_guard_silently_ignored() {
    let source = r#"
trait T { param d : Length }
structure def S {
    param active : Bool = true
    where active {
        port mount : out T { param d : Length = 5mm }
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    assert_no_diagnostics(
        &diagnostics,
        "port in block guard (should be silently dropped)",
    );
    // Positive assertion: the port declared inside the guarded block must NOT
    // appear in the compiled template's top-level ports list — it should have
    // been silently dropped rather than promoted.
    assert!(
        template.ports.is_empty(),
        "expected no compiled ports — port declared inside a guarded block should be \
         silently dropped, not promoted to top-level, got: {:?}",
        template.ports.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

/// Characterization test: `Connect` inside a `where {}` block is silently ignored by
/// `compile_guarded_members` — no error diagnostic is emitted.
///
/// The ports `a` and `b` are declared at the top level so the parser can resolve
/// them; the `connect a -> b` statement lives inside the guarded block and is
/// silently dropped by the current implementation.
#[test]
fn connect_in_block_guard_silently_ignored() {
    let source = r#"
trait T { param d : Length }
structure def S {
    param active : Bool = true
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    where active {
        connect a -> b
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    assert_no_diagnostics(
        &diagnostics,
        "connect in block guard (should be silently dropped)",
    );
    // Positive assertion: the `connect a -> b` inside the guarded block must NOT
    // appear in the compiled template's connections list — it should have been
    // silently dropped.  (The top-level port declarations for `a` and `b` are
    // compiled normally, so template.ports will be non-empty; only the connection
    // that lives inside the guard is checked here.)
    assert!(
        template.connections.is_empty(),
        "expected no compiled connections — connect inside a guarded block should be \
         silently dropped, not promoted to top-level, got: {:?}",
        template
            .connections
            .iter()
            .map(|c| format!("{} -> {}", c.left_port, c.right_port))
            .collect::<Vec<_>>()
    );
}

/// Characterization test: `AssociatedType` inside a `where {}` block is rejected at
/// parse time — the grammar does not permit `type X = Y` inside a guarded block.
///
/// This means the `AssociatedType` arm in `compile_guarded_members` is unreachable
/// via normal parsing. This test pins that parser-level boundary: the compiler never
/// sees an `AssociatedType` in a guarded context because the grammar prevents it.
#[test]
fn associated_type_in_block_guard_rejected_by_parser() {
    let source = r#"
structure def S {
    param active : Bool = true
    where active {
        type Material = Steel
    }
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        !parsed.errors.is_empty(),
        "expected a parse error — `type X = Y` is not valid inside a `where {{}}` guarded block"
    );
    // Pin the exact error count so a future grammar change that makes `type`
    // valid inside guards (but introduces a different parse error) cannot
    // silently make this test pass for the wrong reason.
    assert_eq!(
        parsed.errors.len(),
        1,
        "expected exactly one parse error for `type X = Y` inside a guarded block, \
         got: {:?}",
        parsed.errors
    );
}

/// Characterization test: `MetaBlock` inside a `where {}` block is silently ignored
/// by `compile_guarded_members` — no diagnostic of any severity is emitted and the
/// guarded meta entry does not appear in the compiled template's meta map.
#[test]
fn meta_block_in_block_guard_silently_ignored() {
    let source = r#"
structure def S {
    param active : Bool = true
    meta {
        tag = "top"
    }
    where active {
        meta {
            guarded = "yes"
        }
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    assert_no_diagnostics(
        &diagnostics,
        "meta block in block guard (should be silently dropped)",
    );
    // Positive assertion: the meta entry from the guarded block must NOT appear
    // in the compiled template's meta map — it should be silently dropped.
    assert!(
        !template.meta.contains_key("guarded"),
        "expected 'guarded' key from the guarded meta block to be silently dropped, \
         got meta: {:?}",
        template.meta
    );
    // Positive assertion: the top-level meta block must still compile correctly.
    // Without this, an entirely-empty meta map would trivially satisfy the
    // negative assertion above, masking a complete compilation failure.
    assert_eq!(
        template.meta.get("tag").map(String::as_str),
        Some("top"),
        "top-level meta block should still compile — only the guarded meta block is dropped"
    );
}

/// Reference safety: a top-level constraint referencing a guarded param should
/// produce a diagnostic. Currently the unguarded-reference check only walks
/// value_cells, not top-level constraints.
#[test]
fn reference_safety_toplevel_constraint_to_guarded() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Scalar = 5mm where active
    constraint x > 2mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // Should contain a diagnostic about unguarded reference to guarded cell
    let guard_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("unguarded") || msg.contains("guarded")
        })
        .collect();

    assert!(
        !guard_warnings.is_empty(),
        "expected diagnostic about unguarded constraint referencing guarded cell x, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Reference safety: cross-guard completeness.
/// (A) Else-member referencing differently-guarded cell should produce diagnostic.
/// (B) A guarded constraint referencing a differently-guarded cell should produce diagnostic.
#[test]
fn reference_safety_cross_guard_completeness() {
    // Sub-case A: else_member cross-guard (already covered but included for completeness)
    let source_a = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        param x : Scalar = 5mm
    }
    where b {
    } else {
        let y = x
    }
}
"#;

    let (_, diagnostics_a) = compile_first_template(source_a);
    let guard_warnings_a: Vec<_> = diagnostics_a
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("differently-guarded") || msg.contains("guarded")
        })
        .collect();
    assert!(
        !guard_warnings_a.is_empty(),
        "sub-case A: expected cross-guard diagnostic for else_member y referencing guarded x, got: {:?}",
        diagnostics_a.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Sub-case B: guarded constraint referencing differently-guarded cell
    let source_b = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        param x : Scalar = 5mm
    }
    where b {
        constraint x > 0mm
    }
}
"#;

    let (_, diagnostics_b) = compile_first_template(source_b);
    let guard_warnings_b: Vec<_> = diagnostics_b
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("differently-guarded") || msg.contains("guarded")
        })
        .collect();
    assert!(
        !guard_warnings_b.is_empty(),
        "sub-case B: expected cross-guard diagnostic for guarded constraint referencing differently-guarded x, got: {:?}",
        diagnostics_b.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Step 29: Nested guards should NOT trigger false-positive cross-guard diagnostics.
///
/// Inner member y under __guard_1 (= a AND b) references x under __guard_0 (= a).
/// Since the inner guard implies the outer guard (it's a conjunction containing the outer),
/// the reference is safe. No 'differently-guarded' diagnostic should be emitted.
#[test]
fn reference_safety_nested_guard_no_false_positive() {
    let source = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        param x : Scalar = 1mm
        where b {
            let y = x
        }
    }
}
"#;

    let (_, diagnostics) = compile_first_template(source);
    let cross_guard_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("differently-guarded")
        })
        .collect();
    assert!(
        cross_guard_warnings.is_empty(),
        "nested guard reference should NOT produce false-positive cross-guard diagnostic, got: {:?}",
        cross_guard_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
