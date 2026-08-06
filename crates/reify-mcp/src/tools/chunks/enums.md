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

Generic payloads — an `enum` may take type parameters, and payload field types may
reference them. Type arguments are inferred from the payload at construction:
```
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}

param r : Result<Length, String> = Ok { value: 5mm }
```
`Ok { value: 5mm }` infers `T = Length`. Recursive generic ADTs are legal too —
`enum Tree<T> { Leaf { value: T }, Node { left: Tree<T>, right: Tree<T> } }`.
`Result<T, E>` is a prelude enum (`crates/reify-compiler/stdlib/result.ri`); see
`docs/prds/v0_6/generic-data-carrying-enums.md` and the runnable example
`examples/m6_generic_enum.ri`.

## Usage

Enum values are accessed with dot notation:
```
param fit_type : FitType = FitType.Clearance
param direction : Directionality
```

## Match Expressions

Pattern matching on enums with exhaustiveness checking. Patterns name the variant
**unqualified** — `FitType.Clearance` is how a *value* is written (see Usage above), but
`FitType.Clearance =>` as a *pattern* is a parse error:
```
let clearance = match fit_type {
    Clearance => 0.1mm,
    Transition => 0.02mm,
    Interference => -0.05mm
}
```

- Patterns are unqualified variant names; arms are comma-separated
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

Payloads shipped in v0.6; these bounds apply to the current surface.

- **Construction payload values must be compile-time literals.** A param reference or other
  non-literal value in a payload field is a hard error:
  `non-constant payload value for field 'radius' of variant 'Circle' is not yet supported`.
- **Payload fields are readable only through a `match` binder.** Dot access does not reach
  into a payload — `outline.width` reports `member access not yet supported: .width`.
- **Payload construction names the variant unqualified.** Type qualification
  (`FitType.Clearance`) is the form for a bare-variant *value* only; a payload construction
  may not be qualified — `Shape.Circle { radius: 2mm }` fails to parse
  (`Parse error: invalid param: ...`). Write `Circle { radius: 2mm }`. Match *patterns* are
  likewise always unqualified (see Match Expressions above).
- **Empty-brace construction does not parse.** `Point {}` reports
  `Parse error: syntax error: {}` — write the bare variant as `Point`. This is a
  grammar-level restriction rather than a PRD-deferred item.
- **Not supported:** positional/tuple payloads (`ScalarForce(Real)` — named-field is the
  sole form), partial binding (`Rect { width: w, .. }`), nested destructuring within one
  pattern (`Rect { width: Circle { ... } }`), payload-value guards, and pipe-alternation
  across payload-binding arms. Authority for these five:
  `docs/prds/v0_6/data-carrying-enums.md` §10. That §10 also defers generic payloads —
  superseded: generics shipped in v0.6 (see Declaration above).

## Option Type

`Option<T>` with `some(value)` / `none` is compiler-intrinsic, not an enum — it has
no `enum` declaration and no variants you can match on. An `Option` is read with the
**recovery combinators**, which are prelude-registered (no import needed):
```
param base : Length = 1mm
param coating : Option<Length> = some(0.25mm)

let bare  = unwrap_or(coating, 0mm)
let has   = is_some(coating)
let alt   = or_else(coating, some(0.05mm))
let total = map_or(coating, base, |c: Length| base + c)
```
That evaluates to `bare = 0.25mm`, `has = true`, `alt = some(0.25mm)`,
`total = 1.25mm`. With `coating = none` the same source gives `bare = 0mm`,
`has = false`, `alt = some(0.05mm)`, `total = 1mm`.

Recovery is driven by the **subject** — the first argument:

| Combinator | subject `some(x)` | subject `none` |
|---|---|---|
| `unwrap_or(o, dflt)` | `x` | `dflt` |
| `or_default(o, dflt)`, `fallback(o, dflt)` | `x` | `dflt` (aliases of `unwrap_or`) |
| `or_else(o, alt)` | `o`, intact | `alt` |
| `is_some(o)` / `is_none(o)` | `true` / `false` | `false` / `true` |
| `map_or(o, dflt, \|x: T\| ...)` | `f(x)` | `dflt` |
| `ok_or(o, err)` | `Ok { value: x }` | `Err { error: err }` |

`map_or` is the **only** combinator that binds the payload to a name, so it is what
replaces a `some(c) => ...` match arm. `ok_or` bridges an `Option` into a `Result`
you *can* match on. An `undef` subject propagates `undef` through every combinator
(Kleene three-valued).

Declared in `crates/reify-compiler/stdlib/option_recovery.ri` (`ok_or` in
`crates/reify-compiler/stdlib/result.ri`); runnable examples
`examples/m6_fallback_recovery.ri` and `examples/option_map_or.ri`.
