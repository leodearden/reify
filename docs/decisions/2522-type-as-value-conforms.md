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
