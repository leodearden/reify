//! CLI integration tests for `reify check` with a `RepresentationWithin`
//! assertion (Determinacy γ, task-4199).
//!
//! ## OCCT-gated test (step-9 RED / step-10 GREEN)
//!
//! `check_representation_within_violated_under_occt` exercises the full
//! headline signal: `reify check examples/representation_within.ri` must exit
//! non-zero (FAILURE) and print "VIOLATED" when OCCT is present, because the
//! coarse sphere (50 mm deflection) produces a sampled facet-chord deviation
//! far above the `1um` bound declared in `CurvedBallCheck`.
//!
//! Without OCCT the same command exits 0 — the assertion is `Indeterminate`
//! when tessellation cannot run (C1 graceful degradation).
//!
//! These tests are RED until step-10 adds `module_has_representation_within`
//! to `cmd_check` and routes it through the kernel-backed
//! `set_capture_repr_tol(true)` → `tessellate_realizations` → `check` path.
//!
//! ## C2 guard (always GREEN)
//!
//! `check_non_representation_within_module_is_unaffected` verifies that a
//! plain module (no `RepresentationWithin` constraints) is byte-for-byte
//! unaffected by the new routing: it must still exit 0 on the
//! `Engine::new(None)+check()` path.
//!
//! ## Export refusal (task η, #6170)
//!
//! PRD `docs/prds/v0_6/precision-nominal-representation-guarantee.md` §1.1 /
//! C-SURFACE (2): a design declaring a `RepresentationWithin` bound the export
//! path cannot demonstrate it honours must REFUSE to write the artifact rather
//! than write it and report success. Measured on this branch before the refusal
//! landed, BOTH export modes reproduced §1.1 verbatim on
//! `repr_within_with_stl_output.ri` — exit 0, a file written, and the bound
//! reported INDETERMINATE:
//!
//! ```text
//! $ reify build f.ri -o out.step   → exit 0, "Wrote out.step (15542 bytes)"
//! $ reify build f.ri               → exit 0, "Wrote ./o.stl (684 bytes)"
//! ```
//!
//! The two modes are structurally distinct and each needs its own guard:
//!
//! * **Mode A (`-o <file>`)** — `cmd_build` calls `std::fs::write` BEFORE it
//!   evaluates `has_error_diagnostic`, so an Error diagnostic alone cannot
//!   withhold the file. The refusal must gate the WRITE itself, which is what
//!   `build_dash_o_refusal_does_not_overwrite_an_existing_file` pins.
//! * **Mode B (no `-o`)** — the engine withholds the file by emitting an
//!   empty-bytes artifact, which the CLI writer skips.
//!
//! The three REFUSAL tests use the cheap `repr_within_with_stl_output.ri`
//! fixture (a `box`, not a sphere), so none of them imports the 5-20 s OCCT
//! tessellation cost PRD §6's gate-cost rule warns about — the refusal is taken
//! before any realization runs. The C2 negative
//! (`build_dash_o_still_exports_a_module_without_a_bound`) reuses `bracket.ri`,
//! the same module `cli_build.rs`'s `build_valid_bracket_exits_success` already
//! exports; being the positive control it MUST reach the exporter, so it pays a
//! full realization + export and is the one test here whose runtime tracks the
//! kernel rather than the parser.
//!
//! All four are deliberately NOT OCCT-gated: η's refusal is a static
//! module-shape decision taken before any measurement, so it fires identically
//! in stub and OCCT builds — unlike
//! `check_representation_within_violated_under_occt` above, which needs a real
//! measured deviation.

use crate::common;

