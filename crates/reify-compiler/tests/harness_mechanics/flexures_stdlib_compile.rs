#![allow(clippy::doc_overindented_list_items)]
//! Tests for `crates/reify-compiler/stdlib/flexures.ri` —
//! `std.flexures` module: `RotationalStiffness` alias, `FlexureCompliance`
//! structure_def, and the `flexure_compliance(joint)` accessor stdlib fn —
//! all in a single module, enabled by the skeleton pre-pass (task 3895).
//!
//! Observable signal for PRD §11 Phase 1 label β
//! (docs/prds/v0_3/compliant-joints-flexures.md). Per the PRD, this file
//! parses the structure_def and confirms the compiled shape matches the
//! PRD §4.2 spec.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `trajectory_stdlib_compile.rs` / `buckling_stdlib_compile.rs`),
//! that `FlexureCompliance` is correctly represented in the compiled module,
//! and that the `yield_margin <= 1` dimensionless-ratio bound on
//! `FlexureCompliance` is declared at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper pattern in `trajectory_stdlib_compile.rs`.

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;
use reify_test_support::compile_source_with_stdlib;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/flexures` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/flexures")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/flexures module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/flexures` module
/// (task 3895: structure_def and accessor fn now live in the single module).
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/flexures, \
                 got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect the param-kind value cells (ignoring `let` and auto cells) from a
/// template, returning them in the file order they were declared.
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
/// Mirrors `buckling_stdlib_compile.rs::require_default`.
fn require_default<'a>(template: &'a TopologyTemplate, member: &str) -> &'a CompiledExpr {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{}.{} missing", template.name, member));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member))
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/flexures module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_flexures_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in flexures.ri: {:?}",
        errors
    );
}

// ─── step-3: RotationalStiffness alias ───────────────────────────────────────

