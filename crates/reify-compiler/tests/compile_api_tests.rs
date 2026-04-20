//! Public-API tests for `reify_compiler::compile` — moved from
//! `crates/reify-compiler/src/lib.rs`'s `mod tests` block as part of task 2031.
//!
//! These tests drive through the public `compile` entry point and inspect
//! the resulting `CompiledModule`. They use only public re-exports from
//! `reify_compiler`, `reify_syntax`, and `reify_types`.

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, EntityKind, GeomRef, ModifyKind, PatternKind, PrimitiveKind,
    SweepKind, TransformKind, compile,
};

#[test]
fn entity_kind_display() {
    assert_eq!(EntityKind::Structure.to_string(), "structure");
    assert_eq!(EntityKind::Occurrence.to_string(), "occurrence");
    assert_eq!(EntityKind::Structure, EntityKind::Structure);
    assert_ne!(EntityKind::Structure, EntityKind::Occurrence);
    assert_eq!(format!("{:?}", EntityKind::Structure), "Structure");
}

// --- Verify new geometry function calls compile into realizations ---

#[test]
fn compile_linear_pattern_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern(w, 1, 0, 0, 4, 20)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    // linear_pattern is a geometry function, so should produce a realization
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for linear_pattern call, got {}",
        template.realizations.len()
    );
    // Verify it's a Pattern op with Linear kind
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                ..
            }
        ),
        "expected Pattern(Linear), got {:?}",
        op
    );
}

#[test]
fn compile_mirror_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let mirrored = mirror(w, 0, 0, 0, 1, 0, 0)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_mirror"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for mirror call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Pattern {
                kind: PatternKind::Mirror,
                ..
            }
        ),
        "expected Pattern(Mirror), got {:?}",
        op
    );
}

#[test]
fn compile_linear_pattern_2d_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern_2d(w, 1, 0, 0, 3, 20, 0, 1, 0, 4, 30)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat2d"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for linear_pattern_2d call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear2D,
                ..
            }
        ),
        "expected Pattern(Linear2D), got {:?}",
        op
    );
    // Verify correct number of named args (11: target + 10 params)
    if let CompiledGeometryOp::Pattern { args, .. } = op {
        assert_eq!(args.len(), 11, "expected 11 args, got {}", args.len());
        assert_eq!(args[0].0, "target");
        assert_eq!(args[1].0, "dx1");
        assert_eq!(args[4].0, "count1");
        assert_eq!(args[5].0, "spacing1");
        assert_eq!(args[6].0, "dx2");
        assert_eq!(args[9].0, "count2");
        assert_eq!(args[10].0, "spacing2");
    }
}

#[test]
fn compile_linear_pattern_2d_wrong_arity_produces_diagnostic() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = linear_pattern_2d(w, 1, 0, 0, 3, 20)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_linpat2d_err"));
    assert!(parsed.errors.is_empty());
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("linear_pattern_2d")
                && d.message.contains("11 arguments")),
        "expected arity diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_arbitrary_pattern_produces_realization() {
    // arbitrary_pattern(target, dx1, dy1, dz1, dx2, dy2, dz2) = 7 args = target + 2 triples
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0, 0, 0, 20, 0)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for arbitrary_pattern call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Pattern {
                kind: PatternKind::Arbitrary,
                ..
            }
        ),
        "expected Pattern(Arbitrary), got {:?}",
        op
    );
    // Verify args: target + 6 transform coords (2 triples)
    if let CompiledGeometryOp::Pattern { args, .. } = op {
        assert_eq!(args.len(), 7, "expected 7 args, got {}", args.len());
        assert_eq!(args[0].0, "target");
        assert_eq!(args[1].0, "t0_dx");
        assert_eq!(args[2].0, "t0_dy");
        assert_eq!(args[3].0, "t0_dz");
        assert_eq!(args[4].0, "t1_dx");
    }
}

#[test]
fn compile_arbitrary_pattern_too_few_args_produces_diagnostic() {
    // Needs at least 4 args (target + 1 triple)
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat_err1"));
    assert!(parsed.errors.is_empty());
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("arbitrary_pattern")),
        "expected arity diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_arbitrary_pattern_non_triple_args_produces_diagnostic() {
    // 6 args = target + 5 coords, but (6-1)%3 != 0
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = arbitrary_pattern(w, 10, 0, 0, 5, 0)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_arbpat_err2"));
    assert!(parsed.errors.is_empty());
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("arbitrary_pattern")),
        "expected arity diagnostic for non-triple args, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_loft_produces_realization() {
    let source = r#"structure S {
    param r: Scalar = 10mm
    let swept = loft(r, r)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_loft"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for loft call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Loft,
                ..
            }
        ),
        "expected Sweep(Loft), got {:?}",
        op
    );
}

