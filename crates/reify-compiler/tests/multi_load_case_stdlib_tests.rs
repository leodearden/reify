//! Tests for `crates/reify-compiler/stdlib/fea_multi_case.ri` — `std.fea.multi_case` module:
//! `LoadCase` and `MultiCaseResult` structure definitions for the v0.3.x
//! multi-load-case FEA workflow.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `LoadCase` and `MultiCaseResult` are correctly represented in the
//! compiled module, and that parameter shapes and defaults match the spec.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `solver_elastic_tests.rs`.
//!
//! Accessor argument contract (pinned in `multi_load_case_stdlib_smoke_e2e`):
//!   `result_for(mcr, key)` — `mcr` is `args[0]`, `key` is `args[1]`.

use reify_ir::*;
use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/fea/multi_case` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fea/multi_case")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fea/multi_case module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/fea/multi_case` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/fea/multi_case, got templates: {:?}",
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

// ─── module-level invariant ───────────────────────────────────────────────────

/// The std/fea/multi_case module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_fea_multi_case_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in fea_multi_case.ri (std/fea/multi_case): {:?}",
        errors
    );
}

// ─── LoadCase param shape ─────────────────────────────────────────────────────

/// `LoadCase` is the multi-load-case FEA solver-input bundle. It must declare
/// exactly four params with the canonical names and types:
///
///   - `name     : String`
///   - `loads    : List<Load>`    (conformance enforced — elements must satisfy `trait Load`)
///   - `supports : List<Support>` (conformance enforced — elements must satisfy `trait Support`)
///   - `options  : Option<ElasticOptions>`  (none = use solver defaults)
///
/// `loads` and `supports` are typed `List<Load>` / `List<Support>` respectively;
/// conformance is enforced at compile time via `TypeNotConformingToTrait` (task ζ/4444
/// tightened from the `List<Real>` placeholder). Precedent: `ModalResult.boundary_conditions
/// : List<Support>` in `modal_analysis.ri:244`.
///
/// Only `options` carries a default (`none`); the other three must be caller-supplied.
#[test]
fn loadcase_struct_has_correct_param_shape() {
    let template = find_structure("LoadCase");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "LoadCase should have exactly 4 param cells, got: {:?}",
        names
    );

    let expected: &[(&str, Type)] = &[
        ("name", Type::String),
        // After task ζ/4444: loads is List<Load> (List<TraitObject("Load")>), not List<Real>.
        ("loads", Type::List(Box::new(Type::TraitObject("Load".to_string())))),
        // After task ζ/4444: supports is List<Support> (List<TraitObject("Support")>), not List<Real>.
        ("supports", Type::List(Box::new(Type::TraitObject("Support".to_string())))),
        (
            "options",
            Type::Option(Box::new(Type::StructureRef("ElasticOptions".to_string()))),
        ),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "LoadCase missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "LoadCase.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }

    // Pin the canonical declaration order: name, loads, supports, options.
    // LoadCase follows the positional-argument convention used elsewhere in
    // the stdlib (e.g. ElasticResult). A silent re-ordering would change
    // positional-construction semantics without test coverage.
    assert_eq!(
        names,
        vec!["name", "loads", "supports", "options"],
        "LoadCase params must be declared in canonical order [name, loads, supports, options]"
    );
}

// ─── LoadCase param defaults ──────────────────────────────────────────────────

/// Each `LoadCase` param must carry the correct default (or absence thereof):
///
///   `name`     — no default (caller must name every load case explicitly)
///   `loads`    — no default (must be caller-supplied)
///   `supports` — no default (must be caller-supplied)
///   `options`  — `none` (bare `ElasticOptions()` defaults apply when unspecified)
///
/// The `options = none` default uses `CompiledExprKind::OptionNone` with
/// `result_type == Option<ElasticOptions>`.  The other three params (`name`,
/// `loads`, `supports`) have no defaults.
#[test]
fn loadcase_param_defaults_match_spec() {
    let template = find_structure("LoadCase");

    // name, loads, supports — must have NO default
    for no_default in &["name", "loads", "supports"] {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == *no_default)
            .unwrap_or_else(|| panic!("LoadCase.{} param cell missing", no_default));
        assert!(
            cell.default_expr.is_none(),
            "LoadCase.{} should have no default_expr (caller must supply it), \
             but got: {:?}",
            no_default,
            cell.default_expr
        );
    }

    // options = none
    let options_default = require_default(template, "options");
    assert!(
        matches!(&options_default.kind, CompiledExprKind::OptionNone),
        "options default should be OptionNone, got: {:?}",
        options_default.kind
    );
    assert_eq!(
        options_default.result_type,
        Type::Option(Box::new(Type::StructureRef("ElasticOptions".to_string()))),
        "options default's result_type should be Option<ElasticOptions>, got: {:?}",
        options_default.result_type
    );
}

