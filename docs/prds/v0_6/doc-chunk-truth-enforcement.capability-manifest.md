# Capability manifest — doc-chunk-truth-enforcement

PRD: `docs/prds/v0_6/doc-chunk-truth-enforcement.md` · Authored at decompose time 2026-07-25, batch 5477–5480 (pre-stamped; single-landing SOP). Substrate probes run against main `92551d18d6`.

Machine-readable twin: `doc-chunk-truth-enforcement.capability-manifest.yaml`.

## α — #5477 Consolidate doc-chunk smoke tests into `harness_doc_chunks.rs`

| Capability | Evidence binding | Verdict |
|---|---|---|
| C1 harness contract exists and sanctions `harness_<subsystem>.rs` by name | `tests/infra/test_harness_kloc_cap.sh` header (C1 naming + module-path-preservation guarantee, PRD merge-gate-compile-cost.md §5); four in-crate precedents on main: `harness_auto_binding.rs`, `harness_langcore.rs`, `harness_patterns.rs`, `harness_traits.rs` in `crates/reify-compiler/tests/` (ls-verified 2026-07-25) | PASS |
| Absorption target 1 wired on main | `crates/reify-compiler/tests/geometry_chunk_smoke.rs` landed via #5364 merge `b81331913d`; grandfather entry at `tests/infra/harness-layout-baseline.manifest:126` (grep-verified) — α removes it (ratchet shrink) | PASS |
| Absorption target 2 upstream | producer: **#5347** (in-progress 2026-07-25; adds `stdlib_chunk_geometry_ops_smoke.rs` + fixture per its metadata.files) — hard `add_dependency` edge 5477→5347 wired | PASS (producer-upstream, dep wired) |
| Merge gate runs the harness | consumer wired on main: the verify pipeline runs reify-compiler integration tests (`examples_smoke.rs` precedent) + `test_harness_kloc_cap.sh` (run-all classified `pool`) on every merge_request | PASS |

## β — #5479 Fence gate + one-time retag sweep

| Capability | Evidence binding | Verdict |
|---|---|---|
| Compile helper exists | `reify_test_support::compile_source_with_stdlib` — imported and used by the landed `geometry_chunk_smoke.rs` (merged through the full gate) | PASS |
| Path-based chunk access from reify-compiler | `examples_smoke.rs` `concat!(env!("CARGO_MANIFEST_DIR"), …)` idiom on main; reify-mcp does not depend on reify-compiler (the #5347/#5364 placement conclusion) | PASS |
| Fence census premise | 2026-07-25 census: 116 fences across 17 `chunks/*.md` — 111 untagged bare ` ``` `, 5 tagged ` ```reify ` (traits.md; 3 fragment-shaped). Retag scope is real and bounded | PASS |
| Reachability-check substrate | `crates/reify-mcp/src/tools/language_chunks.rs` holds one `include_str!` per chunk + the `TOPICS` list (read 2026-07-25) — text-scannable | PASS |
| RED-first signal producible | phantom `rotate(geo, axis, angle)` is compiler-rejected ("rotate() expects 2 or 5 arguments", per #5347's probe evidence) — planting it in a `reify` fence must fail the gate | PASS |
| In-flight content not trapped | hard edges 5479→{5477, #5389} — retag of geometry.md waits for #5389's content (the #5364 trap class) | PASS |

## γ — #5478 PDOCCOVER detector + brittle-parse guard

| Capability | Evidence binding | Verdict |
|---|---|---|
| Registries are textually extractable | 8 `pub const *_NAMES: &[&str]` literals in `crates/reify-compiler/src/units.rs` (:21/:102/:130/:215/:551/:674/:803/:873, grep-verified 2026-07-25); extraction exercised live by this session's census script | PASS |
| Omission premise is real | census 2026-07-25: **83/133 names undocumented**, incl. the full clearance-oracle family (`interferes`/`interferes_with`/`min_clearance` + `intersects`), all 11 affine constructors, all 6 topology-invariant helpers | PASS |
| Non-zero-exit signal producible under any sibling ordering | the demonstration does not depend on the clearance names staying undocumented — 79 further names remain after #5389; γ deliberately has no content-task deps | PASS |
| Audit architecture accommodates a new pattern | one-module-per-pattern layout (`ptodo.rs`, `pdead_dead_code.rs`, … in `crates/reify-audit/src/`); `--pattern` enum parse/usage/`pattern_selects` in `src/bin/reify-audit.rs` (:249–273, :419–444); PDOCCOVER stays out of the jcodemunch-required predicate (pure text-scan, PTODO precedent) | PASS |
| Same-day sibling seam | `placeholder-type-eradication-ratchet` leaf δ (PTYPE) adds an arm to the same enum — additive; second lander rebases; first-filed owns its own pattern only (sidecar-collision check run 2026-07-25) | PASS |

## δ — #5480 PDOCCOVER hard gate + seeded baseline

| Capability | Evidence binding | Verdict |
|---|---|---|
| Gate-wiring exemplar on main | `tests/infra/test_reify_audit_ptodo.sh` (classified `intra-run-serial`) + `scripts/reify-audit-freshness.sh` + `test_reify_audit_ptodo_orphan_hardgate.sh` — the exact PTODO shape to mirror | PASS |
| Drift-guard registration path | `tests/infra/run-all-classification.manifest` row lands same-diff (overlay rule); wallclock/nextest registrations N/A (no elapsed-time assertions; shell test) | PASS |
| Baseline mechanism precedent | committed `crates/reify-audit/ptodo-baseline.txt` + `ptodo-baseline-gen` single-derivation regenerator (§6.6) — the shape δ mirrors | PASS |
| Seeding cannot trap in-flight content | hard edges 5480→{5478, #5347, #5389}; baseline seeded from post-content truth, stale-entry FAIL active from day one with an honest ledger | PASS |
| G6 numeric premise | "83/133" measured (not guessed) by live extraction 2026-07-25; the seeded baseline size is whatever remains post-content — no asserted count is frozen into a RED test | PASS |

## Rejection-mechanism note (G6 branch 4)

Both PRD gates ARE the rejection mechanisms being introduced; each leaf's signal includes its own RED-first demonstration (plant-phantom → RED for β; delete-mention/add-name → RED for δ), so no signal asserts a rejection backed by an absent mechanism.
