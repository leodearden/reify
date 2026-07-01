# Capability manifest — reify-eval-fea-decomposition

Mechanizes G3+G6 for `docs/prds/reify-eval-fea-decomposition.md`. One block per leaf (D, S); intermediates (M, A, B, P) listed with their unlock target. Any FAIL binding blocks the batch.

**Substrate note.** This PRD introduces **no novel `.ri` syntax and no numeric FEA-accuracy claim** — the grammar gate (`tree-sitter parse`) and the G6 numeric-floor branch are **N/A**. Substrate here is Rust crate structure + bash selection scripts; every assumed capability was verified by **reading the actual source** (cited below) rather than the `.ri` `prd-decompose-verify` workflow, which has no premise to probe for an infra PRD. Branches that DO apply: G6 branch-3 (end-to-end capability → dependency set) and branch-4 (negative/selection assertion → mechanism observed to fire).

---

## Intermediate: M — measure FEA test CPU by category
- **Unlocks:** A, P. No user-observable-signal requirement (intermediate); its output is the measurements doc + go/no-go.
- Capability: `cargo nextest`/`cargo test` per-test timing over `reify-eval` → **PASS** (`.config/nextest.toml` + nextest already in the verify pipeline; `scripts/verify.sh` runs it today).
- No source-file lock; reads/runs tests only.

## Intermediate: A — extract `reify-compute-contract`
- **Unlocks:** B.
- Capability→producer (types exist, OCCT-free): `ComputeFn/ComputeOutcome/StructuredComputeDetail/DispatchError` at `grep:crates/reify-eval/src/engine_compute.rs:34-131`; `RealizationReadHandle/RealizedContent` at `:145-242`; `CancellationHandle` at `grep:crates/reify-eval/src/graph.rs:128`; `ElasticResult/ShellChannels` at `grep:crates/reify-eval/src/persistent_cache.rs:601/:572`. All transitively OCCT-free (deps: `reify-ir`, `reify-core`, `reify-solver-elastic` — none reach `occt-sys`; verified `crates/reify-eval/Cargo.toml:8-53` normal deps). → **PASS** (relocation of existing wired types).
- Re-export compat (INV-2): existing crate-facade pattern. → **PASS**.

## Intermediate: B — create `reify-eval-fea`
- **Unlocks:** D.
- Capability→producer (solvers are pure, wired): trampolines at `grep:crates/reify-eval/src/compute_targets/mod.rs:226 register_compute_fns` (production dispatch entry), solver files `compute_targets/*.rs`, `modal_ops.rs`, `dynamics_ops.rs` — verified **zero** `GeometryKernel`/OCCT refs in production code (agent seam analysis 2026-07-01). → **PASS** (wired-on-main via `register_compute_fns`).
- Field-population (anti-`Undef`): N/A here — B moves existing producers that already write real `Value::Field{source:Sampled}` (the `Undef` risk was closed by tasks 4091/4468/4458/4086, all done); B is a relocation, not a new producer.
- OCCT-free (INV-1): `reify-eval-fea` deps = contract + `reify-solver-elastic/fdm/stdlib` + `faer`, none OCCT. → **PASS** (verified via the same normal-dep audit).

## Intermediate: P — OCCT-free integration crate for engine-driven synthetic-mesh FEA e2e
- **Unlocks:** S.
- Capability→producer (`make_simple_engine`, `register_compute_fns` public): `grep:crates/reify-test-support/src/helpers.rs fn make_simple_engine` + `reify_eval::compute_targets::register_compute_fns` (public). → **PASS** (wired-on-main).
- Substrate premise (FEA e2e are OCCT-independent under kernel-less engine): verified — task 4091 synthetic box-tet fallback; `cli_build_fea.rs` states "FEA solver is pure-Rust and OCCT-independent"; only 3 files (`fea_face_selector_bc_e2e.rs`, `dynamics_body_mass_props.rs`, `rigid_moment_of_inertia_autoderive_smoke.rs`) spawn a real kernel — **left in `reify-eval`** by design. → **PASS**.
- INV-1 for the e2e crate (dev-dep on `reify-eval` does NOT taint): verified via `occt-scope-lib.sh:101-104` — dev-deps non-transitive; `reify-eval`'s normal closure has no `reify-kernel-occt`. → **PASS**.

---

## LEAF: D — scope wiring + selection proof (Track 1)
**Signal:** `tests/infra/test_fea_crate_selection.sh` + `tests/infra/test_occt_gated_scope.sh` green (BT-1, BT-2).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `reify-eval-fea` is OCCT-free ⇒ excluded from `occt_touching_set` | `producer:task-B` upstream (INV-1); `occt-scope-lib.sh:101-104` computes exclusion automatically | **PASS** |
| selection machinery emits an affected-set that excludes `reify-eval-fea` on an OCCT change (branch-3 end-to-end) | `grep:scripts/affected-crates-lib.sh:115-124` (mechanism exists on main); `reify-eval-fea` not in reverse-closure of `reify-kernel-occt` because nothing OCCT-adjacent depends on it (Design X: `reify-eval`→`reify-eval-fea`, not the reverse) | **PASS** |
| `test_occt_gated_scope.sh` self-polices without manual assertion edits | `grep:tests/infra/test_occt_gated_scope.sh` Test 3 auto-derives via `occt_touching_set` | **PASS** |
| `--scope all` plan-count unchanged (no throughput ambush) | throughput sentinel `all=14` in `docs/notes/verify-scope-throughput.md`; crate growth adds at most a `-p` flag, not a plan line | **PASS** |

## LEAF: S — align affected-crates-lib.sh dev-dep transitivity (Track 2)
**Signal:** `tests/infra/test_affected_crates_lib.sh` green with the dev-dep-non-transitive assertion + `test_verify_scope.sh` MG-* green (BT-5, BT-6).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| the OCCT-free e2e crate exists to be excluded | `producer:task-P` upstream | **PASS** |
| the dev-dep-non-transitive reverse-closure (the fix) | delivered by **S itself**; reference implementation exists at `grep:scripts/occt-scope-lib.sh:71-104` (`adj_normal`/`adj_dev` + `normal_closure`-of-direct-dev-deps) — S ports this model to `affected-crates-lib.sh` | **PASS** (fix + test are the same task) |
| **negative/selection assertion** (branch-4): a `reify-kernel-occt` change does NOT select the e2e crate | S authors the scenario in `test_affected_crates_lib.sh` and the assertion **observes** the exclusion fire (not a silent tabulation) | **PASS** (rejection-mechanism = the closure fix, observed by the new test) |
| **C2 preserved** — merge gate never narrows | `grep:scripts/verify.sh` forces `scope=all` on `DF_VERIFY_ROLE=merge`/`MERGE_HEAD` (contract C2, `verify-scope-contract.md`); S touches only the branch-scope closure, not the merge path; BT-6 pins it via `test_verify_scope.sh` MG-* | **PASS** |

**No FAIL bindings.** Batch may queue (subject to the M go/no-go flip-condition holding A/B/D/P/S `deferred` until M clears — see PRD §Pre-conditions).
