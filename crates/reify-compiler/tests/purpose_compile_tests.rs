//! Purpose compilation tests.
//!
//! Tests for compiling purpose declarations into CompiledPurpose entries.

use reify_ir::*;
use reify_compiler::*;
use reify_test_support::parse_and_compile;
use reify_core::*;

// ── Step 9: basic purpose compilation ───────────────────────────

#[test]
fn compile_basic_purpose() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 80mm > 0mm
}
"#;

    let module = parse_and_compile(source);

    // Should have 1 template (Bracket) and 1 compiled purpose
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );

    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "mfg_ready");
    assert!(!purpose.is_pub);
    assert_eq!(purpose.params.len(), 1);
    assert_eq!(purpose.params[0].name, "subject");
    assert_eq!(purpose.params[0].entity_kind, "Structure");
    assert_eq!(purpose.constraints.len(), 1);
    assert!(purpose.objective.is_none());
}

// ── Step 11: reflective schema query subject.params ───────────────────────────

#[test]
fn compile_purpose_with_reflective_params_query() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
    param height : Length = 60mm
    let area = width * height
    constraint width > 0mm
}

purpose check_params(subject : Widget) {
    constraint 1 > 0
}
"#;

    let module = parse_and_compile(source);

    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "check_params");
    assert_eq!(purpose.params[0].entity_kind, "Widget");

    // The reflective query subject.params should resolve to the list of
    // param ValueCellIds from the Widget template: ["width", "height"]
    // (not "area" which is a let, not a param).
    // task-4137: compile_purpose now ALSO emits geometric_params (width+height
    // are Length-typed), so resolved_queries.len() >= 1; locate by query_kind.
    let query = purpose
        .resolved_queries
        .iter()
        .find(|q| q.query_kind == "params" && q.param_name == "subject")
        .expect("expected a ResolvedSchemaQuery with query_kind='params' and param_name='subject'");
    assert_eq!(query.param_name, "subject");
    assert_eq!(query.query_kind, "params");
    assert_eq!(query.resolved_ids.len(), 2);
    // Should contain width and height ValueCellIds
    let id_names: Vec<&str> = query
        .resolved_ids
        .iter()
        .map(|id: &ValueCellId| id.member.as_str())
        .collect();
    assert!(id_names.contains(&"width"), "should contain width");
    assert!(id_names.contains(&"height"), "should contain height");
}

// ── Step 19: compile_module helper should catch compile errors ───────────────

#[test]
#[should_panic(expected = "compile errors")]
fn compile_module_rejects_purpose_with_unknown_identifier() {
    // The compile_module helper should fail when a purpose references
    // an unknown identifier. Without diagnostic checking, this silently passes.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose broken(subject : Structure) {
    constraint nonexistent_var > 0mm
}
"#;

    let _module = parse_and_compile(source);
}

// ── Step 23 / task 4009 δ: let bindings in purposes compile + storage ────────

/// (a) Single let: compiles with no error, lets populated, name/cell_id/expr correct.
#[test]
fn purpose_let_storage_single_let() {
    let source = r#"
structure Widget {
    param a : Length = 80mm
    param b : Length = 50mm
}

purpose marg(subject : Widget) {
    let m = subject.a - subject.b
    constraint m > 0mm
}
"#;

    let module = compile_module_with_diagnostics(source);

    // No PurposeLetUnsupported error
    let unsupported: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::PurposeLetUnsupported))
        .collect();
    assert!(
        unsupported.is_empty(),
        "expected no PurposeLetUnsupported diagnostics, got: {:?}",
        unsupported
    );

    // No Severity::Error diagnostics
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics, got: {:?}",
        errors
    );

    let purpose = module
        .compiled_purposes
        .iter()
        .find(|p| p.name == "marg")
        .expect("expected purpose 'marg'");

    assert_eq!(purpose.lets.len(), 1, "expected 1 let binding");
    assert_eq!(purpose.lets[0].name, "m");
    assert_eq!(
        purpose.lets[0].cell_id,
        ValueCellId::new("marg", "m"),
        "cell_id should be {{entity:marg, member:m}}"
    );

    // expr should be BinOp(Sub, ...) for `subject.a - subject.b`
    match &purpose.lets[0].expr.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(
                *op,
                BinOp::Sub,
                "expected BinOp::Sub for 'subject.a - subject.b'"
            );
        }
        other => panic!("expected BinOp for let expr, got {:?}", other),
    }
}

/// (b) Multi-let ordering: lets.len()==2, lets[1].expr references ValueCellId::new("marg","m").
#[test]
fn purpose_let_multi_let_earlier_let_visibility() {
    let source = r#"
structure Widget {
    param a : Length = 80mm
    param b : Length = 50mm
}

purpose marg(subject : Widget) {
    let m = subject.a - subject.b
    let n = m * 2
    constraint n > 0mm
}
"#;

    let module = compile_module_with_diagnostics(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics, got: {:?}",
        errors
    );

    let purpose = module
        .compiled_purposes
        .iter()
        .find(|p| p.name == "marg")
        .expect("expected purpose 'marg'");

    assert_eq!(purpose.lets.len(), 2, "expected 2 let bindings");
    assert_eq!(purpose.lets[0].name, "m");
    assert_eq!(purpose.lets[1].name, "n");

    // lets[1].expr should reference ValueCellId::new("marg","m")
    let earlier_let_id = ValueCellId::new("marg", "m");
    let n_expr = &purpose.lets[1].expr;
    assert!(
        purpose_let_expr_contains_value_ref(n_expr, &earlier_let_id),
        "expected lets[1].expr to contain ValueRef(marg.m) from earlier let, got {:?}",
        n_expr
    );
}

/// (c) Forward-ref still rejected: `let p = q + 1mm  let q = subject.a`
///     → q is unknown when p compiles → compile error.
#[test]
#[should_panic(expected = "compile errors")]
fn purpose_let_forward_ref_rejected() {
    let source = r#"
structure Widget {
    param a : Length = 80mm
}

purpose marg(subject : Widget) {
    let p = q + 1mm
    let q = subject.a
    constraint p > 0mm
}
"#;
    let _module = parse_and_compile(source);
}

/// Helper: recursively check if a `CompiledExpr` contains a `ValueRef` pointing at `id`.
fn purpose_let_expr_contains_value_ref(expr: &CompiledExpr, id: &ValueCellId) -> bool {
    match &expr.kind {
        CompiledExprKind::ValueRef(ref_id) => ref_id == id,
        CompiledExprKind::BinOp { left, right, .. } => {
            purpose_let_expr_contains_value_ref(left, id)
                || purpose_let_expr_contains_value_ref(right, id)
        }
        CompiledExprKind::UnOp { operand, .. } => {
            purpose_let_expr_contains_value_ref(operand, id)
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            purpose_let_expr_contains_value_ref(condition, id)
                || purpose_let_expr_contains_value_ref(then_branch, id)
                || purpose_let_expr_contains_value_ref(else_branch, id)
        }
        _ => false,
    }
}

// ── Step 25: unsupported member variants should emit error ───────────────

/// Helper: parse source and compile, returning the CompiledModule without
/// asserting on compile errors. Used to inspect diagnostics directly.
fn compile_module_with_diagnostics(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: extract the SI scalar values from both sides of a BinOp expression.
///
/// Panics with a descriptive message if the expression is not a
/// `BinOp { left: Literal(Scalar), right: Literal(Scalar) }`.
/// Returns `(left_si_value, right_si_value)`.
fn extract_binop_scalar_sides(expr: &CompiledExpr) -> (f64, f64) {
    if let CompiledExprKind::BinOp { left, right, .. } = &expr.kind {
        let left_val = if let CompiledExprKind::Literal(Value::Scalar { si_value, .. }) = &left.kind
        {
            *si_value
        } else {
            panic!(
                "expected Scalar literal for left side of BinOp, got {:?}",
                left.kind
            )
        };
        let right_val =
            if let CompiledExprKind::Literal(Value::Scalar { si_value, .. }) = &right.kind {
                *si_value
            } else {
                panic!(
                    "expected Scalar literal for right side of BinOp, got {:?}",
                    right.kind
                )
            };
        (left_val, right_val)
    } else {
        panic!("expected BinOp constraint expression, got {:?}", expr.kind)
    }
}


#[test]
fn compile_purpose_no_false_positives_from_explicit_arms() {
    // Verify that a valid purpose with only constraints compiles cleanly
    // (no false positives from the explicit error arms added in step 26).
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose ok(subject : Structure) {
    constraint 80mm > 0mm
}
"#;

    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no compile errors for valid purpose, got: {:?}",
        errors
    );
}

