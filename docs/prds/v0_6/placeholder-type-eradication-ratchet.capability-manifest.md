# Capability manifest — placeholder-type-eradication-ratchet

PRD: `docs/prds/v0_6/placeholder-type-eradication-ratchet.md` · authored 2026-07-24.
Binds each leaf signal's asserted capabilities to evidence (mechanized G3+G6). All
file:line evidence verified on main 2026-07-24; probe baselines run against
`target/debug/reify` built 2026-07-24 14:59 (post-dates last stdlib change 2026-07-19).
Machine-readable twin: `placeholder-type-eradication-ratchet.capability-manifest.yaml`.

**D3 workflow note (2026-07-24):** `scripts/prd-decompose-verify.mjs` was invoked
(run wf_39c2297a-44b) but every subagent failed on the account's weekly usage limit
(resets Jul 27) — its `blocks:true` output is a harness error, not a premise verdict.
The manual probe-set equivalent was executed in-session with captured output; every
premise below marked "probe 2026-07-24" is one of those runs. Probe transcript:
`takes_marker(5mm)` vs `structure def Marker` param → exit 1
`no matching overload for takes_marker(Scalar[m])`; `takes_mech(mechanism())` → exit 0;
`takes_mech(1.0)` → exit 1 `no matching overload for takes_mech(Real)`;
`flexure_compliance(5mm)` → exit 0 + `W_FLEXURE_NON_JOINT_ARG` only;
`flexure_compliance(prb_notch_circular(…))` → exit 0;
`PiecewisePolynomialProfile(mechanism: 1.0)` → exit 0, no diagnostic;
`BucklingOptions(mode: "shift_invert")` → exit 0; `unwrap_or(some(5mm), 0mm)` → exit 0.

## α — FlexureJoint marker vertical slice

