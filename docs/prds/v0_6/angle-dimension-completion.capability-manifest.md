# Capability manifest — angle-dimension-completion (P)

Binds each leaf's signal capabilities to evidence (G3 + G6 mechanized). Evidence gathered
2026-08-10 at HEAD `9ca5c6ad9f` by the chartering session's six-agent investigation; probes
ran `target/debug/reify` (2026-08-05; interim commits docs-only). Sidecar twin:
`angle-dimension-completion.capability-manifest.yaml`.

## α — CURVATURE → rad·m⁻¹

- **curvature-const-exists-m⁻¹-today** → grep:`crates/reify-core/src/dimension.rs:259`
  `from_exps(&[(0, -1)])`, doc :248-258 (the 3603 alias rationale α re-adjudicates). PASS.
- **literal-pin-flips-deliberately** → grep:`dimension.rs:1909-1931`
  `curvature_constant_is_length_inverse` asserts slot 7 == ZERO — the RED test α turns. PASS.
- **production-duplicate-bound** → grep:`crates/reify-eval/src/geometry_ops.rs:9166-9170`
  hand-built `CURVATURE_DIM` (from_exps is private); test-local twin
  `geometry_ops/tests.rs:16885-16889`. No drift test exists today
  (`value_type_kind_matches` is kind-only, `reify-eval/src/lib.rs:283`) — α adds the
  drift-pin. PASS (hazard bound, not asserted-fixed).
- **rejection-mechanism-fires (G6 branch 4)** → probe: `param t : Angle = 5mm` →
  `reify check` exit 1, "declared … but its initializer evaluates to …" (control observed
  2026-08-10); therefore `tests/prd-gate/fixtures/curvature_rad_literal.ri`
  (`param kc : Curvature = 0.2rad / 1mm`) is rejected pre-α and accepted post-α — the D3
  workflow probes the pre-state rejection empirically. PASS.
- **rad·m⁻¹-arithmetic-exists** → probe: `1deg / 1mm` → `17.453292519943297 rad·m^-1`
  (composed display already renders it). PASS.
- **no-corpus-migration** → grep: zero `: Curvature` declarations in stdlib/examples/tests
  (sole analog is the AbsorptionCoeff twin, `materials_optical_tests.rs:178`); zero code
  computes radius-of-curvature from a curvature value. PASS.
- **numeric-pins-unaffected** → grep: all curvature numeric pins are raw f64 vs OCCT
  (`kernel_queries_curvature_smoke.rs:144-195` etc.); η = 1 keeps values byte-identical. PASS.
- **consumer-wired** → producer-consumer edge: registry τ8 (#6010, pending, unclaimed,
  deps [6002]) writes the curvature query row; edge 6010 → α wired at decompose. PASS.

## β — Doctrine artifacts

- **invariants-registry-exists** → grep:`docs/invariants.md` table format
  `| ID | Invariant | Enforcement | Status | Owner`; INV-DIM row enters `proposed`/`doc+test`
  honestly (no enforcement claim). PASS.
- **design-invariant-family-appendable** → grep:`docs/legibility/design-invariants.md:21`
  "Other invariant families may be appended later; keep slugs stable"; G7 + /review read the
  file at run time. PASS.
- **5825-carve-out-discharged** → task #5825 (done) text: "arc = r · theta … Needs its own
  task — do NOT fold it in" — β is that owner for the teaching half. PASS.
- **idiom-substrate-real** → probe: `(s/r) * 1rad` → `2.5 rad`; `theta / 1rad` → `2.5`
  dimensionless; `: Angle` annotation checks; `1 rad` (spaced) is a parse error. PASS.

## γ — Docs-truth four-pack

- **grammar-fixture** → `tests/prd-gate/fixtures/angle_crossing_idiom.ri` parses
  (`tree-sitter parse --quiet` exit 0, measured 2026-08-10). PASS.
- **chunk-slot-disjoint** → pending #5790 (ξ) owns the angle-surface slice of
  `chunks/units.md` (:50-52 today); γ adds a new crossing-doctrine section — same file,
  disjoint content (PRD D9). PASS (seam declared, no edge).