// ── Step 1 (task-416): purpose constraint with user-declared unit ─────────────

#[test]
fn purpose_constraint_with_user_declared_unit() {
    // 'thou' (thousandth of an inch) is NOT in the hardcoded unit_to_scalar
    // table, so resolving it requires the unit registry to be threaded into
    // the purpose scope via scope.set_unit_registry().  Without the fix in
    // traits.rs (compile_purpose calls scope.set_unit_registry()), 'thou'
    // would silently fail to resolve and emit an "unknown unit" error.
    let source = r#"
unit thou : Length = 0.0000254

structure Part {
    param diameter : Length = 500thou
}

purpose machining_tolerance(subject : Structure) {
    constraint 1thou > 0mm
}
"#;

    let module = parse_and_compile(source);
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );

    // Verify 'thou' resolved to the correct SI value (≈0.0000254 m).
    // Without the unit registry being threaded into the purpose scope, the
    // literal would fail to resolve and we would never reach this assertion.
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");
    let constraint = &purpose.constraints[0];
    let (left_si, right_si) = extract_binop_scalar_sides(&constraint.expr);
    assert!(
        (left_si - 0.0000254).abs() < 1e-12,
        "1thou should compile to ≈0.0000254 m (SI), got {}",
        left_si
    );
    assert!(
        right_si.abs() < 1e-12,
        "0mm should compile to ≈0.0 m (SI), got {}",
        right_si
    );
}

// ── Step 3 (task-416): unknown unit in purpose constraint emits error ─────────

#[test]
fn purpose_constraint_with_unknown_unit_emits_error() {
    // A unit name that is neither in the hardcoded unit_to_scalar table nor
    // declared in the module should emit a Severity::Error diagnostic
    // containing "unknown unit".
    let source = r#"
structure Part {
    param x : Length = 1mm
}

purpose check(subject : Structure) {
    constraint 1parsec > 0mm
}
"#;

    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an error for unknown unit 'parsec', but got none"
    );
    let has_unknown_unit = errors.iter().any(|d| d.message.contains("unknown unit"));
    assert!(
        has_unknown_unit,
        "expected 'unknown unit' in error message, got: {:?}",
        errors
    );
}

// ── Step 5 (task-416): affine user-declared unit in purpose applies offset ────

#[test]
fn purpose_constraint_with_affine_unit_applies_offset() {
    // Declares affine unit degC (offset 273.15) and uses '100degC' in a
    // purpose constraint.  The compiled literal must have si_value ≈ 373.15
    // (100 × 1 + 273.15), proving the offset IS applied via
    // lookup_unit_in_registry() in the purpose scope.
    let source = r#"
unit degC : Temperature = 1 offset 273.15

structure Furnace {
    param setpoint : Temperature = 25degC
}

purpose hot_enough(subject : Structure) {
    constraint 100degC > 200degC
}
"#;

    let module = parse_and_compile(source);
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");
    let constraint = &purpose.constraints[0];

    // The constraint is: 100degC > 200degC
    // Left side should be Literal(Scalar { si_value ≈ 373.15 }).
    // Right side should be Literal(Scalar { si_value ≈ 473.15 }).
    let (left_si, right_si) = extract_binop_scalar_sides(&constraint.expr);
    assert!(
        (left_si - 373.15).abs() < 1e-9,
        "100degC should compile to 373.15K, got {}",
        left_si
    );
    assert!(
        (right_si - 473.15).abs() < 1e-9,
        "200degC should compile to 473.15K, got {}",
        right_si
    );
}

// ── Step 1 (task-1717): self is not valid in purpose scope ───────────────────

/// `is_entity_scope` is false for purpose scopes — only compile_entity sets it
/// true for structures and occurrences.  Using `self.param` in a purpose
/// constraint must produce an error diagnostic ("unresolved name: self"), not
/// silently resolve as if self were in scope.
#[test]
fn self_in_purpose_body_rejected() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
}

purpose check(subject : Widget) {
    constraint self.width > 0mm
}
"#;
    let module = compile_module_with_diagnostics(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an error for `self` used in purpose scope, but got none"
    );
    let has_self_error = errors.iter().any(|d| {
        let lower = d.message.to_lowercase();
        // Error format defined in expr.rs – update if the wording changes
        lower.contains("unresolved name: self")
    });
    assert!(
        has_self_error,
        "expected an error containing 'unresolved name: self', got: {:?}",
        errors
    );
}

// ── Step 1 (task-2181, updated by task-2289): reflective aggregation compiles
// ── as a marker placeholder variant (expanded by activate_purpose at runtime)

/// Shared helper for all `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS` tests.
///
/// Compiles `forall p in part.<member>: determined(p)` in a purpose body
/// and asserts the three acceptance criteria:
/// (a) no "member access not yet supported" diagnostic,
/// (b) `collection.kind` is the marker variant
///     `CompiledExprKind::PurposeReflectiveAggregation { param_name: "part",
///     query_kind: <member> }` (task-2289),
/// (c) `collection.result_type == Type::List(Box::new(Type::Real))` —
///     the compile-time placeholder element type is unchanged; activation
///     refines it from looked-up cell types.
///
/// Using a single helper avoids duplicating ~60 lines per member name and
/// naturally extends to cover every entry in `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS`
/// (currently `params`, `geometric_params`, `material_params`).
fn assert_reflective_member_compiles_as_placeholder(member: &str) {
    let source = format!(
        r#"
structure Part {{
    param length : Length = 100mm
}}

purpose check_part(part : Structure) {{
    constraint forall p in part.{member}: determined(p)
}}
"#
    );
    let module = compile_module_with_diagnostics(&source);

    // (a) No "member access not yet supported" diagnostic
    let unsupported: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .collect();
    assert!(
        unsupported.is_empty(),
        "expected no 'member access not yet supported' diagnostics for member '{}', got: {:?}",
        member,
        unsupported
    );

    // (b) and (c): constraint is a Quantifier whose collection is the new
    // PurposeReflectiveAggregation placeholder variant with
    // result_type == Type::List(Box::new(Type::Real)).
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    let constraint = &purpose.constraints[0];
    match &constraint.expr.kind {
        CompiledExprKind::Quantifier { collection, .. } => {
            // (c) collection result_type must be List<Real> at compile time
            assert_eq!(
                collection.result_type,
                Type::List(Box::new(Type::Real)),
                "expected collection result_type to be List<Real> for member '{}', got {:?}",
                member,
                collection.result_type
            );
            // (b) collection kind must be the placeholder marker variant
            match &collection.kind {
                CompiledExprKind::PurposeReflectiveAggregation {
                    param_name,
                    query_kind,
                } => {
                    assert_eq!(
                        param_name, "part",
                        "expected param_name to be 'part' (the source-level subject \
                         identifier) for member '{}', got '{}'",
                        member, param_name
                    );
                    assert_eq!(
                        query_kind, member,
                        "expected query_kind to be '{}', got '{}'",
                        member, query_kind
                    );
                }
                other => panic!(
                    "expected PurposeReflectiveAggregation collection for member '{}', \
                     got {:?}",
                    member, other
                ),
            }
        }
        other => panic!(
            "expected Quantifier constraint expression for member '{}', got {:?}",
            member, other
        ),
    }
}

/// `subject.params` compiles to a `PurposeReflectiveAggregation` placeholder
/// (task-2289 step-7; was empty `ListLiteral` per task-2181).
///
/// RED before step-7 impl: compiler still emits empty `ListLiteral`.
#[test]
fn compile_purpose_reflective_params_compiles_as_placeholder() {
    assert_reflective_member_compiles_as_placeholder("params");
}

/// `part.geometric_params` compiles to a `PurposeReflectiveAggregation`
/// placeholder (task-2289 step-7).
///
/// RED before step-7 impl: analogous to the params test above.
#[test]
fn compile_purpose_reflective_geometric_params_compiles_as_placeholder() {
    assert_reflective_member_compiles_as_placeholder("geometric_params");
}

