# Angle-crossing discoverability — acceptance walk

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack) |
| Task | #6181 |
| Branch | `task/6181` — first walked at `b0d2128279` (steps 1–3 landed), amended at `6ac1bb7ad5`, re-walked at `9d5d2ca6fd` (step-5 landed) |
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
the queries re-run until they hit. Those changes are itemised under
"Wording changed because the walk failed it".

## Verdict table

Every citation below was re-taken from the re-walk at `9d5d2ca6fd`;
see "Second amendment pass" for why.

| # | Goal phrase | Surface expected | Verdict | Evidence |
|---|---|---|---|---|
| 1 | "turn this ratio into an angle" | units chunk section + INDEX row | **HIT** | `units.md:54`, `INDEX.md:46`, `SKILL.md:88` |
| 2 | "why do I multiply by 1rad" | same section / SKILL index line | **HIT** (after fix) | `units.md:56`, `units.md:66`, `SKILL.md:88-96` |
| 3 | "arc length from radius and angle" | `s = r·θ/η` teaching + exemplar | **HIT** | `units.md:56`, `units.md:63`, `INDEX.md:46` |
| 4 | "Hz to rad/s" / "angular frequency" | 2π rad/cycle paragraph | **HIT** (after fix) | `units.md:83`, `SKILL.md:96` |
| 5 | "why is torque N·m/rad" | one-sentence crossing rationale | **PARTIAL — rationale hits, spelling absent (pending #5790)** | `units.md:85` |

## Per-query evidence

**Reading the blocks below.** Each is real stdout from the re-walk at
`9d5d2ca6fd`; long matched lines are elided with `…`, and any omitted hit is
marked `[…N more]`. Blocks recording a first-pass RED are labelled
`# FIRST PASS` with the sha they were measured at and are historical;
everything else matches the tree as committed.

### Q1 — "turn this ratio into an angle"

```
$ grep -rin "ratio into an angle\|ratio -> angle\|ratio.*Angle" $S
crates/reify-mcp/src/tools/chunks/units.md:9:A vector of rational exponents over 10 base dimensions (7 SI + Angle + SolidAngle + Money):
crates/reify-mcp/src/tools/chunks/units.md:54:## Turning a Ratio into an Angle (and Back)
crates/reify-mcp/src/tools/chunks/units.md:56:When you have a geometric ratio and want an angle — … you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** This crossing is for an **arc-measure** ratio — `s / r`, a length over a length that *is* an angle in radians. …
crates/reify-mcp/src/tools/chunks/units.md:61:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:62:let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
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
crates/reify-mcp/src/tools/chunks/units.md:58:**Which ratio, though.** … Do not put `* 1rad` on a producer's result or its argument — `atan(o / a) * 1rad` is a hard error (it declares `rad` but computes `rad^2`). …
crates/reify-mcp/src/tools/chunks/units.md:61:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:62:let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
crates/reify-mcp/src/tools/chunks/units.md:63:let arc   : Length = r * theta / 1rad    // arc length s = r*theta/eta  (0.005 m)
crates/reify-mcp/src/tools/chunks/units.md:66:Always the **no-space** literal: `1rad`. The spaced form `1 rad` is `Parse error: syntax error: rad`.
crates/reify-mcp/src/tools/chunks/units.md:83:**Angular frequency is a different crossing** — … `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above …
examples/best_practices/INDEX.md:46:| `angle_crossings.ri` | An angle reading of an **arc-measure** ratio is an explicit crossing: `* 1rad` to enter Angle, `/ 1rad` to leave …
.claude/skills/reify-design/SKILL.md:88:- **Turning an arc-measure ratio into an angle — and why the `* 1rad`**:
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` enters Angle, `theta / 1rad` leaves; arc length is
.claude/skills/reify-design/SKILL.md:90:  `r * theta / 1rad`. No-space literal only (`1 rad` is a parse error). Not optional:
.claude/skills/reify-design/SKILL.md:94:  queries return `Angle`, and `atan(o/a) * 1rad` is a hard error (declares `rad`, computes
.claude/skills/reify-design/SKILL.md:95:  `rad^2`). Both readings typecheck, so the wrong one is silent. `omega = 2*pi * f * 1rad` is a
```

(13 hits, all shown.) But the phrase as an author would *type* it — prose,
not a token — did not:

```
$ grep -rin "multiply by" $S          # FIRST PASS, at b0d2128279
(hits: 0)
```

**RED.** Fixed by rewording `units.md:56` to use the verbs (see below); at
the re-walk:

```
$ grep -rin "multiply by" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
```

**HIT.**

### Q3 — "arc length from radius and angle"

```
$ grep -rin "arc length" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… or you have an angle and want a plain number, an arc length, or a rate …
crates/reify-mcp/src/tools/chunks/units.md:63:let arc   : Length = r * theta / 1rad    // arc length s = r*theta/eta  (0.005 m)
examples/best_practices/INDEX.md:46:… Arc length is `r * theta / 1rad`; `omega = 2*pi * f * 1rad` is the separate 2π rad/cycle class. …
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` enters Angle, `theta / 1rad` leaves; arc length is
```

**HIT**, and it lands on the identity itself plus the compiling exemplar.
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
crates/reify-mcp/src/tools/chunks/units.md:83:**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. …
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
crates/reify-mcp/src/tools/chunks/units.md:83:**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above — see "The 2π rad/cycle distinction (D4)". …
$ grep -rn "Hz" $S
crates/reify-mcp/src/tools/chunks/units.md:83:[same line]
```

**HIT.**

### Q5 — "why is torque N·m/rad"

```
$ grep -rin "torque" $S
crates/reify-mcp/src/tools/chunks/types.md:18:    param max_torque : Torque
crates/reify-mcp/src/tools/chunks/units.md:52:Angle is the 8th base dimension (not dimensionless). Catches `torque + energy` as a type error. …
crates/reify-mcp/src/tools/chunks/units.md:85:**Why torque is N·m/rad**: the same crossing. Work is `tau * theta` and `theta` carries `rad`, so `tau` must carry `rad^-1` for the product to close on Energy — `INV-AD-1` in `docs/legibility/design-invariants.md` carries the argument. …
```

The *why* — the crossing rationale this leaf owns — **HITS** at
`units.md:85`. The *how to spell it* does not exist yet:

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

Both changes are inside γ's own append-only `units.md` section — no line at
or before `units.md:52` was touched, so #5790's slice stays untouched
(verified at the time of that pass: `git diff -U0` reported only `@@ -56 +56 @@`
and `@@ -79 +79 @@`; the line numbers below are that pass's, with the
re-walk's in parentheses).

1. **`units.md:56`** (unmoved) — the lead sentence ended "…you write the crossing
   yourself." It now reads "…you write the crossing yourself: **multiply by
   `1rad`** to enter Angle, **divide by `1rad`** to leave it." Q2 drove
   this: the section taught the operation only as code, so an author asking
   in prose ("why do I multiply by 1rad") matched nothing.

2. **`units.md:79`** (now `units.md:83`) — the angular-frequency paragraph
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

| # | Goal phrase | Surface expected | Verdict | Evidence |
|---|---|---|---|---|
| 6 | "angle from opposite and adjacent" / "do I multiply an atan by 1rad" | the arc-measure vs trigonometric disambiguation | **HIT on all three surfaces** — SKILL.md only after the second amendment pass below | `units.md:58`, `INDEX.md:46`, `SKILL.md:88`, `SKILL.md:92-94` |
| 1′ | "turn this ratio into an angle" (re-run) | heading still hits, now scoped one line later | **HIT, no longer misleading** | `units.md:54` → `units.md:58` |
| 5′ | "why is torque N·m/rad" (re-run) | one-sentence crossing rationale, no task id | **PARTIAL, unchanged** — spelling still pending #5790 | `units.md:85` |

Re-run at `9d5d2ca6fd`, after the second amendment pass below wrote the
SKILL.md rows:

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
`tkt_0RSAXZQ7NVJTVHFWPCZR1GAFSX` rather than edited here.

## Scope note

No automated test greps these docs for prose. The PRD records this
acceptance as `kind: manual`, and a committed prose-pinning test would be a
documentation meta-test — no runtime signal, and it would make the doc
expensive to edit. The executable pin for this leaf is
`examples/best_practices/angle_crossings.ri`, which rides the pre-existing
corpus gates (`examples_smoke.rs` compile + INDEX↔dir bidirectionality, and
the two 24-way `reify-eval` corpus sweeps).
