# Capability manifest — `process-dfm-thickness-metrology.md`

Mechanizes `/prd` gates **G3** (substrate exists / wired) + **G6** (premise valid) per leaf, per
`.claude/skills/prd/project.md` → *Capability Manifest*. Each row binds a leaf's asserted capability
to **evidence** (file:line on main, or a queued/upstream producer) + a **verdict**. Any **FAIL**
binding blocks queueing until resolved. Authored 2026-06-08 against the four-agent substrate sweep.

Evidence forms: **wired-on-main** (anti-orphan — symbol reached from a production path, not
test-only), **grammar-fixture** (G3 — novel syntax parses, or N/A), **numeric-floor** (G6 — `bound >
floor`, or "no fabricated bound"), **field-population** (producer writes a real non-`Undef` value).
Verdicts: **PASS** / **PASS (producer-self)** (the leaf *is* the producer of a not-yet-wired
capability — wiring is the leaf's own scope) / **PASS (upstream)** (delivered by a wired
dependency) / **FAIL**.

> **G3 stance.** This PRD owns the **eval-reachable solid→SDF wire** — a deliberately-orphaned-today
> capability (the C-17-shape gap). The wire's four pieces (α `ingest_mesh`+densify, β `Voxelize`
> stage, γ `realize_solid_sdf`+first Voxel demand) are therefore **PASS (producer-self)**: the gate
> is not "is it wired today" (it is not — that is the point) but "is the substrate it assembles real
> + wired, and is the assembly the leaf's own scope". Every substrate the wire assembles is
> **wired-on-main** (§3 table). The two stub options that did *not* resolve to a clean producer-self
> are recorded in the PRD §0/§4 as declined; this manifest binds the chosen approach B.

---

## Leaf α — OpenVDB Mesh→Voxel execute + grid→`SampledField` densify (`reify-kernel-openvdb`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Mesh→Voxel realization primitive exists | wired-on-main | `realize_voxel_from_mesh_with_options` (`kernel_real.rs:106`) → `realize_voxel_from_mesh` (`:79`), done `#3095`; `MeshToVoxelOptions{voxel_size,narrow_band}` (`mesh_to_voxel_options.rs:20`) | PASS |
| Grid→`SampledField` densifier reusable (not file-coupled) | wired-on-main | `lower_to_sampled` is a `pub fn` over any `&OpenVdbGridSource` (`ingest.rs:294`); already builds the production `SampledField` for `read_vdb_file` (`ingest.rs:648`) | PASS |
| `ingest_mesh` override + `densify_grid_to_sampled` wired (not test-only) | wired-on-main (producer-self) | α adds them; β (the executor) + γ (the helper) are the production callers. Anti-orphan signal = `cargo test -p reify-kernel-openvdb` exercising them + `rg` showing the override reached. Today `ingest_mesh` is the trait default `OperationFailed` (`geometry.rs:2545`) — α is its producer | PASS (producer-self) |
| Densified interior `φ` is a real sampleable value (not `Undef`) | field-population | `SampledField.data: Vec<f64>` (`value.rs:90`) populated from the grid via `lower_to_sampled`; α's signal asserts interior `φ ≈ −1.0mm` within `h` on a 2mm box (a real number, not a sentinel) | PASS (producer-self) |
| No novel `.ri` grammar | grammar-fixture | Rust-only leaf; no `.ri` syntax | PASS (N/A) |

## Leaf β — dispatcher `Voxelize` projection + conversion-executor Mesh→Voxel stage (`reify-eval`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Dispatcher plans `BRep→Mesh→Voxel` | wired-on-main | `dispatch(...)` BFS (`dispatcher.rs:671`) reaches Voxel via the advertised `Convert{Mesh}→Voxel` (`register.rs:121`, `#3438`); called on the per-op hot path (`engine_build.rs:4045`) | PASS |
| Conversion-executor structure exists (walks `plan.conversions`) | wired-on-main | `engine_build.rs:4125` (`#4050`), tessellate-source + ingest-target loop | PASS |
| `(Mesh,Voxel)=Voxelize` projection + executor stage wired | wired-on-main (producer-self) | β adds the `(Mesh,Voxel)` row to `v03_conversion_projection` (today only `(BRep,Mesh)=Tessellate`, `dispatcher.rs:597`) and makes the executor run that stage via α's `ingest_mesh`. γ is the production demander. Anti-orphan signal = the updated `dispatcher_integration` test executes the chain + `rg` | PASS (producer-self) |
| No novel `.ri` grammar | grammar-fixture | Rust-only leaf | PASS (N/A) |

