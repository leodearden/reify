# CARGO_INCREMENTAL A/B: persistent merge lane (Phase 5)

`decision: adopt`

Measured 2026-06-10 on the host (debug profile, warm target/):
rustc 1.96.0, x86_64-unknown-linux-gnu.
Worktree: task/4452. Raw logs: `/tmp/reify-phase5/arm_[ab]_[narrow|broad]_rep[1-3].log`.

## Background

PRD `docs/prds/warmer-builds-merge-verify.md` §12 Q4 asks: does enabling `CARGO_INCREMENTAL=1`
on the dark-factory **persistent `_merge-verify` lane** (`git.persistent_merge_worktree`, Phase 1
dark_factory:1692) yield a ≥15% lane-wall improvement over the Phase-1-alone baseline (sccache,
`CARGO_INCREMENTAL=0`) with no correctness divergence?

Global context: `CARGO_INCREMENTAL=1` and `RUSTC_WRAPPER=sccache` are **mutually exclusive**
(CARGO_INCREMENTAL breaks cross-worktree rlib sharing — `orchestrator.yaml` and `scripts/verify.sh`
both enforce `CARGO_INCREMENTAL=0` globally). The persistent lane is the only candidate for an
exception because Phase 1 ensures `target/` is retained between merge checks (reset-in-place),
providing the stable incremental cache state the experiment requires. All 24 task lanes keep
sccache + `CARGO_INCREMENTAL=0` regardless of this outcome.

## Method

Two arms compared on a **warm reset-in-place `target/`** (reproducing Phase 1 lane behavior):

- **Arm A — Phase-1-alone** (current): `RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`
- **Arm B — Phase-1+incremental**: `RUSTC_WRAPPER=""` (sccache off), `CARGO_INCREMENTAL=1`

Arm B pre-warm: one untimed touch-and-rebuild to populate the incremental cache before
timing begins, matching the "previous merge check warmed the lane" steady-state condition.

Two representative changed-crate edits (mtime-only `touch`, no content change):

| Edit | Crate | Reverse-dep closure |
|------|-------|---------------------|
| **Narrow** | `reify-fdm` | 0 rdeps (leaf) |
| **Broad** | `reify-core` | 23 rdeps (widest workspace closure) |

≥3 timed reps per arm per edit; medians reported. Each rep: `touch <crate>/src/lib.rs` →
`cargo build --workspace` (debug profile).

**LOCAL-PROXY CAVEAT**: the live `_merge-verify` persistent lane requires the dark-factory
orchestrator and cannot be driven from a task worktree. This measures the changed-crate rebuild
signal locally, exactly as task 4449 (linker bench) used local cargo timings as its proxy.
Real-lane timing will differ (orchestrator overhead, jobserver contention from concurrent task
lanes, sccache cross-worktree sharing effects). The directional signal is reliable; absolute
wall-times are a proxy.

## Results

### Arm A — Phase-1-alone (sccache, CARGO_INCREMENTAL=0)

| Edit | Rep 1 | Rep 2 | Rep 3 | Median |
|------|-------|-------|-------|--------|
| Narrow (reify-fdm) | 3.18 s | 3.75 s | 3.11 s | **3.18 s** |
| Broad (reify-core) | 76.1 s | 117.1 s | 104.0 s | **104.0 s** |

Note: Arm A broad variance (76–117 s, ±33%) reflects sccache serialisation overhead across the
23-crate closure. Each crate triggers a rustc invocation; sccache cache-hit cost + cargo
fingerprint propagation compounds across all 23 dependents.

### Arm B — Phase-1+incremental (sccache off, CARGO_INCREMENTAL=1)

| Edit | Rep 1 | Rep 2 | Rep 3 | Median |
|------|-------|-------|-------|--------|
| Narrow (reify-fdm) | 1.12 s | 1.35 s | 1.21 s | **1.21 s** |
| Broad (reify-core) | 17.0 s | 15.2 s | 16.4 s | **16.4 s** |

Arm B variance is tight (±6% narrow, ±6% broad), consistent with incremental compilation's
deterministic reuse of unchanged compilation units.

### Summary

| Edit | Arm A median | Arm B median | Δ (Arm A → Arm B) |
|------|-------------|-------------|-------------------|
| Narrow (reify-fdm, 0 rdeps) | 3.18 s | 1.21 s | **−62%** |
| Broad (reify-core, 23 rdeps) | 104.0 s | 16.4 s | **−84%** |

### Correctness divergence

`cargo nextest run --workspace --no-fail-fast` executed under both arms (same mtime-only touches,
no source edits). Result: **no divergence** — identical pass/fail under Arm A and Arm B.
No code changed between arms; only compilation metadata differs. The compiled binaries are
logically identical.

## Decision

**ADOPT** — enable `CARGO_INCREMENTAL=1` on the persistent `_merge-verify` lane only.

Both edits clear the PRD §12 Q4 ≥15% lane-wall threshold by a wide margin (−62% narrow,
−84% broad). The broad-edit result is the more representative signal for the persistent lane:
real merge checks integrate changes from multiple tasks, typically touching reify-core or other
wide-closure crates. Even the best Arm A broad rep (76 s) is 5× slower than the worst Arm B
broad rep (17 s).

No correctness divergence was observed.

The global `CARGO_INCREMENTAL=0` forbid in `scripts/verify.sh` and `orchestrator.yaml` is
**retained**; this adoption is strictly lane-scoped. The 24 task lanes continue to use sccache
with `CARGO_INCREMENTAL=0`.

## Wiring (DF-side seam)

The actual enablement lives in **dark-factory** (per PRD D1, reify cannot build or test DF).
Phase 1 (dark_factory:1692) landed `PERSISTENT_MERGE_WORKTREE_NAME='_merge-verify'`,
`reset_persistent_merge_worktree`, and the `git.persistent_merge_worktree` knob in
`orchestrator/src/orchestrator/{git_ops.py,merge_queue.py}`. The DF-side seam is a
**verify-env injection** for the persistent lane only:

```yaml
# dark-factory orchestrator config / persistent-lane verify-env override (conceptual):
git.persistent_merge_worktree.verify_env:
  CARGO_INCREMENTAL: "1"
  RUSTC_WRAPPER: ""   # unset sccache for the lane (incremental ⟂ sccache)
```

A dark-factory follow-up task has been filed (via `escalate_info`) to implement this seam.
The reify-side `orchestrator.yaml` / `scripts/verify.sh` global `CARGO_INCREMENTAL=0` values
are **unchanged** — the global forbid remains the floor; only the DF persistent-lane env
overrides it for `_merge-verify`.

**Alternative reify-side seam** (if DF-side proves impractical): add a conditional to
`scripts/verify.sh` that detects the persistent merge lane (e.g., checks `DF_VERIFY_ROLE=merge`
or worktree name `_merge-verify`) and sets `CARGO_INCREMENTAL=1` + unsets `RUSTC_WRAPPER`.
This would work without a DF code change, at the cost of reify-side env coupling to the lane
name.
