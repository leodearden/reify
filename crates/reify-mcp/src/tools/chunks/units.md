# Units and Dimensional Analysis

## Core Model

Dimensions are part of the type. Units are part of literal syntax. Two quantities with the same dimension and different units are the SAME type. The type checker operates on dimensions; unit conversion is automatic.

## Dimension Representation

A vector of rational exponents over 10 base dimensions (7 SI + Angle + SolidAngle + Money):
```
[Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle, SolidAngle, Money]

Length       = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
Force        = [1, 1, -2, 0, 0, 0, 0, 0, 0, 0]   // M*L*T^-2
Pressure     = [-1, 1, -2, 0, 0, 0, 0, 0, 0, 0]  // M*L^-1*T^-2
```

Multiplication adds exponent vectors. Division subtracts. Checked at compile time.

## Unit Declarations

```
unit mm : Length = 0.001m
unit USD : Money
unit degC : Temperature offset 273.15K
```

## Named Dimension Aliases

```
type Force    = Mass * Length / Time^2
type Pressure = Force / Length^2
type Density  = Mass / Length^3
```

35 standard named dimensions in `std.units.dimensions`.

## Temperature Handling

`degC` and `degF` are offset units:
```
param max_temp : Temperature = 150degC        // Absolute: 423.15 K
param delta_t  : TemperatureDiff = 20degC      // Difference: 20 K
```

- `Temperature + TemperatureDiff → Temperature` (valid)
- `Temperature - Temperature → TemperatureDiff` (valid)
- `Temperature + Temperature` → type error

## Dimensioned Geometry Arguments

Geometry constructors take **dimensioned** lengths. At a length-semantic argument position — a box's
width, a fillet radius, a `translate` component, a polygon vertex coordinate — a bare number is
**rejected** with a diagnostic. It is not read as metres, and it is not read as millimetres.

That rejection is the whole point. The alternative — silently reading a bare `20` as the SI base
unit — would turn `box(20, 20, 10)` into a **20-metre** part where the author meant `20mm`: a
**1000x scale error** that nothing downstream catches, because the model stays perfectly
self-consistent at the wrong size. A rejected literal is the only place that mistake is cheap.

**Bare `0` is not special-cased.** A zero length still has a dimension, so `box(0, 0, 0)` and
`translate(g, 0, 0, 5)` are rejected exactly like any other bare number. This is the one an author
expects to be exempt; it is not.

```reify
structure def Bracket {
    param plate_h : Length = 10mm

    // Every length-semantic argument carries a unit. A bare `20` is not
    // "20 by default" — it is rejected outright.
    let plate = box(20mm, 20mm, plate_h)

    // `mirror` carries both halves of the rule in one call: the PIVOT POINT
    // (ox, oy, oz) is length-semantic, so bare `0` is rejected there too —
    // while the AXIS DIRECTION (1, 0, 0) is a unit vector and stays bare.
    let mirrored = mirror(plate, 0mm, 0mm, 0mm, 1, 0, 0)

    // Dividing a length by a bare number preserves the length, so the
    // hand-centring idiom needs no extra unit on `-plate_h/2`.
    let centred = translate(plate, 0mm, 0mm, -plate_h/2)
}
```

**What it looks like when you get it wrong.** Each row pairs the rejected form with the migration
that replaces it. `g` stands for any let-bound geometry.

<!-- The ```reify-rejected info string is what scopes this block, matched BYTE-EXACTLY. It is
     deliberately NOT ```reify: the forms in the left column are meant to FAIL, and a ```reify
     fence is compiled with a zero-error assertion, so tagging this block ```reify would report
     "the documented migration does not compile" — the opposite of what is wrong.

     units_chunk_smoke.rs::documented_rejected_forms_are_actually_rejected scrapes these rows and
     runs the real compiler over BOTH columns: the left must produce an Error that names the
     offending argument and carries the migration hint, and the right must compile CLEAN. So a
     row whose migration stopped working is RED here, not stale advice.
     units_chunk_smoke.rs::bare_zero_is_not_special_cased additionally requires the D1 row below
     to still be present.

     SCOPED TO THE COMPILE LAYER. Every row here is rejected by the COMPILER, which is what that
     test can observe. Constructors rejected only at build/eval time are a separate block; do not
     move a row between the two without moving it in the tests as well.

     FORMAT IS LOAD-BEARING: one row per line, the two columns separated by `-->`, never wrapped.
     `//` annotations are stripped before scraping, so a row may carry one. The test binds
     `let g = box(10mm, 10mm, 10mm)` around each form, which is what makes the `g` rows real. -->

```reify-rejected
box(20, 20, 10)                    -->  box(20mm, 20mm, 10mm)
box(0, 0, 0)                       -->  box(0mm, 0mm, 0mm)                    // D1: no special case for zero
translate(g, 0, 0, 5)              -->  translate(g, 0mm, 0mm, 5mm)
fillet(box(10mm, 10mm, 10mm), 1)   -->  fillet(box(10mm, 10mm, 10mm), 1mm)
mirror(g, 0, 0, 0, 1, 0, 0)        -->  mirror(g, 0mm, 0mm, 0mm, 1, 0, 0)     // trailing axis stays bare
```

