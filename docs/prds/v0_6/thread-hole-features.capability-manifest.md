# Capability manifest — thread-hole-features (decompose 2026-07-24)

Binds each leaf signal's asserted capabilities to evidence (G3+G6 mechanized). Probes
run 2026-07-24 against `target/debug/reify` (built Jul 22, embedded stdlib current;
no stdlib file newer than the binary; compiler/eval commits since = diagnostics-cache
work only). Fixtures committed at `docs/prds/v0_6/fixtures/` (thread_/hole_/fit_
prefixes). Machine twin: `thread-hole-features.capability-manifest.yaml` (task_ids
stamped by commit_planning).

## α — thread_solid builtin (boundary rows 1–3)

- `helix-geometry-op-exists` — **PASS** (wired) — `GeometryOp::Helix` +
  `make_helix_wire` in production paths: `crates/reify-ir/src/geometry.rs:365-366,
  917-918`; `crates/reify-kernel-occt/src/ffi.rs:760`; eval ctor
  `crates/reify-eval/src/geometry_ops.rs:3147-3172`.
- `helix-wire-sweepable` — **PASS** (producer:task-5342, upstream dep) — #5342 fixes
  `BRepLib::BuildCurves3d` absence in `make_helix_wire`; its acceptance is verbatim
  "sweep(posed circle profile, helix(24,7,63)) → non-Undef Solid, volume ≈ closed
  form" — exactly the extent α consumes. In-progress, live heartbeat at decompose.
- `sweep-two-geom-arg-lowering` — **PASS** (wired) —
  `crates/reify-compiler/src/geometry.rs:2045-2086` (`SweepKind::Sweep`), geometry-arg
  index table `:297-299`.
- `volume-band-floor` — **PASS** (floor stated) — band ±25% around
  V_core + [0,1.5]·A_profile·L_helix (closed-form core + ridge estimate); floor =
  tessellation/boolean tolerance ≪ band (torus precedent: `assert_volume_near(…,0.02)`).
- `loud-envelope-rejection` — **producer: this leaf** — `E_ThreadSolidOutOfEnvelope` +
  recorded UndefCause are α's own deliverable; house pattern proven at
  `crates/reify-stdlib/src/tolerancing.rs:197-214` (`diagnose`, re-exported to
  reify-expr `lib.rs:1960,1989`). Eval-exit-nonzero substrate: cmd_eval Severity::Error
  gate (task 4458).

## β — #thread_repr pragma + ambient constant (rows 4–6)

- `pragma-grammar` — **PASS** (grammar-fixture) —
  `fixtures/thread_repr_pragma.ri` parses (`tree-sitter parse --quiet` exit 0).
- `unknown-pragma-warns-today` — **PASS** (probe) — `reify check` on the same fixture
  emits `warning: unknown pragma #thread_repr` and exits 0 (captured 2026-07-24);
  β's registration removes the warning for this key; bogus-arg error is β's deliverable.
- `compiled-module-pragma-precedent` — **PASS** (wired) — `apply_module_pragmas`
  `crates/reify-compiler/src/module_pragmas.rs:23-30`; `CompiledModule.kernel_pragma`
  consumed at `crates/reify-eval/src/engine_build.rs:3295,4185,6275`.
- `ambient-constant-injection` — **producer: this leaf** (C2) — dangling-flag risk
  (the #deterministic precedent, module_pragmas.rs:440-448) is closed by β's own
  signal: pragma flip changes eval'd volume (consumer ships in-slice; requires α,
  upstream).

## γ — std.features.holes module (rows 8–10)

- `stdlib-structure-ctor-derived-lets` — **PASS** (probe + fixture) —
  `fixtures/thread_spec_ctor_derived_lets.ri`: `reify eval` prints
  `Test.spec = ThreadSpec{…}` and `Test.td = 0.0042 m` (M5×0.8 tap drill) — let-path
  ctor, named args, derived lets, no stdlib re-declaration.
- `match-enum-in-let` — **PASS** (probe) — `fixtures/fit_match_enum_let.ri` evals
  (`Test.extra = 0.0005 m` for Fit.Medium); `if…then…else` twin
  `fixtures/fit_if_then_else_let.ri`. (ports_mechanical.ri:58-62 comment stale; γ
  updates it.)
- `constraint-in-structure-checked` — **PASS** (wired) — grammar.js:685+531;
  behavioural test `crates/reify-eval/tests/ports_mechanical_thread_eval.rs:191-272`;
  `examples/bracket.ri:10-12`.
