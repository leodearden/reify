// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the PrusaSlicer subprocess core (`reify_fdm::slice`,
//! task η).
//!
//! Every test here is GREEN-able WITHOUT a live PrusaSlicer (which is not on
//! PATH in CI): PATH discovery uses a synthetic `$PATH` string + a fake
//! executable in a tempdir; subprocess spawn/cancel/reap uses injected stub
//! binaries (`sh -c …`); G-code→Toolpath reuses ζ's parser on the committed
//! fixture; determinism is asserted by parsing the committed fixture twice.
//!
//! Force-recompile note: this comment busts the sccache hit for a stale test
//! binary compiled in the now-deleted `_merge-verify` warm-lane worktree,
//! whose baked `env!("CARGO_MANIFEST_DIR")` pointed at a non-existent path
//! (same fix pattern as commit 99bba2f39d for reify-doc).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reify_fdm::slice::{
    SliceError, SliceRunOutcome, SliceSettings, compose_slicer_args, discover_slicer,
    infill_pattern_arg, run_slicer, serialize_toolpath_canonical, slice_body,
};
use reify_fdm::{BeadRole, InfillPattern, parse_prusaslicer_gcode};

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

// ── step-7: run_slicer cancellation + SIGTERM→SIGKILL reap (injected stubs) ──────

/// `true` iff `pid` no longer names any process — neither live nor zombie — i.e.
/// the child was **fully reaped**. A still-running process AND an un-reaped
/// zombie both answer `kill(pid, 0)` with success (`0`); only a reaped pid (or a
/// never-existent one) yields `ESRCH`. So this is a precise "no orphan AND no
/// zombie" probe, which is exactly the cancellation contract (SIGTERM→grace→
/// SIGKILL→`wait`): the spawned child must be gone, not lingering as either.
#[cfg(unix)]
fn pid_fully_reaped(pid: i32) -> bool {
    // SAFETY: `kill` with signal 0 performs only an existence/permission check;
    // it delivers no signal and mutates no process state.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return false; // alive or zombie — NOT reaped
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Read the pid a stub published (via an atomic temp-write+rename) to `pidfile`.
///
/// The rename means `pidfile` only ever appears with the complete pid bytes, so
/// a single read parses cleanly; the short retry only covers filesystem
/// visibility latency, never a torn write.
#[cfg(unix)]
fn read_published_pid(pidfile: &Path) -> i32 {
    for _ in 0..200 {
        if let Some(pid) = std::fs::read_to_string(pidfile)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("stub never published a parseable pid to {}", pidfile.display());
}

/// (a) A long-running slicer cancelled the instant it is up: `run_slicer` returns
/// `Cancelled` promptly (≪ the 30 s sleep) and the spawned child is reaped — no
/// orphan, no zombie.
///
/// The stub publishes its own pid ($$) atomically (write to `.tmp` then rename),
/// then `exec`s `sleep 30`. `exec` preserves the pid, so the published pid is
/// exactly the process `run_slicer` must reap. The cancel poll is gated on the
/// pidfile existing — this is "pre-cancel" in spirit (cancellation fires as soon
/// as the child is alive) while the atomic publish removes the write-vs-kill race
/// a bare `|| true` would introduce.
#[cfg(unix)]
#[test]
fn run_slicer_cancel_reaps_child_promptly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("child.pid");
    let pf = pidfile.display().to_string();
    let script = format!("echo $$ > '{pf}.tmp'; mv '{pf}.tmp' '{pf}'; exec sleep 30");
    let args = vec!["-c".to_string(), script];

    let cancel = || pidfile.exists();
    let start = Instant::now();
    let outcome = run_slicer(
        Path::new("/bin/sh"),
        &args,
        &cancel,
        Duration::from_millis(200),
    );
    let elapsed = start.elapsed();

    assert!(
        matches!(outcome, SliceRunOutcome::Cancelled),
        "a cancelled run must report Cancelled, got {outcome:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "cancellation must return promptly (≪ the 30 s sleep), took {elapsed:?}"
    );
    let pid = read_published_pid(&pidfile);
    assert!(
        pid_fully_reaped(pid),
        "the spawned child (pid {pid}) must be reaped — no orphan, no zombie"
    );
}

/// (b) A SIGTERM-IGNORING slicer is still cancelled and reaped, via SIGKILL
/// escalation after the grace window.
///
/// The stub installs `trap '' TERM` (so SIGTERM is a no-op), publishes its pid,
/// then blocks ~indefinitely in short 0.2 s hops (a bounded ~30 s loop so a RED
/// run — where the step-6 stub never kills it — self-terminates rather than
/// leaving an immortal looper, and a SIGKILL of the shell leaves at most a
/// ≤0.2 s grandchild). If SIGKILL escalation were missing, the TERM-ignoring
/// child would never die and `run_slicer` could not return `Cancelled` before the
/// loop ends — so `Cancelled` + reaped + ≪30 s together prove the
/// SIGTERM→grace→SIGKILL→`wait` path.
#[cfg(unix)]
#[test]
fn run_slicer_cancel_escalates_to_sigkill_when_term_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("child.pid");
    let pf = pidfile.display().to_string();
    let script = format!(
        "trap '' TERM; echo $$ > '{pf}.tmp'; mv '{pf}.tmp' '{pf}'; \
         n=0; while [ \"$n\" -lt 150 ]; do sleep 0.2; n=$((n+1)); done"
    );
    let args = vec!["-c".to_string(), script];

    let cancel = || pidfile.exists();
    let grace = Duration::from_millis(200);
    let start = Instant::now();
    let outcome = run_slicer(Path::new("/bin/sh"), &args, &cancel, grace);
    let elapsed = start.elapsed();

    assert!(
        matches!(outcome, SliceRunOutcome::Cancelled),
        "a TERM-ignoring run must still report Cancelled (via SIGKILL), got {outcome:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "SIGKILL escalation must bound cancellation well under the ~30 s loop, took {elapsed:?}"
    );
    let pid = read_published_pid(&pidfile);
    assert!(
        pid_fully_reaped(pid),
        "the TERM-ignoring child (pid {pid}) must be SIGKILLed and reaped"
    );
}

