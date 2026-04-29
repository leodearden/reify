//! Statement-form `forall` per-element elaboration tests (task 2364, spec §5.4).
//!
//! Task 2363 introduced `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` AST nodes with stub error diagnostics in the
//! compiler. Task 2364 lifts those stubs and emits one `CompiledConnection` /
//! `CompiledConstraint` per collection element, with each generated decl
//! carrying a span back to the source `forall` and a label encoding the
//! bound-variable name and element index.
//!
//! These tests pin:
//!   * Per-element emission for `ListLiteral` and `Ident`-resolved-to-collection-sub
//!     collections (PRD criteria 5, 8).
//!   * Empty-collection: zero decls, no error (PRD criterion 6).
//!   * Undef-count collection: zero decls, no error (PRD criterion 7,
//!     first half — re-elaboration on count change is out of scope).
//!   * Label format `forall@<var>[<idx>]` (PRD criterion 10).
//!   * Span anchored at the source forall declaration.
//!   * Body-where-clause routing through guarded groups (PRD criterion 9).
//!   * Constraint-instantiation body shape.
//!   * Chain body shape (pairwise per element).
//!   * Non-iterable collection diagnostic.

use reify_test_support::{compile_source, errors_only};
use reify_types::{BinOp, CompiledExprKind, ModulePath, Value};

/// Recover the `MemberDecl::ForallConstraint` span by re-parsing `source`,
/// finding the structure named `structure_name`, and returning the span of
/// the first ForallConstraint member encountered. Panics if not found.
fn find_forall_constraint_span(
    source: &str,
    structure_name: &str,
) -> reify_types::SourceSpan {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Structure(s) = decl {
            if s.name == structure_name {
                for m in &s.members {
                    if let reify_syntax::MemberDecl::ForallConstraint(f) = m {
                        return f.span;
                    }
                }
            }
        }
    }
    panic!("no ForallConstraint found in structure {}", structure_name);
}

/// Recover the `MemberDecl::ForallConnect` span by re-parsing `source`,
/// finding the structure named `structure_name`, and returning the span of
/// the first ForallConnect member encountered. Panics if not found.
fn find_forall_connect_span(
    source: &str,
    structure_name: &str,
) -> reify_types::SourceSpan {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Structure(s) = decl {
            if s.name == structure_name {
                for m in &s.members {
                    if let reify_syntax::MemberDecl::ForallConnect(f) = m {
                        return f.span;
                    }
                }
            }
        }
    }
    panic!("no ForallConnect found in structure {}", structure_name);
}

/// `forall v in [1, 2, 3]: constraint v > 0` should emit exactly 3
/// CompiledConstraints, each comparing the substituted literal element
/// against 0 (BinOp::Gt with Literal(Int) on the left), and each carrying a
/// span equal to the `MemberDecl::ForallConstraint` source span.
#[test]
fn forall_constraint_over_list_literal_emits_per_element_constraints() {
    let source = r#"
structure S {
    forall v in [1, 2, 3]: constraint v > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors after lifting ForallConstraint stub, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .collect();

    assert_eq!(
        forall_constraints.len(),
        3,
        "expected exactly 3 forall@v[*] constraints, got {}: labels = {:?}",
        forall_constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // Recover the source forall span to assert each emitted constraint
    // carries it.
    let forall_span = find_forall_constraint_span(source, "S");
    for c in &forall_constraints {
        assert_eq!(
            c.span, forall_span,
            "forall-emitted constraint span must equal the source forall span; \
             label = {:?}",
            c.label
        );
    }

    // Each emitted constraint is `<element> > 0` — a BinOp::Gt whose left is a
    // Literal(Int) matching the per-element value, and whose right is
    // Literal(Int(0)).
    for (i, c) in forall_constraints.iter().enumerate() {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                assert_eq!(
                    *op,
                    BinOp::Gt,
                    "expected BinOp::Gt in element {} body, got {:?}",
                    i,
                    op
                );
                let expected = (i as i64) + 1;
                match &left.kind {
                    CompiledExprKind::Literal(Value::Int(n)) => assert_eq!(
                        *n, expected,
                        "expected substituted literal {} on left of element {}, got {}",
                        expected, i, n
                    ),
                    other => panic!(
                        "expected Literal(Int({})) on left of element {}, got {:?}",
                        expected, i, other
                    ),
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(0)) => {}
                    other => panic!(
                        "expected Literal(Int(0)) on right of element {}, got {:?}",
                        i, other
                    ),
                }
            }
            other => panic!(
                "expected BinOp(Gt) for element {}, got {:?}",
                i, other
            ),
        }
    }
}

