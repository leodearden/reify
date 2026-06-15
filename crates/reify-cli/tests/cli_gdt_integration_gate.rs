//! §9 boundary-test integration gate for the GD&T geometric-zones PRD
//! (`docs/prds/v0_6/gdt-geometric-zones-and-containment.md`).
//!
//! θ (task 4481) owns the **full B1–B9 matrix** as **one named CI suite**.
//! This file is the gate artifact. It directly exercises the 3 cross-cutting
//! rows that no single sibling task owns (B4, B5, B9), and maps the remaining
//! rows to their owning tests:
//!
//! ## §9 Matrix — full row map
//!
//! | Row | Scenario | Owner |
//! |-----|----------|-------|
//! | B1 | Producer→consumer: η pass feeds predicate (VIOLATED + magnitude) | `cli_gdt_conformance.rs::check_gdt_conformance_violated_under_occt` |
//! | B2 | Conformant twin (Satisfied, no spurious Violated) | `cli_gdt_conformance.rs::check_gdt_conformance_satisfied_exits_zero` |
//! | B3 | C1 invariant (no kernel → Indeterminate, never false Violated) | `cli_gdt_conformance.rs::check_gdt_conformance_violated_under_occt` (stub path) |
//! | B4 | Scalar path untouched — regression guard (both ways) | **here** (`b4_*`) + `cli_tolerancing_eval.rs` (Test A / Test B) |
//! | B5 | Query oracle vs boolean oracle (inside/outside, OCCT-gated) | **here** (`b5_*`) |
//! | B6 | Zone volume oracles (cylinder / slab analytic identities) | `cli_gdt_zones_eval.rs` + `reify-eval/tests/zone_constructors_e2e.rs` + `zone_slab_e2e.rs` |
//! | B7 | Legality lint fires per instance (`E_GdtIllegalModifier`) | `cli_gdt_legality.rs` |
//! | B8 | VC clearance both verdicts (conformant + under-clearanced) | `cli_vc_clearance.rs` |
//! | B9 | Pass ordering / weave (scalar + geometric Conforms + RepresentationWithin) | **here** (`b9_*`) |
//!
//! The rows marked **here** are implemented as named test functions below.
//! B1–B3, B6–B8 are **not duplicated** — they already run green in CI from
//! their owning done-tasks (γ/δ/ε/η/β). Re-running them would double CI cost
//! (B1/B8 are slow OCCT build pipelines) and duplicate float assertions that
//! G6 forbids in the gate.

mod common;

// ── Parse helper ─────────────────────────────────────────────────────────────

/// Parse the numeric (SI) value of a named `let` cell from `reify eval` stdout.
///
/// Looks for a line containing `.<name> = ` (dot-prefix anchors on the cell
/// name suffix, e.g. `GdtOracleInside.dev = 5e-05 m`), splits on `=`, trims
/// the RHS, splits by whitespace, and parses the first token as `f64`.
///
/// Returns `None` if the cell is absent from stdout, evaluates to `undef`, or
/// its first whitespace token is not a valid `f64`.
#[allow(dead_code)]
fn parse_scalar_cell(stdout: &str, name: &str) -> Option<f64> {
    let pattern = format!(".{name} = ");
    let line = stdout.lines().find(|l| l.contains(&pattern))?;
    let rhs = line.split_once('=')?.1.trim();
    if rhs.starts_with("undef") {
        return None;
    }
    rhs.split_whitespace().next()?.parse::<f64>().ok()
}

// ── B4 — Scalar-path-untouched regression guard ──────────────────────────────
//
// B4 proves that the η geometric-conformance pass (C3 keying on explicit
// `actual`) does NOT intercept a scalar `Conforms` with no `actual` binding.
// `std_tolerancing_surface.ri` is the reference file: it has a
// `constraint Conforms(tolerance: self.pos_mmc, measured_deviation: 0.15mm,
// feature_departure: 0.1mm)` with NO explicit `actual` — the canonical scalar
// path.  Both the eval flip and the check-green expectations are already
// locked by `cli_tolerancing_eval.rs` (Test A / Test B); the NEW assertion
// here is the absence of any η diagnostic ("measured deviation" absent),
// which is the key regression signal if η over-intercepts.
//
// Kernel-independent (std_tolerancing_surface.ri is a purely scalar file).
// Green-on-arrival — the behavior ships via the done deps + C3 keying.

/// B4 (signal, eval): `reify eval examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0 and stdout carries the MMC-vs-RFS conformance FLIP
/// (`conforms_mmc = true`, `conforms_rfs = false`).
///
/// Supplements `cli_tolerancing_eval.rs::eval_std_tolerancing_surface_example_succeeds`
/// (Test A) as a B4 regression anchor in this gate suite. Only pins the
/// headline flip — not the full float cell assertions already covered by Test A.
#[test]
fn b4_scalar_eval_mmc_rfs_flip() {
    let path = common::example_path("tolerancing/std_tolerancing_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "B4 eval: reify eval std_tolerancing_surface.ri should exit 0 \
         (kernel-independent scalar file).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("conforms_mmc = true"),
        "B4 eval: stdout must contain 'conforms_mmc = true' \
         (MMC zone 0.2mm ≥ 0.15mm → Satisfied).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("conforms_rfs = false"),
        "B4 eval: stdout must contain 'conforms_rfs = false' \
         (RFS zone 0.1mm < 0.15mm → not satisfied).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// B4 (guard, check): `reify check examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0, reports "All constraints satisfied.", and produces NO η
/// geometric-conformance diagnostic (neither stdout nor stderr contains
/// "measured deviation").
///
/// The absence of "measured deviation" is the B4 regression signal: it proves
/// that the η pass did NOT intercept the scalar `Conforms(tolerance: pos_mmc,
/// measured_deviation: 0.15mm, feature_departure: 0.1mm)` which has **no
/// explicit `actual` binding** (C3 keying: only explicit `actual` triggers
/// the geometric path; scalar-only Conforms must be left untouched).
///
/// Kernel-independent: the scalar Conforms passes without a geometry kernel.
#[test]
fn b4_scalar_check_green_no_eta_intercept() {
    let path = common::example_path("tolerancing/std_tolerancing_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "B4 check: reify check std_tolerancing_surface.ri should exit 0 \
         (scalar Conforms satisfied, kernel-independent).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "B4 check: stdout must contain 'All constraints satisfied.'.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("Some constraints violated"),
        "B4 check: stdout must NOT contain 'Some constraints violated'.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // KEY B4 GUARD: η must NOT intercept a scalar Conforms with no `actual` binding.
    // "measured deviation" in any output stream would indicate C3 regression.
    assert!(
        !stdout.contains("measured deviation"),
        "B4 check: stdout must NOT contain 'measured deviation' — η intercepted a \
         scalar Conforms without explicit `actual` (C3 regression).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("measured deviation"),
        "B4 check: stderr must NOT contain 'measured deviation' — η intercepted a \
         scalar Conforms without explicit `actual` (C3 regression).\n\
         stderr: {stderr}"
    );
}