/// `subject.material_params` compiles to a `PurposeReflectiveAggregation`
/// placeholder (task-2289 step-7).
///
/// `material_params` is the third entry in `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS`
/// and previously had no dedicated compile-time coverage.
#[test]
fn compile_purpose_reflective_material_params_compiles_as_placeholder() {
    assert_reflective_member_compiles_as_placeholder("material_params");
}

// ── Note: the task-2181 regression guard for entity-scope StructureRef member
// access was removed by task 3540 (SIR-α). The limitation it guarded —
// `material.density` in a structure constraint erroring with "member access
// not yet supported" — has been intentionally lifted: the compile-side
// member-access path now lowers `StructureRef`/`TraitObject` projections to
// `CompiledExpr::index_access`, and the eval-side IndexAccess arm reads from
// `Value::StructureInstance.fields`. The positive case (entity-scope
// `self.<sub>.<field>` chains evaluating through cleanly) is covered by
// `crates/reify-eval/tests/structure_instance_e2e.rs::nested_compositional_construction_member_access`.

// ── Step 3 (task-2181): regular member access on StructureRef subject ────────

/// Test that `subject.mass` in a purpose body compiles to a remappable `ValueRef`.
///
/// Fixture: `purpose lightweight(subject : Structure) { constraint subject.mass > 0
///           minimize subject.mass }`.
///
/// Assertions:
/// (a) no "member access not yet supported" diagnostic is emitted;
/// (b) `constraints[0].expr` is a `BinOp(Gt, left, _)` where `left` is a
///     `ValueRef(id)` with `id.entity == "lightweight"` (purpose name, pre-remap)
///     and `id.member == "mass"`, and `left.result_type == Type::Real`;
/// (c) `objective` is `Some(Minimize(expr))` where `expr.kind` is
///     `ValueRef(id)` with `id.entity == "lightweight"` and `id.member == "mass"`.
///
/// RED: fails before step 4 because the catch-all emits "member access not yet supported".
#[test]
fn compile_purpose_regular_member_compiles_as_remappable_valueref() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose lightweight(subject : Structure) {
    constraint subject.mass > 0
    minimize subject.mass
}
"#;
    let module = compile_module_with_diagnostics(source);

    // (a) No "member access not yet supported" diagnostic
    let unsupported: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .collect();
    assert!(
        unsupported.is_empty(),
        "expected no 'member access not yet supported' diagnostics, got: {:?}",
        unsupported
    );

    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "lightweight");
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    // (b) constraint is BinOp(Gt, ValueRef(lightweight.mass : Real), _)
    let constraint = &purpose.constraints[0];
    match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(
                *op,
                BinOp::Gt,
                "expected BinOp::Gt for 'subject.mass > 0', got {:?}",
                op
            );
            // left must be ValueRef with entity == "lightweight::subject" and member == "mass"
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.entity, "lightweight::subject",
                        "ValueRef entity must equal `purpose::param` per task-2181 β stamp scheme, got {:?}",
                        id.entity
                    );
                    assert_eq!(
                        id.member, "mass",
                        "ValueRef member must be 'mass', got {:?}",
                        id.member
                    );
                    assert_eq!(
                        left.result_type,
                        Type::Real,
                        "expected result_type == Type::Real for subject.mass, got {:?}",
                        left.result_type
                    );
                }
                other => panic!(
                    "expected ValueRef for left side of constraint BinOp, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected BinOp constraint expression, got {:?}", other),
    }

    // (c) objective is Some(1-term WeightedSum Minimize(ValueRef(lightweight::subject.mass)))
    let obj = purpose
        .objective
        .as_ref()
        .expect("expected Some(ObjectiveSet) for objective");
    assert_eq!(obj.combination, ObjectiveCombination::WeightedSum);
    assert_eq!(obj.terms.len(), 1, "expected 1 term");
    let term = &obj.terms[0];
    assert_eq!(term.sense, ObjectiveSense::Minimize, "expected Minimize");
    match &term.expr.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "lightweight::subject",
                "objective ValueRef entity must equal `purpose::param` per task-2181 β stamp scheme, got {:?}",
                id.entity
            );
            assert_eq!(
                id.member, "mass",
                "objective ValueRef member must be 'mass', got {:?}",
                id.member
            );
        }
        other => panic!(
            "expected ValueRef for minimize objective expr, got {:?}",
            other
        ),
    }
}

// ── Step 5 (task-2181): m5_purpose.ri acceptance gate ────────────────────────

/// Acceptance test: `examples/m5_purpose.ri` must compile with zero Error
/// diagnostics under `compile_with_stdlib` (the "41/42 → 42/42 clean" gate).
///
/// Also asserts that exactly 3 purposes are compiled (manufacturing_ready,
/// lightweight, dimensionally_valid) as a secondary sanity check that none
/// were silently dropped.
///
/// RED: fails before step 6 when any of the five member-access sites in
/// m5_purpose.ri still hit the catch-all "member access not yet supported".
#[test]
fn m5_purpose_example_compiles_under_stdlib_with_zero_errors() {
    const M5_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m5_purpose.ri");
    let src = std::fs::read_to_string(M5_PATH)
        .expect("failed to read examples/m5_purpose.ri — check CARGO_MANIFEST_DIR resolution");

    let parsed = reify_syntax::parse(&src, ModulePath::single("m5_purpose"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in m5_purpose.ri: {:?}",
        parsed.errors
    );

    let module = reify_compiler::compile_with_stdlib(&parsed);

    // Primary acceptance gate: zero Error-severity diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling m5_purpose.ri under stdlib, got:\n{:#?}",
        errors
    );

    // Secondary check: all three user-defined purposes must be present.
    // NOTE: compile_with_stdlib merges is_pub prelude purposes (e.g. simulation_ready,
    // design_review from std.determinacy.purposes) into compiled_purposes (task-4016 ζ),
    // so the total count is >= 3. Use resilient name-based assertions instead of exact count.
    let purpose_names: Vec<&str> = module
        .compiled_purposes
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        purpose_names.contains(&"manufacturing_ready"),
        "expected 'manufacturing_ready' purpose; found: {purpose_names:?}"
    );
    assert!(
        purpose_names.contains(&"lightweight"),
        "expected 'lightweight' purpose; found: {purpose_names:?}"
    );
    assert!(
        purpose_names.contains(&"dimensionally_valid"),
        "expected 'dimensionally_valid' purpose; found: {purpose_names:?}"
    );
    assert!(
        module.compiled_purposes.len() >= 3,
        "expected at least 3 compiled purposes, got: {purpose_names:?}"
    );
}

// ── Task-2200: concrete-subject member validation ────────────────────────────

/// RED test: accessing a non-existent member on a concrete (non-wildcard) subject type
/// must produce an Error diagnostic containing "has no member" and the member name.
///
/// Source: `Widget` structure with a `mass` param; purpose accesses `subject.bogus`
/// (a member that does not exist on Widget). Today this compiles silently and
/// produces a ValueRef to a non-existent cell. After implementation it must
/// emit a `Severity::Error` diagnostic.
///
/// RED: fails before step-4 impl because the current code emits no diagnostic
/// for member access on concrete subjects and silently returns a ValueRef.
#[test]
fn compile_purpose_concrete_subject_unknown_member_errors() {
    let source = r#"
structure Widget {
    param mass : Mass = 5kg
}

purpose check(subject : Widget) {
    constraint subject.bogus > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // Must have at least one Error diagnostic mentioning "has no member" and "bogus".
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("has no member")
                && d.message.contains("bogus")
        })
        .collect();

    assert!(
        !no_member_errors.is_empty(),
        "expected a Severity::Error diagnostic containing 'has no member' and 'bogus', \
         but none was emitted.\nAll diagnostics: {:#?}",
        module.diagnostics
    );

    // Also verify the diagnostic mentions the structure name "Widget".
    let mentions_widget = no_member_errors
        .iter()
        .any(|d| d.message.contains("Widget"));
    assert!(
        mentions_widget,
        "expected the 'has no member' diagnostic to mention 'Widget', but got: {:#?}",
        no_member_errors
    );

    // Anti-cascade: `make_poison_literal` returns Type::Error, which suppresses
    // cascading type-mismatch diagnostics from the `> 0` comparison.  Pin this
    // contract by asserting the total Error count stays small.
    let all_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        all_errors.len() <= 2,
        "anti-cascade: expected ≤ 2 Error diagnostics (1 'has no member' + at most 1 other), \
         got {}: {:#?}",
        all_errors.len(),
        all_errors
    );
}

