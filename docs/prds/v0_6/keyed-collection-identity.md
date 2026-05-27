# Keyed Collection Identity

Status: contract. Authored 2026-05-27 in interactive `/prd` session under G1–G6+META gates. Spec-gap-filling batch `spec-gap-2026-05-27`, cluster `tuple-and-keyed-collection-types`. **AskUserQuestion was unreachable in the authoring environment; the load-bearing design forks below were resolved by reasoned default and are flagged `[DEFAULT — needs Leo ratification]`. Do not queue tasks until Leo confirms §2.1 (the gap is member-identity for `sub`-collections, NOT a new value-Map literal) and §2.4 (do NOT reuse persistent-naming-v2 machinery).**

Resolves spec §18 deferred item **#8 — "Keyed collection identity: Named/keyed members in collections instead of positional."** Greenfield — no gap-register cluster covers it.

## §0 — Purpose and scope

Spec §3.4 commits counted sub-structures to `List<T>` with **positional** identity: `sub vents : List<Vent>` plus `constraint vents.count == vent_count`, indexed `vents[0]`, `vents[1]`. The spec itself flags the consequence (§3.4): "v0.1 uses positional indexing." Positional identity is **unstable under schema re-elaboration**: inserting a vent at the front renumbers every subsequent member, so any constraint, `connect`, or override that referenced `vents[2]` now silently binds a different vent. This is the §18.8 gap: collections want **stable, named member identity** so a member can be addressed by a key that survives count changes and reordering.

This PRD adds **keyed `sub`-collection members**: a `sub`-collection whose members carry stable `String` keys. `vents["intake"]` resolves to the same member regardless of how many siblings exist or what order they elaborate in. The key is the member's evaluation-graph identity at the schema level.

### §0.1 — What this is NOT (G1 pushback, resolved)

- **NOT a new value-`Map` literal.** `map{ "intake" => v }` already exists (spec §5.9) and gives keyed *value* collections — `Map<String, Vent>` of plain values. That is a value, not a *member*: its entries are not sub-structures, do not participate in containment, cannot be `connect`-ed, and have no schema identity. The §18.8 gap is specifically about **members** (`sub`), not values. (Verified: `map{...}` parses today; the gap is elsewhere.)
- **NOT a generic "Lists/Sets gain keys" feature.** A `List<Length>` of plain scalars has no identity problem worth solving — positional is fine for value lists. Identity instability *only* bites for `sub`-collections of **structures**, because structures are addressable targets of constraints/connects/overrides. So the feature is scoped to `sub`-collections of structure types.

> **The keyed-collection gap = stable string-key identity for `sub`-collection *members*, so constraints/connects/overrides bind to the intended member across schema re-elaboration.** Value-`Map` and value-`List` are untouched.

## §1 — Consumer (G1)

| Mechanism | Consumer (user surface or PRD) |
|---|---|
| Keyed `sub`-collection declaration | A `.ri` design that declares `sub vents : Keyed<Vent>` (or chosen surface §2.2) and addresses `vents["intake"]` in a constraint/override. User-observable: re-elaboration adds/removes a sibling and the `"intake"` constraint still binds the same member (CLI eval + a regression `.ri`). |
| Stable member key (eval-graph identity) | The engine's containment-tree + freshness walk: `vents["intake"]` resolves to a stable `NodeId` path; `connect ... -> vents["intake"].port` rebinds correctly after re-elaboration. |
| Keyed member override block | `.ri`: `sub vents : Keyed<Vent> { "intake" => { area = 5mm } }` — a user sets a per-member param by key. CLI shows `vents["intake"].area == 5mm`. |
| Keyed iteration | `forall (k, v) in vents: constraint v.area > 1mm` — quantify over keyed members. (Grammar reality-checked, §3.) |

**Engine-integration sub-check (G1).** Keyed `sub`-members touch the **containment tree + schema re-elaboration** path (the structural-classifier that today recognizes `count == N` on `List<Structure>` — `reify-eval/src/structural_classifier.rs`). The consumer is the existing schema-elaboration + freshness-walk seam: keyed members produce stable `NodeId` paths consumed by the freshness-only walk (`engine-integration-norm.md` §3.6) and the `connect` desugaring (spec §6.1). This is an **extension of an existing seam (schema elaboration), not a new one** — no norm extension needed; cite §3.6 freshness-walk as the in-engine consumer.

