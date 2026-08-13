# Angle-crossing discoverability — acceptance walk

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack) |
| Task | #6181 |
| Branch | `task/6181` — first walked at `b0d2128279` (steps 1–3 landed), amended at `6ac1bb7ad5`, re-walked at `9d5d2ca6fd` (step-5 landed), re-walked again at `b8473a248d` (third amendment pass), enforcement claim re-measured and re-walked at the fifth pass (esc-6181-3) |
| Date | 2026-08-10 |
| Acceptance kind | `manual` — the capability manifest records that acceptance *is* this committed transcript |
| Instrument | `grep -rin <terms> crates/reify-mcp/src/tools/chunks examples/best_practices/INDEX.md .claude/skills/reify-design/SKILL.md` |

**Why grep is the instrument.** There is no intent-search engine in reify.
An author's actual discovery surface is the MCP `available_topics` listing
plus a chunk fetch, and grepping `examples/best_practices/INDEX.md` (which
that file's own "How to use this index" section instructs: *"Before probing
the language, grep this file"*). So the walk greps the three surfaces γ
landed into, using the keywords a goal phrase actually reduces to. `$S`
below abbreviates those three paths.

**This was executed, not asserted.** Three queries came back with **zero
hits** on the first pass; the wording landed in steps 1–3 was changed and
the queries re-run until they hit. Those three are itemised under "Wording
changed because the walk failed it"; a fourth, caused by a later content
edit and caught the same way, is item 5 of the third amendment pass.

## Verdict table

Every citation below was re-taken from the re-walk at `b8473a248d`; see
"Third amendment pass" for why.

**Citations are anchors, not line numbers.** The verdict tables name the
heading or bold lead phrase a hit lands under; raw `file:line` appears only
inside the verbatim stdout blocks, where it is a measurement artifact
stamped with the sha it was taken at. Two rounds of line-number rot inside
this one task (itemised under the second and third amendment passes) are the
argument: `units.md` and `SKILL.md` are live edit targets for sibling leaves
— notably #5790 (ξ), whose slice rewrites the *Angle as Base Dimension*
section sitting directly above this content — and every line below an
insertion point silently invalidates. Headings survive insertions above
them; line numbers do not.

| # | Goal phrase | Surface expected | Verdict | Evidence (anchor) |
|---|---|---|---|---|
| 1 | "turn this ratio into an angle" | units chunk section + INDEX row | **HIT** | units.md → §*Turning a Ratio into an Angle (and Back)* (the heading itself); INDEX.md → `angle_crossings.ri` row; SKILL.md → *Turning an arc-measure ratio into an angle* bullet |
| 2 | "why do I multiply by 1rad" | same section / SKILL index line | **HIT** (after fix) | units.md → that section's lead sentence, and its *Always the no-space literal* line; SKILL.md → same bullet |
| 3 | "arc length from radius and angle" | `s = r·θ/η` teaching + exemplar | **HIT** (after fix) | units.md → that section's code block (`arc2` line) and the paragraph under it; INDEX.md → `angle_crossings.ri` row; SKILL.md → same bullet |
| 4 | "Hz to rad/s" / "angular frequency" | 2π rad/cycle paragraph | **HIT** (after fix) | units.md → *Angular frequency is a different crossing*; SKILL.md → same bullet's closing line |
| 5 | "why is torque N·m/rad" | one-sentence crossing rationale | **PARTIAL — rationale hits, spelling absent (pending #5790)** | units.md → *Why torque is N·m/rad* |

## Per-query evidence

**Reading the blocks below.** Each is real stdout from the re-walk at
`b8473a248d`; long matched lines are elided with `…`. Blocks recording a
first-pass RED are labelled `# FIRST PASS` with the sha they were measured
at and are historical; everything else matches the tree as committed. The
`file:line` numbers inside these blocks are measurement artifacts of that
sha — the verdict tables above carry the durable anchors.

### Q1 — "turn this ratio into an angle"

```
$ grep -rin "ratio into an angle\|ratio -> angle\|ratio.*Angle" $S
crates/reify-mcp/src/tools/chunks/units.md:9:A vector of rational exponents over 10 base dimensions (7 SI + Angle + SolidAngle + Money):
crates/reify-mcp/src/tools/chunks/units.md:54:## Turning a Ratio into an Angle (and Back)
crates/reify-mcp/src/tools/chunks/units.md:56:When you have a geometric ratio and want an angle — … you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** This crossing is for an **arc-measure** ratio — `s / r`, a length over a length that *is* an angle in radians. …
crates/reify-mcp/src/tools/chunks/units.md:64:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:65:let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
examples/best_practices/INDEX.md:46:| `angle_crossings.ri` | An angle reading of an **arc-measure** ratio is an explicit crossing: `* 1rad` to enter Angle, `/ 1rad` to leave …
.claude/skills/reify-design/SKILL.md:88:- **Turning an arc-measure ratio into an angle — and why the `* 1rad`**:
.claude/skills/reify-design/SKILL.md:93:  ratio needs no crossing: `atan`/`atan2`/`asin`/`acos` and the `angle`/`angle_between_surfaces`
```

**HIT on all three surfaces.** The section heading is itself the goal
phrase, which is why it is phrased in intent terms rather than as
"Angle-crossing idiom". `units.md:9` is a spurious `ratio.*Angle` match on
"rational exponents … Angle"; it is kept because this block is real stdout,
not a curated one.

### Q2 — "why do I multiply by 1rad"

The keyword `1rad` hit from the first pass:

```
$ grep -rn "1rad" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** … Do not put `* 1rad` on a producer's result. On an *annotated* binding that is a hard error — `let bad : Angle = atan(o / a) * 1rad` declares `rad` but computes `rad^2`. Everywhere else the compiler stays quiet … (re-taken at the fifth pass; see below)
crates/reify-mcp/src/tools/chunks/units.md:64:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:65:let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
crates/reify-mcp/src/tools/chunks/units.md:66:let arc   : Length = r * theta / 1rad    // round-trips back to s       (0.005 m)
crates/reify-mcp/src/tools/chunks/units.md:69:let arc2  : Length = r * phi / 1rad      // arc length s = r*phi/eta    (0.00104719… m)
crates/reify-mcp/src/tools/chunks/units.md:77:Always the **no-space** literal: `1rad`. The spaced form `1 rad` is `Parse error: syntax error: rad`.
crates/reify-mcp/src/tools/chunks/units.md:94:**Angular frequency is a different crossing** — … `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above …
examples/best_practices/INDEX.md:46:| `angle_crossings.ri` | An angle reading of an **arc-measure** ratio is an explicit crossing: `* 1rad` to enter Angle, `/ 1rad` to leave …
.claude/skills/reify-design/SKILL.md:88:- **Turning an arc-measure ratio into an angle — and why the `* 1rad`**:
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` enters Angle, `theta / 1rad` leaves; arc length is
.claude/skills/reify-design/SKILL.md:90:  `r * theta / 1rad`. No-space literal only (`1 rad` is a parse error). Not optional:
.claude/skills/reify-design/SKILL.md:94:  queries return `Angle`, and annotated `let bad : Angle = atan(o/a) * 1rad` is a hard error
.claude/skills/reify-design/SKILL.md:95:  (declares `rad`, computes `rad^2`) — unannotated, or inside the call as `atan((o/a) * 1rad)`, …
.claude/skills/reify-design/SKILL.md:96:  it is silent instead. Both readings typecheck, so the wrong one is silent. `omega = 2*pi * f * 1rad` is a
```

(14 hits, all shown.) But the phrase as an author would *type* it — prose,
not a token — did not:

```
$ grep -rin "multiply by" $S          # FIRST PASS, at b0d2128279
(hits: 0)
```

**RED.** Fixed by rewording that section's lead sentence to use the verbs
(see below); at the re-walk:

```
$ grep -rin "multiply by" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
```

**HIT.**

### Q3 — "arc length from radius and angle"

```
$ grep -rin "arc length" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… or you have an angle and want a plain number, an arc length, or a rate …
crates/reify-mcp/src/tools/chunks/units.md:69:let arc2  : Length = r * phi / 1rad      // arc length s = r*phi/eta    (0.00104719… m)
crates/reify-mcp/src/tools/chunks/units.md:74:author usually wants: an arc length computed from an angle that was *not* derived
examples/best_practices/INDEX.md:46:… Arc length is `r * theta / 1rad`; `omega = 2*pi * f * 1rad` is the separate 2π rad/cycle class. …
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` enters Angle, `theta / 1rad` leaves; arc length is
```

**HIT**, and it now lands on the *forward* direction — an arc computed from
an independently-known angle — rather than only on the round-trip. The third
amendment pass moved it there; see below for the intermediate state in which
this query briefly fell out of the code block entirely.
The INDEX row states in the anti-pattern column that unannotated
`r * theta` silently yields `m·rad` rather than a Length, which is what an
author burned by it will recognise. The durable owner of that residual is
`docs/legibility/design-invariants.md` → "Crossing catalogue and
identities" (the `s = rθ/η` identity and its arc-length discharge note);
see the amendment passes below for why this paragraph no longer cites a
task id for it.

### Q4 — "Hz to rad/s" / "angular frequency"

The doctrine-flavoured spelling hit from the first pass:

```
$ grep -rin "angular frequency\|rad/cycle" $S
crates/reify-mcp/src/tools/chunks/units.md:94:**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. …
examples/best_practices/INDEX.md:46:… `omega = 2*pi * f * 1rad` is the separate 2π rad/cycle class. …
.claude/skills/reify-design/SKILL.md:96:  separate class (2π rad/cycle; no `cycle` unit). → `angle_crossings.ri`
```

But the way the question is actually asked — by unit name — did not:

```
$ grep -rn "rad/s" $S                 # FIRST PASS, at b0d2128279
(hits: 0)
$ grep -rn "Hz" $S                    # FIRST PASS, at b0d2128279
(hits: 0)
```

**RED, and the sharpest miss in the walk**: "Hz to rad/s" is the literal
form of the question, and neither unit name appeared anywhere on the three
surfaces. Fixed by naming both units in the paragraph's lead; at the
re-walk:

```
$ grep -rn "rad/s" $S
crates/reify-mcp/src/tools/chunks/units.md:94:**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above — see "The 2π rad/cycle distinction (D4)". …
$ grep -rn "Hz" $S
crates/reify-mcp/src/tools/chunks/units.md:94:[same line]
```

**HIT.**

### Q5 — "why is torque N·m/rad"

```
$ grep -rin "torque" $S
crates/reify-mcp/src/tools/chunks/types.md:18:    param max_torque : Torque
crates/reify-mcp/src/tools/chunks/units.md:52:Angle is the 8th base dimension (not dimensionless). Catches `torque + energy` as a type error. …
crates/reify-mcp/src/tools/chunks/units.md:96:**Why torque is N·m/rad**: the same crossing. Work is `tau * theta` and `theta` carries `rad`, so `tau` must carry `rad^-1` for the product to close on Energy — `INV-AD-1` in `docs/legibility/design-invariants.md` carries the argument. …
```

The *why* — the crossing rationale this leaf owns — **HITS** under
units.md → *Why torque is N·m/rad*. The *how to spell it* does not exist
yet:

```
$ grep -rn "Nm\b" $S
(hits: 0; grep exit status 1)
```

**PARTIAL HIT, recorded rather than closed.** The `Nm` literal spelling and
the angle-surface teaching are #5790 (ξ)'s slice under PRD D9 and are not
landed. Writing them here to turn this line green would be a scope breach
that also risks contradicting ξ whichever leaf lands first, so the gap is
recorded with its owner named instead. **Re-run this query after #5790
lands**; it should become a full hit without any change to γ's content.

## Wording changed because the walk failed it

Both changes are inside γ's own append-only `units.md` section — nothing at
or above the §*Angle as Base Dimension* section was touched, so #5790's
slice stays untouched (verified at the time of that pass: `git diff -U0`
reported only `@@ -56 +56 @@` and `@@ -79 +79 @@`).

1. **units.md → the §*Turning a Ratio into an Angle (and Back)* lead
   sentence** — it ended "…you write the crossing
   yourself." It now reads "…you write the crossing yourself: **multiply by
   `1rad`** to enter Angle, **divide by `1rad`** to leave it." Q2 drove
   this: the section taught the operation only as code, so an author asking
   in prose ("why do I multiply by 1rad") matched nothing.

2. **units.md → *Angular frequency is a different crossing*** — the paragraph
   opened straight into
   "…`omega = 2*pi * f * 1rad` carries 2π rad/cycle". It now names the units
   first: "— this is the one to reach for to get from a frequency in `Hz` to
   an angular velocity in `rad/s`." Q4 drove this: the paragraph was written
   in the doctrine's vocabulary (2π rad/cycle, `Frequency`,
   `AngularVelocity`) and never in the author's (`Hz`, `rad/s`).

Both failures share a root cause worth recording: **content written in the
doctrine's vocabulary is not discoverable by an author who only has the
goal.** The unit names and the plain verbs are what a goal phrase reduces
to, and neither survived the first drafting pass.

## Amendment pass — review-driven, walked at `6ac1bb7ad5`

A comprehensive review of the landed four-pack found that the walk above
had scored its own strongest result on a rule that is **narrower than the
section stated**. Q1 deliberately made the section heading the author's
goal phrase and scored it a HIT on all three surfaces — but "turn this
ratio into an angle" is also exactly what an author holding *opposite over
adjacent* greps, and the section taught `* 1rad` as if it were the only
answer. It is the answer for an **arc-measure** ratio only. Measured on the
built binary (`target/release/reify`, 2026-08-10 17:07):

```
$ ./target/release/reify eval /tmp/probe2_6181.ri     # o/a = 2.5 both ways
P2.theta      = 2.5 rad                  # (s / r) * 1rad   — arc measure
P2.theta_trig = 1.1902899496825317 rad   # atan(s / r)      — trigonometric
```

Both typecheck, so the type system gives **no** signal between them; the
discoverability win was therefore routing trig-ratio authors to the wrong
idiom. Putting the crossing on a producer *is* caught:

```
$ ./target/release/reify check /tmp/probe2_6181.ri    # let bad : Angle = atan(s / r) * 1rad
error: let binding 'bad' declared `Scalar[rad]` but its initializer evaluates to `Scalar[rad^2]`
EXIT=1
```

This also under-represented `INV-AD-1`, whose catalogue lists inverse trig /
`phase` / `arg` and the geometry `angle` / `angle_between_surfaces` queries
as *shipped* named Angle producers — `* 1rad` is one row of it, not the
whole.

### Re-walk of the affected queries

| # | Goal phrase | Surface expected | Verdict | Evidence (anchor) |
|---|---|---|---|---|
| 6 | "angle from opposite and adjacent" / "do I multiply an atan by 1rad" | the arc-measure vs trigonometric disambiguation | **HIT on all three surfaces** — SKILL.md only after the second amendment pass below | units.md → *Which ratio, though.*; INDEX.md → `angle_crossings.ri` row; SKILL.md → *Turning an arc-measure ratio into an angle* bullet (its lead and its trig-producer lines) |
| 1′ | "turn this ratio into an angle" (re-run) | heading still hits, now scoped by the paragraph under it | **HIT, no longer misleading** | units.md → §*Turning a Ratio into an Angle (and Back)*, then *Which ratio, though.* |
| 5′ | "why is torque N·m/rad" (re-run) | one-sentence crossing rationale, no task id | **PARTIAL, unchanged** — spelling still pending #5790 | units.md → *Why torque is N·m/rad* |

Re-run at `b8473a248d` (the second amendment pass below wrote the SKILL.md
rows; the third re-took the citations):

```
$ grep -rin "arc-measure" $S
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** This crossing is for an **arc-measure** ratio — `s / r`, a length over a length that *is* an angle in radians. …
examples/best_practices/INDEX.md:46:| `angle_crossings.ri` | An angle reading of an **arc-measure** ratio is an explicit crossing: `* 1rad` to enter Angle, `/ 1rad` to leave …
.claude/skills/reify-design/SKILL.md:88:- **Turning an arc-measure ratio into an angle — and why the `* 1rad`**:
.claude/skills/reify-design/SKILL.md:92:  `let arc = r * theta` silently yields `m·rad`. Arc-measure ratios only — a *trigonometric*
$ grep -rin "atan\|trigonometric" $S
crates/reify-mcp/src/tools/chunks/stdlib.md:23:`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2` (take `Angle`)
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** … A **trigonometric** ratio already has a named producer and needs no crossing: `atan`, `atan2`, `asin`, `acos` and the geometry `angle` / `angle_between_surfaces` queries all return `Angle` directly. …
examples/best_practices/INDEX.md:46:… A *trigonometric* ratio needs no crossing — `atan`/`atan2`/`asin`/`acos` and the geometry `angle` queries return Angle directly. …
.claude/skills/reify-design/SKILL.md:92:  `let arc = r * theta` silently yields `m·rad`. Arc-measure ratios only — a *trigonometric*
.claude/skills/reify-design/SKILL.md:93:  ratio needs no crossing: `atan`/`atan2`/`asin`/`acos` and the `angle`/`angle_between_surfaces`
.claude/skills/reify-design/SKILL.md:94:  queries return `Angle`, and `atan(o/a) * 1rad` is a hard error (declares `rad`, computes
```

The SKILL.md rows above exist only because the second amendment pass wrote
them (see below): at the first amendment pass this block claimed a SKILL.md
hit that was not in the tree.

The heading itself was deliberately **not** renamed: it is the Q1 hit, and
renaming it would have traded a correctness fix for a discoverability
regression. The disambiguation was placed immediately after the lead
sentence instead, so a trig author lands on the heading and is redirected
before reaching the code block.

### Other changes in this pass

1. **Stale task citations dropped.** The exemplar cited `#5825` for the
   arc-length residual and `units.md` cited `#5799` for the torque
   rationale. Both are `status: done`, so both cites were orphaned under
   CLAUDE.md's convention — and neither is even the right owner: #5825's
   own description says *"STILL OUT OF SCOPE: `arc = r * theta` evaluating
   to `m·rad` … Needs its own task — do NOT fold it in"*, and #5799's
   deliverable was re-dimensioning ROTATIONAL_STIFFNESS / ROTATIONAL_DAMPING
   to rad⁻², not the torque dimension. Both now point at
   `docs/legibility/design-invariants.md` (the "Crossing catalogue and
   identities" discharge note and `INV-AD-1` respectively), which is the
   durable owner. **No follow-up task was filed** for the arc-length
   residual: the doctrine states plainly that *"no enforcement is possible
   or chartered for the arc-length case itself — the teaching above is the
   whole deliverable"*, and this leaf is that teaching.
2. **The exemplar now pins the spelling the docs teach.** All three doc
   surfaces teach `omega = 2*pi * f * 1rad`, but the executable line read
   `2.0 * 3.14159265358979 * f * 1rad` — so the advertised "worked,
   compile-gated exemplar" did not actually compile-gate the taught form,
   and a best-practices file modelled a hand-expanded magic number. `pi` is
   a built-in (`crates/reify-compiler/src/constants.rs:16`); the exemplar
   now uses it and evaluates to `314.1592653589793 rad·s^-1`.
3. **The constraints now discriminate.** The five original guards
   (`r > 0mm`, `s > r`, `arc > 0mm`, `ratio > 1`, `f > 0Hz`) were all
   trivially satisfied, so a regression that preserved dimensions while
   moving magnitudes would have left the file green. They are now bands
   around the measured values plus `theta_trig < theta`. **Honest scope:**
   no automated corpus gate asserts constraint satisfaction —
   `examples_smoke.rs` compiles only, the two `reify-eval` sweeps assert
   zero stale-Undef and zero snapshot↔cache divergence, and
   `auto_type_param_determinism_tests.rs` discards its `CheckResult` and
   times the call (`let _ = check_source_with_stdlib(&src)`, :662). The
   bands are enforced by `reify check`, which is the per-file command
   `SKILL.md` already mandates before shipping — not by CI.
4. **One verbatim copy of the compiler diagnostics.** The measured
   diagnostic strings were transcribed in two ungated prose copies. The
   exemplar (the compile-gated artifact) is now marked the canonical copy
   and `units.md` paraphrases the error *shape*, following the
   `design-invariants.md` "Canonical copy: this section" precedent.

## Second amendment pass — review-driven, re-walked at `9d5d2ca6fd`

A review of the first amendment pass caught a false evidence line in this
transcript, and behind it a real gap in the landed content.

1. **Q6 was recorded as a HIT on a surface that had never been written.**
   The row above claimed the disambiguation landed on all three surfaces
   and quoted `.claude/skills/reify-design/SKILL.md:89:  only for an
   *arc-measure* ratio. …` — a line that did not exist anywhere in the
   tree. The disambiguation had in fact landed on `units.md:58`,
   `INDEX.md:46` and the exemplar only: measured at `5ba4379a18`,
   `grep -rin "arc-measure\|arc measure\|atan\|trigonometric"
   .claude/skills/reify-design/SKILL.md` returned zero hits. The fix was
   not to soften the row. SKILL.md is a design author's first-class
   discovery surface and both readings of a ratio typecheck, so an author
   who read only SKILL.md was being routed to `* 1rad` for an
   opposite-over-adjacent ratio with no signal from the type system. The
   missing content was written instead (step-5, commit `9d5d2ca6fd`: the
   bullet at `SKILL.md:88-96` now scopes the crossing to an arc-measure
   ratio, names the trigonometric producers, and records that
   `atan(o/a) * 1rad` is a hard error), the walk was re-executed, and the
   Q6 block above is that re-run's real stdout.

2. **Every line citation was re-derived, not recomputed.** The first
   amendment pass inserted `units.md:58` and shifted everything below it
   without refreshing the blocks measured at `b0d2128279` — `units.md`
   :59→:61, :61→:63, :64→:66, :79→:83, :81→:85 — and step-5 then replaced
   the SKILL.md rows (:88-93 → a :88-96 bullet). Each citation in both
   verdict tables and in every evidence block was re-taken from the re-run
   rather than from arithmetic, and the blocks that record a first-pass RED
   now carry the sha they were measured at so a reader can tell historical
   output from current.

3. **What this says about the walk as an instrument.** Two of the three
   surfaces carried the disambiguation and the transcript reported three,
   because that row was written from intent rather than from output — the
   same failure mode the walk exists to catch in the docs. An acceptance
   artifact that silently repairs its own false evidence is worth less than
   one that records the miss, so the miss is recorded here rather than
   edited away.

**Filed, not fixed:** the Q6 `atan` grep also surfaces
`crates/reify-mcp/src/tools/chunks/stdlib.md:23`, which lists `asin`,
`acos`, `atan`, `atan2` alongside `sin`/`cos`/`tan` under a single
"(take `Angle`)" annotation and never records that the inverse four
*return* `Angle` — the fact this whole leaf turns on. Measured at
`9d5d2ca6fd` on `target/release/reify`: `let ok : Angle = asin(0.5)` checks
green and evaluates to `0.5235987755982989 rad`, and `let bad = asin(1rad)`
also checks green, evaluating to `1.5707963267948966 rad` (= `asin(1.0)`,
the `rad` ignored rather than consumed). `stdlib.md` is outside this task's
five assigned files, so it is filed as follow-up ticket
`tkt_0RSAXZQ7NVJTVHFWPCZR1GAFSX` rather than edited here. Re-measured at
`b8473a248d`: still one line, still annotated only "(take `Angle`)".

## Third amendment pass — review-driven, re-walked at `b8473a248d`

A comprehensive review of the second amendment pass raised three points. All
three are addressed here; one of them turned up a fourth failure of the same
kind the walk exists to catch.

1. **The exemplar claimed a gate it does not have.** Its constraint comment
   read "…these fail `reify check` instead, so each identity above is pinned
   by a predicate and not only by a comment" — which reads as an active CI
   gate. It is not one. Constraint satisfaction is a check-time *result*, not
   an Error-severity compile diagnostic, so `examples_smoke.rs` (zero
   Error-severity diagnostics) never sees a violated band, and the two
   `reify-eval` sweeps assert stale-Undef and snapshot↔cache divergence, not
   constraint status. This note already disclosed that; the reviewer's point
   was that the *disclosure* lived in a notes file an author will not read
   while the *misleading claim* lived in the exemplar, which is the surface
   they do read. The exemplar's comment now carries the same scope. The
   durable fix — a corpus-wide constraint-satisfaction gate — needs a new
   test under `crates/reify-eval/tests/`, outside this task's five assigned
   files, and is filed as follow-up ticket `tkt_0RSC4D0TRN61DMWKB71V1NZ83J`.

2. **The chunk's code block was not reproducible on its own.** It annotated
   `(2.5 rad)`, `(2.5)` and `(0.005 m)` while never binding `s` or `r` —
   those values only resolved with the exemplar open alongside, which is
   exactly the reading posture an MCP chunk does *not* get. Chunks are
   fetched standalone; that is the premise this whole leaf rests on. The
   block now binds `s = 5mm` and `r = 2mm` first.

3. **The arc-length identity was only ever shown as a round trip.** With
   `theta = (s/r) * 1rad`, the line `arc = r * theta / 1rad` is `s` by
   construction — dimensionally instructive, but it never computes an arc
   from an *independently known* angle, which is the case an author reaching
   for `s = r·θ` actually has. It also made the `arc > 4.9mm` / `arc < 5.1mm`
   band largely a restatement of `s ≈ 5mm`. Both the chunk and the exemplar
   now also carry the forward direction: `phi = 30deg`,
   `arc2 = r * phi / 1rad`. Measured on `target/release/reify`:

   ```
   $ ./target/release/reify eval examples/best_practices/angle_crossings.ri
   …
   AngleCrossings.arc2 = 0.0010471975511965976 m     # = 2mm · π/6
   AngleCrossings.phi  = 0.5235987755982988 rad      # 30deg
   …                                                 # 10 bindings, EXIT=0
   $ ./target/release/reify check examples/best_practices/angle_crossings.ri
   …                                                 # OK ×12, elided
   All constraints satisfied.                        # EXIT=0
   ```

   The new band discriminates, which was the point of adding bands at all —
   with `phi` perturbed to `31deg` (a 3.3% drift that preserves every
   dimension):

   ```
   $ ./target/release/reify check /tmp/drift_6181.ri
   VIOLATED AngleCrossings#constraint[8]
   error: constraint AngleCrossings#constraint[8] violated
   EXIT=1
   ```

4. **Line-number citations replaced by anchors.** The verdict tables pinned
   roughly twenty-five `file:line` citations into three files this note does
   not own, two of which are live edit targets for pending sibling leaves.
   This note was its own best evidence of the cost: it already documents two
   rounds of exactly that rot inside this one task. The tables now cite the
   heading or bold lead phrase a hit lands under; raw `file:line` survives
   only inside the verbatim stdout blocks, where it is a measurement artifact
   already stamped with its sha. Headings survive insertions above them.

5. **A fourth wording fix the walk caught — and it was caused by (3).**
   Rewriting the `arc` comment for the round-trip/forward distinction moved
   Q3's "arc length" hit *out of the code block*: the re-walk found it
   matching only the section lead and the new prose paragraph, with no code
   line at all. That is the same failure mode as the two first-pass RED
   queries — content edited for correctness, discoverability silently lost —
   and it is the reason the walk is re-executed rather than reasoned about
   after every content change. The phrase now sits on the `arc2` line, so Q3
   lands on the forward direction, a strictly better hit than the round-trip
   line it had before.

## Fourth pass — review re-issued against the pre-amendment tree, checked at `5692dbd44e`

A comprehensive review arrived naming the same three points the third pass had
already closed; it was taken against `502fdbc24c`, before that pass landed.
Verified against the tree rather than re-applied:

| Review point | Where it landed | Verified at `5692dbd44e` |
|---|---|---|
| Exemplar claims a gate it does not have | `b8473a248d` | the *Bands, not sign guards* comment carries `HONEST SCOPE` |
| Chunk code block unreproducible / round-trip only | `b8473a248d` | the block binds `s = 5mm`, `r = 2mm`, and carries `phi`/`arc2` |
| Line-number citations rot | `5692dbd44e` | both verdict tables are anchor-only; zero `file:line` in any row |

The reviewer's preferred fix for the first point — a corpus-wide constraint gate
rather than a softened comment — is **#6215**, filed at the third pass as
`tkt_0RSC4D0TRN61DMWKB71V1NZ83J` and now task **#6215**, status `pending`
(re-measured via `get_task` at the fifth pass — it has not been started).

**One residual, closed here: the honest-scope claim has a named expiry, and one
of its three sites is unowned.** "No CI gate runs `reify check` over this
corpus" is true as measured at `5692dbd44e` —
`crates/reify-eval/tests/best_practices_constraint_gate.rs` is absent from
`main` — but #6215 is filed (`pending`) to add exactly that gate, and #6246 is already
filed to refresh this note and `INDEX.md` when it lands. #6246's file list names
those two only. The same claim also sits in the exemplar's *Bands, not sign
guards* comment (`examples/best_practices/angle_crossings.ri`), a third site
nothing was tracking; that comment now names #6215, so a reader who meets it
after the gate lands can tell it is stale. **Whoever executes #6246 should
refresh all three.**

The walk was **not** re-executed for this pass, and did not need to be: neither
edited file is in `$S` (the instrument greps `chunks/`, `INDEX.md` and
`SKILL.md` — the exemplar and this note are not walked surfaces), so no citation
above shifts.

## Fifth pass — enforcement claim was measurably false, re-measured and split

A review (esc-6181-3) found the one paragraph whose job is to separate what the
compiler catches from what it silently accepts making two claims that do not
hold. Every claim was re-measured against `./target/release/reify` in this
worktree before editing; all three reproduce exactly as the reviewer reported.

| Probe | Measured result | Old text said |
|---|---|---|
| `let bad : Angle = atan(s / r) * 1rad` | `error: let binding 'bad' declared \`Scalar[rad]\` but its initializer evaluates to \`Scalar[rad^2]\`; declared type and initializer type must agree` | hard error ✓ (the only true half) |
| `let unann = atan(s / r) * 1rad` (no annotation) | `check` green; `eval` → `1.1902899496825317 rad^2` | implied hard error ✗ |
| `let z : Angle = atan((s / r) * 1rad)` (argument side) | `check` green (`All constraints satisfied.`); `eval` → `1.1902899496825317 rad` — byte-identical to `atan(s / r)` | "or its argument … is a hard error" ✗ |

So the `rad` on the argument side is **ignored, not consumed**, and the result-side
error needs an *annotation* to fire. The old sentence grouped both under "is a hard
error" and then told the reader the atan cases were the caught ones ("otherwise
silent") — the exact inversion of the measurement, in a leaf whose stated premise is
enforcement honesty.

**Fixed in two files.** `units.md`'s *Which ratio, though.* paragraph now splits along
the measured boundary (annotated → error; unannotated and argument-side → silent).
`SKILL.md`'s index bullet carried the same unqualified claim and got the same
qualification. The exemplar (`angle_crossings.ri`) — the designated canonical copy —
was already correctly scoped: it says only "Putting the crossing on a producer's
result IS caught" and shows the annotated `let bad : Angle = …`, so it needed no edit
and the divergence was units.md/SKILL.md drifting from their own canonical source.

**Re-walked.** `grep -rn "1rad" $S` re-run after the edit: `units.md` line numbers are
unchanged (56/58/64/65/66/69/77/94), `SKILL.md`'s bullet grew one line (94–96 where it
was 94–95), and every verdict-table anchor still hits — the tables are anchor-only
since the third pass, which is why a one-line growth inside a bullet costs nothing.
The Q2 stdout block above was re-taken for the two changed lines; the historical
block inside "Amendment pass — walked at `6ac1bb7ad5`" was deliberately left alone,
since it is stamped at that sha and was accurate there.

**One more measured correction, same pass.** The same review also observed that this
note called #6215 "now in progress" (twice) and "in flight" (once). Re-measured via
`get_task(6215)`: its status is `pending` — filed, unstarted, `priority: low`. Those
three spots now say `pending`. It is a small thing, but a note whose whole subject is
docs-truth does not get to assert a task state it did not measure; the same
re-measure-before-asserting rule that produced the table above applies to its own
prose. (#6246 was re-measured too — `pending`, and "already filed" was accurate.)

## Sixth pass — #6215 landed; the honest-scope residual is now stale

#6215 landed on `main` at `ef2c7dd875` (merge of `task/6215`; the gate itself
was authored at `77e6670d02` and consolidated at `cd4a3b06be` into
`crates/reify-eval/tests/harness_corpus_gates.rs` as a `#[path]`-included
module — not the standalone `tests/best_practices_constraint_gate.rs` this
note and #6215's own filing described. The consolidation's own doc comment
explains why: the standalone form was flagged `reason=unregistered-standalone`
by `scripts/check-harness-baseline-registration.sh`). The real path is
`crates/reify-eval/tests/harness_corpus_gates/best_practices_constraint_gate.rs`.

The landed gate's measured baseline is 7 `.ri` files, 31 constraints, 28
Satisfied / 3 Indeterminate / 0 Violated — it does **not** match the 6
files, 19 constraints, 16 Satisfied figure #6215's architect recorded,
because that figure predates `angle_crossings.ri` (#6181) joining the
corpus. The divergence is not speculative: the gate's own doc comment,
still unedited at `best_practices_constraint_gate.rs:614-621`, says "This
is expected GREEN on the measured baseline (16 Satisfied / 3 Indeterminate
/ 0 Violated, with all 3 Indeterminate listed in `EXPECTED_INDETERMINATE`
above)" and, one sentence later, "in-flight task #6181 will add
`examples/best_practices/angle_crossings.ri`" — proof the figure was
recorded pre-#6181. `corpus_files()` is a directory walk, so it absorbed
the seventh file with no gate edit at all, which is exactly why the gate
stayed green straight through #6181 landing — that is the interesting
fact here, not a spurious "matches" (the stale doc comment itself is
tracked as follow-up task #6280, out of this docs-only step's scope).
With all 3 Indeterminate entries pinned in its
`EXPECTED_INDETERMINATE` allowlist — `clearance_oracle.ri` constraints [0] and
[1] (the `intersects`/`distance` geometry-consumer builtins, which need a
realized kernel and resolve only on the build()/tessellate() path, not the
pure value-eval surface this gate and `reify check` run on) and
`discrete_choice.ri` constraint [0] (the `auto(free)` solved-root sharp edge:
the root is only approximately ±1, and `==` is exact). Each entry is checked
bidirectionally — a new non-listed Indeterminate fails as lost coverage, a
listed entry that turns Satisfied fails as stale — which is why the gate's own
doc comment insists `EXPECTED_INDETERMINATE` is **not** a `SKIP_SET`: it
exempts no file from anything, it just asserts a different expected status.

This closes the expiry the fourth pass flagged: **"No CI gate runs `reify
check` over this corpus" is now false** — a gate does, covering constraint
satisfaction specifically, with the geometry-consumer residual above pinned
rather than silently dropped. `INDEX.md` is updated in the same commit as
this note (both are #6246's file list). The third site the fourth pass named —
the exemplar's own *Bands, not sign guards* comment in
`examples/best_practices/angle_crossings.ri` — remains out of #6246's declared
scope and is unrefreshed here; whoever next touches that file should update
it.

## Scope note

No automated test greps these docs for prose. The PRD records this
acceptance as `kind: manual`, and a committed prose-pinning test would be a
documentation meta-test — no runtime signal, and it would make the doc
expensive to edit. The executable pin for this leaf is
`examples/best_practices/angle_crossings.ri`, which rides the pre-existing
corpus gates (`examples_smoke.rs` compile + INDEX↔dir bidirectionality, and
the two 24-way `reify-eval` corpus sweeps) plus, since #6215 landed, a
constraint-satisfaction gate
(`crates/reify-eval/tests/harness_corpus_gates/best_practices_constraint_gate.rs`
— see the sixth pass above). Those first two gates assert compilation and
structure, **not** constraint satisfaction; the constraint gate is the one
that does, asserting the file's twelve bands are Satisfied (zero Violated,
zero unpinned Indeterminate) — no longer only the per-file `reify check` that
`SKILL.md` mandates before shipping, though that remains the author-facing
command for the same check. `angle_crossings.ri` carries no
`EXPECTED_INDETERMINATE` entry of its own, so all twelve of its constraints
are asserted `Satisfied` by the corpus gate.
