# PRD: Appearance viewport egress — material + DisplayOutput style → the live viewport

**Milestone:** v0_6 · **Status:** active (design-first) · **Date:** 2026-06-24
**Approach:** **B + H** (engine consumer + IPC payload extension + frontend render + two-way boundary tests) —
multi-crate types→snapshot→IPC→Three.js seam, the user-visible payoff of the appearance batch.
**Umbrella:** task **4291** (`io-display-output-viewport.md` forward-stub). **Batch:** PRD-2 of 3
(PRD-1 `appearance-substrate`, PRD-3 `multi-pane-viewport`, PRD-2 this). **Hard-depends on PRD-1's
`Color`/`Finish`/`Appearance` types + `resolve_appearance`/`resolve_color` seam; shares the GUI-snapshot
`: Output` Display-walk with PRD-3** (style here, pane there).

---

## 0. Thesis

This is the **user-visible payoff** PRD of the appearance batch: it carries the body's material
`Appearance` (PRD-1) and the `DisplayOutput.style` **display override** across the
engine → Tauri → Three.js boundary so the GUI viewport shows model-driven appearance instead of the
arbitrary deterministic hash color. Two channels reach the renderer: the **material appearance** rides
**on** `MeshData` (a per-mesh field, like `scalar_channels`); the **`DisplayOutput.style` override** rides
in a **separate `GuiState.display_appearance` map** keyed by subject `entity_path` (a model directive,
exactly like PRD-3's `display_panes`). The Three.js layer merges them with **session/interactive**
recolor under a four-layer **precedence stack**. The headless CLI export driver
(`I_DISPLAY_OUTPUT_DEFERRED`) correctly stays a no-op — this PRD drives the *GUI snapshot* path.

---

## 1. Goal — what a user observes (G2)

Open a `.ri` in the dev GUI:

1. **Material appearance renders.** A body made of a library material (`Steel_AISI_1045`) renders with that
   material's appearance — mid-dark neutral grey with a slight satin sheen — instead of the arbitrary
   Catppuccin hash color (`colorForEntity`).
2. **`DisplayOutput.style` overrides.** A
   `DisplayOutput(subject: part, style: DisplayStyle(color: Color(named:"RAL9001", r:0.96, g:0.95, b:0.88), finish: Finish.Gloss, opacity: 0.5, wireframe: false))`
   overrides that body to the RAL9001 cream gloss at 0.5 opacity — model-encoded, version-controlled.
   *(The override color is pinned with explicit rgb alongside the name so the asserted on-screen color does
   not depend on PRD-1's tactical RAL-seed breadth — see §9 and PRD-1 §OQ1.)*
3. **Precedence holds.** A manual interactive recolor / FEA colorize on that body still wins over the model
   default (session > model); removing the `DisplayOutput` falls back to the material appearance; a body
   with **no** material falls back to the hash color.

The observable signal is a single CI-able scripted `reify-debug` MCP session against a committed `.ri`
(task ε, §8) asserting the steel-grey default, the RAL9001/Gloss/0.5 override, and the three precedence
cases via per-mesh material-state / screenshot deltas.

---

## 2. The layered precedence stack (ratified — Leo Q2 "model = overridable default")

```
hash-default color        (no material → colorForEntity, meshManager.ts:43)   ← lowest precedence
  └ material appearance     (PRD-1: steel → grey satin; carried ON MeshData.appearance, task β)
      └ display style override   (DisplayOutput.style: color / finish / opacity / wireframe;
      │                           carried in GuiState.display_appearance map, task γ)
          └ session / interactive  (manual recolor, FEA colorize, visibility; existing meshManager paths)  ← wins
```

The frontend (task δ) computes each mesh's effective material by walking this stack top-down: the highest
layer that supplies a value wins per channel. The two model layers are **distinct transport channels** —
material appearance is intrinsic to the mesh (per-mesh `MeshData` field), the display override is a
model-level directive (subject-keyed `GuiState` map). They are merged client-side, not at the engine.

---

## 3. Background — verified substrate (re-verified 2026-06-24; file:line drifts — prefer named symbols)

- **The recognize-but-skip is in the HEADLESS CLI export driver, a DIFFERENT consumer.** `build_outputs`
  → `OutputTarget::DisplayDeferred` → `I_DISPLAY_OUTPUT_DEFERRED` info diagnostic + `continue`
  (`build_outputs_with_result`, `crates/reify-eval/src/engine_build.rs:3889`). A headless `reify build`
  has no viewport; **that skip correctly stays a no-op.** This PRD drives the *GUI snapshot* path.
- **GUI snapshot path (the real consumer):** `tessellate_snapshot` (`engine_build.rs:8199`) →
  `TessellateResult { meshes: Vec<MeshSurface>, values: ValueMap, … }` → `build_gui_state`
  (`gui/src-tauri/src/engine.rs:2416`) maps `Vec<MeshSurface>` → `Vec<MeshData>` (the `.map(|surface| MeshData {…})`
  at `engine.rs:2579`) and **already holds `result.values`** — the evaluated `ValueMap` needed to walk
  `DisplayOutput` occurrences. Transported over the Tauri `mesh-update` / snapshot channel →
  `meshManager.sync()` → Three.js. Precedent for an engine→GUI display hint: `MeshSurface.default_visible`.
- **`MeshData` (Rust, `gui/src-tauri/src/types.rs:363`)** has 9 fields — `entity_path`, `vertices`,
  `indices`, `normals`, `scalar_channels`, `displaced_positions`, `element_kind`, `region_tags`,
  `vector_channels` — **no style/appearance field.** `validate()` (`:446`) length-checks each per-mesh
  channel; a new optional appearance field is additive. The TS mirror is `interface MeshData`
  (`gui/src/types.ts:7`) + the `raw → typed` converter (`:114`).
- **`DisplayStyle { opacity : Real = 1.0; wireframe : Bool = false }`** (`stdlib/io.ri:122`) +
  `occurrence def DisplayOutput : Output { param subject : Solid; param pane : Int = 0;
  param style : DisplayStyle = DisplayStyle(); param format : OutputFormat = OutputFormat.Display }`
  (`io.ri:161`). **No color/finish field yet** — task γ extends `DisplayStyle`. `DisplayOutput`
  deliberately has **no `constraint determined(subject)`** (`io.ri:160`) → can preview undetermined geometry.
- **`GuiState` (`types.rs:162`)** is the snapshot payload (`meshes`, `values`, `constraints`, `files`,
  diagnostics, **`tensegrity_wires`**). `tensegrity_wires: Vec<TensegrityWireData>` (`:203`) carries
  `#[serde(default)]` (`:202`) — **the exact precedent for the `display_appearance` map**: a model-level
  descriptor list extracted from value cells, added as a forward-compat field, transported on the existing
  path — not a per-mesh field, not a new channel. PRD-3's `display_panes` mirrors the same pattern.
- **Three.js is ready:** `meshManager.ts` constructs `MeshStandardMaterial` (`:249`) with only `color`
  set today (`color: colorForEntity(entityPath)`); `metalness` / `roughness` / `opacity` / `wireframe` /
  `sheen` are all available at defaults. FEA colorize uses `MeshPhongMaterial` with vertex colors
  (`:243`); ghost overlay uses `MeshBasicMaterial` (`:184`). The interactive recolor path
  (`:777`/`:798`) rebuilds the material. Entity color is a deterministic hash — `colorForEntity` (`:43`)
  → `ACCENT_PALETTE[hashEntityPath(...)]` (`:23`).
- **The shared Output-walk lands in PRD-3.γ (task 4765, in-progress).** PRD-3 introduces
  `collect_display_routing(module, values) -> Vec<DisplayDirective>` inside `build_gui_state` (reusing
  `build_outputs`' four-gate enumeration: `engine_build.rs:3863` `conforms_to_output`, resolve
  `ValueCellId` in `values`, read `instance.fields.get(...)` off the `Value::StructureInstance`) and the
  `#[serde(default)] GuiState.display_panes` field. **This PRD's γ extends that walk** to also read
  `.style` and emit a sibling `display_appearance` map.
- **Transport ownership (G4 settled):** per `gui-event-channel-inventory.md` **§2.4**, payload extensions
  to the existing snapshot channel (adding a field to `MeshData`; adding a `#[serde(default)]` map to
  `GuiState`) are owned by the **citing PRD** as "ordinary kernel/IPC-types work" — NOT a new channel,
  NOT the inventory PRD's job. Listed precedents: `scalar_channels`, `element_kind`, `region_tags`,
  `vector_channels` (per-mesh fields) and `tensegrity_wires` (a `GuiState` map). `engine-integration-norm.md`
  **§3.10** explicitly *excludes* the Tauri/GUI seam and hands it to GR-016. So **this PRD owns the
  `MeshData` appearance field and the `GuiState.display_appearance` map.**

---

## 4. G3 gate — grammar **and** semantic (PASS)

The only novel `.ri` surface is the **extended `DisplayStyle`** (task γ adds `color`/`finish` to the
landed `opacity`/`wireframe`) and its ctor call. Validated 2026-06-24 against `tree-sitter-reify`:

Committed-shape fixture `/tmp/prd-gate-fixtures/appearance-viewport-egress-1.ri` →
`tree-sitter parse --quiet` **exit 0, 0 ERROR/MISSING nodes**.

| Fragment exercised | Verdict |
|---|---|
| `structure def DisplayStyle { param color : Color = Color(r:0.7,g:0.7,b:0.7); param finish : Finish = Finish.Satin; param opacity : Real = 1.0; param wireframe : Bool = false }` (extended struct-def) | ✅ parses |
| `DisplayOutput(subject: part, style: DisplayStyle(color: Color(named:"RAL9001", r:0.96, g:0.95, b:0.88), finish: Finish.Gloss, opacity: 0.5, wireframe: false))` (nested ctor, named + rgb both-fields) | ✅ parses |

**Conclusion: no novel grammar.** The extended `DisplayStyle` reuses the same struct-def + nested-ctor +
qualified-enum (`Finish.Gloss`) shapes PRD-1 §3 validated for `Appearance`. The only substrate
dependencies are **semantic**, resolved by explicit cross-PRD prerequisite edges (not grammar work):

- `Color` / `Finish` types (for the `DisplayStyle` extension) → **PRD-1.α (task 4760)**.
- `resolve_appearance` / `resolve_color` Rust seam (for material→appearance) → **PRD-1.β (task 4761)**.
- the shared `collect_display_routing` Output-walk + `GuiState` snapshot machinery → **PRD-3.γ (task 4765)**.

`grammar_confirmed = true` for every leaf (no grammar-producer prerequisite task). The Rust/TS mechanisms
(`MeshData` field, `display_appearance` map, `meshManager` precedence) are not `.ri` syntax — their
substrate is the §3-verified `build_gui_state` / `MeshData` / `MeshStandardMaterial` surfaces.

---

## 5. Sketch of approach + resolved design decisions (ratified — not re-litigated)

A two-channel egress: the **material appearance** is computed engine-side and attached per-mesh; the
**`DisplayOutput.style` override** is collected by the shared Output-walk into a subject-keyed map; the
frontend merges both with session state under the precedence stack.

1. **`MeshData` carries an optional material appearance (task α + β).** Add
   `appearance : Option<MeshAppearance>` to `MeshData` (Rust + TS lockstep, GR-016 §3.2), where
   `MeshAppearance` is the **flattened, renderer-facing** PBR record `{ color: [f32;4] /*rgba*/,
   metalness: f32, roughness: f32, finish: u8 }`. **Flattened, not a Rust mirror of the reify `Appearance`
   stdlib structure** — matching how `scalar_channels`/`element_kind`/`region_tags` are flat primitives the
   Three.js layer reads directly, and keeping α independent of PRD-1's stdlib types (α can land in
   parallel with PRD-1). `engine.rs build_gui_state` populates it (task β) from
   `resolve_appearance(body)` (PRD-1.β seam) **iff the realized entity resolves to a material**; raw
   geometry with no material leaves it `None` → frontend hash fallback (precedence layer 1).
2. **`DisplayStyle` gains the override surface (task γ).** Extend the landed `DisplayStyle` with
   `param color : Color = Color()` and `param finish : Finish = Finish.Satin` (consuming PRD-1's types),
   keeping `opacity`/`wireframe`. The shared `collect_display_routing` walk (PRD-3.γ) is extended to read
   `.style` off each `DisplayOutput` occurrence and emit a sibling `#[serde(default)]`
   `GuiState.display_appearance : Vec<AppearanceDirective>` where
   `AppearanceDirective { subject: String, style: DisplayStyleData }` and `DisplayStyleData` is the
   flattened style `{ color: [f32;4], finish: u8, opacity: f32, wireframe: bool }`. The join key is the
   subject `entity_path` — identical to `display_panes`' join key.
3. **Precedence enforced in the frontend material layer (task δ).** `meshManager` (+ `App.tsx` joining
   `display_appearance` to meshes by `entity_path`, mirroring the `display_panes` join) computes each
   mesh's effective material per the §2 stack and applies it to `MeshStandardMaterial`
   (`color`/`metalness`/`roughness`/`opacity`/`wireframe`; `finish` modulates `roughness`/sheen). The
   existing interactive-recolor / FEA-colorize / visibility paths stay the top (session) layer.
4. **Ghost / highlight expressible through the same override layer.** Ghosting = low `DisplayStyle.opacity`;
   highlight = a `DisplayStyle.color` — no separate mechanism for the Tier-1 use cases (skeleton decision 5).

### Ratified decisions (do not re-litigate)

1. **Two model channels, merged client-side.** Material appearance is a **per-mesh `MeshData` field**
   (intrinsic — "what the body is made of"); the `DisplayOutput.style` override is a **subject-keyed
   `GuiState.display_appearance` map** (a model directive — "how to display this subject"). They are
   *not* unified engine-side; the renderer merges them with session state. This honors skeleton decision 1
   ("the shared walk produces both an appearance map *and* a pane map") and keeps `MeshData` free of
   model-override directives (the same separation PRD-3 keeps for pane routing).
2. **`MeshData.appearance` carries flattened PBR fields, not the reify `Appearance` type.** Renderer-facing
   contract; keeps α free of any PRD-1 dependency. `MeshAppearance` ≠ the stdlib `Appearance` structure —
   it is its egress projection.
3. **`MeshData.appearance` is populated iff the entity resolves to a material.** `Some(MeshAppearance)` ⇒
   precedence layer 2 (material); `None` ⇒ layer 1 (hash). This makes "no-material → hash" honest rather
   than every body collapsing to the neutral-grey default that `resolve_appearance` returns for an
   unset-but-present material.
4. **The `display_appearance` map is PRD-2-owned and sibling to `display_panes`.** PRD-3.γ (task 4765)
   owns the walk scaffold + `display_panes`; PRD-2.γ extends the same walk to emit the sibling
   `display_appearance` field. PRD-2 does **not** restructure PRD-3's `DisplayDirective` struct — the
   reserved `style` slot in PRD-3 §7.1's comment goes unused in favor of a clean sibling field (cleaner
   ownership boundary; mirrors the `tensegrity_wires`/`display_panes` sibling-field precedent). *(Companion:
   PRD-3 §7.1's reserved-slot comment may be dropped in a follow-up doc edit — non-blocking.)*
5. **The integration-gate override color is pinned with explicit rgb.** Task ε's `.ri` uses
   `Color(named:"RAL9001", r:…, g:…, b:…)` (both-fields) so its asserted on-screen color holds whether
   `resolve_color` seeds RAL9001 or falls back to the rgb fields — decoupling ε from PRD-1's tactical
   RAL-seed breadth (PRD-1 §OQ1). G6-honest (§9).

---

## 6. Cross-PRD relationship (G4)

| Other PRD / seam | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **PRD-1 `appearance-substrate`** | **consumes** | `Color`/`Finish`/`Appearance` types (γ extends `DisplayStyle`); `resolve_appearance`/`resolve_color` Rust seam (β); library editorial appearance (ε's steel-grey) | PRD-1 owns types + resolution + material source; **this** consumes + renders | **hard dep** β→4761, γ→4760, ε→4762 (wired at decompose) |
| **PRD-3 `multi-pane-viewport`** | **shares seam** | the `build_gui_state` `: Output` Display-walk: PRD-3 reads `.pane`→`display_panes`, **this** reads `.style`→`display_appearance` — one walk, two sibling `GuiState` maps | **PRD-3 owns the walk scaffold (`collect_display_routing` + `DisplayDirective` + `display_panes`); this extends the walk to emit `display_appearance`** | **resolved — hard dep γ→4765**; no reciprocal ambiguity (both PRDs agree PRD-3 introduces the walk) |
| `gui-event-channel-inventory.md` (GR-016) | extends | `MeshData.appearance` (per-mesh field) + `GuiState.display_appearance` (map) on the existing snapshot channel | **this** owns (per §2.4 payload-extension precedent) | settled (no new channel) |
| `engine-integration-norm.md` (GR-017) | excluded | §3.10 hands the Tauri/GUI seam to GR-016; the `build_gui_state` extension is not a new §3 seam | this (extends existing surface) | settled |
| `io-export-import-completion.md` (landed) | sibling | the CLI `I_DISPLAY_OUTPUT_DEFERRED` skip stays a no-op | unchanged | leave as-is |
| `io-display-output-viewport.md` stub / tracker **task 4291** | activates | this PRD activates the **style** half of `DisplayOutput`→viewport drive; PRD-3 the **pane** half | this (style) + PRD-3 (pane) | 4291 stays the umbrella tracker; no dep edge → 4291 |
| Functional surface-finish **capstone** (deferred [MILESTONE]) | forward-compat | renderer reads `Appearance`; the functional-finish capstone later becomes a second producer of it | contract stable | seam preserved (PRD-1 decision 1) |

**Shared-seam resolution (confirmed against PRD-3 §6).** PRD-3 owns the walk because it is *independent*
(no PRD-1 dep) and must be able to land first; if PRD-2 owned the walk, PRD-3 would inherit PRD-2→PRD-1's
appearance chain, defeating its independence. PRD-3.γ (task 4765) introduces `collect_display_routing`,
`DisplayDirective`, and `GuiState.display_panes`; **PRD-2.γ depends on 4765** and extends the walk to also
emit `GuiState.display_appearance`. No reciprocal ambiguity. **Companion action (done at this batch's
decompose):** wire `add_dependency(PRD-2.γ → 4765)`.

No new engine-integration-norm §3 seam. No reciprocal-ownership ambiguity. No contested pair introduced.

---

## 7. Contract (B + H) — the engine→GUI appearance seam

The seam PRD-1 produces, PRD-3 shares, and the Three.js layer consumes. Specified up front so the
integration task lands as a first-class leaf rather than starving under the narrow-lock orchestrator.

### 7.1 Per-mesh material channel (Rust + TS lockstep, GR-016 §3.2)

```rust
// gui/src-tauri/src/types.rs — MeshData gains:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub appearance: Option<MeshAppearance>,

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]   // mirrored in gui/src/types.ts
pub struct MeshAppearance {
    pub color: [f32; 4],   // rgba, linear [0,1]; resolve_color → rgb, a=1.0 (opacity is a style override)
    pub metalness: f32,    // [0,1]
    pub roughness: f32,    // [0,1]
    pub finish: u8,        // Matte=0, Satin=1, Gloss=2 (cosmetic; modulates roughness/sheen client-side)
}
```

**Invariants.**
- **Populated iff material.** `engine.rs build_gui_state` sets `appearance = Some(MeshAppearance)` for a
  realized entity that resolves to a material (via `resolve_appearance(body)`, PRD-1.β); `None` for raw
  geometry. `None` ⇒ frontend hash fallback (precedence layer 1) — **never** a silent neutral-grey for
  genuinely material-less geometry.
- **Non-sentinel.** When `Some`, `color`/`metalness`/`roughness` are real resolved values (PRD-1's
  `resolve_color` is total and loud-on-unknown-name), never `Undef`/placeholder.
- **Back-compat.** `#[serde(default)]` ⇒ old payloads (no field) deserialize as `None`; existing
  `MeshData::validate()` length contracts are unaffected (appearance is not a per-vertex channel).

### 7.2 Model-override channel — the shared Output-walk's sibling map

```rust
// gui/src-tauri/src/types.rs — GuiState gains (mirroring tensegrity_wires / display_panes):
#[serde(default)]
pub display_appearance: Vec<AppearanceDirective>,

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]   // mirrored in gui/src/types.ts
pub struct AppearanceDirective {
    /// entity_path of the resolved DisplayOutput.subject — MUST equal the MeshData.entity_path
    /// of that subject's realization (the join key; identical convention to DisplayDirective.subject).
    pub subject: String,
    pub style: DisplayStyleData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DisplayStyleData {
    pub color: [f32; 4],   // rgba; from DisplayStyle.color via resolve_color, a = opacity
    pub finish: u8,        // Matte/Satin/Gloss
    pub opacity: f32,      // DisplayStyle.opacity [0,1]
    pub wireframe: bool,   // DisplayStyle.wireframe
}
```

Produced by extending PRD-3.γ's `collect_display_routing` (the same four-gate enumeration over
`result.values`) to read `.style` off each `DisplayOutput` occurrence's `Value::StructureInstance`,
resolve its `Color`/`Finish` via PRD-1's `resolve_color`, and emit one `AppearanceDirective` per styled
occurrence.

**Invariants.**
- **Join-key identity.** `AppearanceDirective.subject == MeshData.entity_path` of the subject's
  realization. A `DisplayOutput.subject` resolving to no realized mesh ⇒ its directive is **dropped**
  (logged), never dangling — the same rule as `DisplayDirective` (PRD-3 §7.2 inv.1).
- **One directive per styled occurrence.** Independent of `pane`: a `DisplayOutput(pane:0, style:…)`
  (default pane, explicit style) still emits an `AppearanceDirective` — pane and style are orthogonal
  channels off the same occurrence.
- **Empty when absent.** A module with no `DisplayOutput`, or whose `DisplayOutput`s leave `style`
  defaulted, yields an empty `display_appearance` ⇒ renders by material appearance / hash only.
- **Last-good semantics.** `display_appearance` follows `meshes`' existing last-good-on-failure behavior
  in `build_gui_state` (no flicker to empty on a transient eval error) — same as `display_panes`.

### 7.3 Frontend precedence (task δ)

For each mesh, the effective `MeshStandardMaterial` is computed per the §2 stack, **per channel**, highest
layer first:

| Channel | layer 4 session | layer 3 `display_appearance[subject].style` | layer 2 `MeshData.appearance` | layer 1 hash |
|---|---|---|---|---|
| color | recolor / FEA wins | `style.color` | `appearance.color` | `colorForEntity` |
| metalness/roughness | (session may override) | `style.finish`→roughness/sheen | `appearance.metalness/roughness` | material defaults |
| opacity | session | `style.opacity` | 1.0 | 1.0 |
| wireframe | session | `style.wireframe` | false | false |

**Invariants.**
- **Session wins.** An active interactive recolor / FEA colorize / ghost on an entity overrides the model
  layers entirely (existing `meshManager` recolor path, `:777`/`:798`).
- **Fallback chain.** Remove the `DisplayOutput` ⇒ layer 3 absent ⇒ render by `MeshData.appearance`
  (layer 2). Remove the material ⇒ layer 2 `None` ⇒ render by `colorForEntity` (layer 1).
- **`MeshData` stays model-override-free.** Only `display_appearance` carries the `DisplayOutput.style`
  override; `MeshData.appearance` carries only the intrinsic material look.

---

## 8. Boundary-test sketch (B + H) — the integration gate's signal (task ε)

A single scripted `reify-debug` MCP session against a committed `.ri`, each row facing both the producer
(engine → `MeshData.appearance` / `display_appearance`) and the consumer (Three.js precedence). Reuses
the existing `gui/test` harness (`open_file`/`wait_for_idle`/`viewport_state`/`store_state`/`screenshot`).

| # | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|
| B1 | **Material appearance renders (steel)** | a body of `Steel_AISI_1045`, no `DisplayOutput` | the mesh's material is the editorial steel grey-satin (color + metalness/roughness delta vs `colorForEntity` hash) — `viewport_state`/material-state probe (β + PRD-1.γ/4762) |
| B2 | **No-material → hash fallback** | a raw `box(…)` with no material | `MeshData.appearance == None`; mesh renders the `colorForEntity` hash color (precedence layer 1, inv. §7.1) |
| B3 | **`DisplayOutput.style` overrides** | `DisplayOutput(subject: steel_body, style: DisplayStyle(color: Color(named:"RAL9001", r:0.96,g:0.95,b:0.88), finish: Finish.Gloss, opacity: 0.5, wireframe: false))` | `display_appearance` has a directive for the body; the mesh renders RAL9001-cream, gloss (low roughness), 0.5 opacity — overriding the steel material (layer 3 > 2) |
| B4 | **Join-key holds / dangling dropped** | a `DisplayOutput(subject: undetermined, style:…)` whose subject has no realized mesh | no phantom mesh styled; the directive is dropped + logged (inv. §7.2) |
| B5 | **Session recolor wins** | B3 scene, then an interactive recolor / FEA colorize on the styled body | the session color wins over the model override (layer 4 > 3) — material-state probe |
| B6 | **Remove DisplayOutput → material fallback** | B3 scene with the `DisplayOutput` removed | the body renders the steel material appearance again (layer 3 absent → layer 2) |
| B7 | **Wireframe / opacity override** | `DisplayStyle(wireframe: true)` and a separate `DisplayStyle(opacity: 0.25)` (ghost) | the meshes render wireframe / translucent respectively via the same override channel (skeleton decision 5) |

B1+B3+B5+B6 are ε's integration-gate observable signal (the §1 goal realized end-to-end); B2/B4/B7 cover
the fallback/edge invariants. The whole sketch is one CI-able scripted session.

---

## 9. G6 — premise validity per leaf signal

| Leaf / signal | Asserted premise | Basis (achievable / true / producible from this leaf's dependency set) |
|---|---|---|
| α `MeshData.appearance` field | Rust serde round-trips `Some/None`; TS mirror shape-checks | additive `#[serde(default)] Option<…>` field (§3 `tensegrity_wires`/`scalar_channels` precedent). Pure types work — no numeric/rejection premise. (Intermediate, roped to ε — synthetic-input serde test is **not** a standalone leaf signal.) |
| β material→`MeshData.appearance` | a material-bearing body's `MeshData.appearance` is `Some` with the resolved color; raw geometry is `None` | `resolve_appearance`/`resolve_color` ship in **PRD-1.β (4761, upstream)**; `build_gui_state` holds `result.values` (§3). **Field-population OK** — β writes a real `MeshAppearance` on the production path, `None` only for genuinely material-less geometry (decision 3). |
| γ `DisplayStyle` ext + `display_appearance` | extended `DisplayStyle` type-checks; a `DisplayOutput(style:…)` `.ri` yields a non-empty `display_appearance` whose `subject` == the mesh `entity_path` | grammar PASS (§4); `Color`/`Finish` from **PRD-1.α (4760, upstream)**; the walk from **PRD-3.γ (4765, upstream)**. Join-key is the established `display_panes` convention. |
| δ frontend precedence | recolor wins; remove `DisplayOutput` → material; no-material → hash | every layer's source is present: session paths wired on main (`meshManager:777/798`, hash `:43`); material from α/β; override from γ. **Branch 3 (end-to-end):** all capabilities upstream of δ. |
| **ε integration gate (LEAF)** | steel renders grey-satin; the `DisplayOutput` overrides to RAL9001-cream/gloss/0.5; the three precedence cases hold | steel editorial appearance ← **PRD-1.γ (4762, ε→4762)**; resolve/material/override/precedence ← δ's transitive closure (β→4761, γ→4760+4765). **No downstream capability.** **Override color is pinned with explicit rgb** (decision 5) so the asserted color holds regardless of PRD-1's RAL-seed breadth (PRD-1 §OQ1) — not a guessed/unbacked premise. |

**No numeric bound / floor is asserted** anywhere — the render assertions are color/material-state /
screenshot deltas observed via `reify-debug` MCP, not solver tolerances. G6 branches 1 (numeric bound)
and 2 (closed-form exactness) **do not fire**. No negative-assertion/rejection signal (branch 4) — N/A.
The two substantive premises are β's field-population (PRD-1.β-upstream + β-owned egress) and ε's
end-to-end capability (every requirement in ε's transitive dependency set, none downstream).

---

## 10. Pre-conditions for activating

- **PRD-1 `appearance-substrate` types + resolution seam land:** `Color`/`Finish`/`Appearance` +
  `Visual` (task **4760**); `Material : Visual` + `resolve_appearance`/`resolve_color` (task **4761**);
  library editorial appearance (task **4762**, for ε's steel-grey). Hard deps β→4761, γ→4760, ε→4762.
- **PRD-3 `multi-pane-viewport` shared Output-walk lands:** `collect_display_routing` +
  `DisplayDirective` + `GuiState.display_panes` (task **4765**). Hard dep γ→4765.
- **`DisplayOutput`/`DisplayStyle` surface + the GUI snapshot path** — landed (tasks 4287/4288 done;
  `tessellate_snapshot`/`build_gui_state` present, §3). G3 grammar: **N/A** beyond the §4-validated
  `DisplayStyle` extension (no grammar-producer task).
- **Three.js `MeshStandardMaterial` knobs** — available at defaults today (§3); δ wires against them.
- Geometry realization is real (`box(…)`/library materials realize today) — ε builds against it.

---

## 11. Decomposition plan

B+H shape: foundation (α) + the two model channels (β material, γ override) → vertical slice render (δ) →
integration gate (ε). Greek labels → task IDs at decompose. **Minimal vertical slice (Leo):**
α(`MeshData.appearance` field) → β(one material via `resolve_appearance`) → δ(render that mesh's material
color) end-to-end **first** — proving material color reaches the viewport — *before* γ adds the override
channel and δ layers in the full precedence. The slice needs only α+β; γ/ε are the override + gate breadth.

- **α — `MeshData` appearance field (Rust + TS lockstep).** *Modules:* `gui/src-tauri/src/types.rs`
  (`MeshData.appearance: Option<MeshAppearance>` + the `MeshAppearance` struct), `gui/src/types.ts`
  (mirror + the `raw→typed` converter at `:114`). *Intermediate* (foundation; unlocks β, δ; roped to ε —
  the serde/TS shape test is not a standalone user signal). *Signal (intermediate):* unlocks β + δ; Rust
  serde `Some/None` round-trip + TS shape test as supporting coverage. *Prereqs:* —. `grammar_confirmed=true`.
- **β — engine material→`MeshData.appearance` (GUI snapshot path).** *Modules:* `gui/src-tauri/src/engine.rs`
  (`build_gui_state` MeshData mapping populates `appearance` from `resolve_appearance`) — BRE acquires the
  reify-eval footprint. *Intermediate* (unlocks δ, ε). *Signal:* a material-bearing body's
  `MeshData.appearance` is `Some` with the resolved color (raw geometry `None`), observed via `reify-debug`
  MCP mesh-data / `viewport_state` probe. *Prereqs:* α, **PRD-1.β (4761)**. `grammar_confirmed=true`.
- **γ — `DisplayStyle` color/finish + `display_appearance` map via the shared walk.** *Modules:*
  `crates/reify-compiler/stdlib/io.ri` (extend `DisplayStyle` with `color : Color`/`finish : Finish`),
  `gui/src-tauri/src/types.rs` (`GuiState.display_appearance` + `AppearanceDirective`/`DisplayStyleData`),
  `gui/src/types.ts` (mirror), the shared `collect_display_routing` walk extension in
  `gui/src-tauri/src/engine.rs` (read `.style`, emit the sibling map) — BRE acquires. *Intermediate*
  (unlocks δ). *Signal:* the extended `DisplayStyle` passes `reify check` (exit 0, zero
  `unresolved type`/`unresolved name`); a `DisplayOutput(style:…)` `.ri` yields a non-empty
  `display_appearance` whose `subject` matches the rendered mesh's `entity_path` (Rust serde round-trip +
  the join surfaced to the frontend). *Prereqs:* **PRD-1.α (4760)**, **PRD-3.γ (4765)**.
  `grammar_confirmed=true`.
- **δ — Three.js precedence stack.** *Modules:* `gui/src/viewport/meshManager.ts` (compute the effective
  `MeshStandardMaterial` per §7.3), `gui/src/App.tsx` (join `display_appearance` to meshes by
  `entity_path`, mirroring the `display_panes` join). *Intermediate* (unlocks ε; has its own observable
  signal). *Signal:* vitest + `reify-debug` MCP — a manual recolor still wins; removing the
  `DisplayOutput` falls back to material; a no-material body falls back to hash (the three precedence
  cases). *Prereqs:* α, β, γ. `grammar_confirmed=true`.
- **ε — integration gate (LEAF).** *Modules:* `examples/` (the committed `.ri`) + `gui/test/` harness.
  *Leaf — signal = §8 boundary sketch.* *Signal:* one CI-able scripted `reify-debug` MCP session against a
  committed `.ri` (a `Steel_AISI_1045` body + a `DisplayOutput`-styled body) asserts: steel renders the
  editorial grey-satin (B1); the `DisplayOutput` overrides to RAL9001-cream/Gloss/0.5-opacity (B3); a
  manual recolor wins (B5); removing the `DisplayOutput` falls back to the material (B6); a no-material
  body falls back to hash (B2). *Consumer:* end user viewing a styled `.ri`. *Prereqs:* δ,
  **PRD-1.γ (4762)**. `grammar_confirmed=true`.

DAG: α root; β→α,4761; γ→4760,4765; δ→α,β,γ; **ε→δ,4762** (sole leaf). Vertical slice α→β→δ(material-base)
first; γ adds the override channel; δ completes the precedence; ε gates end-to-end.

---

## 12. Out of scope (named)

- The appearance **types** + material source + 3MF color export (**PRD-1** `appearance-substrate`).
- The **pane** model / N-pane layout (**PRD-3** `multi-pane-viewport`). This PRD reads only `.style`.
- The headless CLI export driver's `I_DISPLAY_OUTPUT_DEFERRED` skip — correctly stays a no-op.
- **Functional / real** surface finish, coating, treatment (deferred capstone — PRD-1 decision 1/2).
- Texture maps (UVs + asset pipeline).
- Persisting interactive overrides into the `.ri` (session state stays session state — no AST writeback).
- Range *enforcement* on `metalness`/`roughness`/`opacity`/rgb (downstream clamps; PRD-1 §OQ2).
- Per-vertex / per-face appearance (only per-body / per-mesh in v1).

---

## 13. Open questions (tactical — deferred, not design-blocking)

1. **`finish` → renderer mapping.** Does cosmetic `Finish` (Matte/Satin/Gloss) drive `roughness`
   (Matte→high, Gloss→low) and/or `MeshStandardMaterial.sheen`, or stay advisory while the `Appearance`'s
   own `metalness`/`roughness` carry the look? *Suggested:* metalness/roughness drive the PBR look;
   `finish` applies a small roughness/sheen nudge for the "satin sheen" in the §1 goal. Mirrors PRD-1 §OQ4
   (the capstone owns the functional mapping). Decide during δ.
2. **`MeshData.appearance` color space.** Linear vs sRGB for the rgba — must match how `colorForEntity`
   feeds `MeshStandardMaterial.color` today (Three.js `Color` is sRGB-in by default). *Suggested:* match
   the existing `colorForEntity` convention exactly so material and hash colors are comparable. Decide
   during β/δ.
3. **`wireframe`/ghost interaction with FEA colorize.** When a body has both a `DisplayStyle.wireframe`
   override and an active FEA colorize (session layer), which wins — does session fully replace, or does
   wireframe compose? *Suggested:* session wins outright per the precedence stack (§7.3 inv. "session wins
   outright"); revisit if a compose use-case appears. Decide during δ.
4. **RAL9001 seed coordination with PRD-1.** ε pins explicit rgb so it is robust regardless; *optionally*
   ask PRD-1's β to seed RAL9001 so the `named`-only form also resolves. *Suggested:* leave ε rgb-pinned
   (decision 5); a one-line PRD-1 §OQ1 seed addition is a nice-to-have, not a dependency. Decide if/when
   PRD-1.β widens its seed.
