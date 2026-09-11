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
| (a) docs-only | `docs/note.md` | — | 20 | 0 |
| (b) reify-doc (non-OCCT) | `crates/reify-doc/src/lib.rs` | `reify-doc` | 20 | 19 |
| (c) reify-eval (OCCT) | `crates/reify-eval/src/lib.rs` | `reify-eval` | 20 | 19 |
| (d) gui-only | `gui/src/editor/foo.ts` | — | 20 | 3 |

Machine-parseable sentinel block for `tests/infra/test_verify_throughput.sh`'s
drift guard.  Update by re-running the regeneration commands in the section
below and replacing the counts; then re-run the test to confirm it passes.

<!-- THROUGHPUT-COUNTS:BEGIN -->
| shape | all | branch |
|-------|-----|--------|
| docs-only  | 20 |  0 |
| reify-doc  | 20 | 19 |
| reify-eval | 20 | 19 |
| gui-only   | 20 |  3 |
<!-- THROUGHPUT-COUNTS:END -->

_Counts bumped 2026-06-25 (task 4839): `add_test_passes()` emitted one
`--no-run` test-binary compile pass per profile BEFORE `@@SEMAPHORE_ACQUIRE@@`
(outside the held slot). With `--profile debug` (one profile) that was +1
non-comment plan line per non-zero plan._

_Counts bumped 2026-06-25 (task 4853): `add_test_passes()` emitted one
`./scripts/verify.sh compile-gate` line AFTER `psi-gate` and BEFORE the
`--no-run` test-binary compile passes — an admit-on-timeout PSI/RSS backstop
for the heavy nextest test-binary link. With `--profile debug` (one profile)
that was +1 non-comment plan line per non-zero `add_test_passes` plan._

_Counts bumped 2026-06-27 (task 4862): `add_test_passes()` reverts task 4839 —
the `--no-run` test-binary compile pass per profile is removed; build+execution
run as one unbroken slot-held block again. The `./scripts/verify.sh compile-gate`
line (task 4853) is KEPT but repositioned as a block-entry load gate before
`@@SEMAPHORE_ACQUIRE@@`. Net change: −1 per non-zero non-docs plan (the removed
--no-run pass). docs-only branch stays 0; gui-only branch stays 3. Regenerated via:_

```bash
bash scripts/verify.sh all --profile debug --scope all --include-infra --print-plan | grep -cE '^[^#]'
```

_Counts bumped 2026-07-06 (task 5125): the reify-audit release pre-build, its
positive-assertion guard, and the `tests/infra/run_all.sh` wholesale-suite
line moved from the per-task `INCLUDE_INFRA` tier to a new
`DF_VERIFY_ROLE=merge` gate (fixes M-way pool contention against the shared
16-slot semaphore — every per-task lane previously ran the full 103-test
suite, and the merge gate, which never passes `--include-infra`, ran none of
it). Net change: −3 non-comment plan lines from every role=task `scope=all`
count, and from `scope=branch` for shapes with `RUN_RUST=1` (reify-doc,
reify-eval). docs-only branch stays 0 and gui-only branch stays 3 (both
`RUN_RUST=0` there, so the removed lines never applied under branch scope).
The oracle command is unchanged (role=task is the default; no `DF_VERIFY_ROLE`
is set) — only the resulting counts shifted. The human-readable "Plan-Step
Counts" table above now equals this machine sentinel exactly (13 for every
non-zero cell): the pre-2026-07-06 +1 table↔sentinel offset was a stale
hand-maintained table artifact, not a semantic difference — both renderings
count the same non-comment `--print-plan` lines from the same role=task oracle,
so they are kept identical from here on._

