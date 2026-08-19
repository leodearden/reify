# Angle-crossing doctrine placement — decision record

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack — the same leaf #6181/#6267 landed into) |
| Task | #6290 |
| Spun out of | esc-6267-1 / esc-6267-2 on #6267, which itself compressed the section #6181 (leaf γ) wrote |
| Branch | `task/6290` — measured at `5df2f9f172a5933e3042aeddfa88237416e65d` (prerequisite P1); re-verified byte-identical at `94aaecff43a458d1ce6a3fe8bdcc88d2e0f5588d` (after S1–S4 landed) |
| Date | 2026-08-19 |
| Instrument | Byte census: `wc -c`/`wc -l` over `crates/reify-mcp/src/tools/chunks/*.md`, plus `sed -n` slices of `units.md`. Discoverability walk's instrument, unchanged from `docs/notes/angle-crossing-discoverability-2026-08-10.md`: `grep -rn <term> crates/reify-mcp/src/tools/chunks examples/best_practices/INDEX.md .claude/skills/reify-design/SKILL.md`, plus `grep -rnwE "gradient|divergence|curl|laplacian" crates/reify-mcp/src/tools/chunks/`. |

**What this note is not.** This is a DECISION record — it explains why
`units.md` was left unedited and what was landed instead. It is not an
invariant and does not belong in `CLAUDE.md`'s Pointers table; the two guard
tests named below are the durable, executable artifact. A reader who wants
the enforcement mechanism should go straight to
`crates/reify-mcp/src/tools/language_chunks.rs`'s test module; this note is
for the reader who wants to know why no lever was pulled.

## 1. Measurements (prerequisite P1, at `5df2f9f172a5933e3042aeddfa88237416e65d`)

**This was executed, not asserted** — every number below is real stdout,
re-run twice (P1, and again after S1–S4 landed; see §5) and found identical
both times.

### Byte census — all 17 chunks

```
$ for f in crates/reify-mcp/src/tools/chunks/*.md; do wc -c "$f"; done | sort -n
1202 crates/reify-mcp/src/tools/chunks/guards.md
1456 crates/reify-mcp/src/tools/chunks/collections.md
1491 crates/reify-mcp/src/tools/chunks/functions.md
1498 crates/reify-mcp/src/tools/chunks/constraints.md
1573 crates/reify-mcp/src/tools/chunks/occurrences.md
1579 crates/reify-mcp/src/tools/chunks/purposes.md
1653 crates/reify-mcp/src/tools/chunks/types.md
1677 crates/reify-mcp/src/tools/chunks/fields.md
1700 crates/reify-mcp/src/tools/chunks/parameters.md
1723 crates/reify-mcp/src/tools/chunks/connect.md
1758 crates/reify-mcp/src/tools/chunks/structures.md
2111 crates/reify-mcp/src/tools/chunks/syntax.md
3232 crates/reify-mcp/src/tools/chunks/traits.md
4606 crates/reify-mcp/src/tools/chunks/stdlib.md
6117 crates/reify-mcp/src/tools/chunks/units.md
7200 crates/reify-mcp/src/tools/chunks/geometry.md
7227 crates/reify-mcp/src/tools/chunks/enums.md
```

`units.md` is the **3rd largest of 17** — below `enums.md` (7227) and
`geometry.md` (7200) — against a corpus mean of 47803 / 17 ≈ **2812 bytes**.

### `units.md` internal split

```
$ wc -c crates/reify-mcp/src/tools/chunks/units.md
6117 crates/reify-mcp/src/tools/chunks/units.md
$ wc -l crates/reify-mcp/src/tools/chunks/units.md
98 crates/reify-mcp/src/tools/chunks/units.md
$ sed -n '54,$p' crates/reify-mcp/src/tools/chunks/units.md | wc -c
4514
$ sed -n '58p' crates/reify-mcp/src/tools/chunks/units.md | wc -c
1042
$ sed -n '1,53p' crates/reify-mcp/src/tools/chunks/units.md | wc -c
1603
```

So `## Turning a Ratio into an Angle (and Back)` (L54–EOF, 45 of 98 lines) is
4514 / 6117 = **73.8%** of the file; L58 alone (`**Which ratio, though.**`)
is 1042 / 6117 = **17.0%**; L1–53 — #5790 (ξ)'s pending chartered slice — is
1603 bytes.

### Goal-vocabulary singleton check (`Hz`, `rad/s`)