/// GREEN test: accessing an existing member on a concrete (non-wildcard) subject type
/// must NOT produce a "has no member" error and must emit a ValueRef.
///
/// Source: `Widget` structure with a `mass` param; purpose accesses `subject.mass`
/// (a member that DOES exist on Widget). The validation must pass.
///
/// Assertions:
/// (a) No "has no member" diagnostic;
/// (b) The constraint expression is BinOp(Gt, ValueRef(check.mass : Real), _).
#[test]
fn compile_purpose_concrete_subject_valid_member_compiles_cleanly() {
    let source = r#"
structure Widget {
    param mass : Mass = 5kg
}

purpose check(subject : Widget) {
    constraint subject.mass > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // (a) No "has no member" diagnostic.
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("has no member"))
        .collect();
    assert!(
        no_member_errors.is_empty(),
        "expected no 'has no member' diagnostics for valid member 'mass', got: {:#?}",
        no_member_errors
    );

    // (b) Compiled purpose exists with one constraint.
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "check");
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    // Constraint must be BinOp(Gt, ValueRef(check::subject.mass : Real), _).
    let constraint = &purpose.constraints[0];
    match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(*op, BinOp::Gt, "expected BinOp::Gt for 'subject.mass > 0'");
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.entity, "check::subject",
                        "ValueRef entity must equal `purpose::param` per task-2181 β stamp scheme, got {:?}",
                        id.entity
                    );
                    assert_eq!(
                        id.member, "mass",
                        "ValueRef member must be 'mass', got {:?}",
                        id.member
                    );
                    assert_eq!(
                        left.result_type,
                        Type::Real,
                        "result_type must be Type::Real (compile-time fallback), got {:?}",
                        left.result_type
                    );
                }
                other => panic!("expected ValueRef for left of BinOp, got {:?}", other),
            }
        }
        other => panic!("expected BinOp constraint expression, got {:?}", other),
    }
}

/// Characterization test: the generic `Structure` wildcard subject must NOT trigger
/// a "has no member" error even when a non-existent member is accessed.
///
/// This pins the documented limitation: the wildcard form binds to any structure at
/// activation time, so there is no static template against which to validate members.
///
/// Ensures no future over-reach (e.g., adding a synthetic "Structure" template to
/// the registry) silently breaks this invariant.
#[test]
fn compile_purpose_wildcard_structure_subject_bogus_member_still_silent() {
    let source = r#"
purpose check(subject : Structure) {
    constraint subject.bogus > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // The wildcard case must NOT produce "has no member" diagnostics.
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("has no member"))
        .collect();
    assert!(
        no_member_errors.is_empty(),
        "expected no 'has no member' diagnostics for wildcard Structure subject, \
         but got: {:#?}\n(Wildcard subjects have no static template to validate against.)",
        no_member_errors
    );
}

/// Characterization test: an `Occurrence` wildcard subject must NOT trigger
/// a "has no member" error even when a non-existent member is accessed.
///
/// Sibling to `compile_purpose_wildcard_structure_subject_bogus_member_still_silent`.
/// The `Occurrence` entity kind is not registered in the template registry,
/// so the registry-miss guard in `compile_expr_guarded` applies — the compiler
/// skips member validation entirely (see wildcard-path comments in
/// `compile_expr_guarded`).
///
/// No `structure Occurrence` or `occurrence def Occurrence` is declared here
/// because registering a template named "Occurrence" would defeat the test by
/// taking the member-known branch instead of the registry-miss path.
///
/// Ensures no future change (e.g., adding a stdlib `Occurrence` template)
/// silently removes this silent-fallthrough guarantee without a test failure.
#[test]
fn compile_purpose_wildcard_occurrence_subject_bogus_member_still_silent() {
    let source = r#"
purpose check(subject : Occurrence) {
    constraint subject.bogus > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // The registry-miss path must NOT produce "has no member" diagnostics.
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("has no member"))
        .collect();
    assert!(
        no_member_errors.is_empty(),
        "expected no 'has no member' diagnostics for wildcard Occurrence subject, \
         but got: {:#?}\n(Unregistered wildcard kinds fall through silently via \
         the registry-miss guard in `compile_expr_guarded`.)",
        no_member_errors
    );

    // Confirm no error-severity diagnostics — ensures compilation reached the
    // member-access path and did not bail out early (e.g., if a future change
    // makes unregistered entity kinds a hard error before member access is
    // evaluated, this assertion catches the regression rather than silently
    // passing because `no_member_errors` would still be empty).
    let error_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for wildcard Occurrence subject \
         (registry-miss path should be fully silent), but got: {:#?}",
        error_diags
    );
}

/// Characterization test: a **user-declared** `structure Structure { ... }` must
/// NOT trigger a "has no member" error when a non-existent member is accessed on a
/// purpose subject typed as `Structure`.
///
/// This pins the documented fragility from esc-2200-41 S2: the guard
/// `struct_name != "Structure"` (now `struct_name != WILDCARD_STRUCTURE_KIND`) makes
/// a user-defined template literally named `Structure` indistinguishable from the
/// language-level wildcard form — member validation is skipped entirely, even though
/// a concrete template exists in the registry.
///
/// **Positive registration precondition (esc-2213-28 S1):** Before the wildcard-guard
/// assertions, this test now positively asserts that `structure Structure` was
/// actually registered in `module.templates`.  This catches the silent-rejection
/// regression: if `Structure` ever becomes a reserved name (or the parser/typechecker
/// silently drops the declaration), `module.templates` would contain no entry for
/// `Structure`, and the wildcard guard would still suppress any "has no member"
/// diagnostic — making the test trivially green while the precondition (template
/// registered) does not hold.  The `Severity::Error` guard below catches *loud*
/// failures only; this positive assertion catches silent ones.
///
/// When a future task replaces the magic-string guard with a proper semantic
/// predicate (e.g., `entity_kind.is_wildcard()`), this test will turn RED because
/// the user-declared `Structure` template would then be validated like any other
/// concrete type.  That red is intentional: it forces a deliberate decision about
/// whether the registered template should be used for validation.
#[test]
fn compile_purpose_user_defined_structure_named_structure_bypasses_validation() {
    // A user-declared structure that happens to share the wildcard sentinel name.
    // Accessing the non-existent member `bogus` must be silent today because the
    // magic-string guard treats any `Structure`-typed subject as the wildcard form.
    // `subject.mass` references a *real* param of the declared template — it is
    // included as a real-param probe so the combined `no_member_errors` filter
    // covers both a spurious member access (`bogus`) and a legitimate one (`mass`).
    // If a future refactor makes the wildcard guard registry-aware, `bogus` would
    // start producing a `has no member` diagnostic for the right reason while
    // `mass` would remain silent because it is a legitimate member of the
    // registered template.
    let source = r#"
structure Structure {
    param mass : Mass = 1kg
}
purpose check(subject : Structure) {
    constraint subject.bogus > 0
    constraint subject.mass > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // ── Positive precondition ──────────────────────────────────────────────────
    // The user-declared `Structure` template MUST be registered.  If a future
    // change makes `Structure` a reserved name (or causes the parser/typechecker
    // to silently drop the declaration), `find_template` returns None — the
    // wildcard-guard assertions below would then pass vacuously.  This assertion
    // is the direct positive confirmation called for by esc-2213-28 S1.
    let template = find_template(&module.templates, "Structure").unwrap_or_else(|| {
        panic!(
            "expected user-declared `structure Structure` to be registered in \
             module.templates (positive precondition for the wildcard-guard \
             characterization below); got templates: {:?}",
            module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
        )
    });
    // Corroborate that the template body parsed and the `mass` param compiled —
    // guards against partial-registration regression (name present, body dropped).
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "mass"),
        "expected registered `Structure` template to contain a `mass` value \
         cell, but value_cells were: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );

    // ── Wildcard-guard characterization ───────────────────────────────────────
    // The magic-string guard must NOT produce "has no member" diagnostics even
    // though a concrete template named "Structure" is now registered.
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("has no member"))
        .collect();
    assert!(
        no_member_errors.is_empty(),
        "expected no 'has no member' diagnostics for a user-declared `structure Structure` \
         subject (magic-string guard bypasses validation for any subject typed as \
         'Structure'), but got: {:#?}\n\
         If this fails after a semantic-predicate refactor, see the test doc-comment.",
        no_member_errors
    );

    // ── Bail-out safety net (third layer) ─────────────────────────────────────
    // Guard against a false-green: if compilation bails out early (e.g., because
    // `Structure` becomes reserved or the parser short-circuits on the name), no
    // member-access code runs at all, so `no_member_errors` is trivially empty for
    // the wrong reason.  A Severity::Error diagnostic from *any* cause would mean
    // the member-access branch was never reached, making the assertions above
    // meaningless.  The positive registration assertion (above) already catches
    // silent rejection; this catches loud (Error-severity) rejection.  Mirroring
    // the defense in `compile_purpose_wildcard_occurrence_subject_bogus_member_still_silent`.
    let error_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for user-declared `structure Structure` subject \
         (compilation must reach the member-access path to make the no-member-error \
         assertion meaningful), but got: {:#?}",
        error_diags
    );
}

