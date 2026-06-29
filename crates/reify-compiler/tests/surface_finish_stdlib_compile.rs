//! Tests for `crates/reify-compiler/stdlib/surface_finish.ri` —
//! `std.surface_finish` module: Coating/Treatment structures,
//! CoatingProcess/FinishProcess/TreatmentProcess enums,
//! SurfaceTreated trait, and ArealCostRate type alias.
//!
//! PRD reference: docs/prds/v0_6/surface-finish-functional.md task α.
//!
//! Contract coverage:
//! (a) load-clean: zero Error diagnostics in std/surface_finish
//! (b) vocabulary symbols present — Coating + Treatment structures;
//!     CoatingProcess, FinishProcess, TreatmentProcess enums;
//!     SurfaceTreated trait (all members defaulted → additive conformance)
//! (c) ArealCostRate alias resolves via compile-a-source assertion
//! (d) representative producer `Part : SurfaceTreated` compiles clean
//!     (with Coating, FinishProcess, Treatment, finishing_cost .sum)
//! (e) bare `structure def Bare : SurfaceTreated {}` compiles clean
//!     (all SurfaceTreated members have defaults — no required_members)
//!
//! RED before step-2 lands the module: `load_stdlib_module()` panics when
//! `std/surface_finish` is absent from the production stdlib.

