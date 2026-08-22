//! Compiler typing tests for the free-function `generate(n, |i| expr)` combinator
//! (task 3994 / structural-query ζ, PRD §5.9 / §2.3).
//!
//! `generate(n, |i| expr)` applies the lambda to indices `0..n-1` and collects the
//! results into a `List` whose element type is the lambda body type.
//!
//! Observable signals exercised here:
//!   - `generate(4, |i| i * 1mm)` types its cell to `List<Length>` (result typing, step-2).
//!   - `generate(3, |i| i)` types its cell to `List<Int>`         (index-param Int seeding, step-4).
//!   - non-Int count `generate(3mm, …)` / `generate(2.5, …)` emits ArgTypeMismatch (step-6).

use reify_core::{DiagnosticCode, Severity, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::{compile_source, compile_source_with_stdlib};

/// Helper: fetch a structure template's let-cell `default_expr.result_type`.
fn cell_result_type(
    compiled: &reify_compiler::CompiledModule,
    structure: &str,
    cell: &str,
) -> Type {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == structure)
        .unwrap_or_else(|| panic!("{} template not found", structure));
    let vc = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell)
        .unwrap_or_else(|| panic!("value cell '{}' not found in {}", cell, structure));
    vc.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{}' has no default_expr", cell))
        .result_type
        .clone()
}

/// Helper: collect Error-severity diagnostic messages.
fn error_messages(compiled: &reify_compiler::CompiledModule) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

// ─── step-1: result typing ───

/// `generate(4, |i| i * 1mm)` types cell `xs` to `List<Length>` with zero Error
/// diagnostics. The body `i * 1mm` is `Length` whether `i` is Int or Real, so this
/// exercises the result-typing arm ALONE (independent of index-param Int seeding).
///
/// RED today: `generate` is unrecognized by `infer_list_helper_return_type`, so the
/// first-arg fallback types `xs` as `Int` (the count's type), not `List<Length>`.
#[test]
fn generate_result_types_to_list_of_body_type() {
    let source = r#"
        structure S {
            let xs = generate(4, |i| i * 1mm)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics, got: {:?}",
        errors
    );

    assert_eq!(
        cell_result_type(&compiled, "S", "xs"),
        Type::List(Box::new(Type::length())),
        "expected xs : List<Length>",
    );
}

// ─── step-3: index param typed Int ───

/// `generate(3, |i| i)` types cell `xs` to `List<Int>`. The body `i` returns the
/// sole index param verbatim, so the cell element type is exactly the param type —
/// `List<Int>` ONLY IF the unannotated lambda param `i` is seeded to `Int`, not the
/// default Real.
///
/// RED today: unannotated lambda params default to `Type::dimensionless_scalar()`
/// (Real), so the cell types as `List<Real>` (= `List<Scalar{DIMENSIONLESS}>`), not
/// `List<Int>`. The index-param Int seeding (step-4) makes this GREEN.
#[test]
fn generate_seeds_index_param_to_int() {
    let source = r#"
        structure S {
            let xs = generate(3, |i| i)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics, got: {:?}",
        errors
    );

    assert_eq!(
        cell_result_type(&compiled, "S", "xs"),
        Type::List(Box::new(Type::Int)),
        "expected xs : List<Int> (the sole unannotated index param `i` seeded to Int)",
    );
}

// ─── step-5: non-integer count compile diagnostic ───

/// Helper: collect the messages of diagnostics carrying `ArgTypeMismatch`.
/// (ArgTypeMismatch is always emitted at Error severity via `Diagnostic::error`.)
fn arg_type_mismatch_messages(compiled: &reify_compiler::CompiledModule) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .map(|d| d.message.clone())
        .collect()
}

/// A dimensioned (Length) count `generate(3mm, …)` emits an `ArgTypeMismatch`
/// referencing generate's count argument — the count must be a non-negative `Int`.
///
/// RED today: `builtin_arg_slots("generate")` returns `vec![]`, so
/// `check_builtin_arg_types` emits nothing for the count arg. The new
/// `ExpectedArg::Int` slot (step-6) makes this GREEN.
#[test]
fn generate_dimensioned_count_emits_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(3mm, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    let mismatches = arg_type_mismatch_messages(&compiled);
    assert!(
        !mismatches.is_empty(),
        "expected an ArgTypeMismatch for the dimensioned count `3mm`, got none",
    );
    assert!(
        mismatches.iter().any(|m| m.contains("generate")),
        "ArgTypeMismatch message should reference `generate`: {:?}",
        mismatches,
    );
}

/// A dimensionless Real count `generate(2.5, …)` likewise emits `ArgTypeMismatch`
/// (the count must be `Int`, not `Real`).
#[test]
fn generate_real_count_emits_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(2.5, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    assert!(
        !arg_type_mismatch_messages(&compiled).is_empty(),
        "expected an ArgTypeMismatch for the Real count `2.5`, got none",
    );
}

/// A well-typed `Int` count `generate(3, …)` emits NO `ArgTypeMismatch` — guards
/// against a false positive on the valid call.
#[test]
fn generate_int_count_emits_no_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(3, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    let mismatches = arg_type_mismatch_messages(&compiled);
    assert!(
        mismatches.is_empty(),
        "a well-typed Int count must not emit ArgTypeMismatch, got: {:?}",
        mismatches,
    );
}

// ─── amend: a user-defined `fn generate` is NOT shadowed by the builtin ───

