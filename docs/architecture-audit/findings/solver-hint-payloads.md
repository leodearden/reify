# Audit: `@solver_hint` content payloads for v0.1

**PRD path:** `docs/prds/solver-hint-payloads.md`
**Auditor:** audit-solver-hint-payloads
**Date:** 2026-05-12
**Mechanism count:** 11
**Gap count:** 6 (5 non-WIRED, 1 PARTIAL)

## Top concerns

- **Preamble FICTION:** PRD opens by asserting "Tasks #275 and #276 are done … solver-side integration through engine + constraint solver" is wired. Code search across `reify-constraints`, `reify-eval`, and `reify-doc` (non-test) finds **zero readers** of `ValueCellDecl.solver_hints`. The compiler stores hints but no back-end consumes them — `@solver_hint` is currently a no-op for solver behavior. This affects all three PRD items: tests only check that hints are *stored*, not that they *influence* anything.
- **Doc-page acceptance criterion is unimplementable as written:** PRD §"Acceptance" requires `reify doc` to "render the two collection constants in the stdlib docs page". The `cmd_doc` CLI (`crates/reify-cli/src/main.rs:393`) only takes a user file as input — there is no stdlib-docs page surface, and `reify-doc/src/lib.rs` exposes only a per-module formatter. Acceptance is gated on a separate PRD that has not produced the surface.
- **Example file acceptance criterion partially unmet:** `examples/m11_annotations.ri` does **not** contain a block exercising `standard_bolt_lengths`/`standard_sheet_thicknesses`. PRD §"Acceptance" bullet 1 was not landed (the file covers only `@test`).
- **Representation drift from PRD shape:** PRD §1 describes the collections as "expressed as a list of `Length` values" (implying constants); shipped form is zero-arg `pub fn` because Reify lacks top-level `const` (acknowledged in the .ri file's header comment). Task 2455 (deferred, "convert to module-level constants once syntax supports them") is the bookmark for closing the drift.

## Mechanisms

### M-001: `@solver_hint` annotation parse + lowering

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/annotations.rs:135-144` (validate_annotations context check), `crates/reify-compiler/src/annotations.rs:395-454` (`extract_solver_hints`); tests `crates/reify-compiler/tests/solver_hint_tests.rs` 18 cases pass
- **Blocks:** none
- **Note:** Per PRD preamble, task #275 (compiler-side annotation extraction) is done. The annotation is recognised on structure/occurrence/param/let contexts, both args (kind + ident) are parsed into `SolverHint { kind, collection, span }`.

### M-002: `SolverHintKind::DiscreteSet` / `PreferStock` enum variants

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:767-779`
- **Blocks:** none
- **Note:** Both DiscreteSet and PreferStock are first-class variants of `SolverHintKind`.

### M-003: `SolverHintKind::PreferredStrategy` enum variant

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:778`; `crates/reify-compiler/src/annotations.rs:409`; `crates/reify-compiler/tests/solver_hint_tests.rs:290-330` (table-driven argmin_default/slvs_default/custom_xyz tests)
- **Blocks:** none
- **Note:** PRD §3 (optional). The compile-time path is shipped. Any identifier accepted; PreferredStrategy hints skipped by `validate_solver_hint_collections` (`annotations.rs:494`).

### M-004: `std.stock` stdlib module containing `standard_bolt_lengths` + `standard_sheet_thicknesses`

- **State:** PARTIAL
- **Failure mode:** DRIFT (PRD describes "list of `Length` values" / "collection constants"; shipped as zero-arg `pub fn` returning `List<Length>`)
- **Evidence:** `crates/reify-compiler/stdlib/standard_stock.ri:1-21`; `crates/reify-compiler/src/stdlib_loader.rs:100`; tests `crates/reify-compiler/tests/standard_stock_tests.rs` verify values + dimensions
- **Blocks:** none directly; task 2455 (deferred) bookmarks the const conversion
- **Note:** Functionally equivalent — the .ri header comment acknowledges "Exposed as zero-arg `pub fn` because Reify lacks top-level `const`; migrate when added." Both collections cover the PRD-specified increments verbatim. The function-vs-constant distinction does not affect compile-time `@solver_hint` resolution (the identifier resolves through `functions.iter().any(|f| f.name == *name)` per `annotations.rs:498`).

### M-005: Doc comments on stdlib stock collections

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/stdlib/standard_stock.ri:7-9, 15-17` (triple-slash doc comments present per PRD §1 "mandatory" requirement)
- **Blocks:** none
- **Note:** Doc strings are present. Whether they surface in LSP hover and doc generator depends on M-010 / M-011 below.

### M-006: Stdlib module wired into prelude loader (`STDLIB_MODULES`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/stdlib_loader.rs:100` — `("std.stock", include_str!("../stdlib/standard_stock.ri"))`
- **Blocks:** none
- **Note:** Joins prelude load order; sequential compilation with growing prelude lets the module reference `Length` (declared in `std.units`).

### M-007: Hint-payload name resolution at compile time (use-site)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/annotations.rs:487-505` (`validate_solver_hint_collections`); `crates/reify-compiler/src/entity.rs:1033,1145`, `src/guards.rs:379,456`; tests `crates/reify-compiler/tests/solver_hint_payload_tests.rs` 3 cases pass (positive discrete_set, positive prefer_stock, negative unresolved-name error)
- **Blocks:** none
- **Note:** Hint collection identifiers go through the same scope + functions-list resolution that `compile_expr` `Ident` uses. Unresolved name = `Error` diagnostic; type-checking that the resolved entity is `List<Length>`-typed is **explicitly deferred** to a later compiler pass (`annotations.rs:474-477`, task 2334 follow-up cited but not searched per audit-brief boundary rule).

### M-008: Solver-side / runtime consumption of `SolverHint` (the "what the hint actually does")

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no runtime/solver backing)
- **Evidence:** Recursive grep for `\.solver_hints` outside test/`test_support` namespaces in `crates/`: **zero readers**. `crates/reify-constraints/src/` has zero references to `SolverHint`, `solver_hint`, `DiscreteSet`, or `PreferStock`. `crates/reify-eval/src/` has zero non-test references. Auto-param resolution in `crates/reify-eval/src/concurrent.rs:263-466` resolves auto params via the constraint solver but does not consult hint collections.
- **Blocks:** Real PRD goal ("nudge solver toward stock values") for *every* use of `@solver_hint`; PRD acceptance bullet "`reify check` produces no warnings" is met only because hints are silently ignored.
- **Note:** PRD preamble claims task #276 wired solver-side integration. Code evidence directly contradicts this: hints are accumulated on `ValueCellDecl` and never read. The integration tests added by tasks 2333/2339 only check the *compile-side* artefact (`cell.solver_hints[0].kind == DiscreteSet` and the collection name string). No test confirms a solver run changes outputs based on `@solver_hint`. This is the largest gap surfaced by the audit — same shape as GR-001 (compile-side contract with no runtime backing), but distinct mechanism.

