# Topology-Selector Value Type

**Status:** deferred — design-first PRD (B+H). Version-agnostic language foundation. Authored 2026-05-31.

**Goal.** A first-class topology-selector *value type* in Reify, so `face(body, "top")`
(and the edge / body / predicate constructors) evaluates to a typed `Selector` value carrying
**kind/dimensionality + a deferred query spec** — enabling construct-time kind type-checking
(`FaceSelector` vs `EdgeSelector` vs `BodySelector` are distinct types) and composable,
introspectable selectors as language values. Selector **resolution** (which specific
faces/edges a spec picks) fundamentally needs a realized mesh and therefore **stays at solve /
geometry-build time**, reusing the existing `crates/reify-eval/src/topology_selectors.rs`
machinery. The typed value's benefit is *construct-time kind safety + composability*, **not**
eliminating solve-time resolution.

---

## 1. Background

Today selectors are stringly-typed at the seam that matters most. The FEA Load/Support
hierarchy (task **2881**, *done*) ships as `structure def`s whose geometry-target fields are
**opaque `String` placeholders**, validated at best at solve time:

```reify
// crates/reify-compiler/stdlib/fea_multi_case.ri (landed, task 2881)
structure def PressureLoad : Load {
    param magnitude : Real   = 0.0
    param face      : String = ""        // "placeholder for `Selector` until the
                                          //  topology-selector type lands"
    param direction : String = "normal"
}
structure def BodyForce : Load {
    param body          : String = ""    // "placeholder for `BodySelector`"
    param force_density : Real   = 0.0
}
```

The comments in that file already **name the target types** (`FaceSelector`, `BodySelector`).
Because `face`/`body` are bare `String`s, `PressureLoad(face: <a volume selector>)` is *not* a
type error — the mistake survives compilation and surfaces, at best, at solve time when the
runtime query path (`faces_by_area` / `faces_by_normal` / `edges_by_length` over a
`GeometryKernel`) tries and fails to resolve it; selector→node-set resolution is owned
downstream by task **4092** (*pending*).

Substrate reality (verified 2026-05-31):

- **No selector value or type exists.** `Value` (`crates/reify-ir/src/value.rs`, 29 variants)
  has no `Selector`; `Type` (`crates/reify-core/src/ty.rs`, 29 variants) has no selector type.
  `grep FaceSelector/BodySelector` across `crates/` = 0 type/value definitions.
- **`Type::Geometry` is a single opaque variant** — `Solid`, `Surface`, `Curve` all collapse to
  it. There is *no* face/edge/solid distinction at the type level today, so a selector-**kind**
  type is genuinely *more* granular than the geometry types themselves.
- **Existing predicate selectors are eager and incomplete.** `faces_by_normal(body, …)`
  compiles to `Type::List(Type::Geometry)` (`crates/reify-compiler/src/units.rs`,
  `topology_selector_result_type`) and is dispatched by `try_eval_topology_selector`
  (`crates/reify-eval/src/geometry_ops.rs`) in the kernel-bearing `post_process_topology_selectors`
  pass. The eval-side dispatch for the eleven task-2699 names (task **2691**) was **cancelled**,
  so most predicate selectors do not currently resolve on main.
- **`face(body, "top")` is a plain function call** — it already parses (`grammar.js`
  `function_call`). No novel grammar is required (see §6 / G3). Prior art `@face("name")` ad-hoc
  selectors resolve to a *frame* and are a distinct feature.
- **Resolution substrate to reuse exists**: the `topology_selectors.rs` predicate functions, plus
  `FeatureTag` / `FeatureTagTable` / `resolve_unique_by_tag` (`crates/reify-ir/src/geometry.rs`).
- **No coercion-node substrate.** Int→Real widening lives *only* in `type_compatible`
  (`crates/reify-compiler/src/type_compat.rs:232`); there is no `Coerce`/`Widen` IR node. The
  Selector→List coercion bridge (§4) therefore introduces its own eval-side resolve node — this
  PRD owns that mechanism (it does not assume one).

This PRD designs the typed-selector capability as a **decoupled future feature**. Per Leo's
decision, FEA proceeds on the current runtime-`String` basis and is **not blocked** on this work.

