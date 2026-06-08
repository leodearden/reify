# PRD (forward-stub): PointCloud import (`PointCloudInput`)

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub · **Date:** 2026-06-03
**Parent:** `io-export-import-completion.md` §8 (deferred row 1). **Tracker:** task ι.

## Why deferred

Closes the `std.io.formats` import gaps **`PointCloudInput`** + **`PointCloudFormat`** + the
documented point-cloud geometry path (gap-register P15 `io-step-import-missing`, point-cloud half).
Deferred from the export/import-completion PRD because it needs a **brand-new value type** and
**four file readers**, orthogonal to the B-rep export/STEP-import work that PRD delivers.

## Substrate gap (verified 2026-06-03)

- **No `PointCloud` value type** anywhere (`reify-ir/src/value.rs`, `reify-compiler/src/types.rs`,
  eval). It must be added end-to-end (value variant, type-resolver name, sampleable surface).
- **No point-cloud readers** (PLY / PCD / XYZ / LAS). Each format needs a parser (vendored crate or
  hand-rolled).
- The parent PRD's `step_import` establishes the **geometry-import eval seam**; point-cloud import
  reuses that seam shape but produces a `PointCloud`, not a B-rep `Geometry`.

## Sketch (when activated)

1. Add `PointCloud` value type + `Type` resolution; decide its downstream surface (sampleable? a
   geometry kind for meshing/registration? a bare point list?).
2. `enum PointCloudFormat { PLY, PCD, XYZ, LAS }` + `occurrence def PointCloudInput : Input`
   (declared with concrete defaults, mirroring `STEPInput` — no `= undef`).
3. `point_cloud_import(path, format) -> PointCloud` builtin over the parent's geometry-import seam.
4. Reader per format; `XYZ` (plain ASCII) is the minimal first slice.

## Pre-conditions for activating

- Parent `io-export-import-completion.md` landed (geometry-import seam + `step_import` precedent).
- A **named downstream consumer** for `PointCloud` (meshing, registration, or display) — without it
  this is a producer-orphan (G1). Identify before activating.

## Decomposition (when activated — not filed now)

α PointCloud value type + resolver · β `PointCloudFormat`/`PointCloudInput` surface · γ XYZ reader
(vertical slice) · δ PLY/PCD/LAS readers · ε downstream consumer wiring.
