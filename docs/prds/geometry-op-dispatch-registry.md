# Geometry-op dispatch-registry refactor

**Status:** authored 2026-06-18 · **Type:** structural / build-infra (version-agnostic foundation)
**Slug:** `geometry-op-dispatch-registry` · **Manifest:** `geometry-op-dispatch-registry.capability-manifest.md`

## 1. Consumer & user-observable surface (G1)

**Primary consumer: orchestrator throughput / developer concurrency — not a new `.ri` feature.** Be honest about this. The payoff is structural: adding a geometry op stops textually colliding inside shared exhaustive `match` sites, so geom-* tasks stop serializing behind a single hot in-file merge anchor.

Origin: a jun18 contention analysis found that of 164 pending orchestrator tasks, **zero** were fully unblocked — the backlog serializes behind a few hot-module lock-holders. Narrowing over-greedy directory locks is handled separately; the *residual* serialization is that even with file-level locks, every new `GeometryOp` variant must touch several central exhaustive `match` sites **in one file at once**. That can only be removed structurally, by making op dispatch additive/append-only.

**In-engine seam (G1 engine-integration sub-check):** this restructures the existing **op-execute** dispatch path (`engine-integration-norm.md` §3.1) and its compiler/IR feeders. It introduces **no new seam** — it re-homes the data the existing dispatch already encodes into an append-only table. The named consumer of the new descriptor table is the build walk in `crates/reify-eval/src/engine_build.rs` (op-execute) plus the compiler name-dispatch in `crates/reify-compiler/src/geometry.rs`.

**User-observable signal (the honest one).** Because there is no new runtime feature, completion is observable two ways, both real and checkable:
1. **Behavioral equivalence** — `reify check` / `reify eval` over the existing `.ri` example corpus produce byte-identical diagnostics and handle/mesh graphs before and after. Nothing a user sees changes (that *is* the requirement: a pure refactor).
2. **A guard/completeness test** — adding a `GeometryOp` variant without registering a descriptor row makes a CI test go **RED** with a clear message. The test failing is the signal; it gates every future op addition.

This is **not** the rejected "a unit test passes against synthetic input" leaf signal: the completeness test enumerates the **real** production enum, and the equivalence tests run the real CLI over the real corpus.

## 2. The contention sites today (G3 substrate — confirmed jun18, re-confirm at impl time)

A new geometry op currently forces simultaneous edits across **three distinct dispatch axes** in three crates. All anchors below were re-confirmed jun18 at the cited lines.

### Axis 1 — variant→facts (the *hottest*; genuinely exhaustiveness-forced)
`crates/reify-eval/src/engine_build.rs`, all matching exhaustively over `GeometryOp` (no `_ =>` arm):
- `geometry_op_to_operation` (`:1511–1591`, 49 arms) — variant → `Operation`.
- `parent_handles_for_op` (`:1254–1340`, 10 or-pattern arms) — variant → parent-handle arity + which fields are parents.
- `substitute_op_parents` (`:1350–1437`, 6 arms) — the exact inverse/mirror of the above (rewrites the same parent fields).
- The IR-side `kind_name()` token table in `crates/reify-ir/src/geometry.rs`, pinned by `GEOMETRY_OP_VARIANT_COUNT = 48` (`:7128`).

These are **pure per-variant static data** — they read no runtime state (verified: all take only `&GeometryOp` / `&mut GeometryOp` + a map). `Operation` (47 unit variants) is defined in the **same crate** as `GeometryOp` (`crates/reify-ir/src/geometry.rs:262` and `:539`), so the whole variant→facts fact-set lifts cleanly into one table **in `reify-ir`**.

### Axis 2 — name→op (canary + name-lists, *not* exhaustiveness-forced)
`crates/reify-compiler/src/geometry.rs`:
- `compile_geometry_call` (`:1052–1967`) — has a `_ =>` "unsupported geometry function" wildcard (`:1963`), so the match itself is **not** a collision point.
- The collision is `EXPECTED_DISPATCH_COUNT = 56` (`:2178`) plus the four membership lists (`GEOM_ARG_FUNCTIONS` / `NO_GEOM_ARG_FUNCTIONS` / `BOOLEAN_OP_FUNCTIONS` / `LOFT_FUNCTIONS`), asserted by `all_dispatch_functions_accounted_for` (`:2267`). Adding a function name forces a one-line bump + list edit at a shared site.

