//! Consolidated integration-test harness for the reify-kernel-occt tests that CONSUME the
//! shared `tests/common/` helpers — the family split out of `harness_occt` by task #7466.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet): see
//! `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1/C2 — kept there, not restated here.
//! `harness_occt` measured 19700 lines (156 root + 18385 across 56 module files + 1159
//! external) against CAP_LINES = 20000, i.e. 98% of the cap with 300 lines of headroom, and
//! WARNing on every run. Rule (a)'s own remedy is a SPLIT into a second
//! `harness_<subsystem>.rs` — never a CAP_LINES bump, and never a
//! `tests/infra/harness-layout-baseline.manifest` grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to grow).
//!
//! THE SEAM is a rule you can check, not a theme you have to judge: a test belongs here iff
//! it consumes ANY of the shared `tests/common/` helpers. `grep -rn 'common::'
//! crates/reify-kernel-occt/tests/harness_occt/` is empty today and must stay empty; the
//! compiler enforces that direction, because `harness_occt` no longer declares `mod common;`
//! at all, so a new `crate::common` use over there simply fails to build.
//!
//! That rule is also what the split buys, and it is why the CONSUMERS are the side that
//! moved even though they are the smaller side (16 modules against 40). The guard's own rule
//! (a) remedy for an `external_lines`-bearing unit is to "move the including submodules — and
//! the include with them — into their own harness": this root carries the bare `mod common;`,
//! so the 1159-line external include is charged to THIS unit alone and `harness_occt` now
//! measures `external_files = 0`. Had the non-consumers moved instead, both roots would have
//! needed the include, and rustc really does compile a separate copy per test binary — 1159
//! duplicated lines, counted twice under the C2 cap.
//!
//! `mod common;` above is bare (no `#[path]`) — the ONE principled exception Section 6 of the
//! kLOC guard encodes. `common` was deliberately NOT moved under a harness directory; it is a
//! retained `tests/` SIBLING at `tests/common/mod.rs`, and crate-root-relative resolution
//! lands on it precisely because it is still there. Every OTHER module below needs explicit
//! `#[path]`: this harness root is an integration-test crate root, where a bare `mod <file>;`
//! would resolve to a sibling `tests/<file>.rs`, not into the `harness_occt_measurement/`
//! subdir.
//!
//! What the rule selects, descriptively: 13 of the 16 members assert on the parsed result of
//! a geometry QUERY — they are the consumers of [`common::parse_bbox`] / `bbox_of` and
//! [`common::parse_xyz`] / `xyz_of`, the JSON parsers `tests/common/mod.rs` exists to hold.
//! The remaining 3 (`chamfer_with_history_integration`, `fillet_with_history_integration`,
//! `local_feature_helper_contract`) consume the local-feature HISTORY-record assertions from
//! the same file instead. So "asserts on a measured quantity" is NOT the criterion and never
//! was — "uses `common::`" is. Both harnesses continue to share `tests/fixtures/`, which costs
//! nothing under C2: it counts only `.rs` files reached by a `mod` / `#[path]` declaration.
//!
//! Layout-only (invariant I3): no `#[test]` fn is added, removed or renamed by this split,
//! and every module keeps its stem, so each `<file>::<test>` module path — and thus every
//! `test(/^<file>::/)`-shaped filterset — resolves exactly as before. Only the binary id
//! moves, from `reify-kernel-occt::harness_occt` to
//! `reify-kernel-occt::harness_occt_measurement`. No `binary(...)` / `--test` selector
//! anywhere in this repo names either one.
//!
//! cfg retention: 15 of the 16 members carry a crate-level `#![cfg(has_occt)]` and
//! `face_differential_integration` carries `#![cfg(all(has_occt, feature = "test-fixtures"))]`.
//! Those inner attributes are retained VERBATIM on the moved submodules rather than hoisted to
//! an outer `#[cfg]` on the `mod` declarations below — an inner `#![cfg(...)]` on a
//! `#[path]`-loaded submodule compiles correctly in every cfg state and preserves the
//! `<file>::<test>` module path. This root is therefore not itself `#![cfg(has_occt)]`-gated,
//! matching `harness_occt`; gating it would be redundant with, not equivalent to, the members'
//! own attributes. `tests/common/mod.rs` carries its own `#![cfg(has_occt)]`, so the shared
//! helpers vanish in the same cfg state their only consumers do.

mod common;

#[path = "harness_occt_measurement/analytic_datum_tests.rs"]
mod analytic_datum_tests;
#[path = "harness_occt_measurement/boolean_result_normalization_integration.rs"]
mod boolean_result_normalization_integration;
#[path = "harness_occt_measurement/chamfer_with_history_integration.rs"]
mod chamfer_with_history_integration;
#[path = "harness_occt_measurement/closest_point_on_shape_integration.rs"]
mod closest_point_on_shape_integration;
#[path = "harness_occt_measurement/extrude_infinite_integration.rs"]
mod extrude_infinite_integration;
#[path = "harness_occt_measurement/extrude_symmetric_integration.rs"]
mod extrude_symmetric_integration;
#[path = "harness_occt_measurement/face_differential_integration.rs"]
mod face_differential_integration;
#[path = "harness_occt_measurement/fillet_with_history_integration.rs"]
mod fillet_with_history_integration;
#[path = "harness_occt_measurement/local_feature_helper_contract.rs"]
mod local_feature_helper_contract;
#[path = "harness_occt_measurement/loft_guided_integration.rs"]
mod loft_guided_integration;
#[path = "harness_occt_measurement/loft_with_history_integration.rs"]
mod loft_with_history_integration;
#[path = "harness_occt_measurement/per_edge_chamfer.rs"]
mod per_edge_chamfer;
#[path = "harness_occt_measurement/reflection_det_negative_integration.rs"]
mod reflection_det_negative_integration;
#[path = "harness_occt_measurement/shell_open_curated_faces.rs"]
mod shell_open_curated_faces;
#[path = "harness_occt_measurement/sweep_guided_integration.rs"]
mod sweep_guided_integration;
#[path = "harness_occt_measurement/topology_extract_integration.rs"]
mod topology_extract_integration;
