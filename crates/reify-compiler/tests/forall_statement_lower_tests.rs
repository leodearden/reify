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
        if let reify_syntax::Declaration::Structure(s) = decl
            && s.name == structure_name
        {
            for m in &s.members {
                if let reify_syntax::MemberDecl::ForallConstraint(f) = m {
                    return f.span;
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
        if let reify_syntax::Declaration::Structure(s) = decl
            && s.name == structure_name
        {
            for m in &s.members {
                if let reify_syntax::MemberDecl::ForallConnect(f) = m {
                    return f.span;
                }
            }
        }
    }
    panic!("no ForallConnect found in structure {}", structure_name);
}

/// Assert that `template` has zero connections and zero `forall@`-prefixed
/// constraint labels. Covers the "empty-collection / undef-count → zero
/// emissions" invariant shared by Connect-form forall tests (PRD criteria 6
/// and 7 first-half). The prefix `forall@` (without the bound-variable name)
/// future-proofs the helper against callers that use bound variable names
/// other than `v`, while remaining a strict superset of the previous
/// `forall@v[` filter for all current callers.
fn assert_no_forall_connect_emissions(template: &reify_compiler::TopologyTemplate) {
    assert_eq!(
        template.connections.len(),
        0,
        "expected zero connections (empty/undef-count forall), got {}: {:?}",
        template.connections.len(),
        template
            .connections
            .iter()
            .map(|c| (c.left_port.as_str(), c.right_port.as_str()))
            .collect::<Vec<_>>()
    );
    let forall_label_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@"))
        })
        .count();
    assert_eq!(
        forall_label_count,
        0,
        "expected zero forall@* constraint labels (empty/undef-count forall), got {}",
        forall_label_count
    );
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

/// `forall v in vents: ...` over a collection sub whose count cell exists
/// but is not statically resolvable (count constraint references an
/// undefaulted Int param) emits zero per-element CompiledConstraints AND
/// populates a `CompiledForallTemplate` capturing the deferred body so the
/// runtime re-elaboration phase can emit per-element constraints once the
/// count becomes known. Pins PRD criterion 7's compile-time half.
///
/// The runtime re-elaboration *constraint-arm* is implemented in task 2629;
/// see `crates/reify-eval/tests/forall_runtime_re_elaboration.rs::
/// edit_param_count_undef_to_known_emits_per_element_forall_constraints`
/// for the post-edit_param coverage. The runtime *connect-arm* is tracked
/// in follow-up task **2690** (depends on 2629; needs
/// `EvaluationGraph::connections` plumbing + connect-body re-emission).
#[test]
fn forall_constraint_over_undef_count_collection_sub_emits_no_decls_no_error() {
    // Fixture pins the deferred (count-cell-exists-but-undef) path: `n`
    // has no default, `constraint vents.count == n` synthesises the
    // `__count_vents` count cell, which is initially Undef so the
    // `Deferred` branch in `resolve_forall_elements` is taken (not the
    // `Skip` branch — which fires only when the collection sub has no
    // count cell at all).
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
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

    // task 2629 step-18: pin that the deferred forall body IS captured into
    // `template.forall_templates` even though zero `forall@*` CompiledConstraints
    // are emitted at compile time. The runtime re-elaboration block in
    // `engine_edit.rs` reads this Vec to drive per-element emissions on
    // count-becomes-known transitions.
    assert_eq!(
        template.forall_templates.len(),
        1,
        "expected exactly one CompiledForallTemplate captured for the deferred forall, got {}",
        template.forall_templates.len()
    );
}

