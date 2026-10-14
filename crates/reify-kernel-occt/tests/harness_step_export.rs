//! Consolidated integration-test harness for this crate's STEP **export-path** tests.
//!
//! The crate's THIRD harness root. It was ORIGINATED under rule (a) of the 20 kLOC
//! compile-unit cap enforced by `tests/infra/test_harness_kloc_cap.sh`: `harness_occt.rs` had
//! consolidated all 51 former standalone binaries in this crate (#5277), and adding this
//! task's module to it pushed that unit to 20048/20000. The cap's sanctioned remedy is a
//! SPLIT into a second `harness_<subsystem2>.rs` — "never allowed to balloon unbounded, and
//! never accommodated by raising the cap".
//!
//! THAT ARITHMETIC IS SPENT; the split is not. Task #7466 independently split `harness_occt`
//! along the `tests/common/` seam into `harness_occt.rs` + `harness_occt_measurement.rs`, so
//! `harness_occt` now measures well under the cap and would absorb this module without
//! reaching it. Read the live sizes off the guard, never off a number quoted here. What
//! survives #7466 is the SUBSYSTEM seam below, which is the load-bearing reason this root
//! exists — so do not fold it back on the grounds that the headroom came back.
//!
//! The subsystem seam is the export path, which is why the split lands on this boundary
//! rather than an arbitrary line-count cut: `step_plane_angle_guard_integration` is the
//! crate's first STEP-export module, and it shares no helper with the geometry/topology
//! predicates that fill `harness_occt`. The cut is also what keeps the split cheap — nothing
//! under this root references `tests/common/`, so there is deliberately no `mod common;`
//! below. Post-#7466 that helper is declared by `harness_occt_measurement.rs` alone, so it is
//! still compiled ONCE for the crate, not once per root.
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