/// A user-defined `fn generate(...)` must WIN over the builtin combinator: the
/// call resolves to the USER function (its declared param types + return type),
/// and NEITHER the index-param Int seeding NOR the `List<body>` result typing of
/// the builtin is applied.
///
/// This pins TWO non-shadowing contracts that are otherwise untested:
///   1. The load-bearing Int-seeding suppression guard in `expr.rs`
///      (`!functions.iter().any(|f| f.name == "generate")`).
///   2. The structural result-type non-shadowing — a user fn resolves to a
///      `UserFunctionCall` (carrying its declared return type) BEFORE the builtin
///      list-helper result-type ladder is consulted.
///
/// Two assertions catch the two regression modes:
///   - Cell `xs` types to the user fn's declared return type `Length`, NOT
///     `List<Int>` / `List<Real>` → proves the builtin result typing is bypassed.
///   - Zero Error diagnostics → the *unseeded* lambda `|i| i` types `(Real) -> Real`
///     and matches the declared `f: (Real) -> Real` by exact equality. Had a
///     regression dropped the seeding guard, the lambda would be seeded to
///     `(Int) -> Int`, which no longer matches `f` → overload-resolution NoMatch →
///     an Error diagnostic, tripping this assertion.
#[test]
fn user_defined_generate_is_not_shadowed_by_builtin() {
    let source = r#"
        fn generate(count: Int, f: (Real) -> Real) -> Length {
            count * 1mm
        }
        structure S {
            let xs = generate(3, |i| i)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "a user-defined `generate` must resolve without errors (the lambda must \
         NOT be Int-seeded, so it still matches `f: (Real) -> Real`); got: {:?}",
        errors,
    );

    assert_eq!(
        cell_result_type(&compiled, "S", "xs"),
        Type::length(),
        "cell `xs` must type to the user fn's declared return type `Length`, not \
         the builtin's `List<body>` — the user `generate` is not shadowed",
    );
}

// ─── task #5385: generate() over a GEOMETRY body ───

/// Fetch a structure template by name.
fn template<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    structure: &str,
) -> &'a reify_compiler::TopologyTemplate {
    compiled
        .templates
        .iter()
        .find(|t| t.name == structure)
        .unwrap_or_else(|| panic!("{structure} template not found"))
}

/// The realizations bound to geometry-list let `list_name`, in index order.
fn list_realizations<'a>(
    template: &'a reify_compiler::TopologyTemplate,
    list_name: &str,
) -> Vec<&'a reify_compiler::RealizationDecl> {
    let mut out: Vec<_> = template
        .realizations
        .iter()
        .filter(|r| {
            r.list_binding
                .as_ref()
                .is_some_and(|b| b.list_name == list_name)
        })
        .collect();
    out.sort_by_key(|r| r.list_binding.as_ref().map(|b| b.index).unwrap_or(0));
    out
}

/// Fetch a value cell by member name.
fn value_cell<'a>(
    template: &'a reify_compiler::TopologyTemplate,
    cell: &str,
) -> &'a reify_compiler::ValueCellDecl {
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell)
        .unwrap_or_else(|| panic!("value cell '{cell}' not found in {}", template.name))
}

/// `generate(3, |i| cylinder(…))` lowers to THREE sibling realizations — one
/// per index — each named `holes#k` and tagged with its `GeometryListBinding`,
/// and the `holes` cell types as `List<Geometry>`.
///
/// This is the compile-side half of the headline repro. RED today: `generate`
/// is not a geometry function, `is_geometry_let` rejects it, zero realizations
/// are emitted, and `infer_list_helper_return_type` types the cell `List<Real>`
/// — which is exactly why it evaluates to `[undef, undef, undef]`.
#[test]
fn generate_over_geometry_emits_one_realization_per_index() {
    let source = r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "a geometry-producing generate() must compile clean; got: {errors:?}",
    );

    let t = template(&compiled, "S");
    let elements = list_realizations(t, "holes");
    assert_eq!(
        elements.len(),
        3,
        "expected 3 list-bound realizations for `holes`, got {:#?}",
        t.realizations,
    );

    for (k, r) in elements.iter().enumerate() {
        assert_eq!(
            r.list_binding,
            Some(reify_compiler::GeometryListBinding {
                list_name: "holes".to_string(),
                index: k,
                // Every sibling carries the FULL compile-time count, so eval
                // can tell a dropped element from a genuinely shorter list.
                len: 3,
            }),
        );
        assert_eq!(
            r.name.as_deref(),
            Some(format!("holes#{k}").as_str()),
            "element {k} must carry the synthetic per-element name",
        );
        assert!(
            r.operations.iter().any(|op| matches!(
                op,
                reify_compiler::CompiledGeometryOp::Primitive {
                    kind: reify_compiler::PrimitiveKind::Cylinder,
                    ..
                }
            )),
            "element {k} must carry the cylinder primitive; got {:#?}",
            r.operations,
        );
    }

    // Distinct ids — three separate realizations, not one aliased three times.
    let ids: std::collections::HashSet<_> = elements.iter().map(|r| &r.id).collect();
    assert_eq!(ids.len(), 3, "element realizations must have distinct ids");

    // The cell is a LIST of geometry — not a dimensionless scalar (today's lie)
    // and not a single Geometry.
    assert_eq!(
        value_cell(t, "holes").cell_type,
        Type::List(Box::new(Type::Geometry)),
    );
}

