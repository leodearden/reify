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

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

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
/// `.count` of a geometry-list let types to `Int`, because it is folded to the
/// statically-known element count at compile time rather than evaluated
/// against a cell whose Value is authoritative only after hydration.
#[test]
fn geometry_list_count_cell_types_to_int() {
    let source = r#"
        structure S {
            let holes = generate(3, |i| cylinder(5mm, 20mm))
            let n = holes.count
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(errors.is_empty(), "compile errors: {errors:?}");

    assert_eq!(
        cell_result_type(&compiled, "S", "n"),
        Type::Int,
        "`holes.count` must compile to an Int-typed expr",
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

/// Negative control for the suppression above: a genuinely non-geometry list
/// still gets the fold's own diagnostic, so `AlreadyDiagnosed` is not a
/// blanket mute on every single-argument fold.
#[test]
fn union_all_over_a_scalar_list_still_reports_a_non_geometry_list() {
    let source = r#"
        structure S {
            let xs = [1, 2, 3]
            let combined = union_all(xs)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.iter().any(|m| m.contains("must be a geometry list")),
        "a scalar list must still be diagnosed by the fold; got: {errors:?}",
    );
}
