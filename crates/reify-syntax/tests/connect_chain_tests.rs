//! Connect and chain statement tests.
//!
//! Tests for `connect a -> b` and `chain a -> b -> c` declarations.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("connect_test"));
    (module.declarations, module.errors)
}

// ── Step 1: simple connect ────────────────────────────────────────

#[test]
fn parse_connect_simple() {
    let (decls, errors) =
        parse_decls("structure S { port a : out T  port b : in T  connect a -> b }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // Two ports + one connect = 3 members
    assert_eq!(
        structure.members.len(),
        3,
        "expected 3 members, got {:?}",
        structure.members
    );

    assert!(matches!(&structure.members[0], MemberDecl::Port(_)));
    assert!(matches!(&structure.members[1], MemberDecl::Port(_)));

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    // Check left port ref
    match &connect.left.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }

    // Check operator
    assert_eq!(connect.operator, ConnectOp::Forward);

    // Check right port ref
    match &connect.right.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }

    // No connector or body
    assert!(connect.connector_type.is_none());
    assert!(connect.params.is_empty());
    assert!(connect.port_mappings.is_empty());
}

// ── Step 3: connect with connector ────────────────────────────────

#[test]
fn parse_connect_with_connector() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8 } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));
    assert_eq!(connect.params.len(), 1);
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => assert!((*n - 8.8).abs() < 1e-10),
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }
}

// ── Step 5: connect with port mapping ─────────────────────────────

#[test]
fn parse_connect_with_port_mapping() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b { shaft -> input_bore } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert!(connect.connector_type.is_none());
    assert_eq!(connect.port_mappings.len(), 1);
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── Step 7: bidirectional connect ─────────────────────────────────

#[test]
fn parse_connect_bidirectional() {
    let (decls, errors) =
        parse_decls("structure S { port a : bidi T  port b : bidi T  connect a <-> b }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.operator, ConnectOp::Bidirectional);
}

// ── Step 9: simple chain ──────────────────────────────────────────

#[test]
fn parse_chain_simple() {
    let (decls, errors) = parse_decls("structure S { chain a -> b -> c }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 1);
    let chain = match &structure.members[0] {
        MemberDecl::Chain(c) => c,
        other => panic!("expected Chain, got {:?}", other),
    };

    assert_eq!(chain.elements.len(), 3);
    match &chain.elements[0].kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }
    match &chain.elements[1].kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }
    match &chain.elements[2].kind {
        ExprKind::Ident(name) => assert_eq!(name, "c"),
        other => panic!("expected Ident('c'), got {:?}", other),
    }
}

// ── Step 11: connect with member access ───────────────────────────

#[test]
fn parse_connect_with_member_access() {
    let (decls, errors) = parse_decls(
        "structure S { sub m = Motor()  sub c = Coupling()  connect m.shaft -> c.driver }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    match &connect.left.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "m"),
                other => panic!("expected Ident('m'), got {:?}", other),
            }
            assert_eq!(member, "shaft");
        }
        other => panic!("expected MemberAccess, got {:?}", other),
    }

    match &connect.right.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "c"),
                other => panic!("expected Ident('c'), got {:?}", other),
            }
            assert_eq!(member, "driver");
        }
        other => panic!("expected MemberAccess, got {:?}", other),
    }
}

// ── task-246: mixed params and mappings ───────────────────────────

#[test]
fn parse_connect_mixed_params_and_mappings() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8, shaft -> input_bore } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));

    assert_eq!(
        connect.params.len(),
        1,
        "expected 1 param, got {:?}",
        connect.params
    );
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => {
            assert!((*n - 8.8).abs() < 1e-10, "expected 8.8, got {}", n)
        }
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }

    assert_eq!(
        connect.port_mappings.len(),
        1,
        "expected 1 port_mapping, got {:?}",
        connect.port_mappings
    );
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── task-246: mixed multiple entries ──────────────────────────────

#[test]
fn parse_connect_mixed_multiple_entries() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8, thickness = 2mm, shaft -> bore, flange -> seat } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));

    assert_eq!(
        connect.params.len(),
        2,
        "expected 2 params, got {:?}",
        connect.params
    );
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => assert!((*n - 8.8).abs() < 1e-10),
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }
    assert_eq!(connect.params[1].0, "thickness");
    match &connect.params[1].1.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert!(
                (value - 2.0).abs() < f64::EPSILON,
                "expected value 2.0, got {}",
                value
            );
            assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
        }
        other => panic!(
            "expected QuantityLiteral {{ value: 2.0, unit: \"mm\" }}, got {:?}",
            other
        ),
    }

    assert_eq!(
        connect.port_mappings.len(),
        2,
        "expected 2 port_mappings, got {:?}",
        connect.port_mappings
    );
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "bore");
    assert_eq!(connect.port_mappings[1].0, "flange");
    assert_eq!(connect.port_mappings[1].1, "seat");
}