_Counts bumped 2026-07-09 (task 5093): added `scripts/check-nan-safe-ordering.sh`
(INV-FEA-3 NaN-safe-ordering grep gate) to the `DO_LINT` Rust-infra lint block in
`scripts/verify.sh`, beside `check_event_inventory.sh`. Net change: +1 non-comment
plan line wherever that block runs — every `scope=all` plan, and `scope=branch` for
the `RUN_RUST=1` shapes (reify-doc, reify-eval). docs-only branch stays 0 and
gui-only branch stays 3 (the Rust-infra lint block is not emitted under those branch
scopes). The Plan-Step Counts table and the machine sentinel move 13 → 14 in lockstep._

_Counts bumped 2026-07-24 (task 5300): added
`./scripts/check-harness-baseline-registration.sh --from-git` (the diff-scoped
harness-layout baseline-registration drift gate) to `build_plan` inside the
`RUN_RUST=1` block in `scripts/verify.sh`, immediately after
`check-infra-classification-manifest.sh`. Net change: +1 non-comment plan line
wherever `RUN_RUST=1` — every `scope=all` plan, and `scope=branch` for the
`RUN_RUST=1` shapes (reify-doc, reify-eval). docs-only branch stays 0 and
gui-only branch stays 3 (`RUN_RUST=0` there, so the gate is not emitted). The
machine sentinel moves 15 → 16 for those cells._

_Counts bumped 2026-08-03 (task 5076): added
`scripts/check-compute-trampoline-registration.sh` (the INV-FEA-1
compute-trampoline delegation grep gate) to the `DO_LINT` Rust-infra lint block
in `scripts/verify.sh`, beside `check-nan-safe-ordering.sh`. Net change: +1
non-comment plan line wherever that block runs — every `scope=all` plan, and
`scope=branch` for the `RUN_RUST=1` shapes (reify-doc, reify-eval). docs-only
branch stays 0 and gui-only branch stays 3 (the Rust-infra lint block is not
emitted under those branch scopes). The machine sentinel moves 16 → 17 for
those cells. The human-readable "Plan-Step Counts" table had drifted stale at
14 (it was last updated by task 5093 and missed the 5300 bump); it is
re-synced to 17 here, restoring the table↔sentinel lockstep that task 5125
established as the standing convention._

_Counts bumped again 2026-08-03 (task 5076, second of two): `add_test_passes()`
now emits a gui-feature TEST-EXECUTION pass (`-p reify-gui --features gui`) at
the tail of the profile loop, inside the `@@SEMAPHORE_ACQUIRE@@` /
`@@SEMAPHORE_RELEASE@@` bracket. This is the execution half of the same
INV-FEA-1 gap the lint-side grep gate above closes statically: reify-gui's
`#[cfg(feature = "gui")]` code was reached by NO workspace pass (all run without
`--features gui`) and was only COMPILE-checked, so a change flipping
`engine.rs`'s gui arm to `MorphRegistration::Unavailable` compiled clean and
passed every pass silently. Net change: +1 non-comment plan line wherever
`add_test_passes()` emits — every `scope=all` plan, and `scope=branch` for the
`RUN_RUST=1` shapes (reify-doc, reify-eval). docs-only branch stays 0 and
gui-only branch stays 3 (`RUN_RUST=0` there). The sentinel and the table move
17 → 18 in lockstep for those cells. It is emitted ONCE per plan rather than
per profile — `--features` is a feature axis, not a profile axis — so
`--profile both` does not double it; and it is skipped for
`DF_VERIFY_ROLE=offline`, whose plan runs the heavy `#[ignore]` partition only,
so neither of those shapes sees the +1._

