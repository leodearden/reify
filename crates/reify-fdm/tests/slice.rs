// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the PrusaSlicer subprocess core (`reify_fdm::slice`,
//! task η).
//!
//! Every test here is GREEN-able WITHOUT a live PrusaSlicer (which is not on
//! PATH in CI): PATH discovery uses a synthetic `$PATH` string + a fake
//! executable in a tempdir; subprocess spawn/cancel/reap uses injected stub
//! binaries (`sh -c …`); G-code→Toolpath reuses ζ's parser on the committed
//! fixture; determinism is asserted by parsing the committed fixture twice.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reify_fdm::InfillPattern;
use reify_fdm::slice::{
    SliceRunOutcome, SliceSettings, compose_slicer_args, discover_slicer, infill_pattern_arg,
    run_slicer,
};

/// The canonical PrusaSlicer binary names probed on `$PATH`, in priority order.
const CANDIDATES: &[&str] = &[
    "prusa-slicer",
    "prusa-slicer-console",
    "PrusaSlicer",
    "prusaslicer",
];

/// Write a file named `name` under `dir`, marking it executable on unix, and
/// return its path.
fn write_exe(dir: &std::path::Path, name: &str, executable: bool) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write fake exe");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("set perms on fake exe");
    }
    path
}

// ── step-1: discover_slicer ────────────────────────────────────────────────────

/// A fake `prusa-slicer` executable on the synthetic PATH is discovered, and the
/// returned path is exactly the fake exe.
#[test]
fn discovers_executable_candidate_on_synthetic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = write_exe(dir.path(), "prusa-slicer", true);

    // PATH = a noise dir that has no candidate, then the dir holding the fake exe.
    let other = tempfile::tempdir().expect("tempdir2");
    let path_var = format!(
        "{}:{}",
        other.path().display(),
        dir.path().display()
    );

    let found = discover_slicer(&path_var, CANDIDATES);
    assert_eq!(
        found.as_deref(),
        Some(exe.as_path()),
        "discover_slicer must find the executable prusa-slicer on the synthetic PATH"
    );
}

/// An empty PATH yields no slicer (the W_FDM_SLICER_UNAVAILABLE trigger).
#[test]
fn empty_path_yields_none() {
    assert_eq!(
        discover_slicer("", CANDIDATES),
        None,
        "an empty PATH must yield no slicer"
    );
}

/// A PATH whose dirs contain no candidate yields None (the absent-slicer case).
#[test]
fn path_without_candidate_yields_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A same-dir file with a NON-candidate name must not be matched.
    write_exe(dir.path(), "some-other-tool", true);
    let path_var = format!("{}", dir.path().display());

    assert_eq!(
        discover_slicer(&path_var, CANDIDATES),
        None,
        "a PATH with no candidate-named executable must yield None"
    );
}

/// A file with the right name but NO executable bit is not matched (unix only —
/// on non-unix discovery falls back to existence).
#[cfg(unix)]
#[test]
fn non_executable_candidate_is_not_matched() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_exe(dir.path(), "prusa-slicer", false); // exists but not +x
    let path_var = format!("{}", dir.path().display());

    assert_eq!(
        discover_slicer(&path_var, CANDIDATES),
        None,
        "a non-executable file named prusa-slicer must NOT be matched"
    );
}

// ── step-3: SliceSettings + compose_slicer_args ────────────────────────────────

/// `true` iff `args` contains `flag` immediately followed by `value` (the
/// `Command::args` convention: each flag and its value are separate elements).
fn has_flag_value(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2)
        .any(|w| w[0] == flag && w[1] == value)
}

/// `true` iff `args` contains the bare `flag` (e.g. `--export-gcode`).
fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn sample_settings() -> SliceSettings {
    SliceSettings {
        layer_height: 0.2,
        walls: 3,
        top_bottom_layers: 4,
        infill_density: 0.2,
        infill_pattern: InfillPattern::Gyroid,
    }
}

