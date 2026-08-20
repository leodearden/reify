# Capability manifest — `dimension-checked-readers` (units-gating program PRD 5)

**PRD:** `docs/prds/v0_6/dimension-checked-readers.md` (landed `efba5a8036`)
**Decomposed:** 2026-07-28/29 · **Substrate HEAD:** `efba5a8036`
**Batch:** tasks **5791, 5795, 5797–5815** (21 leaves) · 39 dependency edges (36 intra-batch, 3 cross-PRD)
**Machine-readable twin:** `docs/prds/v0_6/dimension-checked-readers.capability-manifest.yaml`
(21 labels · 69 capability bindings · 0 FAIL · 56 mechanical `delivered_check`s).

**Zero source drift.** `git diff --name-only dc83d4fd60..HEAD` touches **nothing under
`crates/`, `examples/` or `gui/`** — only the five units-program PRD `.md` files, four
`tests/prd-gate/fixtures/*.ri`, `scripts/verify.sh`, `tests/infra/test_verify_scope.sh` and
`.claude/skills/prd/project.md`. Every anchor the PRD verified at `dc83d4fd60` therefore
holds verbatim, re-confirmed by direct read for all 13 reader chokepoints, both solver
readers, the four `.ri` load declarations and the whole diagnostic substrate.

---

## D3 verdict

Run manually (the Workflow tool is unavailable in subagent sessions), following the contract
in `.claude/skills/prd/project.md` → *Decompose mode — run the substrate-verification workflow*:

| Round | Command chain | Prover | Adversary | `blocks` |
|---|---|---|---|---|
| 1 | `prd-decompose-verify.py bind` → `prd-capability-check.py --json` → `synthesize` | 17/17 PASS | 22 records, **9 FAIL** | **true** — 13 blocking signals |
| 2 (post-amendment) | same chain, amended premises + 4 new committed fixtures | **23/23 PASS** | 22 records, 0 FAIL | **false** — `blocking: []` |

Round 1 blocked exactly as designed. All 13 blocking signals were resolved by **amendment**
(G6 resolutions (a) *move the signal to the task that can produce it* and (c) *change the
asserted configuration so the claim becomes true*), never by weakening a gate. Every amendment
is written verbatim into the affected task's description as a **BINDING DECOMPOSE AMENDMENT**
so the implementer never re-derives the PRD's wrong version.