- **corpus-slot-disjoint** → pending #5792 (ο) owns `best_practices/angles_and_torque.ri`;
  γ adds `angle_crossings.ri` + its own INDEX row; INDEX↔dir bidirectional test
  (`examples_smoke.rs::best_practices_index_matches_corpus_directory`) gates it. PASS.
- **skill-index-format** → `.claude/skills/reify-design/SKILL.md` "Probe-verified idioms —
  index" one-line-per-idiom format (:59-92). PASS.

## δ — ANGULAR_ACCELERATION + closure exemplar

- **closure-identities-true-under-rad⁻²-MOI (G6 branch 1/2)** → probe (scratch eval):
  `kg·m²·rad⁻²` × `(rad/s)²` → exactly `m^2·kg·s^-2` (ENERGY); τ/I → `rad·s^-2`;
  L = Iω → `m^2·kg·s^-1·rad^-1`; L/t → exactly TORQUE. Closed-form dimension-vector
  identities — no numeric tolerance asserted anywhere. PASS.
- **today's-MOI-does-not-close (the dep's necessity)** → probe: with kg·m² MOI,
  ½Iω² → `m^2·kg·rad^2·s^-2` (matches nothing). The capability is delivered by upstream
  **#5844** (pending; DAG-direction: δ → 5844 wired at decompose — G6 branch 3). PASS.
- **constant-is-new-with-named-consumer** → grep: zero `ANGULAR_ACCELERATION` hits repo-wide;
  consumers named: τ/I display, the CI exemplar (same diff), future typed q̈;
  `Rate<AngularVelocity>` (units.ri:116) already composes the vector — δ adds the *name*. PASS.
- **angular-momentum-NOT-chartered** → grep: surfaced nowhere typed or raw (only RNEA's
  internal f64 gyroscopic term) — deferred per G1 (§11). PASS (scope honesty).

## σ — `ElasticResult.shear_angles` channel

- **producer-data-exists (field-population)** → grep:`compute_targets/elastic_static.rs:282`
  `fea.nodal_gradient: Vec<[[f64;3];3]>` stored on the production path; wrap-time projections
  at `:1127-1179` (divergence/gradient/curl) are the exact pattern; the channel is a
  ~5-line linear map + one resample tuple. No `Undef` sentinel involved. PASS.
- **declared-codomain-wrap** → grep:`crates/reify-expr/src/sampled.rs:137-193` stride-n
  sampling reads the declared `Type::Vector{quantity}`; `:297-305` wraps non-dimensionless
  scalars as `Value::Scalar{dimension}` — an Angle codomain needs zero new machinery. PASS.
- **channel-attachment-precedent** → grep:`compute_targets/mod.rs:166-173`
  `sampled_curl_field`; `.ri` decl site `solver_elastic.ri:546` area. #6164 (pending) is the
  sibling; σ → 6164 wired (file-lock + pattern coherence). PASS.
- **tensor-single-quantity (why a channel, not tensor storage)** →
  grep:`crates/reify-core/src/ty.rs:281-286` one `quantity` slot; author component access
  does not exist (`expr.rs:4712-4728` IndexAccess rejects tensors; `:4418-4433` member
  access rejects) — a named reading path is structurally the only one. PASS.
- **consumer-shipped-same-diff** → the worked shear-limit example is part of σ's own diff
  (G1: no current corpus demand exists — the example IS the named consumer, C-as-integration-
  gate shape). PASS (declared, not pre-existing).
- **registry-invisible** → I-REG-5 / decision 9: registry never claims `.ri`-declared names —
  no τ-row collision. PASS.

## ι — Boundary declarations

