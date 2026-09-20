# Angle-crossing doctrine placement — decision record

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack — the same leaf #6181/#6267 landed into) |
| Task | #6290 |
| Spun out of | esc-6267-1 / esc-6267-2 on #6267, which itself compressed the section #6181 (leaf γ) wrote |
| Branch | `task/6290` — the branch has been rebased multiple times since the original P1 measurement pass and since a later, mid-implementation measurement pass, so neither pass's branch-tip SHA resolves today; branch-tip SHAs are the wrong thing to cite for measurement provenance on a branch that gets rebased. §1 anchors the byte census to a fixed historical commit instead, for exactly that reason — see there for which one, and why. |
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

## 1. Measurements (prerequisite P1; re-run at fixed historical commit `db3caf593c`)

**Load-bearing vs. snapshot.** `units.md`'s own four numbers — 6117 bytes,
98 lines, 4514-byte section, 1042-byte L58 — and its 3rd-of-17 rank are the
load-bearing figures this decision rests on. Everything else below in this
note — other chunks' byte counts, the corpus total/mean, the
`reify-audit` pdoccover pass count — is this pass's snapshot: recompute
rather than cite it forward. A mismatch on a non-load-bearing figure below
is expected drift, not a falsified record; a mismatch on one of the four
load-bearing figures is the thing to investigate. (See "Revision
history" at the end of this note for how this section's anchor, and an
earlier guard-test attempt referenced in §4, evolved.)