/// task-2629 step-3: when `forall v in <coll_sub>` is compiled over a
/// collection sub whose count is undef, the compiler produces zero per-element
/// constraints (preserves PRD criterion 7 first-half) AND populates a
/// `CompiledForallTemplate` capturing the per-element body so the runtime
/// re-elaboration phase (engine_edit) can emit per-element constraints once
/// the count becomes known.
///
/// Pins:
/// (a) zero `forall@*`-labelled constraints in `template.constraints`;
/// (b) `template.forall_templates.len() == 1`;
/// (c) the entry's metadata: `variable == "v"`, `parent_entity == "S"`,
///     `collection_sub_name == "vents"`, `count_cell == ValueCellId::new("S","__count_vents")`;
/// (d) the entry's `body` is `CompiledForallBody::Constraint` whose
///     `body_expr` references `S.vents[0].mass` (the canonical placeholder
///     element, rewritten at runtime to `S.vents[i].mass` per emission).
///
/// The fixture uses `param n: Int` with no default + `constraint vents.count == n`
/// so the synthesized `__count_vents` cell exists but evaluates to undef
/// (matching the runtime entry-point in `engine_edit::edit_param`'s
/// collection-count phase).
///
/// RED before step-4 (types do not yet exist) and step-5 (capture path
/// not yet wired through `forall_elaborate.rs`).
#[test]
fn compile_time_forall_template_populated_for_undef_count_constraint() {
    use reify_compiler::CompiledForallBody;
    use reify_types::ValueCellId;

    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
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

    // (a) Zero `forall@*` constraints in this template — the compile-time
    //     silent-skip half of PRD criterion 7 is preserved.
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

    // (b) Exactly one captured forall template.
    assert_eq!(
        template.forall_templates.len(),
        1,
        "expected exactly 1 forall template, got {}",
        template.forall_templates.len()
    );

    let ft = &template.forall_templates[0];

    // (c) Metadata.
    assert_eq!(ft.variable, "v");
    assert_eq!(ft.parent_entity, "S");
    assert_eq!(ft.collection_sub_name, "vents");
    assert_eq!(ft.count_cell, ValueCellId::new("S", "__count_vents"));

    // (d) Body shape: Constraint with body_expr referencing S.vents[0].mass.
    // Task 2690 added a sibling `Connect` variant to `CompiledForallBody`,
    // so this pattern is no longer irrefutable; an explicit `else` branch
    // panics so the wrong discriminant surfaces a clear failure.
    let CompiledForallBody::Constraint { body_expr } = &ft.body else {
        panic!(
            "expected CompiledForallBody::Constraint variant, got {:?}",
            &ft.body
        );
    };
    // The body should reference S.vents[0].mass somewhere in its expression
    // tree. Walk the expr and confirm.
    let mut found_vent_mass = false;
    body_expr.walk(&mut |node| {
        if let CompiledExprKind::ValueRef(id) = &node.kind
            && id.entity == "S.vents[0]"
            && id.member == "mass"
        {
            found_vent_mass = true;
        }
    });
    assert!(
        found_vent_mass,
        "body_expr must reference S.vents[0].mass; got {:?}",
        body_expr.kind
    );
}

