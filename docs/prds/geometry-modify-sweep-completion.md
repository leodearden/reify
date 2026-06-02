# PRD: Geometry modify + sweep completion (selection overloads, offsets, split, extrude-to, nurbs_surface)

**Status:** active — version-agnostic geometry foundation. Authored 2026-06-02.
**Slug:** `geometry-modify-sweep-completion`
**Approach:** **B** for the self-contained vertical slices (offset_solid, split, fillet_all,
nurbs_surface, offset_surface, offset_curve, thicken_asymmetric, extrude_to); **B+H** for the
**edge/face-selection seam** — the shared mechanism behind per-edge fillet/chamfer, chamfer_asymmetric,
shell_open, and draft (contract + two-way boundary tests below).

Sibling of `docs/prds/geometry-primitive-constructors.md` (the primitives PRD): that PRD makes the
*constructors* real; this PRD makes the *modify/sweep* surface real. It **consumes** the primitives
PRD's dimensionality refinement and 2D-profile constructors, and the kernel-geometry-queries PRD's
`edges`/`faces` selector resolution.

## Goal

A `.ri` author can call every `std.geometry.modify` (§3.5) and `std.geometry.sweep` (§3.6)
operation the stdlib reference advertises and get real geometry — including the **selective** forms
that operate on a curated subset of a solid's edges or faces:

```reify
let b       = box(20mm, 10mm, 15mm)
let top     = edges_at_height(b, 15mm, 0.1mm)            // List<Geometry> of the 4 top edges (KGQ, live)
let rounded = fillet(b, top, 2mm)                        // 3-arg per-edge form — rounds ONLY those 4
let cut     = chamfer_asymmetric(b, top, 1mm, 2mm)       // asymmetric setbacks on a curated edge list
let hollow  = shell_open(b, 1mm, faces_by_normal(b, vec3(0,0,1), 1deg))  // open the top face
let tapered = draft(b, faces_by_normal(b, vec3(1,0,0), 1deg), 3deg, plane_xy(0mm)) // taper one face
let grown   = offset_solid(b, 1mm)                       // grow every face outward 1mm
let pieces  = split(b, plane_xy(5mm))                    // -> List<Solid>, two halves
let patch   = nurbs_surface(grid, weights, u_knots, v_knots, 1, 1)  // first free-standing Surface
let thicker = offset_surface(rectangle(20mm,10mm), 2mm)
let plate   = thicken_asymmetric(rectangle(20mm,10mm), 1mm, 2mm)
let boss    = extrude_to(circle(4mm), patch)             // extrude until it meets a target surface
```

Today the all-edges 2-arg `fillet`/`chamfer`, `shell` (with face *indices*), `draft` (3-arg, no
face selection), `thicken`, and every sweep op already run end-to-end. But the selective/extended
forms above emit either `fillet() expects 2 arguments, got 3`-style arity errors or the bare
`unsupported geometry function: <name>` diagnostic (`reify-compiler/src/geometry.rs:1270`). After
this PRD they evaluate to non-`Undef` geometry, mesh in the GUI viewport, and export to STEP.

## Background — why this exists

The 2026-06-01 stdlib-reference gap survey (`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`,
group **G-B**) found the modify/sweep surface is the most phantom-laden cluster in the geometry
chapter. Two `done` tasks shipped **nothing that landed on main**:

- **Task 315** (`done` 2026-04-14, *"chamfer implementation + asymmetric"*) claims
  `chamfer(solid, edges, distance)` and `chamfer_asymmetric(solid, edges, d1, d2)`. `rg` over
  production code confirms **`chamfer_asymmetric` is absent**, and `chamfer` is the all-edges 2-arg
  form only — no `edges` argument anywhere.
- **Task 316** (`done` 2026-04-14, *"offset_solid, shell_open, split"*) claims all three.
  `rg` confirms **`offset_solid`, `shell_open`, `split`, and `BRepAlgoAPI_Splitter` are all absent**
  from production code (`split` appears only in shell-extraction *metadata*, not as a geometry op).

These are real regressions — fold into this PRD and flag the phantom-done (task μ reconciles them).

