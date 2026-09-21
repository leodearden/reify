//! Consolidated integration-test harness for the M8/M9 milestone acceptance corpora.
//!
//! Task #7654 splits these eight modules out of `harness_engine`, which measured
//! 18002 of the 20000 lines `tests/infra/test_harness_kloc_cap.sh` rule (a) allows a
//! single harness unit — 90.0%, with `module_lines` (15655 across 28 module files)
//! dominating the breakdown. Rule (a) resolves that squeeze by SPLIT, never by
//! raising the cap: precedent #5620 (`harness_topology_selector`, 21470 split into
//! 16786 + 4709), #6760 (which split `harness_auto_resolution` out of this same
//! unit) and #7466. The seam is the one the PRD's own grouping principle picks —
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 group a crate's tests by
//! subsystem module PREFIX — and it was already named as a distinct cluster by
//! `harness_engine`'s own header, which described that unit as holding "the
//! `engine_`/`m8_`/`m9_` clusters". This root is the `m8_`/`m9_` third of that list.
//!
//! WHAT THIS UNIT IS. Milestone ACCEPTANCE corpora, not engine internals. Every
//! module here takes `.ri` source and drives it through the full public pipeline —
//! `compile_with_stdlib` or `check_source`, then `Engine::eval` — and asserts on the
//! resulting values, types and diagnostics. Seven of the eight read real
//! `examples/*.ri` design files as their corpus; `m8_m11_regression_checkpoint`
//! drives one inline source that exercises a feature from each of M8 through M11.
//! What separates them from the `engine_` cluster left behind in `harness_engine` is
//! the direction of the assertion: these pin a MILESTONE's end-to-end behaviour
//! against a corpus, while the modules that stay pin engine-level entry points
//! (`Engine::eval_cached`, `Engine::build_outputs`,
//! `Engine::redispatch_geometry_consuming_compute_nodes`) against inputs the test
//! constructs.
//!
//! WHY THIS NAME. It deliberately matches no other integration-test target in the
//! workspace: two same-named targets make an unqualified `cargo test --test <name>`
//! and an unqualified nextest `binary(<name>)` ambiguous. That is the same
//! constraint that made #6760 name its split `harness_auto_resolution` instead of
//! mirroring `reify-compiler`'s `harness_auto_binding`. No
//! `crates/*/tests/harness_milestone_integration.rs` existed anywhere in the
//! workspace before this root.
//!
//! LAYOUT-ONLY — no `#[test]` fn is added, removed or renamed. Each module keeps its
//! stem, so every `<file>::<test>` module path resolves unchanged, and with it every
//! `test(/^<file>::/)` filterset and every `-- <module>::<test>` filter argument.
//! Only the binary id moves, from `reify-eval::harness_engine` to
//! `reify-eval::harness_milestone_integration`. Explicit `#[path]` is required
//! below: this harness root is an integration-test crate root, where a bare
//! `mod <file>;` would resolve to the sibling `tests/<file>.rs` rather than the
//! `harness_milestone_integration/` subdir — Section 6 of the kLOC guard enforces
//! the explicit form for exactly that reason.
//!
//! NO PATH FIXUPS were needed. The move is directory-to-directory at the SAME depth
//! (`tests/harness_engine/` → `tests/harness_milestone_integration/`), so nothing
//! relative re-depths — unlike `harness_cache`'s consolidation, which moved files up
//! out of `tests/` and had to deepen three `include_str!` sites by one `../`. In any
//! case there are no `include_str!`/`include_bytes!`/`include!` sites and no
//! `#[global_allocator]` among these eight: every path-sensitive construct is either
//! `env!("CARGO_MANIFEST_DIR")`-anchored (crate-root relative, so unaffected by the
//! surrounding source directory) or a runtime `std::fs::read_to_string`
//! (process-CWD relative — the crate root under `cargo test`), which is what
//! `m8_stdlib_integration`'s 14 `../../examples/*.ri` reads are.
//!
//! WHOLE-UNIT SIZE — this root plus every `harness_milestone_integration/*.rs`
//! module below; this unit includes NOTHING from outside its own module directory —
//! is measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
//! Re-measure with that guard's `harness_layout_unit_lines` rather than trusting a
//! number pinned here.
//!
//! SCOPE. `tests/m5_integration.rs`, `tests/m6_data_carrying_enum.rs`,
//! `tests/m10_combined.rs`, `tests/m10_geometric_types.rs`,
//! `tests/m11_field_calculus.rs` and `tests/m11_full_integration.rs` are milestone
//! corpora too, but they predate leaf EVAL-3 and remain grandfathered top-level
//! standalone binaries with rows in `tests/infra/harness-layout-baseline.manifest`,
//! outside EVAL-3's consolidated set. They are deliberately NOT folded in here.
//! Folding them is a separate consolidation with a path-fixup risk this split does
//! not carry — those files move `tests/` → `tests/<dir>/`, which DOES change
//! relative depth — and it would do nothing for the squeeze this split exists to
//! relieve, since they are already outside `harness_engine`.
//!
//! Module order: alphabetical by stem. No module here carries a rationale comment
//! whose ordering matters, and no module here is used by another, so there is no
//! accretion order to preserve.
#[path = "harness_milestone_integration/m8_3_stdlib_integration.rs"]
mod m8_3_stdlib_integration;
#[path = "harness_milestone_integration/m8_4_stdlib_integration.rs"]
mod m8_4_stdlib_integration;
#[path = "harness_milestone_integration/m8_m11_regression_checkpoint.rs"]
mod m8_m11_regression_checkpoint;
#[path = "harness_milestone_integration/m8_stdlib_integration.rs"]
mod m8_stdlib_integration;
#[path = "harness_milestone_integration/m9_combined.rs"]
mod m9_combined;
#[path = "harness_milestone_integration/m9_constraint_def.rs"]
mod m9_constraint_def;
#[path = "harness_milestone_integration/m9_integration.rs"]
mod m9_integration;
#[path = "harness_milestone_integration/m9_trait_conformance.rs"]
mod m9_trait_conformance;
