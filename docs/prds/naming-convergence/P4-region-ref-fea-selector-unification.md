# PRD — P4: Region-reference ⇄ FEA-target unification (the pose-vs-set boundary)

> **Program:** naming & selection convergence (P0–P4). **Charter/evidence:** `./00-findings.md`
> (§4 layer violation — "the road not taken: FEA intent-named regions"; §5 split-brain — namespace 4),
> the brief `./P4-region-ref-fea-selector-unification.brief.md`, and the **keystone contract**
> `./P0-region-reference-layer-model.md` (committed + decomposed 2026-06-24 — its §3 D1 / §6.2 /
> invariant 4 delegate the FEA **pose-vs-set** decision to this PRD). Authored 2026-06-24 via a
> `/prd` session (Leo + Claude). Substrate **G3-verified against current `main`** this session (§3).
>
> **Status:** active — **thin convergence delta**. The FEA String→region-reference *bridge itself* is
> already owned + filed by the in-flight **v0.6 FEA-selector migration** (`docs/prds/v0_6/fea-load-support-selector-migration.md`):
> field migration **4370 (Bmig)**, two-way boundary test **4371 (BT)**, selector→node-set **4092**, with
> type-substrate **4368/4369 done**. **P4 does NOT re-file any of that** (command + P0 §8). P4 owns the
> one P0-delegated piece v0.6 never addressed: the **pose-vs-set boundary** (a `Value::Frame` is **not**
> an FEA region target) + the coherence record. One leaf, dependency-gated on **4370 + 4811**.
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

| Brief deliverable | Already owned + filed by | Status (main, 2026-06-24) |
|---|---|---|
| 3. FEA field migration (`String → Selector`, 5 fields) + worked-example migration | **4370** (v0.6 Bmig) | pending |
| 1. `validate_selector_target` **accept-set** (add `Value::Selector`) | **4370** (v0.6 Bmig) | pending |
| 2. region-reference → FE node/element-set resolution (selector → handle-set → DOF) | **4092** (structural-analysis-fea P2) | pending |
| 4. two-way boundary test (selector producer ↔ FEA consumer), BT1–BT7 | **4371** (v0.6 BT) | pending |
| type substrate (`SelectorKind::Vertex`; kind-agnostic param acceptance) | **4368 / 4369** | **done** |

Because `Value::Selector` **is** the canonical `RegionRef` post-P0, **4370 accepting `Value::Selector`
already collapses the FEA string namespace onto the one resolver** — even P0 §6.2's *"FEA-target
contract row (P4 must satisfy)"* (a 2-manifold ref accepted; a 3-manifold ref to a `face:` param is a
kind error) is satisfied by **4371 BT1/BT2**.

**What is genuinely left for P4** (neither v0.6 nor P0 delivers it):

- **The pose-vs-set boundary.** v0.6 4371 BT4 tests *"a non-selector target (e.g. a `Real`) rejected"*
  for the kind-agnostic `FixedSupport` only; **nothing tests a `Value::Frame` (a *pose*) rejected at the
  single-kind LOAD fields** (`PointLoad.point`, `PressureLoad.face`, …), and — critically — **today it
  is a silent accept** (§3, the negative-sentinel finding). P4 owns making the pose-vs-set rejection
  *fire, with a clear diagnostic* (the explicit reject side of `validate_selector_target`, whose reject
  arm is presently an opaque `_ => None`), and a committed fixture proving it.
- **The coherence record + the brief's `r3b` false-premise correction** (§3): the brief's deliverable-4
  *"flip the `r3b_displacement_at_selector_grammar.ri` negative fixture"* rests on a fixture that **does
  not exist on main** — P4 *creates* a real pose-vs-set guard instead of "flipping" a phantom.

**Clean split (no contested seam):** v0.6 4370 owns the **accept-side** of `validate_selector_target`
(`String → Selector`); P4 owns the **reject-side** (explicit `Value::Frame`-reject + pose-vs-set
diagnostic), **landing after 4370** (hard dep) so the two never touch the function concurrently —
the same land-after-the-prereq churn-avoidance pattern P2 Thread C uses against P0 β.

## 3. Substrate verification (G3) — verified against `main`, 2026-06-24

