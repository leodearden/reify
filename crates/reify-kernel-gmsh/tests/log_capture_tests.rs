//! Tests for `reify_kernel_gmsh::log_capture` — the discipline that folds
//! gmsh's captured message stream into a failing operation's error.
//!
//! The `annotated` cases below are PURE: they call no gmsh function, take no
//! `init::GMSH_LOCK`, and touch no process-global option, so they neither
//! serialise against nor perturb the rest of this crate's gmsh tests. That
//! separation is the point of `annotated` being a free function over
//! `&[String]` rather than a method reading the live capture buffer.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_ir::GeometryError;
use reify_kernel_gmsh::log_capture::{LogCapture, MAX_APPENDED_LOG_LINES, annotated};
use reify_kernel_gmsh::{ffi, init};

/// Unwrap the `OperationFailed` payload, or fail naming what came back.
fn operation_failed_message(err: GeometryError) -> String {
    match err {
        GeometryError::OperationFailed(msg) => msg,
        other => panic!("expected GeometryError::OperationFailed, got: {other:?}"),
    }
}

/// A capture that produced nothing must leave the message byte-identical —
/// no header, no trailing newline, no "0 lines" noise. `LogCapture::armed`
/// is best-effort, so this is also the path a failed `logger_start` degrades
/// to: the caller gets exactly the error it would have got without capture.
#[test]
fn an_empty_capture_leaves_the_message_untouched() {
    let msg = operation_failed_message(annotated(
        GeometryError::OperationFailed("boom".into()),
        &[],
    ));
    assert_eq!(
        msg, "boom",
        "an empty capture must add no header and no noise",
    );
}

/// Under the cap: the original last-error annotation is PRESERVED as the
/// message prefix, and every captured line follows it, each on its own line
/// and in capture order.
///
/// Order is asserted by increasing `find` offsets and "own line" by matching
/// trimmed whole lines, so the test pins the reading ORDER and the line
/// BREAKS without pinning the indent — an indent change is a presentation
/// choice, not a contract.
#[test]
fn an_under_cap_capture_appends_every_line_in_order_after_the_original_message() {
    let original = "gmshModelMeshGenerate: ierr=1 (HXT 3D mesh failed)";
    let lines = vec![
        "Info: Classifying surfaces".to_string(),
        "Info: Meshing 3D...".to_string(),
        "Error: HXT 3D mesh failed".to_string(),
    ];

    let msg = operation_failed_message(annotated(
        GeometryError::OperationFailed(original.into()),
        &lines,
    ));

    assert!(
        msg.starts_with(original),
        "the pre-existing last-error annotation must be preserved as the prefix; got: {msg}",
    );

    let header = format!("gmsh log ({} of {} lines):", lines.len(), lines.len());
    let header_at = msg
        .find(&header)
        .unwrap_or_else(|| panic!("expected header {header:?} in: {msg}"));

    // Scan forward from the header so the third line's text cannot be
    // satisfied by the identical phrase inside `original`.
    let mut cursor = header_at + header.len();
    for line in &lines {
        let at = msg[cursor..]
            .find(line.as_str())
            .map(|offset| cursor + offset)
            .unwrap_or_else(|| panic!("captured line {line:?} missing or out of order in: {msg}"));
        cursor = at + line.len();
        assert_eq!(
            msg.lines().filter(|l| l.trim() == line).count(),
            1,
            "captured line {line:?} must appear exactly once, on a line of its own; got: {msg}",
        );
    }
}

/// Over the cap: gmsh's diagnosis sits at the END of a capture, so the tail
/// is what survives. The header reports `{shown} of {total}` uniformly, so a
/// reader never has to infer whether elision happened.
///
/// Every bound is derived from `MAX_APPENDED_LOG_LINES` rather than written
/// as a literal, so the test cannot drift away from the implementation's cap.
#[test]
fn an_over_cap_capture_keeps_the_tail_and_reports_the_full_count() {
    let total = MAX_APPENDED_LOG_LINES + 60;
    let lines: Vec<String> = (0..total).map(|i| format!("line-{i}")).collect();

    let msg = operation_failed_message(annotated(
        GeometryError::OperationFailed("boom".into()),
        &lines,
    ));

    let header = format!("gmsh log ({MAX_APPENDED_LOG_LINES} of {total} lines):");
    assert!(
        msg.contains(&header),
        "expected header {header:?} in: {msg}",
    );

    let first_kept = format!("line-{}", total - MAX_APPENDED_LOG_LINES);
    let last_kept = format!("line-{}", total - 1);
    let last_elided = format!("line-{}", total - MAX_APPENDED_LOG_LINES - 1);
    assert!(
        msg.contains(&first_kept) && msg.contains(&last_kept),
        "the kept tail must run from {first_kept:?} to {last_kept:?}; got: {msg}",
    );
    assert!(
        !msg.contains("line-0") && !msg.contains(&last_elided),
        "the elided head must be absent, through {last_elided:?}; got: {msg}",
    );
}

