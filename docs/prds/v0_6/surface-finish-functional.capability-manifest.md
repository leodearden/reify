# Capability manifest — `surface-finish-functional.md`

Mechanizes G3 + G6 per leaf (overlay → *Capability Manifest — reify evidence forms*). Each binding is
`capability → evidence`; any **FAIL** blocks the batch. Verified 2026-06-29 against
`target/release/reify` (Jun-24 build) + `tree-sitter-reify`. Empty-value sentinel = `Value::Undef`.

Leaf labels match the decomposition (§9): α vocabulary · β Appearance seam · γ cost/mass · δ 3MF · ε viewport.

---

## α — functional-finish vocabulary stdlib module

| Capability | Evidence | Verdict |
|---|---|---|
| `enum CoatingProcess`/`FinishProcess`/`TreatmentProcess` + `structure def Coating`/`Treatment` + `trait SurfaceTreated` parse | `grammar-fixture: docs/prds/v0_6/fixtures/surface_finish_functional.ri` → `tree-sitter parse --quiet` exit 0, **0 ERROR** | **PASS** |
| `pub type ArealCostRate = Money / Area` resolves; `cost_per_area = 0USD/m^2` literal type-checks | live `reify eval` → `Coating.cost_per_area = 0 USD·m^-2` (alias mechanism: `units.ri Velocity = Length / Time` precedent) | **PASS** |
| `um` (micro) resolves to `Length`; `thickness = 15um` checks | live: `1um < 1mm` & `1um > 0.0001mm` constraints OK; `µm` micro-sign does **not** parse (use `um`) | **PASS** |
| all-defaulted trait ⇒ additive conformance (`Uncoated`/`AsMachined`/`Anneal` sentinels) | `reify check docs/prds/v0_6/fixtures/surface_finish_functional.ri` exit 0, "All constraints satisfied.", zero unresolved | **PASS** |
| stdlib load order: new module sees `Color`/`Finish`/`Appearance` (+ optionally `SurfaceFinish`) | `stdlib_loader.rs`: `materials_appearance` `:62`, `tolerancing` `:145` — register the new module after both | **PASS** (register-after; α's job) |
| consumer named (anti-orphan) | β (Rust seam) + δ/ε (egress) + γ (cost) all consume α's types | **PASS** (intermediate, unlocks β/γ/δ/ε) |

## β — Appearance-derivation seam (third producer, Rust)

| Capability | Evidence | Verdict |
|---|---|---|
| `resolve_appearance`/`resolve_appearance_opt`/`resolve_color` exist + wired on the production path | `grep: crates/reify-eval/src/appearance.rs` (pub fns) + `engine_build.rs:2414` `resolve_instance_color`→`resolve_appearance` (the 3MF production path); PRD-1 #4761 **done** | **PASS** (upstream, wired) |
| DAG-direction (anti-inversion) | producer `resolve_*` is **PRD-1 #4761 (done)**, upstream of β; β extends, never depends-down | **PASS** |
| field-population (anti-sentinel) | β writes a **real** `Appearance` (`color = resolve_color(coating.color)`, non-`Undef`) on the production path; `Uncoated` → falls through to material/neutral (no silent `Undef`/black — `resolve_color` is total) | **PASS** (β-owned, production path) |
| `Appearance`/`Finish`/`Color` shapes UNCHANGED (forward-compat invariant) | contract reused verbatim from `materials_appearance.ri`; β adds a producer, edits no struct/enum | **PASS** |
| body carries `coating`/`finish_process` navigable like `body.material` | live: `Part : SurfaceTreated { material, coating, … }` checks; `resolve_appearance` already does `data.fields.get("material")` — same field-navigation for `.coating` | **PASS** |
| consumer named | δ (3MF) + ε (viewport) both consume the functional `resolve_appearance` | **PASS** (intermediate) |

## γ — cost + mass roll-up (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| flat `[coating.process_cost, treatment.cost].sum : Money` + nested BOM `.sum` over `sub`→`let` | live `reify eval surface_finish_functional.ri` → `Plate.finishing_cost = 16 USD`, `Bracket = 8 USD`, `AssemblyBOM.total_finishing_cost = 24 USD`; `.sum` Money idiom = `cost_aggregation.ri` + its locked-total test | **PASS** |
| area-based `cost_per_area * area(geometry) : Money`; `coat_density * area(geometry) * thickness : Mass` | live `reify eval surface_finish_area_cost.ri` (**top-level**) → `coat_cost = 1.2 USD`, `coat_mass = 0.0018 kg` (exact) | **PASS** (top-level) |
| **numeric-floor / hazard** (area kernel-gating) | `area()`/`volume()` kernel-gated: error on pure `reify check` value surface (still exit 0), **`undef` for nested `sub`-instances**, realized only top-level/build. Mitigation: area-based signal asserted on a **top-level** part; flat path used for nested BOM. **No solver tolerance/accuracy floor is asserted** (exact dimensioned arithmetic) | **PASS** (hazard bound + mitigated) |
| signal is real eval value, not synthetic-input unit test | leaf signal = `reify eval` printing `total_finishing_cost`/`coat_cost`/`coat_mass`; Rust test asserts the **locked** values (mirrors `cost_aggregation_tests.rs` — the user-observable eval path) | **PASS** |
| DAG-direction | depends only on α (vocabulary); cost arithmetic is upstream-shipped Money idiom | **PASS** |

## δ — 3MF egress reflects coating color (LEAF, integration gate)

| Capability | Evidence | Verdict |
|---|---|---|
| 3MF `<basematerials>` per-body color egress exists + wired | `grep: reify-ir/src/geometry.rs write_3mf` + `reify-eval/src/engine_build.rs:2414 resolve_instance_color`; **PRD-1 δ #4763 done** | **PASS** (upstream, wired) |
| coating-derived color reaches the egress | δ over β (`resolve_appearance` functional-aware) over #4763; color → exact bytes via `resolve_color` | **PASS** (β + #4763 upstream) |
| DAG-direction | β upstream (in-batch); #4763 done (out-of-batch); both upstream of δ | **PASS** |
| field-population | δ writes a real RGB on the production path (the leaf deliverable, not owned downstream) | **PASS** |
| signal user-observable | `reify build -o x.3mf` → unzip → `<basematerials>` RGB = anodize-derived; no `W_3MF_NO_MATERIALS` (CLI e2e) | **PASS** |

## ε — viewport egress reflects functional finish (LEAF, integration gate)

| Capability | Evidence | Verdict |
|---|---|---|
| viewport consumes `resolve_appearance` | PRD-2 #4775 **done** (`build_gui_state`/`MeshData` appearance path reads `resolve_appearance`) | **PASS** (upstream, wired) |
| functional appearance reaches the viewport | ε over β (functional `resolve_appearance`) over #4775 | **PASS** (β + #4775 upstream) |
| DAG-direction | β upstream (in-batch); #4775 done (out-of-batch); both upstream of ε | **PASS** |
| override-color premise robust | ε pins `Color(named:"RAL9005", r:…,g:…,b:…)` (explicit rgb alongside `named`) so the asserted on-screen color holds regardless of RAL-seed breadth (PRD-2 decision 5 pattern) | **PASS** (not a guessed/unbacked premise) |
| signal user-observable | scripted `reify-debug` MCP material-state/screenshot delta: polished→sheen, anodize→dark, display-override>functional, session>all | **PASS** |

---

**Batch verdict:** all bindings **PASS**. No `declared-only`/`test-only`/`producer-absent`/
`producer-downstream`/`fixture-ERROR`/`bound≤floor`/`rejection-absent`. The one substantive hazard
(`area()` kernel-gating → nested `undef`) is **bound and mitigated** (top-level assertion for the
area-based path; flat path for nested BOM). Cleared to queue.
