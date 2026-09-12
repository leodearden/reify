# Capability manifest — damped-modal-bonded-heterogeneous

Binds each decomposition leaf's asserted capabilities to evidence (mechanizes G3+G6).
Substrate evidence verified 2026-08-27 against main-equivalent branch HEAD `81329c84a1`
(engine files identical to main for every cited symbol) with `target/debug/reify` probes;
committed fixtures under `tests/prd-gate/fixtures/`. Anchors cite symbols, not lines.
Machine-readable twin: `damped-modal-bonded-heterogeneous.capability-manifest.yaml`
(stamped by `commit_planning`).

## α — Damped mixin + DampedMaterial + preset conformance

- **mixin-trait-substrate** — grammar/semantic fixture `tests/prd-gate/fixtures/damped_material_mixin_conformance.ri` (user trait w/ Real param; `trait X : ElasticMaterial + T2` multi-parent refinement; conformer; trait-typed param; `loss_factor` access) → `reify check` exit 0, all constraints OK, probed 2026-08-27. **PASS**
- **anonymous-intersection-rejected (boundary honesty)** — `param m : A + B` is a parse error (probed); design uses the named-trait spelling. **PASS** (rejection observed)
- **preset-provenance-convention** — `materials_fea.ri` presets carry per-field `*_provenance : MaterialPropertyProvenance` (grep, symbol present); α extends the same convention. **PASS**
- **naming-hazard-handled** — unrelated `trait Damping : MaterialSpec` (with `damping_ratio` + `loss_factor` params) pre-exists in `materials_mechanical.ri`; α adds cross-reference comments both sides; ι disambiguates in discoverability. **PASS** (hazard named, owner assigned)
- Known wart, not blocking: #6870 (trait-typed access erases dimensions; `loss_factor : Real` immune).

## β — MaterialDamping descriptor + extract_damping arm

- **single-dispatch-point-exists** — `extract_damping` in `crates/reify-eval/src/modal_ops.rs`, nominal type-name match (`RayleighDamping` arm; default (0,0)); adding one arm is the designed extension seam. **PASS**
- **damping-ratio-slot-exists** — `Mode.damping_ratio : Real` declared in stdlib `modal_analysis.ri`; populated post-eigensolve; consumed by `prepare_modal_integrator` (transient path). **PASS**
- **trait-typed-descriptor-default-param** — `param extra : DampingDescriptor = NoDamping()` shape: trait-typed param with instance default probed green (mixin fixture, same shape). **PASS**
- **degenerate-exactness floor (G6)** — ζ=η/2 for single-material is an algebraic identity (SE ratio ≡ 1); asserted band 1e-9 relative ≫ fp-associativity floor (~1e-12). **PASS**
- **loud-rejection (B8)** — capability delivered by β's own negative test observing the rejection fire (INV-SF-2/SF-6: coded diagnostic, nonzero exit). Producer: β itself.

## γ — classify_material any-source + resolved {d_local, frame, rho, eta}