_Counts corrected 2026-08-05 (task 5076, review amendment): the gui-feature
TEST-EXECUTION pass above is no longer emitted unconditionally. It is now
narrowed on the same affected-crate axis every other narrowed pass uses —
emitted unconditionally for `scope=all` (the merge gate never narrows), emitted
whenever the affected-crate closure is unavailable (the `ALL` sentinel, an empty
changed-file list, or a malformed override — fail wide), and otherwise emitted
only when `reify-gui` is in that closure. A `--features gui`
build is a distinct feature-unification of the dependency graph, so it shares
artifacts with no other pass and costs 20m42s cold / ~137s warm on its own; a
`-p reify-doc` branch plan was paying that for code no reify-doc change can
reach, while the SAME plan already narrowed reify-gui's ungated tests away.
Membership is tested against `affected_crates()`'s REVERSE-dependency closure
rather than a hand-listed trigger set, so a change to an indirect dependency
(`reify-syntax`, `reify-ir`, …) cannot fall out of the trigger: measured on this
tree, `crates/reify-eval/src/lib.rs` and `crates/reify-mesh-morph/src/lib.rs`
both yield closures containing `reify-gui`, and `crates/reify-doc/src/lib.rs`
does not. Net change: −1 non-comment plan line for the `scope=branch` cells of
shapes (b) and (c), 18 → 17. **Both** of those cells move even though a real
reify-eval change WOULD pull in reify-gui: `plan_for_shape_narrowed` drives them
through `REIFY_AFFECTED_CRATES_OVERRIDE` with a literal single-crate list
(`reify-eval`), not the real closure, so the fixture's `AFFECTED` is exactly
`reify-eval` and lacks `reify-gui`. Every `scope=all` cell stays 18 (narrowing
is structurally unreachable there), docs-only branch stays 0 and gui-only branch
stays 3 (`RUN_RUST=0`). The table and the sentinel move in lockstep._

_Counts UNCHANGED 2026-08-13 (task 6030): the amendment above described the
gui-feature pass's unconditional arm as `NARROW_ACTIVE=0`, i.e. as the merge
gate. That equivalence was never true — `NARROW_ACTIVE` is a
narrowing-ACTIVATION flag, not a scope oracle, and it is also 0 on the
`--scope staged` per-commit-hook tier (`hooks/project-checks` execs
`verify.sh all --profile debug --scope staged --include-infra`), which was
therefore paying the `--features gui` link for closures that cannot reach
`reify-gui`. The emission condition now reads `SCOPE` and a separate
`AFFECTED_CLOSURE` directly. The three arms, their fail-wide taxonomy and the
measurements behind them are documented ONCE, on the decision itself — the
"NARROWED on the same affected-crate axis" bullet in `scripts/verify.sh`'s
`add_test_passes` — and are deliberately not restated here._

_What matters for THIS note. First, `NARROW_ACTIVE`, `AFFECTED`,
`AFFECTED_ALL_FLAGS` and the `--workspace` coupling on
staged-without-`--narrow` are all value-identical to before, so **the
THROUGHPUT-COUNTS sentinel does NOT move** and no cell of the table above
changes: all four shapes are captured only at `--scope all` and `--scope
branch`, and this change is confined to `--scope staged`. Confirmed by running
`tests/infra/test_verify_throughput.sh` green rather than by asserting it.
Second, the saving is NARROWER than "every commit" — only a staged diff whose
paths ALL map to crates and whose reverse closure excludes `reify-gui` takes the
narrowed arm. Measured on a hermetic fixture at `--scope staged` with no
override: a scripts-only staged diff yields the `ALL` sentinel and a
`tests/infra`-only one yields an EMPTY closure, and both still fail wide and keep
paying the link. Reclaiming the `tests/infra`-only shape is NOT a one-line
change: an empty closure is genuinely overloaded, since `decide_scope`'s
git-failure fail-wide paths also return `RUN_RUST=1` with an empty
`CHANGED_FILES_RAW`, so "provably no crates" would first need a distinct
closure-available sentinel to tell it apart from "the diff could not be read".
The merge gate remains unconditional by contract, so a hook-tier skip is
LATENCY, never a coverage hole._

_Counts bumped 2026-08-01 (task 5629): added
`./scripts/tree-sitter-freshness.sh ensure` (the compiled-tree-sitter-parser
freshness gate) to `build_plan` inside the `RUN_RUST=1` block in
`scripts/verify.sh`, immediately after `tree-sitter-generate.sh` — that
ordering is load-bearing, since the fingerprint must be taken after
`src/parser.c` is regenerated on disk and forced before any cargo leaf
compiles it. Net change: +1 non-comment plan line wherever `RUN_RUST=1` —
every `scope=all` plan, and `scope=branch` for the `RUN_RUST=1` shapes
(reify-doc, reify-eval). docs-only branch stays 0 and gui-only branch stays 3
(`RUN_RUST=0` there, so the leaf is not emitted). The machine sentinel moves
18 → 19 for the `scope=all` cells and 17 → 18 for the shape (b)/(c)
`scope=branch` cells._

