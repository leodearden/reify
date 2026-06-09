# ADR-2522: Type-as-value design for generic conforms<T : Geometry, R : Trait>(g, Type<R>) → Bool

**Status:** Proposed  
**Date:** 2026-06-09  
**Task:** #2522

---

## Context

The geometry-traits PRD (`docs/prds/geometry-traits.md`) specifies two runtime-conformance
surfaces:

1. **Three monomorphic per-trait helpers** — `is_watertight(g) → Bool`, `is_manifold(g) → Bool`,
   `is_orientable(g) → Bool` — landed in task 2320. These are name-dispatched by
   `try_eval_conformance_query` (`crates/reify-eval/src/geometry_ops.rs`, wired from
   `crates/reify-eval/src/engine_build.rs`) via a lookup-by-cell-id sideband that
   resolves a geometry `ValueCellId` to a `GeometryHandleId` without storing the
   handle in the value map.

2. **One generic conformance query** — `conforms<T : Geometry, R : Trait>(g, Type<R>) → Bool`
   — deferred to v0.2 (PRD *Out of scope*, Decision 2). This form requires
   *type-as-value*: passing a trait name `R` as a runtime argument via a `Type<R>`
   parameter. The PRD notes that Reify's type system currently lacks this capability.

This ADR specifies the design for that deferred generic form. Its status is **Proposed**;
the implementation is explicitly scoped to v0.2.

---

## Problem Statement

The generic `conforms(g, Type<R>)` form cannot be added today because three concrete gaps
exist in the language pipeline:

### Gap 1 — No `Value::TraitTag` carrier

The `Value` enum (`crates/reify-ir/src/value.rs:739`) has no variant that carries a
trait identity. Existing variants are all scalars, collections, or named structural
references. A trait name like `Watertight` is not currently a value.

This is an intentional constraint defended by Decision 1 of the geometry-traits PRD:

> `Type::Geometry` has no `Value` variant … **Do not relax this invariant.** It is
> load-bearing for the snapshot/journal/content-hash architecture: a
> `Value::Geometry(GeometryHandleId)` would have to participate in `ContentHash`,
> `Journal` replay, and `BTreeSet`/`BTreeMap` ordering, but handle ids are
> per-realization, per-kernel, non-persistent cookies — none of those round-trips
> are well-defined.

A trait identity, however, is *different* from a geometry handle. A trait name is a
stable, content-addressable symbol: it does not vary across realizations, kernels, or
sessions. It round-trips cleanly through `ContentHash`, `Journal` replay, and
`BTreeSet`/`BTreeMap` ordering. This ADR extends Decision 1's reasoning to establish
that `Value::TraitTag` is *not* prohibited by the same invariant.

### Gap 2 — No `Type<R>` type variant

The `Type` enum (`crates/reify-core/src/ty.rs:73`) has three related but distinct
variants:

- `TraitObject(String)` — denotes "a value whose structure conforms to this trait"
  (i.e., a *value type* bound by a trait). The doc comment reads: "conformance
  enforcement deferred; records the declaration only."
- `TypeParam(String)` — an unresolved type variable awaiting substitution (e.g., `T`
  in a generic definition).
- `StructureRef(String)` — a concrete structure name at an instantiation site.

None of these represents "a runtime value whose content *is* a trait identity" — the
type of the `Type<R>` parameter in `conforms<T, R>(g : T, _ : Type<R>)`. That requires
a new variant.

### Gap 3 — No identifier-resolution path for bare trait names as values

In the current compiler and elaborator, bare trait names (`Watertight`, `Manifold`,
etc.) are recognized in type positions (bounds, parameter type annotations, `TraitObject`
resolution) but not in value/expression positions. There is no `ExprKind` variant for
a trait-tag literal, and no elaboration rule that lowers a bare identifier to a
`Value::TraitTag`. The call `conforms(p, Watertight)` would today fail to resolve
`Watertight` as an expression.

---

## Decision

Implement `conforms<T : Geometry, R : Trait>(g : T, _ : Type<R>) → Bool` in v0.2 using a
**type-as-value** design: trait identities become first-class runtime values (`Value::TraitTag`),
and `Type<R>` becomes a new type variant that carries the `R : Trait` bound. The generic form
dispatches through the existing `try_eval_conformance_query` hub, with the three task-2320
monomorphic helpers refactored into syntactic sugar over the generic, preserving all existing
`is_watertight(g)` / `is_manifold(g)` / `is_orientable(g)` call sites.

