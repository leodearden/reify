# PRD: Persistent Topology Naming v2 (Solvespace-Style)

Status: deferred to v0.2 per 2026-04-26 decision.
Design resolved 2026-04-28 — see "Resolved design decisions" below.

## Goal

Replace or augment the v0.1 feature-tag persistent-naming scheme with an attribute-based scheme in the spirit of Solvespace: features attach stable IDs to the faces and edges they create, and the IDs survive most parameter changes and topology edits. This addresses architecture §16 open question #10 (geometric queries and selectors / persistent naming).

## Background

Persistent naming is the "identify the same face after the model has been edited" problem — long known to be hard in parametric CAD. The v0.1 spec §6.1 acknowledges the limitation directly: only construction-history-named features (`@face(top)`) are stable; computed selectors (`@face(faces_by_normal(...))`) may become invalid when upstream parameters change topology, with the ad-hoc port's frame falling back to `undef`.

This is workable for v0.1 because:

- Most well-modeled designs name their important features explicitly.
- Computed-selector breakage produces a clear diagnostic (broken selector + the parameter change that triggered it), so failures are debuggable.
- The full solution interacts with topology, kernels, and constraint systems in ways that need the rest of the architecture stable first.

But v0.1's feature-tag scheme breaks under common edits — a fillet that removes an edge, a Boolean that splits a face, parameter changes that re-order topology in OCCT's internal representation. Users end up either over-relying on construction history naming (brittle to refactors) or routing constraints through coordinate-based queries (defeats parametric intent).

## Why deferred