## §2 — Resolved design decisions

### §2.1 — The model: stable string key on `sub`-collection members `[DEFAULT — needs Leo ratification]`

- A keyed `sub`-collection is a containment-tree node holding **N member structures, each tagged with a unique `String` key**, ordered (insertion order preserved for iteration determinism) but addressed by key.
- `coll["key"]` resolves to the member with that key. Out-of-key access is an evaluation-graph-level failure (spec §3.4 convention for missing key), NOT a panic.
- Keys are **author-assigned** in the declaration/literal (no auto-generated keys in v1 — that would reintroduce the positional-instability problem under a different name). Duplicate keys within one collection = compile error (`E_DUP_MEMBER_KEY`).
- The key is **schema identity**: the member's `NodeId` path incorporates the key (`...::vents["intake"]`) so freshness/cache/`connect` bindings are key-addressed and survive re-elaboration. This is the load-bearing point — it is why positional `[0]` is replaced.

*Rationale for the default:* §18.8's literal text is "named/keyed members in collections instead of positional." The instability that motivates it is a *member-identity* problem (constraints/connects rebinding), which only exists for `sub` structures. Solving it for value-collections would be scope inflation with no identity consumer. *Flag:* if Leo intends §18.8 to mean value-`Map`/`Set` ergonomics rather than `sub`-member identity, the design pivots entirely — stop and re-author.

### §2.2 — Surface syntax `[DEFAULT — needs Leo ratification]`

Two grammar additions (both verified failing today, §3):

**Declaration form** — a keyed counterpart to `sub xs : List<T>`:
```
sub vents : Keyed<Vent>
```
`Keyed<T>` is the keyed-collection type constructor (parses as an ordinary `parameterized_type` today — §3 — so no *type-position* grammar work; only resolution work to recognize `Keyed` as a collection kind).

**Keyed override/literal block** — assign members by key:
```
sub vents : Keyed<Vent> {
    "intake"  => { area = 5mm }
    "exhaust" => { area = 8mm }
}
```
This block form (`{ key => { overrides } }`) does **not** parse today (§3) → grammar work (task α). It mirrors the existing `map{ k => v }` `=>` convention so it reads consistently.

**Access** — `vents["intake"]` — **already parses** (general `index_access` with a string-literal index; verified §3). No grammar work for access; only resolution + eval work to make a string index on a `Keyed` collection resolve by member key.

*Rationale:* reuse `Keyed<T>` parameterized-type grammar (free) + `=>` block (consistent with `map{}`) + existing string `index_access` (free). Minimizes new grammar to one production (the keyed `sub` block). *Flag:* the type-constructor *name* `Keyed` is a naming choice — `Map`-typed `sub` (`sub vents : Map<String, Vent>`) is an alternative that needs zero new type but conflates value-Map with sub-Map. Default picks a distinct `Keyed` to keep the member-vs-value distinction sharp. Leo may prefer another keyword.

### §2.3 — Interaction with `Map`, `connect`, member systems (G4 seams, declared)

- **`Map` (value collection):** untouched. `Keyed<T>` (members) and `Map<K,V>` (values) are distinct kinds. `Keyed<Vent>` is a `sub`-only construct; `Map<String, Vent>` remains a value. A future "promote a `Map` to keyed members" bridge is out of scope (§6).
- **`connect`:** `connect motor.shaft -> vents["intake"].inlet` must rebind to the keyed member, not a positional slot. The `connect` desugaring (spec §6.1) consumes the keyed `NodeId` path. **This PRD owns** the keyed-member resolution; `connect` is an existing consumer that gains key-addressed targets. Boundary-tested §7.
- **Member access / containment tree:** keyed members are children in the containment tree (spec §4.7 `sub`) with a key-bearing path segment. Dot-access `vents["intake"].area` chains through the keyed node.
- **`count` / structure-controlling constraint:** `constraint vents.count == N` still works (count = number of keyed members). The structural-classifier (`structural_classifier.rs`) extends to recognize `Keyed<Structure>` alongside `List<Structure>`. Iteration order = insertion order (deterministic).

