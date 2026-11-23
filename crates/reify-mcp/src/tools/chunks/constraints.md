# Constraints

Constraints are first-class entities in Reify: named, parameterized, composed, inherited, and collected into libraries.

## Inline Constraints

```reify-fragment
constraint thickness > 1mm
constraint head_diameter > shank_diameter
constraint forall f in faces: f.flatness < 0.01mm
constraint forall p in geometric_params: determined(p)
```

Anonymous predicates that must hold. Default connective between predicate lines is `and` (conjunction).

## Gating on Interference & Clearance
<!-- ORACLE-XREF -->

A constraint that gates on two parts not fouling, or on a minimum gap between them, must ASK THE
KERNEL — never hand-roll a bounding-box overlap test, and never hand-compute the gap from the
parameters that placed the parts.

Over let-bound geometry the low-ceremony pair is `intersects(a, b) -> Bool` (do they overlap?) and
`distance(a, b) -> Length` (the true minimum surface gap). Bind the operands AND the query to
`let`s first, then write the `constraint` against the bound names.

Both forms need a realized geometry kernel, and there are traps that make a wrong gate read as a
PASS rather than an error. Those, the posed and multi-body form, and a worked example are in the
`geometry` chunk — topic `geometry` of `reify_language_reference`. Read it before writing a
clearance gate.

## Constraint Definitions

```reify-fragment
constraint def MinWallThickness {
    param wall : Length
    param process : ManufacturingProcess

    wall >= process.min_wall_thickness
}

constraint def Coaxial {
    param a : CylindricalFeature
    param b : CylindricalFeature

    distance(a.axis, b.axis) == 0mm
    angle(a.axis.direction, b.axis.direction) == 0deg
}
```

Bare expressions in a constraint body are assertions (predicate lines).

## Optimization

```reify-fragment
minimize subject.mass
maximize subject.stiffness
```

Optimization directives can appear in purpose declarations or inline. `minimize`/`maximize` keywords.

**Objectives take no `where` guard.** Neither `minimize X where C` nor
`where C { minimize X }` is supported. The suffix form parses but the compiler
DISCARDS the guard silently: the objective runs as if unguarded, and every
command that resolves `auto` (`reify check`, `build`, `eval` and the rest; see
the `solving` chunk) reports success with a value that ignores the guard.
Measured: `minimize thickness where thickness > limit`, with `limit` 5mm and
`thickness` bracketed to [2mm, 6mm], resolves `thickness` to 4mm, and
`reify check` and `reify build` both exit 0. Express the predicate as a
separate `constraint` member instead:

```reify-fragment
constraint peak_stress < material.yield_stress
minimize mass
```

How objectives choose `auto` values, and what they do not reach yet: the `solving` chunk — topic `solving` of `reify_language_reference`.

## Quantifiers

```reify-schematic
forall x in collection: predicate(x)    // Universal; predicate is a metavariable — pdoccover:allow — grammar metavariable
exists x in collection: predicate(x)    // Existential; predicate is a metavariable — pdoccover:allow — grammar metavariable
```

Vacuous truth: `forall x in []: P(x)` evaluates to `true`.
Vacuous falsity: `exists x in []: P(x)` evaluates to `false`.

## Constraint Status

Constraints have a satisfaction status: `satisfied`, `violated`, or `indeterminate`.

Every indeterminate verdict carries its cause, such as `undefined inputs: Flange.radius`. A cause is EXPECTED when an input is deliberately unfinished, such as an unbound param. It is UNEXPECTED when a mechanism failed to deliver a value, such as a solve that failed. Plain `reify check` reports indeterminate constraints and exits 0 for them; it fails on errors, such as an infeasible solve. `reify check --strict` fails on any indeterminate constraint.

Letting the solver choose a value with `auto`, and which commands resolve it: the `solving` chunk — topic `solving` of `reify_language_reference`.
