# Capability manifest — `appearance-substrate.md`

Mechanizes G3 + G6 per leaf (gates.md → *Capability Manifest*). Each binding is `capability → evidence`
with a verdict. Any `declared-only | test-only | producer-absent | producer-downstream |
producer-extent-short | fixture-ERROR | bound≤floor | rejection-absent` **blocks** the batch. All bindings
below resolve **PASS** or **producer-upstream**. Re-verified 2026-06-24 against `target/release/reify`
(built this session) + source tree.

Evidence commands: grammar/semantic gate = `tree-sitter parse --quiet <fixture>` + `reify check <fixture>`;
wiring greps = paths below; empty-value sentinel = `Value::Undef` / `None` / `(0,0,0)`-on-unknown-name.

---

## α — appearance vocabulary stdlib module

| Capability | Evidence | Verdict |
|---|---|---|
| `enum Finish` / `structure def Color` (both-fields) / `Appearance` / `trait Visual` parse | grammar-fixture `docs/prds/v0_6/fixtures/appearance_surface.ri` → `tree-sitter parse --quiet` exit 0, **0 ERROR nodes** | **PASS** |
| same surface type-checks (no `unresolved type`/`unresolved name`) | `reify check docs/prds/v0_6/fixtures/appearance_surface.ri` → exit 0, "All constraints satisfied." | **PASS** |
| nested ctor defaults (`Appearance(color: Color(named:…, r:…))`) + qualified-enum default (`Finish.Satin`) + trait param default (`Visual{appearance=Appearance()}`) | same fixture exercises all; precedents `io.ri:131` (`STEPVersion.AP214`), `io_formats.ri:19` (`DisplayStyle()`), `materials_mechanical.ri:48` (`TemperatureDependent` trait default) | **PASS** |
| new module loadable before materials_mechanical | `stdlib_loader.rs:56` registers materials_mechanical; α inserts `materials_appearance` registration earlier (producer:task-α, this leaf) | **PASS (self, wired by α)** |

## β — `Material : Visual` + Rust resolution seam

| Capability | Evidence | Verdict |
|---|---|---|
| `Material` struct exists, no defaults (additive `appearance` is back-compat) | `grep:materials_mechanical.ri:73` `structure def Material { name; density; youngs_modulus }` ("no defaults") | **PASS** |
| `Material(...)` ctor + struct-ctor-default eval to **non-`Undef`** | ambient-default-material.md §2 ("`Material(name:…, density:…)` ctor evaluation is verified working", probe 2026-06-10); `Provenance(...)`/`DisplayStyle()` defaults eval (`io_formats.ri:19`) | **PASS (field-population)** |
| multi-trait conformance `: A + B` (for `Material : Visual`, and γ's library) | `grep:kinematic.ri:128` `Prismatic : DrivingJoint + HasMotion`; gate fixture `DemoAlloy : DemoElastic + Visual` parses+checks | **PASS** |
| `resolve_color` (hex exact + RAL seed + unknown→warn) | NEW Rust — producer:task-β (this leaf); hex→rgb is a parse identity (exact, no tolerance) | **producer-upstream (β-owned)** |
| `resolve_appearance(body)` reads `body.material.appearance` | NEW Rust over the `Physical.material` binding (`grep:structural_physical.ri:45`) + the `Visual` member (α) — producer:task-β | **producer-upstream (β-owned, α upstream)** |
| unknown-name **rejection** (`W_UNKNOWN_COLOR_NAME`, no silent black) | rejection-mechanism authored + observed in β (B4); not silent-default (`feedback_silent_defaults_pattern`) | **PASS (rejection-backed, β-owned)** |

## γ — library-wide appearance

| Capability | Evidence | Verdict |
|---|---|---|
| 4 library materials exist as `: ElasticMaterial` structures | `grep:materials_fea.ri:154` (Steel), `:192` (Al), `:230` (Ti), `:271` (ABS) | **PASS** |
| structure member-default eval to **non-`Undef`** (editorial appearance) | `MaterialPropertyProvenance` member defaults are the in-tree precedent (`materials_fea.ri:160-183`) | **PASS (field-population)** |
| `Visual` trait + `Appearance`/`Color` types in scope | producer:task-α (upstream); `Visual` member added via `: ElasticMaterial + Visual` (multi-trait, PASS above) | **producer-upstream (α)** |

## δ — 3MF per-body color egress (LEAF / integration gate)

| Capability | Evidence | Verdict |
|---|---|---|
| `write_3mf` + `ThreeMfOptions` + `W_3MF_NO_MATERIALS` gate wired on the export path | `grep:reify-ir/src/geometry.rs:2569` (`write_3mf`), `:2517-2520` (`include_materials`/`include_colors`), `:2537` (`"W_3MF_NO_MATERIALS"`), `:2692` (gate); task #4286 `done`, merged `84da8ea` | **PASS (wired on main)** |
| mesh egress per-body color channel (`Option<Rgb8>`) | NEW IR field — producer:task-δ (this leaf) | **producer-upstream (δ-owned)** |
| `engine_build` populates color from `resolve_appearance` on the **production** path (not test-only) | NEW wiring — producer:task-δ; consumes β's `resolve_appearance` (β upstream) | **PASS (field-population: δ writes real RGB; β upstream)** |
| 3MF `<basematerials>`/color is a writable OPC resource | fixed 3MF/OPC schema (3MF Core spec); exact bytes (no tolerance) — `zip` container already used by `write_3mf` (#4286) | **PASS** |
| no capability owned **downstream** of δ | β (`resolve_appearance`) is upstream; #4286 is `done` upstream; no leaf depends-on δ | **PASS (DAG-direction: no inversion)** |

---

**Numeric floor:** N/A — no FEA/solver/accuracy bound is asserted (color = exact bytes; no AABB/round-trip
tolerance). G6 branches 1/2 do not fire.

**Net:** 0 FAIL bindings. The two NEW capabilities (`resolve_*`, β; mesh color channel + egress, δ) are
each produced **upstream of or by** the leaf that asserts them; every consumed substrate (types via α,
`Material`/`Physical.material`, `write_3mf`/#4286) is wired on main or upstream in the batch DAG.
