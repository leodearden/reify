# Printer v0.1 — Multi-Material Toolchanger Build

A high-performance multi-material 3D printer designed and modelled in reify, built as a dogfood test of reify's parametric assembly capabilities and as a hardware testbed for planned Klipper firmware advances.

## Goals & quantified targets

| Dimension | Target | Stretch |
|---|---|---|
| Build envelope (X × Y × Z) | 500 × 800 × 600 mm | — |
| Acceleration (head, X/Y) | 100 m/s² (~10g) | 1000 m/s² (~100g) |
| Chamber temperature | 80°C continuous | 130°C capable |
| Tools (toolchanger) | 4 | 6 |
| Per-tool MMU | 1 (single filament) | 2-4 filaments per tool |
| Budget (parts, all-in) | £4,000 | — |
| Noise | "not too loud" — silent compressor, no belt-tooth tonal | — |
| Operating environment | Climate-controlled shipping container workshop | — |

## Priorities (ordered)

1. **Speed and accuracy.** Low vibration, smooth motion at high speed and acceleration.
2. **Price-performance.**
3. **Quiet.**
4. **Heated enclosure** (chamber heater, full enclosure).
5. **Multi-tool capability** (toolchanger + per-tool MMU).
6. **Klipper firmware testbed** for closed-loop FOC servo, snap-limited trajectory planning, ML-tuned predictive motion compensation.

## Architectural decisions (with rationale)

### Kinematics: CoreXY, toolchanger, Z-bed

CoreXY chosen over alternatives:
- **Cartesian / bed-slinger**: ruled out — moving heated bed limits acceleration and chamber sealing.
- **Hbot**: torsional load on gantry from offset belt force.
- **Delta**: build-envelope vs. footprint trade-off poor at 500×800; multi-material toolchanger awkward.
- **CoreXY toolchanger** (E3D Motion System / Jubilee / Prusa XL pattern): standard solution for multi-tool builds; large stationary build volume; gantry mass minimisable.

Z-bed (vs. gantry-Z): gantry kinematics decoupled from Z mass; chamber volume bounded by stationary walls; heated bed remains thermally coupled to insulated chamber bottom. Bed parks just below the build zone behind an insulated shutter for cool-down / part removal; no bellows around moving bed.

### Drive: Vectran tendons + capstans

6mm Vectran braided rope on capstan drives, in preference to GT2/AT3 toothed belts.

| Property | Vectran capstan | Toothed belt |
|---|---|---|
| Specific stiffness | Higher | Lower |
| Creep | Negligible | Measurable (especially at 80-130°C) |
| Tooth-passing tonal vibration | None | Present (audible + structural) |
| CTE | Near-zero | Glass-fibre core helps but non-zero |
| Continuous service temp | ~150°C | Glass/steel cord ~120°C, polyurethane matrix limits below this |
| Cost | £5-15/m for 6mm braid | Cheaper |
| Service / replacement | Hours (re-wind capstans) | Minutes |

Vectran is the right rope for a heated chamber: Dyneema softens at ~70°C, Kevlar abrades poorly, steel cable rings and fatigues.

Drive is by **anchored (winch) capstan, not friction wrap**: both rope ends are fixed to a ~30mm grooved drum, so the coupling is positive — zero slip. A friction capstan would accumulate micro-slip under the rapid, unbalanced reversals and resonant excitation of high-acceleration printing (unacceptable for positional accuracy), and the relative rope/drum creep would shorten tendon life — both rule it out. The drum reels rope onto one side as it pays out the other; its axial length is sized for the full per-axis feed (~1.3m → ~14 turns at 6mm pitch) plus the base wraps, and it is helically grooved so the wrap lay is deterministic. Because the wrap band migrates axially (~one pitch/turn, ~80mm over full travel), each strand runs through a **passive translating fairlead** that tracks the band and holds the fleet angle ≈0 — a guide, not a tensioner, so it adds no compliance. Larger drum/idler D/d trades footprint for rope bend-fatigue life (a tendon-life lever).

