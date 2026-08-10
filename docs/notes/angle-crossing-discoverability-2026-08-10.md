# Angle-crossing discoverability — acceptance walk

## Provenance

| Field | Value |
|---|---|
| PRD | `docs/prds/v0_6/angle-dimension-completion.md` (leaf γ, docs-truth four-pack) |
| Task | #6181 |
| Branch | `task/6181`, walked at `b0d2128279` (steps 1–3 landed) |
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

| # | Goal phrase | Surface expected | Verdict | Evidence |
|---|---|---|---|---|
| 1 | "turn this ratio into an angle" | units chunk section + INDEX row | **HIT** | `units.md:54`, `INDEX.md:46`, `SKILL.md:88` |
| 2 | "why do I multiply by 1rad" | same section / SKILL index line | **HIT** (after fix) | `units.md:56`, `units.md:64`, `SKILL.md:88-92` |
| 3 | "arc length from radius and angle" | `s = r·θ/η` teaching + exemplar | **HIT** | `units.md:56`, `units.md:61`, `INDEX.md:46` |
| 4 | "Hz to rad/s" / "angular frequency" | 2π rad/cycle paragraph | **HIT** (after fix) | `units.md:79`, `SKILL.md:93` |
| 5 | "why is torque N·m/rad" | one-sentence crossing rationale | **PARTIAL — rationale hits, spelling absent (pending #5790)** | `units.md:81` |

## Per-query evidence

### Q1 — "turn this ratio into an angle"

```
$ grep -rin "ratio into an angle\|ratio -> angle\|ratio.*Angle" $S
crates/reify-mcp/src/tools/chunks/units.md:54:## Turning a Ratio into an Angle (and Back)
crates/reify-mcp/src/tools/chunks/units.md:56:When you have a geometric ratio and want an angle — or you have an angle and want a plain number, an arc length, or a rate — you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. `rad` never appears out of a quotient on its own.
crates/reify-mcp/src/tools/chunks/units.md:59:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:60:let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
examples/best_practices/INDEX.md:46:| `angle_crossings.ri` | An angle reading of a geometric ratio is an explicit crossing: `* 1rad` to enter Angle, `/ 1rad` to leave …
.claude/skills/reify-design/SKILL.md:88:- **Turning a ratio into an angle — and why the `* 1rad`**: write the crossing explicitly.
```

**HIT on all three surfaces.** The section heading is itself the goal
phrase, which is why it is phrased in intent terms rather than as
"Angle-crossing idiom".

### Q2 — "why do I multiply by 1rad"

The keyword `1rad` hit from the first pass:

```
$ grep -rn "1rad" $S
crates/reify-mcp/src/tools/chunks/units.md:59:let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
crates/reify-mcp/src/tools/chunks/units.md:61:let arc   : Length = r * theta / 1rad    // arc length s = r*theta/eta  (0.005 m)
crates/reify-mcp/src/tools/chunks/units.md:64:Always the **no-space** literal: `1rad`. The spaced form `1 rad` is `Parse error: syntax error: rad`.
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` to enter Angle, `theta / 1rad` to leave; arc length is
[…8 more]
```

But the phrase as an author would *type* it — prose, not a token — did not:

```
$ grep -rin "multiply by" $S          # FIRST PASS
(hits: 0)
```

**RED.** Fixed by rewording `units.md:56` to use the verbs (see below), then:

```
$ grep -rin "multiply by" $S          # AFTER FIX
crates/reify-mcp/src/tools/chunks/units.md:56:… you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. …
```

**HIT.**

### Q3 — "arc length from radius and angle"

```
$ grep -rin "arc length" $S
crates/reify-mcp/src/tools/chunks/units.md:56:… or you have an angle and want a plain number, an arc length, or a rate …
crates/reify-mcp/src/tools/chunks/units.md:61:let arc   : Length = r * theta / 1rad    // arc length s = r*theta/eta  (0.005 m)
examples/best_practices/INDEX.md:46:… Arc length is `r * theta / 1rad` …
.claude/skills/reify-design/SKILL.md:89:  `let theta : Angle = (s/r) * 1rad` to enter Angle, `theta / 1rad` to leave; arc length is
```

**HIT**, and it lands on the identity itself plus the compiling exemplar.
This is the #5825 case: the INDEX row states in the anti-pattern column
that unannotated `r * theta` silently yields `m·rad` rather than a Length,
which is what an author burned by it will recognise.

### Q4 — "Hz to rad/s" / "angular frequency"

The doctrine-flavoured spelling hit from the first pass:

```
$ grep -rin "angular frequency\|rad/cycle" $S
crates/reify-mcp/src/tools/chunks/units.md:79:**Angular frequency is a different crossing** …
examples/best_practices/INDEX.md:46:… `omega = 2*pi * f * 1rad` is the separate 2π rad/cycle class. …
.claude/skills/reify-design/SKILL.md:93:  separate class (2π rad/cycle; there is no `cycle` unit). → `angle_crossings.ri`
```

But the way the question is actually asked — by unit name — did not:

```
$ grep -rn "rad/s" $S                 # FIRST PASS
(hits: 0)
$ grep -rn "Hz" $S                    # FIRST PASS
(hits: 0)
```

**RED, and the sharpest miss in the walk**: "Hz to rad/s" is the literal
form of the question, and neither unit name appeared anywhere on the three
surfaces. Fixed by naming both units in the paragraph's lead, then:

```
$ grep -rn "rad/s" $S                 # AFTER FIX
crates/reify-mcp/src/tools/chunks/units.md:79:**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above — see "The 2π rad/cycle distinction (D4)". There is no `cycle` unit to write; the typed layer forces the distinction, because `Frequency` and `AngularVelocity` are different types and neither silently stands in for the other.
$ grep -rn "Hz" $S                    # AFTER FIX
crates/reify-mcp/src/tools/chunks/units.md:79:[same line]
```

**HIT.**

### Q5 — "why is torque N·m/rad"

```
$ grep -rin "torque" $S
crates/reify-mcp/src/tools/chunks/types.md:18:    param max_torque : Torque
crates/reify-mcp/src/tools/chunks/units.md:52:Angle is the 8th base dimension (not dimensionless). Catches `torque + energy` as a type error. …
crates/reify-mcp/src/tools/chunks/units.md:81:**Why torque is N·m/rad** (#5799): the same crossing. Work is `tau * theta` and `theta` carries `rad`, so `tau` must carry `rad^-1` for the product to close on Energy — `INV-AD-1`.
```

The *why* — the crossing rationale this leaf owns — **HITS** at
`units.md:81`. The *how to spell it* does not exist yet:

```
$ grep -rn "Nm\b" $S
(hits: 0)
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
(verified: `git diff -U0` reports only `@@ -56 +56 @@` and `@@ -79 +79 @@`).

1. **`units.md:56`** — the lead sentence ended "…you write the crossing
   yourself." It now reads "…you write the crossing yourself: **multiply by
   `1rad`** to enter Angle, **divide by `1rad`** to leave it." Q2 drove
   this: the section taught the operation only as code, so an author asking
   in prose ("why do I multiply by 1rad") matched nothing.

2. **`units.md:79`** — the angular-frequency paragraph opened straight into
   "…`omega = 2*pi * f * 1rad` carries 2π rad/cycle". It now names the units
   first: "— this is the one to reach for to get from a frequency in `Hz` to
   an angular velocity in `rad/s`." Q4 drove this: the paragraph was written
   in the doctrine's vocabulary (2π rad/cycle, `Frequency`,
   `AngularVelocity`) and never in the author's (`Hz`, `rad/s`).

Both failures share a root cause worth recording: **content written in the
doctrine's vocabulary is not discoverable by an author who only has the
goal.** The unit names and the plain verbs are what a goal phrase reduces
to, and neither survived the first drafting pass.

## Scope note

No automated test greps these docs for prose. The PRD records this
acceptance as `kind: manual`, and a committed prose-pinning test would be a
documentation meta-test — no runtime signal, and it would make the doc
expensive to edit. The executable pin for this leaf is
`examples/best_practices/angle_crossings.ri`, which rides the pre-existing
corpus gates (`examples_smoke.rs` compile + INDEX↔dir bidirectionality, and
the two 24-way `reify-eval` corpus sweeps).