The design decomposes into four pipeline stages:

---

### Stage A — Parsing: new `ExprKind::TraitTag` variant

Add a new variant to `ExprKind` in `crates/reify-ast/src/ast.rs`:

```rust
/// A trait-tag literal used as a value in a type-as-value context:
/// `conforms(g, Watertight)` lowers the bare identifier `Watertight`
/// to this variant when it resolves to a known trait name.
TraitTag { name: String },
```

The parser itself does not distinguish trait names from ordinary identifiers at parse
time — they both produce `ExprKind::Ident`. The trait-tag *elaboration* (identity
resolution) happens in the type-checking pass (Stage B), which re-lowers
`ExprKind::Ident` to `ExprKind::TraitTag` when the identifier resolves to a declared
trait in the trait registry. This is an incremental extension of the existing
`<...>` generic-arg grammar (which already parses `Box<Bolt>`, `Bearing<auto: Seal>` etc.
at elaboration time), not a grammar change.

---

### Stage B — Type-checking: new `Type::TraitTagType` variant

Add a new variant to the `Type` enum in `crates/reify-core/src/ty.rs`:

```rust
/// The type of a runtime trait-tag value: `Type<R>` in the source.
/// Distinct from:
///   - `TraitObject(String)` — the type of *values conforming to* a trait
///     (e.g., `param material : Material`); values are conforming geometry/structs.
///   - `TypeParam(String)` — an unresolved type variable awaiting substitution.
///   - `StructureRef(String)` — a concrete structure name at an instantiation site.
/// `Type<R>` is "a value *whose content is* the trait name R"; the value space
/// is exclusively trait identities, not conforming instances.
TraitTagType { trait_name: String },
```

The `R : Trait` bound on `conforms` is checked using the **existing trait-conformance
predicate** (predicate #66 in the Reify type-checker, also used by the auto-type-param-
resolution machinery documented in `docs/prds/auto-type-param-resolution.md`). When the
compiler resolves the call `conforms(g, Watertight)`, it:

1. Looks up `Watertight` in the trait registry.
2. Checks that `Watertight` satisfies the `R : Trait` bound (it does — it is a declared
   geometry marker trait from `crates/reify-compiler/stdlib/geometry_traits.ri`).
3. Assigns the argument type `Type::TraitTagType { trait_name: "Watertight" }`.
4. Assigns the result type `Type::Bool`.

This keeps the `R : Trait` constraint checked at compile time rather than at runtime,
maintaining type safety without a full dependent-type system.

---

### Stage C — Lowering: `CompiledExprKind::Literal(Value::TraitTag(...))`

Lower the elaborated `ExprKind::TraitTag { name }` to a `CompiledExpr` in
`crates/reify-ir/src/expr.rs` carrying a resolved, stable trait identifier:

```rust
CompiledExpr {
    kind: CompiledExprKind::Literal(Value::TraitTag("Watertight".to_string())),
    result_type: Type::TraitTagType { trait_name: "Watertight".to_string() },
    content_hash: <hash of the trait name string>,
}
```

The trait name is the stable symbol — it is the string declared in the stdlib
(`geometry_traits.ri`) and checked against the trait registry. No handle, id, or
per-realization cookie is involved.

---

### Stage D — Runtime representation and eval-time matching

#### `Value::TraitTag`

Add a new variant to the `Value` enum in `crates/reify-ir/src/value.rs`:

```rust
/// A runtime trait-identity value. Carries the stable, content-addressable
/// name of a declared trait.
///
/// # Value-representability invariant (Decision 1 extension)
///
/// Unlike `Value::Geometry(GeometryHandleId)` — which is forbidden because
/// handle ids are per-realization, per-kernel, non-persistent cookies — a
/// trait name is a stable, persistable symbol. It round-trips through:
///   - `ContentHash` (hash the string bytes, deterministically)
///   - `Journal` replay (string is the same across sessions)
///   - `BTreeSet`/`BTreeMap` ordering (alphabetical, total, stable)
///
/// The `Ord`/`PartialOrd` impls use lexicographic order on the name string.
/// `ContentHash` hashes the string bytes (same strategy as `Value::String`).
TraitTag(String),
```