- **solver-core-generic** — `MaterialField` trait + `ConstantField` + `DiscreteCellField` + `element_stiffness_p1/p2_with_field` in `reify-solver-elastic` (`material_field.rs`, `assembly/mod.rs`), independently unit-tested (`assembly_anisotropic.rs`, `heterogeneous_warmstart_integration.rs`). **PASS**
- **dispatch-gap-real** — `classify_material` (`compute_targets/elastic_static.rs`) special-cases `FieldSourceKind::AsPrintedZones` only; all other Field values fall to `ExpectedStructureInstance`. Verified by code read + check-level probes. **PASS** (the gap γ closes exists)
- **resolved-value-lacks-rho-eta** — Rust `AnisotropicMaterial { d_local, frame }` only (`material_field.rs`, grep 2026-08-27): density dropped at resolution; γ's extension is necessary, not optional, for modal. **PASS**
- **raw-lambda-out-of-scope pin** — `tests/prd-gate/fixtures/raw_lambda_material_field_rejected.ri` → `reify check` exit 1 ("unresolved type in lambda param"), probed; stays red until #6871 (out of scope). **PASS** (rejection observed; boundary pinned)
- **fdm-regression consumer** — `examples/fdm_bracket.ri` calls `as_printed_material` → heterogeneous solve (landed #3786/#4757/#3787); B1 preserves it. **PASS**

## δ — sandwich_material / material_zones declarative constructors

- **field-return-from-pub-fn** — `fn_field` intercepting-builtin eval arm (#4220); stdlib `compose` is the landed precedent of a pub fn returning a Field. **PASS**
- **declarative-source-kind precedent** — `FieldSourceKind::AsPrintedZones` + trampoline-built field (`as_printed_material`, ComputeNode target `fdm::as_printed_material_r_fast`) is the exact producer pattern δ copies. **PASS**
- **constructor-rejection (B10)** — delivered by δ's own negative test (layer-sum mismatch rejected at the constructor, coded diagnostic). Producer: δ itself.
- **field-source-kinds enumeration** — `docs/prds/field-source-kinds.md` exists; δ updates it same-diff. **PASS** (target exists)

## ε — layered synthetic mesh (interface snapping)

- **synthetic-mesh-builders-exist** — `build_beam_mesh` (modal) and the `synthetic_grid_counts` grid (elastic-static) are hand-built, gmsh-free (gmsh is a `[dev-dependencies]` entry of reify-eval only — Cargo.toml verified). **PASS**
- **centroid-sampling-precedent** — the FDM dims path computes per-tet centroids and calls the field locator (elastic_static solve path). **PASS**
- **snapping + straddle-assertion (B9)** — delivered by ε's own tests. Producer: ε itself.

## ζ — heterogeneous modal assembly + Field overload

- **modal-homogeneous-gap-real** — `assemble_modal_km(mesh, density, &IsotropicElastic)` calls legacy `element_stiffness`, not `_with_field` (code read); density is a single global scalar — no per-element mass path exists anywhere (grep: zero `density` hits in `material_field.rs` / `assembly/*.rs`), and `extract_density` returns 0.0 for `Value::Field` (latent, statics-masked). ζ builds both; γ threads `rho`. **PASS** (the gaps ζ/γ close exist, and a leaf owns each)
- **p2-with-field-exists-test-only** — `element_stiffness_p2_with_field` exists and is bit-verified vs the legacy P2 path, but is production-unwired (test-only); ζ wires it. The elastostatic dims path is P1-only and must NOT be mirrored (bending lock: P1 9–11% vs P2 ~1% on slender beams, `modal_benchmarks` record) — C4 names this explicitly. **PASS**
- **eigensolve-untouched** — faer-backed `solve_eigen_dense` / `solve_eigen_shift_invert` operate on assembled K/M only; ζ changes assembly, not the eigensolver. **PASS**
- **field-overload-precedent** — the `Field<Point3<Length>, AnisotropicMaterial>` dims overload of `solve_elastic_static` parses + resolves today (stdlib `solver_elastic.ri`; overload preference over the `ConstitutiveLaw` wildcard documented in-file). **PASS**
- **bi-material-band floor (G6)** — 5% band vs composite-EI Euler-Bernoulli on a slender (L/t ≥ 20) P2 fixture; measured P2 slender accuracy precedent: SS mode −1.6% (#6663 record), cantilever +0.02%. Bound ≫ floor. **PASS**

## η — MSE post-process → Mode.damping_ratio + SE shares

- **element-matrices-recomputable** — element stiffness is a pure fn of (order, phys nodes, material); mesh + samples in scope in the modal trampoline; `ModalAssembly` retains K/M only, so SE is recomputed per mode (design accounts for the cost). **PASS**
- **MSE-vs-laminate band floor (G6)** — 10% band on ζ_mode vs closed-form flexural SE split, slender P2 fixture; discretization + shear-share deviation at λ≥20 is small-single-digit %. Bound > floor with margin. **PASS**
- **degenerate identity** — shared with β (ζ=η/2). **PASS**

## θ — integration exemplar (AFrame-shaped)

- **corpus-gate-exists** — `best_practices_constraint_gate` (in `crates/reify-eval/tests/harness_corpus_gates.rs`, #6215) compiles + constraint-gates `examples/best_practices/`; INDEX.md exists. **PASS**
- **ratio-band floor (G6, decision 7)** — pure-bending analytic bounds for the fixture's η inputs (0.03/0.001): panel ≈3.4× (core flexural SE share 8.3–8.6%, independently re-derived at review), pillar ≈7.8× (EG fill EI share ≈24%); FEA shear participation only raises EG shares. Gate bands: panel [3.0, 25] (floor = analytic minus discretization margin), pillar [5, 25]. The spec's "5–10×" is deliberately NOT asserted. **PASS**
- **consumer** — printer_v01 AFrame (G1); this exemplar is its committed stand-in.

## ι — docs-truth (chunks + cheatsheet + discoverability)

- **chunk-registry-exists** — `crates/reify-mcp/src/tools/chunks/*.md` present (fields.md etc.); `.claude/skills/reify-design/SKILL.md` tracked. **PASS**
- **signature-compile acceptance** — every documented signature compiles in a smoke `.ri` (delivered by ι's own check).

## κ — PRD-close

- **terminal-stamp obligation** — overlay "PRD terminal status" section is the contract; deliverable is the Status flip + AS-AUTHORED freeze + landed-leaf IDs, here and in this manifest. Producer: κ itself.

## μ — [MILESTONE] assembly-derived material fields

- Human decision gate (DO NOT IMPLEMENT); no capability bindings — its deliverable is an escalation. Deps: γ, δ, #6626, #6660.

## D3 workflow adjudication (run wf_7ba44d82-6cc, 2026-08-27, 32 agents)

The per-leaf premise-verification workflow returned `blocks: true` with 6 records; each
was adjudicated by the decompose steward with executed evidence before filing:

- **Every record carrying an executed command PASSED** — including both rejection
  premises (anonymous intersection param type exit 1; `raw_lambda_material_field_rejected.ri`
  exit 1), `examples/fdm_bracket.ri` check 0, both β descriptor fixtures, and
  P2 modal evals of committed `examples/modal/` files.
- **4 blocking records were evidence-free adversary rows** (`exit=?`, no command) directly
  contradicted by executed prover probes in the same journal → adjudicated noise.
- **1 record (ζ) was a harness-expressiveness critique**, not a substrate falsification:
  the `ir/absent` probe kind cannot assert an output *value* on a clean eval. The value
  claim was verified by direct executed eval instead; the harness gap is filed as **#6876**.
- **1 record (η) was a REAL defect in an out-of-scope producer**: `mechanism_modal_analysis`
  returns `Mode.damping_ratio: 0` + `ModalResult.damping: undef` under
  `RayleighDamping(beta: 1e-4)` where ζ≈0.018 is correct — filed as **#6875**. The FEA
  `modal_analysis` path this PRD builds on was then verified healthy by direct eval:
  `Mode.damping_ratio = 0.013031…` = βω/2 exactly at f1 = 41.479 Hz, descriptor echoed,
  shapes populated (probe `fea_rayleigh_zeta_probe.ri`, session probes dir). η's
  premise stands **strengthened**: the existing Rayleigh value-source on this path is
  demonstrably live end-to-end at the author surface.
