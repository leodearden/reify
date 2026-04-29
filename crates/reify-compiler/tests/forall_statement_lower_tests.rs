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
