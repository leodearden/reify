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

/// Look up a structure template by name within the `std/flexures` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/flexures, got templates: {:?}",
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
    let module = load_stdlib_module();

    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "RotationalStiffness")
        .unwrap_or_else(|| {
            panic!(
                "expected `pub type RotationalStiffness` in std/flexures, got \
                 type_aliases: {:?}",
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