use reify_compiler::{stdlib_loader, EntityKind};
use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/surface_finish` CompiledModule from the production stdlib
/// loader.  Panics if absent — expected RED failure until step-2 registers it.
fn load_stdlib_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/surface_finish")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/surface_finish module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up an enum definition by name within the `std/surface_finish` module.
fn find_enum(name: &str) -> &'static reify_ir::EnumDef {
    let module = load_stdlib_module();
    module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `enum {}` in std/surface_finish, got enum_defs: {:?}",
                name,
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        })
}

/// Look up a trait by name within the `std/surface_finish` module.
fn find_trait(name: &str) -> &'static reify_compiler::CompiledTrait {
    let module = load_stdlib_module();
    module
        .trait_defs
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "std/surface_finish should contain trait '{}'; found: {:?}",
                name,
                module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        })
}

// ─── (a) load-clean ──────────────────────────────────────────────────────────

/// The std/surface_finish module must load through the production stdlib path
/// with zero error-severity diagnostics.
#[test]
fn std_surface_finish_loads_with_no_errors() {
    let module = load_stdlib_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in surface_finish.ri: {:?}",
        errors
    );
}

// ─── (b) vocabulary: enums ───────────────────────────────────────────────────

/// CoatingProcess must have exactly the six variants in declaration order.
#[test]
fn coating_process_enum_has_expected_variants() {
    let e = find_enum("CoatingProcess");
    assert_eq!(
        e.variants,
        vec![
            "Uncoated".to_string(),
            "Anodize".to_string(),
            "PowderCoat".to_string(),
            "Electroplate".to_string(),
            "Passivate".to_string(),
            "Paint".to_string(),
        ],
        "CoatingProcess variants must match the declared order; got: {:?}",
        e.variants
    );
}

/// FinishProcess must have exactly the seven variants in declaration order.
#[test]
fn finish_process_enum_has_expected_variants() {
    let e = find_enum("FinishProcess");
    assert_eq!(
        e.variants,
        vec![
            "AsMachined".to_string(),
            "Ground".to_string(),
            "Polished".to_string(),
            "Lapped".to_string(),
            "BeadBlasted".to_string(),
            "Brushed".to_string(),
            "AsCast".to_string(),
        ],
        "FinishProcess variants must match the declared order; got: {:?}",
        e.variants
    );
}

/// TreatmentProcess must have exactly the six variants in declaration order.
#[test]
fn treatment_process_enum_has_expected_variants() {
    let e = find_enum("TreatmentProcess");
    assert_eq!(
        e.variants,
        vec![
            "Anneal".to_string(),
            "Temper".to_string(),
            "CaseHarden".to_string(),
            "Nitride".to_string(),
            "Carburize".to_string(),
            "ShotPeen".to_string(),
        ],
        "TreatmentProcess variants must match the declared order; got: {:?}",
        e.variants
    );
}

// ─── (b) vocabulary: structures ──────────────────────────────────────────────

/// `structure def Coating` must exist as a Structure template.
#[test]
fn coating_structure_exists() {
    let module = load_stdlib_module();
    let found = module
        .templates
        .iter()
        .any(|t| t.name == "Coating" && t.entity_kind == EntityKind::Structure);
    assert!(
        found,
        "expected `structure def Coating` in std/surface_finish; got templates: {:?}",
        module
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

/// `structure def Treatment` must exist as a Structure template.
#[test]
fn treatment_structure_exists() {
    let module = load_stdlib_module();
    let found = module
        .templates
        .iter()
        .any(|t| t.name == "Treatment" && t.entity_kind == EntityKind::Structure);
    assert!(
        found,
        "expected `structure def Treatment` in std/surface_finish; got templates: {:?}",
        module
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

// ─── (b) vocabulary: SurfaceTreated trait ────────────────────────────────────

/// SurfaceTreated must exist with no refinements and no required_members
/// (all three params — coating, finish_process, treatment — are defaulted,
/// so conformance is additive: no conformer needs to supply them).
/// The defaulted params live in `trait_def.defaults`.
#[test]
fn surface_treated_trait_is_additive() {
    let t = find_trait("SurfaceTreated");

    // No trait refinements (not `: Process` or similar).
    assert!(
        t.refinements.is_empty(),
        "SurfaceTreated should have no trait refinements; got: {:?}",
        t.refinements
    );

    // All three params have defaults → required_members is empty.
    assert!(
        t.required_members.is_empty(),
        "SurfaceTreated should have 0 required_members (all params are defaulted); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Exactly 3 defaults: coating, finish_process, treatment.
    assert_eq!(
        t.defaults.len(),
        3,
        "SurfaceTreated should have exactly 3 defaulted members \
         (coating, finish_process, treatment); got {} — {:?}",
        t.defaults.len(),
        t.defaults.iter().map(|d| &d.name).collect::<Vec<_>>()
    );

    // name is Option<String>; check via as_deref().
    for expected in &["coating", "finish_process", "treatment"] {
        assert!(
            t.defaults.iter().any(|d| d.name.as_deref() == Some(expected)),
            "SurfaceTreated defaults must include '{}'; got: {:?}",
            expected,
            t.defaults.iter().map(|d| &d.name).collect::<Vec<_>>()
        );
    }
}

// ─── (c) ArealCostRate alias resolves ────────────────────────────────────────

/// Verify that `ArealCostRate` resolves as the Money/Area dimension:
/// `param r : ArealCostRate = 0USD/m^2` must compile without errors.
/// Uses a compile-a-source assertion (more stable than alias-registry
/// introspection for a derived/binary dim-op alias).
#[test]
fn areal_cost_rate_alias_resolves_in_user_source() {
    let source = r#"
structure def RateHolder {
    param r : ArealCostRate = 0USD/m^2
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "ArealCostRate param = 0USD/m^2 should compile without errors; got: {:?}",
        errors
    );
}

// ─── (d) representative producer Part : SurfaceTreated ───────────────────────

/// A Part carrying Coating (Anodize, 15um, Color, spec, cost, density),
/// FinishProcess.Polished, Treatment (Temper, T6, cost), and a finishing_cost
/// via `[…].sum` must compile with zero Error diagnostics.
/// Color is resolved from the prelude (std.materials.appearance).
#[test]
fn surface_treated_producer_compiles_clean() {
    let source = r#"
structure def Part : SurfaceTreated {
    param coating : Coating = Coating(
        process: CoatingProcess.Anodize,
        thickness: 15um,
        color: Color(named: "RAL9005", r: 0.05, g: 0.05, b: 0.06),
        spec: "MIL-A-8625 Type II",
        process_cost: 12USD,
        coat_density: 3000kg/m^3
    )
    param finish_process : FinishProcess = FinishProcess.Polished
    param treatment : Treatment = Treatment(
        process: TreatmentProcess.Temper,
        spec: "T6",
        cost: 4USD
    )
    let finishing_cost : Money = [coating.process_cost, treatment.cost].sum
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Part : SurfaceTreated producer source should compile clean; got: {:?}",
        errors
    );
}

// ─── (e) additive conformance (boundary B2) ──────────────────────────────────

/// A bare `structure def Bare : SurfaceTreated {}` must compile with zero
/// Error diagnostics — all SurfaceTreated members are defaulted
/// (Uncoated/AsMachined/Anneal sentinels), so conformance adds no new
/// required members and breaks no existing body.
#[test]
fn bare_surface_treated_conformer_compiles_clean() {
    let source = r#"
structure def Bare : SurfaceTreated {}
"#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "bare `SurfaceTreated` conformer should compile clean (additive conformance); got: {:?}",
        errors
    );
}
