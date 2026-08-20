# PRD — FEA Load/Support String→Selector Migration

**Status:** **ACTIVE** (2026-07-20) — all hard gates (4118/4119/4120/4092/4368/4369) **landed**. **Approach B + H** (FEA is a G5 load-bearing seam; contract + two-way boundary tests below). Authored 2026-06-08; re-gated 2026-07-20.

> **⟢ 2026-07-20 re-gate (design session Leo + escalation-watcher).** Reconciles this PRD with the true
> post-4370 state and Leo's "complete work, source-breaking, disallow-string, truly-consume" ruling:
> 1. **Bmig (4370) landed NARROWED** (esc-4370-24): only `PressureLoad.face → Option<FaceSelector> = none`,
>    and it kept a **legacy `String` transition arm** (`extract_pressure_face_name`'s `Value::String`
>    branch) for non-breaking migration. The other 5 fields (§4.3) are still `String` on main and are owned
>    by no landed/pending task. **This re-gate adds a `Bmig2` leaf** (§9) that completes them.
> 2. **Disallow string (Leo).** Source-breaking is now accepted (before external users). Bmig2 **removes**
>    the `String` transition arms and migrates **all** bare-string call sites (~159 sites / ~60 files) to
>    typed ctors. Enforcement rides on the general **struct-ctor field-type conformance chokepoint**
>    (`docs/prds/struct-ctor-field-type-conformance.md`) — a NEW cross-PRD dependency (§8); this PRD does
>    not build the enforcement, it consumes it.
> 3. **Truly consume (Leo).** The migrated selectors must actually drive the solver's FE node-sets (resolve
>    `vertex()/face()/...` → node-set → applied load/BC), not the current coordinate heuristics. Today
>    `extract_loads` ignores `PointLoad.point` entirely and `FixedSupport.target` flows two String/handle
>    paths. Bmig2 wires 4092's selector→node-set into `extract_loads`/`extract_supports` — a real
>    solver-integration deliverable, split into its own leaf if scope warrants (§9).
> 4. **A1/A2 landed** as tasks **4368/4369** — §4.1/§4.2 are now *landed substrate*, not "this PRD adds."

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
| **D8** *(2026-07-20)* | **Disallow string — source-breaking.** Bmig2 **removes** the legacy `String` transition arms (4370 kept `extract_pressure_face_name`'s `Value::String` branch; the same pattern must NOT be reintroduced for the other fields) and migrates **every** bare-string call site (`PointLoad(point:"tip")` ×67, `FixedSupport(target:"root")` ×85, + Traction/BodyForce/Pinned — ~159 sites / ~60 files) to typed ctors (`vertex()/face()/edge()/body()`). A bare string in a selector field is a **compile error**, emitted by the general struct-ctor conformance chokepoint (D-new-11). | Leo, 2026-07-20: get the breakage out before external users. A retained String arm is exactly the silent-accept the migration exists to kill. |
| **D9** *(2026-07-20)* | **Truly consume — the solver resolves the selector to its FE node-set.** Bmig2 wires 4092's `selector → Vec<GeometryHandleId> → FE node/element set` into `extract_loads`/`extract_supports` (`crates/reify-eval/src/compute_targets/elastic_static.rs`), replacing today's coordinate heuristics: `PointLoad.point` (currently **unread** — `extract_loads` :4028-4052 consumes only `force`/`direction`) applies the tip force at the resolved vertex node-set; `FixedSupport.target` (currently a kernel-less `Value::String` match `any_support_targets` :345 **and** a resolved-handle path :1705) reads the `Value::Selector`. **Split-leaf allowed:** if the kernel-less-vs-kernel resolution question (the trampoline resolves only the 6 named box faces today) warrants its own design, the true-consume wiring is a sibling leaf `Bmig2-consume` gated on Bmig2's field typing. | Leo, 2026-07-20: "anything less is a broken feature chain." Typing the field without consuming it is a phantom-green — the 4370 trap at the solver layer. |
| **D10** *(2026-07-20)* | **Field typing: required selectors where a target is mandatory; keep `PressureLoad.face` `Option`.** `PointLoad.point : VertexSelector`, `FixedSupport.target`/`PinnedSupport.target : Selector` (kind-agnostic), `TractionLoad.face : FaceSelector`, `BodyForce.body : BodySelector` are **required** (no default — a load/support with no target is meaningless; source-breaking on bare `Ctor()` is accepted). `PressureLoad.face` **stays `Option<FaceSelector> = none`** (a faceless pressure = no traction is a valid state; 4370's choice preserved). **✅ RATIFIED (Leo, 2026-07-20):** required for point/target/traction/body; `face` stays `Option`. (The chokepoint's `Option`-unwrap arm enforces both shapes; bare `Ctor()` on a required field is a compile error — accepted source-breaking.) | v0.6 §4.3 already intended "no `String=""` default → required"; 4370 deviated only for `face`. Required matches the honest-target intent; `face` is the genuine nullable case. |
| **D11** *(2026-07-20)* | **Enforcement is the general struct-ctor conformance chokepoint, not built here.** This PRD types the fields + migrates call sites; the compile-time rejection of wrong-kind / non-selector / string / pose args at those fields is delivered by `docs/prds/struct-ctor-field-type-conformance.md` (hard cross-PRD dep, §8). **Sequencing: the chokepoint lands first** (green on the pre-migration corpus); Bmig2 lands on top, enforced by the then-live chokepoint. | Reachable, general wire-site (the `validate_selector_target` plan was a false premise). Avoids duplicating enforcement in the FEA trampoline (INV-5). |

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

- **~~Hard gates~~ — ALL LANDED (2026-07-20):** tasks **4118**/**4119** (constructors + `resolve()` + `ResolveSelector`), **4120** (BT gate), **4092** (selector → FE node-set), **4368**/**4369** (A1/A2). This PRD is no longer `deferred` — it is **active** and ready to decompose the residual `Bmig2` + true-consume work (§9).
- **NEW hard dep (2026-07-20):** the general **struct-ctor field-type conformance chokepoint** (`docs/prds/struct-ctor-field-type-conformance.md`) must land its Error-flip (δ) **before** Bmig2's disallow-string enforcement is observable. Bmig2's field typing can proceed in parallel; the *rejection* signal (BT1/BT4 negatives) is gated on the chokepoint. Sequence: chokepoint δ → Bmig2 enforcement.
- **Soft seam:** full `Named`-leaf resolution depends on `persistent-naming-v2` (D7) — interim behavior is specified; not a hard gate.
- **No grammar change** — G3 grammar-gate N/A (§6).
- **4093 PART 1 landed** (the `List<Real>`→`List<Load>`/`List<Support>` tightening) — no longer a sequencing concern; re-verify no residual `LoadCase` collision at Bmig2 dispatch.

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

## 9. Decomposition plan (one bullet per task; **ready to decompose — 2026-07-20 re-gate**)

> **Ready to queue the residual (Bmig2 + Bmig2-consume + re-dep BT/4371)** once (a) Leo confirms D10
> field-typing and (b) the struct-ctor conformance chokepoint (`struct-ctor-field-type-conformance.md`)
> is filed so Bmig2 can `depends_on` its δ. A1/A2/Bmig already landed (4368/4369/4370). Re-run `/prd`
> decompose mode; author the capability manifest for the residual leaves only.

Approach **B + H**. **Landed DAG:** `4368(A1) → 4369(A2) → 4370(Bmig, narrowed)`. **Residual DAG (2026-07-20):**
`struct-ctor-chokepoint(δ) → Bmig2 → Bmig2-consume → 4371(BT)`; Bmig2 completes the field migration +
disallow-string, Bmig2-consume wires true node-set consumption, 4371 is the re-dep'd integration gate.

- **A1 — `SelectorKind::Vertex` extension. ✅ LANDED as task 4368.** (`vertex()/vertices()`, `extract_vertices`, `resolve()` vertex arm; `type_resolution.rs:560`.)
- **A2 — kind-agnostic selector param acceptance. ✅ LANDED as task 4369.** (bare `Selector` → `Type::AnySelector`; `type_resolution.rs:567`; compatible with `Selector(k)` ∀k.)
- **Bmig — FEA field migration + example migration. ✅ LANDED as task 4370 — but NARROWED** (esc-4370-24) to **`PressureLoad.face → Option<FaceSelector> = none` ONLY**, riding a new generic `hoist_nested_selectors.rs` compiler phase, and keeping a legacy `String` transition arm. The other 5 §4.3 fields did **not** migrate. *(Merged `1d886f3f45`.)*
- **Bmig2 — Complete the residual field migration + disallow string (NEW leaf, source-breaking).** Flip the remaining fields (§4.3 + `PinnedSupport.target`) to their selector types per D10: `PointLoad.point → VertexSelector`, `FixedSupport.target`/`PinnedSupport.target → Selector` (kind-agnostic), `TractionLoad.face → FaceSelector`, `BodyForce.body → BodySelector`; **remove** all legacy `String` transition arms (D8, incl. 4370's `extract_pressure_face_name` `Value::String` branch); migrate **all** bare-string call sites (~159 / ~60 files) to typed ctors incl. `examples/fea_cantilever_smoke.ri` + `fea_multi_case.ri` worked examples. *Modules:* `crates/reify-compiler/stdlib/fea_multi_case.ri`, `examples/**`, `crates/reify-*/tests/**` fixtures, `crates/reify-eval/src/compute_targets/elastic_static.rs` (extractor String-arm removal). *Signal:* **leaf** — `PointLoad(point: vertex(b,"tip"))` compiles; wrong-kind → `E_SELECTOR_KIND_MISMATCH`; a bare string at any selector field → compile error; migrated cantilever `reify check`s clean (BT3/BT4/BT5). *Prereq:* the **struct-ctor conformance chokepoint (δ)** for the reject signal (§7/§8); D10 field-typing confirmed. *grammar_confirmed: true.*
- **Bmig2-consume — Truly consume selectors in the solver (NEW leaf; D9).** Wire 4092's `selector → node/element set` into `extract_loads`/`extract_supports` (`elastic_static.rs`): `PointLoad.point` applies the tip force at the resolved vertex node-set (currently **unread**); `FixedSupport.target` reads `Value::Selector` on both the kernel-less (`any_support_targets`) and handle paths. Resolve the kernel-less-vs-kernel resolution question (trampoline resolves only 6 named box faces today) — may fold into Bmig2 or stay a sibling. *Signal:* **leaf** — a migrated FEA solve applies the BC/load at the *typed-selected* node-set; cantilever tip-deflection within existing tolerance (BT5 true-consume half). *Prereq:* Bmig2 (typed fields), 4092. *grammar_confirmed: true.*
- **BT — boundary-test integration gate. (= task 4371, re-dep.)** Implement §5 BT1–BT7 facing both sides. **Re-dep on Bmig2 + Bmig2-consume + the chokepoint (δ)** (BT3/BT4/BT5 were un-authorable on narrowed-4370 main; BT1/BT4 negatives need the chokepoint). *Modules:* `crates/reify-eval/tests/` (+ `.ri` fixture dir), `examples/`. *Signal:* **leaf / integration-gate** — the §5 table is green end-to-end. *Prereq:* Bmig2, Bmig2-consume, chokepoint δ. *grammar_confirmed: true.*

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
