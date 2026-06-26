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

use crate::InfillPattern;

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