/// Characterization test: a sub-component name on a concrete subject must NOT
/// trigger a "has no member" diagnostic — sub_components are valid member
/// kinds even though their type resolution is not yet implemented.
///
/// Pins the robustness guarantee added in the task-2200 amendment: the
/// validation checks value_cells, ports, AND sub_components, so port/sub
/// member access is not false-positively rejected.
#[test]
fn compile_purpose_concrete_subject_sub_component_no_false_positive() {
    let source = r#"
structure Motor {
    param power : Mass = 100kg
}

structure Drone {
    sub motor = Motor()
}

purpose check(subject : Drone) {
    constraint subject.motor > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // "motor" is a sub-component of Drone — must NOT produce "has no member".
    let no_member_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("has no member"))
        .collect();
    assert!(
        no_member_errors.is_empty(),
        "expected no 'has no member' diagnostics for sub-component 'motor', \
         got: {:#?}\n(sub_components are valid member kinds; \
         type resolution for them is a separate follow-up task.)",
        no_member_errors
    );
}

// ── task-2181 β: single-param regression lock ─────────────────────────────────

/// Contract C6 regression lock (task-2181 β): a single-StructureRef-param purpose
/// must NOT trigger the multi-param rejection, and must compile its body with the
/// per-param `{purpose}::{param}` entity stamp on member refs.
///
/// Pins: `subject.mass` in a single-param purpose compiles to
/// `ValueRef { entity: "lightweight::subject", member: "mass", result_type: Type::Real }`.
/// This is the pre-remap form; `activate_purpose` rewrites the entity stamp to the
/// actual entity_ref at eval time via `expr.remap_entity("lightweight::subject", entity_ref)`.
///
/// Single-param purposes are behavior-identical before and after β for the
/// activation remap (one stamp → one remap target), so all existing activation
/// tests continue to pass after this change.
#[test]
fn compile_purpose_single_param_emits_purpose_param_stamped_valueref() {
    // No structure template needed: subject : Structure is the wildcard kind and
    // member resolution falls through without consulting any template.  Including a
    // Bracket structure would be dead context that misleads the reader into thinking
    // subject.mass is resolved against it.
    let source = r#"
purpose lightweight(subject : Structure) {
    constraint subject.mass > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // (a) No multi-param rejection diagnostic emitted.
    // Match on the stable '(task-2201)' tag rather than prose that may be reworded.
    let rejection_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("(task-2201)"))
        .collect();
    assert!(
        rejection_errors.is_empty(),
        "expected NO '(task-2201)' rejection for a single-param purpose, \
         but got: {:#?}",
        rejection_errors
    );

    // (b) Exactly 1 compiled purpose with 1 constraint.
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "lightweight");
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    // (c) Constraint left side is ValueRef with entity == "lightweight::subject" (β stamp).
    let constraint = &purpose.constraints[0];
    match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, .. } => {
            assert_eq!(
                *op,
                BinOp::Gt,
                "expected BinOp::Gt for 'subject.mass > 0', got {:?}",
                op
            );
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.entity, "lightweight::subject",
                        "ValueRef entity must equal `purpose::param` per task-2181 β stamp scheme, got {:?}",
                        id.entity
                    );
                    assert_eq!(
                        id.member, "mass",
                        "ValueRef member must be 'mass', got {:?}",
                        id.member
                    );
                    assert_eq!(
                        left.result_type,
                        Type::Real,
                        "expected result_type == Type::Real for subject.mass, got {:?}",
                        left.result_type
                    );
                }
                other => panic!(
                    "expected ValueRef for left side of constraint BinOp, got {:?}",
                    other
                ),
            }
        }
        other => panic!("expected BinOp constraint expression, got {:?}", other),
    }
}

// ── task-2181 β: multi-param per-param stamp signal-of-record ─────────────────

/// Signal-of-record test for task-2181 β: a two-StructureRef-param purpose must
/// compile without the `(task-2201)` rejection diagnostic and must stamp each
/// param's member refs with the disjoint `{purpose}::{param}` entity scheme.
///
/// Contract C1 (PRD §4.1): `part.length` → `ValueRef("fits_within::part", "length")`
/// and `envelope.length` → `ValueRef("fits_within::envelope", "length")` — the two
/// refs are disjoint because their entity stamps differ.
///
/// This is the inverse of `compile_purpose_rejects_multi_structureref_params` (which
/// asserts rejection); that test and this one are now in tension — step-4 removes the
/// rejection, at which point `compile_purpose_rejects_multi_structureref_params` will
/// REGRESS and must be deleted.
///
/// Forward pointer: activation of multi-param purposes with per-param entity bindings
/// is added by task γ (`activate_purpose_with_bindings`).
///
/// RED before step-4: assertion #2 (no `(task-2201)` rejection) fails because
/// the multi-param reject at traits.rs:286-301 still fires. Assertions #5/#6 already
/// pass after step-2 changed the stamp.
#[test]
fn compile_purpose_multi_param_per_param_stamping_distinguishes_entities() {
    let source = r#"
purpose fits_within(part : Structure, envelope : Structure) {
    constraint part.length > envelope.length
}
"#;
    let module = compile_module_with_diagnostics(source);

    // (1) Module compiles (even if with errors, the purpose is included per the
    // accumulate-and-continue pattern).
    // (2) No (task-2201) rejection diagnostic — the multi-param reject must be gone.
    let rejection_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("(task-2201)"))
        .collect();
    assert!(
        rejection_errors.is_empty(),
        "expected NO '(task-2201)' rejection for multi-param purpose fits_within, \
         but got: {:#?}",
        rejection_errors
    );

    // (3) Exactly 1 compiled purpose with 1 constraint.
    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "fits_within");
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    // (4) Constraint expression is BinOp(Gt, left, right).
    let constraint = &purpose.constraints[0];
    let (left, right) = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(
                *op,
                BinOp::Gt,
                "expected BinOp::Gt for 'part.length > envelope.length', got {:?}",
                op
            );
            (left.as_ref(), right.as_ref())
        }
        other => panic!("expected BinOp constraint expression, got {:?}", other),
    };

    // (5) Left side: ValueRef with entity == "fits_within::part" and member == "length".
    match &left.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "fits_within::part",
                "left ValueRef entity must equal 'fits_within::part' (per-param stamp C1), got {:?}",
                id.entity
            );
            assert_eq!(
                id.member, "length",
                "left ValueRef member must be 'length', got {:?}",
                id.member
            );
        }
        other => panic!("expected ValueRef for left side of BinOp, got {:?}", other),
    }

    // (6) Right side: ValueRef with entity == "fits_within::envelope" and member == "length".
    match &right.kind {
        CompiledExprKind::ValueRef(id) => {
            assert_eq!(
                id.entity, "fits_within::envelope",
                "right ValueRef entity must equal 'fits_within::envelope' (per-param stamp C1), got {:?}",
                id.entity
            );
            assert_eq!(
                id.member, "length",
                "right ValueRef member must be 'length', got {:?}",
                id.member
            );
        }
        other => panic!("expected ValueRef for right side of BinOp, got {:?}", other),
    }
}

