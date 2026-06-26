// SPDX-License-Identifier: AGPL-3.0-or-later

//! PrusaSlicer subprocess invocation core (task η).
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` task η (slice 2). This module is
//! the pure-ish subprocess half of the `fdm::slice` ComputeNode: it discovers a
//! PrusaSlicer binary on `$PATH`, composes a deterministic settings/CLI profile,
//! runs the slicer **as a subprocess** (never FFI — AGPL boundary, PRD DD#4)
//! with cooperative SIGTERM→SIGKILL cancellation, and parses the resulting
//! G-code into a [`crate::Toolpath`] (delegating to ζ's
//! [`crate::parse_prusaslicer_gcode`]).
//!
//! The cancellation signal is a `Fn() -> bool` closure, NOT a
//! `reify_eval::CancellationHandle`: reify-fdm must not depend on reify-eval
//! (the reverse dependency edge). The eval-side trampoline
//! (`reify-eval/src/compute_targets/fdm_slice.rs`) supplies
//! `|| cancellation.is_cancelled()`.
//
// The implementation is built incrementally across task η steps 1–12.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::{InfillPattern, Toolpath, ToolpathParseError, parse_prusaslicer_gcode};

/// The canonical PrusaSlicer binary names probed on `$PATH`, in priority order.
///
/// Covers the GUI binary (`prusa-slicer`), the headless console binary
/// (`prusa-slicer-console`, the Windows/AppImage CLI name), and the
/// CamelCase / lowercase spellings shipped by various distributions /
/// AppImages. The eval-side trampoline passes this slice to
/// [`discover_slicer`]; tests pass their own subset.
pub const DEFAULT_SLICER_NAMES: &[&str] = &[
    "prusa-slicer",
    "prusa-slicer-console",
    "PrusaSlicer",
    "prusaslicer",
];

/// Discover a PrusaSlicer binary on the `$PATH`-style `path_var` string.
///
/// Splits `path_var` on the platform path separator (`:` on unix, `;` on
/// Windows) via [`std::env::split_paths`], then for each non-empty directory
/// tries each `candidates` name in order, returning the first `dir/name` that
/// exists and is an executable regular file. Returns `None` when no candidate is
/// found — the W_FDM_SLICER_UNAVAILABLE trigger (PRD open Q4).
///
/// Iteration is **directory-major**: every candidate is tried in `$PATH`
/// directory order, so an earlier `$PATH` entry wins over a later one
/// regardless of candidate priority (standard `$PATH` lookup semantics).
///
/// On unix "executable" means a regular file with at least one of the three
/// execute bits set (`mode & 0o111 != 0`); on other platforms it means any
/// existing regular file (the execute bit has no unix-style meaning there).
pub fn discover_slicer(path_var: &str, candidates: &[&str]) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            // An empty `$PATH` component (e.g. `""` or a `::`) conventionally
            // means "the current directory"; we deliberately skip it so a bare
            // candidate name can never be resolved relative to the CWD.
            continue;
        }
        for &name in candidates {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// `true` iff `path` is an existing regular file that is executable.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

/// `true` iff `path` is an existing regular file (non-unix: no execute bit).
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

// ── Settings → CLI args ────────────────────────────────────────────────────────

/// The mechanically-relevant subset of an `FDMProcess` that drives the slicer
/// CLI (PRD open Q5). Field names mirror the stdlib `FDMProcess`; the eval-side
/// trampoline (`reify-eval`) reads them off the `FDMProcess` value.
///
/// Only the parameters that change the deposited bead geometry are carried —
/// build direction, base material, and provenance are not slicer inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceSettings {
    /// Slicer layer thickness in mm → `--layer-height`.
    pub layer_height: f64,
    /// Perimeter shell count → `--perimeters`.
    pub walls: u32,
    /// Solid top/bottom layer count → `--top-solid-layers` + `--bottom-solid-layers`.
    pub top_bottom_layers: u32,
    /// Infill volume fraction in `(0, 1]` → `--fill-density <pct>%`.
    pub infill_density: f64,
    /// Infill geometry → `--fill-pattern` (see [`infill_pattern_arg`]).
    pub infill_pattern: InfillPattern,
}

/// Map an [`InfillPattern`] to its PrusaSlicer `--fill-pattern` string.
///
/// The strings are PrusaSlicer's canonical `InfillPattern` config values
/// (`src/libslic3r/PrintConfig.cpp`): note `Triangular → "triangles"` (PrusaSlicer
/// names the triangular pattern `triangles`, not `triangular`).
pub fn infill_pattern_arg(pattern: InfillPattern) -> &'static str {
    match pattern {
        InfillPattern::Gyroid => "gyroid",
        InfillPattern::Cubic => "cubic",
        InfillPattern::Grid => "grid",
        InfillPattern::Triangular => "triangles",
        InfillPattern::Honeycomb => "honeycomb",
    }
}

/// Compose the pinned, deterministic PrusaSlicer CLI argument vector for
/// `settings`, writing the G-code to `out_path`.
///
/// Each flag and its value is a separate `Vec` element (the
/// [`std::process::Command::args`] convention). The order is **fixed** and the
/// run is pinned single-threaded (`--threads 1`) so the produced G-code is
/// reproducible run-to-run — the precondition for the verify-and-lock golden
/// (PRD task η). The body STL/source path is supplied separately by the caller
/// (it is the trailing positional arg appended by [`run_slicer`]); this function
/// owns only the settings → flag mapping and the explicit `-o` output path.
pub fn compose_slicer_args(settings: &SliceSettings, out_path: &Path) -> Vec<String> {
    let tb = settings.top_bottom_layers.to_string();
    vec![
        // Export sliced G-code (not the 3MF project).
        "--export-gcode".to_string(),
        "--layer-height".to_string(),
        fmt_num(settings.layer_height),
        "--perimeters".to_string(),
        settings.walls.to_string(),
        "--top-solid-layers".to_string(),
        tb.clone(),
        "--bottom-solid-layers".to_string(),
        tb,
        "--fill-density".to_string(),
        format!("{}%", fmt_num(settings.infill_density * 100.0)),
        "--fill-pattern".to_string(),
        infill_pattern_arg(settings.infill_pattern).to_string(),
        // Determinism pin: single-threaded slicing → reproducible G-code.
        "--threads".to_string(),
        "1".to_string(),
        "-o".to_string(),
        out_path.to_string_lossy().into_owned(),
    ]
}

/// Format an `f64` as a short, deterministic decimal: fixed 6-decimal rendering
/// with trailing zeros (and a bare trailing dot) trimmed.
///
/// Rounding to 6 decimals before trimming absorbs the binary-float noise that a
/// raw `{}` would surface (e.g. `0.2 * 100.0 == 20.000000000000004` → `"20"`),
/// keeping the composed args byte-stable across runs.
fn fmt_num(x: f64) -> String {
    let s = format!("{x:.6}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

// ── Subprocess run + cooperative cancellation ──────────────────────────────────

/// Outcome of a single [`run_slicer`] invocation.
#[derive(Debug)]
pub enum SliceRunOutcome {
    /// The slicer exited 0 and produced G-code (read from the `-o` output path).
    Completed {
        /// The produced G-code source, verbatim.
        gcode: String,
    },
    /// The run was cancelled via `cancel_poll` (the child was reaped — no orphan).
    Cancelled,
    /// The slicer could not be spawned, exited non-zero, or produced no output.
    Failed {
        /// Human-readable failure summary (spawn error / exit status).
        message: String,
    },
}

/// Coarse poll interval for the wait/cancel loop.
const RUN_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Spawn `bin` with `args`, waiting to completion while polling `cancel_poll` at
/// a coarse interval.
///
/// On normal exit: a zero status reads the G-code from the `-o` output path the
/// `args` carry → [`SliceRunOutcome::Completed`]; a non-zero status or missing
/// output → [`SliceRunOutcome::Failed`]. A spawn error (e.g.
/// [`std::io::ErrorKind::NotFound`]) is also `Failed`. When `cancel_poll()`
/// becomes true the run is cancelled with a bounded SIGTERM→`grace`→SIGKILL
/// escalation that reaps the child (no orphan/zombie) — see [`cancel_child`].
///
/// `cancel_poll` is a plain `Fn() -> bool` closure rather than a
/// `reify_eval::CancellationHandle` so this crate stays free of the reverse
/// `reify-fdm → reify-eval` dependency edge; the trampoline supplies
/// `|| cancellation.is_cancelled()`.
pub fn run_slicer(
    bin: &Path,
    args: &[String],
    cancel_poll: &dyn Fn() -> bool,
    grace: Duration,
) -> SliceRunOutcome {
    // stdout/stderr → null: the G-code is read from the `-o` file, and draining
    // pipes inside the poll loop would risk a fill-buffer deadlock on a chatty
    // slicer. Keeping them unpiped sidesteps that entirely.
    let mut child = match Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return SliceRunOutcome::Failed {
                message: format!("failed to spawn slicer {}: {e}", bin.display()),
            };
        }
    };

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return finish_run(status, args),
            Ok(None) => {}
            Err(e) => {
                // Best-effort reap before surfacing the wait error.
                let _ = child.kill();
                let _ = child.wait();
                return SliceRunOutcome::Failed {
                    message: format!("error waiting on slicer: {e}"),
                };
            }
        }
        if cancel_poll() {
            return cancel_child(child, grace);
        }
        std::thread::sleep(RUN_POLL_INTERVAL);
    }
}

/// Map a finished slicer process's exit status to an outcome.
fn finish_run(status: ExitStatus, args: &[String]) -> SliceRunOutcome {
    if status.success() {
        match read_output_gcode(args) {
            Some(gcode) => SliceRunOutcome::Completed { gcode },
            None => SliceRunOutcome::Failed {
                message: "slicer exited 0 but produced no readable -o output".to_string(),
            },
        }
    } else {
        SliceRunOutcome::Failed {
            message: format!("slicer exited with non-zero {status}"),
        }
    }
}

/// The value following the first `-o` flag in `args`, if any.
fn output_path_from_args(args: &[String]) -> Option<&str> {
    let mut prev: Option<&str> = None;
    for a in args {
        if prev == Some("-o") {
            return Some(a);
        }
        prev = Some(a);
    }
    None
}

/// Read the G-code the slicer wrote to its `-o` output path.
fn read_output_gcode(args: &[String]) -> Option<String> {
    std::fs::read_to_string(output_path_from_args(args)?).ok()
}

/// Handle a cancellation request observed mid-run: stop `child` with a bounded
/// SIGTERM→`grace`→SIGKILL escalation and **always** reap it (no orphan, no
/// zombie), then report [`SliceRunOutcome::Cancelled`].
///
/// On unix the polite first step is SIGTERM via [`libc::kill`] —
/// [`std::process::Child::kill`] only ever sends SIGKILL, so a graceful stop
/// needs the explicit signal. The child is then polled for up to `grace`; if it
/// has not exited, it is force-killed with SIGKILL (`child.kill()`, which is
/// uncatchable). A final blocking `child.wait()` reaps the process on **every**
/// path — including the SIGTERM-honoured fast path, where [`Child::try_wait`] has
/// already collected the status (a second `wait()` then returns the cached status
/// without a syscall). The non-unix fallback has no portable graceful-signal API,
/// so it goes straight to `child.kill()` + `wait()`.
///
/// Cancellation is only reached from [`run_slicer`]'s loop immediately after a
/// `try_wait` that reported the child still running, so `child.id()` is a live,
/// not-yet-reaped pid — no pid-reuse hazard on the SIGTERM.
fn cancel_child(mut child: Child, grace: Duration) -> SliceRunOutcome {
    #[cfg(unix)]
    {
        // SAFETY: `kill` only delivers a signal to the given pid; a child that
        // has since exited yields ESRCH, which we deliberately ignore (the
        // unconditional `wait()` below still reaps it).
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        if wait_within(&mut child, grace).is_none() {
            // Still alive after the grace window → escalate to SIGKILL.
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        // No portable SIGTERM: force-kill directly.
        let _ = child.kill();
    }
    // Reap unconditionally so no zombie/orphan survives this call.
    let _ = child.wait();
    SliceRunOutcome::Cancelled
}

/// Poll `child` for exit for up to `grace`, returning its [`ExitStatus`] if it
/// exits in time or `None` if it is still alive when `grace` elapses.
///
/// A `try_wait` error is treated like "still alive" (→ `None`) so the caller
/// escalates to SIGKILL rather than leaking a process on a transient wait error.
fn wait_within(child: &mut Child, grace: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(RUN_POLL_INTERVAL);
    }
}

// ── slice_body orchestration ────────────────────────────────────────────────────

/// A failure (or non-completion) of [`slice_body`].
#[derive(Debug)]
pub enum SliceError {
    /// No PrusaSlicer binary was available (`bin == None`) — the
    /// W_FDM_SLICER_UNAVAILABLE trigger (PRD open Q4). The eval-side trampoline
    /// turns this into a degraded (empty) Toolpath + an `Info` diagnostic, never
    /// a hard error.
    SlicerUnavailable,
    /// The run was cancelled cooperatively via `cancel_poll` (child reaped).
    Cancelled,
    /// The slicer failed to spawn, exited non-zero, or produced no G-code.
    Run(String),
    /// The produced G-code could not be parsed into a [`Toolpath`].
    Parse(ToolpathParseError),
    /// Allocating the temp output directory/path failed.
    Io(String),
}

impl std::fmt::Display for SliceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SliceError::SlicerUnavailable => write!(f, "no PrusaSlicer binary available"),
            SliceError::Cancelled => write!(f, "slice cancelled"),
            SliceError::Run(m) => write!(f, "slicer run failed: {m}"),
            SliceError::Parse(e) => write!(f, "g-code parse failed: {e}"),
            SliceError::Io(m) => write!(f, "slice i/o error: {m}"),
        }
    }
}

impl std::error::Error for SliceError {}

/// Orchestrate a full slice: compose the deterministic CLI for `settings`, run
/// `bin` as a subprocess on `body_source`, and parse the produced G-code into a
/// [`Toolpath`] (via ζ's [`parse_prusaslicer_gcode`]).
///
/// `bin` is the **already-resolved** slicer path — the eval-side trampoline does
/// the [`discover_slicer`] step and passes the result here. `None`
/// short-circuits to [`SliceError::SlicerUnavailable`] without spawning anything
/// (the slicer-absent path). The G-code is written to a unique temp output path
/// (auto-removed when this call returns, after [`run_slicer`] has read it back
/// into memory); `cancel_poll`/`grace` are forwarded to [`run_slicer`] for the
/// cooperative SIGTERM→SIGKILL cancellation.
///
/// `body_source` is the exported body model (STL/3MF/…) passed to the slicer as
/// the trailing positional argument; the composed flags own only the settings →
/// flag mapping and the explicit `-o` output path.
pub fn slice_body(
    bin: Option<&Path>,
    body_source: &Path,
    settings: &SliceSettings,
    cancel_poll: &dyn Fn() -> bool,
    grace: Duration,
) -> Result<Toolpath, SliceError> {
    let bin = bin.ok_or(SliceError::SlicerUnavailable)?;

    // Unique temp directory for the produced G-code; removed on drop. Held in a
    // binding until after `run_slicer` returns (it reads the output back into the
    // returned `String`, so the file is no longer needed past that point).
    let out_dir = tempfile::tempdir().map_err(|e| SliceError::Io(e.to_string()))?;
    let out_path = out_dir.path().join("reify-slice.gcode");

    // settings → pinned CLI flags, then the body model as the trailing positional.
    let mut args = compose_slicer_args(settings, &out_path);
    args.push(body_source.to_string_lossy().into_owned());

    match run_slicer(bin, &args, cancel_poll, grace) {
        SliceRunOutcome::Completed { gcode } => {
            parse_prusaslicer_gcode(&gcode).map_err(SliceError::Parse)
        }
        SliceRunOutcome::Cancelled => Err(SliceError::Cancelled),
        SliceRunOutcome::Failed { message } => Err(SliceError::Run(message)),
    }
}
