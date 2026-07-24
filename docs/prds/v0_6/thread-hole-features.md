# Thread & hole features — modeled/cosmetic threads, standards-derived hole family, fitted bores

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-24 · **Shape:** B+H (contract + two-way boundary tests)

Authored from the 2026-07-24 language-review /prd session (brief:
`~/.claude/spawn-briefs/2026-07-24-prd-threads-holes.md`); all design decisions below were
resolved interactively with Leo in that session. Substrate probes (fixtures
`holes-1..14.ri`, debug binary @ main 366c63a679) back every G3 claim; re-runnable in
< 1 min.

## 1. Goal — what a user observes when this lands

1. **The capstan drum's helical groove is real geometry.** `prj/printer_v01/dev_capstan.ri`'s
   documented comment-ware groove (`difference(drum, sweep(posed circle, helix(...)))`,
   blocked-on-#5342 caveat at its MODELLING CAVEATS header) evaluates to a solid whose
   volume differs from the smooth core by the groove volume; the GUI viewport shows a
   grooved drum.
2. **Holes are declared, not hand-composed.** A designer writes
   `let m4 = TappedHole(thread: m4_spec, depth: 8mm)` and
   `let body = difference(stock, m4.cutter)`; `reify eval` prints the hole's
   standards-derived diameters (tap drill 3.3 mm for M4×0.7); `reify check` enforces the
   DFM constraints (engagement depth, tap-drill consistency, depth sanity) and reports
   violations as first-class constraint results.
3. **Threads are cosmetic by default and modeled on demand.** The same `.ri` file, with
   `#thread_repr(modeled)` added at the top (or `repr: ThreadRepr.Modeled` on one
   feature), produces a measurably different solid — modeled helical thread form — with
   the volume delta inside a stated band. Cosmetic stays the default because boolean CUT
   cost is superlinear in tool complexity (known bottleneck, 2026-07-21 N-hole survey).
4. **Bores carry their fit.** printer_v01's six hand-composed bearing/press-fit/shaft
   bores become `FittedBore`s whose `FitDesignation` (e.g. H7) yields a real IT tolerance
   width via the existing `iso_it_tolerance` builtin, consumed by a check-time constraint.

## 2. Background

- `ThreadSpec` (`crates/reify-compiler/stdlib/ports_mechanical.ri:66-78`) already computes
  ISO 68-1/261 derived dims (minor/pitch Ø, tap drill, clearance) and carries
  `thread_form : Option<Geometry> = none` — an explicitly deferred carrier slot ("until a
  thread-helix kernel op exists"). This PRD fills the gap the slot anticipates.
- **No thread-solid generator exists anywhere.** A helix *wire* primitive exists
  (`GeometryOp::Helix`, `make_helix_wire`), but #5342 (helix wires carry no 3D curve —
  `BRepLib::BuildCurves3d` missing) blocks every sweep/pipe consumer. #5342 is
  in-progress (live heartbeat 2026-07-24). #5343 (pipe +Z start-tangent) is **not**
  needed here: the generator poses its profile internally in Rust.
- **No hole features exist**; every hole in the corpus is an inline
  `difference(solid, cylinder/torus)` (printer_v01 ×6 bores,
  `examples/tolerancing/vc_bolt_pattern_clearance.ri`, `examples/perforated_plate.ri`).
  printer_v01 has **zero** fastener holes — axle bolts are explicitly unmodelled — so the
  dogfood leaf both retrofits bores and *adds* the first fastener holes.
- `MeshingOptions.threads` is CPU worker threads (cache-key note), **not** screw threads —
  a red-herring name collision; this PRD deliberately avoids the bare word "threads" in
  its pragma/type names (`#thread_repr`, `ThreadRepr`).
- `iso_it_tolerance` (`crates/reify-stdlib/src/tolerancing.rs:112-128`) is the house
  pattern for standards builtins: pure `fn(&[Value]) -> Value`, dispatcher arm, `diagnose`
  classifier (`E_TolerancingOutOfEnvelope`), `.ri` derived-let caller, layered tests
  pinning published table cells.
- The stale comment at `ports_mechanical.ri:58-62` ("if/match don't parse in a let RHS")
  is **no longer true** — probed 2026-07-24: `match` (bare-variant arms) and
  `if…then…else` both parse and evaluate in let RHS. The fit-class table deferral it
  justified is lifted by this PRD.

## 3. Substrate reality (G3 record — probed, not assumed)

| Capability | Status | Evidence |
|---|---|---|
| `let x = ThreadSpec(...)` named-arg ctor + derived-let reads | **works** | probe: `reify eval` prints `Test.td = 0.0042 m`; no stdlib re-declaration on the `let` path |
| geometry lets + `constraint` inside `structure def` | **works** | `examples/bracket.ri`; probe holes-7 |
| `match` on enum / `if…then…else` in let RHS | **works** | probes holes-10/12; `examples/m5_guarded_enum.ri` |
| `#thread_repr(modeled)` pragma syntax | **parses** (generic pragma); unknown key warns (`warning: unknown pragma`) | probe holes-14 |
| `some(geometry)` construction | checks; payload untested end-to-end | probe holes-5; every in-tree `Option<Geometry>` is `= none` |
| `difference(body, h.cutter)` (instance geometry member in geometry op) | **absent** — compile rejection | probes holes-1/2/8/9; **delivered by uniform-member-access γ = task 5426** |
| `.ri` fn returning Geometry in geometry position | **absent** ("unsupported geometry function") | probe holes-6; **delivered by uniform-member-access ε = task 5428** |
| enum-typed ctor param binding, *user-module* enums | **broken** ("Enum(Fit)" vs "structure type") | probe holes-13; stdlib enums bind fine (holes-3); fix = uniform-member-access ζ = task 5429 (not a dep — all enums here are stdlib) |
| `sub` of stdlib structure | re-declaration hazard | `ports_mechanical_thread_eval.rs:5-14`; #5391 gate (4); this PRD uses the `let` path only |
| thread solid generator | **absent** | built here (task α), gated on #5342 |

Kernel-less `reify check` cannot realize geometry: **no leaf signal in this PRD asserts a
check-time geometry value** (uniform-member-access C3-vi rule — static acceptance +
loud-indeterminate). Geometry observables use `reify eval`/`build`.

## 4. Resolved design decisions

1. **Cosmetic by default; opt-in per feature; realization-level global override.**
   Industry default + superlinear boolean cost. Per-feature switch:
   `param repr : ThreadRepr = ThreadRepr.Inherit` (`enum ThreadRepr { Inherit, Cosmetic,
   Modeled }`). Global override: module pragma `#thread_repr(cosmetic|modeled)`.
   **Precedence: explicit per-feature value > pragma > built-in default (Cosmetic).**
2. **Pragma reflection via ambient constant — the `#deterministic` trap avoided.** The
   `#deterministic` precedent (module_pragmas.rs:440-448) landed a module flag whose real
   consumer stayed a separate per-call channel. Here the pragma's consumer ships in the
   same PRD: `#thread_repr` → `CompiledModule.thread_repr` → compiler-injected ambient
   prelude constant `ambient_thread_repr : ThreadRepr` (default `Cosmetic`) → stdlib
   feature lets `match` on it. The pragma leaf's own e2e signal is the volume flip (§9 ζ).
3. **Hole surface = semantic structures with a `cutter` geometry member** (Leo: option
   (c) over (a)): stdlib structures carry ThreadSpec/fit/depth semantics, expose
   `cutter : Geometry`, and conform to the port layer (a tapped hole IS a ThreadedPort
   site). Consumers write `difference(body, h.cutter)` — delivered by uniform-member-access
   **5426** (hard dep for consumption leaves). Plain-recipe `.ri` fns (option (a)) become
   possible via **5428** and are allowed as sugar, not the spine.
4. **One dual-gender generator.** `thread_solid(...)` produces external (male) thread
   solids AND internal-cutter envelopes (internal cutter ≈ male form + class allowance).
   Feature *structures* here are the hole family only; one external acceptance demo
   (threaded rod example). Full fastener part families → #5391.
5. **Generator is a Rust geometry builtin with primitive args, composing existing ops.**
   No new kernel op: builds profile-posed-at-helix-start internally, emits
   Helix + Sweep + boolean `GeometryOp` composition (zone_cylinder precedent). Args are
   Lengths/Bools only; enum→primitive mapping happens in stdlib `match` (probed). Hard
   dep #5342; #5343 not needed.
6. **Fit = `FitDesignation` structure with a real consumer today** (INV-SF-3/-5
   compliant): `structure def FitDesignation { param letter : String; param grade : Int;
   let tolerance_width = iso_it_tolerance(grade, nominal…) }`. The grade half is consumed
   NOW via the existing ISO 286-1 IT builtin; the `letter` half (fundamental deviation)
   is **owned by #5391** — the String param carries a PTODO citing #5391 (live,
   non-terminal). Hole slot: `param fit : Option<FitDesignation> = none`.