// ── task-4012 ε: guarded blocks in purpose bodies lower to implication constraints ──

/// Step-1 RED suite: guarded where-arm lowering tests.
///
/// All tests in this module are RED before the step-2 implementation because the
/// current `MemberDecl::GuardedGroup` arm emits "guarded blocks in purpose bodies
/// are not yet supported" for the entire group, so:
///   - tests asserting `errors.is_empty()` fail (the group error fires), and
///   - tests asserting a specific constraint count / structure fail (no constraints
///     are produced when the whole group is rejected).
mod guarded {
    use super::*;

    /// (a) A where-arm with a single constraint lowers to exactly one
    /// `CompiledConstraint` whose top-level expression is `BinOp::Implies`.
    ///
    /// RED: `compile_module_with_diagnostics` returns the "guarded blocks not yet
    /// supported" error → `assert!(errors.is_empty())` fails.
    #[test]
    fn guarded_where_arm_lowers_to_implies() {
        let source = r#"
structure Frame {
    param material : Scalar = 1.0
    param youngs_modulus : Scalar = 200.0
}

purpose p(subject : Structure) {
    where subject.material > 0.0 {
        constraint subject.youngs_modulus > 0.0
    }
}
"#;
        let module = compile_module_with_diagnostics(source);

        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no compile errors for where-arm in purpose, got: {:#?}",
            errors
        );

        let purpose = module
            .compiled_purposes
            .iter()
            .find(|p| p.name == "p")
            .expect("expected compiled purpose 'p'");

        assert_eq!(
            purpose.constraints.len(),
            1,
            "expected exactly 1 compiled constraint from the where-arm, got {}",
            purpose.constraints.len()
        );

        let constraint = &purpose.constraints[0];
        match &constraint.expr.kind {
            CompiledExprKind::BinOp { op, .. } => {
                assert_eq!(
                    *op,
                    BinOp::Implies,
                    "guarded where-arm must lower to BinOp::Implies, got {:?}",
                    op
                );
            }
            other => panic!(
                "expected BinOp::Implies as top-level constraint expr, got {:?}",
                other
            ),
        }
    }

    /// (b) A guarded block containing an unsupported member kind (`param`) alongside
    /// a valid constraint: the constraint must still compile to `BinOp::Implies` (1
    /// constraint emitted) and an error must be produced for the unsupported member.
    ///
    /// RED: the current implementation rejects the entire GuardedGroup with a single
    /// generic error → `assert_eq!(purpose.constraints.len(), 1)` fails (0 constraints).
    #[test]
    fn guarded_unsupported_member_kind_emits_error() {
        let source = r#"
structure Frame {
    param material : Scalar = 1.0
    param youngs_modulus : Scalar = 200.0
}

purpose p(subject : Structure) {
    where subject.material > 0.0 {
        constraint subject.youngs_modulus > 0.0
        param x : Scalar = 1.0
    }
}
"#;
        let module = compile_module_with_diagnostics(source);

        // After impl: exactly 1 constraint from the where-arm constraint (the param is
        // rejected, but the constraint compiles to BinOp::Implies).
        // RED before impl: the whole GuardedGroup is rejected → 0 constraints.
        let purpose = module
            .compiled_purposes
            .iter()
            .find(|p| p.name == "p")
            .expect("expected compiled purpose 'p'");
        assert_eq!(
            purpose.constraints.len(),
            1,
            "expected 1 compiled constraint from the valid where-arm constraint \
             (only the unsupported `param x` should emit an error)"
        );

        // The unsupported param inside the guarded block must emit a Severity::Error.
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "expected a Severity::Error diagnostic for `param x` inside a guarded block"
        );
    }

    /// (c) An else-arm constraint lowers to `(not C) implies B`.
    ///
    /// RED (step-3): step-2 impl only handles `g.members` (where-arm);
    /// `g.else_members` is not yet processed → 0 constraints from the else-arm →
    /// `assert_eq!(purpose.constraints.len(), 1)` fails.
    #[test]
    fn guarded_else_arm_lowers_to_not_implies() {
        // Condition is always-false so the else arm is "active" semantically,
        // but the compile test only checks the structural shape (not eval).
        let source = r#"
structure Frame {
    param z : Scalar = 5.0
}

purpose p(subject : Structure) {
    where 0.0 > 1.0 {
    } else {
        constraint subject.z > 0.0
    }
}
"#;
        let module = compile_module_with_diagnostics(source);

        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no compile errors for else-arm in purpose, got: {:#?}",
            errors
        );

        let purpose = module
            .compiled_purposes
            .iter()
            .find(|p| p.name == "p")
            .expect("expected compiled purpose 'p'");

        assert_eq!(
            purpose.constraints.len(),
            1,
            "expected 1 compiled constraint from the else-arm, got {}",
            purpose.constraints.len()
        );

        // The constraint must be `(not C) implies B`.
        let constraint = &purpose.constraints[0];
        match &constraint.expr.kind {
            CompiledExprKind::BinOp { op, left, .. } => {
                assert_eq!(
                    *op,
                    BinOp::Implies,
                    "else-arm must lower to BinOp::Implies, got {:?}",
                    op
                );
                // left must be UnOp::Not wrapping the condition.
                match &left.kind {
                    CompiledExprKind::UnOp { op: unop, .. } => {
                        assert_eq!(
                            *unop,
                            UnOp::Not,
                            "else-arm antecedent must be UnOp::Not(condition), got {:?}",
                            unop
                        );
                    }
                    other => panic!(
                        "expected UnOp::Not for else-arm antecedent, got {:?}",
                        other
                    ),
                }
            }
            other => panic!(
                "expected BinOp::Implies for else-arm constraint expr, got {:?}",
                other
            ),
        }
    }

    /// (d) A guard-scoped `let` binding inside a where-arm: the let is appended to
    /// `CompiledPurpose.lets` and the guarded constraint's right side references the
    /// let's value cell via a ValueRef.
    ///
    /// RED (step-5): same note as (c) — step-2 impl already handles guarded lets, so
    /// this test is GREEN at write-time (not RED-first per plan intent; deviation
    /// logged in esc-4012-147).
    #[test]
    fn guarded_let_binding_appended_and_referenced_in_constraint() {
        let source = r#"
structure Widget {
    param a : Length = 80mm
    param b : Length = 50mm
}

purpose marg(subject : Structure) {
    where subject.a > 0mm {
        let m = subject.a - subject.b
        constraint m > 0mm
    }
}
"#;
        let module = compile_module_with_diagnostics(source);

        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no compile errors for guard-scoped let, got: {:#?}",
            errors
        );

        let purpose = module
            .compiled_purposes
            .iter()
            .find(|p| p.name == "marg")
            .expect("expected compiled purpose 'marg'");

        // The let binding must be appended to purpose.lets.
        assert_eq!(purpose.lets.len(), 1, "expected 1 let in purpose.lets");
        assert_eq!(purpose.lets[0].name, "m");
        assert_eq!(
            purpose.lets[0].cell_id,
            ValueCellId::new("marg", "m"),
            "let cell_id should be {{marg, m}}"
        );

        // The constraint from the where-arm must exist and be BinOp::Implies.
        assert_eq!(
            purpose.constraints.len(),
            1,
            "expected 1 constraint from the where-arm"
        );
        let constraint = &purpose.constraints[0];
        match &constraint.expr.kind {
            CompiledExprKind::BinOp { op, .. } => {
                assert_eq!(*op, BinOp::Implies, "where-arm must lower to BinOp::Implies");
            }
            other => panic!("expected BinOp::Implies, got {:?}", other),
        }

        // The right side of the implication (the body) must reference the let
        // cell `m` via a ValueRef(marg, m).
        let right = match &constraint.expr.kind {
            CompiledExprKind::BinOp { right, .. } => right.as_ref(),
            _ => unreachable!(),
        };
        let let_cell_id = ValueCellId::new("marg", "m");
        assert!(
            purpose_let_expr_contains_value_ref(right, &let_cell_id),
            "constraint body (right side of implication) must reference the \
             guard-scoped let 'm' via ValueRef(marg, m); got {:?}",
            right
        );
    }

    /// (e) Duplicate let name in where-arm and else-arm: both entries are pushed to
    /// `CompiledPurpose.lets`, producing two entries with the same `cell_id`.
    ///
    /// This pins the accepted v1 last-writer-wins behaviour documented in the NOTE
    /// comment in traits.rs: the injection loop in engine_purposes.rs seeds the same
    /// `ValueCellId` twice, and the second write wins in `snapshot.values`.
    ///
    /// If a future task adds a "duplicate let name" diagnostic, update this test to
    /// assert the diagnostic and expect a single `lets` entry instead.
    #[test]
    fn guarded_duplicate_let_name_in_arms_produces_two_lets_entries() {
        let source = r#"
structure Widget {
    param a : Length = 80mm
    param b : Length = 50mm
}

purpose p(subject : Structure) {
    where subject.a > 0mm {
        let v = subject.a
        constraint v > 0mm
    } else {
        let v = subject.b
        constraint v > 0mm
    }
}
"#;
        let module = compile_module_with_diagnostics(source);

        // Both arms compile without errors.
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no compile errors for duplicate guard-scoped let name in arms, \
             got: {:#?}",
            errors
        );

        let purpose = module
            .compiled_purposes
            .iter()
            .find(|p| p.name == "p")
            .expect("expected compiled purpose 'p'");

        // V1 contract: both lets are pushed — two entries, each with the same cell_id.
        // The injection loop seeds the same ValueCellId twice; last-writer-wins.
        assert_eq!(
            purpose.lets.len(),
            2,
            "expected 2 let entries (one per arm, both named 'v'), got {}",
            purpose.lets.len()
        );
        assert_eq!(purpose.lets[0].name, "v");
        assert_eq!(purpose.lets[1].name, "v");

        let expected_cell_id = ValueCellId::new("p", "v");
        assert_eq!(
            purpose.lets[0].cell_id, expected_cell_id,
            "lets[0] cell_id must be {{p, v}}"
        );
        assert_eq!(
            purpose.lets[1].cell_id, expected_cell_id,
            "lets[1] cell_id must be {{p, v}}"
        );

        // Two constraints: one per arm, both BinOp::Implies (where-arm + else-arm).
        assert_eq!(
            purpose.constraints.len(),
            2,
            "expected 2 constraints (where-arm + else-arm), got {}",
            purpose.constraints.len()
        );
    }
}

