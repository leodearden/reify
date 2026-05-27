//! Tests for `crates/reify-compiler/stdlib/flexures.ri` —
//! `std.flexures` module: `FlexureCompliance` structure_def and the
//! `flexure_compliance(joint)` accessor stdlib fn — the value-type substrate
//! for the v0.3 compliant-joints-flexures PRD.
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
//! production. This mirrors the helper trio in `trajectory_stdlib_compile.rs`.

use reify_compiler::*;
use reify_types::*;

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

/// Return the `std/flexures.types` CompiledModule — value types
/// (`RotationalStiffness` alias + `FlexureCompliance` structure_def) live
/// in a separate stdlib module, loaded immediately before `std.flexures`.
/// See SPLIT RATIONALE in `flexures_types.ri` (esc-3851-32).
fn load_stdlib_types_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/flexures/types")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/flexures.types module; \
                 available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/flexures.types`
/// module (the value-types module — see `load_stdlib_types_module`).
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_types_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/flexures.types, \
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
#[allow(dead_code)]
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

/// Recursively walk an expression tree collecting `(method_name, member_name)`
/// pairs from `MethodCall { object: ValueRef(member), method: name, .. }`
/// nodes. The traversal also recurses into `BinOp`, `UnOp`, and nested
/// `MethodCall` receivers so a deeply-nested chain surfaces the pair.
#[allow(dead_code)]
fn collect_method_call_chain(expr: &CompiledExpr) -> Vec<(&str, &str)> {
    let mut pairs = Vec::new();
    match &expr.kind {
        CompiledExprKind::MethodCall { object, method, .. } => {
            if let CompiledExprKind::ValueRef(cell_id) = &object.kind {
                pairs.push((method.as_str(), cell_id.member.as_str()));
            }
            pairs.extend(collect_method_call_chain(object));
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            pairs.extend(collect_method_call_chain(left));
            pairs.extend(collect_method_call_chain(right));
        }
        CompiledExprKind::UnOp { operand, .. } => {
            pairs.extend(collect_method_call_chain(operand));
        }
        _ => {}
    }
    pairs
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

/// `RotationalStiffness` is the canonical PRD §4.2 type for
/// `FlexureCompliance.effective_stiffness`. The proper dimensioned type
/// (N·m/rad) is owned by the un-filed compliant-joints-flexures α task
/// (Joint surface extension); β ships a `pub type RotationalStiffness =
/// Real` placeholder so call sites can already spell the canonical name and
/// the future α task retargets a single alias line — same placeholder
/// posture as `trajectory.ri:56 pub type JointValue = Real`.
///
/// Test pins three invariants: (a) the alias is present in
/// `module.type_aliases`, (b) `is_pub == true` so downstream modules /
/// user code can reference the canonical spelling, (c) the alias resolves
/// transitively to `Type::Real`. Assertion shape mirrors
/// `type_alias_compile_tests.rs:33-52` and `:481-498`.
#[test]
fn rotational_stiffness_alias_resolves_to_real() {
    // RotationalStiffness now lives in `std.flexures.types` after the
    // split filed under esc-3851-32 — see SPLIT RATIONALE in
    // `flexures_types.ri`.
    let module = load_stdlib_types_module();

    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "RotationalStiffness")
        .unwrap_or_else(|| {
            panic!(
                "expected `pub type RotationalStiffness` in std/flexures.types, \
                 got type_aliases: {:?}",
                module
                    .type_aliases
                    .iter()
                    .map(|a| &a.name)
                    .collect::<Vec<_>>()
            )
        });

    assert!(
        alias.is_pub,
        "RotationalStiffness must be `pub` so downstream modules / user code \
         can reference the canonical spelling; got is_pub = {}",
        alias.is_pub
    );

    assert_eq!(
        alias.resolved_type,
        Some(Type::Real),
        "RotationalStiffness placeholder alias must resolve to Type::Real; \
         got: {:?}",
        alias.resolved_type
    );
}

// ─── step-5: FlexureCompliance param shape ───────────────────────────────────

