# Capability manifest — typed-fea-authoring-surface

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/typed-fea-authoring-surface.md`. Each capability a task's signal asserts is bound to evidence; any binding resolving to `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor` blocks the batch. Verified 2026-06-09 against base `main` @ post-4369 (`bb2f47e062`).

## Summary

| Leaf | Binding class | Verdict |
|---|---|---|
| α | grammar-fixture (`structure def Gravity : Load` — supertrait + `param … : Acceleration`) | **PASS** (existing grammar; widely used) |
| α | producer-exists (`trait Load`, `Acceleration`, `STANDARD_GRAVITY()`) | **PASS** |
| β | wired-on-main (`extract_loads` dispatch seam + `apply_body_force` kernel tested) | **PASS** |
| β | field-population (self-weight solve writes a real displacement field, not `Undef`) | **PASS** (rides existing elastic-solve path) |
| β | **numeric (G6)** — sign/linearity/density-scaling/zero **properties**, no tight bound | **PASS** (no floor to cross; §4.3) |
| γ | wired-on-main (refinement-chain `phase_fn_arg_conformance` live) + loader-order fix feasible | **PASS** |
| δ | producer-upstream (γ removes the conformance error) + grep no-live-caller | **PASS** |
| ε | producer-upstream (α supplies `Gravity`; typed `PointLoad` exists) | **PASS** |
| ζ | producer-upstream (α, ε) + diagnostic-emission (`TypeNotConformingToTrait` real) | **PASS** |
| η | integration-gate (all caps upstream: β, δ, ε, ζ) | **PASS** (C-as-gate; the H signal) |

**No FAIL bindings.** No tight numeric bounds anywhere — the one numerical leaf (β) deliberately asserts physical *properties*, sidestepping the P1-tet bending-lock / Dirichlet-`k` hazards (overlay G6; precedents esc-3453, esc-3770, esc-3821-44). G3 is a near-no-op: every assumed capability already exists wired-on-main; the only "new" substrate is the loader-order relocation in γ, which is a move not a creation.

## α — `Gravity : Load` type surface

- **Capability:** `structure def Gravity : Load { magnitude : Acceleration, direction : List<Real> }` parses, conforms to `Load`, and type-checks in `List<Load>`.
  - **Evidence (grammar-fixture / anti-mismatch):** supertrait `structure def X : Y` and `param … : Acceleration` are existing grammar — `grep` `Watertight : Closed + Manifold` (geometry_traits.ri:74), `DrivingJoint : Joint` (kinematic.ri), and the established `Load` conformers `PointLoad : Load` / `PressureLoad : Load` / `TractionLoad : Load` / `BodyForce : Load` (fea_multi_case.ri:296/363/391/421). `grammar_confirmed=true`; **no grammar producer task.** PRD §4.1 records the exact form probed green this session. **PASS.**
- **Capability:** `Load` marker trait, `Acceleration` dimension, `STANDARD_GRAVITY()` default all exist.
  - **Evidence (producer-exists):** `trait Load { }` (fea_types.ri:32); `STANDARD_GRAVITY() -> Acceleration` (units.ri:133) confirms both the fn and the `Acceleration` return-type dimension; `PointLoad` field shape (`force : Real`, `direction : List<Real> = [0,0,-1]`, fea_multi_case.ri:298/319) is the mirrored precedent. **PASS.**
- *Deps:* none (root).

## β — Gravity solver bridge + self-weight property test

- **Capability:** a `Gravity` arm in `extract_loads` computes `body_force = ρ·magnitude·direction` and calls the kernel `apply_body_force`.
  - **Evidence (wired-on-main / anti-orphan):** the dispatch seam is live — `extract_loads` (elastic_static.rs:1520) matches `data.type_name == "PointLoad"` (:1532) and `"PressureLoad"` (:1557) and **ignores everything else** (PRD §3.2 corollary confirmed); β adds the third arm at the same production seam, routed through the `@optimized("solver::elastic_static")` ComputeNode trampoline (engine-integration-norm §3.1/§3.4). The kernel primitive `apply_body_force` is `pub` and **tested** — `grep` neumann.rs:242 (def) → `integrate_body_force_generic` (:137), with the `apply_body_force_p1_*`/`_p2_*` reference-tet + volume-scaling + accumulate-linearly tests (neumann.rs:723–1040). **PASS** (β is the wiring task; both the seam it plugs into and the primitive it calls are on-main, not test-only).
- **Capability (field-population):** the self-weight solve produces a **non-`Undef`** displacement field.
  - **Evidence:** the bridge feeds the **existing** elastic assembly/solve path that already produces a real displacement field for `PointLoad` (the same `f` force vector, elastic_static.rs:1041); adding a body-force contribution to that vector inherits the populated-field path. Density is read from the classified material `value_inputs[0]` — `param density : Density` is declared on `trait ElasticMaterial` (materials_fea.ri:91) and every conformer carries a concrete value (Steel 7850, Al 2700, Ti 4430, …; materials_fea.ri:135/173/211/252). **PASS** (not a `tests/`-only construction).
- **Capability (G6 — numeric):** sign (downward), linearity (`2×` magnitude ⇒ `2×` disp), density-scaling (`2×` ρ ⇒ `2×` disp), zero (`magnitude=0` ⇒ no contribution).
  - **Binding (numeric — PASS, no floor crossed):** these are **physical properties of the linear-elastic operator**, exact up to solver tolerance — there is **no absolute-accuracy bound** to compare against a method floor, so the bending-lock / Dirichlet-`k` floors do not bind (PRD §4.3). If any *absolute* deflection comparison is later wanted it reuses the existing cantilever-smoke **±50 %** P1-tet method-error budget (fea_cantilever_smoke.ri:18-19), which is **above** the 9–10 % bending-lock floor. **PASS.**
- **Two-way boundary test (the H, §7.1):** eval→kernel (`ρ·m·d` handed to `apply_body_force` for a known fixture) + kernel→result (extend the existing neumann.rs body-force test). A units bug on either side fails one direction.
- *Deps:* α.

## γ — Supertrait material unwrap

- **Capability:** `trait ElasticMaterial : ConstitutiveLaw` makes `Steel_AISI_1045` *transitively* conform to `ConstitutiveLaw`, so it passes **directly** to `solve_elastic_static(material: ConstitutiveLaw)`.
  - **Evidence (wired-on-main / refinement-chain live):** the fn-arg conformance post-pass `phase_fn_arg_conformance` is called from the production compile path (lib.rs:451) → `check_fn_arg_conformance` (conformance/mod.rs:270) → `satisfies_trait_bound` / `trait_satisfies` (entity.rs:3732/3747) walk refinement chains transitively. The error a supertrait removes is real and emitted today — `type '{}' does not conform to trait '{}' required by param '{}'` (conformance/mod.rs:541). **PASS.**
  - **Evidence (loader-order fix feasible):** the forward-ref is real and the fix is a *move*, not a new capability — `materials_fea.ri` loads at stdlib_loader.rs:83 **before** `constitutive.ri` at :86; relocating the empty `trait ConstitutiveLaw { }` marker (constitutive.ri:74) to `structural_physical.ri` (slot :79) or the top of `materials_fea.ri` lands it before `ElasticMaterial` (materials_fea.ri:88). The `Orthotropic`/`AnisotropicMaterial` defs stay in `constitutive.ri` and see the marker via the growing prelude. **PASS** (cheaper than the brief's module-reorder; no `MaterialPropertyProvenance` / `fdm_correlations.ri` ripple — PRD §4.2).
- **Two-way boundary test (the H, §7.2):** positive (`solve_elastic_static(Steel_AISI_1045(), …)` no wrapper compiles — the pass that errors today) + negative/preserve (`solve_buckling` callers still compile; a bare `box(...)` at the `material` slot still errors). Guards against the supertrait widening conformance too far.
- *Deps:* none (root).

## δ — Drop the wrapper from callers + retire the shim

- **Capability:** `fea_cantilever_smoke.ri` passes the material directly (no `ConstitutiveLawInput`), and the `ConstitutiveLawInput` struct is retired.
  - **Evidence (producer-upstream):** the conformance error that *required* the wrapper is removed by **γ** (the `δ ← γ` dep); the `ConstitutiveLawInput` struct exists today (solver_elastic.ri:459) and is the exact shim to retire. **Signal** = `reify check examples/fea_cantilever_smoke.ri` clean + `grep -r ConstitutiveLawInput` finds no live caller. `FEAMaterialInput` is **left in place** (shared with `solve_buckling`, vestigial-but-out-of-scope — PRD §5). **PASS.**
- *Deps:* γ.

## ε — Migrate `multi_load_bracket.ri`

- **Capability:** `point_load("…", vec3(…))` ×2 → `PointLoad(point:, force:, direction:)`; `gravity(5·STANDARD_GRAVITY())` → `Gravity(magnitude: 5·STANDARD_GRAVITY())`.
  - **Evidence (producer-upstream):** typed `PointLoad` already exists (fea_multi_case.ri:296); `Gravity` is supplied by **α** (the `ε ← α` dep). The "before" state is confirmed — retired `point_load` at multi_load_bracket.ri:62/68 and `gravity(...)` at :74. **Signal** = `reify check examples/multi_load_bracket.ri` clean with typed ctors and no retired builtins. **PASS.**
- *Deps:* α.

## ζ — Tighten `LoadCase`

- **Capability:** `LoadCase.loads : List<Real>` → `List<Load>` and `.supports : List<Real>` → `List<Support>` (fea_multi_case.ri:89/98) **activates** conformance enforcement; a non-`Load` element now errors.
  - **Evidence (producer-upstream):** needs `Gravity : Load` (**α**) so the bracket's transport case has a typed home, and the migrated bracket (**ε**) so its forms pass the now-enforced conformance — the `ζ ← α, ε` deps (PRD §4.4). The placeholder fields exist today (`param loads : List<Real>` :89, `param supports : List<Real>` :98).
  - **Evidence (diagnostic-emission):** the negative control's asserted code is real and emitted on the production conformance path — `DiagnosticCode::TypeNotConformingToTrait` with a single label at the arg span (conformance/mod.rs:508/544). **Signal** = typed `LoadCase(loads:[PointLoad…, Gravity…], supports:[FixedSupport…])` compiles; a `LoadCase` with a bare `Real` / retired-builtin `Undef` / `gravity()` kind-Map in `loads` now emits `TypeNotConformingToTrait`. **PASS.**
- *Deps:* α, ε.

## η — Integration gate (critical)

- **Capability (end-to-end / C-as-gate):** the migrated `multi_load_bracket.ri` + a `fea_multi_case` fixture + `fea_cantilever_smoke.ri` all parse, type-check, and run with typed `PointLoad`/`FixedSupport`/`Gravity` — **no** retired builtins, **no** `List<Real>` placeholder, **no** `ConstitutiveLawInput`; AND the β self-weight property-solve is green.
  - **Evidence (DAG-direction / anti-inversion):** every required capability is produced by an **upstream** dep — β (gravity solve), δ (wrapper drop), ε (bracket migration), ζ (LoadCase tightening). None is owned by a task that depends on η. This is the G2 escape-hatch integration gate and the H boundary signal (§7); a single CI `.ri` example / test harness is the gate. **PASS.**
- *Deps:* β, δ, ε, ζ.

## DAG

```
α ──┬─► β ──────────────┐
    ├─► ε ──► ζ ─────────┤
    └────────┘           ├─► η (gate)
γ ──► δ ─────────────────┘
```

Intra-batch edges (9): β←α, δ←γ, ε←α, ζ←α, ζ←ε, η←β, η←δ, η←ε, η←ζ.
Out-of-batch edges (0): per PRD §6 / §5 the selector PRD 4370/4371 seam is **independent — NO dep edge** (Leo, 2026-06-09); the typed signature (task 4093) is already `done`, so no prerequisite remains.
