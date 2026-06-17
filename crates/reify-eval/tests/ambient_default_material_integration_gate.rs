//! Eval-layer integration gate — §9 boundary-test table (task #4499/D).
//!
//! # Purpose
//!
//! This file is the EVAL-layer integration gate for the ambient-default-material
//! PRD (`docs/prds/v0_6/ambient-default-material.md`). It pins the §9 boundary
//! rows observable at the eval layer: the end-to-end CI-example gate (row 2 +
//! line-present e2e), the line-removed negative direction, the no-density error
//! (row 7), and the water-gone source-absence guard (row 8).
//!
//! **Model:** sibling type-hygiene λ gate
//! (`crates/reify-eval/tests/type_hygiene_integration_gate.rs`) — same
//! split-by-layer, one-row-per-canonical-test, OCCT-gated-numeric-but-compile-
//! clean-unconditional pattern.
//!
//! # §9 boundary-test table — eval-layer rows + owner cross-reference
//!
//! | Row | Description                                                      | Owner (this file fn)                                               |
//! |-----|------------------------------------------------------------------|--------------------------------------------------------------------|
//! | 1   | parse forms: top-level + purpose-nested both accepted            | `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs` |
//! | 2   | injection fills required param + mass evaluates (e2e positive)   | `ci_example_compiles_clean_and_evaluates_steel` (rows 2 + e2e)   |
//! | 3   | explicit member wins over ambient default (DD3)                  | `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs` |
//! | 4   | file-level + purpose-nested coexist, no cross-scope duplicate    | `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs` |
//! | 5   | duplicate same-scope → exactly one DuplicateAmbientDefault error | `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs` |
//! | 6   | wrong value type → AmbientDefaultTypeMismatch at decl span       | `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs` |
//! | 7   | no ambient + no material → E_DynamicsNoDensity (hard error)      | `row_7_no_ambient_no_material_errors_no_density`                  |
//! | 8   | water-default symbol absent from production source               | `row_8_water_default_symbol_absent_from_source`                   |
//! | e2e | line removed → MissingRequiredMember naming `material`           | `line_removed_errors_naming_mechanism`                            |

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, DimensionVector, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, parse_and_compile_with_stdlib};

// ── Path constant ─────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/ambient_default_material/ambient_default_surface.ri"
);

// ── Box geometry constants (must match example file dims) ─────────────────────

/// Bracket box dims: 50mm × 30mm × 10mm
const BRACKET_W: f64 = 0.05;
const BRACKET_H: f64 = 0.03;
const BRACKET_D: f64 = 0.01;

/// Plate box dims: 20mm × 20mm × 20mm
const PLATE_W: f64 = 0.02;
const PLATE_H: f64 = 0.02;
const PLATE_D: f64 = 0.02;

/// Steel density: 7850 kg/m³
const STEEL_DENSITY: f64 = 7850.0;

// ── Helper: assert a Value::Scalar is MASS-dimensioned at expected SI value ───

fn assert_mass_scalar(actual: Option<&Value>, expected_si: f64, label: &str) {
    let tol = 1e-9_f64;
    match actual {
        Some(Value::Scalar { si_value, dimension }) => {
            assert_eq!(
                *dimension,
                DimensionVector::MASS,
                "{label}: mass cell must be MASS-dimensioned, got dimension {:?}",
                dimension
            );
            assert!(
                (si_value - expected_si).abs() < tol,
                "{label}: expected mass = {expected_si:.6e} kg, got {si_value:.6e} kg \
                 (delta {:.3e}, tol {tol:.0e})",
                (si_value - expected_si).abs()
            );
        }
        other => panic!(
            "{label}: expected Value::Scalar{{dimension: MASS}}, got: {other:?}"
        ),
    }
}

// ── CI-example gate (rows 2 + e2e positive, OCCT-gated) ──────────────────────

