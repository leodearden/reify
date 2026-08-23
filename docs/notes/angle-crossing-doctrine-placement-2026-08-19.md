# Angle-crossing doctrine placement — decision record

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack — the same leaf #6181/#6267 landed into) |
| Task | #6290 |
| Spun out of | esc-6267-1 / esc-6267-2 on #6267, which itself compressed the section #6181 (leaf γ) wrote |
| Branch | `task/6290`, measurements anchored to ancestor commit `2e3f228d2d` ("Merge task/5982 into main" — this branch's merge-base when that pass ran; still resolvable, though the branch has been rebased again since) — the branch was rebased after both the P1 and the post-S1–S4 measurement passes, so neither pass's branch-tip SHA resolves post-rebase; branch-tip SHAs are the wrong thing to cite here for exactly that reason. See §5 for the pass that replaces both. |
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
The original P1 stdout and the post-S1–S4 re-verification stdout are
superseded by this pass; both were genuine at the time, but their
branch-tip provenance SHAs do not resolve post-rebase (see the Branch row
above), so citing their numbers instead of re-running would have been
unverifiable, not merely stale.

`units.md`'s own four numbers — 6117 bytes, 98 lines, 4514-byte section,
1042-byte L58 — and its "3rd largest of 17" rank are the load-bearing
figures this decision rests on, and they reproduce exactly across every
pass, including this one. The *other* chunks' byte counts are not
load-bearing and do drift as unrelated sibling tasks land documentation on
`main` between passes: two rows moved between the original P1 pass and this
one (`stdlib.md` 4606 → 5022, `geometry.md` 7200 → 8243, via `21991bb8a6`
and `e88dbdf1b4` landing on `main`). A future re-run that finds a
*non-`units.md`* row mismatched against this block is expected drift, not a
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
8243 crates/reify-mcp/src/tools/chunks/geometry.md
```

`units.md` is the **3rd largest of 17** — below `geometry.md` (8243) and
`enums.md` (7227) — against a corpus mean of 49262 / 17 ≈ **2898 bytes**.
`geometry.md` and `enums.md` have swapped rank since P1 (`geometry.md` was
7200, now the corpus's largest chunk at 8243), but `units.md`'s own rank —
3rd of 17 — is unaffected either way.

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
not edit `units.md` at all — every measurement above and in §5 confirms the
chunk corpus is byte-identical to `main` before and after S1–S4 landed.

## 3. Why no change

**(i) Absolute size is not an outlier.** `units.md` at 6117 bytes is the 3rd
largest of 17 chunks, below `geometry.md` (8243) and `enums.md` (7227);
corpus mean ~2898. No per-topic size policy exists anywhere in `docs/prds/`
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

S1–S4 attempted to close that gap by converting both hand-run greps into
permanent tests in `crates/reify-mcp/src/tools/language_chunks.rs`'s
existing `#[cfg(test)] mod tests`:
`angle_crossing_goal_vocabulary_survives_in_the_corpus` and
`field_operator_names_keep_a_chunk_mention`, backed by a new
`pub fn chunk_corpus()` accessor and a private `corpus_contains_word`
matcher mirroring `reify_audit::pdoccover::contains_word`'s semantics.

**Comprehensive review rejected both, twice, as documentation
meta-tests, and S8–S9 removed them** along with the test-only
infrastructure that existed solely to back them (`chunk_corpus()`,
`CORPUS`, and three matcher helpers) — the crate is byte-identical to
`main` again (§5). The rejection is not re-litigated here: both tests
asserted on the *prose* of `include_str!`-embedded markdown constants —
that a handful of hand-picked words survive somewhere in the corpus —
which pins wording, not behaviour, and cannot detect whether the
documentation is correct or findable. `field_operator_names_keep_a_chunk_mention`
additionally duplicated `reify_audit::pdoccover::documented_names`
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
`FIELD_OP_NAMES` registry — exactly the check S3/S4 tried to
re-implement — but it is opt-in and un-baselined
(`crates/reify-audit/pdoccover-baseline.txt` does not exist), so it is not
part of the default `cargo test` sweep and would not have caught the
esc-6267-3 regression either. Baselining PDOCCOVER and wiring it into the
default sweep is filed as a follow-up rather than done here: fused-memory
ticket **`tkt_0RSS7P2AHEMW2RSYGCHV7DNMQX`** (spawned from #6290,
pre-curation — a *ticket* id, not a `#NNNN` task number). It is
deliberately not inline in this task: it lands in `reify-audit`, not
`reify-mcp`, and is likely to touch verify-pipeline files, which per
`CLAUDE.md` forces the full `--scope all --profile both` gate — a
high-risk gate change has no place on a branch whose entire verdict is
"change nothing."

## 5. Re-verification (S6–S10)

S6 and S7 were docs/comment-only passes that did not change this section's
conclusions. **S8 and S9 did**: they deleted the two guard tests S1–S4
built (rejected on review, §4) and the now-unused `chunk_corpus()`/`CORPUS`
infrastructure that backed them, so the counts below are lower than every
earlier pass recorded, on purpose. S10 is this repair itself.

The `Hz`/`rad/s` and PDOCCOVER-name grep instruments still reproduce
byte-for-byte against the §1 blocks above — re-run fresh for this section,
not assumed unchanged, since neither S8 nor S9 touched any chunk `.md`
file (same hits, same lines — `units.md:94` and `units.md:92`
respectively, nothing in `INDEX.md` or `SKILL.md`). The PDOCCOVER suite:

```
$ cargo test -p reify-audit --test pdoccover
PASS: 22 | FAIL: 0 | SKIP: 0
```

`cargo test -p reify-mcp` is green at **110/110** — down from the
113/113 S1–S4 established, which is the expected delta: exactly the three
tests S8 deleted (the two rejected guards plus
`contains_word_matches_word_boundaries_only`, which existed only to cover
one of them).

```
$ cargo clippy -p reify-mcp --all-targets -- -D warnings
BUILD OK | warnings: 0 | errors: 0
```

And directly, rather than inferred from instrument output — the chunk
corpus itself has zero diff against `main`, and after S9,
`language_chunks.rs` itself is byte-identical to `main` too:

```
$ git diff --stat main -- crates/reify-mcp/src/tools/chunks/
(no output)
$ git diff --stat main -- crates/reify-mcp/
(no output)
$ git diff main -- crates/reify-mcp/src/tools/language_chunks.rs
(no output)
```

This is what discharges hard constraints 1, 3 and 4, and keeps #5790 (ξ)'s
L1–53 seam untouched.

**One honestly-reported wrinkle.** An unscoped `git diff --stat main` is
*not* this note alone right now, and pasting it as if it were would repeat
the exact defect S7 fixed: `main` has independently advanced since this
branch's own base — a `task/6376` merge (NaN-safe-ordering / FEA
hardening, touching `reify-expr`/`reify-stdlib`/`docs/prds/compute-fea-hardening.md`/
`scripts/check-nan-safe-ordering.sh` and its test) plus a same-day
"nightly trickle sightings" docs commit — none of which this task touched
or is responsible for; it is base drift, exactly like the sibling-chunk
byte-census drift §1 already documents, just at the whole-repo scope
instead of one directory. The instrument that actually isolates *this
task's own diff* from that drift is the merge-base-anchored one, not a
diff against `main`'s constantly-moving tip:

```
$ git diff --name-only "$(git merge-base HEAD main)" HEAD
docs/notes/angle-crossing-doctrine-placement-2026-08-19.md
```

Deliberately `--name-only`, not `--stat`: an insertion count for a diff
whose only file *is this note* is self-referential — every later edit to
§5 changes the number §5 quotes, so a pasted line count is falsified by
the act of pasting it. The path list is edit-stable and carries the whole
claim: this task's entire diff, relative to where it actually branched, is
this note and nothing else. `crates/reify-mcp/` nets to zero change —
S1–S4 added the rejected guards, S8–S9 removed them again, per the empty
`git diff --stat main -- crates/reify-mcp/` above — and no chunk `.md`
file, in particular not `units.md`, was ever touched. The merge-base SHA
is likewise not pasted: it moves on every rebase, exactly like the
branch-tip SHAs the Provenance row already warns about.