/// The list-literal form is the same defect with no lambda involved: a
/// geometry constructor outside geometry-let head position. It lowers to the
/// same N-sibling-realization shape.
#[test]
fn geometry_list_literal_emits_one_realization_per_element() {
    let source = r#"
        structure S {
            let parts = [cylinder(5mm, 20mm), box(1mm, 1mm, 1mm)]
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "a geometry list literal must compile clean; got: {errors:?}",
    );

    let t = template(&compiled, "S");
    let elements = list_realizations(t, "parts");
    assert_eq!(
        elements.len(),
        2,
        "expected 2 list-bound realizations for `parts`, got {:#?}",
        t.realizations,
    );
    assert_eq!(elements[0].name.as_deref(), Some("parts#0"));
    assert_eq!(elements[1].name.as_deref(), Some("parts#1"));
    assert!(
        elements[0].operations.iter().any(|op| matches!(
            op,
            reify_compiler::CompiledGeometryOp::Primitive {
                kind: reify_compiler::PrimitiveKind::Cylinder,
                ..
            }
        )),
        "parts#0 must be the cylinder; got {:#?}",
        elements[0].operations,
    );
    assert!(
        elements[1].operations.iter().any(|op| matches!(
            op,
            reify_compiler::CompiledGeometryOp::Primitive {
                kind: reify_compiler::PrimitiveKind::Box,
                ..
            }
        )),
        "parts#1 must be the box; got {:#?}",
        elements[1].operations,
    );

    assert_eq!(
        value_cell(t, "parts").cell_type,
        Type::List(Box::new(Type::Geometry)),
    );
}

/// A `generate` count that is not a literal cannot be unrolled at compile
/// time, and a realization graph cannot be sized at eval time. That is a
/// COMPILE-TIME Error naming the construct — never a silently `List<Real>`-
/// typed cell that evaluates to `[undef, …]`, which is exactly the failure
/// class this task exists to kill.
#[test]
fn generate_with_non_literal_count_over_geometry_is_a_loud_error() {
    let source = r#"
        structure S {
            param n : Int = 3
            let holes = generate(n, |i| cylinder(5mm, 20mm))
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "a geometry-producing generate() with a non-literal count must be an \
         ERROR, not a silent undef list",
    );
    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains("generate")
                && d.message.contains("literal")
                && d.message.contains("Int")
        })
        .collect();
    assert!(
        !matching.is_empty(),
        "expected an error naming generate() and the literal-Int-count \
         requirement; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
    assert!(
        matching.iter().any(|d| !d.labels.is_empty()),
        "the diagnostic must carry a span label anchored at the call",
    );

    // …and nothing is emitted for `holes`: no half-lowered realization.
    let t = template(&compiled, "S");
    assert!(
        t.realizations
            .iter()
            .all(|r| !r.name.as_deref().is_some_and(|n| n.starts_with("holes"))),
        "no realization may be emitted for the rejected let; got {:#?}",
        t.realizations,
    );
}

/// A list literal mixing geometry and non-geometry elements is likewise a
/// loud compile-time Error rather than a `List<Real>` cell of undefs.
#[test]
fn mixed_kind_geometry_list_literal_is_a_loud_error() {
    let source = r#"
        structure S {
            let bad = [cylinder(5mm, 20mm), 3]
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let matching: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("geometry") && d.message.contains("list literal"))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected an error naming the mixed-kind list literal; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
    assert!(
        matching.iter().any(|d| !d.labels.is_empty()),
        "the diagnostic must carry a span label",
    );

    let t = template(&compiled, "S");
    assert!(
        t.realizations
            .iter()
            .all(|r| !r.name.as_deref().is_some_and(|n| n.starts_with("bad"))),
        "no realization may be emitted for the rejected let; got {:#?}",
        t.realizations,
    );
}

/// Compile-level companion to `geometry_list_count_is_determinate`: the
/// `.count` of a geometry-list let is CONSTANT-FOLDED at compile time into the
/// statically-known element count, rather than surviving as a `MethodCall`
/// against a cell whose Value is authoritative only after hydration.
///
/// The KIND assertion is the load-bearing one. Typing alone is TAUTOLOGICAL
/// with respect to the fold (review esc-5385-4 S4): `expr.rs` maps
/// `"count" => Type::Int` for ANY list receiver, so `let xs = [1,2,3]
/// let n = xs.count` also types to `Int` through the ordinary un-folded path —
/// a `Type::Int` assertion therefore passes unchanged with the fold deleted
/// outright. Only `Literal(Int(3))` in the compiled tree tells the two apart.
#[test]
fn geometry_list_count_constant_folds_to_a_literal_int() {
    let source = r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let n = holes.count
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(errors.is_empty(), "compile errors: {errors:?}");

    let n_expr = value_cell(template(&compiled, "S"), "n")
        .default_expr
        .as_ref()
        .expect("cell `n` must carry a default_expr");
    assert!(
        matches!(n_expr.kind, CompiledExprKind::Literal(Value::Int(3))),
        "`holes.count` must be folded to the literal element count, not left as \
         a MethodCall on the not-yet-hydrated list cell; got: {:?}",
        n_expr.kind,
    );

    // The type is a CONSEQUENCE of the fold rather than evidence of it, but a
    // fold that produced the right count at the wrong type would still be a
    // regression — so keep it pinned.
    assert_eq!(
        cell_result_type(&compiled, "S", "n"),
        Type::Int,
        "`holes.count` must compile to an Int-typed expr",
    );

    // Control — the ordinary, UN-folded `.count` path over a scalar list.
    // It agrees on the type and disagrees on the kind, which is precisely why
    // only the kind assertion above can witness the fold.
    let unfolded = compile_source(
        r#"
        structure S {
            let xs = [1, 2, 3]
            let n = xs.count
        }
    "#,
    );
    let unfolded_expr = value_cell(template(&unfolded, "S"), "n")
        .default_expr
        .as_ref()
        .expect("cell `n` must carry a default_expr");
    assert_eq!(
        unfolded_expr.result_type,
        Type::Int,
        "control: the un-folded path types to Int too — typing cannot tell the \
         two apart",
    );
    assert!(
        !matches!(unfolded_expr.kind, CompiledExprKind::Literal(_)),
        "control: the un-folded path must NOT be a literal, or the kind \
         assertion above would be tautological too; got: {:?}",
        unfolded_expr.kind,
    );
}