// ── task-4137: geometric_params and material_params compile-time resolution ───

/// RED (step-3): `compile_purpose` must emit a `ResolvedSchemaQuery` with
/// `query_kind == "material_params"` whose `resolved_ids` are exactly {mat}
/// (the Material-typed param); `span : Length` must be excluded.
///
/// Fails today because compile_purpose emits no material_params query;
/// the material_params path is added in task-4137 step-4.
#[test]
fn compile_purpose_resolves_material_params_query() {
    let source = r#"
structure Material {
    param density : Real = 1.0
}

structure Beam {
    param mat : Material = Material(density: 7850.0)
    param span : Length = 1000mm
    constraint span > 0mm
}

purpose check(subject : Beam) {
    constraint 1 > 0
}
"#;

    let module = parse_and_compile(source);

    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "check");
    assert_eq!(purpose.params[0].entity_kind, "Beam");

    // Find the material_params query by query_kind.
    let mat_query = purpose
        .resolved_queries
        .iter()
        .find(|q| q.query_kind == "material_params" && q.param_name == "subject")
        .expect(
            "expected a ResolvedSchemaQuery with query_kind='material_params' \
             and param_name='subject'; compile_purpose must emit this query \
             (task-4137 step-4 not yet applied)",
        );

    assert_eq!(mat_query.param_name, "subject");
    assert_eq!(mat_query.query_kind, "material_params");

    // resolved_ids must be exactly {mat}; span (Length/geometric) must be excluded.
    let mut member_names: Vec<&str> = mat_query
        .resolved_ids
        .iter()
        .map(|id: &ValueCellId| id.member.as_str())
        .collect();
    member_names.sort();

    assert_eq!(
        member_names,
        vec!["mat"],
        "material_params resolved_ids must be exactly {{mat}}; \
         span (Length-typed) must be excluded. Got: {:?}",
        member_names
    );
}

/// RED (step-1): `compile_purpose` must emit a `ResolvedSchemaQuery` with
/// `query_kind == "geometric_params"` whose `resolved_ids` are exactly
/// {width, twist} (the Length- and Angle-typed params); `ratio : Real`
/// (dimensionless) must be excluded.
///
/// Fails today because `compile_purpose` (traits.rs) only emits a "params"
/// query; the geometric_params path is added in task-4137 step-2.
#[test]
fn compile_purpose_resolves_geometric_params_query() {
    let source = r#"
structure Widget {
    param width : Length = 80mm
    param twist : Angle = 30deg
    param ratio : Real = 1.0
    constraint width > 0mm
}

purpose check(subject : Widget) {
    constraint 1 > 0
}
"#;

    let module = parse_and_compile(source);

    assert_eq!(
        module.compiled_purposes.len(),
        1,
        "expected 1 compiled purpose"
    );
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.name, "check");
    assert_eq!(purpose.params[0].entity_kind, "Widget");

    // Find the geometric_params query by query_kind (not positional index,
    // since params is also emitted and order may vary).
    let geo_query = purpose
        .resolved_queries
        .iter()
        .find(|q| q.query_kind == "geometric_params" && q.param_name == "subject")
        .expect(
            "expected a ResolvedSchemaQuery with query_kind='geometric_params' \
             and param_name='subject'; compile_purpose must emit this query \
             (task-4137 step-2 not yet applied)",
        );

    assert_eq!(geo_query.param_name, "subject");
    assert_eq!(geo_query.query_kind, "geometric_params");

    // resolved_ids must be exactly {width, twist}; ratio (Real/dimensionless) excluded.
    let mut member_names: Vec<&str> = geo_query
        .resolved_ids
        .iter()
        .map(|id: &ValueCellId| id.member.as_str())
        .collect();
    member_names.sort();

    assert_eq!(
        member_names,
        vec!["twist", "width"],
        "geometric_params resolved_ids must be exactly {{width, twist}}; \
         ratio (dimensionless Real) must be excluded. Got: {:?}",
        member_names
    );
}

/// Protects the `TraitObject("Material")` arm of `is_material_param_type`
/// against silent regression.
///
/// When Material is declared only as a `trait` (no `structure Material`),
/// `param mat : Material` resolves to `Type::TraitObject("Material")` rather
/// than `Type::StructureRef("Material")` (type_resolution.rs precedence rule).
/// `compile_purpose` must still emit a `material_params` `ResolvedSchemaQuery`
/// covering that param.
///
/// Complements the unit tests for `is_material_param_type` in `types.rs`.
#[test]
fn compile_purpose_resolves_material_params_query_via_trait_object() {
    let source = r#"
trait Material {
    param density : Real
}

structure Beam {
    param mat : Material = auto
    param span : Length = 1000mm
}

purpose check(subject : Beam) {
    constraint 1 > 0
}
"#;
    // Use compile_module_with_diagnostics so that any solver-level auto-param
    // diagnostics don't abort the test — we care only about the resolved query.
    let module = compile_module_with_diagnostics(source);

    let purpose = module
        .compiled_purposes
        .iter()
        .find(|p| p.name == "check")
        .expect("expected compiled purpose 'check'");

    // compile_purpose must emit the material_params query even when mat's type
    // is Type::TraitObject("Material") (second arm of is_material_param_type).
    let mat_query = purpose
        .resolved_queries
        .iter()
        .find(|q| q.query_kind == "material_params" && q.param_name == "subject")
        .expect(
            "expected a ResolvedSchemaQuery with query_kind='material_params' \
             and param_name='subject'; TraitObject arm of is_material_param_type \
             must fire when Material is declared only as a trait",
        );

    // resolved_ids must include mat (TraitObject-typed); span (geometric) excluded.
    let member_names: Vec<&str> = mat_query
        .resolved_ids
        .iter()
        .map(|id: &ValueCellId| id.member.as_str())
        .collect();

    assert!(
        member_names.contains(&"mat"),
        "material_params must include the TraitObject-typed 'mat' param. Got: {:?}",
        member_names
    );
    assert!(
        !member_names.contains(&"span"),
        "span (Length-typed, geometric) must NOT appear in material_params. Got: {:?}",
        member_names
    );
}