- `iso-it-tolerance-wired` — **PASS** (wired) —
  `crates/reify-stdlib/src/tolerancing.rs:112-128`, dispatcher arm `:15-22`, reached
  from `eval_builtin` (`crates/reify-stdlib/src/lib.rs:225,286`); value pin precedent
  24.969 µm (`crates/reify-compiler/tests/tolerancing_tests.rs:1177-1262`).
- `string-param-type` — **PASS** (wired) — `examples/cost_aggregation.ri:22`
  (`param supplier : String`); FitDesignation.letter PTODO(#5391) per C3/INV-SF-5.
- `option-geometry-fill` — **producer: this leaf** (row 10 closes the untested cell) —
  generic machinery PASS (`OptionSome` expr.rs:2196, `some(310MPa)` materials_fea.ri:161,
  `fixtures/thread_form_some_geometry.ri` checks); Geometry payload end-to-end is γ's
  test deliverable.
- `stdlib-enum-ctor-binding` — **PASS** (probe) — stdlib-path enums bind
  (thread_spec_ctor fixture); the user-module wart
  (`fixtures/fit_enum_user_module_wart.ri`, "Enum(Fit)" vs "structure type") is NOT on
  γ's path (all enums stdlib); fix tracked as uniform-member-access ζ = 5429 (no edge).
- `same-structure-cutter-composition` — **PASS** (probe) —
  `fixtures/hole_same_struct_cutter.ri` checks clean (γ's own tests run pre-5426).

## δ — capstan groove (row 11)

- `sweep-along-helix` — **PASS** (producer:task-5342, upstream dep) — same binding as
  α's `helix-wire-sweepable`; δ's DSL-level composition is #5342's literal acceptance
  fixture shape.
- `groove-volume-formula` — **PASS** (floor stated) — ΔV ≈ π·r²·L_helix,
  L_helix = sqrt((2πRn)² + h²) (±15% band; #5342's own acceptance math ≈1.359 m for
  24/7/63 is the basis).
- `target-file-caveat` — **PASS** (wired) — `prj/printer_v01/dev_capstan.ri:27-30`
  MODELLING CAVEATS header names the exact gap + #5342/#5343.

## ε — printer retrofit + first fastener holes (row 12)

- `instance-cutter-consumption` — **PASS** (producer:task-5426, upstream dep) —
  uniform-member-access γ delivers `difference(plate, h.cutter)` on let-instances
  (its fixtures `member_geom_let_instance.ri`/`member_geom_alias.ri`); today's
  rejection captured in `fixtures/hole_cutter_member_gap.ri` ("argument 2 must be a
  geometry expression").
- `feature-structures` — **producer: task γ** (intra-batch, upstream).
- `retrofit-volume-invariance` — **PASS** (floor stated) — same-dims bore swap; < 0.1%
  assertion vs identical closed forms.
- `nested-difference-hazard` — **manual** — #5318 (dense-sieve no-op) documented in
  PRD §7; row 12's volume assertions detect the shape; if reproduced, ε blocks on
  #5318 (no silent workaround).

## ζ — modeled-thread e2e (rows 4–5, example level)

- Producers α, β, γ (intra-batch) + 5426 (consumption) — all upstream. Volume-delta
  band per C1-I1 (derived, not guessed).

## η — docs / chunks / cheatsheet / discoverability

- `chunk-registration-mechanism` — **PASS** (wired) —
  `crates/reify-mcp/src/tools/language_chunks.rs:3-66` (include_str! + match + TOPICS);
  count tests `:77-97` + `crates/reify-mcp/tests/reference_tools_tests.rs:35-129`
  (η updates the 17-topic assertions).
- `chunk-serving-path` — **PASS** (wired) — `crates/reify-mcp/src/tools/reference.rs:24`
  (`get_chunk` behind `reify_language_reference`).
- `cheatsheet-exists` — **PASS** (wired) — `.claude/skills/reify-design/SKILL.md:24-55`.
- `doc-gap-real` — **PASS** (probe) — zero hits for ThreadSpec/tap_drill/counterbore/
  helix in chunks (2026-07-24 grep); signature-verification is in-task (no drift test
  exists — the stale-torus caveat at main-tree printer.ri:128-133 is the live example).

## θ — integration gate (rows 1–12)

- Aggregates the above; every producer upstream (α β γ δ ε ζ + 5342 + 5426). New
  gate-resident harness registers drift-guards same-diff (run-all classification /
  wallclock-bounds / nextest partitions as applicable) — **manual** binding, enforced
  by this session's decompose walk per the overlay's gate-test rule.

## Non-leaf batch entries

- Docs-landing task (deterministic; merge-queue provenance; roots α/δ dep on it).
- Broader-DFM bookmark [MILESTONE] (pending; no capabilities — trigger task).
