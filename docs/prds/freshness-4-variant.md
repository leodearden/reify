# Freshness 4-Variant Cache State

## Goal

Implement and wire through the eval pipeline the four-variant `Freshness` enum specified in `reify-implementation-architecture.md` §7.1 (lines 716–728) and §9.2 (lines 880–890): `Final`, `Intermediate { generation: u64 }`, `Pending { last_substantive: ResultRef }`, `Failed { error: ErrorRef }`. Every cache entry must carry a `Freshness`. Downstream nodes consuming a `Failed` input propagate as `Pending` with a diagnostic chain. UI/diagnostic consumers can distinguish the four states for badge display. Computation failures (out-of-bounds index, missing-key map lookup, kernel error, etc.) terminate evaluation locally as `Failed` instead of halting the graph (spec §9.6, lines 1799–1819 and arch §9.3).

## Background

- **Spec §9.6 (lines 1799–1819)** — language-level statement that computation failures are graph-level events, not `Result<T, E>`. Names the four `Freshness` variants.
- **Arch §3.5 (lines 432–436)** — describes "freshness propagation" as a lightweight mode within the dirty-cone walk, distinct from value recomputation. Crucial: a node can have unchanged value but updated freshness, and downstream gating (e.g. "only run on final inputs") needs the metadata even when no value changed.
- **Arch §4.3 (lines 524–532)** — `NodeCache` struct shows `freshness: Freshness` as a field.
- **Arch §7.1 (lines 716–728)** — canonical 4-variant definition. States that all four variants live in the same cache infrastructure; content-hash keying, warm starting, dependency traces, and early cutoff all work on non-`Final` results without modification.
- **Arch §7.2 (lines 730–749)** — propagation rule: a node's output is `Intermediate` if any input is `Intermediate` or the node itself is still refining. Eager eval with cost-aware gating; `Pending` retains last substantive result for UI but does NOT trigger downstream re-evaluation.
- **Arch §9.1–9.3 (lines 868–897)** — `Failed` is terminal for that evaluation. Downstream consumers become `Pending` with a diagnostic chain. Constraint violations are NOT `Failed` — they produce a cached `violated` result and continue (priority-reduced). This distinction matters: `Failed` is for computation breakage (panic, kernel error, out-of-bounds, missing key); `violated` is a constraint outcome.
- **Today's state:** the existing `EvalState` / cache plumbing (memory hint 7ac9d86f) is decoupled enough to receive a freshness field, but freshness is not yet propagated through the recursive evaluator. The diagnostic infrastructure (#2253 typed `DiagnosticCode` follow-up) is the natural carrier for the `error: ErrorRef` payload of `Failed`.
- **UI surface:** badge display in the GUI (Outline / Parameters panels) and in LSP-served diagnostics. Pending vs Failed must be visually distinct because Pending is "we have a stale-but-valid value" and Failed is "this computation broke." Constraint violations remain on the existing constraint-diagnostic channel and are not Freshness-Failed.

## Scope

1. Define a `Freshness` enum in the eval/types layer with the four variants. `ResultRef` and `ErrorRef` are opaque references into the cache and event journal respectively (their concrete representation is an implementation detail of this PRD).
2. Extend the cache entry / `NodeCache` structure to carry `Freshness` per the §4.3 sketch.
3. Wire the propagation rule from arch §7.2 into the evaluator: when computing a node, derive the output's freshness from input freshness and self state.
4. Wire the failure path: when a computation fails (panic caught, kernel error, out-of-bounds index, missing key), mark the node `Failed { error: ErrorRef }` and emit an `EventKind::Failed` realization event. Downstream consumers see the failed input and produce `Pending { last_substantive }` with a diagnostic chain referencing the original failure.
5. Implement freshness-only propagation walks (arch §3.5) so that a value-unchanged-but-freshness-changed transition propagates downstream metadata without value recomputation.
6. UI plumbing: surface `Freshness` to the GUI and LSP layers so panels can render distinct badges for `Final` (no badge), `Intermediate` (spinner / progress), `Pending` (warning), `Failed` (error). Constraint-violation rendering stays on the existing channel.
7. Tests: unit tests at the eval layer for each freshness transition; integration tests for chains of nodes with mixed freshness inputs; failure-injection tests for `Failed` propagation; tests pinning that constraint violations do NOT trigger `Failed`.

