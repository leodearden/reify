# PRD: `@solver_hint` content payloads for v0.1

**Status:** v0.1 scope.
**Spec reference:** `docs/reify-language-spec.md` §12.1.
**Pre-existing infrastructure:** Tasks **#275** (compiler-side annotation extraction → `SolverHint { kind, collection, span }` on `ValueCellDecl`) and **#276** (solver-side integration through engine + constraint solver) are **done**. `extract_solver_hints` in `crates/reify-compiler/src/annotations.rs` parses the two arg form `@solver_hint("<kind>", <ident>)` and recognises `discrete_set` / `prefer_stock`. Tests exist at `crates/reify-compiler/tests/solver_hint_tests.rs`.

This PRD is about the **payload data the hints reference**: the `<ident>` second arg today resolves to a *name string* but there are no stdlib collections under those names for designs to point at. Without payloads, every realistic `@solver_hint` use site needs to define its own collection — which defeats the point ("standard bolt lengths" should be a stdlib fact, not per-design boilerplate).

## Goals

Ship the standard collections that the spec examples reference, validate that `@solver_hint("discrete_set", standard_bolt_lengths)` resolves end-to-end, and add the optional `preferred_strategy` payload kind.

## Items

### 1. Stdlib collections

Add a new prelude file `crates/reify-compiler/stdlib/standard_stock.ri` with at minimum:

- **`standard_bolt_lengths`** — `Length` collection covering the ISO 4014 / 4017 length series. Initial scope: 8 mm through 100 mm at the standard increments (`8, 10, 12, 14, 16, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 90, 100`, all in mm). Expressed as a list of `Length` values.
- **`standard_sheet_thicknesses`** — `Length` collection covering common metal stock gauges. Initial scope (mm): `0.5, 0.8, 1.0, 1.2, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0`.

Wire the new module into `crates/reify-compiler/src/stdlib_loader.rs` (`load_stdlib_context` / `STDLIB_MODULES` array) so it joins the prelude load order alongside `std.units`, `std.materials_mechanical`, etc. Pick a module path; recommended: `std.stock`.

Both collections must satisfy whatever collection trait the constraint-solver `discrete_set` / `prefer_stock` integration (task #276) consumes. Confirm the trait during architect phase by reading `reify-constraints` and the integration done by #276 — do **not** invent a new trait if one is already in use.

Doc comments are mandatory on both collections. They surface in the doc generator (separate PRD) and in LSP hover.

### 2. Wire-up validation

Add an integration test at `crates/reify-compiler/tests/solver_hint_payload_tests.rs` (or fold into the existing `solver_hint_tests.rs`) that:

- Compiles a small structure annotated `@solver_hint("discrete_set", standard_bolt_lengths) param length : Length = auto` under `compile_with_stdlib` — succeeds, no warnings, the resulting `ValueCellDecl.solver_hints` carries the right `collection` and resolves to a `Vec<Length>` of the expected length when looked up.
- Same but for `prefer_stock` + `standard_sheet_thicknesses`.
- Negative case: `@solver_hint("discrete_set", standard_doesnotexist)` produces an unresolved-identifier error at the use site (validates that hint payload references go through normal name resolution, not a special-cased lookup).

### 3. Optional: `preferred_strategy` hint kind

**Goal:** unblock library authors who want to nudge the solver toward a particular search heuristic (e.g. `"backtrack_then_relax"`).

**Argument grammar:** `@solver_hint("preferred_strategy", <ident>)` where the `<ident>` is one of a small known set defined by the constraint solver back-ends:
- `argmin_default`
- `slvs_default`
- (extensible without a language change — `extract_solver_hints` should accept any identifier and let the back-end emit a runtime warning for unknown names)

**Implementation:** add `PreferredStrategy` to `SolverHintKind` in `crates/reify-compiler/src/types.rs`, extend `extract_solver_hints` to recognise the new string, and forward to the solver back-end alongside the existing `DiscreteSet`/`PreferStock` flow.

**Defer if straightforward — drop if scope-tight.** The two collection-payload kinds are the priority; this third kind is bonus. Mark its task `priority: medium` in fused-memory and leave it undone if the budget runs out without blocking anything else.

## Acceptance

- `examples/m11_annotations.ri` (already present) gains a small block that uses both `standard_bolt_lengths` and `standard_sheet_thicknesses` with `@solver_hint`, producing no warnings under `reify check`.
- The new tests pass.
- `reify doc` (separate PRD) renders the two collection constants in the stdlib docs page with their doc comments.

## Task slicing

Three tasks (collapse to two if `preferred_strategy` is dropped):

1. **stdlib stock collections** — new `std.stock` module with `standard_bolt_lengths` and `standard_sheet_thicknesses`, wired into the prelude loader. **High priority.**
2. **end-to-end wire-up test** — integration test that round-trips a `@solver_hint("discrete_set", standard_bolt_lengths)` annotation to the solver. **High priority.**
3. **`preferred_strategy` hint kind** *(optional)* — add `PreferredStrategy` to `SolverHintKind`, extend `extract_solver_hints`, plumb through. **Medium priority. Drop if scope-tight.**
