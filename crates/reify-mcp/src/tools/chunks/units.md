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
