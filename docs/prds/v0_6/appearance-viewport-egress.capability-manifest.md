# Capability manifest — `appearance-viewport-egress`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/appearance-viewport-egress.md`. One block per task; each
capability the task's signal asserts is bound to evidence. **Any binding resolving to a FAIL value
(`declared-only`/`test-only`/`producer-absent`/`producer-downstream`/`producer-extent-short`/`fixture-ERROR`/`bound≤floor`/`rejection-absent`)
blocks the batch.** All bindings below are **PASS**.

Evidence verified 2026-06-24 against the reify worktree at HEAD. Empty-value sentinel: `Value::Undef` /
`None` / placeholder. Production entry paths: `gui/src-tauri/src/engine.rs build_gui_state`, `MeshData`
mapping; `gui/src/viewport/meshManager.ts`; the PRD-1/PRD-3 cross-PRD producer tasks (all reify-project,
all upstream).

---

## α — `MeshData.appearance` field (Rust + TS lockstep)  *(intermediate; roped to ε)*

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `MeshData` accepts an additive optional field | grammar/substrate (Rust types) | `grep:gui/src-tauri/src/types.rs:363` `pub struct MeshData` + `:202` `#[serde(default)]` on `tensegrity_wires` is the additive-`#[serde(default)]` precedent | PASS |
| TS mirror + raw→typed converter exist | substrate (TS) | `grep:gui/src/types.ts:7` `interface MeshData`, `:114` converter | PASS |
| Downstream consumer exists in-batch | anti-orphan (DAG-direction) | `producer:α → consumer:β` (populate) + `consumer:δ` (render) — both **downstream in-batch**; α is a foundation roped to leaf ε (C-as-integration-gate) | PASS |

No numeric/rejection premise. The serde/TS shape test is supporting coverage, **not** a standalone user
signal — α is intermediate.

## β — engine material→`MeshData.appearance`  *(intermediate)*

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `resolve_appearance(body)` / `resolve_color` Rust seam | capability→producer (anti-orphan) | `producer:task-4761` (PRD-1.β — ships the seam in reify-eval/reify-ir, PRD-1 §4.2/§7.3) **upstream** (β→4761 wired) | PASS |
| `build_gui_state` holds the evaluated `ValueMap` + maps surfaces→`MeshData` | wired-on-main | `grep:gui/src-tauri/src/engine.rs:2416` `fn build_gui_state`, `:2579` `.map(|surface| MeshData {…})`, holds `result.values` | PASS |
| `MeshData.appearance` is **populated** with a non-sentinel value | field-population (result-field twin) | β writes `Some(MeshAppearance{…})` on the production path (`build_gui_state`) for material-bearing entities; `None` (honest hash fallback) only for genuinely material-less geometry — **not** `Undef`/placeholder (PRD decision 3, contract §7.1) | PASS |
| `MeshAppearance` is the flattened egress projection, not the stdlib `Appearance` | DAG-direction (β independent of PRD-1 types) | `MeshAppearance` is a Rust struct of primitives (α); the only PRD-1 dependency is the **value** via `resolve_appearance` (4761), not the stdlib type — α stays PRD-1-independent | PASS |

## γ — `DisplayStyle` color/finish + `display_appearance` map  *(intermediate)*

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| extended `DisplayStyle` struct-def + nested ctor parse | grammar-fixture (anti-mismatch) | `grammar-fixture:/tmp/prd-gate-fixtures/appearance-viewport-egress-1.ri` → `tree-sitter parse --quiet` exit 0, **0 ERROR/MISSING** (PRD §4) | PASS |
| `Color` / `Finish` types resolve (the `DisplayStyle` extension) | capability→producer | `producer:task-4760` (PRD-1.α — ships `Color`/`Finish`/`Appearance`/`Visual`, G3 PASS) **upstream** (γ→4760 wired) | PASS |
| the shared `: Output` Display-walk scaffold | capability→producer | `producer:task-4765` (PRD-3.γ — ships `collect_display_routing` + `DisplayDirective` + `GuiState.display_panes`) **upstream** (γ→4765 wired); γ extends the same walk to emit the sibling `display_appearance` | PASS |
| `GuiState` accepts a `#[serde(default)]` sibling map | wired-on-main | `grep:gui/src-tauri/src/types.rs:162` `pub struct GuiState` + `:202-203` `#[serde(default)] tensegrity_wires` — exact sibling-field precedent | PASS |
| `DisplayOutput.style` is readable off the occurrence | wired-on-main | `grep:crates/reify-compiler/stdlib/io.ri:164` `param style : DisplayStyle = DisplayStyle()`; the four-gate enumeration reads `instance.fields.get(...)` (`engine_build.rs:3863` `conforms_to_output`) — PRD-3.γ's walk machinery | PASS |
| `reify check` accepts the extended `DisplayStyle` (no `unresolved type`) | semantic substrate | gated on 4760 (the types); the ctor/struct shapes themselves type-check (PRD-1 §3 validated the equivalent `Appearance` shapes) | PASS (4760 upstream) |

