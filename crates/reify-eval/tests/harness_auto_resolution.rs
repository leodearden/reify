//! Consolidated integration-test harness for the auto-resolution subsystem.
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/auto_*.rs` binaries — the end-to-end half of the
//! auto type-parameter resolution pipeline (backtracking search, binding-site
//! enumeration, `sub` override, completion, determinism, topology-fingerprint trigger,
//! resolved-value population) — into this single compile unit to cut the merge-gate
//! link count. It is the reify-eval counterpart of `reify-compiler`'s
//! `harness_auto_binding`, which consolidated that crate's compile-side `auto_*` tests
//! under leaf CMP-1 (task #5283); same subsystem, opposite side of the compile/eval
//! seam. The name deliberately DIFFERS from the compiler root's rather than mirroring
//! it: two same-named integration-test targets in one workspace make an unqualified
//! `cargo test --test <name>` and an unqualified nextest `binary(<name>)` ambiguous.
//! `auto_resolution` is the repo's own name for this side of it (see
//! `docs/architecture-audit/findings/auto-resolution-backtracking.md`).
//!
//! WHY THIS IS A SEPARATE ROOT FROM `harness_engine`. EVAL-3 originally folded these
//! seven files in with the `engine_`/`m8_`/`m9_` clusters. Measured on the merged tree
//! that put `harness_engine` at 20363 lines, 363 over `tests/infra/test_harness_kloc_cap.sh`'s
//! 20000-line rule (a) cap, with `module_lines` dominating the breakdown. §7 of the PRD
//! resolves an over-cap harness by SPLIT, never by raising the cap (precedent task #5620,
//! where `harness_topology_selector` at 21470 was split into 16786 + 4709), and the
//! breakdown's `module_lines`-dominant remedy is exactly "split the module dir into a
//! second `harness_<subsystem2>.rs`". The auto-binding cluster is the natural seam: all
//! seven files exercise one subsystem, none of them is referenced by `use crate::…` from
//! any module that stays behind, and none of them consumes the `common/differential.rs`
//! include (its sole consumer, `flat_sort_kahn_core_delegation`, stays in
//! `harness_engine`, so the 2185-line external stays attributed there and is not
//! duplicated into a second unit).
//!
//! Layout-only — no `#[test]` fn is added or removed. Each former file is included as a
//! stem-named module so its `<file>::<test>` module path (and thus every
//! `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is required:
//! this harness root is an integration-test crate root, where a bare `mod <file>;` would
//! resolve to the sibling `tests/<file>.rs`, not the `harness_auto_resolution/` subdir.
//!
//! No path fixups were needed for the seven files moved here: every path-sensitive
//! construct in them is either `env!("CARGO_MANIFEST_DIR")`-anchored (crate-root
//! relative, so unaffected by an extra source subdirectory) or a runtime
//! `std::fs::read_to_string` (process-CWD relative — the crate root under `cargo test`).
//! There are no `include_str!`/`include!` sites and no `#[global_allocator]` among them.
//!
//! Whole-unit size — this root plus every `harness_auto_resolution/*.rs` module below; this
//! unit includes nothing from outside its own module directory — is measured and capped
//! by `tests/infra/test_harness_kloc_cap.sh` rule (a).
//!
//! Module order: alphabetical by stem. No module here carries a rationale comment whose
//! ordering matters, and no module here is used by another, so there is no accretion
//! order to preserve.
#[path = "harness_auto_resolution/auto_backtracking_e2e.rs"]
mod auto_backtracking_e2e;
#[path = "harness_auto_resolution/auto_binding_sites_remaining_resolution.rs"]
mod auto_binding_sites_remaining_resolution;
#[path = "harness_auto_resolution/auto_sub_override_resolution.rs"]
mod auto_sub_override_resolution;
#[path = "harness_auto_resolution/auto_type_param_completion_e2e.rs"]
mod auto_type_param_completion_e2e;
#[path = "harness_auto_resolution/auto_type_param_determinism_tests.rs"]
mod auto_type_param_determinism_tests;
#[path = "harness_auto_resolution/auto_type_param_topology_trigger_tests.rs"]
mod auto_type_param_topology_trigger_tests;
#[path = "harness_auto_resolution/auto_type_param_value_population_e2e.rs"]
mod auto_type_param_value_population_e2e;
