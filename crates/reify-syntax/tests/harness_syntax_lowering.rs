//! Consolidated integration-test harness for reify-syntax's CST→AST lowering tests.
//!
//! Task #7040 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf C-syntax):
//! splits the lowering family out of `harness_syntax.rs`. The subsystem is the AST half of
//! reify-syntax — `ts_parser.rs`'s `lower_*` pass (`lower_enum`, `lower_binary_expr`, …),
//! which turns the tree-sitter CST into `reify_ast` nodes. Membership is mechanical rather
//! than curated: this root holds exactly `harness_syntax/*_lowering_tests.rs` as it stood
//! at the split, and no `*_lowering_tests.rs` stayed behind — a rule a reviewer re-derives
//! with one glob. A same-stem CST-level companion is NOT the membership test and must not
//! be used as one: only three of the fourteen (`auto_binding_sites`, `numeric_separators`,
//! `radix_literals`) have a `*_grammar_tests` sibling left in `harness_syntax`, and the
//! other eleven have no same-stem counterpart there at all. Decide by the glob.
//!
//! WHY THIS IS A SEPARATE ROOT FROM `harness_syntax`. Measured before the split,
//! `harness_syntax` stood at 18957 lines = 94.8% of
//! `tests/infra/test_harness_kloc_cap.sh`'s 20000-line rule (a) cap — 1043 lines of
//! headroom — with `module_lines` (18739) dominating the breakdown. §7 of the PRD resolves
//! a cap-pressured harness by SPLIT, and the `module_lines`-dominant remedy is exactly
//! "split the module dir into a second `harness_<subsystem2>.rs`" (precedents: task #5620,
//! which split `harness_topology_selector` 21470 into 16786 and 4709; task #5282, which
//! split `harness_engine` 20363 and landed `harness_auto_resolution` at 4482). Raising
//! `CAP_LINES` was not an option: it would loosen the C2 ratchet to fit its first offender
//! and contradict the ratified §3 W1 / §7 10–20 kLOC band. Nor a harness-layout-baseline
//! grandfather row (that route is superseded — esc-5056-11), nor parking the overflow in
//! one of the seven nextest override binaries (refused — task #6461). Measured outcome,
//! in the guard's own `total root module module_files external external_files` order:
//!
//!   harness_syntax           18957 148 18739 63 70 1  ->  14569 134 14365 49 70 1
//!   harness_syntax_lowering      0   0     0  0  0 0  ->   4469  95  4374 14  0 0
//!
//! THIS UNIT INCLUDES NOTHING FROM OUTSIDE ITS OWN MODULE DIRECTORY — that is the
//! load-bearing `external 0 / external_files 0` above, not an incidental one. reify-syntax
//! keeps a shared tree-sitter CST helper at `tests/common/mod.rs`, and rustc compiles a
//! separate copy of such a helper into every binary that includes it, so a `mod common;`
//! here would duplicate its compile cost and partly undo the relief this split exists to
//! buy. Omitting it is safe precisely because all ten `crate::common` consumers are
//! grammar/parser-side modules that stayed in `harness_syntax`: not one `*_lowering_tests`
//! module references it. Re-check that property before moving any module in or out of here.
//!
//! Layout-only — no `#[test]` fn was added or removed (invariant I3). Each file is included
//! as a stem-named module so its `<stem>::<test>` module path, and thus every
//! `test(/^<stem>::/)` filterset, resolves unchanged; only the enclosing binary id changed.
//! Verified against a `cargo nextest list` baseline captured before any file moved: 696
//! tests in the package before and after, the binary-id-stripped name multiset
//! byte-identical, `harness_syntax` 603 → 452 over 49 stems and this root 0 → 151 over 14.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <stem>;` would resolve to a sibling `tests/<stem>.rs`, not the
//! `harness_syntax_lowering/` subdir. Section 6 of `tests/infra/test_harness_kloc_cap.sh`
//! enforces that C1 mandate on every harness root in the live tree.
//!
//! No path fixups were needed for the fourteen files moved here. The one path-sensitive
//! construct among them is `enum_named_field_lowering_tests.rs`'s
//! `include_str!("../../../../tree-sitter-reify/test/fixtures/dce-2-nameddecl.ri")`, which
//! rustc resolves relative to the containing FILE's directory; source and destination are
//! sibling directories at the same depth, so it resolves to the identical path — verified
//! by a clean build, not assumed (a break would have been a compile error). There are no
//! other `include!`/`include_str!` sites, no `env!("CARGO_MANIFEST_DIR")` or relative
//! filesystem access, and no `#[global_allocator]`. The modules' only `use` lines name
//! `reify_ast` and `reify_core::ModulePath`, both crate-external, so no cross-module
//! `crate::` or `super::` reference had to be rewritten and every file moved verbatim.
//!
//! Whole-unit size — this root plus every `harness_syntax_lowering/*.rs` module below — is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
//!
//! Module order: alphabetical by stem. No module here is used by another and none carries a
//! rationale comment whose ordering matters, so there is no accretion order to preserve.
#[path = "harness_syntax_lowering/auto_binding_sites_lowering_tests.rs"]
mod auto_binding_sites_lowering_tests;
#[path = "harness_syntax_lowering/aux_at_lowering_tests.rs"]
mod aux_at_lowering_tests;
#[path = "harness_syntax_lowering/enum_named_field_lowering_tests.rs"]
mod enum_named_field_lowering_tests;
#[path = "harness_syntax_lowering/enum_type_param_lowering_tests.rs"]
mod enum_type_param_lowering_tests;
#[path = "harness_syntax_lowering/imaginary_literal_lowering_tests.rs"]
mod imaginary_literal_lowering_tests;
#[path = "harness_syntax_lowering/joint_with_lowering_tests.rs"]
mod joint_with_lowering_tests;
#[path = "harness_syntax_lowering/namespaced_ref_lowering_tests.rs"]
mod namespaced_ref_lowering_tests;
#[path = "harness_syntax_lowering/numeric_separators_lowering_tests.rs"]
mod numeric_separators_lowering_tests;
#[path = "harness_syntax_lowering/radix_literals_lowering_tests.rs"]
mod radix_literals_lowering_tests;
#[path = "harness_syntax_lowering/relate_at_auto_lowering_tests.rs"]
mod relate_at_auto_lowering_tests;
#[path = "harness_syntax_lowering/trait_assoc_fn_call_lowering_tests.rs"]
mod trait_assoc_fn_call_lowering_tests;
#[path = "harness_syntax_lowering/trait_assoc_fn_member_lowering_tests.rs"]
mod trait_assoc_fn_member_lowering_tests;
#[path = "harness_syntax_lowering/unit_expr_lowering_tests.rs"]
mod unit_expr_lowering_tests;
#[path = "harness_syntax_lowering/value_pow_lowering_tests.rs"]
mod value_pow_lowering_tests;