#[test]
fn compile_shell_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let hollowed = shell(w, 1)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_shell"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for shell call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Shell,
                ..
            }
        ),
        "expected Modify(Shell), got {:?}",
        op
    );
}

#[test]
fn compile_thicken_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let thickened = thicken(w, 2)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_thicken"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for thicken call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                ..
            }
        ),
        "expected Modify(Thicken), got {:?}",
        op
    );
}

#[test]
fn compile_draft_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let drafted = draft(w, 0.1, w)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_draft"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for draft call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Draft,
                ..
            }
        ),
        "expected Modify(Draft), got {:?}",
        op
    );
}

#[test]
fn compile_circular_pattern_produces_realization() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    let pattern = circular_pattern(w, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_circpat"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for circular_pattern call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Pattern {
                kind: PatternKind::Circular,
                ..
            }
        ),
        "expected Pattern(Circular), got {:?}",
        op
    );
}

// --- Binary boolean op compilation tests (step-3) ---

#[test]
fn compile_union_nested_calls_produces_three_ops() {
    let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm), box(20mm, 20mm, 20mm))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    // union(box, box) should produce 1 realization with 3 ops
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization, got {}",
        template.realizations.len()
    );
    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        3,
        "expected 3 ops (box, box, union), got {}",
        ops.len()
    );
    assert!(
        matches!(
            ops[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Primitive::Box at ops[0], got {:?}",
        ops[0]
    );
    assert!(
        matches!(
            ops[1],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Primitive::Box at ops[1], got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Union, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
}

// --- Nested boolean compilation test (step-11) ---

#[test]
fn compile_nested_boolean_produces_five_ops() {
    // union(difference(box, cylinder), sphere)
    // Expected flat ops:
    //   0: Box
    //   1: Cylinder
    //   2: Boolean{Difference, Step(0), Step(1)}
    //   3: Sphere
    //   4: Boolean{Union, Step(2), Step(3)}
    let source = r#"structure S {
    let r = union(difference(box(20mm, 20mm, 20mm), cylinder(5mm, 20mm)), sphere(10mm))
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_nested_bool"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        5,
        "expected 5 ops for nested boolean, got {}: {:?}",
        ops.len(),
        ops
    );
    assert!(
        matches!(
            ops[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "ops[0] expected Box, got {:?}",
        ops[0]
    );
    assert!(
        matches!(
            ops[1],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                ..
            }
        ),
        "ops[1] expected Cylinder, got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "ops[2] expected Boolean{{Difference,0,1}}, got {:?}",
        ops[2]
    );
    assert!(
        matches!(
            ops[3],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                ..
            }
        ),
        "ops[3] expected Sphere, got {:?}",
        ops[3]
    );
    assert!(
        matches!(
            ops[4],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(2),
                right: GeomRef::Step(3)
            }
        ),
        "ops[4] expected Boolean{{Union,2,3}}, got {:?}",
        ops[4]
    );
}

// --- Error case tests for boolean arg validation (step-9, step-10) ---

#[test]
fn compile_union_wrong_arity_emits_diagnostic() {
    // union(box(...)) with 1 arg should fail with arity diagnostic
    let source = r#"structure S {
    let r = union(box(10mm, 10mm, 10mm))
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_union_arity"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    // Should produce no realization (compilation failed)
    assert_eq!(
        template.realizations.len(),
        0,
        "expected 0 realizations for wrong-arity union, got {}",
        template.realizations.len()
    );
    // Should have a diagnostic mentioning "expects 2 arguments"
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("expects 2 arguments")),
        "expected 'expects 2 arguments' diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_union_non_geometry_arg_emits_diagnostic() {
    // union(42, box(...)) — first arg is a scalar literal, not geometry
    // The parser may reject bare number literals in function position,
    // so we use a param reference (Scalar param) which is a valid expr but not geometry.
    let source = r#"structure S {
    param w: Scalar = 10mm
    let r = union(w, box(10mm, 10mm, 10mm))
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("test_union_nongeom"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    // Should produce no realization (compilation failed)
    assert_eq!(
        template.realizations.len(),
        0,
        "expected 0 realizations for non-geometry arg union, got {}",
        template.realizations.len()
    );
    // Should have at least one diagnostic
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected diagnostics for non-geometry arg, got none"
    );
}

// --- union_all / intersection_all fold compilation tests (step-7) ---

