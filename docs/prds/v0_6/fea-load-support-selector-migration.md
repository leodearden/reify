# PRD — FEA Load/Support String→Selector Migration

**Status:** **deferred → ready to activate** after the topology-selector constructors (tasks **4118 / 4119 / 4120**) **and** the selector→node-set resolver (task **4092**) land on main. **Approach B + H** (FEA is a G5 load-bearing seam; contract + two-way boundary tests below). Authored 2026-06-08.

**Origin.** Interactive `/unblock` of task **4093** (2026-06-08, escalation `esc-4093-148`). 4093 bundled two pieces; its **PART 2** — "retire the `String` placeholder selector fields on `PointLoad`/`FixedSupport`/`PressureLoad` in favor of typed selectors" — was dropped as a **false premise** (no `.ri`-facing selector constructor exists *yet*; the placeholders were shipped deliberately by tasks 2881/2882) and 4093 was re-scoped to PART 1 (the `List<Real>`→`List<Load>`/`List<Support>` signature tightening). **This PRD owns the dropped PART 2 work, done properly as a gated consumer.**

**Design source.** `docs/prds/topology-selector-value-type.md` (the selector value/type substrate; B+H, authored 2026-05-31). This PRD is the **deliberately-decoupled FEA follow-on consumer** that PRD names in its §10 / §2-G1 / §8 ("FEA loads" row). It does **not** re-author the selector type — that exists (tasks **4116/4117 done**). It **consumes** the constructors (4118/4119) + node-set resolver (4092), and **extends** the selector type system in two narrow ways the original PRD explicitly deferred (Vertex kind; kind-agnostic param — see §3 D2/D3 and the §8 G4 note).

**Code anchors** are as of `HEAD b2ed6d2587`; main moves fast — **re-locate every symbol at implementation time** (cite-by-symbol, the line is a hint).

---

## 1. Background — the stringly-typed seam the substrate now lets us close

The FEA Load/Support hierarchy (tasks 2881/2882, *done*) ships as `structure def`s whose geometry-target fields are **opaque `String` placeholders**, validated at best at solve time (`crates/reify-compiler/stdlib/fea_multi_case.ri`, re-locate — line hints as of authoring):

| structure def | field | current type | natural selector kind |
|---|---|---|---|
| `PointLoad : Load` (:301) | `point` (:302) | `String = ""` | **Vertex / 0-D** (see D2 — kind did not exist) |
| `FixedSupport : Support` (:335) | `target` (:336) | `String = ""` | **any of Face/Edge/Vertex** (a clamp region; see D3) |
| `PressureLoad : Load` (:368) | `face` (:370) | `String = ""` | **FaceSelector** |
| `TractionLoad : Load` (:396) | `face` (:397) | `String = ""` | **FaceSelector** |
| `BodyForce : Load` (:426) | `body` (:427) | `String = ""` | **BodySelector** |

Because `face`/`body`/`target`/`point` are bare `String`s, `PressureLoad(face: <a body selector>)` is **not** a type error — the mistake survives compilation and surfaces (at best) at solve time, or silently embeds a wrong-dimensioned target. The comments in that file already name the target types and point the migration here (topology-selector PRD task ζ, *companion prose*).

**Why this is now actionable (and was not when 2881/2882 shipped):** the typed-selector *substrate* landed (`Value::Selector`/`Type::Selector`/`SelectorKind{Face,Edge,Body}` — tasks 4116/4117, *done*, `crates/reify-core/src/ty.rs:37`, `crates/reify-ir/src/value.rs`), and the `.ri`-facing **constructors** (`faces_by_normal`/`edges_at_height`/`faces`/`edges` → `Value::Selector`; `face()/edge()/body()` named; `resolve()`; the `ResolveSelector` coercion node) are **queued** as tasks 4118 (γ) / 4119 (δ) / 4120 (ε). Selector → **FE node-set** resolution is owned by task **4092**. Once those land, the migration is a localized consumer change — *plus* two narrow type-system extensions Leo elected to take on here (§3).

`reify check examples/fea_cantilever_smoke.ri` today constructs loads/supports with string literals:

```reify
let tip_load = PointLoad(point: "tip", force: 1000.0)   // "tip" = a named vertex, untyped
let mount    = FixedSupport(target: "root")             // "root" = a named face, untyped
```

---