/// task 2629 step-23: `forall v in <coll_sub>: constraint <body> where <cond>`
/// over a deferred-count collection sub must NOT capture a runtime template
/// (the runtime engine has no guarded-group plumbing for per-element where
/// clauses), and must emit a `Diagnostic::info` flagging the limitation so
/// it is discoverable.
///
/// The reviewer's preferred Option (a) treatment: deferred + where-clause is
/// in the same "future scope" bucket as Instantiation and Chain.
///
/// Pins:
/// (a) `errors_only(&module).is_empty()` — info diagnostics are not errors.
/// (b) Zero `forall@*`-labelled CompiledConstraints in `template.constraints`
///     (preserves the silent-skip-on-deferred contract).
/// (c) `template.forall_templates.is_empty()` — the where-clause case must
///     NOT push a `CompiledForallTemplate`, otherwise the runtime would
///     silently emit guard-less per-element constraints (the reviewer's
///     exact concern).
/// (d) Exactly one `Diagnostic::info` whose message contains both
///     "where-clause" and "deferred-count" — pins a stable substring so
///     future refactors can't silently regress to capture.
///
/// The fixture matches the existing
/// `forall_constraint_over_undef_count_collection_sub_emits_no_decls_no_error`
/// shape, but adds a `where v.hot` body where-clause to drive the new path.
///
/// RED before step-24: today the deferred-Constraint arm captures a template
/// with `where_expr: Some(...)` and the runtime would silently drop the
/// guard at re-emission time.
#[test]
fn forall_constraint_with_where_clause_over_undef_count_collection_sub_skips_capture_with_info_diagnostic()
 {
    use reify_types::Severity;

    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
    param hot : Bool = false
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    forall v in vents: constraint v.mass < 50kg where v.hot
}
"#;
    let module = compile_source(source);

    // (a) No errors — info diagnostics must not surface as errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for deferred-count forall with where clause, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero `forall@*`-labelled CompiledConstraints — silent-skip preserved.
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
        "expected zero forall@* constraints when count is undef and body has \
         a where-clause, got {}",
        forall_constraints_count
    );

    // (c) NO captured template — the where-clause case must not push.
    assert!(
        template.forall_templates.is_empty(),
        "expected zero CompiledForallTemplates for deferred-count forall \
         with where-clause (would drop guard at runtime), got {} entries",
        template.forall_templates.len()
    );

    // (d) Exactly one info diagnostic flagging the limitation, with a stable
    //     substring pin.
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| d.message.contains("where-clause") && d.message.contains("deferred-count"))
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 info diagnostic mentioning 'where-clause' and \
         'deferred-count' for the deferred-count forall with where clause, \
         got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );
}

/// task 2629 amendment (reviewer suggestion 1): the deferred-count
/// info-diagnostic contract for the `Instantiation` body shape — sibling
/// to the where-clause case — must also be pinned. Without this test, a
/// future refactor could accidentally start pushing a
/// `CompiledForallTemplate` for a `forall v in <coll_sub>: constraint
/// SomeDef(...)` over a deferred-count collection, and the runtime would
/// silently emit per-element constraints with the wrong `inst_idx`
/// allocation (or no inst_idx at all).
///
/// Pins:
/// (a) `errors_only(&module).is_empty()` — info diagnostics are not errors.
/// (b) Zero `forall@*`-labelled CompiledConstraints in `template.constraints`.
/// (c) `template.forall_templates.is_empty()` — the Instantiation body
///     must NOT push a runtime template (task 2690 / future scope).
/// (d) Exactly one `Diagnostic::info` mentioning the limitation, with a
///     stable substring tying it to the future-scope task.
#[test]
fn forall_constraint_inst_body_over_undef_count_collection_sub_skips_capture_with_info_diagnostic()
 {
    use reify_types::Severity;

    let source = r#"
constraint def MinThreshold {
    param value : Scalar
    value > 0
}
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    forall v in vents: constraint MinThreshold(value: v.mass)
}
"#;
    let module = compile_source(source);

    // (a) No errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for deferred-count forall inst body, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero `forall@*`-labelled CompiledConstraints — no per-element
    //     emissions at compile time for a deferred-count Instantiation body.
    let forall_constraints_count = template
        .constraints
        .iter()
        .filter(|c| {
            c.label
                .as_deref()
                .is_some_and(|s| s.starts_with("forall@") || s.contains(":forall@"))
        })
        .count();
    assert_eq!(
        forall_constraints_count, 0,
        "expected zero forall@* constraints when count is undef and body is \
         a constraint-inst, got {}",
        forall_constraints_count
    );

    // (c) NO runtime template captured.
    assert!(
        template.forall_templates.is_empty(),
        "expected zero CompiledForallTemplates for deferred-count forall \
         with constraint-inst body (Instantiation is task 2690 future scope), \
         got {} entries",
        template.forall_templates.len()
    );

    // (d) Exactly one info diagnostic with stable substring "future scope".
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| d.message.contains("future scope"))
        .filter(|d| d.message.contains("constraint-instantiation"))
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 info diagnostic mentioning 'future scope' and \
         'constraint-instantiation' for the deferred-count forall with inst \
         body, got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );
}

