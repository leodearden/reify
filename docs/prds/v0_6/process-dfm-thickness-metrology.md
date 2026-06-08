# PRD: `std.process` thickness-metrology DFM — auto-measured min-wall + min-feature

**Status:** Ready to decompose · **Promoted from deferred stub:** 2026-06-08 · **Milestone:** v0_6+
**Approach:** **B (vertical slice) + H (contract + two-way boundary tests)** per `preferences_implementation_chain_portfolio`.
**Splits from:** `docs/prds/v0_6/process-dfm-geometry-metrology.md` (superseded) — this is the
**research-gated half**; the ship-now half (overhang + draft) is `process-dfm-overhang-draft.md`.
**Consumes (upstream, being decomposed concurrently):** `docs/prds/v0_6/process-dfm-overhang-draft.md` —
the auto-measurement check-time pass (`Engine::measure_dfm_rules`, sibling γ=4408), the
`dfm::diagnose` extension point (sibling δ=4409), and `DFMRule.subject : Solid` (sibling β=4407).
**Builds on (done substrate, on main):** kernel SDF primitives `#3095` (`realize_voxel_from_mesh` /
`sample_sdf_at`, merged `70f0449139`), the Mesh→Voxel capability descriptor `#3438`, the
in-realization conversion-executor structure `#4050`, and the medial-walk parallelization `#3182`.

> This PRD was promoted from the deferred forward-stub of the same name. The promotion ran a fresh
> `/prd` author pass (2026-06-08): a four-agent substrate sweep pinned the exact state of the
> eval-reachable solid→SDF wire, the human ratified **approach B** (the multi-kernel dispatcher
> route) over the corrected check-time alternative, and ratified shipping non-convex geometry gated
> by a conservative-bound boundary test. The build-time/check-time design question raised at the gate
> is resolved in §4 D1.

---

## §0 — What this PRD ships, and what changed on promotion

A DFM-minded designer annotates a thin-walled part with a `DFMRule` and `reify check`
**auto-measures** the realized solid's thinnest wall / feature against the process capability
(`min_feature_size`), flagging manufacturability violations **with no hand-declared measured
thickness** — exactly the surface the sibling `process-dfm-overhang-draft.md` ships for overhang /
draft, extended with a thickness category arm.

The thickness family was deferred (not the overhang/draft family) because it is gated on an
**eval-reachable solid→SDF wire that does not exist** — a C-17-shape orphan: the kernel SDF
primitives are done and on main but **stranded from the eval production path**. This PRD **owns and
builds that wire** (the §6 G4 seam). The promotion's substrate sweep corrected three framings the
stub carried:

1. **There are no Voxel/SDF ops on the `GeometryKernel` trait at all** (the trait is BRep-native;
   `geometry.rs:2317`) — so the stub's option (a) "the trait's Voxel ops return `OperationFailed`"
   was inaccurate. The realized voxel grid is an **opaque C++ FFI handle**; `sample_sdf_at` is
   point-by-point FFI (O(N³) FFI calls — a non-starter for the medial walk).
2. **`SampledField` (`reify-ir/src/value.rs:90`) is the convergence type** — a CPU-resident dense
   `Vec<f64>` buffer, sampled by pure-CPU trilinear interpolation (`medial.rs:602`). The production
   `read_vdb_file` path *produces* one; the thickness primitive `bidirectional_distances(sdf:
   &SampledField, …)` (`medial.rs:863`) *consumes* one; the densifier `lower_to_sampled`
   (`ingest.rs:294`, a reusable `pub fn`) already builds one from an `OpenVdbGridSource`.
3. **The multi-kernel dispatcher is production-wired** (`engine_build.rs:4045`, the per-op realization
   hot path) and already advertises `Convert{Mesh}→Voxel` (`register.rs:121`, from `#3438`), but the
   terminal Mesh→Voxel **execution** is not wired (§3). That gap — not missing algorithm research —
   is the whole of the wire.

---

## §1 — Consumer (G1)

**Named consumer: a DFM-minded designer who wants `reify check` to flag thin walls / thin features
WITHOUT hand-declaring the measured thickness.** The user annotates a part with a `DFMRule`
(`rule_name`, `severity`, `applies_to : Process`, `subject : Solid`); the engine realizes the
subject solid, builds an SDF, measures the thinnest wall (min-wall) and thinnest solid cross-section
(min-feature), compares each against the process capability `min_feature_size`, and emits a
`DFMSeverity`-tagged diagnostic. **User-observable surface: the CLI.** `reify check` emits
`W_DFM_MIN_WALL` / `E_DFM_MIN_FEATURE` (etc.) for a part that violates, and nothing for one that
conforms.