Separately, `docs/reify-stdlib-reference.md` §3.5–3.6 advertises a broader surface than the language
has: per-edge `fillet`/`chamfer` selection, `fillet_all`, `offset_surface`, `offset_curve` (3
overloads), `thicken_asymmetric`, a face-selection argument on `draft`, `extrude_to`, and a
standalone `nurbs_surface` constructor (which the primitives PRD explicitly deferred).

The proven template is the existing modify/sweep path, which runs end-to-end through five layers
(cpp FFI → `GeometryOp` variant → `reify-compiler` lowering arm → `reify-eval` construction →
`reify-kernel-occt` execute), exhaustively dispatched with no stub arms
(`reify-kernel-occt/src/lib.rs:2002-2419`).

### The load-bearing decision: selection is a *list argument*, not a new mechanism

The per-edge `fillet`/`chamfer`, per-face `shell_open`/`draft`, and `chamfer_asymmetric` all need a
way to name a *subset* of a solid's edges/faces. The gap register's premise — that this is blocked
on the deferred topology-selector substrate — is **stale**. The kernel-geometry-queries PRD
(`docs/prds/v0_3/kernel-geometry-queries.md`, Phase 3) already shipped, on main:

- `edges(Solid) → List<Geometry>` and `faces(Solid) → List<Geometry>` (task **3616** KGQ-η, `done`)
  via the §4 sub-handle lowering (each list element is a `Value::GeometryHandle` addressing a
  specific OCCT `TopExp`-indexed edge/face).
- the predicate selectors `edges_at_height` / `edges_by_length` / `edges_parallel_to` /
  `faces_by_normal` / `faces_by_area` resolving to the same `List<Geometry>` (task **3560**, `done`;
  resolution in `reify-eval/src/topology_selectors.rs:572-864`), and `adjacent_faces`/`shared_edges`
  (task **3619** KGQ-κ, `done`).

So the selection argument is simply a `List<Geometry>` — exactly what these selectors already
produce. At the type level `Type::Geometry` is opaque (`Curve`/`Surface`/`Solid` collapse to it,
`reify-core/src/ty.rs`), so the doc's `edges: List<Curve>` / `open_faces: List<Surface>` are
`List<Geometry>` today; no typed-selector value is required. This PRD adds the **modify-op argument
plumbing**: a curated-handle field on the op + an eval-side resolution helper that maps each
`List<Geometry>` element to its OCCT sub-handle + a per-element kernel application
(`BRepFilletAPI_MakeFillet::Add(r, edge)` / `MakeChamfer::Add(d, edge)` / per-face removal / per-face
draft). That shared mechanism is the **B+H seam** (Contract + boundary tests below). It is exactly
task **3205**'s scope (deferred only because it was filed against `2699`, which KGQ has since
superseded) — task α re-homes it.

## Scope

**In scope** — the advertised modify/sweep gaps (gap register G-B + the two §3.2-curves/§3.5 items
the primitives PRD deferred):

- **Selection seam (B+H):** the curated-edge/face argument mechanism (`GeometryOp::Fillet { edges }`
  + resolution helper + per-element kernel application).
- **Per-edge / per-face forms:** `fillet(solid, edges, radius)`, `chamfer(solid, edges, distance)`,
  `chamfer_asymmetric(solid, edges, d1, d2)`, `shell_open(solid, thickness, open_faces)`,
  `draft(solid, faces, angle, neutral_plane)` (the 4-arg face-selection form).
- **Self-contained solid ops:** `offset_solid(solid, distance)`, `split(solid, tool: Plane) →
  List<Solid>`, `fillet_all(solid, radius)` (named alias of the existing all-edges fillet).
- **Surface producers + Surface-consuming ops:** `nurbs_surface(...) → Surface`,
  `offset_surface(surface, distance) → Surface`, `offset_curve(curve, distance)` (3 overloads),
  `thicken_asymmetric(surface, above, below) → Solid`, `extrude_to(profile, target: Surface) →
  Solid`.
- Docs + LSP-completion correction; reconciliation of phantom-done 315/316.

**Out of scope:**

- **`box()` positional arg-order mismatch** (§3.2; gap register G-B row) — belongs to the primitives
  PRD / doc-reconcile, not modify/sweep. Not touched here.
