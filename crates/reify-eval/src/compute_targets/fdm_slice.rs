// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline for `fdm::slice` — the PrusaSlicer-subprocess ComputeNode that
//! turns an FDM body + `FDMProcess` into a structured `Toolpath` value (task η /
//! 3789, slice 2 of `docs/prds/v0_5/fdm-as-printed-fea.md`).
//!
//! Mirrors the task-δ split (`as_printed_material.rs`): the pure subprocess core
//! (discover / compose / run / parse) lives in `reify_fdm::slice`; this module
//! holds the eval-side trampoline, the `Toolpath → Value::StructureInstance`
//! marshalling, and the full-reslice-with-cache warm state.
//!
//! When PrusaSlicer is absent from `$PATH` (the W_FDM_SLICER_UNAVAILABLE case,
//! PRD open Q4) the node degrades honestly: it still emits a (degraded/empty)
//! `Toolpath` value plus a single `Severity::Info` diagnostic carrying
//! `DiagnosticCode::FdmSlicerUnavailable` — never an error.
//
// The implementation is built incrementally across task η steps 13–18; this
// placeholder keeps the module well-formed before the first RED test lands.