## 2. What a user observes when this lands (G1 + G2)

**G1 — consumers (concrete, present today):**
- The FEA Load/Support `structure def`s in `crates/reify-compiler/stdlib/fea_multi_case.ri` (the producer surface migrated here).
- `examples/fea_cantilever_smoke.ri` (migrated to typed selectors; runs in CI).
- The bracket dogfood (task **2930**) and arbitrary-geometry FEA (`structural-analysis-fea.md`), which gain compile-time target-kind safety.
- Downstream: task **4092** resolves the typed targets to FE node sets (this PRD stops at handing 4092 a typed `Selector`, not a `String`).

**G2 — user-observable leaf signals (each a committed `.ri` in CI or a diagnostic):**
1. `PressureLoad(face: faces_by_normal(b, [0,0,1], 1deg))` **compiles** and the pressure is applied to the +Z face's node set (volume/stress result reflects the selected face). *(stdlib `.ri` example in CI.)*
2. `PressureLoad(face: body(b, "blob"))` is a **compile-time** `E_SELECTOR_KIND_MISMATCH` ("`FaceSelector` expected, `BodySelector` found"). *(compile-fail fixture.)*
3. `PointLoad(point: vertex(b, "tip"), force: 1000.0)` compiles; `PointLoad(point: faces(b))` is a compile-time `E_SELECTOR_KIND_MISMATCH`. *(needs the Vertex kind — D2.)*
4. `FixedSupport(target: face(b, "root"))` **and** `FixedSupport(target: edge(b, "spine"))` both compile (kind-agnostic target accepts any kind — D3); a non-selector target is rejected. *(stdlib `.ri` example + compile-fail fixture.)*
5. `examples/fea_cantilever_smoke.ri` (migrated) `reify check`s clean and the elastic solve applies the BC at the typed-selected node set.

---

## 3. Resolved design decisions

| # | Decision | Rationale |
|---|---|---|
| **D1** | **Migrate the unambiguous load fields to single-kind selectors:** `PressureLoad.face` / `TractionLoad.face` → `FaceSelector`; `BodyForce.body` → `BodySelector`. The fields change from `param … : String = ""` to a selector-typed param (default handling per §4.3). | These map 1:1 onto the existing `SelectorKind{Face,Body}`; pure consumer wiring on the queued substrate. |
| **D2** | **Add `SelectorKind::Vertex` (0-D)** + `vertex(g, name)` / `vertices(g)` constructors, so `PointLoad.point : VertexSelector`. **This reopens topology-selector PRD D2** ("Vertex deferred — no FEA need"); FEA's point-load *is* that consumer, so the deferral premise no longer holds. (Leo, interactive /unblock 2026-06-08.) | Keeps the named-target idiom (`vertex(b,"tip")`), not coordinate entry; a point load is naturally a *located* feature on the realized body, so a topology selector is the consistent model. The extension follows the existing K1/K2/K3 invariants + `Display`→`"VertexSelector"`, dim 0. |
| **D3** | **`FixedSupport.target` is kind-agnostic** — it accepts a selector of **any** kind (Face/Edge/Vertex/Body), because a clamp can fix a face, an edge, or a vertex. Introduce a kind-agnostic selector **param acceptance** (a param typed as the bare `Selector` super-form accepts any `Selector(k)`), mirroring the existing one-directional `Selector(_) → List<Geometry>` acceptance in `type_compatible`. (Leo, interactive /unblock 2026-06-08.) | A support region is genuinely polymorphic over dimensionality; forcing one kind would either over-restrict (FaceSelector-only) or need N overloads. Kind-agnostic acceptance is the minimal type-system addition; downstream 4092 maps *any* resolved handle → its node set uniformly. |
| **D4** | **Construct-time kind safety is the headline win.** A wrong-kind target on a single-kind field (`PressureLoad.face : FaceSelector`) is a compile-time `E_SELECTOR_KIND_MISMATCH` (the diagnostic introduced by topology-selector α/β); the kind-agnostic `FixedSupport.target` rejects only **non-selector** args. | Moves the failure from solve time to compile time — the motivating G2 signal. |
| **D5** | **Migrate `examples/fea_cantilever_smoke.ri` + `fea_multi_case.ri` worked examples** to the typed constructors in the same batch as the field-type change (no stale uncompilable example). `LoadCase` bundling is orthogonal and owned by 4093 PART 1. | A field-type change that left the canonical example uncompilable would fail the example's own CI check (the C-07 fake-done trap). |
| **D6** | **Gated, not blocked.** This PRD assumes the constructors (4118/4119) + `resolve()` + `ResolveSelector` (4118) + selector→node-set (4092) and **depends on them**; it builds none of them. Until they merge, it stays `deferred`. | Honest G3/G6: the real precondition is "constructors/resolver pending," not "substrate absent." |
| **D7** | **Named-leaf resolution is delegated** (inherits topology-selector D8): `face(b,"root")` / `vertex(b,"tip")` build `Named` leaves; name→sub-shape handle resolution is owned by `persistent-naming-v2` (soft seam; interim `resolve_unique_by_tag`, else `W_TOPOLOGY_TAG_STALE` + `[]`). | The construct-time kind-safety win does not depend on full named resolution; the cantilever's `"tip"`/`"root"` are the unique-tag interim case. |

