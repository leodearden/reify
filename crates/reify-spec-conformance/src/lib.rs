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
//!
//! # Carry-forward: `fixtures/_*/` is not a spec section — gamma must skip it
//!
//! A leading underscore on a directory directly under `fixtures/` marks it as
//! **not a spec section**. Today there is exactly one: `fixtures/_placement-probe/`,
//! whose resident is deliberately unparseable, carries no directive annotations,
//! and is a deliberate violator of the corpus-cleanliness guard — it is the
//! sentinel that keeps that guard's registered exclusion arm for this tree from
//! going vacuous (pinned by `tests/fixture_tree.rs`), not a conformance fixture.
//!
//! This is an obligation on leaf gamma (#6761): the manifest generator and the
//! directive/annotation harness walk `fixtures/**/*.ri`, so they MUST skip any
//! `fixtures/_*/` directory rather than ingest its contents. Ingesting the probe
//! would red on a directive-less, unparseable file, or force an ad-hoc special
//! case discovered at implementation time. The rule is a naming convention, not
//! a marker file, precisely so it costs a future wave nothing to add another
//! non-section resident.