## Leaf γ — `realize_solid_sdf` wire + first Voxel demand + `not(has_openvdb)` degradation (`reify-eval`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| A realized subject solid handle is reachable at measurement time | wired-on-main | `tessellate_realizations` populates realized data; internal realized-handle queries `engine_build.rs:756/798` (the sibling pass uses the same access for `BoundingBox`) | PASS |
| OpenVDB kernel obtainable via the registry (not ad-hoc) | wired-on-main | `kernel_registry.rs` `inventory` factory; `openvdb_factory` `cfg(has_openvdb)`-gated (`register.rs:157`) | PASS |
| `realize_solid_sdf` drives the chain + densifies; first Voxel demand | wired-on-main (producer-self) | γ adds the demand + the helper over β's chain + α's densify. Today nothing demands Voxel in production (`demanded_reprs_for_template` yields only BRep/Mesh, `engine_build.rs:1625`) — γ is that producer. Anti-orphan signal = a solid → `Some(SampledField)` + `rg` showing the demand | PASS (producer-self) |
| `not(has_openvdb)` / no-kernel → `None`, never a fabricated number (D5) | field-population | The registration is `cfg(has_openvdb)`-gated, so a stub build has no kernel → demand unsatisfiable → γ returns `None`; the pass (ζ) maps `None` → self-describing `Undef` + diagnostic. γ's signal asserts the stub-build `None` (no panic, no number) | PASS (producer-self) |
| No novel `.ri` grammar | grammar-fixture | Rust-only leaf | PASS (N/A) |

## Leaf δ — min-wall measurement (min-reduction over `d⁺+d⁻`) (`reify-shell-extract` / `reify-eval`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Thickness primitive `bidirectional_distances` exists + is parallelized | wired-on-main | `medial.rs:863` (d⁺+d⁻ gradient walk); walk parallelized by `#3182` (done) — mitigates the O(N³) perf hazard | PASS |
| Min-reduction to a min-wall scalar | wired-on-main (producer-self) | δ adds the reduction (today `compute_medial_mask` uses the distances only for the equality filter, `medial.rs:497`; no min-wall scalar). ζ is the production consumer | PASS (producer-self) |
| **G6 — conservative lower bound, inequality on a measured resolution, no fabricated bound** | numeric-floor | **Floor = voxel resolution `h` + `chord_tol`.** RED test asserts `\|t−2.0mm\| ≤ h+chord_tol` **and** `t ≤ 2.0mm + h` on a **2.0mm analytic box** (planar faces ⇒ `chord_tol=0` ⇒ band `±h`) — an **inequality tied to a measured `h`**, never an exact float, never machine-epsilon. `bound = ±(h+chord_tol)` > 0 by construction. Biased low ⇒ a passing check is trustworthy. **esc-3453 (guessed %) / esc-3770 (1e-12) class structurally avoided** | PASS |
| **D4 — non-convex conservative-bound holds** | numeric-floor | L-bracket / C-channel fixture asserts `t ≤ w + h` (conservative-lower-bound preserved on re-entrant geometry). The test is the D4 gate; if it fails at impl time, fall back to convex scope (§9 Q4) | PASS (gated by the boundary test) |
| Below-resolution reported, not rounded | field-population | δ emits a self-describing below-resolution diagnostic for features `< ~2h`; signal asserts it (no silent number) | PASS (producer-self) |

## Leaf ε — min-feature measurement (ridge-min `2×min|φ|`) (`reify-shell-extract` / `reify-eval`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| Ridge-min over the `SampledField` | wired-on-main (producer-self) | ε adds the reduction (no ridge-min exists today); reuses γ's `SampledField` + CPU sampling (`medial.rs:602`). ζ is the consumer | PASS (producer-self) |
| **G6 — min-feature definition pinned (anti-ambiguity); conservative band** | numeric-floor | Pinned to **thinnest solid cross-section** = `2×min interior \|φ\| at a ridge` — NOT edge/face/gap (gap overlaps `Distance`/`min_clearance`). Same `±(h+chord_tol)` band, biased low. RED test: a thin-rib-plus-wide-face fixture reports the **rib**, not the face (the anti-ambiguity assertion); `min_feature ≈ rib ± (h+chord_tol)` | PASS |
| No novel `.ri` grammar | grammar-fixture | Rust-only leaf | PASS (N/A) |

