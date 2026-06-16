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
///
/// Panics if more than one line matches the pattern (ambiguous output) so that
/// a format change or a name collision fails loudly rather than silently
/// reading the wrong value.
fn parse_scalar_cell(stdout: &str, name: &str) -> Option<f64> {
    let pattern = format!(".{name} = ");
    let matches: Vec<&str> = stdout.lines().filter(|l| l.contains(&pattern)).collect();
    let line = match matches.len() {
        0 => return None,
        1 => matches[0],
        n => panic!(
            "parse_scalar_cell: {n} lines match '{pattern}'; expected exactly one. \
             Use a fully-qualified cell name to disambiguate.\n\
             Matches: {matches:?}"
        ),
    };
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
// path.
//
// The eval-flip (conforms_mmc=true / conforms_rfs=false) is already covered
// by `cli_tolerancing_eval.rs::eval_std_tolerancing_surface_example_succeeds`
// (Test A); duplicating it here would add CI cost without novel signal.
// The NEW assertion this gate uniquely owns is the KEY B4 GUARD below:
// the *absence* of any η diagnostic ("measured deviation" absent from all
// output streams), proving C3 non-interception.
//
// Kernel-independent (std_tolerancing_surface.ri is a purely scalar file).
// Green-on-arrival — the behavior ships via the done deps + C3 keying.

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

// ── B5 — Query oracle vs boolean oracle cross-check (OCCT-gated) ─────────────
//
// B5 cross-checks that `max_deviation(actual, nominal)` (query oracle) and
// `volume(difference(actual, zone))` (boolean oracle) produce AGREEING verdicts
// on two constructed fixtures:
//   • gdt_oracle_inside.ri  — actual translated 0.05mm, zone = box(10.2mm³) → INSIDE
//   • gdt_oracle_outside.ri — actual translated 0.5mm,  zone = box(10.2mm³) → OUTSIDE
//
// box() is centered at origin (corner = (−w/2, −h/2, −d/2) in OCCT), so
// box(10.2mm,10.2mm,10.2mm) directly contains box(10mm,10mm,10mm)±0.1mm
// without an extra translate.
//
// Oracle agreement derivation (analytic — not tuned):
//   inside:  d=0.05mm, t=0.1mm → dev = 0.05mm < t → query=INSIDE;
//            actual fully in zone → empty OCCT Cut → pokeout ≈ 0.0 m³ < FLOOR
//   outside: d=0.5mm,  t=0.1mm → dev = 0.5mm > t  → query=OUTSIDE;
//            poke = (d−t)·face_area = 0.4mm·(10mm)² ≈ 4e-8 m³ ≫ FLOOR
//   FLOOR = 1e-9 m³ gives ~40× separation margin (G6: floor-bounded, NOT tuned).
//
// Stub mode (no OCCT): geometry cells are Undef → skip oracle assertions.
// The file-integrity assert (exit 0, no "Error:") is unconditional.
// Fixtures are committed in this PR; both tests are GREEN on arrival.

/// B5 (inside): `reify eval examples/tolerancing/gdt_oracle_inside.ri` —
/// actual shifted 0.05 mm (d < t = 0.1mm) — both oracles say INSIDE.
///
/// Query oracle: `dev = max_deviation(actual, nominal)` < 1e-4 m (= 0.1mm).
/// Boolean oracle: `pokeout = volume(difference(actual, zone))` < 1e-9 m³.
///
/// Fixture `examples/tolerancing/gdt_oracle_inside.ri` is committed in this PR.
#[test]
fn b5_oracle_inside_oracles_agree() {
    let path = common::example_path("tolerancing/gdt_oracle_inside.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Unconditional: fixture must be parseable (exit 0, no Error diagnostics).
    assert!(
        status.success(),
        "B5 inside: reify eval gdt_oracle_inside.ri must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "B5 inside: stderr must not contain 'Error:' diagnostics.\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "B5 inside: skipping oracle assertions — OCCT unavailable \
             (dev/pokeout cells are Undef without kernel)"
        );
        return;
    }

    // OCCT path: parse both oracle cells and assert oracle agreement.
    let dev_m = parse_scalar_cell(&stdout, "dev")
        .unwrap_or_else(|| panic!("B5 inside: could not parse 'dev' cell.\nstdout: {stdout}"));
    let pokeout_m3 = parse_scalar_cell(&stdout, "pokeout")
        .unwrap_or_else(|| panic!("B5 inside: could not parse 'pokeout' cell.\nstdout: {stdout}"));

    // Zone half-width: t = 0.1mm = 1e-4 m.  d = 0.05mm → dev < t → INSIDE.
    let verdict_query = dev_m < 1e-4_f64;
    // FLOOR = 1e-9 m³.  Inside: empty OCCT Cut → pokeout ≈ 0.0 < FLOOR → INSIDE.
    const BOOL_FLOOR_M3: f64 = 1e-9;
    let verdict_bool = pokeout_m3 < BOOL_FLOOR_M3;

    assert!(
        verdict_query,
        "B5 inside: query oracle must say INSIDE \
         (dev = {dev_m:.6e} m should be < 1e-4 m zone; d=0.05mm, t=0.1mm)."
    );
    assert!(
        verdict_bool,
        "B5 inside: boolean oracle must say INSIDE \
         (pokeout = {pokeout_m3:.6e} m³ should be < 1e-9 m³ FLOOR; \
         actual fully within zone → empty OCCT Cut → ≈0 m³)."
    );
    assert_eq!(
        verdict_query, verdict_bool,
        "B5 inside: query and boolean oracles must AGREE (both INSIDE). \
         dev = {dev_m:.6e} m, pokeout = {pokeout_m3:.6e} m³."
    );
}

/// B5 (outside): `reify eval examples/tolerancing/gdt_oracle_outside.ri` —
/// actual shifted 0.5 mm (d > t = 0.1mm) — both oracles say OUTSIDE.
///
/// Query oracle: `dev = max_deviation(actual, nominal)` ≥ 1e-4 m (not inside).
/// Boolean oracle: `pokeout = volume(difference(actual, zone))` ≥ 1e-9 m³.
///
/// Expected outside poke-out: (d − t) · face_area = 0.4mm · (10mm)² ≈ 4e-8 m³
/// (~40× FLOOR margin — analytic, G6 floor-bounded inequality, not tuned).
///
/// Fixture `examples/tolerancing/gdt_oracle_outside.ri` is committed in this PR.
#[test]
fn b5_oracle_outside_oracles_agree() {
    let path = common::example_path("tolerancing/gdt_oracle_outside.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "B5 outside: reify eval gdt_oracle_outside.ri must exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "B5 outside: stderr must not contain 'Error:' diagnostics.\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "B5 outside: skipping oracle assertions — OCCT unavailable \
             (dev/pokeout cells are Undef without kernel)"
        );
        return;
    }

    let dev_m = parse_scalar_cell(&stdout, "dev")
        .unwrap_or_else(|| panic!("B5 outside: could not parse 'dev' cell.\nstdout: {stdout}"));
    let pokeout_m3 = parse_scalar_cell(&stdout, "pokeout")
        .unwrap_or_else(|| panic!("B5 outside: could not parse 'pokeout' cell.\nstdout: {stdout}"));

    // d = 0.5mm > t = 0.1mm → dev > t → query says OUTSIDE (not inside).
    let verdict_query = dev_m < 1e-4_f64; // false → OUTSIDE
    // Poke-out ≈ 4e-8 m³ ≫ 1e-9 m³ FLOOR → bool says OUTSIDE (not inside).
    const BOOL_FLOOR_M3: f64 = 1e-9;
    let verdict_bool = pokeout_m3 < BOOL_FLOOR_M3; // false → OUTSIDE

    assert!(
        !verdict_query,
        "B5 outside: query oracle must say OUTSIDE \
         (dev = {dev_m:.6e} m should be ≥ 1e-4 m zone; d=0.5mm, t=0.1mm)."
    );
    assert!(
        !verdict_bool,
        "B5 outside: boolean oracle must say OUTSIDE \
         (pokeout = {pokeout_m3:.6e} m³ should be ≥ 1e-9 m³ FLOOR; \
         poke-out ≈ (0.5−0.1)mm·(10mm)² ≈ 4e-8 m³)."
    );
    assert_eq!(
        verdict_query, verdict_bool,
        "B5 outside: query and boolean oracles must AGREE (both OUTSIDE). \
         dev = {dev_m:.6e} m, pokeout = {pokeout_m3:.6e} m³."
    );
}