/// `forall v in vents: constraint v.mass < 50kg` over a 3-element collection
/// sub should emit exactly 3 CompiledConstraints, each whose body resolves
/// to a `BinOp::Lt` whose left is a `ValueRef` into the scoped sub-component
/// instance for the matching index (`S.vents[0].mass`, `S.vents[1].mass`,
/// `S.vents[2].mass`). Pins PRD criterion 5: forall over a collection sub
/// with a known count emits one decl per element, with the bound var
/// substituted by an indexed access.
#[test]
fn forall_constraint_over_collection_sub_with_known_count_emits_per_element_constraints() {
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    constraint vents.count == 3
    forall v in vents: constraint v.mass < 50kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for collection-sub forall, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .collect();

    assert_eq!(
        forall_constraints.len(),
        3,
        "expected exactly 3 forall@v[*] constraints, got {}: labels = {:?}",
        forall_constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // Each emitted constraint is `vents[i].mass < 50kg` — a BinOp::Lt whose
    // left walks down to a ValueRef with entity == "S.vents[<i>]" and member
    // == "mass". (The index lookup goes through compile_expr's existing
    // collection-member-access lowering — see compile_indexed_collection_member_access
    // in collection_sub_tests.rs.)
    for (i, c) in forall_constraints.iter().enumerate() {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, .. } => {
                assert_eq!(
                    *op,
                    BinOp::Lt,
                    "expected BinOp::Lt in element {} body, got {:?}",
                    i,
                    op
                );
                match &left.kind {
                    CompiledExprKind::ValueRef(id) => {
                        assert_eq!(
                            id.entity,
                            format!("S.vents[{}]", i),
                            "expected scoped entity S.vents[{}] for element {}, got {}",
                            i,
                            i,
                            id.entity
                        );
                        assert_eq!(
                            id.member, "mass",
                            "expected member 'mass' for element {}, got {}",
                            i, id.member
                        );
                    }
                    other => panic!(
                        "expected ValueRef(S.vents[{}].mass) on left of element {}, got {:?}",
                        i, i, other
                    ),
                }
            }
            other => panic!(
                "expected BinOp(Lt) for element {}, got {:?}",
                i, other
            ),
        }
    }
}