---

## 2. What a user observes when this lands

1. **Construct-time kind errors.** Combining or mis-targeting selectors of the wrong
   dimensionality is a *compile-time* diagnostic, not a solve-time failure:

   ```reify
   union(faces(b), edges(b))      // error E_SELECTOR_KIND_MISMATCH:
                                   //   cannot combine FaceSelector with EdgeSelector
   ```

   and (once the FEA follow-on migrates the load fields — out of scope here) a wrong-kind load
   target is caught at compile time rather than at solve.

2. **Typed, composable selector values.** Selectors are values with an algebra:

   ```reify
   let top      = faces_by_normal(b, [0, 0, 1], 1deg)   // : FaceSelector
   let big      = faces_by_area(b, 100mm^2 .. 1e9mm^2)  // : FaceSelector
   let top_big  = intersect(top, big)                   // : FaceSelector
   let everything_but_top = difference(faces(b), top)   // : FaceSelector
   fillet(b, edges_at_height(b, 0mm, 0.01mm), 1mm)       // FaceSelector/EdgeSelector
                                                         //   coerces to List<Geometry>
   ```

3. **Identical geometry for existing predicate-selector call sites.** `.ri` files that already
   feed `faces_by_normal(...)` / `edges_*` into `fillet` / `single` produce the same realized
   geometry — the re-typing to `Selector` + coercion-to-`List<Geometry>` is observably
   transparent at those sites (boundary test §8).

**Consumer (G1).** Two named consumers, both concrete and present today:
- **In-PRD user surface:** the `E_SELECTOR_KIND_MISMATCH` diagnostic + the composition algebra,
  exercised by a committed `.ri` example in CI (no downstream PRD required to observe it).
- **Downstream beneficiary:** the FEA Load/Support `structure def`s (`fea_multi_case.ri`, task
  2881 *done*) whose `face`/`body` fields migrate `String → FaceSelector`/`BodySelector`. That
  migration is a **follow-on consumer PRD** (§10), *not* folded into this PRD's core.

---

## 3. Resolved design decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **First-class** `Value::Selector(SelectorValue)` + `Type::Selector(SelectorKind)`. | Selectors are a primitive with deferred-resolution semantics distinct from data aggregates; exact kind-checking; cleanest algebra. (Leo, 2026-05-31.) |
| D2 | `SelectorKind = { Face, Edge, Body }` (dimensionality 2 / 1 / 3). `Vertex` (0-D) deferred — no FEA need, no existing predicate selectors. | Matches the motivating Face/Edge/Volume triple; keeps the kind set minimal. **UPDATE 2026-06-08: D2 is being REVERSED by the FEA consumer.** `docs/prds/v0_6/fea-load-support-selector-migration.md` (D2/A1) adds `SelectorKind::Vertex` (0-D) + `vertex()/vertices()` for `PointLoad.point` (FEA point-load *is* the consumer the original deferral assumed absent), and (A2) adds a kind-agnostic selector param for `FixedSupport.target`. Both extensions are **owned by that PRD**, built as strict K1/K2/K3-respecting extensions of this substrate — do not re-file them here. |
| D3 | **Bound** selectors: each leaf carries its target geometry handle. Construction is **kernel-free** (packages an already-realized body handle + query args); only **resolution** touches the kernel. | `face(body, …)` binds `body` at construct time; cleanly separates the kernel-free construct phase from the kernel-bearing resolve phase, which is what makes the deferral honest (G6). |
| D4 | **Bridge by coercion** (Leo, 2026-05-31): one `resolve(selector, kernel) → Vec<GeometryHandleId>` path, reused by **(a)** an eager `Type::Selector(k) → Type::List(Type::Geometry)` coercion where a list is expected and **(b)** the FEA solve path. Predicate selectors are **re-typed** to return `Selector`; existing call-site syntax is unchanged. | Single resolution path; no second selection idiom. Low-risk because the eager predicate-eval path was incomplete on main (task 2691 cancelled). |
| D5 | Coercion is realized by a new `CompiledExprKind::ResolveSelector { selector }` (result type `List<Geometry>`), **inserted by the compiler** at call sites where a `Selector(k)` arg meets a `List<Geometry>` param, and **evaluated only in kernel-bearing passes** via `resolve()`. | No coercion-node substrate exists (§1); this is the minimal, explicit bridge. Matches the existing constraint that selector evaluation already only happens in kernel-bearing passes. |
| D6 | **Composition included** (Leo, 2026-05-31): `union` / `intersect` / `difference` combinators, **closed under a single kind**. Mixed-kind composition is a **compile-time** `E_SELECTOR_KIND_MISMATCH`. `filter`-style refinement is sugar for `intersect`. | Delivers the "composable, introspectable" goal; the kind-closure invariant is the headline construct-time safety property and a clean G6-checkable boundary test. |
| D7 | **No new grammar.** Constructors are plain function calls; `FaceSelector`/`EdgeSelector`/`BodySelector` are type-name identifiers mapped to `Type::Selector(kind)` in the compiler's type resolver. | `face(body,"top")`, `union(a,b)` already parse; a type annotation already parses an identifier type name. The grammar gate is N/A (§6). |
| D8 | **Named-leaf resolution is delegated**, not owned here. `face(body, name)` constructs a `Named` leaf; mapping a user feature *name* → sub-shape handle is owned by **`persistent-naming-v2`** (the known contested seam). Until it lands, `Named` leaves resolve via `resolve_unique_by_tag` for the unique-tag case and otherwise emit `W_TOPOLOGY_TAG_STALE` / an "unresolved selector name" diagnostic. **Predicate and composition leaves resolve fully in v1.** | Keeps this PRD's resolution self-contained for predicate/composition while honoring the existing seam ownership (G4). The construct-time kind-safety win does **not** depend on named resolution. |