- **boundaries-enumerated** → investigation table (PRD §2.6, §5 D8): angular traffic today at
  STEP cone semi-angle (`occt_wrapper.cpp` export_step; probe: `CONICAL_SURFACE` writes raw
  radians under OCCT's *default* radian unit), OCCT `angle_rad` FFI fields, SolveSpace
  `angle_deg` (degrees, `solvespace.rs:230`), MCP `set_parameter` (inbound bare f64 →
  installed as SI radians silently, `mcp_context.rs:499-530`; schema text misdescribes),
  dynamics `compliance_cell_f64` silent erasure (`dynamics/eval.rs:98-110`), 2π marshalling
  (`input_shape.rs:116-126`, `free_vibration.rs:25-31`). PASS.
- **declaration-precedents** → `eigenvalue_to_frequency_hz` name+formula;
  ZVShaper marshalling-boundary comments; fdm m→mm convention comment
  (`fdm_slice.rs:299-306`); `spring_rate_for_lumped_dof` refusal guard + coded warning
  (`modal_ops.rs:1300-1316`). PASS.
- **length-mislabel-not-owned-here** → the STEP/3MF 1000× length defect is filed as a
  standalone bug task (§11); ι declares the *angle* regime and coordinates (§12 Q4). PASS.

## D3 verification record (run `wf_7c1a08bd-af5`, 2026-08-10)

The decompose-time Enumerator→Prover‖Adversary→Synthesize workflow returned four blocking
findings; each is adjudicated here (the gate's own resolution paths):

1. **α/β "fixtures not committed on main"** — true at probe time by construction: the
   sanctioned reify landing sequence writes artifacts untracked, files the batch, stamps the
   sidecar via `commit_planning`, then lands EVERYTHING in one hook-gated commit in the same
   session turn (overlay "Landing PRD artifacts"). The fixtures (`angle_crossing_idiom.ri`,
   `curvature_rad_literal.ri`, `rotational_closure_prestate.ri`,
   `shear_angles_field_decl_pre.ri`, `shear_angles_vec3_angle_param_pre.ri`,
   `shear_angles_vec3_wrongq_ctrl.ri`) are in that commit. Sequencing gate, not a content
   defect — the adversary's own probes on the fixtures all PASS.
2. **δ "ir-kind cannot evaluate stdout"** — real harness limitation; resolved by REBINDING:
   the closure identity is pinned by a typed param (`param rot_ke : Energy = …` in
   `rotational_closure_prestate.ri`) so `reify check` exit 0/1 IS the machine gate.
   Re-probed directly 2026-08-10: typed-Energy exit 0; `: Torque` control exit 1 with the
   two-dimension diagnostic; parse exit 0. Stdout-vector identities stay prose evidence.
3. **σ "no Vector3 component extraction / deg comparison in-language"** — real substrate
   gap (`.x` / `v[0]` / `norm<` all reject; reduction-builtin "resolution" at the check
   surface is the known unknown-fn silent-accept, vacuous). Adjudicated per the adversary's
   option (1)+(3): component-value comparisons live in the Rust e2e pins
   (`differential_field_ops_e2e.rs`, same home as #6164's); the `.ri` example scopes to
   sampling + typed passing. PRD §9 B4 / §12 Q6 updated. Also applies to #6164's signal —
   flagged in the hand-back.
4. **Bonus adversary confirmations (recorded as PASS evidence):** the σ channel-decl
   spelling `Field<Point3<Length>, Vector3<Angle>>` parses AND resolves (previously
   unexercised Angle codomain in the `Field<D,C>` arm); the `Vector3<Angle>` accept-probe is
   non-vacuous (wrong-quantity rejection observed); α's `curvature_smoke.ri` exit-0 is
   VACUOUS w.r.t. the dimension flip (kernel-less graceful skip) — the real post-α corpus
   guards are Rust-side; do not cite example exit-0 as corpus-safety evidence.

## υ — GUI channel bridge

- **wire-format-gap** → grep:`gui/src-tauri/src/types.rs:731`
  `scalar_channels: HashMap<String, Vec<f32>>` — no unit/dimension slot; producer list is
  Pa-only today (`engine.rs:6785-6847`). PASS (gap bound).
- **signed-clamp-gap** → agent-verified `scalarRange.ts:38-40` drops negative values; no
  signed channel exists today, so #6164's `.rotation` would be the first to hit it. PASS.
- **consumer** → #6164's `.rotation` channel (ruled, pending) + honest legends for existing
  Pa channels. Recommend Leo wire 6164 → υ (P cannot mutate 6164 — session constraint). PASS.