// ── B9 — Pass ordering / weave (OCCT-gated) ──────────────────────────────────
//
// B9 proves the C5 caller-order weave contract: mixing a Satisfied scalar
// Conforms [0], a Violated geometric Conforms [1] (η), and a Satisfied
// RepresentationWithin [2] in one module produces results in declaration order
// and neither pass perturbs the other's verdicts.
//
// gdt_pass_weave.ri declares:
//   GdtPassWeave structure:
//     [0] scalar Conforms (no `actual`): tol=0.1mm, dev=0.05mm → Satisfied
//     [1] geometric Conforms (explicit `actual`): actual=translate(part,0.5mm)
//         vs 0.1mm zone → Violated under OCCT (like B1)
//   WeaveSphereCheck structure:
//     [0] RepresentationWithin(subject, 1mm) over fine sphere at #precision(0.1mm)
//         → Satisfied under OCCT / Indeterminate in stub mode
//
// Constraint labels (ConstraintInstDecl format + ConstraintNodeId Display):
//   scalar_label = "Conforms#0[0]"    (first  `constraint Conforms(...)` → Conforms#0[0])
//   geo_label    = "Conforms#1[0]"    (second `constraint Conforms(...)` → Conforms#1[0])
//   rw_label     = "WeaveSphereCheck#constraint[0]"  (ConstraintNodeId Display)
//
// Note: `constraint Conforms(...)` is a ConstraintInstDecl whose label follows
// the pattern "{name}#{instance_idx}[{predicate_idx}]" (entity.rs ~4175).
// `constraint RepresentationWithin(...)` is intercepted engine-side and the
// resulting entry uses the ConstraintNodeId Display form.
//
// Under OCCT: scalar=OK, geometric=VIOLATED, RW=OK; exit non-zero;
//   "Some constraints violated."; exactly one "VIOLATED".
// Stub mode: scalar=OK, geometric=INDETERMINATE, RW=INDETERMINATE; exit 0;
//   no "VIOLATED"; order still preserved.
//
// Fixture committed in this PR; test is GREEN on arrival.

