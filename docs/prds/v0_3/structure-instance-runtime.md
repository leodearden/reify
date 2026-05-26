# Structure-Instance Runtime

Status: contract (operationalizes the 2026-05-12 GR-001 Option B resolution). Authored 2026-05-12 in interactive session. Approved by Leo before queueing tasks.

Resolves clusters C-01 / C-08 / C-16 / C-29 and gaps GR-001 / GR-011 / GR-019 / GR-031 per `docs/architecture-audit/gap-register.md`.

## §0 — Purpose and supersession

This document is the **contract** for runtime evaluation of structure constructors. Today, `Steel_AISI_1045()` → `Value::Undef` because `reify_stdlib::eval_builtin` returns Undef for unknown names (the ctor parses, types, lowers — but the eval step has no producer for a struct-instance value). The contract introduces a typed `Value::StructureInstance` variant, the workspace-wide adapter sweep that admits it, the compile-lowering path from `StructureName(...)` to the new variant, the persistent cache key composition, and the first vertical slice of stdlib `structure def` rewrites that replace the current Rust-side builtin-dispatch entries (`point_load`, `fixed_support`, the `Steel_AISI_1045()` builtin-Undef fallthrough).

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (`preferences_implementation_chain_naming`) — is what this contract is designed to prevent for the structure-instance seam specifically. Resolution mode is **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline. The full-workspace `match Value` adapter sweep is high-priority wide-lock per `feedback_orchestrator_narrow_locks_favor_upfront_design`.

This document is named in `docs/architecture-audit/gap-register.md` GR-001's `### Follow-up PRD` sub-section and is the resolution mechanism for GR-011 (cluster C-08 Load/Support type system), GR-019 (cluster C-16 Material starter library), and GR-031 (cluster C-29 composed stress recovery).

## §1 — What is settled

Per GR-001 resolution 2026-05-12: **Option B — typed Value variant, nominal conformance everywhere.**

- `Value::StructureInstance { type_id: StructureTypeId, fields: PersistentMap<String, Value> }` is added.
- Struct constructors lower to this variant.
- Trait conformance stays strictly nominal. `structure def Foo : TraitName { ... }` declares the bound; `entity::satisfies_trait_bound` consults declared bounds; structural-shape admission is NOT introduced.
- Existing Rust-side builtin-dispatch entry points (`point_load`, `fixed_support`, etc.) are rewritten as stdlib `.ri` `structure def` declarations producing `Value::StructureInstance`.
- `Value::Map` continues to exist for genuinely-map-shaped data (multi-case results `Map<String, ElasticResult>`, dictionary config).
- snake_case ctors consolidate on PascalCase (`point_load` → `PointLoad`).
- Hybrid-1 (typed-only structural admission) is deferred per GR-001 §"Resolution"; B → hybrid-1 is additive if friction surfaces.

Full rationale recorded in `docs/architecture-audit/gap-register.md` GR-001 §"Resolution" — do not re-open here.

The ComputeNode contract (`docs/prds/v0_3/compute-node-contract.md` §2 `ComputeFn` signature, §1 GR-001 link) already anticipates `Value::StructureInstance` arms in its trampoline `&Value` inputs (`options`, `value_inputs`). This PRD produces the variant; the ComputeNode contract consumes it. The cross-PRD ordering is: this PRD's foundation slice (§8 Wave 1) lands before compute-node-contract.md §8 task η (FEA first real consumer).

## §2 — `Value::StructureInstance` shape

**Variant.** Added to `Value` (definition at `crates/reify-types/src/value.rs:294`):

```rust
pub enum Value {
    // ... existing variants ...
    StructureInstance {
        type_id: StructureTypeId,
        fields: PersistentMap<String, Value>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructureTypeId(pub u32);
```

