//! Ring-1 language-spec conformance suite for `docs/reify-language-spec.md`
//! (PRD `docs/prds/v0_6/spec-conformance-suite.md`, D1/D2).
//!
//! Integration-test-only crate. At leaf β it carries the fixture tree
//! (`fixtures/<section>/…`, see `fixtures/README.md`) and its placement test,
//! and deliberately exports NO public API — the directive/annotation harness
//! (`//@ key: value`, `//~ ERROR E_*`) and the generated `manifest.json`
//! arrive with leaf γ (#6761), together with the code that consumes them.
//!
//! Not to be confused with two false friends already in the tree:
//!   * `reify-compiler/src/conformance/` — struct-ctor / GD&T field conformance.
//!   * `reify-kernel-conformance` — the kernel-pair producer×consumer matrix.
//!
//! # Carry-forward: the OCCT crate set is a hand-synced pair
//!
//! At β this crate has empty `[dependencies]` and no `[dev-dependencies]`, so
//! `cargo tree -p reify-spec-conformance -e normal,dev` reaches no
//! `reify-kernel-occt` and the crate is NOT occt-touching. The moment it takes
//! a dependency on `reify-cli`, `reify-eval`, `reify-config`, or
//! `reify-test-support`-via-eval, it becomes occt-touching and BOTH
//! `scripts/occt-touching-crates.txt` AND the `package(...)` filter literal in
//! `.config/nextest.toml`'s occt-group override must be updated in that same
//! diff — `tests/infra/test_occt_gated_scope.sh` Test 3 asserts the declared
//! set EQUALS the cargo-metadata-derived set in BOTH directions, so updating
//! one file only is a merge-gate failure.