/// Task 3849 landed the dimensioned `RotationalStiffness` (and its siblings
/// `TranslationalStiffness` / `RotationalDamping` / `TranslationalDamping`) as
/// built-in `NAMED_DIMENSIONS`, so the former `pub type RotationalStiffness =
/// Real` placeholder alias in `flexures.ri` is DELETED (task 4547, Disposition
/// 2). The alias was dead: a `NAMED_DIMENSIONS` lookup takes priority over a
/// module type-alias, so `effective_stiffness : RotationalStiffness` already
/// resolved to the built-in dimension (see
/// `flexure_compliance_struct_has_correct_param_shape`); the alias only carried
/// a false doc claim that the dimensioned type "has not been filed yet".
///
/// Test pins the post-deletion invariant: (a) NO `RotationalStiffness` entry
/// remains in `module.type_aliases`; (b) `effective_stiffness` STILL resolves to
/// the built-in `DimensionVector::ROTATIONAL_STIFFNESS` (the built-in dimension
/// shadows the now-absent alias), exactly as its three aliasless siblings already
/// do in `kinematic.ri` (Prismatic/Revolute spring_rate/damping).
#[test]
fn rotational_stiffness_alias_removed_builtin_dimension_shadows() {
    let module = load_stdlib_module();

    assert!(
        !module
            .type_aliases
            .iter()
            .any(|a| a.name == "RotationalStiffness"),
        "the dead `pub type RotationalStiffness = Real` alias must be DELETED \
         (task 3849 landed the built-in ROTATIONAL_STIFFNESS dimension that \
         already shadows it); got type_aliases: {:?}",
        module
            .type_aliases
            .iter()
            .map(|a| &a.name)
            .collect::<Vec<_>>()
    );

    // The deletion is safe: the `effective_stiffness` param annotation
    // `RotationalStiffness` resolves via NAMED_DIMENSIONS (not the alias), so the
    // field keeps its built-in ROTATIONAL_STIFFNESS dimension.
    let template = find_structure("FlexureCompliance");
    let params = param_cells(template);
    let effective_stiffness = params
        .iter()
        .find(|vc| vc.id.member == "effective_stiffness")
        .expect("FlexureCompliance must have an effective_stiffness param");
    assert_eq!(
        effective_stiffness.cell_type,
        Type::Scalar {
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        "effective_stiffness must resolve to the built-in ROTATIONAL_STIFFNESS \
         dimension even with the alias deleted; got: {:?}",
        effective_stiffness.cell_type
    );
}

// ─── step-5: FlexureCompliance param shape ───────────────────────────────────

/// `FlexureCompliance` is the value-type container for the PRD §4.2 seven-
/// field flexure compliance contract. Per PRD §4.2:
///
///   - `effective_stiffness   : RotationalStiffness`    (proper dimensioned
///                                                       type kg·m²·s⁻²·rad⁻²
///                                                       per task α; PRD
///                                                       §4.2 spelling)
///   - `max_stress            : Pressure`               (at range endpoint)
///   - `max_stress_at_neutral : Pressure`               (zero unless preloaded)
///   - `yield_margin          : Real`                   ((yield-max_stress)/yield)
///   - `parasitic_error       : Option<Length>`         (compound-flexure
///                                                       orthogonal-DOF motion)
///   - `prb_validity_range    : Range<Angle>`            (tightened from Real
///                                                       by task 4576)
///   - `at_yield              : Bool`                   (true if
///                                                       max_stress >= yield)
///
/// `RotationalStiffness` resolves to `Type::Scalar { dimension:
/// DimensionVector::ROTATIONAL_STIFFNESS }` via NAMED_DIMENSIONS (task 3849
/// registered the proper dimensioned type in dimension.rs). The former
/// `pub type RotationalStiffness = Real` placeholder alias in flexures.ri was
/// DELETED (task 4547, Disposition 2): a NAMED_DIMENSIONS lookup always
/// shadowed it, so its removal is behaviour-preserving.
/// `Pressure` resolves to `Type::Scalar { dimension:
/// DimensionVector::PRESSURE }` via the standard dimensioned-type path.
/// `Range<Angle>` now resolves via the Range arm added to
/// `type_resolution.rs::resolve_parameterized_builtin_type` (task 4576).
///
/// Test pins three invariants: (a) exactly 7 param cells (no accidental
/// 8th field), (b) declaration order matches the canonical order above,
/// (c) each param's resolved `cell_type` matches the canonical expected
/// type. Defaults and the `yield_margin <= 1` constraint are pinned in
/// later steps (step-7, step-9).
#[test]
fn flexure_compliance_struct_has_correct_param_shape() {
    let template = find_structure("FlexureCompliance");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        7,
        "FlexureCompliance should have exactly 7 param cells \
         (effective_stiffness, max_stress, max_stress_at_neutral, \
         yield_margin, parasitic_error, prb_validity_range, at_yield); \
         got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "effective_stiffness",
            Type::Scalar {
                dimension: DimensionVector::ROTATIONAL_STIFFNESS,
            },
        ),
        (
            "max_stress",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        (
            "max_stress_at_neutral",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("yield_margin", Type::dimensionless_scalar()),
        (
            "parasitic_error",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        ),
        (
            "prb_validity_range",
            Type::Range(Box::new(Type::Scalar {
                dimension: DimensionVector::ANGLE,
            })),
        ),
        ("at_yield", Type::Bool),
    ];

    // Param declaration order is part of the contract — pin it explicitly
    // (mirrors the order-sensitive assertion in
    // `trajectory_stdlib_compile.rs::waypoint_struct_has_correct_param_shape`).
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "FlexureCompliance params must be declared in canonical order \
         (effective_stiffness, max_stress, max_stress_at_neutral, \
         yield_margin, parasitic_error, prb_validity_range, at_yield); \
         got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "FlexureCompliance missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "FlexureCompliance.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── step-7: FlexureCompliance literal-valued defaults ───────────────────────

/// Every `FlexureCompliance` param must carry a sentinel-zero literal
/// default so the stub `flexure_compliance(joint)` accessor — which
/// currently returns `FlexureCompliance()` until λ wires real cache lookup
/// (PRD §11 task λ) — has a well-typed value to return. The defaults
/// expected (per the module header §6.* sentinel-zero rationale):
///
///   effective_stiffness   = 0N*m/rad^2 (Scalar { ROTATIONAL_STIFFNESS, si_value 0.0 })
///   max_stress            = 0Pa    (Scalar { PRESSURE, si_value 0.0 })
///   max_stress_at_neutral = 0Pa    (Scalar { PRESSURE, si_value 0.0 })
///   yield_margin          = 0.0    (Real)
///   parasitic_error       = none   (CompiledExprKind::OptionNone)
///   prb_validity_range    = 0.0    (Real placeholder for Range<Angle>)
///   at_yield              = false  (Bool)
///
/// Strict-equality discipline for real-valued defaults mirrors the
/// `cg_tolerance` precedent in `solver_elastic_tests.rs:336-346`: IEEE-754
/// round-to-nearest is deterministic on the same decimal input, so strict
/// equality catches silent regressions (e.g., `1e-12` instead of `0.0`)
/// that an absolute-tolerance check would let through.
///
/// For the Real-typed defaults we accept either `Literal(Value::Real(0.0))`
/// or `Literal(Value::Int(0))` — mirrors the future-proofing rationale at
/// `solver_elastic_tests.rs:567-579` and `buckling_stdlib_compile.rs:356-358`
/// (`= 0` could lex as Int or Real depending on the literal coercion path,
/// and we want this test robust against a future literal-typing change).
#[test]
fn flexure_compliance_params_have_literal_defaults() {
    let template = find_structure("FlexureCompliance");

    // effective_stiffness = 0N*m/rad^2 — Scalar{ROTATIONAL_STIFFNESS, si_value 0.0}
    // (task 4547 D2: dimensioned-zero default; was a bare `0.0` under the
    // now-deleted `RotationalStiffness = Real` alias). The compound-unit literal
    // folds to a single Scalar, same as the `1N*m/rad^2` fold proven in
    // flexure_dimension_types.rs and the sibling `0Pa` defaults below.
    let effective_stiffness_default = require_default(template, "effective_stiffness");
    match &effective_stiffness_default.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::ROTATIONAL_STIFFNESS,
                "effective_stiffness default should carry ROTATIONAL_STIFFNESS \
                 dimension (= 0N*m/rad^2); got: {:?}",
                dimension
            );
            assert_eq!(
                *si_value, 0.0,
                "effective_stiffness default si_value should be exactly 0.0 \
                 (= 0N*m/rad^2); got: {}",
                si_value
            );
        }
        other => panic!(
            "effective_stiffness default should be Literal(Value::Scalar \
             {{ ROTATIONAL_STIFFNESS, 0.0 }}) (= 0N*m/rad^2); got: {:?}",
            other
        ),
    }

    // max_stress = 0Pa — Scalar{PRESSURE, si_value 0.0}.
    let max_stress_default = require_default(template, "max_stress");
    match &max_stress_default.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "max_stress default should carry PRESSURE dimension; got: {:?}",
                dimension
            );
            assert_eq!(
                *si_value, 0.0,
                "max_stress default si_value should be exactly 0.0 (= 0Pa); got: {}",
                si_value
            );
        }
        other => panic!(
            "max_stress default should be Literal(Value::Scalar {{ PRESSURE, 0.0 }}) \
             (= 0Pa); got: {:?}",
            other
        ),
    }

    // max_stress_at_neutral = 0Pa — same shape as max_stress.
    let max_stress_at_neutral_default = require_default(template, "max_stress_at_neutral");
    match &max_stress_at_neutral_default.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "max_stress_at_neutral default should carry PRESSURE dimension; got: {:?}",
                dimension
            );
            assert_eq!(
                *si_value, 0.0,
                "max_stress_at_neutral default si_value should be exactly 0.0 (= 0Pa); got: {}",
                si_value
            );
        }
        other => panic!(
            "max_stress_at_neutral default should be Literal(Value::Scalar \
             {{ PRESSURE, 0.0 }}) (= 0Pa); got: {:?}",
            other
        ),
    }

    // yield_margin = 0(.0) — accept Int(0) or Real(0.0).
    let yield_margin_default = require_default(template, "yield_margin");
    match &yield_margin_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {}
        CompiledExprKind::Literal(Value::Int(0)) => {}
        other => panic!(
            "yield_margin default should be Literal(Value::Real(0.0)) or \
             Literal(Value::Int(0)); got: {:?}",
            other
        ),
    }

    // parasitic_error = none — CompiledExprKind::OptionNone (per
    // option_compile_tests.rs:78 / :205 / multi_load_case_stdlib_tests.rs:209).
    let parasitic_error_default = require_default(template, "parasitic_error");
    assert!(
        matches!(&parasitic_error_default.kind, CompiledExprKind::OptionNone),
        "parasitic_error default should be CompiledExprKind::OptionNone (= `none`); \
         got: {:?}",
        parasitic_error_default.kind
    );

    // prb_validity_range = 0deg..0deg — sentinel-zero Range<Angle> (task 4576).
    // RED until step-6 changes flexures.ri param type to Range<Angle> with default 0deg..0deg.
    let prb_validity_range_default = require_default(template, "prb_validity_range");
    match &prb_validity_range_default.kind {
        CompiledExprKind::RangeConstructor {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(
                lower_inclusive,
                "prb_validity_range default lower_inclusive should be true (0deg..0deg)"
            );
            assert!(
                upper_inclusive,
                "prb_validity_range default upper_inclusive should be true (0deg..0deg)"
            );
            let check_zero_angle = |opt: &Option<Box<CompiledExpr>>, label: &str| {
                let expr = opt
                    .as_deref()
                    .unwrap_or_else(|| panic!("prb_validity_range default {label} bound missing"));
                match &expr.kind {
                    CompiledExprKind::Literal(Value::Scalar {
                        si_value,
                        dimension,
                    }) => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::ANGLE,
                            "prb_validity_range default {label} bound should have ANGLE dimension; got: {:?}",
                            dimension
                        );
                        assert_eq!(
                            *si_value, 0.0,
                            "prb_validity_range default {label} bound si_value should be 0.0 (= 0deg); got: {}",
                            si_value
                        );
                    }
                    other => panic!(
                        "prb_validity_range default {label} bound should be Literal(Scalar{{ANGLE, 0.0}}); got: {:?}",
                        other
                    ),
                }
            };
            check_zero_angle(lower, "lower");
            check_zero_angle(upper, "upper");
        }
        other => panic!(
            "prb_validity_range default should be RangeConstructor{{0deg..0deg}} (task 4576); got: {:?}",
            other
        ),
    }

    // at_yield = false — Bool(false).
    let at_yield_default = require_default(template, "at_yield");
    match &at_yield_default.kind {
        CompiledExprKind::Literal(Value::Bool(v)) => assert!(
            !*v,
            "at_yield default should be false (PRB ctors flip this true on \
             yield-exceedance only); got: {}",
            v
        ),
        other => panic!(
            "at_yield default should be Literal(Value::Bool(false)); got: {:?}",
            other
        ),
    }
}

