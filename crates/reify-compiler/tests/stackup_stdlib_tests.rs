//! Tests for stdlib/stackup.ri — tolerance stack-up authoring types
//! (Distribution, StackupMethod, Contributor, StackupResult).
//!
//! TDD structure mirrors tolerancing_tests.rs: each step adds one
//! structural assertion against the compiled std/stackup module, plus a
//! capstone acceptance test via the production parse_with_stdlib path.
//!
//! Field/variant names are the §4.1/§4.2 seam with the T1 builtins in
//! reify-stdlib/src/stackup.rs (task 3996 DONE @ a416709).

use reify_compiler::*;
use reify_core::*;

// ─── helper ───────────────────────────────────────────────────────────────────

/// Return the `std/stackup` CompiledModule from the production stdlib loader.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/stackup")
        .expect("stdlib should contain std/stackup module")
}

// ─── step-1/3: Distribution + StackupMethod enums ────────────────────────────

/// Step 1: std/stackup compiles without errors and declares the Distribution
/// enum with exactly three variants: Normal, Uniform, Triangular.
/// These variant names are the §4.1 seam with the T1 Value::Enum shapes in
/// reify-stdlib/src/stackup.rs.
#[test]
fn stackup_module_compiles_clean_and_declares_distribution() {
    let module = load_stdlib_module();

    // Zero error-severity diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in stackup.ri: {:?}",
        errors
    );

    // Distribution enum is present with exactly 3 variants.
    let dist = module
        .enum_defs
        .iter()
        .find(|e| e.name == "Distribution")
        .expect("expected 'Distribution' enum in std/stackup");

    assert_eq!(
        dist.variants.len(),
        3,
        "Distribution should have 3 variants, got: {:?}",
        dist.variants
    );
    for variant in &["Normal", "Uniform", "Triangular"] {
        assert!(
            dist.variants.contains(&variant.to_string()),
            "Distribution missing variant '{}', variants: {:?}",
            variant,
            dist.variants
        );
    }
}

// ─── step-3: StackupMethod enum ──────────────────────────────────────────────

/// Step 3: StackupMethod enum is present with exactly three variants:
/// WorstCase, Rss, MonteCarlo — corresponding to the three stack-up
/// computation methods (worst_case / rss / monte_carlo builtins in T1).
#[test]
fn stackup_method_enum_has_three_variants() {
    let module = load_stdlib_module();

    let sm = module
        .enum_defs
        .iter()
        .find(|e| e.name == "StackupMethod")
        .expect("expected 'StackupMethod' enum in std/stackup");

    assert_eq!(
        sm.variants.len(),
        3,
        "StackupMethod should have 3 variants, got: {:?}",
        sm.variants
    );
    for variant in &["WorstCase", "Rss", "MonteCarlo"] {
        assert!(
            sm.variants.contains(&variant.to_string()),
            "StackupMethod missing variant '{}', variants: {:?}",
            variant,
            sm.variants
        );
    }
}

// ─── step-5: Contributor structure params ────────────────────────────────────

/// Step 5: Contributor structure has exactly five Param cells whose member
/// names match the §4.1 T1 Value::Map keys verbatim:
/// {nominal, plus_tol, minus_tol, sign, distribution}.
/// The `distribution` cell's type must be Type::Enum("Distribution").
#[test]
fn contributor_structure_params_align_with_t1_map_keys() {
    let module = load_stdlib_module();

    let contributor = module
        .templates
        .iter()
        .find(|t| t.name == "Contributor")
        .expect("expected 'Contributor' template in std/stackup");

    let param_cells: Vec<_> = contributor
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .collect();

    assert_eq!(
        param_cells.len(),
        5,
        "Contributor should have 5 Param cells \
         (nominal, plus_tol, minus_tol, sign, distribution), got: {:?}",
        param_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );

    let param_names: Vec<&str> = param_cells.iter().map(|vc| vc.id.member.as_str()).collect();
    for name in &["nominal", "plus_tol", "minus_tol", "sign", "distribution"] {
        assert!(
            param_names.contains(name),
            "Contributor missing param '{}', params: {:?}",
            name,
            param_names
        );
    }

    // `distribution` cell must be typed as Enum("Distribution").
    let dist_cell = param_cells
        .iter()
        .find(|vc| vc.id.member == "distribution")
        .expect("expected 'distribution' param cell");
    assert_eq!(
        dist_cell.cell_type,
        Type::Enum("Distribution".to_string()),
        "Contributor.distribution should be Enum(Distribution), got {:?}",
        dist_cell.cell_type
    );
}
