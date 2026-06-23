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
| Associated functions | Default-providing or required `fn` members; may take `self` or be trait-static |

Traits do NOT contain geometry or identity/state.

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

## Associated Functions

Traits may declare associated `fn` members — instance functions (taking `self`) or trait-static functions (no receiver).

**Default-providing instance function** — provides a default body; conformers inherit it and may override it:

```reify
trait Cylindrical {
    param diameter : Length
    param length : Length
    fn lateral_area(self) -> Scalar<Area> { pi * diameter * length }
}
```

Inside a trait instance `fn`, bare member names (`diameter`, `length`) are sugar for `self.diameter` / `self.length`.

**Required (bodyless) function** — no body; every conformer must supply a matching `fn`, or a conformance error is raised:

```reify
fn loss_factor(self) -> Real
```

**Trait-static function** — no `self` receiver; called directly on the trait name:

```reify
trait Defaultable {
    fn make_default() -> Length { 10mm }
    fn scaled(factor : Real) -> Length { 10mm * factor }
}
```

### Calling associated functions

**Instance dispatch** — `obj.(Trait::fn)(args)`: resolves to the conformer's associated function (trait default or per-conformer override).

```reify
let wetted = pin.(Cylindrical::lateral_area)()
```

**Static dispatch** — `Trait::fn(args)`: calls a trait-static function directly; no receiver or conformance relationship required.

```reify
let gap : Length = Defaultable::make_default()
let wide : Length = Defaultable::scaled(3.0)
```

## Conformance

Nominal + structural hybrid. Explicit trait declaration (`: BoltShaped`) is primary. Conformance is interleaved with determinacy — a fully `undef` structure trivially conforms; full conformance only verifiable when all relevant parameters are determined.

## Overloading

Entity definition overloading is NOT supported. Function (`fn`) overloading by parameter types IS permitted since argument types are always statically known.