| Mechanism (introduced here) | Consumer |
|---|---|
| `OpenVdbKernel::ingest_mesh` (Mesh→Voxel execute) + `densify_grid_to_sampled` (Voxel handle → `SampledField`) | The `realize_solid_sdf` helper calls them to obtain a queryable SDF of a realized solid |
| `ConversionProjection::Voxelize` + the conversion-executor Mesh→Voxel stage (`reify-eval`) | The multi-kernel realization path (§3.3) executes the `BRep→Mesh→Voxel` chain demanded by `realize_solid_sdf` |
| `realize_solid_sdf(handle) -> Option<SampledField>` (the eval-side wire) | The thickness measurement selectors; `reify check` reflects the verdict; **first production consumer to demand a Voxel realization** |
| `min_wall_thickness` / `min_feature_size_measure` selectors (`reify-shell-extract` / `reify-eval`) | The auto-measurement pass's thickness arm |
| `measure_dfm_rules` **thickness arm** (extends sibling γ=4408) | A designer's `DFMRule` annotation — `reify check` auto-runs the thickness measurement per process category |
| `dfm::diagnose` thickness arms (`{W,E}_DFM_MIN_WALL` / `_MIN_FEATURE`, extends sibling δ=4409) | The pass routes each measurement + the rule's `DFMSeverity` through `diagnose` |

**Engine-integration-norm §3 seam (per `.claude/skills/prd/project.md` G1 sub-check).** The
solid→SDF wire plugs into the **existing §3.3 multi-kernel dispatch** seam — it fills in the
`Convert{Mesh}→Voxel` execution the descriptor `#3438` advertised as planning-only; **no new §3
entry** for the wire. The thickness measurement rides the **sibling's new check-time DFM-measurement
§3 entry** (sibling ε=4410) — it is a new *category arm* within that already-catalogued pass, not a
new seam. No norm change is owned by this PRD.

---

## §2 — Sketch of approach (the "what changes")

The slice is "build the wire, then two reductions, then one pass arm." The measurement primitive,
the densifier, the severity bridge, the declarative DFM surface, and the check-time pass **all
already exist** — the work is the four-gap Mesh→Voxel→`SampledField` execution path plus the two
reductions and the category arm.

### 2.1 The eval-reachable solid→SDF wire (this PRD OWNS it — approach B, §6 G4)

Route a realized BRep solid to a queryable SDF **through the multi-kernel dispatcher** (the §3.3
seam), so the SDF is a *realized representation* (forward-compatible with the unified build DAG, §4
D1) rather than a bespoke check-time kernel call. The chain
`tessellate(solid) → realize_voxel_from_mesh → densify → SampledField` exists in pieces; this PRD
assembles it by closing four pinned gaps:

1. **`OpenVdbKernel::ingest_mesh`** (`reify-kernel-openvdb/src/kernel_real.rs`) — implement the trait
   method to wrap the existing `realize_voxel_from_mesh_with_options` (today it falls through to the
   trait default `OperationFailed`, `geometry.rs:2545`). `voxel_size = h` (the honest-floor
   resolution, §3b); `narrow_band` from `MeshToVoxelOptions` (`mesh_to_voxel_options.rs:20`).
2. **`densify_grid_to_sampled(handle) -> SampledField`** (`reify-kernel-openvdb`) — extract the
   realized opaque voxel grid into a CPU-resident `SampledField`, reusing the existing
   `lower_to_sampled` / `OpenVdbGridSource` densifier (`ingest.rs:294`). This is the one genuinely
   new kernel primitive (no realized-grid→`SampledField` path exists today).
3. **`ConversionProjection::Voxelize` + the executor stage** (`reify-eval/src/dispatcher.rs` +
   `engine_build.rs:4125`) — extend `v03_conversion_projection` (`dispatcher.rs:597`, today
   `(BRep,Mesh)=Tessellate` only) with a `(Mesh,Voxel)=Voxelize` row, and make the in-realization
   conversion executor execute that stage via gap 1 (today it rejects any non-`(BRep,Mesh)` stage).
4. **`realize_solid_sdf(handle) -> Option<SampledField>`** (`reify-eval`) — the consumer-side wire:
   demand a Voxel realization of the subject (the **first production Voxel demander**), drive the
   dispatcher's `BRep→Mesh→Voxel` chain, densify (gap 2), and return the `SampledField`. **No OpenVDB
   kernel in the registry (`not(has_openvdb)`) → the demand is unsatisfiable → return `None`**, which
   the pass turns into a self-describing `Undef` + diagnostic (§4 D5), **never a fake number**.

The OpenVDB kernel is obtained **via the kernel registry** (`kernel_registry.rs`, the §3.3 factory),
not ad-hoc instantiation — ad-hoc instantiation would recreate the C-17 orphan in a new spot.

### 2.2 Min-wall measurement (`reify-shell-extract` / `reify-eval`)

Reuse `bidirectional_distances` (`medial.rs:863`); add a **min-reduction** over `d⁺ + d⁻` across
medial voxels (today `compute_medial_mask` uses the distances for the equality filter but the
*reduction* to a single min-wall scalar does not exist). Returns a conservative lower bound (§3b).

### 2.3 Min-feature measurement (`reify-shell-extract` / `reify-eval`)