### §2.4 — Do NOT reuse persistent-naming-v2 machinery `[DEFAULT — needs Leo ratification]`

Persistent-naming-v2 (`docs/prds/v0_2/persistent-naming-v2.md`) solves **geometry-topology** identity: attaching `(feature_id, role, local_index)` attributes to OCCT faces/edges so geometric selectors survive topology edits. That is a **kernel/geometry-layer** concern with a different store, lifetime, and failure mode.

Keyed-collection identity is **schema/evaluation-graph-layer**: an author-assigned `String` on a containment-tree member. It is *given* by the author, not *derived* from a kernel walk. Sharing machinery would couple two unrelated layers and inherit persistent-naming's geometry-pipeline complexity for no benefit.

> **Decision: separate. Declare the conceptual seam ("both are stable identity") in the cross-PRD table; share zero code.** The key is a first-class `String` member tag in `reify-core`/`reify-ir`, not a persistent-naming attribute.

*Rationale:* different layer, different lifetime (author-time key vs derived geometric attribute), different store. *Flag:* if Leo foresees a unified "stable identity" subsystem spanning both, that is an architectural decision above this PRD — flag and defer; this PRD proceeds standalone.

## §3 — Grammar gate (G3)

Parse-tested with `tree-sitter parse --quiet` from `tree-sitter-reify/` (2026-05-27, tree-sitter 0.26.8).

| Fragment | Fixture | Result | Resolution |
|---|---|---|---|
| `sub vents : Keyed<Vent>` | keyed-sub-A | **OK** (parameterized_type — `Keyed` is just an identifier) | no grammar work for declaration *type*; resolution work only |
| `vents["intake"]` | idx-str | **OK** (general index_access) | no grammar work for access; resolution + eval only |
| `sub vents : Keyed<Vent> { "intake" => { area = 5mm } }` | keyed-sub-lit (`sub ... = map{...}` variant) | **FAIL** (`sub` takes no keyed `=>` block today) | grammar work — keyed `sub`-member block (task α) |
| `["a": 1mm]` colon-keyed list literal | list-colon | **FAIL** | rejected — not the chosen surface (we use `=>` block on `sub`, not a colon list literal) |
| `forall (k, v) in vents: constraint v.area > 1mm` | keyed-forall | **see below** | grammar reality-check during α; if tuple-pattern `forall` binder fails, depend on tuple PRD α OR scope to value-only `forall v in vents.values()` |

**Single grammar prerequisite task (α)** covers: the keyed `sub`-member block (`{ "key" => { overrides } }`). The keyed-collection *type* (`Keyed<T>`) and *access* (`coll["k"]`) need **no** grammar work — only type-resolution and eval wiring.

**Keyed-`forall` binder dependency note.** `forall (k, v) in vents` needs a tuple/pair pattern binder. If that pattern reuses the tuple-pattern grammar, this PRD's keyed-iteration leaf (task ε) gains a **cross-PRD dependency on `tuple-type.md` task α** (tuple `let`-pattern grammar generalized to `forall` binders). To keep this PRD independently shippable, the **v1 default scopes keyed iteration to value-binder form** `forall v in vents: ...` (member values only, no key in the binder) — which uses existing `forall` grammar — and defers `(k, v)` pair-binder iteration to a follow-up gated on tuple-type (§6). This removes the hard cross-PRD grammar edge from the critical path. *Flag in §4.*

## §4 — Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/tuple-type.md` | optional consume | `(k, v)` pair-binder in `forall (k,v) in keyed_coll` would reuse tuple-pattern grammar (tuple-type task α). **v1 avoids this** by scoping keyed iteration to value-binder `forall v in coll`. | tuple-type (if/when pair-binder added) | **deferred** — not on this PRD's critical path; §6 follow-up |
| `docs/prds/v0_2/persistent-naming-v2.md` | conceptual sibling only | Both are "stable identity," **different layers** (schema-member key vs geometry-topology attribute). §2.4: zero shared code. | n/a (no shared mechanism) | independent — declared non-seam |
| `connect` system (spec §6.1) | this PRD produces | Keyed-member `NodeId` path; `connect ... -> vents["intake"].port` binds key-addressed. **This PRD owns** keyed-member resolution; `connect` desugaring is an existing consumer. | this-prd | produced (boundary-tested §7) |
| Schema re-elaboration / structural-classifier (`reify-eval`) | this PRD extends | `structural_classifier.rs` recognizes `Keyed<Structure>` alongside `List<Structure>` for `count`-controlled re-elaboration. Existing seam (§3.6 freshness-walk), extended. | this-prd | produced |

