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
