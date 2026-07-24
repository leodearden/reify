# Capability manifest — uniform-member-access

Mechanizes G3+G6 for `docs/prds/v0_6/uniform-member-access.md` (per `.claude/skills/prd/project.md` → *Capability Manifest — reify evidence forms*). Probes run 2026-07-24 against main `ef6863e770` (debug binary, user-module-only fixtures — no stdlib-embedding staleness exposure); fixtures committed at `docs/prds/v0_6/fixtures/member_*.ri`. Machine-readable sidecar: `uniform-member-access.capability-manifest.yaml` (stamped by `commit_planning`). D3 adversarial verification: `scripts/prd-decompose-verify.mjs` run `wf_ffb35317-a74` (this decompose session).

Grammar gate: **N/A batch-wide** — every target shape parses today (all probes reached semantic diagnostics, never parse errors); `grammar_confirmed=true` on every task.

## α — uniform member-path resolver (intermediate)

| Capability | Evidence | Verdict |
|---|---|---|
| chained scalar reads already evaluate (regression floor, not new work) | probe: `reify eval member_chain_scalar.ri` → `Test.v = 0.014 m`, `Test.direct = 0.007 m` (ctor overrides threaded through nested instances) | PASS |
| derived-let instance reads already evaluate | probe: `member_let_read.ri` → `Test.drill = 0.008 m` | PASS |
| privacy substrate exists (D6 reuses, not invents) | `crates/reify-compiler/tests/priv_member_visibility_tests.rs` on main; `E_PRIV_MEMBER_ACCESS` registered `crates/reify-core/src/diagnostics.rs:3391` | PASS (wired) |
| member read-back path to absorb | `template.value_cells` read-back `crates/reify-compiler/src/expr.rs:6506-6560`; `materialize_template_lets` `crates/reify-expr/src/lib.rs:1123-1179` | PASS (wired) |

## β — type-driven geometry-position acceptance (intermediate)

| Capability | Evidence | Verdict |
|---|---|---|
| geometry-position rejection is syntactic today (the gap is real) | probe: `member_geom_let_instance.ri` → `difference() argument 2 must be a geometry expression`, exit 1 (rejection OBSERVED — G6 branch 4) | PASS |
| sub path acceptance to generalize from | probe: `member_sub_geom_baseline.ri` → check exit 0 (`GeomRef::Sub` path, `geometry_boolean.rs:40-107`) | PASS (wired) |
| registry cross-check harness to extend | task-1733 pattern, `geometry.rs` §Registry cross-check tests | PASS (wired) |

## γ — per-instance lazy realization (LEAF: the geometry-member-in-geometry-ops capability)

| Capability | Evidence | Verdict |
|---|---|---|
| target is non-vacuous: let-instance member rejected today | probe: `member_geom_let_instance.ri` exit 1 + diagnostic observed; alias form `member_geom_alias.ri` identically | PASS |
| mechanism gap located: geometry members absent from instance values | probe: `member_instance_geom_field.ri` eval → `Test.c = Cutter { d, half_d }` (no `body`); geometry lets have no value cell (`entity.rs:1327-1330`) | PASS |
| elaboration machinery to generalize (D5 producer) | `elaborate_child_instance` `crates/reify-eval/src/unfold.rs:287`, ctor-override threading proven (task 3814, done) | PASS (wired) |
| R3f no-clobber constraint honored (C3-vi) | `docs/design/symbolic-eval-nested-selector-resolution.md` §R3f; regression witness `restrict_field_b5_integration` | PASS (constraint recorded) |
| numeric bound achievable: drilled volume within 1% of closed form | basis: task-325 volume-vs-closed-form precedent (sphere/box/Pappus); OCCT boolean volume accuracy ≫ 1% on prism∖cylinder; floor ≪ bound | PASS |
| eval realizes geometry (the volume signal is observable under `reify eval`) | probe: `member_instance_geom_field.ri` eval emitted OCCT realization + `<Geometry: Cutter#realization[0]>` template value | PASS |

## δ — chained geometry member paths (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| chain-ending-in-geometry rejected today | probe: `member_chain_geom.ri` exit 1, same diagnostic observed | PASS |
| scalar-chain substrate works (only the geometry endpoint is new) | probe: `member_chain_scalar.ri` eval values correct (§α row 1) | PASS |
| nested-SUB chain elaboration | `producer:task-5360` (pending-high, **upstream** — real `add_dependency` edge δ←5360; anti-inversion checked: 5360 has no deps on this batch) | PASS (queued upstream) |

## ε — .ri fn returning Geometry (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| rejection today is the wildcard arm, not a parse failure | probe: `member_fn_geometry.ri` → `unsupported geometry function: clearance_cutter`, exit 1 (rejection observed; `geometry.rs:2620`, `UNSUPPORTED_GEOMETRY_FN_MSG:2668`) | PASS |
| fn-with-Geometry-return grammar exists | fixture parses (semantic diagnostic reached) | PASS |

## ζ — enum ctor binding (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| over-rejection is real and specific | probe: `member_enum_ctor.ri` → `argument 'fit' has type 'Enum(Fit)' but param 'fit' requires structure type 'Fit'`, exit 1 (observed) | PASS |
| stdlib enum params bind (asymmetry proves resolvability) | brief probe corpus 2026-07-24: `ThreadSpec(...)` enum args bind on the let path | PASS |
| constraint premise arithmetically true | `4mm + 0.2mm > 4mm` (trivial) | PASS |

## η — sub-path consolidation (intermediate)

| Capability | Evidence | Verdict |
|---|---|---|
| suites that pin behavioral parity exist | `cross_sub_geometry_diagnostic_tests.rs`, `solid_param_tests.rs`, priv suites — on main under `crates/reify-compiler/tests/` | PASS (wired) |
| duplication to remove is real (INV-5 driver) | twin `match_self_sub_member` sites (task 3682 precedent), shape probes in `geometry_boolean.rs:40-107` + `expr.rs` value-level sites | PASS |

## θ — docs/doc-chunks/cheatsheet/discoverability (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| doc-chunk surface exists | `crates/reify-mcp/src/tools/chunks/` on main | PASS (wired) |
| cheatsheet surface exists | `reify-design` skill (`.claude/skills/reify-design/`) | PASS (wired) |

## ι — boundary-test suite (LEAF, integration gate)

| Capability | Evidence | Verdict |
|---|---|---|
| all 12 rows' fixtures committed + probed | §2 of the PRD (this manifest's probe corpus) | PASS |
| negative rows (7, 10, 11) are rejection-mechanism-backed | rows 10/11 assert NEW diagnostics delivered by β/α (upstream in-batch); row 7 extends live `E_PRIV_MEMBER_ACCESS` | PASS |
| drift-guard rule acknowledged | any new gate-resident `tests/infra/test_*.sh` / wallclock assertion carries same-diff registration (overlay §Gate-test drift-guard); nextest heavy/smoke classification same-diff for kernel-bound e2e | recorded |