// ── step-9: slice_body orchestration (injected stub, no live PrusaSlicer) ────────

/// `slice_body` with a present (stub) slicer composes args, runs the subprocess,
/// and parses the produced G-code into a populated [`reify_fdm::Toolpath`] —
/// delegating the parse to ζ's `parse_prusaslicer_gcode`. The stub copies the
/// committed bracket fixture to the `-o` path, so the resulting toolpath has the
/// fixture's role + layer structure.
#[cfg(unix)]
#[test]
fn slice_body_present_slicer_yields_populated_toolpath() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stub = write_stub_script(dir.path(), "ok-slicer.sh", &emit_fixture_body());
    // The stub ignores the body model (it only reads `-o`); a real slicer would
    // slice it. A real dummy file stands in for the exported body geometry.
    let body = dir.path().join("body.stl");
    std::fs::write(&body, b"solid dummy\nendsolid dummy\n").expect("write dummy body");

    let tp = slice_body(
        Some(&stub),
        &body,
        &sample_settings(),
        &|| false,
        Duration::from_millis(500),
    )
    .expect("slice_body must succeed with a present stub slicer");

    assert!(!tp.beads.is_empty(), "slice_body must produce beads");
    let count = |role| tp.beads.iter().filter(|b| b.role == role).count();
    assert!(count(BeadRole::Perimeter) > 0, "≥1 perimeter bead");
    assert!(
        count(BeadRole::SolidInfill) + count(BeadRole::SparseInfill) > 0,
        "≥1 infill bead (solid or sparse)"
    );
    assert!(tp.layers.len() >= 2, "≥2 layers, got {}", tp.layers.len());
}

/// `slice_body` with no slicer (`bin == None`) returns
/// [`SliceError::SlicerUnavailable`] — the W_FDM_SLICER_UNAVAILABLE trigger —
/// without panicking and without spawning anything.
#[test]
fn slice_body_absent_slicer_is_slicer_unavailable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = dir.path().join("body.stl");

    let result = slice_body(
        None,
        &body,
        &sample_settings(),
        &|| false,
        Duration::from_millis(500),
    );
    assert!(
        matches!(result, Err(SliceError::SlicerUnavailable)),
        "an absent slicer must yield SlicerUnavailable, got {result:?}"
    );
}

// ── step-11: determinism-locked golden (core half, no live PrusaSlicer) ──────────

/// Absolute path to the committed canonical Toolpath golden snapshot.
fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/toolpath_bracket.golden")
}

/// The canonical Toolpath serialization is **byte-stable run-to-run** and matches
/// the committed golden snapshot.
///
/// This is the determinism half that needs no live slicer: `parse_prusaslicer_gcode`
/// is a pure, deterministic function and `Toolpath` holds only order-stable `Vec`s
/// (adjacency is sorted+deduped by ζ, no HashMap/HashSet in the output), so parsing
/// the committed fixture twice and serializing each yields identical bytes — and
/// that serialization is locked against `toolpath_bracket.golden`.
///
/// Regenerate the golden after an intended serialization change with:
/// `REIFY_UPDATE_GOLDEN=1 cargo test -p reify-fdm --test slice toolpath_serialization`.
#[test]
fn toolpath_serialization_is_deterministic_and_golden_locked() {
    let src = fixture_gcode();

    let tp1 = parse_prusaslicer_gcode(&src).expect("fixture must parse");
    let s1 = serialize_toolpath_canonical(&tp1);
    let tp2 = parse_prusaslicer_gcode(&src).expect("fixture must parse");
    let s2 = serialize_toolpath_canonical(&tp2);
    assert_eq!(
        s1, s2,
        "canonical serialization must be byte-identical run-to-run"
    );

    if std::env::var_os("REIFY_UPDATE_GOLDEN").is_some() {
        std::fs::write(golden_path(), &s1).expect("write golden snapshot");
        return;
    }

    let golden = std::fs::read_to_string(golden_path()).expect(
        "golden snapshot must exist — regenerate with \
         REIFY_UPDATE_GOLDEN=1 cargo test -p reify-fdm --test slice toolpath_serialization",
    );
    assert_eq!(
        s1, golden,
        "canonical serialization must match the committed golden snapshot"
    );
}