The `mirror` row is the one to read twice: the pivot `0, 0, 0` becomes `0mm, 0mm, 0mm` while the
axis direction `1, 0, 0` stays exactly as it was. One call, both halves of the rule.

Every rejection reads the same way — one diagnostic per offending argument, so a `box` with three
bare dimensions reports three:

```
box: width argument expects Length, got Int; pass a dimensioned length such as `5mm`
```

The `pass a dimensioned length such as `5mm`` tail is `reify-core::units::LENGTH_MIGRATION_HINT`,
rendered by both the compile-time and the run-time check, so one authoring mistake reads identically
whichever layer catches it.

**Which command catches it.** `reify eval` and `reify build` reject every form above — exit 1,
every time. `reify check` is **not** equivalent. It prints the same `error:` lines, but its EXIT
CODE depends on which layer owns the constructor's argument slots.

`box`, `translate`, `fillet`, `rotate_around` and the pivot triple of 7-argument `mirror` are
checked by the **compiler**, and `reify check` exits 1 on a bare argument. `helix`, `polygon`,
`arc`, `line_segment`, `interp`, `bezier` and `nurbs` have no compile-time length slot at all:
their bare arguments are caught only at build/eval time, so `reify check` prints the rejection,
adds `failed to compile geometry operation: missing or non-Length argument '<arg>' for <op>`, and
then **exits 0**.

<!-- These rows are the SAME rule as the block above; they differ only in which layer sees them.
     The ```reify-rejected-at-eval tag is matched BYTE-EXACTLY and carries the OPPOSITE
     compile-layer assertion to the ```reify-rejected block:
     units_chunk_smoke.rs::documented_eval_only_rejections_are_invisible_to_the_compile_layer
     requires each left column here to compile with ZERO errors. If a compile-layer slot is ever
     added for one of these, that test goes RED and the row must move to the other block in the
     same commit — so this note cannot outlive the gap it warns about. Same row format.

     THAT HAS ALREADY HAPPENED ONCE. `mirror` was a third row here until its 7-argument pivot
     triple gained compile-layer `ox`/`oy`/`oz` LENGTH slots; the test went red naming the row, and
     it now sits in the ```reify-rejected block above. Do not re-add it here. -->

```reify-rejected-at-eval
helix(10, 2, 50)             -->  helix(10mm, 2mm, 50mm)
polygon(0, 0, 10, 0, 5, 10)  -->  polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)
```

**So gate a design on `reify eval` or `reify build`, never on `reify check`'s exit status alone.**
The exit-code gap is a known residual, tracked in
`docs/prds/v0_6/check-diagnostic-truthfulness.md`.

<!-- SYNC: which claim in this section is pinned by an executable test, and which is prose — so the
     unpinned ones are visibly unpinned rather than looking equally guarded.

     FORMAT IS LOAD-BEARING. Every cite is written WHOLE on ONE line as `<path>::<fn_name>`, never
     wrapped and never tabulated into a two-column layout.
     units_chunk_smoke.rs::cited_test_paths_in_the_units_chunk_resolve resolves each one against
     the tree — the file must exist and must declare that fn — so a renamed or deleted test is RED
     there rather than silently turning a PINNED row into a false claim.

  bare dimensions rejected, with the migration hint — PINNED by
    units_chunk_smoke.rs::documented_rejected_forms_are_actually_rejected
    crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs::bare_box_dimensions_drop_the_op_with_a_coded_error
  bare `0` is not special-cased (D1) — PINNED by
    units_chunk_smoke.rs::bare_zero_is_not_special_cased
    crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs::bare_zero_box_dimensions_are_not_special_cased
  one authoring mistake reported by BOTH layers, with distinct codes — PINNED by
    crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs::bare_fillet_source_carries_both_layers_with_distinct_codes
  the eval-only constructors are invisible to the COMPILER — PINNED by
    units_chunk_smoke.rs::documented_eval_only_rejections_are_invisible_to_the_compile_layer
  `reify check` EXITS 1 on a bare `mirror` pivot — PINNED at the CLI seam by
    crates/reify-cli/tests/harness_cli/cli_check.rs::check_rejects_bare_scalar_mirror_origin_before_reaching_build
  the REST of `reify check`'s EXIT CODE (1 for box/translate/fillet, 0 for helix/polygon) —
    UNPINNED, prose only. Measured 2026-08-30 against a debug `reify` binary; the guards in this
    harness compile in-process and never run the CLI, so nothing here observes an exit status.
    Re-verify before relying on it. Residual tracked in
    docs/prds/v0_6/check-diagnostic-truthfulness.md.
  a unit in a DIMENSIONLESS slot is not reported — UNPINNED, prose only. Measured 2026-08-30:
    scale(g, 2mm) accepted, dimensioned axis direction in mirror and linear_pattern accepted.
    Re-verify before relying on it. -->

**What legitimately stays bare.** Not every geometry argument is a length. Unit-vector and
axis-direction components, repeat counts, NURBS weights, knot values, polynomial degrees, `scale`
factors and indices are dimensionless by construction, and putting a unit on one of them says
something the author does not mean.

That half is an **authoring rule you uphold, not one the compiler enforces** — the gate runs in one
direction only. Measured 2026-08-30: `scale(g, 2mm)` is accepted, as is a dimensioned axis direction
in `mirror` or `linear_pattern`. So a unit in a dimensionless slot will not be reported; write these
bare because it is what you mean, not because you will be told.

**Which argument of which constructor is length-semantic** is catalogued per position in the
`geometry` chunk — topic `geometry` of `reify_language_reference` — constructor by constructor,
under "Dimensioned arguments". This section states the rule and the idiom; that one enumerates the
positions — read it before dimensioning an unfamiliar signature.

Worked, compile-gated exemplar: `examples/best_practices/dimensioned_arguments.ri`.

## Angle as Base Dimension

Angle is the 8th base dimension (not dimensionless). Catches `torque + energy` as a type error. Trig functions are typed: `sin : Angle → Dimensionless`.

## Turning a Ratio into an Angle (and Back)

When you have a geometric ratio and want an angle — or you have an angle and want a plain number, an arc length, or a rate — you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. `rad` never appears out of a quotient on its own.

**Which ratio, though.** This crossing is for an **arc-measure** ratio — `s / r`, a length over a length that *is* an angle in radians. A **trigonometric** ratio already has a named producer and needs no crossing: `atan`, `atan2`, `asin`, `acos` and the geometry `angle` / `angle_between_surfaces` queries all return `Angle` directly. Do not put `* 1rad` on a producer's result. On an *annotated* binding that is a hard error — `let bad : Angle = atan(o / a) * 1rad` declares `rad` but computes `rad^2`. Everywhere else the compiler stays quiet: unannotated, `let unann = atan(o / a) * 1rad` checks green and evaluates to `1.19… rad^2`; and on the **argument** side `atan((o / a) * 1rad)` also checks green, returning the same `1.19… rad` as `atan(o / a)` — the `rad` ignored rather than consumed. Picking the wrong one of the two readings is silent as well: for `o / a = 2.5`, `atan(o / a)` is `1.19… rad` and `(o / a) * 1rad` is `2.5 rad`, and both typecheck. `* 1rad` is one row of the crossing catalogue, not the whole of it.

```
let s : Length     = 5mm                 // an arc measured along the rim
let r : Length     = 2mm                 // its radius

