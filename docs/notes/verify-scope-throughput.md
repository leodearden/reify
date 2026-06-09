# Per-Task Verify Throughput: `--scope branch` vs `--scope all`

Recorded 2026-06-09 as the T6 evidence artifact for
`docs/prds/verify-scope-contract.md §7`.

## Methodology

Plan-step counts are derived from the canonical oracle:

```bash
verify.sh all --profile debug --scope {all,branch} --include-infra --print-plan
```

run inside an isolated throwaway git repo (branch fixture) containing only
the scripts `verify.sh` needs — identical to the technique in
`tests/infra/test_verify_scope.sh`.  For the crate shapes, the
`REIFY_AFFECTED_CRATES_OVERRIDE` knob pins a deterministic representative
affected set in place of the cargo-metadata reverse-closure (which requires a
real workspace).

**Count** = non-comment lines in `--print-plan` output
(`grep -cE '^[^#]'`).

Measurements were taken 2026-06-09 on a 32-core x86_64 host with warm
sccache (Rust compilation artifacts already cached).  Timings are
machine- and load-dependent.

Real-run narrowing uses the actual `cargo metadata` reverse-closure, which
may include more crates than the representative override used here.  The
plan-step counts reflect the hermetic fixture counts; wall-clock timings below
reflect a real run on this host.

## Plan-Step Counts

| Shape | Changed file | Override | scope=all | scope=branch |
|-------|-------------|---------|-----------|--------------|
| (a) docs-only | `docs/note.md` | — | 14 | 0 |
| (b) reify-doc (non-OCCT) | `crates/reify-doc/src/lib.rs` | `reify-doc` | 14 | 13 |
| (c) reify-eval (OCCT) | `crates/reify-eval/src/lib.rs` | `reify-eval` | 14 | 13 |
| (d) gui-only | `gui/src/editor/foo.ts` | — | 14 | 3 |

## Heavy-Work Narrowed Markers

`scope=all` always produces: `cargo clippy --workspace`, the full OCCT gated
pass (4 crates: `reify-kernel-occt reify-eval reify-cli reify-config`), and
`cargo nextest run --workspace --exclude <occt-crates>`.

Under `scope=branch` + narrowing:

| Shape | OCCT gated pass | cargo flags | cargo present |
|-------|----------------|-------------|---------------|
| (a) docs-only | absent | — | no (empty plan) |
| (b) reify-doc (non-OCCT) | absent | `-p reify-doc` (not `--workspace`) | yes |
| (c) reify-eval (OCCT) | present, narrowed to `-p reify-eval` | `-p reify-eval` (not `--workspace`) | yes |
| (d) gui-only | absent | — | no (GUI npm only) |

For shape (b), the one step that differs from scope=all is the removal of the
OCCT gated pass (replaced with nothing — `reify-doc` is non-OCCT) combined
with narrowing `--workspace` to `-p reify-doc`.

For shape (c), the one step that differs is the OCCT gated pass being narrowed
from 4 crates to 1 (`-p reify-eval`) — eliminating `reify-kernel-occt`,
`reify-cli`, and `reify-config` from the gated run.

For shape (d), 11 of the 14 scope=all steps are Rust/OCCT; branch scope drops
all of them and retains only the 3 GUI npm steps.

## Wall-Clock Measurements

### Shape (a): docs-only — scope=branch

Measured on a 32-core x86_64 host with warm sccache, real `verify.sh` run
(not `--print-plan`) on a branch fixture where only `docs/note.md` is changed:

```
real  0.233 s
```

The branch scope detects that only docs were changed, produces an empty plan
(0 steps), and exits immediately.  The equivalent scope=all run would proceed
to execute all 14 steps including `cargo clippy --workspace` (≈ 20 s warm),
the OCCT gated pass (≈ 30+ min warm for the 4-crate suite), and
`cargo nextest run --workspace --exclude ...` (≈ 10+ min warm).

### Plan-generation overhead (scope=all, --print-plan)

```
real  0.188 s
```

Scripting overhead only — plan is printed but no steps execute.

## Delta as Evidence

- **docs-only branch:** saves all 14 steps. Verify exits in < 0.3 s.
- **non-OCCT crate branch (reify-doc):** skips the OCCT gated pass entirely
  (the single heaviest step); narrows `--workspace` clippy and nextest to
  `-p reify-doc`.  13 vs 14 plan steps.
- **OCCT-touching crate branch (reify-eval):** gated pass narrowed from
  4 crates to 1; clippy and nextest narrowed to `-p reify-eval`.
  13 vs 14 plan steps.
- **gui-only branch:** skips all 11 Rust/OCCT steps; runs only the 3 GUI npm
  steps.  3 vs 14 plan steps.

No numeric improvement threshold is asserted here.  The step counts and the
absent/narrowed heavy-work markers are the evidence.

## Orchestrator Context

The orchestrator runs narrower per-task sub-actions (not `all`):

```bash
verify.sh test  --scope branch --include-infra   # nextest + infra tests only
verify.sh lint  --scope branch --include-infra   # clippy + typecheck only
```

Both inherit the same narrowing logic: a docs-only branch skips both entirely;
a non-OCCT crate branch narrows each to `-p <affected-crates>`.

## Regenerating Plan-Step Counts

When `verify.sh`'s plan changes, re-derive the counts using the same oracle
the drift guard in `tests/infra/test_verify_throughput.sh` uses.  Run each
pair inside a branch fixture (branch off main with only the shape file
committed) to drive the branch-scope diff correctly:

```bash
# Shape (a) docs-only (run on a branch with docs/note.md committed)
bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan | grep -cE '^[^#]' || true
bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan | grep -cE '^[^#]' || true

# Shape (b) reify-doc (branch with crates/reify-doc/src/lib.rs committed)
REIFY_AFFECTED_CRATES_OVERRIDE="reify-doc" bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan | grep -cE '^[^#]' || true
REIFY_AFFECTED_CRATES_OVERRIDE="reify-doc" bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan | grep -cE '^[^#]' || true

# Shape (c) reify-eval (branch with crates/reify-eval/src/lib.rs committed)
REIFY_AFFECTED_CRATES_OVERRIDE="reify-eval" bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan | grep -cE '^[^#]' || true
REIFY_AFFECTED_CRATES_OVERRIDE="reify-eval" bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan | grep -cE '^[^#]' || true

# Shape (d) gui-only (branch with gui/src/editor/foo.ts committed)
bash scripts/verify.sh all --profile debug --scope all    --include-infra --print-plan | grep -cE '^[^#]' || true
bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan | grep -cE '^[^#]' || true
```

After regenerating, update the sentinel count block (added in S4) and re-run
`tests/infra/test_verify_throughput.sh` to confirm the drift guard passes.