---

## 4. Contract (B + H)

This PRD owns two seams: **(A)** two narrow selector-type-system extensions (Vertex kind; kind-agnostic param acceptance) and **(B)** the FEA stdlib field migration that consumes them. An architect reading this section can implement both without further design discussion.

### 4.1 Type-system extension A1 — `SelectorKind::Vertex` (extends topology-selector substrate)

```rust
// crates/reify-core/src/ty.rs  — extend the existing enum (4116 shipped Face/Edge/Body)
pub enum SelectorKind { Face, Edge, Body, Vertex }   // Vertex dimensionality = 0
// dimensionality(): Vertex => 0   (Edge=1, Face=2, Body=3)
// Display:          Vertex => "VertexSelector"
```

- `Type::Selector(SelectorKind::Vertex)` and `Value::Selector` with `kind = Vertex` follow the **same** K1 (kind closure) / K2 (kernel-free construction) / K3 (canonical-order + dedup) invariants as the shipped kinds.
- **Constructors** (mirroring 4118/4119): `vertex(g, name)` → `Named` leaf (`VertexSelector`); `vertices(g)` → `All` leaf (`VertexSelector`). No predicate vertex selectors in v1 (no FEA need beyond named/all).
- **`resolve()`** (extends `topology_selectors.rs`): a `Vertex` `Leaf{All}` → `extract_vertices(kernel, target)` (add beside `extract_faces`/`extract_edges`); `Leaf{Named}` → delegated (D7). Vertex set ops dedup by `GeometryHandleId` (K3).

### 4.2 Type-system extension A2 — kind-agnostic selector param acceptance

A param may be typed as a **kind-agnostic** selector that accepts a `Selector(k)` value of **any** `k`. Minimal mechanism (exact spelling is §11 tactical):
- `type_compatible`: a kind-agnostic selector param is compatible with `Type::Selector(k)` for **every** `k` (one-directional: any concrete selector → the agnostic param, never the reverse).
- The existing rule `Type::Selector(a)` compatible-with `Type::Selector(b)` **iff** `a==b` is **unchanged** for single-kind params (D1/D2 fields keep exact-kind checking).
- Coercion to node-sets is kind-uniform: the agnostic target resolves via the **same** `resolve(selector, kernel) → Vec<GeometryHandleId>` and 4092 maps the handles → node set regardless of kind.

### 4.3 FEA field migration B (`fea_multi_case.ri`)

| structure def | field | from | to |
|---|---|---|---|
| `PressureLoad` | `face` | `String = ""` | `FaceSelector` |
| `TractionLoad` | `face` | `String = ""` | `FaceSelector` |
| `BodyForce` | `body` | `String = ""` | `BodySelector` |
| `PointLoad` | `point` | `String = ""` | `VertexSelector` (D2) |
| `FixedSupport` | `target` | `String = ""` | kind-agnostic `Selector` (D3) |

Default handling: a selector-typed field has **no** `String = ""` placeholder default; an unset target is a missing-required-field or an explicit `undef` per the structure-instance default rules (`reference_undef_default_trait_only_not_structure_params` confirms `= undef` defaults now compile on structure params — re-verify at implementation time). The runtime trampoline (`crates/reify-stdlib/src/loads.rs` / `supports.rs`, `validate_selector_target` — task 3076 narrowed it) updates its accept set from "opaque String/Map" to the typed selector value.