**No reciprocal-ownership ambiguity.** Persistent-naming is a declared non-seam; the tuple edge is deferred off the critical path; `connect`/schema are this-PRD-owned extensions of existing seams.

## §5 — Approach: B + H

G5 check: cross-crate (`tree-sitter-reify`, reify-ast, reify-compiler, reify-core (Keyed type kind), reify-ir (keyed member value/identity), reify-eval (structural-classifier + resolution + `connect`)) → ≥ 5 crates, and it touches a **load-bearing seam (schema re-elaboration + connect)**. **B + H.** H component: the member-identity contract (§2.1/§2.3) + the boundary-test sketch (§7) facing the schema-elaborator and the `connect` desugaring.

## §6 — Out of scope

- **`(k, v)` pair-binder keyed iteration** (`forall (k, v) in coll`). v1 uses value-binder `forall v in coll`. Pair-binder is a follow-up gated on `tuple-type.md` (§4). Flagged.
- **Auto-generated keys.** v1 requires author-assigned keys. Auto-keys would reintroduce the instability the feature removes.
- **Keyed `Set` / keyed value-`List`.** The feature is `sub`-member-only. Value collections stay positional/value-keyed.
- **`Map`→keyed-`sub` promotion bridge.**
- **Renaming/re-keying a member at runtime** (key is fixed at declaration).
- **Persistent-naming unification.** §2.4 — explicitly separate.

## §7 — Boundary-test sketch (H component; faces schema-elaborator + `connect`)

### 7.1 Schema-elaboration-side (engine builds keyed members)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Keyed declaration elaborates. `sub vents : Keyed<Vent> { "intake" => {area=5mm}, "exhaust" => {area=8mm} }`. | grammar α + resolution landed. | Containment tree holds 2 keyed members; `vents.count == 2`; `vents["intake"].area == 5mm`. |
| **Identity survives re-elaboration (the headline).** Add `"bypass"` member; re-elaborate. | as above. | `vents["intake"]` resolves to the *same* `NodeId` path as before the add; the `"intake"` constraint binding is unchanged. (Positional `[0]` would have shifted.) |
| Duplicate key. `{ "intake" => {...}, "intake" => {...} }`. | grammar α. | `E_DUP_MEMBER_KEY` compile error, named, no panic. |
| Missing key access. `vents["nonexistent"]`. | elaborated. | Evaluation-graph-level failure per spec §3.4 (not a panic). |
| `count`-controlled re-elaboration. `constraint vents.count == n; n = 3` with keyed members. | structural-classifier extended. | Re-elaboration triggers; classifier recognizes `Keyed<Structure>` as structure-controlling. |
| Iteration determinism. `forall v in vents: constraint v.area > 1mm`. | value-binder forall (existing grammar). | Iterates members in insertion order; constraint applies to all; deterministic. |

### 7.2 `connect`-side (assembly topology binds to keyed members)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Connect to keyed member. `connect manifold.outlet -> vents["intake"].inlet`. | keyed members + connect desugaring consume keyed path. | Topology edge binds to the `"intake"` member's port; connector instance owned by nearest common ancestor (spec §6.1). |
| **Connect survives re-key-neutral edit (the headline for connect).** Add a sibling member; the `vents["intake"]` connect edge is unchanged. | as above. | Edge still targets `"intake"`; no silent rebinding to a different member. |
| Connect to missing key. `connect x -> vents["ghost"].inlet`. | elaborated. | Named diagnostic (compile or eval-graph failure), no panic. |

The **integration-gate leaf** (task δ) names the 7.1 "identity survives re-elaboration" scenario as its observable signal, closing G2.

## §8 — Decomposition plan

Labels α…ε; IDs at decompose time. Approach B vertical slice under §2/§7 contract.

### Phase 1 — Grammar foundation