/// B9: `reify check examples/tolerancing/gdt_pass_weave.ri` produces three
/// constraint result lines in declaration order with correct verdicts (under
/// OCCT and in stub mode), proving C5 caller-order weave + cross-pass
/// non-perturbation.
///
/// Fixture `examples/tolerancing/gdt_pass_weave.ri` is committed in this PR.
#[test]
fn b9_pass_ordering_and_weave() {
    let path = common::example_path("tolerancing/gdt_pass_weave.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    // Constraint labels for `constraint Conforms(...)` instantiations follow
    // the ConstraintInstDecl format "{name}#{instance_idx}[{predicate_idx}]".
    // SINGLE SOURCE OF TRUTH: entity.rs `expand_constraint_inst` (the
    // ConstraintInstDecl compiler that allocates instance_idx and formats the
    // label string).  If that function is relabeled, update these strings here
    // too.  Two separate `constraint Conforms(...)` calls produce Conforms#0[0]
    // and Conforms#1[0] (instance_idx increments per Conforms instantiation).
    // `constraint RepresentationWithin(...)` is intercepted engine-side by
    // dispatch_constraints / eval_representation_within; its ConstraintNodeId
    // Display is "WeaveSphereCheck#constraint[0]" (one RepresentationWithin in
    // the WeaveSphereCheck template).
    let scalar_label = "Conforms#0[0]";
    let geo_label    = "Conforms#1[0]";
    let rw_label     = "WeaveSphereCheck#constraint[0]";

    // The three labels must appear in stdout in declaration order.
    let scalar_pos = stdout.find(scalar_label).unwrap_or_else(|| {
        panic!(
            "B9: '{scalar_label}' not found in stdout.\n\
             stdout: {stdout}\nstderr: {stderr}"
        )
    });
    let geo_pos = stdout.find(geo_label).unwrap_or_else(|| {
        panic!(
            "B9: '{geo_label}' not found in stdout.\n\
             stdout: {stdout}\nstderr: {stderr}"
        )
    });
    let rw_pos = stdout.find(rw_label).unwrap_or_else(|| {
        panic!(
            "B9: '{rw_label}' not found in stdout.\n\
             stdout: {stdout}\nstderr: {stderr}"
        )
    });

    // Declaration order: scalar[0] before geometric[1] before RW[0].
    assert!(
        scalar_pos < geo_pos,
        "B9: scalar [0] must appear before geometric [1] in stdout \
         (declaration order preserved; scalar_pos={scalar_pos}, geo_pos={geo_pos}).\n\
         stdout: {stdout}"
    );
    assert!(
        geo_pos < rw_pos,
        "B9: geometric [1] must appear before RepresentationWithin in stdout \
         (declaration order preserved; geo_pos={geo_pos}, rw_pos={rw_pos}).\n\
         stdout: {stdout}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → geometric=INDETERMINATE, RW=INDETERMINATE, scalar=OK.
        // Exit 0 (no VIOLATED); no "VIOLATED" in stdout.
        assert!(
            status.success(),
            "B9 stub: should exit 0 (geometric=INDETERMINATE, RW=INDETERMINATE → \
             no VIOLATED).\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stdout.contains("VIOLATED"),
            "B9 stub: stdout must NOT contain 'VIOLATED' (C1: no false Violated).\n\
             stdout: {stdout}"
        );
        assert!(
            stdout.contains(&format!("OK {scalar_label}")),
            "B9 stub: scalar Conforms [0] must be OK (kernel-independent scalar path).\n\
             stdout: {stdout}"
        );
        assert!(
            stdout.contains(&format!("INDETERMINATE {geo_label}")),
            "B9 stub: geometric Conforms [1] must be INDETERMINATE (no kernel → C1).\n\
             stdout: {stdout}"
        );
        assert!(
            stdout.contains(&format!("INDETERMINATE {rw_label}")),
            "B9 stub: RepresentationWithin must be INDETERMINATE (no kernel → C1).\n\
             stdout: {stdout}"
        );
        eprintln!(
            "B9: stub-mode assertions passed — OCCT unavailable, \
             full VIOLATED verdict check skipped"
        );
        return;
    }

    // OCCT mode: scalar=OK, geometric=VIOLATED, RW=OK; exit non-zero; one VIOLATED.
    assert!(
        !status.success(),
        "B9 OCCT: should exit non-zero \
         (geometric Conforms is VIOLATED: 0.5mm deviation > 0.1mm zone).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "B9 OCCT: stdout must contain 'Some constraints violated.'.\nstdout: {stdout}"
    );
    assert!(
        stdout.contains(&format!("OK {scalar_label}")),
        "B9 OCCT: scalar Conforms [0] must be OK (η must NOT perturb scalar Conforms).\n\
         stdout: {stdout}"
    );
    assert!(
        stdout.contains(&format!("VIOLATED {geo_label}")),
        "B9 OCCT: geometric Conforms [1] must be VIOLATED (0.5mm > 0.1mm zone).\n\
         stdout: {stdout}"
    );
    assert!(
        stdout.contains(&format!("OK {rw_label}")),
        "B9 OCCT: RepresentationWithin must be OK (η must NOT perturb RW verdict).\n\
         stdout: {stdout}"
    );
    // Exactly one VIOLATED (from the geometric Conforms only — non-perturbation proof).
    let violated_count = stdout.matches("VIOLATED").count();
    assert_eq!(
        violated_count, 1,
        "B9 OCCT: exactly one 'VIOLATED' expected (geometric Conforms only); \
         got {violated_count}.\nstdout: {stdout}"
    );
}
