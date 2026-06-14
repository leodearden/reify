//! GD&T check-time legality diagnostics (task 4475 β).
//!
//! Integration tests for:
//! - C1: `Engine::enumerate_gdt_callouts` — the shared callout enumerator
//! - C2: `Engine::check` + `check_gdt_legality` — the rule-table legality pass
//! - C3: `Engine::run_gdt_check_passes` — the shared pub aggregation seam (task 4589)
//!
//! Tests are added incrementally (steps 1–8 for 4475, step-1/2 for 4589);
//! each step adds RED tests that fail until the corresponding impl step makes them pass.

#[allow(unused_imports)]
use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── C1 enumerator contract (step-1 RED / step-2 GREEN) ────────────────────────

/// Parse and compile the given source with stdlib; return (module, values) pair.
fn eval_with_stdlib(source: &str) -> (reify_compiler::CompiledModule, reify_ir::ValueMap) {
    let module = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);
    (module, result.values)
}

/// C1-A: `enumerate_gdt_callouts` returns exactly the GeometricTolerance-conforming
/// instances in declaration order; a non-GDT instance is excluded.
///
/// Fixture declares (in order):
///   1. A `DimensionalTolerance` (non-GDT — does NOT conform to GeometricTolerance).
///   2. A `Flatness(material_condition: MaterialCondition.MMC, ...)` (GDT — Form family).
///
/// Expected: exactly 1 callout returned (the Flatness), in that slot; the
/// DimensionalTolerance is excluded.
#[test]
fn c1_enumerator_returns_gdt_instances_and_excludes_non_gdt() {
    const SOURCE: &str = r#"
structure def Fixture {
    let dim_tol = DimensionalTolerance(
        nominal: 10mm,
        upper_deviation: 0.05mm,
        lower_deviation: -0.05mm
    )
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    // Exactly one callout (the Flatness); DimensionalTolerance excluded.
    assert_eq!(
        callouts.len(),
        1,
        "expected exactly 1 GDT callout (Flatness); got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );

    let callout = &callouts[0];

    // type_name must be "Flatness"
    assert_eq!(
        callout.type_name, "Flatness",
        "expected type_name=Flatness, got {:?}",
        callout.type_name
    );

    // material_condition must be Some("MMC")
    assert_eq!(
        callout.material_condition.as_deref(),
        Some("MMC"),
        "expected material_condition=Some(MMC), got {:?}",
        callout.material_condition
    );

    // The instantiation span must be non-empty (not a prelude synthetic span).
    assert!(
        !callout.span.is_empty(),
        "expected non-empty instantiation span, got {:?}",
        callout.span
    );
}

/// C1-B: when a module contains no GeometricTolerance-conforming instances,
/// `enumerate_gdt_callouts` returns an empty vector.
#[test]
fn c1_enumerator_returns_empty_for_non_gdt_module() {
    const SOURCE: &str = r#"
structure def NoGdt {
    let dim_tol = DimensionalTolerance(
        nominal: 5mm,
        upper_deviation: 0.01mm,
        lower_deviation: -0.01mm
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    assert!(
        callouts.is_empty(),
        "expected empty callouts for non-GDT module, got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );
}

/// C1-C: `enumerate_gdt_callouts` returns multiple callouts in declaration order.
///
/// Fixture declares two GDT callouts: first a Flatness(RFS), then a Circularity(RFS).
/// Expected: [Flatness, Circularity] in that order.
#[test]
fn c1_enumerator_declaration_order_is_preserved() {
    const SOURCE: &str = r#"
structure def MultiGdt {
    let f = Flatness(
        tolerance_value: 0.1mm,
        feature: box(1mm, 1mm, 1mm)
    )
    let c = Circularity(
        tolerance_value: 0.05mm,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;

    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let callouts = engine.enumerate_gdt_callouts(&module, &values);

    assert_eq!(
        callouts.len(),
        2,
        "expected 2 callouts, got {}: {:?}",
        callouts.len(),
        callouts.iter().map(|c| &c.type_name).collect::<Vec<_>>()
    );
    assert_eq!(callouts[0].type_name, "Flatness");
    assert_eq!(callouts[1].type_name, "Circularity");
    // Both default to RFS
    assert_eq!(callouts[0].material_condition.as_deref(), Some("RFS"));
    assert_eq!(callouts[1].material_condition.as_deref(), Some("RFS"));
}

// ── C2 rule table — RFS-only Form family (step-3 RED / step-4 GREEN) ──────────

/// C2-A: `Flatness(material_condition: MMC, ...)` produces exactly one
/// `GdtIllegalModifier` error diagnostic; the label span equals the callout's
/// instantiation span (B7 oracle).
///
/// Fails until `check_gdt_legality` is hooked into `Engine::check`.
#[test]
fn c2_flatness_mmc_emits_illegal_modifier_error() {
    const SOURCE: &str = r#"
structure def CheckForm {
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;
    let module = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);

    // Collect only GdtIllegalModifier diagnostics.
    let gdt_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect();

    assert_eq!(
        gdt_errors.len(),
        1,
        "expected exactly 1 GdtIllegalModifier diagnostic; got {}: {:?}",
        gdt_errors.len(),
        gdt_errors
    );

    let diag = gdt_errors[0];
    assert_eq!(
        diag.severity,
        Severity::Error,
        "GdtIllegalModifier must be an error"
    );

    // The label span must be non-empty and must match the callout's instantiation span.
    assert!(
        !diag.labels.is_empty(),
        "GdtIllegalModifier diagnostic must carry at least one label"
    );
    let label_span = diag.labels[0].span;
    assert!(
        !label_span.is_empty(),
        "label span must be non-empty (B7 oracle)"
    );

    // Cross-check: the span must equal what the C1 enumerator reports.
    let callout_span = {
        let eval_result = make_simple_engine().eval(&module);
        let callouts = make_simple_engine().enumerate_gdt_callouts(&module, &eval_result.values);
        assert_eq!(callouts.len(), 1, "expected exactly 1 GDT callout for span cross-check");
        callouts[0].span
    };
    assert_eq!(
        label_span, callout_span,
        "diagnostic label span must equal the C1 callout instantiation span (B7)"
    );
}

/// C2-B: `Flatness(material_condition: RFS, ...)` (or defaulted) produces NO
/// `GdtIllegalModifier` diagnostic — RFS is always legal.
///
/// Fails until `check_gdt_legality` is hooked into `Engine::check`.
#[test]
fn c2_flatness_rfs_is_silent() {
    const SOURCE: &str = r#"
structure def CheckFormRfs {
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;
    let module = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);

    let gdt_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect();

    assert!(
        gdt_errors.is_empty(),
        "Flatness(RFS) must produce no GdtIllegalModifier; got: {:?}",
        gdt_errors
    );
}

// ── C2 rule table — FOS-eligibility / family matrix (step-5 RED / step-6 GREEN) ─

/// Helper: collect all `GdtIllegalModifier` error diagnostics from `Engine::check`.
fn illegal_modifier_diags(source: &str) -> Vec<reify_core::Diagnostic> {
    let module = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);
    result
        .diagnostics
        .into_iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect()
}

/// C2-C: `Position(material_condition: MMC, ...)` is FOS-eligible (Location family,
/// Cylindrical zone by default) — must emit NO `GdtIllegalModifier`.
///
/// Fails until the Location family is classified as FOS-eligible (step-6).
/// (Currently passes because Position falls through the default arm — step-6
/// must keep it passing when it explicitly classifies Position as FOS-eligible.)
#[test]
fn c2_position_mmc_is_legal() {
    let diags = illegal_modifier_diags(r#"
structure def CheckPosition {
    let pos = Position(
        tolerance_value: 0.05mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert!(
        diags.is_empty(),
        "Position(MMC) is FOS-eligible and must produce no GdtIllegalModifier; got: {:?}",
        diags
    );
}

/// C2-D: `StraightnessOfAxis(material_condition: MMC, ...)` is the FOS-axis
/// form variant — MMC-eligible; must emit NO `GdtIllegalModifier`.
///
/// Fails until FormAxis is classified as FOS-eligible (step-6).
#[test]
fn c2_straightness_of_axis_mmc_is_legal() {
    let diags = illegal_modifier_diags(r#"
structure def CheckStraightnessOfAxis {
    let s = StraightnessOfAxis(
        tolerance_value: 0.02mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert!(
        diags.is_empty(),
        "StraightnessOfAxis(MMC) is FOS-eligible and must produce no GdtIllegalModifier; got: {:?}",
        diags
    );
}

/// C2-E: `Parallelism(zone_shape: Cylindrical, material_condition: MMC, ...)` is
/// FOS-eligible (Orientation + Cylindrical zone) — must emit NO `GdtIllegalModifier`.
///
/// Fails until the Orientation/Cylindrical gate is implemented (step-6).
#[test]
fn c2_parallelism_cylindrical_mmc_is_legal() {
    let diags = illegal_modifier_diags(r#"
structure def CheckParallelismCyl {
    let p = Parallelism(
        tolerance_value: 0.03mm,
        material_condition: MaterialCondition.MMC,
        zone_shape: ZoneShape.Cylindrical,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert!(
        diags.is_empty(),
        "Parallelism(Cylindrical, MMC) is FOS-eligible and must produce no GdtIllegalModifier; got: {:?}",
        diags
    );
}

/// C2-F: `Parallelism(material_condition: MMC, ...)` with the default Width zone
/// is NOT FOS-eligible — must emit `GdtIllegalModifier`.
///
/// Fails until the Orientation/Width-zone gating is implemented (step-6).
#[test]
fn c2_parallelism_width_mmc_emits_illegal_modifier() {
    let diags = illegal_modifier_diags(r#"
structure def CheckParallelismWidth {
    let p = Parallelism(
        tolerance_value: 0.03mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert_eq!(
        diags.len(),
        1,
        "Parallelism(Width, MMC) must emit exactly 1 GdtIllegalModifier; got {}: {:?}",
        diags.len(),
        diags
    );
}

/// C2-G: `CircularRunout(material_condition: MMC, datum_refs: ...)` is Runout
/// (RFS-only) — must emit `GdtIllegalModifier`.
///
/// Fails until the Runout family is classified as RFS-only (step-6).
#[test]
fn c2_circular_runout_mmc_emits_illegal_modifier() {
    let diags = illegal_modifier_diags(r#"
structure def CheckCircularRunout {
    let r = CircularRunout(
        tolerance_value: 0.01mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert_eq!(
        diags.len(),
        1,
        "CircularRunout(MMC) must emit exactly 1 GdtIllegalModifier; got {}: {:?}",
        diags.len(),
        diags
    );
}

/// C2-H: `ProfileOfSurface(material_condition: MMC, ...)` is Profile (RFS-only)
/// — must emit `GdtIllegalModifier`.
///
/// Fails until the Profile family is classified as RFS-only (step-6).
#[test]
fn c2_profile_of_surface_mmc_emits_illegal_modifier() {
    let diags = illegal_modifier_diags(r#"
structure def CheckProfileOfSurface {
    let p = ProfileOfSurface(
        tolerance_value: 0.02mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#);
    assert_eq!(
        diags.len(),
        1,
        "ProfileOfSurface(MMC) must emit exactly 1 GdtIllegalModifier; got {}: {:?}",
        diags.len(),
        diags
    );
}

/// C2-I: All-RFS variants of the FOS-eligible families must remain silent.
#[test]
fn c2_fos_eligible_rfs_variants_are_silent() {
    // Position (RFS, Cylindrical default)
    assert!(
        illegal_modifier_diags(r#"
structure def C { let p = Position(tolerance_value: 0.05mm, feature: box(1mm,1mm,1mm), datum_refs: box(1mm,1mm,1mm)) }
"#).is_empty(),
        "Position(RFS) must be silent"
    );

    // StraightnessOfAxis (RFS)
    assert!(
        illegal_modifier_diags(r#"
structure def C { let s = StraightnessOfAxis(tolerance_value: 0.02mm, feature: box(1mm,1mm,1mm)) }
"#).is_empty(),
        "StraightnessOfAxis(RFS) must be silent"
    );

    // Parallelism (RFS, Width default)
    assert!(
        illegal_modifier_diags(r#"
structure def C { let p = Parallelism(tolerance_value: 0.03mm, feature: box(1mm,1mm,1mm), datum_refs: box(1mm,1mm,1mm)) }
"#).is_empty(),
        "Parallelism(RFS) must be silent"
    );

    // CircularRunout (RFS)
    assert!(
        illegal_modifier_diags(r#"
structure def C { let r = CircularRunout(tolerance_value: 0.01mm, feature: box(1mm,1mm,1mm), datum_refs: box(1mm,1mm,1mm)) }
"#).is_empty(),
        "CircularRunout(RFS) must be silent"
    );

    // ProfileOfSurface (RFS)
    assert!(
        illegal_modifier_diags(r#"
structure def C { let p = ProfileOfSurface(tolerance_value: 0.02mm, feature: box(1mm,1mm,1mm)) }
"#).is_empty(),
        "ProfileOfSurface(RFS) must be silent"
    );
}

// ── C2 rule table — removed-in-2018 family (step-7 RED / step-8 GREEN) ────────

/// C2-J: `Concentricity(...)` produces a `GdtRemoved2018` warning (not an error),
/// with a label at the instantiation span, and a message naming the replacements
/// (Position / Profile / Runout).
///
/// Fails until the Removed family rule is implemented (step-8).
#[test]
fn c2_concentricity_emits_removed_2018_warning() {
    const SOURCE: &str = r#"
structure def CheckConcentricity {
    let c = Concentricity(
        tolerance_value: 0.01mm,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#;
    let module = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);

    let removed: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtRemoved2018))
        .collect();

    assert_eq!(
        removed.len(),
        1,
        "Concentricity must emit exactly 1 GdtRemoved2018 diagnostic; got {}: {:?}",
        removed.len(),
        removed
    );

    let diag = removed[0];
    assert_eq!(diag.severity, Severity::Warning, "GdtRemoved2018 must be a warning");

    assert!(
        !diag.labels.is_empty(),
        "GdtRemoved2018 must carry at least one label"
    );
    assert!(
        !diag.labels[0].span.is_empty(),
        "GdtRemoved2018 label span must be non-empty"
    );

    // The message must name at least one replacement characteristic.
    let msg = &diag.message;
    assert!(
        msg.contains("Position") || msg.contains("Profile") || msg.contains("Runout"),
        "GdtRemoved2018 message must name replacement characteristics; got: {:?}",
        msg
    );
}

/// C2-K: `Symmetry(...)` also produces a `GdtRemoved2018` warning.
///
/// Fails until the Removed family rule is implemented (step-8).
#[test]
fn c2_symmetry_emits_removed_2018_warning() {
    const SOURCE: &str = r#"
structure def CheckSymmetry {
    let s = Symmetry(
        tolerance_value: 0.01mm,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#;
    let module = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);

    let removed: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtRemoved2018))
        .collect();

    assert_eq!(
        removed.len(),
        1,
        "Symmetry must emit exactly 1 GdtRemoved2018 diagnostic; got {}: {:?}",
        removed.len(),
        removed
    );
    assert_eq!(removed[0].severity, Severity::Warning, "GdtRemoved2018 must be a warning");
}

/// C2-L: `Concentricity(material_condition: MMC, ...)` emits only the
/// `GdtRemoved2018` warning — NOT an additional `GdtIllegalModifier` error.
///
/// Fails until the Removed family suppresses the GdtIllegalModifier path (step-8).
#[test]
fn c2_concentricity_mmc_yields_only_removed_2018_not_illegal_modifier() {
    const SOURCE: &str = r#"
structure def CheckConcentricityMmc {
    let c = Concentricity(
        tolerance_value: 0.01mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm),
        datum_refs: box(1mm, 1mm, 1mm)
    )
}
"#;
    let module = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.check(&module);

    let removed: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtRemoved2018))
        .collect();
    let illegal: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect();

    assert_eq!(
        removed.len(),
        1,
        "Concentricity(MMC) must emit exactly 1 GdtRemoved2018; got {}: {:?}",
        removed.len(),
        removed
    );
    assert!(
        illegal.is_empty(),
        "Concentricity(MMC) must NOT emit GdtIllegalModifier (removed-2018 takes precedence); got: {:?}",
        illegal
    );
}

// ── C3 run_gdt_check_passes pub seam (task 4589 step-1 RED / step-2 GREEN) ───

/// C3-A: `Engine::run_gdt_check_passes` emits exactly one `GdtIllegalModifier`
/// Error diagnostic for a `Flatness(material_condition: MMC, ...)` module.
///
/// This locks the new pub aggregation seam: the CLI `--purpose` branch calls this
/// method directly (bypassing `Engine::check`), and the test proves it returns
/// the same diagnostic as the existing C2 tests that exercise `check()`.
///
/// Fails to compile until `run_gdt_check_passes` is added (task 4589 step-2 RED).
#[test]
fn run_gdt_check_passes_emits_illegal_modifier_for_flatness_mmc() {
    const SOURCE: &str = r#"
structure def FlatnessMmcSeam {
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        material_condition: MaterialCondition.MMC,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;
    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let diags = engine.run_gdt_check_passes(&module, &values);

    let gdt_errors: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect();

    assert_eq!(
        gdt_errors.len(),
        1,
        "run_gdt_check_passes must emit exactly 1 GdtIllegalModifier for Flatness(MMC); got {}: {:?}",
        gdt_errors.len(),
        gdt_errors
    );
    assert_eq!(
        gdt_errors[0].severity,
        Severity::Error,
        "GdtIllegalModifier from run_gdt_check_passes must be Severity::Error"
    );
}

/// C3-B: `Engine::run_gdt_check_passes` emits ZERO `GdtIllegalModifier` diagnostics
/// for an all-RFS Flatness module — confirming no over-escalation.
///
/// Fails to compile until `run_gdt_check_passes` is added (task 4589 step-2 RED).
#[test]
fn run_gdt_check_passes_silent_for_flatness_rfs() {
    const SOURCE: &str = r#"
structure def FlatnessFrsSeam {
    let flatness = Flatness(
        tolerance_value: 0.1mm,
        feature: box(1mm, 1mm, 1mm)
    )
}
"#;
    let (module, values) = eval_with_stdlib(SOURCE);
    let engine = make_simple_engine();

    let diags = engine.run_gdt_check_passes(&module, &values);

    let gdt_errors: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GdtIllegalModifier))
        .collect();

    assert!(
        gdt_errors.is_empty(),
        "run_gdt_check_passes must emit no GdtIllegalModifier for Flatness(RFS); got: {:?}",
        gdt_errors
    );
}
