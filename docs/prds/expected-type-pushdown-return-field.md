# Expected-type push-down — return-type & struct-field-init positions (STUB)

**Status:** stub / blocked-on-consumer — **not yet designed**. Full `/prd` author pass is triggered after `expected-type-pushdown.md`'s integration gate (task ε) lands.
**Milestone:** version-agnostic compiler-typing foundation (root `docs/prds/`).
**Date:** 2026-06-19.

---

## Why this is a stub

`expected-type-pushdown.md` builds the contextual expected-type channel and wires it into the **let-binding** and **function-argument** positions (its first increment). Leo's scoping decision (2026-06-19): ship let + arg first, then design the remaining bidirectional-typing positions once that channel is proven in-tree — rather than over-scoping the first increment.

This stub is the **named consumer** for "how far bidirectional typing extends" (the parent PRD's G1 generalization). It exists so the remaining positions are tracked, not forgotten, and so the trigger to design them is explicit.

## What it will cover (the remaining positions)

Both reproduce on `main` (2026-06-19) as warning + silent default — the same family the parent PRD fixes for let/arg:

1. **Return position.** `fn mk() -> List<Length> { [] }` — the empty body literal should resolve to `List<Length>` from the declared return type (today: warns, defaults to `List<Real>`). Push-down pushes the return annotation into the body's tail expression.
2. **Struct-field-init position.** A structure field with a declared collection type initialized with an empty literal (e.g. `let f : List<Length> = []` is the let form; the field-init form is the value supplied at sub-construction / field default) should resolve from the field's declared type.

Both reuse the **same expected-type channel** (`expected_type: Option<&Type>`) the parent PRD produces — this stub extends the *consumer* sites, not the mechanism. The kind-mismatch arm (`CollectionLiteralKindMismatch`) and the unbound-generic arm (`TypeUndetermined`) carry over unchanged.

## Pre-conditions for activating

- `expected-type-pushdown.md` task **ε** (integration gate) is **done** (the channel + let + arg positions landed and verified).
- Re-run `/prd` author on this file with that channel in-tree; verify the return/field substrate (how the return tail expression and field-init values reach `compile_expr`, and whether return-type enforcement has the same decorative-annotation gap lets had) against main at that time.

## Out of scope (inherited)

- The mechanism itself (owned by the parent PRD).
- General annotation-vs-initializer enforcement for non-collection-literal RHS (the parent PRD's filed follow-up task).
- `Type::Unknown` / bottom-up first-use unification (parent decision 2).
- Runtime `Value::infer_type` defaults (off the compiler path).

## Decomposition plan

Deferred — produced by the triggered `/prd` author pass. (Likely shape: extend the channel to the return tail-expression and field-init sites, with positive/negative/non-regression boundary tests mirroring the parent's §7.)