---

## 4. Contract (B+H)

The seam this PRD owns is the **Selector value/type + its resolution**. An architect reading
this section can implement the producer side without further discussion.

### 4.1 Types (`reify-core`)

```rust
// crates/reify-core/src/ty.rs  — lives beside Type so both Type and Value can name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectorKind { Face, Edge, Body }   // dimensionality 2 / 1 / 3

pub enum Type {
    // … existing 29 variants …
    Selector(SelectorKind),                  // NEW
}
```

`SelectorKind::Display` → `"FaceSelector"` / `"EdgeSelector"` / `"BodySelector"` (the names the
FEA loads migrate to and the type-resolver maps from).

### 4.2 Value (`reify-ir`)

```rust
// crates/reify-ir/src/value.rs
pub enum Value {
    // … existing 29 variants …
    Selector(SelectorValue),                 // NEW
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectorValue {
    pub kind: SelectorKind,                  // == kind of every leaf/operand (invariant K1)
    pub node: SelectorNode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectorNode {
    Leaf  { target: GeometryHandleRef, query: LeafQuery },
    Union(Vec<SelectorValue>),
    Intersect(Vec<SelectorValue>),
    Difference(Box<SelectorValue>, Box<SelectorValue>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LeafQuery {
    Named(String),                                  // D8 — delegated resolution
    All,                                            // extract_faces / extract_edges (all)
    ByNormal   { dir: [f64; 3], tol_rad: f64 },     // Face
    ByArea     { min_m2: f64, max_m2: f64 },        // Face
    ByLength   { min_m: f64, max_m: f64 },          // Edge
    ByHeight   { z_m: f64, tol_m: f64 },            // Edge
    ByParallel { axis: [f64; 3], tol_rad: f64 },    // Edge
}
```

`GeometryHandleRef` = the existing `Value::GeometryHandle` payload (realization_ref +
upstream_values_hash + kernel_handle); reuse it verbatim so the deterministic sub-handle hashing
in `topology_selectors.rs` (`compose_sub_handle_hash`) carries over.

**Invariants.**
- **K1 (kind closure).** For any `SelectorValue`, `kind` equals the kind of every child operand
  and is consistent with every leaf `LeafQuery` (e.g. `ByNormal`/`ByArea` ⇒ `Face`;
  `ByLength`/`ByHeight`/`ByParallel` ⇒ `Edge`; `Named`/`All` ⇒ any kind). Violations are
  **rejected at construction (compile time)** → `E_SELECTOR_KIND_MISMATCH`.
