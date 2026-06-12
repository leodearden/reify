//! Tests for stdlib/solver_elastic.ri — FEA solver-options (`ElasticOptions`),
//! solver-result container (`ElasticResult`), and the supporting `ElementOrder`
//! enum.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `materials_fea_tests.rs`), that the enum and structures are
//! correctly represented in the compiled module, and that the positivity and
//! upper-bound constraints on `ElasticOptions.max_iter` and
//! `ElasticOptions.cg_tolerance`, and the non-negativity constraints on
//! `ElasticResult.iterations` and `ElasticResult.max_von_mises`, are declared
//! at the structure-def level.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `materials_fea_tests.rs`.

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;
use reify_test_support::collect_value_ref_members;

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

// ─── step-1: ShellForce enum ─────────────────────────────────────────────────

/// `ShellForce` is the enum controlling whether the FEA solver uses shell
/// formulation. The variant order `[Off, Auto, On]` is canonical: it reflects
/// the natural "never / default / always" mental model. Pinning the order
/// makes any future re-ordering a deliberate decision rather than a silent
/// ABI/tag-encoding change — same discipline as `ElementOrder`'s `[P1, P2]`
/// pin. PRD reference: docs/prds/v0_4/structural-analysis-shells.md (T17).
#[test]
fn shell_force_enum_has_off_auto_on_variants_in_canonical_order() {
    let module = load_stdlib_module();

    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "ShellForce")
        .unwrap_or_else(|| {
            panic!(
                "expected `enum ShellForce` in std/solver/elastic, got enum_defs: {:?}",
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        enum_def.variants,
        vec!["Off".to_string(), "Auto".to_string(), "On".to_string()],
        "ShellForce variants should be [Off, Auto, On] in canonical order, got: {:?}",
        enum_def.variants
    );
}

// ─── step-5: ElasticOptions param shape ──────────────────────────────────────

/// `ElasticOptions` is the FEA solver-input knob structure. It must declare
/// exactly twelve params with the canonical names and types:
///
///   - `element_order          : ElementOrder`   (selects P1 / P2 elements)
///   - `mesh_size              : Option<Length>`  (none = solver derives from tolerance)
///   - `max_iter               : Int`             (CG iteration cap)
///   - `cg_tolerance           : Real`            (CG convergence threshold)
///   - `threads                : Option<Int>`     (none = solver picks)
///   - `shell_threshold        : Real`            (thickness/extent ratio for auto-shell
///     classification; PRD T17 line 63)
///   - `shell_voxel_size       : Option<Length>`  (voxel resolution for medial extraction;
///     none = solver derives thickness/3)
///   - `shell_branch_prune_ratio : Real`          (medial-axis spurious-branch pruning
///     threshold; empirical placeholder)
///   - `shell_force            : ShellForce`      (off/auto/on tri-state forcing)
///   - `force_tet              : Bool`            (disable hex/wedge promotion entirely;
///     default false; PRD hex-wedge-meshing.md task #9)
///   - `require_hex_wedge      : Bool`            (upgrade tet fall-back to hard error;
///     default false; PRD hex-wedge-meshing.md task #9)
///   - `deterministic          : Bool`            (force single-threaded + fixed-order
///     reductions for bit-stable cross-machine results; default false; PRD task #18)
///
/// `mesh_size`, `threads`, and `shell_voxel_size` are encoded as `Option<T> = none`
/// rather than PRD-style sentinels (e.g., `auto`, `num_cpus::get()`) because the
/// language has no `auto` keyword and no `num_cpus::get()` builtin; the right
/// semantics are "user did not specify, solver decides" — matching the design
/// decision recorded in plan.json.
#[test]
fn elastic_options_struct_has_correct_param_shape() {
    let template = find_structure("ElasticOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        12,
        "ElasticOptions should have exactly 12 param cells, got: {:?}",
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
        ("cg_tolerance", Type::dimensionless_scalar()),
        ("threads", Type::Option(Box::new(Type::Int))),
        ("shell_threshold", Type::dimensionless_scalar()),
        (
            "shell_voxel_size",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        ),
        ("shell_branch_prune_ratio", Type::dimensionless_scalar()),
        ("shell_force", Type::Enum("ShellForce".to_string())),
        ("force_tet", Type::Bool),
        ("require_hex_wedge", Type::Bool),
        ("deterministic", Type::Bool),
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
///   element_order    = ElementOrder.P1
///   mesh_size        = none
///   max_iter         = 1000
///   cg_tolerance     = 0.000001
///   threads          = none
///   force_tet        = false   (PRD hex-wedge-meshing.md task #9, §"Two opposing
///                               escape hatches")
///   require_hex_wedge = false  (PRD hex-wedge-meshing.md task #9, §"Two opposing
///                               escape hatches")
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
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 1000, "max_iter default should be 1000, got: {}", v)
        }
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

    // force_tet = false (PRD docs/prds/v0_3/hex-wedge-meshing.md task #9,
    // §"Two opposing escape hatches"; default false preserves the
    // "promotion is automatic when detection succeeds" policy)
    let force_tet_default = require_default(template, "force_tet");
    match &force_tet_default.kind {
        CompiledExprKind::Literal(Value::Bool(v)) => {
            assert!(!v, "force_tet default should be false, got: {}", v)
        }
        other => panic!(
            "force_tet default should be Literal(Value::Bool(false)), got: {:?}",
            other
        ),
    }

    // require_hex_wedge = false (PRD docs/prds/v0_3/hex-wedge-meshing.md task #9,
    // §"Two opposing escape hatches"; default false preserves the
    // "promotion is automatic when detection succeeds" policy)
    let require_hex_wedge_default = require_default(template, "require_hex_wedge");
    match &require_hex_wedge_default.kind {
        CompiledExprKind::Literal(Value::Bool(v)) => {
            assert!(!v, "require_hex_wedge default should be false, got: {}", v)
        }
        other => panic!(
            "require_hex_wedge default should be Literal(Value::Bool(false)), got: {:?}",
            other
        ),
    }

    // deterministic = false (PRD task #18). Default false keeps the standard
    // performance path (multi-threaded for large problems); deterministic = true
    // is an opt-in that forces single-threaded + fixed-order reductions for
    // bit-stable cross-machine reproducibility.
    let deterministic_default = require_default(template, "deterministic");
    match &deterministic_default.kind {
        CompiledExprKind::Literal(Value::Bool(v)) => {
            assert!(!v, "deterministic default should be false, got: {}", v)
        }
        other => panic!(
            "deterministic default should be Literal(Value::Bool(false)), got: {:?}",
            other
        ),
    }
}