/// Exactly at the cap: nothing is elided, and the header reports
/// `{shown} of {total}` in the same uniform form as the two cases either
/// side of it.
///
/// This is the boundary neither of those can see. Here
/// `lines.len().saturating_sub(MAX_APPENDED_LOG_LINES)` is 0, so an
/// off-by-one in the slice pivot would silently drop `line-0` while leaving
/// both the under-cap and over-cap tests green.
#[test]
fn an_exactly_at_cap_capture_keeps_every_line() {
    let lines: Vec<String> = (0..MAX_APPENDED_LOG_LINES)
        .map(|i| format!("line-{i}"))
        .collect();

    let msg = operation_failed_message(annotated(
        GeometryError::OperationFailed("boom".into()),
        &lines,
    ));

    let header = format!("gmsh log ({MAX_APPENDED_LOG_LINES} of {MAX_APPENDED_LOG_LINES} lines):");
    assert!(
        msg.contains(&header),
        "expected header {header:?} in: {msg}",
    );
    assert!(
        msg.lines().any(|l| l.trim() == "line-0"),
        "nothing may be elided at exactly the cap, but the first line is missing; got: {msg}",
    );
    assert_eq!(
        msg.lines().filter(|l| l.trim().starts_with("line-")).count(),
        MAX_APPENDED_LOG_LINES,
        "every captured line must survive at exactly the cap; got: {msg}",
    );
}

/// Only `OperationFailed` carries a gmsh message worth extending. Every
/// other variant passes through untouched — appending a log tail to, say, an
/// `InvalidReference(handle)` would change the error's shape for no gain.
#[test]
fn a_non_operation_failed_error_passes_through_unchanged() {
    let err = annotated(
        GeometryError::InitFailed("nope".into()),
        &["Info: x".to_string()],
    );
    match err {
        GeometryError::InitFailed(msg) => assert_eq!(
            msg, "nope",
            "a non-OperationFailed payload must not be rewritten",
        ),
        other => panic!("annotated must not change the error variant, got: {other:?}"),
    }
}

/// The guard's whole observable contract, through its public surface only:
/// `annotate` reads the LIVE capture (not an empty list), and `drop` stops
/// it (not "the test remembered to").
///
/// Deliberately does NOT touch `"General.Terminal"`: capture is independent
/// of it — see `ffi::logger_start`'s measurement and
/// `ffi_smoke_tests::gmsh_logger_captures_mesh_generate_output_even_with_terminal_silenced`
/// — so this test needs no option-restoring guard of its own and leaves no
/// process-global option behind. It is the only test in this file that takes
/// `GMSH_LOCK`; the `annotated` cases above are pure.
#[test]
fn log_capture_guard_folds_captured_lines_into_the_error_and_stops_on_drop() {
    // Recover from a poisoned lock the way `mesh_to_volume` does: the
    // `ffi::clear()` below wipes any half-built model a panicked prior test
    // left behind.
    let guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();

    let annotated_err = {
        let capture = LogCapture::armed(&guard);
        // `ffi::clear()` is the "something that logs" on purpose. It needs no
        // geometry, costs ~0ms, and — unlike any `mesh_generate` — cannot be
        // silenced by the mesher-poisoning hazard its sibling test in this
        // binary provokes (see that test's "Why this test is not in
        // mesh_to_volume_tests.rs"). It also doubles as this test's cleanup.
        ffi::clear().expect("ffi::clear failed");
        capture.annotate(GeometryError::OperationFailed("boom".into()))
    };

    let msg = operation_failed_message(annotated_err);
    assert!(
        msg.contains("boom"),
        "annotate must keep the caller's message; got: {msg}",
    );
    assert!(
        msg.contains("Info: Clearing all models"),
        "annotate must drain the LIVE capture, not format an empty list; got: {msg}",
    );

    // Still under GMSH_LOCK, after the guard dropped. `logger_stop` both
    // stops capture AND drains the buffer, so an empty read here is the
    // direct witness that `drop` fired: without it those 3+ lines would
    // still be buffered and would surface as some later caller's diagnosis.
    let leftover = ffi::logger_get().expect("ffi::logger_get failed");
    assert!(
        leftover.is_empty(),
        "dropping LogCapture must stop and drain the capture; {} lines left: {leftover:?}",
        leftover.len(),
    );
}
