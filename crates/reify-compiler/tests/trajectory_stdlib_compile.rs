//! Tests for `crates/reify-compiler/stdlib/trajectory.ri` —
//! `std.trajectory` module: `Profile`, `BoundaryCondition`, `SplineKind`,
//! `Waypoint`, `NaturalSpline`, `ClampedSpline`, `PeriodicSpline`, and
//! `PiecewisePolynomialProfile` — the value-type substrate for the v0.3
//! trajectory-input-shaping PRD.
//!
//! Observable signal for PRD §11 Phase 1 label α
//! (docs/prds/v0_3/trajectory-input-shaping.md). Per the PRD, this file
//! parses the trait, enum, and structure_defs and confirms the compiled
//! shape matches the PRD §4.1 spec.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `buckling_stdlib_compile.rs` / `solver_elastic_tests.rs`), that
//! the declared traits, enum, and structures are correctly represented in
//! the compiled module, and that the `waypoints.count > 0` constraint on
//! `PiecewisePolynomialProfile` is declared at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `buckling_stdlib_compile.rs`.

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;
use reify_test_support::collect_value_ref_members;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/trajectory` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/trajectory")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/trajectory module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/trajectory` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/trajectory, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a trait definition by name within the `std/trajectory` module.
/// Mirrors `find_structure` but on `module.trait_defs`.
fn find_trait(name: &str) -> &'static CompiledTrait {
    let module = load_stdlib_module();
    module
        .trait_defs
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `trait {}` in std/trajectory, got trait_defs: {:?}",
                name,
                module
                    .trait_defs
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up an enum definition by name within the `std/trajectory` module.
/// Mirrors `find_structure` but on `module.enum_defs`.
fn find_enum(name: &str) -> &'static EnumDef {
    let module = load_stdlib_module();
    module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `enum {}` in std/trajectory, got enum_defs: {:?}",
                name,
                module
                    .enum_defs
                    .iter()
                    .map(|e| &e.name)
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

/// Assert the signature of a two-param evaluator helper fn
/// (`evaluate_profile`, `evaluate_profile_dot`, or `evaluate_profile_ddot`).
///
/// All three share the exact same shape:
///   `pub fn <name>(p: Profile, t: Time) -> List<JointValue>`
///
/// This helper centralises the lookup + pub + 2-param + return-type checks so
/// that a future signature change (e.g. tightening `JointValue`) is a
/// one-line edit here rather than a three-place edit prone to drift.
///
/// `profile_duration` is intentionally NOT covered by this helper — its shape
/// differs (1 param, `Time` return) and it remains a standalone test.
fn assert_evaluator_signature(name: &str) {
    let module = load_stdlib_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| {
            panic!(
                "{} not found in std/trajectory; found functions: {:?}",
                name,
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "{} should be pub", name);

    assert_eq!(
        func.params.len(),
        2,
        "{} should take exactly 2 params (p, t); got: {:?}",
        name,
        func.params
    );

    // Param order is part of the contract — p first, then t.
    assert_eq!(
        func.params[0],
        ("p".to_string(), Type::TraitObject("Profile".to_string())),
        "{} param[0] should be (\"p\", TraitObject(\"Profile\")); got: {:?}",
        name,
        func.params[0]
    );
    assert_eq!(
        func.params[1],
        (
            "t".to_string(),
            Type::Scalar {
                dimension: DimensionVector::TIME,
            }
        ),
        "{} param[1] should be (\"t\", Scalar<TIME>); got: {:?}",
        name,
        func.params[1]
    );

    assert_eq!(
        func.return_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
        "{} return type should be List<Real> (= List<JointValue>); got: {:?}",
        name,
        func.return_type
    );
}

/// Recursively walk an expression tree collecting `(method_name, member_name)`
/// pairs from `MethodCall { object: ValueRef(member), method: name, .. }`
/// nodes. The traversal also recurses into `BinOp`, `UnOp`, and nested
/// `MethodCall` receivers so a deeply-nested chain like
/// `waypoints.count > 0` surfaces `("count", "waypoints")`.
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

/// Returns `true` iff `expr` has the structural shape of a dimensioned-zero
/// BinOp chain: `0 * <anything>` (optionally followed by `/` or `*` factors).
///
/// **Shape-only:** the right operand is NOT inspected — `0 * 5` (dimensionless)
/// also returns `true`. Callers MUST pair this with an independent `result_type`
/// check to confirm the actual dimension; see each call site below.
///
/// Examples that match (shape only — `result_type` supplies the dimension):
///   `0 * 1N`            → BinOp(Mul, Int(0), Scalar{..})                  ✓
///   `0 * 1m / 1s`       → BinOp(Div, BinOp(Mul, Int(0), ..), ..)          ✓
///   `0 * 1m / (1s*1s)`  → BinOp(Div, BinOp(Mul, Int(0), ..), BinOp(..))   ✓
///
/// Used to distinguish the dimensioned-zero RHS of `velocity_limit > 0 * 1m/1s`
/// (required for Scalar<Velocity> — esc-3115 rule) from the plain
/// `Literal(Int(0))` RHS of dimensionless constraints like `vibration_tolerance > 0`.
fn is_dimensioned_zero_binop(expr: &reify_ir::CompiledExpr) -> bool {
    match &expr.kind {
        CompiledExprKind::BinOp { op: BinOp::Mul, left, .. } => {
            matches!(&left.kind, CompiledExprKind::Literal(Value::Int(0)))
                || matches!(&left.kind, CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0)
        }
        CompiledExprKind::BinOp { op: BinOp::Div, left, .. } => {
            is_dimensioned_zero_binop(left)
        }
        _ => false,
    }
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/trajectory module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_trajectory_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in trajectory.ri: {:?}",
        errors
    );
}

// ─── step-3: Profile marker trait ────────────────────────────────────────────

/// `Profile` is the marker trait for every motion-profile variant declared
/// in this module — currently only `PiecewisePolynomialProfile` (PRD §4.1),
/// with `TrapezoidalProfile`, `SCurveProfile`, etc. queued in later phases.
///
/// Empty in α by design: every member shared across profile variants would
/// force every variant to carry redundant data, defeating the marker-trait
/// purpose. Future shared members (e.g. `total_duration`) land in their
/// own phases when the design has settled on a single representation.
///
/// Test pins three invariants: (a) the trait is found, (b) it has zero
/// required members + zero defaults (marker trait), (c) it has no
/// refinements (top-level marker, no parent trait).
#[test]
fn profile_trait_exists_with_no_params() {
    let trait_def = find_trait("Profile");

    assert!(
        trait_def.required_members.is_empty(),
        "Profile should declare zero required members (marker trait); \
         got: {:?}",
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.defaults.is_empty(),
        "Profile should declare zero defaults (marker trait); got: {:?}",
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.refinements.is_empty(),
        "Profile should declare zero refinements (top-level marker, no \
         parent trait); got: {:?}",
        trait_def.refinements
    );
}

// ─── step-5: BoundaryCondition marker trait ──────────────────────────────────

/// `BoundaryCondition` is the marker trait for every spline boundary-mode
/// variant declared in this module: `NaturalSpline`, `ClampedSpline`,
/// `PeriodicSpline` (PRD §4.1). The semantic invariants on each variant
/// ("zero second derivative at endpoints", "specified tangents", "endpoints
/// agree") are evaluator-time concerns in β, not authoring-time params, so
/// the trait is intentionally empty.
///
/// Test pins three invariants: (a) the trait is found, (b) it has zero
/// required members + zero defaults, (c) it has no refinements (top-level
/// marker, no parent trait).
#[test]
fn boundary_condition_trait_exists_with_no_params() {
    let trait_def = find_trait("BoundaryCondition");

    assert!(
        trait_def.required_members.is_empty(),
        "BoundaryCondition should declare zero required members (marker \
         trait); got: {:?}",
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.defaults.is_empty(),
        "BoundaryCondition should declare zero defaults (marker trait); \
         got: {:?}",
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.refinements.is_empty(),
        "BoundaryCondition should declare zero refinements (top-level \
         marker, no parent trait); got: {:?}",
        trait_def.refinements
    );
}

// ─── step-7: SplineKind enum ─────────────────────────────────────────────────

/// `SplineKind` selects which polynomial degree the β evaluator uses when
/// building the per-segment coefficients for a `PiecewisePolynomialProfile`
/// (PRD §4.1):
///
///   - `CubicSpline`    — degree-3 polynomial per segment; the default
///     choice when waypoints carry only positions
///     (vels / accels are `none`).
///   - `QuinticSpline`  — degree-5 polynomial per segment; selected when
///     waypoints carry explicit `vels` AND `accels`.
///
/// Test pins the variant vector exactly (order-sensitive) — the assertion
/// shape mirrors `boundary2_producer.rs::compiled.enum_defs[0].variants`
/// (the canonical precedent for stdlib enum-variant assertions).
#[test]
fn spline_kind_enum_has_cubic_and_quintic_variants() {
    let enum_def = find_enum("SplineKind");

    assert_eq!(
        enum_def.variants,
        vec!["CubicSpline".to_string(), "QuinticSpline".to_string()],
        "SplineKind variants must match the PRD §4.1 spec exactly \
         (order-sensitive: CubicSpline, QuinticSpline); got: {:?}",
        enum_def.variants
    );
}

// ─── step-9: Waypoint param shape ────────────────────────────────────────────