- **K2 (construction is kernel-free).** Building a `SelectorValue` issues **no** kernel queries;
  it only references an already-realized `target` handle.
- **K3 (determinism).** Resolution order follows the kernel's canonical `TopExp::MapShapes`
  order; set ops dedup by `GeometryHandleId` while preserving first-seen canonical order.

### 4.3 Resolution (`reify-eval`)

```rust
// crates/reify-eval/src/topology_selectors.rs (extends the existing module)
pub fn resolve<K: GeometryKernel + ?Sized>(
    selector:    &SelectorValue,
    kernel:      &mut K,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<GeometryHandleId>, QueryError>;
```

Dispatch:
- `Leaf { target, ByNormal{..} }` → `faces_by_normal(kernel, target, dir, tol_rad)` (and likewise
  `ByArea→faces_by_area`, `ByLength→edges_by_length`, `ByHeight→edges_at_height`,
  `ByParallel→edges_parallel_to`) — **reuse `topology_selectors.rs` verbatim**.
- `Leaf { target, All }` → `extract_faces`/`extract_edges` per kind.
- `Leaf { target, Named(name) }` → delegate (D8): `persistent-naming-v2` resolver when present;
  interim = `resolve_unique_by_tag` for the unique case, else push `W_TOPOLOGY_TAG_STALE` and
  return `[]`.
- `Union/Intersect/Difference` → resolve children, then set-union/intersection/difference of
  `GeometryHandleId`s (K3 dedup + order).

`resolve` is the **single** executor. It is invoked from (a) the `ResolveSelector` eval arm
(eager coercion) and (b) the FEA solve path (4092, downstream) — both in kernel-bearing contexts.

### 4.4 Coercion (`reify-compiler` + `reify-eval`)

- **Type acceptance** (`type_compatible`): `Type::Selector(a)` is compatible with
  `Type::Selector(b)` **iff** `a == b`; and `Type::Selector(_)` is compatible with a
  `Type::List(Type::Geometry)` **param** (one-directional: selector → list, never the reverse).
- **Node insertion** (compiler): when an arg of type `Selector(k)` is bound to a
  `List<Geometry>` param, wrap it in `CompiledExprKind::ResolveSelector { selector }` with
  `result_type = List(Geometry)`.
- **Eval** (`ResolveSelector`): evaluate the inner `Value::Selector`, call `resolve(&sel, kernel, &mut diags)`,
  yield `Value::List(handles.map(GeometryHandle))`. Evaluated only in kernel-bearing passes
  (same passes that already evaluate selector cells); a `Selector` reaching a non-kernel context
  with no consuming resolve simply stays a first-class `Selector` value.
- **Runtime kind-match** (`value_type_kind_matches`, `reify-eval`): `Value::Selector(sv)` matches
  `Type::Selector(k)` iff `sv.kind == k` (and `Value::Undef` remains the wildcard sentinel).

### 4.5 Language surface (constructors)

| Constructor | Result | LeafQuery | Resolution owner |
|---|---|---|---|
| `face(g, name)` / `edge(g, name)` / `body(g, name)` | `FaceSelector`/`EdgeSelector`/`BodySelector` | `Named(name)` | persistent-naming-v2 (D8) |
| `faces(g)` / `edges(g)` | `FaceSelector` / `EdgeSelector` | `All` | this PRD |
| `faces_by_normal(g, dir, tol)` | `FaceSelector` | `ByNormal` | this PRD (reuse) |
| `faces_by_area(g, range)` | `FaceSelector` | `ByArea` | this PRD (reuse) |
| `edges_by_length(g, range)` | `EdgeSelector` | `ByLength` | this PRD (reuse) |
| `edges_at_height(g, z, tol)` | `EdgeSelector` | `ByHeight` | this PRD (reuse) |
| `edges_parallel_to(g, axis, tol)` | `EdgeSelector` | `ByParallel` | this PRD (reuse) |
| `union(a, b, …)` / `intersect(a, b, …)` / `difference(a, b)` | same kind | composite | this PRD |