// ─── task #5385: union_all/intersection_all over a List<Geometry> ───

/// Count `Boolean` ops with the given operator in the realization named `name`.
fn boolean_op_count(
    compiled: &reify_compiler::CompiledModule,
    structure: &str,
    name: &str,
    op: reify_compiler::BooleanOp,
) -> usize {
    let t = template(compiled, structure);
    let r = t
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some(name))
        .unwrap_or_else(|| {
            panic!("no realization named '{name}' in {structure}; got {:#?}", t.realizations)
        });
    r.operations
        .iter()
        .filter(|o| matches!(o, reify_compiler::CompiledGeometryOp::Boolean { op: got, .. } if *got == op))
        .count()
}

/// `union_all(<geometry-list let>)` accepts the single list argument and folds
/// it left-to-right into n-1 binary Union ops.
///
/// RED today: the single arg hits the `expects at least 2 arguments` gate.
#[test]
fn union_all_accepts_a_single_geometry_list_let() {
    let source = r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(errors.is_empty(), "union_all over a geometry list: {errors:?}");
    assert_eq!(
        boolean_op_count(&compiled, "S", "combined", reify_compiler::BooleanOp::Union),
        2,
        "a 3-element list folds to exactly n-1 = 2 Union ops",
    );
}

/// The same for a list literal passed inline, and for `intersection_all`.
#[test]
fn boolean_all_accepts_a_direct_geometry_list_literal() {
    let source = r#"
        structure S {
            let u = union_all([cylinder(5mm, 20mm), cylinder(6mm, 20mm)])
            let x = intersection_all([cylinder(5mm, 20mm), cylinder(6mm, 20mm)])
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(errors.is_empty(), "boolean-all over a list literal: {errors:?}");
    assert_eq!(
        boolean_op_count(&compiled, "S", "u", reify_compiler::BooleanOp::Union),
        1,
        "a 2-element list folds to exactly 1 Union op",
    );
    assert_eq!(
        boolean_op_count(
            &compiled,
            "S",
            "x",
            reify_compiler::BooleanOp::Intersection
        ),
        1,
        "intersection_all must fold with BooleanOp::Intersection",
    );
}

/// A single List argument that is NOT a geometry list reports THAT, rather
/// than the bare arity text — the arity message would send the user looking
/// for a second argument they do not need.
#[test]
fn union_all_over_a_non_geometry_list_reports_the_element_kind() {
    let source = r#"
        structure S {
            let xs = [1, 2, 3]
            let combined = union_all(xs)
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|m| m.contains("geometry list")),
        "expected a 'must be a geometry list' error; got: {errors:?}",
    );
    assert!(
        !errors
            .iter()
            .any(|m| m.contains("expects at least 2 arguments")),
        "the bare arity message must NOT be what the user sees here; got: {errors:?}",
    );
}