### M-009: Hint-collection looked up *by the solver* yields `Vec<Length>` of the expected length

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §2 explicitly says "resolves to a `Vec<Length>` of the expected length when looked up." The shipped test (`crates/reify-compiler/tests/solver_hint_payload_tests.rs:49-60`) only asserts `cell.solver_hints[0].collection == "standard_bolt_lengths"` (a string match on the name). It does NOT perform the lookup-to-`Vec<Length>` step. `standard_stock_tests.rs:122-149` covers the value-evaluation of the collections in isolation, but the linkage from `SolverHint` to a `Vec<Length>` is nowhere wired.
- **Blocks:** Same downstream consumers as M-008.
- **Note:** The PRD's "round-trip … to the solver" task slice (item 2) was implemented as a round-trip to `ValueCellDecl`, not to the solver. Auditor judgement: this is the symptom that surfaces M-008 most cleanly.

### M-010: `preferred_strategy` back-end runtime-warning emission for unknown strategy names

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** PRD §3 says "the back-end emit a runtime warning for unknown names". No back-end consumes solver hints (M-008), therefore no runtime warning is plumbed. `validate_solver_hint_collections` deliberately skips PreferredStrategy at compile time (`annotations.rs:494-496`) to preserve the spec §12.2 advisory invariant — the back-end half of the contract is missing.
- **Blocks:** none operationally (no consumer to be confused), but documents that the "extensible without a language change" property only holds at compile time.
- **Note:** Library authors who write `@solver_hint("preferred_strategy", weird_typo)` will see no diagnostic anywhere — compile-time accepts any ident, runtime never runs.