// ─── step-9: FlexureCompliance yield_margin upper-bound constraint ───────────

/// `FlexureCompliance` declares the structure-level constraint
/// `yield_margin <= 1` — the defense-in-depth encoding of the
/// dimensionless-ratio bound on `yield_margin = (yield - max_stress) /
/// yield`. Mathematically:
///
///   - At `max_stress = 0` the margin reaches its maximum of 1.
///   - At `max_stress = yield` the margin is 0 (boundary of `at_yield`).
///   - At `max_stress > yield` the margin goes negative — the "at_yield"
///     regime where PRB ctors emit `W_FlexureYielding` (PRD §5.3) and set
///     `at_yield = true`.
///
/// So `yield_margin > 1` is non-physical and indicates a PRB-ctor bug
/// (e.g. swapped numerator/denominator). Encoding this as a first-class
/// `constraint` declaration matches the project convention from task 2544
/// ("the contract in production code is made explicit rather than relying
/// solely on test coverage") and mirrors the same shape that
/// `BucklingOptions.n_modes > 0` (solver_buckling.ri:87) and
/// `PiecewisePolynomialProfile.waypoints.count > 0` (trajectory.ri:230)
/// already use as upper-bound / lower-bound structure-level invariants.
///
/// SIR-α's `check_constraints_against_templates` machinery (task 3540,
/// landed) evaluates structure-level constraints at the eval path, so this
/// fires at construction with no further plumbing in β.
///
/// Test pins (a) exactly one constraint (tight count, mirroring
/// trajectory's `piecewise_polynomial_profile_constrains_waypoints_nonempty`
/// discipline at trajectory_stdlib_compile.rs:705), (b) the constraint is a
/// `BinOp::Le` shape, (c) the LHS resolves to `ValueRef(yield_margin)`,
/// (d) the RHS is `Literal::Int(1)` or `Literal::Real(1.0)` (mirrors the
/// future-proofing rationale at trajectory_stdlib_compile.rs:752-760
/// covering both Int and Real literal forms for the `0` / `1` RHS).
#[test]
fn flexure_compliance_constrains_yield_margin_upper_bound() {
    let template = find_structure("FlexureCompliance");

    assert_eq!(
        template.constraints.len(),
        1,
        "FlexureCompliance should declare exactly 1 constraint \
         (yield_margin <= 1); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let constraint = &template.constraints[0];

    // Match BinOp::Le at the top level.
    let (left, right, op) = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => (left.as_ref(), right.as_ref(), op),
        other => panic!(
            "FlexureCompliance constraint should be a BinOp; got: {:?}",
            other
        ),
    };
    assert_eq!(
        *op,
        BinOp::Le,
        "FlexureCompliance constraint should use BinOp::Le (yield_margin <= 1); \
         got: {:?}",
        op
    );

    // LHS must be a ValueRef whose member is `yield_margin`.
    match &left.kind {
        CompiledExprKind::ValueRef(cell_id) => assert_eq!(
            cell_id.member, "yield_margin",
            "FlexureCompliance constraint LHS should reference yield_margin; \
             got member: {}",
            cell_id.member
        ),
        other => panic!(
            "FlexureCompliance constraint LHS should be ValueRef(yield_margin); \
             got: {:?}",
            other
        ),
    }

    // RHS must be the literal `1`. Accept either `Int(1)` or `Real(1.0)`,
    // mirroring the future-proofing rationale established in
    // `trajectory_stdlib_compile.rs:752-760` /
    // `buckling_stdlib_compile.rs:356-358`: `yield_margin : Real = 0.0` so
    // the `1` literal could lex as Int (parser-default) or Real (coerced
    // by typing context). Accepting both keeps this test robust against a
    // future literal-coercion change.
    match &right.kind {
        CompiledExprKind::Literal(Value::Int(1)) => {}
        CompiledExprKind::Literal(Value::Real(v)) if *v == 1.0 => {}
        other => panic!(
            "FlexureCompliance constraint RHS should be \
             Literal(Value::Int(1)) or Literal(Value::Real(1.0)); got: {:?}",
            other
        ),
    }
}

