# PRD — P4: Region-reference ⇄ FEA-target unification (the pose-vs-set boundary)

> **Program:** naming & selection convergence (P0–P4). **Charter/evidence:** `./00-findings.md`
> (§4 layer violation — "the road not taken: FEA intent-named regions"; §5 split-brain — namespace 4),
> the brief `./P4-region-ref-fea-selector-unification.brief.md`, and the **keystone contract**
> `./P0-region-reference-layer-model.md` (committed + decomposed 2026-06-24 — its §3 D1 / §6.2 /
> invariant 4 delegate the FEA **pose-vs-set** decision to this PRD). Authored 2026-06-24 via a
> `/prd` session (Leo + Claude). Substrate **G3-verified against current `main`** this session (§3).
>
> **Status:** active — **thin convergence delta**. The FEA String→region-reference *bridge itself* is
> owned by the **v0.6 FEA-selector migration** (`docs/prds/v0_6/fea-load-support-selector-migration.md`):
> field migration **4370 (Bmig)**, two-way boundary test **4371 (BT)**, selector→node-set **4092**, with
> type-substrate **4368/4369 done**. **P4 does NOT re-file any of that** (command + P0 §8). P4 owns the
> one P0-delegated piece v0.6 never addressed: the **pose-vs-set boundary** (a `Value::Frame` is **not**
> an FEA region target) + the coherence record. One leaf (task **4833 / P4-π**), dependency-gated.
>
> **⟢ 2026-07-20 revision (design session Leo + escalation-watcher).** Two facts below went stale and
> the reject-side *mechanism* changed — corrected in-place (§2 table, §3 rows, §4 D3, §5 π):
> 1. **Bmig/4370 landed narrowed** (Leo's ruling esc-4370-24): only `PressureLoad.face → Option<FaceSelector>`
>    migrated; `PointLoad.point`/`FixedSupport.target`/`TractionLoad.face`/`BodyForce.body`/`PinnedSupport.target`
>    remain `String` — now owned by a **v0.6 Bmig2** leaf (see that PRD's revision). "5 fields migrated" is false on main.
> 2. **The pose-vs-set reject is NOT wired at `validate_selector_target`** (that function is a red herring —
>    unreachable from the `structure def` ctor path; its `None→Value::Undef` has no diagnostic channel).
>    The reject *mechanism* is now delivered by the general **struct-ctor field-type conformance** chokepoint
>    (`docs/prds/struct-ctor-field-type-conformance.md`, committed `af8c26ed96`): its C1-row-12 pose-vs-set
>    diagnostic (`a coordinate pose is not a region target; select a face/edge/vertex instead`). **P4-π (4833)
>    is re-scoped to a *consumer/verifier*** of that chokepoint (commit the pose fixtures; assert its
>    diagnostic), not the owner of a `validate_selector_target` reject arm. D1/D2 policy (set-only; coordinate
>    loads are a named future follow-up) is **unchanged**.
>
> **Do NOT touch task 3523 or esc-3523-75/76.** Line numbers below are **snapshots at time of writing**
> — verify against `main` at dispatch.

---

## 1. Why this PRD exists (and who consumes it — G1)

FEA Load/Support geometry targets are the **fourth disconnected naming namespace** (findings §5):
opaque `target:`/`face:`/`point:` **strings** validated by `validate_selector_target`
(`crates/reify-stdlib/src/helpers.rs:214`), disconnected from the selector / role / provenance
namespaces. The convergence program collapses all four onto **one region resolver** (P0 invariant 5):
P0 D4 + P2 Thread C kill the dead `user_label` + no-op `LeafQuery::Named` user-string namespaces;
P0 D5 / 4815 deprecate the `@face("top")` string-key form; and the **v0.6 FEA-selector migration**
collapses namespace 4 by re-typing the FEA target fields `String → Selector` (4370) — so after that
chain lands, an FEA target **is** a canonical `RegionRef` (P0 D1: `Value::Selector` *is* the canonical
region reference; 4811 is a pure alias/doc reframe, no behaviour change) resolved through the **one**
selector resolver.

That collapse leaves exactly **one** question open — the one P0 explicitly handed to P4 (P0 §3 D1,
§6.2 consumer rows, **invariant 4**): a **pose** (`Value::Frame`, e.g. `@point`/`frame3(...)`) and a
**region reference** (a `Selector` — a *set*) are **distinct types with distinct meaning**; *"a
`RegionRef` (a set) is not accepted where a pose is required"* — and, the converse this PRD pins, **a
pose is not accepted where a region target is required.** v0.6 chose region-references-only for the FEA
fields (its D2: `PointLoad.point : VertexSelector`, a *named* 0-D region, not a coordinate); P0 reopened
"does FEA *also* accept a coordinate pose?" and assigned the decision here. **Resolved set-only (§4 D1).**

- **Consumer (G1):** the FEA Load/Support surface (`crates/reify-compiler/stdlib/fea_multi_case.ri`,
  `crates/reify-stdlib/src/{loads,supports,helpers}.rs`) — the **producer-orphan-free** consumer that
  exists today; and the **converged single region resolver** (P0 invariant 5) the FEA targets now route
  through. P4 introduces **no new mechanism** — it *completes the discipline* on the v0.6-migrated seam
  (the explicit pose-reject arm) and *records* the seam's convergence. No new in-engine seam (overlay
  G1 catalogue N/A — this is a stdlib trampoline + type-conformance guard, not a kernel/dispatch seam).

## 2. The G4 reality — ownership split with the in-flight v0.6 migration (the load-bearing section)

The P4 **brief** was written in the same-day briefs batch (`f2e04933db`) **without cross-checking that
the v0.6 FEA-selector migration was already decomposed and partly executed.** Verified this session,
the brief's deliverables 1–4 are **already owned and filed** by the v0.6 chain — P4 must not re-file
them:

| Brief deliverable | Already owned + filed by | Status (main, **2026-07-20**) |
|---|---|---|
| 3. FEA field migration (`String → Selector`, 5 fields) + worked-example migration | **4370** (v0.6 Bmig) + **Bmig2** (new v0.6 leaf) | **4370 done — `PressureLoad.face` ONLY** (narrowed, esc-4370-24); other 5 fields → Bmig2, pending |
| 1. `validate_selector_target` **accept-set** (add `Value::Selector`) | **4370** (v0.6 Bmig) — *but this is the red-herring site* | done (4370 added `Value::Selector`), **but irrelevant**: unreachable from ctor fields — enforcement is the struct-ctor chokepoint, not this fn |
| 2. region-reference → FE node/element-set resolution (selector → handle-set → DOF) | **4092** (structural-analysis-fea P2) | **done**; "truly consume" wiring into the solver → v0.6 Bmig2/true-consume leaf |
| 4. two-way boundary test (selector producer ↔ FEA consumer), BT1–BT7 | **4371** (v0.6 BT) | blocked (BT3/4/5 need Bmig2; BT1/4 need the struct-ctor chokepoint) |
| type substrate (`SelectorKind::Vertex`; kind-agnostic param acceptance) | **4368 / 4369** | **done** |

Because `Value::Selector` **is** the canonical `RegionRef` post-P0, **4370 accepting `Value::Selector`
already collapses the FEA string namespace onto the one resolver** — even P0 §6.2's *"FEA-target
contract row (P4 must satisfy)"* (a 2-manifold ref accepted; a 3-manifold ref to a `face:` param is a
kind error) is satisfied by **4371 BT1/BT2**.

