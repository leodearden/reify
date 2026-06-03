# PRD (forward stub / DEFERRED): `std.process` geometry-metrology DFM engine

**Status:** DEFERRED forward-design stub · **Author session:** 2026-06-03 · **Milestone:** v0_6+ (post `process-dfm-completion`)
**Relationship:** the substrate-heavy second half of `docs/prds/v0_6/process-dfm-completion.md` §0/§5. That PRD ships the *declarative* DFM surface; **this** PRD is the *geometry-metrology* engine that auto-measures features from the realized solid. Held by deferred tracker tasks **MET-1 / MET-2** (filed `deferred`, not activated).

> This is a **stub**, not a ready-to-decompose PRD. It records scope, the substrate gap, and the
> known hazards so the excluded work is tracked rather than dropped. It must be promoted to a full
> B+H PRD (re-walk all gates, verify each new kernel query's feasibility, set numeric floors) before
> the trackers are activated. Do **not** decompose it as-is.

---

## §0 — Why this is deferred (the substrate gap)

`process-dfm-completion.md` makes DFM rules check **design-declared / user-supplied** measured values
and one geometry-backed rule (`fits_build_volume`) built on the **existing** `BoundingBox` query.
The metrology engine instead **auto-extracts** the measured features from the realized subject solid.
That requires kernel measurement queries that **do not exist today**:

| Measurement | Process category | Kernel reachability | Hazard |
|---|---|---|---|
| **Overhang angle** vs build direction | `Adding` (AM self-support) | Facet/face-normal scan vs build vector — `GeometryQuery::FaceNormal` exists **per BRep face**, but a curved overhang needs **mesh-facet** sampling; new query | Mesh-vs-BRep face granularity; build-direction convention |
| **Draft angle** vs pull direction | `Forming` (molding/casting) | Same shape as overhang (normal vs pull axis); new query | Pull-axis convention; undercut detection |
| **Min wall thickness** | `Subtracting`/`Adding`/`Parting` | **Research-grade**: medial-axis transform or ray-cast thickness sampling; no kernel primitive | Medial-axis is numerically delicate; sampled ray-cast is a **lower bound** (G6 numeric — assert a bound, never exactness) |
| **Min feature size** | several | Thin-region / small-face detection; new query | Definition ambiguity (edge vs face vs gap) |

Min-wall-thickness in particular is a hard geometry problem — the same class of "the bound the
formulation can actually reach" trap catalogued for FEA (memory
`reference_fea_accuracy_formulation_floor_survey`). It must not be specced with a guessed numeric
floor frozen into a RED test (the G6 esc-3453/esc-3770 failure mode).

---

## §1 — Consumer (G1, provisional)

A DFM-minded designer who wants `reify check` to flag manufacturability violations **without**
manually declaring every measured feature — the engine realizes the subject solid and measures it.
`DFMRule.subject : Structure` (decorative in `process-dfm-completion`) becomes load-bearing: the
engine reads the subject's realized geometry and runs every rule whose `applies_to` process matches.

---

## §2 — Sketch (provisional, to be hardened on promotion)

1. **New `GeometryQuery` variants** in `reify-ir/src/geometry.rs` + OCCT/mesh backing:
   `OverhangFaces { build_dir, max_angle }`, `DraftAngle { pull_dir }`, `MinWallThickness`,
   `MinFeatureSize`. Each needs an `engine-integration-norm.md` §3 seam (likely §3.7
   KernelAttributeHook or a new measurement seam — resolve at promotion).
2. **Auto-measurement DFM pass** (`reify-eval`): on `reify check`, for each `DFMRule`, realize
   `subject`, run the matching measurement query, compare against `applies_to`'s capability param,
   emit a `DFMSeverity`-tagged diagnostic via the **existing** `dfm::diagnose` from
   `process-dfm-completion` (reuse, don't re-author the severity bridge).
3. **`DFMRule.subject : Structure` activation** — register/resolve `subject`, wire it to the
   realization + measurement path.

---

## §3 — Cross-PRD seam ownership (G4)

| Seam | Owner |
|---|---|
| Declarative DFM surface (capability params, `Manufacturable`, `DFMRule`, `dfm::diagnose` severity bridge) | `process-dfm-completion.md` (**upstream** — this PRD consumes it) |
| New `GeometryQuery` measurement variants + measurement pass | **this PRD** (on promotion) |
| `Geometry` type registration | `tolerancing-gdt-surface-completion.md` #3116 (a `subject : Structure` / geometry-typed feature may depend on it — verify at promotion) |

---

## §4 — Deferred tracker tasks

- **MET-1** — Kernel geometry-metrology queries for DFM (overhang / draft / min-wall / min-feature).
  `deferred`; B+H; depends on `process-dfm-completion` δ.
- **MET-2** — Auto-measured DFM evaluation pass (realize subject → measure → run rules → diagnostics;
  activate `DFMRule.subject : Structure`). `deferred`; depends on MET-1 + `process-dfm-completion` δ.

## §5 — Promotion checklist (before activating MET-1/MET-2)

- [ ] Verify each new `GeometryQuery` is implementable on the OCCT/mesh substrate (prototype the
      hardest — min-wall — first; it may need a medial-axis or sampled ray-cast approach).
- [ ] Set an **honest numeric floor** for min-wall / overhang accuracy (sampled = lower bound; no
      exactness claim) — G6.
- [ ] Pick the `engine-integration-norm.md` §3 seam for the measurement pass (or author a norm
      extension).
- [ ] Re-walk G1–G6 + META; write the capability manifest.