/// OCCT-gated: `reify check examples/representation_within.ri` on a coarse
/// sphere (`#precision(50mm)`) with a tight `RepresentationWithin(subject, 1um)`
/// assertion exits non-zero (FAILURE) and prints "VIOLATED" when OCCT is
/// available.
///
/// Stub-mode (no OCCT): the same command exits 0 — the assertion is
/// `Indeterminate` when realization cannot run (C1 graceful degradation →
/// empty `achieved_repr_tol` map → never a false Violated).
///
/// RED: currently `cmd_check` routes all no-purpose modules through
/// `Engine::new(None)+check()` (no kernel, no tessellation), so the map stays
/// empty and the assertion is `Indeterminate` even under OCCT.  GREEN after
/// step-10 adds the `module_has_representation_within` routing.
#[test]
fn check_representation_within_violated_under_occt() {
    let path = common::example_path("representation_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no tessellation → map stays empty → Indeterminate → exit 0.
        // Must NOT be non-zero and must NOT print "VIOLATED".
        assert!(
            status.success(),
            "stub mode: reify check representation_within.ri should exit 0 \
             (RepresentationWithin is Indeterminate without OCCT — C1 graceful \
             degradation).\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stdout.contains("VIOLATED"),
            "stub mode: stdout must not contain 'VIOLATED' \
             (Indeterminate, not Violated).\nstdout: {stdout}"
        );
        eprintln!(
            "skipping VIOLATED assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the full tessellate → check pipeline must fire.
    // CurvedBall at #precision(50mm) ≈ 0.32 m chord deviation >> 1um (1e-6 m).
    assert!(
        !status.success(),
        "OCCT mode: reify check representation_within.ri should exit non-zero \
         (coarse sphere deviation >> 1um bound → Violated → FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "OCCT mode: stdout must contain 'VIOLATED' \
         (RepresentationWithin assertion fires: sampled deviation >> 1um bound).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ζ / C-SURFACE 1: surfaces that do not measure (task 6169)
// ═══════════════════════════════════════════════════════════════════════════════

/// The user-observable half of C-SURFACE 1, at the PRD §3.2 probe shape.
///
/// `cmd_build` never calls `set_capture_repr_tol`, so `achieved_repr_tol` stays
/// empty on the build surface and nothing can measure the subject. The verdict
/// is therefore `Indeterminate` — but it must say *why*, and point at the
/// surface that can answer, instead of blaming the operand kinds.
///
/// The VERDICT and the surface attribution are OCCT-independent: the map is
/// empty on the build surface in both kernel modes. The REMEDY is not, and is
/// gated accordingly — `Engine::unmeasured_reason` tests kernel CAPABILITY
/// before `capture_repr_tol` so that whatever it offers can actually work on
/// the binary in hand. With OCCT the terminal remedy is `reify check` (it will
/// register the kernel and measure); in stub mode `reify check` is a dead end,
/// so the remedy jumps straight to the kernel. Asserting the `reify check`
/// token unconditionally would pass in stub mode while recommending something
/// that cannot answer there — exactly the defect class this test guards.
///
/// CAPABILITY, not presence: this binary's registry is never empty in either
/// mode. `reify-kernel-manifold`'s `inventory::submit!` is unconditional and
/// `main.rs`'s `extern crate reify_kernel_manifold as _;` states verbatim that
/// the `"manifold"` key is always present, so a stub-mode binary still gets
/// `default_kernel_name == Some("manifold")` via `pick_lexmin_brep_kernel`'s
/// lex-min fallback. An earlier revision keyed arm 1 on
/// `default_kernel_name.is_none()`, which is therefore false on EVERY shipped
/// binary — the stub-mode branch below would have failed (arm 2 fires, naming
/// `reify check`), and it passed only because the local build has OCCT. The
/// discriminator now asks whether any registered adapter claims a
/// `(_, ReprKind::BRep)` pair, which OCCT alone does.
///
/// `--verbose` is deliberately not passed — plain `reify build` already prints
/// both the status line and the reason.
#[test]
fn build_surface_reports_attributable_indeterminate() {
    let path = common::fixture_path("representation_within_build_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("build", &path);

    assert!(
        status.success(),
        "Indeterminate is not a failure — `reify build` must still exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE SphereCheck#constraint[0]"),
        "the build surface cannot measure, so the verdict is Indeterminate \
         (never a false Violated).\nstdout: {stdout}\nstderr: {stderr}"
    );

    assert!(
        !stderr.contains("operator undefined for these operand kinds"),
        "C-SURFACE 1: the misattribution must be gone — the operands are fully \
         defined here (`subject` carries a default); it is the SURFACE that \
         cannot answer.\nstderr: {stderr}"
    );

    // Counted, not `find`-ed: `Engine::build` runs a constraint pass and then a
    // post-geometry re-check, and the stale-diagnostic `retain` that would
    // dedupe them only fires when the re-check UPGRADES an Indeterminate —
    // which it never does here. A regression that let both passes' diagnostics
    // reach `BuildResult` must fail here rather than pass on the first match.
    let attributions: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("SphereCheck#constraint[0]"))
        .collect();
    assert_eq!(
        attributions.len(),
        1,
        "INV-SF-4: exactly one line must name the constraint that could not be \
         evaluated, with a reason.\nstderr: {stderr}"
    );
    let attribution = attributions[0];

    assert!(
        attribution.contains("does not measure"),
        "INV-SF-4: the reason must name the surface — that token is stable \
         across every remedy.\nline: {attribution}"
    );
    if reify_kernel_occt::OCCT_AVAILABLE {
        assert!(
            attribution.contains("reify check"),
            "INV-SF-4: a kernel is live on this binary, so the surface that DOES \
             measure is one `reify check` away and must be named.\n\
             line: {attribution}"
        );
        assert!(
            !attribution.contains("kernel"),
            "a kernel is demonstrably registered on this build (`cmd_build` uses \
             `Engine::with_registered_kernel`), so blaming one would be a false \
             claim — the same INV-SF-4 misattribution ζ removes, relocated from \
             the operand kinds to the kernel.\nline: {attribution}"
        );
    } else {
        assert!(
            attribution.contains("geometry kernel"),
            "stub mode: `reify check` cannot measure on this binary either, so \
             the remedy must jump straight to what is actually missing rather \
             than hand the user a dead end.\nline: {attribution}"
        );
        assert!(
            !attribution.contains("reify check"),
            "stub mode: pointing at `reify check` is the dead-end remedy under \
             test — that binary's `reify check` has no kernel either.\n\
             line: {attribution}"
        );
    }
    // `report_eval_output` prints every diagnostic as "{severity}: {message}",
    // so the severity that actually reached the user is readable off the line.
    // Asserted POSITIVELY: a mere "not an error" check would sail past a
    // regression that re-emitted this at Warning severity.
    assert!(
        attribution.starts_with("info:"),
        "INV-SF-2 severity-hygiene corollary: `reify build` on a bounded module is \
         a path a healthy design routinely hits, so this is Info — not a warning \
         and not an error.\nline: {attribution}"
    );
}

/// The "check must not change" half: the `reify check` VERDICT and exit
/// contract on the same fixture are unaffected by the build-surface fix.
///
/// Named for the verdict, not the bytes: under OCCT the stderr really is
/// unchanged, but in stub mode it deliberately is not (see below), so pinning
/// byte-for-byte output here would be a false claim.
///
/// Under OCCT the map is non-empty, so the fast-path guard's first conjunct
/// already fails and neither the added scan nor the added diagnostic can apply
/// — the assertion is evaluated and `Satisfied` exactly as before.
///
/// Under stub mode the map stays empty for a different reason (no kernel, so
/// tessellation cannot run), and this surface legitimately gains an attributable
/// Info in place of the misattributed message that was wrong there too. That
/// swap is the change under review on this surface, so both of its halves are
/// asserted rather than merely described: exit 0 and "no VIOLATED" held before
/// the change too and cannot fail for any regression in it.
///
/// The stub-mode remedy names the geometry kernel, not `reify check` — pointing
/// a `reify check` run back at `reify check` would be a dead end — so the
/// assertion is on "does not measure", the token stable across both remedies.
#[test]
fn check_surface_verdict_is_unchanged_for_build_surface_fixture() {
    let path = common::fixture_path("representation_within_build_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        assert!(
            status.success(),
            "stub mode: no kernel → Indeterminate (C1 graceful degradation) → exit 0.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stdout.contains("VIOLATED"),
            "stub mode: Indeterminate, not Violated.\nstdout: {stdout}"
        );
        assert!(
            !stderr.contains("operator undefined for these operand kinds"),
            "stub mode: the misattribution was wrong here too and must be gone — \
             the operands are fully defined (`subject` carries a default); it is \
             the missing measurement that blocks the verdict.\nstderr: {stderr}"
        );
        assert!(
            stderr.contains("does not measure"),
            "stub mode: removing the misattribution must not leave a bare, \
             unexplained INDETERMINATE (INV-SF-4) — the surface must say why.\n\
             stderr: {stderr}"
        );
        eprintln!(
            "skipping the OCCT Satisfied assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build). Stub-mode check carries the \
             attributable Info in place of the misattributed warning; that is the \
             intended strict improvement, not a regression, and both halves are \
             asserted above."
        );
        return;
    }

    assert!(
        status.success(),
        "OCCT mode: the fine sphere clears the 1mm bound → Satisfied → exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK SphereCheck#constraint[0]"),
        "OCCT mode: tessellation measures the subject below the bound → Satisfied.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "OCCT mode: the summary line is unchanged.\nstdout: {stdout}"
    );
    assert!(
        !stderr.contains("does not measure"),
        "the attribution must NOT reach a surface that DID measure — this is the \
         direct guard on `reify check` output being unchanged.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("operator undefined for these operand kinds"),
        "and the misattribution must not reappear here either.\nstderr: {stderr}"
    );
}

/// OCCT-gated: a `reify check` run on a binary where a geometry kernel IS live
/// must never tell the user to build one.
///
/// This is the reviewer's exact repro shape, at the surface a user actually
/// touches. `capture_repr_tol && achieved_repr_tol.is_empty()` has more than
/// one cause; "no geometry kernel" is only one of them. Here the subject
/// declares no realization, so nothing is tessellated and the map stays empty
/// even though OCCT is running — and the pre-fix remedy answered that with
/// "a geometry kernel is required — build with OCCT", a false statement and a
/// dead end. That is the same INV-SF-4 misattribution class ζ exists to
/// remove, merely relocated from the operand kinds to the kernel.
///
/// The eval-level pair
/// (`kernel_present_but_nothing_tessellated_does_not_blame_the_kernel` and
/// `measurement_requested_but_unmeasured_points_at_the_kernel_not_at_check`)
/// brackets the same discriminator with a stub kernel, so it gates in
/// stub-mode CI. Only a real OCCT run reproduces the reported defect
/// end-to-end, which is what this test adds.
///
/// The OCCT gate is load-bearing rather than incidental: in stub mode this
/// same fixture legitimately lands in the no-kernel arm and SHOULD name the
/// kernel, so asserting the negative there would be wrong.
#[test]
fn check_with_kernel_present_does_not_claim_a_kernel_is_missing() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping: OCCT unavailable (cfg(has_occt) not set — stub-mode \
             build). With no kernel registered this fixture legitimately lands \
             in the missing-kernel arm and SHOULD name the kernel; the negative \
             asserted below only holds when a kernel is present. The eval-level \
             stub-kernel test covers this arm in stub-mode CI."
        );
        return;
    }

    let path = common::fixture_path("representation_within_no_realization.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "Indeterminate is not a failure — `reify check` must still exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE Checker#constraint[0]"),
        "nothing was tessellated for the subject, so the verdict is \
         Indeterminate (never a false Violated).\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Counted, not `find`-ed: a regression that emitted the Info twice must
    // fail here rather than pass on the first match.
    let attributions: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("Checker#constraint[0]"))
        .collect();
    assert_eq!(
        attributions.len(),
        1,
        "INV-SF-4: exactly one line must name the constraint that could not be \
         evaluated, with a reason.\nstderr: {stderr}"
    );
    let attribution = attributions[0];

    // `report_eval_output` prints every diagnostic as "{severity}: {message}",
    // so the severity that actually reached the user is readable off the line.
    // Asserted POSITIVELY: a mere "not an error" check would sail past a
    // regression that re-emitted this at Warning severity.
    assert!(
        attribution.starts_with("info:"),
        "INV-SF-2 severity-hygiene corollary: a subject with no realization is a \
         path a healthy design routinely hits, so this is Info — not a warning \
         and not an error.\nline: {attribution}"
    );
    assert!(
        attribution.contains("does not measure"),
        "INV-SF-4: the reason must still name the surface — that token is stable \
         across every remedy.\nline: {attribution}"
    );
    assert!(
        attribution.contains("realization"),
        "INV-SF-4: with a kernel present the actionable check is whether the \
         subject declares a realization.\nline: {attribution}"
    );
    // Cause tokens, not substrings of arm 2's current sentence: a reworded
    // kernel remedy ("a BRep kernel must be built in") must still fail this
    // gate, so the negative is on `kernel` / `OCCT` themselves — matching the
    // eval-level sibling
    // `kernel_present_but_nothing_tessellated_does_not_blame_the_kernel`.
    assert!(
        !attribution.contains("kernel") && !attribution.contains("OCCT"),
        "OCCT is demonstrably live on this binary (the sibling fixture \
         representation_within_build_surface.ri reports OK on it), so any \
         mention of a missing kernel is a FALSE claim and a dead end — the very \
         defect class ζ removes.\nline: {attribution}"
    );

    assert!(
        !stderr.contains("operator undefined for these operand kinds"),
        "C-SURFACE 1, re-pinned on a second fixture shape: the operands are \
         fully defined here (`subject` carries a default); it is the missing \
         measurement that blocks the verdict.\nstderr: {stderr}"
    );
}
/// C2 guard: a module with no `RepresentationWithin` constraints must not be
/// affected by the new routing in `cmd_check`.
///
/// Uses `crates/reify-cli/tests/fixtures/bracket.ri` — a module with satisfied
/// constraints and NO `RepresentationWithin` constraint, which is what this
/// test actually gates: the RepresentationWithin side effects
/// (`set_capture_repr_tol` + `tessellate_realizations`) must not fire for it,
/// and it must exit 0 with every constraint satisfied.
///
/// Corrected for task #5748: the original rationale called bracket.ri "a plain
/// numeric module … with no geometry" that stays on `Engine::new(None)+check()`.
/// That was never quite true — bracket.ri declares `let body = box(width,
/// height, thickness)` — and #5748's D1 made the distinction observable: `check`
/// now routes ANY geometry-bearing module through
/// `Engine::with_registered_kernel` + `build()`, so this fixture takes the
/// kernel-backed arm.  The assertions below are unchanged and still hold; only
/// the stated reason was wrong.
///
/// This test is GREEN immediately and must remain GREEN after step-10.
#[test]
fn check_non_representation_within_module_is_unaffected() {
    // bracket.ri declares no RepresentationWithin constraint → the repr-tol
    // capture/tessellate side effects stay off (it is kernel-routed by #5748's
    // has_geometry gate, but that is a different axis).
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "C2: reify check bracket.ri should exit 0 — no RepresentationWithin \
         constraints → existing Engine::new(None)+check() path unchanged.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "C2: stdout should contain 'All constraints satisfied' for bracket.ri.\n\
         stdout: {stdout}"
    );
}
// ── Export refusal (task η, #6170) ───────────────────────────────────────────