| Capability | Evidence | Verdict |
|---|---|---|
| exact-equality overload rejection (bare literal cannot match a StructureRef param) | `grep:crates/reify-compiler/src/type_compat.rs:1176-1183` — concrete non-generic param match rule is `param_ty == arg_ty`; NoMatch → poison error `expr.rs:2958-3037` (production path) | PASS (wired) |
| builtin-ladder signature-arm seam for typing `prb_*` | `grep:crates/reify-compiler/src/expr.rs:3417` — `is_joint_typed_fn` arm (#4311 template); first-arg fallback being displaced at `expr.rs:3540` | PASS (wired) |
| `structure def` name → `Type::StructureRef` resolution | `grep:crates/reify-compiler/src/type_resolution.rs:656` | PASS (wired) |
| `DrivingJoint` trait + conformance machinery for `FlexureJoint : DrivingJoint` | kinematic.ri trait hierarchy (#3845 done); `satisfies_trait_bound` `entity.rs:3732` (#4310 done) | PASS |
| same-module visibility of a new structure def to same-file fn bodies | task 3895 pre-pass; recorded at `flexures.ri:196-198` | PASS |
| DiagnosticCode registry accepts a new code | `crates/reify-core/src/diagnostics.rs:156` enum + reserve-then-emit precedent (#4308) | PASS |
| rejection asserted (G6 branch 4): `flexure_compliance(5mm)` → coded Error, exit ≠ 0 | **baseline probe 2026-07-24**: exits 0, `All constraints satisfied.`, only `W_FLEXURE_NON_JOINT_ARG` Warning — silent-accept confirmed; the rejection is **this leaf's own deliverable** (producer = α), asserted by its committed negative fixture BT1 | PASS (self-delivered) |

## β — kinematic-family struct-field retargets

| Capability | Evidence | Verdict |
|---|---|---|
| `Mechanism` / `BodyId` nominal types exist + reachable cross-module | kinematic.ri:30/:237 (#3845); stdlib DAG foundation #4574 done | PASS |
| producers statically typed (`mechanism()`/`body()` → Mechanism, `body_id_of` → BodyId, joint ctors → kind StructureRefs) | `grep:crates/reify-compiler/src/joint_signatures.rs:127` (#4311 done, wired `expr.rs:3417`) | PASS (wired) |
| ctor field-type rejection at Error severity (G6 branch 4) | `producer:task-5306` (upstream via real dep edge; α #5302 in-progress supplies the checker). **Baseline probe 2026-07-24**: `PiecewisePolynomialProfile(mechanism: 1.0)` exits 0 with NO diagnostic — silent-accept confirmed, rejection owned upstream | PASS (producer-upstream) |
| eval-time guard posture where static typing can't bite | `E_MECHANISM_NONDRIVING_JOINT` runtime guard (#4309 done) | PASS |
| examples migrate off dummy literals to real producers | `mechanism()` builtin (`crates/reify-stdlib/src/mechanism.rs:187-202`) produces the runtime handle; dummy site inventoried (`examples/trajectory/tots_optimal_ptp.ri:58`) | PASS |

## γ — blanket-escape purge + census refresh

| Capability | Evidence | Verdict |
|---|---|---|
| canonical-cite grammar accepted by PTODO | `crates/reify-audit/src/ptodo.rs` `has_canonical_cite`/`extract_cites` scan the whole marker line (stdlib-surface-type-substrate decision 6 precedent) | PASS |
| census doc exists to refresh | `docs/notes/stdlib-real-placeholder-audit.md` (task 3090) | PASS |
| live cite target for joint-value markers | sibling η filed in the same batch (id stamped at decompose; γ requires η to *exist and stay non-terminal*, not to complete — no DAG inversion) | PASS |
| `Velocity` alias removal is safe | units.ri:86-95 — alias recorded dead since #4580 registered the named dimension and builtins shadow it; removal-safety re-verified by repo grep in-task | PASS |
| escape inventory (28 lines, 7 files) | `git grep -n 'ptodo:allow type-placeholder'` snapshot 2026-07-24 (kinematic 6, trajectory 8, dynamics 6, flexures 4, units 1, trajectory_fns 1, fdm 1, +flexures header 1) | PASS |

## δ — PTYPE detector + hard gate

| Capability | Evidence | Verdict |
|---|---|---|
| per-pattern detector registry + CLI plumbing | `crates/reify-audit/src/lib.rs:88` Pattern enum; bin allowlist `bin/reify-audit.rs:264`; dispatch loop `:613-664` | PASS (wired) |
| verify-pipeline hard-gate seam | `scripts/verify.sh:1994-2114` pre-build + `tests/infra/test_reify_audit_ptodo.sh` exit-code gate (High count = exit code, #4559) | PASS (wired) |
| liveness lane available to extract + share | `ptodo.rs` β-lane (read-only tasks.db resolution, §6.7 fail-soft contract) | PASS |
| infra-test drift-guard registration path | `tests/infra/run-all-classification.manifest` present (184 lines); same-diff rule per overlay | PASS |
| zero-baseline at landing | `producer:task-γ` upstream (hard dep) — BT9 | PASS (producer-upstream) |

## ε — mode/kind String→enum retargets

| Capability | Evidence | Verdict |
|---|---|---|
| enum surface substrate | `Type::Enum(String)` (`ty.rs:130`); .ri enum-def precedent (materials_chemical.ri `CorrosionClass` et al.) | PASS |
| ctor rejection at Error severity | `producer:task-5306` (upstream via real dep edge) — same chokepoint as β | PASS (producer-upstream) |
| target sites inventoried | solver_buckling.ri:84 `mode : String`; tensegrity.ri:143/:178 `kind : String` | PASS |

## ζ — combinator-intercept drift guards

| Capability | Evidence | Verdict |
|---|---|---|
| intercept exists on the production eval path | `crates/reify-expr/src/lib.rs:735-775` name+arity gate → `option_recovery.rs` `eval_combinator` (+ `map_or`/`map_err` ctx arms) | PASS (wired) |
| stdlib-compiled discriminating harness precedent | `crates/reify-expr/tests/option_recovery_eval_tests.rs:275` (`e2e_unwrap_or_some_5mm_with_stdlib`) | PASS |
| coverage gap confirmed (the guard is not redundant) | `result_combinator_eval_tests.rs` / `result_fallback_eval_tests.rs`: zero `compile_source_with_stdlib` uses (2026-07-24 grep) — Result family, `map_or`, `map_err`, absent-key `get_or` undiscriminated | PASS |

## η — [MILESTONE] joint-value-type owner

| Capability | Evidence | Verdict |
|---|---|---|
| decision gate only at filing time — no code capability asserted; the task's existence is γ's cite substrate | typed mechanism/joint surface (β) wired as upstream substrate for the eventual decision | PASS (manual) |