### M-011: Stdlib docs page surface for `reify doc` (acceptance criterion target)

- **State:** FICTION
- **Failure mode:** F1 (acceptance assumes infrastructure that does not exist)
- **Evidence:** `crates/reify-cli/src/main.rs:393-` (`cmd_doc`) takes one user file as input; rejects extra positional args. `crates/reify-doc/src/lib.rs` exports per-module `build_doc_model` (referenced via task 2342) — no stdlib-walking surface. The doc-generator HAS the per-param `@solver_hint` rendering (tests at `crates/reify-doc/tests/fmt_markdown_tests.rs:1828-1873`, `fmt_html_tests.rs:2063` — these are about hints on *consumer* params, not the stdlib collection definitions themselves).
- **Blocks:** none for solver-hint-payloads PRD specifically; the cross-PRD reify-doc-tool owns the missing surface
- **Note:** PRD §"Acceptance" bullet 3 is "(separate PRD)" — but it is listed as a hard acceptance gate. Auditor reading: this is acceptance leakage across PRDs; classifying as FICTION since the surface itself does not exist, even though the responsibility lies elsewhere. Cross-PRD breadcrumb below.

### M-012: `examples/m11_annotations.ri` exercises `standard_bolt_lengths` + `standard_sheet_thicknesses` under `reify check` with no warnings

- **State:** TODO
- **Failure mode:** F2 (PRD acceptance bullet not implemented)
- **Evidence:** `examples/m11_annotations.ri` (read end-to-end) covers `@test`, inline + constraint-def references, sub-structures. It contains **zero** references to `standard_bolt_lengths` or `standard_sheet_thicknesses`. Grep across `examples/` and all worktrees confirms only `stdlib/standard_stock.ri` itself names the collections.
- **Blocks:** PRD §"Acceptance" bullet 1 explicit non-coverage
- **Note:** Smallest gap; trivially addressable by appending a small @solver_hint block to m11_annotations.ri. Useful future audit signal: PRD acceptance bullets that touch existing example files are easy to forget when the example file isn't in the task's metadata.files.

## Cross-PRD breadcrumbs

- **`reify-doc-tool` PRD** owns M-011 (stdlib docs page rendering). The hint-payloads PRD's acceptance criterion 3 cannot be met until that surface ships. The doc generator already has per-param `@solver_hint` rendering for consumer-side annotations (`reify-doc/tests/fmt_markdown_tests.rs:1828`), so the stdlib-walking part is the only missing piece.
- **Compute-node-infrastructure / FEA PRD family:** the FICTION pattern for M-008/M-009 (compile-side stores annotation, no downstream reads it) mirrors GR-001 and the broader theme of "PRD preamble claims integration done; code only stores the artefact". Phase 3 may want to fold solver-hint-payloads into the same disposition group.
- **Task 2455 (deferred):** the const-vs-fn drift in M-004 is bookmarked here. Cross-cutting with any future Reify language work that adds top-level `const`.
- **Auto-param resolution:** `crates/reify-eval/src/concurrent.rs:263-466` is the natural consumer site for M-008. If a future PRD wires hints into the solver, it would hook there (auditor noted location; not following).