/// MODE A — the §1.1 headline. `reify build <bounded design> -o <tempdir>/out.step`
/// must exit non-zero, name the refusal on stderr, and create NO file.
///
/// `!output_path.exists()` is the negative-assertion idiom the parse-error and
/// compile-error tests in `cli_build.rs` already use. The `-o` target format is
/// irrelevant to the refusal (it is decided before any serializer is chosen), so
/// the helper's default `.step` target is fine. With `-o` present the
/// declarative driver does not fire (io-export B10), so the fixture's own
/// `o.stl` is not written into `tests/fixtures/` either.
#[test]
fn build_dash_o_refuses_to_write_for_a_bounded_module() {
    let result = common::run_build("repr_within_with_stl_output.ri");

    assert!(
        !result.status.success(),
        "reify build -o on a design declaring a RepresentationWithin bound must exit \
         non-zero.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stderr.contains("E_REPR_BOUND_UNENFORCED_ON_EXPORT"),
        "stderr must name the refusal with the stable E_* token; got: {}",
        result.stderr
    );
    assert!(
        !result.output_path.exists(),
        "NO file may be created at the -o target for a refused build (§1.1: the \
         artifact must be refused, not written-and-reported-successful)"
    );
}

/// MODE A — the refusal gates the WRITE, not merely the exit code.
///
/// `cmd_build` calls `std::fs::write(path, &data)` BEFORE it evaluates
/// `has_error_diagnostic`, so a refusal implemented purely as an Error
/// diagnostic would still truncate and overwrite whatever sits at the `-o`
/// target — exiting non-zero while having already destroyed the user's file.
/// Seeding the target with sentinel bytes and requiring them back byte-for-byte
/// is what distinguishes a real write-gate from a diagnostic bolt-on.
#[test]
fn build_dash_o_refusal_does_not_overwrite_an_existing_file() {
    const SENTINEL: &[u8] = b"pre-existing bytes that must survive a refused build";

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let target = dir.path().join("out.step");
    std::fs::write(&target, SENTINEL).expect("failed to seed the -o target");

    let (status, stdout, stderr) = common::run_with_args(&[
        "build",
        &common::fixture_path("repr_within_with_stl_output.ri"),
        "-o",
        target.to_str().expect("temp path is not valid UTF-8"),
    ]);

    assert!(
        !status.success(),
        "a refused build must exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        std::fs::read(&target).expect("the -o target must still exist"),
        SENTINEL,
        "a refused build must NOT truncate or overwrite a pre-existing file at the -o \
         target — the refusal has to gate the write itself, not ride the diagnostic \
         stream after `std::fs::write` has already run"
    );
}