/// §9 row 2 + line-present e2e: compile the CI example
/// `examples/ambient_default_material/ambient_default_surface.ri`, assert zero
/// Error-severity diagnostics (unconditional primary signal), then under real
/// OCCT assert:
///
/// - Each Physical structure's `mass` cell (from Physical's
///   `let mass = volume(geometry) * material.density`) == 7850·V_box within 1e-9.
/// - Each structure's `mp` cell (from `let mp = body_mass_props(geometry,
///   material.density)`) is a MassProperties StructureInstance whose `mass`
///   field si_value == 7850·V_box within 1e-9.
///
/// **RED (step-3):** `examples/ambient_default_material/ambient_default_surface.ri`
/// does not exist yet → `read_to_string` panics.
/// **GREEN (step-4):** the example is created.
#[test]
fn ci_example_compiles_clean_and_evaluates_steel() {
    // Read unconditionally: fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/ambient_default_material/ambient_default_surface.ri must exist \
         (task #4499 step-4 creates it)",
    );

    // Compile with stdlib — panics on any Error-severity diagnostic; that's the
    // primary green signal (auto-gated by examples_smoke.rs too).
    let compiled = parse_and_compile_with_stdlib(&source);

    // Skip OCCT-dependent eval assertions on runners without OCCT.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT eval assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // No Error-severity build diagnostics either.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "CI example build must produce no errors under OCCT; got: {build_errors:#?}"
    );

    // ── Bracket (50mm × 30mm × 10mm) assertions ────────────────────────────
    let bracket_v = BRACKET_W * BRACKET_H * BRACKET_D;
    let bracket_expected_mass = STEEL_DENSITY * bracket_v;

    // Physical.mass from `let mass = volume(geometry) * material.density`.
    assert_mass_scalar(
        result.values.get(&ValueCellId::new("Bracket", "mass")),
        bracket_expected_mass,
        "Bracket.mass (Physical let mass)",
    );

    // body_mass_props mp.mass from `let mp = body_mass_props(geometry, material.density)`.
    let bracket_mp = match result.values.get(&ValueCellId::new("Bracket", "mp")) {
        Some(Value::StructureInstance(d)) => d,
        other => panic!(
            "Bracket.mp must be a MassProperties StructureInstance, got {other:?}\n\
             (diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        bracket_mp.type_name, "MassProperties",
        "Bracket.mp must be type MassProperties"
    );
    assert_mass_scalar(
        bracket_mp.fields.get("mass"),
        bracket_expected_mass,
        "Bracket.mp.mass (body_mass_props)",
    );

    // ── Plate (20mm × 20mm × 20mm) assertions ─────────────────────────────
    let plate_v = PLATE_W * PLATE_H * PLATE_D;
    let plate_expected_mass = STEEL_DENSITY * plate_v;

    assert_mass_scalar(
        result.values.get(&ValueCellId::new("Plate", "mass")),
        plate_expected_mass,
        "Plate.mass (Physical let mass)",
    );

    let plate_mp = match result.values.get(&ValueCellId::new("Plate", "mp")) {
        Some(Value::StructureInstance(d)) => d,
        other => panic!(
            "Plate.mp must be a MassProperties StructureInstance, got {other:?}\n\
             (diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        plate_mp.type_name, "MassProperties",
        "Plate.mp must be type MassProperties"
    );
    assert_mass_scalar(
        plate_mp.fields.get("mass"),
        plate_expected_mass,
        "Plate.mp.mass (body_mass_props)",
    );
}

// ── e2e negative: line removed → MissingRequiredMember naming `material` ──────

/// Strip the top-level `default Material = ...` line from the committed
/// example source, leaving the purpose-nested aluminum default and the
/// structures intact. The stripped source must produce at least one
/// `DiagnosticCode::MissingRequiredMember` Error naming `material`
/// (each Physical structure loses its injected member).
///
/// This is the "remove the line → loud error naming the mechanism" direction
/// (PRD §7(iii)).
///
/// **RED (step-5):** `strip_default_line` does not exist yet.
/// **GREEN (step-6):** the helper is implemented.
#[test]
fn line_removed_errors_naming_mechanism() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/ambient_default_material/ambient_default_surface.ri must exist \
         (task #4499 step-4 creates it)",
    );

    // Strip the file-level `default Material = ...` line.
    let stripped = strip_default_line(&source);

    // compile_source_with_stdlib (non-panicking): line removal intentionally
    // produces Errors (MissingRequiredMember for each Physical structure that
    // loses its injected `material` member).
    let compiled = reify_test_support::compile_source_with_stdlib(&stripped);

    let missing_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::MissingRequiredMember)
                && d.severity == Severity::Error
        })
        .collect();

    assert!(
        !missing_errors.is_empty(),
        "line-removed direction: must produce at least one MissingRequiredMember Error \
         when the ambient default line is stripped; got no such errors \
         (all diagnostics: {:#?})",
        compiled.diagnostics
    );

    // Each error must name `material` in the message or label text.
    for err in &missing_errors {
        let mentions_material = err.message.contains("material")
            || err
                .labels
                .iter()
                .any(|l| l.message.contains("material"));
        assert!(
            mentions_material,
            "MissingRequiredMember error must name `material` (the mechanism); \
             got message: {:?}, labels: {:?}",
            err.message, err.labels
        );
    }

    // Two Physical structures (Bracket + Plate) each lose their injected member.
    assert_eq!(
        missing_errors.len(),
        2,
        "line-removed direction: expected exactly 2 MissingRequiredMember errors \
         (one per file-level Physical structure that omitted material); \
         got {} (all diagnostics: {:#?})",
        missing_errors.len(),
        compiled.diagnostics
    );
}

