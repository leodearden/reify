#![allow(clippy::doc_overindented_list_items)]
//! Tests for `crates/reify-compiler/stdlib/modal_analysis.ri` —
//! `std.modal.analysis` module: `DampingDescriptor`, `NoDamping`,
//! `RayleighDamping`, `Mode`, `ModalResult`, and `ModalOptions` structure
//! definitions for the v0.3 modal-analysis kernel surface (task α), plus
//! the task η ForcingFunction family: `ForcingFunction` marker trait,
//! `StepForce`, `ImpulseForce`, `HarmonicForce`, `SampledForce`, and
//! `ForcingTimeHistory` structure definitions for the transient-response
//! forcing-time-history input surface (PRD §5.1 / §10 task η).
//!
//! Observable signal for PRD §10 tasks α and η
//! (docs/prds/v0_3/modal-analysis.md). Per the PRD, this file parses
//! the structure_defs and confirms type resolution matches the expected
//! shape.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `buckling_stdlib_compile.rs`), that the five α structures
//! (`NoDamping`, `RayleighDamping`, `Mode`, `ModalResult`, `ModalOptions`)
//! and one α trait (`DampingDescriptor`) are correctly represented in the
//! compiled module, that the positivity constraints on
//! `ModalOptions.{n_modes, tol, max_iters}` are declared at the
//! structure-def level, and that the η ForcingFunction family (one marker
//! trait + five structure_defs with constraints and defaults) matches the
//! PRD §5.1 spec.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `buckling_stdlib_compile.rs`.

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/modal/analysis` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/modal/analysis")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/modal/analysis module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/modal/analysis` module.
///
/// `Mode`, `ModalResult`, `ModalOptions`, `NoDamping`, and `RayleighDamping`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the module.
#[allow(dead_code)]
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/modal/analysis, got templates: {:?}",
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
/// Mirrors `collect_value_ref_members` in `buckling_stdlib_compile.rs:98-108`.
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

/// The std/modal/analysis module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_modal_analysis_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in modal_analysis.ri: {:?}",
        errors
    );
}

// ─── step-3: DampingDescriptor marker trait declared ─────────────────────────