/// The mechanically-relevant FDMProcess subset maps to the pinned, deterministic
/// PrusaSlicer CLI flags + the explicit output path.
#[test]
fn compose_args_maps_mechanically_relevant_flags() {
    let settings = sample_settings();
    let out = Path::new("/tmp/reify-slice-out.gcode");
    let args = compose_slicer_args(&settings, out);

    assert!(
        has_flag_value(&args, "--layer-height", "0.2"),
        "layer_height → --layer-height 0.2; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "--perimeters", "3"),
        "walls → --perimeters 3; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "--top-solid-layers", "4"),
        "top_bottom_layers → --top-solid-layers 4; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "--bottom-solid-layers", "4"),
        "top_bottom_layers → --bottom-solid-layers 4; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "--fill-density", "20%"),
        "infill_density 0.2 → --fill-density 20%; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "--fill-pattern", "gyroid"),
        "Gyroid → --fill-pattern gyroid; got {args:?}"
    );
    assert!(
        has_flag(&args, "--export-gcode"),
        "must request G-code export; got {args:?}"
    );
    assert!(
        has_flag_value(&args, "-o", out.to_str().unwrap()),
        "must pin the explicit output path via -o; got {args:?}"
    );
}

/// Determinism-pinning flags: single-threaded slicing so the G-code is
/// reproducible run-to-run (verify-and-lock golden precondition).
#[test]
fn compose_args_pins_determinism_flags() {
    let args = compose_slicer_args(&sample_settings(), Path::new("/tmp/out.gcode"));
    assert!(
        has_flag_value(&args, "--threads", "1"),
        "must pin --threads 1 for deterministic output; got {args:?}"
    );
}

/// InfillPattern → PrusaSlicer fill-pattern string mapping (≥2 patterns), and the
/// mapping is reflected in the composed `--fill-pattern` arg.
#[test]
fn infill_pattern_maps_to_prusaslicer_strings() {
    assert_eq!(infill_pattern_arg(InfillPattern::Gyroid), "gyroid");
    assert_eq!(infill_pattern_arg(InfillPattern::Grid), "grid");
    assert_eq!(infill_pattern_arg(InfillPattern::Cubic), "cubic");

    // The composed args reflect the mapping for a non-default pattern too.
    let mut settings = sample_settings();
    settings.infill_pattern = InfillPattern::Grid;
    let args = compose_slicer_args(&settings, Path::new("/tmp/out.gcode"));
    assert!(
        has_flag_value(&args, "--fill-pattern", "grid"),
        "Grid → --fill-pattern grid; got {args:?}"
    );
}

// ── step-5 / step-7: run_slicer (injected stub binaries, no live PrusaSlicer) ───

/// Absolute path to the committed ζ PrusaSlicer-vocabulary fixture.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prusaslicer_bracket.gcode")
}

fn fixture_gcode() -> String {
    std::fs::read_to_string(fixture_path()).expect("read committed fixture")
}

/// Write a `#!/bin/sh` stub "slicer" with `body`, mark it +x, return its path.
#[cfg(unix)]
fn write_stub_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write stub script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod +x stub");
    path
}

/// A stub-slicer body that copies the committed fixture to the `-o` output path
/// the args carry, then exits 0 — emulating a successful slice.
#[cfg(unix)]
fn emit_fixture_body() -> String {
    format!(
        "out=\"\"\nprev=\"\"\nfor a in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi\n  prev=\"$a\"\ndone\ncp \"{}\" \"$out\"\n",
        fixture_path().display()
    )
}

/// A successful slice: the stub writes the fixture to `-o` and exits 0; run_slicer
/// returns `Completed` with the produced G-code verbatim.
#[cfg(unix)]
#[test]
fn run_slicer_completed_reads_output_gcode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = write_stub_script(dir.path(), "ok-slicer.sh", &emit_fixture_body());
    let out = dir.path().join("out.gcode");
    let args = compose_slicer_args(&sample_settings(), &out);

    let outcome = run_slicer(&stub, &args, &|| false, Duration::from_millis(200));
    match outcome {
        SliceRunOutcome::Completed { gcode } => {
            assert_eq!(gcode, fixture_gcode(), "produced G-code must equal the fixture");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

/// A non-zero exit is captured as `Failed` (here `sh -c 'exit 7'`).
#[cfg(unix)]
#[test]
fn run_slicer_nonzero_exit_is_failed() {
    let args = vec!["-c".to_string(), "exit 7".to_string()];
    let outcome = run_slicer(
        Path::new("/bin/sh"),
        &args,
        &|| false,
        Duration::from_millis(200),
    );
    assert!(
        matches!(outcome, SliceRunOutcome::Failed { .. }),
        "a non-zero slicer exit must map to Failed, got {outcome:?}"
    );
}
