# Doc-chunk truth enforcement — chunk-fence compile gate + PDOCCOVER omission audit

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-25
**Origin:** 2026-07-24 language review + docs-truth session (brief `~/.claude/spawn-briefs/2026-07-25-docs-truth-enforcement-prd.md`); substrate re-verified against main `92551d18d6` on 2026-07-25.

## Goal

The authoritative language reference (`crates/reify-mcp/src/tools/chunks/*.md`, 17 topic files served to design agents via the `reify_language_reference` MCP tool and the reify-design skill) becomes **mechanically un-driftable in two directions**:

1. **Commission drift** (documented-but-wrong): every fence-tagged `.ri` example in the chunks compiles zero-Error against stdlib on every merge, and every fence is explicitly triaged compiled-vs-schematic — a phantom signature like the `rotate(geo, axis, angle)` that cost live probe cycles in the printer_v01 dogfood turns the merge gate RED.
2. **Omission drift** (real-but-invisible): every name in the compiler's builtin name registries appears in at least one chunk file, is explicitly allow-listed with a reason, or sits in a committed shrinking baseline — the invisible clearance oracle (`interferes`/`min_clearance`/`intersects`/`distance` had ZERO chunk presence while the dogfood session shipped mechanically-checkable interferences) can never recur silently.

A user observes: `cargo test -p reify-compiler --test harness_doc_chunks` fails naming file + fence when a chunk documents a signature the compiler rejects; `reify-audit --pattern PDOCCOVER` exits non-zero listing every undocumented registry name not consciously dispositioned.

## Background

The 2026-07-24 language review established four drift directions in the chunks: aspirational, stale-vs-landed, internally contradictory, and omission. Cost evidence: ~40% of the printer_v01 dogfood session's CLI spend went to probing what the language can do.

This PRD is the **mechanical enforcement layer** of the docs-truth program. The other layers exist:
- **Layer 1 (forward process gate), landed** `8757de2947`: the `/prd` overlay's "Docs-truth gate" — language-surface PRDs must carry doc leaves. Catches *future* PRDs; does nothing for content already drifted or for edits outside PRD flow.
- **Layer 2 (exemplar corpus), filed** #5397: `examples/best_practices/` compile-gated by the existing `examples_smoke.rs`. Covers idioms, not the reference chunks.
- **Content fixes, in flight**: #5347 (stdlib.md signature audit, in-progress), #5364 (geometry.md audit, **done**, merged `b81331913d`), #5389 (clearance-oracle chunk content, in-progress).

Substrate census (2026-07-25, main `92551d18d6`):
- Fences across the 17 chunk files: **116 total — 111 untagged bare ` ``` `, 5 tagged ` ```reify ` (all in traits.md, of which 3 are fragments)**. The untagged majority are schematic signature tables (`box(width, depth, height) -> Solid`), type declarations, and grammar templates — legitimately non-compilable. #5364's landed test reached the same conclusion and pinned signatures via curated fixtures instead of scraping.
- Registry coverage: **83 of 133 names across the 8 `units.rs` name registries are absent from all chunks** — including the entire clearance oracle (3 kinematic + `intersects`), all 11 affine-map constructors, all 6 topology-invariant helpers (`is_watertight` …), 24/32 topology selectors, and 30/64 geometry functions (the curve family `helix`/`interp`/`bezier`/`nurbs`, `sweep_guided`, the zone family, …).
- `crates/reify-compiler/tests/` now has four `harness_*.rs` compile units (`harness_auto_binding`, `harness_langcore`, `harness_patterns`, `harness_traits`) — in-crate precedent for the C1 harness-layout contract (`tests/infra/test_harness_kloc_cap.sh`, `docs/prds/merge-gate-compile-cost.md` §5). #5364's `geometry_chunk_smoke.rs` landed as a **baseline-manifest grandfather** (`tests/infra/harness-layout-baseline.manifest:126`) after going merge_verify_red as an unsanctioned standalone — the exact trap this PRD's consolidation resolves.

## Sketch of approach

### (a) Chunk-fence compile gate — consolidated `harness_doc_chunks.rs`

One C1-sanctioned compile unit `crates/reify-compiler/tests/harness_doc_chunks.rs` declaring `mod` entries for modules under `crates/reify-compiler/tests/harness_doc_chunks/`:

