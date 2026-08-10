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
/// OCCT-INDEPENDENT: the map is empty on the build surface in both kernel
/// modes, so the behaviour is identical and there is nothing to gate.
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

    let attribution = stderr
        .lines()
        .find(|l| l.contains("SphereCheck#constraint[0]"))
        .unwrap_or_else(|| {
            panic!(
                "INV-SF-4: exactly the constraint that could not be evaluated must \
                 be named, with a reason.\nstderr: {stderr}"
            )
        });
    assert!(
        attribution.contains("does not measure"),
        "INV-SF-4: the reason must name the surface.\nline: {attribution}"
    );
    assert!(
        attribution.contains("reify check"),
        "INV-SF-4: the user must be pointed at the surface that does measure.\n\
         line: {attribution}"
    );
    assert!(
        !attribution.starts_with("error:"),
        "INV-SF-2 severity-hygiene corollary: `reify build` on a bounded module is \
         a path a healthy design routinely hits, so this is Info — not an error.\n\
         line: {attribution}"
    );
}

/// The "check must not change" half: `reify check` on the same fixture is
/// unaffected by the build-surface fix.
///
/// Under OCCT the map is non-empty, so the fast-path guard's first conjunct
/// already fails and neither the added scan nor the added diagnostic can apply
/// — the assertion is evaluated and `Satisfied` exactly as before.
///
/// Under stub mode the map stays empty for a different reason (no kernel, so
/// tessellation cannot run), and this surface legitimately gains the same
/// attributable Info in place of the misattributed message that was wrong there
/// too. Both standing gate invariants — exit 0, no "VIOLATED" — are untouched
/// by an Indeterminate verdict at Info severity, so those are what is asserted.
#[test]
fn check_surface_output_is_unchanged_for_build_surface_fixture() {
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
        eprintln!(
            "skipping the OCCT Satisfied assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build). Stub-mode check now carries \
             the attributable Info in place of the misattributed warning; that is \
             the intended strict improvement, not a regression."
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

/// C2 guard: a module with no `RepresentationWithin` constraints must not be
/// affected by the new routing in `cmd_check`.
///
/// Uses `crates/reify-cli/tests/fixtures/bracket.ri` — a plain numeric module
/// with satisfied constraints and no geometry.  The no-purpose path must still
/// route through `Engine::new(None)+check()` (unchanged) and exit 0.
///
/// This test is GREEN immediately and must remain GREEN after step-10.
#[test]
fn check_non_representation_within_module_is_unaffected() {
    // bracket.ri has no RepresentationWithin constraints → existing path.
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
