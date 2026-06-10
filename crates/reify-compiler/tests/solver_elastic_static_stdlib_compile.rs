//! Tests for `crates/reify-compiler/stdlib/solver_elastic.ri` —
//! the `fn solve_elastic_static` declaration in `std.solver.elastic`.
//!
//! Observable signal for PRD §8 task η (docs/prds/v0_3/compute-node-contract.md):
//! the stdlib function must carry `@optimized("solver::elastic_static")` so the
//! @optimized → ComputeNode lowering fires at eval time.
//!
//! These are RED tests for step-1. They fail until step-2 adds the declaration.

use reify_compiler::*;
use reify_core::{DiagnosticCode, Severity, Type};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/solver/elastic` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics with a helpful message (listing available paths) if the module is not
/// found — the expected failure mode before step-2 lands the declaration.
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

/// Look up `solve_elastic_static` in the stdlib module's `functions` vec.
///
/// Panics if not found — the expected failure mode for step-1 (RED).
fn find_fn() -> &'static CompiledFunction {
    let module = load_stdlib_module();
    module
        .functions
        .iter()
        .find(|f| f.name == "solve_elastic_static")
        .unwrap_or_else(|| {
            panic!(
                "fn solve_elastic_static not found in std/solver/elastic; \
                 available functions: {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// Pin: `fn solve_elastic_static` must carry `@optimized("solver::elastic_static")`.
///
/// The @optimized → ComputeNode lowering in `engine_eval.rs:2793-2944` inspects
/// `CompiledFunction.optimized_target`; if it is `None` the function body is
/// inlined instead of dispatched. This test ensures the lowering fires correctly.
#[test]
fn solve_elastic_static_has_optimized_target() {
    let f = find_fn();
    assert_eq!(
        f.optimized_target,
        Some("solver::elastic_static".to_string()),
        "fn solve_elastic_static must be annotated @optimized(\"solver::elastic_static\")"
    );
}

/// Pin: `fn solve_elastic_static` must have exactly 7 parameters.
///
/// Expected signature:
///   (material: ElasticMaterial, length: Length, width: Length, height: Length,
///    loads: List<Load>, supports: List<Support>, options: ElasticOptions)
///
/// A param-count change here means the trampoline's `value_inputs` indexing
/// (step-8) needs to be updated in lock-step with this test.
#[test]
fn solve_elastic_static_has_seven_params() {
    let f = find_fn();
    assert_eq!(
        f.params.len(),
        7,
        "expected 7 params (material, length, width, height, loads, supports, options), \
         got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );
}

/// Pin: `fn solve_elastic_static`'s first parameter (`material`) must have
/// type `Type::TraitObject("ConstitutiveLaw")` (task δ/3780).
///
/// Before step-2 the type is `TraitObject("ElasticMaterial")` → RED.
/// After step-2 changes the param annotation to `: ConstitutiveLaw` → GREEN.
#[test]
fn solve_elastic_static_material_param_is_constitutive_law() {
    let f = find_fn();
    let (name, ty) = &f.params[0];
    assert_eq!(
        name.as_str(),
        "material",
        "expected params[0] to be 'material', got {:?}",
        name
    );
    assert_eq!(
        *ty,
        Type::TraitObject("ConstitutiveLaw".to_string()),
        "expected material param type to be TraitObject(\"ConstitutiveLaw\"), got {:?}",
        ty
    );
}

/// Pin: `fn solve_elastic_static`'s fifth parameter (`loads`) must have
/// type `Type::List(Box::new(Type::TraitObject("Load")))`.
///
/// RED before step-2 (param is still `List<Real>`);
/// GREEN after step-2 changes the param annotation to `: List<Load>`.
#[test]
fn solve_elastic_static_loads_param_is_list_load() {
    let f = find_fn();
    let (name, ty) = &f.params[4];
    assert_eq!(
        name.as_str(),
        "loads",
        "expected params[4] to be 'loads', got {:?}",
        name
    );
    assert_eq!(
        *ty,
        Type::List(Box::new(Type::TraitObject("Load".to_string()))),
        "expected loads param type to be List<TraitObject(\"Load\")>, got {:?}",
        ty
    );
}

/// Pin: `fn solve_elastic_static`'s sixth parameter (`supports`) must have
/// type `Type::List(Box::new(Type::TraitObject("Support")))`.
///
/// RED before step-2 (param is still `List<Real>`);
/// GREEN after step-2 changes the param annotation to `: List<Support>`.
#[test]
fn solve_elastic_static_supports_param_is_list_support() {
    let f = find_fn();
    let (name, ty) = &f.params[5];
    assert_eq!(
        name.as_str(),
        "supports",
        "expected params[5] to be 'supports', got {:?}",
        name
    );
    assert_eq!(
        *ty,
        Type::List(Box::new(Type::TraitObject("Support".to_string()))),
        "expected supports param type to be List<TraitObject(\"Support\")>, got {:?}",
        ty
    );
}

/// Caller-compile positive (PointLoad + FixedSupport):
/// `solve_elastic_static(Steel_AISI_1045(), ..., [PointLoad(...)], [FixedSupport(...)], ElasticOptions())`
/// must compile with ZERO Error diagnostics (direct-pass via task γ/4441 supertrait,
/// ConstitutiveLawInput shim retired in task δ/4442).
#[test]
fn solve_elastic_static_direct_point_load_compiles_clean() {
    let src = r#"
structure FEACantileverTest {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module = reify_test_support::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for direct [PointLoad(...)]/[FixedSupport(...)] call, \
         got {:?}",
        errors
    );
}

/// Caller-compile positive (PressureLoad + FixedSupport):
/// `solve_elastic_static(Steel_AISI_1045(), ..., [PressureLoad(...)], [FixedSupport(...)], ElasticOptions())`
/// must compile with ZERO Error diagnostics (direct-pass via task γ/4441 supertrait,
/// ConstitutiveLawInput shim retired in task δ/4442).
#[test]
fn solve_elastic_static_direct_pressure_load_compiles_clean() {
    let src = r#"
structure FEAPressureTest {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [PressureLoad(magnitude: 1000000.0, face: "x_max", direction: "normal")],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module = reify_test_support::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for direct [PressureLoad(...)]/[FixedSupport(...)] call, \
         got {:?}",
        errors
    );
}

/// Caller-compile negative: passing a non-conforming list `[Steel_AISI_1045()]`
/// as the `loads` argument must yield at least one Error diagnostic with code
/// `DiagnosticCode::TypeNotConformingToTrait`.
///
/// The material arg is passed directly (ConstitutiveLawInput shim retired δ/4442);
/// only the loads arg is intentionally wrong so the conformance-error assertion fires.
#[test]
fn solve_elastic_static_non_conforming_loads_yields_type_not_conforming_to_trait() {
    let src = r#"
structure FEABadLoads {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [Steel_AISI_1045()],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module = reify_test_support::compile_source_with_stdlib(src);
    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
        })
        .collect();
    assert!(
        !conformance_errors.is_empty(),
        "expected at least one TypeNotConformingToTrait Error diagnostic for \
         [Steel_AISI_1045()] as loads arg, got all diagnostics: {:?}",
        module.diagnostics
    );
}

/// Regression guard: `solve_load_cases` with a `LoadCase` bundle must still
/// compile with ZERO Error diagnostics (direct-pass — ConstitutiveLawInput shim
/// retired in task δ/4442; the multi-case path is intentionally untouched).
#[test]
fn solve_load_cases_still_compiles_clean_after_tightening() {
    let src = r#"
structure FEAMultiCaseTest {
    let result = solve_load_cases(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [LoadCase(
            name: "c",
            loads: [PointLoad(point: "tip", force: 1000.0)],
            supports: [FixedSupport(target: "root")]
        )],
        ElasticOptions()
    )
}
"#;
    let module = reify_test_support::compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for solve_load_cases regression guard, got {:?}",
        errors
    );
}

// ─── Task δ/4442 struct-retirement tests ─────────────────────────────────────

/// GREEN test (after step-2): `ConstitutiveLawInput` is no longer declared in
/// the `std/solver/elastic` stdlib module.
///
/// The Reify compiler silently accepts calls to unknown constructor names
/// (emitting zero diagnostics), so the retirement cannot be verified via
/// diagnostic-counting.  Instead we inspect the compiled module's `templates`
/// vec directly — the authoritative record of every `structure def` compiled
/// from `solver_elastic.ri`.  If `ConstitutiveLawInput` is absent there, the
/// struct is gone.
///
/// This was RED while the struct still existed (pre step-2).  It is GREEN
/// after step-2 removes the `pub structure def ConstitutiveLawInput` block from
/// `solver_elastic.ri`.
///
/// Paired with a regression guard that the direct-pass form still compiles
/// clean (already GREEN post-γ/4441) so the test jointly captures
/// "wrapper gone AND direct-pass works".
#[test]
fn constitutive_law_input_struct_is_retired() {
    // ── negative probe: ConstitutiveLawInput must NOT be in the compiled module ──
    let module = load_stdlib_module();
    let found = module
        .templates
        .iter()
        .find(|t| t.name == "ConstitutiveLawInput");
    assert!(
        found.is_none(),
        "ConstitutiveLawInput structure should not exist in std/solver/elastic \
         after task δ/4442 retires the shim, but found: {:?}",
        found
    );

    // ── positive control: a known-retained struct IS in `templates` ───────────
    // This guards the absence assertion above — if `structure def` blocks were
    // ever routed to a different field, `found.is_none()` would trivially pass
    // even with the struct still present.  Asserting FEAMaterialInput (kept per
    // PRD §5) proves that structs DO land in `module.templates`, so the negative
    // probe is meaningful rather than vacuous.
    assert!(
        module.templates.iter().any(|t| t.name == "FEAMaterialInput"),
        "FEAMaterialInput should be present in std/solver/elastic templates \
         (retained per PRD §5); absence would make the ConstitutiveLawInput \
         negative probe above untrustworthy"
    );

    // ── positive regression: direct-pass still compiles clean ─────────────────
    let good_src = r#"
structure DirectPassRegression {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module2 = reify_test_support::compile_source_with_stdlib(good_src);
    let errors2: Vec<_> = module2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "regression: expected zero Error diagnostics for direct-pass \
         solve_elastic_static(Steel_AISI_1045(), ...) after struct retirement, \
         got {:?}",
        errors2
    );
}
