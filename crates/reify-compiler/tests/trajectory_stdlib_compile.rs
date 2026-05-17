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
