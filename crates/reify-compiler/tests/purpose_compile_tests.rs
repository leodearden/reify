//! Purpose compilation tests.
//!
//! Tests for compiling purpose declarations into CompiledPurpose entries.

use reify_compiler::*;
use reify_test_support::parse_and_compile;
use reify_types::*;

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
    assert_eq!(
        purpose.resolved_queries.len(),
        1,
        "expected 1 resolved reflective query"
    );
    let query = &purpose.resolved_queries[0];
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

// ── Step 23: let bindings in purposes should emit error ───────────────

#[test]
#[should_panic(expected = "compile errors")]
fn compile_purpose_rejects_let_bindings() {
    // Let bindings in purpose bodies are not yet supported: the compiled
    // expression is discarded and constraints referencing let-bound names
    // would produce ValueCellIds with no backing eval graph node.
    // The compiler should emit a Severity::Error diagnostic.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
}

purpose check(subject : Structure) {
    let half_w = 80mm / 2
    constraint half_w > 10mm
}
"#;

    let _module = parse_and_compile(source);
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
fn compile_purpose_rejects_guarded_blocks() {
    // The grammar's purpose_member reuses guarded_block, so a where-guarded
    // constraint block parses into MemberDecl::GuardedGroup. The compiler
    // should emit a Severity::Error diagnostic rather than silently dropping it.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
}

purpose check(subject : Structure) {
    where 80mm > 10mm {
        constraint 60mm > 5mm
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
        !errors.is_empty(),
        "expected compile error for guarded block in purpose, but got none"
    );
    let has_guarded_error = errors.iter().any(|d| {
        d.message
            .contains("guarded blocks in purpose bodies are not yet supported")
    });
    assert!(
        has_guarded_error,
        "expected diagnostic about unsupported guarded blocks, got: {:?}",
        errors
    );
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

// ── Step 1 (task-2181): reflective aggregation compiles as empty list ─────────

/// Shared helper for all `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS` tests.
///
/// Compiles `forall p in subject.<member>: determined(p)` in a purpose body
/// and asserts the three acceptance criteria:
/// (a) no "member access not yet supported" diagnostic,
/// (b) `collection.result_type == Type::List(Box::new(Type::Real))`,
/// (c) `collection.kind` is an empty `ListLiteral`.
///
/// Using a single helper avoids duplicating ~60 lines per member name and
/// naturally extends to cover every entry in `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS`
/// (currently `params`, `geometric_params`, `material_params`).
fn assert_reflective_member_compiles_empty(member: &str) {
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

    // (b) and (c): constraint is a Quantifier whose collection is an empty
    // ListLiteral with result_type == Type::List(Box::new(Type::Real)).
    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 compiled purpose");
    let purpose = &module.compiled_purposes[0];
    assert_eq!(purpose.constraints.len(), 1, "expected 1 constraint");

    let constraint = &purpose.constraints[0];
    match &constraint.expr.kind {
        CompiledExprKind::Quantifier { collection, .. } => {
            // (b) collection result_type must be List<Real>
            assert_eq!(
                collection.result_type,
                Type::List(Box::new(Type::Real)),
                "expected collection result_type to be List<Real> for member '{}', got {:?}",
                member,
                collection.result_type
            );
            // (c) collection kind must be an empty ListLiteral
            match &collection.kind {
                CompiledExprKind::ListLiteral(elements) => {
                    assert!(
                        elements.is_empty(),
                        "expected empty ListLiteral for subject.{}, got {} elements",
                        member,
                        elements.len()
                    );
                }
                other => panic!(
                    "expected ListLiteral collection for member '{}', got {:?}",
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

/// `subject.params` compiles to an empty ListLiteral (step 1, task-2181).
///
/// RED before step-2 impl: catch-all at `expr.rs` fires and emits
/// "member access not yet supported: .params".
#[test]
fn compile_purpose_reflective_params_compiles_as_empty_list() {
    assert_reflective_member_compiles_empty("params");
}

/// `part.geometric_params` compiles to an empty ListLiteral (step 1, task-2181).
///
/// RED before step-2 impl: analogous to the params test above.
#[test]
fn compile_purpose_reflective_geometric_params_compiles_as_empty_list() {
    assert_reflective_member_compiles_empty("geometric_params");
}

/// `subject.material_params` compiles to an empty ListLiteral (task-2181).
///
/// `material_params` is the third entry in `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS`
/// and previously had no dedicated compile-time coverage.
#[test]
fn compile_purpose_reflective_material_params_compiles_as_empty_list() {
    assert_reflective_member_compiles_empty("material_params");
}

// ── Amendment (task-2181 review-1): entity-scope StructureRef regression ──────

/// Regression guard: entity-scope `StructureRef` member access must NOT be
/// silently routed through the purpose-subject branch.
///
/// When two structures are compiled together, `param material : Material` in an
/// entity body registers `material` as `Type::StructureRef("Material")` (because
/// `Material` is a known structure name).  The purpose-subject branch in
/// `expr.rs` is gated by `!scope.is_entity_scope`; without that guard,
/// `material.density > 0` in a structure constraint would silently emit
/// `ValueRef(entity_name, "density")` — a dangling ref to a non-existent cell —
/// instead of the correct "member access not yet supported" error.
#[test]
fn entity_scope_structureref_member_access_still_errors() {
    // Two structures in the same compilation unit so `Material` lands in
    // `structure_names` and `param material : Material` resolves to
    // `Type::StructureRef("Material")` rather than falling back to Type::Real.
    let source = r#"
structure Material {
    param density : Real = 7850.0
}

structure Widget {
    param material : Material = Material(density: 7850.0)
    constraint material.density > 0
}
"#;
    let module = compile_module_with_diagnostics(source);

    // The "member access not yet supported" diagnostic must still fire.
    // If the purpose-subject branch misfired, this would be empty.
    let unsupported: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("member access not yet supported"))
        .collect();
    assert!(
        !unsupported.is_empty(),
        "expected 'member access not yet supported' for entity-scope StructureRef member \
         access, but no such diagnostic was emitted.\n\
         This likely means the purpose-subject branch misfired in entity scope.\n\
         All diagnostics: {:?}",
        module.diagnostics
    );
}

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
            // left must be ValueRef with entity == purpose name and member == "mass"
            match &left.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.entity, "lightweight",
                        "ValueRef entity must equal purpose name (pre-remap), got {:?}",
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
        other => panic!(
            "expected BinOp constraint expression, got {:?}",
            other
        ),
    }

    // (c) objective is Some(Minimize(ValueRef(lightweight.mass)))
    match &purpose.objective {
        Some(OptimizationObjective::Minimize(expr)) => {
            match &expr.kind {
                CompiledExprKind::ValueRef(id) => {
                    assert_eq!(
                        id.entity, "lightweight",
                        "objective ValueRef entity must equal purpose name (pre-remap), got {:?}",
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
        other => panic!(
            "expected Some(Minimize(_)) for objective, got {:?}",
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
    const M5_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/m5_purpose.ri"
    );
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

    // Secondary check: all three purposes must be present.
    assert_eq!(
        module.compiled_purposes.len(),
        3,
        "expected 3 compiled purposes (manufacturing_ready, lightweight, dimensionally_valid), got: {:?}",
        module.compiled_purposes.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}