// ── task-397 step-1: trailing comma in connect body ───────────────

#[test]
fn parse_connect_trailing_comma() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8, shaft -> input_bore, } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));

    assert_eq!(
        connect.params.len(),
        1,
        "expected 1 param, got {:?}",
        connect.params
    );
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => {
            assert!((*n - 8.8).abs() < 1e-10, "expected 8.8, got {}", n)
        }
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }

    assert_eq!(
        connect.port_mappings.len(),
        1,
        "expected 1 port_mapping, got {:?}",
        connect.port_mappings
    );
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── task-397 step-2: mapping before param ordering ────────────────

#[test]
fn parse_connect_mapping_before_param() {
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { shaft -> input_bore, grade = 8.8 } }",
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));

    assert_eq!(
        connect.params.len(),
        1,
        "expected 1 param, got {:?}",
        connect.params
    );
    assert_eq!(connect.params[0].0, "grade");
    match &connect.params[0].1.kind {
        ExprKind::NumberLiteral { value: n, .. } => {
            assert!((*n - 8.8).abs() < 1e-10, "expected 8.8, got {}", n)
        }
        other => panic!("expected NumberLiteral(8.8), got {:?}", other),
    }

    assert_eq!(
        connect.port_mappings.len(),
        1,
        "expected 1 port_mapping, got {:?}",
        connect.port_mappings
    );
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "input_bore");
}

// ── task-396 step-7: valid connect body produces no spurious errors ────

#[test]
fn connect_body_valid_no_spurious_errors() {
    // A well-formed connect body with both params and port mappings must
    // produce zero parse errors even after the diagnostic refactoring.
    // This guards against the refactored code accidentally triggering
    // diagnostics on valid input.
    let (decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = 8.8, shaft -> bore } }",
    );
    assert!(
        errors.is_empty(),
        "expected no parse errors for valid connect body, got: {:?}",
        errors
    );

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };
    assert_eq!(connect.connector_type.as_deref(), Some("BoltSet"));
    assert_eq!(
        connect.params.len(),
        1,
        "expected 1 param, got {:?}",
        connect.params
    );
    assert_eq!(connect.params[0].0, "grade");
    assert_eq!(
        connect.port_mappings.len(),
        1,
        "expected 1 port_mapping, got {:?}",
        connect.port_mappings
    );
    assert_eq!(connect.port_mappings[0].0, "shaft");
    assert_eq!(connect.port_mappings[0].1, "bore");
}

// ── task-396 step-5: malformed port mapping caught by check_and_lower! ──

#[test]
fn connect_body_malformed_mapping_emits_diagnostic() {
    // `{ shaft -> }` has a "from" but no "to"; tree-sitter error recovery
    // sets has_error() on the connect_statement (has_error propagates from
    // descendants). check_and_lower! catches this before lower_connect_body
    // is reached, emitting "invalid connect: ...".
    // Body-level diagnostics are tested directly in ts_parser::tests.
    let (_decls, errors) =
        parse_decls("structure S { port a : out T  port b : in T  connect a -> b { shaft -> } }");
    assert!(
        !errors.is_empty(),
        "expected at least one parse error for malformed port mapping, got none"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("invalid connect")),
        "expected check_and_lower! to emit 'invalid connect', got: {:?}",
        errors
    );
}

// ── task-396 step-3: malformed param caught by check_and_lower! ─

#[test]
fn connect_body_malformed_param_emits_diagnostic() {
    // `{ grade = }` has a name but no value expression; tree-sitter error
    // recovery sets has_error() on the connect_statement (has_error propagates
    // from descendants). check_and_lower! catches this before lower_connect_body
    // is reached, emitting "invalid connect: ...".
    // Body-level diagnostics are tested directly in ts_parser::tests.
    let (_decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = } }",
    );
    assert!(
        !errors.is_empty(),
        "expected at least one parse error for malformed connect parameter, got none"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("invalid connect")),
        "expected check_and_lower! to emit 'invalid connect', got: {:?}",
        errors
    );
}