- The v0.1 scheme is functional with clear failure semantics — designers know when a selector breaks.
- Persistent naming v2 needs the rest of the geometry stack stable: multi-kernel dispatch (`multi-kernel.md`) changes which kernel creates which faces, which directly affects what attributes are available for naming.
- Solvespace-style attribute naming is not a drop-in algorithm — it requires changes throughout the geometry pipeline (every feature must annotate created topology) and a redesign of the selector resolution logic.
- The architecture (§16 #10) lists this as open at v0.1 priority, but the scope of the redesign is squarely v0.2+.

## Sketch of approach

Solvespace's approach (and similar attribute-based schemes in commercial CAD) attaches a stable ID to every face/edge/vertex created by a feature, derived from the feature's identity plus a local index within the feature's contribution. When the feature re-runs with different parameters, it re-attaches the same IDs to the analogous outputs. Selectors then resolve via attribute lookup rather than geometric query.

Concretely for Reify: every constructive operation (`extrude`, `revolve`, `fillet`, `union`, etc.) is wrapped in a layer that, given the topology produced by the underlying kernel, walks the result and attaches `(feature_id, role, local_index)` attributes. `feature_id` is the stable evaluation-graph node identity (already stable by §6.5 path-based identity). `role` distinguishes "side face of extrusion" from "cap face". `local_index` orders multiple instances of the same role.

Selector resolution becomes attribute lookup: `@face(top)` matches a face whose `(feature_id, role)` pair corresponds to the named attachment. Computed selectors still exist as a fallback for cases where attribute-based naming is impossible (e.g. naming a face that was created by an unknown imported STEP file).

The augmentation question — replace v0.1's scheme entirely, or layer the new one on top — is a design decision for the implementation phase. Layering preserves backward compatibility with v0.1 source files; replacing is cleaner but breaks them.

The interaction with multi-kernel dispatch (`multi-kernel.md`) is significant: each kernel adapter must implement the attribute-attachment hook, and conversions between kernels must propagate attributes (B-rep face attributes survive triangulation as triangle-tagged attributes; mesh attributes survive remeshing within tolerance).

## Pre-conditions for activating

- v0.1 alpha has shipped and real users have documented persistent-naming pain (likely common).
- Multi-kernel dispatch design is locked — naming has to span kernel boundaries.

## Resolved design decisions (2026-04-28)

Backed by the 2026-04-28 reference-implementation study covering Solvespace, FreeCAD-RealThunder, OpenSCAD-CSG, CadQuery / build123d, OnShape, Parasolid, and the academic literature (Kripac 1995, Bidarra 2005, Kim et al. 2016, Marcheix & Pebay 2018, Manifold's MeshGL pattern).

**Replace, not augment.** Pre-release means no source files to break. v0.2 absorbs the v0.1 `name = "..."` syntax as an additional attribute slot rather than maintaining a parallel scheme. One mechanism, one mental model.

**Attribute shape.** Each face/edge/vertex emitted by a constructive operation carries:
```
TopologyAttribute {
    feature_id: FeatureId,           // path-based node identity (§6.5), already stable
    role: Role,                       // per-op enum (Cap(top), Side, NewEdge, ...)
    local_index: u32,                 // deterministic geometric ordering, construction-order tiebreak
    user_label: Option<String>,       // absorbs v0.1 `name = "top"` syntax
    mod_history: Vec<ModEntry>,       // lineage postfix; see below
}
```
Selector resolution prefers `user_label` matches over `(role, local_index)` matches when both apply.

**Modification-history postfix.** When a downstream feature splits a face — fillet bisecting an edge, Boolean cutting a face — children inherit the parent's `(feature_id, role, local_index)` plus a `ModEntry { splitting_feature_id, split_index }` appended to `mod_history`. Selectors that resolved to the parent return the *set* of children, surfacing ambiguity for the user to disambiguate rather than silently rebinding. Pattern from FreeCAD-RealThunder (`;:M2`/`;:G3` postfix), Kim et al. 2016 (`[n:total]` SFI), OnShape (`qSplitBy`).

**Local-index ordering.** Deterministic geometric ordering (centroid, normal, principal axis, etc.) with construction-order tiebreak only for genuine geometric ties. Symmetric splits (e.g. fillet of full circular edge) are unsolved across the literature; we accept arbitrary tiebreak with a diagnostic.

**Selector resolution unified.** Attribute lookup is the only path for native geometry. Computed-selector fallback (`@face(faces_by_normal(...))`) survives only for imported geometry (STEP/STL/etc.) where no construction history exists.

**Multi-kernel propagation via `KernelAttributeHook` trait.** Generic best-effort hook. Manifold (when 2295 lands) provides the first concrete impl using its existing `originalID` + per-triangle `faceID` + `MeshGL` merge-vector pattern (one geometric vertex, multiple property-vertices for attribute discontinuities at intersection curves). Fidget/OpenVDB don't implement the trait — selectors over SDF or voxel reps fall through to computed selectors. Heavy remeshing within tolerance discards attributes with a diagnostic; we don't try to preserve them through arbitrary geometric transformations.

**Diagnostic on local_index reassignment.** Emit when an existing selector's resolved topology changes after an edit purely due to ordering shuffle (i.e. not because of a split — splits are handled by mod_history).

**Selector vocabulary v2.** Lifted from CadQuery / build123d / OnShape with project-specific extensions:

- **Direction filters:** `+X`, `-X`, `|Z` (parallel-to-axis), `#X` (perpendicular), `+vec(<vec3>)` (arbitrary direction — required for non-Cartesian use cases like mould-pull alignment for parting-line selection).
- **Extremal selectors, two flavours:** by-bounds (`>Y`) and by-center (`>>Y`). They differ for non-flat faces; conflating them is a footgun. Both are first-class.
- **Geometry-type filters:** `%Plane`, `%Cylinder`, `%Cone`, `%Sphere`, `%Torus`, `%Circle`, `%Line`, plus `%Geom` (universal match — any Geometry-implementing type, used as a no-op filter when chaining other predicates).
- **Boolean combinators:** `and`, `or`, `not`, `except`.
- **Topological walks:** `adjacent_to(x)`, `owner_body(x)`, `ancestors(x)`, `siblings(x)`.
- **History-based selectors:** `created_by(feature_id)` (OnShape's `qCreatedBy`), `split_by(feature_id)` (`qSplitBy`).
- **Attribute primitives:** `has_attribute(key)`, `attribute_eq(key, value)`. These are what makes the auto-attribute scheme directly user-queryable from source.

**OCCT integration notes:**

- `BRepAlgoAPI_*` modify/generated/deleted hooks cover most ops cleanly.
- Loft (`BRepOffsetAPI_ThruSections`), sweep, and sewing (`BRepBuilderAPI_Sewing`) do **not** expose standard `Modified`/`Generated` maps. Custom history mappers required, following the FreeCAD pattern. Budget extra implementation effort for the Sweeps task.

## Decomposition plan

The PRD decomposes into 10 tasks when activated:

1. **Attribute data model + propagation through OCCT.** Tuple definition, storage on the realization handle, `BRepAlgoAPI_*` modify/generated/deleted hooks, `TopExp_Explorer` walks. The data model and propagation are tightly coupled — single integration test exercises both.
2. **Selector resolution.** Attribute-lookup primary path; computed-selector fallback (imported geometry only); user-label preference rule.
3. **Modification-history threading.** Feature-split detection, `mod_history` propagation, set-valued resolution semantics for selectors that hit ambiguity.
4. **Local-index reassignment diagnostic.** Emit when a selector's resolved topology changes due to ordering shuffle.
5. **Sweeps.** Extrude, revolve, sweep, loft. Custom OCCT history mappers for loft and sweep (which don't expose standard `Modified`/`Generated` maps). Larger task than the per-op-theme average.
6. **Primitives.** Box, cylinder, sphere, cone, torus.
7. **Local features.** Fillet, chamfer.
8. **Booleans.** Union, intersect, subtract.
9. **`KernelAttributeHook` trait + Manifold implementation.** Generic hook; Manifold gets the first concrete impl using `originalID`/`faceID`/`MeshGL` merge vectors. Gated on Manifold landing per 2295.
10. **Selector vocabulary v2.** Direction (incl. `+vec`), extremal (both flavours), geometry-type filter (incl. `%Geom` universal), Boolean combinators, topological walks, history-based selectors, attribute primitives. Splittable into `10a` (cheap: direction, type, combinators) and `10b` (walks, history, attribute queries) if too big for one task.

## Deferred to v0.3+

- **Adjacent-element backup keys.** Kim et al. 2016 propose `FaceName1#FaceName2` for edges and face-triple for vertices, exploiting adjacency as a more robust invariant than direct identity. Deferred because the modification-history postfix handles the most common failure modes; revisit when telemetry shows residual reliability gaps in the primary key.
- **Hash-compaction of attribute names.** RealThunder's StringHasher trick (FreeCAD hit 22.6 MiB attribute storage on real models, compressed to 3.6 MiB via SHA-1 segment hashing). Our scheme is short by construction; deferred until telemetry shows modification-history postfixes accumulating to memory or perf pain in deeply-stacked feature trees.

## Deferred to v0.3+ (tracker bookmarks)

The items below are tracked future work — concrete extensions with activation triggers and
references — not active v0.2 scope and not items we rule out. They differ from the "Out of
scope" list, which records things we explicitly do not plan to implement. Each subsection
corresponds to a task bookmark that survives in source control alongside this PRD.

### Hash-compaction of attribute storage (task #2561)

**Applicability.** This bookmark applies if the v0.2 implementation (or a subsequent
iteration) introduces any attribute-name component that grows unboundedly with edit depth —
for example a modification-history postfix such as `:mod_history=…`. The v0.2 "Sketch of
approach" describes a fixed-width `(feature_id, role, local_index)` tuple and does not yet
specify such a component; if that remains true through release this bookmark is dormant.

**Idea.** Apply the RealThunder StringHasher pattern from the FreeCAD Topological Naming
project: segment-hash any unbounded attribute-name component into a fixed-width digest and
persist it alongside a digest → original-substring sidecar table. Attribute storage shrinks
because the variable-length component is replaced by a short hash. The sidecar allows
lossless round-trip when the full string is needed.

**Published numbers** (RealThunder FreeCAD implementation, studied 2026-04-28; see reference
below for source): order-of-magnitude reduction in attribute storage size, at the cost of a
modest recompute overhead (~30% in the reported benchmark) because hashing is not free.

**Why deferred.**
- The v0.2 attribute tuple `(feature_id, role, local_index)` is fixed-width by construction;
  any future variable-length component is the only growth surface.
- Variable-length component length would grow with edit depth, not model complexity — most
  designs stay shallow.
- The hash sidecar adds debugging and round-trip overhead: `:H7af3b2c1:` is opaque without
  the sidecar, complicating error messages and serialized diagnostics.

**When to action.**
- Telemetry shows per-realization attribute storage exceeding ~10 MiB.
- Per-realization serde wall-time exceeds 50 ms attributable to attribute handling, confirmed
  by profile evidence.
- A connected kernel rejects or truncates attribute names above a documented byte limit (e.g.
  Manifold via `KernelAttributeHook`; substitute the actual limit when known).

**Reference.** RealThunder Topological Naming Algorithm wiki, StringHasher section:
<https://github.com/realthunder/FreeCAD_assembly3/wiki/Topological-Naming-Algorithm>
(studied 2026-04-28).

## Out of scope for this PRD

- Full algebraic naming theory (a research direction, not an engineering target).
- Cross-kernel face identity preservation under heavy remeshing (best-effort only; lossy remeshing emits a diagnostic and discards attributes).
- Cross-tool stability for STEP/IGES round-trips (other kernels assign identity differently; computed-selector fallback handles imported geometry).