- **Task α — Grammar: keyed `sub`-member block `sub xs : Keyed<T> { "k" => { overrides } }`.**
  - **Observable signal:** fixture parses (`tree-sitter parse --quiet` exit 0); corpus test asserts the keyed-`sub` block CST shape (member-key string + override block per entry); `sub xs : List<T>` and `map{ }` unaffected (no regression). `grammar_confirmed=false`.
  - **Prereqs:** none.
  - **Crates touched:** tree-sitter-reify, reify-ast.

### Phase 2 — Type/identity model

- **Task β — `Keyed<T>` collection kind in `reify-core` + keyed-member identity (`NodeId` path carries the key) + duplicate-key diagnostic.**
  - **Observable signal:** compiler unit test resolves `Keyed<Vent>` to the keyed-collection kind (distinct from `Map`/`List`); a keyed member's resolved `NodeId` path includes the key segment; `E_DUP_MEMBER_KEY` on duplicate. **Intermediate** — unlocks γ, δ.
  - **Prereqs:** α.
  - **Crates touched:** reify-core, reify-ir (keyed member representation), reify-compiler.

### Phase 3 — Resolution + access (vertical slice)

- **Task γ — Keyed-member resolution: `coll["key"]` resolves by key; `coll["key"].member` chains; missing-key → eval-graph failure.**
  - **Observable signal:** `.ri` `sub vents : Keyed<Vent> { "intake" => {area=5mm} }; let a = vents["intake"].area` evaluates with `a == 5mm` via CLI; `vents["ghost"]` produces a clean eval-graph failure. **Leaf-ish (CLI-observable)**, also unlocks δ.
  - **Prereqs:** β.
  - **Crates touched:** reify-eval (index resolution on Keyed), reify-compiler.

### Phase 4 — Integration gate (identity stability under re-elaboration — the headline)

- **Task δ — Stable identity across schema re-elaboration + structural-classifier extension + `connect` to keyed member.**
  - **Observable signal:** `examples/keyed_vents.ri` (CI-run): declares keyed `vents`, a `constraint vents["intake"].area > 1mm`, and a `connect` to `vents["intake"]`; a sibling-add edit (via a `count`-controlled param) re-elaborates; a regression assertion confirms `vents["intake"]` resolves to the *same* member (its `area` constraint and connect edge unchanged) — whereas the positional `[0]` baseline would shift. CLI eval + structural-classifier test. **Leaf — the integration gate** (§7.1 headline + §7.2 connect headline).
  - **Prereqs:** γ. (structural-classifier extension folded in.)
  - **Crates touched:** reify-eval (structural_classifier.rs, freshness walk, connect desugaring consumption), examples/.

### Phase 5 — Iteration (value-binder; no tuple dependency)

- **Task ε — Value-binder keyed iteration `forall v in coll: ...`.**
  - **Observable signal:** `.ri` `forall v in vents: constraint v.area > 1mm` applies the constraint to every keyed member (CLI eval shows all members constrained; insertion-order determinism test). Uses existing `forall` grammar (`grammar_confirmed=true`). **Leaf.**
  - **Prereqs:** γ.
  - **Crates touched:** reify-eval (forall over Keyed), reify-compiler.

### Dependency view

```
α → β ─→ γ ─┬─→ δ   (integration gate: identity-stable + connect)
            └─→ ε   (value-binder iteration)
```

`(k, v)` pair-binder iteration is a **deferred follow-up** gated on `tuple-type.md` (§6) — NOT in this batch.

## §9 — Open questions (tactical; deferred)

1. **Type-constructor keyword.** `Keyed<T>` vs reusing `Map<String, T>` for the `sub` form vs another keyword. Default `Keyed<T>` (sharp member-vs-value distinction). Decide at β if Leo prefers otherwise. **Design-adjacent — flagged in §2.2, lean tactical since access/semantics are unchanged by the spelling.**
2. **Key type.** v1 keys are `String`. Could later admit `Int`/enum keys. Tactical; `String` only for v1.
3. **`E_DUP_MEMBER_KEY` / missing-key exact wording + whether missing-key is compile-time (when key is a literal) or eval-graph (when computed).** Decide at γ. **Suggested:** literal-key misses → compile error; computed-key misses → eval-graph failure (matches spec §3.4 Map convention).
4. **Override-block param syntax inside `=> { ... }`.** Confirm it reuses the existing `sub`-override block grammar exactly. Decide at α.
