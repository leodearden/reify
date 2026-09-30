//! Consolidated integration-test harness for the doc-chunk truth-enforcement
//! suites (MCP doc chunks vs. what the compiler actually accepts).
//!
//! Task #5477 (PRD docs/prds/v0_6/doc-chunk-truth-enforcement.md task α; C1 contract
//! in tests/infra/test_harness_kloc_cap.sh, PRD docs/prds/merge-gate-compile-cost.md §5):
//! folds the former standalone `tests/geometry_chunk_smoke.rs` (#5364) and
//! `tests/stdlib_chunk_geometry_ops_smoke.rs` (#5347) binaries into this single compile
//! unit, reversing their grandfather rows in tests/infra/harness-layout-baseline.manifest
//! rather than growing that shrinking ratchet. Layout-only — no `#[test]` fn is added or
//! removed. Each former file is included as a stem-named module so its `<file>::<test>`
//! module path (and thus every `test(/^<file>::/)` filterset) resolves unchanged.
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_doc_chunks/` subdir. Unlike harness_langcore.rs / harness_patterns.rs, the
//! shared `common` helper module is deliberately NOT declared here — neither absorbed
//! file declares any `mod` or uses that helper, so declaring it would pull
//! tests/common/mod.rs into this compile unit for nothing, in a PRD whose whole point is
//! cutting merge-gate compile cost.
//!
//! Modules authored DIRECTLY into this compile unit rather than absorbed from a
//! standalone binary, so none becomes another grandfathered top-level `tests/*.rs`
//! row. The C1 harness-layout contract in tests/infra/test_harness_kloc_cap.sh
//! should read them that way — no baseline-manifest row is reversed by them, and
//! none is added:
//!
//! - `fence_gate` (#5479, same PRD, leaf β) — the repo-wide gate over every
//!   `chunks/*.md` fence.
//! - `chunk_io` (#6956) — where the chunk corpus lives, how a chunk is read and
//!   listed, and how a corpus gate reports.
//! - `chunk_markdown` (#6956) — how a chunk's markdown divides into fenced code
//!   blocks and the sections those fences cannot end; the one fence parser.
//! - `chunk_prose` (#6974) — the unfenced-prose model every prose scan reads.
//! - `chunk_cite_gate` (#6974) — the one cite scanner, and the corpus-wide cite
//!   and maintainer-note gates.
//! - `doc_forms` (#6974) — documented call forms, read from markdown spans and
//!   parsed sources, and their pairing.
//! - `signature_fixtures` (#6974) — the compile-verified fixtures whose calls
//!   stand for documented signatures, and what "compiles clean" means for them.
//! - `unfenced_signature_gate` (#6974) — every signature in any chunk's unfenced
//!   prose is exercised by a compile-verified fixture.
//! - `schematic_listing_gate` (#6956) — every signature in a gated chunk's
//!   ```` ```reify-schematic ```` listings is exercised by a compile-verified
//!   fixture.

#[path = "harness_doc_chunks/angle_crossings_diagnostics_smoke.rs"]
mod angle_crossings_diagnostics_smoke;
#[path = "harness_doc_chunks/chunk_cite_gate.rs"]
mod chunk_cite_gate;
#[path = "harness_doc_chunks/chunk_io.rs"]
mod chunk_io;
#[path = "harness_doc_chunks/chunk_markdown.rs"]
mod chunk_markdown;
#[path = "harness_doc_chunks/chunk_prose.rs"]
mod chunk_prose;
#[path = "harness_doc_chunks/doc_forms.rs"]
mod doc_forms;
#[path = "harness_doc_chunks/enums_chunk_option_smoke.rs"]
mod enums_chunk_option_smoke;
#[path = "harness_doc_chunks/fence_gate.rs"]
mod fence_gate;
#[path = "harness_doc_chunks/functions_chunk_overloading_smoke.rs"]
mod functions_chunk_overloading_smoke;
#[path = "harness_doc_chunks/geometry_chunk_smoke.rs"]
mod geometry_chunk_smoke;
#[path = "harness_doc_chunks/oracle_xref_smoke.rs"]
mod oracle_xref_smoke;
#[path = "harness_doc_chunks/schematic_listing_gate.rs"]
mod schematic_listing_gate;
#[path = "harness_doc_chunks/signature_fixtures.rs"]
mod signature_fixtures;
#[path = "harness_doc_chunks/stdlib_chunk_geometry_ops_smoke.rs"]
mod stdlib_chunk_geometry_ops_smoke;
#[path = "harness_doc_chunks/unfenced_signature_gate.rs"]
mod unfenced_signature_gate;
#[path = "harness_doc_chunks/units_chunk_smoke.rs"]
mod units_chunk_smoke;