7. **Small DFM check() set in scope; broader DFM is its own PRD** (bookmark [MILESTONE]
   task filed at decompose): engagement depth (warn threshold ≥ 1.0×D, conservative,
   material-agnostic — material coupling explicitly deferred to the DFM PRD), tap-drill
   consistency (pilot override vs spec), depth/counterbore sanity. Implemented as
   structure `constraint`s → check()-time results (probed substrate).
8. **G2 leaves are dogfood-real:** capstan groove modeled for real (dep #5342) + printer
   bore retrofit to `FittedBore` + first fastener clearance holes. The groove is direct
   sweep-along-helix geometry (single-structure composition — no 5426 dep); the retrofit
   consumes the full feature surface (5426 dep).
9. **Cosmetic representations:** TappedHole cutter = cylinder Ø `tap_drill` (material-
   accurate for print-then-tap); ClearanceHole = cylinder Ø from fit-class `match`
   (Close/Medium/Coarse, documented ISO 273 approximations — exact tables → #5391);
   CounterboreHole = clearance + coaxial flat-bottom cylinder (defaults from documented
   SHCS approximations); CountersinkHole = clearance + 90° cone; FittedBore = cylinder Ø
   nominal. Modeled (TappedHole) = cosmetic ∪ internal `thread_solid`.
10. **Cutter frame convention:** every cutter is built in the feature's local frame —
    hole axis = +Z, mouth plane at z = 0, material side −Z→depth... (see C4 for the
    exact convention and overshoot rule). Consumers pose cutters with existing
    `translate/rotate` (world-posed port placement stays with `placement-relations-belt`).
11. **Port conformance:** `TappedHole : ThreadedPort` (supplies `thread_spec`; inherits
    `frame : Frame3` from LocatedPort — default identity frame at the cutter's local
    origin), `FittedBore : Bore`. Clearance/counterbore/countersink holes do NOT conform
    to ThreadedPort (no thread); they are plain feature structures carrying an optional
    fit.
12. **Naming avoids the "threads" collision** (`MeshingOptions.threads` = CPU threads):
    pragma `#thread_repr`, enum `ThreadRepr`, builtin `thread_solid`, module
    `std.features.holes` (new stdlib file `features_holes.ri`, prelude-sequenced after
    `std.ports.mechanical` and `std.tolerancing` in `stdlib_loader.rs`).

## 5. Contract section (H)

### C1 — `thread_solid` generator builtin

```
thread_solid(
    nominal_diameter : Length,   // major Ø D
    pitch            : Length,   // P
    length           : Length,   // threaded length along the axis
    right_handed     : Bool,     // true = right-hand helix
    internal         : Bool,     // false = male solid; true = internal-cutter envelope
    allowance        : Length,   // radial class allowance; ≥ 0; applied outward when internal
) -> Solid
```

- **Geometry:** ISO 68-1 60° flank profile (H = 0.866·P, standard crest/root truncations),
  swept along `helix(pitch_radius, P, length)`; **male** form = core cylinder(Ø minor) ∪
  swept ridge; **internal cutter** = male form dilated radially by `allowance`
  (the envelope of material removed by tapping). Left-hand = mirrored helix.
- **Local frame:** axis +Z, thread starts at z = 0, extends to z = `length` (C4
  convention).
- **Invariants:**
  - I1 volume ∈ [V_core, V_core + 1.5·A_profile·L_helix] where
    V_core = π·(minor/2)²·length, A_profile = the truncated-triangle profile area,
    L_helix = length/P · sqrt((π·d2)² + P²) (d2 = pitch Ø). Boundary tests assert a
    **generous** band (±25%) — derived, not guessed (G6 basis: closed-form core +
    ridge-volume estimate; tessellation/boolean tolerance absorbed by the band).
  - I2 handedness: mirroring `right_handed` mirrors the solid (volume invariant,
    chirality flips — asserted via a probe point off the mirror plane).
  - I3 failure is **loud**: invalid inputs (pitch ≤ 0, length < P, D ≤ 1.0825·P → minor
    ≤ 0) emit a coded diagnostic (`E_ThreadSolidOutOfEnvelope`, iso_it `diagnose`
    pattern) and return Undef **with recorded UndefCause** (INV-SF-1, INV-SF-6). No
    silent empty solid.
- **Args are primitives by design** — enum→primitive mapping (`ThreadTighteningDirection`
  → `right_handed`, `ThreadClass` → `allowance`) lives in stdlib `match` (probed
  substrate), keeping the builtin free of DSL enum coupling.

### C2 — `#thread_repr` pragma + ambient constant

- Grammar: existing generic pragma form (probed). Compiler registers the key:
  `#thread_repr(cosmetic)` | `#thread_repr(modeled)`; any other argument → **error-severity
  diagnostic with code** (INV-SF-2/-6; unknown *key* today warns — registration removes
  the warning for this key).
- `CompiledModule.thread_repr : Option<ThreadReprPragma>` (types.rs, alongside
  `kernel_pragma`); consumed by injecting `ambient_thread_repr : ThreadRepr` (values
  `Cosmetic`/`Modeled`; never `Inherit`) into the prelude scope at compile time; absent
  pragma → `Cosmetic`.
- Stdlib resolution (single owner, one helper):
  `fn resolve_thread_repr(r : ThreadRepr) -> ThreadRepr = match r { Inherit =>
  ambient_thread_repr, Cosmetic => ThreadRepr.Cosmetic, Modeled => ThreadRepr.Modeled }`
  — every feature's cutter let matches on `resolve_thread_repr(repr)`
  (INV no-lockstep-duplication: features never read the ambient directly).
- **Consumer ships in-PRD**: leaf ζ's observable is the pragma flip changing eval'd
  volume — the pragma can never land as a dangling flag.

### C3 — `FitDesignation` seam contract (consumed by #5391)

```
structure def FitDesignation {
    param letter : String            // "H", "g", … — fundamental deviation; PTODO(#5391)
    param grade  : Int               // IT grade 5..18
    param nominal : Length           // the feature's nominal Ø (bound by the owning feature)
    let tolerance_width = iso_it_tolerance(grade, nominal, nominal)
}
```

- **This PRD owns:** the structure, the grade→IT-width consumption, the
  `Option<FitDesignation> = none` slot on every hole/bore feature, and one check-time
  consumer (FittedBore's seat-tolerance constraint, §6 row 9).
- **#5391 owns:** letter → fundamental-deviation resolution (ISO 286 tables), limit
  dimensions (`upper_limit`/`lower_limit`), letter validation, and any
  clearance/interference pairing checks. Until then `letter` is carried, printed by
  `reify eval`, and validated only for non-emptiness; the param carries
  `// TODO(#5391): fundamental-deviation tables resolve letter → limits` (PTODO-conformant,
  INV-SF-5: the stand-in String names its owner).
- #5391 wires `add_dependency` edges onto this PRD's γ (stdlib module) — recorded in §8.

### C4 — cutter member contract

- `cutter : Geometry` is a **derived let** on every feature structure; local frame: hole
  axis = +Z, **mouth plane at z = 0, cutter extends into material along −Z** to
  `depth`, and **overshoots the mouth by `overshoot : Length = 0.1mm`** (param) above
  z = 0 so coincident-face booleans are robust. Through-holes: the user sets `depth` ≥
  body thickness + overshoot (no hidden "through" magic in v1; a `through` sugar is an
  open question, §11).
- Counterbore/countersink negative volumes are coaxial and included in the single
  `cutter` value (one member, one difference at the call site).
- **Inert-until-consumed** (uniform-member-access D3 ruling): a feature instance's
  cutter never auto-joins the parent's rendered solid set; it realizes at consumption
  sites only.
- Consumption spine: `difference(body, h.cutter)` — **requires 5426**; chained paths
  (`bracket.mount.cutter`) additionally 5427; `.ri` recipe fns 5428. Until 5426 lands,
  γ's own tests exercise cutters *within* the defining structure (same-structure
  composition — probed) and via eval field reads.

### C5 — feature structures (normative member sets)

| Structure | Conforms | Key params (beyond `repr`, `fit`, `overshoot`) | Cutter (cosmetic) |
|---|---|---|---|
| `TappedHole` | `ThreadedPort` | `thread : ThreadSpec`, `depth : Length`, `pilot : Option<Length> = none` | cylinder Ø tap_drill × (depth+overshoot); modeled: ∪ `thread_solid(internal)` |
| `ClearanceHole` | — | `thread : ThreadSpec`, `depth`, `fit_class : HoleFitClass = Medium` | cylinder Ø clearance(fit_class) |
| `CounterboreHole` | — | ClearanceHole's + `cbore_diameter`, `cbore_depth` (defaults: documented SHCS approximations) | clearance ∪ cbore cylinder |
| `CountersinkHole` | — | ClearanceHole's + `csk_diameter`, `csk_angle : Angle = 90deg` | clearance ∪ cone |
| `FittedBore` | `Bore` | `nominal : Length`, `depth : Length` | cylinder Ø nominal |

DFM constraints (task γ): TappedHole `constraint depth >= nominal_diameter` (engagement,
conservative 1.0×D floor; material coupling → DFM PRD); TappedHole pilot-vs-tap_drill
consistency when `pilot` is `some(...)`; all: `depth > 0mm`, cbore/csk sub-depths <
`depth`. Every violated constraint surfaces at `reify check` as a first-class result
(probed substrate; no new diagnostic channel).

## 6. Boundary-test sketch (H — two-way)

Producer side = generator/compiler/stdlib; consumer side = dogfood/examples via 5426.

| # | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|
| 1 | `thread_solid` M5×0.8×10, external | #5342 landed | non-Undef Solid; volume in C1-I1 band |
| 2 | handedness mirror | row 1 | left/right volumes equal; chirality probe point differs |
| 3 | `thread_solid` invalid (minor ≤ 0) | — | `E_ThreadSolidOutOfEnvelope` emitted; Undef with recorded cause; `reify eval` exit nonzero (INV-SF-2) |
| 4 | pragma flip on same file | α, β, γ | eval'd volume differs cosmetic→modeled; delta in ridge band |
| 5 | precedence: explicit `repr` beats pragma | β, γ | `repr: Cosmetic` under `#thread_repr(modeled)` yields cosmetic volume |
| 6 | `#thread_repr(bogus)` | β | coded error diagnostic; nonzero exit |
| 7 | `difference(plate, h.cutter)` consumer | γ + **5426** | compiles, evals; plate volume reduced by cutter∩plate volume (closed-form band) |
| 8 | TappedHole DFM: depth < D | γ | check() reports the engagement constraint violated |
| 9 | FittedBore H7-style fit | γ | `fit.tolerance_width` = published IT cell (exact-value pin, iso_it 24.969 µm pattern); seat constraint consumes it at check() |
| 10 | ThreadSpec.thread_form filled | α, γ | `some(thread_solid(...))` binds; `is_some(spec.thread_form)`; eval prints Some(…) (closes the §2 carrier-slot arc + the untested some(geometry) cell) |
| 11 | capstan groove | **#5342**, δ | drum volume delta ≈ π·r²·L_helix within ±15% (G6 basis: #5342's own acceptance math) |
| 12 | printer retrofit invariance | γ, 5426, ε | pure bore→FittedBore retrofit leaves each part volume unchanged (< 0.1%); added clearance holes change volume by computed amount |

The integration-gate task (θ) names this table as its observable signal; rows 1–6 face
the producer, 7–12 the consumer.

## 7. Pre-conditions for activating

- **#5342** (helix 3D curve) landed — hard dep of α and δ. In-progress with live
  heartbeat at authoring time.
- **uniform-member-access batch filed** (done 2026-07-24: tasks 5424–5434; PRD
  `docs/prds/v0_6/uniform-member-access.md`, landing as task 5433): **5426** is a hard
  dep of every consumption leaf (ε, ζ, θ rows 7/12); **5428** soft (recipe sugar only);
  **5427** only if chained-path consumption appears (not asserted by any leaf signal).
- This PRD's own docs-landing task gates the batch roots (5433 pattern).
- **NOT** #5343 (generator poses internally); **NOT** ISO 286 letter tables (#5391);
  **NOT** `placement-relations-belt` (local-frame cutters only).

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `uniform-member-access.md` (5424–5434) | consumes | `h.cutter` in geometry ops (5426); `.ri` geometry fns (5428) | other-prd | filed; dep edges wired at decompose |
| #5391 standard-parts program | produces-for | `FitDesignation` contract (C3); hole/thread features; `thread_solid` | this-prd (features/contract); #5391 (letter tables, catalogs, fastener parts) | #5391 live in parallel; it dep-edges onto γ; its gate (4) (`sub` re-declaration) unchanged here |
| `placement-relations-belt.md` (parallel session) | references | world-posed port/feature placement; `.world_frame` | other-prd | reference only — cutters are local-frame (C4) |
| sub-iteration PRD (per uniform-member-access §note) | references | indexed feature arrays (`holes[i].cutter`) | other-prd | out of scope here; hole *patterns* use existing pattern ops over cutter geometry |
| #5342 helix 3D curve | consumes | sweepable helix wire | task 5342 | in-progress; hard dep α, δ |
| #5343 pipe +Z | none | — | task 5343 | explicitly not a dep |
| broader-DFM PRD | defers-to | material-coupled DFM rules (engagement vs material, wall thickness, printability) | bookmark [MILESTONE] task (filed at decompose) | pending trigger |
| `task/5395` design-invariants | consumes doc | G7 slugs INV-SF-1..6 | landed-in-flight | walked in §9 |

No new contested-ownership pair is introduced (checked against the phase-3 breadcrumb
map §3 trio).

## 9. Decomposition plan

Labels α–θ; real ids at decompose. Every leaf names its observable signal; G7 walked
against INV-SF-1..6 (notes inline; no waivers needed).

- **α — `thread_solid` geometry builtin (dual-gender) + envelope diagnostics** ·
  crates: reify-eval, reify-compiler (registry), reify-kernel-occt tests ·
  deps: **#5342** ·
  signal: boundary rows 1–3 as gate-resident tests — volume band, handedness, loud
  envelope failure (`E_ThreadSolidOutOfEnvelope` + UndefCause). G7: INV-SF-1/-6 designed
  in (C1-I3). *Gate-test drift-guard: new gate-resident tests register in the same diff
  (run-all classification / nextest partitions as applicable).*
- **β — `#thread_repr` pragma → `CompiledModule` → `ambient_thread_repr` injection** ·
  crates: reify-compiler ·
  deps: α (its e2e consumer path) ·
  signal: boundary rows 4–6 — pragma flip changes eval'd volume of a direct
  `thread_solid`-consuming fixture; bogus arg = coded error, nonzero exit. G7:
  INV-SF-2/-6 (row 6), declared-intent-consumed (consumer in same slice).
- **γ — stdlib `std.features.holes` module: ThreadRepr/HoleFitClass/FitDesignation +
  five feature structures + DFM constraints + loader wiring** ·
  crates: reify-compiler (stdlib .ri + loader), reify-eval tests ·
  deps: α, β ·
  signal: `reify eval` on a fixture prints TappedHole derived dims incl. cutter field;
  `reify check` reports DFM violations (row 8) and the FittedBore IT pin (row 9);
  ThreadSpec.thread_form fill (row 10). Also updates the stale `ports_mechanical.ri`
  comment + fit-class-aware clearance via `match`. G7: INV-SF-3 (fit slot consumed),
  INV-SF-5 (letter PTODO → #5391).
- **δ — capstan helical groove modeled for real (dogfood leaf 1)** ·
  files: `prj/printer_v01/dev_capstan.ri` ·
  deps: **#5342** (only — direct sweep-along-helix; no feature-surface dep, so it can
  land first and de-risk the geometry path) ·
  signal: boundary row 11 — volume-delta band e2e test + caveat header deleted.
- **ε — printer_v01 bore retrofit + first fastener clearance holes (dogfood leaf 2)** ·
  files: `prj/printer_v01/printer.ri`, `dev_capstan.ri` ·
  deps: γ, **5426** ·
  signal: boundary row 12 — six bores become `FittedBore` (volume-invariant), idler axle
  clearance holes added (computed volume delta), check() green with fit/DFM constraints
  active.
- **ζ — modeled-thread e2e: threaded-rod example + modeled TappedHole variant** ·
  files: `examples/` ·
  deps: α, β, γ, 5426 ·
  signal: boundary rows 4–5 at example level; external demo per decision 4.
- **η — doc chunks + reify-design cheatsheet + discoverability (project PRD gate)** ·
  crates: reify-mcp (`chunks/` + `language_chunks.rs` + topic-count tests),
  `.claude/skills/reify-design/SKILL.md` ·
  deps: γ (surface frozen), ζ (idioms real) ·
  signal: new/extended chunk documents ThreadSpec + hole family + `thread_solid` +
  `#thread_repr` + helix/sweep (closing the audited helix/sweep_guided doc hole) with
  signatures verified against the compiler registries in-task; TOPICS registration +
  count tests updated; an intent query ("how do I make a tapped hole") through
  `reify_language_reference` returns the chunk; cheatsheet gains the hole/thread idiom.
- **θ — two-way boundary-test integration gate** ·
  deps: α, β, γ, δ, ε, ζ ·
  signal: §6 table rows 1–12 all green on the merge gate (the C-as-integration-gate
  leaf). *Same-diff drift-guard registrations for any new gate-resident harness.*
- **Bookmark (filed at decompose): broader-DFM PRD [MILESTONE]** — pending; scope:
  material-coupled engagement, wall-thickness/edge-distance, printability of modeled
  threads; consumes γ's feature surface + #5391 materials.
- **Docs-landing task** — this PRD's commit lands via merge queue; batch roots (α, δ)
  dep on it (5433 pattern).

DAG: α → β → γ → {ε, ζ} → θ; δ independent after #5342; η after γ+ζ; #5342 → {α, δ};
5426 → {ε, ζ}; landing-task → {α, δ}.

## 10. Out of scope

- Fastener part families, nuts/washers/inserts, head-dimension tables, catalog mechanism
  — **#5391**.
- ISO 286 letter (fundamental-deviation) tables, limit dims, fit pairing checks — **#5391**
  (C3 seam).
- Indexed per-hole instances (`holes[i]`) — sub-iteration PRD; patterns of cutter
  geometry via existing `linear_pattern`/`circular_pattern` remain available.
- World-posed feature/port placement — `placement-relations-belt.md`.
- Drawing/callout output (no drawing surface exists), thread process modeling
  (rolled/cut), metric-exotic thread systems beyond the four `ThreadSystem` variants.
- Material-coupled DFM — bookmark PRD.

## 11. Open questions (tactical)

1. **`through : Bool` sugar on holes** (auto-length from consuming body): needs body
   extent at feature site — defer; C4's explicit-depth rule is safe. Decide at ε if the
   dogfood shows real friction. **Suggested resolution:** keep explicit depth in v1.
2. **Thread runout/chamfer ends on `thread_solid`:** cosmetic ends (flat) vs 45° runout
   cones. Impl-time; band I1 already absorbs either. **Suggested:** flat in v1.
3. **`csk_diameter` default rule** (from clearance Ø + head projection): pick constant at
   γ with the documented-approximation comment pattern.
4. **Exact `HoleFitClass` constants** (ISO 273 close/medium/coarse per-nominal deltas vs
   the current `D + 0.5·P` medium approximation): γ chooses; comment documents deviation;
   exact tables land with #5391.
5. **Whether `FittedBore` should warn when `fit` is `none`** (silent nominal bore) — lean
   no (a plain bore is legitimate); revisit with #5391's pairing checks.

## 12. Capability manifest

Committed beside this PRD at decompose:
`thread-hole-features.capability-manifest.md` + `.yaml` sidecar (delivered_checks stamped
by commit_planning).
