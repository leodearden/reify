//! Auto-measurement check-time pass: `Engine::measure_dfm_rules` (task 4408 γ).
//!
//! Tests the `DFMRule.subject`-driven check-time pass that auto-measures a
//! realized solid's overhang/draft against the process capability and emits
//! `{W,E}_DFM_OVERHANG` / `_DRAFT` / `E_DFM_UNDERCUT` diagnostics.
//!
//! # Structure
//!
//! - **No-kernel C1 no-op** (step-5 / step-6): verifies that when there is no
//!   geometry kernel, `measure_dfm_rules` is a complete no-op — no false
//!   violation, no DFM diagnostic.  Uses `check_source_with_stdlib` (a
//!   no-kernel `SimpleConstraintChecker` engine).
//!
//! - **OCCT-gated overhang tests** (step-7 / step-8): build a solid with a face
//!   dipping below the build plane beyond the process limit → expect
//!   `{W,E}_DFM_OVERHANG`; a conforming solid → expect nothing.
//!   Gated on `reify_kernel_occt::OCCT_AVAILABLE` (early-return when absent).
//!
//! - **OCCT-gated draft/undercut tests** (step-9 / step-10): build a wall with
//!   insufficient draft → expect `{W,E}_DFM_DRAFT`; a re-entrant wall →
//!   expect `E_DFM_UNDERCUT`; a conforming part → expect nothing.
//!   Gated on `reify_kernel_occt::OCCT_AVAILABLE`.

use reify_test_support::check_source_with_stdlib;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Assert that no diagnostic whose message contains `substr` is present.
fn assert_no_dfm_diagnostic(result: &reify_eval::CheckResult, substr: &str) {
    let dfm_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(substr))
        .collect();
    assert!(
        dfm_diags.is_empty(),
        "expected no diagnostic containing {:?}, but got: {:#?}",
        substr,
        dfm_diags
    );
}

/// Assert that exactly `count` diagnostics containing `substr` are present.
fn assert_dfm_diagnostic_count(result: &reify_eval::CheckResult, substr: &str, count: usize) {
    let dfm_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains(substr))
        .collect();
    assert_eq!(
        dfm_diags.len(),
        count,
        "expected {count} diagnostic(s) containing {:?}, but got {}: {:#?}",
        substr,
        dfm_diags.len(),
        dfm_diags
    );
}

// ── step-5 / step-6: no-kernel C1 no-op ──────────────────────────────────────

/// A DFMRule bound to an Adding process and a Solid part — but run through the
/// no-kernel `SimpleConstraintChecker` engine (via `check_source_with_stdlib`).
///
/// C1 invariant: no kernel → no live geometry handle → `measure_dfm_rules` is
/// a complete no-op.  The result must contain NO `_DFM_OVERHANG`,
/// `_DFM_DRAFT`, or `_DFM_UNDERCUT` diagnostics, and no Violated constraint.
///
/// RED (step-5): passes trivially because `measure_dfm_rules` is not yet called
/// from `check()`.  GREEN (step-6): continues to pass because the no-kernel C1
/// guard short-circuits the pass.
#[test]
fn c1_no_kernel_no_dfm_diagnostics() {
    // A conforming Adding process (all required params supplied).
    // build_volume is a Solid param — accepted at compile time; in a no-kernel
    // engine it evaluates to Value::Undef (no geometry realization).
    let source = r#"
import std.process

structure def FDMPrinter : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 45deg
}

// The DFMRule conformer — all four required params.
// subject binds to a Solid geometry call; without a kernel it stays Undef.
structure def OverhangCheck : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDMPrinter()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let result = check_source_with_stdlib(source);

    // C1: no false violation — none of the DFM diagnostic codes present.
    assert_no_dfm_diagnostic(&result, "_DFM_OVERHANG");
    assert_no_dfm_diagnostic(&result, "_DFM_DRAFT");
    assert_no_dfm_diagnostic(&result, "_DFM_UNDERCUT");

    // No constraint entry should be Violated either.
    let violated: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == reify_ir::Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "C1: no-kernel check should produce no Violated constraints; got: {:#?}",
        violated
    );
}

// ── step-7 / step-8: OCCT-gated overhang tests ───────────────────────────────

/// Build an OCCT engine for integration tests.
///
/// Mirrors `tests/achieved_repr_tol.rs::make_occt_engine`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Compile `source` with stdlib, panicking on compile errors.
///
/// Routes through `reify_compiler::parse_with_stdlib` so that prelude-stdlib
/// enum names (e.g. `DFMSeverity`) participate in `EnumAccess` disambiguation.
/// Using `reify_syntax::parse` directly would miss the enum namespace context
/// and produce "unresolved name: DFMSeverity" errors.
fn compile_with_stdlib(source: &str) -> reify_compiler::CompiledModule {
    reify_test_support::parse_and_compile_with_stdlib(source)
}