/// MODE B — `reify build <bounded design>` with NO `-o` must refuse the declared
/// `: Output` occurrence.
///
/// The fixture is copied into a tempdir first because its `path: "o.stl"` is
/// design-file-relative (io-export B7): running in place would write `o.stl`
/// into `tests/fixtures/`. `!o.stl.exists()` plus the absence of `"Wrote "` on
/// stdout is what proves the occurrence produced no file and the CLI did not
/// claim otherwise. `common::run_with_args_in` pins the child cwd to that
/// tempdir, so a stray write cannot escape it by any route.
#[test]
fn build_without_dash_o_refuses_the_declared_output_occurrence() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let ri = dir.path().join("f.ri");
    std::fs::copy(common::fixture_path("repr_within_with_stl_output.ri"), &ri)
        .expect("failed to copy the bounded fixture");

    let (status, stdout, stderr) = common::run_with_args_in(
        dir.path(),
        &["build", ri.to_str().expect("temp path is not valid UTF-8")],
    );

    assert!(
        !status.success(),
        "reify build (no -o) on a bounded design must exit non-zero.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !dir.path().join("o.stl").exists(),
        "the refused STLOutput occurrence must write no file.\nstdout: {stdout}\n\
         stderr: {stderr}"
    );
    assert!(
        stderr.contains("E_REPR_BOUND_UNENFORCED_ON_EXPORT"),
        "stderr must name the refusal with the stable E_* token; got: {stderr}"
    );
    assert!(
        !stdout.contains("Wrote "),
        "stdout must NOT claim an artifact was written for a refused occurrence; \
         got: {stdout}"
    );
}

/// The C2 negative at the CLI boundary: a design with NO bound still exports
/// exactly as before.
///
/// Reuses `bracket.ri` — the geometry-bearing unbounded fixture
/// `cli_build.rs`'s `build_valid_bracket_exits_success` already builds — so this
/// asserts the same success path that test does, plus the absence of the refusal
/// token. GREEN before and after η, and it is what bounds the refusal's blast
/// radius (PRD C2 / §3.1(f)).
#[test]
fn build_dash_o_still_exports_a_module_without_a_bound() {
    let result = common::run_build("bracket.ri");

    assert!(
        result.status.success(),
        "an UNBOUNDED design must still exit 0.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("Wrote "),
        "an unbounded design must still report the written artifact; got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "an unbounded design must still write its -o target"
    );
    assert!(
        !result.stderr.contains("E_REPR_BOUND_UNENFORCED_ON_EXPORT"),
        "the refusal must NOT fire for a design that declares no bound; got: {}",
        result.stderr
    );
}
