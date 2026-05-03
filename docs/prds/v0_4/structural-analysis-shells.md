# PRD: Shell Elements for Thin-Body Structural Analysis

Status: stub — deferred, candidate v0.4. Sibling to v0.3 linear-elastostatic FEA. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Add 2D shell elements (Mindlin-Reissner triangles, DKT, or MITC3+) to Reify's structural-analysis stack so that thin bodies — flexures, sheet metal, panels, casings — solve accurately and cheaply. Tet-only FEA underperforms badly here; shells are the conventional answer in commercial CAD-FEA.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships P1/P2 tetrahedral elements only. This is fine for blocky bodies but breaks down for thin features:

- **Shear locking** — linear (P1) tets exhibit severe shear locking in bending, often underestimating thin-body deflection by 30–50%. P2 tets reduce locking substantially but don't eliminate it; even P2 underperforms shells by 2–5× on thin features.
- **Element-count explosion** — maintaining sane aspect ratio (<10:1) through a 1mm flexure in a 100mm part demands many tets across thickness, which cascades to many tets longitudinally. A single flexure can drive element counts to 50K–200K when the underlying physics needs ~500 DOFs.
- **Stress concentrations at flexure necks** — tet meshes need local refinement that nobody asks for explicitly.

The v0.3 PRD acknowledges this gap and adds a thin-body diagnostic warning that points at this PRD as the eventual fix.

Shell elements are mature in commercial FEA: Abaqus S3R, Ansys SHELL181, MFEM RT_TraceFiniteElement, etc. Reify's handwritten faer-rs solver path makes adding shells tractable — element kernel is the new work, the linear-algebra layer is reused.

## Why deferred to v0.4

- Needs **v0.3 linear-static FEA shipped** as foundation — shares solver kernel, BCs, materials, options. Building shells before tets is backwards.
- **Mid-surface extraction from B-rep is a hard CAD problem** in itself. OCCT and Manifold have partial capabilities; full robustness across arbitrary geometries is research-grade. v0.4 timing lets us spike on the easier cases first.
- **Shell formulation choice has long-term consequences** — Mindlin-Reissner with DKT is the classical choice; MITC3+ is more modern and robust on coarse meshes. Picking right requires actual user pain to learn from.
- **Mixed shell/tet coupling** at junctions (a flexure attaches to a solid block) is its own design surface — not optional for real assemblies, but solvable independently of shell formulation.

## Sketch of approach

Three logically separable pieces:

1. **Shell element formulation.** Implement DKT (Discrete Kirchhoff Triangle) or MITC3+ shell elements: 3-node triangle, displacement + rotation DOFs at each node, integrated through-thickness analytically given a thickness parameter. P2 variants (6-node) for higher accuracy.
2. **Mid-surface extraction.** Detect thin features (length/thickness > threshold), extract mid-surface from B-rep (OCCT `BRepOffsetAPI_MakeOffset` or Manifold-based approach), tag with thickness parameter. Handle simple cases (constant thickness) first; varying-thickness deferred.
3. **Shell/tet coupling at junctions.** Constraint equations or transition elements where a shell meets a solid. Multiple standard approaches; pick simplest that works for v0.4 use cases.

User-visible options:
- **Auto-detect mode (default):** Reify identifies thin features, asks user to confirm shell treatment via a diagnostic suggestion.
- **Explicit opt-in:** annotate a body with `@shell(thickness = ...)` to force shell treatment.
- **Explicit opt-out:** annotate with `@solid` to force tet treatment even on thin bodies.

## Pre-conditions for activating

- v0.3 linear-static FEA shipped (kernel, BCs, materials, mesher, validation suite).
- Topology selectors mature enough to identify mid-surfaces and to express face-tagged thicknesses.
- Concrete user demand for thin-body FEA (flexures, sheet metal designs in active use).

## Open design questions

- **Shell formulation pick** — DKT (simple, well-validated, slight thick-shell weakness) vs. MITC3+ (more robust, slightly more complex) vs. a 4-node quad option (better for sheet metal but requires quad meshing).
- **Mid-surface extraction approach** — OCCT-native? Manifold-based? Custom voxel-then-medial-axis? Each has different robustness/quality trade-offs.
- **Auto-detection threshold** — at what aspect ratio does Reify suggest shells? Probably configurable.
- **Coupling at shell/tet junctions** — penalty constraints, MPCs (multi-point constraints), or transition elements? Lean: MPCs (clean, well-understood).
- **Composite / laminated shells** — definitely out of scope for v0.4; revisit when aerospace/composite users emerge.
- **Curved shells with through-thickness variation** — out of scope for v0.4; constant thickness only.
- **Shell stability / buckling analysis** — separate PRD, v0.5+.

## Out of scope for this PRD

- Composite (laminated) shells — domain-specific add-on.
- Shells with thickness varying across the surface — needs separate UX design.
- Shell stability / buckling — separate analysis kind.
- Membrane-only or plate-only formulations — covered as degenerate cases of full shell.
- Beams / 1D structural elements — sibling PRD if demand emerges.
- Auto-detection of thin features done in v0.3 (warning only) — this PRD makes the warning actionable.

## Relationship to other PRDs and tasks

- **Successor to `structural-analysis-fea.md`** — addresses the thin-body limitation explicitly called out in v0.3. Shares solver kernel, materials, BC framework, options structure.
- **Partial overlap with `hex-wedge-meshing.md`** — both address thin-body issue but via different routes (hex/wedge is anisotropic 3D solid mesh; this PRD is 2D shell formulation). Hex/wedge is smaller scope and ships sooner; shells is the proper fix.
- **Depends on solid linear-static** — sibling, not predecessor; can ship in parallel logically but architecturally needs the kernel basics first.
- **Composes with `multi-load-case-fea.md`** — shells participate in multi-load workflows the same way solids do.
- **Composes with `fea-gui-rendering.md`** — shells render differently (mid-surface display + thickness extrusion view); GUI work needs to support both representations.