**What is genuinely left for P4** (neither v0.6 nor P0 delivers it):

- **The pose-vs-set boundary, as a committed verification** — but the *reject mechanism* now lives in the
  general **struct-ctor field-type conformance** chokepoint (`docs/prds/struct-ctor-field-type-conformance.md`),
  not in P4. That chokepoint's C1-row-12 delivers the pose-vs-set diagnostic (`a coordinate pose is not a
  region target; select a face/edge/vertex instead`) at every selector-typed ctor field, closing the §3
  silent-accept once it lands (staged warn→error). **P4-π (4833) owns committing the negative fixtures**
  — a `Value::Frame` at `PressureLoad(face:)`, `PointLoad(point:)`, and `FixedSupport(target:)` — and
  **asserting that chokepoint diagnostic fires**. (Nuance for `point`/`target`: pre-Bmig2 those fields are
  still `String`, so a Frame there is a String-mismatch error from the chokepoint; post-Bmig2 they are
  selector-typed and it becomes the kind-specific pose-vs-set diagnostic. Either way the silent accept dies.)
- **The coherence record + the brief's `r3b` false-premise correction** (§3): the brief's deliverable-4
  *"flip the `r3b_displacement_at_selector_grammar.ri` negative fixture"* rests on a fixture that **does
  not exist on main** — P4 asserts a *real* pose-vs-set guard (delivered by the chokepoint) instead of
  "flipping" a phantom.

**Clean split (no contested seam):** the struct-ctor conformance chokepoint owns the **general reject
mechanism** (any concrete field-type mismatch, incl. pose-vs-set); the v0.6 migration owns **typing the
FEA fields** (Bmig/4370 done for `face`; Bmig2 for the rest); **P4-π owns the FEA-target *verification***
(pose fixtures + diagnostic assertion). Three owners, no shared edit site — superseding the original
"4370 accept-side / P4 reject-side at `validate_selector_target`" split, which rested on the now-corrected
false premise that `validate_selector_target` guards the ctor fields (it does not — §3).

## 3. Substrate verification (G3) — verified against `main`, 2026-06-24

| Assumed capability | Verdict | Evidence (snapshot) |
|---|---|---|
| ~~`validate_selector_target` rejects `Value::Selector` AND `Value::Frame`~~ **STALE / RED HERRING (2026-07-20)** | **now MISLEADING** | Post-4370 the fn accepts `Value::Selector` too (`helpers.rs:218`: `Value::Selector(_) \| Value::Map(_) \| Value::String(_) => Some(())`). More importantly it is **NOT the ctor-field guard**: its only callers are `supports.rs:120,138` (`DisplacementSupport`/`RollerSupport` native builtins, **zero call sites in the corpus**); the `structure def` load/support ctors never reach it, and its `None→Value::Undef` has no diagnostic channel. The real guard is the struct-ctor chokepoint (`struct-ctor-field-type-conformance.md`). |
| FEA target fields are still `String = ""` placeholders | **PARTIALLY STALE (2026-07-20)** | `PressureLoad.face` is now `Option<FaceSelector> = none` (4370). Still `String = ""`: `PointLoad.point` (`fea_multi_case.ri:316`), `FixedSupport.target` (`:355`), `TractionLoad.face` (`:447`), `BodyForce.body` (`:477`), `PinnedSupport.target` (`:380`) — owned by **v0.6 Bmig2** (pending). Re-locate at dispatch. |
| `SelectorKind::Vertex` exists (dim 0, `Display→"VertexSelector"`) | **TRUE** | `crates/reify-core/src/ty.rs:39-79` `{Face,Edge,Body,Vertex}`; `dimensionality()` 2/1/3/0 (task **4368 done**). |
| `Value::Frame` exists; `@point(x,y,z) → Value::Frame` eager/kernel-free | **TRUE** | `crates/reify-ir/src/value.rs:970-973` `Frame{origin,basis}`; `crates/reify-expr/src/lib.rs:1194-1228` builds `Value::Frame{…, basis: identity-quaternion}`. (P0 invariant 4 / P2 §2.) |
| **NEGATIVE-SENTINEL: a `Value::Frame` at an FEA target is *silently accepted* today** | **TRUE (the gap P4 closes)** | `reify check` on `structure G { let pose = frame3(point3(0mm,0mm,0mm), orient_identity()); let s = FixedSupport(target: pose) }` exits **0 + "All constraints satisfied."** with **no diagnostic** (this session). The `String`-typed field does **no** nominal arg-vs-param rejection of a `Frame` (same silent-accept class as task **4575** — overlay G3 §2). Rejection capability is **absent** today. |
| brief's `r3b_displacement_at_selector_grammar.ri` "current guard to flip" | **FALSE PREMISE** | **File does not exist on main**; no harness/golden references it. There is nothing to "flip" — P4 *creates* a real guard (§4 D4). |
| `resolve(selector,kernel,diags) → Vec<GeometryHandleId>` with a `Vertex` arm | **TRUE** | `crates/reify-eval/src/topology_selectors.rs:1358-1376`; `SelectorKind::Vertex => kernel.extract_vertices(handle)` (`:1499`) (tasks 4118/4368). |
| **Grammar gate** — P4's fixture syntax parses | **PASS** | `tree-sitter parse --quiet` exit 0 for `FixedSupport(target: frame3(point3(0mm,0mm,0mm), orient_identity()))` and `PressureLoad(magnitude:…, face: <pose>, direction:"normal")`, both wrapped in a `structure`. **No novel syntax.** (`@point(…)` as a call-arg does **not** parse — irrelevant here: P4 uses the parseable `frame3(…)` pose form.) |

No unverified substrate remains. The one **false premise** (`r3b`) is corrected (§4 D4); the
**negative-sentinel** (silent accept) is the gap the leaf closes (§4 D3, §5).

## 4. Resolved design decisions

| # | Decision | Rationale / source |
|---|---|---|
| **D1** | **FEA region targets are region-references only — set-only.** A `Value::Frame` (a coordinate *pose*: `@point`/`frame3(…)`) is **not** an FEA load/support target. Targets are `RegionRef`s named by intent (`vertex()/face()/edge()/body()`/predicate). | Leo, 2026-06-24. Honors P0 §6.2 / invariant 4 (pose ≠ region-set, distinct types) and v0.6 D2 (named-target idiom, not coordinate entry). A point load is a *located feature on the realized body*, named by a vertex selector — not a free coordinate. |
| **D2** | **Coordinate/`Frame` loads are a *named future follow-up*, not built here** (no consumer today). If a consumer for "load at an arbitrary coordinate" is ever demonstrated, it returns as a separate PRD: it needs a `frame → FE-node` resolver (nearest/coincident — **unimplemented**, unowned; 4092 is selector→node only) **and** a parseable frame-target surface — both currently absent. | Leo, 2026-06-24 (Q1 answer). Avoids a G3/G6-blocked, premature build that overlaps 4092's domain. Recorded in §6 / §7, not filed (no consumer ⇒ not runnable). |
| **D3** *(2026-07-20 re-point)* | **The reject *mechanism* is the general struct-ctor conformance chokepoint (`struct-ctor-field-type-conformance.md`); P4-π is its FEA-target *verifier*.** The rejection of a `Value::Frame` at any FEA region-target **fires with a structured pose-vs-set diagnostic** (closing the §3 silent-accept) via that chokepoint's C1-row-12 — **not** by wiring `validate_selector_target` (the corrected false premise: that fn is unreachable from the `structure def` ctor fields, §3). P4-π commits the negative fixtures (Frame at `face`/`point`/`target`) and **asserts** the chokepoint diagnostic fires. | The §3 live finding: today it is a silent accept — the *general* unenforced-struct-ctor-conformance class (the "same silent-accept class as 4575" the negative-sentinel row names), **not** the retired `validate_selector_target`. The general chokepoint is the honest, reachable wire-site; the *explicit, diagnosable* reject of a *pose* is the discipline P0 §6.2 names as "P4 must satisfy." **P4-π depends_on the chokepoint's Error-flip (δ) + 4811**; FEA field typing itself is v0.6 (Bmig/Bmig2). |
| **D4** | **Correct the brief's `r3b` false premise: *create* a real guard, don't "flip" a phantom.** The negative fixture asserting the pose-vs-set boundary is a **new** committed `.ri` + a `reify check`/`eval` diagnostic; it does not depend on the non-existent `r3b` fixture. | §3 substrate: `r3b…ri` does not exist on main. (`feedback_verify_todo_premise_before_reopen` — grep the named site before trusting the citation.) |

## 5. Decomposition plan (G2 signal drafted; hard check at decompose)

Approach **B** (single guard leaf — **not** B+H): the FEA seam is a G5 load-bearing seam, but its
**H** treatment (the two-way boundary-test gate) is **already owned upstream** by v0.6 **4371** + P0
**4813**; P4 adds **one** boundary case those don't cover (the pose-vs-set reject on the load fields)
+ the coherence record. Active blast radius ≤ ~2 crates (`reify-stdlib` helpers + an `examples/`-or-
`tests/` fixture). No new integration seam.

- **P4-π (task 4833) — Pose-vs-set FEA-target boundary VERIFIER** *(leaf / `depends_on` the struct-ctor
  chokepoint's Error-flip δ, + 4811).* *(2026-07-20 re-scope: consumer/verifier of the chokepoint, not
  the owner of a `validate_selector_target` reject arm.)*
  *Scope:* a committed negative fixture passing a `Value::Frame` (the parseable `frame3(point3(…),
  orient_identity())` pose) to FEA region-target fields — at minimum a single-kind **load** field
  (`PressureLoad(face: <pose>)` and `PointLoad(point: <pose>)`) **and** the kind-agnostic
  `FixedSupport(target: <pose>)` — and the assertion that `reify check` emits the **structured
  pose-vs-set diagnostic** delivered by the general struct-ctor conformance chokepoint (C1-row-12:
  `a coordinate pose is not a region target; select a face/edge/vertex instead`), **not** the current
  silent accept. **No wiring in P4:** the reject mechanism is the chokepoint (the `validate_selector_target`
  "verify-or-wire" plan was a false premise — §3; that fn does not guard ctor fields). P4-π only commits
  fixtures + asserts. Keep the v0.6-migrated region-target examples (`fea_cantilever_smoke.ri`,
  `fea_multi_case.ri`, and post-Bmig2 the migrated cantilever) checking clean (no regression).
  *Note on field kinds:* pre-Bmig2, `point`/`target` are still `String`, so a Frame there is a
  `String`-mismatch diagnostic; post-Bmig2 they are selector-typed and it is the kind-specific pose-vs-set
  message. `face` is selector-typed today (4370) → pose-vs-set message immediately once the chokepoint lands.
  *Modules:* committed fixtures only, under `crates/reify-stdlib/tests/` **or** `tests/prd-gate/fixtures/`
  (architect's call — `metadata.files = []`, footprint acquired at edit time after the chokepoint's δ lands).
  *User-observable signal (leaf, CLI diagnostic):* the committed fixture makes `reify check` emit the
  pose-vs-set diagnostic on a `Frame`-targeted load **and** support — where today it exits 0 / "All
  constraints satisfied." with no diagnostic — and the migrated region-target examples still check clean.
  *Consumer:* the converged FEA target surface + the single region resolver (P0 invariant 5).
  *G6 (branch 4 — rejection-mechanism):* the rejection is **delivered by the struct-ctor conformance
  chokepoint** (`struct-ctor-field-type-conformance.md` δ) — verified live for allowlisted types there;
  today's behaviour is a **silent accept** (§3) — the rejection-observation is **deferred to post-chokepoint
  dispatch** (it cannot be observed pre-enforcement); the task **owns** asserting the diagnostic on the FEA
  fields, so it is never `producer-extent-short`. *grammar_confirmed:
  true* (no novel syntax; §3 gate PASS).

**The coherence record is the PRD + §2 G4 table itself** (committed) — no separate prose task (P0 ε /
4815 already did the spec §6.1.3/§8.12 reframe). **DAG (2026-07-20):** `struct-ctor-chokepoint(δ) → P4-π(4833) ← 4811`
(supersedes `4370 → P4-π`: the chokepoint, not 4370, delivers the reject P4-π asserts; Bmig2 typing is a
soft ordering preference for the kind-specific message, not a hard dep).

## 6. Out of scope (owned elsewhere — do NOT re-file)

- **FEA field migration `String → Selector` + `validate_selector_target` accept-set + example
  migration** → **v0.6 4370 (Bmig).**
- **Region-reference → FE node/element-set resolution (selector → handle-set → DOF)** → **4092**
  (structural-analysis-fea P2).
- **The two-way selector↔FEA boundary test (BT1–BT7)** → **v0.6 4371 (BT).** P4 adds only the
  pose-vs-set case those don't cover.
- **`SelectorKind::Vertex` + kind-agnostic param acceptance** → **4368/4369 (done).**
- **The region-reference model / `SelectorKind` framing / `@`-family `@point→Frame` de-dup** → **P0**
  (4811) / **P2** (4828 τA).
- **Coordinate / `Value::Frame` FEA loads** (point-load-at-coordinate) → **named future follow-up
  (D2)** — not built; no consumer today; needs a `frame→FE-node` resolver + a frame-target surface.

## 7. Open questions (tactical — non-blocking)

1. **Exact diagnostic code/spelling for the pose-vs-set reject.** Whether to reuse an existing
   type-mismatch code, emit a dedicated pose-vs-set diagnostic, or surface the post-4370 selector
   type-conformance message — decide at dispatch after confirming what 4370's selector-typed fields
   already emit on a non-selector arg.
2. **Verify-or-wire extent (D3).** Whether the post-4370 type-conformance already rejects a `Frame` at
   the **single-kind load fields** (4371 BT4 only proves it for the kind-agnostic `FixedSupport`); if
   yes, P4-π is fixture-only; if not, it wires the `validate_selector_target` reject arm. Resolved by
   re-running the §3 probe against post-4370 `main` at dispatch.
3. **Fixture home.** `crates/reify-stdlib/tests/` (unit-adjacent) vs `tests/prd-gate/fixtures/` (the
   prd-gate corpus the brief's phantom `r3b` implied) — architect's call at edit time.
4. **Future coordinate-load surface (D2).** If/when a consumer appears, the parseable target form
   (`frame3(…)` vs a reopened `@point` grammar production) and the `frame→FE-node` resolution
   semantics (nearest vs coincident node) are that PRD's questions, not P4's.
