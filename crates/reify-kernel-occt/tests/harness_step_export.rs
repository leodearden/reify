//! Consolidated integration-test harness for this crate's STEP **export-path** tests.
//!
//! The crate's second harness root, and deliberately so. `harness_occt.rs` consolidated all
//! 51 former standalone binaries in this crate (#5277) and has since grown to ~95% of the
//! 20 kLOC compile-unit cap enforced by `tests/infra/test_harness_kloc_cap.sh`; adding this
//! task's module to it pushed the unit to 20048/20000. The cap's sanctioned remedy for that
//! is the one applied here — "a harness that grows past the cap must be SPLIT into a second
//! `harness_<subsystem2>.rs`, never allowed to balloon unbounded, and never accommodated by
//! raising the cap". So do NOT fold this root back into `harness_occt.rs`: that re-reds the
//! gate. Add a STEP-export module here instead.
//!
//! The subsystem seam is the export path, which is why the split lands on this boundary
//! rather than an arbitrary line-count cut: `step_plane_angle_guard_integration` is the
//! crate's first STEP-export module, and it shares no helper with the geometry/topology
//! predicates that fill `harness_occt`. The cut is also what keeps the split cheap — nothing
//! under this root references `tests/common/`, so there is deliberately no `mod common;`
//! below and the 1159-line shared helper is compiled ONCE for the crate, not twice.
//!
//! Explicit `#[path]` is mandatory here, exactly as in `harness_occt.rs`: this file is an
//! integration-test crate root, where a bare `mod <stem>;` resolves to a sibling
//! `tests/<stem>.rs` rather than into the `harness_step_export/` subdir.
//!
//! This root is deliberately NOT `#![cfg(...)]`-gated. Each module carries its own retained
//! inner `#![cfg(...)]` attribute, which reproduces that module's exact compile matrix and
//! preserves its `<stem>::<test>` module path (and thus every nextest `test(/^<stem>::/)`
//! NAME filterset). Gating the root instead would silently widen those conditions to every
//! future module added here.

#[path = "harness_step_export/step_plane_angle_guard_integration.rs"]
mod step_plane_angle_guard_integration;