- **Absorbed curated-fixture modules** (the primary signature-truth mechanism for schematic tables): #5364's landed `geometry_chunk_smoke.rs` and #5347's `stdlib_chunk_geometry_ops_smoke.rs` move under the harness dir with their original stems (`mod geometry_chunk_smoke;` …), preserving every existing `cargo test` selector per the C1 contract guarantee. Their `harness-layout-baseline.manifest` grandfather entries are removed in the same diff (ratchet shrink).
- **New `fence_gate.rs` module**: walks the chunks dir (resolved `concat!(env!("CARGO_MANIFEST_DIR"), "/../reify-mcp/src/tools/chunks")` — the `examples_smoke.rs` idiom; reify-mcp does not depend on reify-compiler, so the test lives here and reads the files by path, the same conclusion #5347/#5364 reached). Per file, a line-level fence parser (fences at line start; no markdown crate needed) enforces:
  1. ` ```reify ` fence → content compiles as a complete module via `reify_test_support::compile_source_with_stdlib` with zero `Severity::Error` diagnostics; failure names file + fence ordinal + the diagnostics.
  2. Bare untagged ` ``` ` fence → **FAIL naming file:line**. Every fence must carry an explicit tag.
  3. Any other explicit tag (`reify-schematic`, `reify-fragment`, `text`, `ebnf`, …) → exempt by convention.
  4. **Reachability**: every `chunks/*.md` file is both `include_str!`-referenced and topic-listed in `crates/reify-mcp/src/tools/language_chunks.rs` (text-scan) — an on-disk chunk unreachable through the MCP tool is omission drift of a whole file.
- **One-time retag sweep**, same diff as the fence gate: all 116 fences get explicit tags. Schematic tables → `reify-schematic`; genuinely partial code snippets → `reify-fragment`; complete examples → `reify` (and must compile). The three fragment-shaped fences currently tagged `reify` in traits.md are retagged or made self-contained.

### (b) PDOCCOVER — reify-audit omission-drift pattern

New module `crates/reify-audit/src/pdoccover.rs` + `--pattern PDOCCOVER` CLI arm, mirroring the PTODO architecture (one module per pattern, text-scanning, no reify-compiler dependency):

- **Registry discovery by pattern, not hardcoded list**: textually extract every `pub const <IDENT>_NAMES: &[&str] = &[…];` block from `crates/reify-compiler/src/units.rs`. A newly added registry is auto-covered — the anti-omission property applies to the registry list itself. A **brittle-parse guard** test pins extraction against the real file: the discovered set must include the 8 known registries (`GEOMETRY_FUNCTION_NAMES` :21, `GEOMETRY_QUERY_HELPER_NAMES` :102, `GEOMETRY_KINEMATIC_QUERY_NAMES` :130, `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` :215, `AFFINE_MAP_CONSTRUCTOR_NAMES` :551, `TOLERANCING_MARKER_NAMES` :674, `GEOMETRY_QUERY_NAMES` :803, `DYNAMICS_QUERY_NAMES` :873 — floor, not exact) with non-empty name lists, so a units.rs refactor breaks the guard RED instead of PDOCCOVER silently passing on zero names.
- **Per-name disposition** — a name is compliant iff exactly one of:
  1. **Documented**: word-boundary match in at least one `chunks/*.md`.
  2. **Allowed**: the registry entry's line in `units.rs` carries an inline `// doccover:allow — <reason>` (PTODO's site-local escape philosophy; reason mandatory).
  3. **Baselined**: listed in the committed `crates/reify-audit/pdoccover-baseline.txt` (known debt, expected to shrink).
- **FAIL findings** (High severity, hard gate): a name that is none of the three; a **stale baseline entry** (name now documented — must be removed in the same diff, keeping the ratchet honest, per the harness-layout C2 lesson); a **stale allow entry** (allowed name that is also documented — dead escape, per the PTODO orphaned-cite analogy).
- **Gate wiring mirrors PTODO**: a `tests/infra/test_reify_audit_pdoccover.sh` verify-step test with its `run-all-classification.manifest` row registered **same-diff** (overlay gate-test drift-guard rule), paired with the existing `scripts/reify-audit-freshness.sh` binary-freshness discipline.
- **Baseline seeding**: generated post-#5347/#5389 landing from live truth, via a regenerator sharing the single derivation with the detector (the `ptodo-baseline-gen` §6.6 lesson: one Rust derivation for both generation and the ratchet comparison).

## Resolved design decisions

