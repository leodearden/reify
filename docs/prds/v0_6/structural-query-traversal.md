# Structural Query & Traversal

Status: contract. Authored 2026-05-27 in interactive `/prd` session under G1–G6+META gates. Spec-gap-filling batch `spec-gap-2026-05-27`, cluster `structural-query-traversal`. **AskUserQuestion was unreachable in the authoring environment; the load-bearing design forks are resolved by reasoned default below and re-stated in `## DESIGN FORKS FOR LEO`. Tuples are explicitly NOT being added (batch constraint) — no design decision below relies on them.**

Resolves two spec gaps (both past v0.2; version labels ignored):
- **§18 deferred item #2 — "Rich structural query/traversal: `children`/`members` pseudo-collection filterable by trait"** (the spec's intended traversal surface for walking an entity's sub-tree).
- **§5.9 — `List.generate(n, lambda)` combinator** (shown as an example in the spec, currently unimplemented anywhere in the workspace).

These two ship together because the headline structural-query consumers (bolt-pattern bills-of-material, conformance roll-ups) are most natural when a list of indices/poses can be *generated* and a sub-tree can be *traversed* in the same `.ri`. They share zero implementation, but co-shipping gives one coherent "query + build a list" surface.

---

## §0 — Purpose and scope

### §0.1 — Structural query/traversal (§18.2)

Today a parent can reach a **named** child member by dot-access (`self.motor.shaft_diameter`, spec §8.3) but cannot ask *open-ended structural questions* over its sub-tree: "how many sub-components do I contain?", "give me every descendant that conforms to trait `Bolt`", "is every contained `Vent` determined?". The spec (§18 item 2) names the intended surface: **`children` / `members` pseudo-collections, filterable by trait**. This is the substrate for assembly bills-of-material, conformance/manufacturing-readiness roll-ups, and mass aggregation over a contained set.

This PRD adds three read-only schema-traversal accessors on a structure entity, each yielding a `List` of entity references that flows into the existing free-function collection surface (`count(...)`, `filter(...)`, `forall ... in ...`):

- **`self.children`** — the structure's **direct** `sub`-declared children (one level).
- **`self.members`** — the structure's direct `sub`-declared children **including `aux` subs** (see §3 — `aux` inclusion is the deliberate distinction from a future product-only view), plus collection-sub elements flattened (each `sub xs : List<T>` contributes its elements).
- **`self.descendants`** — the transitive closure: every node reachable by walking the containment tree downward (children, their children, …). This is the spec's "walk an entity's sub-tree" surface.

Each is **filterable by trait** via the existing free-function form: `filter(self.descendants, Bolt)` returns the sub-list of descendants conforming to trait `Bolt`. (Method-call form `self.descendants.filter(Bolt)` does **NOT** parse — GR-040, verified §4 — so the surface is free-function throughout.)

### §0.2 — `List.generate` combinator (§5.9)