// ── task-4197 α: determinacy intrinsics compiler sugar ──────────────────────

/// BT2: `AllGeometryDetermined(X)` desugars to the same `CompiledExpr` as the
/// hand-written `forall __p in X.geometric_params: determined(__p)` (golden-equivalence).
///
/// RED before step-4 impl: AllGeometryDetermined is not yet in the recognizer map
/// (step-2 only added "params"), so (A) does not desugar and its hash differs.
#[test]
fn all_geometry_determined_desugars_to_same_compiled_expr_as_hand_written_forall() {
    // (A) using the compiler-sugar intrinsic
    let source_a = r#"
purpose g(subject : Structure) {
    constraint AllGeometryDetermined(subject)
}
"#;

    // (B) hand-written reflective forall — the canonical expansion
    let source_b = r#"
purpose g(subject : Structure) {
    constraint forall __p in subject.geometric_params: determined(__p)
}
"#;

    let module_a = compile_module_with_diagnostics(source_a);
    let module_b = compile_module_with_diagnostics(source_b);

    // Both must compile without Error diagnostics
    let errors_a: Vec<_> = module_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "AllGeometryDetermined (A): expected no Error diagnostics, got: {:?}",
        errors_a
    );

    let errors_b: Vec<_> = module_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_b.is_empty(),
        "hand-written forall geometric (B): expected no Error diagnostics, got: {:?}",
        errors_b
    );

    // Both purposes must have exactly one constraint
    let purpose_a = module_a
        .compiled_purposes
        .iter()
        .find(|p| p.name == "g")
        .expect("expected compiled purpose 'g' from source_a");
    let purpose_b = module_b
        .compiled_purposes
        .iter()
        .find(|p| p.name == "g")
        .expect("expected compiled purpose 'g' from source_b");

    assert_eq!(
        purpose_a.constraints.len(),
        1,
        "AllGeometryDetermined purpose must have 1 constraint"
    );
    assert_eq!(
        purpose_b.constraints.len(),
        1,
        "hand-written geometric forall purpose must have 1 constraint"
    );

    // The desugared expr must be content-hash-equal to the hand-written form.
    let hash_a = purpose_a.constraints[0].expr.content_hash;
    let hash_b = purpose_b.constraints[0].expr.content_hash;
    assert_eq!(
        hash_a, hash_b,
        "AllGeometryDetermined must desugar to the same CompiledExpr \
         (identical content_hash) as `forall __p in subject.geometric_params: determined(__p)`.\n\
         A hash: {:?}\n\
         B hash: {:?}",
        hash_a, hash_b
    );
}

/// BT1: `AllParamsDetermined(X)` desugars to the same `CompiledExpr` as the
/// hand-written `forall __p in X.params: determined(__p)` (golden-equivalence).
///
/// RED before step-2 impl: (A) currently compiles AllParamsDetermined as a
/// UserFunctionCall / overload-error poison, so its content_hash differs from
/// the reflective Quantifier in (B).
#[test]
fn all_params_determined_desugars_to_same_compiled_expr_as_hand_written_forall() {
    // (A) using the compiler-sugar intrinsic
    let source_a = r#"
purpose design_review(subject : Structure) {
    constraint AllParamsDetermined(subject)
}
"#;

    // (B) hand-written reflective forall — the canonical expansion
    let source_b = r#"
purpose design_review(subject : Structure) {
    constraint forall __p in subject.params: determined(__p)
}
"#;

    let module_a = compile_module_with_diagnostics(source_a);
    let module_b = compile_module_with_diagnostics(source_b);

    // Both must compile without Error diagnostics
    let errors_a: Vec<_> = module_a
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_a.is_empty(),
        "AllParamsDetermined (A): expected no Error diagnostics, got: {:?}",
        errors_a
    );

    let errors_b: Vec<_> = module_b
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_b.is_empty(),
        "hand-written forall (B): expected no Error diagnostics, got: {:?}",
        errors_b
    );

    // Both purposes must have exactly one constraint
    let purpose_a = module_a
        .compiled_purposes
        .iter()
        .find(|p| p.name == "design_review")
        .expect("expected compiled purpose 'design_review' from source_a");
    let purpose_b = module_b
        .compiled_purposes
        .iter()
        .find(|p| p.name == "design_review")
        .expect("expected compiled purpose 'design_review' from source_b");

    assert_eq!(
        purpose_a.constraints.len(),
        1,
        "AllParamsDetermined purpose must have 1 constraint"
    );
    assert_eq!(
        purpose_b.constraints.len(),
        1,
        "hand-written forall purpose must have 1 constraint"
    );

    // The desugared expr must be content-hash-equal to the hand-written form.
    // (CompiledExpr does not derive PartialEq; ContentHash does.)
    let hash_a = purpose_a.constraints[0].expr.content_hash;
    let hash_b = purpose_b.constraints[0].expr.content_hash;
    assert_eq!(
        hash_a, hash_b,
        "AllParamsDetermined must desugar to the same CompiledExpr \
         (identical content_hash) as `forall __p in subject.params: determined(__p)`.\n\
         A hash: {:?}\n\
         B hash: {:?}",
        hash_a, hash_b
    );
}

/// BT3: `AllParamsDetermined` / `AllGeometryDetermined` used OUTSIDE a purpose body
/// must emit `DiagnosticCode::DeterminacyIntrinsicScope` (E_DETERMINACY_INTRINSIC_SCOPE)
/// and must NOT emit a "no matching overload" / unknown-user-fn diagnostic for the
/// intrinsic name (invariant A3: no silent fall-through).
///
/// RED before step-6 impl: no scope guard exists yet, so the call falls through to
/// the function-overload resolver and emits a "no matching overload" error instead.
#[test]
fn all_params_determined_outside_purpose_body_emits_scope_diagnostic() {
    // Use inside a structure constraint — not a purpose body
    let source = r#"
structure Foo {
    param w : Length = 1mm
    constraint AllParamsDetermined(w)
}
"#;
    let module = compile_module_with_diagnostics(source);

    // Must have a DeterminacyIntrinsicScope diagnostic
    let scope_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeterminacyIntrinsicScope))
        .collect();
    assert!(
        !scope_diags.is_empty(),
        "expected a DeterminacyIntrinsicScope diagnostic for intrinsic used outside purpose body, \
         got diagnostics: {:?}",
        module.diagnostics
    );

    // The scope diagnostic message must contain "E_DETERMINACY_INTRINSIC_SCOPE"
    assert!(
        scope_diags[0].message.contains("E_DETERMINACY_INTRINSIC_SCOPE"),
        "DeterminacyIntrinsicScope diagnostic must contain 'E_DETERMINACY_INTRINSIC_SCOPE' in message. \
         Got: {:?}",
        scope_diags[0].message
    );

    // Must NOT have a "no matching overload" or unknown-user-fn diagnostic for
    // the intrinsic name — the scope guard must intercept before overload resolution.
    let overload_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("no matching overload")
                && d.message.contains("AllParamsDetermined")
        })
        .collect();
    assert!(
        overload_diags.is_empty(),
        "must NOT emit a 'no matching overload' diagnostic for AllParamsDetermined \
         (scope guard must intercept first), got: {:?}",
        overload_diags
    );
}

/// BT3 variant: `AllGeometryDetermined` used outside a purpose body also emits
/// `DiagnosticCode::DeterminacyIntrinsicScope`.
#[test]
fn all_geometry_determined_outside_purpose_body_emits_scope_diagnostic() {
    let source = r#"
structure Bar {
    param h : Length = 2mm
    constraint AllGeometryDetermined(h)
}
"#;
    let module = compile_module_with_diagnostics(source);

    let scope_diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DeterminacyIntrinsicScope))
        .collect();
    assert!(
        !scope_diags.is_empty(),
        "expected a DeterminacyIntrinsicScope diagnostic for AllGeometryDetermined outside \
         purpose body, got diagnostics: {:?}",
        module.diagnostics
    );

    assert!(
        scope_diags[0].message.contains("E_DETERMINACY_INTRINSIC_SCOPE"),
        "DeterminacyIntrinsicScope diagnostic must contain 'E_DETERMINACY_INTRINSIC_SCOPE'. \
         Got: {:?}",
        scope_diags[0].message
    );
}