### 4.4 Example migration (the worked idiom)

```reify
// examples/fea_cantilever_smoke.ri (migrated)
let tip_load = PointLoad(point: vertex(beam, "tip"), force: 1000.0)   // VertexSelector
let mount    = FixedSupport(target: face(beam, "root"))              // FaceSelector → kind-agnostic target
```

---

## 5. Boundary-test sketch (B + H) — faces both sides of the seam

| # | Scenario | Precondition | Postcondition (assert) | Side |
|---|---|---|---|---|
| BT1 | Single-kind field rejects wrong kind | `.ri`: `PressureLoad(face: body(b,"x"))` | compile fails, one `E_SELECTOR_KIND_MISMATCH` naming `FaceSelector` expected / `BodySelector` found; span at the arg | producer (type-checker) |
| BT2 | Single-kind field accepts right kind + resolves | `PressureLoad(face: faces_by_normal(b,+Z,1deg))` | compiles; `resolve()` yields the +Z face handle(s); 4092 maps to the face node set | consumer (coercion + node-set) |
| BT3 | Vertex kind end-to-end (A1) | `PointLoad(point: vertex(b,"tip"))` | compiles as `VertexSelector`; `extract_vertices`/named resolve yields the vertex handle; `PointLoad(point: faces(b))` → `E_SELECTOR_KIND_MISMATCH` | producer + consumer |
| BT4 | Kind-agnostic target accepts any kind (A2) | `FixedSupport(target: face(b,"r"))` **and** `FixedSupport(target: edge(b,"s"))` **and** `…vertex(b,"v")` | all three compile; a **non-selector** target (e.g. a `Real`) is rejected | producer (type-checker) |
| BT5 | Migrated cantilever runs in CI | `examples/fea_cantilever_smoke.ri` (migrated) | `reify check` clean; elastic solve applies the BC at the typed-selected node set; tip-deflection within the existing FEA tolerance | consumer (end-to-end) |
| BT6 | Named-leaf interim (D7) | `face(b,"nope")` with no matching tag | resolves to `[]` + one `W_TOPOLOGY_TAG_STALE`; no panic | producer (delegated seam) |
| BT7 | Vertex construction is kernel-free (K2) | build `vertex(b,"tip")` with a counting kernel, do not resolve | zero kernel queries during construction | producer (invariant) |

The integration-gate task (§9 leaf) names this table as its observable signal (closes G2).

---

## 6. Substrate verification (G3)

| Assumed capability | Status | Evidence / owner |
|---|---|---|
| `Value::Selector` / `Type::Selector` / `SelectorKind{Face,Edge,Body}` | **exists** | tasks 4116/4117 *done*; `reify-core/src/ty.rs:37`, `reify-ir/src/value.rs` |
| `face()/edge()/body()` + predicate constructors → `Value::Selector`; `resolve()`; `ResolveSelector` coercion node | **queued (gate)** | tasks **4118** (γ), **4119** (δ); this PRD depends on them |
| `E_SELECTOR_KIND_MISMATCH` diagnostic | **exists/queued** | introduced by topology-selector α/β (4116/4117 area); confirm on main at activation |
| Selector → FE node-set mapping | **owned elsewhere (gate)** | task **4092** (pending); this PRD hands 4092 a typed `Selector` instead of a `String` |
| `vertex()/vertices()` + `SelectorKind::Vertex` + `extract_vertices` | **ABSENT → this PRD adds it (A1, D2)** | strict extension of the 4116 substrate; reopens topology-selector D2 |
| kind-agnostic selector param acceptance | **ABSENT → this PRD adds it (A2, D3)** | extends `type_compat.rs` selector rules |
| `face(b,"top")`, `vertex(b,"tip")`, `PressureLoad(face: …)` parse | **exists — grammar gate N/A** | plain function calls + named-args (topology-selector PRD §6/D7); type-name identifiers in type position already parse. **No novel syntax.** |
| `= undef` / missing default on a structure param | **exists** | `reference_undef_default_trait_only_not_structure_params` (re-verify) |

No unverified assumed substrate remains: every capability either exists, is a named gated prerequisite, or is an explicitly-owned extension added here.

---

## 7. Pre-conditions for activating

