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

// ─── step-5: BucklingOptions defaults ────────────────────────────────────────

/// Each `BucklingOptions` param must carry the canonical default declared in
/// PRD §4 (with the decimal-vs-scientific encoding adjustment for `tol`):
///
///   n_modes    = 10
///   mode       = "shift_invert"
///   sigma      = 0.0
///   tol        = 0.00000001    (= 1e-8 in PRD; decimal because Reify's
///                                number grammar has no scientific notation)
///   max_iters  = 1000
///   auto_dense = true
///
/// Strict-equality discipline for real-valued defaults mirrors the
/// `cg_tolerance` precedent in `solver_elastic_tests.rs:336-346`: IEEE-754
/// round-to-nearest is deterministic on the same decimal input, so strict
/// equality catches silent regressions (e.g., `9.999e-9`) that an absolute-
/// tolerance check would let through.
#[test]
fn buckling_options_param_defaults_match_spec() {
    let template = find_structure("BucklingOptions");

    // n_modes = 10
    let n_modes_default = require_default(template, "n_modes");
    match &n_modes_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 10, "n_modes default should be 10, got: {}", v)
        }
        other => panic!(
            "n_modes default should be Literal(Value::Int(10)), got: {:?}",
            other
        ),
    }

    // mode = "shift_invert"
    let mode_default = require_default(template, "mode");
    match &mode_default.kind {
        CompiledExprKind::Literal(Value::String(s)) => assert_eq!(
            s, "shift_invert",
            "mode default should be \"shift_invert\", got: {:?}",
            s
        ),
        other => panic!(
            "mode default should be Literal(Value::String(\"shift_invert\")), got: {:?}",
            other
        ),
    }

    // sigma = 0.0 (strict equality, IEEE-754 round-to-nearest deterministic)
    let sigma_default = require_default(template, "sigma");
    match &sigma_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.0,
            "sigma default should be exactly 0.0, got: {}",
            v
        ),
        other => panic!(
            "sigma default should be Literal(Value::Real(0.0)), got: {:?}",
            other
        ),
    }

    // tol = 0.00000001 (= 1e-8 in decimal; strict-equality discipline per
    // cg_tolerance precedent at solver_elastic_tests.rs:336-346)
    let tol_default = require_default(template, "tol");
    match &tol_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.00000001,
            "tol default should be exactly 0.00000001 (= 1e-8), got: {}",
            v
        ),
        other => panic!(
            "tol default should be Literal(Value::Real(0.00000001)), got: {:?}",
            other
        ),
    }

    // max_iters = 1000
    let max_iters_default = require_default(template, "max_iters");
    match &max_iters_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 1000, "max_iters default should be 1000, got: {}", v)
        }
        other => panic!(
            "max_iters default should be Literal(Value::Int(1000)), got: {:?}",
            other
        ),
    }

    // auto_dense = true
    let auto_dense_default = require_default(template, "auto_dense");
    match &auto_dense_default.kind {
        CompiledExprKind::Literal(Value::Bool(v)) => {
            assert!(*v, "auto_dense default should be true, got: {}", v)
        }
        other => panic!(
            "auto_dense default should be Literal(Value::Bool(true)), got: {:?}",
            other
        ),
    }
}

// ─── step-7: BucklingOptions positivity constraints ──────────────────────────

/// `BucklingOptions` enforces strict-positivity invariants on three params via
/// structure-level constraint declarations:
///
///   constraint n_modes > 0
///   constraint tol > 0
///   constraint max_iters > 0
///
/// Rationale for each:
///   n_modes    — a non-positive request would target zero eigenmodes — a
///                degenerate solve. Same task-2544 explicit-contract convention
///                as `ElasticOptions.max_iter > 0`.
///   tol        — Lanczos convergence test `||r||/||b|| < tol` requires `tol`
///                strictly positive; zero or negative silently exhausts
///                `max_iters` on every solve (mirrors `cg_tolerance > 0`).
///   max_iters  — a non-positive cap lets Lanczos exit before doing any work
///                (mirrors `ElasticOptions.max_iter > 0`).
///
/// Explicitly NOT constrained: `sigma` (any real shift is physically valid),
/// `auto_dense` (Bool — trivially constrained), `mode` (string allowlist
/// validation deferred to the trampoline per PRD §4; Reify constraint clauses
/// cannot express string-set membership).
///
/// Encoding these as first-class `constraint` declarations matches the
/// project convention in task 2544: "the contract in production code is
/// made explicit rather than relying on test coverage."
///
/// The assertion shape mirrors
/// `solver_elastic_tests.rs::elastic_options_constrains_positivity_invariants`.
#[test]
fn buckling_options_constrains_positivity_invariants() {
    let template = find_structure("BucklingOptions");

    assert!(
        template.constraints.len() >= 3,
        "BucklingOptions should declare at least 3 constraints \
         (n_modes > 0, tol > 0, max_iters > 0), got {} constraints",
        template.constraints.len()
    );

    for required in &["n_modes", "tol", "max_iters"] {
        let matched = template.constraints.iter().any(|c| {
            // Check the constraint expression is a `>` BinOp with a ValueRef
            // to the required member on the left side and the literal `0` on
            // the right side. Accept either `Int(0)` or `Real(0.0)` for the
            // RHS literal (mirrors the solver_elastic_tests.rs:567-579
            // future-proofing rationale).
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt || !collect_value_ref_members(left).contains(required) {
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
            "BucklingOptions should declare `constraint {} > 0`; got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-9: Mode param shape ────────────────────────────────────────────────

/// `Mode` is a single buckling eigenpair. It must declare exactly the two
/// PRD §4 params with the canonical types:
///
///   - `eigenvalue : Real`                                       (load multiplier)
///   - `mode_shape : Field<Point3<Length>, Vector3<Length>>`     (displacement field)
///
/// The PRD's "Real placeholder for mode_shape per #3117 workaround" footnote
/// is stale — task #3117 landed and the `Field<D, C>` resolver arm at
/// `type_resolution.rs:1313` accepts the precise type in `param` positions,
/// confirmed by `ElasticResult.displacement` / `ElasticResult.stress` /
/// `ElasticResult.frame` already using their proper Field types. See plan.json
/// design-decision-1 for the full rationale.
///
/// Mode is a solver-populated output container — no defaults are meaningful.
#[test]
fn mode_struct_has_eigenvalue_and_mode_shape_params() {
    let template = find_structure("Mode");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        2,
        "Mode should have exactly 2 param cells (eigenvalue, mode_shape), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("eigenvalue", Type::Real),
        (
            "mode_shape",
            Type::Field {
                domain: Box::new(Type::Point {
                    n: 3,
                    quantity: Box::new(Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    }),
                }),
                codomain: Box::new(Type::Vector {
                    n: 3,
                    quantity: Box::new(Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    }),
                }),
            },
        ),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "Mode missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "Mode.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}