/// task 2629 amendment (reviewer suggestion 1): the deferred-count
/// info-diagnostic contract for the `forall v in <coll_sub>: connect ...`
/// body shape (Connect) must be pinned. The Connect arm is tracked by
/// follow-up task 2690 — no template captured at compile time, info
/// diagnostic emitted so the limitation is discoverable.
///
/// Task 2690 inverts the prior contract: the deferred Connect arm now CAPTURES
/// a runtime template (so `Engine::edit_param`'s collection-count phase can
/// re-emit per-element connections when the count becomes known) and the
/// "task 2690 future scope" info diagnostic is gone.
///
/// Pins:
/// (a) No errors.
/// (b) Zero CompiledConnections in `template.connections` (the collection
///     sub's count is still undef at compile time, so per-element emissions
///     are deferred to runtime).
/// (c) `template.forall_templates.len() == 1` — the runtime template was
///     captured.
/// (d) Captured entry's metadata: `variable == "v"`, `parent_entity == "S"`,
///     `collection_sub_name == "vents"`, and
///     `count_cell == ValueCellId::new("S","__count_vents")`.
/// (e) Captured entry's body is `CompiledForallBody::Connect` with
///     `left_port_template == "vents[0].inlet"`,
///     `right_port_template == "air_channel"`,
///     `operator == ConnectOp::Forward`,
///     `connector_type.is_none()`,
///     `params.is_empty()`,
///     `port_mappings.is_empty()`.
/// (f) The OLD info diagnostic ("task 2690 future scope" + "forall connect")
///     is GONE.
#[test]
fn forall_connect_over_undef_count_collection_sub_captures_runtime_template() {
    use reify_compiler::CompiledForallBody;
    use reify_syntax::ConnectOp;
    use reify_types::{Severity, ValueCellId};

    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : out Air { param d : Length = 5mm }
}
structure def S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    port air_channel : in Air { param d : Length = 5mm }
    forall v in vents: connect v.inlet -> air_channel
}
"#;
    let module = compile_source(source);

    // (a) No errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for deferred-count forall connect body, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero CompiledConnections at compile time — count is still deferred.
    assert_no_forall_connect_emissions(template);

    // (c) Exactly one captured runtime template.
    assert_eq!(
        template.forall_templates.len(),
        1,
        "expected exactly 1 captured CompiledForallTemplate for deferred-count \
         forall connect, got {} entries",
        template.forall_templates.len()
    );

    let ft = &template.forall_templates[0];

    // (d) Metadata.
    assert_eq!(ft.variable, "v");
    assert_eq!(ft.parent_entity, "S");
    assert_eq!(ft.collection_sub_name, "vents");
    assert_eq!(ft.count_cell, ValueCellId::new("S", "__count_vents"));

    // (e) Body shape: Connect with substituted port-name templates.
    //
    // The fixture deliberately uses the simple form (no `via T(args...)`,
    // no explicit port-mappings) so this single test pins:
    //   * `params.is_empty()`,
    //   * `port_mappings.is_empty()`,
    //   * `connector_type.is_none()`.
    // A richer fixture exercising connector params is left as future
    // coverage; the per-element substitution path is shared with the
    // resolved (non-deferred) Connect arm, which is already tested.
    match &ft.body {
        CompiledForallBody::Connect {
            left_port_template,
            operator,
            right_port_template,
            connector_type,
            params,
            port_mappings,
        } => {
            assert_eq!(
                left_port_template, "vents[0].inlet",
                "expected left port template to be 'vents[0].inlet'"
            );
            assert_eq!(
                right_port_template, "air_channel",
                "expected right port template to be 'air_channel'"
            );
            assert_eq!(
                *operator,
                ConnectOp::Forward,
                "expected operator to be ConnectOp::Forward"
            );
            assert!(
                connector_type.is_none(),
                "expected no explicit connector_type, got {:?}",
                connector_type
            );
            assert!(
                params.is_empty(),
                "expected no params for simple connect, got {:?}",
                params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
            );
            assert!(
                port_mappings.is_empty(),
                "expected no port_mappings for simple connect, got {:?}",
                port_mappings
            );
        }
        other => panic!("expected CompiledForallBody::Connect, got {:?}", other),
    }

    // (f) The OLD info diagnostic must be gone.
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| d.message.contains("future scope"))
        .filter(|d| d.message.contains("forall connect"))
        .collect();
    assert_eq!(
        info_diags.len(),
        0,
        "expected zero 'task 2690 future scope' info diagnostics for forall \
         connect (now captured), got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );
}

