# PRD — Typed FEA Authoring Surface (Gravity·Load, material unwrap, caller migration)

**Milestone:** v0_6 · **Status:** active · **Approach:** B + H (contract + two-way boundary tests on the solver-bridge + conformance seams) · **Authored:** 2026-06-09

**Origin:** interactive `/unblock 4094` (2026-06-09). Task **4094** ("migrate FEA callers/fixtures to typed Load/Support, drop `List<Real>`/`ConstitutiveLawInput` workarounds") was **cancelled as superseded**; Leo chose (Option D) to fold the whole typed-FEA authoring cluster into this one capability PRD. Disposition record: `~/.claude/projects/-home-leo-src-reify/memory/reference_unblock_4094_typed_fea_fold_prd.md`. Spawn-brief contract: `~/.claude/spawn-briefs/typed-fea-authoring-surface-prd-brief.md`.

**Empirically verified against the live compiler this session (base `main` @ post-4369, prebuilt `target/debug/reify check`).** Two stale premises in the brief were corrected during authoring — see **Pre-conditions** §3.1 and §3.2. The premise corrections *strengthened* the design (the material-unwrap is cheaper than feared; the gravity bridge is real wiring, not a mirror of a non-existent path).

---

## 1. Goal & user-observable surface

Let a `.ri` author write an FEA problem with **fully typed Load / Support / Material values and no compiler-workaround shims** — and have it parse, type-check, and (for the direct-solve path) run. Today the authoring surface leaks three workarounds: a `ConstitutiveLawInput(law:)` wrapper around every material, a retired-builtin/`List<Real>` placeholder soup that silently accepts garbage, and **no typed home for gravity** at all.

**Before (today — every line a workaround):**
```reify
let ci = ConstitutiveLawInput(law: Steel_AISI_1045())          // wrapper shim
let r  = solve_elastic_static(ci.law, l, w, h, [tip], [mt], ElasticOptions())
// multi_load_bracket.ri:
loads: [point_load("load_face", vec3(0N, -5000N, 0N))]          // retired builtin → Undef
loads: [gravity(5 * STANDARD_GRAVITY())]                        // kind-Map, no typed home
// LoadCase.loads : List<Real>                                  // placeholder; accepts anything
```

**After (this PRD):**
```reify
let r = solve_elastic_static(Steel_AISI_1045(), l, w, h, [tip], [mt], ElasticOptions())  // direct
loads: [PointLoad(point: "load_face", force: 5000.0, direction: [0.0, -1.0, 0.0])]        // typed
loads: [Gravity(magnitude: 5 * STANDARD_GRAVITY())]                                        // typed Load
// LoadCase.loads : List<Load>   (a non-Load element is now a conformance error)
```

**Consumer / user-observable signals** (the chain terminates at the `.ri` author via `reify check` + a runnable CI example — no orphan producers):
- `reify check examples/fea_cantilever_smoke.ri` compiles clean with the material passed **directly** — no `ConstitutiveLawInput`. (Today this exact pass errors: `type 'Steel_AISI_1045' does not conform to trait 'ConstitutiveLaw'`.)
- `reify check examples/multi_load_bracket.ri` compiles clean with typed `PointLoad` + `Gravity` — no retired `point_load`/`gravity` builtins, no `List<Real>` placeholder.
- A `LoadCase` constructed with a **non-`Load`** element (a bare `Real`, a retired-builtin `Undef`, or a `gravity()` kind-Map) now emits a `E_*`/`TypeNotConformingToTrait` diagnostic — the placeholder no longer silently accepts garbage.
- A live `solve_elastic_static([Gravity(...)], ...)` self-weight solve produces a **nonzero downward** displacement field that **scales linearly** with gravity magnitude and material density (property signal — see §4.3, no tight numeric bound).

---

## 2. Sketch of approach

Three coupled deliverables, one authoring surface. All three are pure-`.ri` + loader + one eval-trampoline arm; **no reify-compiler Rust** is required (the type-checker capabilities they rely on already exist — §3).