SDF ridge-min on the same `SampledField`: `min_feature = 2 × min over ridge voxels of |φ|`. Measures
the **thinnest solid cross-section** (thin wall / rib / web) — **not** edge length, face diameter,
hole diameter, or gap-between-surfaces (gap overlaps the existing `Distance` / `min_clearance`
query). Pinned definition in §3b.

### 2.4 Thickness arm in the pass + `dfm::diagnose` (`reify-eval` / `reify-stdlib`)

Extend the sibling's `measure_dfm_rules` (4408) with a thickness category arm: for a `DFMRule`
whose process category carries `min_feature_size` (`Subtracting` / `Adding` / `Parting`),
`realize_solid_sdf(subject)`, run min-wall + min-feature, compare vs `min_feature_size`, route the
result + the rule's `DFMSeverity` through `dfm::diagnose`. Extend `diagnose` (`dfm.rs:249`,
sibling δ=4409) with `{I,W,E}_DFM_MIN_WALL` / `_MIN_FEATURE` arms. **No kernel / unsatisfiable SDF
demand → `Indeterminate` / no-op, never a false `Violated`** (the C1 invariant, §7).

---

## §3 — Pre-conditions / substrate verification (G3)

Verified during the 2026-06-08 four-agent substrate sweep; file:line on main. Verdicts:
**exists+wired** (reusable on the production path), **exists, scope-limited / unreached** (present
but not driven end-to-end), **absent** (this PRD builds it).

| Construct / capability | Verdict | Evidence |
|---|---|---|
| `SampledField` CPU-resident dense buffer + CPU trilinear sampling | **exists+wired** | `struct SampledField { … data: Vec<f64> … }` (`reify-ir/src/value.rs:90`); `sample_at_world` pure-CPU trilinear (`reify-shell-extract/src/medial.rs:602`) |
| Thickness primitive `bidirectional_distances` (d⁺+d⁻, parallelized) | **exists+wired** | `medial.rs:863`; walk parallelized by `#3182` (done). Consumed via `shell_extract_compute.rs` |
| Kernel SDF primitives `realize_voxel_from_mesh` / `sample_sdf_at` | **exists+wired** (`#3095`) | `reify-kernel-openvdb/src/kernel_real.rs:79,170`; `realize_voxel_from_mesh_with_options` (`:106`) reshapes a `Mesh` |
| Grid→`SampledField` densifier `lower_to_sampled` (reusable `pub fn`) | **exists+wired** | `reify-kernel-openvdb/src/ingest.rs:294`; takes any `&OpenVdbGridSource`, not file-coupled |
| Multi-kernel dispatcher (BFS planning, §3.3) | **exists+wired** | `dispatch(...)` (`reify-eval/src/dispatcher.rs:671`) called on the per-op realization hot path (`engine_build.rs:4045`) |
| OpenVDB `Convert{Mesh}→Voxel` capability descriptor | **exists+wired** (`#3438`) | `register.rs:121`; `cfg(has_openvdb)`-gated registry factory (`register.rs:157`) |
| In-realization conversion executor structure | **exists, scope-limited** (`#4050`) | `engine_build.rs:4125` walks `plan.conversions`; **only `(BRep,Mesh)=Tessellate` is executable** (`v03_conversion_projection`, `dispatcher.rs:597`); any other stage is rejected (`engine_build.rs:4194`) |
| `OpenVdbKernel::ingest_mesh` (Mesh→Voxel execute) | **absent** → α | no override on `OpenVdbKernel`; falls through to trait default `OperationFailed` (`geometry.rs:2545`). `register.rs:104` notes trait-`execute()` "degrades… until task ε" — **that task is stale/unowned; this PRD owns it** |
| `ConversionProjection::Voxelize` + executor Mesh→Voxel stage | **absent** → β | `v03_conversion_projection` (`dispatcher.rs:597`) has no `(Mesh,Voxel)` row |
| Production demand for a Voxel/SDF realization | **absent** → γ | `demanded_reprs_for_template` (`engine_build.rs:1625`) yields only BRep/Mesh (export-format-driven); Voxel demanded only in dispatcher unit tests. **This PRD's measurement is the first production Voxel demander** |
| Realized-solid → `SampledField` extraction (one call) | **absent** → α+γ | `realize_voxel_from_mesh` returns an opaque FFI handle; no densify-from-realized-grid path exists |
| Min-reduction over `d⁺+d⁻` (min-wall scalar) | **absent** → δ | `compute_medial_mask` (`medial.rs:497`) uses the distances for the equality filter; no min-wall reduction |
| Ridge-min `2×min\|φ\|` (min-feature scalar) | **absent** → ε | no ridge-min reduction exists |
| Auto-measurement check-time pass `measure_dfm_rules` | **upstream (sibling γ=4408)** | `process-dfm-overhang-draft.md` §2.2; modeled on `RepresentationWithin` interception (`engine_constraints.rs:30-90,654-685`) |
| `dfm::diagnose` + `parse_dfm_severity` (severity bridge) | **exists+wired** (extended by sibling δ=4409) | `reify-stdlib/src/dfm.rs:249,134` |
| `min_feature_size : Length` on `Subtracting`/`Adding`/`Parting` | **exists+wired** (done `#4273`) | `crates/reify-compiler/stdlib/process.ri:55,65,93` |
| `DFMRule.subject : Solid` | **upstream (sibling β=4407)** | added by `process-dfm-overhang-draft.md` β; `"Solid" => Type::Geometry` (`type_resolution.rs:563`) |