/// task 2690 amendment (reviewer suggestion 2): pin the connector-spec drop
/// contract for the rich-form `forall v in <coll_sub>: connect a -> b : T(p = e)`
/// body shape over a deferred-count collection.
///
/// The runtime re-emission path (`engine_edit.rs`) does not propagate the
/// `connector_type` or `params` from the captured template — only the
/// port-to-port connection is materialised. Per the amendment, the
/// deferred-capture path emits an info diagnostic surfacing this scope
/// limitation. Capture itself still proceeds: `connector_type` and
/// compiled `params` are stored on the captured template for a future task
/// to consume when connector-spec-aware runtime emission lands.
///
/// Pins:
/// (a) No errors.
/// (b) Zero CompiledConnections (deferred-count → runtime emission).
/// (c) Exactly one captured `CompiledForallTemplate`.
/// (d) Captured Connect body has `connector_type == Some("BoltSet")`,
///     a non-empty `params` Vec containing the `grade` entry.
/// (e) Exactly one `Diagnostic::info` mentioning the connector-spec
///     drop, anchored at the source forall span.
#[test]
fn forall_connect_rich_form_over_undef_count_collection_sub_emits_connector_drop_info_diagnostic() {
    use reify_compiler::CompiledForallBody;
    use reify_types::Severity;

    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : out Air { param d : Length = 5mm }
}
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    port air_channel : in Air { param d : Length = 5mm }
    forall v in vents: connect v.inlet -> air_channel : BoltSet { grade = 10.9 }
}
"#;
    let module = compile_source(source);

    // (a) No errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for rich-form deferred-count forall connect, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero CompiledConnections at compile time — count is deferred.
    assert_no_forall_connect_emissions(template);

    // (c) Exactly one captured runtime template.
    assert_eq!(
        template.forall_templates.len(),
        1,
        "expected exactly 1 captured CompiledForallTemplate for rich-form \
         deferred-count forall connect, got {}",
        template.forall_templates.len()
    );

    // (d) Captured Connect body carries the connector_type and params.
    let ft = &template.forall_templates[0];
    match &ft.body {
        CompiledForallBody::Connect {
            connector_type,
            params,
            ..
        } => {
            assert_eq!(
                connector_type.as_deref(),
                Some("BoltSet"),
                "expected captured connector_type == Some(\"BoltSet\"), got {:?}",
                connector_type
            );
            assert_eq!(
                params.len(),
                1,
                "expected exactly 1 captured param, got {}: {:?}",
                params.len(),
                params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
            );
            assert_eq!(
                params[0].0, "grade",
                "expected captured param name 'grade', got {:?}",
                params[0].0
            );
            match &params[0].1.kind {
                CompiledExprKind::Literal(Value::Real(v)) => assert!(
                    (v - 10.9).abs() < 1e-9,
                    "expected captured grade param compiled to Literal(Real(10.9)), got Real({})",
                    v
                ),
                other => panic!(
                    "expected Literal(Real(10.9)) for captured grade param, got {:?}",
                    other
                ),
            }
        }
        other => panic!(
            "expected CompiledForallBody::Connect for rich-form capture, got {:?}",
            other
        ),
    }

    // (e) Exactly one info diagnostic mentioning the connector-spec drop.
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| {
            d.message.contains("connector type and params are not propagated")
                || d.message.contains("connector spec dropped")
        })
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 info diagnostic naming the connector-spec drop \
         for rich-form deferred-count forall connect, got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // The diagnostic should be anchored at the source forall span (via
    // its label). Re-parse the source to recover the span and assert the
    // diagnostic's primary label points at it.
    let forall_span = find_forall_connect_span(source, "S");
    let diag = info_diags[0];
    let label_spans: Vec<reify_types::SourceSpan> =
        diag.labels.iter().map(|l| l.span).collect();
    assert!(
        label_spans.contains(&forall_span),
        "expected diagnostic label span to match the source forall span; \
         labels = {:?}, forall_span = {:?}",
        label_spans,
        forall_span
    );
}

