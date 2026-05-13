# Node-Traits Unification — merge parallel taxonomies into one scheduler input

**Milestone:** v0.3
**Status:** contract-resolving (B+H per `preferences_implementation_chain_portfolio`)
**Date:** 2026-05-13
**Source:** GR-038 / cluster C-36 of the 2026-05-12 architecture audit; escalated to its own PRD in the 2026-05-12 investigate-further triage session.
**Supersedes:** `docs/prds/node-trait-composition.md` (acceptance criteria #1, #3, #5 absorbed and re-met under the unified surface).

---

## §0 — Purpose and supersession

The original `node-trait-composition.md` PRD landed two independent mechanisms for "what kind of node is this, and what scheduling/cancellation policy applies":

1. **Static type-level affordances** — `NodeTraits` bitflags (4 flags) + `NodeArchKind` enum (7 variants) in `crates/reify-types/src/node_traits.rs`. Declarative, never read by the scheduler.
2. **Per-instance / per-type policy** — `NodePolicyOverrides` + `NodeKind` (5 variants) + `NodeCommitmentOverride` in `crates/reify-runtime/src/commitment.rs`. The actual scheduler hook (`CommitmentTracker::should_continue`).

The two never converged. `node_traits.rs:147-153` documents the divergence and defers the merge "once future tasks introduce the missing struct counterparts." The scheduler reads `NodePolicyOverrides`; ignores `NodeTraits`. Audit `findings/node-trait-composition.md` filed six gap-by-gap mechanisms (M-002, M-003, M-005, M-008, M-009, M-011) flagging FICTION/PARTIAL state across the trait surface.

Synthesis §3 named this "new pattern B: two parallel taxonomies that don't compose." This PRD resolves the GR-038 disposition.

**Direction (chosen 2026-05-13, recorded in §3):** *C′ — refined bridge.* Retire only the duplicate kind enum (`NodeArchKind` → collapses into a single canonical `NodeKind` aligned with `NodeId`'s 5 variants). Keep both `NodeTraits` (static type-level affordances) and `NodePolicyOverrides` (per-instance / per-type policy) because they answer different architectural questions (§7.6 traits vs §7.3 policy). Build the missing bridges that the original PRD's acceptance criteria already required.

This is **B+H** per portfolio: contract document (§§4–6) + boundary tests facing both ways (§9) + vertical-slice decomposition with user-observable leaves (§10).

---

## §1 — Goal

After this PRD lands, the scheduler dispatches on a single declarative surface that the §7.6 trait→priority intent and the §7.3 policy override intent both feed into through enforced bridges. A maintainer adding a new node kind declares its `NodeTraits` once, gets a kind-derived priority and commitment policy by default, and per-instance overrides retain their existing precedence. The "two parallel taxonomies" critique is resolved by retiring the redundant kind enum and explicitly naming the orthogonal-by-design separation between traits and policy.

User-observable surface (drives G1):

- **Operator introspection** — `reify dev inspect-node <node-id>` (new CLI subcommand under the existing `reify-debug` MCP plumbing) prints a node's kind, declared traits, derived priority, derived policy, and any active instance/type overrides. The leaf observable for the bridge work.
- **Diagnostic surface** — `W_PROGRESSIVE_INVARIANT_VIOLATED` fires when a non-`PROGRESSIVE` node attempts to write `Freshness::Intermediate` to its cache entry. The leaf observable for the PROGRESSIVE invariant guard.
- **Boundary test fixtures** — `crates/reify-eval/tests/node_traits_boundary.rs` (new) exercises 7 scenarios facing both crate sides; failures surface in `cargo test` output and CI.

---

## §2 — Background

### Two surfaces, two questions

`NodeTraits` (arch §7.6) and `NodePolicyOverrides` (arch §7.3) answer **different** questions:

- **§7.6 traits** = static type-level *affordances*. "This kind needs warm-state preserved" (`WARM_STARTABLE`), "may emit intermediates" (`PROGRESSIVE`), "is sub-frame and never cancellable" (`IMMEDIATE`), "is subject to commitment policy at all" (`COMMITTABLE`). A declaration about implementation shape.
- **§7.3 policy overrides** = per-instance / per-type *operator policy*. `CommitIfSlow` (default), `AlwaysCancelWhenStale`, `OnlyRunOnFinalInputs`. A declaration about how the operator wants this node treated under stale-input conditions.

Neither is fully a superset of the other. `OnlyRunOnFinalInputs` doesn't fit any §7.6 trait flag; `WARM_STARTABLE` doesn't fit any commitment-override slot. The duplication is solely in the **kind taxonomy** (`NodeArchKind` 7 variants vs `NodeKind` 5 variants), which both surfaces use as a key.

### Where the actual failures sit

The audit's six gap-by-gap mechanisms cluster into three classes of **missing bridge**:

| Audit M-finding | Class | Today | Bridge needed |
|---|---|---|---|
| M-002 | per-NodeId trait map | `default_traits(NodeArchKind)` exists but no scheduler call site reads it | per-`NodeId` trait map + scheduler init populates from kind defaults |
| M-005 | (same) | no struct/hashmap associates `NodeId` with `NodeTraits` | (same) |
| M-003 | trait→priority | doc-only mapping table at `node_traits.rs:204-225`; no `traits_to_priority` function | `traits_to_priority(NodeTraits) -> Priority` + scheduler init default-populates `node_priorities` |
| M-008 | IMMEDIATE→never-cancelled | arch §7.5 says P0/P1-fast never cancelled; no code path enforces | scheduler cancellation guard checks `priority ∈ {P0Interactive, P1Fast}` and skips |
| M-009 | PROGRESSIVE invariant | flag is "wired" via compile-time anchor only (test admits this); underlying behaviour is universal | invert the contract: non-`PROGRESSIVE` writing `Intermediate` triggers `W_PROGRESSIVE_INVARIANT_VIOLATED` |
| M-011 | tests against wrong taxonomy | tests assert `NodeArchKind::Realization.default_traits()`, not `NodeId::Realization`'s effective traits | tests rebuilt against unified `NodeKind` (collapsed) keyed on actual `NodeId` |
| (also flagged) M-013 | WARM_STARTABLE↔protocol | flag and `impl WarmStartable` have no enforced correspondence | runtime gate at scheduler init: debug-build assertion that declared traits and registry entries are co-extensive |

Bridges, not enum collapse, are the load-bearing fix.

### Architecture refs

- `docs/reify-implementation-architecture.md` §7.3 (Task Commitment Policy) — the §7.3 surface.
- `docs/reify-implementation-architecture.md` §7.5 (Cancellation Refinement) — P0/P1-fast never cancelled invariant.
- `docs/reify-implementation-architecture.md` §7.6 (Node Traits) — the four-flag taxonomy and trait→priority intent.
- `docs/reify-implementation-architecture.md` §3.3 (Two-Cone Scheduling Model) — Priority enum semantics (P0Interactive < P1Fast < P1Slow < P3Speculative).

### Sibling new-pattern-B (informs but doesn't dictate)

GR-011 / cluster C-08 (Load/Support nominal-vs-kind-tagged) is the **other** new-pattern-B occurrence in the 2026-05-12 audit. It resolved under GR-001 by collapsing the parallel surfaces into one typed runtime representation (`Value::StructureInstance` per `docs/prds/v0_3/structure-instance-runtime.md`). That shape works for GR-011 because its two surfaces (snake_case Map and PascalCase struct) ARE answering the same question. GR-038's two surfaces (static traits and per-instance policy) are NOT, so the same collapse-everything shape doesn't apply. Hence C′ rather than B.

---

## §3 — Direction call

**Direction C′ — refined bridge.** Decided 2026-05-13 by Leo (recorded via `/prd` author-mode `AskUserQuestion`).

| Decision | Choice | Rationale |
|---|---|---|
| Kind taxonomy | Collapse `NodeArchKind` into `NodeKind`; mirror `NodeId` exactly (5 variants: `Value`, `Constraint`, `Realization`, `Resolution`, `Compute`) | Removes the only genuinely redundant surface. Drops the two placeholder kinds (`SchemaNode`/`SourceNode`) that have no `NodeId` counterpart today; re-add when their runtime variants land. |
| Trait surface | Keep `NodeTraits` as four-flag bitfield | Static affordances aren't policy. `PROGRESSIVE` and `WARM_STARTABLE` don't fit a `NodeCommitmentOverride` enum; absorbing them would cramp the type. |
| Policy surface | Keep `NodePolicyOverrides` with existing instance > type > default precedence | Per-instance commitment/gating policy is a user-affordance the scheduler must consult; flattening into bitflags loses the precedence chain. |
| Bridges | Build them: per-`NodeId` trait map; `traits_to_priority`; `default_overrides(NodeKind, NodeTraits)`; `WARM_STARTABLE` ↔ `WarmStartable` runtime gate; `PROGRESSIVE` invariant guard | Bridges are where the original PRD failed; this PRD's mass is bridges. |
| Scheduler dispatch | Unchanged at the call site (`config.node_overrides.resolve(node_id)`); but `node_overrides` and `node_priorities` are now default-populated from kind-derived values when no explicit entry exists | Existing scheduler test suite (`concurrent_eval.rs`) keeps passing; new behaviour is purely additive. |
| PROGRESSIVE | Wire as invariant guard | Inverts the contract — `PROGRESSIVE` becomes a positive permit; absence becomes enforced. Gives the flag teeth. |
| WARM_STARTABLE ↔ protocol | Runtime gate, debug-build assert | Adds the M-013 bridge without a proc-macro. |
| GR-007 ticket (`tkt_0RNVQ0MQMVRKAA3PB6W8TP2324`) | Not superseded; composes above | The ticket's `[node_overrides]` reify.toml schema sits between type-overrides and kind-derived defaults in the new precedence chain. Real `add_dependency` edge from this PRD's δ task to the ticket at decompose time. |

Retired: `NodeArchKind`, `default_traits(NodeArchKind)` (replaced by `default_traits(NodeKind)`).

Preserved: `NodeTraits`, `NodePolicyOverrides`, `NodeCommitmentOverride`, `NodeKind`, `CommitmentTracker`, `SchedulerConfig`.

---

## §4 — Surfaces (interface contract part 1)

### Canonical kind enum (under `reify-types`)

```rust
// crates/reify-types/src/node_traits.rs (after Phase 1 task α)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Value,         // ↔ NodeId::Value(ValueCellId)
    Constraint,    // ↔ NodeId::Constraint(ConstraintNodeId)
    Realization,   // ↔ NodeId::Realization(RealizationNodeId)
    Resolution,    // ↔ NodeId::Resolution(ResolutionNodeId)
    Compute,       // ↔ NodeId::Compute(ComputeNodeId)
}

impl NodeKind {
    /// Total From conversion (NodeId is a sealed 5-variant enum).
    pub fn of(node_id: &NodeId) -> Self { /* match */ }

    /// Architecture-specified default trait set per kind.
    pub const fn default_traits(self) -> NodeTraits {
        match self {
            NodeKind::Value       => NodeTraits::IMMEDIATE,
            NodeKind::Constraint  => NodeTraits::empty(), // see §12 Q-1
            NodeKind::Resolution  => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
            NodeKind::Realization => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
            NodeKind::Compute     => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
        }
    }
}
```

`reify_runtime::commitment::NodeKind` is removed; the runtime crate re-exports the canonical `reify_types::NodeKind` (preserves all `NodePolicyOverrides::set_type(NodeKind, …)` call sites). The `From<&NodeId> for NodeKind` impl moves to `reify-types` alongside `NodeId` (currently in `reify-eval`; lifting `NodeId` is out of scope — the `From` impl can live in `reify-runtime` with the existing `commitment.rs` line if dependency direction blocks the move; Phase 1 task α resolves which).

### `NodeTraits` (unchanged surface, four flags)

`NodeTraits` retains its four flags, bitfield ops, `default()`, `union`, `intersection`, `contains`, `Not`. No API churn. Sole change: `default_traits` moves from `NodeArchKind` to `NodeKind` (per above).

### `NodePolicyOverrides` (extended)

```rust
// crates/reify-runtime/src/commitment.rs (after Phase 2 task δ)
impl NodePolicyOverrides {
    /// Resolve effective override for `node_id`, consulting (in precedence order):
    ///   1. instance overrides
    ///   2. type overrides
    ///   3. config-file overrides         (from reify.toml [node_overrides], via GR-007 ticket)
    ///   4. kind-derived defaults         (from default_overrides(NodeKind, NodeTraits))
    ///   5. NodeCommitmentOverride::default() = CommitIfSlow
    pub fn resolve(&self, node_id: &NodeId) -> NodeCommitmentOverride { /* ... */ }
}
```

Levels 1–2 are existing. Level 4 is new under this PRD (kind-derived default). Level 3 is owned by GR-007 ticket `tkt_0RNVQ0MQMVRKAA3PB6W8TP2324`; this PRD's δ task wires the precedence slot but leaves the file-ingestion path to the ticket.

---

## §5 — Bridges (interface contract part 2)

Five bridge functions/structures, one per audit M-finding bridge-class. Live in `reify-types` (declarations) and `reify-runtime` (scheduler integration).

### B1 — Per-`NodeId` trait map (M-002, M-005)

```rust
// crates/reify-types/src/node_traits.rs (Phase 1 task β)
#[derive(Clone, Debug, Default)]
pub struct NodeTraitsMap {
    instance: HashMap<NodeId, NodeTraits>,
    by_kind:  HashMap<NodeKind, NodeTraits>,
}

impl NodeTraitsMap {
    pub fn set_instance(&mut self, node_id: NodeId, traits: NodeTraits);
    pub fn set_type(&mut self, kind: NodeKind, traits: NodeTraits);
    pub fn resolve(&self, node_id: &NodeId) -> NodeTraits {
        self.instance.get(node_id).copied()
            .or_else(|| self.by_kind.get(&NodeKind::of(node_id)).copied())
            .unwrap_or_else(|| NodeKind::of(node_id).default_traits())
    }
}
```

Stored on `SchedulerConfig` as a new field `pub node_traits: NodeTraitsMap`. Default-empty preserves existing behaviour.

### B2 — `traits_to_priority` (M-003, M-008)

```rust
// crates/reify-types/src/node_traits.rs (Phase 2 task γ)
pub const fn traits_to_priority(traits: NodeTraits) -> Priority {
    if traits.contains(NodeTraits::IMMEDIATE) {
        Priority::P1Fast       // see §12 Q-2: P0Interactive vs P1Fast for kind-derived default
    } else if traits.contains(NodeTraits::COMMITTABLE) {
        Priority::P1Slow
    } else {
        Priority::P3Speculative
    }
}
```

`Priority` lifts to `reify-types` (currently in `reify-runtime`). If lift is blocked by other dependents, `traits_to_priority` lives in `reify-runtime` instead (tactical, not load-bearing — Phase 2 task γ resolves).

Scheduler init (Phase 2 task γ): for each NodeId in the eval set without an explicit entry in `config.node_priorities`, default-populate from `traits_to_priority(node_traits.resolve(node_id))`. External priority assignments still take precedence — the GUI can still set a specific cell to `P0Interactive` while the user is editing.

### B3 — `default_overrides` (composition — Phase 2 task δ)

```rust
// crates/reify-runtime/src/commitment.rs (Phase 2 task δ)
pub fn default_overrides(kind: NodeKind, traits: NodeTraits) -> NodeCommitmentOverride {
    if !traits.contains(NodeTraits::COMMITTABLE) {
        // Arch §7.6 row 4: "Absent this trait, always cancellable."
        NodeCommitmentOverride::AlwaysCancelWhenStale
    } else {
        NodeCommitmentOverride::CommitIfSlow
    }
}
```

`NodePolicyOverrides::resolve` consults this when no instance / type / config-file override applies (level 4 in the precedence chain). Wires arch §7.6's "absent COMMITTABLE → always cancellable" as code, not docs.

### B4 — IMMEDIATE → never-cancelled scheduler guard (M-008 enforcement, Phase 3 task η)

The cancellation path in `CommitmentTracker::should_continue` (today: `concurrent.rs:329-369`) gains:

```rust
// Pseudocode — Phase 3 task η
pub fn should_continue(&self, node_id: &NodeId, in_dirty_cone: bool, priority: Priority) -> bool {
    if matches!(priority, Priority::P0Interactive | Priority::P1Fast) {
        // Arch §7.5: P0/P1-fast never cancelled.
        return true;
    }
    // ... existing dirty-cone + commitment logic
}
```

`B2` guarantees `IMMEDIATE → P1Fast` (or `P0Interactive` per §12 Q-2). Composed with B4, the §7.6 invariant is enforced at the scheduler.

### B5 — `WARM_STARTABLE` ↔ `WarmStartable` registry gate (M-013, Phase 3 task ζ)

```rust
// crates/reify-runtime/src/scheduler.rs (Phase 3 task ζ)
#[cfg(debug_assertions)]
fn assert_warm_startable_coextensive(
    node_traits: &NodeTraitsMap,
    registry: &WarmStartableRegistry,
) {
    for kind in NodeKind::iter() {
        let declared = kind.default_traits().contains(NodeTraits::WARM_STARTABLE);
        let registered = registry.contains_kind(kind);
        debug_assert_eq!(
            declared, registered,
            "NodeKind {kind:?}: WARM_STARTABLE declaration ({declared}) \
             must match WarmStartableRegistry presence ({registered})"
        );
    }
}
```

`WarmStartableRegistry` is a new lightweight type (`HashMap<NodeKind, fn() -> Box<dyn WarmStartable>>` or equivalent) populated by the existing `reify-solver-elastic` and `reify-kernel-occt` impl sites at scheduler construction. Release builds skip the assertion (the cost is debug-only); the registry itself is always consulted by the warm-state donation/restoration path.

### B6 — `PROGRESSIVE` invariant guard (M-009 fix, Phase 3 task θ)

`CacheStore` write path (today: `cache.rs::insert_*` family) gains an invariant check:

```rust
// Pseudocode — Phase 3 task θ
pub fn write_intermediate(&mut self, node_id: NodeId, value: Value, ...) {
    let traits = self.node_traits.resolve(&node_id);
    if !traits.contains(NodeTraits::PROGRESSIVE) {
        debug_assert!(false, "non-PROGRESSIVE node {node_id:?} emitted Intermediate");
        self.diagnostics.emit(W_PROGRESSIVE_INVARIANT_VIOLATED { node_id });
        // proceed (don't drop the write — this is a soft invariant in release builds)
    }
    // ... existing insert
}
```

`W_PROGRESSIVE_INVARIANT_VIOLATED` is a new diagnostic code under the existing `W_*` warning family.

---

## §6 — Resolution chain & scheduler dispatch invariants

### Trait resolution chain (per `NodeId`)

| Level | Source | Set how |
|---|---|---|
| 1 | per-instance | `NodeTraitsMap::set_instance(node_id, traits)` |
| 2 | per-kind | `NodeTraitsMap::set_type(kind, traits)` |
| 3 | kind-derived default | `NodeKind::default_traits(kind)` |

### Policy resolution chain (per `NodeId`)

| Level | Source | Set how |
|---|---|---|
| 1 | per-instance | `NodePolicyOverrides::set_instance(node_id, override)` |
| 2 | per-kind | `NodePolicyOverrides::set_type(kind, override)` |
| 3 | reify.toml `[node_overrides]` | GR-007 ticket `tkt_0RNVQ0MQMVRKAA3PB6W8TP2324` |
| 4 | kind+traits-derived default | `default_overrides(kind, traits)` |
| 5 | hard default | `NodeCommitmentOverride::CommitIfSlow` |

### Priority resolution chain (per `NodeId`)

| Level | Source | Set how |
|---|---|---|
| 1 | external explicit | `SchedulerConfig.node_priorities.insert(node_id, priority)` (GUI / test) |
| 2 | trait-derived | `traits_to_priority(node_traits.resolve(node_id))` |

### Scheduler dispatch invariants (post-PRD)

- I-1: For every `NodeId` in the eval set, `node_priorities.get(&id)` returns `Some(_)` after scheduler init (either explicit or derived).
- I-2: For every `NodeId` whose effective `Priority` is `P0Interactive` or `P1Fast`, `should_continue` returns `true` regardless of dirty-cone state. (B4)
- I-3: For every `NodeKind` whose `default_traits` contains `WARM_STARTABLE`, `WarmStartableRegistry::contains_kind` returns `true` (debug builds, B5).
- I-4: For every cache write of `Freshness::Intermediate`, the target node's effective `NodeTraits` contains `PROGRESSIVE` (debug builds, B6).
- I-5 (existing, preserved): per-instance override > per-type override > default in both `NodePolicyOverrides` and `NodeTraitsMap`.

---

## §7 — Cross-PRD relationship

| Other PRD / artifact | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/node-trait-composition.md` | supersedes | acceptance criteria #1 (trait→priority), #3 (config-file ingestion partial), #5 (IMMEDIATE never-cancel) | this PRD | wired (companion task ι annotates parent with supersession header) |
| `docs/prds/v0_3/compute-node-contract.md` | references | `NodeKind::Compute` is in the merged enum; ComputeNode's default_traits = `WARM_STARTABLE \| COMMITTABLE` | this PRD | wired (no edit needed in compute-node-contract) |
| `docs/prds/v0_3/freshness-4-variant.md` | references | B6 PROGRESSIVE guard rides on `Freshness::Intermediate` emission semantics | freshness-4-variant | wired (consume only) |
| `docs/prds/v0_3/engine-integration-norm.md` | informs | future realization-kind dispatchers introducing new `NodeId` variants must declare the kind's `default_traits` and (if WARM_STARTABLE) register an impl | engine-integration-norm | informational (norm checklist absorbs this rule once landed) |
| GR-007 ticket `tkt_0RNVQ0MQMVRKAA3PB6W8TP2324` | composes-above | reify.toml `[node_overrides]` is precedence level 3 (between type-override and kind-derived default) | ticket | queued (real `add_dependency` edge from this PRD's δ at decompose time) |
| `docs/prds/v0_3/structure-instance-runtime.md` | sibling new-pattern-B | informs shape; no direct seam | none | informational (different problem, different shape — see §2) |

No reciprocal-ownership ambiguity. No new contested-ownership pairs introduced.

---

## §8 — Pre-conditions for activating

- **None block authoring** — the original PRD is shipped; `NodeId`, `NodeTraits`, `NodePolicyOverrides`, `CommitmentTracker`, `Priority`, `SchedulerConfig` all exist.
- **GR-007 ticket** is a coordination point, not a blocker (this PRD's δ wires the precedence slot; the ticket fills the slot independently).
- **No grammar work** — this PRD introduces no new Reify syntax. The `reify dev inspect-node` CLI subcommand is shell, not Reify; the `[node_overrides]` reify.toml schema (owned by GR-007 ticket) is TOML, not Reify. G3 = no-op.

---

## §9 — Boundary test sketch (cross-crate; facing both ways)

Single test crate location: `crates/reify-eval/tests/node_traits_boundary.rs` (new). `reify-eval` is the natural meeting point — depends on both `reify-types` (declarations) and `reify-runtime` (scheduler dispatch).

| ID | Scenario | Side | Pre-conditions | Post-conditions |
|---|---|---|---|---|
| T1 | `NodeKind::default_traits` matches §7.6 declared assignments table | reify-types | enum stable | for each variant, `default_traits` returns the documented set; `Constraint` empty (or `IMMEDIATE` per §12 Q-1) |
| T2 | `default_overrides(kind, default_traits(kind))` matches §7.3 documented kind defaults | reify-runtime | B3 wired | `Compute/Realization/Resolution → CommitIfSlow`; `Constraint` (if traits empty) → `AlwaysCancelWhenStale`; `Value → CommitIfSlow` (IMMEDIATE has COMMITTABLE-absence implication TBD via §12 Q-3) |
| T3 | Scheduler integration: a NodeId with no explicit priority gets the kind-derived priority | reify-runtime | B1+B2 wired | A `NodeId::Compute(...)` with no `node_priorities` entry runs at `P1Slow`; assertion via `concurrent_eval`-style fixture inspecting actual schedule order |
| T4 | Precedence chain holds in both surfaces | both | both maps populated with mixed instance/type entries | `node_traits.resolve(n)` and `node_overrides.resolve(n)` both return per-instance > per-type > default in synchronized fashion |
| T5 | `WARM_STARTABLE` ↔ registry coextension | reify-runtime | B5 wired | (a) building a fixture `NodeKind` declaring `WARM_STARTABLE` without registry impl panics at scheduler init in debug; (b) registry entry without trait declaration also panics; (c) release builds skip the assertion (no panic) |
| T6 | IMMEDIATE → never-cancelled at scheduler | reify-runtime | B2+B4 wired | A `NodeId::Value(...)` placed in dirty cone with `should_continue` invoked returns `true` (not cancelled); contrast: same fixture with priority `P1Slow` returns `false` |
| T7 | PROGRESSIVE invariant guard | reify-eval (cache) | B1+B6 wired | A non-`PROGRESSIVE` node calling the cache `write_intermediate` path triggers `debug_assert` panic in debug; emits `W_PROGRESSIVE_INVARIANT_VIOLATED` in release; PROGRESSIVE-tagged node writes succeed silently |

T5/T6/T7 each consume two of the seven new bridges and exercise both the producer side (declaration) and consumer side (enforcement) — the H-discipline two-way property.

---

## §10 — Decomposition plan

DAG with Greek-letter labels; task IDs assigned at `/prd decompose`. Each leaf names a user-observable signal per G2.

### Phase 1 — Foundation (unblocks remaining phases)

| Label | Title | Crates | Observable signal | Prereqs |
|---|---|---|---|---|
| α | Collapse NodeArchKind → NodeKind; lift `default_traits` to canonical `NodeKind` (mirror `NodeId` 5 variants) | reify-types, reify-runtime, reify-eval | `cargo test -p reify-types -p reify-runtime -p reify-eval` green; existing 12 NodeTraits unit tests + `precedence_instance_wins_over_type_wins_over_default` + scheduler integration tests all pass; `NodeArchKind` symbol absent from `cargo doc --no-deps` for reify-types | (none) |
| β | Add `NodeTraitsMap` + `SchedulerConfig.node_traits` field | reify-types, reify-runtime | new `cargo test -p reify-types::node_traits_map` test exercises set_instance / set_type / resolve precedence; SchedulerConfig docstring lists the new field; concurrent_eval.rs unchanged (default-empty preserves behaviour) | α |

### Phase 2 — Vertical slice (minimum-viable end-to-end through new bridges)

| Label | Title | Crates | Observable signal | Prereqs |
|---|---|---|---|---|
| γ | Implement `traits_to_priority` + scheduler-init default-population of `node_priorities` | reify-types (or reify-runtime if Priority lift blocked), reify-runtime | T3 boundary test passes: a NodeId with no explicit priority gets kind-derived priority; `concurrent_eval.rs` regression tests still green | β |
| δ | Implement `default_overrides(NodeKind, NodeTraits)`; extend `NodePolicyOverrides::resolve` to consult it (precedence level 4) | reify-runtime | T2 boundary test passes: kind-derived default override matches arch §7.3; existing `precedence_instance_wins_over_type_wins_over_default` + `set_type_override_resolves_to_type_value_and_isolates_other_kinds` + `set_instance_override_resolves_to_instance_value_and_isolates_other_nodes` still green | γ |
| ε | `reify dev inspect-node <node-id>` CLI subcommand (under reify-debug MCP / CLI plumbing) prints kind/traits/derived-priority/derived-policy/instance-overrides | reify-cli (or wherever `reify dev` subcommands live), reify-runtime | `reify dev inspect-node Compute(foo)` against an example `.ri` engine emits text matching documented format; CLI integration test asserts the output shape | γ, δ |

### Phase 3 — Invariant gates (the bridges' teeth)

| Label | Title | Crates | Observable signal | Prereqs |
|---|---|---|---|---|
| ζ | `WarmStartableRegistry` + scheduler-init coextension assert | reify-runtime, reify-types (registry type), reify-solver-elastic, reify-kernel-occt (registration call sites) | T5 boundary test passes: declared-without-impl fixture panics in debug; release skips; existing warm-state donation tests still green | β, α |
| η | IMMEDIATE → never-cancelled cancellation guard | reify-runtime | T6 boundary test passes: P0/P1-fast nodes in dirty cone are not cancelled; existing AlwaysCancelWhenStale + commitment-decision tests still green | γ |
| θ | PROGRESSIVE invariant cache-write guard + `W_PROGRESSIVE_INVARIANT_VIOLATED` diagnostic | reify-eval, reify-runtime | T7 boundary test passes: non-PROGRESSIVE writing Intermediate panics in debug, emits diagnostic in release; existing `progressive_emission` test rewritten to assert positive-permit shape | β |

### Phase 4 — Companion correction tasks

| Label | Title | Crates | Observable signal | Prereqs |
|---|---|---|---|---|
| ι | Annotate `docs/prds/node-trait-composition.md` with supersession header pointing at this PRD; update `docs/architecture-audit/gap-register.md` GR-038 disposition to "PRD authored" with supersession pointer | docs only | grep finds supersession line in both files; CI link-check (if present) passes | (none — can land in parallel with code phases) |
| κ | Update arch doc `§7.6` to enumerate the bridge functions (or add `§7.6.1`); update `§7.3` to note that `NodePolicyOverrides` composes ABOVE kind-derived defaults; update `§7.5` to note IMMEDIATE→P1Fast/P0Interactive guarantee is now code-enforced via B4 | docs only | grep finds new function names + precedence chain in arch doc | (none) |

### DAG edges (intra-batch)

```
α → β → γ → δ → ε
        γ → η
β → ζ
β → θ
ι, κ — no intra-batch deps (parallel; can land any time)
```

### Integration-gate task

**ε** is the explicit user-observable integration gate (CLI surface). T1–T7 boundary tests are tied to ζ/η/θ as their per-task observable signals — closing G2 via the C-as-integration-gate pattern (per `preferences_implementation_chain_portfolio`).

Total: 9 leaf tasks (α, β, γ, δ, ε, ζ, η, θ, ι, κ — 10 if ι and κ split). All ≤ ~3-crate locks; ζ has the widest blast radius (4 crates: reify-runtime/types/solver-elastic/kernel-occt), candidate for "wide-lock" tag at decompose per `feedback_orchestrator_narrow_locks_favor_upfront_design`.

---

## §11 — Out of scope for this PRD

- **Adding new traits beyond the four §7.6 names.** `IMMEDIATE`, `WARM_STARTABLE`, `PROGRESSIVE`, `COMMITTABLE` only. Future arch-doc work may add more (e.g. `IDEMPOTENT`, `PURE`); not here.
- **Splitting `Priority` into more variants.** P0Interactive / P1Fast / P1Slow / P3Speculative stay as today.
- **Replacing `NodePolicyOverrides`'s 3-valued enum with a richer policy DSL.** `CommitIfSlow` / `AlwaysCancelWhenStale` / `OnlyRunOnFinalInputs` only.
- **Per-instance trait overrides via reify.toml.** This PRD wires `NodeTraitsMap::set_instance` programmatically; configuration-file ingestion of trait overrides (analogous to GR-007 for policy) is a future ticket if needed.
- **Re-adding SchemaNode / SourceNode kinds.** Belongs to whichever PRD eventually adds the corresponding `NodeId` variants (likely a follow-up under structure-instance-runtime or a dedicated schema-node PRD).
- **Lifting `Priority` and `NodeId` to `reify-types` if dependency direction is currently blocked.** Tactical resolution at task α; if blocked, `traits_to_priority` lives in `reify-runtime` and this PRD's surface descriptions adjust accordingly.
- **The "dedicated UI widget" for policy overrides** (arch §7.3) — original PRD's out-of-scope item; remains out of scope here.
- **Custom-derive macro for compile-time WARM_STARTABLE↔protocol gate.** B5 uses a runtime registry instead. Future work could add a derive if maintainer pain warrants.

---

## §12 — Open questions (tactical; surfaced but not decided in this session)

1. **Q-1: ConstraintNode default traits.** Today `NodeArchKind::ConstraintNode.default_traits()` returns empty (`node_traits.rs:222-224` admits "Predicate evaluation is cheap but §7.6 does not yet classify it"). After collapse to `NodeKind::Constraint`: keep empty (→ kind-derived priority is `P3Speculative`, kind-derived policy is `AlwaysCancelWhenStale` per B3) OR assign `IMMEDIATE` (matches predicate-cheapness, kind-derived priority `P1Fast`, never-cancelled). **Suggested resolution:** keep empty pending a §7.6 doc update; constraints' actual scheduling cost is small enough that `P3Speculative` is acceptable as a default-default, and the GUI/scheduler can override per-instance for tight-loop predicates. Decide during task α.

2. **Q-2: IMMEDIATE → P0Interactive vs P1Fast** for the kind-derived default. `traits_to_priority` returns one or the other; arch §7.5 treats both as never-cancelled, so B4 doesn't care. **Suggested resolution:** P1Fast for the kind-derived default; P0Interactive is set per-instance by the GUI for the cell currently being edited (matches today's external-priority-source convention). Decide during task γ.

3. **Q-3: `default_overrides(NodeKind::Value, IMMEDIATE)`** — IMMEDIATE has no implication for COMMITTABLE; absence of COMMITTABLE in the trait set means B3 returns `AlwaysCancelWhenStale`. But Value cells are sub-frame and never cancel anyway (B4 short-circuits). Whether to override the B3 derivation for Value (return `CommitIfSlow` instead) is cosmetic. **Suggested resolution:** let B3 return `AlwaysCancelWhenStale`; B4 makes this moot at the scheduler for Value cells; document the cosmetic mismatch. Decide during task δ.

4. **Q-4: `traits_to_priority` location** — `reify-types` (canonical) or `reify-runtime` (if `Priority` lift is dependency-blocked). **Suggested resolution:** lift `Priority` if cheap; otherwise live in `reify-runtime`. Decide during task γ.

5. **Q-5: B6 release-build behaviour for non-PROGRESSIVE intermediate writes** — debug panics, release emits diagnostic and proceeds (soft invariant). Alternative: release also drops the write. **Suggested resolution:** soft invariant in release (proceed) — preserves the existing universal-emission behaviour for any node written with the assumption of PROGRESSIVE-permissiveness. Decide during task θ.

6. **Q-6: `NodeTraitsMap::set_instance` for ConstraintNode currently empty.** If a per-instance constraint declares `IMMEDIATE`, the new B4 guard kicks in and never-cancels it. Is that desired, or should constraints be treated as a class with a hard policy ceiling? **Suggested resolution:** allow per-instance override; the ceiling lives in operator policy (set_type), not in code. Decide during task β.

---

## §13 — Gap-register companion edits (handled by task ι)

- `docs/architecture-audit/gap-register.md` GR-038 Disposition → refine to "**PRD authored — `docs/prds/v0_3/node-traits-unification.md`** (2026-05-13). Direction: C′ refined bridge (retire NodeArchKind only; keep NodeTraits + NodePolicyOverrides as orthogonal surfaces; build five named bridges)."
- Add to GR-038 Notes: "Supersedes original `docs/prds/node-trait-composition.md` acceptance criteria #1, #3, #5. Sibling new-pattern-B occurrence (GR-011) resolves under GR-001 with a different shape because its surfaces ARE answering the same question; node-traits-unification's are not."
- `docs/prds/node-trait-composition.md` — prepend supersession header pointing at this PRD; do NOT delete (still cited by other audit findings; remains the foundation).