/// `DampingDescriptor` is the marker trait the two damping-descriptor
/// structures (`NoDamping`, `RayleighDamping`) refine. Empty trait surface,
/// no methods — matches the marker-trait precedent in
/// `fea_multi_case.ri:288 trait Support { }` and
/// `trajectory.ri::trait BoundaryCondition { }`.
///
/// The trait must exist as an entry in `CompiledModule.trait_defs` (not
/// `templates`, which stores `Structure` / `Occurrence` entities only) in
/// the compiled `std/modal/analysis` module so the `: DampingDescriptor`
/// refinement clause on `NoDamping` / `RayleighDamping` resolves at
/// structure-def compile time, and so `Type::TraitObject("DampingDescriptor")`
/// resolves on `ModalResult.damping` and `ModalOptions.damping` once those
/// land.
#[test]
fn damping_descriptor_trait_declared() {
    let module = load_stdlib_module();

    let matches: Vec<_> = module
        .trait_defs
        .iter()
        .filter(|t| t.name == "DampingDescriptor")
        .collect();

    assert_eq!(
        matches.len(),
        1,
        "expected exactly one `trait DampingDescriptor` in \
         std/modal/analysis::trait_defs; got {} matches. Module trait_defs: {:?}",
        matches.len(),
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-5: NoDamping marker structure ──────────────────────────────────────

/// `NoDamping` is a zero-field marker structure refining `DampingDescriptor`.
/// Semantically equivalent to `RayleighDamping(alpha: 0, beta: 0)` but a
/// distinct nominal type so the future `modal_analysis` trampoline can
/// discriminate the no-damping fast path via SIR-α nominal type-tag.
///
/// Assertions mirror the "no constraints or defaults" discipline from
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`
/// (445-472): zero params, zero constraints, and refines `DampingDescriptor`
/// via `template.trait_bounds`.
#[test]
fn no_damping_marker_structure() {
    let template = find_structure("NoDamping");

    // (a) zero param cells — pure marker structure
    let params = param_cells(template);
    assert_eq!(
        params.len(),
        0,
        "NoDamping should be a zero-field marker structure, but got params: {:?}",
        params.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    // (b) no constraints — nothing to constrain
    assert!(
        template.constraints.is_empty(),
        "NoDamping should declare no constraints (zero-field marker); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (c) refines DampingDescriptor via the structure-def `: DampingDescriptor`
    // refinement clause. The plan analysis points at the `materials_fea.ri::
    // Steel_AISI_1045 : ElasticMaterial` precedent; `TopologyTemplate.
    // trait_bounds` (types.rs:518) is the canonical store for the names of
    // traits a structure declares conformance to.
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "DampingDescriptor"),
        "NoDamping should refine DampingDescriptor; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-7: RayleighDamping param shape ─────────────────────────────────────

/// `RayleighDamping` declares two PRD §4.2 params with the canonical types:
///
///   - `alpha : Real`  (mass-proportional damping coefficient)
///   - `beta  : Real`  (stiffness-proportional damping coefficient)
///
/// Per-mode damping ratio: ζ_i = (α + β·ω_i²) / (2·ω_i). Preserves mode-shape
/// orthogonality so transient response stays in real arithmetic.
///
/// Assertions:
///   (a) exactly 2 params, (b) the two params are (alpha, beta) of type Real
///       in declaration order,
///   (c) neither carries a `default_expr` (input-only fields without a
///       canonical default — PRD §4.2 lists no defaults),
///   (d) no constraints — alpha and beta are conventionally non-negative
///       but physically meaningful at zero (stiffness-only or mass-only
///       damping). Mirrors `solver_buckling.ri:97-107` "explicitly NOT
///       constrained" discipline applied to `sigma`,
///   (e) refines `DampingDescriptor`.
#[test]
fn rayleigh_damping_param_shape() {
    let template = find_structure("RayleighDamping");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        2,
        "RayleighDamping should have exactly 2 param cells (alpha, beta), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[("alpha", Type::Real), ("beta", Type::Real)];
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.id.member.as_str(),
            *expected_name,
            "RayleighDamping param at index {} should be `{}`, got `{}`",
            i,
            expected_name,
            cell.id.member
        );
        assert_eq!(
            cell.cell_type, *expected_ty,
            "RayleighDamping.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults on either param
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "RayleighDamping.{} should have no default_expr (no canonical \
             default for damping coefficients per PRD §4.2), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) no constraints — mirrors solver_buckling.ri:97-107 "explicitly NOT
    // constrained" discipline applied to sigma (zero is physically valid).
    assert!(
        template.constraints.is_empty(),
        "RayleighDamping should declare no constraints (alpha/beta are \
         conventionally non-negative but physically meaningful at zero — \
         stiffness-only or mass-only damping); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    // (e) refines DampingDescriptor
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "DampingDescriptor"),
        "RayleighDamping should refine DampingDescriptor; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-9: Mode param shape (no constraints, no defaults) ──────────────────

/// `Mode` (in std/modal/analysis — NOT std/solver/buckling's coexisting
/// Mode; see plan.json design-decision-6) must declare exactly the four
/// PRD §4.1 params with the canonical types:
///
///   - `frequency          : Real`                 (placeholder for Scalar<Frequency>;
///                                                  encoded as Real per plan design-decision-3)
///   - `shape              : List<Vector3<Dimensionless>>`  (mass-normalized eigenvector;
///                                                  dimensionless under Φᵀ·M·Φ = I — NOT a placeholder)
///   - `participation_mass : Real`                 (effective modal mass along reference direction)
///   - `damping_ratio      : Real`                 (ζ_i derived from Rayleigh α/β, or 0 for undamped)
///
/// Mode lives in std/modal/analysis; the `find_structure` helper at the top
/// of this file already filters to that module via `load_stdlib_module()`,
/// so this lookup does NOT see buckling's Mode template even though both
/// modules share the simple name (per the per-module template storage
/// invariant pinned in plan design-decision-6).
#[test]
fn mode_struct_has_correct_param_shape() {
    let template = find_structure("Mode");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "Mode should have exactly 4 param cells \
         (frequency, shape, participation_mass, damping_ratio), got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("frequency", Type::Real),
        (
            "shape",
            Type::List(Box::new(Type::vec3(Type::dimensionless_scalar()))),
        ),
        ("participation_mass", Type::Real),
        ("damping_ratio", Type::Real),
    ];

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.id.member.as_str(),
            *expected_name,
            "Mode param at index {} should be `{}`, got `{}`",
            i,
            expected_name,
            cell.id.member
        );
        assert_eq!(
            cell.cell_type, *expected_ty,
            "Mode.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }
}

/// `Mode` is a solver-populated output container — every field is determined
/// by the modal solve, so caller-supplied defaults are meaningless and no
/// per-field scalar invariant is expressible per-field (frequency depends on
/// the geometry, shape is collection-shaped, participation_mass and
/// damping_ratio are derived). Mirrors `Mode` discipline in
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`
/// (445-472).
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

    // No constraints: frequency / participation_mass / damping_ratio are all
    // physically non-negative but every modal-solver implementation enforces
    // that as a producer invariant; declaring them at the structure-def
    // level would be redundant duplication and could fire spuriously on
    // floating-point round-off. shape is collection-shaped, not scalar.
    assert!(
        template.constraints.is_empty(),
        "Mode should declare no constraints (solver-only-produced output \
         container, producer-enforced invariants only); got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-11: ModalResult param shape (no constraints, no defaults) ──────────

/// `ModalResult` is the solver-output container (PRD §4.1). It must declare
/// exactly the six PRD §4.1 params with the canonical types, in declaration
/// order:
///
///   - `part                  : String`                  (PLACEHOLDER for the
///                                                        future `Part`
///                                                        structure_def from
///                                                        the v0.3 solver-
///                                                        elastic PRD — see
///                                                        plan design-decision-2)
///   - `modes                 : List<Mode>`              (computed eigenpairs;
///                                                        `Mode` is module-local
///                                                        → `Type::StructureRef`)
///   - `boundary_conditions    : List<Support>`           (`Support` is the
///                                                        marker trait from
///                                                        `std.fea.multi_case`,
///                                                        in the growing prelude
///                                                        → `Type::TraitObject`,
///                                                        same as trajectory's
///                                                        `List<BoundaryCondition>`)
///   - `damping               : DampingDescriptor`       (trait-typed
///                                                        → `Type::TraitObject`)
///   - `mass_matrix_norm       : Real`                    (‖M‖ diagnostic)
///   - `stiffness_matrix_norm  : Real`                    (‖K‖ diagnostic)
///
/// Type representations confirmed against the trajectory precedent
/// (`trajectory_stdlib_compile.rs:628-639`): a module-local structure name
/// resolves to `Type::StructureRef`, a prelude marker trait resolves to
/// `Type::TraitObject`, and `List<Trait>` wraps the trait object in
/// `Type::List`.
///
/// `ModalResult` is solver-populated only: every field is determined by the
/// modal solve, so no caller-supplied defaults are meaningful and no scalar
/// constraint is declared at the structure-def level (collection invariants
/// such as "modes non-empty / sorted by frequency" are enforced at the future
/// modal_analysis trampoline, mirroring `BucklingResult` discipline at
/// `solver_buckling.ri:196-205` and the no-constraints-no-defaults shape from
/// `buckling_stdlib_compile.rs::mode_struct_has_no_constraints_or_defaults`).
#[test]
fn modal_result_struct_has_correct_param_shape() {
    let template = find_structure("ModalResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        6,
        "ModalResult should have exactly 6 param cells (part, modes, \
         boundary_conditions, damping, mass_matrix_norm, \
         stiffness_matrix_norm), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("part", Type::String),
        (
            "modes",
            Type::List(Box::new(Type::StructureRef("Mode".to_string()))),
        ),
        (
            "boundary_conditions",
            Type::List(Box::new(Type::TraitObject("Support".to_string()))),
        ),
        (
            "damping",
            Type::TraitObject("DampingDescriptor".to_string()),
        ),
        ("mass_matrix_norm", Type::Real),
        ("stiffness_matrix_norm", Type::Real),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ModalResult params must be declared in canonical order \
         (part, modes, boundary_conditions, damping, mass_matrix_norm, \
         stiffness_matrix_norm); got: {:?}",
        names
    );

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ModalResult.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — solver-populated output container
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ModalResult.{} should have no default_expr (solver-only-produced \
             output container), but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (c) no constraints — collection invariants (modes non-empty / sorted)
    // are enforced at the future modal_analysis trampoline, not declared at
    // the structure-def level (mirrors BucklingResult discipline at
    // solver_buckling.ri:196-205).
    assert!(
        template.constraints.is_empty(),
        "ModalResult should declare no constraints (solver-only-produced \
         output container; collection invariants are trampoline-enforced); \
         got: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-13: ModalOptions param shape ───────────────────────────────────────

/// `ModalOptions` is the modal-analysis solver-input knob bundle (PRD §4.3).
/// It must declare exactly the eight params (the seven PRD §4.3 params plus the
/// task-4066 `element_order` selector) with the canonical types, in declaration
/// order:
///
///   - `n_modes             : Int`                       (# modes to extract)
///   - `boundary_conditions  : List<Support>`             (`Support` marker
///                                                        trait from
///                                                        `std.fea.multi_case`
///                                                        → `List<TraitObject>`)
///   - `damping             : DampingDescriptor`         (trait-typed
///                                                        → `Type::TraitObject`)
///   - `sigma               : Real`                       (spectral shift origin)
///   - `tol                 : Real`                       (convergence tolerance)
///   - `max_iters           : Int`                        (Lanczos iteration cap)
///   - `reference_direction  : Vector3<Dimensionless>`     (unit excitation
///                                                        direction — a unit
///                                                        vector is
///                                                        dimensionless, so
///                                                        `Dimensionless` is
///                                                        mathematically
///                                                        accurate, NOT a
///                                                        placeholder)
///   - `element_order        : ElementOrder`               (P1/P2 finite-element
///                                                        order for the (K, M)
///                                                        assembly; task 4066 —
///                                                        `Type::Enum("ElementOrder")`,
///                                                        same as
///                                                        `ElasticOptions.element_order`)
///
/// `reference_direction` uses `Vector3<Dimensionless>` — identical to the
/// `Mode.shape : List<Vector3<Dimensionless>>` encoding. `Vector3<Real>` is
/// NOT valid .ri syntax: the `Vector3<Q>` resolver requires `Q` to resolve
/// to a `DimensionVector`, and `Real` is a primitive scalar, not a dimension
/// name. `Vector3<Dimensionless>` resolves to
/// `Type::vec3(Type::dimensionless_scalar())` (same representation pinned by
/// `mode_struct_has_correct_param_shape`).
///
/// This test pins ONLY the param count, names, declaration order, and types.
/// Defaults are pinned separately by step-15
/// (`modal_options_param_defaults_match_spec`) and constraints by step-17
/// (`modal_options_constrains_positivity_invariants`), so this test
/// deliberately asserts neither.
#[test]
fn modal_options_struct_has_correct_param_shape() {
    let template = find_structure("ModalOptions");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        8,
        "ModalOptions should have exactly 8 param cells (n_modes, \
         boundary_conditions, damping, sigma, tol, max_iters, \
         reference_direction, element_order), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("n_modes", Type::Int),
        (
            "boundary_conditions",
            Type::List(Box::new(Type::TraitObject("Support".to_string()))),
        ),
        (
            "damping",
            Type::TraitObject("DampingDescriptor".to_string()),
        ),
        ("sigma", Type::Real),
        ("tol", Type::Real),
        ("max_iters", Type::Int),
        (
            "reference_direction",
            Type::vec3(Type::dimensionless_scalar()),
        ),
        // task 4066 — P1/P2 finite-element-order selector for the (K, M)
        // assembly; `Type::Enum("ElementOrder")`, exactly like
        // `ElasticOptions.element_order` (solver_elastic_tests.rs:204).
        ("element_order", Type::Enum("ElementOrder".to_string())),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ModalOptions params must be declared in canonical order \
         (n_modes, boundary_conditions, damping, sigma, tol, max_iters, \
         reference_direction, element_order); got: {:?}",
        names
    );

    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ModalOptions.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }
}

// ─── step-15: ModalOptions literal-valued defaults ───────────────────────────

/// `ModalOptions` carries literal-valued defaults on its four scalar knobs
/// (PRD §4.3), limited to literal-valued per plan design-decision-5
/// (trait-typed / Vector3-literal default code-paths are not exercised by
/// any existing stdlib structure_def, so those three params stay defaultless
/// and required-at-construction in this wave):
///
///   - `n_modes   = 10`           (mirrors `BucklingOptions.n_modes`; the
///                                "first few modes" inspection workflow)
///   - `sigma     = 0.0`          (smallest-|λ| / lowest-frequency cluster)
///   - `tol       = 0.000000001`  (= 1e-9; decimal literal because Reify's
///                                number grammar has no scientific notation —
///                                strict-equality discipline per
///                                solver_buckling.ri:62-64)
///   - `max_iters = 200`          (PRD §4.3 — NOT 1000; modal converges
///                                faster than buckling)
///
/// `boundary_conditions`, `damping`, and `reference_direction` are required
/// at construction (no canonical default — see plan design-decision-5).
///
/// Mirrors `buckling_stdlib_compile.rs::buckling_options_param_defaults_match_spec`
/// (208-288), including the strict-equality float discipline (IEEE-754
/// round-to-nearest of these exact decimal literals is deterministic).
#[test]
fn modal_options_param_defaults_match_spec() {
    let template = find_structure("ModalOptions");

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

    // sigma = 0.0 (strict equality; IEEE-754 round-to-nearest deterministic)
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

    // tol = 0.000000001 (= 1e-9 in decimal; strict-equality discipline per
    // solver_buckling.ri:62-64 decimal-encoding note)
    let tol_default = require_default(template, "tol");
    match &tol_default.kind {
        CompiledExprKind::Literal(Value::Real(v)) => assert_eq!(
            *v, 0.000000001,
            "tol default should be exactly 0.000000001 (= 1e-9), got: {}",
            v
        ),
        other => panic!(
            "tol default should be Literal(Value::Real(0.000000001)), got: {:?}",
            other
        ),
    }

    // max_iters = 200 (NOT 1000 — PRD §4.3 specifies 200 for modal)
    let max_iters_default = require_default(template, "max_iters");
    match &max_iters_default.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(*v, 200, "max_iters default should be 200, got: {}", v)
        }
        other => panic!(
            "max_iters default should be Literal(Value::Int(200)), got: {:?}",
            other
        ),
    }

    // boundary_conditions / damping / reference_direction are required at
    // construction — no canonical default (plan design-decision-5).
    for member in ["boundary_conditions", "damping", "reference_direction"] {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("ModalOptions.{} missing", member));
        assert!(
            cell.default_expr.is_none(),
            "ModalOptions.{} should have NO default_expr (required at \
             construction per plan design-decision-5), but got: {:?}",
            member,
            cell.default_expr
        );
    }
}

// ─── step-17: ModalOptions positivity-invariant constraints ──────────────────

/// `ModalOptions` must declare exactly the three PRD §4.3 positivity
/// constraints at the structure-def level:
///
///   constraint n_modes   > 0
///   constraint tol       > 0
///   constraint max_iters > 0
///
/// Making the contract explicit in production code rather than relying solely
/// on test coverage is the task-2544 convention (recorded in memory id
/// 0773d3a8).
///
/// Scope note: this test asserts only the *presence and shape* of the
/// constraint AST nodes on the compiled `ModalOptions` template. It does NOT
/// instantiate `ModalOptions(n_modes: 0)` and assert a diagnostic — that
/// user-observable signal is satisfied compositionally (plan
/// design-decision 7), not re-verified here. These structure-def
/// declarations feed the SIR-α generic constraint-firing pipeline, which is
/// pinned end-to-end by
/// `crates/reify-eval/tests/stress_error_messages.rs::constraint_violation_diagnostic`
/// (constraint → `Satisfaction::Violated` diagnostic) and the
/// `Value::StructureInstance` round-trip in
/// `crates/reify-eval/tests/structure_instance_e2e.rs`. A modal-specific
/// eval test would duplicate that generic coverage without adding signal.
///
/// Explicitly NOT constrained (regression-gated by the tight count==3):
///   - `sigma`               : any spectral shift is physically valid (the
///                              negative side of the spectrum is meaningful);
///                              `sigma >= 0` would wrongly forbid it. Mirrors
///                              the `BucklingOptions.sigma` discipline.
///   - `reference_direction` : the `norm() > 0` invariant is a method-call on
///                              Vector3, NOT a scalar predicate, so it is not
///                              expressible in Reify's `constraint` grammar.
///                              Deferred to the runtime trampoline (future
///                              task ζ) per plan design-decision-4, mirroring
///                              `BucklingOptions.mode` allowlist-deferral.
///   - `damping`             : trait-typed; not scalar-predicable.
///   - `boundary_conditions` : collection of trait objects; not scalar-
///                              predicable.
///
/// Assertion shape mirrors
/// `buckling_stdlib_compile.rs::buckling_options_constrains_positivity_invariants`
/// (320-376), including the tight count==3 regression gate and the
/// Int(0)/Real(0.0) RHS-literal future-proofing.
#[test]
fn modal_options_constrains_positivity_invariants() {
    let template = find_structure("ModalOptions");

    // Tight count: exactly 3 constraints. A weaker `>= 3` would let a bogus
    // 4th constraint (e.g., an accidental `constraint sigma >= 0` that would
    // silently exclude negative-side-of-spectrum shifts) pass. The .ri file's
    // "explicitly NOT constrained" note is enforced here as a regression gate.
    assert_eq!(
        template.constraints.len(),
        3,
        "ModalOptions should declare exactly 3 constraints \
         (n_modes > 0, tol > 0, max_iters > 0); sigma / damping / \
         boundary_conditions / reference_direction are explicitly NOT \
         constrained per the .ri file. Got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for required in &["n_modes", "tol", "max_iters"] {
        let matched = template.constraints.iter().any(|c| {
            // Constraint expression must be a `>` BinOp with a ValueRef to the
            // required member on the LHS and the literal `0` on the RHS.
            // Accept either `Int(0)` or `Real(0.0)` for the RHS literal
            // (mirrors buckling_stdlib_compile.rs:356-360 future-proofing).
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
            "ModalOptions should declare `constraint {} > 0`; got constraints: {:?}",
            required,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── η additions: ForcingFunction family ─────────────────────────────────────

/// Recursively walk an expression tree collecting `(method_name, member_name)`
/// pairs from `MethodCall { object: ValueRef(member), method: name, .. }` nodes.
/// The traversal also recurses into `BinOp`, `UnOp`, and nested `MethodCall`
/// receivers so a deeply-nested chain like `sources.count > 0` surfaces
/// `("count", "sources")`.
///
/// Ported verbatim from `crates/reify-compiler/tests/trajectory_stdlib_compile.rs:125-144`
/// (same helper used by `piecewise_polynomial_profile_constrains_waypoints_nonempty`
/// for the `waypoints.count > 0` assertion shape needed here).
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

// ─── step-3 (η): StepForce param shape ───────────────────────────────────────

/// `StepForce` (PRD §5.1) applies a unit-step force at a location. Must
/// declare exactly 4 params in declaration order:
///
///   - `at        : String`                   (PLACEHOLDER for LocationId)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `magnitude : Force`                    (positive scalar size)
///   - `start_time : Time`                    (step onset time)
///
/// Must refine `ForcingFunction` via `trait_bounds`. No defaults on any
/// param (all caller-supplied). Constraint lands in step-6.
#[test]
fn step_force_struct_has_correct_param_shape() {
    let template = find_structure("StepForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "StepForce should have exactly 4 param cells \
         (at, direction, magnitude, start_time), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::String),
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "magnitude",
            Type::Scalar {
                dimension: DimensionVector::FORCE,
            },
        ),
        (
            "start_time",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "StepForce params must be in canonical order (at, direction, magnitude, start_time)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "StepForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "StepForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "ForcingFunction"),
        "StepForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-7 (η): ImpulseForce param shape ────────────────────────────────────

/// `ImpulseForce` (PRD §5.1) applies a Dirac-delta impulse at a location.
/// Must declare exactly 4 params in declaration order:
///
///   - `at        : String`                   (PLACEHOLDER for LocationId)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `impulse   : Real`                     (PLACEHOLDER for ImpulseDim = N·s)
///   - `time      : Time`                     (delta-application time)
///
/// Must refine `ForcingFunction` via `trait_bounds`. No defaults.
/// `impulse : Real` is a PLACEHOLDER for the unrepresentable `ImpulseDim`
/// (= N·s = momentum = MASS·LENGTH·TIME⁻¹; not in NAMED_DIMENSIONS).
/// Constraint lands in step-10.
#[test]
fn impulse_force_struct_has_correct_param_shape() {
    let template = find_structure("ImpulseForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "ImpulseForce should have exactly 4 param cells \
         (at, direction, impulse, time), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::String),
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        ("impulse", Type::Real), // PLACEHOLDER for ImpulseDim (design-decision-3)
        (
            "time",
            Type::Scalar {
                dimension: DimensionVector::TIME,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ImpulseForce params must be in canonical order (at, direction, impulse, time)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ImpulseForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ImpulseForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "ForcingFunction"),
        "ImpulseForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-11 (η): HarmonicForce param shape ──────────────────────────────────

/// `HarmonicForce` (PRD §5.1) applies F(t) = amplitude·sin(2π·frequency·t + phase).
/// Must declare exactly 5 params in declaration order:
///
///   - `at        : String`                   (PLACEHOLDER for LocationId)
///   - `direction : Vector3<Dimensionless>`   (unit excitation vector)
///   - `amplitude : Force`                    (positive peak force)
///   - `frequency : Frequency`                (positive cycles/second)
///   - `phase     : Angle`                    (phase offset; default 0deg)
///
/// Must refine `ForcingFunction`. The `phase` param carries a default of
/// `0deg` (zero Angle literal) per PRD §5.1 default spec; the other four
/// are caller-supplied (no defaults). Constraints land in step-14.
#[test]
fn harmonic_force_struct_has_correct_param_shape() {
    let template = find_structure("HarmonicForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        5,
        "HarmonicForce should have exactly 5 param cells \
         (at, direction, amplitude, frequency, phase), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::String),
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "amplitude",
            Type::Scalar {
                dimension: DimensionVector::FORCE,
            },
        ),
        (
            "frequency",
            Type::Scalar {
                dimension: DimensionVector::FREQUENCY,
            },
        ),
        (
            "phase",
            Type::Scalar {
                dimension: DimensionVector::ANGLE,
            },
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "HarmonicForce params must be in canonical order \
         (at, direction, amplitude, frequency, phase)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "HarmonicForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults on at/direction/amplitude/frequency; phase HAS a default
    for name in &["at", "direction", "amplitude", "frequency"] {
        let cell = params.iter().find(|vc| vc.id.member == *name).unwrap();
        assert!(
            cell.default_expr.is_none(),
            "HarmonicForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            name,
            cell.default_expr
        );
    }
    // phase = 0deg — must have a default that is a zero Angle literal
    let phase_default = require_default(template, "phase");
    match &phase_default.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, dimension })
            if *si_value == 0.0 && *dimension == DimensionVector::ANGLE =>
        {
            // correct: 0deg = 0 radians in SI
        }
        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => {
            // acceptable fallback if literal-lowering emits Real for zero
        }
        other => panic!(
            "HarmonicForce.phase default should be Literal(0 Angle), got: {:?}",
            other
        ),
    }

    // (d) refines ForcingFunction
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "ForcingFunction"),
        "HarmonicForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-15 (η): SampledForce param shape ───────────────────────────────────

/// `SampledForce` (PRD §5.1 / §5.3) applies a non-uniform-sample force table
/// (Duhamel/Newmark-β fallback). Must declare exactly 4 params in order:
///
///   - `at           : String`         (PLACEHOLDER for LocationId)
///   - `direction    : Vector3<Dimensionless>` (unit excitation vector)
///   - `time_samples : List<Time>`     (non-uniform time stamps)
///   - `force_samples: List<Force>`    (force magnitudes at each sample)
///
/// Must refine `ForcingFunction`. No defaults. Constraints land in step-18.
/// The cross-list invariant `time_samples.count == force_samples.count` is NOT
/// expressible in Reify constraint grammar (deferred to trampoline task θ).
#[test]
fn sampled_force_struct_has_correct_param_shape() {
    let template = find_structure("SampledForce");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count
    assert_eq!(
        params.len(),
        4,
        "SampledForce should have exactly 4 param cells \
         (at, direction, time_samples, force_samples), got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("at", Type::String),
        ("direction", Type::vec3(Type::dimensionless_scalar())),
        (
            "time_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::TIME,
            })),
        ),
        (
            "force_samples",
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::FORCE,
            })),
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "SampledForce params must be in canonical order \
         (at, direction, time_samples, force_samples)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "SampledForce.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "SampledForce.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) refines ForcingFunction
    assert!(
        template
            .trait_bounds
            .iter()
            .any(|t| t == "ForcingFunction"),
        "SampledForce should refine ForcingFunction; got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-17 (η): SampledForce non-empty sample constraints ──────────────────

/// `SampledForce` must declare EXACTLY 2 constraints:
///   - `time_samples.count > 0`
///   - `force_samples.count > 0`
///
/// Uses `collect_method_call_chain` to surface the `("count", "time_samples")`
/// and `("count", "force_samples")` method-call pairs on the LHS. Mirrors
/// `piecewise_polynomial_profile_constrains_waypoints_nonempty`
/// (trajectory_stdlib_compile.rs:702-761) for the `waypoints.count > 0` shape.
///
/// Cross-list invariant `time_samples.count == force_samples.count` is NOT
/// constrained here (Reify grammar: single-cell scalar predicates only).
#[test]
fn sampled_force_constrains_samples_nonempty() {
    let template = find_structure("SampledForce");

    assert_eq!(
        template.constraints.len(),
        2,
        "SampledForce should declare exactly 2 constraints \
         (time_samples.count > 0, force_samples.count > 0); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for required_member in &["time_samples", "force_samples"] {
        let matched = template.constraints.iter().any(|c| {
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt {
                        return false;
                    }
                    let pairs = collect_method_call_chain(left);
                    if !pairs.contains(&("count", *required_member)) {
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
            "SampledForce should declare `constraint {}.count > 0`; \
             got constraints: {:?}",
            required_member,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-13 (η): HarmonicForce amplitude + frequency positivity constraints ──

/// `HarmonicForce` must declare EXACTLY 2 constraints:
///   - `amplitude > 0N`  (PRD §5.1 user-observable-signal anchor)
///   - `frequency > 0Hz` (zero/negative frequency is physically degenerate)
///
/// Tight count==2 regression-gates against accidental extras (e.g., a spurious
/// `phase >= 0` that would wrongly forbid negative phase offsets).
///
/// RHS literal is `Value::Scalar { si_value: 0.0, dimension: FORCE/FREQUENCY }`
/// (dimensioned) OR `Value::Real(0.0)` (future-proofing).
///
/// This test serves as the PRD §5.1 user-observable-signal anchor:
/// `HarmonicForce(amplitude: -1N, ...)` → constraint-violation diagnostic.
/// The actual ctor-firing is verified compositionally by
/// `stress_error_messages.rs::constraint_violation_diagnostic` (plan
/// design-decision-7) — this test pins only the structure-def-level AST.
#[test]
fn harmonic_force_constrains_amplitude_and_frequency_positive() {
    let template = find_structure("HarmonicForce");

    assert_eq!(
        template.constraints.len(),
        2,
        "HarmonicForce should declare exactly 2 constraints \
         (amplitude > 0N, frequency > 0Hz); got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    for (required_member, required_dim) in &[
        ("amplitude", DimensionVector::FORCE),
        ("frequency", DimensionVector::FREQUENCY),
    ] {
        let matched = template.constraints.iter().any(|c| {
            match &c.expr.kind {
                CompiledExprKind::BinOp { op, left, right } => {
                    if *op != BinOp::Gt
                        || !collect_value_ref_members(left).contains(required_member)
                    {
                        return false;
                    }
                    match &right.kind {
                        CompiledExprKind::Literal(Value::Scalar {
                            si_value,
                            dimension,
                        }) if *si_value == 0.0 && dimension == required_dim => true,
                        CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                        _ => false,
                    }
                }
                _ => false,
            }
        });
        assert!(
            matched,
            "HarmonicForce should declare `constraint {} > 0`; \
             got constraints: {:?}",
            required_member,
            template
                .constraints
                .iter()
                .map(|c| &c.expr.kind)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-9 (η): ImpulseForce impulse positivity constraint ──────────────────

/// `ImpulseForce` must declare exactly 1 constraint: `impulse > 0`.
///
/// `impulse : Real` (PLACEHOLDER for ImpulseDim) uses bare `0` (not `0unit`)
/// because the field is Real-typed — same shape as `n_modes > 0` on Int.
/// Direction carries the sign; impulse is the positive scalar size.
/// Mirrors `step_force_constrains_magnitude_positive` discipline (tight count==1).
#[test]
fn impulse_force_constrains_impulse_positive() {
    let template = find_structure("ImpulseForce");

    assert_eq!(
        template.constraints.len(),
        1,
        "ImpulseForce should declare exactly 1 constraint (impulse > 0); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt || !collect_value_ref_members(left).contains(&"impulse") {
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
        "ImpulseForce should declare `constraint impulse > 0`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-5 (η): StepForce magnitude positivity constraint ───────────────────

/// `StepForce` must declare exactly 1 constraint: `magnitude > 0N`.
///
/// Convention: `direction : Vector3<Dimensionless>` carries the sign (unit
/// vector); `magnitude : Force` carries the positive scalar size. A negative
/// magnitude is meaningless when direction is the sign-carrying unit vector.
/// PRD §5.1 user-observable signal; task-2544 explicit-contract convention.
///
/// Mirrors `modal_options_constrains_positivity_invariants` (lines 773-826)
/// discipline: tight count==1 regression gate, and the dimensioned RHS literal
/// is accepted as `Value::Scalar { si_value: 0.0, dimension: FORCE }` OR
/// `Value::Real(0.0)` (same future-proofing as Int(0)/Real(0.0) at lines
/// 807-810, applied to the dimensioned-literal lowering path).
#[test]
fn step_force_constrains_magnitude_positive() {
    let template = find_structure("StepForce");

    // Tight count: exactly 1 constraint (regression gate — no accidental extras)
    assert_eq!(
        template.constraints.len(),
        1,
        "StepForce should declare exactly 1 constraint (magnitude > 0N); \
         got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt
                    || !collect_value_ref_members(left).contains(&"magnitude")
                {
                    return false;
                }
                match &right.kind {
                    CompiledExprKind::Literal(Value::Scalar { si_value, .. })
                        if *si_value == 0.0 =>
                    {
                        true
                    }
                    CompiledExprKind::Literal(Value::Real(v)) if *v == 0.0 => true,
                    _ => false,
                }
            }
            _ => false,
        }
    });
    assert!(
        matched,
        "StepForce should declare `constraint magnitude > 0N`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── step-1 (η): ForcingFunction marker trait declared ───────────────────────

/// `ForcingFunction` is the marker trait for the four transient-forcing
/// primitives (PRD §5.1 / §10 task η). Empty trait surface, no methods —
/// same marker-trait pattern as `trait DampingDescriptor { }` (lines 154-176)
/// and `trait Support { }` (fea_multi_case.ri:288).
///
/// The trait must exist as an entry in `CompiledModule.trait_defs` (not
/// `templates`, which stores `Structure` / `Occurrence` entities only) in
/// the compiled `std/modal/analysis` module so the `: ForcingFunction`
/// refinement clause on each conformer resolves at structure-def compile
/// time, and so `Type::TraitObject("ForcingFunction")` resolves on
/// `ForcingTimeHistory.sources : List<ForcingFunction>`.
#[test]
fn forcing_function_trait_declared() {
    let module = load_stdlib_module();

    let matches: Vec<_> = module
        .trait_defs
        .iter()
        .filter(|t| t.name == "ForcingFunction")
        .collect();

    assert_eq!(
        matches.len(),
        1,
        "expected exactly one `trait ForcingFunction` in \
         std/modal/analysis::trait_defs; got {} matches. Module trait_defs: {:?}",
        matches.len(),
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-19 (η): ForcingTimeHistory param shape ──────────────────────────────

/// `ForcingTimeHistory` (PRD §5.1) is the aggregate container that bundles N
/// forcing sources at the per-Part layer. Must declare exactly 2 params in
/// declaration order:
///
///   - `part    : String`                      (PLACEHOLDER for future `Part` type;
///                                              mirrors `ModalResult.part : String`)
///   - `sources : List<ForcingFunction>`       (List of trait-object conformers;
///                                              resolves to
///                                              `Type::List(Box::new(Type::TraitObject("ForcingFunction")))`)
///
/// Must NOT refine `ForcingFunction` — `ForcingTimeHistory` is the AGGREGATE
/// container, not a forcing primitive. `trait_bounds` must be empty.
/// No defaults on either param (all caller-supplied).
/// Constraint lands in step-22.
#[test]
fn forcing_time_history_struct_has_correct_param_shape() {
    let template = find_structure("ForcingTimeHistory");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    // (a) tight count — exactly 2 params
    assert_eq!(
        params.len(),
        2,
        "ForcingTimeHistory should have exactly 2 param cells (part, sources), \
         got: {:?}",
        names
    );

    // (b) param names + types in declaration order
    let expected: &[(&str, Type)] = &[
        ("part", Type::String),
        (
            "sources",
            Type::List(Box::new(Type::TraitObject("ForcingFunction".to_string()))),
        ),
    ];
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "ForcingTimeHistory params must be in canonical order (part, sources)"
    );
    for (i, (expected_name, expected_ty)) in expected.iter().enumerate() {
        let cell = &params[i];
        assert_eq!(
            cell.cell_type, *expected_ty,
            "ForcingTimeHistory.{} should be {:?}, got {:?}",
            expected_name, expected_ty, cell.cell_type
        );
    }

    // (c) no defaults — all caller-supplied
    for cell in &params {
        assert!(
            cell.default_expr.is_none(),
            "ForcingTimeHistory.{} should have no default_expr (caller-supplied), \
             but got: {:?}",
            cell.id.member,
            cell.default_expr
        );
    }

    // (d) NOT a ForcingFunction conformer — trait_bounds must be empty
    // ForcingTimeHistory is the AGGREGATE container; only the four primitives
    // (StepForce, ImpulseForce, HarmonicForce, SampledForce) refine ForcingFunction.
    assert!(
        template.trait_bounds.is_empty(),
        "ForcingTimeHistory should NOT refine ForcingFunction (it is the \
         aggregate container, not a forcing primitive); got trait_bounds: {:?}",
        template.trait_bounds
    );
}

// ─── step-21 (η): ForcingTimeHistory sources non-empty constraint ─────────────

/// `ForcingTimeHistory` must declare EXACTLY 1 constraint: `sources.count > 0`.
///
/// Uses `collect_method_call_chain` to surface the `("count", "sources")` pair
/// on the LHS. Mirrors `sampled_force_constrains_samples_nonempty` discipline
/// (tight count==1, `BinOp::Gt`, RHS `Literal(Int(0))` or `Real(0.0)`).
///
/// This constraint encodes the PRD §1 `E_TransientForcingMissing` diagnostic
/// at the structure-def level — an empty `sources` list is caught at
/// construction (via SIR-α's `check_constraints_against_templates`) rather than
/// waiting for the transient_response trampoline (task θ) to flag it. Follows
/// the task-2544 explicit-contract convention mirrored by
/// `PiecewisePolynomialProfile.constraint waypoints.count > 0` (trajectory.ri:230).
#[test]
fn forcing_time_history_constrains_sources_nonempty() {
    let template = find_structure("ForcingTimeHistory");

    assert_eq!(
        template.constraints.len(),
        1,
        "ForcingTimeHistory should declare exactly 1 constraint \
         (sources.count > 0); got {} constraints: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );

    let matched = template.constraints.iter().any(|c| {
        match &c.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                if *op != BinOp::Gt {
                    return false;
                }
                let pairs = collect_method_call_chain(left);
                if !pairs.contains(&("count", "sources")) {
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
        "ForcingTimeHistory should declare `constraint sources.count > 0`; \
         got constraints: {:?}",
        template
            .constraints
            .iter()
            .map(|c| &c.expr.kind)
            .collect::<Vec<_>>()
    );
}

// ─── task-4108: modal uses shared stdlib ElementOrder (no local copy) ─────────

/// Pins three invariants after the task-4108 prelude-enum value-lowering fix:
///
/// (a) `std/modal/analysis` does NOT own a local `enum ElementOrder` — the
///     type is now fully resolved from `std/solver/elastic`'s prelude copy.
/// (b) `ModalOptions.element_order`'s compiled default is
///     `Literal(Value::Enum { type_name: "ElementOrder", variant: "P1" })` —
///     identical to `ElasticOptions.element_order`'s default (solver_elastic_tests
///     `elastic_options_param_defaults_match_spec`), confirming the enum-access
///     lowered correctly through the prelude seeding path.
/// (c) The shared `ElementOrder` enum in `std/solver/elastic` carries variants
///     `["P1", "P2"]` in canonical order — the set modal's runtime trampoline
///     (`variant == "P2"`) reads. This is a cross-module drift anchor: it re-
///     anchors the `[P1, P2]` pin once modal's local copy has been dropped, so
///     the trampoline's dependency on solver_elastic's copy is explicit.
///
/// RED until step-4:
///   - assertion (a) fails: modal_analysis.ri still declares `enum ElementOrder`
///     (line ~290), so modal's `enum_defs` DOES contain an `ElementOrder` entry.
///   - assertions (b) and (c) pass (modal already compiled OK via its local copy,
///     and solver_elastic's ElementOrder is unaffected).
#[test]
fn modal_options_element_order_resolves_to_shared_stdlib_enum() {
    let modal_module = load_stdlib_module();

    // ── (a) modal has NO local ElementOrder enum_def ──────────────────────────
    assert!(
        modal_module.enum_defs.iter().all(|e| e.name != "ElementOrder"),
        "std/modal/analysis should NOT declare a local `enum ElementOrder` after \
         task-4108 drops the duplicate; got enum_defs: {:?}",
        modal_module
            .enum_defs
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // ── (b) ModalOptions.element_order default == Literal(Value::Enum{ElementOrder, P1}) ──
    let modal_options = find_structure("ModalOptions");
    let element_order_default = require_default(modal_options, "element_order");
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
            "ModalOptions.element_order default should be \
             Literal(Value::Enum {{ ElementOrder, P1 }}), got: {:?}",
            other
        ),
    }

    // ── (c) The shared solver_elastic ElementOrder carries [P1, P2] ───────────
    // Cross-module drift anchor: modal's runtime trampoline reads `variant == "P2"`
    // from the shared enum. Re-anchor the [P1, P2] canonical order here so the
    // solver_elastic dependency is explicit even after modal drops its local copy.
    let elastic_module = stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/elastic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/solver/elastic; available: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        });
    let enum_def = elastic_module
        .enum_defs
        .iter()
        .find(|e| e.name == "ElementOrder")
        .unwrap_or_else(|| {
            panic!(
                "std/solver/elastic should contain `enum ElementOrder`; got: {:?}",
                elastic_module
                    .enum_defs
                    .iter()
                    .map(|e| &e.name)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        enum_def.variants,
        vec!["P1".to_string(), "P2".to_string()],
        "std/solver/elastic ElementOrder variants should be [P1, P2] in canonical order; \
         modal trampoline reads `variant == \"P2\"` against this set. Got: {:?}",
        enum_def.variants
    );
}
