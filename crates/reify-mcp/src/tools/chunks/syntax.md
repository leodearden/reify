# Reify Syntax Overview

## Lexical Structure

**Identifiers:**
- `snake_case` — values, parameters, ports, sub-structures, fields, modules
- `PascalCase` — types, traits, entity definitions
- `SCREAMING_SNAKE` — compile-time constants (convention)

**Comments:**
```
// Line comment
/* Block comment — nests correctly */
/// Doc comment — attached to next declaration
```

**Numeric literals:** `42`, `3.14`, `1.5e-3`, `0xFF`, `0b1010`, `1_000_000`

**Quantity literals** — number immediately followed by unit, no space:
```
5mm     3.2kN     45deg     293.15K
5kN*m   2.1kg/m^3   9.81m/s^2
```

**Range literals:** `2mm..5mm` (closed), `0deg..<360deg` (half-open), `>2mm`, `<=100MPa`

**String literals** — double-quoted; support interpolation holes and brace escapes:
```
"hello"               // plain string
"thickness is {t}"    // { expr } hole: evaluates expr, splices rendered text
"doubled is {2 * t}"  // holes accept full expressions, not just identifiers
"{{braces}}"          // {{ / }} collapse to literal { / } (no hole)
```
- Render rules: plain strings render bare (no quotes); dimensioned scalars render as `value unit` (`5mm` → `5 mm`); `undef` renders as the literal text `undef` and does not poison the string
- An empty hole `{}` is a parse error

**Special values:** `undef` (not yet decided), `auto` (solver decides), `some(v)`/`none` (Option)

## Declaration Shape

All entity declarations follow:
```
<entity_kind> def <Name><TypeParams>? <TraitList>? <WhereClause>? {
    <members>
}
```

Entity kinds: `structure`, `occurrence`, `constraint`, `field`

## Member Kinds

- `param` — value parameter (public interface)
- `port` — interaction point
- `sub` — contained sub-entity
- `let` — computed binding (private by default)
- `type` — type alias
- `constraint` — inline predicate

## Expressions

Arithmetic: `+`, `-`, `*`, `/`, `^`, `%` (with dimensional analysis)
Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`
Logical: `and`, `or`, `not`, `implies`
Conditional: `if cond then a else b`
Lambda: `|x| x * 2`
Match: `match expr { pattern => result, ... }`