**This was executed, not asserted** — every number below is real stdout,
re-run against a clean extraction of commit `db3caf593c` ("Merge task/6441
into main"): `git archive db3caf593c | tar -x` into a scratch directory
outside the repository (`git worktree add` needs write access to the
shared `.git/worktrees/` metadata directory, which this task does not
have; the archive extraction is an equivalent clean checkout for a
read-only census). `db3caf593c` is cited as a fixed historical SHA, not as
"the merge base" — a merge-base is a moving description that a later
rebase invalidates (this branch's own merge-base with `main` has already
moved once since this commit was chosen, to a later commit — see §5's
re-verification), whereas a named commit's tree never changes and stays
resolvable for as long as the commit stays reachable. Independently
re-verified here: `db3caf593c` is still an ancestor of both the current
branch tip and current `main`, so — unlike a merge-base or a branch-tip
SHA — it stays citable no matter how many more times either moves.

The *other* chunks' byte counts are not load-bearing and drift as
unrelated sibling tasks land documentation on `main`: relative to the P1
baseline, `stdlib.md` moved once (4606 → 5022, via
`21991bb8a6`/`e88dbdf1b4`) and `geometry.md` moved twice (7200 → 8243 →
8622, via `a29bcbe812`); both hops predate `db3caf593c` and are already
reflected in the block below. `constraints.md` (1646) and `traits.md`
(3335) are already at their post-`#6213`-amend values in that block — see
Revision history for the earlier, mis-anchored pass that predated the
amend.

### Byte census — all 17 chunks

```
$ for f in crates/reify-mcp/src/tools/chunks/*.md; do wc -c "$f"; done | sort -n
1202 crates/reify-mcp/src/tools/chunks/guards.md
1456 crates/reify-mcp/src/tools/chunks/collections.md
1491 crates/reify-mcp/src/tools/chunks/functions.md
1573 crates/reify-mcp/src/tools/chunks/occurrences.md
1579 crates/reify-mcp/src/tools/chunks/purposes.md
1646 crates/reify-mcp/src/tools/chunks/constraints.md
1653 crates/reify-mcp/src/tools/chunks/types.md
1677 crates/reify-mcp/src/tools/chunks/fields.md
1700 crates/reify-mcp/src/tools/chunks/parameters.md
1723 crates/reify-mcp/src/tools/chunks/connect.md
1758 crates/reify-mcp/src/tools/chunks/structures.md
2111 crates/reify-mcp/src/tools/chunks/syntax.md
3335 crates/reify-mcp/src/tools/chunks/traits.md
5022 crates/reify-mcp/src/tools/chunks/stdlib.md
6117 crates/reify-mcp/src/tools/chunks/units.md
7227 crates/reify-mcp/src/tools/chunks/enums.md
8622 crates/reify-mcp/src/tools/chunks/geometry.md
```

`units.md` is the **3rd largest of 17** — below `geometry.md` (8622) and
`enums.md` (7227) — against a corpus mean of 49892 / 17 ≈ **2935 bytes**.
(That mean and the exact `geometry.md` figure are this pass's snapshot, not
a constant — see the load-bearing note above; recompute rather than cite
them forward.) `geometry.md` and `enums.md` swapped rank between P1 and later
passes (`geometry.md` was 7200, then 8243, then 8622 as of `a29bcbe812`);
`units.md`'s own rank — 3rd of 17 — is unaffected by any of it.

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
PASS: 27 | FAIL: 0 | SKIP: 0
```

Only `FAIL: 0` is load-bearing here, per the note at the top of §1 —
`reify-audit` is outside this task's scope, so its pass count drifts
independent of this task's own change (an earlier pass here recorded
`PASS: 22`).

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
corpus mean ~2935 (§1's byte census; snapshot, not load-bearing — see §1).
No per-topic size
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

This branch tried closing that gap with two hand-rolled guard tests in
`language_chunks.rs` (added, then removed after review — see Revision
history). **Comprehensive review rejected both, twice, as documentation
meta-tests**, and the rejection holds on two independent grounds. First,
both tests asserted on the *prose* of `include_str!`-embedded markdown
constants — that a handful of hand-picked words survive somewhere in the
corpus — which pins wording, not behaviour, and cannot detect whether the
documentation is correct or findable. Second, one of them duplicated
`reify_audit::pdoccover::documented_names`
(`crates/reify-audit/src/pdoccover.rs`), which computes the identical
word-boundary-mention question against the *live* nine-member
`FIELD_OP_NAMES` registry; that guard's hard-coded four-of-nine copy
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
#5480 seeds the baseline") before #6290 filed anything — which is why
that doc comment still names #5480 rather than a fresher id. **#5480 was
itself coalesced into #6931** by the 2026-08-28 backlog sweep
(`x_coalesced_from: [5480, 6233, 6891]`) and is now `status: deferred`,
so **#6931 — not #5480, and not the now-superseded ticket id — is the
task to follow for this gap's live status today.** It is deliberately
not inline in this task: it lands in
`reify-audit`, not `reify-mcp`, and is likely to touch verify-pipeline
files, which per `CLAUDE.md` forces the full `--scope all --profile both`
gate — a high-risk gate change has no place on a branch whose entire
verdict is "change nothing."

## 5. Re-verification

The counts below are lower than an earlier pass on this branch recorded,
on purpose — reflecting the guard-test removal described in §4 and
Revision history.

The `Hz`/`rad/s` and PDOCCOVER-name grep instruments still reproduce
byte-for-byte against the §1 blocks above — re-run fresh for this section,
not assumed unchanged, since no chunk `.md` file was touched (same hits,
same lines — `units.md:94` and `units.md:92` respectively, nothing in
`INDEX.md` or `SKILL.md`). The PDOCCOVER suite:

```
$ cargo test -p reify-audit --test pdoccover
PASS: 27 | FAIL: 0 | SKIP: 0
```

Green again (`FAIL: 0`), matching §1's baseline re-run above; the count
itself is a snapshot, not load-bearing (see §1).

`cargo test -p reify-mcp` is green at **110/110** — three fewer than an
earlier pass on this branch, the expected delta from removing the two
rejected guards plus `contains_word_matches_word_boundaries_only` (which
existed only to cover one of them). Unlike `reify-audit`, `reify-mcp`
*is* a suite this task touches; per the load-bearing note in §1, the
totals themselves are a snapshot — the load-bearing fact is the delta,
exactly those three tests and no others.

```
$ cargo clippy -p reify-mcp --all-targets -- -D warnings
BUILD OK | warnings: 0 | errors: 0
```

And directly, rather than inferred from instrument output — every diff
instrument below is anchored to this branch's **merge base with `main`**,
not to `main`'s moving tip (a deliberate choice, not the original one —
see Revision history). A check anchored to a moving target — `main`'s
tip, a branch-tip SHA — cannot survive a rebase, because a rebase is
precisely the operation that moves the target out from under it; this is
the same failure mode this note's own Provenance row documents for
branch-tip SHAs, and §1 documents for the byte census.

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

## Revision history

This note went through several revisions before landing; the full detail
is in `git log` for this file. The high-level shape, for a reader
wondering why other sections mention tests or anchors that are not in the
current text:

- **Census re-anchored.** An earlier pass anchored §1's byte census to
  ancestor commit `2e3f228d2d`; that attribution did not hold up under
  re-verification (no tree at that SHA produces the pasted block). A
  later revision re-ran the census against `db3caf593c` instead and
  replaced the block with that run's real stdout.
- **Guard tests added, then removed.** An earlier revision added two
  tests to `crates/reify-mcp/src/tools/language_chunks.rs`
  (`angle_crossing_goal_vocabulary_survives_in_the_corpus`,
  `field_operator_names_keep_a_chunk_mention`) plus a `chunk_corpus()`
  accessor and matcher helpers to back them. Comprehensive review
  rejected both, twice, as documentation meta-tests (§4); a later
  revision removed them and their supporting infrastructure, leaving
  `language_chunks.rs` byte-identical to `main`.
- **§5 diff instruments re-anchored from `main` to merge-base.** An
  earlier revision anchored §5's diff checks to `main`'s tip and, for a
  time, pasted a non-empty whole-directory hunk attributed to task
  #6213; a later rebase past #6213 made that pasted output stale in the
  opposite direction. A later revision re-anchored every diff instrument
  in §5 to `git diff --name-only "$(git merge-base HEAD main)" HEAD`,
  which is invariant under both rebase and sibling drift.