// ─── step-5 (shell params): ElasticOptions shell defaults ────────────────────

/// Each of the four new shell-related `ElasticOptions` params must carry the
/// canonical default declared in PRD T17
/// (`docs/prds/v0_4/structural-analysis-shells.md`):
///
///   shell_threshold        = 0.2          (PRD T17, §"Resolved design decisions"
///                                          → classification rule;
///                                          thickness/extent ratio)
///   shell_voxel_size       = none         (solver derives thickness/3 at runtime;
///                                          PRD T1/T2/T18)
///   shell_branch_prune_ratio = 1.0         (canonical PRD T17 default;
///                                          correctly represented as
///                                          Value::Real(1.0) since task 3184
///                                          added int-vs-real to the AST)
///   shell_force            = ShellForce.Auto  (PRD T17, §"Resolved design
///                                              decisions"; "auto-classification
///                                              by default")
///
/// `0.2` and `1.0` are asserted with strict equality — same IEEE-754
/// round-to-nearest discipline as `cg_tolerance`. (Formerly `1.01` as a
/// workaround for the int-vs-real parser bug; task 3184 fixed that.)
/// `shell_voxel_size = none` mirrors the `mesh_size = none` precedent;
/// the result_type is `Option<Length>`.
/// `shell_force = ShellForce.Auto` mirrors the `element_order = ElementOrder.P1`
/// pattern.
#[test]
fn elastic_options_shell_param_defaults_match_spec() {
    let template = find_structure("ElasticOptions");

    // shell_threshold = 0.2 (strict equality, PRD T17 line 63)
    let shell_threshold_default = require_default(template, "shell_threshold");
    match &shell_threshold_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.2,
            "shell_threshold default should be exactly 0.2, got: {}",
            v
        ),
        other => panic!(
            "shell_threshold default should be Literal(Value::Real(0.2)), got: {:?}",
            other
        ),
    }

    // shell_voxel_size = none, with result_type Option<Length>
    let shell_voxel_size_default = require_default(template, "shell_voxel_size");
    assert!(
        matches!(&shell_voxel_size_default.kind, CompiledExprKind::OptionNone),
        "shell_voxel_size default should be OptionNone, got: {:?}",
        shell_voxel_size_default.kind
    );
    assert_eq!(
        shell_voxel_size_default.result_type,
        Type::Option(Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        })),
        "shell_voxel_size default's result_type should be Option<Length>, got: {:?}",
        shell_voxel_size_default.result_type
    );

    // shell_branch_prune_ratio = 1.0 (strict Real equality). The canonical
    // PRD T17 default is 1.0. Task 3184 fixed the int-vs-real AST distinction
    // so `1.0` now correctly lowers to Value::Real(1.0) rather than Value::Int(1).
    // The stdlib was previously set to 1.01 as a workaround; that workaround
    // is now reverted.
    let shell_branch_prune_ratio_default = require_default(template, "shell_branch_prune_ratio");
    match &shell_branch_prune_ratio_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 1.0,
            "shell_branch_prune_ratio default should be exactly 1.0, got: {}",
            v
        ),
        other => panic!(
            "shell_branch_prune_ratio default should be Literal(Value::Real(1.0)), got: {:?}",
            other
        ),
    }

    // shell_force = ShellForce.Auto
    let shell_force_default = require_default(template, "shell_force");
    match &shell_force_default.kind {
        CompiledExprKind::Literal(Value::Enum { type_name, variant }) => {
            assert_eq!(
                type_name, "ShellForce",
                "shell_force default should be ShellForce.Auto, got type_name {:?}",
                type_name
            );
            assert_eq!(
                variant, "Auto",
                "shell_force default should be ShellForce.Auto, got variant {:?}",
                variant
            );
        }
        other => panic!(
            "shell_force default should be Literal(Value::Enum {{ ShellForce, Auto }}), got: {:?}",
            other
        ),
    }
}