**Grammar gate (`references/grammar-gate.md`):** this PRD introduces **no novel syntax** — it adds
no `.ri` grammar at all. The capability `min_feature_size` already exists on the three relevant
process traits; `DFMRule.subject : Solid` is supplied by the sibling; the new diagnostics, selectors,
kernel methods, and dispatcher projection are all Rust-side. **`grammar_confirmed = true` (N/A — no
`.ri` grammar)** for every leaf. The η example reuses existing forms (`process.ri` conformers +
`DFMRule` instances), exercised in CI.

### §3b — Honest numeric floor (G6 — CRITICAL, domain = numerical)

Min-wall and min-feature are **sampled estimates, never exact**. Per the overlay's esc-3453 (guessed
%) / esc-3770 (impossible 1e-12) cautionary corpus, the floor is tied to a **measurable resolution
parameter**, not a guessed percentage, and the RED test asserts an **inequality**, never an exact
float and never machine-epsilon:

> `min_wall_thickness(solid)` returns a **conservative lower bound** at voxel resolution `h =
> voxel_size`, accurate to `± (h + chord_tol)`, where `chord_tol` is the BRep tessellation chord
> tolerance. Features thinner than `~2h` (below the narrow-band half-width) may be missed and are
> **reported as such** (a self-describing diagnostic), never silently rounded. The value is **biased
> low** so a *passing* DFM check is trustworthy.

**RED test asserts an INEQUALITY on a known-thickness fixture** (a **2.0 mm analytic box** — planar
faces tessellate exactly, so `chord_tol = 0` and the band tightens to `±h`):

```
let t = min_wall_thickness(box_2mm, h);
assert!((t - 2.0mm).abs() <= h + chord_tol);  // resolution band, NOT an exact float
assert!(t <= 2.0mm + h);                       // conservative-lower-bound contract
```

**Min-feature definition (pinned, anti-ambiguity):** the **thinnest solid cross-section** =
`2 × min interior SDF magnitude at a local ridge`. It measures the narrowest material (thin wall /
rib / web). It does **NOT** measure edge length, face diameter, hole diameter, or
gap-between-surfaces (that overlaps the existing `Distance` / `min_clearance` query).

**Floor vs the domain hazards.** The overlay's numeric hazards (FEA bending lock, Dirichlet BC,
spline end-conditions, eigensolver conditioning) do not apply here. The relevant floor is the
**voxel-resolution sampling floor `h`** plus `chord_tol`, which is exactly what the contract ties the
bound to — `bound = ±(h + chord_tol)` against a *measured* resolution parameter, not a guessed
absolute. This structurally avoids the esc-3453 / esc-3770 class.

---

## §4 — Resolved design decisions

