# PRD: Functional surface finish / treatment / coating — the third Appearance producer

**Milestone:** v0_6 · **Status:** active (design-first, decompose-ready) · **Date:** 2026-06-29
**Approach:** **B + H** (stdlib vocabulary + Rust appearance-derivation seam + cost/mass roll-up + two-way
boundary tests) — multi-crate types→resolution→egress seam that extends a load-bearing shipped contract
and is exercised on two user surfaces (3MF export + GUI viewport).
**Capstone of:** umbrella **task 4291** (the `io-display-output-viewport` appearance batch). Milestone
tracker **task 4784** (forward-stub committed `87a77725cc`).
**Parents (cosmetic precursor, both done):** `appearance-substrate.md` (PRD-1, task **4763** δ — 3MF
per-body color on the EXPORT surface) + `appearance-viewport-egress.md` (PRD-2, task **4775** ε —
appearance + `DisplayOutput.style` precedence on the VIEWPORT surface).

---

## 0. Thesis

The cosmetic appearance system (PRD-1 + PRD-2) ships a **display-only** notion of finish:
`Finish { Matte, Satin, Gloss }` + a `Color`, driving the viewport and 3MF color — *purely how a part
looks*. `Appearance` is the stable, source-agnostic, renderer/export-facing contract; **materials** are
its first producer (PRD-1) and the **`DisplayOutput.style` override** its second (PRD-2).

This capstone makes **functional surface finish / treatment / coating** first-class, spec-bearing **part
properties**, and makes them the **third producer of `Appearance`** — so specifying a *real* coating
(anodize, powder-coat with a RAL spec) or finish (polished, bead-blasted) **automatically** yields the
cosmetic look, *without changing the `Appearance` contract the renderer/3MF export already consume*. That
is the load-bearing **forward-compat invariant** the whole capstone exists for (PRD-1 ratified decision 1).
The cosmetic `Finish` enum is **subsumed** by becoming a *projection target* of the functional model — it
is not replaced, the `Appearance` contract is untouched.

**Substrate discovery that reshapes this capstone (re-verified 2026-06-29).** The stub assumed surface
finish was greenfield. It is not: the **roughness/lay/process metrology** (`tolerancing.ri` `SurfaceFinish`,
ISO 1302) and the **process-capability/DFM** surface (`process.ri` `SurfaceTreating` / `HeatTreating` /
`Process.cost`) already exist on main, and **cost roll-up** is already an eval-proven DSL idiom
(`io.ri` `Costed.line_cost` + `.sum` → `total_cost : Money`). Per Leo's ratified "**Reuse + bridge**"
decision, this PRD **reuses** those and **owns only the genuine gaps**: a `Coating` color-spec type, a
finish-process appearance vocabulary, a part-level attachment trait, **the Rust Appearance-derivation seam
(third producer)**, and the per-part finishing **cost + mass** roll-up.

---

## 1. Goal — what a user observes (G2)

Three CI-testable user surfaces, all reusing shipped consumers:

1. **The functional vocabulary parses + type-checks.** A `.ri` instantiating `Coating`/`CoatingProcess`/
   `FinishProcess`/`Treatment`/`TreatmentProcess` + a `Part : SurfaceTreated` carrying a coating with a
   real RAL/MIL spec passes `reify check` (exit 0; **zero** `unresolved type`/`unresolved name`). The
   committed gate fixture `docs/prds/v0_6/fixtures/surface_finish_functional.ri` demonstrates this (§3).