- **(a) `Gravity : Load` + solver bridge.** Add `structure def Gravity : Load` (scalar magnitude + dimensionless direction, mirroring `PointLoad` — §4.1) to `fea_multi_case.ri` alongside the other `Load` conformers, and bridge it into the live solver: add a `Gravity` arm to `extract_loads` (`crates/reify-eval/src/compute_targets/elastic_static.rs:1520`) that computes `body_force = ρ · magnitude · direction` (force-per-volume, N·m⁻³) and feeds the **existing, tested** kernel primitive `apply_body_force` (`crates/reify-solver-elastic/src/boundary/neumann.rs`). This routes through the `@optimized("solver::elastic_static")` ComputeNode trampoline — the existing op-execute / ComputeNode-dispatch seam (`docs/prds/v0_3/engine-integration-norm.md` §3.1/§3.4); no new seam.
- **(b) Material unwrap (supertrait).** Make `trait ElasticMaterial : ConstitutiveLaw`. Because the function-argument conformance pass follows trait **refinement chains transitively** (`satisfies_trait_bound` → `trait_satisfies`, `entity.rs:3732/3747`; verified §3.1), `Steel_AISI_1045` then *transitively* conforms to `ConstitutiveLaw` and may be passed **directly** to `solve_elastic_static(material: ConstitutiveLaw)` — the `ConstitutiveLawInput` wrapper is removed from callers and the shim retired.
- **(c) Caller migration + `LoadCase` tightening.** Migrate `multi_load_bracket.ri` (`point_load`/`gravity` → `PointLoad`/`Gravity`) and drop the wrapper from `fea_cantilever_smoke.ri`; then tighten `LoadCase.loads : List<Real>` → `List<Load>` and `LoadCase.supports : List<Real>` → `List<Support>` (`fea_multi_case.ri:89/98`), which **activates conformance enforcement** on the previously-placeholder fields.

---

## 3. Pre-conditions (substrate — all verified this session)

Every substrate capability this PRD assumes was confirmed against the live compiler. **No grammar work is queued** — all syntax is existing (`grammar_confirmed = true` for every leaf).

| Substrate | Status | Evidence |
|---|---|---|
| Supertrait grammar `trait X : Y` | exists, widely used | `DrivingJoint : Joint` (kinematic.ri:84), `Conductive : ElectricallyCharacterized` (materials_electrical.ri:64), `Watertight : Closed + Manifold` (geometry_traits.ri:74), … |
| Refinement-chain conformance on fn args | live | `phase_fn_arg_conformance` (lib.rs:451) → `check_fn_arg_conformance` → `satisfies_trait_bound`/`trait_satisfies` walk refinements transitively (conformance/mod.rs:818-822, entity.rs:3747) |
| `Load`/`Support`/`ConstitutiveLaw`/`ElasticMaterial` marker traits | exist | fea_types.ri:32-35, constitutive.ri:74, materials_fea.ri:88 |
| `solve_elastic_static(loads: List<Load>, supports: List<Support>)` | landed (task 4093) | solver_elastic.ri:526-537 |
| `Gravity : Load` scalar-magnitude+direction type surface | **compiles + conforms + type-checks in `List<Load>`** (probed) | user-level fixture compiled clean this session |
| `Acceleration` dimension + `STANDARD_GRAVITY()` as a param default | exists | units.ri:133 (`STANDARD_GRAVITY() -> Acceleration`) |
| Kernel `apply_body_force` (force-per-volume → nodal loads) | exists, tested | reify-solver-elastic/src/boundary/neumann.rs (`integrate_body_force_generic:137`) |

### 3.1 Premise correction #1 — the wrapper is load-bearing, but the `.ri` comments explaining *why* are stale