/// A box bottom face (normal -Z) dips 90° past the build plane.
/// `max_overhang_angle = 0deg` means any downward-pointing face is a violation.
/// Exactly one `W_DFM_OVERHANG` must be emitted (Warning severity).
#[test]
fn overhang_warning_rule_emits_w_dfm_overhang() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping overhang_warning_rule_emits_w_dfm_overhang: OCCT not available");
        return;
    }

    // A box has a bottom face (normal = -Z) that dips 90 deg past the build
    // plane.  max_overhang_angle = 0 deg means ANY downward-pointing face
    // triggers the rule.  This gives a definite overhang violation.
    let source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def Part {
    let body = box(50mm, 30mm, 20mm)
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    // Expect exactly one W_DFM_OVERHANG (Warning severity).
    assert_dfm_diagnostic_count(&result, "W_DFM_OVERHANG", 1);
    assert_no_dfm_diagnostic(&result, "E_DFM_OVERHANG");
}

/// Same setup but the rule uses `DFMSeverity.Error` → `E_DFM_OVERHANG`.
#[test]
fn overhang_error_rule_emits_e_dfm_overhang() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping overhang_error_rule_emits_e_dfm_overhang: OCCT not available");
        return;
    }

    let source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRuleError : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Error
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "E_DFM_OVERHANG", 1);
    assert_no_dfm_diagnostic(&result, "W_DFM_OVERHANG");
}

/// A box with a generous max_overhang_angle (89 deg, near 90 deg horizontal
/// limit) should have no downward face beyond the limit → no DFM_OVERHANG.
///
/// Note: unsupported_overhang_faces checks faces whose normal dips more than
/// max_overhang_angle below horizontal.  With max_overhang_angle = 89deg,
/// only faces dipping MORE than 89 deg are violations.  The bottom face of a
/// box dips 90 deg, so it would still violate even at 89 deg.
///
/// Use max_overhang_angle = 90deg (π/2) — this is the boundary: only faces
/// pointing STRAIGHT down (dip = 90 deg) would be exactly at the limit.
/// The selector validates max_overhang_angle ∈ [0, π/2], so 90deg is valid.
/// A box bottom face dips exactly 90 deg. With threshold sin(90°)=1, the
/// check is `n·ẑ < -1` which is never true (n·ẑ >= -1 always) → no violation.
#[test]
fn overhang_conforming_part_no_dfm_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping overhang_conforming_part_no_dfm_diagnostic: OCCT not available");
        return;
    }

    let source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 90deg
}

structure def OverhangRuleOK : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_no_dfm_diagnostic(&result, "_DFM_OVERHANG");
}

// ── step-9 / step-10: OCCT-gated draft/undercut tests ────────────────────────

/// A `Forming` conformer with `draft_angle = 45deg`.  The `subject` is a
/// simple box.  Box walls are vertical (draft angle = 0 deg from vertical,
/// or equivalently the wall normal is horizontal).  Since the wall-face
/// normal is perpendicular to pull_dir (ẑ), draft = π/2 - acos(n·ẑ).
/// For a vertical wall, n·ẑ = 0, so draft = π/2 - π/2 = 0 deg.
/// The process requires 45 deg draft → violation.
///
/// draft_angle = 45 deg means `signed_min_draft < 45deg` is true for a box
/// (whose walls have 0 deg draft) → emits `W_DFM_DRAFT`.
#[test]
fn draft_warning_rule_emits_w_dfm_draft() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping draft_warning_rule_emits_w_dfm_draft: OCCT not available");
        return;
    }

    let source = r#"
import std.process

structure def Stamping : Forming {
    param duration         : Time   = 30s
    param cost             : Money  = 2USD
    param min_bend_radius  : Length = 2mm
    param max_draw_depth   : Length = 50mm
    param draft_angle      : Angle  = 45deg
}

structure def DraftRule : DFMRule {
    param rule_name  : String      = "draft-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Stamping()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "W_DFM_DRAFT", 1);
    assert_no_dfm_diagnostic(&result, "E_DFM_DRAFT");
}

