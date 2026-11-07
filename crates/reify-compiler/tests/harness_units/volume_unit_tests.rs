//! Tests for the `L` (litre) volume unit symbol in stdlib/units.ri (task λ, #5788).
//!
//! Pins docs/prds/v0_6/angle-units-surface-convergence.md cluster C / decision
//! D7: `pub unit L : Volume = 0.001` is hand-declared, exactly as θ's `Nm` is,
//! because si_units.rs only concatenates `<prefix><name>` and has no
//! cubic-metre base entry — so no generated spelling can ever produce `L`.
//!
//! WHY this matters beyond the symbol: `L` is already a rung of the curated
//! Volume ladder (`reify_core::display_units`, si_scale 1e-3), so before this
//! task the display surface advertised a unit the compiler could not resolve.
//! That is the concrete drift defect the cluster exists to close.
//!
//! Case-distinctness from a lowercase `l` is an EXISTING property of
//! `UnitRegistry::lookup` (crates/reify-compiler/src/units.rs), an exact
//! HashMap key match with zero case folding — this file PINS that property for
//! `L`/`l` rather than building it, mirroring torque_unit_tests.rs's
//! `Nm`/`nm`/`NM`/`nM` block.
//!
//! Cross-file references here name SYMBOLS, never line numbers: a hand-copied
//! line number rots on the first unrelated edit to the cited file and no gate
//! catches the drift.

use crate::common::{assert_eq_rel, assert_simple_unit, stdlib_param_si_value};
use reify_core::{DimensionVector, Severity};
use reify_test_support::compile_source_with_stdlib;

// ─── L — Volume, exactly 1e-3 m³, case-distinct from lowercase `l` ───────────

#[test]
fn l_litre_unit_resolves_to_volume() {
    // (a) Registry shape: `L` exists in std/units as a Volume unit with factor
    // 1e-3 and no offset.
    assert_simple_unit("L", DimensionVector::VOLUME, 1e-3, 1e-12);

    // (b) End-to-end literal resolution: `1L` reaches a Scalar carrying the SI
    // magnitude 1e-3 and the Volume dimension.
    let (v, d) = stdlib_param_si_value("Volume", "1L");
    assert_eq_rel(v, 1e-3, 1e-12, "1L should be 1e-3 m^3 (litre)");
    assert_eq!(d, DimensionVector::VOLUME);

    // (c) 1 L = 1000 cm³ is a DEFINITION, not a measured conversion, so the two
    // spellings must agree exactly. The 1e-12 relative tolerance here absorbs
    // only f64 representation of the `0.001` and `0.01^3` factors, not any
    // physical imprecision — contrast the curated ladder's `g/cm^3` rung, whose
    // 999.9999999999999 is why Invariant R's scale comparison is relative at
    // all (see the header note on that rung in
    // tests/prd-gate/fixtures/unit_curated_labels_ascii.ri).
    let (litre_v, litre_d) = stdlib_param_si_value("Volume", "2L");
    let (cc_v, cc_d) = stdlib_param_si_value("Volume", "2000cm^3");
    assert_eq_rel(litre_v, cc_v, 1e-12, "2L should equal 2000cm^3 in SI");
    assert_eq!(litre_d, cc_d);
    assert_eq!(litre_d, DimensionVector::VOLUME);
}

#[test]
fn lowercase_l_stays_unknown() {
    // NEGATIVE, mirroring θ's `NM`/`nM` case-distinctness pin. `l` is neither a
    // base/derived unit name nor an SI prefix, so si_units.rs's
    // `<prefix><name>` concatenation can never produce it and it must stay
    // unknown after D7 declares the uppercase `L`.
    //
    // Uses `compile_source_with_stdlib` directly rather than
    // `stdlib_param_si_value`, which panics on any Error diagnostic and so
    // cannot express a negative (torque_unit_tests.rs's
    // `nm_torque_is_case_distinct_from_nm_nanometre` documents the same
    // constraint).
    let source = "structure def S { param x : Volume = 5l }";
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an Error diagnostic for unknown unit 'l', got none"
    );
    // Assert on the diagnostic's RENDERING of the offending token, not on a
    // bare `'l'`: virtually any diagnostic this snippet could produce contains
    // a lowercase 'l' (the dimension name "Volume" does, as do "resolve",
    // "literal", "invalid"), so a bare-char check cannot tell "unknown unit: l"
    // apart from a completely unrelated error. Same hazard the sibling code
    // already guards — annotations/display.rs asserts on the QUOTED label token
    // so the 'L' inside "Length" cannot satisfy it, and θ picks the distinctive
    // multi-char "NM"/"nM" tokens for exactly this reason.
    //
    // Both emitters (`unit_resolve_error_to_diagnostic` and expr.rs's
    // quantity-literal path) render this as `unknown unit: <name>`, so strip
    // that prefix and compare the remainder EXACTLY — a hypothetical
    // `unknown unit: litre` must not satisfy the pin either.
    assert!(
        errors
            .iter()
            .any(|d| d.message.strip_prefix("unknown unit: ") == Some("l")),
        "expected an Error rendering the offending unit as `unknown unit: l`; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Loads the already-committed, previously-unreferenced prd-gate fixture
/// (`tests/prd-gate/fixtures/unit_curated_labels_ascii.ri`) and asserts it
/// compiles clean — the end-to-end user-observable λ signal.
///
/// Every literal in that fixture is one rung of a curated ladder written in the
/// ASCII form this task normalizes onto, so the fixture is simultaneously the
/// grammar evidence (it already parses) and the resolution gate (`1L` is the
/// single line that does not resolve before D7). Because `unknown unit` is
/// file-fatal, the fixture currently stops at its `let vol_litre = 1L` line and
/// prints nothing — which is exactly why wiring it up is this task's job, the
/// same way θ wired up `unit_nm_torque_immediate.ri`.
///
/// Path resolution mirrors torque_unit_tests.rs's
/// `prd_gate_fixture_unit_nm_torque_immediate_compiles_clean`: the committed
/// fixture is the single source of truth, read from disk rather than inlined,
/// so it cannot drift out of sync. Read-only input: this test must never edit
/// it.
#[test]
fn prd_gate_fixture_unit_curated_labels_ascii_compiles_clean() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/prd-gate/fixtures/unit_curated_labels_ascii.ri");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));

    let module = compile_source_with_stdlib(&src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fixture {} should compile with no errors, got: {:?}",
        path.display(),
        errors
    );
}