/// Strip the top-level `default Material = ...` line from source.
///
/// Drops only file-indentation-level lines (not indented) whose trimmed text
/// starts with `default Material =`, so the purpose-nested aluminum default
/// (indented) is preserved. Rejoins the remaining lines.
fn strip_default_line(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            // Keep the line unless it's an unindented `default Material = ...`.
            // An unindented line has the same content when trimmed from the left,
            // or more precisely: it starts without leading whitespace.
            let is_unindented = !line.starts_with(' ') && !line.starts_with('\t');
            let is_default_material = trimmed.starts_with("default Material =");
            !(is_unindented && is_default_material)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── §9 row 7: no ambient + no material → E_DynamicsNoDensity (kernel-indep) ──

/// §9 row 7: `body_mass_props(body)` on a body with NO `default Material` and
/// NO explicit `material` member emits EXACTLY ONE `DiagnosticCode::DynamicsNoDensity`
/// hard Error whose message names all three fix phrases ("explicit density argument",
/// "Material", "default Material").
///
/// Also asserts NO diagnostic message contains "water" or "DefaultDensity"
/// (the water fallback is gone — ambient-default-material C, task 4498).
///
/// Kernel-INDEPENDENT: uses `MockGeometryKernel` so this assertion runs on
/// every runner regardless of OCCT availability.
///
/// References C's landed
/// `dynamics_body_mass_props.rs::body_mass_props_without_material_density_errors_with_no_density`
/// and type-hygiene λ gate `row_9_body_mass_props_no_material_errors_with_no_density`.
#[test]
fn row_7_no_ambient_no_material_errors_no_density() {
    const SRC: &str = r#"
structure def BmpNoAmbient {
    let body = box(50mm, 30mm, 10mm)
    let mp = body_mass_props(body)
}
"#;

    // No compile errors expected (body_mass_props signature is valid without material).
    let compiled = parse_and_compile_with_stdlib(SRC);

    // Build with MockGeometryKernel — kernel-independent (density ladder runs
    // before any geometry query).
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Exactly one DynamicsNoDensity Error.
    let no_density_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DynamicsNoDensity))
        .collect();
    assert_eq!(
        no_density_errors.len(),
        1,
        "§9 row 7: body_mass_props(b) with no ambient default and no material must emit \
         exactly one E_DynamicsNoDensity error (no water fallback); \
         got {} (all diagnostics: {:#?})",
        no_density_errors.len(),
        result.diagnostics
    );
    assert_eq!(
        no_density_errors[0].severity,
        Severity::Error,
        "§9 row 7: DynamicsNoDensity must be Severity::Error (hard error, task 4498)"
    );

    // Message must name all three fixes.
    let msg = &no_density_errors[0].message;
    assert!(
        msg.contains("explicit density argument"),
        "§9 row 7: error message must mention 'explicit density argument'; got: {msg:?}"
    );
    assert!(
        msg.contains("Material"),
        "§9 row 7: error message must mention 'Material'; got: {msg:?}"
    );
    assert!(
        msg.contains("default Material"),
        "§9 row 7: error message must mention 'default Material' (ambient fix); got: {msg:?}"
    );

    // No diagnostic must mention water/DefaultDensity (water fallback removed in C).
    let water_mentions: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("water") || d.message.contains("DefaultDensity"))
        .collect();
    assert!(
        water_mentions.is_empty(),
        "§9 row 7: no diagnostic must mention 'water' or 'DefaultDensity' \
         (water fallback removed — task 4498); got: {water_mentions:#?}"
    );
}

// ── §9 row 8: water-default symbol absent from production source ───────────────

