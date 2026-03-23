# Enum Types

v0.1 enums are C-style: simple named alternatives with no associated data.

## Declaration

```
enum Directionality { In, Out, Bidi }
enum FitType { Clearance, Transition, Interference }
enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
```

## Usage

Enum values are accessed with dot notation:
```
param fit_type : FitType = FitType.Clearance
param direction : Directionality
```

## Match Expressions

Pattern matching on enums with exhaustiveness checking:
```
let clearance = match fit_type {
    FitType.Clearance => 0.1mm
    FitType.Transition => 0.02mm
    FitType.Interference => -0.05mm
}
```

- Exhaustiveness enforced — must cover all variants or use `_` wildcard
- Multiple variants with `|`: `Socket | Button => recessed_drive`
- No fall-through
- When discriminant is `undef`, result is `undef`

## Option Type

`Option<T>` with `some(value)` / `none` is compiler-intrinsic, not an enum:
```
param coating : Option<CoatingSpec> = none

let total = match coating {
    some(c) => base + c.thickness
    none => base
}
```