## δ — Three.js precedence stack  *(intermediate; own observable signal)*

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `MeshStandardMaterial` knobs (color/metalness/roughness/opacity/wireframe) available | wired-on-main | `grep:gui/src/viewport/meshManager.ts:249` `new MeshStandardMaterial({ color: … })` — metalness/roughness/opacity/wireframe at defaults today (PRD §3) | PASS |
| interactive recolor / session path (layer 4) | wired-on-main | `grep:gui/src/viewport/meshManager.ts:777` + `:798` recolor material rebuild; FEA `MeshPhongMaterial` `:243`; ghost `MeshBasicMaterial` `:184` | PASS |
| hash fallback (layer 1) | wired-on-main | `grep:gui/src/viewport/meshManager.ts:43` `colorForEntity` → `:23` `ACCENT_PALETTE` | PASS |
| material layer (2) + override layer (3) sources | capability→producer (in-batch) | `producer:α/β` (`MeshData.appearance`) + `producer:γ` (`display_appearance`) — both **upstream in-batch** | PASS |
| join `display_appearance` → meshes by `entity_path` | wired-on-main (pattern) | mirrors the `display_panes` join (PRD-3 §7.3 / `App.tsx` `entity_path` join); same key convention | PASS |

## ε — integration gate **(LEAF — the user-observable signal)**

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `Steel_AISI_1045` carries an editorial `Appearance` (the grey-satin default, B1) | capability→producer + DAG-direction | `producer:task-4762` (PRD-1.γ — the 4 FEA materials gain `: ElasticMaterial + Visual` + editorial appearance) **upstream** (ε→4762 wired) | PASS |
| `resolve_appearance` resolves a **library** material's appearance | capability→producer | `producer:task-4761` (PRD-1.β) **upstream** of ε via ε→δ→β→4761 (transitive closure) | PASS |
| `DisplayStyle.color`/`finish` override surface + `display_appearance` map | capability→producer | `producer:γ` **upstream** via ε→δ→γ (→4760, →4765) | PASS |
| `MeshData.appearance` per-mesh material channel | capability→producer | `producer:α/β` **upstream** via ε→δ | PASS |
| frontend precedence (recolor wins / material fallback / hash fallback) | capability→producer | `producer:δ` **upstream** (ε→δ) | PASS |
| **override color is producible regardless of RAL-seed breadth** (B3) | numeric/exactness (anti-guess) | ε pins `Color(named:"RAL9001", r:0.96,g:0.95,b:0.88)` (both-fields) → `resolve_color` returns the rgb whether or not RAL9001 is seeded (PRD-1 §4.2 fallback is loud + total). The asserted on-screen color is the rgb, **not** a guessed/unbacked named-resolution premise (PRD decision 5; PRD-1 §OQ1) | PASS |
| no numeric solver bound asserted | numeric floor | render assertions are color/material-state / screenshot deltas via `reify-debug` MCP — **no** AABB/solver tolerance ⇒ G6.1/G6.2 do not fire; no floor to clear | PASS (N/A) |
| no negative-assertion / rejection signal | rejection-mechanism | ε asserts no "X is rejected" ⇒ branch 4 N/A | PASS (N/A) |

**DAG-direction (anti-inversion) summary:** every capability ε's signal requires is delivered by a task
**upstream** of ε (δ, and transitively β→4761, γ→4760+4765, α; plus ε→4762). **No** required capability is
owned by a task that depends on ε. The leaf's premise is producible from its dependency set.

---

## Cross-PRD dependency edges wired at decompose (all reify-project, plain integer IDs)

| Edge | Rationale |
|---|---|
| β → 4761 (PRD-1.β) | `resolve_appearance`/`resolve_color` seam |
| γ → 4760 (PRD-1.α) | `Color`/`Finish` types for the `DisplayStyle` extension |
| γ → 4765 (PRD-3.γ) | the shared `collect_display_routing` Output-walk scaffold (γ extends it) |
| ε → 4762 (PRD-1.γ) | `Steel_AISI_1045` editorial appearance (B1 steel-grey) |

Intra-batch: β→α; δ→α,β,γ; ε→δ.

**No FAIL binding. Batch clears the manifest gate.**