## Out of scope

- The full event-journal infrastructure beyond what's needed for `EventKind::Failed` emission (other event kinds are tracked separately).
- Persistent / on-disk freshness across runs.
- The cost-aware gating policy heuristic from §7.2 ("when idle local resources are available") — implement the mechanism (Pending as gate), not the policy tuning.
- Per-node policy overrides ("only run on final inputs", "always cancel when stale") — these are part of the node-trait composition PRD; this PRD provides the freshness machinery they consume.
- Determinacy stack traces (§8.4) — backward walk over `undef` is a separate concern from `Failed` chain reporting.
- LSP server protocol changes — pass freshness through existing diagnostic / hover surfaces only.

## Acceptance criteria

- `Freshness` enum exists in `reify-types` (or `reify-eval` if it lives closer to the cache), with the four variants matching the spec exactly. Public type, doc-commented, citing `reify-language-spec.md` §9.6 and `reify-implementation-architecture.md` §7.1 with line numbers.
- The `NodeCache` (or equivalent) carries a `freshness` field. Reads and writes go through a single API.
- The evaluator computes output freshness using the §7.2 rule: `if self.still_refining { Intermediate } else if any input != Final { Intermediate } else { Final }`. Failure path emits `Failed`. Downstream of `Failed` emits `Pending { last_substantive }` with a chain.
- Freshness-only propagation walks the dirty cone without recomputing values when input freshness changes but input values do not.
- A failure-injection test asserts: forced panic in a leaf node → leaf `Failed` → mid node `Pending` with chain pointing to leaf → realization event `EventKind::Failed` emitted exactly once for the leaf.
- A constraint-violation test asserts: violation produces a cached `violated` constraint result, NOT `Failed`. Downstream nodes remain `Final`/`Intermediate` per §9.3.
- GUI panels render four distinct badge states. (Smoke-test: open `examples/m5_purpose.ri` or any file with intermediate / failed nodes and confirm by snapshot or inspection.)
- `cargo test --workspace` green; new tests included.

## Task breakdown (queueing aim: 5–7 tasks)

1. **Define the `Freshness` enum and `ResultRef`/`ErrorRef` opaque types** in the appropriate crate (likely `reify-types`). Add doc comments citing spec/arch with line numbers. Land as a no-op refactor — type exists, nothing yet uses it.
2. **Extend `NodeCache` / cache entry to carry `freshness`**. Default to `Final` everywhere on read; existing tests must stay green. Provide a single API surface for reading/writing freshness.
3. **Wire the propagation rule** in the recursive evaluator (`evaluate(snapshot, node_id)` from arch §3.1 / §7.2). Output freshness is derived from input freshnesses and the node's own `still_refining` flag. Add unit tests for each row of the §7.2 truth table over a small synthetic graph.
4. **Implement the `Failed` failure path**: catch evaluation failures (panic boundary, explicit error returns from kernel ops, out-of-bounds index, missing-key map lookup) and produce `Failed { error: ErrorRef }`. Emit an `EventKind::Failed` realization event. Add failure-injection tests.
5. **Implement `Pending` propagation downstream of `Failed`** with a diagnostic chain referencing the original failure node. Pending must NOT trigger downstream re-evaluation (per §7.2 — "naturally quiets the downstream subtree").
6. **Implement freshness-only propagation walks** (arch §3.5) for the value-unchanged-but-freshness-changed case. Cover the case where an upstream node transitions Intermediate→Final without value change, and downstream freshness must update without value recompute.
7. **GUI and LSP surfacing**: thread `Freshness` through diagnostic / outline / parameters panels so the UI shows distinct badges for the four variants. Confirm constraint-violation rendering uses the existing `violated` result channel, not the `Failed` channel.

## Dependencies

- Task 4 (`Failed` failure path) depends on #2253 (typed `DiagnosticCode` constraint-domain variants) — `ErrorRef` should reference structured codes once that lands. If #2253 has merged, link the constraint-domain variants in. If not, use a temporary `code: None` placeholder and file a follow-up to migrate after #2253.