_Counts bumped 2026-08-01 (task 5629, review round 2): added a SECOND
tree-sitter leaf, `./scripts/tree-sitter-freshness.sh check`, to `build_plan`
after the cargo compile wave. The `ensure` leaf above only ATTEMPTS the repair —
it bumps mtimes and trusts cargo to act on them, and never fails for a condition
it believes it fixed — so a plan carrying `ensure` alone had no evidence the
rebuild actually happened. `check` asserts, after the fact, that the archive
cargo just linked was built from the sources on disk. Guarded on
`RUN_RUST=1 && (DO_LINT || DO_TYPECHECK)`, i.e. exactly when a `cargo check` /
`cargo clippy` leaf precedes it — an assertion emitted before anything compiled
would hard-fail the very staleness `ensure` had queued a repair for. Net change:
+1 non-comment plan line in the same cells as the `ensure` leaf, since all four
shapes here run `action=all`. The machine sentinel moves 19 → 20 for the
`scope=all` cells and 18 → 19 for the shape (b)/(c) `scope=branch` cells._

_Counts NOT bumped 2026-08-20 (task 5629, amendment pass): the `check` leaf's
guard widened from `RUN_RUST=1 && (DO_LINT || DO_TYPECHECK)` to
`RUN_RUST=1 && (DO_LINT || DO_TYPECHECK || DO_TEST)`. The `DO_TEST` carve-out was
reasoned from "action=test has no compile leaf before this pole", but the leaf is
emitted AFTER `add_test_passes`, and on an `action=test` plan `add_test_passes`
emits `cargo nextest run --workspace` — which compiles the parser. So the
test-only tier forced a rebuild via `ensure` and then asserted nothing, carrying
the whole one-level-up false GREEN the leaf exists to close.
**The sentinel does NOT move and no cell of the table above changes:** all four
shapes are captured at `action=all`, which already satisfied `DO_LINT`. Confirmed
by re-running the regeneration command at HEAD (still 20), not by assuming it.
The widening is observable only on an `action=test` plan, where
`verify.sh test --profile both --scope all --print-plan` now ends with
`./scripts/tree-sitter-freshness.sh check` after the last nextest leaf._

_Counts RE-DERIVED 2026-08-28 (task 5629, rebase onto main): the two notes
above were originally written on the task branch against a 16-count baseline and
read 16 → 17 → 18 while they sat there — which is why their COMMIT SUBJECTS
still say "16 → 17" and "17 → 18". `main` meanwhile moved the same cells
independently: task 5076 added two leaves (16 → 18) and then narrowed the
gui-feature test pass out of the shape (b)/(c) `scope=branch` cells (18 → 17).
So NEITHER side's numbers described the rebased tree, and both sides' sentinel
blocks conflicted textually on every replayed commit. The endpoints in the two
notes above were therefore rewritten during conflict resolution to ride on
main's baseline (18 → 19, then 19 → 20), and the numbers are not a hand-merge:
they were re-measured on the rebased tree with the documented `--print-plan`
oracle below, via `tests/infra/test_verify_throughput.sh`, whose
`note(X) == live(Y)` assertions report the live count regardless of whether they
pass. Measured at this HEAD: `scope=all` = 20 for all four shapes;
`scope=branch` = 0 (docs-only), 19 (reify-doc), 19 (reify-eval), 3 (gui-only) —
54 passed / 0 failed. The deltas the two notes claim (+1 each, in the
`RUN_RUST=1` cells) are unchanged in kind; only their absolute endpoints moved,
because two independent +1s landed underneath them. The human-readable table and
this sentinel are re-synced in lockstep per the standing task-5125 convention._