/// `Waypoint` is the per-knot data the spline interpolates between
/// (PRD §4.1). It must declare exactly the four params with the canonical
/// types:
///
///   - `t      : Time`                          (knot time)
///   - `values : List<JointValue>`              (per-joint positions)
///   - `vels   : Option<List<JointValue>>`      (optional per-joint q̇)
///   - `accels : Option<List<JointValue>>`      (optional per-joint q̈)
///
/// `JointValue` is the module-level alias for `Real` (see header §1), so
/// `List<JointValue>` compiles to `Type::List(Box::new(Type::dimensionless_scalar()))`.
/// `Time` resolves to `Type::Scalar { dimension: DimensionVector::TIME }`
/// via the same dimensional-type path that `lead_time : Time` in
/// `stdlib/io.ri:77` already uses.
///
/// `Waypoint` is caller-supplied — there are no meaningful defaults on any
/// field (the spline path through the waypoints is entirely determined by
/// the caller's data). `vels` / `accels` are `Option`-typed so the caller
/// can omit per-knot derivative data when the chosen `SplineKind` does not
/// need it (cubic interpolation works with positions alone).
#[test]
fn waypoint_struct_has_correct_param_shape() {
    let template = find_structure("Waypoint");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "Waypoint should have exactly 4 param cells (t, values, vels, \
         accels), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "t",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
        ("values", Type::List(Box::new(Type::dimensionless_scalar()))),
        (
            "vels",
            Type::Option(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
        (
            "accels",
            Type::Option(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
    ];

    // Param declaration order is part of the contract — pin it explicitly
    // (mirrors the order-sensitive enum-variant assertion in
    // `spline_kind_enum_has_cubic_and_quintic_variants`).
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "Waypoint params must be declared in canonical order \
         (t, values, vels, accels); got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "Waypoint missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "Waypoint.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // Waypoint is caller-supplied — every param must have no default.
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "Waypoint.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // Waypoint declares no structure-level constraints (the only meaningful
    // cross-field invariant — `vels.len() == values.len()` when present —
    // is owned by the β-phase profile builder once it sees all waypoints
    // together).
    assert!(
        template.constraints.is_empty(),
        "Waypoint should declare no structure-level constraints \
         (collection-shape invariants are profile-level, not knot-level); \
         got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-11: NaturalSpline refines BoundaryCondition ────────────────────────

/// `NaturalSpline` is the zero-DOF "natural" boundary marker for a piecewise-
/// polynomial profile — semantically "zero second derivative at the
/// endpoints" (PRD §4.1). The semantic invariant is enforced by the β-phase
/// evaluator when it builds the spline coefficients, not as an authoring-
/// time param.
///
/// Test pins three invariants: (a) the structure refines `BoundaryCondition`
/// (via `template.trait_bounds`), (b) it has zero params (marker), (c) it
/// declares no constraints or defaults.
#[test]
fn natural_spline_refines_boundary_condition_with_no_params() {
    let template = find_structure("NaturalSpline");

    assert_eq!(
        template.trait_bounds,
        vec!["BoundaryCondition".to_string()],
        "NaturalSpline must refine BoundaryCondition; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    assert!(
        params.is_empty(),
        "NaturalSpline should declare zero params (marker structure); \
         got: {:?}",
        params
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        template.constraints.is_empty(),
        "NaturalSpline should declare no constraints (semantic invariant \
         is evaluator-enforced); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-13: ClampedSpline refines BoundaryCondition w/ velocity tangents ───

/// `ClampedSpline` is the "clamped" boundary marker for a piecewise-
/// polynomial profile — semantically "specified tangent vectors at the
/// first and last waypoint" (PRD §4.1). The first / last q̇ values are
/// authoring-time data because they are an explicit caller intent, not a
/// derivable evaluator invariant.
///
/// Test pins four invariants: (a) the structure refines `BoundaryCondition`
/// (via `template.trait_bounds`), (b) it declares exactly two params with
/// the canonical `List<JointValue>` (= `List<Real>`) shape, (c) the params
/// are caller-supplied (no defaults), (d) it declares no structure-level
/// constraints (the only meaningful invariant — `start_velocity.len() ==
/// end_velocity.len() == waypoint.values.len()` — is profile-level and
/// owned by the β-phase profile builder).
#[test]
fn clamped_spline_refines_boundary_condition_with_velocity_tangents() {
    let template = find_structure("ClampedSpline");

    assert_eq!(
        template.trait_bounds,
        vec!["BoundaryCondition".to_string()],
        "ClampedSpline must refine BoundaryCondition; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        2,
        "ClampedSpline should declare exactly 2 params (start_velocity, \
         end_velocity); got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("start_velocity", Type::List(Box::new(Type::dimensionless_scalar()))),
        ("end_velocity", Type::List(Box::new(Type::dimensionless_scalar()))),
    ];

    // Param declaration order is part of the contract — pin it explicitly
    // (mirrors the order-sensitive enum-variant assertion in
    // `spline_kind_enum_has_cubic_and_quintic_variants`).
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ClampedSpline params must be declared in canonical order \
         (start_velocity, end_velocity); got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ClampedSpline missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ClampedSpline.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ClampedSpline.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    assert!(
        template.constraints.is_empty(),
        "ClampedSpline should declare no structure-level constraints \
         (collection-shape invariants are profile-level); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-15: PeriodicSpline refines BoundaryCondition ───────────────────────

/// `PeriodicSpline` is the zero-DOF "periodic" boundary marker for a
/// piecewise-polynomial profile — semantically "first and last waypoint
/// agree" (PRD §4.1). Like `NaturalSpline`, the semantic invariant is
/// enforced by the β-phase evaluator when it builds the spline
/// coefficients, not as an authoring-time param.
///
/// Test pins three invariants: (a) the structure refines `BoundaryCondition`
/// (via `template.trait_bounds`), (b) it has zero params (marker), (c) it
/// declares no constraints or defaults.
#[test]
fn periodic_spline_refines_boundary_condition_with_no_params() {
    let template = find_structure("PeriodicSpline");

    assert_eq!(
        template.trait_bounds,
        vec!["BoundaryCondition".to_string()],
        "PeriodicSpline must refine BoundaryCondition; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    assert!(
        params.is_empty(),
        "PeriodicSpline should declare zero params (marker structure); \
         got: {:?}",
        params
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        template.constraints.is_empty(),
        "PeriodicSpline should declare no constraints (semantic invariant \
         is evaluator-enforced); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-17: PiecewisePolynomialProfile param shape ─────────────────────────

/// `PiecewisePolynomialProfile` is the α-phase concrete `Profile` variant —
/// it carries the four authoring-time params the β-phase evaluator needs to
/// build per-segment polynomial coefficients (PRD §4.1):
///
///   - `mechanism   : Real`               (TODO(mechanism-type) placeholder —
///     retargets to the kinematic-
///     completion `Mechanism` type when
///     that PRD lands)
///   - `waypoints   : List<Waypoint>`     (per-knot data; ordered by `t`)
///   - `boundary    : BoundaryCondition`  (variant chooses tangent / endpoint
///     policy — Natural / Clamped /
///     Periodic)
///   - `spline_kind : SplineKind`         (CubicSpline | QuinticSpline)
///
/// `List<Waypoint>` compiles to `Type::List(Box::new(Type::StructureRef
/// ("Waypoint")))` (the structure_def is in the same module). `BoundaryCondition`
/// resolves to `Type::TraitObject("BoundaryCondition")` (trait-typed param
/// precedent: `param m : MaterialSpec` in `trait_typed_param_tests.rs`).
/// `SplineKind` resolves to `Type::Enum("SplineKind")` (precedent:
/// `hardness_scale : Enum(HardnessScale)` in `materials_mechanical_tests.rs`).
///
/// Test pins five invariants: (a) the structure refines `Profile`, (b) the
/// four params exist in canonical order with the expected types, (c) every
/// param has no default (caller-supplied), (d) the constraint count is
/// asserted separately by step-19, (e) the structure refines exactly one
/// trait (Profile, not Profile + BoundaryCondition or similar).
#[test]
fn piecewise_polynomial_profile_has_correct_param_shape() {
    let template = find_structure("PiecewisePolynomialProfile");

    assert_eq!(
        template.trait_bounds,
        vec!["Profile".to_string()],
        "PiecewisePolynomialProfile must refine Profile; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "PiecewisePolynomialProfile should declare exactly 4 params \
         (mechanism, waypoints, boundary, spline_kind); got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("mechanism", Type::dimensionless_scalar()),
        (
            "waypoints",
            Type::List(Box::new(Type::StructureRef("Waypoint".to_string()))),
        ),
        (
            "boundary",
            Type::TraitObject("BoundaryCondition".to_string()),
        ),
        ("spline_kind", Type::Enum("SplineKind".to_string())),
    ];

    // Param declaration order is part of the contract — pin it explicitly
    // (mirrors the order-sensitive enum-variant assertion in
    // `spline_kind_enum_has_cubic_and_quintic_variants`). This is the
    // assertion that makes the docstring's "in canonical order" claim true.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "PiecewisePolynomialProfile params must be declared in canonical \
         order (mechanism, waypoints, boundary, spline_kind); got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "PiecewisePolynomialProfile missing required param '{}'; \
                     got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "PiecewisePolynomialProfile.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "PiecewisePolynomialProfile.{} should have no default_expr \
             (caller-supplied), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }
}

// ─── step-19: PiecewisePolynomialProfile waypoints non-empty constraint ──────

/// `PiecewisePolynomialProfile` declares the structure-level constraint
/// `waypoints.count > 0` — the explicit-contract encoding of the PRD §11-α
/// observable signal "empty waypoints list rejected" (the convention is
/// "make the contract explicit in production code rather than relying on
/// test coverage" — task #2544; same pattern `BucklingOptions.n_modes > 0`
/// already uses in `solver_buckling.ri`).
///
/// SIR-α's `check_constraints_against_templates` machinery (task 3540,
/// landed) evaluates structure-level template constraints at the eval path,
/// so this constraint fires at construction without any further plumbing
/// in this task — runtime rejection is verified end-to-end at β when a
/// concrete `PiecewisePolynomialProfile(waypoints=[])` fixture lands.
///
/// Test pins (a) exactly one constraint (tight count, mirroring buckling's
/// discipline), (b) the constraint is a `BinOp::Gt` shape, (c) the LHS
/// surfaces the `("count", "waypoints")` method-call pair via the
/// `collect_method_call_chain` helper, (d) the RHS is `Literal::Int(0)`.
#[test]
fn piecewise_polynomial_profile_constrains_waypoints_nonempty() {
    let template = find_structure("PiecewisePolynomialProfile");

    assert_eq!(
        template.constraints.len(),
        1,
        "PiecewisePolynomialProfile should declare exactly 1 constraint \
         (waypoints.count > 0); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let constraint = &template.constraints[0];

    // Match BinOp::Gt at the top level.
    let (left, right, op) = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => (left.as_ref(), right.as_ref(), op),
        other => panic!(
            "PiecewisePolynomialProfile constraint should be a BinOp; \
             got: {:?}",
            other
        ),
    };
    assert_eq!(
        *op,
        BinOp::Gt,
        "PiecewisePolynomialProfile constraint should use BinOp::Gt \
         (waypoints.count > 0); got: {:?}",
        op
    );

    // LHS must surface the `("count", "waypoints")` method-call pair.
    let chain = collect_method_call_chain(left);
    assert!(
        chain.contains(&("count", "waypoints")),
        "PiecewisePolynomialProfile constraint LHS should contain a \
         `.count` MethodCall on `waypoints`; got chain: {:?}",
        chain
    );

    // RHS must be the literal `0`. Accept either `Int(0)` or `Real(0.0)`,
    // mirroring the future-proofing rationale established in
    // `buckling_stdlib_compile.rs:357-358` /
    // `solver_elastic_tests.rs:567-579`: `waypoints.count` is `Type::Int`
    // today so the `0` literal stays `Int`, but accepting `Real(0.0)` keeps
    // this test robust against a future literal-coercion change and
    // consistent with the precedent it explicitly cites.
    match &right.kind {
        CompiledExprKind::Literal(Value::Int(0)) => {}
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {}
        other => panic!(
            "PiecewisePolynomialProfile constraint RHS should be \
             Literal(Value::Int(0)) or Literal(Value::Real(0.0)); got: {:?}",
            other
        ),
    }
}

// ─── step-21: evaluate_profile fn signature ───────────────────────────────────

/// `evaluate_profile` is the primary evaluator helper that samples a
/// `PiecewisePolynomialProfile` (or any future `Profile` variant) at time `t`,
/// returning the per-joint position vector (PRD §4.1 line 241).
///
/// Signature: `pub fn evaluate_profile(p: Profile, t: Time) -> List<JointValue>`
///
/// `p : Profile` resolves to `Type::TraitObject("Profile")` — the same
/// trait-typed param resolution verified by `fn_signature_resolves_stdlib_trait_name`
/// in `fn_signature_type_resolution_tests.rs:60-86` (using `MaterialSpec`).
/// `t : Time` resolves to `Type::Scalar { dimension: DimensionVector::TIME }` —
/// already in use for `Waypoint.t` (trajectory_stdlib_compile.rs, step-9).
/// Return type `List<JointValue>` = `List<Real>` via the module-level alias
/// (trajectory.ri header §1).
///
/// Param declaration order is part of the contract — pinned here in the same
/// way step-9 pins `Waypoint`'s (t, values, vels, accels) order.
/// `is_pub == true` because downstream tasks (β/γ/δ/ε/η/ι/ξ) call this fn
/// from user .ri code.
///
/// The shared assertion logic lives in `assert_evaluator_signature` — all three
/// evaluate_* fns share the same 2-param (p:Profile, t:Time) → List<Real> shape.
#[test]
fn evaluate_profile_fn_signature() {
    assert_evaluator_signature("evaluate_profile");
}

// ─── step-23: evaluate_profile_dot fn signature ───────────────────────────────

/// `evaluate_profile_dot` is the first-derivative evaluator helper — it samples
/// the per-joint velocity vector q̇(t) from a `Profile` at time `t`
/// (PRD §4.1 line 242).
///
/// Signature: `pub fn evaluate_profile_dot(p: Profile, t: Time) -> List<JointValue>`
///
/// First-derivative companion to `evaluate_profile` (step-21/22). Param shape
/// is identical: `(p: Profile, t: Time)` — same 2-param (p, t) → List<Real>
/// contract asserted via `assert_evaluator_signature`.
#[test]
fn evaluate_profile_dot_fn_signature() {
    assert_evaluator_signature("evaluate_profile_dot");
}

// ─── step-25: evaluate_profile_ddot fn signature ──────────────────────────────

/// `evaluate_profile_ddot` is the second-derivative evaluator helper — it
/// samples the per-joint acceleration vector q̈(t) from a `Profile` at time
/// `t` (PRD §4.1 line 243).
///
/// Signature: `pub fn evaluate_profile_ddot(p: Profile, t: Time) -> List<JointValue>`
///
/// Second-derivative companion to `evaluate_profile` (step-21/22) and
/// `evaluate_profile_dot` (step-23/24). Same 2-param (p, t) → List<Real>
/// contract asserted via `assert_evaluator_signature`.
#[test]
fn evaluate_profile_ddot_fn_signature() {
    assert_evaluator_signature("evaluate_profile_ddot");
}

// ─── helpers (step-29+) ───────────────────────────────────────────────────────

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
/// Mirrors `require_default` in `modal_options_validation_tests.rs:97-106`.
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

// ─── step-27: profile_duration fn signature ───────────────────────────────────

/// `profile_duration` is the duration accessor — it returns the profile's
/// total `[0, T]` span as a `Time`-dimensioned scalar (PRD §4.1 line 244).
///
/// Signature: `pub fn profile_duration(p: Profile) -> Time`
///
/// This fn differs from the three evaluate_profile* companions in two ways:
///   (a) it takes a single param `p: Profile` (no `t` param — duration is a
///       property of the profile, not a function of the evaluation instant);
///   (b) its return type is `Type::Scalar { dimension: DimensionVector::TIME }`
///       (a Time-dimensioned scalar, not a `List<JointValue>`).
///
/// `p : Profile` resolves to `Type::TraitObject("Profile")` (same as steps
/// 21/23/25). `is_pub == true` for the same downstream-consumer reason.
/// Single-param assertion mirrors `STANDARD_GRAVITY` (zero-param precedent)
/// in `standard_gravity_tests.rs:22-50`.
#[test]
fn profile_duration_fn_signature() {
    let module = load_stdlib_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "profile_duration")
        .unwrap_or_else(|| {
            panic!(
                "profile_duration not found in std/trajectory; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "profile_duration should be pub");

    assert_eq!(
        func.params.len(),
        1,
        "profile_duration should take exactly 1 param (p); got: {:?}",
        func.params
    );

    assert_eq!(
        func.params[0],
        ("p".to_string(), Type::TraitObject("Profile".to_string())),
        "profile_duration param[0] should be (\"p\", TraitObject(\"Profile\")); \
         got: {:?}",
        func.params[0]
    );

    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: DimensionVector::TIME,
        },
        "profile_duration return type should be Scalar<TIME>; got: {:?}",
        func.return_type
    );
}

// ─── step-29: Shaper marker trait ────────────────────────────────────────────

/// `Shaper` is the marker trait for every input-shaper variant (PRD §5).
/// Currently only `TOTSShaper` refines it (Phase 4, task ι). Future Phase 2
/// task δ will add ZVShaper / ZVDShaper / EIShaper refinements.
///
/// Empty in this task by design: each variant carries its own per-strategy
/// fields; the trait exists only to give the `shaper` param on the
/// `input_shape` dispatcher a single nominal type so the SIR-α nominal
/// type-tag dispatches correctly.
///
/// Test pins three invariants: (a) the trait is found, (b) it has zero
/// required members + zero defaults (marker trait), (c) it has no
/// refinements (top-level marker, no parent trait).
/// Mirrors `profile_trait_exists_with_no_params` (step-3) and
/// `boundary_condition_trait_exists_with_no_params` (step-5).
#[test]
fn shaper_trait_exists_with_no_params() {
    let trait_def = find_trait("Shaper");

    assert!(
        trait_def.required_members.is_empty(),
        "Shaper should declare zero required members (marker trait); \
         got: {:?}",
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.defaults.is_empty(),
        "Shaper should declare zero defaults (marker trait); got: {:?}",
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.refinements.is_empty(),
        "Shaper should declare zero refinements (top-level marker, no \
         parent trait); got: {:?}",
        trait_def.refinements
    );
}

// ─── step-31: JointLimit param shape ─────────────────────────────────────────

/// `JointLimit` is the per-joint actuator constraint consumed by TOTSShaper
/// (PRD §5.2). It must declare exactly two params:
///
///   - `joint     : Real`  (TODO(joint-type) placeholder for the future
///     kinematic-completion Joint type)
///   - `max_force : Scalar<Force>`  (task 4580: tightened from Real)
///
/// Both fields are caller-supplied — no canonical defaults. JointLimit
/// refines no trait (zero `trait_bounds`). Constraint `max_force > 0` is
/// asserted separately in step-33.
///
/// Mirrors `waypoint_struct_has_correct_param_shape` (step-9) and
/// `rayleigh_damping_param_shape` in modal_options_validation_tests.rs.
#[test]
fn joint_limit_struct_has_correct_param_shape() {
    let template = find_structure("JointLimit");

    // JointLimit refines no trait (not a BoundaryCondition, not a Profile).
    assert_eq!(
        template.trait_bounds,
        Vec::<String>::new(),
        "JointLimit should refine no traits; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        2,
        "JointLimit should have exactly 2 param cells (joint, max_force); \
         got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("joint", Type::dimensionless_scalar()),
        (
            "max_force",
            Type::Scalar {
                dimension: DimensionVector::FORCE,
            },
        ),
    ];

    // Param declaration order is part of the contract.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "JointLimit params must be in canonical order (joint, max_force); \
         got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!("JointLimit missing required param '{}'; got: {:?}", member, names)
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "JointLimit.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // Both fields are caller-supplied — no canonical defaults.
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "JointLimit.{} should have no default_expr (caller-supplied); \
             got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }
}

// ─── step-33: JointLimit max_force positivity constraint ─────────────────────

/// `JointLimit` must declare exactly one constraint: `max_force > 0 * 1N`.
///
/// A "max force" of zero or negative is physically degenerate — only positive
/// values are meaningful as an actuator limit. The dimensioned-zero RHS
/// (`0 * 1N`) is required because `max_force : Scalar<Force>` — a bare `0`
/// would be `Type::dimensionless_scalar()`, dim-incompatible with Force
/// (esc-3115 rule). Making the contract explicit in production code (task
/// #2544 convention) rather than relying solely on test coverage.
///
/// Tight count == 1 is a regression gate: `joint : Real` is explicitly NOT
/// constrained (it is an entity-handle placeholder — no meaningful scalar
/// predicate on a handle).
///
/// Mirrors `modal_options_constrains_positivity_invariants` in
/// modal_options_validation_tests.rs and
/// `piecewise_polynomial_profile_constrains_waypoints_nonempty` (step-19).
///
/// These constraint declarations feed the SIR-α generic constraint-firing
/// pipeline, which is pinned end-to-end by
/// `crates/reify-eval/tests/stress_error_messages.rs::constraint_violation_diagnostic`
/// (constraint → `Satisfaction::Violated` diagnostic) and the
/// `Value::StructureInstance` round-trip in
/// `crates/reify-eval/tests/structure_instance_e2e.rs`. A JointLimit-specific
/// construction-time firing test would duplicate that generic coverage.
#[test]
fn joint_limit_constrains_max_force_positive() {
    let template = find_structure("JointLimit");

    // Tight count: exactly 1 constraint (regression-gate against accidental
    // over-declaration of a constraint on `joint`).
    assert_eq!(
        template.constraints.len(),
        1,
        "JointLimit should declare exactly 1 constraint (max_force > 0); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let constraint = &template.constraints[0];

    // Constraint must be BinOp::Gt.
    let (left, right, op) = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => (left.as_ref(), right.as_ref(), op),
        other => panic!(
            "JointLimit constraint should be a BinOp; got: {:?}",
            other
        ),
    };
    assert_eq!(
        *op,
        BinOp::Gt,
        "JointLimit constraint should use BinOp::Gt (max_force > 0); \
         got: {:?}",
        op
    );

    // LHS must reference `max_force`.
    let lhs_refs = collect_value_ref_members(left);
    assert!(
        lhs_refs.iter().any(|m| m.as_str() == "max_force"),
        "JointLimit constraint LHS should reference `max_force`; \
         got refs: {:?}",
        lhs_refs
    );

    // RHS must be dimensioned-zero: `0 * 1N` compiles to
    // BinOp { op: Mul, left: Literal(Int(0)), right: Literal(Scalar{si:1.0, dim:FORCE}) }.
    // A bare `0` is Type::dimensionless_scalar() and is dim-incompatible with
    // Scalar<Force> (esc-3115 rule) — the dimensioned form is required.
    let rhs_ok = match &right.kind {
        CompiledExprKind::BinOp { op: rhs_op, left: rhs_left, right: rhs_right } => {
            *rhs_op == BinOp::Mul
                && matches!(&rhs_left.kind, CompiledExprKind::Literal(Value::Int(0)))
                && matches!(
                    &rhs_right.kind,
                    CompiledExprKind::Literal(Value::Scalar { dimension, .. })
                        if *dimension == DimensionVector::FORCE
                )
        }
        _ => false,
    };
    assert!(
        rhs_ok,
        "JointLimit constraint RHS should be dimensioned-zero `0 * 1N` \
         (BinOp::Mul, left=Literal(Int(0)), right=Literal(Scalar{{dim:FORCE}})); \
         got: {:?}",
        right.kind
    );
}

// ─── step-35: TOTSShaper param shape ─────────────────────────────────────────

/// `TOTSShaper` is the time-optimal trajectory shaper value type (PRD §5.2).
/// It must refine the `Shaper` marker trait and declare exactly 7 params in
/// canonical order:
///
///   - `modes             : List<Mode>`        (cross-module: Mode from std.modal.analysis)
///   - `actuator_limits   : List<JointLimit>`  (JointLimit declared in this file above)
///   - `velocity_limit    : Scalar<Velocity>`   (task 4580: tightened from Real)
///   - `acceleration_limit: Scalar<Acceleration>` (task 4580: tightened from Real)
///   - `vibration_tolerance: Real`             (genuinely dimensionless residual fraction)
///   - `max_iters         : Int`               (solver iteration cap)
///   - `tol               : Real`              (convergence threshold)
///
/// `Mode` resolves via the growing-prelude cross-module mechanism —
/// std.modal.analysis is loaded at slot 16 BEFORE std.trajectory at slot 17
/// (stdlib_loader.rs:110-116). Type encoding: `Type::List(Box::new(
/// Type::StructureRef("Mode")))` — identical to ModalResult.modes.
///
/// ⚠ Duplicate-Mode note: the stdlib has TWO `structure def Mode` declarations
/// with different field shapes — `modal_analysis.ri:187` (frequency, shape,
/// participation_mass, damping_ratio) and `solver_buckling.ri:148` (eigenvalue,
/// mode_shape). `Type::StructureRef("Mode")` carries only the name, so the
/// assertion below cannot distinguish which Mode was bound by name resolution.
/// Correct resolution is guaranteed by load order: slot 16 (std.modal.analysis)
/// is compiled before slot 17 (std.trajectory), so the modal-analysis Mode wins
/// the first-wins shadow rule. `modal_analysis.ri:137-141` documents this
/// coexistence; if name-shadowing ever surfaces as a problem, the fallback is a
/// one-line rename in `trajectory.ri`.
///
/// Does NOT assert defaults (step-37) or constraints (step-39).
/// Mirrors `piecewise_polynomial_profile_has_correct_param_shape` (step-17)
/// and `modal_options_struct_has_correct_param_shape` in
/// modal_options_validation_tests.rs.
#[test]
fn tots_shaper_struct_has_correct_param_shape() {
    let template = find_structure("TOTSShaper");

    // (a) refines Shaper marker trait.
    assert_eq!(
        template.trait_bounds,
        vec!["Shaper".to_string()],
        "TOTSShaper must refine Shaper; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (b) tight param count.
    assert_eq!(
        params.len(),
        7,
        "TOTSShaper should have exactly 7 params \
         (modes, actuator_limits, velocity_limit, acceleration_limit, \
          vibration_tolerance, max_iters, tol); got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "modes",
            Type::List(Box::new(Type::StructureRef("Mode".to_string()))),
        ),
        (
            "actuator_limits",
            Type::List(Box::new(Type::StructureRef("JointLimit".to_string()))),
        ),
        (
            "velocity_limit",
            Type::Scalar {
                dimension: DimensionVector::VELOCITY,
            },
        ),
        (
            "acceleration_limit",
            Type::Scalar {
                dimension: DimensionVector::ACCELERATION,
            },
        ),
        ("vibration_tolerance", Type::dimensionless_scalar()),
        ("max_iters", Type::Int),
        ("tol", Type::dimensionless_scalar()),
    ];

    // (c) Param declaration order is part of the contract.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "TOTSShaper params must be in canonical order; got: {:?}",
        names
    );

    // (d) type assertion per param.
    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "TOTSShaper missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "TOTSShaper.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── step-37: TOTSShaper param defaults ──────────────────────────────────────

/// `TOTSShaper` declares two param defaults per PRD §5.2:
///   - `max_iters : Int = 100`         — solver iteration cap
///   - `tol       : Real = 0.000001`   — convergence threshold (= 1e-6 in decimal)
///
/// The other five params (modes, actuator_limits, velocity_limit,
/// acceleration_limit, vibration_tolerance) are required at construction —
/// no canonical default exists for these caller-supplied values.
///
/// Decimal-encoding discipline: Reify's grammar has no scientific notation,
/// so 1e-6 is spelled as `0.000001` (same convention as modal_analysis.ri
/// tol = 0.000000001 = 1e-9 at modal_analysis.ri:356). IEEE-754
/// round-to-nearest of these exact decimal literals is deterministic.
///
/// Mirrors `modal_options_param_defaults_match_spec` in
/// modal_options_validation_tests.rs.
#[test]
fn tots_shaper_param_defaults_match_spec() {
    let template = find_structure("TOTSShaper");

    // max_iters = 100 per PRD §5.2 explicit default.
    let max_iters_default = require_default(template, "max_iters");
    match &max_iters_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 100, "max_iters default should be 100, got: {}", v)
        }
        other => panic!(
            "max_iters default should be Literal(Value::Int(100)), got: {:?}",
            other
        ),
    }

    // tol = 0.000001 = 1e-6 per PRD §5.2; decimal-encoding (no sci notation
    // in Reify grammar). Strict-equality safe — IEEE-754 deterministic.
    let tol_default = require_default(template, "tol");
    match &tol_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.000001,
            "tol default should be exactly 0.000001 (= 1e-6), got: {}",
            v
        ),
        other => panic!(
            "tol default should be Literal(Value::Real(0.000001)), got: {:?}",
            other
        ),
    }

    // The other five params are required at construction — no canonical
    // default (caller must supply modes, actuator_limits, velocity_limit,
    // acceleration_limit, vibration_tolerance).
    for member in [
        "modes",
        "actuator_limits",
        "velocity_limit",
        "acceleration_limit",
        "vibration_tolerance",
    ] {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("TOTSShaper.{} missing", member));
        assert!(
            cell.default_expr.is_none(),
            "TOTSShaper.{} should have NO default_expr (required at \
             construction), but got: {:?}",
            member,
            cell.default_expr
        );
    }
}

// ─── step-39: TOTSShaper design-param positivity/range constraints ────────────

/// `TOTSShaper` must declare exactly 6 constraints per PRD §5.2 + §11 Phase 2:
///
///   constraint velocity_limit     > 0 * 1m / 1s        (dimensioned-zero: task 4580)
///   constraint acceleration_limit > 0 * 1m / (1s * 1s) (dimensioned-zero: task 4580)
///   constraint vibration_tolerance > 0                  (dimensionless: plain `> 0`)
///   constraint vibration_tolerance <= 1   (upper bound: (0,1] interval)
///   constraint max_iters           > 0
///   constraint tol                 > 0
///
/// velocity_limit and acceleration_limit use dimensioned-zero RHS because their
/// param types are Scalar<Velocity>/Scalar<Acceleration> (esc-3115 rule: a bare
/// `0` is dimensionless and dim-incompatible with a dimensioned LHS).
/// vibration_tolerance/max_iters/tol remain plain (dimensionless params).
///
/// The `vibration_tolerance ∈ (0, 1]` interval decomposes into two scalar
/// predicates because Reify's constraint grammar admits BinOp predicates but
/// no interval form. `BinOp::Le` handles `<= 1` (confirmed in type_compat.rs).
///
/// Tight count == 6 regression-gates against accidental over/under-declaration.
/// Explicitly NOT constrained (collection invariants deferred to κ-task):
///   `modes : List<Mode>` and `actuator_limits : List<JointLimit>`.
///
/// Mirrors `modal_options_constrains_positivity_invariants` in
/// modal_options_validation_tests.rs.
///
/// These declarations feed the SIR-α generic constraint-firing pipeline; the
/// construction-time `Satisfaction::Violated` signal is pinned end-to-end by
/// `crates/reify-eval/tests/stress_error_messages.rs::constraint_violation_diagnostic`
/// and `crates/reify-eval/tests/structure_instance_e2e.rs` — no duplicate
/// TOTSShaper-specific construction-time firing test is needed here.
#[test]
fn tots_shaper_constrains_design_param_invariants() {
    let template = find_structure("TOTSShaper");

    // Tight count: exactly 6 constraints.
    assert_eq!(
        template.constraints.len(),
        6,
        "TOTSShaper should declare exactly 6 constraints; \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // velocity_limit: positivity constraint with dimensioned-zero RHS
    // (`0 * 1m / 1s` = Scalar<Velocity>). Two-layer check:
    //   (1) structural shape: is_dimensioned_zero_binop — `0 * <unit>` chain.
    //   (2) result_type == Scalar{VELOCITY} — catches a wrong-unit regression
    //       (e.g. `0 * 1N`) that is_dimensioned_zero_binop alone would miss,
    //       since esc-3115 only fires at module-load time.
    // Mirrors the FORCE dimension-pin in joint_limit_constrains_max_force_positive.
    let velocity_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                *op == BinOp::Gt
                    && collect_value_ref_members(left)
                        .iter()
                        .any(|m| m.as_str() == "velocity_limit")
                    && is_dimensioned_zero_binop(right)
                    && matches!(
                        &right.result_type,
                        Type::Scalar { dimension } if *dimension == DimensionVector::VELOCITY
                    )
            }
            _ => false,
        }
    });
    assert!(
        velocity_matched,
        "TOTSShaper should declare `constraint velocity_limit > 0 * 1m/1s` \
         (dimensioned-zero BinOp, result_type=Scalar{{VELOCITY}}; task 4580); \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // acceleration_limit: positivity constraint with dimensioned-zero RHS
    // (`0 * 1m / (1s * 1s)` = Scalar<Acceleration>). Same two-layer check as
    // velocity_limit above.
    let accel_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                *op == BinOp::Gt
                    && collect_value_ref_members(left)
                        .iter()
                        .any(|m| m.as_str() == "acceleration_limit")
                    && is_dimensioned_zero_binop(right)
                    && matches!(
                        &right.result_type,
                        Type::Scalar { dimension } if *dimension == DimensionVector::ACCELERATION
                    )
            }
            _ => false,
        }
    });
    assert!(
        accel_matched,
        "TOTSShaper should declare `constraint acceleration_limit > 0 * 1m/(1s*1s)` \
         (dimensioned-zero BinOp, result_type=Scalar{{ACCELERATION}}; task 4580); \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // vibration_tolerance, max_iters, tol: plain positivity constraints (> 0),
    // dimensionless params — plain `Literal(Int(0))` RHS.
    for required in &["vibration_tolerance", "max_iters", "tol"] {
        let matched = template.constraints.iter().any(|c| {
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt
                        || !collect_value_ref_members(left).iter().any(|m| m.as_str() == *required)
                    {
                        return false;
                    }
                    match &right.kind {
                        CompiledExprKind::Literal(Value::Int(0)) => true,
                        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                        _ => false,
                    }
                }
                _ => false,
            }
        });
        assert!(
            matched,
            "TOTSShaper should declare `constraint {} > 0` (dimensionless plain RHS); \
             got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }

    // Upper-bound constraint: vibration_tolerance <= 1 (completing the (0,1]
    // interval per PRD §11 Phase 2 ε spec). Accept Int(1) or Real(1.0) RHS.
    let le_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Le
                    || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "vibration_tolerance")
                {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(1)) => true,
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 1.0 => true,
                    _ => false,
                }
            }
            _ => false,
        }
    });
    assert!(
        le_matched,
        "TOTSShaper should declare `constraint vibration_tolerance <= 1` \
         (upper bound completing the (0,1] interval per PRD §11 Phase 2 ε); \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-41: ZVShaper param shape and constraint ────────────────────────────

/// `ZVShaper` is the Zero-Vibration impulse shaper (PRD §5.1).
/// It must refine the `Shaper` marker trait and declare exactly 2 params:
///
///   - `target_frequency : Frequency`   (Type::Scalar{dimension: FREQUENCY})
///   - `damping_ratio    : Real = 0.0`  (default 0.0 per PRD §5.1 — ZV
///     assumes undamped as the base case)
///
/// Exactly 1 constraint: `target_frequency > 0Hz` (BinOp::Gt, RHS
/// Value::Scalar{si_value:0.0, dimension:FREQUENCY} — the dimensioned
/// literal is required per the esc-3115 rule; a bare `0` is Type::dimensionless_scalar()
/// and dim-incompatible with Frequency).
///
/// Uses the HarmonicForce Frequency-constraint pattern
/// (modal_options_validation_tests.rs:1296-1348) and the TOTSShaper
/// trait-bounds + param-shape assertion style (step-35).
///
/// Landed alongside the structure_def in trajectory.ri:513-518.
#[test]
fn zv_shaper_struct_has_correct_param_shape_and_constraint() {
    // step-41
    let template = find_structure("ZVShaper");

    // (a) refines Shaper marker trait.
    assert_eq!(
        template.trait_bounds,
        vec!["Shaper".to_string()],
        "ZVShaper must refine Shaper; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (b) tight param count.
    assert_eq!(
        params.len(),
        2,
        "ZVShaper should have exactly 2 params (target_frequency, damping_ratio); \
         got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "target_frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        ("damping_ratio", Type::dimensionless_scalar()),
    ];

    // (c) param declaration order is part of the contract.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ZVShaper params must be in canonical order \
         (target_frequency, damping_ratio); got: {:?}",
        names
    );

    // (d) type assertion per param.
    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ZVShaper missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ZVShaper.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // (e) target_frequency has no default (caller-supplied frequency).
    let tf_cell = params
        .iter()
        .find(|vc| vc.id.member == "target_frequency")
        .unwrap();
    assert!(
        tf_cell.default_expr.is_none(),
        "ZVShaper.target_frequency should have NO default_expr (caller-supplied); \
         got: {:?}",
        tf_cell.default_expr
    );

    // (f) damping_ratio defaults to 0.0 (ZV assumes undamped per PRD §5.1).
    let dr_default = require_default(template, "damping_ratio");
    match &dr_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {}
        other => panic!(
            "ZVShaper.damping_ratio default should be Literal(Real(0.0)); \
             the .ri file declares `0.0` (decimal literal → Value::Real), \
             not Int(0); got: {:?}",
            other
        ),
    }

    // (g) exactly 1 constraint: target_frequency > 0Hz.
    assert_eq!(
        template.constraints.len(),
        1,
        "ZVShaper should declare exactly 1 constraint (target_frequency > 0Hz); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let constraint = &template.constraints[0];
    let matched = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            if *op != BinOp::Gt
                || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "target_frequency")
            {
                false
            } else {
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, dimension })
                        if *si_value == 0.0 && *dimension == DimensionVector::FREQUENCY
                )
            }
        }
        _ => false,
    };
    assert!(
        matched,
        "ZVShaper should declare `constraint target_frequency > 0Hz` \
         (BinOp::Gt, LHS refs target_frequency, \
          RHS Value::Scalar{{si_value:0.0, dimension:FREQUENCY}} — \
          dimensioned literal required per esc-3115 rule); \
         got: {:?}",
        constraint.expr.kind
    );
}