Pre-tension is **set-and-locked at assembly** on a rigid adjustable anchor — deliberately *not* a compliant/constant-force idler. A sprung tensioner sits in series with the drive and collapses its stiffness: a pre-tensioned rope drive's stiffness is the *sum* of its two antagonistic strands (k₁+k₂), and a zero-rate device on either strand removes that strand's contribution. Locking the preload rigid keeps the full rope stiffness. The thermal length mismatch (steel / epoxy-granite structure vs. near-zero-CTE Vectran — ~1mm over a ~1m span × 60°C, ≈ a few hundred N at the rope's ~300–700 N/mm) is covered by preload margin and periodic re-tension, with the load-side linear scales closing the position loop; an active servo-held preload is the fallback if drift proves too large.

**Paired in-plane tendons**: every tendon is doubled and routed in the plane of its rail axis, nulling moments around the bearing axis. Standard high-precision-servo practice.

### Motors: industrial AC servo (existing parts)

On-shelf inventory drives the motor/drive choice:
- **7× Granite Devices Argon** drives (industrial servo, ~1.5kW class, FOC, regen, SimpleMotion fieldbus)
- **5× Granite Devices Ioni-HC** drives (~600W class)
- **4× 240V AC servo motors** matched to Argons

Allocation:
- 2× Argon + 240V servos → CoreXY X/Y
- 2× Argon + 240V servos → 4-corner electronic Z-tilt (or 2-corner with mechanical coupling to other 2)
- 2-3× Ioni-HC + sourced surplus motors → toolchanger lock, MMU drive(s) if servo-precision needed
- Steppers → per-tool extruders (4×)

This supersedes earlier consideration of drone-motor outrunners. Industrial AC servos have lower rotor inertia, integrated absolute encoders (typically 17-20 bit), industrial environmental ratings, and full FOC closed-loop in the drive itself.

### Frame: polymer-concrete-filled steel sandwich

Laser-cut steel skeleton (designed for tab-and-slot self-jigging, minimal welding), filled with heterogeneous-aggregate epoxy granite (mineral cast). Mass damping ratio of polymer concrete is 5-10× steel; aggregate grading scatters acoustic waves of varying wavelength (heterogeneous fill). Estimated total frame mass 250-400 kg. Container workshop has a concrete floor and full-width end doors, so single-piece assembly is feasible — modularity is for assembly access, not transport.

3DP inserts/guides/aligners for assembly fixturing and bracketry. Material selection by location:
- Outside chamber / cool zones: PETG, ABS, PC.
- In-chamber, ≤80°C: ASA, PC, PCABS.
- In-chamber, 130°C upgrade: PEEK, PEI, PEKK-CF only.

### Bearings: HIWIN-first, air bearings on R&D track 2

The bearing interface (mounting bolt pattern, shoulder height, preload geometry) is designed to accept either HIWIN HG/MG-class profile rail cars **or** a DIY self-compensating cylindrical air bearing of equal envelope. Build v0.1 with HIWIN cars; develop air bearings in parallel as track-2 R&D; swap when proven.

**Air bearing design** (track 2):
- Self-compensating well-and-lip topology (Slocum / MIT PERG pattern). Each pad's perimeter channel is fed by a well on the opposite side of the bearing, throttled by a tight lip clearance. When the bearing displaces toward pad A, gap-A closes but the lip feeding pad A opens, raising pad-A pressure → restoring force. Diagonal cross-coupling gives genuine stiffness without discrete orifices.
- 4-6 pads around the circumference. Three controlled height tiers required: pad land (smallest), lip throttle (intermediate, 1-3× pad gap), well/manifold (deep).
- Manufacturing process: laser-cut stencil → soluble resist on rail at pad locations → silicone-mold-cast wax for wells/channels → loaded epoxy (graphite/CNT/CFRP-milled) over wax → CFRP filament-wind hoop wrap → bake/blow/wash out wax + resist → pressurise to 1 MPa, ~5μm radial expansion gives working pad clearance.
- Three-tier height likely needs post-cast lip honing on a precision lathe (cast intentionally tight, finish-grind lip diameter to spec).
- Air supply: existing P4 / 3°C dew point shop air (26 cfm) augmented with a 0.01μm coalescing + activated carbon stage and point-of-use desiccant cartridge to reach ISO 8573-1 class 1-2.
- Sacrificial PEEK / Vespel set-down rings at each end for graceful contact on power-off.

Precision-ground stainless rails for both options (cylindricity carries the design either way).

### Reaction-mass cancellation

Counter-mass on each of X and Y, accelerated equal-and-opposite to the moving carriage, so frame reaction force ≈ 0. Mandatory for the 1000 m/s² stretch (300N peak reaction force into the frame would otherwise produce visible vibration even in a 250-400kg frame). Counter-mass v1 sized to mean tool-loaded carriage mass; ±15% residual imbalance accepted across the 4-tool range. v2 may add variable counter-mass (mass slides on a screw, set per loaded tool).

### Encoders & feedback

- **Motor-side**: integrated absolute encoders on the AC servos (already present; no spec issue at 80°C, watch grade selection for 130°C stretch).
- **Load-side** (recommended for high-accel work): magnetic strip linear scales on each rail. RLS LM10 or equivalent, ~£100/axis with read head. Ground-truth position feedback for input shaping, ML training data, and tendon/capstan compliance compensation. **Defer if budget pressure forces a cut**, but the firmware testbed deliverables (priority #6) need this to be useful.
- **Bed levelling**: load-cell probe (BTT EBB-style or DIY; load cells are well-trodden in Klipper).
- **Resonance characterisation**: standard ADXL345 accelerometers temporarily mounted on toolhead during commissioning.

### Chamber & bed

- Insulated wall panels (steel skin + mineral wool / aerogel core, sealed). Thermal break between chamber-side panels and structural polymer-concrete frame to keep matrix Tg headroom.
- Bed: 12mm Mic-6 cast aluminium tooling plate, 500×800; flexible PEI-coated steel sheet on magnetic substrate.
- Chamber heater: 2-3 kW resistive coil + circulation fan; PID control on Klipper. 80°C target, sized for 130°C capability.
- Bed heater: 2 kW silicone pad bonded to underside of Mic-6.
- Bed parks ~50-100mm below build zone behind a heated insulated shutter for cool-down. No bellows.

### Electronics segregation

Klipper host (Raspberry Pi 5 or x86 SBC) and Granite drives mount **outside** the chamber in a ventilated enclosure. Stepper drivers (extruders) and any sensor breakouts inside chamber as needed; rated for chamber temp.

## Sub-assembly breakdown (reify model structure)

Each is a top-level reify file under `prj/printer_v01/` importing a shared `envelope.ri` with the global parameters (build_x, build_y, build_z, chamber_temp_target, chamber_temp_max, peak_accel, etc.).

| ID | Assembly | Notes |
|---|---|---|
| A | Frame | Polymer-concrete-filled steel sandwich; structural |
| B | Y-axis | Long stationary rails (800mm travel), paired tendons, Y counter-mass |
| C | Gantry + X-axis | CFRP tube X rail (500mm travel), paired tendons, X counter-mass on gantry beam |
| D | Toolhead carriage + dock | 4-tool kinematic mount, dock geometry, electrical/cooling/air connectors |
| E | Z-bed | Mic-6 plate, 4-corner electronic-tilt servo Z, parking shutter |
| F | Chamber | Insulated panels, sealed, heater, circulation, lighting |
| G | Electronics | Drive enclosure, host, breakout, wiring loom |
| H | Air system | Compressor, filtration cascade, dryer, distribution to bearings + chamber pneumatics |
| I | MMU (per tool) | Filament feeders, drying chamber, runout sensors |
| J | Auxiliary | Filament dryer (passive), part cooling fan, sensors |

## Reify dogfood plan

What gets exercised, and where reify is likely to bite:

**In good shape today:**
- Hierarchical assembly with parameter linking (envelope.ri → sub-assemblies)
- OCCT solid modelling for frame solids, polymer-concrete fill bodies, mounted hardware
- Money/dimensional system for mass + cost rollup (recent apr26 batch)
- Linear/circular pattern for fastener arrays, idler arrays
- Tendon routing as splines (visualisation only, no physics)

**Likely gaps to file as reify-side PRDs in `docs/prds/`:**
1. **Sheet-metal flatten / DXF export** — required for laser-cut steel parts. Almost certainly not implemented. Workaround for v0.1: model flat patterns directly in 2D, validate fold by re-folding in viewer for assembly check, export 2D directly to DXF. File a proper sheet-metal-flatten PRD as follow-up.
2. **BOM extraction** — need to investigate current state. Money-aware mass/cost rollup is the foundation; a `bom.ri` aggregator function over the assembly tree may already work. Confirm and gap-fill as needed.
3. **Kinematic constraint primitives** — would be valuable for verifying counter-mass synchronicity, toolchanger dock pickup paths, Z-tilt range. Not reify's current strength. Workaround: static representations + numerical hand-checks; defer kinematic constraints as a future reify feature.
4. **CFRP tube standard library element** — minor. Hollow round tube extrusion with standard OD/ID. Probably trivial.

**Reify modeling conventions for this project:**
- Top-level: `prj/printer_v01/envelope.ri` declares all global parameters and named planes/axes (kinematic centerlines).
- Each sub-assembly imports envelope; adds local parameters.
- Units: mm-based (reify default).
- Materials: declared via Money-aware mass/density attributes for rollup.
- Naming: `<assembly_id>_<part_name>` (e.g., `A_frame_lower_panel`, `B_y_rail_left`).

## Phased build plan

| Phase | Deliverable | Reify involvement | Hardware? |
|---|---|---|---|
| 0 | Top-level envelope, frame architecture, kinematic centerlines, BOM rough estimate, design-review checkpoint | Yes (model) | No |
| 1 | Frame skeleton laser-cut, welded, polymer-concrete poured, levelled in container | Yes (DXF export) | Yes |
| 2 | Y-axis: rails mounted (HIWIN), tendons routed, capstan + Y motor, drive commissioned at low accel (10-20 m/s²), counter-mass installed | Yes (geometry) | Yes |
| 3 | Gantry + X-axis: CFRP rail, carriage with placeholder mass, tendons + capstan + X motor, CoreXY commissioning at low accel | Yes | Yes |
| 4 | Z-bed: Mic-6 plate, heaters, ballscrew/threaded column drive, 4-corner electronic level | Yes | Yes |
| 5 | Toolhead + dock: dock geometry on frame, 2 tools for v1 (Rapido HF or equivalent), kinematic mount, electrical/cooling, tool change cycle test | Yes | Yes |
| 6 | Chamber + heater: insulated panels, parking shutter, heater + control, commission at 80°C | Yes | Yes |
| 7 | MMU + extruder: per-tool filament feeders, drying | Yes | Yes |
| 8 | Calibration + first prints: bed levelling, resonance characterisation, input shaping baseline, ABS at 80°C | No | Yes |
| 9 | Performance push: tune to 100 m/s² target with input shaping + counter-mass tuning; install linear scales if not already | No | Yes |
| 10+ | R&D tracks (concurrent, see below) | Mixed | Mixed |

## Track-2 R&D items (concurrent with main build)

Self-contained projects that don't gate v0.1:

1. **Klipper-SimpleMotion bridge** (firmware, ~2-4 weeks). Trajectory streaming at 1-4 kHz from Klipper to Granite drives over SimpleMotion bus, with real-time position/velocity/torque feedback exposed back to Klipper for shaping and learning. Granite `smv2` SDK is the entry point. Step/dir baseline as fallback for v0 commissioning. **Aligned with priority #6 — this is the testbed.**
2. **Snap-limited trajectory planner** (motion planning research). 4th-order trajectory generation in Klipper. Reduces actuator stress at high acceleration. May fold into the SimpleMotion bridge work.
3. **Air bearings** (mechanical R&D, 5-10 prototype iterations expected). Process trials before machine integration: lip-honing process, wax burn-out cleanliness, CFRP creep test under cyclic 1 MPa, single-bearing flow + stiffness characterisation.
4. **ML-tuned predictive motion compensation** (control research). Position-dependent forward and inverse mechanical model, identified from load-side encoder data. Needs the Klipper bridge (1) and load-side scales installed.
5. **130°C chamber capability** (test program). High-Tg epoxy formulation for chamber-side panels (or thermal-break design), high-grade encoder/magnet selection, in-chamber 3DP material qualification.

## Open risks & mitigations

| Risk | Mitigation |
|---|---|
| Reify sheet-metal flatten gap | v0.1 workaround: model 2D flat patterns directly. Follow-up: file reify PRD. |
| Air bearing manufacturing repeatability | HIWIN bolt-pattern fallback designed in from day one. |
| Klipper-SimpleMotion bridge effort overruns | Step/dir baseline keeps v0.1 commissioning unblocked. |
| Counter-mass mass variation across tools | v1: fixed-mass sized to mean tool. v2: variable-position counter-mass. |
| 800mm gantry beam compliance | CFRP tube specced for EI; verify with a static deflection test before commit. |
| Z-bed parking-shutter thermal sealing | Bench prototype the shutter geometry before integrating. |
| £4k budget pressure | Cut levers in priority order: defer linear scales (-£300), 2-tool toolchanger v1 (-£400), defer ODrive/skip Ioni-HC peripheral axes (-£200-400). |
| Encoder thermal limits at 130°C stretch | Mount motors outside chamber from day one; magnetic encoders never see chamber air. |

## Decision log (from architecture conversation)

- **CoreXY toolchanger** chosen; alternatives ruled out per analysis above.
- **Vectran 6mm braid + capstan drives** chosen over toothed belts.
- **Anchored (winch) capstan over friction wrap** — friction micro-slip under rapid/unbalanced reversals + resonance is unacceptable for accuracy, and friction creep wears the tendon; positive anchoring eliminates both. Re-introduces axial wrap-band migration, handled by a passive translating fairlead.
- **Set-and-locked rigid pre-tension over a compliant/constant-force tensioner** — a sprung tensioner adds series compliance that collapses the antagonistic drive stiffness (k₁+k₂); thermal drift absorbed by preload margin + periodic re-tension (+ load-side scales), with active servo-held tension as the fallback.
- **Industrial AC servos + Granite Argon drives** chosen over drone outrunner concept once on-shelf inventory was confirmed (lower inertia, integrated encoders, better fit).
- **Counter-masses required** by 1000 m/s² stretch goal; designed in from v0.1 even at 100 m/s² baseline.
- **Z-bed** chosen over gantry-Z for kinematic decoupling and chamber sealing simplicity. Bed parks below build zone behind insulated shutter (no bellows).
- **HIWIN-first bearing interface, air bearings on track 2** to decouple R&D risk from build schedule.
- **Self-compensating well-and-lip air bearing topology** (Slocum / MIT PERG pattern) resolves the orifice/restrictor question.
- **Chamber 80°C commission, 130°C capable design** to defer high-cost material upgrades until needed.
- **Motors mounted outside chamber** to keep encoders + windings + magnets in spec at 130°C.
- **Klipper-SimpleMotion bridge** identified as primary firmware deliverable, aligned with priority #6.
- **Single-piece frame** (no sub-760mm modular requirement) — container workshop has full-width end doors.

## Out of scope for v0.1

- 130°C chamber commissioning (designed-for, not commissioned-at).
- Air bearings on the production machine (track 2 R&D).
- ML motion compensation (track 2 R&D, gated on Klipper bridge + load-side scales).
- More than 2 tools at first commission (4-tool dock designed in, 2 fitted for v1).
- Variable counter-mass.
- Full snap-limited trajectory planning (track 2).
