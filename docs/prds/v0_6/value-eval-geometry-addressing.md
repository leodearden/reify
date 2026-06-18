# Value-eval geometry addressing — honest decline + symbolic descriptors + carried-topology resolution

**Status:** authored 2026-06-17 (PRD author session, `/prd` Option 4).
**Milestone:** v0_6.
**Approach:** B + H (contracts + two-way boundary tests) — high-stakes seam (selector substrate, value model `GeometryHandleRef`, result-model topology, ≥3 crates, ≥2 cross-PRD consumers).

---

## 0. Provenance, supersession, cross-PRD frame

This PRD is the **value-eval (Face B) complement** to the ratified **Option A — Unified Build DAG** (`docs/design/engine-unified-build-dag-option-a.md`; PRDs `engine-unified-build-dag.md` + `engine-build-dag-substrate.md`). Both address one architectural seam — *what a geometry-derived value means when the geometry it derives from is not (yet) realized* — but on the two surfaces that seam has:

- **Face A — build-path scheduling.** A geometry-derived value (selector, `bounding_box`) is needed by a downstream **build** node, but the legacy phase-by-kind pipeline dispatched the consumer before the producer realized. This is a *scheduling* defect: the producer exists, it ran in the wrong order. **Owned wholly by the unified-build-dag family** (α 4354 / β 4355 / γ 4356 / δ 4357 / ε 4358 done; cutover η 4360 / θ 4361 / θ2 4531 / ι 4362 pending). Its two canonical regression proofs are esc-3205 (curated-edge `fillet(b, edges_at_height(b,…), r)`) and esc-4275 (`fits_build_volume(bounding_box(part), …)`). **This PRD does not touch Face A.**

- **Face B — value-eval semantics.** A geometry-derived value is needed on the pure value-eval surface (`eval_cached` / `Engine::eval`), where **nothing is ever realized**. No schedule fixes this — there is no realization to order. `box(...)` → `Value::Undef` (geometry constructors are realization ops, not stdlib builtins — `reify-stdlib::eval_builtin` has no arm and falls through), and `faces_by_normal(box(),…)` needs a realized `GeometryHandle` target it will never get. The value is **silently `Undef`** and **propagates silently** (`peak_deviation(track, Undef)` → `Undef`). **This is Face B, and this PRD owns it.**

The triggering incident is **task 4577** ("stdlib-surface-type P"), whose release-only e2e `printer_print_envelope_eval_e2e` failed with `peak_* = Undef`. 4577 was resolved by **Option 1** (revert `LocationId` to `Real`, keep modal force `at : Selector` on the build path) — a tactical split that closed the regression without redrawing the boundary. **This PRD is the forward fix; it is not needed to close 4577.**

### Reconciliation with the ratified design (the crux — consistent extension, not carve-out)

The L2 decision deliberately keeps the value-eval surface **expression-only by executor selection** (`design-option-a.md:130` tags `eval_cached` "editor/LSP, **expr-only, no kernel**"; `:136` "stays expr-only by executor selection (kernel-less executors for the geometry node kinds), not a separate code path"). This is post-cutover *intent*, not merely a current limitation: even after θ threads the kernel into the warm **build** surfaces (`build_snapshot`/`tessellate_from_values`/`concurrent`), `eval_cached` selects kernel-less executors and stays expr-only.

**Every rung of this PRD preserves that property** — it admits *kernel-less symbolic descriptors* and *carried-data resolution* into eval, and **never** a kernel query or a realization:

- **Rung 1 declines** — it never realizes; maximally expr-only.
- **Rung 2 mints symbolic descriptors** — zero kernel queries (consistent with task 4118's already-kernel-free `build_leaf_selector`).
- **Rung 3 resolves against data the build path baked into the result value** — eval reads carried topology, never calls OCCT.

So this PRD is framed as a **consistent extension** of the ratified design, reconciled at the seam table in §6, not an amendment to it. The shared `E_EVAL_UNRESOLVED` diagnostic code (already minted, `reify-core/diagnostics.rs:2335`) is the explicit cross-PRD contract: the unified driver emits it on the build path; this PRD emits it on the value-eval path; §6 fixes the no-double-fire boundary.

---

## 1. Consumer + user-observable surface

Every mechanism here has a present, named consumer (G1):

| Rung | Mechanism | Consumer | User-observable surface |
|---|---|---|---|
| **1** | `E_EVAL_UNRESOLVED` at value-eval typed-consumption sites | **Every author** + the dogfood guard (4577's e2e) | `reify eval` / `reify check` emits `E_EVAL_UNRESOLVED` with the offending span instead of a silent `Undef`-bearing result |
| **2** | Symbolic (unrealized) geometry/selector descriptors in eval | **Rung 3** (resolution) + **Rung 1** (declines when a symbolic value can't resolve) | `reify eval` shows `box()` / `faces_by_normal(box(),…)` yielding a content-stable symbolic value, not `Undef` |
| **3** | Selector→node resolution against carried result topology | **modal `transient_response`/`displacement_at`** (the re-homed task 4122 deliverable) + **input-shaping trajectory iteration** (4577's printer scenario) | `displacement_at(history, faces_by_normal(part, +Z, tol), dir)` returns the correct realized-mesh node's Φ-projected response on a fixture where the queried face is **not** the antinode |

**Why selectors at all (Leo, 2026-06-17):** string/index location addressing is structurally fragile — it breaks when upstream geometry shifts, and named addressing only works when a name exists (it can't always). Selectors are the durable addressing primitive for the cases string/index cannot serve. This PRD makes them usable on the surface (value-eval accessors) where 4577 proved they are needed.

---

## 2. Sketch of approach (three rungs, increasing ambition; Rung 1 lands first)

### Rung 1 — honest decline (the floor)

A value-eval accessor/builtin that **cannot resolve a required geometry/selector-typed argument to a usable concrete value** emits **`E_EVAL_UNRESOLVED`** (error severity) with the offending span, instead of returning `Undef`. Valuable standalone even if Rungs 2–3 never land: it immediately makes the entire 4577 *class* loud.

**Firing condition = consumer resolution-failure, NOT "arg is `Undef`" (the coherence invariant).** This formulation is deliberately stable across the rung sequence, because what an unresolved argument *looks like* changes as Rungs 2–3 land:
- Pre-Rung-2: the arg is `Undef` (geometry never realized).
- Post-Rung-2, pre-Rung-3: the arg is a *symbolic* selector that the consumer's resolution path cannot yet resolve.
- Post-Rung-3: the consumer resolves against carried topology and **does not fire** (the floor is lifted).

Framed as *"the consumer attempted to resolve a required geometry-derived input and failed"* it stays correct monotonically as the substrate fills in — Rung 1 is the floor that Rungs 2–3 raise, never a frozen `Undef`-test that Rung 2 would falsify.

**Firing locus (decided 2026-06-17 — the scoping invariant).** Rung 1 fires at **typed-consumption sites**, not construction sites:

- ✅ Fire when an accessor/`@optimized` builtin's required geometry/selector argument fails to resolve — `peak_deviation(track, <unresolved-selector>)`, `displacement_at(h, <unresolved-selector>, dir)`.
- ❌ Do **not** fire merely because an unrealized geometry value exists in a buffer. `eval_cached` is the editor/LSP surface; a half-typed or unrealized `box()` is `Undef`/symbolic there *all the time*, and that is **incompleteness, not error**. Firing on every construction would spam the editor and turn currently-green eval scenarios RED.

The discriminator is *"a consumer required this value and could not resolve it,"* not *"a geometry value is Undef."* This dovetails with the unified driver's existing "decline that class" semantics rather than fighting them.

### Rung 2 — symbolic eval substrate (the boundary redraw — the real "Option 4")

`box()`, `bounding_box()`, `faces_by_normal()` etc. in pure value-eval mint **content-stable symbolic descriptors** targeting the compile-time `RealizationNodeId` — kernel-free, no realization. This generalizes beyond selectors: it is "what does *any* geometry-derived value mean in eval."

**Representation (revised during substrate verification — see §4 DD-2).** The substrate already separates content-identity from the live kernel handle: `GeometryHandleRef`'s `PartialEq`/`content_hash` **exclude** `kernel_handle` (GHR-β, `reify-ir/src/value.rs:380–420`) — identity is `(realization_ref, upstream_values_hash)`, both computable in eval from the compile-time realization graph with no kernel. A symbolic handle is therefore exactly "a `GeometryHandleRef` whose content identity is known but whose `kernel_handle` is absent." Represent the absence as `kernel_handle: Option<GeometryHandleId>` (or a `KernelHandleSlot` newtype) where `None` = symbolic/unrealized. Content-based consumers (the vast majority of the ~136 `geometry_ops.rs` sites) are **untouched** (they already ignore the field); only kernel-deref sites must handle `None` — and those decline (→ Rung 1) because a kernel-deref on a symbolic handle is precisely the unrealized-in-pure-eval case.

Selector construction over a symbolic target reuses 4118's kernel-free `build_leaf_selector`; `resolve_selector_target` is widened to accept a symbolic `GeometryHandle` (it already only reads identity fields).

### Rung 3 — resolution against carried topology (re-homes task 4122)

A value-eval resolver maps a symbolic selector → node-set/value against the **realized-mesh topology the build path baked into the result value** (ModalResult / ElasticResult), kernel-free (reading baked data, never OCCT). This is where **task 4122's modal-addressability deliverable is re-homed** (Leo, 2026-06-17): 4122's "ModalResult carries realized-mesh topology so the resolver maps named/interior locations to mesh nodes" *is* Rung 3, extended from string/named locations to symbolic selectors. 4122's TODO ownership (`modal_analysis.ri:438/:506/:558/:623`) and its reuse-the-FEA-result-model-topology mandate (`4084/4091/4092` + `topology_selectors.rs`) migrate into this rung.

**Carried-topology requirement (the hard coupling, §4 DD-3).** To resolve `faces_by_normal(part, +Z, tol)` against carried mesh *kernel-free*, the carried topology must include enough to evaluate the selector predicate from baked data: per-face (or per-node) **normals**, face↔node association, and Part/LocationId association. This couples Rung 3 to the FEA result-model topology chain (4084/4091/4092), exactly as 4122 already cross-references — Rung 3 **reuses** that machinery, it does not invent a modal-only parallel path.

---

## 3. Pre-conditions (substrate verified on main, 2026-06-17)

- **Grammar — DISCHARGED.** The target idiom `displacement_at(response, faces_by_normal(response, zdir, tol), zdir)` is call-nesting in existing grammar; `tree-sitter parse --quiet` accepts a `structure def` fixture with 0 ERROR nodes, and existing `examples/kernel_queries/all_queries_walk.ri:150` (`single(faces_by_normal(...))`) already proves call-as-arg. **No novel syntax; `grammar_confirmed = true` for every leaf.**
- **`E_EVAL_UNRESOLVED` exists** as a minted `DiagnosticCode` (`reify-core/src/diagnostics.rs:2335`), currently emitted only by `engine_fixpoint::run_unified_pass`. Rung 1 adds value-eval-path emission; §6 fixes composition.
- **GHR-β** — `GeometryHandleRef.kernel_handle` is already documented ephemeral and excluded from `content_hash`/`PartialEq` (`reify-ir/src/value.rs:380–420`). Rung 2's symbolic representation is a *narrowing* of an already-content-based identity, not a new identity model.
- **4118 kernel-free construction** — `build_leaf_selector` issues zero kernel queries (`geometry_ops.rs`, returns `Value::Selector` or `Undef` on kind-closure violation). Rung 2 reuses it verbatim over symbolic targets.
- **Honest-decline precedent on the eval path already exists** — `engine_eval.rs:843` (task 250 ad-hoc ports: "selector fails → port frame undef, **diagnostic emitted**") and `geometry_ops.rs:21697` ("…error rather than a silent Undef"). Rung 1 generalizes an established pattern, it does not introduce one.
- **4082 done** — `Mode.shape` Φ is serialized in ModalResult; Rung 3's missing piece is the geometry/topology side, exactly as task 4122 scoped it.

---

## 4. Resolved design decisions

- **DD-1 — Container.** One PRD, three rungs; Rung 1 sequenced to land first and standalone-valuable. (Alternative rejected: a separate tiny honest-decline task — rejected because co-designing the diagnostic semantics with the rungs that later *lift* the floor keeps the contract coherent. Leo, 2026-06-17.)
- **DD-2 — Symbolic-handle representation = `Option<GeometryHandleId>` (absence), NOT a `GeometryTarget { Realized, Symbolic }` enum.** *Reversal of the author's initial proposal*, grounded in the GHR-β discovery: identity is already `(realization_ref, upstream_values_hash)` and `kernel_handle` is already ephemeral/excluded. An enum would conflate the realization-node identity (the real identity) with the ephemeral handle. `Option` makes "no live kernel handle" un-ignorable at the deref sites (the only sites that must change) while leaving content-based consumers untouched. (Flagged for Leo's review per the delegated-resolution agreement.)
- **DD-3 — Rung 3 resolves against carried result topology, reusing the FEA result-model topology chain (4084/4091/4092), not a modal-only path.** Inherited from 4122's ratified routing (esc-3823-140 "A-modified").
- **DD-4 — Rung 1 is error severity, scoped to typed-consumption sites on the pure value-eval surface** (§2). Not a `W_` warning (Leo: "silent errors cause untold harm — stamp them out"); not construction-site-broad (would break editor incompleteness).
- **DD-5 — 4122 is re-homed into Rung 3** as its integration leaf; superseded-and-absorbed rather than left as an external downstream consumer (Leo, 2026-06-17). Its dependency edges (3823, 4092) and TODO markers migrate at decompose.
- **DD-6 — Two-way boundary test is the H component (§7).** The symbolic-vs-realized content-identity agreement and the carried-topology-vs-live-kernel resolution agreement are both naturally two-way and are the acceptance bars.

---

## 5. Out of scope

- **Face A (build-path scheduling)** — owned by the unified-build-dag family; this PRD must not duplicate or fork it.
- **Kernel queries / realization in `eval_cached`** — explicitly preserved as out of scope; every rung stays kernel-free. A genuine *geometry-in-the-loop solver round* remains "a future PRD if demanded" (`design-option-a.md`).
- **Inline-literal geometry-query args** (the `faces_by_normal(b, vec3(0,0,1), 1deg)`-with-inline-args → `None` fall-through noted in `examples/kernel_queries/directional_selectors.ri`) — a *related* face of the smell but a distinct dispatcher issue (let-bound-intermediate workaround exists); not folded here unless decompose finds it free.
- **Retargeting modal force `at : Selector`** (build-path, already fine per 4577) — unchanged.

---

## 6. Cross-PRD relationship + seam-owner table (G4)

| Seam | This PRD owns | Other side owns | Reconciliation |
|---|---|---|---|
| `value-eval-geometry-addressing ↔ engine-unified-build-dag` | Value-eval (Face B) `E_EVAL_UNRESOLVED` emission + symbolic descriptor substrate | Build-path (Face A) scheduling + `engine_fixpoint` `E_EVAL_UNRESOLVED` emission | Rung 1 emission **scoped to the pure value-eval surface** (`eval_cached`/`Engine::eval`-without-build), must **not double-fire** against `engine_fixpoint`'s emission once the unified driver is default. Shared code, two emission sites, disjoint surfaces. |
| `value-eval-geometry-addressing ↔ FEA result-model topology` (4084/4091/4092) | Rung 3 selector→node resolution in eval | The carried realized-mesh topology representation on result values | Rung 3 **reuses** the FEA result-model topology machinery (per 4122's routing); carried topology must expose per-face/node normals + face↔node + Part/LocationId association (DD-3). |
| `topology-selectors ↔ persistent-naming-v2` (existing contested seam, overlay G4) | Symbolic-target selector construction/resolution arms | `try_eval_topology_selector` build-path dispatch arms | Rung 2/3 widen `resolve_selector_target` to accept symbolic targets; build-path arms unchanged. No new contested seam introduced. |

**Seam owner:** this PRD (Leo, accountable). No reciprocal "the other owns it" patterns: Face A↔Face B ownership is explicitly disjoint by surface.

---

## 7. Boundary-test sketch (H — facing both ways)

Two two-way boundary tests are the acceptance spine:

1. **Symbolic ⇄ realized content-identity agreement.** A `box(...)` evaluated symbolically in pure eval (Rung 2) and the *same* `box(...)` realized on the build path must produce `GeometryHandleRef`s that **compare equal and hash identically** (GHR-β already guarantees this for the identity fields — the test pins that the symbolic mint populates them correctly). Facing both ways: eval-mint → identity; build-realize → identity; equal.
2. **Carried-topology ⇄ live-kernel resolution agreement.** A selector resolved against carried result topology in eval (Rung 3) and the same selector resolved against the live kernel on the build path must yield the **same node-set** on a fixture. Facing both ways: eval-resolution (baked data) vs build-resolution (OCCT) → identical selection.

These prevent the two ways this design can silently diverge: a symbolic handle that doesn't match its realization (breaks caching/identity), and an eval resolver that selects different nodes than the kernel would (breaks correctness).

---

## 8. Decomposition plan (one bullet per leaf, with observable signal)

**Rung 1 — honest decline (lands first; independent of Rungs 2–3):**

- **R1a — value-eval `E_EVAL_UNRESOLVED` emission at typed-consumption sites.** Detect: an accessor/`@optimized` builtin on the pure value-eval surface receives a geometry/selector-typed arg that is `Undef`-from-non-realization; emit `E_EVAL_UNRESOLVED` (error) with the offending span; scope-guard against build-path intermediate eval and against non-consumed editor incompleteness. *Signal:* `reify eval` on a fixture with `peak_deviation(track, faces_by_normal(box(),…))` emits `E_EVAL_UNRESOLVED` (was: silent `peak=Undef`); a build-path scenario with the same idiom does **not** emit it (scope-guard); LSP `eval_cached` on an un-consumed unrealized `box()` emits **nothing** (editor-incompleteness guard). `grammar_confirmed=true`.

**Rung 2 — symbolic eval substrate (depends on nothing structurally; B1→B2):**

- **R2a — symbolic geometry descriptors in eval (`box`/`bounding_box`).** `kernel_handle: Option<GeometryHandleId>`; pure-eval geometry constructors mint a symbolic `Value::GeometryHandle` (`realization_ref` = compile-time `RealizationNodeId`, `kernel_handle = None`); audit/repair kernel-deref sites to handle `None` (decline → R1a). *Signal:* `reify eval` shows `box()` yielding a symbolic `GeometryHandle` value (not `Undef`), `content_hash` byte-stable across runs and **equal to the realized handle's** (two-way test §7.1).
- **R2b — symbolic-target selector construction.** Widen `resolve_selector_target` to accept a symbolic `GeometryHandle`; `faces_by_normal`/`edges_*` over a symbolic target reuse 4118 `build_leaf_selector`. *Signal:* `faces_by_normal(box(),…)` in eval yields a `Value::Selector` (was `Undef`), content-stable.

**Rung 3 — resolution against carried topology (re-homes 4122; C1→C2→C3):**

- **R3a — result values carry selector-resolvable topology.** ModalResult (and the shared FEA result-model path) carry node coords + per-face/node normals + face↔node + Part/LocationId association, reusing 4084/4091/4092. *Signal:* a ModalResult fixture's carried topology round-trips and exposes the face normals a selector predicate needs.
- **R3b — eval-path selector→node resolver (the re-homed 4122 deliverable).** Map a symbolic selector → node-set against carried topology, kernel-free; project Φ. *Signal:* `displacement_at(history, faces_by_normal(part, +Z, tol), dir)` resolves to the correct **non-antinode** mesh node and returns a finite Φ-projected response on a fixture where the queried location differs from the fundamental-mode antinode (distinguishing from 4122's current antinode fallback); same node-set as live-kernel resolution (two-way test §7.2). Resolves `modal_analysis.ri:438/:506/:558/:623`.
- **R3c — integration gate (e2e).** Restore the 4577 printer/trajectory scenario to selector-valued locations end-to-end. *Signal:* the formerly-RED-then-Option-1-reverted `printer_print_envelope` scenario, re-expressed with selector locations, produces finite non-degenerate `peak_*` (no `Undef`), under the full `--scope all --profile both` gate that runs the release-only e2e.

**Dependency shape:** R1a independent (lands first). R2a → R2b. R3a; R3b depends on R2b + R3a; R3c depends on R3b. 4122's existing edges (3823, 4092) flow into R3a/R3b. Cross-PRD: coordinate the shared `E_EVAL_UNRESOLVED` with the unified-build-dag cutover (η/θ) per §6 — no hard edge, but the no-double-fire boundary is a decompose-time check.

---

## 9. Open (tactical) questions

- **Q1 (R1a):** exact mechanism for "consumer required this geometry-derived value and could not resolve it" vs ordinary `Undef`/incompleteness at a consumption site — does the arg's `CompiledExpr` carry enough (geometry-classified call shape via `is_geometry_query_call` / `GEOMETRY_FUNCTION_NAMES`) to classify without a new IR field, or is a thin provenance tag needed? Resolve at R1a implementation; the negative-assertion probes (build-path does-not-fire + editor-incompleteness does-not-fire) are the guard either way. Note the firing condition is consumer-resolution-failure (§2), so it remains correct after Rung 2 turns the arg from `Undef` into a symbolic-but-unresolvable selector.
- **Q2 (R2a):** `Option<GeometryHandleId>` vs a `KernelHandleSlot` newtype with a named `Symbolic` constructor — both satisfy DD-2; pick for ergonomics of the ~136 deref-site audit.
- **Q3 (R3a):** does carrying per-*node* normals suffice, or are per-*face* normals required for `faces_by_normal` predicate parity with the kernel? Settle against the two-way resolution test (§7.2) — whichever makes eval-selection == kernel-selection.
- **Q4 (R3c):** which fixture is the canonical non-antinode dogfood — the printer envelope, or a purpose-built modal cantilever with a queried mid-span face? Prefer reusing the 4577 printer scenario for continuity.
