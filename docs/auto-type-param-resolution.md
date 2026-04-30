# How `auto` Type-Parameter Resolution Works

**Applies to:** Reify v0.1
**Status:** Implemented
**Audience:** Language users and compiler contributors who need to understand how `Bearing<auto: Seal>` is resolved.
**Not a PRD:** For the design rationale and acceptance criteria see `docs/prds/auto-type-param-resolution.md`.

---

## Algorithm at a glance

`auto` in a type-parameter position (e.g. `Bearing<auto: Seal>`) is resolved at
elaboration time, before value-parameter resolution begins. The algorithm is a
**per-parameter BFS** over the candidate types that are in scope at the use site:

1. **Phase A — Enumerate candidates.** Walk the in-scope name table and collect
   every concrete type whose trait/kind bounds satisfy the required bound (using
   the trait-conformance predicate). Trait bounds (`T: Seal`), kind bounds
   (`N: Nat`), and composite bounds (`T: TraitA + TraitB`) are all supported;
   composite bounds use set intersection of each sub-bound's candidate set.

2. **Phase B — Feasibility filter.** For each candidate (in alphabetical order),
   instantiate the parameterized definition and run the value-`auto` solver's
   constraint-feasibility check on the resulting scope's top-level constraints.
   A candidate is accepted if no constraint evaluates to `Violated`. Constraints
   that depend on as-yet-`undef` parameters produce `Indeterminate`, which counts
   as feasible (monotonicity guarantee: adding more information can only flip
   `Indeterminate → Satisfied/Violated`, never the reverse).

3. **Phase C — Select.** Dispatch based on the number of feasible candidates and
   whether the `auto` is strict or free:

   | Feasible | Strict `auto` (default)       | `auto(free)`                              |
   |----------|-------------------------------|-------------------------------------------|
   | 0        | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error | `E_AUTO_TYPE_PARAM_NO_CANDIDATE` error |
   | 1        | Use it. No diagnostic.        | Use it. No diagnostic.                    |
   | ≥ 2      | `E_AUTO_TYPE_PARAM_AMBIGUOUS` error | `W_AUTO_TYPE_PARAM_NON_UNIQUE` warning; pick lexicographic-first by FQN |

Resolution is then wired as a topology-change source (architecture §6.2 row 5): the
resolved concrete type substitutes into the schema, which re-elaborates downstream
nodes.

---

## The cap of 10

**Phase A is capped at 10 candidates per parameter.** If more than 10 concrete
types satisfying the bound are in scope, the compiler errors with
`E_AUTO_TYPE_PARAM_POOL_OVERFLOW`, listing the alphabetically-first 10 and
pointing at the bound. The user must disambiguate by importing only the desired
candidates or by writing the type explicitly.

Why 10? It is an architectural-decision default chosen to bound per-parameter BFS
cost at a known constant (≤ 10 feasibility checks per parameter). The cap is the
canonical value; do not change it without explicit signoff and a PRD update.

---

## Lexicographic tiebreak by fully qualified name (FQN)

Where the algorithm must produce a deterministic ordering without a unique winner,
it uses **lexicographic order by fully qualified name** in two places:

1. **Candidate ordering in Phase A.** The enumerated pool is sorted alphabetically
   by FQN before Phase B begins. This means Phase B always visits `pkg::AAA` before
   `pkg::BBB`, making rejection-reason diagnostics reproducible.

2. **`auto(free)` selection in Phase C.** When ≥ 2 feasible candidates remain under
   `auto(free)`, the lexicographically-first FQN is chosen. Because Phase A's sort
   is already by FQN, this is simply `accepted[0]`.

The result: the same source file produces the same resolution on every run and every
machine.

---

## Multiple `auto:` parameters — declared order

When a definition has more than one `auto:` type parameter (e.g.
`Coupling<auto: A, auto: B>`), each parameter resolves in **declared order**:

- Parameter `A` resolves first (Phases A → B → C).
- Parameter `B`'s candidate enumeration (Phase A) runs after `A` is already resolved
  and substituted, so `B`'s feasibility filter sees the concrete `A`.

**Halt-on-first-failure.** If any parameter fails (overflow, no candidate, or
ambiguity), the orchestrator records that parameter's outcome and stops immediately.
No later parameters are enumerated or selected. This is the v0.1 "no cross-parameter
backtracking" rule; see [Deferred to v0.2](#deferred-to-v02-cross-parameter-backtracking) below.

**Per-parameter `free` flag.** Each `auto` parameter carries its own `free` flag.
A strict param and a free param in the same definition may produce different Phase C
arms (error vs. warning + selected) independently.

---

## Diagnostic codes

| Code | Kind | Meaning |
|------|------|---------|
| `E_AUTO_TYPE_PARAM_POOL_OVERFLOW` | error | More than 10 in-scope types satisfy the bound; lists alphabetically-first 10. |
| `E_AUTO_TYPE_PARAM_NO_CANDIDATE` | error | Zero feasible candidates; lists each rejected candidate with the constraint that ruled it out. |
| `E_AUTO_TYPE_PARAM_AMBIGUOUS` | error | ≥ 2 feasible candidates under strict `auto`; suggests writing the type explicitly (e.g. `Bearing<ORingSeal>`). |
| `W_AUTO_TYPE_PARAM_NON_UNIQUE` | warning | ≥ 2 feasible candidates under `auto(free)`; reports which FQN was chosen and the alternatives. |

All diagnostics include the bound (`T: Seal`), the use-site span, and the full
candidate list considered (alphabetical, capped at 10).

---

## Worked example

```reify
trait Seal {}
structure ORingSeal : Seal { ... }

structure Bearing<T: Seal> {
    bore_diameter : Length
}

sub bearing1 : Bearing<auto: Seal> { bore_diameter = 25mm }
```

**Phase A:** one in-scope `Seal` implementor: `ORingSeal`.
**Phase B:** instantiate `Bearing<ORingSeal>`, check constraints — all feasible.
**Phase C:** exactly 1 feasible → select `ORingSeal`. No diagnostic.

Result: `bearing1` is resolved as `Bearing<ORingSeal>` with `bore_diameter = 25mm`.

---

## Deferred to v0.2: cross-parameter backtracking

v0.1's per-parameter BFS is **incomplete** for definitions with multiple coupled
`auto:` parameters: if parameter `A`'s locally-feasible choice rules out every
candidate for parameter `B`, the algorithm fails even though some other choice for
`A` would have left a feasible `B`.

Full combinatorial search — depth-first over the candidate cross-product with
backtracking — is **deferred to v0.2**. See
`docs/prds/v0_2/auto-resolution-backtracking.md` for the design (depth bound of 6
parameters, cross-product hard cap of 100 000 assignments, backjumping via
existing "rejected because" reasons, deterministic lexicographic-first selection).

The v0.1 failure mode is graceful: the diagnostic lists candidates and rejection
reasons so the user can see what went wrong and either constrain further or write
the type explicitly.

---

## References

- `docs/prds/auto-type-param-resolution.md` — PRD with design rationale, acceptance
  criteria, and task breakdown.
- `docs/prds/v0_2/auto-resolution-backtracking.md` — v0.2 cross-parameter
  backtracking design.
- `docs/reify-language-spec.md` §3.9 "Type Parameters and Inference" — normative
  spec text for type-parameter `auto`.
- `docs/reify-language-spec.md` §9.3 "`auto` Resolution" — normative spec for
  `auto` resolution policy.
- `docs/reify-implementation-architecture.md` §6.2 row 5 — "Auto type resolution"
  as a topology-change source.
- `crates/reify-compiler/src/auto_type_param.rs` — implementation (module doc-comment
  documents Phases A/B/C and Multi-Param Orchestration in detail).