/// `forall v in [1, 2, 3]: constraint v > 0` should emit constraints with
/// labels `forall@v[0]`, `forall@v[1]`, `forall@v[2]` exactly (no
/// whitespace tolerance). Pins the label-format convention used for
/// diagnostic provenance — a per-element constraint failure cites the
/// element index via the label (PRD criterion 10).
#[test]
fn forall_constraint_label_encodes_element_index() {
    let source = r#"
structure S {
    forall v in [1, 2, 3]: constraint v > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let labels: Vec<String> = template
        .constraints
        .iter()
        .filter_map(|c| c.label.clone())
        .filter(|l| l.starts_with("forall@v["))
        .collect();

    assert_eq!(
        labels,
        vec!["forall@v[0]".to_string(), "forall@v[1]".to_string(), "forall@v[2]".to_string()],
        "expected exact labels forall@v[0..3], got {:?}",
        labels
    );

    // Sanity: no two labels should be equal (uniqueness — also implied
    // by the assert above, but pin explicitly so a future label-reuse
    // bug fails this assertion specifically).
    let unique_count = labels.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(
        unique_count,
        labels.len(),
        "all forall@v[*] labels must be unique, got {:?}",
        labels
    );
}

/// `forall v in vents: ...` over a collection sub *without* a count
/// constraint should emit zero CompiledConstraints and zero errors. Pins
/// PRD criterion 7's "no decls when count is undef" half — at compile
/// time we cannot statically resolve the count, so we defer to a future
/// SchemaNode-style abstraction (out of scope for task 2364).
///
/// TODO(future): once SchemaNode-style re-elaboration is in place, this
/// test should be updated to assert that the constraints are emitted
/// once the count becomes known at graph-build time. For now we only
/// pin the silent-skip half of the criterion.
#[test]
fn forall_constraint_over_undef_count_collection_sub_emits_no_decls_no_error() {
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    forall v in vents: constraint v.mass < 50kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for undef-count forall, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();

    assert_eq!(
        forall_constraints_count, 0,
        "expected zero forall@* constraints when count is undef, got {}",
        forall_constraints_count
    );
}

/// `forall v in [1, 2, 3]: constraint v > 0 where heavy` should emit one
/// `CompiledGuardedGroup` per element (3 total), each containing exactly
/// one `forall@v[*]`-labelled constraint and each whose `guard_expr` is
/// a `ValueRef` resolving to the `heavy` value cell. Pins PRD criterion 9:
/// per-element where-clause routing through the existing
/// `compile_per_decl_constraint_guard` helper. The where-condition
/// `heavy` does not reference the bound var, so substitution is a no-op
/// on the condition AST — but the routing logic must still produce one
/// guarded group per element so per-element diagnostics keep their
/// element-index provenance.
#[test]
fn forall_constraint_with_body_where_clause_emits_per_element_guarded_groups() {
    let source = r#"
structure S {
    param heavy : Bool = true
    forall v in [1, 2, 3]: constraint v > 0 where heavy
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for forall with body where clause, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // Each guarded element produces its own single-constraint guarded group.
    assert_eq!(
        template.guarded_groups.len(),
        3,
        "expected 3 guarded groups (one per forall element), got {}: {:?}",
        template.guarded_groups.len(),
        template
            .guarded_groups
            .iter()
            .map(|g| {
                g.constraints
                    .iter()
                    .map(|c| c.label.as_deref())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    );

    // None of the forall constraints should also leak into the top-level
    // `constraints` vec — they live inside the guarded groups.
    let top_level_forall = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();
    assert_eq!(
        top_level_forall, 0,
        "guarded forall constraints must not appear in top-level constraints, got {} \
         (labels: {:?})",
        top_level_forall,
        template
            .constraints
            .iter()
            .filter_map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    for (i, group) in template.guarded_groups.iter().enumerate() {
        // Exactly one constraint per group, labelled forall@v[i].
        assert_eq!(
            group.constraints.len(),
            1,
            "expected exactly 1 constraint in guarded group {}, got {}",
            i,
            group.constraints.len()
        );
        let label = group.constraints[0].label.as_deref();
        assert!(
            label.is_some_and(|s| s.starts_with("forall@v[")),
            "expected guarded group {} constraint label to start with `forall@v[`, got {:?}",
            i,
            label
        );

        // The guard_expr should be a ValueRef pointing at S.heavy.
        match &group.guard_expr.kind {
            CompiledExprKind::ValueRef(id) => {
                assert_eq!(
                    id.entity, "S",
                    "expected guard_expr.entity == 'S' for group {}, got {}",
                    i, id.entity
                );
                assert_eq!(
                    id.member, "heavy",
                    "expected guard_expr.member == 'heavy' for group {}, got {}",
                    i, id.member
                );
            }
            other => panic!(
                "expected guard_expr to be ValueRef(S.heavy) for group {}, got {:?}",
                i, other
            ),
        }
    }
}

/// `forall v in [1, 2, 3]: constraint MinThreshold(value: v)` over a
/// single-predicate `constraint def` should emit exactly 3
/// CompiledConstraints, one per element. Each label combines the
/// existing constraint-inst provenance (`<inst_name>#<inst_idx>[<pred_idx>]`)
/// with the forall element-index suffix (`:forall@<var>[<i>]`), so a
/// per-element diagnostic can cite both the originating constraint def
/// and the element that triggered the failure. The substituted `value`
/// arg is the per-element literal — verified by walking the predicate's
/// CompiledExpr.
#[test]
fn forall_constraint_inst_body_emits_per_element_inst_predicates() {
    let source = r#"
constraint def MinThreshold {
    param value : Scalar
    value > 0
}
structure S {
    forall v in [1, 2, 3]: constraint MinThreshold(value: v)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for forall + constraint-inst body, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // Pick out the forall-emitted inst constraints. Per design decision
    // (plan §design_decisions[1]), the label format combines
    // `<name>#<inst_idx>[<pred_idx>]` with `:forall@<var>[<i>]`.
    let forall_inst: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.contains(":forall@v["))
        })
        .collect();

    assert_eq!(
        forall_inst.len(),
        3,
        "expected exactly 3 MinThreshold#*:forall@v[*] constraints, got {}: {:?}",
        forall_inst.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // Label format pin: each label must match
    // `MinThreshold#<i>[0]:forall@v[<i>]`. The single predicate of the
    // def gives pred_idx == 0; the per-element fresh instantiation gives
    // an inst_idx that increments per element (0, 1, 2).
    for (i, c) in forall_inst.iter().enumerate() {
        let expected = format!("MinThreshold#{}[0]:forall@v[{}]", i, i);
        assert_eq!(
            c.label.as_deref(),
            Some(expected.as_str()),
            "label mismatch for element {}: got {:?}",
            i,
            c.label
        );
    }

    // Body verification: each emitted constraint substitutes `value` with
    // the per-element literal. So `value > 0` becomes `<i+1> > 0` — a
    // BinOp::Gt whose left is Literal(Int(<i+1>)) and right is Literal(Int(0)).
    for (i, c) in forall_inst.iter().enumerate() {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                assert_eq!(
                    *op,
                    BinOp::Gt,
                    "expected BinOp::Gt for element {}, got {:?}",
                    i,
                    op
                );
                let expected = (i as i64) + 1;
                match &left.kind {
                    CompiledExprKind::Literal(Value::Int(n)) => assert_eq!(
                        *n, expected,
                        "expected substituted literal {} on left of element {}, got {}",
                        expected, i, n
                    ),
                    other => panic!(
                        "expected Literal(Int({})) on left of element {}, got {:?}",
                        expected, i, other
                    ),
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(0)) => {}
                    other => panic!(
                        "expected Literal(Int(0)) on right of element {}, got {:?}",
                        i, other
                    ),
                }
            }
            other => panic!(
                "expected BinOp(Gt) for element {}, got {:?}",
                i, other
            ),
        }
    }
}

/// `forall v in []: constraint v > 0` should emit zero CompiledConstraints
/// and zero errors. Pins PRD criterion 6 (empty collection produces no
/// decls, no diagnostic). The empty literal is a degenerate but legal
/// program — the forall statement is vacuously satisfied.
#[test]
fn forall_constraint_over_empty_list_literal_emits_no_decls_no_error() {
    let source = r#"
structure S {
    forall v in []: constraint v > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for empty-list forall, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();

    assert_eq!(
        forall_constraints_count, 0,
        "expected zero forall@* constraints from empty-list forall, got {}",
        forall_constraints_count
    );
}

/// `forall v in vents: connect v.inlet -> air_channel` over a 3-element
/// collection sub of a structure with a port should emit exactly 3
/// CompiledConnections, each with `left_port == "vents[<i>].inlet"`,
/// `right_port == "air_channel"`, and `span == forall_decl.span`. Pins
/// PRD criterion 8: forall over a connection statement emits one
/// connection per element, with the bound var substituted by an indexed
/// access into the collection sub. The indexed-sub-component port name
/// formatting (`vents[0].inlet`) flows through the existing dotted-port-name
/// branch in `compile_connection`, mirroring the `motor.shaft` style.
#[test]
fn forall_connect_emits_per_element_connections() {
    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : out Air { param d : Length = 5mm }
}
structure def S {
    sub vents : List<Vent>
    constraint vents.count == 3
    port air_channel : in Air { param d : Length = 5mm }
    forall v in vents: connect v.inlet -> air_channel
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for forall connect over collection sub, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    assert_eq!(
        template.connections.len(),
        3,
        "expected exactly 3 CompiledConnections (one per forall element), got {}: \
         left_ports = {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| c.left_port.as_str())
            .collect::<Vec<_>>()
    );

    // Recover the source forall span; every emitted connection must carry it.
    let forall_span = find_forall_connect_span(source, "S");

    for (i, conn) in template.connections.iter().enumerate() {
        let expected_left = format!("vents[{}].inlet", i);
        assert_eq!(
            conn.left_port, expected_left,
            "expected left_port == {:?} for element {}, got {:?}",
            expected_left, i, conn.left_port
        );
        assert_eq!(
            conn.right_port, "air_channel",
            "expected right_port == 'air_channel' for element {}, got {:?}",
            i, conn.right_port
        );
        assert_eq!(
            conn.operator,
            reify_syntax::ConnectOp::Forward,
            "expected ConnectOp::Forward for element {}, got {:?}",
            i, conn.operator
        );
        assert_eq!(
            conn.span, forall_span,
            "expected forall-emitted connection span to equal source forall span \
             for element {}, got connection.span = {:?}, forall_span = {:?}",
            i, conn.span, forall_span
        );
    }
}

/// `forall v in vents: chain v.a -> v.b -> v.c` over a 3-element collection
/// sub of a structure with 3 chain-compatible ports should desugar to
/// `count * (chain_len - 1)` = 6 CompiledConnections, in element-major
/// order: each element's pairwise chain is emitted contiguously before
/// the next element starts. Each emitted connection carries
/// `span = forall_decl.span`. Pins step-18's pairwise per-element
/// desugaring of a chain body inside a forall.
#[test]
fn forall_connect_chain_body_emits_per_element_pairwise_connections() {
    let source = r#"
trait T { param d : Length }
structure def Vent {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 1mm }
    port c : in T { param d : Length = 1mm }
}
structure def S {
    sub vents : List<Vent>
    constraint vents.count == 3
    forall v in vents: chain v.a -> v.b -> v.c
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for forall chain over collection sub, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // 3 elements × 2 windows(2) pairs per chain = 6 CompiledConnections.
    assert_eq!(
        template.connections.len(),
        6,
        "expected 6 CompiledConnections (3 elements × 2 chain pairs), got {}: \
         (left -> right) = {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );

    let forall_span = find_forall_connect_span(source, "S");

    // Element-major pairwise pattern: per-element chain pairs emitted in
    // source order, contiguously, before the next element starts.
    let expected: Vec<(String, String)> = (0..3)
        .flat_map(|i| {
            vec![
                (format!("vents[{}].a", i), format!("vents[{}].b", i)),
                (format!("vents[{}].b", i), format!("vents[{}].c", i)),
            ]
        })
        .collect();

    for (k, conn) in template.connections.iter().enumerate() {
        let (exp_l, exp_r) = &expected[k];
        assert_eq!(
            conn.left_port.as_str(),
            exp_l.as_str(),
            "connection {} left_port mismatch: expected {:?}, got {:?}",
            k,
            exp_l,
            conn.left_port
        );
        assert_eq!(
            conn.right_port.as_str(),
            exp_r.as_str(),
            "connection {} right_port mismatch: expected {:?}, got {:?}",
            k,
            exp_r,
            conn.right_port
        );
        assert_eq!(
            conn.operator,
            reify_syntax::ConnectOp::Forward,
            "expected ConnectOp::Forward for connection {}, got {:?}",
            k,
            conn.operator
        );
        assert_eq!(
            conn.span, forall_span,
            "expected forall-emitted chain connection {} span to equal source \
             forall span; got connection.span = {:?}, forall_span = {:?}",
            k, conn.span, forall_span
        );
    }
}

/// `forall v in 42: constraint v > 0` should emit exactly one error
/// diagnostic whose message contains `cannot iterate over non-collection
/// type` (matching the expression-form quantifier wording at
/// `expr.rs:1791-1799` for symmetry) and whose label span is anchored at
/// the collection sub-expression. No `forall@v[*]` constraints should be
/// emitted (anti-cascade — we don't elaborate when the collection isn't
/// iterable). Pins step-20.
#[test]
fn forall_over_non_iterable_collection_emits_diagnostic() {
    let source = r#"
structure S {
    forall v in 42: constraint v > 0
}
"#;
    let module = compile_source(source);

    // Filter for the non-iterable error specifically (the compile pipeline
    // may emit other diagnostics — info or warning — that we don't pin here).
    let non_iterable_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_types::Severity::Error
                && d.message.contains("cannot iterate over non-collection type")
        })
        .collect();
    assert_eq!(
        non_iterable_errors.len(),
        1,
        "expected exactly 1 non-iterable error, got {}: {:?}",
        non_iterable_errors.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    let err = non_iterable_errors[0];
    let label = err
        .labels
        .first()
        .expect("non-iterable diagnostic must carry a label");
    assert!(
        label.span.start < label.span.end,
        "label span must be non-empty, got {}..{}",
        label.span.start,
        label.span.end
    );

    // The label's span should sit inside the source forall declaration's
    // span (the collection sub-expression is part of the forall AST).
    let forall_span = find_forall_constraint_span(source, "S");
    assert!(
        label.span.start >= forall_span.start && label.span.end <= forall_span.end,
        "non-iterable label span must be inside the forall decl span: \
         label = {}..{}, forall = {}..{}",
        label.span.start,
        label.span.end,
        forall_span.start,
        forall_span.end
    );

    // Anti-cascade: no per-element constraints should be emitted.
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");
    let forall_constraints_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();
    assert_eq!(
        forall_constraints_count, 0,
        "expected zero forall@* constraints when collection is non-iterable, got {}",
        forall_constraints_count
    );
}

/// `forall v in vents: chain v.a -> v.b -> v.c` over a collection sub *without*
/// a count constraint should emit zero connections and zero errors. Pins PRD
/// criterion 7's "no decls when count is undef" half for the Chain form.
/// Also pins that the "chain statement requires at least two elements" diagnostic
/// is NOT emitted — the undef-count early-return in `resolve_forall_elements`
/// stops execution before the outer loop is entered, so the chain guard never
/// fires.
///
/// TODO(future): once SchemaNode-style re-elaboration is in place, update
/// this test to assert that connections are emitted when the count becomes known.
#[test]
fn forall_connect_chain_body_over_undef_count_collection_sub_emits_no_connections_no_error() {
    let source = r#"
trait T { param d : Length }
structure def Vent {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 1mm }
    port c : in T { param d : Length = 1mm }
}
structure def S {
    sub vents : List<Vent>
    forall v in vents: chain v.a -> v.b -> v.c
}
"#;
    let module = compile_source(source);

    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for undef-count forall chain, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    assert_eq!(
        template.connections.len(),
        0,
        "expected zero connections when count is undef for chain forall, got {}: {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );

    // The chain guard must not fire — the early-return before the loop
    // prevents reaching it.
    let chain_diagnostic_count = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("chain statement requires at least two elements"))
        .count();
    assert_eq!(
        chain_diagnostic_count, 0,
        "expected no chain-too-short diagnostic for undef-count forall, got {}: {:?}",
        chain_diagnostic_count,
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// `forall v in []: chain v.a -> v.b -> v.c` should emit zero connections and
/// zero errors. The critical pin: the chain-body branch checks
/// `cd.elements.len() < 2` INSIDE the per-element outer loop, so an empty
/// outer iteration must NOT spuriously fire the "chain statement requires at
/// least two elements" diagnostic. This test guards against a future refactor
/// that hoists the guard outside the loop (which would fire once for the
/// empty-list case). Pins PRD criterion 6 for the Chain form.
#[test]
fn forall_connect_chain_body_over_empty_list_literal_emits_no_connections_no_diagnostic() {
    let source = r#"
trait T { param d : Length }
structure def Vent {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 1mm }
    port c : in T { param d : Length = 1mm }
}
structure def S {
    forall v in []: chain v.a -> v.b -> v.c
}
"#;
    let module = compile_source(source);

    // No errors at all.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for empty-list forall chain, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    assert_eq!(
        template.connections.len(),
        0,
        "expected zero connections from empty-list forall chain, got {}: {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );

    // Critical pin: "chain statement requires at least two elements" must NOT
    // appear. If the guard is ever hoisted outside the per-element loop it
    // would fire here and break this assertion.
    let chain_diagnostic_count = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("chain statement requires at least two elements"))
        .count();
    assert_eq!(
        chain_diagnostic_count, 0,
        "expected no chain-too-short diagnostic for empty-list forall, got {}: {:?}",
        chain_diagnostic_count,
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Confirm source parses as ForallConnect.
    let _forall_span = find_forall_connect_span(source, "S");
}

/// `forall v in vents: constraint v.mass < 50kg` over a collection sub whose
/// count constraint is explicitly `vents.count == 0` should emit zero
/// CompiledConstraints with forall@v[*] labels and zero errors. Pins PRD
/// criterion 6 for the count-cell-as-zero path: the `(0..0).map(...).collect()`
/// in `resolve_forall_elements` produces an empty Vec, which the caller iterates
/// zero times — identical to the `ListLiteral([])` path but exercised via the
/// count-cell code path instead. Distinct from
/// `forall_constraint_over_empty_list_literal_emits_no_decls_no_error`.
#[test]
fn forall_constraint_over_zero_count_collection_sub_emits_no_decls_no_error() {
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    constraint vents.count == 0
    forall v in vents: constraint v.mass < 50kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for zero-count collection sub forall, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // No forall-emitted constraints.
    let forall_constraint_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .count();
    assert_eq!(
        forall_constraint_count, 0,
        "expected zero forall@v[*] constraints for zero-count sub, got {}",
        forall_constraint_count
    );

    // All remaining constraints are non-forall (the vents.count == 0
    // constraint itself, possibly others). None should carry a forall@* label.
    let any_forall_label = template
        .constraints
        .iter()
        .any(|c| c.label.as_deref().is_some_and(|s| s.starts_with("forall@")));
    assert!(
        !any_forall_label,
        "expected no forall@* labels at all for zero-count sub, got: {:?}",
        template
            .constraints
            .iter()
            .filter_map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );
}

/// `forall v in vents: connect v.inlet -> air_channel` over a collection sub
/// *without* a count constraint should emit zero CompiledConnections and zero
/// errors. Pins PRD criterion 7's "no decls when count is undef" half for the
/// Connect form. At compile time we cannot resolve the count of `vents`, so
/// elaboration is deferred silently.
///
/// TODO(future): once SchemaNode-style re-elaboration is in place, this
/// test should be updated to assert that the connections are emitted once
/// the count becomes known at graph-build time.
#[test]
fn forall_connect_over_undef_count_collection_sub_emits_no_connections_no_error() {
    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : out Air { param d : Length = 5mm }
}
structure def S {
    sub vents : List<Vent>
    port air_channel : in Air { param d : Length = 5mm }
    forall v in vents: connect v.inlet -> air_channel
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for undef-count forall connect, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    assert_eq!(
        template.connections.len(),
        0,
        "expected zero connections when count is undef, got {}: {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );

    // No forall@v[*] labels in constraints either.
    let forall_label_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .count();
    assert_eq!(
        forall_label_count, 0,
        "expected zero forall@v[*] constraint labels from undef-count forall connect, got {}",
        forall_label_count
    );
}

/// `forall v in []: constraint MinThreshold(value: v)` should emit zero
/// CompiledConstraints and zero errors. Pins PRD criterion 6 for the
/// Instantiation body shape: `expand_constraint_inst` is called INSIDE the
/// per-element loop, so an empty collection never invokes it and no inst_idx
/// is allocated. If a future refactor pre-allocates inst_idx outside the loop
/// the total constraint count will become non-zero and this test will catch it.
#[test]
fn forall_constraint_inst_body_over_empty_list_literal_emits_no_decls_no_error() {
    let source = r#"
constraint def MinThreshold {
    param value : Scalar
    value > 0
}
structure S {
    forall v in []: constraint MinThreshold(value: v)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for empty-list forall constraint-inst, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // Zero total constraints — the only constraints that could exist come from
    // the forall, and the empty list means none are emitted.
    assert_eq!(
        template.constraints.len(),
        0,
        "expected zero constraints from empty-list forall constraint-inst, got {}: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| c.label.as_deref())
            .collect::<Vec<_>>()
    );

    // No MinThreshold#*:forall@v[*] labels exist.
    let inst_forall_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.contains(":forall@v["))
        })
        .count();
    assert_eq!(
        inst_forall_count, 0,
        "expected zero MinThreshold#*:forall@v[*] constraints, got {}",
        inst_forall_count
    );

    // Confirm the source parses as ForallConstraint.
    let _forall_span = find_forall_constraint_span(source, "S");
}

/// `forall v in []: connect v.inlet -> hub` should emit zero CompiledConnections
/// and zero errors. Pins PRD criterion 6 for the Connect form: an empty
/// collection is vacuously satisfied — no connections are emitted, no
/// diagnostic is produced. Also confirms via re-parse that the source
/// actually contains a `MemberDecl::ForallConnect` so a parser regression
/// that silently demotes the body does not false-positive this test.
#[test]
fn forall_connect_over_empty_list_literal_emits_no_connections_no_error() {
    let source = r#"
trait T { param d : Length }
structure def Vent {
    port inlet : out T { param d : Length = 1mm }
}
structure def S {
    port hub : in T { param d : Length = 1mm }
    forall v in []: connect v.inlet -> hub
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for empty-list forall connect, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    assert_eq!(
        template.connections.len(),
        0,
        "expected zero connections from empty-list forall connect, got {}: {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );

    // No forall@v[*] labels in constraints either (anti-cascade check).
    let forall_label_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .count();
    assert_eq!(
        forall_label_count, 0,
        "expected zero forall@v[*] constraint labels from empty-list forall, got {}",
        forall_label_count
    );

    // Confirm the source actually contains a ForallConnect (so a parse-shape
    // regression that silently demotes to a different MemberDecl variant
    // doesn't make this test vacuously green).
    let _forall_span = find_forall_connect_span(source, "S");
}

/// Every per-element `CompiledConstraint` emitted by a statement-form forall
/// must carry a `span` whose byte range equals the source
/// `MemberDecl::ForallConstraint(f).span`. Together with the
/// `forall@<var>[<idx>]` label format pinned by
/// `forall_constraint_label_encodes_element_index`, the span and label
/// satisfy the PRD's "span info pointing back to the forall plus the
/// element index" criterion: the span identifies the source forall site for
/// diagnostic anchoring; the label disambiguates per-element provenance.
///
/// Pins step-21: covers the "span" half of the dual-piece criterion. Uses a
/// 3-element collection-sub forall (rather than a list literal) so the test
/// also exercises the indexed-substitution path — the substituted body's
/// expression spans differ per element, but the emitted constraint span
/// must remain anchored at the forall declaration. Also pins that the
/// recovered span is non-empty and not the prelude sentinel (i.e. it
/// identifies a real range in user source).
#[test]
fn forall_constraint_decl_span_anchors_at_forall_source() {
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    constraint vents.count == 3
    forall v in vents: constraint v.mass < 50kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for span-anchoring test, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    let forall_constraints: Vec<_> = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@v["))
        })
        .collect();

    assert_eq!(
        forall_constraints.len(),
        3,
        "expected exactly 3 forall@v[*] constraints, got {}",
        forall_constraints.len()
    );

    // Recover the source forall span via re-parse. Pin the structural
    // properties first so a regression in `f.span` itself (e.g. an unset
    // span defaulting to the prelude sentinel) is reported clearly rather
    // than as an opaque equality mismatch.
    let forall_span = find_forall_constraint_span(source, "S");
    assert!(
        !forall_span.is_empty(),
        "ForallConstraint source span must be non-empty, got {}..{}",
        forall_span.start,
        forall_span.end
    );
    assert!(
        !forall_span.is_prelude(),
        "ForallConstraint source span must not be the prelude sentinel, got {}..{}",
        forall_span.start,
        forall_span.end
    );

    // Every emitted per-element constraint's span must equal the source
    // forall span exactly (byte range identical). This is the diagnostic
    // anchor — a per-element failure points back to the user-visible forall
    // site. The element index travels via the label (pinned separately).
    for c in &forall_constraints {
        assert_eq!(
            c.span, forall_span,
            "forall-emitted constraint span must equal the source forall span; \
             label = {:?}, span = {}..{}, expected = {}..{}",
            c.label, c.span.start, c.span.end, forall_span.start, forall_span.end
        );
    }
}