2. **A real coating drives the cosmetic look on BOTH egress surfaces (the subsumption).**
   - **Export:** `reify build part.ri -o part.3mf` for a body whose part has a `Coating(process: Anodize,
     color: Color(named:"RAL9005", …))` writes the **coating-derived** RGB (anodize-black) into the 3MF
     `<basematerials>` — overriding the bare-material grey — via the same egress PRD-1 δ (#4763) shipped.
   - **Viewport:** opening that `.ri` in the dev GUI renders the body with the coating-derived appearance
     (anodize dark; a `Polished` finish_process → low-roughness/high-sheen) via the same
     `resolve_appearance` seam PRD-2 (#4775) consumes.
3. **Finishing cost + mass roll up.** `reify eval` of an assembly whose parts carry coatings/treatments
   yields `total_finishing_cost : Money` (flat, deterministic — e.g. `24 USD`), and a part with realized
   geometry yields area-based `coat_cost : Money` and `coat_mass : Mass` (e.g. `1.2 USD`, `0.0018 kg`).
   This is the per-part contribution a future design-wide BOM report (`io-lifecycle-bom-cost.md`) will
   aggregate.

---

## 2. Background — verified substrate (re-verified 2026-06-29; reuse map)

The capstone's correctness rests on **what already exists**. Each row was re-verified this session against
`target/release/reify` (Jun-24 build) + `tree-sitter-reify`.

### 2.1 REUSE — do not duplicate (G3 substrate exists; G4 owned elsewhere)

- **Roughness/lay/process metrology (ISO 1302) — `tolerancing.ri`:**
  `structure def SurfaceFinish { param parameter : SurfaceParameter; param value : Length; param direction
  : SurfaceDirection = Multidirectional; param process : String = "" }` (`:254`),
  `enum SurfaceParameter { Ra, Rz, Rq, Rt, Rp, Rv, Rsk, Rku }` (`:25`),
  `enum SurfaceDirection { Parallel, Perpendicular, Crossed, Multidirectional, Circular, Radial }` (`:28`,
  the **lay**), consumed by `fn require_finish(feature, finish) -> Bool` (`:334`). **This covers the
  "Ra/Rz roughness, lay/direction" of the stub scope as a GD&T callout — REUSED, not re-modeled.**
  Owner: `tolerancing-gdt-surface-completion.md`.
- **Process capability / DFM — `process.ri`:**
  `trait Process { param duration : Time; param cost : Money }` (`:37`),
  `trait SurfaceTreating : Process { param coating_thickness : Length; param achievable_finish : Length }`
  (`:104`), `trait HeatTreating : Process { param treatment_temperature : Temperature; param hold_duration
  : Time }` (`:112`). These describe **what a process can achieve** (capability), not a part-borne coating
  spec. Owner: `process-dfm-completion.md`.
- **Cosmetic visual contract — `materials_appearance.ri` (PRD-1, UNCHANGED):**
  `enum Finish { Matte, Satin, Gloss }`, `structure def Color { named, r, g, b }`,
  `structure def Appearance { color, finish, metalness, roughness }`, `trait Visual { appearance }`.
  Rust seam `crates/reify-eval/src/appearance.rs`: `resolve_appearance(body) -> Value` (navigates
  `body.material.appearance`, neutral fallback), `resolve_appearance_opt(body) -> Option<Value>`,
  `resolve_color(&Color, &mut diags) -> Rgb8` (hex/RAL/`W_UNKNOWN_COLOR_NAME`). **The contract is the
  forward-compat invariant — this PRD adds a producer, never mutates the shapes.**
- **Cost roll-up idiom — `io.ri` + Money dimension (eval-proven):**
  `trait Costed : Buy { param quantity_produced : Real; let line_cost : Money = unit_cost *
  quantity_produced }`; assemblies aggregate via the dimension-preserving `.sum` builtin
  (`[a.line_cost, b.line_cost].sum : Scalar<Money>`). `examples/cost_aggregation.ri` + locked-total test
  `crates/reify-compiler/tests/harness_units_materials/cost_aggregation_tests.rs` are the precedent.
- **Egress consumers (both shipped, both done):** the 3MF `<basematerials>` per-body color path
  (`reify-ir/src/geometry.rs` `write_3mf` + `reify-eval/src/engine_build.rs` `resolve_instance_color` →
  `resolve_appearance`) from PRD-1 δ (#4763); the viewport recolor from `resolve_appearance` from PRD-2
  (#4775). **Both already call `resolve_appearance` — extending that one seam reaches both surfaces.**
- **Unit / type-alias substrate:** `units.ri` provides SI-prefixed length units — **`um` (ASCII micro)
  resolves to `Length`** (1e-6 m; verified `1um < 1mm`); **`µm` (micro-sign U+00B5) does NOT parse** — use
  `um`. The type-alias mechanism `pub type X = A / B` works (`Velocity = Length / Time`,
  `HeatCapacity = Energy / Temperature`); `^` and parens are forbidden in alias RHS.

### 2.2 The genuine gap (what this PRD OWNS)

No part-borne **`Coating`** spec (process + thickness + **color/RAL spec** + cost) exists; no
**finish-process appearance vocabulary** exists; **nothing derives an `Appearance` from a finish or
coating** (the third producer); no part-level attachment trait ties them to a body; no per-part
**finishing cost/mass** roll-up exists. These are this PRD's deliverables.

---

## 3. G3 gate — grammar **and** semantic **and** eval, empirically validated (PASS)

Validated 2026-06-29 against `target/release/reify` + `tree-sitter-reify`. Two committed fixtures:

**`docs/prds/v0_6/fixtures/surface_finish_functional.ri`** (the vocabulary + flat-cost BOM) →
`tree-sitter parse --quiet` exit 0 (0 ERROR nodes) **and** `reify check` exit 0 ("All constraints
satisfied.", zero `unresolved type`/`unresolved name`) **and** `reify eval` →
`AssemblyBOM.total_finishing_cost = 24 USD` (= `Plate` 16 + `Bracket` 8, nested `sub`→`let` `.sum`).

**`docs/prds/v0_6/fixtures/surface_finish_area_cost.ri`** (realized area-based cost + mass) →
parse exit 0 **and** `reify eval` → `CoatedPlate.coat_cost = 1.2 USD` (50USD/m² × 0.024 m²),
`CoatedPlate.coat_mass = 0.0018 kg` (3000kg/m³ × 0.024 m² × 25um).

| Fragment exercised | Verdict |
|---|---|
| `enum CoatingProcess { Uncoated, Anodize, PowderCoat, Electroplate, Passivate, Paint }` | ✅ |
| `structure def Coating { process : CoatingProcess; thickness : Length = 0um; color : Color; spec : String; process_cost : Money; cost_per_area : ArealCostRate; coat_density : Density }` | ✅ |
| `pub type ArealCostRate = Money / Area` (areal cost-rate alias) + `cost_per_area = 0USD/m^2` literal | ✅ (`0 USD·m^-2`) |
| `enum FinishProcess { AsMachined, Ground, Polished, Lapped, BeadBlasted, Brushed, AsCast }` | ✅ |
| `enum TreatmentProcess {…}` + `structure def Treatment { process; spec : String; cost : Money }` | ✅ |
| `trait SurfaceTreated { coating : Coating = Coating(); finish_process = AsMachined; treatment = Treatment() }` (all-defaulted ⇒ additive) | ✅ |
| `structure def Plate : SurfaceTreated { … coating : Coating = Coating(process: Anodize, color: Color(named:"RAL9005",…), spec:"MIL-A-8625 Type II", …) }` (nested ctor, RAL + spec) | ✅ |
| flat cost: `let finishing_cost : Money = [coating.process_cost, treatment.cost].sum`; BOM `.sum` over `sub` members | ✅ (`reify eval` exact) |
| area cost/mass: `cost_per_area * area(geometry)`, `coat_density * area(geometry) * thickness` | ✅ at **top level** (`reify eval`) |
| `um` Length unit (`thickness = 15um`) | ✅ (`µm` micro-sign does **not** parse — use `um`) |

**Two empirically-grounded hazards captured for G6 / the decomposition:**
- **`area()`/`volume()` are kernel-gated.** They error on the pure `reify check` value surface
  ("geometry-consumer builtins … only resolvable on the build()/tessellate() path") yet `reify check`
  still **exits 0** (non-fatal); `reify eval` realizes them for **top-level** structures but yields
  **`undef` for nested `sub`-instances**. ⇒ area-based `coat_cost`/`coat_mass` are observable only on a
  **top-level part** via `reify eval` (or the realized build path), **not** nested in a BOM. **Flat
  `process_cost` has no geometry dependency and rolls up deterministically when nested.**
- **Deep member access through a `sub` is unsupported** (`a.p1.coating.color.r` → "member access not yet
  supported: .coating"). `sub`→`let` access (`self.p1.finishing_cost`, the cost-rollup idiom) works;
  `let p = Plate(); p.coating.thickness` (depth-3 through a `let`) works. ⇒ rollups read through
  `sub`→`let`, not deep `sub`→param chains.

**Conclusion:** every leaf is `grammar_confirmed = true`; **no grammar/unit-producer prerequisite task.**
The Rust seam (§7.3) is value-model work, not `.ri` syntax.

---

## 4. Sketch of approach + resolved design decisions (ratified — not re-litigated)

A new stdlib vocabulary module supplies the spec-bearing types; a Rust seam derives an `Appearance` from
them (the third producer); cost/mass roll up via the existing Money idiom; the two shipped egress surfaces
consume the seam unchanged.

### 4.1 The functional-finish vocabulary (task α — new stdlib module)

New `crates/reify-compiler/stdlib/surface_finish.ri`, registered in `stdlib_loader.rs` **after
`materials_appearance.ri`** (needs `Color`/`Finish`/`Appearance`) **and after `tolerancing.ri`** (so it may
optionally reference `SurfaceFinish` — see §OQ1). Exact validated shapes (§3):

```reify
pub type ArealCostRate = Money / Area            // Money per unit area (areal coating rate)

enum CoatingProcess { Uncoated, Anodize, PowderCoat, Electroplate, Passivate, Paint }
structure def Coating {
    param process      : CoatingProcess = CoatingProcess.Uncoated   // Uncoated sentinel ⇒ additive trait
    param thickness    : Length         = 0um
    param color        : Color          = Color()      // reuse Color → RAL/hex/Pantone via resolve_color
    param spec         : String         = ""           // "MIL-A-8625 Type II Class 2" / "RAL9005 powder"
    param process_cost : Money          = 0USD         // flat per-part finishing cost (deterministic path)
    param cost_per_area: ArealCostRate  = 0USD/m^2     // optional area-based rate (realized path)
    param coat_density : Density         = 0kg/m^3      // for coat_mass on the realized path
}

enum FinishProcess { AsMachined, Ground, Polished, Lapped, BeadBlasted, Brushed, AsCast }

enum TreatmentProcess { Anneal, Temper, CaseHarden, Nitride, Carburize, ShotPeen }
structure def Treatment {
    param process : TreatmentProcess = TreatmentProcess.Anneal
    param spec    : String           = ""
    param cost    : Money            = 0USD
}

trait SurfaceTreated {                               // mirror of Visual; does NOT touch Physical (G4)
    param coating        : Coating       = Coating()
    param finish_process : FinishProcess = FinishProcess.AsMachined
    param treatment      : Treatment     = Treatment()
}
```

All members defaulted ⇒ conformance is **additive** (`Uncoated`/`AsMachined`/`Anneal` are the inert
sentinels, mirroring `Visual.appearance`'s neutral default). The Rust seam navigates `body.coating` /
`body.finish_process` directly off the body `StructureInstance` — the same field-navigation pattern
`resolve_appearance` already uses for `body.material`.

### 4.2 The Appearance-derivation seam (task β — the H contract, Rust)

Extend `crates/reify-eval/src/appearance.rs` with the **functional layer**:
- `coating_appearance(coating: &Value) -> Option<Value>` — `None` for `Uncoated`; else an `Appearance`
  whose `color = resolve_color(coating.color)` and whose `finish`/`metalness`/`roughness` derive from
  `process`: `Anodize`→dark, matte/satin, dielectric; `PowderCoat`/`Paint`→`color`, satin/gloss,
  dielectric; `Electroplate`→metallic (high metalness, low roughness); `Passivate`→near-substrate (subtle).
- `finish_modulation(finish_process)` — modulates the *material's* `Appearance` when there is no coating:
  `Polished`→`Gloss`/low-roughness/high-sheen; `Ground`/`AsMachined`→`Satin`; `BeadBlasted`/`AsCast`→
  `Matte`/high-roughness. **This is the cosmetic-`Finish` subsumption** (the functional process *projects
  onto* the cosmetic `Finish` enum).
- Extend **`resolve_appearance(body)`** (and `resolve_appearance_opt`) with the precedence in §7.3:
  **coating (if present) overrides the material's color/finish; else the material appearance modulated by
  `finish_process`; else the existing material/neutral behavior — unchanged.**

The `Appearance` contract is untouched ⇒ 3MF (#4763) and viewport (#4775) consume the result with **no
change**. This single seam reaching both surfaces is the load-bearing reason this PRD is B+H.

### 4.3 Cost + mass roll-up (task γ — DSL-native)

Reuse the Money `.sum` idiom + `process.ri Process.cost` — **no report CLI** (that is
`io-lifecycle-bom-cost`'s job, G4):
- **Flat (deterministic, primary):** `Coating.process_cost` + `Treatment.cost` → part
  `let finishing_cost : Money = [coating.process_cost, treatment.cost].sum`; assembly
  `let total_finishing_cost : Money = [self.p1.finishing_cost, …].sum`. Nested-safe (no geometry).
- **Area-based + mass (realized path):** `let coat_cost : Money = cost_per_area * area(geometry)`;
  `let coat_mass : Mass = coat_density * area(geometry) * thickness`. Realized at **top level** (§3
  hazard). Observed via `reify eval` / `reify build` realization, not nested-eval.

### 4.4 Egress reflects functional finish (tasks δ, ε — reuse the shipped consumers)

δ (3MF, #4763 seam) and ε (viewport, #4775 seam) are *unchanged consumers* of `resolve_appearance` — once
β makes it functional-aware, both surfaces show the coating-derived look automatically. δ/ε are the
integration gates (B7/B-viewport) proving the subsumption end-to-end.

### Ratified decisions (do not re-litigate)

1. **Reuse + bridge** (Leo 2026-06-29). Reuse tolerancing `SurfaceFinish` (roughness/lay), process.ri
   `SurfaceTreating`/`HeatTreating` (capability), `materials_appearance` (cosmetic contract, unchanged),
   the Money `.sum` idiom. Own only the `Coating` spec, the finish/treatment vocabulary, the part trait,
   the Appearance seam, and the cost/mass roll-up.
2. **`Appearance` contract is unchanged; the functional model is its THIRD producer** (PRD-1 decision 1
   forward-compat invariant). The cosmetic `Finish` is **subsumed by projection** (functional process →
   cosmetic `Finish`), never replaced.
3. **Both cost paths in v1** (Leo): flat `process_cost : Money` (deterministic, nested-safe) **and**
   area-based `cost_per_area : ArealCostRate × area(geometry)` (realized, top-level).
4. **Coating mass in v1** (Leo): `coat_mass = coat_density × area(geometry) × thickness` (realized path).
5. **Treatment is a SHALLOW spec record** (Leo): `Treatment { process, spec, cost }`; cost rolls up; **no**
   mechanical-model coupling (hardness/residual-stress→FEA is a deferred follow-up, §11).
6. **Precedence: an explicit display override beats the functional layer** (Leo). The stack extends PRD-2:
   `hash < material.appearance < functional coating/finish < DisplayOutput.style override < session`
   ("model = overridable default" — a `DisplayOutput.style` is an explicit cosmetic override that still
   wins over the model-derived functional appearance).
7. **`finish_process` (appearance driver) is distinct from tolerancing `SurfaceFinish.process` (metrology
   string).** A part may carry both, independently composable (§OQ1).

---

## 5. Cross-PRD relationship (G4)

| Other PRD / seam | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `appearance-substrate.md` (PRD-1, done) | **extends / consumes** | `resolve_appearance`/`resolve_color` Rust seam + `Color`/`Finish`/`Appearance` types | **PRD-1 owns the contract + seam; this PRD adds the functional producer layer to `resolve_appearance`** | additive — `Appearance` shapes UNCHANGED (decision 2); no contest |
| `appearance-viewport-egress.md` (PRD-2, done) | consumes (downstream) | the viewport reads `resolve_appearance` (#4775); becomes functional-aware for free once β lands | PRD-2 owns the viewport render; **this** owns the functional appearance it now renders | ε reuses the #4775 harness; no new viewport seam |
| `io-export-import-completion.md` / PRD-1 δ (#4763, done) | consumes (downstream) | 3MF `<basematerials>` per-body color via `engine_build` `resolve_instance_color` | #4763 owns the egress; **this** supplies the coating-derived color it writes | δ reuses the #4763 egress; no new export seam |
| `tolerancing-gdt-surface-completion.md` | **reuses (no re-own)** | `SurfaceFinish` (Ra/lay metrology), `require_finish` GD&T callout | **tolerancing-gdt owns the roughness metrology** | this PRD does NOT redefine `SurfaceFinish`; a part may carry a tolerancing callout AND a `SurfaceTreated` coating, composable (§OQ1). No reciprocal ambiguity |
| `process-dfm-completion.md` | **reuses (no re-own)** | `Process.cost`, `SurfaceTreating`/`HeatTreating` (process *capability*) | **process-dfm owns process capability**; **this** owns the part-borne `Coating`/`Treatment` *spec* (with color + cost) | distinct layer ("can process achieve X" vs "this part HAS coating Y"); no contest |
| `io-lifecycle-bom-cost.md` (deferred stub) | **produces (upstream-of)** | per-part `finishing_cost : Money` (+ `coat_mass`) this PRD emits | **this** owns the per-part contribution; **io-lifecycle-bom-cost** owns the design-wide `reify report --bom` aggregation (its blocking G1 consumer) | forward seam — non-orphan because our consumer is the eval'd `total_finishing_cost`/`coat_mass` value (§1.3) |
| `gdt-*` drawing callouts | (deferred) | ISO 1302 surface-finish drawing symbol on a 2D drawing | gdt drawing PRD | out of scope (§11); finish data is here, the drawing symbol is theirs |

No new `engine-integration-norm.md` §3 seam — the functional layer extends the existing post-realization
`resolve_appearance` surface PRD-1 established. No reciprocal-ownership ambiguity; no new contested pair.

---

## 6. G6 — premise validity per leaf signal

| Leaf / signal | Asserted premise | Basis (achievable / true / producible from this leaf's dependency set) |
|---|---|---|
| α vocabulary | `surface_finish_functional.ri` → `reify check` exit 0, **zero** `unresolved type`/`unresolved name` | §3 **verified live** (exit 0). `ArealCostRate = Money/Area` alias resolves; `um` resolves to Length; all enums/structs/trait check. Producible from α alone. No numeric premise. |
| β coating→Appearance | `Coating(process: Anodize, color: Color(named:"RAL9005",…))` → `resolve_appearance(body)` yields a **non-`Undef`** dark `Appearance`; `Polished` finish modulates roughness; **`Appearance` shapes unchanged** | `resolve_appearance`/`resolve_color` ship in **PRD-1 (4761, done)**; β extends them. **Field-population OK** — β writes a real `Appearance` on the production path; `Uncoated`→falls through to material/neutral (no silent black). Branch-3 capabilities all upstream of β. |
| γ flat cost (BOM) | assembly `reify eval` → `total_finishing_cost = N USD` exactly (e.g. 24 USD), nested-safe | Money `.sum` idiom is **eval-proven** (`cost_aggregation.ri` + its locked-total test); §3 evaluated `24 USD` live. **No geometry** ⇒ deterministic when nested. No floor (exact Money arithmetic). |
| γ area cost + mass | top-level part `reify eval` → `coat_cost = cost_per_area·area`, `coat_mass = coat_density·area·thickness` exactly (1.2 USD / 0.0018 kg) | §3 evaluated both live at **top level**. **HAZARD bound in the manifest:** `area()` is kernel-gated → `undef` when nested / errors on pure `check` (still exit 0). ⇒ the area-based signal is asserted on a **top-level** part via `reify eval`, never nested. No tolerance/floor (exact arithmetic). |
| δ 3MF (LEAF) | colored-coating body → 3MF `<basematerials>` RGB = **coating-derived** color (not bare-material grey); `W_3MF_NO_MATERIALS` suppressed | the #4763 egress (`write_3mf` + `resolve_instance_color`→`resolve_appearance`) is **done**; β makes `resolve_appearance` functional-aware; color resolved to **exact** bytes by `resolve_color`. δ-owned over β-upstream + #4763-done. No floor. |
| ε viewport (LEAF) | `Steel_AISI_1045` polished part renders low-roughness/high-sheen; an `Anodize`-coated body renders dark; precedence holds | the #4775 viewport reads `resolve_appearance` (done); β supplies the functional appearance; **override color pinned with explicit rgb** alongside `named` (PRD-2 decision 5 pattern) so the asserted on-screen color is robust to RAL-seed breadth. Render assertion = material-state/screenshot delta (not a solver tolerance). |

**No FEA/solver numeric bound or accuracy floor is asserted anywhere** — G6 branches 1 (numeric bound) and
2 (closed-form exactness) **do not fire**; cost/mass are exact dimensioned arithmetic. Branch 4
(rejection) — N/A (no negative assertion). The substantive premises are α (verified live, §3), β's
field-population (PRD-1-upstream + β-owned), and γ's area-realization hazard (bound + top-level mitigation).

---

## 7. Contract (B + H) — the functional-finish → `Appearance` seam + cost roll-up

The contract β produces, δ + ε + γ consume, specified up front so the integration tasks land first-class.

### 7.1 Vocabulary (the spec-bearing producer surface)
The §4.1 shapes. **Invariants:** every member defaulted (additive conformance); `Uncoated`/`AsMachined`/
`Anneal` are the inert sentinels; `Coating.color` reuses `Color` (RAL/hex/Pantone via `resolve_color`);
`spec` carries the verbatim process/standard string (no parsing). **`Appearance`/`Finish`/`Color` are NOT
modified** (the forward-compat invariant).

### 7.2 `SurfaceTreated` trait
`trait SurfaceTreated { coating : Coating = Coating(); finish_process = AsMachined; treatment = Treatment() }`.
**Invariant:** every conformer yields a `Coating`/`FinishProcess`/`Treatment`; the defaults make
conformance additive (no existing body breaks). Does **not** refine or touch `Physical` (G4 —
ambient-default-material owns the `material` binding).

### 7.3 Appearance-derivation seam (Rust — the load-bearing cross-surface seam)
- `coating_appearance(&Coating) -> Option<Appearance>` — `None` iff `Uncoated`; else a non-`Undef`
  `Appearance` (`color = resolve_color(coating.color)`; finish/metalness/roughness per process).
- `finish_modulation(FinishProcess)` applied to the material `Appearance` when no coating overrides.
- `resolve_appearance(body)` precedence (extends PRD-2's stack, decision 6):

| Channel | session (4) | DisplayOutput.style (3) | **functional coating/finish (NEW)** | material.appearance (2) | hash (1) |
|---|---|---|---|---|---|
| color | recolor/FEA wins | `style.color` | coating color, else material color | material color | `colorForEntity` |
| finish/roughness/metalness | session | `style.finish` | coating PBR, else finish_process modulation | material PBR | defaults |

**Invariants:** coating present ⇒ overrides material color (paint/anodize covers the substrate); no coating
⇒ material color modulated by `finish_process`; neither ⇒ existing material/neutral behavior **unchanged**
(back-compat); an explicit `DisplayOutput.style` still wins over the functional layer (model = overridable
default); session (recolor/FEA) tops all. **Never a silent black** — `resolve_color` is total and loud.

### 7.4 Cost + mass roll-up channel
`finishing_cost : Money` (flat, `[coating.process_cost, treatment.cost].sum`); assembly
`total_finishing_cost : Money` via `.sum` over `sub`→`let`. Area-based `coat_cost : Money` /
`coat_mass : Mass` on a top-level realized part. **Invariant:** flat path nested-safe + deterministic;
area-based path realized-geometry-dependent (top-level / build only — §3 hazard).

### 7.5 Boundary-test sketch (faces producer = vocabulary **and** consumers = 3MF + viewport + eval)

| # | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|
| B1 | vocabulary checks | gate fixture | `surface_finish_functional.ri` → `reify check` exit 0, 0 unresolved (α) |
| B2 | additive conformance | a body conforming to `SurfaceTreated` with no explicit coating | type-checks; `coating.process == Uncoated`; existing bodies unaffected (α) |
| B3 | coating→Appearance | `Coating(process: Anodize, color: Color(named:"RAL9005", r:…))` | `resolve_appearance(body)` → dark non-`Undef` Appearance; color = `resolve_color(RAL9005)` (β) |
| B4 | finish modulation | body, no coating, `finish_process: Polished` | `resolve_appearance` keeps material color, lowers roughness / raises sheen (β) |
| B5 | uncoated fall-through | `SurfaceTreated` body, `Uncoated`, no finish | `resolve_appearance` == the material/neutral result, unchanged (β back-compat) |
| B6 | flat cost BOM | `AssemblyBOM` of two coated/treated parts | `reify eval` → `total_finishing_cost = 24 USD` (deterministic, nested) (γ) |
| B7 | area cost + mass | **top-level** `CoatedPlate(box(100,100,10)mm, cost_per_area:50USD/m², coat_density:3000, thickness:25um)` | `reify eval` → `coat_cost = 1.2 USD`, `coat_mass = 0.0018 kg` (γ, realized) |
| B8 | **3MF egress (δ LEAF)** | box part, `Coating(Anodize, RAL9005)`, `reify build -o x.3mf` | unzip → `3D/3dmodel.model` `<basematerials>` RGB = anodize-derived (not bare grey); no `W_3MF_NO_MATERIALS` (δ) |
| B9 | **viewport egress (ε LEAF)** | dev GUI on a `.ri` with a polished body + an anodize-coated body + a `DisplayOutput.style` override | polished→sheen; anodize→dark; explicit `DisplayOutput.style` overrides functional; session recolor wins (ε) |

B8 + B9 are the integration-gate observable signals (the §1.2 subsumption realized end-to-end on both
surfaces); B6/B7 are γ's; B1–B5 face the producer side.

---

## 8. Pre-conditions for activating (all met)

- **No grammar/unit prerequisite** — §3 G3 PASS; `um` + `ArealCostRate` alias resolve today; every leaf
  `grammar_confirmed = true`.
- **PRD-1 appearance contract + `resolve_appearance`/`resolve_color` seam** — landed (tasks 4760/4761,
  done). β extends it.
- **3MF per-body color egress** — landed (PRD-1 δ, **#4763**, done). δ reuses it.
- **Viewport `resolve_appearance` consumer** — landed (PRD-2, **#4775**, done). ε reuses it.
- **Money `.sum` cost idiom + `process.ri Process.cost`** — landed (tasks 2377/2380/2381 + process-dfm).
- Geometry realization is real (`box(…)` realizes; `area`/`volume` on the build/tessellate path) — γ's
  area path + δ build against it.

---

## 9. Decomposition plan

B+H shape: vocabulary (α) → Appearance seam (β) → cost/mass roll-up (γ) ∥ 3MF gate (δ) → viewport gate (ε).
Greek labels → task IDs at decompose. **Minimal vertical slice (Leo's "prove the pipe first"):**
α → β → **δ** (a real coating's color reaches the 3MF, headless + CI-deterministic) proves the
third-producer subsumption end-to-end before γ (cost breadth) and ε (the second surface).

- **α — functional-finish vocabulary stdlib module.** *Modules:* new
  `crates/reify-compiler/stdlib/surface_finish.ri` (`ArealCostRate`, `CoatingProcess`, `Coating`,
  `FinishProcess`, `TreatmentProcess`, `Treatment`, `SurfaceTreated`) +
  `crates/reify-compiler/src/stdlib_loader.rs` (register **after** `materials_appearance` and
  `tolerancing`). *Intermediate* (unlocks β, γ, δ, ε). *Signal:* a committed
  `examples/surface_finish_functional.ri` (auto-discovered by `examples_smoke`) instantiating the full
  vocabulary + a `Part : SurfaceTreated` passes `reify check` exit 0, **zero** `unresolved type`/
  `unresolved name` (mirrors `docs/prds/v0_6/fixtures/surface_finish_functional.ri`, §3). *Prereqs:* —.
  `grammar_confirmed = true`.
- **β — Appearance-derivation seam (third producer, Rust — the H contract).** *Modules:*
  `crates/reify-eval/src/appearance.rs` (`coating_appearance`, `finish_modulation`, extend
  `resolve_appearance`/`resolve_appearance_opt`) — BRE acquires the footprint. *Intermediate* (unlocks δ,
  ε). *Signal:* reify-eval unit coverage — `Coating(Anodize, RAL9005)` → dark non-`Undef` `Appearance`
  (B3); `Polished` no-coating → roughness-modulated material color (B4); `Uncoated`+no-finish →
  unchanged material/neutral (B5, back-compat); `Appearance` shapes unchanged. Verified end-to-end through
  δ (C-as-integration-gate). *Prereqs:* α, **PRD-1 #4761 (done)**. `grammar_confirmed = true`.
- **γ — cost + mass roll-up (LEAF, DSL-native).** *Modules:* `examples/surface_finish_cost.ri` +
  a locked-value test mirroring `crates/reify-compiler/tests/harness_units_materials/cost_aggregation_tests.rs`. *Leaf.* *Signal:*
  `reify eval` of the committed example yields `total_finishing_cost = 24 USD` (flat, nested BOM,
  deterministic — B6) **and** a top-level `CoatedPlate` yields `coat_cost = 1.2 USD`, `coat_mass =
  0.0018 kg` (area-based realized — B7); a Rust test asserts these exact values. *Prereqs:* α.
  `grammar_confirmed = true`.
- **δ — 3MF egress reflects the coating color (headless LEAF + integration gate).** *Modules:*
  `examples/` (the committed `.ri`) + a CLI e2e test over the #4763 `write_3mf` path — BRE acquires.
  *Leaf — signal = §7.5 B8.* *Signal:* `reify build` of a box whose part has `Coating(process: Anodize,
  color: Color(named:"RAL9005", r:…))`, to `-o x.3mf`, writes a 3MF whose `3D/3dmodel.model`
  `<basematerials>` carries the **anodize-derived** RGB (overriding the bare-material grey), and **no**
  `W_3MF_NO_MATERIALS` for that coated body. *Prereqs:* β, **#4763 (out-of-batch, done)**.
  `grammar_confirmed = true`.
- **ε — viewport egress reflects functional finish (LEAF + integration gate).** *Modules:* `examples/`
  (the committed `.ri`) + the `gui/test/` reify-debug MCP harness (reuse PRD-2 #4775). *Leaf — signal =
  §7.5 B9.* *Signal:* one CI-able scripted `reify-debug` MCP session against a committed `.ri` (a polished
  `Steel_AISI_1045` body + an `Anodize`-coated body + a `DisplayOutput.style` override) asserts: polished →
  low-roughness/high-sheen vs hash; anodize → dark; the explicit `DisplayOutput.style` overrides the
  functional layer (decision 6); a session recolor wins. *Consumer:* end user viewing a finished `.ri`.
  *Prereqs:* β, **#4775 (out-of-batch, done)**. `grammar_confirmed = true`.

DAG: α root; β→α, #4761; γ→α; δ→β, #4763; ε→β, #4775. Vertical slice α→β→δ first; γ (cost) parallels β
off α; ε is the second egress surface.

---

## 10. Capability manifest

Committed beside this PRD at `docs/prds/v0_6/surface-finish-functional.capability-manifest.md` — per-leaf
capability→evidence bindings (anti-orphan/wired, DAG-direction, field-population, grammar-fixture,
numeric-floor + the `area()` kernel-gating hazard). Any FAIL binding blocks the batch.

---

## 11. Out of scope (named — future PRDs)

- **Treatment ↔ mechanical/material-model coupling** (heat-treat → hardness; shot-peen → residual stress →
  FEA). `Treatment` is a shallow spec record in v1 (decision 5); the coupling is a deferred follow-up
  (touches the load-bearing FEA/material seam, and has no hardness/residual-stress consumer today).
- **Design-wide BOM/cost report** (`reify report --bom`) — owned by `io-lifecycle-bom-cost.md` (its
  blocking G1 consumer). This PRD ships the per-part `finishing_cost`/`coat_mass` contribution it will
  aggregate.
- **GD&T surface-finish drawing-callout symbol** (the ISO 1302 triangle on a 2D drawing) — `gdt-*`. The
  finish *data* is here; the drawing *symbol* is theirs.
- **STEP material+finish export** (AP242 surface-treatment entities) — big lift; 3MF color (#4763) is the
  v1 export surface. Richer 3MF metadata (coating spec as `<basematerials>` name) is a §OQ.
- **Coating as a geometry operation** (offsetting/growing the solid by `thickness`) — v1 `Coating` is a
  spec annotation, not a geometry op.
- **Widening `Color`'s named-standard table** beyond hex + the PRD-1 RAL seed (Pantone resolves via
  `named` + `W_UNKNOWN_COLOR_NAME` rgb fallback) — tactical, PRD-1 §OQ1.

---

## 12. Open questions (tactical — deferred, not design-blocking)

1. **Part-level Ra spec reuse.** Should `SurfaceTreated` also carry a `surface_finish : SurfaceFinish` ref
   (reusing the tolerancing roughness metrology) so a part's Ra spec lives with its coating, or keep them
   independently composable (a part carries a tolerancing `require_finish` callout AND a `SurfaceTreated`
   coating)? *Suggested:* independently composable in v1 (avoids load-order coupling + a forced
   `SurfaceFinish` default; `finish_process` already drives appearance). Decide during α.
2. **Coating PBR projection table.** The exact `process → (finish, metalness, roughness)` map (e.g. does
   `Electroplate` set metalness 0.9 / roughness 0.1?). *Suggested:* an editorial table in β mirroring
   PRD-1 γ's editorial library appearances; tune in β. Decide during β.
3. **`finish_process` → cosmetic `Finish` granularity.** Does `finish_process` set the cosmetic `Finish`
   enum *and* nudge roughness, or only roughness/sheen? *Suggested:* both (Polished→`Gloss`+low-roughness),
   mirroring PRD-2 §OQ1. Decide during β.
4. **Richer 3MF metadata.** Emit `Coating.spec` as the 3MF `<basematerials>` *name* (not just color)?
   *Suggested:* color-only in v1 (δ); spec-as-name a nice-to-have follow-up. Decide during δ.
5. **Area-based cost surfacing.** Beyond top-level `reify eval`, should `coat_cost`/`coat_mass` be surfaced
   through `reify build` output / the GUI mass readout for nested parts (where `area()` realizes)?
   *Suggested:* v1 asserts top-level `reify eval` (B7); the nested/build surfacing is a follow-up. Decide
   during γ.
