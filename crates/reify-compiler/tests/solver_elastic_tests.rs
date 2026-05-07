//! Tests for stdlib/solver_elastic.ri — FEA solver-options (`ElasticOptions`),
//! solver-result container (`ElasticResult`), and the supporting `ElementOrder`
//! enum.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `materials_fea_tests.rs`), that the enum and structures are
//! correctly represented in the compiled module, and that the positivity
//! constraints on `ElasticOptions.max_iter` and `ElasticOptions.cg_tolerance`
//! are declared at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `materials_fea_tests.rs`.

use reify_compiler::*;
use reify_types::*;

/// Look up a structure template by name within the `std/solver/elastic` module.
///
/// `ElasticOptions` and `ElasticResult` are top-level structures, so we go
/// through `module.templates` and filter on `EntityKind::Structure` to keep
/// the assertion stable against future non-structure additions to the module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/solver/elastic, got templates: {:?}",
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

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/solver/elastic` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/elastic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/solver/elastic module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/solver/elastic module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_solver_elastic_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in solver_elastic.ri: {:?}",
        errors
    );
}

// ─── step-3: ElementOrder enum ───────────────────────────────────────────────

/// `ElementOrder` is the enum selecting between first-order (P1) and
/// second-order (P2) tetrahedral elements for the FEA mesh. The variant order
/// `[P1, P2]` is canonical: P1 is the default (fast, single-precision-stable
/// for most loads) and P2 is the override (accurate near stress
/// concentrations). Pinning the order makes any future re-ordering a
/// deliberate decision rather than a silent ABI change.
#[test]
fn element_order_enum_has_p1_and_p2_variants_in_canonical_order() {
    let module = load_stdlib_module();

    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "ElementOrder")
        .unwrap_or_else(|| {
            panic!(
                "expected `enum ElementOrder` in std/solver/elastic, got enum_defs: {:?}",
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        enum_def.variants,
        vec!["P1".to_string(), "P2".to_string()],
        "ElementOrder variants should be [P1, P2] in canonical order, got: {:?}",
        enum_def.variants
    );
}

// ─── step-5: ElasticOptions param shape ──────────────────────────────────────

/// `ElasticOptions` is the FEA solver-input knob structure. It must declare
/// exactly five params with the canonical names and types:
///
///   - `element_order : ElementOrder`             (selects P1 / P2 elements)
///   - `mesh_size     : Option<Length>`           (none = solver derives from tolerance)
///   - `max_iter      : Int`                      (CG iteration cap)
///   - `cg_tolerance  : Real`                     (CG convergence threshold)
///   - `threads       : Option<Int>`              (none = solver picks)
///
/// `mesh_size` and `threads` are encoded as `Option<T> = none` rather than
/// PRD-style sentinels (e.g., `auto`, `num_cpus::get()`) because the language
/// has no `auto` keyword and no `num_cpus::get()` builtin; the right
/// options-side shape is "user did not specify, solver decides" — matching
/// the design decision recorded in plan.json.
#[test]
fn elastic_options_struct_has_correct_param_shape() {
    let template = find_structure("ElasticOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        5,
        "ElasticOptions should have exactly 5 param cells, got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("element_order", Type::Enum("ElementOrder".to_string())),
        (
            "mesh_size",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        ),
        ("max_iter", Type::Int),
        ("cg_tolerance", Type::Real),
        ("threads", Type::Option(Box::new(Type::Int))),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ElasticOptions missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ElasticOptions.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── step-7: ElasticOptions defaults ─────────────────────────────────────────

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

/// Each `ElasticOptions` param must carry the canonical default declared in
/// the PRD (with the encoding adjustments documented in the file header):
///
///   element_order = ElementOrder.P1
///   mesh_size     = none
///   max_iter      = 1000
///   cg_tolerance  = 0.000001
///   threads       = none
///
/// The defaults pin the standard solver setup so a bare `ElasticOptions()`
/// instantiation compiles. `0.000001` is asserted with a 1e-9 tolerance to
/// accommodate float round-off.
#[test]
fn elastic_options_param_defaults_match_spec() {
    let template = find_structure("ElasticOptions");

    // element_order = ElementOrder.P1
    let element_order_default = require_default(template, "element_order");
    match &element_order_default.kind {
        CompiledExprKind::Literal(Value::Enum { type_name, variant }) => {
            assert_eq!(
                type_name, "ElementOrder",
                "element_order default should be ElementOrder.P1, got type_name {:?}",
                type_name
            );
            assert_eq!(
                variant, "P1",
                "element_order default should be ElementOrder.P1, got variant {:?}",
                variant
            );
        }
        other => panic!(
            "element_order default should be Literal(Value::Enum {{ ElementOrder, P1 }}), got: {:?}",
            other
        ),
    }

    // mesh_size = none, with result_type Option<Length>
    let mesh_size_default = require_default(template, "mesh_size");
    assert!(
        matches!(&mesh_size_default.kind, CompiledExprKind::OptionNone),
        "mesh_size default should be OptionNone, got: {:?}",
        mesh_size_default.kind
    );
    assert_eq!(
        mesh_size_default.result_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        })),
        "mesh_size default's result_type should be Option<Length>, got: {:?}",
        mesh_size_default.result_type
    );

    // max_iter = 1000
    let max_iter_default = require_default(template, "max_iter");
    match &max_iter_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => assert_eq!(
            *v, 1000,
            "max_iter default should be 1000, got: {}",
            v
        ),
        other => panic!(
            "max_iter default should be Literal(Value::Int(1000)), got: {:?}",
            other
        ),
    }

    // cg_tolerance = 0.000001 — strict equality. The Reify parser converts
    // the decimal literal to f64 via the same round-to-nearest-even rule as
    // Rust's `0.000001` literal, so the round-trip is bit-exact. The earlier
    // 1e-9 absolute tolerance was lax enough to silently accept e.g.
    // `9.999e-7` (which would still parse cleanly under a future float-format
    // change); strict equality catches that regression while remaining
    // bit-stable across platforms because IEEE-754 round-to-nearest is
    // deterministic on the same decimal input.
    let cg_tolerance_default = require_default(template, "cg_tolerance");
    match &cg_tolerance_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.000001,
            "cg_tolerance default should be exactly 0.000001, got: {}",
            v
        ),
        other => panic!(
            "cg_tolerance default should be Literal(Value::Real(0.000001)), got: {:?}",
            other
        ),
    }

    // threads = none, with result_type Option<Int>
    let threads_default = require_default(template, "threads");
    assert!(
        matches!(&threads_default.kind, CompiledExprKind::OptionNone),
        "threads default should be OptionNone, got: {:?}",
        threads_default.kind
    );
    assert_eq!(
        threads_default.result_type,
        Type::Option(Box::new(Type::Int)),
        "threads default's result_type should be Option<Int>, got: {:?}",
        threads_default.result_type
    );
}

// ─── step-9: ElasticOptions positivity constraints ───────────────────────────

/// Recursively collect ValueRef member names from a compiled expression tree.
/// Mirrors `collect_value_ref_members` in `stdlib_loader_tests.rs:14-23`.
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

/// `ElasticOptions` enforces the runtime invariant that `max_iter` and
/// `cg_tolerance` are strictly positive via two structure-level constraint
/// declarations:
///
///   constraint max_iter > 0
///   constraint cg_tolerance > 0
///
/// A negative `max_iter` or non-positive `cg_tolerance` is nonsensical and
/// would silently corrupt the solver. Encoding the invariants as first-class
/// `constraint` declarations (rather than relying on documentation + tests)
/// matches the project convention in task 2544: "the contract in production
/// code is made explicit rather than relying on test coverage."
///
/// The assertion shape mirrors the constraint-injection check in
/// `materials_fea_tests.rs::elastic_material_trait_constrains_poisson_ratio_to_half_open_unit`:
/// the test inspects each `template.constraints` entry, walks the BinOp
/// expression with `collect_value_ref_members`, and asserts that the entry's
/// op is `>` and references the expected member name.
#[test]
fn elastic_options_constrains_max_iter_and_cg_tolerance_positive() {
    let template = find_structure("ElasticOptions");

    assert!(
        template.constraints.len() >= 2,
        "ElasticOptions should declare at least 2 constraints (max_iter > 0 \
         and cg_tolerance > 0), got {} constraints",
        template.constraints.len()
    );

    for required in &["max_iter", "cg_tolerance"] {
        let matched = template.constraints.iter().any(|c| {
            // Check the constraint expression is a `>` BinOp with a ValueRef
            // to the required member on the left side and the literal `0` on
            // the right side. Pinning the RHS literal closes a regression
            // window where rewriting `max_iter > 0` to `max_iter > -100` (or
            // `cg_tolerance > -1.0`) would silently weaken the invariant
            // while still passing a name-and-op-only check. We accept either
            // `Int(0)` or `Real(0.0)` for the RHS literal because the Reify
            // parser stores the `0` token as `Int(0)` regardless of the LHS
            // type and a future numeric-promotion change could legitimately
            // emit `Real(0.0)` here.
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt
                        || !collect_value_ref_members(left).contains(required)
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
            "ElasticOptions should declare `constraint {} > 0`; got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── task-3044 step-3: ElasticOptions cg_tolerance upper bound ───────────────

/// `ElasticOptions` must cap `cg_tolerance` strictly below 1:
///
///   constraint cg_tolerance < 1
///
/// `cg_tolerance` is a relative residual norm — the CG solver declares
/// convergence when `||r||/||b|| < cg_tolerance`. If `cg_tolerance >= 1`
/// the test accepts the very first residual (the initial un-preconditioned
/// residual trivially satisfies `||r||/||b|| < 1` for any non-trivial rhs),
/// meaning CG would declare convergence without doing any work. This is the
/// symmetric, meaningless mirror of the `> 0` lower-bound case: just as
/// `cg_tolerance <= 0` makes convergence impossible, `cg_tolerance >= 1`
/// makes convergence trivial.
///
/// The cap is `< 1` (not `< 0.5`) so callers can still pick loose first-pass
/// tolerances like `0.1` or `0.5` (as noted in the field comment at lines
/// 70-73 of solver_elastic.ri). Only the meaningless "any residual passes"
/// case is excluded. Encoding this as a structure-level constraint follows the
/// task-2544 convention: "the contract in production code is made explicit
/// rather than relying solely on test coverage."
///
/// The assertion shape mirrors `elastic_options_constrains_max_iter_and_cg_tolerance_positive`,
/// substituting `BinOp::Lt` (`<`) for `BinOp::Gt` (`>`) and `1` for `0`.
/// RHS literals `Int(1)` and `Real(1.0)` are both accepted for stability
/// across future numeric-promotion changes.
#[test]
fn elastic_options_caps_cg_tolerance_below_one() {
    let template = find_structure("ElasticOptions");

    let matched = template.constraints.iter().any(|c| {
        // The constraint must be a `<` BinOp with a ValueRef to `cg_tolerance`
        // on the left and the literal `1` on the right. Pinning the RHS
        // prevents a silent weakening where the bound is changed to e.g. `< 2`
        // but the name + op check still passes.
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Lt
                    || !collect_value_ref_members(left).contains(&"cg_tolerance")
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
        matched,
        "ElasticOptions should declare `constraint cg_tolerance < 1`; got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task-3044 step-1: ElasticResult non-negativity constraints ──────────────

/// `ElasticResult` must declare non-negativity constraints on `iterations` and
/// `max_von_mises`:
///
///   constraint iterations >= 0
///   constraint max_von_mises >= 0
///
/// `iterations` is a CG iteration count — a negative count is impossible.
/// `max_von_mises` is a stress magnitude (von-Mises equivalent stress is the
/// Frobenius norm of the deviatoric stress tensor) — negative is meaningless.
/// Encoding these as structure-level constraints follows the task-2544
/// convention: "the contract in production code is made explicit rather than
/// relying solely on test coverage."
///
/// The assertion shape mirrors `elastic_options_constrains_max_iter_and_cg_tolerance_positive`
/// (above), substituting `BinOp::Ge` (`>=`) for `BinOp::Gt` (`>`).
/// RHS literals `Int(0)` and `Real(0.0)` are both accepted for stability
/// across future numeric-promotion changes.
#[test]
fn elastic_result_constrains_iterations_and_max_von_mises_nonneg() {
    let template = find_structure("ElasticResult");

    assert!(
        template.constraints.len() >= 2,
        "ElasticResult should declare at least 2 constraints \
         (iterations >= 0 and max_von_mises >= 0), got {} constraints",
        template.constraints.len()
    );

    for required in &["iterations", "max_von_mises"] {
        let matched = template.constraints.iter().any(|c| {
            // The constraint must be a `>=` BinOp with a ValueRef to the
            // required member on the left and the literal `0` on the right.
            // Pinning the RHS prevents a silent weakening where the bound is
            // changed to a negative value but the name + op check still passes.
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Ge
                        || !collect_value_ref_members(left).contains(required)
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
            "ElasticResult should declare `constraint {} >= 0`; got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-11: ElasticResult param shape ──────────────────────────────────────

/// `ElasticResult` is the FEA solver-output container. It must declare
/// exactly five params with the canonical names and types:
///
///   - `displacement  : Real`     (Real placeholder for Field<Point3<Length>, Vector3<Length>>)
///   - `stress        : Real`     (Real placeholder for Field<Point3<Length>, Tensor<2,3,Pressure>>)
///   - `max_von_mises : Pressure`
///   - `converged     : Bool`
///   - `iterations    : Int`
///
/// `displacement` and `stress` use `Real` placeholders pending Field<X,Y>
/// support in `param` positions (see plan.json design decision). The runtime
/// FEA solver (PRD task #16) populates these as Field-typed Maps regardless
/// of the static `param` annotation. This test pins the placeholder type so
/// a future Field<X,Y> migration becomes a deliberate update rather than a
/// silent type drift.
#[test]
fn elastic_result_struct_has_correct_param_shape() {
    let template = find_structure("ElasticResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        5,
        "ElasticResult should have exactly 5 param cells, got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("displacement", Type::Real),
        ("stress", Type::Real),
        (
            "max_von_mises",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("converged", Type::Bool),
        ("iterations", Type::Int),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ElasticResult missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ElasticResult.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}