1. **Consolidated harness, not a third standalone.** `harness_doc_chunks.rs` under the C1 contract. #5364 landed only via a baseline grandfather after a merge_verify_red on `reason=unsanctioned-standalone`; this PRD both avoids the trap and *reverses* #5364's grandfather (baseline entries removed on absorption). Verified: four sibling harnesses already exist in this crate.
2. **Explicit-tag discipline instead of per-fence skip reasons.** The brief suggested SKIP_SET-style mandatory reasons per exempt fence. Rejected: 111/116 fences are schematic — per-fence reasons would be near-total noise. The exempt *tag* is the explicit, diff-reviewable sanction; the hard ban on bare untagged fences is what forces conscious triage, now and for every future fence. (Deviation from the brief, recorded deliberately.)
3. **Curated fixtures remain the signature-truth mechanism for schematic tables.** Fence compilation cannot verify `box(width, depth, height) -> Solid` notation — #5364's header documents why scraping schematic notation needs a fragile grammar. The absorbed fixture modules pin those; the fence gate makes compile-coverage the *default* for every full example added from now on. The two mechanisms compose; neither substitutes for the other.
4. **PDOCCOVER as a baseline ratchet, not an instant hard floor.** 83 missing names is content debt owned by content tasks; a day-one hard gate would either turn main RED or force an 83-entry allow list with fake reasons. Committed baseline + stale-entry hard-FAIL gives monotone shrink with an honest ledger. Precedents: `ptodo-baseline.txt`, `harness-layout-baseline.manifest`.
5. **Registry discovery by `*_NAMES` pattern + brittle-parse floor guard.** A hardcoded registry list would itself be an omission-drift surface.
6. **Sequencing protects in-flight siblings.** The retag sweep and the baseline seed both land *after* #5347/#5389 via hard `add_dependency` edges — without them, PDOCCOVER's stale-baseline strictness or the fence tag ban would red-flag those tasks' merges the same way the harness ratchet red-flagged #5364.
7. **Trailing G7 note (advisory walk, silent-failure family):** every exemption path in this design leaves an observable, diff-reviewable trace — exempt tags in the markdown, reasons inline in units.rs, debt in a committed baseline; stale entries hard-fail rather than accumulate (INV-SF-3 spirit); both gates exit non-zero through existing severity machinery (INV-SF-2 spirit). No silent disposition exists.

## Rejected alternative (recorded, do not pursue)

**Build-time generation of chunk signature tables from compiler registries.** No machine-readable builtin-signature registry exists: the `units.rs` registries are name-only; `crates/reify-compiler/src/builtin_signatures.rs` is a partial per-arg dimension checker (topology-selector family only); `reify-doc-build::build_stdlib_doc_model` covers only `.ri`-defined stdlib items via CompiledModule, not the Rust builtin arms where the drift lives (real arities are buried in match-arm code with ad-hoc error strings, e.g. "rotate() expects 2 or 5 arguments"). A true signature registry is a separate, heavier PRD with independent value (better diagnostics, LSP hover/completion) — named here as future work, not attempted here.

## Pre-conditions

- #5347 (stdlib.md audit + `stdlib_chunk_geometry_ops_smoke.rs`, in-progress 2026-07-25) — absorption target for leaf α; chunk-content prerequisite for β and δ.
- #5389 (clearance-oracle chunk content, in-progress 2026-07-25) — chunk-content prerequisite for β and δ.
- #5364 — already done/merged; its landed artifacts are absorbed by α.
- All other substrate exists on main (verified 2026-07-25): C1 harness contract + four in-crate precedents, `reify_test_support::compile_source_with_stdlib`, reify-audit per-pattern module architecture + `--pattern` CLI, PTODO baseline machinery, `tests/infra` classification manifest.

## Cross-PRD / cross-task relationships

| Counterpart | Seam | Direction / owner | Resolution |
|---|---|---|---|
| #5347, #5364 (chunk signature audits) | their smoke tests + fixtures | this PRD absorbs (α) | α depends on #5347; #5364 landed — α moves its file + removes its baseline entry |
| #5389 (clearance-oracle content) | `chunks/geometry.md` content; the clearance names in the PDOCCOVER census | content owned by #5389; enforcement here | β and δ depend on #5389 (hard edges) |
| #5397 (exemplar corpus) | none — `examples/`, not chunks | complementary layer 2 | no edge; no file overlap |
| `/prd` overlay docs-truth gate (landed `8757de2947`) | process-level; no code seam | overlay owns process, this PRD owns mechanism | this PRD is the mechanical backstop the overlay gate assumes |
| `placeholder-type-eradication-ratchet` leaf δ (PTYPE detector) | `crates/reify-audit/src/bin/reify-audit.rs` `--pattern` enum + `lib.rs` | each PRD owns its own pattern; additive enum arms | file-level overlap only; second lander rebases trivially; no dep edge |
| Future: builtin-signature registry PRD | would supersede curated fixtures with generated tables | unowned; named future work | see Rejected alternative |

