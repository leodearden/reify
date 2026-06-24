# PRD: Appearance substrate — material-driven visual appearance

**Milestone:** v0_6 · **Status:** active (decompose-ready) · **Date:** 2026-06-24
**Approach:** B + H (stdlib/value-model producer + trait-conformance seam + two-way boundary tests) — multi-crate types→resolution→egress seam consumed by ≥2 downstream PRDs.
**Batch:** PRD-1 of 3 under the `io-display-output-viewport` expansion (umbrella tracker **task 4291**).
PRD-2 `appearance-viewport-egress` (authored *after* this, depends on this PRD's types) and PRD-3
`multi-pane-viewport` (authored *concurrently*) are siblings.
**Extends:** `docs/prds/v0_6/io-export-import-completion.md` §4.2 — closes the stubbed
`include_colors` / `W_3MF_NO_MATERIALS` honest-gate (task #4286, landed) with real per-body color.

---

## 0. Thesis

Materials in reify today are **analysis-only** (`density`, `youngs_modulus`, FEA props) and carry **no
visual appearance**; the GUI colors every body by an arbitrary deterministic hash
(`gui/src/viewport/meshManager.ts:43` `colorForEntity` → Catppuccin `ACCENT_PALETTE[hash]`). This PRD
adds a first-class **`Appearance`** descriptor (color + cosmetic finish + PBR metalness/roughness) and
makes **materials** its first producer, so a body *made of* a material has a characteristic,
version-controlled look (steel → mid-dark neutral grey, slight satin sheen; aluminium → lighter grey;
ABS → matte off-white). `Appearance` is the **stable, source-agnostic, consumer-facing contract**; the
viewport consumer is PRD-2, and the **headless** consumer in *this* PRD is the 3MF color export.

---

## 1. Goal — what a user observes (G2)

Two headless, CI-testable leaves (no GUI dependency):

1. **Types parse + type-check.** A `.ri` instantiating `Color`, `Finish`, `Appearance`, and a material
   carrying an `Appearance` passes `reify check` (exit 0; **zero** `unresolved type` / `unresolved
   name`). The committed gate fixture `docs/prds/v0_6/fixtures/appearance_surface.ri` already
   demonstrates this (§3).
2. **Material color reaches a real file.** `reify build part.ri -o part.3mf` for a body whose material
   has a color writes **real per-body color** into the 3MF — the parent PRD's stubbed `include_colors`
   path stops emitting `W_3MF_NO_MATERIALS` for that body and writes actual color data into
   `3D/3dmodel.model` (a 3MF `<basematerials>` / per-object color resource). This closes the honest-gate
   `io-export-import-completion.md` §4.2 left open (task #4286).

The GUI payoff — recoloring the viewport from `Appearance` instead of the hash — is **PRD-2**, which
consumes the types and the resolution seam this PRD ships.

---

## 2. Background — verified substrate (re-verified 2026-06-24; file:line drifts)

- **`Material` is a minimal struct with no defaults:**
  `structure def Material { param name : String; param density : Density; param youngs_modulus : Pressure }`
  — `crates/reify-compiler/stdlib/materials_mechanical.ri:73`. "Callers must supply values at
  construction; there are no defaults." ⇒ adding `param appearance : Appearance = Appearance()` (a
  *defaulted* member) is **back-compatible**: existing `Material(name:, density:, youngs_modulus:)` ctors
  keep type-checking. The `MaterialSpec` trait (`:61`) is the non-struct material contract.
- **Library = 4 FEA materials, as `: ElasticMaterial` structures (NOT `Material` instances):**
  `Steel_AISI_1045` (`materials_fea.ri:154`), `Aluminium_6061_T6` (`:192`), `Titanium_Ti6Al4V`
  (`:230`), `ABS_Plastic` (`:271`). `steel_1080_hot_rolled` does **not** exist (the skeleton's
  example was aspirational). `MaterialPropertyProvenance` (`:58`, all-`String` fields) is the
  **String-with-downstream-validation** precedent for `Color.named`.
- **Body→material binding:** the `Physical` trait carries `param material : Material`
  (`structural_physical.ri:45`); material is read via field access (`body.material.density`). Owned by
  `ambient-default-material.md` — this PRD does **not** re-own it (G4).
- **No appearance/visual field exists anywhere** in any material trait/struct today.
- **3MF honest-gate (landed, task #4286, merged `84da8ea`):** `ThreeMFOutput` declares
  `include_materials`/`include_colors` (`io.ri:149-150`); `write_3mf(&Mesh, ThreeMfOptions, &mut Write)`
  (`reify-ir/src/geometry.rs:2569`) with `ThreeMfOptions { include_materials, include_colors }`
  (`:2517-2520`) emits `ThreeMfWarning::NoMaterials` → `"W_3MF_NO_MATERIALS"` (`:2537`) and writes
  geometry-only when either flag is set but no material/color data is present (`:2692`). The mesh egress
  carries **no per-body color today** — that is exactly what task δ adds.
- **Grammar/semantic precedents (all re-confirmed):** `enum` + qualified-enum defaults
  (`STEPVersion.AP214`, `io.ri:119/131`); struct-ctor defaults (`DisplayStyle()`, `Provenance(...)`,
  `examples/io_formats.ri:19`); traits carrying `param … = default`
  (`trait TemperatureDependent { param reference_temperature : Temperature = 293.15K }`,
  `materials_mechanical.ri:48`); multi-trait conformance `: A + B`
  (`structure def Prismatic : DrivingJoint + HasMotion`, `kinematic.ri:128`); chained-comparison
  constraints (`constraint 0 < poissons_ratio < 0.5`, `materials_mechanical.ri:91`).
- **`materials_optical.ri`** (`refractive_index`, `transmittance`) is the **physical** optics path —
  explicitly **NOT** the cosmetic appearance path (decision 5).
- **stdlib load order** is controlled by `stdlib_loader.rs` (materials_mechanical at `:56`,
  materials_optical `:68`, materials_fea `:80`). The new appearance module must register **before**
  materials_mechanical so `Appearance`/`Visual` are in scope when `Material : Visual` (β) and the
  library `: … + Visual` (γ) compile.

---

## 3. G3 gate — grammar **and** semantic, empirically validated (PASS)

Leo's open gate: prove the chosen `Color` shape — **rgb + optional named standard** (the both-fields
encoding, the only G3 risk) — parses **and** type-checks before committing to it. Validated 2026-06-24
against `target/release/reify` (built this session) and `tree-sitter-reify`:

Committed fixture **`docs/prds/v0_6/fixtures/appearance_surface.ri`** →
`tree-sitter parse --quiet` exit 0 (0 ERROR nodes) **and** `reify check` exit 0
("All constraints satisfied.", **zero** `unresolved type` / `unresolved name`).

| Fragment exercised | Verdict |
|---|---|
| `enum Finish { Matte, Satin, Gloss }` | ✅ |
| `structure def Color { param named : String = ""; param r/g/b : Real = 0.0 }` (both-fields) | ✅ |
| `structure def Appearance { param color : Color = Color(r:0.7,g:0.7,b:0.7); param finish : Finish = Finish.Satin; param metalness/roughness : Real }` | ✅ |
| `trait Visual { param appearance : Appearance = Appearance() }` (trait carrying a struct-ctor default) | ✅ |
| material carrying it: `structure def DemoMaterial : Visual { … appearance : Appearance = Appearance(color: Color(named:"RAL7035", r:…, g:…, b:…), finish: Finish.Satin, metalness:…, roughness:…) }` (nested ctor args) | ✅ |
| multi-trait: `structure def DemoAlloy : DemoElastic + Visual { … }` | ✅ |
| instantiation + member-access read: `let m = DemoMaterial(name:"steel"); let cr = m.appearance.color.r` | ✅ |

**Conclusion:** commit to the both-fields `Color`. The skeleton's fallback (`color : String`,
named+hex only) is **not** needed. The member-access read even type-checks (unlike io-export's
`determined(subject.geometry)` — a constraint-call-argument context, not a plain field read), so the
body→appearance read is available at the DSL level as well as Rust-side. **No new grammar work; no
grammar-producer prerequisite task.** `grammar_confirmed = true` for every leaf.

---

## 4. Sketch of approach + resolved design decisions (ratified — not re-litigated)

### 4.1 The appearance vocabulary (task α — new stdlib module)

New `crates/reify-compiler/stdlib/materials_appearance.ri`, registered in `stdlib_loader.rs` **before**
`materials_mechanical.ri`. Exact validated shapes (from §3):

```reify
enum Finish { Matte, Satin, Gloss }              // cosmetic only (v1); see capstone

structure def Color {
    param named : String = ""        // "" = unset; else "#RRGGBB" or a named standard ("RAL7035")
    param r : Real = 0.0
    param g : Real = 0.0
    param b : Real = 0.0
}   // resolution rule: named non-empty → hex/RAL lookup → rgb; else use r/g/b (task β, Rust-side)

structure def Appearance {
    param color     : Color  = Color(r: 0.7, g: 0.7, b: 0.7)  // neutral grey default (not (0,0,0))
    param finish    : Finish = Finish.Satin
    param metalness : Real   = 0.0     // 0 dielectric … 1 metal
    param roughness : Real   = 0.5     // 0 mirror … 1 fully diffuse
}

trait Visual {
    param appearance : Appearance = Appearance()
}
```

`Visual` lives **with** its `Appearance` type (one cohesive module) — a small, deliberate refinement of
the skeleton's "Visual in β" so α is a complete, independently-checkable contract vocabulary and the
gate fixture maps 1:1 to the shipped surface. β then only *consumes* `Visual`.

### 4.2 `Material : Visual` + the Rust resolution seam (task β — the H contract)

- **`Material : Visual`** in `materials_mechanical.ri`, gaining `param appearance : Appearance =
  Appearance()` (defaulted ⇒ back-compatible, §2). The default's neutral-grey color means an
  un-styled body renders sanely (strictly better than the arbitrary hash) — no silent black.
- **`resolve_color(&Color) -> Rgb8`** (Rust): `#RRGGBB` / `#RGB` hex parsed **exactly**; a small **RAL
  Classic seed** table; an unknown non-empty `named` → **`W_UNKNOWN_COLOR_NAME`** diagnostic + fall
  back to the `(r,g,b)` fields (clamped [0,1]→[0,255]). Empty `named` → use `(r,g,b)`. **Total** (always
  returns a color) and **loud on the unknown-name path** — no silent-default-to-black
  (`feedback_silent_defaults_pattern`).
- **`resolve_appearance(body) -> Appearance`** (Rust): reads `body.material.appearance` (the `Visual`
  member), the neutral default when unset. This is the **single seam** δ (3MF) and PRD-2 (viewport)
  both consume — the load-bearing reason this PRD is B+H.

### 4.3 Library-wide appearance (task γ)

Each FEA library material gains `: ElasticMaterial + Visual` + an **editorial** `appearance` member
(explicit `(r,g,b)`, `named=""` — so γ does **not** depend on the RAL seed breadth): steel grey-satin,
aluminium light-grey, titanium grey-satin, ABS matte off-white. The `Material` struct's default
appearance (neutral grey) is set in β. Values are **editorial, not physically derived** — the physical
optics path (`materials_optical.ri`) is explicitly separate (decision 5).

### 4.4 3MF per-body color egress (task δ — headless leaf + H integration gate)

Closes `W_3MF_NO_MATERIALS` for colored bodies. Over the landed `write_3mf` (#4286): (1) add an optional
per-body color channel to the mesh egress (`reify-ir`); (2) populate it in `engine_build` from
`resolve_appearance(body).color` via β's seam; (3) extend `write_3mf` to emit a 3MF
`<basematerials>` / per-object color resource when color is present and **suppress** `W_3MF_NO_MATERIALS`
for that body. When no color is present, the existing geometry-only + warning behavior is unchanged.
Wired on both the imperative `-o foo.3mf` path and the declarative `ThreeMFOutput` driver (#4287).

### Ratified decisions (do not re-litigate)

1. **`Appearance` is the stable, source-agnostic contract.** Consumers read an `Appearance`; they never
   read material internals. The deferred functional-finish **capstone** becomes a *second producer* of
   `Appearance` and subsumes the cosmetic `Finish` into cosmetic+functional — so `Appearance` is
   designed forward-compatible (no producer identity baked into the type).
2. **Cosmetic-only finish in v1.** `Finish { Matte, Satin, Gloss }` is a pure look (maps to
   roughness/sheen downstream); no manufacturing-spec semantics. Real surface finish/treatment/coating
   is the deferred capstone.
3. **`Visual` trait resolves the Material-vs-library fragmentation.** Both the `Material` struct and the
   library `: ElasticMaterial` structures implement `Visual`, so "what is this made of" yields an
   `Appearance` uniformly. (Trait, not bare field — parallels how `ElasticMaterial` is a trait the
   library already implements. Confirmed by §3.)
4. **`Color` = rgb + optional named standard** (the both-fields encoding, §3-validated). Named standards
   (RAL at minimum) + hex resolved by `resolve_color`; the seed-table breadth is tactical (§OQ) — start
   with hex + a small RAL Classic seed, the rest is out of scope.
5. **Library-wide appearance, editorial.** Every library material gets a plausible default `Appearance`;
   values are editorial, **not** derived from `materials_optical.ri` (which is the *physical* optics
   path, not the cosmetic one).

---

## 5. Cross-PRD relationship (G4)

| Other PRD / seam | Direction | Mechanism | Owner | Status |
|---|---|---|---|---|
| `ambient-default-material.md` (active) | consumes | `Material` concept + `Physical.material` binding + `default Material =` scope injection | **ambient-default** owns the binding/scoping; **this** adds the `appearance` dimension to `Material`/`Visual` | additive, no contest — `Material`'s defaulted `appearance` does not perturb param-injection (injection precedes member compilation) |
| `io-export-import-completion.md` (landed, #4286) | extends | 3MF `include_colors` / `W_3MF_NO_MATERIALS` honest-gate; `write_3mf` | **this** PRD supplies the per-body color egress that closes it | δ wires real color over the landed `write_3mf` (hard dep δ → #4286, done) |
| PRD-2 `appearance-viewport-egress` (sibling, authored *after* this) | produces | `Appearance` types + `resolve_appearance`/`resolve_color` seam + material→appearance source | **this** owns the types + resolution + material source; **PRD-2** consumes them to recolor the viewport | hard dep PRD-2 → this PRD (α, β); PRD-2 wires the edges when authored |
| PRD-3 `multi-pane-viewport` (sibling, authored *concurrently*) | independent | pane model — no appearance-type dependency | distinct | no seam |
| `io-display-output-viewport.md` stub / tracker **task 4291** | upstream-of | the appearance contract this batch expands the tracker into | this batch is *upstream* of 4291 (PRD-2 activates the viewport drive) | no dep edge from this PRD's tasks → 4291 |
| Functional surface-finish **capstone** (deferred [MILESTONE]) | produces | the `Appearance` contract; cosmetic `Finish` subsumed into cosmetic+functional later | **this** ships cosmetic; capstone re-owns finish | forward-compat seam (decision 1) |
| `materials_optical.ri` | sibling | physical optics (refractive index) ≠ cosmetic appearance | distinct | noted, no overlap |

No reciprocal-ownership ambiguity; no new engine-integration-norm §3 seam (the 3MF egress extends the
existing post-realization `build()` / terminal-repr surface that #4286 established). No new norm doc.

---

## 6. G6 — premise validity per leaf signal

| Leaf | Asserted premise | Basis (achievable / true / producible from this leaf's deps) |
|---|---|---|
| α types | `appearance_surface.ri` → `reify check` exit 0, **zero** `unresolved type`/`unresolved name` | §3 **verified** (exit 0, "All constraints satisfied"). Producible from α alone (the types). No numeric premise. |
| β `resolve_color` | `Color(named:"#8899AA")` → `(0x88,0x99,0xAA)` exactly; unknown name → `W_UNKNOWN_COLOR_NAME` + rgb fallback | hex→rgb is a parse identity (**exact**, not a tolerance). Rejection path is rejection-mechanism-backed (the warning is authored + observed, §8 B4). No numeric floor. |
| β `Material.appearance` | `Material(name:,density:,youngs_modulus:)` (no appearance) still type-checks; `Appearance()` constructs a **non-`Undef`** value | `Material` has no defaults today (§2) so the new defaulted member is additive; struct-ctor + ctor-default eval verified working (`Material(...)` ctor — ambient-default-material 2026-06-10; `DisplayStyle()`/`Provenance()` defaults — io_formats). **Field-population OK** (real value, not `Undef`). |
| γ library | each library material `.appearance` reads a **non-`Undef`** editorial `Appearance` | structure member-default eval works (the `MaterialPropertyProvenance` defaults are the precedent). Producible from α + β. |
| δ 3MF (LEAF) | colored body → 3MF carries real per-body RGB; `W_3MF_NO_MATERIALS` **suppressed** for that body | `write_3mf` + the gate landed (#4286, `geometry.rs:2569/2692`); 3MF `<basematerials>`/color is a fixed OPC schema; color resolved to **exact** bytes by β. **Field-population:** δ's egress writes a real RGB on the production path (`engine_build` mesh egress) — the deliverable, **not** owned downstream (β upstream; #4286 done). No numeric floor (exact bytes; no AABB/solver tolerance asserted). |

No FEA/solver numeric bound is asserted anywhere in this PRD — branches G6.1/G6.2 do not fire. The two
substantive premises are α (verified live, §3) and δ's field-population (β-upstream + δ-owned egress).

---

## 7. Contract (B + H) — the `Appearance` stable contract

The contract PRD-2 reads without re-litigation, and δ + the materials produce.

### 7.1 Data shapes (the consumer-facing contract)
`enum Finish { Matte, Satin, Gloss }`; `structure def Color { named: String, r/g/b: Real }`;
`structure def Appearance { color: Color, finish: Finish, metalness: Real, roughness: Real }`.
**Invariant:** source-agnostic — no producer identity is encoded in `Appearance`, so the functional-finish
capstone can become a second producer (decision 1). `metalness`/`roughness` are PBR scalars in [0,1]
(range *not* enforced in v1 — §OQ); `finish` is cosmetic-only (decision 2).

### 7.2 `Visual` trait
`trait Visual { param appearance : Appearance = Appearance() }`. **Invariant:** every `Visual` conformer
yields an `Appearance`; `Material` and the 4 library materials conform. The defaulted member makes
conformance additive (no existing conformer breaks).

### 7.3 Resolution seam (Rust — the load-bearing cross-PRD seam)
- `resolve_color(&Color) -> Rgb8`. **Invariants:** total (always returns a color); `#RRGGBB`/`#RGB` parsed
  exactly; a seeded RAL Classic name → its tabled RGB; an unknown non-empty `named` → `W_UNKNOWN_COLOR_NAME`
  + `(r,g,b)` fallback (**loud, never silent black**); empty `named` → `(r,g,b)`.
- `resolve_appearance(body) -> Appearance`. **Invariants:** a body with a `Material` yields that
  material's `Appearance`; the neutral-grey default when unset. **This is the single seam δ and PRD-2
  both consume** — specified here so neither re-derives it.

### 7.4 Egress channel
The mesh egress (`reify-ir`) gains an **optional** per-body color (`Option<Rgb8>`); `engine_build`
populates it from `resolve_appearance`; `write_3mf` emits a `<basematerials>`/per-object color resource
when present. **Invariant:** color present ⇒ written + `W_3MF_NO_MATERIALS` suppressed for that body;
color absent ⇒ existing geometry-only + warning behavior **unchanged** (back-compat).

### 7.5 Boundary-test sketch (faces producer = materials **and** consumer = 3MF egress + PRD-2 seam)

| # | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|
| B1 | types check | gate fixture | `appearance_surface.ri` → `reify check` exit 0, 0 unresolved (α) |
| B2 | `Material` back-compat | `Material(name:,density:,youngs_modulus:)`, no appearance | still type-checks; `.appearance` = neutral grey; existing material tests stay green (β) |
| B3 | `resolve_color` hex | `Color(named:"#8899AA")` | `resolve_color` → `(0x88,0x99,0xAA)` exactly (β) |
| B4 | `resolve_color` RAL + unknown | `Color(named:"RAL9006")` / `Color(named:"RALZZZZ")` | seeded → tabled RGB; unknown → `W_UNKNOWN_COLOR_NAME` + rgb fallback (β) |
| B5 | body→appearance | body with `Material(appearance: Appearance(color: Color(r:0.4,g:0.4,b:0.42)))` | `resolve_appearance(body).color` resolves to that RGB (β) |
| B6 | library appearance | `Steel_AISI_1045()` | `.appearance` reads non-`Undef`, characteristic grey (γ) |
| B7 | **3MF color egress (δ LEAF / integration gate)** | box body, material with a color, `reify build -o x.3mf` | unzip → `3D/3dmodel.model` has a `<basematerials>`/color resource with the body's RGB; **no** `W_3MF_NO_MATERIALS` for that body (δ) |
| B8 | 3MF no-color back-compat | box, `include_colors=true`, no material color | `W_3MF_NO_MATERIALS` still emitted; geometry written (δ — unchanged) |
| B9 | PRD-2 seam (forward, consumer-facing) | `resolve_appearance`/`resolve_color` shipped | PRD-2 viewport reads per-body `Appearance` via the seam (verified when PRD-2 lands) |

B7 is δ's integration-gate observable signal (closes G2's loop); B9 faces the PRD-2 consumer side.

---

## 8. Pre-conditions for activating

- **No grammar prerequisite** — §3 G3 PASS; every leaf `grammar_confirmed = true`.
- **`write_3mf` + `W_3MF_NO_MATERIALS` honest-gate** — landed (task #4286, merged `84da8ea`). δ has a
  hard dep on it (already `done`).
- **`Material` struct + `Physical.material` binding** — present today (§2); no dependency on the
  `default Material =` mechanism (orthogonal; `ambient-default-material.md` owns it).
- Geometry realization is real (`box(…)` realizes today) — δ's leaf builds against it.

---

## 9. Decomposition plan

Greek labels → task IDs at decompose. **Active batch (flip to pending): α β γ δ.** Minimal vertical
slice (per Leo): **α → β → (one material) → δ** proves the pipe end-to-end before γ's library breadth —
the "one material" is an inline `Material(appearance: …)` in δ's fixture (needs only α + β).

- **α — appearance vocabulary stdlib module.** *Modules:* new
  `crates/reify-compiler/stdlib/materials_appearance.ri` (`Color`, `Finish`, `Appearance`, `Visual`) +
  `crates/reify-compiler/src/stdlib_loader.rs` (register **before** materials_mechanical). *Intermediate*
  (unlocks β, γ, δ; cross-PRD unlocks PRD-2). *Signal:* a committed example
  (`examples/appearance_surface.ri`, auto-discovered by `examples_smoke`) instantiating
  `Color`/`Finish`/`Appearance` + a struct carrying `Appearance` passes `reify check` exit 0, **zero**
  `unresolved type`/`unresolved name` (mirrors `docs/prds/v0_6/fixtures/appearance_surface.ri`, §3).
  *Prereqs:* —. `grammar_confirmed=true`.
- **β — `Material : Visual` + Rust resolution seam.** *Modules:* `materials_mechanical.ri`
  (`Material : Visual` + defaulted `appearance`); Rust `resolve_color` + `resolve_appearance` (reify-eval
  / reify-ir — BRE acquires the footprint). *Intermediate* (unlocks γ, δ; cross-PRD unlocks PRD-2).
  *Signal (intermediate):* unlocks δ (the 3MF leaf) and PRD-2; verified end-to-end through δ
  (C-as-integration-gate) plus reify-eval unit coverage of `resolve_color` (B3/B4) and back-compat
  (B2). *Prereqs:* α. `grammar_confirmed=true`.
- **γ — library-wide appearance.** *Modules:* `crates/reify-compiler/stdlib/materials_fea.ri` (the 4
  materials gain `: ElasticMaterial + Visual` + editorial `appearance`) + a CI example. *Leaf.* *Signal:*
  a committed `examples/material_appearance_library.ri` (runs in CI via `examples_smoke`) instantiating
  each library material; each `.appearance` reads a non-`Undef` `Appearance` with its characteristic
  color (B6). *Prereqs:* α, β. `grammar_confirmed=true`.
- **δ — 3MF per-body color egress (headless LEAF + H integration gate).** *Modules:*
  `reify-ir/src/geometry.rs` (mesh color channel + `write_3mf` color resource),
  `reify-eval/src/engine_build.rs` (populate color via `resolve_appearance`) — BRE acquires the footprint.
  *Leaf — signal = §7.5 B7.* *Signal:* `reify build` of a box whose material has a color, to `-o x.3mf`,
  writes a 3MF whose `3D/3dmodel.model` carries a `<basematerials>`/color resource with the body's RGB,
  and **no** `W_3MF_NO_MATERIALS` is emitted for that colored body (B8 confirms the no-color path is
  unchanged). *Prereqs:* α, β, **#4286** (out-of-batch, done). `grammar_confirmed=true`.

DAG: α root; β→α; γ→α,β; δ→α,β + (out-of-batch) #4286. Vertical slice α→β→δ; γ is breadth after the slice.

---

## 10. Out of scope (named)

- The GUI viewport render path / recolor-from-Appearance (**PRD-2** `appearance-viewport-egress`).
- The multi-pane model (**PRD-3** `multi-pane-viewport`).
- **Functional/real** surface finish, coating, treatment (deferred capstone — decision 1/2).
- Texture maps (UVs + asset pipeline).
- Physically-derived appearance from optical material properties (`materials_optical.ri` stays the
  separate physical path).
- Widening `Color`'s named-standard table beyond hex + a small RAL Classic seed (tactical — §OQ).
- Per-vertex / per-face color (only per-body color in v1).
- Range *enforcement* on `metalness`/`roughness`/rgb (downstream clamps; §OQ).

---

## 11. Open questions (tactical — deferred, not design-blocking)

1. **RAL seed breadth.** Hex always + which RAL Classic entries? *Suggested:* the handful the editorial
   library defaults could reference + a documented "unknown → `W_UNKNOWN_COLOR_NAME`" path; widen later.
   Decide during β.
2. **Range enforcement.** Add `constraint 0 <= metalness <= 1` etc. (chained comparison parses, §2) on
   the `structure def`s, or clamp downstream only? *Suggested:* clamp in `resolve_*` (loud) for v1; revisit
   adding `structure def` constraints once their support is confirmed. Decide during α/β.
3. **3MF color granularity.** `<basematerials>` (one material per body) vs a per-object `<color>` —
   either satisfies B7. *Suggested:* `<basematerials>` (most-portable). Decide during δ.
4. **`finish` → roughness mapping.** Does cosmetic `Finish` drive `roughness` (Matte→high, Gloss→low) at
   resolution time, or stay an independent channel in v1? *Suggested:* independent in v1; the capstone
   owns the functional mapping. Decide during β.