// ─── MultiCaseResult param shape ─────────────────────────────────────────────

/// `MultiCaseResult` is the multi-load-case FEA solver-output container.
/// It must declare exactly one param with the canonical name and type:
///
///   - `cases : Map<String, ElasticResult>`  (keyed by `LoadCase.name`)
///
/// No defaults: every instance must be fully populated by `solve_load_cases`
/// (task #2) — a bare `MultiCaseResult()` with an empty Map is meaningless
/// without a solve. Map key-uniqueness is structurally guaranteed by `BTreeMap`;
/// the only producer-side invariant (non-empty Map) is enforced by
/// `solve_load_cases`.
#[test]
fn multi_case_result_struct_has_correct_param_shape() {
    let template = find_structure("MultiCaseResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        1,
        "MultiCaseResult should have exactly 1 param cell, got: {:?}",
        names
    );

    let cell = params
        .iter()
        .find(|vc| vc.id.member == "cases")
        .unwrap_or_else(|| {
            panic!(
                "MultiCaseResult missing required param 'cases'; got: {:?}",
                names
            )
        });

    assert_eq!(
        cell.cell_type,
        Type::Map(
            Box::new(Type::String),
            Box::new(Type::StructureRef("ElasticResult".to_string())),
        ),
        "MultiCaseResult.cases should be Map<String, ElasticResult>, got {:?}",
        cell.cell_type
    );

    // No default — every instance must be produced by solve_load_cases
    assert!(
        cell.default_expr.is_none(),
        "MultiCaseResult.cases should have no default_expr (solver-only-produced), \
         but got: {:?}",
        cell.default_expr
    );
}

// ─── LoadCase conformance enforcement (task-ζ tighten) ───────────────────────
//
// These tests pin the compile-time conformance enforcement activated by
// task-ζ: tightening `LoadCase.loads : List<Real>` → `List<Load>` and
// `LoadCase.supports : List<Real>` → `List<Support>`.
//
// RED state (current stdlib, still `List<Real>`): the three NEGATIVE
// assertions below FAIL because the conformance walker silently skips
// `List<Real>` slots (Real is not a trait).  After the impl step tightens
// the field types, the walker recurses into the list literals and all three
// NEGATIVE tests turn GREEN.  The POSITIVE guard passes on both old and new
// stdlib.

