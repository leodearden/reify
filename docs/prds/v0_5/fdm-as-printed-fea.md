# PRD: FEA on the As-Printed FDM Structure (umbrella + Rung-0 vertical slice)

Status: **active umbrella** — v0.5. Authored 2026-05-26 via `/prd` (G1–G5+META). The Rung-0 vertical slice is decompose-ready (B+H); higher rungs are deferred to sibling stub PRDs (R1/R2/R3).

## Goal

Let a user run structural FEA on a part that reflects how it is **actually printed** — anisotropic walls + infill core, weak inter-layer (build-Z) bonding — rather than the homogeneous solid that was designed. The headline user-observable result: the same bracket, analysed as-printed, deflects more and concentrates stress differently than the homogeneous-solid analysis, and the weak build direction shows up. The fidelity of the as-printed model **refines automatically over time** (cheap estimate first, sharper physics as compute allows); the user does not pick a fidelity mode.

Motivating end-to-end (Rung-0, decompose-ready):

```
let proc = FDMProcess(
    build_direction: vec3(0mm, 0mm, 1mm),
    layer_height: 0.2mm,
    walls: 3,
    top_bottom_layers: 4,
    infill_density: 0.2,
    infill_pattern: Gyroid,
    material: ABS_Plastic()
)

let printed = as_printed_material(bracket, proc)              // Field<Point3, AnisotropicMaterial>
let r_print = solve_elastic_static(bracket, printed, loads, supports)
let r_solid = solve_elastic_static(bracket, ABS_Plastic(), loads, supports)
// r_print.max_deflection > r_solid.max_deflection; build-Z stress visibly weaker
```

`as_printed_material(...)` consumes the shared constitutive foundation (`anisotropic-heterogeneous-elastostatics.md`) — it produces the `Field<Point3<Length>, AnisotropicMaterial>` that foundation's generalised `solve_elastic_static` already accepts. This PRD owns the *print-structure model* that fills that field; the foundation owns the *solver*.

## Background

Reify has a shipped linear-elastostatic FEA stack (v0.3, `reify-solver-elastic`), tet meshing (`reify-kernel-gmsh`), a material library (`materials_fea.ri`), and ComputeNode warm-start. It has **no** notion of 3D-printing: no slicing, infill, shells/perimeters, layers, print orientation, or process-induced anisotropy (confirmed 2026-05-26 — no prior AM PRD, no AM gap-register entry). `reify-gcode` parses G-code into a low-level command AST (`LinearMove`/`ArcMove`/…) — not a structured toolpath with extrusion roles/widths/layers.

"Occurrence" is established Reify vocabulary (`reify-language-spec.md` §2.8/§4.1.2): a first-class entity kind that *transforms structures* (consumes input structures, produces output structures, composes via `connect`/`chain` on ports). The user's framing — an `FDMSlice` occurrence and an `FDMPrint` occurrence — is idiomatic; each dispatches internally to a ComputeNode (engine-integration-norm §3.4), which the ComputeNode contract explicitly supports for **subprocess-wrapped** work (cancellation via SIGTERM/SIGKILL, warm-state, cost metadata).

### The gap is three stacked layers

1. **Anisotropic + spatially-varying constitutive law** — owned by the shared foundation PRD (prerequisite). Not re-derived here.
2. **A print-structure model** — given a designed solid + print parameters, produce the spatially-varying `AnisotropicMaterial` field (wall/skin/infill zones; build-Z-weakened, infill-density-knocked-down stiffness, oriented by the print frame). **New; this PRD.**
3. **A toolpath representation + slicer integration** — to drive higher-fidelity print models from the *actual* deposited geometry. **New; this PRD (the `FDMSlice` occurrence + structured `Toolpath` type).**

## The fidelity ladder is a progressive ComputeNode, not a user mode

The endpoint is a high-fidelity simulated mesostructure (voids, bridge-sag, inter-filament/inter-layer bonds, weld strength, intra-filament anisotropy from short-carbon-fibre or core/skin filaments, multi-material/multi-nozzle). That is research-grade and decomposes into a fidelity ladder (per the 2026-05-26 prior-art research; no off-the-shelf OSS engine exists, all models are published equations):

