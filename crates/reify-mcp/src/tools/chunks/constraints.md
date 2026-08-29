# Constraints

Constraints are first-class entities in Reify: named, parameterized, composed, inherited, and collected into libraries.

## Inline Constraints

```
constraint thickness > 1mm
constraint head_diameter > shank_diameter
constraint forall f in faces: f.flatness < 0.01mm
constraint forall p in geometric_params: determined(p)
```

Anonymous predicates that must hold. Default connective between predicate lines is `and` (conjunction).

## Constraint Definitions

```
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

```
minimize subject.mass
maximize subject.stiffness
```

Optimization directives can appear in purpose declarations or inline. `minimize`/`maximize` keywords.

**Objectives take no `where` guard.** Neither `minimize X where C` nor
`where C { minimize X }` is supported. The suffix form parses but the compiler
DISCARDS the guard silently — the objective then runs unopposed and drives your
`auto` params to their bounds while the build reports success. Express the
predicate as a separate `constraint` member instead:

```
constraint peak_stress < material.yield_stress
minimize mass
```

## Quantifiers

```
forall x in collection: predicate(x)    // Universal; predicate is a metavariable — pdoccover:allow — grammar metavariable
exists x in collection: predicate(x)    // Existential; predicate is a metavariable — pdoccover:allow — grammar metavariable
```

Vacuous truth: `forall x in []: P(x)` evaluates to `true`.
Vacuous falsity: `exists x in []: P(x)` evaluates to `false`.

## Constraint Status

Constraints have a satisfaction status: `satisfied`, `violated`, or `indeterminate` (when inputs are `undef`).