- **`revolve(profile, axis: Axis, angle)` overload** — the doc's `Axis`-valued revolve. Current
  `revolve` is the 8-arg scalar-component form (`geometry.rs:1293`); an `Axis`-accepting overload is
  a transform/query concern (gap register G-C), not in this cluster.
- **Typed selector kind-safety** (`Selector(Edge)` ≠ `Selector(Face)` at compile time) — owned by
  the topology-selector-value-type PRD (task **4118**, pending). This PRD consumes untyped
  `List<Geometry>`; forward-compat note below.
- **Runtime planarity/closedness validation** of arbitrary geometry via `BRepCheck` — this PRD
  infers `Surface`/`Planar` only for the constructors that produce them by construction (deferred to
  the geometry-traits runtime-query category, same boundary the primitives PRD drew).
- **Arbitrary-plane profiles**, 2D-profile booleans (the primitives PRD's `geometry-2d-profile-ops`
  follow-up owns those).

## Sketch of approach

### Self-contained vertical slices (bare B) — mirror the existing modify/sweep ops

Each is an independent slice through the §3.1 op-execute seam (`engine-integration-norm.md`),
mirroring how `Shell`/`Thicken` are wired today:

1. **cpp** (`reify-kernel-occt/cpp/occt_wrapper.{h,cpp}`): the OCCT call.
   - `offset_solid` → `BRepOffsetAPI_MakeOffsetShape` (**include already present**, line 41, used by
     `thicken`); positive grows, negative shrinks.
   - `split` → `BRepAlgoAPI_Splitter` (**NEW include** — not yet in the wrapper) with the plane as an
     unbounded cutting face; returns multiple shapes.
   - `nurbs_surface` → build a `Geom_BSplineSurface` from the control-point grid + weights + u/v
     knots/degrees → `BRepBuilderAPI_MakeFace`.
   - `offset_surface` → `BRepOffsetAPI_MakeOffsetShape` in surface mode; `offset_curve` →
     `BRepOffsetAPI_MakeOffset` on a planar wire (+ direction / reference-surface overloads).
   - `thicken_asymmetric` → `BRepOffsetAPI_MakeThickSolid` / two-sided offset of the mid-surface.
   - `extrude_to` → `BRepFeat_MakePrism` "until surface" (or prism + boolean trim against the target).
2. **ffi** (`reify-kernel-occt/src/ffi.rs`): cxx bridge declarations.
3. **IR** (`reify-ir/src/geometry.rs`): new `GeometryOp` variants + `kind_name` arms; new `ModifyKind`
   /`SweepKind` entries (`reify-compiler/src/types.rs:1175`/`:1269`).
4. **compiler** (`reify-compiler/src/geometry.rs` + `geometry_modify.rs`): match arms with exact
   arg-count checks + add names to `GEOMETRY_FUNCTION_NAMES` (`units.rs`) + per-op inference arms
   (`geometry_traits_inference.rs` — `nurbs_surface`/`offset_surface` → `dimension=Surface`).
5. **eval** (`reify-eval/src/geometry_ops.rs`): construct the variant from the call.
6. **kernel execute** (`reify-kernel-occt/src/lib.rs`): validate dims → call FFI. `split` returns a
   `List<Solid>` value (the op-execute path must thread a multi-output result — open question 1).

`fillet_all(solid, radius)` needs **no FFI/IR change**: it is a thin compiler alias lowering to the
existing all-edges `GeometryOp::Fillet { target, radius }` (no `edges` field).

### The edge/face-selection seam (B+H)

The per-edge/per-face ops share one mechanism. Geometry selection today is a `List<Geometry>` whose
elements are sub-handles (KGQ §4). This PRD adds:

- a curated-handle field on the relevant ops — `GeometryOp::Fillet { target, radius, edges:
  Vec<GeometryHandleId> }` (empty = all-edges back-compat), and analogous fields on
  `Chamfer`/`ChamferAsymmetric`/`Shell`(reuse `faces_to_remove`)/`Draft`;
- a shared eval-side **resolution helper** in `reify-eval/src/geometry_ops.rs` that takes the
  `List<Geometry>` argument value, resolves each element to its OCCT sub-shape (reusing the KGQ
  sub-handle decoding), and threads the resolved handle vector onto the op;