// ─── step-11: flexure_compliance accessor fn signature + eval ────────────────

/// `flexure_compliance(joint) -> FlexureCompliance` is the PRD §4.2 accessor
/// surfacing the cached `FlexureCompliance` record from a joint. The λ task
/// (3871) replaced the β stub body (`FlexureCompliance()`) with a delegation
/// to the `__flexure_compliance_get` Rust intrinsic, which reads the reserved
/// hidden field `__flexure_compliance` set by PRB ctors (γ / δ / ε / ζ / η /
/// θ) on the joint Map and returns it, or — for a non-joint arg — a Rust-built
/// sentinel default record. The joint parameter is now typed `Length` (λ
/// retargeted the β `Real` placeholder so the accessor overload-matches a
/// PRB-ctor joint, whose native-builtin return type is inferred as its first
/// LENGTH arg) until KCC-ζ (task 3845) lands the per-kind joint structures so
/// we can retarget to `DrivingJoint` / `Joint`.
///
/// Test pins two contracts:
///
///   (a) Signature shape — the function is `pub`, takes one `joint : Length`
///       parameter (λ retarget; see above), and returns
///       `Type::StructureRef("FlexureCompliance")`.
///       The structure-ref return type pins the type-resolution path that
///       `fn_signature_type_resolution_tests.rs::
///       fn_signature_resolves_stdlib_structure_as_return_type` already
///       exercises end-to-end on `ElasticResult` — here we pin the same
///       contract for FlexureCompliance via the std/flexures embedded path.
///
///   (b) Eval shape — calling `flexure_compliance(0.0)` via the production
///       `reify_expr::eval_expr` + `EvalContext::new(&values, &module.
///       functions)` route (rather than reading `func.body.result_expr`
///       directly) yields a `Value::StructureInstance(data)` whose
///       `type_name == "FlexureCompliance"` and whose 7 fields carry the
///       λ default-record values: sentinel-zero for stiffness / stresses /
///       validity-range, `none` parasitic, `at_yield = false`, and the
///       no-yield `yield_margin = 1.0` "maximally safe" sentinel (so a future
///       change that silently drops the body, mangles the intrinsic target, or
///       shuffles the default population is caught here). Routing via `eval_expr`
///       mirrors the discipline in `standard_stock_tests.rs::
///       assert_length_constant` and `standard_gravity_tests.rs::
///       standard_gravity_evaluates_to_9p80665_si_with_acceleration_
///       dimension`: future `let`-bindings inside the body would silently
///       evaporate under a `result_expr` short-circuit.
#[test]
fn flexure_compliance_accessor_fn_signature_and_eval() {
    let module = load_stdlib_module();

    // ── (a) Signature shape ────────────────────────────────────────────────
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "flexure_compliance")
        .unwrap_or_else(|| {
            panic!(
                "expected `pub fn flexure_compliance(...)` in std/flexures, \
                 got functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(
        func.is_pub,
        "flexure_compliance must be `pub` (PRD §4.2 accessor); got is_pub = {}",
        func.is_pub
    );

    assert_eq!(
        func.params.len(),
        1,
        "flexure_compliance should take exactly 1 param (joint); got: {:?}",
        func.params
    );

    assert_eq!(
        func.params[0].0, "joint",
        "flexure_compliance param 0 should be named `joint` (PRD §4.2); got: {}",
        func.params[0].0
    );

    // Joint param is the nominal `FlexureJoint` marker (task #5476,
    // placeholder-ratchet α; PRD docs/prds/v0_6/placeholder-type-eradication-ratchet.md
    // §3.2/§3.3).
    //
    // HISTORY of this assertion, because it has now been retargeted twice and
    // the previous two values were BOTH placeholders:
    //   β  (task 3861) — `Real`.   Pure stand-in; no joint type existed.
    //   λ  (task 3871) — `Length`. Chosen so the accessor would overload-match a
    //                    PRB-ctor result, whose type the compiler inferred from
    //                    its first (LENGTH) argument via the expr.rs first-arg
    //                    fallback. Side effect: a bare `5mm` matched too, so the
    //                    misuse degraded to a sentinel-zero record + a warning.
    //   α  (task 5476) — `StructureRef("FlexureJoint")`. NOT a stand-in: the
    //                    13 `prb_*` ctors now type their call sites as this
    //                    nominal marker (flexure_signatures.rs), so the exact-
    //                    equality overload filter accepts a real joint and
    //                    rejects `Scalar[LENGTH]` with a hard compile Error
    //                    (DiagnosticCode::NoMatchingOverload). The runtime
    //                    representation stays `Value::Map` (PRD §7.1).
    //
    // Pinning this param type is what makes the ratchet non-reversible: loosening
    // it back to any structural type re-opens the bare-literal hole INV-SF-5 bans.
    assert_eq!(
        func.params[0].1,
        Type::StructureRef("FlexureJoint".to_string()),
        "flexure_compliance.joint param type should be the nominal \
         StructureRef(\"FlexureJoint\") marker (task #5476 placeholder-ratchet α). \
         A `Scalar[LENGTH]` here is the RETIRED λ-era placeholder — it lets a bare \
         `5mm` overload-match and silently yield a sentinel-zero compliance \
         record instead of a type error; got: {:?}",
        func.params[0].1
    );

    assert_eq!(
        func.return_type,
        Type::StructureRef("FlexureCompliance".to_string()),
        "flexure_compliance return type should be \
         Type::StructureRef(\"FlexureCompliance\") (PRD §4.2); got: {:?}",
        func.return_type
    );

    // ── (b) Eval shape ─────────────────────────────────────────────────────
    // Build a `flexure_compliance(0m)` call expression and route it through
    // the production eval path (rather than reading `func.body.result_expr`
    // directly) — robust against future `let`-binding refactors per the
    // standard_stock_tests / standard_gravity_tests precedent.
    //
    // The probe arg must carry a `StructureRef("FlexureJoint")` `result_type`:
    // `eval_user_function_call` → `find_matching_compiled_function` selects the
    // overload by exact arg-`result_type` ↔ param-type equality, and task #5476
    // retyped the `joint` param to the nominal `FlexureJoint` marker (see (a)
    // above). The earlier probe types — `Type::dimensionless_scalar()` (β) and
    // `Scalar[LENGTH]` (λ) — would now miss the overload and eval to `Undef`.
    //
    // The VALUE stays a deliberate non-joint `0m`. That mismatch between the
    // static tag and the runtime value is not sloppiness — it is precisely the
    // PRD §3.6 fabrication case (`FlexureJoint()` is statically legal, so a
    // marker-typed expression may carry no `__flexure_compliance` cache entry),
    // and it is what makes this an eval-SHAPE probe: `__flexure_compliance_get`
    // sees a non-Map value and returns the sentinel default record, whose
    // seven-field shape is what the assertions below pin. The eval-time
    // `W_FLEXURE_NON_JOINT_ARG` arm that fires on exactly this case is covered
    // separately (task 4547; reify-stdlib/src/flexures/diagnostics.rs).
    let joint_arg = CompiledExpr::literal(
        Value::length(0.0),
        Type::StructureRef("FlexureJoint".to_string()),
    );
    let call_expr = CompiledExpr::user_function_call(
        "flexure_compliance".to_string(),
        vec![joint_arg],
        Type::StructureRef("FlexureCompliance".to_string()),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    let data = match &result {
        Value::StructureInstance(data) => data,
        other => panic!(
            "flexure_compliance(0.0) should return Value::StructureInstance; \
             got: {:?}",
            other
        ),
    };

    assert_eq!(
        data.type_name, "FlexureCompliance",
        "flexure_compliance(0.0) StructureInstance.type_name should be \
         \"FlexureCompliance\"; got: {}",
        data.type_name
    );

    // 7 fields per PRD §4.2; sentinel-zero defaults per step-7. Count only
    // user-facing fields, excluding `@@`-prefixed overlay keys (e.g. task
    // 4089's `@@source_span`, attached to every ctor and excluded from
    // identity/Display — see reify-ir value.rs).
    let user_field_count = data
        .fields
        .iter()
        .filter(|(k, _)| !k.starts_with("@@"))
        .count();
    assert_eq!(
        user_field_count,
        7,
        "flexure_compliance(0.0) StructureInstance should have exactly 7 \
         user fields (PRD §4.2); got fields: {:?}",
        data.fields
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
    );

    // effective_stiffness = 0 — the runtime sentinel record
    // (make_compliance_record) stays family-agnostic bare Value::Real per task
    // 4587, independent of the .ri declared dimensioned type (the
    // `RotationalStiffness = Real` alias is now deleted, task 4547 D2; this
    // declared-type/runtime-value divergence is intentional tracked debt).
    // Accept Int(0) or Real(0.0) per the literal-coercion future-proofing
    // rationale established in step-7.
    let effective_stiffness = data
        .fields
        .get(&"effective_stiffness".to_string())
        .expect("flexure_compliance(0.0).effective_stiffness missing");
    match effective_stiffness {
        Value::Real(v) if *v == 0.0 => {}
        Value::Int(0) => {}
        other => panic!(
            "flexure_compliance(0.0).effective_stiffness should be Real(0.0) \
             or Int(0) (sentinel-zero default); got: {:?}",
            other
        ),
    }

    // max_stress = 0Pa.
    let max_stress = data
        .fields
        .get(&"max_stress".to_string())
        .expect("flexure_compliance(0.0).max_stress missing");
    match max_stress {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "flexure_compliance(0.0).max_stress should have PRESSURE \
                 dimension; got: {:?}",
                dimension
            );
            assert_eq!(
                *si_value, 0.0,
                "flexure_compliance(0.0).max_stress si_value should be \
                 exactly 0.0 (= 0Pa sentinel-zero default); got: {}",
                si_value
            );
        }
        other => panic!(
            "flexure_compliance(0.0).max_stress should be Value::Scalar \
             {{ PRESSURE, 0.0 }} (= 0Pa); got: {:?}",
            other
        ),
    }

    // max_stress_at_neutral = 0Pa.
    let max_stress_at_neutral = data
        .fields
        .get(&"max_stress_at_neutral".to_string())
        .expect("flexure_compliance(0.0).max_stress_at_neutral missing");
    match max_stress_at_neutral {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "flexure_compliance(0.0).max_stress_at_neutral should have \
                 PRESSURE dimension; got: {:?}",
                dimension
            );
            assert_eq!(
                *si_value, 0.0,
                "flexure_compliance(0.0).max_stress_at_neutral si_value \
                 should be exactly 0.0 (= 0Pa sentinel-zero default); got: {}",
                si_value
            );
        }
        other => panic!(
            "flexure_compliance(0.0).max_stress_at_neutral should be \
             Value::Scalar {{ PRESSURE, 0.0 }} (= 0Pa); got: {:?}",
            other
        ),
    }

    // yield_margin = 1.0 — the no-yield "maximally safe" sentinel. The λ
    // accessor (3871) routes through the `__flexure_compliance_get` intrinsic;
    // a non-joint arg (`0.0` is not a joint Map carrying `__flexure_compliance`)
    // yields the Rust-built default record `make_compliance_record(.., None
    // yield, ..)`, whose `yield_margin` sentinel is 1.0 (not 0.0, which would
    // falsely read as "exactly at the yield boundary"). Pairs with at_yield =
    // false below.
    let yield_margin = data
        .fields
        .get(&"yield_margin".to_string())
        .expect("flexure_compliance(0.0).yield_margin missing");
    match yield_margin {
        Value::Real(v) if *v == 1.0 => {}
        other => panic!(
            "flexure_compliance(0.0).yield_margin should be Real(1.0) (no-yield \
             safe sentinel from the λ __flexure_compliance_get default record); \
             got: {:?}",
            other
        ),
    }

    // parasitic_error = none → Value::Option(None) per the Option default
    // path (option_compile_tests.rs precedent).
    let parasitic_error = data
        .fields
        .get(&"parasitic_error".to_string())
        .expect("flexure_compliance(0.0).parasitic_error missing");
    match parasitic_error {
        Value::Option(None) => {}
        other => panic!(
            "flexure_compliance(0.0).parasitic_error should be \
             Value::Option(None) (= `none`); got: {:?}",
            other
        ),
    }

    // prb_validity_range = 0deg..0deg (sentinel-zero Range<Angle>; task 4576).
    let prb_validity_range = data
        .fields
        .get(&"prb_validity_range".to_string())
        .expect("flexure_compliance(0.0).prb_validity_range missing");
    match prb_validity_range {
        Value::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower_inclusive, "prb_validity_range: lower_inclusive");
            assert!(upper_inclusive, "prb_validity_range: upper_inclusive");
            for (label, bound) in [("lower", lower), ("upper", upper)] {
                let b = bound.as_deref().unwrap_or_else(|| {
                    panic!("flexure_compliance(0.0).prb_validity_range {label} bound missing")
                });
                match b {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(
                            *dimension,
                            reify_core::DimensionVector::ANGLE,
                            "prb_validity_range {label} bound dimension"
                        );
                        assert_eq!(
                            *si_value, 0.0,
                            "prb_validity_range {label} bound si_value (0deg)"
                        );
                    }
                    other => panic!(
                        "flexure_compliance(0.0).prb_validity_range {label} bound \
                         should be ANGLE Scalar(0.0); got: {other:?}"
                    ),
                }
            }
        }
        other => panic!(
            "flexure_compliance(0.0).prb_validity_range should be \
             Value::Range{{0deg..0deg}} (sentinel-zero Range<Angle>; task 4576); \
             got: {:?}",
            other
        ),
    }

    // at_yield = false.
    let at_yield = data
        .fields
        .get(&"at_yield".to_string())
        .expect("flexure_compliance(0.0).at_yield missing");
    match at_yield {
        Value::Bool(v) => assert!(
            !*v,
            "flexure_compliance(0.0).at_yield should be false (PRB ctors \
             flip this true on yield-exceedance only); got: {}",
            v
        ),
        other => panic!(
            "flexure_compliance(0.0).at_yield should be Value::Bool(false); \
             got: {:?}",
            other
        ),
    }
}