The brief and the `.ri` doc-comments attribute the wrapper to **exact type equality** ("`Steel_AISI_1045()` has type `StructureRef`, which does not match `TraitObject(ConstitutiveLaw)`"; solver_elastic.ri:484-499, fea_cantilever_smoke.ri:32-45). That explanation predates the task-4081/4232 conformance pass and is **stale**. The real mechanism: a scalar trait-object param is an overload-resolution **wildcard** (type_compat.rs:664), and a **post-pass** validates conformance (`phase_fn_arg_conformance`). The wrapper is load-bearing because `Steel_AISI_1045` conforms to `ElasticMaterial`, **not** `ConstitutiveLaw` — *not* because of exact equality. **Empirically confirmed:** a direct pass errors with `type 'Steel_AISI_1045' does not conform to trait 'ConstitutiveLaw' required by param 'material'`; the wrapped form compiles clean. This is exactly the error a supertrait removes — so deliverable (b) is cheaper than the brief's "extract `MaterialPropertyProvenance` + reorder whole modules" path (§4.2): only the empty **marker** must move earlier, untouching `MaterialPropertyProvenance` and the `fdm_correlations.ri` ripple.

### 3.2 Premise correction #2 — there is no live eval-layer "gravity-kind solver path" to mirror

The brief asks to "wire the solver dispatch so a typed `Gravity` is consumed **like the existing `gravity`-kind path**." There is no such live path. `gravity()` (loads.rs:134) is a **constructor** that emits a `{kind:"gravity", acceleration}` Map; the live solver's `extract_loads` (elastic_static.rs:1520) only dispatches `PointLoad` and `PressureLoad` by `type_name` and **ignores everything else**. The kernel *has* `apply_body_force`, but nothing in the eval layer bridges gravity to it. So (a)'s solver wiring is **real new bridging** (Gravity → `ρ·g` → `apply_body_force`), not a re-use of an existing dispatch. (Corollary: `multi_load_bracket.ri` "passes `reify check`" today only because its retired-`point_load`→`Undef` and `gravity()`-Map loads are silently swallowed by the `List<Real>` placeholder and the example never live-solves — a latent fake that (c)'s tightening converts into an enforced contract.)

---

## 4. Resolved design decisions

### 4.1 `Gravity` carries a scalar magnitude + a dimensionless direction (not a `Vector3<Acceleration>`)

Mirror `PointLoad`'s established field shape (force scalar + unit `direction : List<Real>`, fea_multi_case.ri:296-320):
```reify
structure def Gravity : Load {
    param magnitude : Acceleration = STANDARD_GRAVITY()     // downward implied by default direction
    param direction : List<Real>  = [0.0, 0.0, -1.0]        // dimensionless unit vector (−Z)
}
```
**Rationale:** (i) the `Vector3<Acceleration>` form has real literal friction — `vec3(0 m/s^2, …)` **fails to parse** (probed this session; acceleration has no compound `m/s^2` literal, only the `1m/(1s*1s)` build form). The scalar form composes cleanly (`STANDARD_GRAVITY()`, `5 * STANDARD_GRAVITY()`). (ii) The named consumer already writes a scalar magnitude (`gravity(5 * STANDARD_GRAVITY())`, multi_load_bracket.ri:74). (iii) `direction` retains generality (centrifuge/lateral-g) without the literal cost. The probed form compiles, conforms to `Load`, and type-checks in `List<Load>`. Body force in the solver bridge is `ρ · magnitude · direction`.

### 4.2 Material unwrap = supertrait, with the *cheap* load-order fix (Leo, 2026-06-09)

`trait ElasticMaterial : ConstitutiveLaw`. The only blocker is load order: `ConstitutiveLaw` is declared in `constitutive.ri` (loader slot ~#9) which loads **after** `materials_fea.ri` (slot ~#8, declares `ElasticMaterial`), so the supertrait reference is a forbidden forward-ref. **Fix:** relocate *only the empty `trait ConstitutiveLaw { }` marker* to load before `materials_fea` (cleanest: declare it at the top of `materials_fea.ri` above `ElasticMaterial`, or in `structural_physical.ri` slot ~#7; remove the duplicate def from `constitutive.ri`, leaving a pointer). The `Orthotropic`/`TransverseIsotropic`/`AnisotropicMaterial` definitions stay in `constitutive.ri` and still see `ConstitutiveLaw` via the growing prelude. This is **strictly cheaper** than the brief's "extract `MaterialPropertyProvenance` + reorder whole modules" plan and avoids the `fdm_correlations.ri` ripple entirely (nothing between the two slots references the marker). Rejected alternatives: **reify-compiler trait-coerce** (more general but touches compiler Rust — bigger/riskier, deferred to a future general-coercion PRD if ever needed) and **keep-the-wrapper** (leaves the noisy surface).

### 4.3 The Gravity self-weight leaf asserts a *property*, not a numeric bound (G6-safe)

The (a) solver-bridge leaf is validated by a self-weight solve whose assertions are **physical properties**, never a tight analytical deflection — sidestepping the P1-tet bending-lock / Dirichlet-`k` hazards (overlay G6; precedents esc-3453, esc-3770):
- **Sign:** clamped bar under `Gravity(direction:[0,0,-1])` displaces net **downward**.
- **Linearity:** `2×` magnitude ⇒ `2×` displacement (within solver tol).
- **Density-scaling:** `2×` material density ⇒ `2×` displacement.
- **Zero:** `magnitude = 0` (or empty load list) ⇒ no gravity contribution.

If any *absolute* comparison is wanted, it reuses the existing cantilever smoke's **±50%** P1-tet method-error budget (fea_cantilever_smoke.ri:18-19), never a tighter figure.

### 4.4 `LoadCase` tightening is gated on (a) + the bracket migration

Tightening `LoadCase.loads`→`List<Load>` must land **after** `Gravity : Load` exists (else the bracket's "transport" case has no typed home) **and after** `multi_load_bracket.ri` is migrated (else its `point_load`/`gravity` forms fail the now-enforced conformance). Encoded as intra-PRD deps ζ ← α, ε (§6).

---

## 5. Out of scope

- **Selector FIELD migration** (`PointLoad.point : VertexSelector`, `FixedSupport.target : FaceSelector`, …) — owned by the deferred PRD `docs/prds/v0_6/fea-load-support-selector-migration.md` (tasks **4368–4371**, gated on 4118/4119/4120 + 4092). This PRD keeps the `String` selector placeholders untouched. **Related, not absorbed** — see §6.
- **`solve_load_cases` live engine integration** (task **3009**) — `multi_load_bracket.ri` keeps its `MultiCaseResult(...)` constructor stub; this PRD does **not** make the multi-case path live-solve. The (a) self-weight leaf uses **direct** `solve_elastic_static`, not `solve_load_cases`.
- **`FEAMaterialInput` removal.** Verified vestigial this session — `solve_buckling(Steel_AISI_1045(), …)` already compiles clean *without* the wrapper (Steel conforms to `ElasticMaterial` directly). But `FEAMaterialInput` is shared with `solve_buckling`; the supertrait approach does not touch it, satisfying the brief's "preserve `solve_buckling`" caution. Removing the vestigial `FEAMaterialInput` is a separate cleanup, deliberately left out to keep blast radius minimal.
- **Heterogeneous anisotropic material field** call surface (`Field<Point3, AnisotropicMaterial>`, anisotropic PRD task ε) — untouched.
- **Dimensional tightening** of other `Real` placeholders (`PointLoad.force`→`Scalar<Force>`, `direction`→`Vec3`, the `Vector3`-typed traction/body-force densities) — that is the broader Real-placeholder audit, not this PRD.

---

## 6. Cross-PRD relationship & seam ownership (G4)

| Seam | Owner | Resolution |
|---|---|---|
| Caller-migration files `multi_load_bracket.ri` + `fea_multi_case.ri` (also touched by selector PRD 4370) | **This PRD** owns the *load-kind* migration (`point_load`/`gravity` → typed ctors + `List<Load>` tightening). 4370 owns the *selector-field* migration (`String` → typed selector). | **Independent — NO dep edge** (Leo, 2026-06-09). 4368–4371 are gated on deeper deps (4118/4119/4120 + 4092) and will not land soon; this PRD is unblocked now. The typed `PointLoad`/`FixedSupport`/`Gravity` ctors are stable, so whichever migration lands second rebases its hunk onto the other. (This **overrides** the brief's suggestion to wire 4370 → dep(this PRD's migration leaf); Leo chose the looser coupling.) |
| `solver_elastic.ri` / `materials_fea.ri` / `constitutive.ri` / `stdlib_loader.rs` material-unwrap edits | **This PRD** | Seam-free w.r.t. 4368–4371 (those files are not touched by the selector PRD). |
| `Gravity` → `apply_body_force` bridge in `elastic_static.rs` | **This PRD** | Plugs into the existing `@optimized("solver::elastic_static")` ComputeNode trampoline (engine-integration-norm §3.1/§3.4) — not a new seam. |

Sibling axes (do not conflate, per brief): **signature typing** `List<Load>`/`List<Support>` = **done (task 4093)**; **selector fields** = deferred (4368–4371). This PRD is the middle axis: caller migration + material unwrap + gravity-as-Load.

---

## 7. Boundary-test sketch (the H in B+H — two-way tests on the high-stakes seams)

FEA is a G5 load-bearing seam (overlay §G5). Two seams get reciprocal boundary tests so neither side can drift silently:

1. **Gravity → kernel body-force seam (leaf β).**
   - *Eval → kernel direction:* a `Gravity(magnitude: m, direction: d)` instance over a material of density ρ produces the body-force vector `ρ·m·d` (N·m⁻³) handed to `apply_body_force` — assert the bridged `[f64;3]` for a known fixture.
   - *Kernel → result direction:* feeding that vector through `apply_body_force` yields the expected nodal-force pattern (reuse/extend the existing `neumann.rs` body-force kernel test). Together they pin: a units bug on either side fails one direction.
2. **Supertrait conformance seam (leaf γ).**
   - *Positive:* `solve_elastic_static(Steel_AISI_1045(), …)` (no wrapper) compiles clean — the exact pass that errors today.
   - *Negative / preserve:* `solve_buckling(Steel_AISI_1045(), …)` and every existing `solve_buckling` caller still compile (the supertrait does not perturb the `ElasticMaterial` param), and a genuinely non-conforming value (e.g. a bare `box(...)`) at the `material` slot still errors. Guards against the supertrait widening conformance too far.

---

## 8. Decomposition plan (one leaf per bullet; observable signal named; substrate confirmed)

DAG roots **α** and **γ** are independent and parallelizable. Every leaf's `grammar_confirmed = true` (no novel syntax — §3).

- **α — `Gravity : Load` type surface.** Add `structure def Gravity : Load { magnitude, direction }` (§4.1) to `fea_multi_case.ri` beside the other `Load` conformers. **Signal:** `reify check` on a fixture constructing `Gravity()`, `Gravity(magnitude: 5*STANDARD_GRAVITY())`, `Gravity(direction:[1,0,0])`, and `[g] : List<Load>` compiles clean and conforms (the exact form probed green this session). *Deps:* none.
- **β — Gravity solver bridge + self-weight property test.** Add a `Gravity` arm to `extract_loads` (elastic_static.rs) computing `body_force = ρ·magnitude·direction` and calling `apply_body_force`; thread material density from `value_inputs[0]`. **Signal:** a `crates/reify-eval/tests/` self-weight e2e where `solve_elastic_static([Gravity(...)], …)` on a clamped bar yields a nonzero **downward** displacement field satisfying the **linearity / density-scaling / sign / zero** properties of §4.3 (no tight numeric bound). Two-way boundary test per §7.1. *Deps:* α.
- **γ — Supertrait material unwrap.** Relocate the empty `trait ConstitutiveLaw { }` marker before `materials_fea` (§4.2); add `trait ElasticMaterial : ConstitutiveLaw`; refresh the stale `.ri` doc-comments (solver_elastic.ri, fea_cantilever_smoke.ri) to describe the refinement-chain mechanism. **Signal:** a fixture passing `Steel_AISI_1045()` **directly** (no `ConstitutiveLawInput`) to `solve_elastic_static` compiles clean — the pass that today errors `does not conform to trait 'ConstitutiveLaw'`; plus `solve_buckling` still compiles (§7.2). *Deps:* none.
- **δ — Drop the wrapper from callers + retire the shim.** Remove `ConstitutiveLawInput(law:)` from `fea_cantilever_smoke.ri` (pass material directly); retire the now-unused `ConstitutiveLawInput` struct from `solver_elastic.ri` (leave `FEAMaterialInput` in place — §5). **Signal:** `reify check examples/fea_cantilever_smoke.ri` clean; `grep -r ConstitutiveLawInput` finds no live caller. *Deps:* γ.
- **ε — Migrate `multi_load_bracket.ri`.** `point_load("load_face", vec3(...))` ×2 → `PointLoad(point:, force:, direction:)`; `gravity(5*STANDARD_GRAVITY())` → `Gravity(magnitude: 5*STANDARD_GRAVITY())`. **Signal:** `reify check examples/multi_load_bracket.ri` clean with typed ctors and no retired builtins. *Deps:* α.
- **ζ — Tighten `LoadCase`.** `loads : List<Real>` → `List<Load>`, `supports : List<Real>` → `List<Support>` (fea_multi_case.ri:89/98); update the header notes. **Signal:** a typed `LoadCase(loads:[PointLoad…, Gravity…], supports:[FixedSupport…])` compiles; a **negative control** — a `LoadCase` with a bare `Real`, a retired-builtin `Undef`, or a `gravity()` kind-Map in `loads` — now emits a `TypeNotConformingToTrait` diagnostic (enforcement activated). *Deps:* α, ε.
- **η — Integration gate (critical).** The end-to-end leaf signal from the brief's G2: the migrated `multi_load_bracket.ri` + a `fea_multi_case` fixture + `fea_cantilever_smoke.ri` all parse, type-check, and run with typed `PointLoad`/`FixedSupport`/`Gravity`, **no** retired builtins, **no** `List<Real>` placeholder, **no** `ConstitutiveLawInput`; AND the β self-weight property-solve is green. A single CI `.ri` example (or test harness) is the gate. *Deps:* β, δ, ε, ζ.

```
α ──┬─► β ──────────────┐
    ├─► ε ──► ζ ─────────┤
    └────────┘           ├─► η (gate)
γ ──► δ ─────────────────┘
```

---

## 9. Open (tactical) questions

These are implementation-time choices, not open design questions (the META gate is satisfied without them):

- **Marker-relocation home (γ):** top of `materials_fea.ri` vs `structural_physical.ri`. Either loads before slot #8; pick whichever keeps the smallest diff and passes the stdlib-load assertion. No semantic difference.
- **Density source in the gravity bridge (β):** read `density` from the classified material `StructureInstance` (`value_inputs[0]`) at `extract_loads` time. If a material ever lacks `density`, fall back to zero body force + an info diagnostic (every `ElasticMaterial` conformer declares `density` today — materials_fea.ri:91 — so this is defensive only).
- **`Gravity` body-force vs per-node convention:** `apply_body_force` expects force-per-volume integrated over elements; confirm the `ρ·g` units land as N·m⁻³ before integration (the boundary test §7.1 pins this).
- **Doc-comment refresh scope (γ):** the stale "exact type equality" comments recur in several `.ri` files; refresh at least solver_elastic.ri + fea_cantilever_smoke.ri; a broader sweep is optional.