**Negative-assertion polarity.** The harness's `rejection` assertion-kind binds `probe_kind:
"check"`. Every PRD-5 leaf signal is `reify eval`-phrased, and `reify check` **prints an
`error:` and exits 0 today** (probe-confirmed below) — the INV-SF-2 outlier PRD 2 owns. Binding
rejections as `rejection` would therefore have produced systematic false FAILs. Instead each
rejection assertion is bound as a **pair**, which is strictly stronger:
1. a `produces` probe on an **already-shipped rejection of the same class and channel**
   (`E_StackupDimMismatch` → exit 1 with the code in stderr) — proves the mechanism exists
   and fires; and
2. an `ir` probe on the leaf's own RED fixture proving **today's silent accept** (exit 0,
   clean) — the vacuity check. That second probe is what caught ι (below).

---

## Committed D3 fixtures (`tests/prd-gate/fixtures/`)

All twelve are NEW files, all parse with 0 ERROR nodes, and each carries its measured
before-image in a header comment so the RED is re-checkable at dispatch (D4).

| Fixture | Role | Measured today |
|---|---|---|
| `dcr_material_dimension_silent.ri` | β RED (B4) | exit 0, `spring_rate` `1.3802083333333336e-12` |
| `dcr_material_dimension_correct.ri` | β twin | exit 0, `spring_rate` `1.3802083333333335` — **exactly 1e12×** |
| `dcr_yield_stress_dimension_silent.ri` | β RED (B5, replaces the PRD's ill-formed row) | exit 0, `prb_validity_range` ±`1.2e-10` rad vs ±`0.0873` rad |
| `dcr_load_ctor_dimension_silent.ri` | γ1/γ2/γ3 RED | exit 0; only a ctor **Warning** on the dimensioned spelling |
| `dcr_load_retype_target_resolves.ri` | γ2 grammar + resolution | `tree-sitter` 0 ERRORs, `reify check` 0, `reify eval` 0, zero diagnostics |
| `dcr_solver_load_dropped_bare.ri` | γ1/π B1 control | `max_von_mises` **5139325.408614099 Pa**, 719 iterations, 0.33 s |
| `dcr_solver_load_dropped_dimensioned.ri` | γ1/π B1 RED | `max_von_mises` **0 Pa**, **0** iterations, 0.18 s |
| `dcr_reader_ctor_dimension_silent.ri` | ε/ζ/η RED | exit 0; `mass: 2 m`, `50 rad·s^-1`, `line_width: 0.4`, `ex: 0.002 m` |
| `dcr_shaper_frequency_dimension_silent.ri` | ζ RED, **consuming** path | exit 0, 1024 shaped waypoints, no kernel, <1 s |
| `dcr_langsurface_crossdim_silent.ri` | κ/θ RED | exit 0; `0.01`, `5`, `0.003 rad`, `2.5e-9`, `2.5e-6` vs `2.5` |
| `dcr_dimension_rejection_channel_fires.ri` | rejection-mechanism binding | `reify eval` **exit 1** `E_StackupDimMismatch`; `reify check` prints it and **exits 0** |
| `dcr_fn_force_param_already_rejects.ri` | ι **vacuity guard** | exit 1 on both `eval` and `check` — *already true* |

---

## The four headline defects, measured end to end

1. **1e12× spring rate.** `Material(youngs_modulus: 200mm)` → `prb_cantilever_beam` gives
   `1.3802083333333336e-12 m²·kg·s⁻²·rad⁻²`; the `200GPa` twin gives `1.3802083333333335`.
   Exit 0, zero Error diagnostics. (β)
2. **The load that vanishes.** In an identical cantilever scene, `PointLoad(force: 1000.0)`
   drives a real solve (`max_von_mises 5139325.408614099 Pa`, 719 iterations) while the
   **units-correct** `PointLoad(force: 1000N)` gives `max_von_mises 0`, `iterations 0`. Exit 0
   both times; the only trace is a ctor-conformance **Warning**. (γ1)
3. **~7e8× operating range.** `Steel_AISI_1045(yield_stress: some(310mm))` collapses
   `prb_validity_range` from ±`0.08726646259971647` rad to ±`1.2097560975609757e-10` rad,
   silently. (β)
4. **The check gate is blind.** `reify check` on a file with a shipped Error-severity
   dimension diagnostic prints `error: E_StackupDimMismatch: …` and **exits 0** with
   "All constraints satisfied." (why every signal here is `reify eval`-phrased; PRD 2 / #5403)

---

## Per-leaf bindings

Full evidence text and the `delivered_check` bodies are in the YAML twin; this is the index.

### α — relocate `arg_acceptance` into `reify-ir` *(INTERMEDIATE)*
- `reify-ir` is a reachable home for both crates — `Value` at `reify-ir/src/value.rs:993`,
  `StructureInstanceData` at `:1277`, `DiagnosticCode` already imported at `:6`;
  `reify-stdlib` already deps `reify-ir`; `reify-eval/Cargo.toml:51` deps `reify-stdlib`. **PASS**
- **FALSIFIED:** the relocation is *not* verbatim — 13 `reify_ir::` self-paths, edition 2024,
  no `extern crate self`. Amendment A1. **PASS (resolved)**
- **UNLISTED:** `accept_arg` has no `Value::Option` arm but every field β/η/ι gate is
  `Option<T>`; a naive adapter would regress all `yield_stress` reads. Amendment A2. **PASS (resolved)**
- **FALSIFIED:** eight classifiers, not nine. **PASS (resolved)**
- **FALSIFIED:** the eval exit gate is `main.rs:1661`, not `:1545` (compile-time). **PASS (resolved)**
- **UNLISTED:** I5's specific `UndefCause` has no exercised path — both
  `push_op_contract_failure` call sites pass the generic code. Amendment A6. **PASS (resolved)**

### β — flexure reader adoption + classifier arm
1e12× RED measured · PRD's B5 fixture ill-formed, replaced · `flexure_diagnose` runs on both
paths (the correct host) · 7 `yield_stress` production readers confirmed. **all PASS**

### γ1 — one shared solver load reader
Inversion measured end to end · **B1 as written is not expressible** (both result types
deliberately frozen; re-specified onto `max_von_mises`) · buckling has no `type_name` guard,
`panic!`s on a non-List, and carries a 1.0 N sentinel. **all PASS**

### γ2 — retype the four load fields + migrate
Retype target is existing grammar (probe-verified) · **PRD's blast-radius figures are
internally inconsistent** (90 vs 35+77=112; independent sweep: 92, split 32/60, bucketed
77/0/15) · migration sets provably disjoint from PRD 4 (read back from #5756/#5758) ·
**unknown ctor field lowers silently to `__arg2`** (migration hazard) · two tests assert on
literal source text. **all PASS**

### γ3 — narrow; reject `TractionLoad`/`BodyForce` loudly
Zero solver occurrences confirmed · the `task θ/3457` deferral is an **orphaned + malformed**
cite (3457 is done) · the rejection cites live task **#5800** (σ), filed at decompose so the cite is never
a forward reference. **all PASS**

### δ — joints + loop closure
`length_input`'s bare hole confirmed at 4 call sites · the strict twin
(`cylindrical_motion_vars`) already exists in the same crate · `JointValue` retarget is
**#5412's**, read-only here. **all PASS**

### ε — dynamics `cell_f64`
**FALSIFIED:** the PRD's signal is false — `mass_properties(2m,…)` already rejects via the
strict `cell_mass_f64`; the blind site is `mass_properties_from_value` (`:354`). Amendment F1
retargets it and adds the "make the existing rejection loud" half. Ctor-side RED real · three
byte-identical copies · **rotational-stiffness dimension owned by task #5799 (ρ)**, with ε reading the
vector from the registry const by reference. **all PASS**

### ζ — trajectory readers
**FALSIFIED:** the ctor alone never reaches the reader; the signal fixture must call
`input_shape`. Replacement fixture is reachable, cheap (<1 s, no kernel) and RED · the error is
2π and silent. **all PASS**

### η — FDM / as-printed
Both readers dimension-blind · both defects measured (0.4 read as metres; 2 mm read as Pa) ·
**UNLISTED:** transport is `ComputeOutcome::Failed`, not the classifier — the fixture needs a
realized `Solid`. **all PASS**

### θ — `safety_factor` × 2 entry points + `analysis.ri`
Both entry points confirmed (the Field interception at `reify-expr/src/lib.rs:394` is the one
that matters) · silent wrong answers measured · retype has zero conformers · **UNLISTED:** the
example migration is a *pair* (`sigma` at `:43` must be dimensioned too; probe-verified the
dimensioned pair still yields exactly 2.5). **all PASS**

### ι — `envelope_critical_load` + `shell_voxel_size`
**VACUITY CAUGHT:** `critical_load(result, 1mm)` is *already* rejected by overload resolution —
that half of the PRD's signal delivers nothing. Re-targeted onto the bare `eval_builtin`
`envelope_critical_load`, which has no `.ri` declaration and no overload gate · the
`shell_voxel_size` double miss is real with **zero corpus coverage** · **FALSIFIED:** no
existing test needs retargeting (`…propagates_from_reference` stays green) · path correction:
`crates/reify-eval/src/shell_extract_compute.rs`, not under `compute_targets/`. **all PASS**

### κ — `min`/`max` + inverse trig
All four wrong answers and all three controls measured · **UNLISTED:** `atan2` cross-dimension
is absent from Leg D's enumeration and differs per argument order · corpus exposure ≈ 0.
**all PASS**

### λ — closure-guard second universe
Harness producer (#5752) upstream via a hard cross-PRD edge · PRD 1's universe provably cannot
see this surface · **the derived universe is feasible (235 literal names, 0 computed dispatch)
but MUST recurse** — `eval_flexures` is a nested dispatcher, and a single-level scan silently
loses the entire `prb_*` family, this PRD's headline surface · the LSP union (95 entries) is
needed in both directions · **`point3` is dimension-polymorphic, not bare-accepting**, and the
`openvdb_stress.ri:36-38` handoff is discharged with a measure-then-decide disposition rule ·
drift-guard registrations same-diff. **all PASS**

### μ / ν / ξ / ο — the four docs-truth leaves
Zero chunk presence today (grep-verified across all 17 chunks) · the μ→#5403 edge is real
because `reify check` exits 0 on an Error today · corpus + INDEX row same-diff · cheatsheet is
an index entry, not a playbook · **FALSIFIED:** `thermal_conductivity` is *not* declared-only
(one DSL constraint reader), and the stale-doc count is 13, not ~12, with 4 lines correctly
`Real` and marked do-not-touch. **all PASS**

### π — integration gate
Every prerequisite upstream (9 edges) · B1 expressible after the re-specification ·
**UNLISTED:** gate residency is **per-binary**, and sizing is load-bearing (a 20×20×800 mm
column costs ~100 s debug; the committed B1 fixtures cost 0.33 s / 0.18 s) — tiny geometry is
mandatory. **all PASS**

### ρ (#5799) / σ (#5800) / τ (#5801) — the three live cite targets, filed at decompose
Each exists so an in-source `#NNNN` cite is never a forward reference and "no task yet owns it"
is impossible (INV-SF-5).
- **ρ #5799** — the `ROTATIONAL_STIFFNESS ≡ Torque` ruling handed to PRD 5 by PRD 3's landed text.
  Byte-identical vectors confirmed; PRD 5 is protected meanwhile by reading every vector from
  the registry const **by reference**, so a re-dimensioning is a one-line change.
