# Capability manifest — dependency-invalidation-stratum

PRD: `docs/prds/v0_6/dependency-invalidation-stratum.md`. Machine-readable twin: `dependency-invalidation-stratum.capability-manifest.yaml` (validated against dark-factory `shared.capability_manifest.CapabilityManifestDoc`, 12 tasks / 32 capabilities). Task ids are stamped by `commit_planning` at decompose.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). Cite-by-symbol; re-locate lines at implementation time.

Verdicts: PASS = binding held at authoring; OPEN = the decision is this leaf's own work product (here: a lead ruling, §11 of the PRD). No FAIL rows.

| Leaf | Capability | Evidence (authoring) | Verdict | delivered_check |
|---|---|---|---|---|
| α | crate-dag-gate-precedent-exists | `crate_dag_assertion.rs::workspace_dag_invariant_via_assert_script` drives `scripts/assert-crate-dag.sh` from the workspace root | PASS | grep `assert-module-scc\.py` present in `module_scc_ratchet.rs` |
| α | stdlib-python-can-read-workspace-members | `/usr/bin/python3` 3.12.3 (tomllib); root `[workspace] members` lists 37 packages | PASS | grep `import tomllib` present in `scripts/assert-module-scc.py` |
| α | rule-reproduces-review-measurement | reproduction: 84 modules, one 24-module SCC (95,192 lines); brace-matched rule gives a 5-module second SCC | PASS | grep `"reify-eval"` present in `scripts/module-scc-baseline.json` |
| α | inv-arch-1-row | no INV-ARCH-* row in `docs/invariants.md` today | PASS | grep `^\| INV-ARCH-1 \|` present |
| β | nine-predicates-exist-at-cited-homes | definitions located in geometry_ops / engine_eval / engine_build | PASS | grep of the nine upward `crate::` paths absent in `crates/reify-eval/src` |
| β | reify-ir-core-only-imports-premise | false for 4 of 9 (reify_compiler / reify_expr / structural_query needed) | OPEN (OQ-1) | — |
| β | structural-query-is-below-the-scc | structural_query has zero `crate::` edges | PASS | — |
| β | residual-after-move | edge-deletion simulation: {engine_admin, engine_build, engine_edit, engine_eval, geometry_ops} + {cache, deps} | PASS | manual (baseline diff in β's commit) |
| β | symbolic-selector-map-single-home | `symbolic_eval_helper_for_name` shared by dispatcher and predicate | PASS | grep `enum SymbolicSelectorCtor` present in `expr_classify.rs` |
| γ | peers-have-no-production-caller | dirty_set 0; check_early_cutoff, compute_input_hash, free derive_output_freshness test-only; topological_order only via dirty_set | PASS | grep `fn (dirty_set\|check_early_cutoff\|compute_input_hash\|topological_order)\b` absent |
| δ | three-walks-and-paired-sites | 3 builders; 5 paired sites + 1 reverse-only rebuild | PASS | grep of the old builders / `struct DependencyMap` absent |
| δ | compute-asymmetry-is-deliberate | reverse has edges #6/#10; forward has no compute loop; #4958: compute nodes dispatched inline | PASS | grep `fn schedulable_traces` present in `deps.rs` |
| δ | explain-undef-is-the-user-surface | reify-cli prints `note: … is undef (because: …)` via `Engine::trace_undef_causes` | PASS | grep `DependencyGraph` present in `undef_tracer.rs` |
| ε | callers-and-bodies | 8 + 1 production callers; duplicated ValueCell BFS; one exhaustive match | PASS | grep `ConePolicy::ThroughRealizations` present in `dirty.rs` |
| ε | old-bodies-gone | post-delivery | PASS | grep `fn compute_dirty_cone(_with_realizations)?\b` absent |
| ζ | walk-has-no-production-caller | callers are tests only; completion site carries `TODO(#5023)` | PASS | grep `propagate_freshness_only\|mod freshness_walk\|mod gating` absent |
| ζ | no-async-completion-path-exists | #5023 / #4958 records: executor deleted by #5065 | PASS | — |
| ζ | runtime-policy-has-own-consumer | reify-runtime `commitment.rs` consumes `OnlyRunOnFinalInputs` itself | PASS | — |
| ζ | inv-eval-6-row-repointed | INV-EVAL-6 enforcement reads "completion transaction + panic boundary" | PASS | grep `INV-EVAL-6.*dirty_cone` present in `docs/invariants.md` |
| η | inline-hash-sites | graph 6, engine_edit 2, + one doc restatement | PASS | grep `id_hash\.combine\(expr_hash\)` absent |
| η | constructor-present | post-delivery | PASS | grep `pub fn new\(\s*id: ValueCellId` present in `graph.rs` |
| η | no-parity-test-today | review measurement: 0 | PASS | manual |
| θ | mirror-and-resolver-exist | private exporter map + `split_once('.')`; deps resolver trio private; semantics agree | PASS | grep `fn sub_operand_resolves` present in `deps.rs` |
| θ | mirror-test-gone | post-delivery | PASS | grep `the_dropped_operand_rule_agrees_with_the_reverse_index` absent |
| ι | keyed-paths-land-in-entity | keyed-sub loop in `from_templates` and compiler keyed access put `sub["key"]` into entity | PASS | — |
| ι | dotted-key-scalar-eval-works-today | probe: `"in.take"` variant of `examples/keyed_vents.ri` evaluates `Manifold.a = 0.005 m` | PASS | — |
| ι | valuecellid-storage-scope | ~2,066 `ValueCellId::new` sites across 16 crates | OPEN (OQ-2) | — |
| ι | grammar-home-present | post-delivery | PASS | grep `pub struct InstancePath` present in `reify-core/src/identity.rs` |
| κ | dead-and-coupled-surface | Engine add/remove_demand 0 callers; GUI reset→add×3→rebuild sequence | PASS | grep of the observed mutators absent in `gui/src-tauri/src/engine.rs` |
| κ | from-roots-present | post-delivery | PASS | grep `pub fn from_roots` present in `demand.rs` |
| λ | differential-harness-exists | `tests/common/differential.rs` included by the harness_cache differential binaries | PASS | manual |
| μ | terminal-status-stamped | overlay closed terminal vocabulary | PASS | grep `Status:\*\* SHIPPED` present in the PRD |
