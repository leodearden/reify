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
use reify_types::*;

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

/// Recursively collect ValueRef member names from a compiled expression tree.
/// Walks `BinOp`, `UnOp`, and `MethodCall` receivers so a chain like
/// `waypoints.count > 0` surfaces `waypoints` from the LHS.
fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<&str> {
    match &expr.kind {
        CompiledExprKind::ValueRef(cell_id) => vec![cell_id.member.as_str()],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut refs = collect_value_ref_members(left);
            refs.extend(collect_value_ref_members(right));
            refs
        }
        CompiledExprKind::UnOp { operand, .. } => collect_value_ref_members(operand),
        CompiledExprKind::MethodCall { object, .. } => collect_value_ref_members(object),
        _ => vec![],
    }
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
///                        choice when waypoints carry only positions
///                        (vels / accels are `none`).
///   - `QuinticSpline`  — degree-5 polynomial per segment; selected when
///                        waypoints carry explicit `vels` AND `accels`.
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