let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
let arc   : Length = r * theta / 1rad    // round-trips back to s       (0.005 m)

let phi   : Angle  = 30deg               // an angle known independently
let arc2  : Length = r * phi / 1rad      // arc length s = r*phi/eta    (0.00104719… m)
```

`arc` recovers the `s` it started from — `r * (s/r)` is `s` by construction, so
it demonstrates the algebra but not the arithmetic. `arc2` is the direction an
author usually wants: an arc length computed from an angle that was *not* derived
from a ratio.

Always the **no-space** literal: `1rad`. The spaced form `1 rad` is `Parse error: syntax error: rad`.

This is not a style preference — the crossing is what makes the binding compile. On an annotated `param`/`let` whose initializer is an *expression*, omitting it is a hard error:

```
let theta : Angle  = s / r      // error: declares rad, initializer is dimensionless
let arc   : Length = r * theta  // error: declares m, initializer is m·rad
```

The verbatim compiler wording for these is transcribed once, in the compile-gated exemplar `examples/best_practices/angle_crossings.ri` — treat that file as the canonical copy and this one as a paraphrase of the error *shape*.

Drop the annotation and the error becomes silence instead: `let arc = r * theta` evaluates clean to `0.005 m·rad`, which is not a Length and will not compose with one.

Honest scope: this bites at annotated bindings over expressions, not universally. A bare *literal* still widens silently (`param theta : Angle = 2.5` evaluates to `2.5`, dimension erased; `sin(2.5)` is accepted). See "Enforcement honesty (D7)" in `docs/legibility/design-invariants.md`.

No field or tensor operator manufactures `rad` from a derivative — gradient, divergence, curl and laplacian stay pure quotient (`INV-AD-2 quotient-pure-derivative-algebra`). The catalogue of sites where `rad` legitimately enters is in `docs/legibility/design-invariants.md` under "Crossing catalogue and identities"; the governing rule is `INV-AD-1 angle-crossings-explicit`.

**Angular frequency is a different crossing** — the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, not the η = 1 rad above, and there is no `cycle` unit to write. `Frequency` and `AngularVelocity` are distinct types, so neither silently stands in for the other. See "The 2π rad/cycle distinction (D4)" in `docs/legibility/design-invariants.md`.

**Torque is `N·m/rad`** by the same crossing: work is `tau * theta` and `theta` carries `rad`, so `tau` must carry `rad^-1` for the product to close on Energy — see "Angle-crossing family (INV-AD-1..4)" in `docs/legibility/design-invariants.md`. How to *spell* a torque literal is not taught here.

Worked, compile-gated exemplar: `examples/best_practices/angle_crossings.ri`.
