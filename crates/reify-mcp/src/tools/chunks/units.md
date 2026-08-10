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

## Angle as Base Dimension

Angle is the 8th base dimension (not dimensionless). Catches `torque + energy` as a type error. Trig functions are typed: `sin : Angle → Dimensionless`.

## Turning a Ratio into an Angle (and Back)

When you have a geometric ratio and want an angle — or you have an angle and want a plain number, an arc length, or a rate — you write the crossing yourself: **multiply by `1rad`** to enter Angle, **divide by `1rad`** to leave it. `rad` never appears out of a quotient on its own.

```
let theta : Angle  = (s / r) * 1rad      // ENTER: ratio -> Angle       (2.5 rad)
let ratio          = theta / 1rad        // LEAVE: Angle -> plain ratio (2.5)
let arc   : Length = r * theta / 1rad    // arc length s = r*theta/eta  (0.005 m)
```

Always the **no-space** literal: `1rad`. The spaced form `1 rad` is `Parse error: syntax error: rad`.

This is not a style preference — the crossing is what makes the binding compile. On an annotated `param`/`let` whose initializer is an *expression*, omitting it is a hard error:

```
let theta : Angle  = s / r      // error: declared `Scalar[rad]` but its initializer evaluates to `Real`
let arc   : Length = r * theta  // error: declared `Scalar[m]` but its initializer evaluates to `Scalar[m·rad]`
```

Drop the annotation and the error becomes silence instead: `let arc = r * theta` evaluates clean to `0.005 m·rad`, which is not a Length and will not compose with one.

Honest scope: this bites at annotated bindings over expressions, not universally. A bare *literal* still widens silently (`param theta : Angle = 2.5` evaluates to `2.5`, dimension erased; `sin(2.5)` is accepted). See "Enforcement honesty (D7)" in `docs/legibility/design-invariants.md`.

No operator over fields or tensors manufactures `rad` from a derivative — gradient, divergence, curl and laplacian stay pure quotient (`INV-AD-2 quotient-pure-derivative-algebra`). Every named site where `rad` legitimately enters is catalogued in `docs/legibility/design-invariants.md` under "Crossing catalogue and identities"; the governing rule is `INV-AD-1 angle-crossings-explicit`.

**Angular frequency is a different crossing** — this is the one to reach for to get from a frequency in `Hz` to an angular velocity in `rad/s`. `omega = 2*pi * f * 1rad` carries 2π rad/cycle, a distinct constant from the η = 1 rad above — see "The 2π rad/cycle distinction (D4)". There is no `cycle` unit to write; the typed layer forces the distinction, because `Frequency` and `AngularVelocity` are different types and neither silently stands in for the other.

**Why torque is N·m/rad** (#5799): the same crossing. Work is `tau * theta` and `theta` carries `rad`, so `tau` must carry `rad^-1` for the product to close on Energy — `INV-AD-1`.

Worked, compile-gated exemplar: `examples/best_practices/angle_crossings.ri`.