#[test]
fn compile_union_all_three_args_produces_five_ops() {
    // union_all(a, b, c) → left-fold: Union(Union(a,b), c)
    // ops: Box_a, Box_b, Boolean{Union,Step(0),Step(1)}, Box_c, Boolean{Union,Step(2),Step(3)}
    let source = r#"structure S {
    let r = union_all(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_union_all"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        5,
        "expected 5 ops for union_all(3 args), got {}: {:?}",
        ops.len(),
        ops
    );
    // ops[0]: Box
    assert!(
        matches!(
            ops[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Box at ops[0]"
    );
    // ops[1]: Box
    assert!(
        matches!(
            ops[1],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Box at ops[1]"
    );
    // ops[2]: Union(Step(0), Step(1))
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Union,Step(0),Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
    // ops[3]: Box
    assert!(
        matches!(
            ops[3],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Box at ops[3]"
    );
    // ops[4]: Union(Step(2), Step(3))
    assert!(
        matches!(
            ops[4],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(2),
                right: GeomRef::Step(3)
            }
        ),
        "expected Boolean{{Union,Step(2),Step(3)}} at ops[4], got {:?}",
        ops[4]
    );
}

// --- difference and intersection compilation tests (step-5, step-6) ---

#[test]
fn compile_difference_nested_calls_produces_three_ops() {
    let source = r#"structure S {
    let r = difference(box(20mm, 20mm, 20mm), box(10mm, 10mm, 10mm))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_diff"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let ops = &template.realizations[0].operations;
    assert_eq!(ops.len(), 3, "expected 3 ops (box, box, difference)");
    assert!(
        matches!(
            ops[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Box at ops[0]"
    );
    assert!(
        matches!(
            ops[1],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "expected Box at ops[1]"
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
}

#[test]
fn compile_intersection_nested_calls_produces_three_ops() {
    let source = r#"structure S {
    let r = intersection(box(10mm, 10mm, 10mm), box(10mm, 10mm, 10mm))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_isect"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let ops = &template.realizations[0].operations;
    assert_eq!(ops.len(), 3, "expected 3 ops (box, box, intersection)");
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Intersection,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Intersection, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
}

// --- Sweep (pipe) compiler tests (task-310 step-13) ---

#[test]
fn compile_sweep_produces_sweep_kind() {
    // sweep(profile, path) = 2 args, both geometry refs
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p, p)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for sweep call"
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Sweep,
                ..
            }
        ),
        "expected Sweep(Sweep), got {:?}",
        op
    );
    // Both profile and path should be in profiles as GeomRefs
    if let CompiledGeometryOp::Sweep { profiles, .. } = op {
        assert_eq!(
            profiles.len(),
            2,
            "sweep should have 2 profiles (profile + path), got {}",
            profiles.len()
        );
        assert_eq!(profiles[0], GeomRef::Step(0));
        assert_eq!(profiles[1], GeomRef::Step(1));
    }
}

#[test]
fn compile_sweep_wrong_arg_count() {
    // sweep with 1 arg (should need 2)
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = sweep(p)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sweep_bad"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected diagnostics for wrong arg count"
    );
}

// --- Transform compiler tests (task-377) ---

#[test]
fn user_function_shadowing_scale_no_realizations() {
    // A user-defined function named `scale` with matching arity (2 args)
    // should shadow the geometry built-in and produce 0 realizations.
    let source = r#"
fn scale(x: Real, factor: Real) -> Real { x * factor }

structure S {
    param p: Scalar = 5mm
    let result = scale(p, 2)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_shadow_scale"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        0,
        "user-function shadowing: scale(p, 2) with user fn should produce 0 realizations"
    );
}

#[test]
fn compile_translate_wrong_arg_count() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = translate(p, p)
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("test_translate_bad"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("translate()")),
        "expected translate() arg-count diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_rotate_wrong_arg_count() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate(p, p)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_rotate_bad"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("rotate()")),
        "expected rotate() arg-count diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_scale_wrong_arg_count() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = scale(p, p, p)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_scale_bad"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("scale()")),
        "expected scale() arg-count diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_rotate_around_wrong_arg_count() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate_around(p, p, p)
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("test_rotate_around_bad"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.message.contains("rotate_around()")),
        "expected rotate_around() arg-count diagnostic, got: {:?}",
        compiled.diagnostics
    );
}

#[test]
fn compile_translate_arg_ordering() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = translate(p, p, p, p)
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("test_translate_args"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let op = &template.realizations[0].operations[0];
    if let CompiledGeometryOp::Transform { kind, args, .. } = op {
        assert_eq!(*kind, TransformKind::Translate);
        let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["target", "dx", "dy", "dz"]);
    } else {
        panic!("expected Transform, got {:?}", op);
    }
}