/// An `Error`-severity Forming rule with `draft_angle = 45deg` on a box
/// (0 deg draft walls) → emits `E_DFM_DRAFT`.
#[test]
fn draft_error_rule_emits_e_dfm_draft() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping draft_error_rule_emits_e_dfm_draft: OCCT not available");
        return;
    }

    let source = r#"
import std.process

structure def Stamping : Forming {
    param duration         : Time   = 30s
    param cost             : Money  = 2USD
    param min_bend_radius  : Length = 2mm
    param max_draw_depth   : Length = 50mm
    param draft_angle      : Angle  = 45deg
}

structure def DraftRuleError : DFMRule {
    param rule_name  : String      = "draft-check"
    param severity   : DFMSeverity = DFMSeverity.Error
    param applies_to : Process     = Stamping()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_dfm_diagnostic_count(&result, "E_DFM_DRAFT", 1);
    assert_no_dfm_diagnostic(&result, "W_DFM_DRAFT");
}

/// A sphere has curved walls; the minimum draft angle over wall-window faces
/// may be negative → `E_DFM_UNDERCUT` (always Error regardless of rule severity).
///
/// A sphere centered at origin has surface normals pointing radially outward.
/// Faces near the equator have normals roughly horizontal (|n·ẑ| small).
/// Faces on the lower hemisphere have n·ẑ < 0 → re-entrant → undercut.
/// min_draft_angle will detect `has_undercut = true` → `E_DFM_UNDERCUT`.
#[test]
fn undercut_emits_e_dfm_undercut() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping undercut_emits_e_dfm_undercut: OCCT not available");
        return;
    }

    // Sphere has lower-hemisphere faces with n·ẑ < 0 → undercut.
    let source = r#"
import std.process

structure def Molding : Forming {
    param duration         : Time   = 60s
    param cost             : Money  = 3USD
    param min_bend_radius  : Length = 1mm
    param max_draw_depth   : Length = 100mm
    param draft_angle      : Angle  = 1deg
}

structure def UndercutRule : DFMRule {
    param rule_name  : String      = "undercut-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Molding()
    param subject    : Solid       = sphere(50mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    // E_DFM_UNDERCUT is always Error regardless of rule severity.
    assert_dfm_diagnostic_count(&result, "E_DFM_UNDERCUT", 1);
}

/// A box with `draft_angle = 0deg` (no draft required): box walls have 0 deg
/// draft which equals the requirement → no draft violation.
/// A box has no re-entrant walls → no undercut.
#[test]
fn draft_conforming_part_no_dfm_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping draft_conforming_part_no_dfm_diagnostic: OCCT not available");
        return;
    }

    let source = r#"
import std.process

structure def Stamping : Forming {
    param duration         : Time   = 30s
    param cost             : Money  = 2USD
    param min_bend_radius  : Length = 2mm
    param max_draw_depth   : Length = 50mm
    param draft_angle      : Angle  = 0deg
}

structure def DraftRuleOK : DFMRule {
    param rule_name  : String      = "draft-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = Stamping()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    assert_no_dfm_diagnostic(&result, "_DFM_DRAFT");
    assert_no_dfm_diagnostic(&result, "_DFM_UNDERCUT");
}

// ── dedup: definition + instantiation → exactly one diagnostic ───────────────

/// A DFMRule that is both defined at top level (source A: template iteration)
/// and instantiated as a sub-component of Part (source B: instance values)
/// must emit exactly ONE W_DFM_OVERHANG — not two.
///
/// The dedup guard in `measure_dfm_rules` keys on `(kind, subject_handle)` and
/// retains only the first occurrence, so double-emission is prevented even if
/// both discovery paths resolve to live handles for the same rule.
#[test]
fn dedup_rule_defined_and_instantiated_emits_one_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping dedup_rule_defined_and_instantiated_emits_one_diagnostic: OCCT not available"
        );
        return;
    }

    // OverhangRule is a top-level template (source A path) AND is instantiated
    // inside Part (source B path).  Both discovery sources could find this rule.
    let source = r#"
import std.process

structure def FDM : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 5USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 0deg
}

structure def OverhangRule : DFMRule {
    param rule_name  : String      = "overhang-check"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = FDM()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}

structure def Part {
    let body = box(50mm, 30mm, 20mm)
    let rule = OverhangRule()
}
"#;

    let compiled = compile_with_stdlib(source);
    let mut engine = make_occt_engine();
    engine.build(&compiled, reify_ir::ExportFormat::Step);
    let result = engine.check(&compiled);

    // Exactly one W_DFM_OVERHANG regardless of how many discovery sources fire.
    assert_dfm_diagnostic_count(&result, "W_DFM_OVERHANG", 1);
}
