# Reify Syntax Overview

## Lexical Structure

**Identifiers:**
- `snake_case` — values, parameters, ports, sub-structures, fields, modules
- `PascalCase` — types, traits, entity definitions
- `SCREAMING_SNAKE` — compile-time constants (convention)

**Comments:**
```reify-schematic
// Line comment
/* Block comment — nests correctly */
/// Doc comment — attached to next declaration
```

**Numeric literals:** `42`, `3.14`, `1.5e-3`, `0xFF`, `0b1010`, `1_000_000`

**Quantity literals** — number immediately followed by unit, no space:
```reify-schematic
5mm     3.2kN     45deg     293.15K
5kN*m   2.1kg/m^3   9.81m/s^2
```

**Range literals:** `2mm..5mm` (closed), `0deg..<360deg` (half-open), `>2mm`, `<=100MPa`

**String literals** — double-quoted; support interpolation holes and brace escapes:
```reify-schematic
"hello"               // plain string
"thickness is {t}"    // { expr } hole: evaluates expr, splices rendered text
"doubled is {2 * t}"  // holes accept full expressions, not just identifiers
"{{braces}}"          // {{ / }} collapse to literal { / } (no hole)
```
- Render rules: plain strings render bare (no quotes); dimensioned scalars render as `value unit` (`5mm` → `5 mm`); `undef` renders as the literal text `undef` and does not poison the string
- An empty hole `{}` is a parse error

**Special values:** `undef` (not yet decided), `auto` (solver decides), `some(v)`/`none` (Option)

## Module Declaration

A file may open with `module <path>`. Comments may precede it; it must come before any `import`.

`<path>` is not a free-form label. It must equal the path derived from the file's location:
- For the file you run (`reify eval`, `reify check`, `reify build`, the GUI), that is its stem: `bracket.ri` declares `module bracket`.
- For an imported file it is the dotted import path. `import parts.bolt` resolves, relative to the directory of the file you run, to `bolt.ri` in a `parts` directory (or to `mod.ri` in a `bolt` directory inside `parts`), and that file declares `module parts.bolt`.

A mismatch is the error `E_MODULE_PATH_MISMATCH`, which names the declaration to write; on the file you run, every CLI subcommand exits 1. Omitting the line is only a `W_MODULE_DECL_MISSING` warning.

Copying or renaming a `.ri` file changes its expected path. For a scratch probe, either update the `module` line or keep the basename and vary the directory.

Name files with `snake_case` identifiers. A stem like `my-part.ri` or `v1.2.ri` can never be matched, because `module my-part` and `module v1.2` are parse errors.

## Declaration Shape

All entity declarations follow:
```ebnf
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