| Rung | What it computes | Cost | PRD |
|---|---|---|---|
| **R-fast** | Geometric zone classification (distance-to-surface → wall/skin/infill) + transverse-isotropic build-Z knockdown + Gibson-Ashby infill. **No slicer.** | analytic | **this PRD (slice 1)** |
| **R0** | Real toolpath (FDMSlice) → bead graph → closed-form orthotropic (Rodríguez 2003) + Halpin-Tsai fibre + lumped-cooling Z-knockdown | analytic | **this PRD (slice 2)** |
| **R1** | Explicit bead/void geometry + Mori-Tanaka/Advani-Tucker orientation + 1-D Coogan thermal + Bellehumeur sintering bond model | analytic + ODE | `fdm-print-mesostructure-r1.md` (stub) |
| **R2** | Numerical RVE periodic-BC homogenisation → full anisotropic tensor | small offline FE | `fdm-print-rve-homogenization-r2.md` (stub) |
| **R3** | Transient thermal field, bridge-sag viscoelastic, flow-resolved fibre orientation, dissimilar-material welding | large FE | `fdm-print-thermal-bondsag-r3.md` (stub) |

**The unifying mechanism (resolved design decision).** `as_printed_material(body, process, options) -> Field<Point3<Length>, AnisotropicMaterial>` is a **progressive ComputeNode** (engine-integration-norm §3.4; ComputeNode `progressive` trait). It emits the cheapest achievable rung immediately and refines upward:

- `Freshness::Pending { last_substantive }` surfaces the current-best rung while higher rungs compute.
- The scheduler (NodeTraits priority) runs viable rungs concurrently when compute allows and serialises under resource pressure — fastest first, slower later. (This is Reify's existing evaluation-task-scheduler behaviour; R1–R3 wire into the *same* node.)
- The downstream FEA node depends on the field; as it refines, FEA **re-solves warm-started** (the foundation PRD's field-arg design keeps the FEA mesh fixed so the prior displacement iterate carries across fidelity bumps). The user watches the result sharpen.
- `options.target_fidelity` caps the ladder; `#deterministic` pins to a single rung for reproducibility (mirrors the v0.3 FEA `#deterministic`-pins-threads pattern).

The explicit `FDMSlice` / `FDMPrint` occurrences remain first-class (compose via `connect`/`chain`) for users who want the toolpath itself or to pin a rung; `as_printed_material` is the convenience node that wraps and schedules them.

```
occurrence def FDMSlice {        // Body + settings → Toolpath  (PrusaSlicer subprocess ComputeNode)
    param process : FDMProcess
    port body : in StructurePort
    port toolpath : out ToolpathPort
}
occurrence def FDMPrint {        // Toolpath → as-printed AnisotropicMaterial field  (progressive R0→R3)
    param process : FDMProcess
    port toolpath : in ToolpathPort
    port printed : out StructurePort
}
```

## Slicer integration: PrusaSlicer, subprocess only

Per the 2026-05-26 AGPL/slicer research:

- **Engine: PrusaSlicer / libslic3r** — most mature, richest structured-extrusion model, broadest settings, non-litigious upstream. (Avoid OrcaSlicer/Bambu — active license litigation; CuraEngine is also AGPL despite the LGPL Cura GUI.)
- **Integration: subprocess from a ComputeNode — never FFI-linked.** Reify is AGPL-3.0, so an AGPL slicer is *compatible*, but **FFI-linking libslic3r would weld Reify's entire license to the slicer's AGPL permanently** (kills future relicensing, pulls in §13 network clause, adds a heavyweight C++ build atop the existing OCCT pain). Subprocess invocation is "mere aggregation" — a clean boundary that keeps Reify's own license unwelded.
- **Recover per-bead geometry by parsing G-code comments** (`;TYPE:External perimeter`/`;TYPE:Internal infill`/`;TYPE:Bridge infill`, `;WIDTH:`, `;HEIGHT:`, layer-Z markers) — no FFI needed; this is what PrusaSlicer's own preview does.
- **Determinism: verify-and-lock** — pin the PrusaSlicer version + full settings profile, add a golden-slice CI test (no slicer guarantees determinism; parallel infill generation is a run-to-run risk). `#deterministic` pins the rung.
- **No incremental reslice exists** in any slicer, so `FDMSlice` warm-state is just full-reslice-with-cache (cache keyed on body realization hash + settings).
- **Distribution:** prefer locating a user-installed PrusaSlicer on `PATH`; if bundled, ship the AGPL source offer.

## The `Toolpath` representation (the FDMSlice → FDMPrint seam)

`reify-gcode`'s command AST is too low-level to drive a print model. This PRD owns a structured `Toolpath` value: an ordered, layer-segmented bead graph where each bead carries `{centerline polyline, width, height, role ∈ {perimeter, solid_infill, sparse_infill, bridge, support}, layer_index, layer_z, nominal_temp, speed}`, plus in-layer and inter-layer adjacency. `reify-gcode` stays the low-level parser; the FDMSlice G-code-comment parser builds the `Toolpath` on top of it (ownership: this PRD owns `Toolpath`; `reify-gcode` is unchanged).

## Built-in property correlations + coupon override (the physics source)

Per the 2026-05-26 effective-property research, with provenance (mirrors `materials_fea.ri`'s per-property `MaterialPropertyProvenance` convention):

- **Build-Z knockdown** (the dominant FDM anisotropy): default `E_z/E_xy ≈ 0.67`, `σ_z/σ_xy ≈ 0.52` (PLA-calibrated, [PMC9828590]); milder Nylon-like values when fusion is known-good.
- **Infill density → modulus**: Gibson-Ashby `E_eff/E_solid = C·ρ^n`, default `C=1, n=2` (bending-dominated; [Gibson & Ashby 1997]).
- **Pattern factors**: gyroid/cubic treated as near-isotropic; grid/triangular/honeycomb get directional factors.
- **Transverse isotropy is the default model** (5 constants); orthotropy (9) for known-unidirectional raster. ±45 alternating raster ⇒ in-plane isotropy is the standard assumption.

Defaults are calibratable: a user supplies measured coupon data (`Ex,Ey,Ez,Gxy,…`, infill curve) to override any constant. Each default carries a source citation; low-confidence defaults (PC anisotropy; FDM-specific Gibson-Ashby exponents; CF chopped-vs-continuous) are flagged in the library.

## Pre-conditions for activating

- **`anisotropic-heterogeneous-elastostatics.md` shipped** — provides `AnisotropicMaterial`, the `Field<Point3, AnisotropicMaterial>` solver argument, per-element assembly, warm-start-across-refinement. **Hard prerequisite.**
- v0.3 FEA stack + ComputeNode integration. **Met.**
- GR-001 struct-ctor runtime (for `FDMProcess(...)`). **Met** (SIR-α 3540).
- For slice 2 only: PrusaSlicer available on `PATH` in dev + CI. (Slice 1 has no slicer dependency.)

## Resolved design decisions (2026-05-26)

1. **Fidelity ladder = one progressive ComputeNode**, scheduler-managed, auto-refining; not a user-selected mode. R1–R3 wire into the same `as_printed_material` node.
2. **Implementation order: R-fast (zones) first, prove e2e, then R0 (slicer), then R1–R3.** Slice 1 ships value with zero external dependency; slice 2 validates the slicer-subprocess + toolpath seam.
3. **Material reaches the solver as a `Field` argument** (owned by the foundation PRD) — chosen over a realization-kind to preserve warm-start across refinement.
4. **PrusaSlicer subprocess, parse G-code comments, no FFI** (AGPL boundary + build hygiene).
5. **`Toolpath` is a new structured type owned here**; `reify-gcode` stays the low-level parser beneath it.
6. **FDM-only for now.** SLS/resin slice+print variants are explicit future work (they largely collapse to the existing near-isotropic path).
7. **No method-call syntax** anywhere in surface examples (GR-040: `a.b.foo()` does not parse) — predicates/queries use free-function form.

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_5/anisotropic-heterogeneous-elastostatics.md` | consumes | `Field<Point3, AnisotropicMaterial>` solver arg; `AnisotropicMaterial`/`ConstitutiveLaw` types | **foundation owns**; this PRD produces the field | hard prereq |
| `v0_3/structural-analysis-fea.md` | consumes | `solve_elastic_static` (generalised by foundation) | FEA / foundation | wired |
| `engine-integration-norm.md` §3.4 | consumes seam | `FDMSlice`/`FDMPrint`/`as_printed_material` are §3.4 ComputeNode consumers (FDMSlice = subprocess) | this PRD | queued |
| `reify-gcode` (crate) | builds on | G-code comment parse → `Toolpath` | this PRD owns `Toolpath`; reify-gcode unchanged | queued |
| `v0_5/fdm-print-mesostructure-r1.md` (stub) | produces (R1 rung) | refinement rung of `as_printed_material` | R1 owns physics; this PRD owns the progressive mechanism | future |
| `v0_5/fdm-print-rve-homogenization-r2.md` (stub) | produces (R2 rung) | RVE-homogenised tensor field | R2 owns; this PRD owns mechanism | future |
| `v0_5/fdm-print-thermal-bondsag-r3.md` (stub) | produces (R3 rung) | transient thermal / bridge-sag / flow fibre | R3 owns; this PRD owns mechanism | future |

No new engine seam is introduced (all consumers plug into §3.4). No reciprocal ownership ambiguity.

## Contract (the seams — approach H)

**C1 — `as_printed_material(body, process, options) -> Field<Point3<Length>, AnisotropicMaterial>`.** A progressive ComputeNode. Invariants: emits a usable field at the cheapest achievable rung within `≤` the FEA significance-budget latency; each higher rung is a *refinement* of the same field (same domain); `target_fidelity` bounds the rung; `#deterministic` pins exactly one rung and forces deterministic execution (incl. pinned slicer settings). Cache-keyed on `(body realization hash, process value-hash, options, rung)`.

**C2 — `Toolpath` value + `FDMSlice` subprocess ComputeNode.** `FDMSlice` invokes PrusaSlicer as a subprocess (cancellation: SIGTERM→SIGKILL on input retick; warm-state: none beyond cache; cost: measured wall-clock + serialized toolpath size per the ComputeNode `cost_per_byte` contract). Output `Toolpath` parsed from G-code comments. Invariant: deterministic given pinned version+settings (golden-test-locked); bead roles/widths/layer-Z fully populated.

**C3 — `FDMPrint` rung-R0 mapping `Toolpath -> Field<Point3, AnisotropicMaterial>`.** Per-region constitutive value from the correlation library; orientation `frame` aligned to build-Z + local bead direction. Invariant: build-Z is the weakest axis; constant-input ⇒ constant field; total over the body.

**C4 — R-fast geometric zone mapping `(body, process) -> Field<Point3, AnisotropicMaterial>`.** Distance-to-surface classification (wall: within `walls × line_width`; skin: within `top_bottom_layers × layer_height` of a top/bottom face; else infill) + transverse-isotropic knockdown + Gibson-Ashby infill. No slicer. Invariant: same output type as C3, so the progressive node treats it as the bottom rung.

## Boundary-test sketch (approach H)

| Scenario | Side | Precondition | Postcondition |
|---|---|---|---|
| Zones field vs homogeneous | producer (R-fast) | bracket + `FDMProcess` | wall zone ≈ bulk stiffness; infill zone knocked down by `ρ^n`; build-Z `E` < in-plane `E` |
| Zones e2e Δ | consumer | as-printed vs homogeneous solve, same loads | as-printed `max_deflection` strictly greater; build-Z stress visibly weaker (asserted) |
| Slicer determinism | producer (R0) | fixed body + pinned PrusaSlicer + settings | two slices byte-identical `Toolpath` (golden) |
| Toolpath roles | producer (R0) | sliced bracket | every bead has a non-default role + width + layer-Z; perimeter/infill/skin counts > 0 |
| FDMSlice cancellation | producer (R0) | rapid body retick mid-slice | slicer subprocess killed; no orphan process; prior cache intact |
| Progressive refinement | integration gate | `as_printed_material` with `target_fidelity = R0`, slicer present | field starts at R-fast, refines to R0; FEA result updates; FEA warm-starts (CG iters drop on the refine) |
| `#deterministic` pin | consumer | pinned rung | repeated runs bit-stable; pins one rung only |

The "Zones e2e Δ" and "Progressive refinement" rows are the G2 integration-gate signals.

## Decomposition plan

B+H. **Slice 1 (R-fast, no slicer) ships first**; slice 2 (R0, slicer) second; R1–R3 are sibling stub PRDs.

**Slice 1 — R-fast zones path (decompose-ready):**

- **α — `FDMProcess` struct + `InfillPattern` enum (stdlib).** Named-arg constructible; per-property provenance scaffolding. *Crates:* reify-compiler stdlib. *Signal (intermediate):* `FDMProcess(...)` evaluates to a `StructureInstance`; unlocks β/γ/δ.
- **β — Built-in correlation library (stdlib + reify-solver-elastic helper).** Build-Z knockdown, Gibson-Ashby infill, pattern factors, transverse-iso defaults — each with source citation; coupon-override entry points. *Signal (intermediate):* unit tests assert each default + provenance; unlocks δ.
- **γ — Geometric zone classifier (`reify-solver-elastic` or new `reify-fdm` crate).** Distance-to-surface → wall/skin/infill per `FDMProcess`. *Signal (intermediate):* classifier test on a box assigns expected zones; unlocks δ.
- **δ — `as_printed_material` R-fast ComputeNode → `Field<Point3, AnisotropicMaterial>`.** Wires γ+β into the foundation's field type; registered §3.4. *Signal (intermediate):* returns a non-constant field on a walled+infilled box; unlocks ε.
- **ε — Integration gate: zones e2e Δ.** `examples/fdm_bracket.ri` solving as-printed vs homogeneous; assert Δ. *Crates:* examples, reify-eval tests. *Signal (leaf, user-observable):* `reify eval examples/fdm_bracket.ri` prints `max_deflection` strictly greater for as-printed; golden committed; GUI zone visualization optional follow-up.

**Slice 2 — R0 slicer path (decompose-ready after slice 1):**

- **ζ — `Toolpath` value type + G-code-comment parser** (on `reify-gcode`). *Signal (intermediate):* parses a PrusaSlicer G-code fixture into roled/widthed/layered beads; unlocks η/θ.
- **η — `FDMSlice` subprocess ComputeNode** (PrusaSlicer invocation, settings-profile composition, cancellation, golden determinism test, PATH discovery). *Signal (leaf, user-observable):* `FDMSlice` on a body emits a `Toolpath`; `reify` surfaces a determinism-locked golden; cancellation test green.
- **θ — `FDMPrint` R0 mapping `Toolpath -> Field` + register as the R0 rung of `as_printed_material`.** Rodríguez closed-form orthotropic + Halpin-Tsai fibre + lumped-cooling Z-knockdown; bead-direction frame. *Signal (intermediate):* R0 field differs from R-fast on a real toolpath; unlocks ι.
- **ι — Integration gate: progressive refinement + warm-start.** R-fast→R0 refinement drives an FEA re-solve that warm-starts. *Signal (leaf):* progressive + warm-start boundary tests green.

**Companion phase:**

- **κ — Cross-PRD prose + dep edges.** Confirm foundation dep edge; ensure R1/R2/R3 stubs reference this PRD's progressive mechanism as their integration point. *Signal (leaf):* dep edges wired; stubs cross-linked.

Dependencies: α→{β,γ,δ}; {β,γ}→δ→ε (slice 1). ε→ζ→{η,θ}; θ→ι (slice 2). Foundation PRD is a hard prereq of δ and θ. κ independent (docs).

## Out of scope for this PRD

- R1–R3 physics (mesostructure, RVE, thermal/bond/bridge-sag) — sibling stub PRDs.
- SLS / MJF / resin / metal-AM processes — future; FDM only here.
- Anisotropic *strength/failure* prediction (weld-strength-driven failure) — R1+ and a future strength PRD; this PRD is elastic-stiffness only.
- Lattice/gyroid *explicit geometry* generation — Reify has none today (conceptual only); R2/R3 may need unit-cell geometry but full-part lattice meshing is out.
- Multi-material / multi-nozzle — R1+ (the `Toolpath` carries enough to extend later).
- GUI zone/contour visualization — optional follow-up; headless path lands first (mirrors the FEA PRD's headless-first decision).

## Open questions (tactical — decide at impl time)

1. **`reify-fdm` crate vs folding into `reify-solver-elastic`.** Zone classifier + correlation library + Toolpath could live in a new `reify-fdm` crate or extend the solver crate. **Suggested:** new `reify-fdm` crate (keeps the solver crate constitutive-only; FDM depends on it). Decide at task γ.
2. **Top/bottom-face detection for skin zones.** Build-Z + face-normal threshold vs slicer-reported skin (R0). **Suggested:** normal-vs-build-Z threshold for R-fast; real skin regions at R0. Decide at task γ.
3. **`#deterministic` default rung.** Pin to the lowest rung (fast, reproducible) or the highest achievable. **Suggested:** lowest, with explicit `target_fidelity` to raise. Decide at task δ.
4. **PrusaSlicer absence handling.** When no slicer is on `PATH`, R0+ rungs are simply unavailable and `as_printed_material` tops out at R-fast. **Suggested:** emit a `W_FDM_SLICER_UNAVAILABLE` info diagnostic, not an error. Decide at task η.
5. **Settings profile surface.** How much of PrusaSlicer's settings surface to expose via `FDMProcess` vs an escape-hatch raw-profile import. **Suggested:** the mechanically-relevant subset in `FDMProcess`; raw-profile import as a follow-up. Decide at task η.