// ─── step-43: ZVDShaper param shape and constraint ───────────────────────────

/// `ZVDShaper` is the Zero-Vibration-Derivative impulse shaper (PRD §5.1).
/// It must refine the `Shaper` marker trait and declare exactly 2 params:
///
///   - `target_frequency : Frequency`  (Type::Scalar{dimension: FREQUENCY})
///   - `damping_ratio    : Real`       (caller-supplied, NO default —
///     ZVD's damping ratio is a required design parameter, unlike ZVShaper
///     which defaults to undamped)
///
/// Exactly 1 constraint: `target_frequency > 0Hz` (BinOp::Gt, RHS
/// Value::Scalar{si_value:0.0, dimension:FREQUENCY} — dimensioned literal
/// required per esc-3115 rule; bare `0` is Type::dimensionless_scalar(), dim-incompatible
/// with Frequency).
///
/// Key distinction from ZVShaper: BOTH params have `default_expr.is_none()`.
///
/// Landed alongside the structure_def in trajectory.ri:536-541.
#[test]
fn zvd_shaper_struct_has_correct_param_shape_and_constraint() {
    // step-43
    let template = find_structure("ZVDShaper");

    // (a) refines Shaper marker trait.
    assert_eq!(
        template.trait_bounds,
        vec!["Shaper".to_string()],
        "ZVDShaper must refine Shaper; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (b) tight param count.
    assert_eq!(
        params.len(),
        2,
        "ZVDShaper should have exactly 2 params (target_frequency, damping_ratio); \
         got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "target_frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        ("damping_ratio", Type::dimensionless_scalar()),
    ];

    // (c) param declaration order is part of the contract.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ZVDShaper params must be in canonical order \
         (target_frequency, damping_ratio); got: {:?}",
        names
    );

    // (d) type assertion per param.
    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ZVDShaper missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ZVDShaper.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // (e) BOTH params are caller-supplied — neither has a default.
    // This distinguishes ZVD from ZV (which defaults damping_ratio to 0.0).
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ZVDShaper.{} should have NO default_expr (caller-supplied); \
             got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (f) exactly 1 constraint: target_frequency > 0Hz.
    assert_eq!(
        template.constraints.len(),
        1,
        "ZVDShaper should declare exactly 1 constraint (target_frequency > 0Hz); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let constraint = &template.constraints[0];
    let matched = match &constraint.expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            if *op != BinOp::Gt
                || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "target_frequency")
            {
                false
            } else {
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, dimension })
                        if *si_value == 0.0 && *dimension == DimensionVector::FREQUENCY
                )
            }
        }
        _ => false,
    };
    assert!(
        matched,
        "ZVDShaper should declare `constraint target_frequency > 0Hz` \
         (BinOp::Gt, LHS refs target_frequency, \
          RHS Value::Scalar{{si_value:0.0, dimension:FREQUENCY}} — \
          dimensioned literal required per esc-3115 rule); \
         got: {:?}",
        constraint.expr.kind
    );
}

