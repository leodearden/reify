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