- a per-element kernel application: `BRepFilletAPI_MakeFillet::Add(r, edge)` /
  `MakeChamfer::Add(d, edge)` / `MakeChamfer::Add(d1, d2, edge, face)` / per-face removal in
  `MakeThickSolid` / per-face `DraftAngle::Add`.

See the **Contract** + **Boundary-test sketch** for the producer→handle map, the consumer→required
shape, and the **anti-zero-edges** rule that closes the documented fake-done trap (task 3295).

## Resolved design decisions

1. **Selection is an untyped `List<Geometry>` argument**, produced by the live KGQ selectors
   (`edges`/`faces`/`edges_at_height`/`faces_by_normal`/…). No typed-selector value is required; the
   ops are **forward-compatible** with the topology-selector-value-type PRD (4118) — when typed
   selectors land, the arg type tightens to also accept `Selector(Edge)`/`Selector(Face)` that
   coerce to `List<Geometry>` via `ResolveSelector`, with no change to op semantics.
2. **Empty edge list = all edges (back-compat).** The 2-arg `fillet(solid, radius)` /
   `chamfer(solid, distance)` keep working by lowering to the op with an empty `edges` vector, which
   the kernel treats as "apply to every edge" — preserving today's behavior and the
   `fillet_all`/`chamfer` semantics.
3. **`fillet_all` is a documented alias, not a new kernel path** — it lowers to the existing
   all-edges `Fillet`. (The doc lists both `fillet`(per-edge) and `fillet_all`; this realizes the
   name without duplicating the kernel call.)
4. **Plane/Axis values are constructible today.** `plane_xy/plane_xz/plane_yz/axis_x/axis_y/axis_z`
   exist and evaluate (`reify-stdlib/src/geometry.rs:804-811`, `make_plane`:1091, `make_axis`:1131) —
   the gap register's "missing" verdict grepped only `reify-compiler`/`reify-eval` and missed
   `reify-stdlib`. So `split(b, plane_xy(5mm))` and `draft(…, neutral_plane = plane_xy(0mm))` have a
   real `Plane` argument; the work is the op's *consumption* of it, not the constructor.
