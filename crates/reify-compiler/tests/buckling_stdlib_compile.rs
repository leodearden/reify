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

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;
use reify_test_support::collect_value_ref_members;

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
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Look up the named param cell on `template` and return its `default_expr`.
/// Panics with a clear message if the cell or its default is missing.
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

/// `BucklingOptions` must declare exactly the seven params from PRD §4 (plus
/// the task-4129 `element_order` extension) with the canonical types:
///
///   - `n_modes       : Int`          (eigenpair count to compute)
///   - `mode          : String`       (algorithm selector; allowlist validated at
///     trampoline per PRD §4)
///   - `sigma         : Real`         (eigenvalue shift origin)
///   - `tol           : Real`         (Lanczos convergence tolerance)
///   - `max_iters     : Int`          (hard cap on Lanczos iterations)
///   - `auto_dense    : Bool`         (fall back to dense GEVD when DOF ≤ ~200)
///   - `element_order : ElementOrder` (P1/P2 finite-element order; task 4129)
///
/// PRD's `Integer` maps to Reify's `Int` builtin (same encoding as
/// `ElasticOptions.max_iter`). `Type::Enum("ElementOrder")` is the EXACT
/// representation the resolver produces for the shared stdlib enum — confirmed
/// by the existing `ElasticOptions.element_order` assertion at
/// `solver_elastic_tests.rs:204` and `ModalOptions.element_order` at
/// `modal_options_validation_tests.rs:627`.
#[test]
fn buckling_options_struct_has_correct_param_shape() {
    let template = find_structure("BucklingOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        7,
        "BucklingOptions should have exactly 7 param cells \
         (n_modes, mode, sigma, tol, max_iters, auto_dense, element_order), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("n_modes", Type::Int),
        ("mode", Type::String),
        ("sigma", Type::Real),
        ("tol", Type::Real),
        ("max_iters", Type::Int),
        ("auto_dense", Type::Bool),
        ("element_order", Type::Enum("ElementOrder".to_string())),
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
        CompiledExprKind::Literal(Value::Real(v)) => {
            assert_eq!(*v, 0.0, "sigma default should be exactly 0.0, got: {}", v)
        }
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

    // element_order = ElementOrder.P1 (enum default; task 4129).
    // Mirrors the assertion shape at modal_options_validation_tests.rs:1692-1707
    // and solver_elastic_tests.rs:281-296.
    let element_order_default = require_default(template, "element_order");
    match &element_order_default.kind {
        CompiledExprKind::Literal(Value::Enum { type_name, variant }) => {
            assert_eq!(
                type_name, "ElementOrder",
                "element_order default type_name should be \"ElementOrder\", got: {:?}",
                type_name
            );
            assert_eq!(
                variant, "P1",
                "element_order default variant should be \"P1\", got: {:?}",
                variant
            );
        }
        other => panic!(
            "BucklingOptions.element_order default should be \
             Literal(Value::Enum {{ type_name: \"ElementOrder\", variant: \"P1\" }}), got: {:?}",
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

    // Tight count: exactly 3 constraints. A weaker `>= 3` would let a bogus
    // 4th constraint (e.g., an accidental `constraint sigma >= 0` that
    // would silently exclude negative-side-of-spectrum shifts) pass. The
    // .ri file's "explicitly NOT constrained" note (sigma, auto_dense,
    // mode) is enforced here as a regression gate.
    assert_eq!(
        template.constraints.len(),
        3,
        "BucklingOptions should declare exactly 3 constraints \
         (n_modes > 0, tol > 0, max_iters > 0); sigma / auto_dense / mode \
         are explicitly NOT constrained per the .ri file. Got {} \
         constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
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
                    if *op != BinOp::Gt || !collect_value_ref_members(left).iter().any(|m| m.as_str() == *required) {
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
                domain: Box::new(Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
                codomain: Box::new(Type::vec3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
            },
        ),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!("Mode missing required param '{}'; got: {:?}", member, names)
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "Mode.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

/// `Mode` is a solver-populated output container — every field is determined
/// by the buckling solve, so caller-supplied defaults are meaningless and no
/// per-field scalar invariant is expressible (`eigenvalue` is any real, the
/// FEA-normalization convention on `mode_shape` is collection-shaped, not
/// scalar). This test pins both invariants as a regression gate so that an
/// accidentally-added default or constraint surfaces immediately. Mirrors
/// the discipline applied to `MultiCaseBucklingResult` further down (no
/// defaults, no constraints).
#[test]
fn mode_struct_has_no_constraints_or_defaults() {
    let template = find_structure("Mode");

    // No defaults: every Mode instance must be solver-populated.
    for cell in param_cells(template) {
        assert!(
            cell.default_expr.is_none(),
            "Mode.{} should have no default_expr (solver-only-produced), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // No constraints: eigenvalue is unrestricted, mode_shape's normalization
    // invariant is a collection invariant and is producer-enforced.
    assert!(
        template.constraints.is_empty(),
        "Mode should declare no constraints (solver-only-produced output \
         container, no scalar predicate is expressible per-field); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-11: BucklingResult param shape ─────────────────────────────────────

/// `BucklingResult` is the single-load-case buckling-solver output container.
/// It must declare exactly the four PRD §4 params with the canonical types:
///
///   - `modes      : List<Mode>`        (computed eigenpairs)
///   - `converged  : Bool`              (all n_modes met the tolerance)
///   - `iterations : Int`               (total Lanczos iteration count)
///   - `pre_stress : ElasticResult`     (linear-static solve feeding K_g)
///
/// Note `Type::StructureRef` is the variant for user-defined structure types
/// resolved from a name (see precedent at
/// `multi_load_case_stdlib_tests.rs:142`,
/// `LoadCase.options : Option<ElasticOptions>`).
#[test]
fn buckling_result_struct_has_correct_param_shape() {
    let template = find_structure("BucklingResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "BucklingResult should have exactly 4 param cells \
         (modes, converged, iterations, pre_stress), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "modes",
            Type::List(Box::new(Type::StructureRef("Mode".to_string()))),
        ),
        ("converged", Type::Bool),
        ("iterations", Type::Int),
        (
            "pre_stress",
            Type::StructureRef("ElasticResult".to_string()),
        ),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "BucklingResult missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "BucklingResult.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── step-13: BucklingResult non-negativity constraint ───────────────────────

/// `BucklingResult` must declare a non-negativity constraint on `iterations`:
///
///   constraint iterations >= 0
///
/// `iterations` is a Lanczos iteration count — a negative count is
/// impossible. Encoding this as a structure-level constraint follows the
/// task-2544 convention: "the contract in production code is made explicit
/// rather than relying solely on test coverage." Mirrors
/// `ElasticResult.iterations >= 0` (`solver_elastic.ri:313-321`).
///
/// The assertion shape mirrors
/// `solver_elastic_tests.rs::elastic_result_constrains_iterations_and_max_von_mises_nonneg`,
/// substituting `BinOp::Ge` (`>=`) for `BinOp::Gt` (`>`).
#[test]
fn buckling_result_constrains_iterations_nonneg() {
    let template = find_structure("BucklingResult");

    // Tight count: exactly 1 constraint. A weaker `>= 1` would let a bogus
    // extra constraint pass — e.g., an accidental `constraint converged ==
    // true` that would forbid representing a non-converged solve in a
    // BucklingResult. The .ri file's "explicitly NOT constrained" note
    // (converged, pre_stress, modes) is enforced here as a regression gate.
    assert_eq!(
        template.constraints.len(),
        1,
        "BucklingResult should declare exactly 1 constraint \
         (iterations >= 0); converged / pre_stress / modes are explicitly \
         NOT constrained per the .ri file. Got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| {
        // The constraint must be a `>=` BinOp with a ValueRef to `iterations`
        // on the left and the literal `0` on the right. Pinning the RHS
        // prevents a silent weakening where the bound is changed to a
        // negative value but the name + op check still passes.
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Ge || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "iterations") {
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
        "BucklingResult should declare `constraint iterations >= 0`; got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-15: MultiCaseBucklingResult param shape ────────────────────────────

/// `MultiCaseBucklingResult` is the multi-load-case buckling-solver output
/// container — the parallel sibling of `MultiCaseResult` (PRD §7). It must
/// declare exactly one param with the canonical name and type:
///
///   - `cases : Map<String, BucklingResult>`   (keyed by LoadCase.name)
///
/// And exactly zero constraints: the only meaningful invariant ("cases must be
/// non-empty") is a collection invariant expressible only at the producer
/// (`solve_buckling_load_cases`, PRD §13 task η); per-instance scalar
/// predicates can't enforce it. Map key-uniqueness is structurally guaranteed
/// by `BTreeMap`. Mirrors the `MultiCaseResult` discipline at
/// `fea_multi_case.ri:243-251`.
#[test]
fn multi_case_buckling_result_struct_has_cases_field() {
    let template = find_structure("MultiCaseBucklingResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        1,
        "MultiCaseBucklingResult should have exactly 1 param cell (cases), got: {:?}",
        names
    );

    let cases_cell = params
        .iter()
        .find(|vc| vc.id.member == "cases")
        .unwrap_or_else(|| {
            panic!(
                "MultiCaseBucklingResult missing required param 'cases'; got: {:?}",
                names
            )
        });

    assert_eq!(
        cases_cell.cell_type,
        Type::Map(
            Box::new(Type::String),
            Box::new(Type::StructureRef("BucklingResult".to_string())),
        ),
        "MultiCaseBucklingResult.cases should be Map<String, BucklingResult>, got {:?}",
        cases_cell.cell_type
    );

    // No default — every instance must be produced by solve_buckling_load_cases
    assert!(
        cases_cell.default_expr.is_none(),
        "MultiCaseBucklingResult.cases should have no default_expr \
         (solver-only-produced), but got: {:?}",
        cases_cell.default_expr
    );

    // Producer-enforced collection invariants: no constraints expected.
    assert!(
        template.constraints.is_empty(),
        "MultiCaseBucklingResult should declare no constraints (collection \
         invariants are producer-enforced, mirroring MultiCaseResult); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}
