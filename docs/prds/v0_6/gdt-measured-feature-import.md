# PRD stub — GD&T measured-feature import (DEFERRED)

**Status:** deferred stub — authored 2026-06-10 as the bookmark half of
`gdt-geometric-zones-and-containment.md` §6. Do **not** decompose; a deferred bookmark task
points here. Promote to a full B+H PRD only when the activation gate below is met.

## Goal (when activated)

Feed Reify's geometric conformance check (`Conforms(tolerance: t, actual: <measured>)`) with
**externally measured geometry** — CMM point sets, scan meshes, or per-feature deviation
tables — so conformance verdicts reflect a real manufactured part instead of a synthetic
deviation. This is the third and final feeder of the `actual` seam (after virtual-condition
design-side geometry and synthetic deviation, both shipped by the parent PRD).

## Named consumer (G1)

`measure_gdt_conformance` / `Conforms.actual` — parent PRD task η (contract C3/C5 in
`gdt-geometric-zones-and-containment.md` §8). The consumer seam is fully specified there; this
PRD's deliverable is the producer: measured data → a `Geometry`-typed (or sampled-field-typed)
actual feature, registered + aligned to the nominal frame.

## Substrate (G3, known today)

- **#4290 (deferred forward-stub):** PointCloud type + PLY/PCD/XYZ/LAS readers — explicitly
  waiting for a named consumer; THIS PRD is that consumer when promoted. Activate 4290, don't
  re-file readers.
- **#4289 (pending):** STEP import — the first B-rep import seam; a scanned/CMM-fitted B-rep
  would ride it.
- VDB scalar-field import is live end-to-end (`FieldSource::Imported` →
  `read_vdb_file` → `SampledField`, `engine_eval.rs:1154-1165`) — a pre-fitted SDF deviation
  field is a viable interim carrier.
- `STEPInput` occurrence (io.ri:169-173) is a declarative tolerance-promise stub, not a reader.

## Hard design problems to resolve at promotion (why this is deferred, not pending)

1. **Registration/alignment**: measured data lives in machine coordinates; containment needs
   it in the nominal frame (best-fit vs datum-simulator alignment per ISO 5459 — design fork).
2. **Feature segmentation**: mapping measured points/mesh regions to the callout's `feature`.
3. **Representation choice**: point-set vs fitted B-rep vs SDF — interacts with
   `MaxDeviation`'s B-rep gating and the 4421–4427 SDF wire.

These need a design session with survey input (metrology formats, QIF) — a human gate, hence
deferred.

## Activation gate

Parent PRD η+θ landed (the consumer seam exists and is CI-gated) AND a real measured-data
source/workflow is in hand to validate against. At promotion: full B+H authoring pass through
the /prd gates; activate #4290; decide the alignment fork with Leo.
