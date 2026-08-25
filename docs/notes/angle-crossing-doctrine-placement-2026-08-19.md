# Angle-crossing doctrine placement — decision record

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack — the same leaf #6181/#6267 landed into) |
| Task | #6290 |
| Spun out of | esc-6267-1 / esc-6267-2 on #6267, which itself compressed the section #6181 (leaf γ) wrote |
| Branch | `task/6290`, measurements anchored to ancestor commit `2e3f228d2d` ("Merge task/5982 into main" — this branch's merge-base when that pass ran; still resolvable, though the branch has been rebased again since) — the branch was rebased after both the P1 measurement pass and a second, mid-implementation measurement pass, so neither pass's branch-tip SHA resolves post-rebase; branch-tip SHAs are the wrong thing to cite here for exactly that reason. See §5 for the pass that replaces both. |
| Date | 2026-08-19 |
| Instrument | Byte census: `wc -c`/`wc -l` over `crates/reify-mcp/src/tools/chunks/*.md`, plus `sed -n` slices of `units.md`. Discoverability walk's instrument, unchanged from `docs/notes/angle-crossing-discoverability-2026-08-10.md`: `grep -rn <term> crates/reify-mcp/src/tools/chunks examples/best_practices/INDEX.md .claude/skills/reify-design/SKILL.md`, plus `grep -rnwE "gradient|divergence|curl|laplacian" crates/reify-mcp/src/tools/chunks/`. |

**What this note is not.** This is a DECISION record — it explains why
`units.md` was left unedited and what was landed instead. It is not an
invariant and does not belong in `CLAUDE.md`'s Pointers table. The durable
artifact of this ticket is the decision itself, plus the measured evidence
in §1 — **not** an executable guard. An earlier revision of this note
pointed a reader here to `crates/reify-mcp/src/tools/language_chunks.rs`'s
test module as "the enforcement mechanism"; that pointer is retracted —
comprehensive review rejected the two guard tests it named as
documentation meta-tests, and they do not ship (§4). This note is for the
reader who wants to know why no lever was pulled — and, per §4, for the
reader asking what (if anything) currently enforces hard constraints 3 and
4.

## 1. Measurements (prerequisite P1; re-run at ancestor commit `2e3f228d2d`, post-rebase)

**This was executed, not asserted** — every number below is real stdout,
re-run at commit `2e3f228d2d`, which was this branch's merge-base at the time
of that pass. The branch has been rebased again since, so `2e3f228d2d` is a
resolvable historical anchor rather than today's merge-base — it is cited
that way, not as a live one, and the numbers below still reproduce.
The original P1 stdout and a second, mid-implementation re-verification
stdout are superseded by this pass; both were genuine at the time, but
their branch-tip provenance SHAs do not resolve post-rebase (see the
Branch row above), so citing their numbers instead of re-running would
have been unverifiable, not merely stale.

`units.md`'s own four numbers — 6117 bytes, 98 lines, 4514-byte section,
1042-byte L58 — and its "3rd largest of 17" rank are the load-bearing
figures this decision rests on, and they reproduce exactly across every
pass, including this one. The *other* chunks' byte counts are not
load-bearing and do drift as unrelated sibling tasks land documentation on
`main` between passes: `stdlib.md` moved once (4606 → 5022, via
`21991bb8a6`/`e88dbdf1b4`) and `geometry.md` has now moved *twice*
(7200 → 8243 → 8622 — the second hop via `a29bcbe812`, which landed on
`main` after the previous pass recorded this block at 8243). Any figure
*derived* from these rows — the corpus mean, and which chunk currently
ranks largest — is exactly as drift-subject as the rows themselves, even
though it is written below as a plain number: treat it as this pass's
snapshot, not a constant to cite forward. A future re-run that finds a
*non-`units.md`* row, or a derived total/mean/rank built only from
non-`units.md` rows, mismatched against this block is expected drift, not a
falsified record; a mismatch on one of `units.md`'s own four numbers would
be the thing to investigate.

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
5022 crates/reify-mcp/src/tools/chunks/stdlib.md
6117 crates/reify-mcp/src/tools/chunks/units.md
7227 crates/reify-mcp/src/tools/chunks/enums.md
8622 crates/reify-mcp/src/tools/chunks/geometry.md
```

`units.md` is the **3rd largest of 17** — below `geometry.md` (8622) and
`enums.md` (7227) — against a corpus mean of 49641 / 17 ≈ **2920 bytes**.
(That mean and the exact `geometry.md` figure are this pass's snapshot, not
a constant — see the drift note above; recompute rather than cite them
forward.) `geometry.md` and `enums.md` swapped rank between P1 and the
previous pass (`geometry.md` was 7200, then 8243), and `geometry.md` has
grown again since (8243 → 8622); `units.md`'s own rank — 3rd of 17 — is
unaffected by any of it.

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
These four names are the derivative-operator subset (four of the nine
members) of `FIELD_OP_NAMES` in `crates/reify-compiler/src/units.rs` —
cited by symbol, not line number: a line number into another crate drifts
on any unrelated edit there, which is how the previous `:1041` cite went
stale (the const actually lives at `:1077`, and was already wrong when
that cite was written, not merely drifted since). `crates/reify-audit/pdoccover-baseline.txt`
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
not edit `units.md`, any other chunk file, or `language_chunks.rs` at
all — §5's merge-base-anchored diff confirms this task's own change is the
note you are reading and nothing else, including across the guard-test
attempt described in §4. (A raw `git diff ... main` against the wider
`chunks/` directory can show *unrelated* drift from sibling tasks landing
on `main` after this branch forked — the same phenomenon as §1's
byte-census drift, on a different instrument; see §5.)

## 3. Why no change

**(i) Absolute size is not an outlier.** `units.md` at 6117 bytes is the 3rd
largest of 17 chunks, below `geometry.md` (8622) and `enums.md` (7227);
corpus mean ~2920 (§1's byte census — the `geometry.md` figure and the mean
are today's snapshot of chunks this task does not touch and will drift
again; `units.md`'s 3rd-of-17 rank is what does not). No per-topic size
policy exists anywhere in `docs/prds/`
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
   tests
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

## 4. What holds the line — and what still does not

This residual has consumed four filings (#6181 → #6267 → esc-6267-2/-3 →
#6290) because nothing executable held hard constraints 3 and 4 — both were
hand-run greps, and constraint 4 had *already* been breached once
(esc-6267-3 caught the `Hz`/`rad/s` removal by eye, after the fact).

An earlier revision of this branch attempted to close that gap by adding
two guard tests to `crates/reify-mcp/src/tools/language_chunks.rs`'s
existing `#[cfg(test)] mod tests`:
`angle_crossing_goal_vocabulary_survives_in_the_corpus` and
`field_operator_names_keep_a_chunk_mention`, backed by a new
`pub fn chunk_corpus()` accessor and a private `corpus_contains_word`
matcher mirroring `reify_audit::pdoccover::contains_word`'s semantics.

**Comprehensive review rejected both, twice, as documentation
meta-tests, and a later revision removed them** along with the test-only
infrastructure that existed solely to back them (`chunk_corpus()`,
`CORPUS`, and three matcher helpers) — `language_chunks.rs` is
byte-identical to `main` again (§5). The rejection is not re-litigated
here: both tests asserted on the *prose* of `include_str!`-embedded
markdown constants — that a handful of hand-picked words survive
somewhere in the corpus — which pins wording, not behaviour, and cannot
detect whether the documentation is correct or findable.
`field_operator_names_keep_a_chunk_mention` additionally duplicated
`reify_audit::pdoccover::documented_names`
(`crates/reify-audit/src/pdoccover.rs`), which computes the identical
word-boundary-mention question against the *live* nine-member
`FIELD_OP_NAMES` registry; this guard's hard-coded four-of-nine copy
verified strictly *less* than the tool it mirrored, and would silently go
stale the moment that registry gained or lost a member.

**So: as of this branch, hard constraints 3 and 4 are enforced by nothing
but a human re-running the greps in §1 by hand — the same gap that has
now caused four filings.** That gap is real, and it is currently **OPEN**;
this note does not close it. The correct owner is PDOCCOVER
(`crates/reify-audit/src/pdoccover.rs`), which already computes
registry-name-vs-chunk word-boundary coverage against the live
`FIELD_OP_NAMES` registry — exactly the check the rejected guard tried to
re-implement — but it is opt-in and un-baselined
(`crates/reify-audit/pdoccover-baseline.txt` does not exist), so it is not
part of the default `cargo test` sweep and would not have caught the
esc-6267-3 regression either.

Baselining PDOCCOVER and wiring it into the default sweep is filed as a
follow-up rather than done here — originally opened as fused-memory
ticket `tkt_0RSS7P2AHEMW2RSYGCHV7DNMQX` (spawned from #6290,
pre-curation), since combined by the curator into pending task **#5480**
("PDOCCOVER hard gate: seeded baseline + verify-sweep wiring, confirmed
to own registry-name-vs-chunk coverage", PRD
`docs/prds/v0_6/doc-chunk-truth-enforcement.md` task δ). #5480 pre-dates
this ticket: `crates/reify-audit/src/lib.rs`'s `PDocCover` doc comment
already named it as the baseline's owner ("the census is non-empty until
#5480 seeds the baseline") before #6290 filed anything, so **#5480, not
the now-superseded ticket id, is the task to follow for this gap's live
status.** It is deliberately not inline in this task: it lands in
`reify-audit`, not `reify-mcp`, and is likely to touch verify-pipeline
files, which per `CLAUDE.md` forces the full `--scope all --profile both`
gate — a high-risk gate change has no place on a branch whose entire
verdict is "change nothing."

## 5. Re-verification

An earlier revision of this branch added the two guard tests described in
§4 to `language_chunks.rs`; comprehensive review rejected them as
documentation meta-tests, and they were removed along with the
`chunk_corpus()`/`CORPUS` infrastructure that backed them, leaving
`language_chunks.rs` byte-identical to `main` again. The counts below are
lower than an earlier pass on this branch recorded, on purpose, reflecting
that removal.

The `Hz`/`rad/s` and PDOCCOVER-name grep instruments still reproduce
byte-for-byte against the §1 blocks above — re-run fresh for this section,
not assumed unchanged, since no chunk `.md` file was touched (same hits,
same lines — `units.md:94` and `units.md:92` respectively, nothing in
`INDEX.md` or `SKILL.md`). The PDOCCOVER suite:

```
$ cargo test -p reify-audit --test pdoccover
PASS: 22 | FAIL: 0 | SKIP: 0
```

`cargo test -p reify-mcp` is green at **110/110** — three fewer than the
count an earlier pass on this branch established, which is the expected
delta: exactly the two rejected guards plus
`contains_word_matches_word_boundaries_only`, which existed only to cover
one of them.

```
$ cargo clippy -p reify-mcp --all-targets -- -D warnings
BUILD OK | warnings: 0 | errors: 0
```

And directly, rather than inferred from instrument output — every diff
instrument below is anchored to this branch's **merge base with `main`**,
not to `main`'s moving tip, and that choice is deliberate rather than
incidental. The text this replaces anchored the same checks to `main`
directly (`git diff main -- <path>`, and a whole-directory `git diff
--stat main -- crates/reify-mcp/src/tools/chunks/`) and, for the
whole-directory check, pasted a non-empty 2-file hunk with a paragraph
attributing it to task #6213 having landed on `main` after this branch's
then-merge-base. Re-run on this tip, that same check is empty: this
branch has since been rebased onto a newer `main` tip whose ancestry
already includes #6213, so the divergence the pasted hunk described no
longer exists relative to the current fork point, and both the pasted
stdout and the paragraph explaining it are stale in the opposite
direction. This is the same failure mode this note's own Provenance row
already documents for branch-tip SHAs, and that §1 already documents for
the byte census: a check anchored to a moving target — `main`'s tip, a
branch-tip SHA — cannot survive a rebase, because a rebase is precisely
the operation that moves the target out from under it.

A merge-base-anchored form does not have that failure mode. It diffs from
this branch's fixed fork point, so it only ever reports *this task's own*
commits — a sibling landing unrelated edits on `main` past that point is
invisible to it by construction, since the check never reads `main`'s tip
at all, and a rebase simply moves the fork point the next run measures
from, rather than invalidating a previously-recorded number. `--name-only`
rather than `--stat`, for the same reason used elsewhere in this note: an
insertion/deletion count is self-referential — any later edit to this
section would change the very count it quotes.

```
$ git diff --name-only "$(git merge-base HEAD main)" HEAD -- crates/reify-mcp/src/tools/chunks/units.md
(no output)
$ git diff --name-only "$(git merge-base HEAD main)" HEAD -- crates/reify-mcp/src/tools/language_chunks.rs
(no output)
$ git diff --name-only "$(git merge-base HEAD main)" HEAD -- crates/reify-mcp/src/tools/chunks/
(no output)
```

Neither `units.md`, nor `language_chunks.rs`, nor any other file under
`chunks/`, appears in this task's own diff from its fork point — this is
what actually discharges hard constraints 1, 2, 3 and 4, and keeps #5790
(ξ)'s L1–53 seam untouched. A sibling task landing unrelated chunk edits
on `main` — the same event that flipped the `main`-anchored version of
this check from empty to non-empty and back — cannot perturb a
merge-base-anchored one, so this form needs no follow-up paragraph
excusing a stale reading.

The same form with no path filter bounds this task's *entire* diff, not
only the chunk directory:

```
$ git diff --name-only "$(git merge-base HEAD main)" HEAD
docs/notes/angle-crossing-doctrine-placement-2026-08-19.md
```

This task's entire diff, relative to where it actually branched, is this
note and nothing else. No chunk `.md` file — in particular not
`units.md` — and no code file was ever touched by this task.