/// An EMPTY geometry list has nothing to fold — a distinct, specific error.
#[test]
fn union_all_over_an_empty_geometry_list_reports_the_empty_fold() {
    let source = r#"
        structure S {
            let holes = generate(0, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<String> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors
            .iter()
            .any(|m| m.contains("empty") && m.contains("at least one element")),
        "expected an empty-fold error naming the requirement; got: {errors:?}",
    );
}

/// An INLINE geometry list longer than `GEOMETRY_LIST_MAX_ELEMENTS` (256) must
/// be told about the CAP, in both of its shapes.
///
/// Before review esc-5385-6 the over-cap diagnostic raised inside
/// `resolve_geometry_list_arg` was discarded into a throwaway `Vec` and the
/// argument reported as `NotGeometry`, so the user was told "this collection's
/// elements are not geometry" — factually wrong (they ARE geometry) and silent
/// about the only thing they could act on. A named over-cap list is capped once
/// at its declaring let; an INLINE one has no declaring let, so the fold is the
/// only place the message can come from.
///
/// The cap is `pub(crate)`, so 256/257 are spelled out here: an integration test
/// cannot import the constant. If the constant moves, the message assertion
/// fails loudly rather than silently passing on a stale number.
#[test]
fn union_all_over_an_over_cap_inline_geometry_list_reports_the_cap() {
    // (i) generate form.
    let generated = compile_source(
        r#"
        structure S {
            let u = union_all(generate(257, |i| box(1mm, 1mm, 1mm)))
        }
    "#,
    );
    let errors = error_messages(&generated);
    assert!(
        errors
            .iter()
            .any(|m| m.contains("limited to 256 geometry elements") && m.contains("257")),
        "expected the element-cap error naming the cap and the actual count; \
         got: {errors:?}",
    );
    assert!(
        !errors.iter().any(|m| m.contains("must be a geometry list")),
        "the elements ARE geometry — the not-a-geometry-list message would send \
         the user hunting a type error that does not exist; got: {errors:?}",
    );

    // (ii) list-literal form, built programmatically (257 hand-written elements
    // would be unreadable). Same cap, same message — the cap binds the
    // realization blow-up, not the syntax that produced it.
    let elements = vec!["box(1mm, 1mm, 1mm)"; 257].join(", ");
    let literal = compile_source(&format!(
        "structure S {{\n    let u = union_all([{elements}])\n}}"
    ));
    let literal_errors = error_messages(&literal);
    assert!(
        literal_errors
            .iter()
            .any(|m| m.contains("limited to 256 geometry elements") && m.contains("257")),
        "an over-cap inline LITERAL must report the cap too; got: {literal_errors:?}",
    );

    // Control: one element fewer compiles clean, so the assertions above are
    // pinning the cap rather than some unrelated failure of large fixtures.
    let at_cap = compile_source(
        r#"
        structure S {
            let u = union_all(generate(256, |i| box(1mm, 1mm, 1mm)))
        }
    "#,
    );
    assert!(
        error_messages(&at_cap).is_empty(),
        "control: exactly at the cap must compile clean; got: {:?}",
        error_messages(&at_cap),
    );
}

/// Regression: a single NON-list geometry argument is unchanged — it still
/// gets the arity message pinned by
/// `geometry_arg_count_span_tests::union_all_arg_count_diagnostic_has_span_label`.
#[test]
fn union_all_over_a_single_non_list_geometry_still_reports_arity() {
    let source = r#"
        structure S {
            let u = union_all(box(10mm, 10mm, 10mm))
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors
            .iter()
            .any(|m| m.contains("union_all() expects at least 2 arguments")),
        "a single non-list geometry arg must still hit the arity gate; got: {errors:?}",
    );
}

/// A list element that is a SELECTOR composition (`union(<selector let>,
/// <selector let>)`) must not be folded as CSG geometry.
///
/// `is_geometry_let`'s `is_selector_composition` guard (task 4119 δ) tells CSG
/// `union`/`difference` apart from selector-algebra `union`/`difference` by
/// asking whether any operand is selector-valued — and for an IDENT operand
/// that question is answered entirely by the `known_selector_lets` set handed
/// to it. `resolve_geometry_list_arg` passed an EMPTY one before review
/// esc-5385-6, so this inline list classified as geometry here while entity.rs
/// pass 1 — which has the real set — routes the same expression to the selector
/// path: two classifiers, one expression, opposite answers.
///
/// The observable consequence is the discriminator: with the empty set the fold
/// emits CSG Boolean ops over selector operands; with the set derived from the
/// scope's registered types it does not, and the user gets a diagnostic instead.
#[test]
fn union_all_over_a_list_holding_a_selector_composition_is_not_folded_as_csg() {
    let compiled = compile_source_with_stdlib(
        r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let c = box(20mm, 20mm, 20mm)
            let sel_a = faces(b)
            let sel_b = faces(c)
            let combined = union_all([union(sel_a, sel_b), box(1mm, 1mm, 1mm)])
        }
    "#,
    );

    assert_eq!(
        total_boolean_ops(&compiled, "S"),
        0,
        "no CSG Boolean op may be emitted: one of the two list elements is a \
         SELECTOR composition, not geometry",
    );
    let errors = error_messages(&compiled);
    assert!(
        errors.iter().any(|m| m.contains("geometry list")),
        "the fold must be refused with a geometry-list diagnostic rather than \
         silently folding selector algebra as CSG; got: {errors:?}",
    );

    // Control: same fixture with both elements real geometry folds normally, so
    // the zero above is the selector guard rather than an artefact of compiling
    // a list literal with stdlib loaded.
    let control = compile_source_with_stdlib(
        r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let combined = union_all([union(b, b), box(1mm, 1mm, 1mm)])
        }
    "#,
    );
    assert!(
        error_messages(&control).is_empty(),
        "control: an all-geometry list must still compile clean; got: {:?}",
        error_messages(&control),
    );
    assert!(
        total_boolean_ops(&control, "S") > 0,
        "control: an all-geometry list must still emit Boolean ops",
    );
}

