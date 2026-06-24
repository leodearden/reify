# PRD: Cross-module parametric type-alias propagation (carry `TypeExpr` across the `CompiledModule` boundary)

**Task:** #4687 (graduated from the 2026-06-11 audit TODO backlog, item 11; #4552).
**Status:** authored 2026-06-24. Decomposable.
**Milestone:** v0.6.

## Goal

Make a **parameterized** prelude type alias (`type Box<T> = …`) instantiable from a
downstream module. Today a parametric prelude alias is **silently dropped** at the
`CompiledModule` boundary; the user gets an `unresolved type` error plus an Info
breadcrumb (task 2777's mitigation). After this PRD, a downstream module can write
`param v : Rate<Length>` (or `Vec3<Pressure>`) against a prelude-defined parametric
alias and have it resolve to the correct `Type`.

**Consumer / user-observable surface.** Two real prelude aliases ship as the
consumers of the new boundary-carrying mechanism, both grounded in existing stdlib
breadcrumbs:

- `pub type Vec3<Q: Dimension> = Vector3<Q>` — generalizes the existing
  `pub type Vec3 = Vector3<Length>` (`trajectory.ri:102`). Real cross-module
  consumers: the raw `Vector3<Length>` field annotations the stdlib comments mark
  for tightening (`fdm.ri:112`, `constitutive.ri`, `ports.ri:53-56`) become the
  friendlier `Vec3<Length>`. Requested by `fea_multi_case.ri:430,459` and
  `trajectory.ri:99`.
- `pub type Rate<Q: Dimension> = Q / Time` in `units.ri` — the DRY generalization of
  the existing dimensional aliases (`Velocity = Length/Time`,
  `VolumetricFlowRate = Volume/Time`, …). This alias carries a **type parameter in
  dimensional-operator position**, which is the case that forces the representation
  decision below — so shipping it doubles as the end-to-end proof of the hard path.

User-observable signal: a committed `.ri` example that `reify check`s clean while
instantiating a parametric prelude alias cross-module (previously emitted
`error: unresolved type` + the 2777 Info diagnostic), and the green stdlib build
(`load_stdlib()` panics on any Error-severity diagnostic, so a clean build is a
load-bearing assertion that the new aliases resolve).

## Background — what breaks today

- The only cross-module alias path is the **prelude seed**
  (`prelude_context.rs:141` → `lib.rs:440` → `aliases_phase.rs:79`). There is **no**
  user-module-to-user-module alias import (only `.type_aliases` from prelude modules
  is seeded). "Module A → module B" in this PRD means "prelude module → user module".
- `aliases_phase.rs:82` **skips** any prelude alias with non-empty `type_params` and
  records its name (`mark_skipped_parametric_prelude`).
- Root cause is one dropped field: the internal `TypeAliasEntry` carries
  `type_expr: Option<reify_ast::TypeExpr>` (the body needed for substitution,
  `type_resolution.rs:19`), but the public `CompiledTypeAlias` (`types.rs:1621`)
  deliberately omits it, so `from_compiled_for_prelude` (`type_resolution.rs:47`)
  hardcodes `type_expr: None` and `resolve_parameterized_alias` (`:1821`) bails at
  `alias_entry.type_expr.as_ref()?`.
- Task **2777** shipped only the use-site `Severity::Info` mitigation (`:1713`):
  *"type 'X' is a parametric prelude alias whose cross-module propagation is not yet
  implemented…"*. The real fix was deferred to this task.

**The scary framing was stale.** Two load-bearing comments are wrong today:
1. *"adds a `reify_syntax` dependency across the boundary"* — `TypeExpr` lives in
   `reify_ast`, not `reify_syntax`, and `CompiledModule` **already** carries
   `reify_ast` types across the boundary (`constraint_defs[].predicates: Vec<reify_ast::Expr>`
   at `types.rs:1670`, `pragmas: Vec<reify_ast::Pragma>` at `:391`). Adding a
   `TypeExpr` field introduces **no new crate dependency**.
2. *"serialize across the boundary"* — `CompiledModule` has **no serde derives** and
   is never written to disk (stdlib is cached in-memory via `OnceLock`). "Crossing
   the boundary" is in-process struct-field passing, not byte serialization.

So this is not an IR-serialization problem. It is a representation choice plus a
soundness (resolution-environment) decision, both resolved below.

## Sketch of approach

1. **Carry the body.** Add `type_expr: Option<reify_ast::TypeExpr>` to
   `CompiledTypeAlias`. Populate it from `TypeAliasEntry.type_expr` in
   `into_compiled()`; stop hardcoding `None` in `from_compiled_for_prelude()`.
2. **Un-skip the seed.** In `aliases_phase.rs`, replace the
   `mark_skipped_parametric_prelude` skip with a real seed of the parametric entry
   (now carrying its `type_expr`). The existing within-module
   `resolve_parameterized_alias` / `resolve_type_alias_expr_with_subst` machinery
   then resolves cross-module use sites **unchanged** (it already substitutes type
   args into the body and is restricted to builtins + aliases, `:2698-2707`).
3. **Retire the 2777 mitigation.** Remove `mark_skipped_parametric_prelude` /
   `is_skipped_parametric_prelude` / `should_emit_skipped_parametric_prelude_info`
   and the Info emission (`:1713`); flip the
   `cross_module_alias_propagation_tests.rs` "emits Info" assertions to
   "resolves correctly".
4. **Definition-site validation guard (strict).** At the **defining** module,
   resolve each `pub` parametric alias body against the module's *exported*
   environment with type-params bound to their **declared bounds**, reusing the
   existing applied-type-arg bound checker (`check_type_param_bounds` /
   `phase_pending_bound_checks`, task 4603 γ, `type_resolution.rs:3537+`). Emit an
   error at the definition site if the body (a) references a non-exported name, or
   (b) uses a param in a position its bound doesn't satisfy. This closes the
   name-capture hazard so consumers can trust the exported body.
5. **Prelude-only scope breadcrumb.** Add an `at-time-of-writing` comment at the seed
   site recording that this carries prelude aliases only; general user-module import
   is deferred (see Out of scope).

**Not an in-engine seam (G1 engine sub-check N/A).** This is compiler-internal type
resolution; it does not route through any of the 7 in-engine seams in
`engine-integration-norm.md §3`. The consumer is the use-site resolver plus the two
stdlib aliases.

### Why raw `TypeExpr`, not a pre-resolved type-scheme

A type parameter can appear in **dimensional-operator position**
(`type Rate<Q> = Q / Time`), and the result `DimensionVector` is computed by
dimensional arithmetic **at substitution time**
(`resolve_type_alias_expr_to_dim_with_subst`, `type_resolution.rs:3279-3296`). There
is no `Type` value that represents "`Q / Time` with `Q` still free" — `DimensionVector`
holds concrete exponents, not a symbolic param. A pre-resolved `Type`-with-holes
(via `substitute_type_params`, `:1839`) is therefore *structurally incapable* of
carrying such a body. Only the raw `TypeExpr` is fully general, and it reuses the
entire existing within-module instantiation path. This mirrors the established
`constraint_defs[].predicates` precedent (raw AST carried across the boundary,
substituted at the call site).

## Pre-conditions for activating (substrate — all confirmed)

- **Grammar (G3).** The bounded consumer forms parse today
  (`tree-sitter parse --quiet`, 0 ERROR nodes, 2026-06-24):
  `pub type Rate<Q: Dimension> = Q / Time`,
  `pub type Vec3<Q: Dimension> = Vector3<Q>`, and the use site
  `param r : Rate<Force>`. Parser test already covers the shape
  (`crates/reify-syntax/tests/type_alias_tests.rs:411` —
  `type Velocity<T> = T / Time`); `grammar.js:438` pre-comments
  `type Stress<T> = Force / Area`. **No grammar work required.**
- **`Dimension` bound exists.** `fields.ri:156` uses `Q: Dimension`; the stdlib
  compiles clean and `load_stdlib()` asserts no Error diagnostics — so `Q: Dimension`
  is confirmed-valid substrate.
- **Bound checker exists.** `check_type_param_bounds` / `phase_pending_bound_checks`
  (task 4603 γ) are the reuse target for the strict guard — no new bound-checking
  substrate to build.
- **Boundary is in-memory.** `CompiledModule`/`CompiledTypeAlias` have no serde; the
  field add is a struct change only.

## Resolved design decisions

| # | Decision | Resolution |
|---|---|---|
| D1 | Representation of the alias body across the boundary | Raw `reify_ast::TypeExpr` (option a). Only fully-general form (dimensional-op-over-param), reuses existing resolver, no new dep/serialization. |
| D2 | Scope | **Prelude-only**. The shared-environment argument makes it sound by construction (any alias a prelude body references is itself seeded into the consumer). |
| D3 | Definition-site validation | **Strict, with bound enforcement.** Resolve body in the defining module against the exported env with type-params bound to declared bounds (reuse `check_type_param_bounds`); error at the def site on non-exported-name reference or bound violation. |
| D4 | Real consumers | **Both** `Vec3<Q: Dimension>` (builtin-arg path, fdm/constitutive/ports migration) **and** `Rate<Q: Dimension>` (dimensional-op-over-param path, units.ri). |
| D5 | Content hash | No change — `CompiledTypeAlias.content_hash` already derives from the full declaration source text. |

## Out of scope

- **General user-module → user-module alias import.** Today only the prelude seeds
  aliases cross-module; this PRD does not add an "import aliases from another user
  module" path (where the name-capture hazard becomes real). Deferred: file a
  **deferred bookmark task** at decompose (`planning_mode=True`, excluded from the
  pending flip) — "general cross-module alias import" — gated on a real consumer,
  with a one-paragraph forward-stub PRD referenced from the bookmark. Owner: this
  project, when a consumer appears.
- **FEA `traction` / `force_density` field migration.** `fea_multi_case.ri:430,459`
  mark `Real` placeholders for `Vector3<Pressure>` / `Vector3<ForceDensity>`. Those
  field changes are owned by the **FEA Load/Support follow-on PRD** (task 4092 area),
  not this PRD. This PRD ships the `Vec3<Q>` alias they will consume.

## Cross-PRD relationship + seam owners

| Seam | Owner | Status |
|---|---|---|
| Parametric-alias boundary mechanism + the two stdlib aliases | **this PRD (#4687)** | active |
| General user-module alias import | this project, deferred bookmark | out of scope; bookmark filed at decompose |
| FEA `traction`/`force_density` → `Vec3<…>` field migration | FEA Load/Support follow-on PRD (4092 area) | out of scope; consumes this PRD's `Vec3<Q>` |
| Bound-checking substrate (`check_type_param_bounds`) | task 4603 γ (landed) | reused, not modified |

No new contested-ownership pair is introduced (the three contested seams in the
overlay are unrelated).

## Decomposition plan

Vertical slices; each leaf names a user-observable signal. `grammar_confirmed=true`
for all (no novel syntax).

- **Task A — Cross-module parametric resolution + dimensional consumer (core slice).**
  Add `type_expr` to `CompiledTypeAlias` (update all construction sites:
  `types.rs`, `type_resolution.rs`, `reify-test-support/src/builders/module.rs`, and
  the 3 compiler test files); populate it in `into_compiled()`; un-skip the
  parametric prelude seed in `aliases_phase.rs`; retire the 2777 skip/Info machinery;
  add the prelude-only breadcrumb; add `pub type Rate<Q: Dimension> = Q / Time` to
  `units.ri`.
  *Signal:* a committed `.ri` example (e.g. under `tests/prd-gate/fixtures/` or
  `examples/`) in a user module with `param v : Rate<Length>` that `reify check`s
  clean and resolves to the Velocity dimension (Length/Time), where the same model
  previously errored + emitted the Info diagnostic; the flipped
  `cross_module_alias_propagation_tests.rs` test asserts resolution success.
  *Consumer:* the `.ri` example + the flipped test. *Files:* `crates/reify-compiler/src/types.rs`,
  `crates/reify-compiler/src/type_resolution.rs`,
  `crates/reify-compiler/src/compile_builder/aliases_phase.rs`,
  `crates/reify-compiler/stdlib/units.ri`,
  `crates/reify-test-support/src/builders/module.rs`,
  `crates/reify-compiler/tests/cross_module_alias_propagation_tests.rs`.

- **Task B — `Vec3<Q>` builtin-arg consumer + stdlib migration.** Add
  `pub type Vec3<Q: Dimension> = Vector3<Q>` in a module visible (per the topo import
  graph) to the migration consumers; migrate ≥1 real cross-module `Vector3<Length>`
  field annotation (`fdm.ri:112` / `constitutive.ri` axes / `ports.ri:53-56`) to
  `Vec3<Length>`, plus the same-module no-arg `Vec3` use at `trajectory.ri:350`.
  *Signal:* the stdlib builds clean (`load_stdlib()` Error-diagnostic panic is the
  assertion) with the migrated fields resolving through the parametric alias, and a
  committed `reify check` example using `Vec3<Pressure>` cross-module resolves.
  *Consumer:* the migrated stdlib fields. *Depends on:* Task A. *Files:* `[]` —
  home-module placement + exact migration set is bounded by the prelude import graph;
  architect (BRE) acquires the footprint (see Open questions).

- **Task C — Definition-site validation guard (strict + bounds).** Validate `pub`
  parametric alias bodies at the defining module against the exported env with
  declared bounds, reusing `check_type_param_bounds`.
  *Signal (negative-assertion):* a committed fixture with an ill-formed `pub`
  parametric alias — body referencing a non-exported name, and a param used against a
  bound it violates — makes `reify check` exit non-zero with a diagnostic **at the
  definition site** (observe the rejection actually fires; silent-accept = FAIL).
  *Consumer:* the rejection fixture. *Depends on:* Task A. *Files:*
  `crates/reify-compiler/src/compile_builder/aliases_phase.rs`,
  `crates/reify-compiler/src/type_resolution.rs`.

- **Bookmark (deferred, not flipped to pending) — general cross-module alias import.**
  Filed `planning_mode=True`, excluded from the pending flip; references a
  one-paragraph forward-stub PRD; gated on a real consumer. (G4 seam deliverable.)

## Open (tactical) questions — deferred to implementation

- **`Vec3<Q>` home module + migration set (Task B).** Which module hosts
  `Vec3<Q>` so it is visible (per `compile_modules_topo` dependency order) to the
  chosen consumers, and which of `{fdm, constitutive, ports}` fall inside that
  visibility. Candidate home: an early/base module (e.g. `units.ri`, universally
  visible) or `trajectory.ri` with consumers restricted to modules that import it.
  Requirement: ≥1 genuine cross-module consumer migrated so the leaf signal is real.
- **`Rate<Q>` companions.** Whether to also ship `Flux<Q: Dimension> = Q / Area`
  (generalizing `HeatFlux`) alongside `Rate<Q>`, or keep the dimensional family to
  one alias for now. (`Density` is unavailable — already a dimension name,
  `materials_fea.ri:157`.)
- **Guard reuse shape (Task C).** Exact entry point into `check_type_param_bounds`
  for alias bodies vs. the existing applied-type-arg call site.