/// NEGATIVE-loads: bare numeric literals in `loads` must emit
/// `TypeNotConformingToTrait` once `loads : List<Load>` is enforced.
///
/// RED on current `List<Real>` stdlib: the conformance walker skips scalar
/// slots whose param type is not a trait, emitting 0 conformance diagnostics.
/// After ζ: walker recurses into the `List<Load>` literal, the bare-int
/// arm (conformance/mod.rs:931, no call-name) fires for each element.
#[test]
fn loadcase_bare_numeric_in_loads_emits_type_not_conforming() {
    let source = r#"
structure def NegativeLoadsFixture {
    let c = LoadCase(name: "x", loads: [1, 2, 3], supports: [FixedSupport(target: "r")])
}
"#;
    let parsed = parse_with_stdlib(
        source,
        ModulePath::from_dotted("test.loadcase_neg_loads").expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "NegativeLoadsFixture should parse without errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let conformance_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();
    assert!(
        !conformance_errors.is_empty(),
        "expected ≥1 TypeNotConformingToTrait for bare numeric literals in \
         LoadCase.loads (after ζ: List<Load> enforces conformance); got 0. \
         diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// NEGATIVE-supports: bare numeric literals in `supports` must emit
/// `TypeNotConformingToTrait` once `supports : List<Support>` is enforced.
///
/// RED on current `List<Real>` stdlib (same reasoning as NEGATIVE-loads).
#[test]
fn loadcase_bare_numeric_in_supports_emits_type_not_conforming() {
    let source = r#"
structure def NegativeSupportsFixture {
    let c = LoadCase(name: "x", loads: [PointLoad(point: "a", force: 1.0)], supports: [4, 5, 6])
}
"#;
    let parsed = parse_with_stdlib(
        source,
        ModulePath::from_dotted("test.loadcase_neg_supports").expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "NegativeSupportsFixture should parse without errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let conformance_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();
    assert!(
        !conformance_errors.is_empty(),
        "expected ≥1 TypeNotConformingToTrait for bare numeric literals in \
         LoadCase.supports (after ζ: List<Support> enforces conformance); got 0. \
         diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// NEGATIVE-cross-trait: a `FixedSupport` (which conforms to `Support`, not
/// `Load`) in the `loads` list must emit `TypeNotConformingToTrait`.
///
/// Exercises the `StructureRef → satisfies_trait_bound(["Support"], "Load") = false`
/// emit arm (conformance/mod.rs:539).  RED on current `List<Real>` stdlib.
#[test]
fn loadcase_cross_trait_in_loads_emits_type_not_conforming() {
    let source = r#"
structure def CrossTraitFixture {
    let c = LoadCase(name: "x", loads: [FixedSupport(target: "r")], supports: [FixedSupport(target: "r")])
}
"#;
    let parsed = parse_with_stdlib(
        source,
        ModulePath::from_dotted("test.loadcase_cross_trait").expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "CrossTraitFixture should parse without errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let conformance_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();
    assert!(
        !conformance_errors.is_empty(),
        "expected ≥1 TypeNotConformingToTrait for FixedSupport (Support, not Load) \
         in LoadCase.loads (after ζ: List<Load> rejects cross-trait conformers); \
         got 0. diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// POSITIVE guard: typed conformers (`PointLoad`, `Gravity` in `loads`;
/// `FixedSupport` in `supports`) must emit ZERO `TypeNotConformingToTrait`
/// diagnostics on both old (`List<Real>`) and new (`List<Load>`/`List<Support>`)
/// stdlib.  Passes on both sides of the ζ-tighten.
#[test]
fn loadcase_typed_conformers_emit_no_type_not_conforming() {
    let source = r#"
structure def PositiveConformanceFixture {
    let c = LoadCase(
        name: "x",
        loads: [PointLoad(point: "a", force: 1.0), Gravity(magnitude: STANDARD_GRAVITY())],
        supports: [FixedSupport(target: "r")]
    )
}
"#;
    let parsed = parse_with_stdlib(
        source,
        ModulePath::from_dotted("test.loadcase_positive_conformance").expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "PositiveConformanceFixture should parse without errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let conformance_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();
    assert!(
        conformance_errors.is_empty(),
        "typed Load/Support conformers in LoadCase.loads/supports must NOT emit \
         TypeNotConformingToTrait; got: {:?}",
        conformance_errors
    );
}

// ─── Task 4870: `body : Solid` overload of solve_load_cases ───────────────────
//
// A NEW arity-4 overload
//   solve_load_cases(material : ConstitutiveLaw, body : Solid,
//                    cases : List<LoadCase>, options : ElasticOptions = ElasticOptions())
// replaces the three scalar dims (length/width/height) of the pre-existing
// arity-6 dims overload with a single `body : Solid` geometry input that becomes
// the shared mesh-realization identity — all cases sharing `body` share ONE
// realized tet VolumeMesh. `Solid` lowers to `reify_core::Type::Geometry`
// (type_resolution.rs), so the body param flows to the shared
// `solver::multi_case` trampoline via `realization_inputs` (a geometry
// realization ValueRef) rather than the cache-key `value_inputs` list —
// `compute_value_input_for_ref` excludes `Type::Geometry` refs from the cache
// key. The compiler-observable evidence of "lowers to a solver::multi_case
// ComputeNode whose body arg is a geometry realization ValueRef" is (a)
// `optimized_target == Some("solver::multi_case")` and (b) the body param being
// `Type::Geometry`; the eval-time shared-realization behavior is pinned by the
// kernel-enabled e2e in reify-eval (step-11).

/// Locate the `body : Solid` overload: the arity-4 `solve_load_cases` whose
/// second parameter lowers to `Type::Geometry`. The pre-existing dims overload
/// is arity 6 (`[material, length, width, height, cases, options]`), so "arity 4
/// with `Type::Geometry` at param[1]" uniquely identifies the new body overload
/// without depending on declaration order.
///
/// Panics if not found — the expected RED failure mode for step-7 (no body
/// overload exists yet); step-8 adds the declaration.
fn find_body_overload() -> &'static CompiledFunction {
    let module = load_stdlib_module();
    module
        .functions
        .iter()
        .find(|f| {
            f.name == "solve_load_cases"
                && f.params.len() == 4
                && matches!(f.params.get(1), Some((_, Type::Geometry)))
        })
        .unwrap_or_else(|| {
            panic!(
                "body-arg overload solve_load_cases(material, body: Solid, cases, options) \
                 not found in std/fea/multi_case; available solve_load_cases param arities: {:?}",
                module
                    .functions
                    .iter()
                    .filter(|f| f.name == "solve_load_cases")
                    .map(|f| f.params.len())
                    .collect::<Vec<_>>()
            )
        })
}

/// Pin: the body overload's signature and `@optimized` target.
///
/// Asserts the exact param names/order [material, body, cases, options] and the
/// three type-carrying params — `material : ConstitutiveLaw` (TraitObject),
/// `body : Solid` (Type::Geometry), `cases : List<LoadCase>` (List of
/// StructureRef) — plus `optimized_target == Some("solver::multi_case")` (the
/// shared trampoline branches on `value_inputs[1] == GeometryHandle`).
///
/// RED before step-8 (`find_body_overload` panics: no arity-4 overload exists).
#[test]
fn solve_load_cases_body_overload_signature_and_optimized_target() {
    let f = find_body_overload();

    assert_eq!(
        f.optimized_target,
        Some("solver::multi_case".to_string()),
        "body overload must be annotated @optimized(\"solver::multi_case\") so the body \
         arg lowers to a solver::multi_case ComputeNode (shared trampoline)"
    );

    let names: Vec<&str> = f.params.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["material", "body", "cases", "options"],
        "body overload param names/order must be [material, body, cases, options]"
    );

    assert_eq!(
        f.params[0].1,
        Type::TraitObject("ConstitutiveLaw".to_string()),
        "material param must be TraitObject(\"ConstitutiveLaw\"), got {:?}",
        f.params[0].1
    );
    // The body param is `Type::Geometry` — the discriminator that (a) excludes it
    // from the cache-key value_inputs list and (b) makes it a realization ValueRef
    // the trampoline reads from realization_inputs and forwards to every case.
    assert_eq!(
        f.params[1].1,
        Type::Geometry,
        "body param must lower to Type::Geometry (the `Solid` surface alias), got {:?}",
        f.params[1].1
    );
    // cases : List<LoadCase> — LoadCase is a structure, so the element lowers to
    // StructureRef (mirrors MultiCaseResult.cases' StructureRef(\"ElasticResult\")).
    assert_eq!(
        f.params[2].1,
        Type::List(Box::new(Type::StructureRef("LoadCase".to_string()))),
        "cases param must be List<StructureRef(\"LoadCase\")>, got {:?}",
        f.params[2].1
    );
}

/// Caller-compile positive: a design binding `let body = box(...)` and calling
/// `solve_load_cases(material, body, [lc1, lc2], ElasticOptions())` must compile
/// with ZERO Error diagnostics — the 4-arg call resolves unambiguously to the
/// new arity-4 body overload (the arity-6 dims overload needs ≥5 positional
/// args), and the geometry `body` let type-checks against `body : Solid`.
///
/// RED before step-8: no arity-4 overload exists, so overload resolution fails
/// (arity mismatch / `body` geometry ≠ `length : Length`) and emits an Error.
#[test]
fn solve_load_cases_body_arg_resolves_and_compiles_clean() {
    let source = r#"
structure def FEABodyMultiCase {
    let body = box(1000mm, 100mm, 100mm)
    let result = solve_load_cases(
        Steel_AISI_1045(),
        body,
        [
            LoadCase(name: "operating", loads: [PointLoad(point: "tip", force: 1000.0)], supports: [FixedSupport(target: "root")]),
            LoadCase(name: "overload", loads: [PointLoad(point: "tip", force: 2000.0)], supports: [FixedSupport(target: "root")])
        ],
        ElasticOptions()
    )
}
"#;
    let parsed = parse_with_stdlib(
        source,
        ModulePath::from_dotted("test.solve_load_cases_body").expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "FEABodyMultiCase should parse without errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics for the body-arg solve_load_cases call \
         (must resolve to the arity-4 `body : Solid` overload); got {:?}",
        errors
    );
}