// ─── FlexureJoint marker (task 5476, placeholder-ratchet α) ──────────────────

/// `structure def FlexureJoint : DrivingJoint { }` — the α static-layer marker
/// that gives `flexure_compliance` its nominal parameter type, in place of the
/// pre-α `Length` placeholder.
///
/// Three properties, each load-bearing:
///
/// 1. **It exists in `std/flexures`.** The marker has to be declared in the same
///    module as the accessor whose signature names it.
/// 2. **It is empty — no value cells at all.** PRD §7.1: the typed-joints
///    migration stays rejected, so the RUNTIME joint representation is still an
///    untouched `Value::Map`. The marker is a static-layer tag only; giving it
///    fields would start exactly the migration the PRD declined. Asserted on
///    `value_cells` (not just `param_cells`) so a stray `let` is caught too.
/// 3. **It conforms to `DrivingJoint`.** This keeps the #4310 compile-time bound
///    checks on `bind`/`sweep`/`dim` uniform — to the mechanism layer a flexure
///    joint "is just a Revolute". Mirrors the accessor used by
///    `kinematic_stdlib_compile.rs::conforming_joints_have_driving_joint_bound`
///    (`template.trait_bounds`), rather than inventing a new one.
///
/// COUPLED TO PRELUDE ORDER. The `: DrivingJoint` clause only resolves because
/// `stdlib_loader.rs` registers `std.flexures` AFTER `std.kinematic`. The stdlib
/// prelude is built by growing sequential registration (no `.ri` file declares
/// an `import`), so registration order IS visibility order: move `std.flexures`
/// back before `std.kinematic` and `DrivingJoint` is not yet in scope when
/// flexures.ri compiles. That failure does not surface here — it surfaces as a
/// resolution Error through the `std_flexures_module_loads_with_no_errors`
/// canary above, which is deliberately left untouched so it can catch it. If
/// this test fails on `trait_bounds` while that canary is red too, fix the
/// module order, not the declaration.
#[test]
fn std_flexures_declares_flexure_joint_marker() {
    let template = find_structure("FlexureJoint");

    assert!(
        template.value_cells.is_empty(),
        "FlexureJoint must be an EMPTY marker — no params, no lets (PRD §7.1: \
         joints stay Value::Map at runtime; the marker is static-layer only). \
         Got value cells: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| (&vc.id.member, &vc.kind))
            .collect::<Vec<_>>(),
    );

    assert!(
        template.trait_bounds.contains(&"DrivingJoint".to_owned()),
        "FlexureJoint must declare conformance to DrivingJoint so the #4310 \
         compile-time bind/sweep/dim bound checks treat it like any other \
         driving joint; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── prb_* ctor call-site typing (task 5476, placeholder-ratchet α) ──────────

/// Compile `source` against the production stdlib and return the `cell_type` of
/// the named `let` cell inside the named structure.
///
/// Uses `compile_source_with_stdlib` (reify-test-support, already a dev-dep of
/// this crate) so the call site is typed by the exact production ladder.
fn user_let_cell_type(source: &str, structure: &str, member: &str) -> Type {
    let module = compile_source_with_stdlib(source);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == structure && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {structure}` in the compiled user module; \
                 got: {:?}",
                module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| {
            panic!(
                "expected `let {member}` in `structure def {structure}`; got cells: {:?}",
                template
                    .value_cells
                    .iter()
                    .map(|vc| &vc.id.member)
                    .collect::<Vec<_>>()
            )
        })
        .cell_type
        .clone()
}

/// A `prb_*` ctor call site must type as the nominal `StructureRef("FlexureJoint")`,
/// NOT as whatever its first argument happens to be.
///
/// This is held up by the `is_flexure_typed_fn` arm in `expr.rs`'s
/// builtin-typing ladder. Remove or mis-order that arm and the failure mechanism
/// is precisely the one PRD §2 names as the root cause of the placeholder: with
/// no signature arm, the call falls through the `NoUserFunctions` ladder to the
/// FIRST-ARG FALLBACK and inherits its first argument's type — `Scalar[LENGTH]`,
/// from `notch_radius = 1mm` / `length = 20mm`. That is exactly why a bare `5mm`
/// used to overload-match `flexure_compliance` and yield a sentinel record
/// instead of a type error. A failure here therefore means the arm is gone, is
/// positioned after the fallback, or no longer covers the probed ctor.
///
/// Two ctors from two DIFFERENT family modules are covered
/// (`prb_notch_circular` — notch.rs; `prb_parallelogram_flexure` — compound.rs)
/// so the arm is shown to fire family-wide rather than for one hard-coded name.
///
/// WHY THE `!= Scalar[LENGTH]` ASSERTION IS THE DISCRIMINATING ONE. The ideal
/// probe would be a ctor whose first arg is NOT a Length, so that name-dispatch
/// and first-arg fallback could not coincidentally agree. No such ctor exists:
/// every one of the 13 takes a Length first (`length` or `notch_radius`;
/// verified against the signature doc comments in beam.rs / notch.rs /
/// hinge.rs:168,182,275 / prismatic.rs:146,220 / compound.rs). The explicit
/// `!= Scalar[LENGTH]` assertion below is therefore the signal that the arm —
/// not the fallback — produced the type. At the unit level the discrimination
/// is structural rather than asserted: `flexure_signatures::flexure_joint_type()`
/// takes no arguments at all, so it cannot consult a first-arg type.
#[test]
fn prb_ctor_call_cell_type_is_flexure_joint() {
    let source = r#"
module prb_cell_type_probe

structure def PrbCellType {
    let steel = Steel_AISI_1045()
    let j_notch = prb_notch_circular(1mm, 0.2mm, 5mm, steel, point3(0mm, 0mm, 0mm), vec3(0, 1, 0))
    let j_compound = prb_parallelogram_flexure(20mm, 5mm, 0.5mm, 10mm, steel, point3(0mm, 0mm, 0mm), vec3(0, 1, 0))
}
"#;

    let expected = Type::StructureRef("FlexureJoint".to_string());
    // The first-arg-fallback type each call would inherit without the arm.
    let fallback = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    for member in ["j_notch", "j_compound"] {
        let actual = user_let_cell_type(source, "PrbCellType", member);

        assert_ne!(
            actual, fallback,
            "`{member}` must NOT be typed {fallback:?} — that is the first-arg \
             fallback (expr.rs) inheriting the ctor's leading Length argument, \
             which is the placeholder PRD §2 eradicates. Wire the \
             `is_flexure_typed_fn` arm into the builtin-typing ladder BEFORE that \
             fallback."
        );
        assert_eq!(
            actual, expected,
            "`{member}` (a prb_* ctor call) must type as \
             StructureRef(\"FlexureJoint\"); got {actual:?}"
        );
    }
}

// ─── BT1, library half: the typed diagnostic code (task 5476) ────────────────

/// `flexure_compliance(5mm)` must produce a `Severity::Error` diagnostic
/// carrying `DiagnosticCode::NoMatchingOverload` (PRD-prose mnemonic
/// `E_NO_MATCHING_OVERLOAD`).
///
/// This is the library half of PRD §7.3 BT1; the user-observable half (the
/// exit-code flip) is
/// `reify-cli/tests/harness_cli/cli_check.rs::check_flexure_compliance_non_joint_exits_failure`.
///
/// The split is forced, not stylistic: `DiagnosticCode` has no `Display`,
/// `code_str()` or `as_str()` anywhere in the workspace — the `E_*`/`W_*`
/// mnemonics live only in doc-comment prose — and this task contractually
/// freezes the no-matching-overload message text, so the mnemonic is never
/// rendered and a CLI substring assertion cannot reach it.  The typed code is
/// therefore asserted here with the house `d.code == Some(...)` idiom (cf.
/// `expr.rs`'s `no_matching_overload_error_carries_no_matching_overload_code`),
/// and the CLI test carries the §G2 exit flip.
///
/// FAILURE READING. Zero Error diagnostics (rather than a wrongly-coded one)
/// means `flexure_compliance` has regressed to a parameter type that `5mm`
/// overload-MATCHES — the pre-α `joint: Length` placeholder being the canonical
/// case: it produces no `NoMatch` at all and the accessor silently returns a
/// sentinel-zero compliance record. One Error with `code: None` instead means
/// the `NoMatchingOverload` tagging in `expr.rs` was dropped.
#[test]
fn flexure_compliance_on_bare_length_emits_no_matching_overload_code() {
    let source = r#"
module flexure_compliance_bare_length_probe

structure def FlexureComplianceBareLength {
    let c = flexure_compliance(5mm)
}
"#;

    let module = compile_source_with_stdlib(source);
    let errors: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "flexure_compliance(5mm) must be a hard Error — a bare Length is not a \
         flexure joint (PRD §7.3 BT1, task #5476). Zero Error diagnostics means \
         the `joint: Length` placeholder signature still matches `5mm` in \
         crates/reify-compiler/stdlib/flexures.ri, so the accessor silently \
         returns a sentinel-zero compliance record. All diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message, &d.code))
            .collect::<Vec<_>>()
    );

    let coded: Vec<&&Diagnostic> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::NoMatchingOverload))
        .collect();

    assert_eq!(
        coded.len(),
        1,
        "expected exactly one Error carrying DiagnosticCode::NoMatchingOverload \
         (INV-SF-6); got errors: {:?}",
        errors
            .iter()
            .map(|d| (&d.message, &d.code))
            .collect::<Vec<_>>()
    );
    assert!(
        coded[0]
            .message
            .contains("no matching overload for flexure_compliance"),
        "the coded Error must be the flexure_compliance call-site NoMatch, and its \
         message text is contractually unchanged by this task (the mnemonic is NOT \
         baked into the message); got: {}",
        coded[0].message
    );
}

/// A SIBLING `DrivingJoint` conformer is rejected too: `flexure_compliance` is
/// matched by NOMINAL EXACT EQUALITY on `FlexureJoint`, not by the
/// `DrivingJoint` trait bound.
///
/// This pins the boundary that actually distinguishes the α design from a
/// trait-based one, and it is counter-intuitive precisely because the marker is
/// declared `structure def FlexureJoint : DrivingJoint { }` — a reader seeing
/// that bound may reasonably assume any `DrivingJoint` matches. It does not:
/// `type_compat.rs::resolve_function_overload` compares parameter types by exact
/// equality, so `Spherical()` (`structure def Spherical : DrivingJoint { }` in
/// kinematic.ri) is as rejected as a bare `5mm`.
///
/// The bound on the marker does real work, just not HERE — it is what keeps the
/// #4310 compile-time `bind`/`sweep`/`dim` bound checks accepting a flexure
/// joint (pinned by `std_flexures_declares_flexure_joint_marker` above and by
/// the BT3 corpus walk in reify-eval-fea-tests). Two independent mechanisms,
/// two separate pins.
///
/// If a future change widens the overload filter to accept trait-bound-compatible
/// arguments, this test fails — which is the point. Reopening
/// `flexure_compliance(<any DrivingJoint>)` would be a variant of the very hole
/// task #5476 closed, and it must be a deliberate, visible decision rather than
/// a silent side effect.
#[test]
fn flexure_compliance_on_sibling_driving_joint_emits_no_matching_overload_code() {
    let source = r#"
module flexure_compliance_sibling_joint_probe

structure def FlexureComplianceSiblingJoint {
    let c = flexure_compliance(Spherical())
}
"#;

    let module = compile_source_with_stdlib(source);
    let coded: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.code == Some(DiagnosticCode::NoMatchingOverload))
        .filter(|d| {
            d.message
                .contains("no matching overload for flexure_compliance")
        })
        .collect();

    assert_eq!(
        coded.len(),
        1,
        "flexure_compliance(Spherical()) must be a hard NoMatchingOverload Error: \
         accessor matching is nominal exact equality on FlexureJoint, NOT \
         conformance to the DrivingJoint bound that both structures carry. Zero \
         such Errors means the overload filter started accepting trait-bound-\
         compatible arguments — a deliberate widening, or an accidental reopening \
         of the placeholder hole (PRD §3.2, task #5476). All diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message, &d.code))
            .collect::<Vec<_>>()
    );
}