| Assumed capability | Verdict | Evidence (snapshot) |
|---|---|---|
| `validate_selector_target` accepts only `Map`/`String`; **rejects `Value::Selector` AND `Value::Frame`** | **TRUE** | `crates/reify-stdlib/src/helpers.rs:214-219`: `match v { Value::Map(_) \| Value::String(_) => Some(()), _ => None }`. Callers: `supports.rs:120,138` (`DisplacementSupport`/`RollerSupport`); the load fields don't call it (retired). The reject side is an **opaque `_ => None`** (no pose-specific diagnostic). |
| FEA target fields are still `String = ""` placeholders | **TRUE** | `fea_multi_case.ri`: `PointLoad.point` (`:315`-area), `FixedSupport.target` (`:354`), `PressureLoad.face` (`:412`), `TractionLoad.face` (`:440`), `BodyForce.body` (`:470`) — all `param … : String = ""`. (4370/Bmig pending; the migration is **not** on main yet.) |
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
| **D3** | **P4 owns the *reject-side* pose-vs-set guard; v0.6 4370 owns the *accept-side*.** The rejection of a `Value::Frame` at any FEA region-target **must fire with a structured pose-vs-set diagnostic** (closing the §3 silent-accept) — *verify-or-wire*: confirm the post-4370 selector-typed fields' type-conformance rejects a non-selector `Frame`, and if any field does not (the single-kind load fields are the risk — 4371 BT4 only covers `FixedSupport`), wire the explicit reject + diagnostic at `validate_selector_target` (replacing the opaque `_ => None`) and ensure it guards every target field. | The §3 live finding: today it is a silent accept (4575 class). 4370 widens the accept-set; the *explicit, diagnosable* reject of a *pose* is the complementary discipline P0 §6.2 names as "P4 must satisfy." Lands **after 4370** (hard dep) — same function, sequential, no churn. |
| **D4** | **Correct the brief's `r3b` false premise: *create* a real guard, don't "flip" a phantom.** The negative fixture asserting the pose-vs-set boundary is a **new** committed `.ri` + a `reify check`/`eval` diagnostic; it does not depend on the non-existent `r3b` fixture. | §3 substrate: `r3b…ri` does not exist on main. (`feedback_verify_todo_premise_before_reopen` — grep the named site before trusting the citation.) |

## 5. Decomposition plan (G2 signal drafted; hard check at decompose)

Approach **B** (single guard leaf — **not** B+H): the FEA seam is a G5 load-bearing seam, but its
**H** treatment (the two-way boundary-test gate) is **already owned upstream** by v0.6 **4371** + P0
**4813**; P4 adds **one** boundary case those don't cover (the pose-vs-set reject on the load fields)
+ the coherence record. Active blast radius ≤ ~2 crates (`reify-stdlib` helpers + an `examples/`-or-
`tests/` fixture). No new integration seam.

- **P4-π — Pose-vs-set FEA-target boundary guard** *(leaf / `depends_on` 4370, 4811).*
  *Scope:* a committed negative fixture passing a `Value::Frame` (the parseable `frame3(point3(…),
  orient_identity())` pose) to FEA region-target fields — at minimum a single-kind **load** field
  (`PressureLoad(face: <pose>)` and `PointLoad(point: <pose>)`) **and** the kind-agnostic
  `FixedSupport(target: <pose>)` — and the assertion that `reify check`/`eval` emits a **structured
  pose-vs-set diagnostic** (a `Frame`/pose is not a region target; name a vertex/face/edge), **not** the
  current silent accept. **Verify-or-wire (D3):** confirm the post-4370 selector-typed-field
  type-conformance rejects the non-selector `Frame`; if any target field does not, wire the explicit
  `Value::Frame`-reject + diagnostic at `validate_selector_target` (`helpers.rs`, replacing `_ => None`)
  and ensure it guards every region-target field. Keep the v0.6-migrated region-target examples
  (`fea_cantilever_smoke.ri`, `fea_multi_case.ri`) checking clean (no regression).
  *Modules:* `crates/reify-stdlib/src/helpers.rs` (+ `loads.rs`/`supports.rs` wiring iff needed); a
  committed fixture under `crates/reify-stdlib/tests/` **or** `tests/prd-gate/fixtures/` (architect's
  call — `metadata.files = []`, footprint acquired at edit time after 4370 lands).
  *User-observable signal (leaf, CLI diagnostic):* the committed fixture makes `reify check` (or `eval`,
  if the guard is the runtime trampoline) emit the pose-vs-set diagnostic on a `Frame`-targeted load
  **and** support — where today it exits 0 / "All constraints satisfied." with no diagnostic — and the
  migrated region-target examples still check clean.
  *Consumer:* the converged FEA target surface + the single region resolver (P0 invariant 5).
  *G6 (branch 4 — rejection-mechanism):* the rejection is **delivered by 4370** (selector-typed fields)
  **+ this task's verify-or-wire**; today's behaviour is a **silent accept** (§3) — the binding's
  rejection-observation is **deferred to post-4370 dispatch** (it cannot be observed pre-migration);
  the task **owns** closing any extent gap, so it is never `producer-extent-short`. *grammar_confirmed:
  true* (no novel syntax; §3 gate PASS).

**The coherence record is the PRD + §2 G4 table itself** (committed) — no separate prose task (P0 ε /
4815 already did the spec §6.1.3/§8.12 reframe). **DAG:** `4370 → P4-π ← 4811`.

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
