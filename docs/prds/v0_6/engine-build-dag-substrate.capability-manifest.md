# Capability Manifest — engine-build-dag-substrate

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/engine-build-dag-substrate.md`. Evidence verified at `HEAD b0077500f5` (re-locate at implementation time). **No numeric bounds, no result-field sampling, no novel `.ri` syntax** → floor / field-population / grammar-fixture checks N/A throughout.

## α — `deps.rs` GeomRef-resolution edge-extraction pass (intermediate; substrate it builds ON)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `ReverseDependencyIndex.realization_index` + `add_realization` + `realization_dependents_of` | wired-on-main | `grep:crates/reify-eval/src/deps.rs:69/104/115` (struct + both methods) | **PASS** |
| `realization_reads` field + `geometry_cell_realization_reads` writer | wired-on-main | `grep:crates/reify-eval/src/deps.rs:32` (field) / `:279` (writer); currently empty at `:38/:344` — α *populates* it | **PASS** (this task is the producer that fills it) |
| `is_geometry_query_call` (constraint-detection) | wired-on-main | `grep:crates/reify-eval/src/geometry_ops.rs:1795` | **PASS** |
| `deps.rs:394` Boolean-operand drop | bug-to-fix exists | design ledger C4 (confirmed) | **PASS** (α fixes it) |

## β — `assert_dag_complete` integration gate (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| Edge graph (all three new edge kinds) | capability→producer | `producer:task-α` (this batch) — **upstream** of β | **PASS** |
| Legacy execution order `L(B)` as oracle | wired-on-main | legacy per-template loop in `crates/reify-eval/src/engine_build.rs` (the default build path) | **PASS** |
| Topo-sort helper | wired-on-main | `dirty.rs` `topological_sort`/`compute_levels` (design ledger C2) | **PASS** |
| Existing corpus to run the assert across | wired-on-main | `crates/reify-eval/tests/` + `tests/golden` (present) | **PASS** |
| DAG-direction | anti-inversion | α is **upstream** of β ✓ | **PASS** |

## γ — `cell_eval_ctx` determinacy unification + warm RED regression (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `EvalContext::with_meta` / `with_determinacy` / `with_runtime_diagnostics` | wired-on-main | `grep:crates/reify-expr/src/lib.rs:91/97/108` | **PASS** |
| `DeterminacyPredicate` cell kind (the warm-Undef site) | wired-on-main | design ledger C11 (the five bare sites: `engine_eval.rs:3252/:3068`, `concurrent.rs:481`, `engine_edit.rs:1053/:2487`) — re-locate at impl | **PASS** (bug-to-fix; symbols confirmed present) |
| `snapshot_values` plumbed into `concurrent.rs` | capability→producer | this task threads it (the producer is γ itself) | **PASS** |
| Task 4317 parity baseline (eval_cached §8.2 locks) | capability→producer + DAG-direction | `producer:task-4317` — **upstream** (γ gated on its merge; D3) | **PASS** (gated) |

**No FAIL bindings.** All capabilities are either wired-on-main or delivered by an upstream producer. Queue-blocking conditions: none, provided task 4317 is merged before γ activates (enforced as a real `add_dependency` edge at decompose time).
