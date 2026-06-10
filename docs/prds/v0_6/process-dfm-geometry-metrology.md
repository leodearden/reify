# PRD (SUPERSEDED → SPLIT): `std.process` geometry-metrology DFM engine

**Status:** SUPERSEDED 2026-06-08 · split into two PRDs after a feasibility sweep · **Milestone:** v0_6

> This deferred stub held **all four** auto-measurement DFM queries (overhang, draft, min-wall,
> min-feature) as one B+H follow-up. A 2026-06-08 three-agent feasibility sweep found the stub
> **overstated the substrate gap** and that the four queries split on a sharp tier boundary. This
> file is kept as a redirect so inbound links (notably `process-dfm-completion.md` §0/§5/§7) still
> resolve. **Do not author or decompose against this file.**

## Split

| Tier | Queries | PRD | Status |
|---|---|---|---|
| **Ship-now** (substrate exists today) | OverhangFaces, DraftAngle + auto-measurement pass + `DFMRule.subject : Solid` | **`process-dfm-overhang-draft.md`** | Ready to decompose (full B+H + capability manifest) |
| **Research-gated** (needs an eval-reachable solid→SDF wire) | MinWallThickness, MinFeatureSize | **`process-dfm-thickness-metrology.md`** | DEFERRED forward stub |

## Why the split

- **Overhang + draft** reuse the existing `faces_by_normal` selector machinery
  (`topology_selectors.rs:583`), `FaceNormal` query, and `tessellate()` — zero new kernel/FFI work.
  The auto-measurement pass is a check-time walk modeled on the proven `RepresentationWithin`
  interception. → ship now.
- **Min-wall + min-feature** depend on an eval-reachable solid→SDF wire that does **not** exist in
  production (the kernel SDF primitive `#3095` is done + on main but stranded from eval — a C-17
  orphan; the shell-extract migration that would have wired it is cancelled), plus a non-convex
  correctness boundary and a `not(has_openvdb)` degradation contract. → research-gated, deferred.

## Substrate corrections recorded during the sweep (carried into the successor PRDs)

- `subject : Structure` → **`subject : Solid`** (`"Structure"` is a purpose-only wildcard sentinel,
  not a registered surface type; a metrology query consumes a geometry handle).
- The `#3116` (`Geometry`-registration) dependency is **spurious** — already landed; `Solid` resolves.
- The new check-time measurement pass needs a **new `engine-integration-norm.md` §3 entry** (owned
  by the overhang-draft PRD); the queries themselves ride the existing §3.1 query path.

## Tracker disposition

Deferred trackers **4276 (MET-1)** / **4277 (MET-2)** are superseded: overhang/draft (MET-1) + the
auto-measurement pass (MET-2) → `process-dfm-overhang-draft.md`; min-wall/min-feature (MET-1) →
`process-dfm-thickness-metrology.md`.
