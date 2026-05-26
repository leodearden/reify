# PRD (stub): `^` in Dimensional Type Expressions

**Milestone:** version-agnostic language foundation
**Status:** STUB — deferred. Authored 2026-05-26 alongside `docs/prds/unit-expressions.md`.
**Type:** grammar/parser extension. Sibling of the unit-expressions PRD; shares its
dimension-exponent algebra.

> This is a **stub**: it names the gap, the consumer, and the relationship to its
> sibling so the gap is tracked, not lost. It is **not yet decomposition-ready** — the
> open design questions in §5 must be resolved (a full `/prd` author pass) before it is
> queued.

---

## 1. Goal (user-observable)

A `.ri` author can write a power in a **dimensional type expression** — the RHS of a
type alias / dimension declaration / type-argument position:

```reify
type Area         = L^2          // instead of L * L
type Volume       = L^3
type Acceleration = L / T^2
type Inertia      = M * L^2      // second moment of mass
```

Today `dimensional_type_expr` (`tree-sitter-reify/grammar.js:707`) supports only `*` and
`/`; `L^2` does **not** parse and must be written `L * L`. The initial-design syntax
notes (`docs/initial-design/syntax-design-decisions.md:204`) specify "Dimension
expressions use `*`, `/`, `^` with the same precedence as unit expressions" — so `^`
here is specified but unimplemented, exactly parallel to the unit-literal gap.

## 2. Relationship to `unit-expressions.md`

| | unit-expressions (active) | this stub |
|---|---|---|
| Surface | `7850kg/m^3` (value position, quantity literal) | `type Area = L^2` (type position) |
| Grammar node | `quantity_literal` / `unit_expr` | `dimensional_type_expr` |
| Shared core | exponent-vector scaling on `DimensionVector` | same |

The two are independent grammar productions but want identical precedence (`^` tightest,
then `*`/`/` left-assoc) and the same underlying dimension algebra. Authoring this PRD
should reuse the unit-expressions resolver's exponent-scaling logic rather than
re-derive it.

## 3. Consumer (G1)

- **Direct user surface:** anyone declaring dimensional type aliases or named dimensions
  in `.ri` (and the stdlib `dimension`/`type` declarations themselves, e.g.
  `crates/reify-compiler/stdlib/units.ri` named dimensions).
- **Spec conformance:** closes the `^`-in-dimension-expr item from
  `docs/initial-design/syntax-design-decisions.md:204` and language-spec dimension
  algebra.

This is real but **lower-pressure** than the value-position gap — the `L * L` workaround
is far less ugly than the density `1kg/(1m*1m*1m)` workaround, which is why this is a
stub and the value-position work ships first.

## 4. Sketch (provisional)

- Add `^` to `dimensional_type_expr` in the grammar (prec tighter than `*`/`/`, integer
  exponent), regen parser, wire lowering, extend the dimensional-type evaluator to scale
  the dimension vector by the exponent.
- Likely 1–2 tasks (grammar+lowering, then the type-alias evaluation path) given the
  shared algebra already exists post-unit-expressions.

## 5. Open design questions (resolve before decomposing)

1. **Scope of "type position".** Does this cover only type-alias RHS, or also
   type-argument positions like `Scalar<L^2>`, `Tensor<2,3,M*L^2>`? The latter touches
   `type_arg_list` grammar, widening scope. **Must decide.**
2. **Signed exponents in type position.** `L^-1`? Or negatives only via `/`? (The
   value-position PRD allows signed; mirror it unless type position has a reason to
   differ.)
3. **Sequencing vs unit-expressions.** Hard dependency on the unit-expressions resolver
   landing (to reuse the exponent algebra), or independent? **Suggested:** soft —
   author after unit-expressions lands so the algebra is reusable.

## 6. Pre-conditions for activating

- `docs/prds/unit-expressions.md` landed (reuse its `DimensionVector` exponent-scaling
  helper; avoids two divergent implementations).
- A full `/prd` author pass resolving §5.

## 7. Out of scope

- Value-position `^` and unit-literal `^` — owned by `docs/prds/unit-expressions.md`.