Spec §5.9 shows `List.generate(bolt_count, |i| point3(...))` to build a list by applying a lambda over `n` indices `0..n-1`. **`List.generate` does not exist** in the workspace (greenfield), and its literal spelling `List.generate(...)` does **not parse** (member-access-then-call is not a grammar form — §4). This PRD adds the combinator under a **free-function spelling** `generate(n, lambda)` (the spec's literal `List.` prefix is a grammar fiction; §4 resolves it). It is the canonical idiom for parametric bolt-circle / vent-array index lists that then drive geometry patterns or keyed-member counts.

### §0.3 — What this is NOT (G1 pushback, resolved)

- **NOT a new containment-tree builder.** The containment tree already exists (subs are named children — spec §4.7; `sub_member_types` / `sub_structure_traits` in `reify-compiler/src/scope.rs`; the build-time sub-realization walk in `reify-eval/src/engine_build.rs`). This PRD adds **read-only query accessors over that existing tree**, not a new tree.
- **NOT geometry traversal.** `children`/`members`/`descendants` walk the **schema/containment tree** (entities), not geometry topology (faces/edges). Topology selectors (`reify-eval/src/topology_selectors.rs`) and persistent-naming are a separate, geometry-layer concern — declared non-seam (§5).
- **NOT a fix for deep cross-sub dot-access.** Spec §8.3 documents that *nested dot-access* `self.outer.inner.body` is a deferred v0.1 limitation. Structural-query is the spec's **intended traversal surface** that sidesteps that gap: `self.descendants` walks the tree directly (the same way auto-surfacing does — sub-placement §4.3) and yields entity references, so it does **NOT depend on** nested dot-access landing. A user who wants "every descendant body" filters `self.descendants` and reads a geometry member off each via single-level access on that descendant's own scope; this PRD does not extend dot-chain depth.
- **NOT keyed/value `Map` ergonomics.** Unrelated to keyed-collection-identity's member-key feature (declared seam, §5).

---

## §1 — Consumer (G1)

| Mechanism | Consumer (user surface or PRD) |
|---|---|
| `self.children` accessor | A `.ri` assembly that evaluates `count(self.children)` and gets the number of direct subs. **User-observable:** CLI eval of `let n = count(self.children)` on a multi-sub structure prints the right integer. |
| `self.members` accessor (incl. `aux`, flattens collection-subs) | A bill-of-materials `.ri`: `let part_count = count(self.members)` over a structure with named subs + a `sub bolts : List<Bolt>` + an `aux` jig → counts all of them. **User-observable:** CLI eval count matches the hand-count including collection elements and `aux`. |
| `self.descendants` accessor (transitive) | A nested assembly (`arm → motor → shaft`): `let all = count(self.descendants)` returns the full sub-tree size. **User-observable:** CLI eval matches the total node count at all depths. |
| Trait filter `filter(self.descendants, Bolt)` | A conformance/BOM query: `let bolts = filter(self.descendants, Bolt); let bolt_count = count(bolts)`. **User-observable:** CLI eval returns exactly the descendants conforming to `Bolt`, excluding non-`Bolt` subs. |
| `forall m in self.members: <pred>` | A manufacturing-readiness roll-up: `constraint forall m in self.members: determined(m)`. **User-observable:** the constraint is present and evaluates over every member (CLI eval / `reify check`). |
| `generate(n, lambda)` combinator | A bolt-circle `.ri`: `let positions = generate(bolt_count, |i| point3(radius * cos(...), radius * sin(...), 0mm))`. **User-observable:** CLI eval shows a `List` of `n` correctly-computed points; `generate(0, f)` is `[]`. |

**Engine-integration sub-check (G1).** These accessors are **not** an in-engine kernel mechanism (no kernel module / dispatcher / runtime trampoline). They are a **compiler-resolution + eval-time expansion** path — directly analogous to the existing **purpose reflective-access** machinery (`subject.params` / `subject.sub_entities` in `reify-compiler/src/expr.rs:434` `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS` + the activation-time expansion in `reify-eval/src/engine_purposes.rs:480–520`), which already resolves a schema query into a `List` of entity references over the value-cell graph. Structural-query **extends that same reflective-resolution path** to general structures (not just purpose subjects) and adds tree-recursion for `descendants`. The named consumer is the **user-observable CLI eval surface** (per the table) — the engine-integration-norm §3 catalogue does not apply because no in-`reify-eval` dispatch/walk seam is being *produced for another producer to plug into*; this PRD is itself the consumer, terminating at CLI eval. (Cross-checked against the norm's §5 checklist: no new orphan-producible `pub fn` in a `kernel-*` crate.)

---

## §2 — Resolved design decisions

### §2.1 — `children` / `members` / `descendants` semantics `[DEFAULT — needs Leo ratification]`

| Accessor | Scope | Includes `aux`? | Collection-subs |
|---|---|---|---|
| `self.children` | direct subs, **one level** | yes | each `sub xs : List<T>` counts as **one** child (the collection node), elements **not** flattened |
| `self.members` | direct subs, **one level** | yes | each collection-sub is **flattened** — every element is a member |
| `self.descendants` | **transitive** downward closure | yes | flattened at every level |

Rationale for the `children` vs `members` split: spec §18 item 2 lists *both* `children` and `members` as distinct names, so they must differ. The natural, useful distinction — and the one that matches assembly-modelling intuition — is **`children` = structural slots** (a `List<Vent>` is *one* slot) vs **`members` = the actual contained entities** (each vent is a member). `descendants` is the transitive form of `members`. This gives a clean three-way: slot-count, member-count (flat one level), total-count (flat all levels).

`aux` inclusion: `aux` subs **are** included in all three accessors (G6 premise §6). `aux` (per sub-placement §2.2) means "no external *geometric/product* effects" — it is a *surfacing/export* axis, **not** a containment-tree-membership axis. An `aux` jig is still a contained entity; a BOM/conformance query over `self.members` should see it. A future *product-only* view (excluding `aux`) is out of scope (§6) and would be a separate `product_members`-style accessor if ever needed. **This is the precise answer to the sub-placement seam question (§5):** structural-query consumes the *same containment tree* sub-placement walks, but on the *membership* axis, where `aux` is included; sub-placement's auto-surfacing walks the *same tree* on the *surfacing* axis, where `aux` is excluded. One tree, two orthogonal predicates.

*Flag:* if Leo wants `members` to exclude `aux` (treating `aux` as "not a real member"), the default flips — but that conflates the surfacing axis with the membership axis, which sub-placement deliberately separated. Default keeps them orthogonal.

### §2.2 — Trait filtering: free-function `filter(coll, Trait)` `[DEFAULT — needs Leo ratification]`

Trait-filtering reuses the **existing free-function `filter`** with a **trait name as the second argument**: `filter(self.descendants, Bolt)` returns the descendants conforming to trait `Bolt`. Conformance is resolved against the schema-level trait set (`sub_structure_traits: HashMap<sub_name, Vec<trait_name>>`, `reify-compiler/src/scope.rs:36`) — a structure conforms to `Bolt` iff `Bolt` ∈ its trait set (transitively through trait refinement). Missing/unknown trait name → named compile diagnostic, not a panic.

- **No method-call form.** `self.descendants.filter(Bolt)` does **not parse** (GR-040; §4). Spec §5.9's `list.filter(|x| ...)` method spelling is a grammar fiction for this surface — the realized spelling is free-function `filter(list, predicate_or_trait)`.
- **Predicate vs. trait second arg.** When the 2nd arg is a **trait name** (an identifier resolving to a trait), it is a conformance filter. When it is a **lambda** (`|x| pred`), it is the ordinary value filter. The two are distinguished at compile time by the arg's resolved kind. v1 ships the **trait-name** form (the §18.2 headline); the lambda-predicate form over entity-ref lists is a tactical extension (§9) — not required for the gap.

*Flag:* an alternate spelling `conforming(self.descendants, Bolt)` (a dedicated query fn) avoids overloading `filter`'s second arg across lambda-vs-trait. Default overloads `filter` to match spec §18.2's literal "filterable by trait" phrasing and to keep one combinator. Leo may prefer the dedicated-fn spelling.

### §2.3 — `generate` spelling: free-function `generate(n, lambda)` `[DEFAULT — needs Leo ratification]`

Spec §5.9's literal `List.generate(n, fn)` does **not parse** (§4). Two spellings considered:
- **`generate(n, |i| expr)`** — free function, parses today, GR-040-clean. **Default.**
- `list_generate(n, |i| expr)` — more explicit but verbose.

Semantics (from spec §5.9 / §3.4): `generate(n, f)` applies `f` to indices `0, 1, …, n-1` and collects the results into a `List` whose element type is the lambda body's type. `generate(0, f)` is valid and returns `[]` (empty list of the appropriate element type — spec §3.4). `n` must be a non-negative integer (`Int`/`Nat`); negative or non-integer `n` → named diagnostic. The index `i` passed to the lambda is an `Int` (`0`-based).

*Flag:* the index type — `Int` vs a dimensionless `Nat`/`Real`. Default `Int` (matches spec example `i * 2 * pi / bolt_count` where `i` is a plain count). If Leo wants `i : Nat`, pin at the eval task.

### §2.4 — Determinacy & ordering `[DEFAULT — needs Leo ratification]`

- **Traversal order is deterministic.** `children`/`members`/`descendants` enumerate in **declaration order** (source order of `sub` declarations), with collection-sub elements in index order, and `descendants` in **pre-order depth-first** (a node before its children). This mirrors the determinism discipline already applied to purpose reflective-access (`engine_purposes.rs:480` sorts members for reproducibility — but here declaration order is more useful than lexicographic and is already available from the compiled template, so we use declaration/pre-order rather than sort).
- **Undef-sub determinacy (G6 §6).** A `sub`-collection whose `count` is `undef` (schema not yet elaborated to a definite element count) contributes **zero elements** to `members`/`descendants` for that collection node (the collection node itself still appears in `children`), matching the existing "empty list ⇒ vacuous-true forall" convention (`engine_purposes.rs:498` comment; spec §3.4 empty-collection rules). Traversal never panics on an undetermined sub-count; it yields a smaller-but-well-defined set. This makes `count(self.descendants)` *monotonic* as schema elaboration resolves counts — a defensible, non-surprising semantics.

*Flag:* declaration-order vs sorted-order for `children`/`members`. Default declaration-order (more useful for BOM/inspection; deterministic from the template). Leo may prefer sorted for hash-stability parity with purpose reflective-access.

### §2.5 — Return type: `List<EntityRef>` `[DEFAULT — needs Leo ratification]`

The accessors return a `List` of **entity references** (the same `EntityRef`/reflective-cell value kind purposes already produce — `engine_purposes.rs` builds `ValueRef` elements). An entity-ref element supports: `count(...)`/`filter(...)`/`forall`, single-level member access on the referenced entity (`m.area`), and trait-conformance test. It does **NOT** auto-resolve to geometry — a descendant's geometry is reached by accessing a geometry member on that descendant (single-level), keeping this independent of the deferred nested-dot-access gap (§0.3).

*Flag:* whether the element type is the existing purpose `EntityRef` reflective kind reused verbatim, or a new `StructureRef`. Default **reuse** the existing reflective-cell kind (the purpose machinery already returns it, and §1's engine sub-check leans on that reuse). Pin the exact value-kind at the type/identity task.

---

## §3 — Relationship to `aux` and the containment tree (sub-placement seam, resolved)

This is the seam the brief flags. Resolution, stated precisely:

- **One containment tree, two consumers.** `docs/prds/v0_6/sub-placement-and-surfacing.md` (§4.2) walks the containment tree to **compose placement transforms down to descendants and auto-surface product geometry**. Structural-query walks **the same tree** to **enumerate entity references for query**. Neither owns the tree (it predates both — spec §4.7, `engine_build.rs`); both are *read consumers*.
- **`aux` is orthogonal on each axis.** Sub-placement's `aux` modifier excludes a sub from the **surfacing/export** axis (not rendered as product, not STEP-exported, not FEA, not mass-counted). Structural-query operates on the **membership** axis, where `aux` **is included** (§2.1) — an `aux` jig is still a contained member for BOM/conformance purposes. So: `aux` sub → **absent** from sub-placement's product surfacing, **present** in structural-query's `members`/`descendants`.
- **Does structural-query consume sub-placement's walk, or build its own?** **Build its own (lightweight) read-walk.** Sub-placement's walk produces *posed geometry handles* (it composes transforms and realizes OCCT shapes — heavy). Structural-query needs only *entity references* (schema-level — cheap, no realization). Sharing sub-placement's transform-composing walk would couple a cheap schema query to expensive geometry realization. **Decision: structural-query's `descendants` walk is a separate, schema-only recursion over the compiled containment template (`sub_member_types`/`sub_structure_traits` + the child templates), reusing the purpose reflective-resolution path, NOT sub-placement's geometry walk.** The two walks visit the same tree nodes but carry different payloads.
- **Ordering parity (declared, not wired).** Both walks should visit children in **declaration order** so that, e.g., the i-th surfaced child and the i-th `self.children` element refer to the same sub. This is a *convention* both PRDs follow; no shared code. Declared here; sub-placement §4 already implies declaration order via its `entity_path` scheme (`parent.sub#realization[i]`, §11 Q2).

**Seam ownership (G4):** structural-query **owns** the schema-only read-walk (`descendants` recursion + reflective resolution extension). Sub-placement **owns** the geometry-posing walk. No reciprocal ambiguity — they are different walks over a shared, pre-existing tree.

---

## §4 — Grammar gate (G3)

Parse-tested with `tree-sitter parse --quiet` from `tree-sitter-reify/` (2026-05-27, tree-sitter 0.26.x). Fixtures retained under `/tmp/prd-gate-fixtures/sq-*.ri`.

| Fragment | Fixture | Result | Resolution |
|---|---|---|---|
| `self.children` (member access) | `sq-children-access` | **OK** | no grammar work — member_access (`grammar.js:1009`) |
| `self.members` | `sq-members-access` | **OK** | no grammar work |
| `self.descendants` | `sq-descendants` | **OK** | no grammar work |
| `count(self.members)` (free-fn) | `sq-count-members` | **OK** | no grammar work — function_call |
| `filter(self.members, Bolt)` (free-fn, trait arg) | `sq-filter-trait` | **OK** | no grammar work — function_call with identifier arg |
| `forall m in self.members: determined(m)` | `sq-forall-members` | **OK** | no grammar work — quantifier_expression |
| `generate(bolt_count, |i| point3(i, 0mm, 0mm))` (free-fn) | `sq-generate-freefn` | **OK** | no grammar work — function_call + lambda_expression |
| `self.members.filter(Bolt)` (method form) | `sq-members-method-filter` | **FAIL** (ERROR @ `.filter(...)`) | **rejected — GR-040**, use free-fn `filter(self.members, Bolt)` |
| `List.generate(bolt_count, |i| ...)` (spec literal) | `sq-list-generate` | **FAIL** (ERROR @ `.generate(...)`) | **rewrite** — member-access-then-call is not a grammar form; spell `generate(n, |i| ...)` |

**G3 verdict: NO grammar work required.** Every realized surface in this PRD parses with today's grammar. The two failures are *fictions in the spec prose* (`list.filter` method form and `List.generate` dotted form) which this PRD **rewrites to the free-function spelling** that already parses — exactly the GR-040 pattern (no method-call syntax; spec's composed examples are fiction, per project memory `reference_reify_grammar_no_method_call_syntax`). All `grammar_confirmed=true`. No grammar-prerequisite task is filed.

> Contrast with the batch siblings: sub-placement and keyed-collection each needed a grammar-work prerequisite task (`at`/`aux` tokens; keyed `=>` block). Structural-query needs **none** — the entire gap is **eval/resolution**, not syntax. This is the answer to the brief's G3 sub-question ("verify whether the gap is grammar or eval/compile"): **it is eval/compile.**

---

## §5 — Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/sub-placement-and-surfacing.md` | shares substrate | Both **read** the pre-existing containment tree. Structural-query walks it schema-only for entity refs (incl. `aux`); sub-placement walks it for posed geometry (excl. `aux`). **Separate walks, shared tree, orthogonal `aux` axis.** Declaration-order convention shared. | tree is pre-existing; **each PRD owns its own walk** | independent — declared non-shared-code seam (§3) |
| `docs/prds/v0_6/keyed-collection-identity.md` | consumes (forward-compatible) | When a `sub`-collection is **keyed** (`Keyed<T>`), its elements appear in `members`/`descendants` and **expose their string key**. Structural-query's flattening of collection-subs must, for a keyed collection, carry each element's key on the entity-ref (so `forall m in self.members` over a keyed collection can read `m`'s key, and a future `key(m)` accessor works). | keyed-collection **owns** the key on the member `NodeId`; structural-query **consumes** it (reads the key off the entity-ref) | **declared, deferred wiring** — v1 structural-query flattens keyed-collection elements like any collection-sub; **exposing the key** through traversal is gated on keyed-collection landing first. See §9 follow-up. No grammar/critical-path edge in v1. |
| `docs/prds/v0_2/persistent-naming-v2.md` | conceptual non-seam | Persistent-naming is **geometry-topology** identity (faces/edges); structural-query is **schema/containment** traversal (entities). Different layers, zero shared code. | n/a | independent — declared non-seam |
| Purpose reflective-access (`reify-compiler` / `reify-eval`, spec §4.4) | this PRD extends | `subject.params`/`subject.sub_entities` reflective resolution (`expr.rs:434`, `engine_purposes.rs:480`) is the existing schema-query→`List<ref>` path. Structural-query **generalizes it** from purpose-subjects to any structure + adds tree-recursion. Existing seam, extended. | this-prd | produced (boundary-tested §7) |
| `connect` topology / spec §6.1 | out of scope | `connect` builds an assembly *topology graph* (edges between ports). Traversing *that* graph (connectivity queries) is a distinct future feature; this PRD traverses the *containment* tree only. | n/a | out of scope (§6) |

**Keyed-collection seam — exact relationship (brief sub-question).** The brief asks whether `members` traversal over keyed subs should expose keys. **Answer: yes, but deferred.** v1 structural-query treats a `Keyed<T>` sub-collection identically to a `List<T>` sub-collection — every element is a `member`. The **key exposure** (each traversed element carrying its stable `String` key, addressable as e.g. `key(m)`) is declared here as a consumer of keyed-collection-identity's key-on-`NodeId` mechanism, and is **gated on keyed-collection landing** (`add_dependency` edge declared, §8). This keeps structural-query independently shippable for the common `List<T>` case while reserving the key-exposure wiring for when both features exist. **No reciprocal-ownership ambiguity:** keyed-collection owns the key; structural-query owns the traversal that reads it.

**No reciprocal-ownership ambiguity** anywhere: sub-placement (separate walks), persistent-naming (non-seam), keyed-collection (clear owner = keyed-collection; structural-query is the downstream reader), purpose-reflective (this PRD extends an existing path).

---

## §6 — G6 premise validation

Structural-query asserts no numeric bounds or closed-form-exactness claims (branches 1/2 N/A — this is a discrete schema query, not a numerical computation). Branch 3 (end-to-end capability) checks:

- **"`count(self.descendants)` returns the right integer"** — the capability requires: (a) the containment template enumerable at compile time (exists — `sub_member_types`/child templates), (b) reflective resolution to `List<ref>` (exists for purposes; this PRD's β/γ extend it), (c) `count` over a `List` (exists — `COLLECTION_AGGREGATION_MEMBERS`, `expr.rs:415`). All in this PRD's dependency set or pre-existing. **Pass.**
- **"`filter(self.descendants, Bolt)` returns exactly the conforming descendants"** — requires the schema trait set per node (`sub_structure_traits`, exists) + the filter dispatch (this PRD's δ). In-set. **Pass.**
- **`generate(n, f)` over `n` indices** — requires lambda eval (exists — `CompiledExprKind::Lambda`, `engine_purposes.rs:624`) + list construction (exists). The `n=0 ⇒ []` and element-type-from-body claims match spec §3.4 exactly. **Pass.**
- **Undef-sub determinacy** — the §2.4 claim ("undef collection-count contributes zero elements, no panic") is validated against the **existing** empty-list-vacuous-true convention (`engine_purposes.rs:498`); it weakens to a well-defined subset rather than asserting an impossible total. **Pass.**
- **`aux` inclusion premise** — the §2.1/§3 claim that `aux` subs appear in `members` rests on `aux` being a *surfacing* axis (sub-placement §3 rule 2), not a *containment* axis. Verified against sub-placement's own text ("still tessellated and shipped to the GUI … it **is** a contained entity"). **Pass.**

No premise is a guess or misattribution. The integration-gate leaf (task ε) names a CLI-eval signal whose every required capability is delivered by ε's own prerequisites (β/γ/δ) — not by anything downstream.

---

## §7 — Approach: B + H

G5 check: cross-crate blast radius — `reify-compiler` (member-access resolution for `children`/`members`/`descendants`; `filter`-with-trait-arg typing; `generate` typing), `reify-eval` (reflective resolution extension + tree recursion + trait-filter dispatch + `generate` eval), `reify-stdlib` (the `generate` builtin), plus `examples/` for the CI signal. ≥ 3 crates, and it **extends a load-bearing reflective-resolution seam** shared with purposes. → **B + H.**

H component:
- **Contract (§2, §3, §8.1 below):** the three-accessor semantics table, the `aux`-orthogonality rule, the return-type contract, the determinism/undef rules.
- **Boundary-test sketch (§8.2):** facing the **compiler resolver** (does `self.descendants` type to `List<EntityRef>`?) and the **eval expander** (does the recursion enumerate the right nodes, in order, including `aux`, excluding undef-count collection elements?), plus the producer/consumer faces of the trait-filter and `generate`.

### §8.1 — Seam contract (signatures, invariants)

- `resolve_structural_query(entity, accessor) -> List<EntityRef>` where `accessor ∈ {children, members, descendants}`. **Invariants:** deterministic (declaration order; pre-order DFS for `descendants`); total (never panics; undef counts → fewer elements); `children ⊆ members` as node sets (children = slot nodes; members = flattened); `members ⊆ descendants`.
- `filter(List<EntityRef>, Trait) -> List<EntityRef>`: returns the conforming subset; conformance is transitive trait-set membership; preserves source order; empty input → empty output; unknown trait name → compile diagnostic.
- `generate(n: Int, f: Fn(Int) -> T) -> List<T>`: applies `f` to `0..n-1` in order; `n=0 → []`; `n<0` or non-int → diagnostic; element type = body type of `f`.

### §8.2 — Boundary-test sketch (faces resolver + expander)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Resolver:** `self.children` types | structure with 2 named subs + 1 collection-sub | `children` types to `List<EntityRef>`; no diagnostic |
| **Expander:** `children` enumerates slots | as above | `count(self.children) == 3` (2 named + 1 collection node); declaration order |
| **Expander:** `members` flattens collection | `sub bolts : List<Bolt>` with `count==4` + 2 named subs | `count(self.members) == 6` (4 bolts + 2) |
| **Expander:** `members` includes `aux` | one `aux sub jig : Jig` among the subs | `jig` present in `members`; `count` includes it |
| **Expander:** `descendants` transitive | `arm` contains `motor` contains `shaft` | `count(self.descendants)` == total node count at all depths; pre-order |
| **Expander:** undef collection-count → no panic | `sub vents : List<Vent>` with `count == undef` | collection node in `children`; **zero** vent elements in `members`; no panic; `forall` over it vacuous-true |
| **Filter producer/consumer:** trait filter | descendants mix `Bolt` and non-`Bolt` | `filter(self.descendants, Bolt)` == exactly the `Bolt`-conforming subset (transitive conformance) |
| **`generate` producer/consumer:** | `generate(4, |i| i)` and `generate(0, |i| i)` | first == `[0,1,2,3]` (Int list); second == `[]`; `generate(3, |i| point3(i*1mm,0mm,0mm))` yields 3 points |
| **Keyed seam (deferred — declared only):** | a `Keyed<Vent>` sub (post keyed-collection) | v1: elements appear in `members` like any collection; key-exposure wiring is the deferred follow-up (§9) |

The **integration-gate leaf** (task ε) names the "`members` flattens collection + includes `aux`, then `filter(...)` by trait gives the right BOM count" end-to-end scenario as its observable signal, closing G2.

---

## §8 — Decomposition plan

Labels α…ζ; IDs at decompose time. Approach B vertical slice under §2/§8.1 contract. **No grammar prerequisite** (§4). Spine (C-as-integration-gate): β → γ → δ → ε.

### Phase 1 — Compiler resolution

- **Task α — Compiler: `children`/`members`/`descendants` member-access typing on a structure entity → `List<EntityRef>`.**
  - **Observable signal:** a `.ri` `let n = count(self.children)` compiles with no diagnostics; a compiler unit test shows `self.children`/`self.members`/`self.descendants` each resolve to `List<EntityRef>` (the reflective-cell list type, reusing the purpose path's element kind); a bogus accessor (`self.grandchildren`) still errors. **Intermediate** — unlocks β, δ. `grammar_confirmed=true`.
  - **Prereqs:** none.
  - **Crates:** reify-compiler (member-access dispatch alongside `COLLECTION_AGGREGATION_MEMBERS`/`PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS`).

### Phase 2 — Eval enumeration (one level)

- **Task β — Eval: `children` + `members` enumeration over the compiled containment template (declaration order; collection-sub flatten for `members`; `aux` included; undef-count → zero elements).**
  - **Observable signal:** CLI eval — `count(self.children)` and `count(self.members)` on a multi-sub structure (2 named + a `sub bolts : List<Bolt>` count==4 + an `aux` jig) return `3` and `6` respectively; an undef-count collection contributes a slot to `children` but zero members and does not panic. Eval unit + a `.ri`. **Intermediate** (unlocks γ, ε) but CLI-observable. `grammar_confirmed=true`.
  - **Prereqs:** α.
  - **Crates:** reify-eval (extend reflective resolution in `engine_purposes.rs`-style path to general structures; collection flatten).

### Phase 3 — Transitive walk

- **Task γ — Eval: `descendants` transitive pre-order DFS over the containment tree (schema-only walk; reuses β's per-node enumeration recursively).**
  - **Observable signal:** CLI eval — `count(self.descendants)` on a 3-level assembly (`arm→motor→shaft`) equals the total node count at all depths; pre-order determinism asserted by an eval test; an `aux` descendant is included. `.ri` + eval test. **Intermediate** (unlocks ε), CLI-observable. `grammar_confirmed=true`.
  - **Prereqs:** β.
  - **Crates:** reify-eval (recursion over child templates; depth-guarded to avoid runaway on degenerate self-reference).

### Phase 4 — Trait filter

- **Task δ — Compiler + eval: `filter(list_of_entity_refs, Trait)` conformance filter (trait-name 2nd arg → transitive `sub_structure_traits` membership; unknown trait → diagnostic).**
  - **Observable signal:** CLI eval — `filter(self.descendants, Bolt)` over a tree mixing `Bolt` and non-`Bolt` subs returns exactly the conforming subset (`count` matches hand-count); `filter(self.descendants, NotATrait)` emits a named compile diagnostic, no panic. `.ri` + tests. **Intermediate** (unlocks ε), CLI-observable. `grammar_confirmed=true`.
  - **Prereqs:** α (typing of `filter` with a trait 2nd arg), γ (something to filter).
  - **Crates:** reify-compiler (filter-with-trait-arg typing), reify-eval (conformance dispatch).

### Phase 5 — Integration gate (BOM end-to-end — the headline)

- **Task ε — Integration example + gate: assembly BOM / conformance roll-up over a multi-level model with `aux` + collection-subs.**
  - **Observable signal:** `examples/structural_query_bom.ri` (CI-run): a 2-level assembly with named subs, a `sub bolts : List<Bolt>`, and an `aux` fixture; evaluates `let part_count = count(self.members)`, `let bolt_count = count(filter(self.descendants, Bolt))`, and `constraint forall m in self.members: determined(m)`. CLI eval shows `part_count` = full member count **including** the `aux` fixture and **flattening** the bolt collection, and `bolt_count` = exactly the `Bolt`-conforming descendants. **Leaf — the integration gate** (§8.2 headline). `grammar_confirmed=true`.
  - **Prereqs:** β, γ, δ.
  - **Crates:** examples/, reify-eval (any wiring gaps surfaced by the end-to-end run).

### Phase 6 — `List.generate` combinator (parallel track)

- **Task ζ — `generate(n, lambda)` combinator (free-fn): apply lambda over `0..n-1`, collect to `List`; `n=0 → []`; element type from body; `n<0`/non-int → diagnostic.**
  - **Observable signal:** CLI eval — `examples/generate_bolt_circle.ri`: `let positions = generate(bolt_count, |i| point3(radius*cos(i*2*pi/bolt_count), radius*sin(i*2*pi/bolt_count), 0mm))` yields a `List` of `bolt_count` correctly-computed `point3`s (golden values for a known `bolt_count`); `generate(0, |i| i) == []`. `.ri` + eval/compiler tests. **Leaf.** `grammar_confirmed=true`.
  - **Prereqs:** none (independent track — lambda eval + list construction both pre-exist).
  - **Crates:** reify-stdlib (the `generate` builtin) or reify-eval (combinator dispatch), reify-compiler (typing: result `List<body_type>`, index `Int`).

### Dependency view

```
α → β → γ ─┐
α ─────────┴→ δ → ε        (structural-query spine + integration gate)
ζ                          (List.generate — independent)
```

Cross-PRD (declared, §5): a **deferred follow-up** (NOT in this batch) wires key-exposure through traversal, gated on `keyed-collection-identity.md`. The structural-query batch itself has **no hard cross-PRD edge** — it ships standalone on today's grammar and the existing containment tree.

---

## §9 — Out of scope (follow-ups)

- **Key exposure through traversal** — making each traversed element of a `Keyed<T>` sub-collection carry/expose its `String` key (e.g. `key(m)` over `self.members`). Declared §5; **gated on `keyed-collection-identity.md`**; deferred follow-up, not this batch.
- **Lambda-predicate filter over entity-ref lists** — `filter(self.descendants, |m| m.area > 5mm)`. v1 ships trait-name filter (the §18.2 headline); value-predicate filter over entity refs is a tactical extension.
- **`product_members` / `aux`-excluding view** — a separate accessor that excludes `aux` (product-only BOM). v1 `members` includes `aux` (§2.1); an `aux`-excluding view is additive future work.
- **Upward / sibling traversal** — `parent`, `siblings`, `ancestors`. v1 is downward-only (`children`/`members`/`descendants`), matching the spec §18.2 "walk an entity's sub-tree."
- **Connectivity-graph traversal** — walking the `connect` assembly-topology graph (spec §6.1) rather than the containment tree. Distinct feature.
- **Geometry-from-descendants sugar** — a one-liner to union/aggregate geometry over `filter(self.descendants, T)`. Reachable today by combining traversal + per-descendant single-level geometry access; dedicated sugar is future work and would interact with the deferred nested-dot-access gap (§0.3) — explicitly out.
- **Full `List` combinator suite** (`fold`/`map`/`concat` as free fns over value lists) — `generate` is the §5.9 gap this PRD closes; the broader combinator suite (whose spec method-form is itself a GR-040 fiction) is a separate cleanup.

---

## §10 — Open (tactical) questions

1. **Element value-kind** — reuse the purpose reflective `EntityRef`/`ValueRef` cell kind verbatim, or introduce a `StructureRef`? Default reuse (§2.5). Pin at α/β.
2. **`descendants` depth guard** — the recursion needs a guard against pathological self-referential schemas. Reuse the existing recursive-unfold depth machinery (cf. `m9_combined.ri` BracketTree depth-gated unfolding) or a fixed cap? Tactical; decide at γ.
3. **Trait-filter diagnostic wording** (`E_*` code) for unknown trait name in `filter(coll, BogusTrait)`. Decide at δ.
4. **`generate` home crate** — `reify-stdlib` builtin (alongside other `eval_builtin` free fns) vs a `reify-eval` combinator-dispatch arm (it takes a lambda, which most stdlib builtins don't). Lean toward eval-side dispatch since it evaluates a lambda; decide at ζ.
5. **`children` vs `members` for non-collection subs** — for a structure with only single (non-collection) subs, `children` and `members` are identical node sets. Confirm that's the intended (harmless) degeneracy and documented, not a bug. Tactical.

---

## DESIGN FORKS FOR LEO

Resolved by reasoned default (AskUserQuestion unreachable). Each is implementable as-defaulted; flagged for ratification.

1. **`children` vs `members` distinction (§2.1).** Default: `children` = structural slots (a `List<T>` is one child); `members` = flattened actual entities (each element is a member); `descendants` = transitive `members`. *Alt:* make them synonyms (but the spec lists both names, so they should differ). **Load-bearing** — it defines the whole surface.
2. **`aux` inclusion in `members`/`descendants` (§2.1, §3).** Default: **included** (`aux` is a surfacing axis, not a membership axis; a jig is still a contained member for BOM/conformance). *Alt:* exclude `aux` (treat as "not a real member") — but that conflates surfacing with membership, which sub-placement deliberately separated, and you'd lose `aux` parts from a BOM. **Load-bearing seam with sub-placement.**
3. **Trait-filter spelling (§2.2).** Default: overload free-fn `filter(coll, Trait)` (matches spec §18.2 "filterable by trait", one combinator). *Alt:* dedicated `conforming(coll, Trait)` (avoids overloading `filter`'s 2nd arg across lambda-vs-trait).
4. **`generate` spelling (§2.3).** Default: free-fn `generate(n, |i| ...)` (`List.generate` is a grammar fiction — does not parse). *Alt:* `list_generate(...)`. Index type default `Int`.
5. **Return type / element kind (§2.5).** Default: `List<EntityRef>` reusing the purpose reflective-cell kind. *Alt:* a new `StructureRef` kind.
6. **Traversal ordering (§2.4).** Default: declaration order (children/members), pre-order DFS (descendants) — most useful for BOM/inspection, deterministic from the template. *Alt:* sorted order (hash-stability parity with purpose reflective-access, which sorts).
7. **Keyed-collection key exposure (§5, §9).** Default: **deferred** — v1 flattens keyed-collection elements like any collection; key-exposure through traversal is a follow-up gated on keyed-collection landing. *Alt:* co-design key exposure now (adds a hard cross-PRD edge and blocks structural-query on keyed-collection).

---

*Decompose note:* under decompose-mode, each task files with `planning_mode=True`, carries `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata, wires the §8 dependency edges, and the batch lands `deferred` (orchestrator STOPPED — do NOT flip to pending). The orchestrator does not yet read those metadata fields (F-infra follow-up substrate).