// ─── step-45: EIShaper param shape and constraints ───────────────────────────

/// `EIShaper` is the Extra-Insensitive impulse shaper (PRD §5.1).
/// It must refine `Shaper` and declare exactly 3 params:
///
///   - `target_frequency    : Frequency`  (Type::Scalar{dimension: FREQUENCY})
///   - `damping_ratio       : Real`       (caller-supplied, no default)
///   - `vibration_tolerance : Real`       (caller-supplied, no default)
///
/// Exactly 3 constraints:
///   (a) `target_frequency > 0Hz`        — BinOp::Gt, RHS Scalar{0.0,FREQ}
///   (b) `vibration_tolerance > 0`       — BinOp::Gt, RHS Int(0)/Real(0.0)
///   (c) `vibration_tolerance <= 1`      — BinOp::Le, RHS Int(1)/Real(1.0)
///
/// The vibration_tolerance ∈ (0,1] interval splits into two scalar predicates
/// (same discipline as TOTSShaper, step-39 at line 1391-1477).
///
/// Landed alongside the structure_def in trajectory.ri:570-578.
#[test]
fn ei_shaper_struct_has_correct_param_shape_and_constraints() {
    // step-45
    let template = find_structure("EIShaper");

    // (a) refines Shaper marker trait.
    assert_eq!(
        template.trait_bounds,
        vec!["Shaper".to_string()],
        "EIShaper must refine Shaper; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (b) tight param count.
    assert_eq!(
        params.len(),
        3,
        "EIShaper should have exactly 3 params \
         (target_frequency, damping_ratio, vibration_tolerance); got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "target_frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        ("damping_ratio", Type::dimensionless_scalar()),
        ("vibration_tolerance", Type::dimensionless_scalar()),
    ];

    // (c) param declaration order is part of the contract.
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "EIShaper params must be in canonical order \
         (target_frequency, damping_ratio, vibration_tolerance); got: {:?}",
        names
    );

    // (d) type assertion per param.
    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "EIShaper missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "EIShaper.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // (e) all three params are caller-supplied — no defaults.
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "EIShaper.{} should have NO default_expr (caller-supplied); \
             got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (f) exactly 3 constraints.
    assert_eq!(
        template.constraints.len(),
        3,
        "EIShaper should declare exactly 3 constraints \
         (target_frequency > 0Hz, vibration_tolerance > 0, \
          vibration_tolerance <= 1); got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (g) target_frequency > 0Hz (dimensioned literal — esc-3115 rule).
    let tf_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt
                    || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "target_frequency")
                {
                    return false;
                }
                matches!(
                    &right.kind,
                    CompiledExprKind::Literal(Value::Scalar { si_value, dimension })
                        if *si_value == 0.0 && *dimension == DimensionVector::FREQUENCY
                )
            }
            _ => false,
        }
    });
    assert!(
        tf_matched,
        "EIShaper should declare `constraint target_frequency > 0Hz`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (h) vibration_tolerance > 0 (lower bound of (0,1] interval).
    let vt_gt_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt
                    || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "vibration_tolerance")
                {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(0)) => true,
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                    _ => false,
                }
            }
            _ => false,
        }
    });
    assert!(
        vt_gt_matched,
        "EIShaper should declare `constraint vibration_tolerance > 0` \
         (lower bound of (0,1] interval); got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (i) vibration_tolerance <= 1 (upper bound of (0,1] interval).
    let vt_le_matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Le
                    || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "vibration_tolerance")
                {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Int(1)) => true,
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 1.0 => true,
                    _ => false,
                }
            }
            _ => false,
        }
    });
    assert!(
        vt_le_matched,
        "EIShaper should declare `constraint vibration_tolerance <= 1` \
         (upper bound completing the (0,1] interval per PRD §5.1); \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-47: CascadedShaper param shape ─────────────────────────────────────

/// `CascadedShaper` chains multiple `Shaper` instances in sequence (PRD §5.1).
/// It must refine `Shaper` and declare exactly 1 param:
///
///   - `shapers : List<Shaper>`  →  Type::List(Box::new(
///     Type::TraitObject("Shaper")))
///
/// Zero constraints: an empty cascade is a valid identity (no shaping) —
/// the collection invariant is deferred to the ε consumer, matching
/// TOTSShaper's modes/actuator_limits discipline. The test asserts exactly
/// 0 constraints as a regression gate against accidental over-declaration.
///
/// Mirrors `CompositeForce.sources : List<ForcingFunction>` pattern for the
/// `Type::List(TraitObject(...))` assertion shape
/// (modal_options_validation_tests.rs).
///
/// Landed alongside the structure_def in trajectory.ri:600-602.
#[test]
fn cascaded_shaper_struct_has_correct_param_shape() {
    // step-47
    let template = find_structure("CascadedShaper");

    // (a) refines Shaper marker trait.
    assert_eq!(
        template.trait_bounds,
        vec!["Shaper".to_string()],
        "CascadedShaper must refine Shaper; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (b) exactly 1 param.
    assert_eq!(
        params.len(),
        1,
        "CascadedShaper should have exactly 1 param (shapers); got: {:?}",
        names
    );

    // (c) param name and type: shapers : List<TraitObject("Shaper")>.
    let shapers_cell = &params[0];
    assert_eq!(
        shapers_cell.id.member, "shapers",
        "CascadedShaper param[0] name should be 'shapers'; got: {:?}",
        shapers_cell.id.member
    );
    assert_eq!(
        shapers_cell.cell_type,
        Type::List(Box::new(Type::TraitObject("Shaper".to_string()))),
        "CascadedShaper.shapers should be Type::List(TraitObject(\"Shaper\")); \
         got: {:?}",
        shapers_cell.cell_type
    );

    // (d) no default (caller-supplied sequence of shapers).
    assert!(
        shapers_cell.default_expr.is_none(),
        "CascadedShaper.shapers should have NO default_expr (caller-supplied); \
         got: {:?}",
        shapers_cell.default_expr
    );

    // (e) zero constraints — regression gate against accidental over-declaration.
    // An empty cascade is a valid identity (no shaping); the collection
    // invariant is deferred to the ε consumer (matches TOTSShaper.modes /
    // actuator_limits discipline).
    assert_eq!(
        template.constraints.len(),
        0,
        "CascadedShaper should declare exactly 0 constraints \
         (collection invariant deferred to ε consumer); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── η (task 3859): EndEffectorTrack lazy-accessor helpers + tests ───────────

/// Generic name-lookup helper used by [`find_trait`], [`find_enum`], and
/// [`find_function`]. Returns the first item in `items` where
/// `key(item) == name`, or panics with a descriptive message listing the
/// available names.
///
/// `kind` labels the item type in the error string (e.g. `"fn"`, `"trait"`).
/// `key` extracts the name string for comparison and error display.
///
/// Not used by [`find_structure`] — that helper additionally filters on
/// `entity_kind` and emits `(name, entity_kind)` pairs in its error message.
fn find_named<T>(
    items: &'static [T],
    name: &str,
    kind: &str,
    key: impl Fn(&T) -> &str,
) -> &'static T {
    let result = items.iter().find(|item| key(item) == name);
    result.unwrap_or_else(|| {
        panic!(
            "expected `{kind} {name}` in std/trajectory, got: {:?}",
            items.iter().map(&key).collect::<Vec<_>>()
        )
    })
}

/// Look up a compiled function by name within the `std/trajectory` module.
fn find_function(name: &str) -> &'static CompiledFunction {
    find_named(&load_stdlib_module().functions, name, "fn", |f| {
        f.name.as_str()
    })
}

// ─── step-29: EndEffectorTrack structure param shape ─────────────────────────

/// `EndEffectorTrack` is the forward-pass simulator's output value type
/// (PRD §6.2). It carries six params that capture the full time-history of
/// end-effector poses across every monitored location:
///
///   - `mechanism        : Real`                    (TODO(mechanism-type) placeholder)
///   - `modal_result     : Real`                    (TODO(modal-result-type) placeholder)
///   - `t_samples        : List<Time>`              (sampling instants)
///   - `nominal_pose     : List<List<Pose3>>`       (outer: time, inner: locations)
///   - `vibration_offset : List<List<Vec3>>`        (outer: time, inner: locations)
///   - `combined_pose    : List<List<Pose3>>`       (outer: time, inner: locations)
///
/// `Pose3` and `Vec3` are module-level aliases for `Real`; so all three nested
/// list params compile to `Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar()))))`.
/// `t_samples : List<Time>` compiles to `Type::List(Box::new(Type::Scalar
/// { dimension: DimensionVector::TIME }))`.
///
/// Test pins four invariants: (a) no trait bound (plain value-type output —
/// NOT a Profile/BoundaryCondition variant); (b) exactly 6 params in canonical
/// order; (c) every param has no default (simulator fully determines output);
/// (d) no structure-level constraint (simulator output — no caller invariant).
#[test]
fn end_effector_track_struct_has_correct_param_shape() {
    let template = find_structure("EndEffectorTrack");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) Plain value type — no trait bound.
    assert!(
        template.trait_bounds.is_empty(),
        "EndEffectorTrack should have no trait bounds (plain simulator output, \
         not a Profile/BoundaryCondition variant); got: {:?}",
        template.trait_bounds
    );

    // (b) Exactly 6 params in canonical order.
    assert_eq!(
        params.len(),
        6,
        "EndEffectorTrack should have exactly 6 param cells; got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("mechanism", Type::dimensionless_scalar()),
        ("modal_result", Type::dimensionless_scalar()),
        (
            "t_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::TIME,
            })),
        ),
        (
            "nominal_pose",
            Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
        (
            "vibration_offset",
            Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
        (
            "combined_pose",
            Type::List(Box::new(Type::List(Box::new(Type::dimensionless_scalar())))),
        ),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "EndEffectorTrack params must be declared in canonical order \
         (mechanism, modal_result, t_samples, nominal_pose, vibration_offset, \
         combined_pose); got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "EndEffectorTrack missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "EndEffectorTrack.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // (c) Simulator output — every param fully determined, no defaults.
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "EndEffectorTrack.{} should have no default_expr (simulator output, \
             fully determined); got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) No structure-level constraint (no caller-authored invariant to enforce).
    assert!(
        template.constraints.is_empty(),
        "EndEffectorTrack should declare no structure-level constraints \
         (simulator output — no caller invariant); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-31: end_effector_track fn signature ────────────────────────────────

/// `end_effector_track` is the lazy accessor that extracts the per-time-sample
/// combined pose list for a single named location (PRD §6.2).
///
/// Signature: `pub fn end_effector_track(track: EndEffectorTrack, location: LocationId) -> List<Pose3>`
///
/// `track : EndEffectorTrack` resolves to `Type::StructureRef("EndEffectorTrack")`
/// — the structure_def is in the same module (same name-resolution path as
/// `List<Waypoint>` in PiecewisePolynomialProfile.waypoints).
/// `location : LocationId` resolves to `Type::dimensionless_scalar()` (LocationId = Real alias).
/// Return type `List<Pose3>` = `List<Real>` via the Pose3 = Real alias.
///
/// Param order is part of the contract — (track, location), not (location, track).
/// `is_pub == true` because downstream tasks (θ/ι/ξ) call this fn from user .ri
/// code.
#[test]
fn end_effector_track_fn_has_correct_signature() {
    let func = find_function("end_effector_track");

    assert!(func.is_pub, "end_effector_track should be pub");

    assert_eq!(
        func.params.len(),
        2,
        "end_effector_track should take exactly 2 params (track, location); \
         got: {:?}",
        func.params
    );

    assert_eq!(
        func.params[0],
        (
            "track".to_string(),
            Type::StructureRef("EndEffectorTrack".to_string())
        ),
        "end_effector_track param[0] should be (\"track\", StructureRef(\
         \"EndEffectorTrack\")); got: {:?}",
        func.params[0]
    );
    assert_eq!(
        func.params[1],
        ("location".to_string(), Type::dimensionless_scalar()),
        "end_effector_track param[1] should be (\"location\", Real) \
         (LocationId = Real alias); got: {:?}",
        func.params[1]
    );

    assert_eq!(
        func.return_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
        "end_effector_track return type should be List<Real> (= List<Pose3>); \
         got: {:?}",
        func.return_type
    );
}

// ─── step-33: deviation_from_nominal fn signature ────────────────────────────

/// `deviation_from_nominal` is the lazy accessor that computes per-time-sample
/// Euclidean distance between the combined pose and the nominal pose at a
/// single named location (PRD §6.2).
///
/// Signature: `pub fn deviation_from_nominal(track: EndEffectorTrack, location: LocationId) -> List<Length>`
///
/// Params are identical to `end_effector_track`: `(track: EndEffectorTrack,
/// location: LocationId)` — same StructureRef + Real pair, same order.
/// Return type `List<Length>` = `Type::List(Box::new(Type::Scalar {
/// dimension: DimensionVector::LENGTH }))` — one Length scalar per time sample.
#[test]
fn deviation_from_nominal_fn_has_correct_signature() {
    let func = find_function("deviation_from_nominal");

    assert!(func.is_pub, "deviation_from_nominal should be pub");

    assert_eq!(
        func.params.len(),
        2,
        "deviation_from_nominal should take exactly 2 params (track, location); \
         got: {:?}",
        func.params
    );

    assert_eq!(
        func.params[0],
        (
            "track".to_string(),
            Type::StructureRef("EndEffectorTrack".to_string())
        ),
        "deviation_from_nominal param[0] should be (\"track\", StructureRef(\
         \"EndEffectorTrack\")); got: {:?}",
        func.params[0]
    );
    assert_eq!(
        func.params[1],
        ("location".to_string(), Type::dimensionless_scalar()),
        "deviation_from_nominal param[1] should be (\"location\", Real) \
         (LocationId = Real alias); got: {:?}",
        func.params[1]
    );

    assert_eq!(
        func.return_type,
        Type::List(Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        })),
        "deviation_from_nominal return type should be List<Scalar<LENGTH>> \
         (= List<Length>); got: {:?}",
        func.return_type
    );
}

// ─── step-35: peak_deviation fn signature ────────────────────────────────────

/// `peak_deviation` is the lazy accessor that returns the maximum Euclidean
/// deviation from nominal across all time samples at a single named location
/// (PRD §6.2).
///
/// Signature: `pub fn peak_deviation(track: EndEffectorTrack, location: LocationId) -> Length`
///
/// Params are identical to the other two η accessors: `(track:
/// EndEffectorTrack, location: LocationId)` — same StructureRef + Real pair.
/// Return type `Length` = `Type::Scalar { dimension: DimensionVector::LENGTH }`
/// — a scalar (NOT a list); this is the single peak value over all time
/// samples (contrast with `deviation_from_nominal` which returns one value per
/// time sample).
#[test]
fn peak_deviation_fn_has_correct_signature() {
    let func = find_function("peak_deviation");

    assert!(func.is_pub, "peak_deviation should be pub");

    assert_eq!(
        func.params.len(),
        2,
        "peak_deviation should take exactly 2 params (track, location); \
         got: {:?}",
        func.params
    );

    assert_eq!(
        func.params[0],
        (
            "track".to_string(),
            Type::StructureRef("EndEffectorTrack".to_string())
        ),
        "peak_deviation param[0] should be (\"track\", StructureRef(\
         \"EndEffectorTrack\")); got: {:?}",
        func.params[0]
    );
    assert_eq!(
        func.params[1],
        ("location".to_string(), Type::dimensionless_scalar()),
        "peak_deviation param[1] should be (\"location\", Real) \
         (LocationId = Real alias); got: {:?}",
        func.params[1]
    );

    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "peak_deviation return type should be Scalar<LENGTH> (= Length, scalar); \
         got: {:?}",
        func.return_type
    );
}

