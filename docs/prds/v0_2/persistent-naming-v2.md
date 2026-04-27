# PRD: Persistent Topology Naming v2 (Solvespace-Style)

Status: deferred to v0.2 per 2026-04-26 decision.

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
- Decision made on replace-vs-augment.
- A reference implementation in a similar parametric system (Solvespace, OpenSCAD-CSG, FreeCAD's "Topological Naming Problem" project) has been studied.

## Out of scope for this PRD

- Full algebraic naming theory (a research direction, not an engineering target).
- Cross-kernel face identity preservation under heavy remeshing (post-v0.2 if at all).
- User-facing naming language extensions beyond the existing `@face`, `@edge`, etc. selector vocabulary.