1. **D1 — Approach B (multi-kernel dispatcher route), framed as a build-side *realization*; the
   build/check distinction is dissolving.** The eval-DAG question raised at the gate ("do the
   build-time / check-time distinctions survive the recent eval-DAG PRD?") is resolved: the
   **unified build DAG** (`docs/design/engine-unified-build-dag-option-a.md`;
   `docs/prds/v0_6/engine-unified-build-dag.md`; flag **default-off**, mostly **pending** — ε=4358,
   θ=4361) **retires the frozen pre-geometry constraint pass (C7, `engine_build.rs:2580`)** and
   reschedules constraints/measurements as worklist nodes **after** their geometry-producing
   realizations. So the SDF is built as a **dispatcher-routed realization** (the §3.3 seam,
   `engine_build.rs:4045`): under the current engine the measurement drives it on-demand
   post-tessellation (the sibling's realized-handle-read pattern); under the unified DAG the demand
   becomes a data-dependency edge and the realization becomes a node — **no rework at cutover**. The
   corrected check-time alternative (a bespoke kernel call) was declined precisely because it would
   be a side-path the cutover must reconcile. The literal trait-op alternative (stub's option a) was
   declined by substrate (BRep-native trait; O(N³) FFI point-sampling; still needs densify).

2. **D2 — The DFM thickness check rides the sibling's `measure_dfm_rules` *pass* (a diagnostic
   emitter), NOT the `.ri constraint def` path.** Therefore it does **not** depend on the unified-DAG
   constraint-re-check (D4 / `E_EVAL_UNRESOLVED` / task 4358), and the "geometry-backed predicate vs
   geometry-in-the-loop solving" boundary is irrelevant (no solver constraint is involved). This
   keeps the cross-PRD surface to sibling γ=4408 + δ=4409.

3. **D3 — `SampledField` is the convergence type; `densify_grid_to_sampled` is the one new kernel
   primitive.** Both `read_vdb_file` (production) and `bidirectional_distances` (the thickness
   primitive) already speak `SampledField`; the gap is purely realized-grid → `SampledField`, closed
   by reusing the existing `lower_to_sampled` densifier. No new field type, no per-point FFI sampling.

4. **D4 — Ship non-convex geometry, gated by a conservative-bound boundary test.** `walk_to_zero`
   (`medial.rs:881`) assumes sign-monotonicity; on re-entrant geometry it can return the first
   crossing. v1 includes an **L-bracket / C-channel fixture** as a B+H boundary test that **asserts
   the conservative-lower-bound property still holds** (`t ≤ true_thickness + h` — biased low, so a
   passing check stays trustworthy). If the property holds, non-convex ships (covering the common DFM
   case — brackets/channels); the test is the gate. If it fails at impl time, the fallback is to
   restrict v1 to convex-ish walls and re-home non-convex correctness to a follow-up (§9 Q4).

5. **D5 — `not(has_openvdb)` / no-kernel → self-describing `Undef` + diagnostic, never a fake
   number.** A stub build (no `cfg(has_openvdb)`) has no OpenVDB kernel in the registry → the Voxel
   demand is unsatisfiable → `realize_solid_sdf` returns `None` → the pass emits a self-describing
   `Undef` + a diagnostic and degrades to `Indeterminate` (the C1 invariant), exactly as the sibling
   pass degrades with no OCCT kernel. The RED test must cover skip-or-degrade on stub builds.

6. **D6 — Reuse the existing `min_feature_size` capability for both measurements; add no `.ri`
   capability param.** `min_feature_size` already exists on `Subtracting`/`Adding`/`Parting`
   (`#4273`). Min-wall and min-feature both gate on it (the process cannot make material thinner than
   `min_feature_size`). This keeps the PRD grammar-clean. A distinct `min_wall_thickness` capability
   param is a noted follow-up (§9 Q1), not v1.

7. **D7 — Bound the grid size; run the measurement inline; defer ComputeNode wrapping.** The medial
   walk is `O(N³ · max_thickness_voxels)` (parallelized by `#3182`). v1 bounds the grid via a coarse
   default `h` (which widens the honest floor — a clean, stated trade-off) and runs inline in the
   pass. Wrapping the measurement in a §3.4 ComputeNode (cancellation / caching) is a noted follow-up
   (§9 Q3), not v1.

8. **D8 — The wire fills in the existing §3.3 multi-kernel seam; the thickness arm rides the
   sibling's new §3 entry.** No new `engine-integration-norm.md` §3 entry is owned by this PRD
   (§1 / §6).

---

## §5 — Out of scope

- **Overhang / draft metrology** — shipped by the sibling `process-dfm-overhang-draft.md`
  (independent of the SDF tier).
- **A general Voxel/SDF *export format* or a user-facing `.ri` SDF query builtin** — the wire is an
  internal realization path consumed by the measurement; promoting it to a user-facing surface is a
  follow-up.
- **`Convert{Voxel→Mesh}` marching cubes** (task `#3440`, pending) — not needed; the chain is
  `BRep→Mesh→Voxel→SampledField`, never back to Mesh.
- **A distinct `min_wall_thickness` capability param** distinct from `min_feature_size` (§9 Q1).
- **§3.4 ComputeNode wrapping of the measurement** (§9 Q3).
- **Fixing `walk_to_zero` sign-monotonicity for re-entrant geometry** beyond the conservative-bound
  contract (§9 Q4) — only triggered if D4's boundary test fails.
- **The unified build DAG itself** — this PRD is forward-compatible with it (§4 D1) but does not
  depend on it; the DFM check rides the pass, not the constraint path (D2).

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Resolution |
|---|---|---|
| Auto-measurement check-time pass (`measure_dfm_rules`) + `dfm::diagnose` extension point + `DFMRule.subject : Solid` | `process-dfm-overhang-draft.md` (**upstream, decomposed**: γ=4408, δ=4409, β=4407) | This PRD **consumes + adds a thickness category arm** (ζ depends on 4408 + 4409). Additive; no contested ownership. |
| Eval-reachable **solid→SDF wire** (`ingest_mesh` + `Voxelize` projection + `densify_grid_to_sampled` + `realize_solid_sdf` + first Voxel demand) | **THIS PRD** (α, β, γ) | No active task owns it; the `register.rs:104` "task ε" reference is stale/unowned. This PRD builds the four pinned gaps (§3). |
| Kernel SDF primitives + Mesh→Voxel descriptor + conversion-executor structure + medial-walk perf | `#3095` / `#3438` / `#4050` / `#3182` (**done, on main**) | Substrate this PRD builds on, **not** a dependency on pending work. |
| §3.3 multi-kernel dispatch (the conversion-executor / kernel-registry path) | `multi-kernel-phase-3.md` (**shipped seam**) | This PRD **fills in** the advertised-but-unexecuted `Convert{Mesh}→Voxel` execution. No new §3 entry. |
| `engine_build.rs` realization-loop restructure (executors, driver re-route) | `engine-unified-build-dag.md` (ε=4358, θ=4361, **default-off, pending**) | **Composes, not contested:** this PRD adds a new *conversion projection* (Mesh→Voxel) at the dispatcher/kernel level, *below* the loop-structure level the unified DAG restructures; the unified-DAG ε wraps `execute_realization_ops` "rollback verbatim", so the new stage rides inside it. This PRD owns the conversion execution; the unified DAG owns the loop restructure. No dependency edge. |
| Thickness measurement primitive (`bidirectional_distances`) + min-reduction / ridge-min | `reify-shell-extract` (primitive exists) + **THIS PRD** (δ, ε add the reductions) | Reuse + add reductions. |
| `min_feature_size` capability + severity bridge (`dfm::diagnose`) | `process-dfm-completion.md` (**shipped**) | Reused; not re-authored. |

No reciprocal-ownership ambiguity. The one seam touching another *in-flight* PRD (the unified DAG)
composes by altitude and carries no dependency edge.

---

## §7 — Contract (B + H)

### §7.1 — Seam signatures + invariants

**`OpenVdbKernel::ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError>`** (α)
- Wraps `realize_voxel_from_mesh_with_options(mesh, opts)`; `opts.voxel_size = h`,
  `opts.narrow_band` from `MeshToVoxelOptions`. Returns the realized voxel grid handle.
- `cfg(not(has_openvdb))`: the kernel is absent from the registry (the registration is
  `cfg(has_openvdb)`-gated) — there is no instance to call. Degradation is handled one level up
  (γ returns `None`).

**`densify_grid_to_sampled(&self, handle: GeometryHandleId) -> Result<SampledField, QueryError>`** (α)
- Extracts the opaque voxel grid into an `OpenVdbGridSource` and lowers via the existing
  `lower_to_sampled` → a CPU-resident `SampledField` (dense `Vec<f64>`, SI units, narrow-band
  half-width recorded). Sampling thereafter is pure-CPU (no FFI per sample).

**`realize_solid_sdf(&mut self, subject: GeometryHandleRef) -> Option<SampledField>`** (γ)
- Pre: a realized closed-solid handle; an OpenVDB kernel present in the registry.
- Drives the dispatcher's `BRep→Mesh→Voxel` chain (tessellate via OCCT, voxelize via α), densifies
  (α), returns `Some(SampledField)`.
- **`None`** iff no OpenVDB kernel (stub build) **or** no realized subject **or** the chain fails —
  the caller (the pass) turns `None` into a self-describing `Undef` + diagnostic and `Indeterminate`
  (C1), **never a false `Violated`** and **never a fabricated number** (D5).

**`min_wall_thickness(sdf: &SampledField, h) -> Length`** (δ)
- Min-reduction over `d⁺ + d⁻` across medial voxels (`bidirectional_distances`).
- **Conservative lower bound**, accurate to `±(h + chord_tol)`, biased low (§3b). Features `< ~2h`
  are reported as below-resolution, not silently rounded.

**`min_feature_size_measure(sdf: &SampledField, h) -> Length`** (ε)
- `2 × min over ridge voxels of |φ|` — the thinnest solid cross-section (§3b). Same conservative
  band; same below-resolution reporting. Not edge/face/gap.

**Thickness pass arm (`measure_dfm_rules` extension)** (ζ)
- Pre: kernel-backed engine, `subject` realized, the rule's process category carries
  `min_feature_size` (`Subtracting`/`Adding`/`Parting`). Post: ≤1 diagnostic per measurement that
  violates, at the rule's `DFMSeverity`; conforming → nothing. **No kernel / `realize_solid_sdf` →
  `None` → `Indeterminate`, never `Violated`** (C1).

### §7.2 — Two-way boundary tests (H)

**Producer side (does the measurement match geometry?):**

| Scenario | Precondition | Postcondition |
|---|---|---|
| Min-wall exact-ish on a known box | a 2.0 mm analytic box, voxel `h`, planar faces (`chord_tol=0`) | `\|t − 2.0mm\| ≤ h`; `t ≤ 2.0mm + h` (conservative lower bound) |
| Non-convex conservative bound (D4) | an L-bracket / C-channel, known thin web `w`, voxel `h` | `t ≤ w + h` (conservative-lower-bound property holds on re-entrant geometry) — **the D4 gate** |
| Min-feature picks the rib, not the face | a part with a thin rib `r` and a wide planar face | reported `min_feature ≈ r ± (h + chord_tol)`, biased low; **not** the face diameter |
| Below-resolution reported, not rounded | a feature thinner than `~2h` | a self-describing below-resolution diagnostic; no silent number |
| Stub-build degradation (D5) | a build with `cfg(not(has_openvdb))` | `realize_solid_sdf → None`; a self-describing `Undef` + diagnostic; **no fabricated thickness** |

**Consumer side (does the pass wire to the user surface?):**

| Scenario | Precondition | Postcondition |
|---|---|---|
| Auto-flag, no hand-declared thickness | `.ri` with a thin-walled part + `DFMRule{severity: Warning, applies_to: <Subtracting>, subject: part}`, wall `< min_feature_size` | `reify check` emits exactly one `W_DFM_MIN_WALL`; the `.ri` declares **no** thickness |
| Severity honored | same with `severity: Error` | diagnostic is `E_DFM_MIN_WALL` |
| Min-feature arm | a part with a sub-capability thin rib | `reify check` emits `{W,E}_DFM_MIN_FEATURE` |
| No-kernel degradation (C1 / D5) | the lightweight (no-OCCT / stub) `reify check` path | thickness rules → `Indeterminate`, exit 0, no `Violated`, a self-describing `Undef` diagnostic |
| Conformer | a part with all walls/features ≥ `min_feature_size` | no DFM diagnostic |
| Anti-orphan | — | `rg` shows the thickness arm invoked from `measure_dfm_rules`, `realize_solid_sdf` demanding Voxel, `ingest_mesh` reached from the conversion executor, and the new `diagnose` arms reached from the pass |

---

## §8 — Decomposition plan (one leaf per task → observable signal)

**Spine:** **α (openvdb execute+densify) → β (dispatcher Voxelize stage) → γ (realize_solid_sdf wire)
→ {δ (min-wall) ‖ ε (min-feature)} → ζ (thickness pass arm + diagnose) → η (e2e example, the
leaf)**. The wire (α/β/γ) is a serial substrate spine across crates (reify-kernel-openvdb →
reify-eval dispatcher → reify-eval helper); δ/ε are parallel reductions; ζ joins them with the
sibling pass; η is the single user-observable integration gate (C-as-integration-gate — every other
task is an intermediate roped into η).

- **α — OpenVDB Mesh→Voxel execute + grid→`SampledField` densify** (`reify-kernel-openvdb`).
  Implement `GeometryKernel::ingest_mesh` (wrap `realize_voxel_from_mesh_with_options`); add
  `densify_grid_to_sampled` (reuse `lower_to_sampled`/`OpenVdbGridSource`); thread
  `MeshToVoxelOptions{voxel_size = h, narrow_band}`.
  **Signal:** `cargo test -p reify-kernel-openvdb` — a 2.0 mm analytic box mesh → `ingest_mesh` →
  `densify_grid_to_sampled` → a `SampledField` whose interior `φ` at the box centre ≈ −1.0 mm within
  `h`; `rg` shows `ingest_mesh` + `densify_grid_to_sampled` wired (not test-only).
  *Deps: none (builds on done `#3095`/`#3438`). grammar_confirmed: N/A (Rust). Downstream: β, γ.*

- **β — dispatcher `Voxelize` projection + conversion-executor Mesh→Voxel stage** (`reify-eval`:
  `dispatcher.rs` + `engine_build.rs`). Add `ConversionProjection::Voxelize` + a `(Mesh,Voxel)` row
  to `v03_conversion_projection`; make the in-realization conversion executor (`engine_build.rs:4125`)
  execute the Mesh→Voxel stage via α's `ingest_mesh`; update the
  `dispatcher_integration::openvdb_two_stage_chain_…_degrades_gracefully` test to assert execution.
  **Signal:** `cargo test -p reify-eval` / `dispatcher_integration` — a BRep solid demanding Voxel
  produces a voxel grid handle via the two-stage chain (OCCT tessellate → OpenVDB voxelize), no
  longer rejected; `rg` shows the `Voxelize` projection on the executor path.
  *Deps: α. grammar_confirmed: N/A. Downstream: γ.*

- **γ — `realize_solid_sdf` consumer wire + first Voxel demand + `not(has_openvdb)` degradation**
  (`reify-eval`). `realize_solid_sdf(subject) -> Option<SampledField>`: demand a Voxel realization,
  drive β's chain, densify (α), obtain the OpenVDB kernel via the registry; `None` on stub-build /
  no-kernel / no-subject.
  **Signal:** `cargo test -p reify-eval` — a realized box solid → `Some(SampledField)` sampleable on
  the CPU; a `cfg(not(has_openvdb))` (stub) build → `None` (self-describing, no panic, no number);
  `rg` shows `realize_solid_sdf` is the first production caller to demand `ReprKind::Voxel`.
  *Deps: β. grammar_confirmed: N/A. Downstream: δ, ε.*

- **δ — min-wall measurement (min-reduction over `d⁺+d⁻`)** (`reify-shell-extract` / `reify-eval`).
  Add the min-reduction across medial voxels via `bidirectional_distances` (today the reduction
  doesn't exist); return a conservative lower bound at resolution `h`.
  **Signal:** `cargo test` — `min_wall_thickness(box_2mm, h)` satisfies `|t−2.0mm| ≤ h+chord_tol`
  **and** `t ≤ 2.0mm + h`; an **L-bracket** fixture's thin web `w` satisfies `t ≤ w + h` (the D4
  non-convex conservative-bound boundary test); a below-`2h` feature is reported below-resolution.
  *Deps: γ. grammar_confirmed: N/A. Downstream: ζ.*

- **ε — min-feature measurement (ridge-min `2×min|φ|`)** (`reify-shell-extract` / `reify-eval`).
  Add the ridge-min reduction over the `SampledField`; pin the thinnest-solid-cross-section
  definition (not edge/face/gap).
  **Signal:** `cargo test` — `min_feature_size_measure(thin_rib_fixture, h) ≈ rib ± (h+chord_tol)`,
  biased low; a fixture with a thin rib **and** a wide face reports the **rib**, not the face (the
  anti-ambiguity check).
  *Deps: γ. grammar_confirmed: N/A. Downstream: ζ.*

- **ζ — thickness arm in `measure_dfm_rules` + `dfm::diagnose` thickness arms** (`reify-eval` +
  `reify-stdlib`). Extend the sibling pass (4408) with a thickness category arm:
  `realize_solid_sdf(subject)` → run δ + ε → compare vs `min_feature_size` → route via
  `dfm::diagnose`; extend `diagnose` (4409) with `{I,W,E}_DFM_MIN_WALL` / `_MIN_FEATURE`; no-kernel /
  `None` → `Indeterminate` (C1).
  **Signal:** `reify check` on a part with a `DFMRule` + a sub-capability thin wall emits
  `W_DFM_MIN_WALL` with **no hand-declared thickness**; `DFMSeverity.Error` → `E_DFM_MIN_WALL`; the
  no-OCCT / stub path → `Indeterminate` (no false `Violated`); a conformer → empty;
  `cargo test -p reify-stdlib` asserts the diagnose severity mapping.
  *Deps: δ, ε, **sibling γ=4408** (the pass), **sibling δ=4409** (the diagnose extension point).
  grammar_confirmed: N/A. Downstream: η.*

- **η — end-to-end CI example + doc reconcile (user-observable leaf / B integration gate)**
  (`examples/` + `docs/`). Commit `examples/process/std_process_dfm_thickness.ri`: a `Subtracting`
  (or `Adding`) process with `min_feature_size`, a thin-walled part + a thin-feature part, `DFMRule`
  instances with `subject : Solid` at each `DFMSeverity`. Reconcile `docs/reify-stdlib-reference.md`
  §8 (the thickness metrology arm + the solid→SDF wire; correct the deferred-thickness pointer).
  **Signal:** `reify check examples/process/std_process_dfm_thickness.ri` (CI) shows the expected
  auto-emitted `W_/E_DFM_MIN_WALL` / `_MIN_FEATURE` set, with **no hand-declared measured
  thickness**; CI green.
  *Deps: ζ, **sibling ζ=4411** (serializes the shared `docs/reify-stdlib-reference.md` §8 edit —
  narrow-lock hygiene). grammar_confirmed: N/A.*

**Tracker disposition (at decompose):** the deferred trackers **4276 (MET-1)** and **4277 (MET-2)**
are already **cancelled** (the originating session's disposition, 2026-06-08); the min-wall /
min-feature scope MET-1 held lands here (α–η). Per the subtask-deprecation norm, the batch is filed
as **top-level** tasks (cite `#3095`, never `3095.2`).

---

## §9 — Open (tactical / implementation-time) questions

1. **Distinct `min_wall_thickness` capability param (D6).** v1 gates both measurements on the
   existing `min_feature_size`. A process whose min-wall limit differs from its min-feature limit
   would want a separate `min_wall_thickness : Length` param — additive, grammar-clean (a `Length`
   param), but deferred to keep v1 minimal. **Suggested resolution:** add it if the η example reads
   better with a distinct limit; otherwise follow-up.
2. **Default voxel resolution `h` (D7).** The honest floor widens with a coarser `h`; the medial walk
   cost grows with a finer one. Pin a default `h` (e.g. `min_feature_size / 4`, so the band is a
   quarter of the capability) with the box fixture. **Decide during α/δ.** Tactical.
3. **§3.4 ComputeNode wrapping of the measurement (D7).** If the inline walk is too slow at the
   default grid, wrap the measurement in a ComputeNode (`compute-node-contract.md` §4/§5) for
   cancellation + caching. **Suggested resolution:** measure first; wrap only if a realistic part
   exceeds ~50 ms. Follow-up.
4. **Non-convex fallback (D4).** If the L-bracket boundary test cannot hold the conservative-lower-
   bound property, restrict v1 to convex-ish walls and re-home non-convex correctness (fix
   `walk_to_zero` sign-monotonicity) to a follow-up. **Decide during δ** against the actual fixture.
5. **Ridge detection threshold (ε).** The "local ridge" predicate for min-feature needs a gradient /
   neighbourhood threshold; pin it with the thin-rib fixture. Tactical.
6. **`DFMSeverity.Info` rendering (ζ).** Mirror the sibling / `process-dfm-completion` §8 Q4 — confirm
   `reify check` renders an `Info`-level thickness diagnostic, or downgrade `Info` rules to a no-op.