/// S6 (review esc-5385-3): a list that expands to EXACTLY ONE element is the
/// sole justification for bypassing `union_all`'s `>= 2` arity gate, so pin it.
///
/// The fold degenerates to "the element itself, zero Boolean ops", which is
/// well-defined — but nothing exercised it before, at either level. Its
/// two-element and three-element siblings are covered above.
#[test]
fn union_all_over_a_one_element_geometry_list_compiles_clean() {
    let source = r#"
        structure S {
            let holes = generate(1, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "a one-element geometry list must fold without hitting the arity gate; \
         got: {errors:?}",
    );
    // The fold must still produce its OWN realization rather than silently
    // aliasing the single element — otherwise "zero Boolean ops" would mean
    // "no realization at all" and `combined` would have nothing to resolve to.
    let t = template(&compiled, "S");
    assert!(
        t.realizations
            .iter()
            .any(|r| r.name.as_deref() == Some("combined")),
        "`combined` must be its own realization; got: {:#?}",
        t.realizations.iter().map(|r| &r.name).collect::<Vec<_>>(),
    );
}

/// S8 (review esc-5385-3): a geometry-list let that was already rejected with
/// its own Error must NOT cascade a second one from a fold over it.
///
/// entity.rs registers the rejected name as `List<Geometry>` with the comment
/// "so downstream references type-check rather than cascade a second,
/// unrelated diagnostic". That was untrue for the boolean folds:
/// `resolve_geometry_list_arg` found a `List<…>` with no cached elements and
/// reported "must be a geometry list", pointing the user at the fold instead
/// of at the non-literal count that is the actual defect.
#[test]
fn a_rejected_geometry_list_let_does_not_cascade_into_its_fold() {
    let source = r#"
        structure S {
            param n : Int = 3
            let holes = generate(n, |i| cylinder(5mm, 20mm))
            let combined = union_all(holes)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "exactly one Error — the non-literal count — must survive; got: {errors:?}",
    );
    assert!(
        errors[0].contains("literal non-negative Int count"),
        "the surviving Error must be the declaring let's, not the fold's; \
         got: {errors:?}",
    );
    assert!(
        !errors.iter().any(|m| m.contains("must be a geometry list")),
        "the fold must stay silent about a let that already failed loudly; \
         got: {errors:?}",
    );
}

/// Negative control for the suppression above — and, unlike its predecessor, a
/// REAL one (review esc-5385-4 S4).
///
/// The version this replaces compiled `let xs = [1,2,3]  let combined =
/// union_all(xs)` on its own: a strict duplicate of
/// `union_all_over_a_non_geometry_list_reports_the_element_kind` with a subset
/// of its assertions, and no control at all over `GeometryListArg::
/// AlreadyDiagnosed` — `xs` never enters `geometry_list_rejected`, so the mute
/// it claimed to be the control for was never in play.
///
/// This version puts BOTH folds in ONE structure, so the two dispositions are
/// decided against the same scope in the same compile:
///   * `union_all(rejected)` — the let already failed loudly, so the fold adds
///     nothing (`AlreadyDiagnosed`);
///   * `union_all(xs)` — an ordinary non-geometry list, so the fold speaks.
///
/// A blanket mute over every single-argument fold loses the second Error; a
/// lost mute adds a third. Only the exact pairing passes, and the label span
/// settles which fold the surviving Error belongs to.
#[test]
fn a_scalar_fold_still_speaks_beside_a_muted_rejected_lets_fold() {
    let source = r#"
        structure S {
            param k : Int = 3
            let rejected = generate(k, |i| cylinder(5mm, 20mm))
            let xs = [1, 2, 3]
            let quiet = union_all(rejected)
            let loud = union_all(xs)
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        errors.len(),
        2,
        "exactly two Errors — the rejected let's own, and the scalar fold's; \
         got: {messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("literal non-negative Int count")),
        "the declaring let must still report its own defect; got: {messages:?}",
    );

    let fold_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("must be a geometry list"))
        .collect();
    assert_eq!(
        fold_errors.len(),
        1,
        "the fold's own diagnostic must fire exactly once — for `xs`, never for \
         the already-diagnosed `rejected`; got: {messages:?}",
    );

    // The span is what proves WHICH fold the survivor belongs to: `union_all(xs)`.
    let labelled: Vec<&str> = fold_errors[0]
        .labels
        .iter()
        .filter_map(|l| source.get(l.span.start as usize..l.span.end as usize))
        .collect();
    assert_eq!(
        labelled,
        vec!["xs"],
        "the surviving fold Error must be anchored at the scalar list argument, \
         not at the rejected geometry-list let",
    );
}

// ─── task #5385 (review esc-5385-4 S5): shadowing must defeat the union_all
//     EXPANSION too, not only the `.count` fold ───

/// Every `Boolean` op across EVERY realization of `structure`.
///
/// A suppressed fold emits no realization to name, so the total is the signal:
/// the fixtures below contain no other boolean geometry, and a fold that
/// wrongly expanded a shadowed name to the outer list's elements would show up
/// here as n-1 Union ops per expanded element.
fn total_boolean_ops(compiled: &reify_compiler::CompiledModule, structure: &str) -> usize {
    template(compiled, structure)
        .realizations
        .iter()
        .flat_map(|r| r.operations.iter())
        .filter(|o| matches!(o, reify_compiler::CompiledGeometryOp::Boolean { .. }))
        .count()
}

/// A value cell's compiled `default_expr`. The compiled tree is where a
/// compile-time rewrite is unambiguously present or absent, and the
/// `ValueCellId`s it carries name the entity each identifier resolved against —
/// which is exactly the shadowing question.
///
/// The shadowing tests below match on this tree STRUCTURALLY rather than on its
/// `Debug` rendering (review esc-5385-6). A rendering probe puts a field name or
/// a derived-`Debug` spelling on the critical path of a NEGATIVE assertion, and
/// a rename would make those pass vacuously — the dangerous direction.
fn default_expr<'a>(
    compiled: &'a reify_compiler::CompiledModule,
    structure: &str,
    cell: &str,
) -> &'a CompiledExpr {
    value_cell(template(compiled, structure), cell)
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{cell}' has no default_expr"))
}

/// The bodies of a `Match` expression's arms, in source order.
fn match_arm_bodies(expr: &CompiledExpr) -> Vec<&CompiledExpr> {
    let CompiledExprKind::Match { arms, .. } = &expr.kind else {
        panic!("expected a compiled Match; got {:#?}", expr.kind);
    };
    arms.iter().map(|arm| &arm.body).collect()
}

/// The single `ValueCellId` argument of a call to `name` — i.e. exactly which
/// cell `union_all(<ident>)` resolved its argument against.
fn sole_call_arg_cell<'a>(expr: &'a CompiledExpr, name: &str) -> &'a ValueCellId {
    let CompiledExprKind::FunctionCall { function, args } = &expr.kind else {
        panic!("expected a compiled call to {name}(); got {:#?}", expr.kind);
    };
    assert_eq!(function.name, name, "expected a call to {name}()");
    let [only] = args.as_slice() else {
        panic!("expected {name}() to have exactly one argument; got {args:#?}");
    };
    let CompiledExprKind::ValueRef(id) = &only.kind else {
        panic!("expected {name}()'s argument to be a ValueRef; got {:#?}", only.kind);
    };
    id
}