The `Value` enum already derives `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, and
implements `ContentHash`. All of these must cover the new variant:

| Obligation | Implementation |
|---|---|
| `PartialEq` / `Eq` | Auto-derived: string equality |
| `PartialOrd` / `Ord` | Lexicographic on the name string (matches `Value::String` strategy) |
| `ContentHash` | Hash the string bytes (same as `Value::String`) |
| `serde::Serialize` / `Deserialize` | Tag `"TraitTag"` + string field (matches existing enum serialization style) |

**Snapshot / Journal determinism:** the only persistent artifact is the trait name
string, which is stable across sessions, kernels, and realizations. No migration of
existing snapshots is required (the variant is new; no existing snapshots contain it).

#### Eval-time dispatch in `try_eval_conformance_query`

Extend `try_eval_conformance_query` in `crates/reify-eval/src/geometry_ops.rs` to handle
the generic call form `conforms(g, <TraitTag literal>)`:

1. **Recognize the call name** `"conforms"` alongside the existing three helper names.
2. **Extract the trait tag** from the second argument: a `CompiledExprKind::Literal(Value::TraitTag(name))`.
3. **Resolve the geometry argument** `g` via the existing lookup-by-cell-id sideband
   (resolve `ValueRef(cell_id)` → `GeometryHandleId` via `named_steps` / `step_handles`).
4. **Dispatch the geometry query** by mapping the trait name to the matching `GeometryQuery`
   variant — the same mapping already hard-coded for the three monomorphic helpers.
5. **Apply the user-assertion escape hatch** — check `template_trait_bounds` for the
   matching marker name and short-circuit to `Bool(true)` before the kernel call, exactly
   as the three helpers do today.

#### Sugar refactoring: monomorphic helpers become generic wrappers

The three task-2320 helpers are refactored into name-dispatch sugar over the generic:

```
is_watertight(g)  ≡  conforms(g, Watertight)
is_manifold(g)    ≡  conforms(g, Manifold)
is_orientable(g)  ≡  conforms(g, Orientable)
```

Existing `is_watertight(g)` / `is_manifold(g)` / `is_orientable(g)` call sites remain
valid without modification — the three names are preserved in the stdlib prelude as
forward-compatible aliases, and the compiler continues to resolve them through the
existing `GEOMETRY_QUERY_HELPER_NAMES` classifier in
`crates/reify-compiler/src/units.rs`.

---

## Alternatives Considered

### Option A — `Value::TraitTag` + `Type::TraitTagType` (CHOSEN)

See the *Decision* section above. Trait identities become first-class runtime values;
the generic `conforms` is the dispatch hub; the three monomorphic helpers become sugar.

**Pros:**
- Closes the type-as-value gap in a *reusable* way: `Value::TraitTag` and
  `Type::TraitTagType` are general machinery that future trait-parametric APIs can build on
  (e.g., `trait_name_of(g) → String`, a hypothetical `has_trait(g, T) → Bool`).
- Uniform generic dispatch: one `conforms` name, any trait, compile-time `R : Trait` check.
- Satisfies the value-representability invariant (stable string symbol, round-trips
  cleanly through `ContentHash` / `Journal` / `BTreeSet`/`BTreeMap` ordering).
- The three monomorphic helpers are preserved as forward-compatible aliases; zero
  source-level churn at existing call sites.

**Cons:**
- Every `Value` derive — `Ord`/`Hash`/`content_hash`/serde — must cover the new variant.
  The obligation table in Stage D enumerates the work.
- Snapshot/journal determinism must be re-validated after adding the variant (even though
  no existing snapshot can contain it, the derive order matters for future snapshots).
- The elaboration-time type-param machinery (`auto-type-param-resolution.md`) and the
  new runtime type-as-value path must be kept distinct: elaboration-time params are
  *type-level* substitutions resolved before lowering; `Value::TraitTag` is a *value-level*
  entity produced at lowering. Confusing the two would be a design error.

---

### Option B — Keep per-trait helpers only; `conforms` as compile-time macro/sugar

Implement `conforms(g, Watertight)` as purely syntactic sugar that the compiler
*immediately* desugars to `is_watertight(g)` at the call site, with no new `Value`
variant and no runtime trait tag. The `conforms` name is an alias resolved entirely
during elaboration.

**Pros:** No new `Value` variant, no derive obligations, no snapshot impact.

**Cons:**
- Only works for the *closed* set of 7 v0.1 traits. Once user-defined traits or an
  open trait extension mechanism is added, compile-time-only sugar breaks: the compiler
  cannot enumerate all future trait names.
- No runtime polymorphism: a function that takes a `Type<R>` parameter and passes
  it to `conforms` is impossible without a value carrier — the trait "tag" cannot be
  stored in a list, returned from a function, or held in a map.
- Locks the architecture into a closed-set assumption that contradicts the long-term
  direction.

---

### Option C — Stringly-typed `conforms(g, "Watertight")`

Express the trait argument as a string literal: `conforms(g, "Watertight")` where the
second argument is `Value::String("Watertight")`, with no new type-system machinery.

**Pros:** No new `Value` variant (uses existing `Value::String`). Trivial to implement.

**Cons:**
- The `R : Trait` bound cannot be checked at compile time: `conforms(g, "NonExistent")`
  is a type-correct expression that fails only at runtime.
- Pollutes the value space: `Value::String("Watertight")` is indistinguishable from any
  other string — a trait tag and a user string share the same representation.
- Defeats the purpose of a type system: users lose autocomplete, static error messages,
  and any future tooling that reasons about trait conformance.

---

### Option D — Defer entirely / never implement

Accept that `conforms<T, R>` is permanently out of scope; the three monomorphic helpers
are the final API surface.

**Pros:** Zero cost for v0.1.

**Cons:**
- The PRD explicitly scopes generic `conforms` to v0.2, not "never". Deferring the
  design (this ADR) to a future implementer without a recorded decision record increases
  the risk of an architecturally inconsistent implementation.
- As the trait set grows (user-defined traits, domain-lib traits), the monomorphic-only
  API becomes unwieldy: every new trait requires a new `is_<trait_name>` stdlib function.

---

## Consequences

### Positive

- The type-as-value gap is closed in a principled, reusable way: `Value::TraitTag` and
  `Type::TraitTagType` are general enough to support future type-valued APIs beyond
  conformance testing.
- Uniform generic dispatch under one `conforms` name; the compiler enforces `R : Trait`
  at compile time, so runtime trait-name mismatches are impossible.
- Existing `is_watertight` / `is_manifold` / `is_orientable` call sites remain valid
  indefinitely — the helpers are preserved as forward-compatible sugar aliases.
- `Value::TraitTag` satisfies the value-representability invariant: stable string content,
  deterministic ordering and hashing, clean journal/snapshot round-trips.

### Negative / Trade-offs

- **Derive obligations.** Every place the `Value` enum is pattern-matched or derived must
  cover `Value::TraitTag`. In practice: `impl Ord for Value`, `impl ContentHash for Value`,
  serde impls, and any match-exhaustiveness site in the codebase. The Stage D table enumerates
  the required impls; exhaustiveness is enforced by the Rust compiler.
- **Snapshot/journal determinism re-validation.** Adding any new `Value` variant requires
  re-running the existing snapshot determinism tests (even though no existing snapshot
  contains `Value::TraitTag`). This is a standard cost of extending the value domain.
- **Elaboration vs. runtime type-param distinction.** The auto-type-param-resolution
  machinery resolves `<T : Trait>` bounds at elaboration time (before lowering). The new
  `Value::TraitTag` is a runtime value produced *at* lowering. Implementers must ensure
  the two paths remain distinct: trait-tag values are not type parameters; type parameters
  are not trait-tag values. A guard in the elaborator should reject `Type<R>` in a position
  that only accepts a type parameter.
- **Open-vs-closed-trait-set risk.** In v0.1 the seven geometry marker traits are a fixed,
  compiler-known set. Generic `conforms` dispatch only pays off once the set is open or
  user-defined. The Option A machinery is designed for extensibility, but the v0.2 test
  suite should validate that the dispatch path correctly rejects unknown trait names with
  a type error rather than a runtime panic.

### Scope boundary

This ADR does not change any currently-shipped code. The `Value::TraitTag` variant,
`Type::TraitTagType`, `ExprKind::TraitTag`, and the `conforms` stdlib function are all
v0.2 additions. The existing `is_watertight` / `is_manifold` / `is_orientable` helpers
and `try_eval_conformance_query` are not modified by this task.

---

## Implementation Notes

### Crate / file map

The four pipeline stages each touch a specific crate and file:

| Stage | Crate | File | Change |
|---|---|---|---|
| A — Parsing / elaboration | `reify-ast` | `crates/reify-ast/src/ast.rs` | Add `ExprKind::TraitTag { name: String }` variant |
| B — Type-checking | `reify-core` | `crates/reify-core/src/ty.rs` | Add `Type::TraitTagType { trait_name: String }` variant |
| C — Lowering | `reify-ir` | `crates/reify-ir/src/expr.rs` | Handle `ExprKind::TraitTag` in the compiler's expression-lowering pass |
| D.1 — Value repr | `reify-ir` | `crates/reify-ir/src/value.rs` | Add `Value::TraitTag(String)` variant + cover all derives |
| D.2 — Eval dispatch | `reify-eval` | `crates/reify-eval/src/geometry_ops.rs` | Extend `try_eval_conformance_query` to handle `"conforms"` + `Value::TraitTag` arg |
| D.3 — Eval wiring | `reify-eval` | `crates/reify-eval/src/engine_build.rs` | Ensure `conforms` is included in the post-process conformance-query pass |
| Sugar aliases | `reify-compiler` | `crates/reify-compiler/src/units.rs` | Add `"conforms"` to `GEOMETRY_QUERY_HELPER_NAMES` (or a sibling classifier) |

Additionally, the compiler's expression-elaboration pass (wherever `ExprKind::Ident` is
resolved to typed expressions) must check whether the identifier names a declared trait
and, if so, re-lower it to `ExprKind::TraitTag` with type `Type::TraitTagType`.

### Suggested sub-task breakdown for v0.2

1. **`Value::TraitTag` + `Type::TraitTagType` variants** — Add both new variants,
   implement all derive obligations (see Stage D table), add unit tests for `Ord`,
   `ContentHash`, and serde round-trip. This is the foundation; all other sub-tasks
   depend on it.

2. **`ExprKind::TraitTag` elaboration** — Add the `ExprKind::TraitTag` variant,
   implement the trait-name → `TraitTag` resolution in the elaborator (re-lower
   `Ident` to `TraitTag` when the identifier is a declared trait), and add
   `Type::TraitTagType` to the type-checking result. Unit tests: known trait resolves
   to `TraitTag`; unknown identifier does not; `R : Trait` bound rejection test.

3. **`conforms` lowering** — Lower `ExprKind::TraitTag { name }` to
   `CompiledExprKind::Literal(Value::TraitTag(name))` in the compiler's expression
   lowering pass. Add `conforms` to the stdlib prelude (alongside the three helpers).

4. **Eval-time dispatch** — Extend `try_eval_conformance_query` to handle `"conforms"`
   with a `Value::TraitTag` second argument. Wire through `engine_build.rs`. Integration
   tests: `conforms(box(10mm,10mm,10mm), Watertight)` returns `Bool(true)`;
   `conforms(open_shell, Watertight)` returns `Bool(false)`;
   user-assertion short-circuit applies to generic form.

5. **Sugar alias validation** — Confirm `is_watertight(g)` remains forward-compatible
   with the generic path. Add a test asserting both spellings produce identical `Bool`
   results for the same geometry.

### Acceptance criteria

The v0.2 implementation is complete when:

1. `conforms(box(10mm,10mm,10mm), Watertight)` evaluates to `Bool(true)` in an
   integration test using the real OCCT kernel.
2. `conforms(open_shell_fixture, Watertight)` evaluates to `Bool(false)`.
3. `conforms(g, UnknownTrait)` is rejected at **compile time** with a
   `R : Trait` bound violation diagnostic (not a runtime panic).
4. `conforms(user_asserted_watertight_struct, Watertight)` short-circuits to
   `Bool(true)` without invoking the kernel (escape-hatch parity with monomorphic helpers).
5. `is_watertight(g)` continues to produce the same result as `conforms(g, Watertight)`
   for all fixtures.
6. `Value::TraitTag("Watertight")` round-trips through serde and `ContentHash` without
   mutation.
7. Snapshot determinism tests pass after the new variant is introduced.

### Forward-compat note

Existing call sites `is_watertight(g)`, `is_manifold(g)`, `is_orientable(g)` are
**unconditionally forward-compatible**: the three helper names are preserved in the
stdlib prelude regardless of whether the refactoring to generic sugar is implemented.
A future implementer may choose to route them through `conforms` internally while
keeping the three names as top-level aliases, or to keep them as independent
implementations — both are valid; the acceptance criteria require only result parity,
not a specific implementation strategy.

---
