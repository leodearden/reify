# Capability manifest — constrained-2d-sketch.md

Per-leaf capability→evidence bindings (mechanized G3+G6), decompose session 2026-07-25.
Machine-readable twin: `constrained-2d-sketch.capability-manifest.yaml` (stamped at
`commit_planning`). D3 verification run: workflow `wf_51d57861-830` (Enumerator →
Prover ‖ Adversary → Synthesize over the three leaves); pre-state probe results and
dispositions recorded per binding below.

Batch shape: α (grammar) → γ (lowering) → δ (solve, also ← β solver substrate) →
ε (profile/kernel) → ζ (2020 exemplar leaf); η (auto) ← δ; θ (docs) ← ε, ζ.
Leaves: ζ, η, θ.

## ζ — 2020 extrusion exemplar (integration-gate leaf)

- **sketch-block-grammar** → `producer:task-α` (upstream, intra-batch; α commits the
  fixtures — content: branch `task/5514` @ `3e850bab7e`). Pre-state
  evidence: `tests/prd-gate/fixtures/sketch_block_target.ri` FAILS today —
  `tree-sitter parse --quiet` exit 1 (probed 2026-07-25) and `reify check` rejects
  with a syntax error (D3 ζ premise 1, rejection observed). α IS the grammar
  producer (`grammar_confirmed=false` on α). PASS.
- **profile-slot-member-acceptance** (substrate) — a member-bound profile is legal
  in PROFILE_SLOT position today: `tests/prd-gate/fixtures/sketch_member_extrude_premise.ri`
  resolves (`reify check` exit 0; D3 ζ premise 2); precondition machinery
  `crates/reify-compiler/src/geometry.rs:727,748` (Surface ∧ Closed ∧ Planar). PASS.
- **aux-construction-geometry** (substrate) — `aux let` parses and compiles today:
  `tests/prd-gate/fixtures/sketch_aux_let_premise.ri` resolves (D3 ζ premise 3);
  `is_aux` marking + body-set exclusion landed with placement-relations-belt §4. PASS.
- **libslvs-ffi-solves** (substrate + producer) — FFI link is real: `Slvs_Solve`
  exercised by CI-run `crates/reify-constraints/tests/solvespace_tests.rs` (synthetic
  system). The `.ri`→libslvs path is batch-delivered: `producer:task-β` (direct-build
  API) + `producer:task-δ` (solve pass), both upstream of ζ. Known pre-state: the
  legacy `recognize_pattern` route is dead on real `.ri` input (PRD §2) — this is
  MOTIVATION for β/δ, not evidence against them. PASS.
- **sketch-solve-diagnostics** → `producer:task-δ` (upstream). `E_SKETCH_*` codes are
  batch-delivered; pre-state absence is expected (disposition: producer-self, G6
  branch 4 — δ's own fixtures observe each rejection fire). PASS.
- **sketch-profile-op** → `producer:task-ε` (upstream). `GeometryOp::SketchProfile`
  + kernel face-with-holes are batch-delivered. PASS.
- **exemplar-ci-gate** (substrate) — `examples/best_practices/` auto-compile gate
  exists on main: `crates/reify-compiler/tests/examples_smoke.rs` (corpus seeded by
  task #5397). PASS.
- **bbox-within-1µm** (numeric floor) — bound is driven-dimension reproduction, not
  method accuracy: dimensions are equality constraints; libslvs converges to
  ~1e-8 m residuals on well-conditioned 2D systems, floor ≪ 1µm bound. `floor:
  1µm > ~1e-8 m`. PASS.

## η — auto(seed) in sketches

- **auto-seed-grammar** → `producer:task-α` (upstream). Pre-state evidence:
  `tests/prd-gate/fixtures/sketch_auto_seed_target.ri` FAILS today — parse exit 1
  (probed 2026-07-25) and `reify check` rejects (D3 η premise, rejection observed).
  PASS.
- **dof-auto-coverage-accounting** → `producer:task-δ` (upstream) delivers the DOF
  ledger + coverage machinery; η delivers the auto-coverage arm (producer-self for
  the nearest-to-seed semantics; η's twin-pair fixtures observe both outcomes). PASS.
- **underconstrained-rejection** (G6 branch 4) → `producer:task-δ` (upstream),
  producer-self observation: η's twin fixture (auto removed) observes
  `E_SKETCH_UNDERCONSTRAINED` fire — never a silent solve. PASS.

## θ — docs-truth (chunk + cheatsheet + discoverability)

- **chunk-smoke-gate** (substrate) — chunk-vs-compiler drift gate exists on main:
  `crates/reify-compiler/tests/geometry_chunk_smoke.rs` (task 5364). θ adds the
  sketch smoke fixture(s) there; every documented signature compiles as written. PASS.
- **documented-surface-exists** → `producer:task-ε` + `producer:task-ζ` (upstream) —
  θ documents only batch-delivered surface; ordered by real dep edges, not prose. PASS.
- **discoverability** — intent-level findability ("draw a custom profile", "constrain
  a sketch") via chunk topic naming + `examples/best_practices/INDEX.md` line (ζ) +
  `.claude/skills/reify-design/SKILL.md` index line (θ). Manual acceptance. PASS.

## Dispositions

- No `declared-only` / `test-only` / `producer-downstream` / `bound≤floor` /
  `rejection-absent` bindings remain. All batch-delivered capabilities bind to
  upstream producers within the batch (DAG-direction verified in Step 4 wiring).
- Pre-state FAILs from D3 probes on batch-delivered capabilities (novel grammar
  rejected today; E_SKETCH_* absent today) are the **expected pre-state** — they are
  the RED targets of their upstream producers (α, δ), per the stamping/D3 SOP.