/// A LAMBDA PARAM that shadows a geometry-list let must not expand to that
/// let's ELEMENTS when folded. This is the `union_all` half of the shadowing
/// guard added for review esc-5385-3; its `.count` half is pinned by
/// `lambda_param_shadowing_a_geometry_list_let_does_not_inherit_its_count`
/// (crates/reify-eval/tests/generate_eval.rs).
///
/// The forbidden regression is silent: the inner `holes` expanding to the OUTER
/// let's three cylinders would make each `combined` element a two-Union fold
/// over geometry the user never named there — a wrong answer with no
/// diagnostic. The outcome demanded instead is a DIAGNOSTIC.
///
/// Self-discriminating: the second compile is the SAME fixture with the binder
/// renamed `j`, so `holes` is no longer shadowed and the fold does run. One
/// identifier separates 0 Boolean ops + an Error from 4 Boolean ops + a clean
/// compile, which is what makes the zero above non-vacuous.
///
/// Note on mechanism: a geometry `generate` body is index-substituted
/// (`substitute_index_ident`, itself shadowing-aware) BEFORE
/// `resolve_geometry_list_arg` sees it, so today the shadowed `holes` is
/// already the literal index by the time the fold is classified, and it is the
/// substitution — not `geometry_list_binding_is_live` — that does the work.
/// The test pins the CONTRACT from the outside, so it holds whichever layer
/// enforces it.
#[test]
fn lambda_param_shadowing_a_geometry_list_let_is_not_expanded_by_union_all() {
    let compiled = compile_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let combined = generate(2, |holes| union_all(holes))
        }
    "#,
    );

    let errors = error_messages(&compiled);
    assert!(
        !errors.is_empty(),
        "a shadowed name under union_all must be diagnosed, never silently \
         expanded to the outer list's elements",
    );
    assert_eq!(
        total_boolean_ops(&compiled, "S"),
        0,
        "no Boolean op may be emitted — the only geometry in S is the outer \
         list's three cylinders, and the shadowed fold must not reach them",
    );
    // The outer list itself is untouched: the guard suppresses the FOLD, not
    // the declaring let.
    assert_eq!(
        list_realizations(template(&compiled, "S"), "holes").len(),
        3,
    );
    // …and `combined`'s compiled argument resolves to the lambda's OWN binder
    // cell (`$lambdaN.S`), not to the outer `S.holes` list cell. Read off the
    // tree, not off its `Debug` rendering (review esc-5385-6):
    // `generate(2, |holes| union_all(holes))` compiles to
    // `FunctionCall(generate)[Literal, Lambda{ body: FunctionCall(union_all) }]`.
    let combined = default_expr(&compiled, "S", "combined");
    let CompiledExprKind::FunctionCall { args, .. } = &combined.kind else {
        panic!("expected `combined` to compile to a generate() call; got {combined:#?}");
    };
    let CompiledExprKind::Lambda { body, .. } = &args[1].kind else {
        panic!("expected generate()'s second argument to be a Lambda; got {:#?}", args[1]);
    };
    let folded = sole_call_arg_cell(body, "union_all");
    assert_ne!(
        *folded,
        ValueCellId::new("S", "holes"),
        "the compiled fold must not reference the OUTER geometry-list cell",
    );
    assert!(
        folded.entity.starts_with("$lambda"),
        "…it must resolve to the lambda's own binder cell; got {folded:?}",
    );

    // Positive control: same fixture, non-shadowing binder name.
    let unshadowed = compile_source(
        r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let combined = generate(2, |j| union_all(holes))
        }
    "#,
    );
    assert!(
        error_messages(&unshadowed).is_empty(),
        "control: without the shadow the fold is legal; got: {:?}",
        error_messages(&unshadowed),
    );
    assert_eq!(
        total_boolean_ops(&unshadowed, "S"),
        4,
        "control: without the shadow each of the 2 elements folds the 3-element \
         list into n-1 = 2 Union ops",
    );
}