### Axis 3 — behavioral compile (`reify-eval`-local; highest regression risk)
`crates/reify-eval/src/geometry_ops.rs`:
- `compile_geometry_op` (`:733–2168`, **1435 lines**) — matches on `reify_compiler::CompiledGeometryOp` (8 variants) with nested `PrimitiveKind`/`ModifyKind`/`SweepKind`/`TransformKind`/`PatternKind`/`CurveKind`/`ProfileKind` sub-matches (7/9/8/5/5/6/4 arms respectively). Exhaustive over the fieldless kind enums.
- The arms are **Engine-free free functions** (file header `:1–4`: "no Engine coupling") but close over `reify_compiler::CompiledGeometryOp`, `reify_expr::eval_expr`, and `reify-eval`-private helpers (`eval_ctx_with_meta`, `resolve_curated_edges_p2`, `eval_named_arg`). So a behavioral table **must live in `reify-eval`** (it cannot be fn-pointers in `reify-ir` — crate layering forbids it).

### Substrate facts that de-risk the design
- **`inventory` is already reify's in-production dispatch-registry** (workspace dep; `KernelRegistration` table in `reify-ir/src/geometry.rs:522/:535`, consumed by `reify-eval/src/kernel_registry.rs`). We are **not** introducing a new registry mechanism — and per §3 we choose an even simpler one.
- **`strum` is already a `reify-ir` dep** (`crates/reify-ir/Cargo.toml:14`). `Operation` and the kind enums already derive `strum::EnumIter`. **`GeometryOp` does not and cannot** derive a parameterless `EnumIter` (all 48 variants carry fields) — the fix is `#[derive(strum::EnumDiscriminants)]`, minting a fieldless `GeometryOpDiscriminants` that **can** derive `EnumIter`/`EnumCount`. This is the linchpin for the completeness test (see G6); no new dependency required.
- **Reify has no wasm/emscripten build target** (no `.github/workflows`, no `wasm32` toolchain target, no `cdylib`/`wasm-pack`; the `wasm32` mentions in `.cargo/config.toml`/`Cargo.lock` are forward-looking guards / unbuilt transitive cfg-gated deps). The brief's "viable under wasm/emscripten" concern is therefore **non-binding today**. Flagged in Open Questions for the day a wasm GUI target lands.

## 3. Sketch of approach — static descriptor table + layered behavioral table