// ── task-396 step-1: ERROR node in connect body caught by check_and_lower! ───

#[test]
fn connect_body_error_node_emits_diagnostic() {
    // `{ >= }` produces an ERROR child inside connect_body; tree-sitter
    // error recovery sets has_error() on the connect_statement (has_error
    // propagates from descendants). check_and_lower! catches this before
    // lower_connect_body is reached, emitting "invalid connect: ...".
    // Body-level diagnostics are tested directly in ts_parser::tests.
    //
    // NOTE: `>=` as the first token inside `{` avoids a GLR ambiguity
    // introduced by the variant_construction grammar production (step-6,
    // data-carrying-enums task α): after `b {`, the variant_construction fork
    // needs an identifier as the field name.  `>=` is NOT an identifier so
    // that fork dies immediately, the connect_body fork cleanly handles
    // `{ >= }` with an ERROR child, and has_error() propagates up through
    // connect_statement so check_and_lower! emits "invalid connect".
    // An identifier-first token such as `shaft >= }` would cause the
    // variant_construction fork to partially match the identifier before dying,
    // orphaning `{ … }` as a member-level ERROR node instead.
    let (_decls, errors) =
        parse_decls("structure S { port a : out T  port b : in T  connect a -> b { >= } }");
    assert!(
        !errors.is_empty(),
        "expected at least one parse error for invalid connect body syntax, got none"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("invalid connect")),
        "expected check_and_lower! to emit 'invalid connect', got: {:?}",
        errors
    );
}

// ── task-396 step-8: malformed outer connect_statement emits diagnostic ─

#[test]
fn connect_statement_malformed_outer_emits_diagnostic() {
    // `connect a ->` is missing the right endpoint; tree-sitter produces a
    // connect_statement node with has_error() == true (MISSING "right" field).
    // Without check_and_lower!, lower_member calls lower_connect which silently
    // returns None via `?` on the missing right field — no diagnostic is emitted.
    let (_decls, errors) = parse_decls("structure S { port a : out T  connect a -> }");
    assert!(
        !errors.is_empty(),
        "expected at least one parse error for malformed connect statement (missing right endpoint), got none"
    );
    assert!(
        errors.iter().any(|e| {
            e.message.contains("invalid connect")
                || e.message.contains("syntax error")
                || e.message.contains("connect")
        }),
        "expected an error mentioning 'invalid connect' or 'syntax error', got: {:?}",
        errors
    );
}

// ── task-396 step-10: valid outer connect statement produces no spurious errors ─

#[test]
fn connect_statement_valid_outer_no_spurious_errors() {
    // A simple well-formed connect must produce zero parse errors even after
    // wrapping the connect_statement arm with check_and_lower!. This guards
    // against check_and_lower! accidentally triggering false positives on
    // valid connect statements where has_error() is false.
    let (decls, errors) =
        parse_decls("structure S { port a : out T  port b : in T  connect a -> b }");
    assert!(
        errors.is_empty(),
        "expected no parse errors for valid connect statement, got: {:?}",
        errors
    );
    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect member, got {:?}", other),
    };
    assert_eq!(connect.operator, ConnectOp::Forward);
    match &connect.left.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }
    match &connect.right.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }
}

// ── parse_connect_reverse ─────────────────────────────────────────

#[test]
fn parse_connect_reverse() {
    let (decls, errors) =
        parse_decls("structure S { port a : out T  port b : in T  connect a <- b }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    assert_eq!(connect.operator, ConnectOp::Reverse);

    match &connect.left.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }
    match &connect.right.expr.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }
}

// ── task-396: comments in connect body produce no spurious errors ──

#[test]
fn connect_body_with_block_comment_no_spurious_errors() {
    // Block comment inside connect body with params — must not produce diagnostics.
    let (_decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { /* inline comment */ grade = 8.8 } }",
    );
    assert!(
        errors.is_empty(),
        "expected no parse errors for connect body with block comment, got: {:?}",
        errors
    );
}

#[test]
fn connect_body_with_line_comment_no_spurious_errors() {
    // Line comment inside connect body with port mappings — must not produce diagnostics.
    let (_decls, errors) = parse_decls(
        "structure S { port a : out T  port b : in T  connect a -> b {\n// comment\nshaft -> bore\n} }",
    );
    assert!(
        errors.is_empty(),
        "expected no parse errors for connect body with line comment, got: {:?}",
        errors
    );
}