/// A MATCH-ARM PAYLOAD BINDER that shadows a geometry-list let must not expand
/// to that let's elements either — the third derived-scope leg (`$matcharmN`),
/// which review esc-5385-4 S5 found unpinned at every level.
///
/// What fires TODAY is the geometry-typed-match rejection (task 3418), which
/// sits upstream of the fold: `compile_geometry_call` refuses the whole `match`
/// before `resolve_geometry_list_arg` is ever consulted, so the arm's
/// `union_all(holes)` never expands anything. That is a diagnostic, not a
/// silently-wrong fold, which is the contract this test exists to pin — and it
/// is deliberately asserted at the contract level (an Error exists, zero
/// Boolean ops, no reference to the outer cell) so that if geometry `match`
/// ever becomes supported, the guard in `resolve_geometry_list_arg` becomes the
/// thing holding this test up rather than the test needing to be rewritten.
#[test]
fn match_arm_binder_shadowing_a_geometry_list_let_is_not_expanded_by_union_all() {
    let compiled = compile_source(
        r#"
        enum Mode { Fold { parts: Solid }, Plain }
        structure S {
            param m : Mode = Mode.Plain
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let combined = match m {
                Fold { parts: holes } => union_all(holes),
                Plain => box(1mm, 1mm, 1mm),
            }
        }
    "#,
    );

    let errors = error_messages(&compiled);
    assert!(
        !errors.is_empty(),
        "a match-arm binder shadowing a geometry list must be diagnosed, never \
         silently folded over the outer list",
    );
    assert_eq!(
        total_boolean_ops(&compiled, "S"),
        0,
        "no Boolean op may be emitted for the shadowed fold; got {:#?}",
        template(&compiled, "S")
            .realizations
            .iter()
            .map(|r| (&r.name, r.operations.len()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        list_realizations(template(&compiled, "S"), "holes").len(),
        3,
        "the declaring let is untouched — only the fold over the shadowed name \
         is refused",
    );

    // The arm body's argument resolves to the arm's OWN payload binder
    // (`$matcharmN.S`), never to the outer `S.holes` list cell — read off the
    // compiled arm rather than its `Debug` rendering (review esc-5385-6).
    let combined = default_expr(&compiled, "S", "combined");
    let bodies = match_arm_bodies(combined);
    let fold_body = bodies
        .iter()
        .find(|b| {
            matches!(&b.kind, CompiledExprKind::FunctionCall { function, .. }
                     if function.name == "union_all")
        })
        .unwrap_or_else(|| panic!("no arm body is a union_all() call; got {bodies:#?}"));
    let folded = sole_call_arg_cell(fold_body, "union_all");
    assert!(
        folded.entity.starts_with("$matcharm"),
        "`union_all`'s argument must resolve to the arm's payload binder; \
         got {folded:?}",
    );
    assert_ne!(
        *folded,
        ValueCellId::new("S", "holes"),
        "the compiled arm must not reference the OUTER geometry-list cell",
    );
}

/// The `.count` leg of the same match-arm binder — the one derived scope that
/// neither of the eval-side shadowing tests covers (review esc-5385-4 S5:
/// "the match-arm-binder leg is unpinned for both").
///
/// Unlike the two `union_all` tests above, this one reaches
/// `geometry_list_binding_is_live` for real. The arm body is compiled by
/// `compile_expr` in a cloned `$matcharm0.S` scope that inherits
/// `geometry_list_lens` VERBATIM and registers the binder in `names` only, so a
/// bare-name lookup keyed on `"holes"` finds the outer let's length. Without
/// the gate the arm body folds to `Literal(Int(3))` — the OUTER list's length,
/// silently, for a binder the caller supplied as a `List<Int>`.
///
/// Self-discriminating: the second compile is the SAME fixture with the binder
/// renamed `ys`. `holes` is then unshadowed, the fold is CORRECT, and the arm
/// body really does become `Literal(Int(3))` — so the negative assertion above
/// is pinning the guard, not an accident of the fixture.
#[test]
fn match_arm_binder_shadowing_a_geometry_list_let_does_not_inherit_its_count() {
    let compiled = compile_source(
        r#"
        enum Tally { Counted { xs: List<Int> }, Unknown }
        structure S {
            param t : Tally = Tally.Unknown
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let n = match t {
                Counted { xs: holes } => holes.count,
                Unknown => 0,
            }
        }
    "#,
    );

    let errors = error_messages(&compiled);
    assert!(errors.is_empty(), "compile errors: {errors:?}");

    // Structural, not `Debug`-rendered (review esc-5385-6): the fold, had it
    // fired, would have REPLACED this arm body with `Literal(Int(3))`, so the
    // arm bodies are the complete set of places it could appear.
    let n = default_expr(&compiled, "S", "n");
    let bodies = match_arm_bodies(n);
    let count_call = bodies
        .iter()
        .find(|b| matches!(&b.kind, CompiledExprKind::MethodCall { method, .. } if method == "count"))
        .unwrap_or_else(|| {
            panic!(
                "the binder's `.count` must survive as a MethodCall on the bound \
                 payload rather than be folded away; got {bodies:#?}"
            )
        });
    let CompiledExprKind::MethodCall { object, .. } = &count_call.kind else {
        unreachable!("selected by the MethodCall predicate above");
    };
    let CompiledExprKind::ValueRef(receiver) = &object.kind else {
        panic!("`.count`'s receiver must be a ValueRef; got {:#?}", object.kind);
    };
    assert!(
        receiver.entity.starts_with("$matcharm"),
        "…and its receiver must be the arm's own binder cell; got {receiver:?}",
    );
    assert_ne!(
        *receiver,
        ValueCellId::new("S", "holes"),
        "…never the OUTER geometry-list cell",
    );
    assert!(
        !bodies
            .iter()
            .any(|b| matches!(&b.kind, CompiledExprKind::Literal(Value::Int(3)))),
        "no folded `Int(3)` may appear in any arm body — that is the OUTER \
         geometry list's length leaking past a shadowing binder; got {bodies:#?}",
    );

    // Positive control: same fixture, non-shadowing binder name — the fold is
    // legitimate there and DOES produce the literal.
    let unshadowed = compile_source(
        r#"
        enum Tally { Counted { xs: List<Int> }, Unknown }
        structure S {
            param t : Tally = Tally.Unknown
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let n = match t {
                Counted { xs: ys } => holes.count,
                Unknown => 0,
            }
        }
    "#,
    );
    let control_bodies = match_arm_bodies(default_expr(&unshadowed, "S", "n"));
    assert!(
        control_bodies
            .iter()
            .any(|b| matches!(&b.kind, CompiledExprKind::Literal(Value::Int(3)))),
        "control: an UNshadowed `holes.count` inside the same match arm must \
         still fold to the outer list's length; got {control_bodies:#?}",
    );
}