/// `FlexureCompliance` is the value-type container for the PRD §4.2 seven-
/// field flexure compliance contract. Per PRD §4.2:
///
///   - `effective_stiffness   : RotationalStiffness`    (= Real placeholder
///                                                       via the alias; PRD
///                                                       §4.2 spelling)
///   - `max_stress            : Pressure`               (at range endpoint)
///   - `max_stress_at_neutral : Pressure`               (zero unless preloaded)
///   - `yield_margin          : Real`                   ((yield-max_stress)/yield)
///   - `parasitic_error       : Option<Length>`         (compound-flexure
///                                                       orthogonal-DOF motion)
///   - `prb_validity_range    : Real`                   (Real placeholder for
///                                                       Range<Angle>; see
///                                                       module header §2)
///   - `at_yield              : Bool`                   (true if
///                                                       max_stress >= yield)
///
/// `RotationalStiffness` resolves transitively to `Type::Real` via the
/// step-4 `pub type` alias, so `effective_stiffness.cell_type ==
/// Type::Real`. `Pressure` resolves to `Type::Scalar { dimension:
/// DimensionVector::PRESSURE }` via the standard dimensioned-type path.
/// `Range<Angle>` is not a resolvable parameterized builtin
/// (`type_resolution.rs::resolve_parameterized_builtin_type` has no Range
/// arm), so `prb_validity_range` ships as `Real` per the module header §2
/// placeholder convention.
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
        ("effective_stiffness", Type::Real),
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
        ("yield_margin", Type::Real),
        (
            "parasitic_error",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        ),
        ("prb_validity_range", Type::Real),
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
///   effective_stiffness   = 0.0    (Real via RotationalStiffness alias)
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

    // effective_stiffness = 0(.0) — accept Int(0) or Real(0.0).
    let effective_stiffness_default = require_default(template, "effective_stiffness");
    match &effective_stiffness_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {}
        CompiledExprKind::Literal(Value::Int(0)) => {}
        other => panic!(
            "effective_stiffness default should be Literal(Value::Real(0.0)) or \
             Literal(Value::Int(0)); got: {:?}",
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

    // prb_validity_range = 0(.0) — accept Int(0) or Real(0.0).
    let prb_validity_range_default = require_default(template, "prb_validity_range");
    match &prb_validity_range_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {}
        CompiledExprKind::Literal(Value::Int(0)) => {}
        other => panic!(
            "prb_validity_range default should be Literal(Value::Real(0.0)) or \
             Literal(Value::Int(0)); got: {:?}",
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
/// surfacing the cached `FlexureCompliance` record from a joint. The β-task
/// stub body returns a default `FlexureCompliance()` until λ (task 3821)
/// replaces it with the cache-lookup logic that reads the reserved hidden
/// field `__flexure_compliance` set by PRB ctors (γ / δ / ε / ζ / η / θ) on
/// the joint Map. The joint parameter is typed as `Real` (placeholder per
/// the module header §3 / `TODO(joint-type)`) until KCC-ζ (task 3845) lands
/// the per-kind joint structures so we can retarget to `DrivingJoint` /
/// `Joint`.
///
/// Test pins two contracts:
///
///   (a) Signature shape — the function is `pub`, takes one `joint : Real`
///       parameter, and returns `Type::StructureRef("FlexureCompliance")`.
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
///       sentinel-zero defaults pinned in step-7 (so a future change that
///       silently drops the body, mangles the ctor target, or shuffles the
///       default population is caught here). Routing via `eval_expr`
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
                module
                    .functions
                    .iter()
                    .map(|f| &f.name)
                    .collect::<Vec<_>>()
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

    // Joint param is `Real` per the module header §3 placeholder convention
    // (`TODO(joint-type)` — retargets to `DrivingJoint` trait or `Joint`
    // marker when KCC-ζ lands; task 3845). Same placeholder posture as
    // `trajectory.ri::PiecewisePolynomialProfile.mechanism : Real`.
    assert_eq!(
        func.params[0].1,
        Type::Real,
        "flexure_compliance.joint param type should be Type::Real (PRD-β \
         placeholder per module header §3; TODO(joint-type) retargets to \
         DrivingJoint when KCC-ζ lands); got: {:?}",
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
    // Build a `flexure_compliance(0.0)` call expression and route it through
    // the production eval path (rather than reading `func.body.result_expr`
    // directly) — robust against future `let`-binding refactors per the
    // standard_stock_tests / standard_gravity_tests precedent.
    let joint_arg = CompiledExpr::literal(Value::Real(0.0), Type::Real);
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

    // 7 fields per PRD §4.2; sentinel-zero defaults per step-7.
    assert_eq!(
        data.fields.len(),
        7,
        "flexure_compliance(0.0) StructureInstance.fields should have \
         exactly 7 entries (PRD §4.2); got fields: {:?}",
        data.fields
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
    );

    // effective_stiffness = 0 (Real via the RotationalStiffness alias).
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

    // yield_margin = 0 (Real). Accept Int(0) or Real(0.0).
    let yield_margin = data
        .fields
        .get(&"yield_margin".to_string())
        .expect("flexure_compliance(0.0).yield_margin missing");
    match yield_margin {
        Value::Real(v) if *v == 0.0 => {}
        Value::Int(0) => {}
        other => panic!(
            "flexure_compliance(0.0).yield_margin should be Real(0.0) or \
             Int(0) (sentinel-zero default); got: {:?}",
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

    // prb_validity_range = 0 (Real placeholder for Range<Angle>). Accept
    // Int(0) or Real(0.0).
    let prb_validity_range = data
        .fields
        .get(&"prb_validity_range".to_string())
        .expect("flexure_compliance(0.0).prb_validity_range missing");
    match prb_validity_range {
        Value::Real(v) if *v == 0.0 => {}
        Value::Int(0) => {}
        other => panic!(
            "flexure_compliance(0.0).prb_validity_range should be Real(0.0) \
             or Int(0) (sentinel-zero default; Range<Angle> placeholder); \
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