#[test]
fn compile_rotate_arg_ordering() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate(p, p, p, p, p)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_rotate_args"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let op = &template.realizations[0].operations[0];
    if let CompiledGeometryOp::Transform { kind, args, .. } = op {
        assert_eq!(*kind, TransformKind::Rotate);
        let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["target", "ax", "ay", "az", "angle"]);
    } else {
        panic!("expected Transform, got {:?}", op);
    }
}

#[test]
fn compile_scale_arg_ordering() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = scale(p, p)
}"#;
    let parsed =
        reify_syntax::parse(source, reify_types::ModulePath::single("test_scale_args"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let op = &template.realizations[0].operations[0];
    if let CompiledGeometryOp::Transform { kind, args, .. } = op {
        assert_eq!(*kind, TransformKind::Scale);
        let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["target", "factor"]);
    } else {
        panic!("expected Transform, got {:?}", op);
    }
}

#[test]
fn compile_rotate_around_arg_ordering() {
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = rotate_around(p, p, p, p, p, p, p, p)
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_types::ModulePath::single("test_rotate_around_args"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    let op = &template.realizations[0].operations[0];
    if let CompiledGeometryOp::Transform { kind, args, .. } = op {
        assert_eq!(*kind, TransformKind::RotateAround);
        let names: Vec<&str> = args.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["target", "px", "py", "pz", "ax", "ay", "az", "angle"]
        );
    } else {
        panic!("expected Transform, got {:?}", op);
    }
}

#[test]
fn loft_nested_in_union_correct_step_refs() {
    // End-to-end regression: loft nested inside union gets step_offset=1
    // (after the box op at index 0).  After the fix, loft profiles reference
    // Step(1) not Step(0).  p is a scalar param — not a geometry ref — so the
    // silent fallback fires and we can observe the corrected step index.
    let source = r#"structure S {
    param p: Scalar = 5mm
    let result = union(box(10mm, 10mm, 10mm), loft(p, p))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_loft_union"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
    // ops layout: [0]=box, [1]=loft, [2]=Boolean(Union, Step(0), Step(1))
    let ops = &template.realizations[0].operations;
    assert_eq!(ops.len(), 3, "expected 3 ops (box + loft + union), got {}", ops.len());
    // ops[0] must be the Box primitive.
    assert!(
        matches!(&ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
        "expected ops[0] to be a Box primitive, got {:?}",
        ops[0]
    );
    // The loft op is at index 1.
    if let CompiledGeometryOp::Sweep { kind, profiles, .. } = &ops[1] {
        assert_eq!(*kind, SweepKind::Loft, "expected Loft kind at ops[1]");
        for (i, profile) in profiles.iter().enumerate() {
            assert_eq!(
                *profile,
                GeomRef::Step(1 + i),
                "loft profile[{}] inside union should be Step({}) not Step(0), got {:?}",
                i,
                1 + i,
                profile
            );
        }
    } else {
        panic!("expected Sweep(Loft) at ops[1], got {:?}", ops[1]);
    }
}

// --- compile_boolean_op regression guards (step-7) ---
// These tests verify the full compile pipeline for boolean ops.
// They pass before extraction (boolean code is still inline) and remain
// as regression guards after step-8 extracts it into compile_boolean_op.

#[test]
fn compile_boolean_op_union_via_compile() {
    let source = r#"structure S {
    let a = union(sphere(1), cylinder(1, 2))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_bool_union"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;
    // Expected: Primitive(Sphere), Primitive(Cylinder), Boolean{Union, Step(0), Step(1)}
    assert_eq!(ops.len(), 3, "expected 3 ops, got {}: {:?}", ops.len(), ops);
    assert!(matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
    assert!(matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }));
    match &ops[2] {
        CompiledGeometryOp::Boolean { op: BooleanOp::Union, left: GeomRef::Step(0), right: GeomRef::Step(1) } => {}
        other => panic!("expected Boolean{{Union, Step(0), Step(1)}}, got {:?}", other),
    }
}

#[test]
fn compile_boolean_op_union_all_via_compile() {
    let source = r#"structure S {
    let a = union_all(sphere(1), sphere(2), sphere(3))
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_bool_union_all"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;
    // Expected left-fold: Sphere(0), Sphere(1), Boolean{Union,0,1}(2), Sphere(3), Boolean{Union,2,3}(4)
    assert_eq!(ops.len(), 5, "expected 5 ops, got {}: {:?}", ops.len(), ops);
    assert!(matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
    assert!(matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
    assert!(matches!(ops[2], CompiledGeometryOp::Boolean { op: BooleanOp::Union, .. }));
    assert!(matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }));
    assert!(matches!(ops[4], CompiledGeometryOp::Boolean { op: BooleanOp::Union, .. }));
}