```
$ grep -rn "Hz" crates/reify-mcp/src/tools/chunks examples/best_practices/INDEX.md .claude/skills/reify-design/SKILL.md
crates/reify-mcp/src/tools/chunks/units.md:94:**Angular frequency is a different crossing** — the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, not the η = 1 rad above, and there is no `cycle` unit to write. `Frequency` and `AngularVelocity` are distinct types, so neither silently stands in for the other. See "The 2π rad/cycle distinction (D4)" in `docs/legibility/design-invariants.md`.
$ grep -rn "rad/s" crates/reify-mcp/src/tools/chunks examples/best_practices/INDEX.md .claude/skills/reify-design/SKILL.md
crates/reify-mcp/src/tools/chunks/units.md:94:[same line]
```

Both terms occur **exactly once**, on the **same line** (`units.md:94`), and
nowhere in `INDEX.md` or `SKILL.md`. This is precisely the pair
`docs/notes/angle-crossing-discoverability-2026-08-10.md` Q4 found **RED,
and the sharpest miss in the walk** on its first pass (zero hits for either
term, at `b0d2128279`) — and esc-6267-3 later caught #6267's compression
pass having dropped this exact vocabulary, by hand, post-hoc.

### PDOCCOVER tripwire check (field-operator names)

```
$ grep -rnwE "gradient|divergence|curl|laplacian" crates/reify-mcp/src/tools/chunks/
crates/reify-mcp/src/tools/chunks/units.md:92:No field or tensor operator manufactures `rad` from a derivative — gradient, divergence, curl and laplacian stay pure quotient (`INV-AD-2 quotient-pure-derivative-algebra`). The catalogue of sites where `rad` legitimately enters is in `docs/legibility/design-invariants.md` under "Crossing catalogue and identities"; the governing rule is `INV-AD-1 angle-crossings-explicit`.
```

Exactly **one** word-boundary hit across all 17 chunks, at `units.md:92`.
These four names are `FIELD_OP_NAMES` at
`crates/reify-compiler/src/units.rs:1041`. `crates/reify-audit/pdoccover-baseline.txt`
does not exist, so PDOCCOVER runs un-baselined; losing this line would add
four High `undocumented-name:` findings, and PDOCCOVER is opt-in (not part
of the default sweep), so the merge gate alone would not catch it.

### PDOCCOVER baseline (before any change in this task)

```
$ cargo test -p reify-audit --test pdoccover
PASS: 22 | FAIL: 0 | SKIP: 0
```

## 2. Verdict

**No relocation (lever a). No compression (lever b).** The angle-crossing
doctrine stays in `units.md` at its current byte weight, and this task does
not edit `units.md` at all — every measurement above and in §5 confirms the
chunk corpus is byte-identical to `main` before and after S1–S4 landed.

## 3. Why no change

**(i) Absolute size is not an outlier.** `units.md` at 6117 bytes is the 3rd
largest of 17 chunks, below `enums.md` (7227) and `geometry.md` (7200);
corpus mean 2812. No per-topic size policy exists anywhere in `docs/prds/`
or `docs/legibility/` (grepped, zero hits). The MCP tool serves one topic
per call (`reference.rs` → `get_chunk`), so a large chunk costs only the
author who asked for that topic. No consumer-side budget complaint exists.

**(ii) The 73.8% ratio is an artifact of L1–53's terseness, not the
section's bloat.** L1–53 is 1603 bytes and is #5790 (ξ)'s pending chartered
slice, whose four listed additions (the ONE angle rule, the torque
spelling, the round-trip promise, the `@display` vocabulary change) all
*grow* it. The ratio falls on its own when ξ lands, with no edit to
`## Turning a Ratio into an Angle (and Back)` at all — spending a ticket to
move a number a pending sibling moves anyway is churn.

**(iii) The bytes are walk-earned.**
`docs/notes/angle-crossing-discoverability-2026-08-10.md` records three
queries that came back RED on the first pass and were fixed by rewording
until they hit, plus a fourth caught the same way in the third amendment
pass. Byte weight that tracks measured author failure is the argument FOR
proportionality, not against it.

**(iv) Lever (a) — relocate to a new chunk — costed: mechanically low,
editorially high.** Mechanical cost is ~6 sites:

1. a new `crates/reify-mcp/src/tools/chunks/<topic>.md` file
2. an `include_str!` constant in `language_chunks.rs`
3. an entry in `TOPICS`
4. a `get_chunk` match arm
5. the `available_topics_returns_17_entries` count in `language_chunks.rs`'s
   tests (and, after S2, the `chunk_corpus().len() == TOPICS.len()`
   assertion riding along with it)
