# Enum Types

Enums are tagged-union types. Each variant is either **bare** (no payload) or carries a **named-field payload** enclosed in braces. Payloads are named-field only — no positional or tuple forms.

## Declaration

Bare variants — simple named alternatives with no payload:
```
enum Directionality { In, Out, Bidi }
enum FitType { Clearance, Transition, Interference }
enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
```

Named-field payload variants — bare and payload-carrying variants may be mixed in one
declaration:
```
enum Shape {
    Circle { radius: Length },
    Rect { width: Length, height: Length },
    Point,
}
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

A payload-carrying variant is constructed in brace form, naming **all** of its declared
fields. A `match` arm binds the payload by naming each field: the binder introduces that
name as a local in the arm's body, so `Circle { radius: r }` makes `r` available as the
radius value.
```
param outline : Shape = Rect { width: 20mm, height: 10mm }

let area = match outline {
    Circle { radius: r } => 3.14159 * r * r,
    Rect { width: w, height: h } => w * h,
    Point => 0mm * 0mm
}
```

## Payload limits

Payloads shipped in v0.6; three bounds apply to the v1 surface.

- **Construction payload values must be compile-time literals.** A param reference or other
  non-literal value in a payload field is a hard error:
  `non-constant payload value for field 'radius' of variant 'Circle' is not yet supported`.
- **Payload fields are readable only through a `match` binder.** Dot access does not reach
  into a payload — `outline.width` reports `member access not yet supported: .width`.
- **Not supported in v1:** positional/tuple payloads (`ScalarForce(Real)` — named-field is
  the sole form), empty-brace construction (`Point {}`), partial binding
  (`Rect { width: w, .. }`), nested patterns, and pipe-alternation across payload-binding
  arms. Authority: `docs/prds/v0_6/data-carrying-enums.md` §10.

## Option Type

`Option<T>` with `some(value)` / `none` is compiler-intrinsic, not an enum:
```
param coating : Option<CoatingSpec> = none

let total = match coating {
    some(c) => base + c.thickness
    none => base
}
```