// ─── step-49: GcodeDialect marker trait ──────────────────────────────────────

/// `GcodeDialect` is the marker trait for G-code dialect selectors (PRD §7.2).
/// Concrete dialects (`MarlinDialect`, `KlipperDialect`) refine it. The trait
/// exists only to give the `gcode_import` dispatcher's `dialect : GcodeDialect`
/// param a single nominal type so the SIR-α nominal type-tag dispatches on the
/// concrete dialect variant.
///
/// Empty by design — the semantic behavior lives in the consumer ο
/// (`gcode_import` parser), not as authoring-time params.
///
/// Test pins three invariants: (a) the trait is found, (b) it has zero
/// required members + zero defaults (marker trait), (c) it has no
/// refinements (top-level marker, no parent trait).
/// Mirrors `shaper_trait_exists_with_no_params` (step-29).
#[test]
fn gcode_dialect_trait_exists_with_no_params() {
    let trait_def = find_trait("GcodeDialect");

    assert!(
        trait_def.required_members.is_empty(),
        "GcodeDialect should declare zero required members (marker trait); \
         got: {:?}",
        trait_def
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.defaults.is_empty(),
        "GcodeDialect should declare zero defaults (marker trait); got: {:?}",
        trait_def
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
    assert!(
        trait_def.refinements.is_empty(),
        "GcodeDialect should declare zero refinements (top-level marker, no \
         parent trait); got: {:?}",
        trait_def.refinements
    );
}

// ─── step-51: MarlinDialect refines GcodeDialect ──────────────────────────────

/// `MarlinDialect` is the zero-DOF marker for the Marlin G-code dialect
/// (PRD §7.1/§7.2). It refines `GcodeDialect` and carries no authoring-time
/// params: the semantic behaviour (G0/G1/G2/G3/G92/F, M-commands ignored)
/// lives in the consumer ο (`gcode_import` parser dispatch), not here.
///
/// Test pins three invariants: (a) the structure refines `GcodeDialect`
/// (via `template.trait_bounds`), (b) it has zero params (marker), (c) it
/// declares no constraints.
/// Mirrors `natural_spline_refines_boundary_condition_with_no_params` (step-11).
#[test]
fn marlin_dialect_refines_gcode_dialect_with_no_params() {
    let template = find_structure("MarlinDialect");

    assert_eq!(
        template.trait_bounds,
        vec!["GcodeDialect".to_string()],
        "MarlinDialect must refine GcodeDialect; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    assert!(
        params.is_empty(),
        "MarlinDialect should declare zero params (marker structure); \
         got: {:?}",
        params
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        template.constraints.is_empty(),
        "MarlinDialect should declare no constraints (semantic behaviour \
         is consumer-ο-enforced); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-53: KlipperDialect refines GcodeDialect ────────────────────────────

/// `KlipperDialect` is the zero-DOF marker for the Klipper G-code dialect
/// (PRD §7.1/§7.2). It refines `GcodeDialect` and carries no authoring-time
/// params: the semantic behaviour (same core G-codes as Marlin plus
/// SET_VELOCITY_LIMIT / INPUT_SHAPER directives) lives in the consumer ο
/// (`gcode_import` parser dispatch), not here.
///
/// Test pins three invariants: (a) the structure refines `GcodeDialect`
/// (via `template.trait_bounds`), (b) it has zero params (marker), (c) it
/// declares no constraints.
/// Mirrors `periodic_spline_refines_boundary_condition_with_no_params` (step-15).
#[test]
fn klipper_dialect_refines_gcode_dialect_with_no_params() {
    let template = find_structure("KlipperDialect");

    assert_eq!(
        template.trait_bounds,
        vec!["GcodeDialect".to_string()],
        "KlipperDialect must refine GcodeDialect; got trait_bounds: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    assert!(
        params.is_empty(),
        "KlipperDialect should declare zero params (marker structure); \
         got: {:?}",
        params
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        template.constraints.is_empty(),
        "KlipperDialect should declare no constraints (semantic behaviour \
         is consumer-ο-enforced); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── ζ step-1: input_shape(profile, shaper) surface + coercion shims ──────────

/// Assert a one-field trait-coercion shim
/// `pub structure def <name> { param <field> : <Trait> }` exists with the
/// `GcodeDialectInput` shape (trajectory.ri): refines no trait, declares
/// exactly one caller-supplied param whose type is `TraitObject(<Trait>)`, and
/// declares no constraints. Centralises the shim shape so `ProfileInput` /
/// `ShaperInput` stay in lock-step with the `GcodeDialectInput` /
/// `FEAMaterialInput` precedent.
fn assert_trait_input_shim(name: &str, field: &str, trait_name: &str) {
    let template = find_structure(name);

    // Pure coercion shim — refines no trait (it is NOT a Profile / Shaper).
    assert_eq!(
        template.trait_bounds,
        Vec::<String>::new(),
        "{} should refine no trait (input-coercion shim, mirrors \
         GcodeDialectInput); got trait_bounds: {:?}",
        name,
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        params.len(),
        1,
        "{} should declare exactly 1 param ({}); got: {:?}",
        name,
        field,
        names
    );

    let cell = params[0];
    assert_eq!(
        cell.id.member, field,
        "{} param should be named `{}`; got: {:?}",
        name, field, names
    );
    assert_eq!(
        cell.cell_type,
        Type::TraitObject(trait_name.to_string()),
        "{}.{} should be TraitObject(\"{}\"); got: {:?}",
        name,
        field,
        trait_name,
        cell.cell_type
    );
    assert!(
        cell.default_expr.is_none(),
        "{}.{} should have no default_expr (caller-supplied shim field); \
         got: {:?}",
        name,
        field,
        cell.default_expr
    );
    assert!(
        template.constraints.is_empty(),
        "{} should declare no constraints (pure coercion shim); got: {:?}",
        name,
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

/// `input_shape` is the impulse-/TOTS-shaper dispatcher (PRD §5.3, §11 Phase 2
/// ζ). It is declared with the `gcode_import` delegate-body pattern so the call
/// site resolves as a typed `UserFunctionCall` (`-> Profile`) while the body
/// reaches `eval_builtin` via the undeclared `input_shape_apply` name.
///
/// Signature: `pub fn input_shape(profile: Profile, shaper: Shaper) -> Profile`
///
/// `profile : Profile` / `shaper : Shaper` resolve to `Type::TraitObject(..)`
/// (the same trait-typed param resolution `evaluate_profile`'s `p : Profile`
/// uses); the return type is `Type::TraitObject("Profile")` (the shaped
/// profile). Param declaration order is part of the contract — pinned here in
/// the same way step-21 pins `evaluate_profile`'s (p, t) order.
#[test]
fn input_shape_fn_signature() {
    let module = load_stdlib_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "input_shape")
        .unwrap_or_else(|| {
            panic!(
                "input_shape not found in std/trajectory; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "input_shape should be pub");

    assert_eq!(
        func.params.len(),
        2,
        "input_shape should take exactly 2 params (profile, shaper); got: {:?}",
        func.params
    );

    // Param order is part of the contract — profile first, then shaper.
    assert_eq!(
        func.params[0],
        (
            "profile".to_string(),
            Type::TraitObject("Profile".to_string())
        ),
        "input_shape param[0] should be (\"profile\", TraitObject(\"Profile\")); \
         got: {:?}",
        func.params[0]
    );
    assert_eq!(
        func.params[1],
        (
            "shaper".to_string(),
            Type::TraitObject("Shaper".to_string())
        ),
        "input_shape param[1] should be (\"shaper\", TraitObject(\"Shaper\")); \
         got: {:?}",
        func.params[1]
    );

    assert_eq!(
        func.return_type,
        Type::TraitObject("Profile".to_string()),
        "input_shape return type should be TraitObject(\"Profile\") (the shaped \
         profile); got: {:?}",
        func.return_type
    );
}

/// `ProfileInput` is the trait-coercion shim that lets a concrete
/// `PiecewisePolynomialProfile` reach `input_shape`'s `profile : Profile` param
/// (the overload resolver uses exact type equality — a bare
/// `StructureRef("PiecewisePolynomialProfile")` does not match the `Profile`
/// trait param). Mirrors `GcodeDialectInput` / `FEAMaterialInput`.
#[test]
fn profile_input_shim_exists() {
    assert_trait_input_shim("ProfileInput", "profile", "Profile");
}

/// `ShaperInput` is the trait-coercion shim that lets a concrete shaper
/// (`ZVDShaper` / `EIShaper` / …) reach `input_shape`'s `shaper : Shaper`
/// param. Mirrors `GcodeDialectInput` / `FEAMaterialInput`.
#[test]
fn shaper_input_shim_exists() {
    assert_trait_input_shim("ShaperInput", "shaper", "Shaper");
}