/// task 2629 amendment (reviewer suggestion 1): the deferred-count
/// info-diagnostic contract for the `forall v in <coll_sub>: chain ...`
/// body shape (Chain) must be pinned. The Chain arm retains compile-time
/// silent-skip semantics; an info diagnostic flags the limitation.
///
/// Pins:
/// (a) No errors.
/// (b) Zero CompiledConnections (deferred-count → silent-skip).
/// (c) `template.forall_templates.is_empty()` — Chain body never
///     captures a runtime template.
/// (d) Exactly one `Diagnostic::info` mentioning the future-scope task.
#[test]
fn forall_chain_over_undef_count_collection_sub_skips_capture_with_info_diagnostic() {
    use reify_types::Severity;

    let source = r#"
trait T { param d : Length }
structure def Vent {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 1mm }
    port c : in T { param d : Length = 1mm }
}
structure def S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    forall v in vents: chain v.a -> v.b -> v.c
}
"#;
    let module = compile_source(source);

    // (a) No errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for deferred-count forall chain body, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero CompiledConnections — silent-skip preserved at compile time.
    assert_no_forall_connect_emissions(template);

    // (c) NO runtime template captured.
    assert!(
        template.forall_templates.is_empty(),
        "expected zero CompiledForallTemplates for deferred-count forall \
         chain (future scope), got {} entries",
        template.forall_templates.len()
    );

    // (d) Exactly one info diagnostic mentioning future scope and chain.
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| d.message.contains("future scope"))
        .filter(|d| d.message.contains("forall chain"))
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 info diagnostic mentioning 'future scope' and \
         'forall chain' for the deferred-count forall chain, got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
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

