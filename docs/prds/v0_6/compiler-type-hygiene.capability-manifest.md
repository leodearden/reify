# Capability manifest — compiler-type-hygiene (wave 2)

PRD: `docs/prds/v0_6/compiler-type-hygiene.md` · Built at decompose, 2026-07-06, against main `e4443acd6b` (anchors verified against `4d696e63cb`, the parent of the PRD commit).
Probe harness: `python3 scripts/prd-capability-check.py tests/prd-gate/compiler-type-hygiene-probe-set.json` → **4/4 PASS, exit 0** (run 2026-07-06; transcripts in the session log; probes are re-runnable in CI).

Evidence forms per the reify overlay: `grep:<file>:<line>` = wired-on-main; `probe:<n>` = a PASS row in the committed probe set; `producer:task-<label>` = upstream task in this batch.

## α — trait type-arg rejection (leaf)

| Capability asserted by the signal | Evidence | Verdict |
|---|---|---|
| `SomeTrait<Foo>` param-annotation syntax parses (no novel grammar) | probe:1 (grammar, 0 ERROR nodes) | PASS |
| Rejection is genuinely NEW — today silent-accept (negative-assertion pre-state) | probe:2 (`reify check` exit 0, `All constraints satisfied.`, no diagnostic); code path grep: `type_resolution.rs:1704-1724` structure intercept, `:1728` fallthrough, `:658-662` bare TraitObject return | PASS (pre-state bound; α's RED test asserts the post-state) |
| 4603 arm shape to mirror exists & is wired | grep:`crates/reify-compiler/src/type_resolution.rs:1688-1724` (production resolution path) | PASS |
| Diagnostic-code family to join exists | grep:`crates/reify-core/src/diagnostics.rs:2173` (TypeArgArity), `:2196` (TypeArgBound) | PASS |
| Breadcrumb cite target is a live, non-terminal task (PTODO gate) | task #5024, status `deferred` (bookmark), verified via fused-memory get_task 2026-07-06 | PASS |

## β1 — runtime truth-table inventory (intermediate → unlocks β2/β3)

| Capability | Evidence | Verdict |
|---|---|---|
| eval_mul / eval_div cascades exist with enumerable intentional arms | grep:`crates/reify-expr/src/lib.rs:4354-4629` (eval_mul), `:4631-4780` (eval_div); read in full 2026-07-06 | PASS |
| Public probe path for an integration test (no pub(crate) needed) | `eval_expr` is pub; existing tests `:8752-8818` already pin Scalar arms from tests | PASS |

## β2 — static Mul/Div table + E_ArithOperandKind (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| Today's mistyping is real (silent Int / guard-defeat) | probe:3, probe:4; grep:`crates/reify-compiler/src/type_compat.rs:1576-1596` (`_ => Type::Int` at :1588/:1595, scalar-preserving arm :1580-1587) | PASS |
| Guard region + precedent guards wired on the production compile path | grep:`crates/reify-compiler/src/expr.rs:1611` (Pow), `:1705` (Mod), `:1728` (Add/Sub), `:1781` (Cmp/4490), `:1806` (logical) | PASS |
| Gradualism substrate (Error/TypeParam pass-through) already structural | grep:`crates/reify-compiler/src/type_compat.rs:1541-1564` (4629 W5 block covers Mul/Div) | PASS |
| Diagnostic-code precedent | grep:`crates/reify-core/src/diagnostics.rs:3092` (CmpOperandKind) | PASS |
| Runtime-supported combos really evaluate (correct-typing bucket is non-empty) | probe:4 stderr/eval transcript — `a * 2.0` evals `vec(2 m, 4 m, 6 m)` while statically Scalar | PASS |

## β3 — static-vs-runtime parity test (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| Cross-crate test placement legal (compiler test can import reify-expr) | grep:`crates/reify-compiler/Cargo.toml` [dev-dependencies] `reify-expr.workspace = true`, `reify-eval.workspace = true` (no cycle: dev-dep only) | PASS |
| Producers upstream (DAG direction) | producer:β1 (runtime table), producer:β2 (static table) — both wired as real dependencies of β3 | PASS |
| Known-honest divergence identified up front (anti-false-premise) | Int/Int div: runtime widens Int→Real on non-divisible (grep:`reify-expr/src/lib.rs:4640-4648`); ledger entry required by PRD decision 4 | PASS |

## γ — cross-registry drift test (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| Compiler-side families are importable (pub consts) | grep:`crates/reify-compiler/src/units.rs:21` (GEOMETRY_FUNCTION_NAMES, pub), `:212` (GEOMETRY_TOPOLOGY_SELECTOR_NAMES, pub), `:1258` (geometry_query_result_type) | PASS |
| Eval-side consumer maps exist & are reachable in-crate | grep:`crates/reify-eval/src/geometry_ops.rs:3856` (is_geometry_query_call), `:3906` (is_geometry_consumer_call, pub(crate)), `:5314-5352` (TopologySelectorHelper name map), `:6389` (try_eval_topology_selector) | PASS |
| Crate direction supports the test (reify-eval → reify-compiler) | grep:`crates/reify-eval/Cargo.toml:15` `reify-compiler.workspace = true` | PASS |
| The drift class is real (documented in-list gap to ledger) | grep:`crates/reify-eval/src/geometry_ops.rs:3933-3941` (`angle` gap comment citing 4952 α); prose-only "Maintenance contract" `:3895-3901` | PASS |

## δ — Wave-3 registry bookmark (stays deferred; no signal to bind)

| Capability | Evidence | Verdict |
|---|---|---|
| Bookmark pattern sanctioned | `preferences_bookmark_task_pattern` (planning_mode=True, excluded from commit_planning) | PASS |
| First-migration-validator named | producer:γ (real dependency edge γ → δ) | PASS |

## ε1 — MemberAccess exhaustive reshape (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| The if-chain + poison tail exist as described | grep:`crates/reify-compiler/src/expr.rs:3403` (MemberAccess arm), `:4690-4706` (poison tail, "member access not yet supported") | PASS |
| Exhaustive-match pattern proven in-crate (no `_`, named leaves) | grep:`crates/reify-compiler/src/type_compat.rs:429-431` ("intentionally exhaustive (no `_` wildcard)"), `type_resolution.rs:2009-2030` | PASS |
| Precedence comments to preserve are enumerable | grep:`expr.rs:3782`, `:4313`, `:4347` (documented "before X because Y" sites) | PASS |
| Behavior-preservation signal producible | examples corpus + compiler test suite exist and run in CI today (`scripts/verify.sh` pipeline) | PASS |

## ε2 — Type discriminants + completeness canary (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| strum available without new third-party dep | grep:workspace `Cargo.toml:77` `strum = { version = "0.26", features = ["derive"] }`; NOT yet in reify-core (Cargo.toml grep empty — task adds the workspace-dep line) | PASS (substrate addition is one Cargo.toml line, named in the task) |
| Discriminants-for-completeness precedent wired on main | grep:`crates/reify-ir/src/geometry.rs:569` (`strum_discriminants` + EnumIter/EnumCount on GeometryOp), tests `:9008`, `:10422` | PASS |
| Type enum anchor (39 variants, no completeness guard today) | grep:`crates/reify-core/src/ty.rs:111` (`pub enum Type`), variant count 39 (awk, 2026-07-06) | PASS |
| Producer upstream | producer:ε1 (handler classes the canary maps onto) | PASS |

## ζ — auto_free dedup (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| Exactly three copies, differences enumerated | grep:`crates/reify-compiler/src/entity.rs:2024-2064` (Site 1, priv-aware, real solver_hints), `:3264-3300` (Site 2, Public, `Vec::new()` hints), `crates/reify-compiler/src/guards.rs:413-446` (guarded, `compile_expr_guarded`) | PASS |
| Extraction substrate (task 1333) landed | `extract_auto_free` referenced at all three sites (grep above); task 1333 status done | PASS |
| Behavior signal producible | auto(free) end-to-end tests exist: `crates/reify-compiler/tests/boundary2_producer.rs` auto-free coverage (task 1335) | PASS |

## λ — integration gate (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| Every §8 boundary row's capability is delivered by λ's dependency closure (α, β3→{β1,β2}, γ, ε2→ε1, ζ) — no row requires a downstream task | dependency edges wired in this batch (DAG-direction check) | PASS |
| Probe fixtures committed & re-runnable | `tests/prd-gate/fixtures/compiler_type_hygiene_*.ri` + `tests/prd-gate/compiler-type-hygiene-probe-set.json` (this commit) | PASS |

**FAIL bindings: none.** The one substrate addition (strum → reify-core) is a single workspace-dep line inside ε2 itself, not an unfiled prerequisite.