/// §9 row 8 (behavioural-deletion regression guard): scan all `*.rs` files
/// under `crates/*/src/` (production source only — NOT tests/) for the symbols
/// `DynamicsDefaultDensity` and `W_DynamicsDefaultDensity`.
///
/// The match list must be EMPTY: ambient-default-material C (task 4498) removed
/// the water-default enum variant and its associated warning, and no production
/// code should reintroduce it.
///
/// The test is restricted to `src/` subdirectories so that this file (under
/// `tests/`) never self-matches the needle string.
///
/// **Anti-vacuity self-checks (step-11):** the test also asserts:
/// 1. `files_scanned > 0`: the walk actually visited production `.rs` files
///    (guards against a silent empty-scan that would make the guard vacuous).
/// 2. Positive-control: a scan for `"fn resolve_body_density"` (a known-present
///    symbol in `crates/reify-eval/src/dynamics_ops.rs`, the file task 4498
///    edited) MUST return a non-empty match list, proving the walker reaches
///    the exact production file the water deletion landed in.
///
/// **RED (step-9):** `scan_rs_sources` does not exist yet.
/// **GREEN (step-10):** the helper is implemented (but walker is vacuous — bug
/// found in review: visits 0 files because it only descends into a dir named
/// `src` at the `crates/` root, but those children are crate dirs, not `src`).
/// **RED (step-11):** anti-vacuity checks are added — they fail with the buggy walker.
/// **GREEN (step-12):** the walker descent logic is fixed.
#[test]
fn row_8_water_default_symbol_absent_from_source() {
    // Root is `crates/` (the parent of all crates), pointed to via CARGO_MANIFEST_DIR
    // of the reify-eval crate (`crates/reify-eval/`).
    let crates_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../");

    let (matches, files_scanned) = scan_rs_sources(
        crates_root,
        &["DynamicsDefaultDensity", "W_DynamicsDefaultDensity"],
    );

    // Anti-vacuity self-check: prove the scan actually visited production .rs files.
    // The workspace has ~106 crates/*/src/**/*.rs files; a correct walk visits at
    // least one. The buggy walker (step-10) visits 0 and matches.is_empty() passed
    // trivially — giving ZERO protection against reintroduction.
    assert!(
        files_scanned > 0,
        "§9 row 8 anti-vacuity: scan_rs_sources visited 0 .rs files under crates/*/src/; \
         the scan is vacuous and gives NO protection (walk is broken — \
         see step-12 for the fix)"
    );

    // Positive-control: 'fn resolve_body_density' lives in
    // crates/reify-eval/src/dynamics_ops.rs (the file task 4498 edited).
    // A correct walk MUST find it; if not, the walk is not reaching production source.
    let (sentinel_matches, _) = scan_rs_sources(crates_root, &["fn resolve_body_density"]);
    assert!(
        !sentinel_matches.is_empty(),
        "§9 row 8 positive-control: 'fn resolve_body_density' must be found in \
         crates/*/src/**/*.rs (expected in crates/reify-eval/src/dynamics_ops.rs, \
         the file task 4498 edited); the walker is not reaching production source"
    );

    // Real guard: the forbidden symbols must be absent from all production source.
    assert!(
        matches.is_empty(),
        "§9 row 8: `DynamicsDefaultDensity` / `W_DynamicsDefaultDensity` must be absent \
         from all crates/*/src/**/*.rs (water fallback removed in task 4498); \
         found in:\n{}",
        matches.join("\n")
    );
}

/// Recursively scan all `*.rs` files under `src/` subdirectories of crates
/// rooted at `crates_root`, returning `(matches, files_scanned)` where
/// `matches` is a list of `"<file>:<line>"` strings for every line containing
/// any of the `needles`, and `files_scanned` is the count of `.rs` files
/// actually read under `src/` dirs.
///
/// Restricted to `src/` dirs: production source only; `tests/` and `benches/`
/// are excluded so this test file cannot self-match its own needle strings.
fn scan_rs_sources(crates_root: &str, needles: &[&str]) -> (Vec<String>, usize) {
    let mut matches = Vec::new();
    let mut files_scanned = 0_usize;
    scan_dir(
        std::path::Path::new(crates_root),
        needles,
        false,
        &mut matches,
        &mut files_scanned,
    );
    (matches, files_scanned)
}

fn scan_dir(
    dir: &std::path::Path,
    needles: &[&str],
    in_src: bool,
    matches: &mut Vec<String>,
    files_scanned: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            // Only recurse into `src/` dirs; never into `tests/` or `benches/`.
            let should_descend = if in_src {
                // Already inside src/ — descend freely.
                true
            } else {
                // At the crate root or above — only descend into dirs named `src`.
                name_str == "src"
            };
            if should_descend {
                scan_dir(&path, needles, true, matches, files_scanned);
            }
        } else if in_src && name_str.ends_with(".rs") {
            // Read the file and scan for needles.
            *files_scanned += 1;
            if let Ok(content) = std::fs::read_to_string(&path) {
                for (line_no, line) in content.lines().enumerate() {
                    if needles.iter().any(|n| line.contains(n)) {
                        matches.push(format!(
                            "{}:{}",
                            path.display(),
                            line_no + 1
                        ));
                    }
                }
            }
        }
    }
}
