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
// The implementation is built incrementally across task η steps 1–12; this
// placeholder keeps the module well-formed before the first RED test lands.