## Decomposition plan (bare B — 4 leaves)

- **α — Consolidate doc-chunk smoke tests into `harness_doc_chunks.rs`.** Create the harness root; move #5364's `geometry_chunk_smoke.rs` and #5347's `stdlib_chunk_geometry_ops_smoke.rs` (+ its fixture) under `harness_doc_chunks/` with original stems; remove both files' `harness-layout-baseline.manifest` grandfather entries same-diff. Modules: reify-compiler tests, tests/infra manifest. **Deps:** #5347. **Observable signal:** `cargo test -p reify-compiler --test harness_doc_chunks` runs both absorbed suites with their original selector paths resolving unchanged; `tests/infra/test_harness_kloc_cap.sh` passes with the baseline entries *removed* (net baseline shrink of ≥1 line vs main).
- **β — Fence gate + one-time retag sweep.** Add `harness_doc_chunks/fence_gate.rs` (compile `reify` fences, ban bare fences, reachability check against `language_chunks.rs`); retag all fences in the 17 chunk files same diff. Modules: reify-compiler tests, reify-mcp chunks (tag-only edits except where a `reify` fence must be made self-contained). **Deps:** α, #5389. **Observable signal:** RED-first demonstration — planting `rotate(geo, axis, angle)` in a `reify` fence fails the harness naming the file + fence with the compiler diagnostics; a bare ` ``` ` fence fails naming file:line; on the clean tree the harness is GREEN.
- **γ — PDOCCOVER detector + brittle-parse guard.** `pdoccover.rs`, `*_NAMES` registry discovery, three-way disposition (documented / `doccover:allow` inline / baseline), stale-entry FAILs, CLI `--pattern PDOCCOVER` arm, unit + guard tests. Modules: reify-audit only. **Deps:** none (baseline file may be empty/absent at this stage; the detector is exercised standalone). **Observable signal:** on a tree without a seeded baseline, `reify-audit --pattern PDOCCOVER` exits non-zero listing the missing names **including the clearance-oracle family** — the live demonstration that this pattern would have caught the printer_v01 blind spot (still reproducible until #5389 lands).
- **δ — PDOCCOVER hard gate + baseline seed.** `tests/infra/test_reify_audit_pdoccover.sh` + its `run-all-classification.manifest` row same-diff; seed `crates/reify-audit/pdoccover-baseline.txt` from post-content-landing truth via the shared-derivation regenerator. Modules: reify-audit, tests/infra. **Deps:** γ, #5347, #5389. **Observable signal:** the verify suite includes the gate GREEN on main with the committed baseline; deleting a documented registry name's chunk mention (or adding a registry name) flips `reify-audit --pattern PDOCCOVER` and the gate test non-zero naming the name; a baseline entry for a documented name likewise FAILs as stale.

Dependency DAG: `#5347 → α → β`, `#5389 → β`, `γ → δ`, `{#5347, #5389} → δ`. γ is independent and can run first.

## Out of scope

- **Documenting the 83 missing names** — content work owned by #5347/#5389 and future content tasks burning down the baseline; this PRD ships the ledger, not the prose.
- **Promoting schematic tables or fragments to compiled examples** — content-quality work, incremental, unowned here.
- **A machine-readable builtin-signature registry / build-time doc generation** — future PRD (see Rejected alternative).
- **The exemplar corpus** — #5397.
- **Non-chunk documentation** (README, docs/notes, GUI help) — different truth surfaces, not served to design agents through `reify_language_reference`.

## Open questions (tactical only)

1. **Baseline regenerator shape** — a `--emit-baseline` flag on the PDOCCOVER pattern vs a separate `pdoccover-baseline-gen` bin (PTODO precedent). Invariant either way: one shared derivation between generator and ratchet. Decide in γ/δ.
2. **Exempt-tag vocabulary** — whether `reify-schematic` and `reify-fragment` both exist or one exempt tag suffices; the gate logic only distinguishes `reify` / explicitly-tagged-other / untagged, so this is a docs-convention choice. Decide in β.
3. **traits.md fences 1 and 3** (trait defs with fn bodies) — whether they compile as complete modules as-is; if not, retag or wrap. Decide in β.
4. **PDOCCOVER finding output cap** — whether to list all missing names or top-N + count when the list is long. Decide in γ.