5. **`nurbs_surface` gets a concrete signature now** (the doc's is a placeholder), mirroring the
   `nurbs` curve constructor (task 320): `nurbs_surface(control_points: List<List<Point3<Length>>>,
   weights: List<List<Real>>, u_knots: List<Real>, v_knots: List<Real>, u_degree: Int, v_degree: Int)
   → Surface`. It is the **first free-standing `Surface` producer** and unblocks `offset_surface`
   inputs and `extrude_to` targets.
6. **Surface-dimension geometry is the primitives PRD's `dimension=Surface` refinement**, not a new
   value-type — same decision the primitives PRD made. `nurbs_surface`/`offset_surface` set
   `dimension=Surface` on the inferred-traits record (`geometry_traits_inference.rs:115-151`,
   landed). No `Type::Surface`.
7. **`split` returns `List<Solid>`** — a multi-output geometry op. The op-execute path already
   produces `List<Geometry>` for selectors; `split` reuses that value shape.
8. **Phantom-done 315/316 are reconciled, not silently re-shipped.** Task μ reopens/annotates them as
   superseded, pointing at the new leaf IDs that actually land the work.

## Pre-conditions for activating

- **Selector resolution is live** — `edges`/`faces`/`edges_at_height`/`faces_by_normal` resolve to
  `List<Geometry>` sub-handles on main (KGQ tasks 3616/3619/3560, `done`). Verified via
  `reify-eval/src/topology_selectors.rs` + `topology_selector_smoke_tests.rs`.
- **2D-profile constructors** (`rectangle`/`circle`/`polygon`/`ellipse` → `dimension=Surface`) are
  the **primitives PRD tasks 4160/4161 (pending)** — the Surface-consuming leaves (κ
  thicken_asymmetric, θ offset_surface, λ extrude_to) **hard-depend** on them. Wired as real
  cross-batch `add_dependency` edges; those leaves stay blocked until the profiles land (honest, not
  fake-done).
- **Dimensionality refinement + `Planar`** (primitives PRD task α / **4155, `done`**) — reused by the
  Surface producers' inference arms.
- **Plane/Axis constructors** ship today (decision 4).
- Constructor-call, nested-call, list-literal, and list-return syntax all parse (confirmed via
  `reify check` on `/tmp/prd-gate-fixtures/modify-sweep-{1,2}.ri`: every fragment parses; the only
  diagnostics are semantic arity/`unsupported`-function errors — the gaps themselves).
- **No novel syntax → G3 grammar gate N/A.**

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometry-primitive-constructors.md` (sibling; α `done`/4155, profiles `pending`/4160,4161) | consumes | `dimension=Surface` inference + `rectangle`/`circle` profiles (input to thicken_asymmetric / offset_surface / extrude_to) | **primitives-prd** | hard cross-batch deps on 4160/4161; α/4155 done |
| `kernel-geometry-queries.md` (shipped: 3616/3619/3560) | consumes | `edges`/`faces`/predicate selectors → `List<Geometry>` sub-handles | **kgq-prd** | live on main; this PRD's selection seam depends on it |
| `topology-selector-value-type.md` (4118, pending) | forward-compat | when typed `Selector(Edge)`/`Selector(Face)` land, modify-op arg types tighten to also accept them via `ResolveSelector` | **selector-value-prd** | **not a dependency** — untyped `List<Geometry>` is the v1 arg; no contested ownership |
| `persistent-naming-v2` (`reify-eval/src/topology_attribute_propagation.rs`; 2655/2831) | preserves | fillet/chamfer `mod_history` threading must survive the new `edges` field (curated-edge feature still seeds attributes on generated faces) | **persistent-naming-prd** | additive — α must not break the existing history hook; covered by α's boundary test |
| `engine-integration-norm.md` §3.1 op-execute | consumes | new `GeometryOp::{OffsetSolid,Split,ChamferAsymmetric,NurbsSurface,OffsetSurface,OffsetCurve,ExtrudeTo}` + curated-handle fields | n/a (norm) | the catalogued seam each new op plugs into |

No new contested-ownership pair is introduced (the three known contested seams in the overlay are
untouched). The `persistent-naming` row is a high-stakes seam (overlay G5) — handled by α's two-way
boundary test + the additive-field design (empty `edges` = today's behavior).

## Contract — the edge/face-selection seam (H)

**Curated-handle field.** Extend the selection-capable ops with a curated sub-handle vector
(empty ⇒ all sub-shapes, preserving today's all-edges/all-faces semantics):

| Op | Field added | Empty-vector meaning |
|---|---|---|
| `Fillet` | `edges: Vec<GeometryHandleId>` | all edges (today's 2-arg form) |
| `Chamfer` | `edges: Vec<GeometryHandleId>` | all edges |
| `ChamferAsymmetric` (new) | `edges: Vec<GeometryHandleId>`, `d1`, `d2` | all edges |
| `Shell` | reuse `faces_to_remove` (sub-handles, not just usize indices) | none removed (today's `shell`) |
| `Draft` | `faces: Vec<GeometryHandleId>` | all draftable faces |

**Producer → handle.** The argument is a `List<Geometry>` whose elements are KGQ sub-handles
(`Value::GeometryHandle` addressing a `TopExp`-indexed edge/face). Producers: `edges(b)`, `faces(b)`,
`edges_at_height`, `edges_by_length`, `edges_parallel_to`, `faces_by_normal`, `faces_by_area`,
`adjacent_faces`, `shared_edges` (all live on main).

**Resolution helper (the single executor).** `reify-eval/src/geometry_ops.rs` gains one helper:
`resolve_subhandle_list(arg: &Value, parent: GeometryHandleId) → Result<Vec<GeometryHandleId>,
Diagnostic>` that (a) requires the argument to be a `List<Geometry>`, (b) decodes each element's
KGQ sub-handle, (c) verifies each addresses a sub-shape *of `parent`* (rejects cross-solid handles),
(d) returns the canonical-ordered, deduped handle vector. Used by Fillet/Chamfer/ChamferAsymmetric
/Shell/Draft alike.

**Kernel application.** The kernel applies the op to **exactly** the resolved sub-shapes:
`BRepFilletAPI_MakeFillet::Add(r, edge)` per resolved edge; `MakeChamfer::Add(d, edge)` /
`Add(d1, d2, edge, face)`; `MakeThickSolid` face-removal list; `DraftAngle::Add(face, dir, angle,
plane)` per resolved face — never the all-edges loop when the handle vector is non-empty.

**Anti-zero-edges rule (closes the fake-done trap).** If the resolution helper yields an **empty**
vector *from a non-empty selector argument* (selector matched nothing, or all handles were
cross-solid and rejected), the op emits a **blocking diagnostic** (`E_EMPTY_SELECTION` or analog) —
it does **not** silently fall through to all-edges and it does **not** return an unmodified solid.
This is the exact regression task 3295 warned about ("kernel invoked but fillets zero edges").

## Boundary-test sketch (H) — faces both sides of the selection seam

| Scenario | Precondition | Postcondition (asserted) |
|---|---|---|
| per-edge fillet rounds only the selection | `fillet(b, edges_at_height(b,15mm,0.1mm), 2mm)` on a 20×10×15 box | evals to `Solid`; **resolved edge set size == 4**; result volume ≠ unfilleted box AND ≠ `fillet_all(b,2mm)` (proves non-empty AND non-all) |
| op records the curated handles | same | recorded `GeometryOp::Fillet { edges, .. }` has `edges.len() == 4` (pins task 3282's assertion, ungated) |
| empty selection is rejected, not silent | `fillet(b, edges_at_height(b, 999mm, 0.1mm), 2mm)` (matches nothing) | **blocking `E_EMPTY_SELECTION`** — not an unmodified solid |
| cross-solid handle rejected | `fillet(b, edges(other_box), 2mm)` | diagnostic — handles don't address `b`'s sub-shapes |
| all-edges back-compat unchanged | `fillet(b, 2mm)` (2-arg) | lowers to empty-`edges` Fillet; mesh identical to today |
| per-face shell opens exactly the selection | `shell_open(b, 1mm, faces_by_normal(b, vec3(0,0,1), 1deg))` | hollow box, **1 face removed**; wall thickness 1mm; volume = shell-wall volume (not solid, not all-faces-open) |
| persistent-naming survives | filleted-edge feature on `b` | generated fillet faces still carry `(feature_id, role)` attributes (history hook intact) |

Task α's observable signal **is** rows 1–5 (the curated-set + anti-zero-edges + back-compat pins);
β/γ/δ reuse the helper and add their per-op positive rows — closing the G2 loop.

## Decomposition plan

Greek labels are intra-batch; actual IDs assigned at decompose time. Each leaf names its
user-observable signal (G2) and the capabilities it asserts (G6 / manifest).

**Phase 1 — selection seam (B+H integration gate; re-homes task 3205)**

- **α — curated edge/face selection seam.** Modules: `reify-ir/src/geometry.rs` (`edges` field on
  `Fillet`), `reify-compiler/src/geometry.rs`+`geometry_modify.rs` (3-arg `fillet` arm),
  `reify-eval/src/geometry_ops.rs` (`resolve_subhandle_list` helper + Fillet dispatch),
  `reify-kernel-occt/src/{lib.rs,ffi.rs,cpp}` (per-edge `MakeFillet::Add`).
  *Signal (leaf, the Contract's two-way boundary test):* `fillet(b, edges_at_height(b,15mm,0.1mm),
  2mm)` rounds exactly 4 edges — resolved set size == 4, volume ≠ unfilleted AND ≠ `fillet_all`;
  empty-selection (`edges_at_height(b,999mm,…)`) → blocking `E_EMPTY_SELECTION`; 2-arg `fillet(b,2mm)`
  unchanged. *Dep:* 3616 (KGQ edges/faces, done). *Unlocks:* β, γ, δ. Re-homes **3205**; **3295**
  depends on this.

**Phase 2 — selection-consuming ops (depend on α's helper)**

- **β — per-edge `chamfer` + `chamfer_asymmetric`.** Adds `edges` to `Chamfer`, new
  `ChamferAsymmetric { edges, d1, d2 }`; kernel `MakeChamfer::Add(d,edge)` / `Add(d1,d2,edge,face)`.
  *Signal:* `chamfer(b, edges_at_height(b,15mm,0.1mm), 1mm)` chamfers only those 4 edges (face count
  + volume delta vs all-edges); `chamfer_asymmetric(b, <those edges>, 1mm, 2mm)` yields distinct
  setbacks (the two chamfer-face widths measured via bbox/section differ as 1mm:2mm within 5%);
  zero distance → diagnostic. **Supersedes phantom-done 315.** *Dep:* α.
- **γ — `shell_open(solid, thickness, open_faces)`.** Resolve `open_faces: List<Geometry>` →
  face sub-handles → existing `MakeThickSolid` removal list. *Signal:* `shell_open(box(10,10,10),
  1mm, faces_by_normal(box, vec3(0,0,1), 1deg))` → hollow box open on top (1 face removed; volume ≈
  shell-wall volume within 3%); empty selection → diagnostic. **Supersedes phantom-done 316
  (shell_open).** *Dep:* α.
- **δ — `draft(solid, faces, angle, neutral_plane)` (4-arg face-selection form).** Add the `faces`
  argument (today's draft is 3-arg, no selection); per-face `DraftAngle::Add`. *Signal:* `draft(box,
  faces_by_normal(box, vec3(1,0,0), 1deg), 3deg, plane_xy(0mm))` tapers only the +X face by 3° about
  z=0 (drafted face normal tilts 3° ± 0.1°; other faces unchanged); empty selection → diagnostic.
  *Dep:* α.

**Phase 3 — self-contained solid ops (bare B; no α dep)**

- **ε — `offset_solid` + `fillet_all`.** `offset_solid`: new `GeometryOp::OffsetSolid` →
  `MakeOffsetShape` (include present). `fillet_all`: compiler alias → all-edges `Fillet`.
  *Signal:* `offset_solid(box(10,10,10), 1mm)` volume > original; `offset_solid(box, -1mm)` volume <
  original; `offset_solid(box, -100mm)` (collapses) → diagnostic; `fillet_all(box, 2mm)` evals
  identical to the 2-arg `fillet(box, 2mm)`. **Supersedes phantom-done 316 (offset_solid).**
- **ζ — `split(solid, tool: Plane) → List<Solid>`.** New `GeometryOp::Split` + new FFI
  `split_shape` via `BRepAlgoAPI_Splitter` (new include). Multi-output value. *Signal:*
  `split(box(10,10,10), plane_xy(5mm))` → `List<Solid>` of length 2, each volume ≈ 500mm³ within
  2%; `split` by a non-intersecting plane → list of length 1 (the original). **Supersedes
  phantom-done 316 (split).**

**Phase 4 — Surface producers + Surface-consuming ops**

- **η — `nurbs_surface(...) → Surface` (full slice).** New FFI building `Geom_BSplineSurface` → face;
  `GeometryOp::NurbsSurface`; inference arm → `dimension=Surface`, `bounded=true`, `planar=false`.
  Signature per decision 5. *Signal:* `nurbs_surface(<2×2 grid with one lifted corner>, <unit
  weights>, [0,0,1,1], [0,0,1,1], 1, 1)` evals to a non-`Undef` Surface-dimension geometry; a sampled
  interior point lies on the bilinear patch within tol; STEP export non-empty; using it as an
  `extrude` profile → `GeometryProfileRequired` (it's not `Closed`). *Dep:* 4155 (dimensionality,
  done).
- **θ — `offset_surface(surface, distance) → Surface`.** New FFI `MakeOffsetShape` (surface mode);
  inference → `dimension=Surface`. *Signal:* `offset_surface(rectangle(20mm,10mm), 2mm)` → a Surface
  whose centroid is offset 2mm along the +Z normal (bbox z ≈ 2mm); area ≈ input area within 2%.
  *Dep:* primitives **4160** (rectangle → Surface).
- **ι — `offset_curve(curve, distance)` (3 overloads).** Overload 1 (2D planar): `MakeOffset` on a
  planar wire. Overload 2 (`reference: Surface`): offset along the surface. Overload 3 (`direction:
  Vector3<Dimensionless>`): offset in a given direction (vec3 exists, `reify-stdlib`:923). *Signal:*
  `offset_curve(<planar arc r=10mm>, 2mm)` → a Curve whose radius is 12mm (arc length ratio 12/10
  within 2%); `offset_curve(c, 2mm, vec3(0,0,1))` and `offset_curve(c, 2mm, <ref surface>)` each eval
  to a non-`Undef` Curve. *Dep:* 320 (curve ctors, done); overload-2 reference uses a `faces()`
  sub-handle (live) or η.
- **κ — `thicken_asymmetric(surface, above, below) → Solid`.** Two-sided offset of the mid-surface.
  *Signal:* `thicken_asymmetric(rectangle(20mm,10mm), 1mm, 2mm)` → a Solid of total thickness 3mm,
  bbox z ∈ [−2mm, +1mm] (asymmetry verified); volume ≈ 200mm²·3mm within 3%. *Dep:* primitives
  **4160** (rectangle → Surface).
- **λ — `extrude_to(profile, target: Surface) → Solid`.** `BRepFeat_MakePrism` "until surface" (or
  prism + boolean trim). *Signal:* `extrude_to(circle(4mm), <a target surface tilted so its height
  ranges 8–12mm over the profile footprint>)` → a Solid whose top face conforms to the target (max
  height = the target's far-edge height within 5%, min height = near-edge height); extruding to a
  non-overlapping target → diagnostic. *Dep:* primitives **4160** (circle profile) + **η**
  (nurbs_surface as the tilted target) — or a `faces()` sub-handle target (live) if η slips.

**Phase 5 — companion correction (depends on all impl leaves)**

- **μ — docs + LSP completion + 315/316 reconciliation.** Modules: `docs/reify-stdlib-reference.md`,
  `reify-lsp/src/completion.rs`, the gap register. *Signal:* LSP completion offers
  `fillet_all`/`offset_solid`/`split`/`chamfer_asymmetric`/`shell_open`/`offset_surface`/
  `offset_curve`/`thicken_asymmetric`/`extrude_to`/`nurbs_surface`; a doc-example `.ri` exercising
  each runs clean under `reify check`; the stdlib reference §3.5–3.6 no longer lists any as
  callable-without-caveat while unimplemented; phantom-done **315** and **316** reopened/annotated as
  superseded by this PRD's leaf IDs; gap-register G-B rows updated. *Dep:* α,β,γ,δ,ε,ζ,η,θ,ι,κ,λ.

**Dependency edges:** β→α, γ→α, δ→α; α→3616(done); θ→4160, κ→4160, λ→{4160,η}; η→4155(done);
ι→320(done); 3295→α; μ→{α,β,γ,δ,ε,ζ,η,θ,ι,κ,λ}. ε/ζ/η have no intra-batch prereqs.

## Open questions (tactical — decide at implementation)

1. **Multi-output op-execute for `split`.** `split → List<Solid>` needs the op-execute path to yield
   a list value. The selector path already produces `List<Geometry>`; confirm `split` reuses that
   result shape (vs. a dedicated multi-shape kernel-result variant). Decide in task ζ.
2. **Sub-handle decode reuse.** The resolution helper decodes KGQ sub-handles. Confirm the KGQ
   `SubKind`/`upstream_values_hash` decode (`reify-eval/src/geometry_ops.rs`, KGQ-η) is callable from
   the modify path without a circular module dep. Decide in task α.
3. **`extrude_to` target acquisition.** Whether the cleanest target is `BRepFeat_MakePrism`'s
   "until" face (needs the target as an OCCT face handle) vs. prism + `BRepAlgoAPI_Cut`. Decide in
   task λ; informs whether η is a hard dep or `faces()` suffices.
4. **`chamfer_asymmetric` IR shape.** New `ChamferAsymmetric` variant vs. extending `Chamfer` with an
   optional second distance. *Suggested:* new variant (matches the one-variant-per-op convention).
   Decide in task β.
5. **`nurbs_surface` weight/knot validation.** Degree-vs-knot-count consistency and weight-grid
   shape checks at the kernel boundary (mirror the `nurbs` curve validation). Decide in task η.
