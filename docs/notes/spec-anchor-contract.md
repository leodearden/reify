# Spec anchor contract

Normative rules for the stable cite targets in `docs/reify-language-spec.md`.
Owning PRD: `docs/prds/v0_6/spec-conformance-suite.md` (D3, §8.1), leaf α
(task #6758). Everything below is a RULE, not a description — later
spec-conformance waves depend on it holding.

## Purpose

The conformance suite quantifies over CLAUSES of the language specification.
Keying a fixture to a clause by section number is fragile: a renumber
re-points every cite at once and nothing detects it. That is not
hypothetical — `tree-sitter-reify/tests/spec_purpose_example_grammar.rs`
`include_str!`s the spec and splits it on the literal heading strings
`### 9.5 Purposes` and `### 4.4 Purpose Declarations`, so an editor renaming
or renumbering either heading breaks a compiled test with no warning. Leaf α
itself renumbered a mis-numbered `### 13.1` inside §15 to `### 15.1`.

An anchor replaces that with an identifier that means exactly one thing and
keeps meaning it.

## Syntax

An anchor is a standalone HTML-comment line:

```
<!-- sc-anchor: sc-XXXXXX -->
```

placed IMMEDIATELY before the paragraph or heading it anchors — no blank line
between them. It is invisible in rendered markdown and greppable in source.

Only the metavariable `sc-XXXXXX` appears anywhere in this document, and that
is deliberate: a realistic-looking example ID in normative documentation is a
collision hazard the moment somebody copies it into the spec. `sc-XXXXXX` is
not hexadecimal, so it cannot match the ID grammar and the hazard is
structurally impossible rather than something a lint has to catch afterwards.

## ID grammar

An ID matches `sc-[0-9a-f]{6}` — six hexadecimal digits.

- **Random at assignment.** IDs are randomly generated, never derived from
  anything about the clause they mark. Mint with:

  ```
  openssl rand -hex 3
  ```

  (fallback `head -c3 /dev/urandom | xxd -p`).

- **Opaque and never positional.** An ID must never encode, or correlate
  with, a section number, a heading, an ordering, or anything else about
  where the clause currently sits. If you can predict the next ID, the scheme
  is broken. Mint the whole batch up front, prove it collision-free against
  the spec, and pair IDs to targets in minted order rather than in document
  order.

- **Never reused.** Once an ID has been assigned it is that clause's forever,
  and it is never reused. A retired ID is **retired forever**: it is recorded
  in the tombstone file and never assigned to anything again. Six hex digits
  is 16.7M values — exhaustion is not a concern, and reuse would silently
  re-point every existing cite.

## Scope of an anchor

- A **heading-attached** anchor covers the whole run of intro prose belonging
  to that heading, up to the first subheading or the next anchored paragraph.
- A **paragraph-attached** anchor covers that paragraph, plus any table or
  fenced block the paragraph introduces.
- An anchor never goes **inside** a fenced code block. The lint skips fenced
  regions when scanning, so an anchor placed inside one is not a live anchor
  at all — it is a dangling cite waiting to happen. The lint REPORTS one
  rather than ignoring it (rule 7), so writing an anchor-shaped example with
  a real hex id inside a fence is a violation; write examples with the
  metavariable id `sc-XXXXXX`, which is not anchor-shaped.

A corollary of the fence rule: prose that DISCUSSES the anchor mechanism
belongs in this note, not in the spec body. The lint treats any spec line
mentioning the marker outside a fence as a malformed anchor.

## Tombstones

Retired IDs live in `docs/reify-language-spec.tombstones`. Row grammar:

```
sc-XXXXXX <YYYY-MM-DD> <reason / forwarding anchor>
```

- Data rows are sorted by ID in `LC_ALL=C` ascending order.
- `#` comment lines and blank lines are ignored anywhere and are NOT sort
  keys — interleave them freely.
- Name the forwarding anchor in the reason when a clause was superseded
  rather than dropped.

**Deleting an anchored paragraph REQUIRES moving its ID into that file in the
SAME diff as the deletion.** This is the rule the whole contract rests on:
without it, "cite by opaque ID" degrades to "cite by an ID that may or may not
still exist", which is strictly worse than citing a section number — a stale
section number is at least visibly stale.

## Consumer rule

A consumer resolves an anchor ONLY by **grepping the literal ID**. No
consumer may parse section numbers, heading text, or line numbers for
identity.

That rule is what makes editing the spec safe. It is why leaf α's
`13.1` → `15.1` renumber inside §15 was a free correction rather than a
breaking change, and why future renumbers stay free. It is also the standard
the existing heading-string consumer named above does not yet meet — retro-fitting
it is a separate piece of work, not licence to add more section-number
coupling.

## Incrementality

Anchors are seeded **section by section**, with each section's first fixture
wave — not all at once. §9.2 (`undef` Semantics) is seeded by leaf α. A
section with no anchors yet is UNANCHORED, and the coverage lane reports it
as such; it is never silently treated as covered. Incompleteness stays
visible.

## Enforcement

`scripts/spec-anchor-lint.sh` checks seven rules: ID format, uniqueness across
the spec, live/tombstone disjointness, tombstone row grammar and sortedness,
anchor placement, deletion-implies-tombstone, and no swallowed anchors — every
anchor-shaped line in the file is either live or reported, tallied with fence
state ignored, so neither an in-fence anchor nor a desynchronised fence toggle
can silently remove a region from the scan. It **hard-fails** — there is no
warn mode, no strict-promotion flag, and no path that gracefully skips a check
it could not run.

Exit codes: `0` clean, `1` at least one violation, `2` usage error,
missing-or-empty input, or an internal failure. `1` and `2` are strictly
distinct so that "could not scan" can never be mistaken for "scanned and found
nothing".

Its self-test is `tests/infra/test_spec_anchor_lint.sh`: every mutant is built
from the real shipped spec, each carries a meta-assertion that the mutation
actually mutated, and a live control asserts the shipped corpus scans clean —
so a clean verdict is a measurement by a working instrument, not the silence
of a broken one.
