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
minimize subject.cost where subject.cost != undef
```

Optimization directives can appear in purpose declarations or inline. `minimize`/`maximize` keywords.

## Quantifiers

```
forall x in collection: predicate(x)    // Universal
exists x in collection: predicate(x)    // Existential
```

Vacuous truth: `forall x in []: P(x)` evaluates to `true`.
Vacuous falsity: `exists x in []: P(x)` evaluates to `false`.

## Constraint Status

Constraints have a satisfaction status: `satisfied`, `violated`, or `indeterminate` (when inputs are `undef`).