- **Hard gates (decompose only after these merge to main):** tasks **4118** (predicate constructors + `resolve()` + `ResolveSelector`), **4119** (composition + `face()/edge()/body()` named constructors), **4120** (boundary-test gate), and **4092** (selector → FE node-set). Until all four land, this PRD is `deferred`.
- **Soft seam:** full `Named`-leaf resolution depends on `persistent-naming-v2` (D7) — interim behavior is specified; not a hard gate.
- **No grammar change** — G3 grammar-gate N/A (§6).
- **Coordinate with 4093 PART 1** (the `List<Real>`→`List<Load>`/`List<Support>` tightening + `LoadCase` retirement): orthogonal (signature vs. field types) but touches the same `fea_multi_case.ri` / `solver_elastic.ri` files — sequence after 4093 lands to avoid a merge collision, or rebase.

---

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/topology-selector-value-type.md` (4116–4120) | this **consumes** + **extends** | `Value::Selector`/`Type::Selector` substrate + `face()/edge()/body()`/predicate constructors + `resolve()` + `ResolveSelector` | topology-selector PRD | 4116/4117 done; 4118/4119/4120 pending (hard gate) |
| topology-selector PRD **D2** (Vertex deferred) | this **reverses** (A1/D2) | adds `SelectorKind::Vertex` + `vertex()/vertices()` + `extract_vertices` as a strict extension | **this PRD** | new |
| topology-selector PRD selector type rules | this **extends** (A2/D3) | kind-agnostic selector param acceptance in `type_compat.rs` | **this PRD** | new |
| task **4092** (selector → FE node-set) | this **produces-for** | hands 4092 a typed `Selector` (any kind) instead of a `String`; 4092 maps handles → node set | task 4092 | pending (hard gate) |
| `docs/prds/v0_2/persistent-naming-v2.md` | this **consumes** (soft) | `Named`-leaf name→handle resolution (D7) | persistent-naming-v2 | blocked (interim behavior) |
| `docs/prds/v0_6/engine-unified-build-dag.md` (Part 2) | **coherence note, no conflict** | reworks *when* selectors resolve (P4 whole-template pass → per-cell worklist executor); this PRD's `resolve()` is unchanged in *what* it computes. The migrated FEA selectors flow through whichever scheduler is active. | Build-DAG Part 2 | independent |
| task **4093** PART 1 (sibling) | **orthogonal** | `List<Real>`→`List<Load>`/`List<Support>` + `LoadCase` retirement; same files | task 4093 | pending (re-scoped) |
| `structural-analysis-fea.md` | this **completes** | the typed-Load/Support consumer chain | FEA PRD | ongoing |

**⚠️ G4 seam-ownership flag for Leo (curator dedupe is OFF).** Extensions A1 (Vertex kind) and A2 (kind-agnostic param) technically extend `topology-selector-value-type.md`'s substrate, which is still mid-flight (4118/4119/4120 pending). Two ownership options:
- **(chosen here)** this consumer PRD owns A1/A2 as strict, invariant-respecting extensions — clean because the topology-selector PRD explicitly *deferred* Vertex (D2, no owner) and never anticipated kind-agnostic params, and amending an in-flight decompose batch is messier.
- **(alternative)** fold A1/A2 into `topology-selector-value-type.md` as an amendment (new tasks under that PRD), keeping all selector-type substrate in one place.

If you prefer the alternative, A1/A2 move out of this PRD's decomposition and become topology-selector tasks this PRD then *gates on* (like 4118/4119). **No duplicate task should be filed for A1/A2 in both places** — decide ownership before decompose.

---

## 9. Decomposition plan (one bullet per task; **decompose deferred until §7 gates land**)

> **Do not queue yet.** Gates 4118/4119/4120 + 4092 are pending and the TaskCurator dedupe is degraded. Re-run `/prd` decompose mode when the gates merge; author the capability manifest then.

Approach **B + H** (FEA seam + a selector-type-system extension). Suggested DAG: **A1 → A2 → Bmig → BT(gate)**; A1/A2 are the substrate extensions (or move to topology-selector PRD per §8), Bmig is the field+example migration, BT is the integration gate.

- **A1 — `SelectorKind::Vertex` extension.** Add the `Vertex` enum variant (dim 0, `Display`→`"VertexSelector"`); `vertex()/vertices()` constructors; `extract_vertices` + `resolve()` vertex arm; K1/K2/K3 for the new kind. *Modules:* `reify-core/src/ty.rs`, `reify-ir/src/value.rs`, `reify-eval/src/topology_selectors.rs`, constructor wiring (mirrors 4118/4119). *Signal:* **intermediate** — `vertex(b,"tip")` type-checks as `VertexSelector`; unit-covers K1 rejection + kernel-free construction (BT3/BT7 substrate). *grammar_confirmed: true.*
- **A2 — kind-agnostic selector param acceptance.** Add the kind-agnostic selector type-name + `type_compatible` "any-kind" acceptance (one-directional); leave single-kind exact-equality unchanged. *Modules:* `reify-compiler/src/type_compat.rs`, type-name resolver, `reify-eval` `value_type_kind_matches`. *Signal:* **intermediate** — a kind-agnostic param accepts `face()/edge()/vertex()/body()` and rejects non-selectors (BT4 substrate). *grammar_confirmed: true.*
- **Bmig — FEA field migration + example migration.** Change the five fields (§4.3) from `String` to their selector types; update `validate_selector_target` accept set; migrate `examples/fea_cantilever_smoke.ri` + `fea_multi_case.ri` worked examples to typed constructors. *Modules:* `crates/reify-compiler/stdlib/fea_multi_case.ri`, `examples/fea_cantilever_smoke.ri`, `crates/reify-stdlib/src/loads.rs` + `supports.rs`. *Signal:* **leaf** — `PressureLoad(face: faces_by_normal(...))` compiles; wrong-kind → `E_SELECTOR_KIND_MISMATCH`; migrated cantilever `reify check`s clean (BT1/BT2/BT5). *Prereq:* A1, A2, 4118/4119, 4092. *grammar_confirmed: true.*
- **BT — boundary-test integration gate.** Implement §5 BT1–BT7 facing both sides (compile-fail fixtures + resolving `.ri` examples + the migrated cantilever end-to-end). *Modules:* `crates/reify-eval/tests/` (+ `.ri` fixture dir), `examples/`. *Signal:* **leaf / integration-gate** — the §5 table is green end-to-end. *Prereq:* Bmig. *grammar_confirmed: true.*

---

## 10. Out of scope

- The selector *type/constructors/resolve* themselves — owned by `topology-selector-value-type.md` (4116–4120); consumed, not rebuilt.
- Selector → FE node-set mapping — owned by task **4092**; this PRD stops at handing 4092 a typed `Selector`.
- Full persistent name → sub-shape resolution — owned by `persistent-naming-v2` (D7 interim here).
- **Predicate** vertex selectors (e.g. "vertices by curvature") — only `vertex()`/`vertices()` (named/all) in v1; no FEA need.
- 4093 PART 1's `List<Real>`→`List<Load>`/`List<Support>` tightening + `LoadCase` retirement — sibling, orthogonal.
- Multi-kind *single-field* validation beyond the kind-agnostic accept (e.g. "this support accepts Face or Edge but not Body") — kind-agnostic is all-or-nothing in v1; a constrained kind-set is a follow-up if a consumer needs it.

---

## 11. Open questions (tactical — surfaced, not blocking)

1. **Kind-agnostic type spelling (A2).** Representation of "any selector" param: bare `Selector` type-name mapping to a `Type::Selector(None)` (widen `SelectorKind` to `Option`) vs. a distinct `Type::AnySelector` marker vs. a compatibility-only rule with no new `Type` variant. **Suggested:** the compatibility-only rule if it avoids touching every `Type::Selector(_)` match site; else `Option<SelectorKind>`. Decide during A2.
2. **`vertex()` constructor name (D2).** `vertex` is a fairly generic identifier; confirm it doesn't shadow common user bindings (cf. topology-selector PRD Open-Q on `body`). **Suggested:** keep `vertex`; revisit on collision. Decide during A1.
3. **`PointLoad.point` default.** Whether an unset `point` is a required-field error or an explicit `undef`-default (re-verify structure-param `= undef` support). Decide during Bmig.
4. **A1/A2 ownership (§8 flag).** This-PRD-owns vs. fold-into-topology-selector. **Suggested:** this PRD owns (clean extension); confirm with Leo before decompose. Decide before decompose.
5. **Coverage of `direction` on `PressureLoad`** (`direction : String = "normal"`) — left as `String` (it's a mode, not a topology target). Confirm no enum-tightening is wanted here. Decide during Bmig.