- **σ #5800** — the `TractionLoad`/`BodyForce` wire-up that γ3 rejects rather than implements. The
  kernel primitives already exist; the missing piece is a `direction` type surface.
- **τ #5801** — `shear_modulus` and `thermal_expansion`, the two genuinely reader-free declarations.

---

## G-gate re-walk

| Gate | Verdict |
|---|---|
| **G1** consumer named | PASS — every mechanism names a `.ri`-author surface, a solver, the merge gate, or a sibling PRD. |
| **G2** user-observable leaf | PASS — every leaf carries a `reify eval` exit-code + `DiagnosticCode`-identity signal with a measured before-image. α is the sole INTERMEDIATE, roped to π (C-as-integration-gate). |
| **G3** substrate verified | PASS — no novel grammar (all four retype targets probe-verified); 6 grammar probes at 0 ERROR nodes; every anchor re-read at HEAD. |
| **G4** seam ownership | PASS — 3 real cross-PRD edges (λ #5810→#5752, γ2 #5798→#5627, μ #5811→#5403). `arg_acceptance` core frozen; ANGLE policy untouched; conformance untouched; `JointValue` left to #5412. |
| **G5** B+H | PASS — §7 contract + §8 two-way boundary sketch present; π is the integration gate and names the table. |
| **G6** premise validity | PASS **after amendment** — 4 falsified signals (γ1/B1, ε, ζ, β/B5), 1 vacuity (ι), 3 wrong counts (γ2, ο, α) all corrected against measurement. |
| **G7** design invariants | PASS, **no waivers**. The batch *implements* INV-SF-1/2/3/6, and INV-SF-5 is satisfied by ρ/σ/τ existing as live cite targets rather than by blanket escapes. INV-SF-7 is honoured by γ2's before/after SI-magnitude requirement. |
