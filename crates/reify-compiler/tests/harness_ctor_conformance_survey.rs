//! Struct-ctor field-type conformance — corpus survey (task #5304).
//!
//! PRD `docs/prds/struct-ctor-field-type-conformance.md`, task β (§8): run the
//! α(+ε) conformance-checking compiler over **all tracked `.ri`** and commit
//! `docs/prds/struct-ctor-field-type-conformance.survey.md` — every
//! conformance site, classified per D9, with the regeneration command. The signal is
//! "mechanized, not a hand audit": every row and every count in that artifact
//! is produced by the code in this unit, with zero hand-derived entries.
//!
//! Task #7543 added the corpus's SECOND half: the Reify snippets embedded as
//! raw-string literals in tracked `.rs` under `crates/`, which
//! `git ls-files -- '*.ri'` cannot reach. Both halves come from one git-index
//! primitive ([`survey::scan_tracked_corpus`]) and are swept by one pipeline, so they
//! cannot disagree about what a ctor-conformance site is; [`survey::corpus_parity`] is
//! what makes a narrowed walker fail loudly instead of writing a falsely-thin
//! artifact.
//!
//! # Why the expensive walk is `#[ignore]`d and the decisions are not
//!
//! Compiling the ~261 `examples/` files is documented as "the single most
//! expensive thing this binary does" by `harness_compilation_surface`'s
//! `examples_smoke.rs`; the ~700 tracked `.ri` are ~2.5× that. The sweep has a
//! SECOND half on top of it (task #7543): the Reify snippets embedded as
//! raw-string literals in the ~1,870 tracked `.rs` under `crates/`, which yield
//! ~3,300 admitted snippets to compile —
//! measured by the generator itself, which prints both halves' counts on every
//! run. Paying any of that on every merge gate would directly fight the
//! merge-gate-compile-cost PRD. So both walks live behind ONE `#[ignore]`d
//! generator, run on demand — while everything they *decide* (corpus
//! enumeration for both halves and the parity gate between them, raw-string
//! extraction and snippet admission, span→line and snippet→host line mapping,
//! ctor-name recovery, field/expected/found extraction, D9 classification,
//! disposition resolution, markdown rendering) is factored into pure helpers
//! that ARE gate-resident and unit-tested here against synthetic inputs, plus
//! two cheap end-to-end sweeps — a 3-file synthetic `.ri` corpus and a
//! synthetic Rust host — and a handful of pinned live files per half.
//! The pipeline is therefore regression-guarded on every gate run at near-zero
//! cost, without either walk ever running there.
//!
//! # Why this is its own binary
//!
//! Split out of `harness_compilation_surface` by #7709, when that unit crossed
//! the advisory warn line of `tests/infra/test_harness_kloc_cap.sh`.
//!
//! # Retiring this unit
//!
//! This is a CENSUS, not a permanent gate. Its product is one document in two
//! halves, and both named consumers have landed: task #5305 (γ, corpus
//! fix-forward) consumed the tracked-`.ri` sites, and task #5306 (δ, the
//! severity flip) fixed the inline sites its flip exposed and kept the rest as
//! deliberate Error pins. What remains is a standing two-half census that a
//! future change to `CTOR_FIELD_CONFORMANCE_SEVERITY`, or to the walker's scope,
//! consults. Its machinery — corpus enumeration for both halves, the parity
//! gate, span→line, D9 classification, the markdown renderer, the stamp guard —
//! stays compiled, linked and run on every merge gate: a real standing cost
//! under `docs/prds/merge-gate-compile-cost.md`.
//!
//! Retirement removes, together:
//!
//! 1. this root plus its whole `harness_ctor_conformance_survey/` module dir;
//! 2. the artifact `docs/prds/struct-ctor-field-type-conformance.survey.md`;
//! 3. `crates/reify-test-support/src/rust_fixture_scan.rs` plus its `pub mod`
//!    line — but ONLY if nothing else has picked it up by then. It is a
//!    library-crate module with no dependency on this survey, written to be
//!    reusable, so check `cargo tree`/callers before deleting rather than
//!    assuming this was its only consumer.
//!
//! Then sweep the prose citations of this unit;
//! `tests/infra/test_cited_test_paths_resolve.sh` catches the full-path ones.
//!
//! The ctor-conformance admission set this survey filters through does not
//! live here: it is `reify_test_support::ctor_conformance`, read by every
//! consumer, so deleting this survey costs nothing that outlives it.
//!
//! Nothing here fails when the artifact is deleted on its own:
//! [`survey::committed_survey_stamps_a_commit_that_is_an_ancestor_of_head`] SKIPS on an
//! absent artifact by design, so a partial retirement degrades to dead weight
//! rather than a merge-gate red.
//!
//! # Layout
//!
//! Explicit `#[path]` is required: this root is an integration-test crate root, where a
//! bare `mod <file>;` would resolve to a sibling `tests/<file>.rs`, not the
//! `harness_ctor_conformance_survey/` subdir. Whole-unit size — this root plus every
//! module below — is measured and capped by `tests/infra/test_harness_kloc_cap.sh`
//! rule (a); re-measure with that guard's `harness_layout_unit_lines` rather than
//! trusting a number pinned here.
//!
//! Module order is the layering: each module imports only from modules listed above it.
#[path = "harness_ctor_conformance_survey/workspace_git.rs"]
mod workspace_git;
#[path = "harness_ctor_conformance_survey/corpus.rs"]
mod corpus;
#[path = "harness_ctor_conformance_survey/owner.rs"]
mod owner;
#[path = "harness_ctor_conformance_survey/survey_site.rs"]
mod survey_site;
#[path = "harness_ctor_conformance_survey/sweep.rs"]
mod sweep;
#[path = "harness_ctor_conformance_survey/disposition.rs"]
mod disposition;
#[path = "harness_ctor_conformance_survey/stamp.rs"]
mod stamp;
#[path = "harness_ctor_conformance_survey/survey.rs"]
mod survey;