/// `forall v in vents: constraint v.mass < 100kg where v.mass > threshold`
/// over a 3-element collection sub should emit exactly 3
/// `CompiledGuardedGroup`s. Each group's guard_expr must be a
/// `BinOp::Gt` whose left is `ValueRef(S.vents[i].mass)` and whose right
/// is `ValueRef(S.threshold)` — proving that the bound variable `v` is
/// substituted inside `wc.condition` by `substitute_expr` in the
/// `ForallConstraintBody::Constraint` where-clause branch of
/// `elaborate_forall_constraint`. A regression that dropped that
/// `substitute_expr` call would leave `v.mass` unresolved and either
/// fail to compile or produce a wrong ValueRef, neither of which the
/// existing `where heavy` test (which does not reference `v`) would catch.
/// Briefing item 4 (guard composition) gap-fill.
#[test]
fn forall_constraint_body_where_clause_referencing_bound_var_substitutes_per_element() {
    let source = r#"
structure Vent {
    param mass : Scalar = 10kg
}
structure S {
    param threshold : Scalar = 0kg
    sub vents : List<Vent>
    constraint vents.count == 3
    forall v in vents: constraint v.mass < 100kg where v.mass > threshold
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for bound-var where-clause forall, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (a) Exactly 3 guarded groups — one per element.
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

    // None of the forall constraints should leak into the top-level
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
        "guarded forall constraints must not appear in top-level constraints, got {}",
        top_level_forall
    );

    for (i, group) in template.guarded_groups.iter().enumerate() {
        // (b) Each group has exactly one constraint labelled `forall@v[i]`.
        assert_eq!(
            group.constraints.len(),
            1,
            "expected exactly 1 constraint in guarded group {}, got {}",
            i,
            group.constraints.len()
        );
        let label = group.constraints[0].label.as_deref();
        let expected_label = format!("forall@v[{}]", i);
        assert_eq!(
            label,
            Some(expected_label.as_str()),
            "label mismatch for group {}: got {:?}",
            i,
            label
        );

        // (c) The guard_expr is `BinOp::Gt` with left = ValueRef(S.vents[i].mass)
        //     and right = ValueRef(S.threshold).  This pins that `v.mass` in
        //     the where-clause condition was substituted to `S.vents[i].mass`
        //     for each element index i.
        match &group.guard_expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                assert_eq!(
                    *op,
                    BinOp::Gt,
                    "expected guard_expr BinOp::Gt for group {}, got {:?}",
                    i, op
                );
                // left: v.mass → S.vents[i].mass after substitution
                match &left.kind {
                    CompiledExprKind::ValueRef(id) => {
                        assert_eq!(
                            id.entity,
                            format!("S.vents[{}]", i),
                            "expected guard_expr left.entity == 'S.vents[{}]' for group {}, got {}",
                            i, i, id.entity
                        );
                        assert_eq!(
                            id.member, "mass",
                            "expected guard_expr left.member == 'mass' for group {}, got {}",
                            i, id.member
                        );
                    }
                    other => panic!(
                        "expected guard_expr left to be ValueRef(S.vents[{}].mass) \
                         for group {}, got {:?}",
                        i, i, other
                    ),
                }
                // right: threshold → S.threshold (unaffected by bound-var substitution)
                match &right.kind {
                    CompiledExprKind::ValueRef(id) => {
                        assert_eq!(
                            id.entity, "S",
                            "expected guard_expr right.entity == 'S' for group {}, got {}",
                            i, id.entity
                        );
                        assert_eq!(
                            id.member, "threshold",
                            "expected guard_expr right.member == 'threshold' for group {}, got {}",
                            i, id.member
                        );
                    }
                    other => panic!(
                        "expected guard_expr right to be ValueRef(S.threshold) \
                         for group {}, got {:?}",
                        i, other
                    ),
                }
            }
            other => panic!(
                "expected guard_expr to be BinOp(Gt) for group {}, got {:?}",
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

    // The chain guard ("chain statement requires at least two elements") is
    // Severity::Error, so its absence is already covered by
    // errors_only(&module).is_empty() above.

    // Confirm the source actually contains a ForallConnect (panics if parse
    // demoted the body, which would make the zero-emissions check vacuously green).
    find_forall_connect_span(source, "S");
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

    // The chain guard ("chain statement requires at least two elements") is
    // Severity::Error, so its absence is already covered by
    // errors_only(&module).is_empty() above.

    // Confirm source parses as ForallConnect (panics if parse demoted the body
    // to a different MemberDecl variant, which would make the test vacuously green).
    find_forall_connect_span(source, "S");
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

    // Confirm the source actually contains a ForallConstraint (panics if parse
    // demoted the body, which would make the zero-emissions check vacuously green).
    find_forall_constraint_span(source, "S");
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

    assert_no_forall_connect_emissions(template);

    // Confirm the source actually contains a ForallConnect (panics if parse
    // demoted the body, which would make the zero-emissions check vacuously green).
    find_forall_connect_span(source, "S");
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

    // Confirm the source parses as ForallConstraint (panics if parse demoted the body).
    find_forall_constraint_span(source, "S");
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

    assert_no_forall_connect_emissions(template);

    // Confirm the source actually contains a ForallConnect (panics if parse
    // demoted the body, which would make the zero-emissions check vacuously green).
    find_forall_connect_span(source, "S");
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

/// task 2717: when `resolve_port_name` returns `None` for a substituted port
/// expression over a deferred-count collection, an `info` diagnostic is emitted
/// before the early return so the user can discover the limitation.
///
/// Fixture: `v.inner.a` (3-level dotted access) substitutes to
/// `MemberAccess { object: MemberAccess { ... }, member: "a" }` — the outer
/// `MemberAccess.object` is itself a `MemberAccess`, which `resolve_port_name`
/// doesn't understand → returns `None`.
///
/// Pins:
/// (a) No errors (deferred path bypasses `compile_connection`).
/// (b) Zero `CompiledConnections` and zero `forall@*` constraint labels.
/// (c) Zero `CompiledForallTemplates` — early return preserves no-capture semantics.
/// (d) Exactly one `Severity::Info` diagnostic whose message contains both
///     `"port shape"` and `"task 2690 future scope"`.
/// (e) The diagnostic's primary label span equals `find_forall_connect_span(source, "S")`.
#[test]
fn forall_connect_over_undef_count_collection_sub_unsupported_port_shape_emits_info_diagnostic() {
    use reify_types::Severity;

    let source = r#"
trait Air { param d : Length }
structure def Inner {
    port a : out Air { param d : Length = 5mm }
}
structure def Vent {
    sub inner : Inner
}
structure def S {
    sub vents : List<Vent>
    param n : Int
    constraint vents.count == n
    port air_channel : in Air { param d : Length = 5mm }
    forall v in vents: connect v.inner.a -> air_channel
}
"#;
    let module = compile_source(source);

    // (a) No errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for deferred-count forall connect with unsupported \
         port shape, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S not found");

    // (b) Zero CompiledConnections and zero forall@* constraint labels.
    assert_no_forall_connect_emissions(template);

    // (c) Zero captured runtime templates — early return preserves no-capture semantics.
    assert!(
        template.forall_templates.is_empty(),
        "expected zero CompiledForallTemplates for deferred-count forall connect \
         with unsupported port shape (early return), got {} entries",
        template.forall_templates.len()
    );

    // (d) Exactly one info diagnostic whose message contains "port shape" and
    //     "task 2690 future scope".
    let info_diags: Vec<&reify_types::Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .filter(|d| d.message.contains("port shape") && d.message.contains("task 2690 future scope"))
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 info diagnostic naming the unsupported port shape, \
         got {}: {:?}",
        info_diags.len(),
        module
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // (e) Diagnostic label span matches the source forall span.
    let forall_span = find_forall_connect_span(source, "S");
    let diag = info_diags[0];
    let label_spans: Vec<reify_types::SourceSpan> =
        diag.labels.iter().map(|l| l.span).collect();
    assert!(
        label_spans.contains(&forall_span),
        "expected diagnostic label span to match the source forall span; \
         labels = {:?}, forall_span = {:?}",
        label_spans,
        forall_span
    );
}

/// `forall v in vents: connect v.inlet <-> air_channel` over a 3-element
/// `bidi`-ported collection sub should emit exactly 3 `CompiledConnection`s,
/// each with `operator == ConnectOp::Bidirectional`. Pins that `cd.operator`
/// is threaded through `elaborate_forall_connect` into the per-element
/// `compile_connection` call rather than being dropped or hardcoded to
/// `Forward`. The existing `forall_connect_emits_per_element_connections`
/// test only exercises `ConnectOp::Forward`; a regression that hardcoded
/// `Forward` for forall connect would be invisible to it. Briefing item 3
/// (connect-form lowering non-Forward operator) gap-fill.
#[test]
fn forall_connect_with_bidirectional_operator_emits_per_element_bidi_connections() {
    let source = r#"
trait Air { param d : Length }
structure def Vent {
    port inlet : bidi Air { param d : Length = 5mm }
}
structure def S {
    sub vents : List<Vent>
    constraint vents.count == 3
    port air_channel : bidi Air { param d : Length = 5mm }
    forall v in vents: connect v.inlet <-> air_channel
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for forall bidi connect over collection sub, got: {:?}",
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
        // The critical assertion: operator must be Bidirectional, not Forward.
        assert_eq!(
            conn.operator,
            reify_syntax::ConnectOp::Bidirectional,
            "expected ConnectOp::Bidirectional for element {}, got {:?}",
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
