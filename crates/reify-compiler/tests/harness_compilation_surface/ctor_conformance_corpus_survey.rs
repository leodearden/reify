//! Struct-ctor field-type conformance — corpus survey generator (task #5304).
//!
//! PRD `docs/prds/struct-ctor-field-type-conformance.md`, task β (§8): run the
//! α(+ε) warn-stage compiler over **all tracked `.ri`** and commit
//! `docs/prds/struct-ctor-field-type-conformance.survey.md` — every warning
//! site, classified per D9, with the regeneration command. The signal is
//! "mechanized, not a hand audit": every row and every count in that artifact
//! is produced by the code in this module, with zero hand-derived entries.
//!
//! # Why this lives HERE and not in a new `tests/*.rs` binary
//!
//! `tests/infra/test_harness_kloc_cap.sh` rule (b) flags any NEW standalone
//! top-level `crates/reify-compiler/tests/*.rs` as `reason=unsanctioned-standalone`
//! unless a grandfather-baseline row is added — an explicitly-discouraged
//! "conscious baseline edit" whose whole point is to stop new test binaries
//! silently re-accreting against the merge-gate link count
//! (`docs/prds/merge-gate-compile-cost.md` §5 C1). Folding the generator into
//! this already-consolidated unit adds no link at all, and the unit is
//! thematically exact — "what you hand the compiler … examples". The sibling
//! `examples_smoke.rs` already runs the identical parse→compile→filter pipeline
//! over `examples/`; β widens the root to the whole tracked corpus.
//!
//! # Why the expensive walk is `#[ignore]`d and the decisions are not
//!
//! Compiling the ~261 `examples/` files is documented as "the single most
//! expensive thing this binary does" (`examples_smoke.rs`); 660 tracked files
//! is ~2.5× that, and paying it on every merge gate would directly fight the
//! merge-gate-compile-cost PRD. So the full corpus walk is ONE `#[ignore]`d
//! generator, run on demand — while everything it *decides* (corpus
//! enumeration, span→line, ctor-name recovery, field/expected/found
//! extraction, D9 classification, markdown rendering) is factored into pure
//! helpers that ARE gate-resident and unit-tested here against synthetic
//! inputs, plus one cheap end-to-end sweep over a 3-file synthetic corpus.
//! The pipeline is therefore regression-guarded on every gate run at near-zero
//! cost, without the walk itself ever running there.