## Leaf ζ — thickness arm in `measure_dfm_rules` + `dfm::diagnose` arms (`reify-eval` / `reify-stdlib`)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| The auto-measurement pass `measure_dfm_rules` exists to extend | producer (upstream) | sibling γ=**4408** (`process-dfm-overhang-draft.md`, decomposed, pending) — wire `add_dependency` ζ→4408 | PASS (upstream) |
| `dfm::diagnose` extension point + severity bridge | producer (upstream) + wired-on-main | `diagnose` (`dfm.rs:249`) + `parse_dfm_severity` (`dfm.rs:134`); the overhang/draft arms land in sibling δ=**4409** — wire ζ→4409. ζ adds sibling thickness arms | PASS (upstream) |
| `DFMRule.subject : Solid` readable | producer (upstream) | sibling β=**4407** (in-progress) adds it; transitively available via 4408 (its consumer) | PASS (upstream) |
| `min_feature_size` capability on the process traits | wired-on-main | `process.ri:55,65,93` (`Subtracting`/`Adding`/`Parting`), done `#4273`. D6: both measurements gate on it; no new param | PASS |
| Thickness diagnostics emit at the rule's severity; conformer → empty | field-population (producer-self) | ζ adds `{I,W,E}_DFM_MIN_WALL`/`_MIN_FEATURE`; `cargo test -p reify-stdlib` asserts severity mapping (mirrors the shipped `diagnose_violation_*` tests `dfm.rs:569-643`) | PASS (producer-self) |
| No-kernel / `None` → `Indeterminate`, never false `Violated` (C1/D5) | wired-on-main (template) | `RepresentationWithin` C1 invariant (`engine_constraints.rs:654-685`); ζ maps γ's `None` to a self-describing `Undef` + `Indeterminate` | PASS |
| No novel `.ri` grammar | grammar-fixture | Rust-only leaf | PASS (N/A) |

## Leaf η — end-to-end CI example + doc reconcile (user-observable leaf / integration gate)

| Asserted capability | Evidence form | Evidence | Verdict |
|---|---|---|---|
| `reify check` auto-emits a thickness diagnostic with NO hand-declared thickness | wired-on-main (integration gate) | Composes α (execute+densify) + β (Voxelize) + γ (wire) + δ/ε (measure) + ζ (pass+diagnose). The `.ri` declares part + `DFMRule` only; the thickness is engine-measured. **The G2 user-observable leaf** | PASS (gated on α–ζ) |
| Example fixtures yield an honest measurement (G6) | numeric-floor | Thin-walled / thin-rib parts at a default `h`; the diagnostic fires on a sub-`min_feature_size` wall — an inequality verdict, not an exact-float assertion in the example | PASS |
| Doc-reconcile claims match shipped behaviour | anti-mismatch | `docs/reify-stdlib-reference.md` §8 updated to the thickness arm + the solid→SDF wire (correcting the deferred-thickness pointer); verified against the green example. Wire η→sibling ζ=**4411** to serialize the shared §8 edit (narrow-lock hygiene) | PASS (at η) |
| `.ri` uses only existing grammar | grammar-fixture | `process.ri` conformers + `DFMRule` instances + `subject : Solid` (sibling) — no novel syntax; CI-exercised | PASS (N/A) |

---

## Summary

No **FAIL** bindings. The wire (α/β/γ) is the standard **producer-self** pattern for a
deliberately-orphaned-today capability this PRD owns (the C-17 solid→SDF gap): every substrate it
assembles is **wired-on-main** (kernel SDF primitives `#3095`, descriptor `#3438`, conversion-executor
`#4050`, densifier `lower_to_sampled`, dispatcher, registry), and the assembly + first production
demand is each leaf's own scope. **G6:** no fabricated numeric bound anywhere — min-wall / min-feature
are conservative lower bounds tied to a *measured* voxel resolution `h` (+ `chord_tol`), all RED tests
assert inequalities (`|t−2mm| ≤ h+chord_tol`, `t ≤ 2mm+h`, `t ≤ w+h` non-convex), never an exact
float / machine-epsilon — the esc-3453/3770 class is structurally avoided; `not(has_openvdb)` returns
`None`/`Undef`, never a number. **G3:** no novel grammar (`min_feature_size`/`subject:Solid`
precedents); the four wire gaps are pinned to file:line with their producer leaf. Cross-PRD deps:
ζ→{4408,4409}, η→4411 (all real `add_dependency` edges). The batch is clear to queue.