## Heavy-Work Narrowed Markers

`scope=all` always produces: `cargo clippy --workspace` and
`cargo nextest run --workspace` (OCCT crates are now in the pool, bounded
by the nextest occt test-group at max-threads=4; task 4451).

Under `scope=branch` + narrowing:

| Shape | OCCT handling | cargo flags | cargo present |
|-------|--------------|-------------|---------------|
| (a) docs-only | N/A | — | no (empty plan) |
| (b) reify-doc (non-OCCT) | N/A | `-p reify-doc` (not `--workspace`) | yes |
| (c) reify-eval (OCCT) | in nextest pool, occt group bounds concurrency | `-p reify-eval` (not `--workspace`) | yes |
| (d) gui-only | N/A | — | no (GUI npm only) |

For shape (b), the scope=branch plan equals scope=all minus: replacing
`--workspace` with `-p reify-doc` in clippy/nextest (narrowing).

For shape (c), the scope=branch plan equals scope=all minus: replacing
`--workspace` with `-p reify-eval` in clippy/nextest (narrowing). Task 4451:
the gated pass is gone; reify-eval runs in the single nextest pool.

For shape (d), 17 of the 20 scope=all steps are Rust; branch scope drops
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
to execute all 20 steps including `cargo clippy --workspace` (≈ 20 s warm)
and `cargo nextest run --workspace` (≈ 10+ min warm; task 4451: OCCT crates
are now in the pool, bounded by the nextest occt group max-threads=4).

### Plan-generation overhead (scope=all, --print-plan)

```
real  0.188 s
```

Scripting overhead only — plan is printed but no steps execute.

## Delta as Evidence

Counts are deliberately NOT restated here.  The Plan-Step Counts table and its
THROUGHPUT-COUNTS sentinel above are the single authoritative copy, and only
the sentinel is checked by `tests/infra/test_verify_throughput.sh`'s drift
guard — so a third hand-maintained copy in this narrative can (and did, between
the 2026-08-03 and 2026-08-05 amendments) fall out of lockstep with both while
every gate stays green.  Read the counts off the table; this section states
only WHAT each shape drops, which is the qualitative half of the evidence.

- **docs-only branch:** saves every step — the plan is empty.  Verify exits in
  < 0.3 s.
- **non-OCCT crate branch (reify-doc):** narrows `--workspace` clippy and
  nextest to `-p reify-doc`, and — since 2026-08-05 (task 5076) — drops the
  `--features gui` test-execution pass, whose affected-crate closure does not
  reach `reify-gui`.  The remaining savings are wall-clock, from skipping
  unaffected crate compilation.
- **staged per-commit-hook tier (`--scope staged`, no `--narrow`):** since
  2026-08-13 (task 6030) the `--features gui` drop applies here too, on the same
  closure-membership test — this tier is `hooks/project-checks`' own invocation.
  It drops for a staged diff whose paths ALL map to crates and whose closure
  excludes `reify-gui`; a scripts-only or `tests/infra`-only staged diff still
  pays it, because its closure comes back unavailable and fails wide.  Nothing
  ELSE narrows on this tier: clippy and the workspace nextest pass keep
  `--workspace`, so this shape is absent from the table above and its plan-step
  counts are untouched.
- **OCCT-touching crate branch (reify-eval):** clippy and nextest narrowed
  to `-p reify-eval` (task 4451: gated pass folded into the nextest pool), and
  the `--features gui` pass likewise dropped — the fixture drives `AFFECTED`
  through `REIFY_AFFECTED_CRATES_OVERRIDE` with the literal single-crate list
  `reify-eval`, so its closure lacks `reify-gui` even though a real reify-eval
  change would pull it in.
- **gui-only branch:** skips all Rust steps; runs only the GUI npm steps.

No numeric improvement threshold is asserted here.  The step counts (per the
table) and the absent/narrowed heavy-work markers are the evidence.

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