6. the duplicated `ALL_TOPICS` list at
   `crates/reify-mcp/tests/reference_tools_tests.rs:14`, plus the
   hand-maintained roster at `.claude/skills/reify-design/SKILL.md:14`

`format_topics_help()` (`crates/reify-mcp/src/tools/reference.rs:49`)
derives from `available_topics()` and needs no edit.

That is cheap. What is not cheap: the doctrine is the operational
*consequence* of `units.md:50–52` — "Angle is the 8th base dimension (not
dimensionless). Catches `torque + energy` as a type error." An author
reaches the `units` topic *because* they hit a dimension error; splitting
the crossing-idiom claim from its consequence into a topic they have no
reason to fetch is a discoverability regression against exactly the walk
#6181 ran and #6267 already compressed too far once. It also re-opens
ground esc-6181-3 settled. **Recommended against** — and the originating
ticket's own guard ("do not restructure chunk topics without explicit
architect/human sign-off") is honoured by declining, not by proceeding.

**(v) Lever (b) — compress L58 — costed: payoff below cost.** L58
(`**Which ratio, though.**`, 1042 bytes, 17.0% of the file) is the annotated
/ unannotated / argument-side `atan` enforcement boundary that
`d1a73a7acc` (fix(6181)) already got wrong once and had to re-split along a
freshly re-measured boundary (esc-6181-3). Compressing it further requires
re-measuring that same boundary against a live compiler and committing the
transcript, to reclaim at most 1042 bytes in a docs chunk with no measured
consumer harm. Cost and regression risk both exceed the payoff.
**Recommended against.**

## 4. What holds the line now

This residual has consumed four filings (#6181 → #6267 → esc-6267-2/-3 →
#6290) because nothing executable held hard constraints 3 and 4 — both were
hand-run greps, and constraint 4 had *already* been breached once
(esc-6267-3 caught the `Hz`/`rad/s` removal by eye, after the fact). S1–S4
converted both into permanent tests in
`crates/reify-mcp/src/tools/language_chunks.rs`'s existing
`#[cfg(test)] mod tests`, which runs at the DEFAULT `cargo test` gate
(unlike opt-in PDOCCOVER):

- **`angle_crossing_goal_vocabulary_survives_in_the_corpus`** (S1/S2) —
  asserts `Hz` and `rad/s` each survive somewhere in the whole chunk corpus
  via the new `pub fn chunk_corpus()` accessor. Replaces the Q4 hand-run
  `grep -rn "Hz"|"rad/s"` instrument.
- **`field_operator_names_keep_a_chunk_mention`** (S3/S4) — asserts
  `gradient`/`divergence`/`curl`/`laplacian` each have a word-boundary
  mention somewhere in the corpus, via a new private `corpus_contains_word`
  helper that mirrors `reify_audit::pdoccover::contains_word`'s semantics
  (including its char-stepped-retry fix for multi-byte chunk prose) without
  adding a `reify-audit` or regex dependency to `reify-mcp`. Replaces the
  hand-run `grep -rnwE "gradient|divergence|curl|laplacian"` instrument.
  Covered in isolation by `contains_word_matches_word_boundaries_only`.

Both guards are asserted over the *whole* corpus, not just the `units`
chunk alone, so a future task may legitimately relocate this vocabulary to
another chunk (were lever (a) ever taken) without tripping either guard —
only an outright drop from the corpus fails them.

## 5. Re-verification after S1–S4 (at `94aaecff43a458d1ce6a3fe8bdcc88d2e0f5588d`)

Every instrument in §1 was re-run after S1–S4 landed. All outputs are
byte-for-byte identical to the P1 blocks above (re-run stdout omitted here
since it is a verbatim duplicate — see the commit history if a
side-by-side is needed). The PDOCCOVER suite is also unchanged:

```
$ cargo test -p reify-audit --test pdoccover
PASS: 22 | FAIL: 0 | SKIP: 0
```

And directly, rather than inferred from unchanged instrument output — the
chunk corpus itself has zero diff against `main`:

```
$ git diff --stat main -- crates/reify-mcp/src/tools/chunks/
(no output)
$ git diff --stat -- crates/reify-mcp/src/tools/chunks/
(no output)
```

This is what discharges hard constraints 1, 3 and 4, and keeps #5790 (ξ)'s
L1–53 seam untouched: this task's entire diff is
`crates/reify-mcp/src/tools/language_chunks.rs` plus this note — no chunk
`.md` file, and in particular not `units.md`, was touched.