// ─── step-9: ElasticOptions positivity constraints ───────────────────────────

/// `ElasticOptions` enforces strict-positivity invariants on four params via
/// structure-level constraint declarations:
///
///   constraint max_iter > 0
///   constraint cg_tolerance > 0
///   constraint shell_threshold > 0
///   constraint shell_branch_prune_ratio > 0
///
/// Rationale for each:
///   max_iter              — a non-positive cap lets the solver exit before
///                           doing any work.
///   cg_tolerance          — must be strictly positive for `||r||/||b|| <
///                           cg_tolerance` to terminate; zero or negative
///                           would silently exhaust `max_iter` on every solve.
///   shell_threshold       — a non-positive thickness/extent ratio would
///                           silently prevent all auto-classification (no body
///                           would ever be flagged as shell-eligible in Auto
///                           mode). PRD T17.
///   shell_branch_prune_ratio — a non-positive ratio would silently disable
///                           medial-axis pruning (no spurious branches
///                           removed). PRD T17.
///
/// Encoding these as first-class `constraint` declarations (rather than
/// relying on documentation + tests) matches the project convention in task
/// 2544: "the contract in production code is made explicit rather than
/// relying on test coverage."
///
/// The assertion shape mirrors the constraint-injection check in
/// `materials_fea_tests.rs::elastic_material_trait_constrains_poisson_ratio_to_half_open_unit`:
/// the test inspects each `template.constraints` entry, walks the BinOp
/// expression with `collect_value_ref_members`, and asserts that the entry's
/// op is `>` and references the expected member name.
#[test]
fn elastic_options_constrains_positivity_invariants() {
    let template = find_structure("ElasticOptions");

    assert!(
        template.constraints.len() >= 4,
        "ElasticOptions should declare at least 4 constraints (max_iter > 0, \
         cg_tolerance > 0, shell_threshold > 0, shell_branch_prune_ratio > 0), \
         got {} constraints",
        template.constraints.len()
    );

    for required in &[
        "max_iter",
        "cg_tolerance",
        "shell_threshold",
        "shell_branch_prune_ratio",
    ] {
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

// ─── task-3044: ElasticOptions cg_tolerance upper bound ─────────────────────

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
                if *op != BinOp::Lt || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "cg_tolerance") {
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

// ─── amend: shell_threshold upper-bound constraint ───────────────────────────

/// `ElasticOptions` must also declare an upper-bound constraint on
/// `shell_threshold`: a threshold ≥ 1 would classify every body as
/// shell-eligible (since `thickness/extent` ∈ [0, 1] for any non-degenerate
/// body — thickness is always ≤ the body's maximum extent), silently
/// defeating the purpose of Auto mode. The constraint `shell_threshold < 1`
/// prevents this silent misuse. PRD T17, §"Resolved design decisions",
/// structural-analysis-shells.md (classification rule).
#[test]
fn elastic_options_constrains_shell_threshold_below_one() {
    let template = find_structure("ElasticOptions");

    let matched = template.constraints.iter().any(|c| {
        // Check for a `<` BinOp with a ValueRef to `shell_threshold` on the
        // left and the literal `1` on the right. Accept Int(1) or Real(1.0)
        // for the RHS — the parser stores the `1` token as Int(1) and a
        // future numeric-promotion change could legitimately emit Real(1.0).
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Lt || !collect_value_ref_members(left).iter().any(|m| m.as_str() == "shell_threshold")
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
        "ElasticOptions should declare `constraint shell_threshold < 1`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task-2990: ElasticOptions force_tet / require_hex_wedge mutual-exclusion ──

/// `ElasticOptions` must declare a mutual-exclusion constraint preventing
/// `force_tet` and `require_hex_wedge` from both being `true`:
///
///   constraint !(force_tet && require_hex_wedge)
///
/// `force_tet` and `require_hex_wedge` are opposing escape hatches (PRD
/// `docs/prds/v0_3/hex-wedge-meshing.md` task #9, §"Two opposing escape
/// hatches"): `force_tet` disables hex/wedge promotion entirely; `require_hex_wedge`
/// upgrades any tet fall-back to a hard error. Setting both `true` is a
/// contradiction — the constraint flags it as a validation error at construction
/// time, following the task-2544 convention: "the contract in production code is
/// made explicit rather than relying solely on test coverage."
///
/// Both the **operator chain** (UnOp::Not over BinOp::And) and the **operand
/// identity** (each child of the And must be a bare `CompiledExprKind::ValueRef`
/// — no UnOp/BinOp wrapping) are pinned to close two regression windows:
///   - a swap to `!(force_tet || require_hex_wedge)` (wrong semantics: would
///     reject the legal "exactly one true" state) would pass on a name-and-op
///     check alone;
///   - a regression to `!(!force_tet && !require_hex_wedge)` (semantically
///     `force_tet || require_hex_wedge`, same wrong semantics one negation
///     deeper) would pass a `collect_value_ref_members` union check because
///     that helper recurses into UnOp — direct ValueRef matching rejects it.
///
/// The test counts exactly ONE such constraint (`.filter(...).count() == 1`)
/// rather than using `.any(...)` so a future duplicate addition is also caught.
#[test]
fn elastic_options_force_tet_and_require_hex_wedge_mutually_exclusive_constraint() {
    let template = find_structure("ElasticOptions");

    let count = template
        .constraints
        .iter()
        .filter(|c| {
            // Outer expression must be `!<operand>` (UnOp::Not).
            let operand = match &c.expr.kind {
                CompiledExprKind::UnOp { op, operand } if *op == UnOp::Not => operand,
                _ => return false,
            };
            // Inner expression must be `<left> && <right>` (BinOp::And).
            let (left, right) = match &operand.kind {
                CompiledExprKind::BinOp { op, left, right } if *op == BinOp::And => {
                    (left.as_ref(), right.as_ref())
                }
                _ => return false,
            };
            // Each child of the And must be a direct ValueRef — no UnOp/BinOp
            // wrapping — so that `!(!force_tet && !require_hex_wedge)` is
            // rejected. Either operand order is accepted (AST does not
            // normalize commutative &&).
            let left_name = match &left.kind {
                CompiledExprKind::ValueRef(cell_id) => cell_id.member.as_str(),
                _ => return false,
            };
            let right_name = match &right.kind {
                CompiledExprKind::ValueRef(cell_id) => cell_id.member.as_str(),
                _ => return false,
            };
            matches!(
                (left_name, right_name),
                ("force_tet", "require_hex_wedge") | ("require_hex_wedge", "force_tet")
            )
        })
        .count();

    assert_eq!(
        count,
        1,
        "ElasticOptions should declare exactly 1 `constraint !(force_tet && require_hex_wedge)`; \
         got {} matching constraints (full constraint list: {:?})",
        count,
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task-3044: ElasticResult non-negativity constraints ─────────────────────

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
                    if *op != BinOp::Ge || !collect_value_ref_members(left).iter().any(|m| m.as_str() == *required) {
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
/// exactly six params with the canonical names and types:
///
///   - `displacement  : Field<Point3<Length>, Vector3<Length>>`
///     (tightened from Real placeholder in task 3117; resolver arm at
///     `type_resolution.rs:1313` confirmed to work in `param` positions)
///   - `stress        : Field<Point3<Length>, Tensor<2,3,Pressure>>`
///     (tightened from Real placeholder in task 3117; same resolver confirmation)
///   - `frame         : Field<Point3<Length>, Matrix<3,3,Real>>`
///     (per-element local-to-global rotation; tightened in task #3641 using
///     the resolver capability confirmed by task 3117)
///   - `max_von_mises : Pressure`
///   - `converged     : Bool`
///   - `iterations    : Int`
///
/// All three Field-typed slots have been tightened from `Real` placeholders:
/// `displacement` and `stress` by task #3117, `frame` by task #3641 — both
/// using the resolver arm at `type_resolution.rs:1313`.
///
/// `frame` is the per-element local-to-global rotation:
///   - For tet results the engine sets `frame = Value::Undef` (tet stress is
///     already in the global Cartesian frame; no per-element local frame).
///   - For shell results the engine populates the per-element MITC3+ local
///     frame from the mid-surface mesher.
///
/// PRD reference: docs/prds/v0_4/structural-analysis-shells.md §
///     "Stress through thickness".
#[test]
fn elastic_result_struct_has_correct_param_shape() {
    let template = find_structure("ElasticResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        7,
        "ElasticResult should have exactly 7 param cells \
         (displacement, stress, frame, shell_channels, max_von_mises, converged, iterations), \
         got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        (
            "displacement",
            Type::Field {
                domain: Box::new(Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
                codomain: Box::new(Type::vec3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
            },
        ),
        (
            "stress",
            Type::Field {
                domain: Box::new(Type::point3(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                })),
                codomain: Box::new(Type::tensor(
                    2,
                    3,
                    Type::Scalar {
                        dimension: DimensionVector::PRESSURE,
                    },
                )),
            },
        ),
        (
            "frame",
            Type::Field {
                domain: Box::new(Type::Point {
                    n: 3,
                    quantity: Box::new(Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    }),
                }),
                codomain: Box::new(Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(Type::dimensionless_scalar()),
                }),
            },
        ),
        // task #4067 step-2: `param shell_channels : ShellStress` added here.
        // Type resolves to StructureRef("ShellStress") — the same pattern used by
        // `param material : Material` → StructureRef("Material") in material_struct_tests.rs.
        ("shell_channels", Type::StructureRef("ShellStress".to_string())),
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

// ─── T16 step-1: ShellStress struct param shape ──────────────────────────────

/// `ShellStress` is the through-thickness stress container for shell elements
/// (PRD task T16, `docs/prds/v0_4/structural-analysis-shells.md` §
/// "Stress through thickness"). It must declare exactly three params:
///
///   - `top    : Field<Point3<Length>, Tensor<2, 3, Pressure>>`  (top-surface stress)
///   - `mid    : Field<Point3<Length>, Tensor<2, 3, Pressure>>`  (mid-surface stress)
///   - `bottom : Field<Point3<Length>, Tensor<2, 3, Pressure>>`  (bottom-surface stress)
///
/// All three were tightened from `Real` placeholders to their proper
/// `Field<Point3<Length>, Tensor<2, 3, Pressure>>` type in task #3641, after
/// task #3117 confirmed the resolver arm at `type_resolution.rs:1313` handles
/// `Field<D, C>` in `param` positions. The `ShellStress` structure has no
/// defaults and no constraints — it is a data-only output container analogous
/// to `ElasticResult` (no user-configurable knobs).
///
/// For tet results the engine populates all three channels with the same field
/// (no through-thickness variation); for shell results the MITC3+ kernel
/// produces distinct top/mid/bottom integration-point stress distributions.
///
/// Rust-side sibling: `crates/reify-solver-elastic/src/shell_result.rs::ShellStress`.
/// Both definitions must stay shape-aligned (top/mid/bottom only); a parity
/// cross-check will be added in engine-integration tasks T18-T20 when both
/// sides are actually consumed together.
#[test]
fn shell_stress_struct_has_top_mid_bottom_field_params() {
    let template = find_structure("ShellStress");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        3,
        "ShellStress should have exactly 3 param cells (top, mid, bottom), got: {:?}",
        names
    );

    // All three channels share the same Field type: per-mesh-node Cauchy stress
    // tensor mapping from Point3<Length> domain to Tensor<2,3,Pressure> codomain.
    let shell_field_ty = Type::Field {
        domain: Box::new(Type::Point {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        }),
        codomain: Box::new(Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            }),
        }),
    };
    let expected: &[(&str, &Type)] = &[
        ("top", &shell_field_ty),
        ("mid", &shell_field_ty),
        ("bottom", &shell_field_ty),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "ShellStress missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, **expected_ty,
            "ShellStress.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}
