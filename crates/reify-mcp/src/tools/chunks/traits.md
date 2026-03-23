# Traits

Traits are non-entity declarations: no identity, no determinacy state. They are named, composable bundles of requirements.

## Syntax

```
pub trait Rigid : Physical {
    let moment_of_inertia = compute_moi(geometry, material.density)
}
```

## Trait Members

| Member kind | Description |
|------------|-------------|
| Parameters | Required named parameters with types |
| Ports | Required interaction points |
| Sub-structure slots | Required contained sub-structures satisfying a trait |
| Associated types | Type-level members that implementing types must bind |
| Constraints | Logical requirements on member relationships |
| `let` bindings | Computed values — both requirement and default, overridable |

Traits do NOT contain geometry, identity/state, or procedural code.

## Trait Composition

```
trait MechatronicActuator : MechanicalActuator + ElectricalDevice + Controllable
```

Conflict resolution:
- Same name, same type → merge silently
- Same name, different type → error
- Constraint composition → conjunction (all must hold)

## Defaults

```
trait StandardThread {
    param handedness : Handedness = Handedness.Right
}

trait Cylindrical {
    param diameter : Length
    param length : Length
    let volume = pi * (diameter/2)^2 * length
}
```

## Conformance

Nominal + structural hybrid. Explicit trait declaration (`: BoltShaped`) is primary. Conformance is interleaved with determinacy — a fully `undef` structure trivially conforms; full conformance only verifiable when all relevant parameters are determined.

## Overloading

Entity definition overloading is NOT supported. Function (`fn`) overloading by parameter types IS permitted since argument types are always statically known.
