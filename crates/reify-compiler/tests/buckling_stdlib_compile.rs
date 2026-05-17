//! Tests for `crates/reify-compiler/stdlib/solver_buckling.ri` —
//! `std.solver.buckling` module: `BucklingOptions`, `Mode`, `BucklingResult`,
//! and `MultiCaseBucklingResult` structure definitions for the v0.5
//! linear-buckling eigensolver kernel surface.
//!
//! Observable signal for PRD §13 task α
//! (docs/prds/v0_5/buckling-eigensolver.md). Per the PRD, this file parses
//! the structure_defs and confirms type resolution matches the expected
//! shape.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `solver_elastic_tests.rs` / `multi_load_case_stdlib_tests.rs`),
//! that the four structures are correctly represented in the compiled
//! module, and that the positivity / non-negativity constraints on
//! `BucklingOptions.{n_modes, tol, max_iters}` and `BucklingResult.iterations`
//! are declared at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `solver_elastic_tests.rs`.

use reify_compiler::*;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/solver/buckling` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/buckling")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/solver/buckling module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/solver/buckling` module.
///
/// `BucklingOptions`, `Mode`, `BucklingResult`, and `MultiCaseBucklingResult`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/solver/buckling, got templates: {:?}",
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
#[allow(dead_code)]
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
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

/// Recursively collect ValueRef member names from a compiled expression tree.
/// Mirrors `collect_value_ref_members` in `solver_elastic_tests.rs:496-507`.
#[allow(dead_code)]
fn collect_value_ref_members(expr: &CompiledExpr) -> Vec<&str> {
    match &expr.kind {
        CompiledExprKind::ValueRef(cell_id) => vec![cell_id.member.as_str()],
        CompiledExprKind::BinOp { left, right, .. } => {
            let mut refs = collect_value_ref_members(left);
            refs.extend(collect_value_ref_members(right));
            refs
        }
        CompiledExprKind::UnOp { operand, .. } => collect_value_ref_members(operand),
        _ => vec![],
    }
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/solver/buckling module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_solver_buckling_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in solver_buckling.ri: {:?}",
        errors
    );
}

// ─── step-3: BucklingOptions param shape ─────────────────────────────────────

/// `BucklingOptions` must declare exactly the six params from PRD §4 with the
/// canonical types:
///
///   - `n_modes    : Int`     (eigenpair count to compute)
///   - `mode       : String`  (algorithm selector; allowlist validated at
///                             trampoline per PRD §4)
///   - `sigma      : Real`    (eigenvalue shift origin)
///   - `tol        : Real`    (Lanczos convergence tolerance)
///   - `max_iters  : Int`     (hard cap on Lanczos iterations)
///   - `auto_dense : Bool`    (fall back to dense GEVD when DOF ≤ ~200)
///
/// PRD's `Integer` maps to Reify's `Int` builtin (same encoding as
/// `ElasticOptions.max_iter`).
#[test]
fn buckling_options_struct_has_correct_param_shape() {
    let template = find_structure("BucklingOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        6,
        "BucklingOptions should have exactly 6 param cells \
         (n_modes, mode, sigma, tol, max_iters, auto_dense), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("n_modes", Type::Int),
        ("mode", Type::String),
        ("sigma", Type::Real),
        ("tol", Type::Real),
        ("max_iters", Type::Int),
        ("auto_dense", Type::Bool),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "BucklingOptions missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "BucklingOptions.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}