**Mechanism (decided):** a plain `static` table à la the `a307bf98` / `NAMED_DIMENSIONS` precedent (`crates/reify-core/src/dimension.rs:479`) — no life-before-main, append-only, merges cleanly. Adding an op = append one row. (The `inventory` alternative was considered and rejected: its decentralised-registration advantage doesn't apply because all 48 variants are defined centrally in one enum in one crate, and it adds dead-strip/link-anchor discipline for no gain. The irreducible enum-edit lands in `reify-ir/src/geometry.rs` regardless, so the table row can sit adjacent.)

```rust
// crates/reify-ir/src/geometry.rs
#[derive(Debug, Clone, strum::EnumDiscriminants)]
#[strum_discriminants(name(GeometryOpDiscriminants),
                      derive(strum::EnumIter, strum::EnumCount, Hash))]
pub enum GeometryOp { /* … 48 variants, unchanged … */ }

pub enum ParentRole { None, Pair, SingleTarget, SingleProfile, VariadicProfiles, /*…*/ TopologySelector }

pub struct OpDescriptor {
    pub disc:       GeometryOpDiscriminants,
    pub operation:  Operation,           // Axis 1: geometry_op_to_operation
    pub parent_role: ParentRole,         // Axis 1: parent_handles_for_op / substitute_op_parents
    pub kind_token: &'static str,        // Axis 1: kind_name()
    pub names:      &'static [&'static str], // Axis 2: which surface fn-names dispatch to this op
}

pub static GEOMETRY_OP_DESCRIPTORS: &[OpDescriptor] = &[
    OpDescriptor { disc: GeometryOpDiscriminants::Box, operation: Operation::PrimitiveBox,
                   parent_role: ParentRole::None, kind_token: "Box", names: &["box", "box_centered"] },
    // … one row per variant …
];

pub fn descriptor_for(d: GeometryOpDiscriminants) -> Option<&'static OpDescriptor> {
    GEOMETRY_OP_DESCRIPTORS.iter().find(|r| r.disc == d)
}
```

The three Axis-1 functions collapse to `descriptor_for(op.into()).operation` / `.parent_role` + a thin field-projection shim (the *classification* is data; reading the actual `GeometryHandleId`s out of the matched fields is an irreducible projection — kept as one small `match` that returns handles by role, not per-variant facts). Axis 2's four lists are **derived** from the `names` columns; `EXPECTED_DISPATCH_COUNT` retires into a table-driven completeness test. Axis 3 gets a **separate** fn-table **in `reify-eval`** keyed by `CompiledGeometryOp` kind (different abstraction layer — 8 variants × nested kinds, not the 48-row `GeometryOp` table; do not try to unify keys).

**Exhaustiveness safety (the G6 contract — see §6).** The compile-time "every variant handled" guarantee that the exhaustive `match` gave for free is **replaced by a CI completeness test**, exactly as the precedent did: iterate `GeometryOpDiscriminants::iter()` (and the kind enums' `::iter()`), assert each resolves to exactly one descriptor / registered fn. This is panic-free — `descriptor_for` returns `Option`, the production miss-path emits a diagnostic, and the completeness test guarantees no miss reaches production. **`Split`** (the topology-selector pseudo-op currently handled by `unreachable!`) becomes a `ParentRole::TopologySelector` descriptor flag, not a panic.

## 4. Pre-conditions

- `strum` available in `reify-ir` — **confirmed** (`Cargo.toml:14`). Adding `EnumDiscriminants`/`EnumCount` needs no new dep.
- `Operation` and `GeometryOp` co-located in `reify-ir` — **confirmed** (`geometry.rs:262`, `:539`).
- The Axis-3 arms are Engine-free — **confirmed** (header `geometry_ops.rs:1–4`); they thread `diagnostics` explicitly and call only `eval_expr` + reify-eval-private helpers, so each arm can become a registered `fn(ctx) -> Result<GeometryOp, String>`.
- No new `.ri` grammar/syntax — this is pure Rust internals; the **grammar gate is N/A** and the `.ri` decompose-verify workflow does not apply (mirrors the cpu-load-admission / warm-lane PRDs — host checks, not `.ri` probes). G3 was satisfied by direct host inspection above.

## 5. Resolved design decisions

- **DD-1 (mechanism):** static `&[OpDescriptor]` table in `reify-ir`, keyed by `GeometryOpDiscriminants`. *Not* `inventory`. (Precedent: `a307bf98`.)
- **DD-2 (scope):** **Full — all three axes** plus canary retirement. Axis 3 (the 1435-line behavioral fn) is in scope but gated behind a characterization-test prerequisite (DD-4).
- **DD-3 (exhaustiveness):** compile-time exhaustive-match guarantee → **CI completeness test** over `GeometryOpDiscriminants::iter()` and the kind enums. Net-strengthening (catches name-mismatch + over-match too, per the precedent's lesson). **No compile-time→runtime-panic trade**: lookup miss returns a typed diagnostic; the test forbids misses.
- **DD-4 (Axis-3 safety — Leo's mandate):** build an **exhaustive characterization/golden suite for `compile_geometry_op` BEFORE refactoring it**. Snapshots the produced `reify_ir::GeometryOp` + diagnostics for every `CompiledGeometryOp` variant × nested kind on the *current* code; the behavioral refactor must keep it byte-identical green. This is the H two-way boundary test for the highest-risk leaf.
- **DD-5 (canary ownership — G4):** this PRD **owns** retirement of `GEOMETRY_OP_VARIANT_COUNT` (`reify-ir`) and `EXPECTED_DISPATCH_COUNT` (`reify-compiler`); both fold into the completeness test (a stronger invariant). `GEOMETRY_QUERY_VARIANT_COUNT` is **left untouched** (queries are out of scope — §7).
- **DD-6 (field-projection residue):** `parent_handles_for_op` / `substitute_op_parents` keep a small role-keyed read/write shim (a `match` over `ParentRole`, ~5 arms, not per-variant) because reading/writing the actual handle fields is a projection, not data. This shim is **O(roles)** not **O(variants)**, so it never collides on a new op.

## 6. Premise validity (G6)

The premise — *registry-izing dispatch is feasible without losing exhaustiveness safety* — is validated:

- **Precedent proof.** `a307bf98` performed exactly this trade in-repo (33 dimension arms → `NAMED_DIMENSIONS` table + `resolve_type_name_round_trips_all_named_dimensions` CI test) and it has absorbed 17+ append-only additions since with the parity test still green. Reify's accepted substitute for compile-time exhaustiveness is a table-iterating CI test.
- **Iterator linchpin.** The completeness test is only *possible* because `strum::EnumDiscriminants` mints a fieldless `GeometryOpDiscriminants` to iterate. Without it, you could not enumerate the 48 field-carrying variants and would fall back to a runtime panic — the exact failure the brief forbids. The derive is in scope in L1 and uses an existing dep. (Producible from the task's own dependency set — confirmed.)
- **No numeric premises.** This is a structural refactor, not numerical; G6 branches 1/2 (numeric bound / closed-form exactness) do not fire. No FEA/spline/eigensolver hazard applies.
- **Rejection-mechanism-backed.** The "adding an op the wrong way is rejected" claim is backed by an *active* mechanism: the completeness test goes RED. Not an aspirational assertion.

## 7. Out of scope

- **`GeometryQuery` dispatch** (28 variants, `GEOMETRY_QUERY_VARIANT_COUNT`). Queries are a **parallel, separate** dispatch family — adding a geometry *op* does not touch the query canary, and vice-versa. The descriptor-table pattern generalises to queries 1:1, so this is a clean **follow-up PRD**, not part of "geometry-*op* dispatch." `GEOMETRY_QUERY_VARIANT_COUNT` stays as-is.
- **Any change to runtime behavior.** This is a behavior-preserving refactor; the equivalence corpus is the acceptance bar.
- **Narrowing the over-greedy directory locks** — handled separately (the other half of the jun18 contention fix).
- **New geometry ops themselves** (#3963, #4195) — they consume this work (§8), they are not part of it.

## 8. Cross-PRD relationship & seam ownership (G4)

| Seam | Other party | Status | Resolution |
|---|---|---|---|
| `reify-ir/src/geometry.rs` `GeometryOp` enum + `*_VARIANT_COUNT` canary | **#3963** (`GeometryOp::AffineApply`), **#4195** (`extrude_to` / `ExtrudeTo`) — both **pending**, both add a new variant | Direct colliders on the enum this refactor restructures | **Registry lands first; #3963/#4195 rebase to append-only registration** and become the live G2 "touches one site" demonstration. Decompose wires `#3963 → L6` and `#4195 → L6` (they depend on the integration gate). They are *pending*, not in-flight, so no live conflict. |
| `reify-ir` crate (not the file) — "data-carrying enums" #3940 batch (`EnumVariantDef`/`Value::Enum`) | #3940/#3942/#3944/#3946 — pending | **False collision** — touches `traits.rs`/`value.rs`/`expr.rs`, **not** `geometry.rs`. No file-level conflict; only a crate-level serialization if a `reify-ir/`-dir lock is taken (which is itself over-greedy — narrow file locks suffice). No edge needed. |
| Prior de-dup precedents | #3191 (`kind_name` → shared Display), #1927 (tightened `all_dispatch_functions_accounted_for`) — both **done** | Same-spirit prior art | Cite as precedent; not blocking. |

No new contested-ownership pair is introduced (the three existing ones in the overlay G4 list are untouched).

## 9. Decomposition plan (one bullet per leaf → its observable signal)

DAG: **L1 → {L2, L3}**; **L4 → L5**; **{L2, L3, L5} → L6**. Roots **L1** and **L4** parallelize. Out-of-batch: **#3963 → L6**, **#4195 → L6**.

- **L1 — descriptor-table foundation (`reify-ir`).** Add `strum::EnumDiscriminants`/`EnumCount` to `GeometryOp` → `GeometryOpDiscriminants`; define `OpDescriptor` + `ParentRole`; populate `GEOMETRY_OP_DESCRIPTORS` (48 rows); add `descriptor_for()`; re-drive `kind_name()` from `kind_token`; retire `GEOMETRY_OP_VARIANT_COUNT` into a completeness test.
  - **Signal:** `cargo test -p reify-ir` green; new completeness test enumerates `GeometryOpDiscriminants::iter()` and asserts exactly one descriptor per discriminant (RED if a variant is unregistered); the existing `kind_name` stable-token test stays byte-identical green (equivalence).
  - **Consumer:** L2, L3 (and transitively L6). **grammar_confirmed:** N/A (no `.ri` syntax).
- **L2 — Axis-1 consumer (`reify-eval/engine_build.rs`).** Rewrite `geometry_op_to_operation`, `parent_handles_for_op`, `substitute_op_parents` as `descriptor_for()` lookups + the DD-6 role-keyed projection shim; delete the three exhaustive matches.
  - **Signal:** existing per-variant tests (`geometry_op_to_operation_maps_every_variant_family`, `parent_handles_for_op_returns_expected_handles_per_variant_family`) stay green (behavioral equivalence over all 48 real variants); a guard test asserts no `GeometryOp::` per-variant arm remains in those three fns; `reify eval` on an existing `.ri` fixture yields an identical handle graph.
  - **Dep:** L1.
- **L3 — Axis-2 canary retirement (`reify-compiler/geometry.rs`).** Derive `GEOM_ARG_FUNCTIONS`/`NO_GEOM_ARG_FUNCTIONS`/`BOOLEAN_OP_FUNCTIONS`/`LOFT_FUNCTIONS` from the descriptor `names` columns; retire `EXPECTED_DISPATCH_COUNT` into a table-driven completeness check; keep the `_ =>` "unsupported geometry function" wildcard for genuinely-unknown names.
  - **Signal:** the `all_dispatch_functions_accounted_for` invariant is preserved as a table-derived check (every dispatched name maps to exactly one descriptor; every descriptor name dispatches); `reify check` over a `.ri` exercising each function name resolves identically; RED if a name list and the table drift.
  - **Dep:** L1. **Tactical note:** the 56 surface names do not map 1:1 to the 48 variants (`box`/`box_centered`→Box; `union_all`→Union; `revolve`/`revolve_full`; `fillet`/`fillet_all`). The `names` column is `&[&str]` (many names per variant); a handful of names that construct via a builder rather than a bare variant may stay as explicit compiler arms — that is fine (Axis 2's goal is retiring the *canary + lists*, not the arg-parsing arms, which already have a wildcard). Resolve the exact name→variant grouping at impl time.
- **L4 — characterization harness for `compile_geometry_op` (`reify-eval`, test-only) [DD-4 prerequisite].** Golden/snapshot suite covering every `CompiledGeometryOp` variant × nested kind with representative args, asserting the produced `reify_ir::GeometryOp` + diagnostics on the **current** code.
  - **Signal:** new `compile_geometry_op_characterization` module green on unrefactored code; a coverage assertion confirms every `CompiledGeometryOp` variant and every nested kind is exercised (drives off the kind enums' `strum::EnumIter`).
  - **Dep:** none (root). **Consumer:** L5.
- **L5 — Axis-3 behavioral refactor (`reify-eval/geometry_ops.rs`).** Restructure `compile_geometry_op` into a `reify-eval`-local fn-table keyed by `CompiledGeometryOp` kind; each former arm becomes a registered `fn(ctx) -> Result<GeometryOp, String>`; add a completeness test over the kind enums; remove the nested per-kind `match` arms.
  - **Signal:** L4's characterization suite stays **byte-identical green** (the equivalence proof); a completeness test asserts every kind has a registered fn; a guard test asserts no nested per-kind `match` with behavioral arms remains; `reify eval`/`reify check` corpus identical.
  - **Dep:** L4. (Independent of L1 — different table, different crate, different key.)
- **L6 — integration gate (cross-crate) [C-as-integration-gate].** Repo-wide guard asserting no central exhaustive dispatch `match` over `GeometryOp` remains; full `--scope all` verify green; end-to-end `.ri` example-corpus run producing identical output vs pre-refactor; confirm both canaries retired and the completeness tests are the live guarantee.
  - **Signal:** the repo-wide guard test passes; `scripts/verify.sh --scope all` green; the `.ri` corpus diff is empty (behavioral equivalence end-to-end).
  - **Deps:** L2, L3, L5 (transitively L1, L4). **Out-of-batch dependents:** #3963, #4195.

## 10. Open (tactical) questions

- Exact name→variant grouping for the Axis-2 `names` columns (the non-1:1 cases above) — resolved at L3 impl time by reading `compile_geometry_call`.
- Whether the Axis-3 fn-table is keyed by `CompiledGeometryOp` discriminant alone or `(variant, kind)` tuple — depends on how the nested kinds factor; L5 impl decision, pinned by L4's coverage map.
- Whether `kind_name()` re-drive should keep a thin `match` for any token that is *not* a simple variant-name string — check at L1 against the current `kind_name` arms.
- **wasm/emscripten:** non-binding today (no such target). If a wasm GUI target is ever added, re-validate that the static-table approach (which has no life-before-main) is trivially fine — it is; only an `inventory`-based alternative would have needed wasm dead-strip re-validation, and we chose the static table partly for that reason.

## 11. Meta-gate

If decomposed and queued without further oversight, the architecture is complete (every axis + canary covered), coherent (one static table + one layered behavioral table at the correct crate layers), cohesive (a single descriptor as source of truth feeding all Axis-1/2 sites), and good (behavior-preserving, with the exhaustiveness guarantee strengthened, not weakened, and the highest-risk leaf gated behind a characterization harness per DD-4). **META: pass.**
