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

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;

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
        Type::List(Box::new(Type::Real)),
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
/// `List<JointValue>` compiles to `Type::List(Box::new(Type::Real))`.
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
        ("values", Type::List(Box::new(Type::Real))),
        (
            "vels",
            Type::Option(Box::new(Type::List(Box::new(Type::Real)))),
        ),
        (
            "accels",
            Type::Option(Box::new(Type::List(Box::new(Type::Real)))),
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
        ("start_velocity", Type::List(Box::new(Type::Real))),
        ("end_velocity", Type::List(Box::new(Type::Real))),
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
        ("mechanism", Type::Real),
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