Type-name identifiers `FaceSelector` / `EdgeSelector` / `BodySelector` in type position resolve
to `Type::Selector(kind)` (D7). The predicate-selector entries **re-type** the existing
`topology_selector_result_type` mappings from `List(Geometry)` to `Selector(kind)` (D4).

---

## 5. Boundary-test sketch (B+H) — faces both sides of the seam

| # | Scenario | Preconditions | Postconditions (assert) | Side |
|---|---|---|---|---|
| BT1 | Wrong-kind composition rejected at compile time | `.ri`: `union(faces(b), edges(b))` | compile fails, exactly one `E_SELECTOR_KIND_MISMATCH`, message names both kinds; span at the `union` call | producer (type-checker) |
| BT2 | Same-kind union resolves to set-union | two `FaceSelector`s over `b` with overlapping faces | `resolve(union)` = canonical-ordered dedup'd union; no diagnostic | producer (resolve) |
| BT3 | `difference` / `intersect` set semantics | `faces(b)` minus `faces_by_normal(b,+Z)` | `resolve` = all faces except the +Z face; intersect of disjoint = `[]` | producer (resolve) |
| BT4 | Eager coercion preserves shipped geometry | existing `.ri` using `fillet(b, edges_at_height(b,0mm,ε), r)` | realized mesh identical to pre-change baseline; `ResolveSelector` inserted; one `resolve` call | consumer (coercion) |
| BT5 | `single(faces_by_normal(b,+Z,1deg))` | one +Z face | coerces `Selector→List<Geometry>`, `single` extracts the one face | consumer (coercion) |
| BT6 | Kind-typed param rejects wrong selector | a stdlib fn `fn needs_face(s: FaceSelector)`; call `needs_face(edges(b))` | compile fails, `E_SELECTOR_KIND_MISMATCH` (`FaceSelector` expected, `EdgeSelector` found) | consumer (typed param — mirrors the FEA load case without the FEA migration) |
| BT7 | Construction is kernel-free (K2) | build `faces_by_normal(b,…)` with a counting kernel, do **not** resolve | zero kernel queries issued during construction | producer (invariant) |
| BT8 | Named-leaf interim behavior (D8) | `face(b, "nope")` with no matching tag | resolves to `[]` + exactly one `W_TOPOLOGY_TAG_STALE`; no panic | producer (delegated seam) |

The integration-gate task (ε) names this table as its observable signal (closes G2).

---

## 6. Substrate verification (G3)

| Assumed capability | Status | Evidence |
|---|---|---|
| `face(body,"top")`, `union(a,b)` parse | **exists** | `grammar.js` `function_call` (`prec(11, seq(name, callTail))`); corpus `function_call_named_args.txt`. **No novel syntax** → grammar gate N/A. |
| `FaceSelector` as a type annotation parses | **exists** | type annotations already accept identifier type names; mapping the identifier → `Type::Selector` is compiler type-resolution (this PRD adds it), not grammar. |
| Predicate resolution fns | **exists** | `topology_selectors.rs`: `faces_by_normal:573`, `faces_by_area:349`, `edges_by_length:272`, `edges_at_height:777`, `edges_parallel_to:637`. |
| Feature-tag resolver | **exists** | `FeatureTag`/`FeatureTagTable` (`reify-ir/geometry.rs:2021`), `resolve_unique_by_tag` (`topology_selectors.rs:864`). |
| Deterministic sub-handle identity | **exists** | `compose_sub_handle_hash` / `make_sub_handle` (`topology_selectors.rs:73,109`). |
| Selector-call dispatch hook | **exists** | `try_eval_topology_selector` (`geometry_ops.rs`), `post_process_topology_selectors` (`engine_build.rs`). |
| **Coercion / resolve IR node** | **ABSENT → this PRD adds it** | no `Coerce`/`Widen` IR node; Int→Real widening is type-level only (`type_compat.rs:232`). `ResolveSelector` (D5) is new substrate **owned here**. |
| Name → sub-shape resolution | **delegated** | persistent-naming-v2 seam (D8 / G4); not assumed present — interim behavior specified. |