`StructureTypeId` is an opaque per-Engine interned u32 (Q-SIR-1: smallest in-memory footprint; tightest-now mirrors GR-001's overall posture). Everything else (declared trait bounds, source location, field layout, version) lives in a side-table `StructureRegistry` keyed by id.

**Side-table.** `StructureRegistry`, held by `Engine`:

```rust
pub struct StructureRegistry {
    by_id: Vec<StructureMeta>,
    by_name: HashMap<String, StructureTypeId>,
}

pub struct StructureMeta {
    pub name: String,                  // canonical PascalCase
    pub version: u32,                  // from @version(N) annotation; default 1
    pub declared_trait_bounds: Vec<TraitName>,
    pub source: Option<SourceSpan>,
    pub field_layout: Vec<(String, Type)>,
}
```

**Why opaque + side-table.** A `StructureInstance` Value carries 4 + sizeof(PersistentMap) bytes for the type_id + fields. Side-table lookups happen at the boundaries that need them (conformance check at `value_type_kind_matches`, cache-key composition, diagnostics) — none of which are inner-loop hot. Hybrid-1 retrofit (carrying declared bounds in-line for fast conformance) is additive if profiling justifies it; not now.

**Version. `@version(N)` annotation** on the `structure def` declaration is the surface; default 1 if absent. Verified to parse (Q-SIR-3 follow-up: `@version(2) structure def Foo : Trait { ... }` parses via the existing tree-sitter `@annotation` grammar). The compiler reads the annotation during structure_def lowering and populates `StructureMeta.version`. Annotation-recognition is additive in the annotation-args PRD's scope; this PRD declares the read-side contract.

**Registry lifecycle.** `StructureRegistry` is populated when stdlib `.ri` is compiled (per-Engine). Cleared on Engine drop. Id-stability across Engine restarts is **not** guaranteed — the per-Engine u32 is purely a fast indirection. All cross-Engine identity flows through the structure name (the cache-key composition in §5 uses the name, not the id).

## §3 — Workspace adapter sweep

**Scope.** Every `match value` / `match Value::` site in the workspace adds a `Value::StructureInstance { .. }` arm. Survey (2026-05-12 grep): 13 distinct files contain `match Value::` patterns:

```
crates/reify-types/src/value.rs                  (variant + traits)
crates/reify-eval/src/engine_eval.rs             (eval-path matches)
crates/reify-eval/src/geometry_ops.rs
crates/reify-expr/src/lib.rs                     (incl. value_type_kind_matches at lib.rs:195)
crates/reify-stdlib/src/fea.rs
crates/reify-stdlib/src/geometry.rs
crates/reify-stdlib/src/joints.rs
crates/reify-stdlib/src/loop_closure.rs
crates/reify-stdlib/src/loop_closure_solver.rs
crates/reify-stdlib/src/mechanism.rs
crates/reify-stdlib/src/snapshot.rs
crates/reify-stdlib/src/supports.rs
crates/reify-stdlib/src/sweep.rs
```

Plus: cache adapter at `crates/reify-eval/src/cache.rs` and `persistent_cache.rs`. Plus: significance_filter.rs and freshness_walk.rs (per ComputeNode contract overlap). Final list assembled by the foundation-slice task agent against the wide-lock charter.

**Sweep rollout.** Single **wide-lock task, high priority** (Q-SIR-2, per `feedback_orchestrator_narrow_locks_favor_upfront_design`: high priority = "tends to land reasonably soon, at some throughput cost"). Critical was offered; high was chosen — broad refactor without urgency. The task's `metadata.files` enumerates all of the above and is the orchestrator's lock charter.

**Per-arm policy.** Each match site gets one of three arms depending on context:

1. **Behaves-like-Map sites** (anywhere a Value's "kind tag" was previously read from `Value::Map`): the `Value::StructureInstance { type_id, fields }` arm reads the structure name via `registry.name_for(type_id)` and dispatches the same way the kind-tag-read did. Adapter helper `value_kind_or_structure_name(&Value) -> Option<&str>` introduced for the common case.
2. **Conformance-check sites** (typed-slot admission): new arm consults `registry.declared_bounds(type_id)` for trait satisfaction. See §4.
3. **Reject sites** (variants the consumer never expects): clean diagnostic + Undef, same as today's default arm for unfamiliar variants.

**`PersistentMap<String, Value>` choice.** `im::HashMap<String, Value>` (already used elsewhere in reify-eval — verify and reuse the same type alias). Field mutations are rare; structural sharing across edits matters for the Engine's snapshot clone discipline (per `graph.rs` Clone-drops-warm-state precedent — cheap clones, deep ownership via the side-table indirection).

## §4 — Compile-lowering: struct-ctor → `Value::StructureInstance`

**Current state.** `StructureName(...)` parses (task 2039, done) and resolves through type-checking. At eval time, `engine_eval.rs` falls through to `reify_stdlib::eval_builtin`, which returns `Value::Undef` for unknown names (`reify-stdlib/src/lib.rs:44`). Comment at the value-cell-types-invariant site (`engine_eval.rs:114-125` — actually the `is_representable_cell_type` helper) explicitly names the future fix: "If a future `Value::TraitObjectInstance` or `Value::StructureInstance` variant is added, add a matching arm in `value_type_kind_matches`."

**Lowering rule.** When a function-call expression's resolved callee is a `StructureDef`, the eval path constructs a `Value::StructureInstance { type_id, fields }`:

- `type_id = registry.id_for(structure_name)` — interned at registry-load time.
- `fields` is built by evaluating each parameter's expression in declaration order: positional args bind by position; named-arg syntax (`StructureName(field: expr, ...)`) binds by name; absent fields use the structure_def's declared default expression. Field-name order is **declaration order** at construction time; sort order at cache-key composition only (§5).
- Conformance is proven at compile time (§5 below). The eval path does not re-check at construction; the construction always succeeds (modulo per-field eval failures that propagate as Undef per existing rules).

**Where the lowering lives.** Two cleanly-separated sites:

- `engine_eval.rs` `try_eval_function_call` (or the equivalent) gains a precedence check: if the resolved callee is a `StructureDef`, route to a new `eval_structure_instance_ctor(structure_id, args, ...)`. Today this site falls through to `eval_builtin`; the new path takes precedence.
- `reify-stdlib::eval_builtin` retains its current shape; it no longer intercepts structure ctors. Existing builtin-dispatch entries for `point_load` / `fixed_support` are retired as part of §6.

**Conformance — compile-time only + debug-build invariant** (Q-SIR-4). `structure def Foo : ElasticMaterial { ... }` declares the bound at compile time; the compiler proves `Foo` conforms to `ElasticMaterial` (existing trait-typed-param machinery; task 2227's `List<TraitObject>` admission applies transitively). At runtime, `value_type_kind_matches` (lib.rs:195) gains the new arm:

```rust
Value::StructureInstance { type_id, .. } => match ty {
    Type::StructureRef(name) => {
        registry.name_for(*type_id).map(|n| n == name).unwrap_or(false)
    }
    Type::TraitObject(bound) => {
        registry.declared_bounds(*type_id)
            .map(|bs| bs.contains(bound))
            .unwrap_or(false)
    }
    _ => false,
}
```

`entity::satisfies_trait_bound` is unchanged for the compile-time path. A debug-build invariant (`cfg(debug_assertions)`) asserts no `Value::StructureInstance` reaches a typed slot whose `Type::StructureRef` / `Type::TraitObject` doesn't match — net for stdlib bugs that would otherwise silently corrupt the type system.

**Default-value evaluation.** Structure_def field defaults that are themselves struct-ctor calls (`param material : ElasticMaterial = Steel_AISI_1045()`) recurse through the same lowering. Fixture verified to parse (Q-SIR-7 example file).

## §5 — Persistent cache key composition

**Key fragment.** A `Value::StructureInstance` serializes for cache-key purposes as the tuple

```
("si", structure_name: &str, version: u32, fields_hash: [u8; 32])
```

where:

- `"si"` is the variant discriminator (analogous to existing per-variant tags in `cache.rs` / `persistent_cache.rs`).
- `structure_name` is the canonical PascalCase name from `StructureMeta.name`. Names are stable across Engine restarts; per-Engine `StructureTypeId` u32s are **not** included (Q-SIR-3: name + version, not id; option C from the menu rejected on those grounds).
- `version` is `StructureMeta.version` (1 unless `@version(N)` annotation overrides). Bumping a structure's version invalidates its persistent cache entries (stdlib field-semantics migration discipline).
- `fields_hash = blake3(sort_by_key(fields).map(|(k, v)| (k, cache_key_of(v))).concat())`. Sort alphabetically by field key for determinism across iteration order. Field-key collision is impossible (PersistentMap is keyed by String). `cache_key_of(v)` recurses through `Value::StructureInstance` values uniformly (e.g. nested `Beam { material: Steel_AISI_1045() }`).

**Invariants.**

- Stable across Engine restarts: yes (structure_name + version, both stable).
- Invariant to field declaration order at the source: yes (sort by key).
- Invalidated by adding/removing/renaming a field: yes (transitively, via `fields_hash` set change).
- Invalidated by a field-type change: yes (transitively, via `cache_key_of(v)`).
- Invalidated by a field-value-semantics change (same field name, same type, different meaning): no — requires explicit `@version` bump in the structure_def. **This is the migration knob.**

**Sites.** Cache-key composition lives at `crates/reify-eval/src/cache.rs` (in-memory cache hashing) and `crates/reify-eval/src/persistent_cache.rs` (on-disk key serialization). Both grow a `Value::StructureInstance` arm in their existing `value_to_cache_key` (or equivalent) function. The arm reads `name` and `version` via the engine's `StructureRegistry`; arm receives a `&StructureRegistry` parameter or accesses via the existing engine context.

**Engine-version-hash interaction.** The existing persistent-cache `ENGINE_VERSION_HASH` (task 2970, done) bounds the overall cache namespace per build of `reify`. Structure rename/field-shape changes that ride a version-hash bump are doubly-invalidated (engine-version + name/fields_hash); intra-version-hash structure_def edits are invalidated by name/fields_hash alone. The two layers are orthogonal.

## §6 — Stdlib structure_def rewrite (first vertical slice)

**Vertical-slice contents** (Q-SIR-5 + Q-SIR-6). Wave 1 rewrites **three** builtin-dispatch ctors — one per cluster sub-shape — as `.ri` `structure def` declarations:

| Rewrite | Cluster | Current (Rust-side) | New (stdlib `.ri`) |
|---|---|---|---|
| `Steel_AISI_1045()` | C-16 / GR-019 | `eval_builtin` falls through → `Value::Undef` (structure def exists at `materials_fea.ri:132` but unreachable) | The existing `structure def Steel_AISI_1045 : ElasticMaterial { ... }` at `materials_fea.ri:132` becomes evaluable through §4's lowering path; no stdlib source change. |
| `PointLoad(...)` (rename from `point_load(...)`) | C-08 / GR-011 | `reify-stdlib/src/loads.rs:33,68+` (builtin dispatch producing kind-tagged `Value::Map`) | `trait Load { ... }` + `structure def PointLoad : Load { ... }` added to `fea_multi_case.ri` (or new file); Rust builtin retired. |
| `FixedSupport(...)` | C-08 / GR-011 | `reify-stdlib/src/supports.rs:59,100+` (builtin dispatch producing kind-tagged `Value::Map`) | `trait Support { ... }` + `structure def FixedSupport : Support { ... }` added to stdlib; Rust builtin retired. |

**Trait declarations.** `trait Load { ... }` and `trait Support { ... }` are added in stdlib in the same wave. These declarations are what makes the rewrite cluster GR-011's Load/Support nominal-trait surface real. Trait member parameters (e.g. `Load`'s `magnitude : Force`, `direction : Vector3`) are designed in the wave-1 task in concert with the existing Rust-side fields under `loads.rs` / `supports.rs` — preserving the field shape current downstream consumers depend on.

**Naming consolidation.** `point_load` → `PointLoad`, `pressure_load` → `PressureLoad`, `fixed_support` → `FixedSupport` (already PascalCase), `pinned_support` → `PinnedSupport` (already PascalCase). The wave-1 rewrite changes the snake_case form for the two ctors it touches (`point_load`, `fixed_support`) — both are stdlib-internal references; user-facing examples and the audit `findings/` corpus are checked for snake_case uses and updated in-task. Wave-2 (SIR-β-load, task 3544) completed the `pressure_load` → `PressureLoad` consolidation; `pinned_support` remains for SIR-β-sup.

**Wave 2 — remaining rewrites.** Each remaining ctor is its own follow-up task (or small batch) once the foundation slice has landed:

- Remaining materials: `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic` (already declared in `materials_fea.ri`; rewrite is purely the lowering becoming live).
- Remaining loads: `PressureLoad` (snake → Pascal + structure_def).
- Remaining supports: `PinnedSupport` (already PascalCase + structure_def).
- Other rewrites surfaced during foundation-slice work (e.g. `LoadCase`, `MultiCaseResult` from multi-load-case-fea PRD; `Provenance`-like records).

Each wave-2 task closes on a per-ctor user-observable signal — `.ri` example evaluates the ctor to a non-Undef inspectable value.

## §7 — Boundary test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level integration) and `crates/reify-types/src/value.rs::tests` + `crates/reify-eval/src/lib.rs::tests` (unit, per-module). The seam is between `reify-types` (Value variant + registry), `reify-eval` (lowering + adapters + cache), `reify-stdlib` (builtin retirement + stdlib trait declarations), and `reify-compiler` (structure_def + @version recognition).

### 7.1 Producer-side (the variant + adapter + lowering machinery looks outward at consumers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Variant construction round-trip.** Construct `Value::StructureInstance { type_id, fields }` via Rust API; pass through Clone, equality, hash. | `Value::StructureInstance` variant exists; `PersistentMap` field type chosen. | Clone preserves structural sharing; PartialEq compares (type_id, fields) deeply; Hash is consistent with PartialEq. Unit test pins. |
| **Adapter sweep coverage.** Every `match value` / `match Value::` site in the workspace has a `Value::StructureInstance` arm. | Adapter sweep task done. | `cargo check --workspace` green. CI `match-exhaustiveness` lint (added in the foundation task, or relied on via rustc's existing exhaustiveness) flags any future un-adapted site. |
| **`value_type_kind_matches` arm.** `Value::StructureInstance` against `Type::StructureRef(name)` returns true iff `registry.name_for(type_id) == name`. Against `Type::TraitObject(bound)` returns true iff `bound ∈ registry.declared_bounds(type_id)`. Against unrelated `Type` variants returns false. | Variant + registry shipped. | Unit test in `crates/reify-expr/src/lib.rs::tests` (alongside existing `value_type_kind_matches_*` tests). |
| **Debug invariant fires.** Construct a `Value::StructureInstance` whose declared bounds don't include `Material`; place it into a `param material : Material` cell. | Debug-build invariant active. | Test triggers the assertion; release build is silent. |
| **Cache-key serialization.** Two `Value::StructureInstance` values with same name, same version, same fields serialize to the same cache key; different field values produce different keys; different name produces different keys; different version produces different keys; field-declaration-order at the source does not affect the key (sorted-by-key invariant). | Cache-key arm shipped. | Unit test in `crates/reify-eval/src/cache.rs::tests`. |
| **Engine-restart cache-key stability.** Cache key for `Steel_AISI_1045()` recovered after Engine drop + re-create matches the pre-drop key. | Cache-key uses name + version, not `StructureTypeId`. | Engine integration test asserts cache hit across restart. |

### 7.2 Consumer-side (the FEA/Load/Support/cache trampoline-providing crates look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **`.ri` ctor evaluates to inspectable Value.** `examples/structure-instance.ri` (per Q-SIR-7): `Steel_AISI_1045()` + `PointLoad(...)` + `FixedSupport(...)` constructed; member access reads (`material.youngs_modulus`). | Wave 1 vertical slice done. | `reify eval examples/structure-instance.ri` prints non-Undef structure-shaped Value; CLI golden output captured. |
| **Trait-typed param admission.** `.ri` fixture: `structure def Beam { param mat : ElasticMaterial = Steel_AISI_1045() }`. | Wave 1 done. | `Beam` constructs without diagnostic; `beam.mat.youngs_modulus` reads through to the inner StructureInstance's field. |
| **Nested compositional structure access.** `examples/structure-instance.ri` extended (per Q-SIR-7 add-on): `structure def Assembly { param primary : Beam = Beam(); param secondary : Beam = Beam(length: 2.0m) }` where `Beam` carries `loads : List<Load> = [PointLoad(...), PointLoad(...)]` and `supports : List<Support> = [FixedSupport()]`. Member-access chain `assembly.primary.material.youngs_modulus` evaluates through nested StructureInstance values. | Wave 1 done; task 2227 `List<TraitObject>` admission already wired. | Member-access chain reads through (`StructureInstance → StructureInstance → field`); list-of-typed-loads admits `PointLoad : Load` per nominal conformance; `reify eval` prints the nested values inspectably. |
| **Linguistic Map-vs-Structure distinction.** `.ri` fixture: `param cases : Map<String, ElasticResult>` (existing multi-load-case shape) coexists in the same file with `param material : ElasticMaterial = Steel_AISI_1045()`. The two stay distinguishable at the Value layer (Value::Map vs Value::StructureInstance) per GR-001 §Resolution's "the two shapes are linguistically distinguishable" tenet. | Wave 1 done; multi-load-case-fea PRD's Map-shaped result form unchanged. | Pattern-match against the values discriminates correctly; both shapes round-trip through cache without conflation. |
| **Nominal conformance enforcement.** `.ri` fixture: `structure def Beam { param mat : ElasticMaterial = PointLoad(magnitude: 1.0) }` — `PointLoad : Load`, not `: ElasticMaterial`. | Wave 1 done. | Compile-time error `E_TRAIT_BOUND_NOT_SATISFIED` (existing diagnostic). Negative-path test. |
| **Persistent cache round-trip.** `.ri` fixture using `Steel_AISI_1045()` evaluates; engine exits; engine restarts; same file re-evaluated. | Wave 1 + persistent_cache.rs arm done. | First evaluation populates persistent cache; second evaluation reads back identical Value (no re-evaluation; verified via instrumentation). |
| **Cache invalidation on `@version` bump.** Same fixture; bump `@version` on `Steel_AISI_1045` from 1 → 2; restart. | Cache-key fragment includes version. | Cache miss on restart; fresh eval; old key garbage-collected by existing cache hygiene. |
| **GR-011 Load/Support nominal traits live.** `.ri` fixture: `param loads : List<Load> = [PointLoad(...), PointLoad(...)]`. | Wave 1 done (trait Load declared, PointLoad : Load). | List admits both; `loads[0].magnitude` reads through. |
| **GR-019 Material starter library reachable.** `.ri` fixture exercises `Steel_AISI_1045()`, `Aluminium_6061_T6()` (wave 2), `Titanium_Ti6Al4V()` (wave 2), `ABS_Plastic()` (wave 2). | Wave 1 (Steel) and wave 2 (rest) done. | All four ctors evaluate to non-Undef. Per-material unit test in the wave-2 follow-up tasks. |
| **GR-031 composed-stress envelope.** `.ri` fixture: `linear_combine(...)` produces a typed envelope; `envelope.max_von_mises` reads as Real. | Wave 1 done + task 3468 (already filed; depends on this PRD per GR-031 disposition). | Envelope helpers compile against the new variant; per-field reads work. |
| **ComputeNode trampoline arm.** Synthetic trampoline registered for `test::echo_material`; argument is `Steel_AISI_1045()` (which evaluates to `Value::StructureInstance`); trampoline unpacks `args[0]` and returns a field's value. | Wave 1 done + compute-node-contract.md §8 task γ done. | The trampoline arm accepts `Value::StructureInstance` per `compute-node-contract.md §2` `ComputeFn` signature; the per-target unpack-and-dispatch contract lives in each consumer crate, not in this PRD's adapter. |

## §8 — Decomposition DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal**. Producer-only tasks closed in isolation are not tolerable per `feedback_task_chain_user_observable`.

Filing happens in a **separate session** after this PRD is committed (per `feedback_commit_prds_before_referencing_tasks`).

### Phase 1 — Foundation slice (vertical; closes GR-001 minimally + opens GR-011/GR-019 partially)

- **Task SIR-α** — Wide-lock foundation: variant + side-table + adapter sweep + compile-lowering + cache-key arm + Steel_AISI_1045 reachable + trait Load/Support + PointLoad/FixedSupport rewrites + `examples/structure-instance.ri`.
  - **Observable signal:** `reify eval examples/structure-instance.ri` (per Q-SIR-7 sketch + nested-composition extension) prints non-Undef structure-shaped Value containing both (a) flat: `Steel_AISI_1045 { youngs_modulus: 205GPa, ... }`, `PointLoad { magnitude: 100.0 }`, `FixedSupport { }`; AND (b) nested: `Assembly { primary: Beam { material: Steel_AISI_1045 { ... }, loads: [PointLoad { magnitude: 100.0 }, PointLoad { magnitude: 50.0 }], supports: [FixedSupport { }], length: 1.0m }, secondary: Beam { ..., length: 2.0m } }`. Member-access chain `assembly.primary.material.youngs_modulus` evaluates through. CLI golden output committed. Plus `cargo test --workspace` green.
  - **Priority:** high (per Q-SIR-2; orchestrator narrow-lock memory).
  - **Prereqs:** none (forks from current main).
  - **Crates touched (foundation-lock charter):**
    - `crates/reify-types/src/value.rs` (variant + StructureTypeId + PersistentMap field choice)
    - `crates/reify-types/src/structure_registry.rs` (new — StructureRegistry + StructureMeta)
    - `crates/reify-eval/src/lib.rs` (value_type_kind_matches new arm at :195)
    - `crates/reify-eval/src/engine_eval.rs` (struct-ctor lowering precedence over eval_builtin)
    - `crates/reify-eval/src/cache.rs` + `persistent_cache.rs` (cache-key arm)
    - `crates/reify-eval/src/geometry_ops.rs`, `engine_eval.rs` adapter arms
    - `crates/reify-eval/src/significance_filter.rs`, `freshness_walk.rs` (per ComputeNode contract overlap)
    - `crates/reify-expr/src/lib.rs` (value-flow adapter arms)
    - `crates/reify-stdlib/src/{fea,geometry,joints,loop_closure,loop_closure_solver,mechanism,snapshot,supports,sweep}.rs` (match-site arms)
    - `crates/reify-stdlib/src/loads.rs` (retire `point_load` builtin entry)
    - `crates/reify-stdlib/src/supports.rs` (retire `fixed_support` builtin entry)
    - `crates/reify-compiler/stdlib/materials_fea.ri` (no source change — declaration is pre-existing; the lowering becomes live)
    - `crates/reify-compiler/stdlib/fea_multi_case.ri` (add `trait Load`, `trait Support`, `structure def PointLoad : Load { ... }`, `structure def FixedSupport : Support { ... }`)
    - `crates/reify-compiler/src/{compile,lowering,structure_def,trait_def}.rs` (recognize `@version(N)` annotation on structure_def; populate StructureMeta.version)
    - `examples/structure-instance.ri` (new — per Q-SIR-7)
    - `crates/reify-eval/tests/structure_instance_e2e.rs` (new — boundary test sketch §7 coverage)
  - **Boundary tests:** §7.1 + §7.2 scenarios that land in wave 1 — particularly the cache round-trip + engine-restart-stability + GR-011 nominal trait fixture.

### Phase 2 — Remaining ctor rewrites (per-ctor; each its own leaf)

- **Task SIR-β-mat** — Remaining materials: `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic`.
  - **Observable signal:** `examples/materials_starter_library.ri` (or extended `examples/structure-instance.ri`): each material constructs, member access reads the documented engineering value (per `materials_fea.ri`'s declared defaults). CLI golden output diff.
  - **Prereqs:** SIR-α.
  - **Closes:** GR-019 in full.

- **Task SIR-β-load** — `PressureLoad` rewrite (snake → Pascal + structure_def + builtin retirement).
  - **Observable signal:** `.ri` fixture constructs `PressureLoad(...)`, evaluates to non-Undef typed StructureInstance; member access reads the load's magnitude/direction fields. Existing `pressure_load` callers (audit + tests) updated to PascalCase.
  - **Prereqs:** SIR-α.

- **Task SIR-β-sup** — `PinnedSupport` rewrite (already PascalCase; just structure_def + builtin retirement).
  - **Observable signal:** `.ri` fixture constructs `PinnedSupport(...)`; field reads work.
  - **Prereqs:** SIR-α.

- **Task SIR-β-mlcfea** — `LoadCase` / `MultiCaseResult` / other multi-load-case ctors per the multi-load-case-fea PRD's scope.
  - **Observable signal:** `.ri` fixture exercises `LoadCase(name: "g", loads: [...], supports: [...])`; member access reads through.
  - **Prereqs:** SIR-α. Coordinate with multi-load-case-fea PRD's existing task list; this is a rewrite of ctor producers, not new functionality.

### Phase 3 — Cross-PRD downstream tasks (gated on this PRD's wave 1)

- **Task SIR-γ** — Task 3468 (GR-031 composed-stress envelope helpers, already filed) becomes unblocked.
  - **Observable signal:** Already named in task 3468 — shell-solve fixture exercises envelope `.max_von_mises` member access.
  - **Prereqs:** SIR-α.
  - **Disposition:** This PRD's wave 1 unblocks task 3468; no new task filed here. Cross-reference recorded in gap-register GR-031 update (§ this PRD's §10 / `Gap-register update` task in the filing session).

- **ComputeNode contract §8 task η** — FEA `solve_elastic_static` first real consumer (`docs/prds/v0_3/compute-node-contract.md` §8 task η).
  - **Observable signal:** Named in compute-node-contract.md.
  - **Prereqs:** SIR-α (this PRD's foundation). η lists "GR-001 resolved" as a precondition; this PRD's wave 1 IS that resolution.
  - **Disposition:** Owned by compute-node-contract.md; this PRD does not duplicate the task. Cross-PRD seam ordering is "this PRD's foundation slice ≤ compute-node-contract.md η."

### Phase 4 — Companion gap-register sweeps

- **Task SIR-δ** — gap-register.md updates: GR-001 `### Follow-up PRD` subsection added; GR-011, GR-019, GR-031 `Notes` rows updated to point at this PRD as the resolution mechanism. (Edited into this PRD-authoring session; see §10.)
  - **Observable signal:** `git diff docs/architecture-audit/gap-register.md` shows the four entries updated.
  - **Prereqs:** PRD committed.

### Dependency view

```
                              ┌→ SIR-β-mat ─→ GR-019 closed
                              │
SIR-α (foundation, high-pri) ─┼→ SIR-β-load
                              ├→ SIR-β-sup
                              ├→ SIR-β-mlcfea (multi-load-case-fea coordination)
                              │
                              ├→ SIR-γ (= task 3468 unblocked; GR-031 closed)
                              │
                              └→ compute-node-contract.md η (FEA first consumer; not owned here)

SIR-δ (gap-register sweep; happens during PRD-authoring session, not orchestrator)
```

## §9 — Open questions (surfaced but not decided in this session)

1. **`PersistentMap<String, Value>` exact type.** `im::HashMap` is in scope per Engine's existing usage; verify in the foundation task that the workspace's existing `im::` types are the right choice, vs. `BTreeMap` (deterministic iteration) or a smaller-than-im PersistentMap. Suggested default: reuse whichever PersistentMap the existing `Engine` ValueCell snapshots use; align with Clone-cheap-via-structural-sharing precedent. **Decide during SIR-α.**

2. **`im::HashMap` vs. `im::OrderedMap` for field iteration determinism.** Display order (debug printing, golden output goldens, ctor reconstruction in error diagnostics) prefers a deterministic iteration. Cache-key composition sorts explicitly so doesn't care. **Decide during SIR-α** based on observable diagnostic / CLI output stability.

3. **Compile-time vs. recognition of `@version(N)` annotation.** Wave 1 requires the compiler to recognize and lower this annotation. The annotation-args PRD (`docs/architecture-audit/annotation-args-session-prompt.md`) is the umbrella for annotation-args work; sequencing question: does this PRD's SIR-α land before, after, or in parallel with annotation-args PRD's recognition machinery? Suggested: foundation task implements the `@version(N)` recognition path narrowly for `structure def` only (path of least resistance — versioned-annotation recognition is decl-scoped, not expression-scoped). Annotation-args PRD generalizes later. **Decide at SIR-α planning time.**

4. **Trait member parameter shape for `Load` / `Support`.** The wave-1 task declares `trait Load { ... }` and `trait Support { ... }`. The exact member fields (magnitude type? direction shape? selector target?) need design work consistent with the existing Rust-side `loads.rs` / `supports.rs` field shapes that downstream consumers (FEA solver, multi-load-case-fea) already depend on. **Resolve during SIR-α** with a 30-minute review pass against `crates/reify-stdlib/src/{loads,supports}.rs` field shapes — preserve, don't redesign.

5. **PRD-corpus snake_case ctor sweep.** Existing PRD prose, examples, and tests reference `point_load(...)`, `pressure_load(...)`, `fixed_support(...)`. The PascalCase rewrite is breaking. Scope of the in-task sweep is bounded by `metadata.files` for SIR-α + SIR-β-load tasks; remaining sites (other PRDs, audit findings doc, examples not in scope) get fix-up commits as encountered. **Track via in-task grep; document residual sites as follow-up if too many.** Not a blocker for SIR-α.

6. **`Value::TraitObjectInstance`.** The audit-invariant comment at `engine_eval.rs:114-125` names both `StructureInstance` AND `TraitObjectInstance` as future variants. This PRD ships only `StructureInstance`. TraitObjectInstance (for a dynamic-dispatch `dyn Trait` value form) is deferred — current trait-typed-param machinery (`Type::TraitObject`) is satisfied by a `Value::StructureInstance` whose declared bounds include the trait, per §4. If true type-erased trait-object values are eventually needed, a sibling variant retrofit is additive. **Not pursued now.**

7. **Method/function dispatch on structure instances.** `Steel_AISI_1045().density_kg_per_m3()` — is `density_kg_per_m3` a member access (field) or a method call? Reify currently has only member access via `.` (no method dispatch — see C-38 in synthesis). This PRD does not introduce method dispatch on StructureInstance; member access only. Method dispatch is a separate language-design question. **Out of scope.**

8. **Hybrid-1 future relaxation.** Per GR-001 §"Resolution," typed-only structural admission for `Value::StructureInstance` (where compile-time `: TraitName` boilerplate can be elided in favor of cell-name-match against typed Value::StructureInstance) is the additive escape hatch. **Not pursued.** Reconsider only if `structure def : TraitName` boilerplate proves a real friction in practice.

9. **Map dict literal grammar.** The Q-SIR-7 add-on initially imagined `Map<String, Beam> = { "left": Beam(), "right": Beam(...) }` as an example shape — verified during grammar gate to NOT parse (tree-sitter-reify has no Map dict literal production). The nested example uses nested-structure-with-named-params instead (`Assembly { primary: Beam(), secondary: Beam(length: 2.0m) }`), which DOES parse. If/when downstream PRDs need user-authored Map literals at source (vs. constructing via builtin functions), grammar work is the prereq — flag as a separate PRD or fix-now under cluster C-06 (grammar fictions, already addressed via `feedback_prd_grammar_gate`). **Out of scope for this PRD.**

## §10 — Gap-register companion edits

In conjunction with PRD commit (separate task: editing `docs/architecture-audit/gap-register.md` in this same session if Leo approves):

- **GR-001** — add `#### Follow-up PRD: structure-instance-runtime.md` sub-section after the existing `#### Resolution (2026-05-12)` block. One paragraph: pointer + status line ("Contract authored 2026-05-12; SIR-α foundation slice queued via separate filing session").
- **GR-011 (cluster C-08 Load/Support type system)** — `Notes` row appended: "Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`. SIR-α foundation slice declares `trait Load` + `trait Support` + first PointLoad/FixedSupport rewrites; SIR-β-load / SIR-β-sup wave-2 tasks close the cluster fully."
- **GR-019 (cluster C-16 Material starter library)** — `Notes` row appended: "Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`. SIR-α makes Steel_AISI_1045 reachable; SIR-β-mat wave-2 task closes the remaining three materials (Aluminium_6061_T6, Titanium_Ti6Al4V, ABS_Plastic)."
- **GR-031 (cluster C-29 composed/derived stress recovery)** — `Notes` row appended: "Functional unblock mechanism: `docs/prds/v0_3/structure-instance-runtime.md` SIR-α. Task 3468 (already filed) executes against this PRD's wave-1 foundation."