No unverified assumed substrate remains: every capability either exists, is added by this PRD,
or is an explicitly-owned cross-PRD seam.

---

## 7. Pre-conditions for activating

- None that block construct-time kind safety or predicate/composition resolution — those are
  fully self-contained.
- **Soft seam:** full `Named`-leaf resolution depends on `persistent-naming-v2` (D8). Activate
  the `Named` constructors with interim behavior; the richer resolution lands when that PRD does.
- Coordinate with task **2699** (pending, stdlib name wiring): this PRD changes the selector
  result-type wiring (`topology_selector_result_type` → `Selector(kind)`). If 2699 lands first,
  task β rebases onto it; otherwise this PRD subsumes the result-type half. Task **2691**
  (cancelled) is *not* a precondition.

---

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_2/persistent-naming-v2.md` | this consumes | `resolve()` `Named`-leaf → name→sub-shape handle | **persistent-naming-v2** | blocked (interim behavior in v1, D8) |
| FEA loads — `fea_multi_case.ri` / task 2881 (done), 4092 (pending) | this produces; FEA consumes | load field `String → FaceSelector`/`BodySelector`; `resolve()` → handles → **node-sets** | migration: **FEA follow-on PRD**; handles→node-sets: **4092**; selector→handles: **this PRD** | queued (follow-on) |
| Root `docs/prds/topology-selectors.md` (function-family v0.1) | sibling | shares `topology_selectors.rs` + feature-tag machinery; no new selector functions added here | n/a | wired |
| Task 2699 (stdlib name wiring, pending) | overlaps | `topology_selector_result_type` result-type wiring | this PRD (re-type) | coordinate (§7) |

This is the **known** `topology-selectors ↔ persistent-naming-v2` contested pair (overlay G4 #3).
It is resolved here by splitting the seam: this PRD owns the `Named` **spec slot** + predicate/
composition resolution; persistent-naming-v2 owns name→handle **resolution**. No new contested
pair is introduced.

---

## 9. Decomposition plan (one bullet per task; signals drafted for the later decompose pass)

Approach **B+H** (blast radius ≥ 4 crates, ≥ 8 mechanisms, ≥ 2 cross-PRD consumers, touches the
type system + eval dispatch). Phases:

**Phase 1 — foundation (types + conformance).**
- **α — Selector value + type substrate.** `SelectorKind` + `Type::Selector` (`reify-core`);
  `Value::Selector` + `SelectorValue`/`SelectorNode`/`LeafQuery` (`reify-ir`); the K1 kind-closure
  constructor guard. *Modules:* reify-core, reify-ir. *Intermediate* — unlocks β, γ, δ. *Signal:*
  unlocks downstream (roped to ε); `Value`/`Type` round-trip + K1 rejection unit-covered.
- **β — type-name resolution + conformance + coercion acceptance.** Map `FaceSelector`/
  `EdgeSelector`/`BodySelector` identifiers → `Type::Selector(kind)`; `type_compatible` kind
  equality + `Selector(k)→List<Geometry>` acceptance; `value_type_kind_matches` arm. *Modules:*
  reify-compiler, reify-eval. *Intermediate* — unlocks γ, δ. *Signal:* unlocks downstream;
  `needs_face(edges(b))` type-rejection compile fixture (feeds BT6).

**Phase 2 — vertical slice (predicate leaf → resolve → eager coercion, end-to-end).**
- **γ — predicate constructors + `resolve()` + `ResolveSelector` coercion node.** Re-type the
  predicate selectors (`faces_by_normal`/…/`edges_parallel_to`) + `faces`/`edges` → `Selector`;
  `resolve()` for predicate + `All` leaves (reuse `topology_selectors.rs`); `CompiledExprKind::ResolveSelector`
  + compiler insertion + eval arm. *Modules:* reify-compiler, reify-ir, reify-eval. **Leaf.**
  *Signal (user-observable):* `.ri` example `fillet(b, edges_at_height(b,0mm,0.01mm), 1mm)`
  realizes **identical geometry** to the pre-change baseline; predicate selectors now resolve
  (BT4/BT5/BT7).

**Phase 3 — kind safety + composition.**
- **δ — composition algebra + Named constructors.** `union`/`intersect`/`difference` with K1
  kind closure; `face`/`edge`/`body` `Named` constructors with interim delegated resolution
  (D8). *Modules:* reify-compiler, reify-eval. **Leaf.** *Signal (user-observable):* `.ri`
  example `union(faces(b), edges(b))` emits exactly one `E_SELECTOR_KIND_MISMATCH`; same-kind
  `union`/`difference` resolve to the set result (BT1/BT2/BT3/BT8).

**Phase 4 — integration gate + companion corrections.**
- **ε — boundary-test suite (the H integration gate).** Implement §5 BT1–BT8 facing both sides.
  *Modules:* reify-eval tests (+ a `tests/` `.ri` fixture dir). **Leaf / integration-gate.**
  *Signal:* the §5 table is green end-to-end. Depends on γ, δ.
- **ζ — companion prose corrections.** Update `fea_multi_case.ri` field comments to point the
  `String → FaceSelector`/`BodySelector` migration at this PRD + the FEA follow-on; add a
  cross-ref stub in root `topology-selectors.md`; note the 2699/4092 relationship. *Modules:*
  stdlib `.ri` comments, docs. **Leaf.** *Signal:* comments cite this PRD path; no code change.

DAG: `α → β → γ → ε`, `β → δ → ε`, `ζ` parallel (depends on α for type names). New diagnostic
code `E_SELECTOR_KIND_MISMATCH` is introduced in α/β and is the headline signal for δ.

---

## 10. Out of scope for this PRD

- **FEA Load/Support migration** (`String → FaceSelector`/`BodySelector` on the `fea_multi_case.ri`
  loads). This is the **downstream consumer**, deliberately decoupled (motivation + Leo). It
  becomes a follow-on PRD once this type lands; it depends on this PRD + 4092.
- **Selector → FE node-set mapping** — owned by task **4092**. This PRD stops at
  `resolve() → Vec<GeometryHandleId>`.
- **Full persistent name → sub-shape resolution** — owned by `persistent-naming-v2` (D8).
- **`Vertex` (0-D) selectors** — no current consumer (D2). *(Superseded 2026-06-08: a consumer emerged — FEA `PointLoad.point`. The Vertex kind + `vertex()/vertices()` constructors, plus a kind-agnostic selector param for `FixedSupport.target`, are now owned by `docs/prds/v0_6/fea-load-support-selector-migration.md` as strict extensions of this substrate.)*
- **Imported-geometry selectors** — out of scope (mirrors the root PRD's exclusion).
- **No new topology-selector *functions*** — this PRD re-types and composes the existing family;
  new selector predicates remain the root function-family PRD's territory.

---

## 11. Open questions (tactical — surfaced, not blocking)

1. **`body(g, name)` constructor name.** `body` is a generic identifier and could shadow user
   bindings; alternatives `solid_body` / `volume`. **Suggested:** keep `body`; revisit if a
   collision surfaces. Decide during δ.
2. **Diagnostic code surface.** One `E_SELECTOR_KIND_MISMATCH` for both mixed-kind composition
   and wrong-kind param binding, vs. two codes. **Suggested:** one code, distinguished by message.
   Decide during β.
3. **Cross-target composition.** Whether `union` may combine selectors over *different* bodies
   (each leaf carries its own target, so it is representable) or is restricted to a shared target
   in v1. **Suggested:** allow it (resolve each child against its own target); add a lint if the
   targets differ. Decide during δ.
4. **`Named`-leaf interim diagnostic.** Exact code/text for "selector name unresolved until
   persistent-naming-v2" vs. reusing `W_TOPOLOGY_TAG_STALE`. **Suggested:** reuse the existing
   warning for the unique-tag-miss case; a dedicated `W_SELECTOR_NAME_UNRESOLVED` only if needed.
   Decide during δ.
5. **`Range` literal for `faces_by_area`/`edges_by_length`.** The constructors take a
   `Range<Area>`/`Range<Length>`; confirm the existing `Range` value lowers cleanly into
   `ByArea`/`ByLength` min/max. **Suggested:** reuse `Value::Range`; verify at γ.
