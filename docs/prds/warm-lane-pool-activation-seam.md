# Warm-lane pool — activation seam (topology bridge + base-coherence contract)

**Status:** authored 2026-06-19. Resolves the implicit deploy-topology choreography that `warm-lane-pool-cow-seeding.md` §9.1/§13 left unspecified, and the cross-repo base-coherence contract the landed dark-factory consumer does not yet honor.

> **Provenance.** Authored via `/prd` after `/unblock 4665` (the deploy capstone) found the activation as-specified would silently no-op. Two false premises in #4665 (wrong inert top-level knob; ext4 lanes) plus a third, deeper gap (the dark-factory consumer does not implement reify's D8/D10 base contract) make activation a *design* problem, not a knob flip. Bookmark task **#4690** tracks this seam; deploy capstone **#4665** consumes it.

## §0 — Relationship to the parent PRD (supersession)

`warm-lane-pool-cow-seeding.md` decided the **mechanism** (D2 loopback XFS-reflink image; D9 pool sizing; D10 always-re-seed-at-acquire with generation-dir base + atomic symlink-flip + reader-refcount GC) and shipped the reify-side primitives (`provision-warm-lane-fs.sh`, `seed-warm-lane.sh`, `refresh-warm-base.sh`, `warm-lane-preflight.sh`). It did **not** specify the **deploy topology**: where the XFS volume is mounted *relative to the orchestrator's worktree base*, and how the dark-factory consumer's base path is wired to reify's generation-dir base model. §9.1 ("mounted under the worktree base") and §13 left the mount↔`worktree_base` choreography implicit. This PRD closes that gap. It does not re-open D2/D9/D10.

## §1 — Consumer + user-observable surface (G1, G2)

**Consumer.** Deploy capstone **#4665** (operator-only: needs sudo for host provisioning + a live orchestrator restart) executes the choreography this PRD produces. The ultimate user surface is **every dispatched reify task agent**, whose first scoped verify starts WARM (reflink-cloned target/) instead of paying a full cold dependency compile.

**User-observable signal (end-to-end).** On the live reify orchestrator, after activation: a dispatched task agent's first scoped verify clones its lane via reflink from a warm base on the XFS volume — observable as (a) `warm-lane-preflight.sh` exiting 0 against the live mount + gen-dir base; (b) the orchestrator journal recording a warm first-verify and a cold-vs-warm delta; (c) `du` showing one base + per-lane reflink deltas, NOT N full `target/` trees; (d) the orchestrator restarting cleanly (no crash-loop); (e) pool-exhaustion still cold-dispatching (never deadlocks, inv.6).

## §2 — Sketch of approach

**Topology — Option A: put the orchestrator's `worktree_base` on the reflink volume.** Reflink CoW (`cp -a --reflink=always`, fail-closed) only works *within one XFS filesystem*. The dark-factory consumer creates lanes at `worktree_base/_lane-K` and reads the warm base under `worktree_base` — and `worktree_base = (project_root / git.worktree_dir).resolve()`. The minimal clean seam is therefore "make `worktree_base` resolve onto the XFS mount", which uses the **existing** `git.worktree_dir` lever and needs **zero dark-factory placement code** (verified: DF has no worktree-location containment assumptions — see §3).

Rejected **Option B (make dark-factory mount-aware** — route lanes/base onto the mount while `.worktrees` stays on ext4): its isolation benefit is largely illusory, because the warm base is fed by the persistent `_merge-verify` worktree's `target/`, which must itself be on the reflink fs for `refresh-warm-base`'s `cp --reflink` to work — so `_merge-verify` lands on XFS under both options. Once `_merge-verify` + lanes are on XFS, B keeps only transient merge/cold-fallback worktrees on ext4 (a minority) at the cost of permanent DF complexity (a second worktree-root + per-kind routing + a new knob). Under the chosen Correct-first base model (below) DF changes anyway, and **Option A's DF change is a strict subset of B's** (base contract only, no placement routing).

**Concrete layout (single XFS loopback mount, dedicated path):**

```
<mount>  (XFS reflink loopback, e.g. /home/leo/src/warm-lanes, boot-persistent)
├── worktrees/                 = worktree_base  (git.worktree_dir resolves here)
│   ├── _merge-verify/target/  advancing source for refresh-warm-base (on XFS)
│   ├── _lane-0 … _lane-{N-1}/  task-dispatch lanes (reflink-cloned target/)
│   └── <task / _merge-* worktrees>  cold-fallback + merge worktrees
└── base/
    └── target            git.warm_lane_base_target_dir  (gen-dir base; `target` -> target.gen.N
                          symlink; sidecars target.rustflags / target.invocation)
```

`base/` is a **sibling** of `worktrees/` on the same XFS fs (reflink works across the fs, not just one dir) and is **outside** `worktree_base` so dark-factory's worktree prune never touches it. `_merge-verify/target` (advancing) → `refresh-warm-base` reflink → `base/target.gen.N` → lane seed reflink → `_lane-K/target`; all hops intra-XFS.

**Placement mechanism (recommended): path-stable symlink.** Keep `git.worktree_dir: .worktrees` and make `<repo>/.worktrees` a symlink to `<mount>/worktrees`; `.resolve()` follows it onto XFS. This keeps the `<repo>/.worktrees` path string stable for the surrounding tooling (`setup-worktree-debug-port.sh`, `land.sh`'s clean-tree gate, the per-worktree hooksPath isolation, `.mcp.json`). The absolute-path alternative (`git.worktree_dir: <mount>/worktrees`) is also DF-safe but moves the path out of `<repo>`; treated as a tactical fallback (§9).

**Base coherence — Correct-first (gen-dir base, no torn-read window).** Activate with a separate generation-dir base at `<mount>/base/target` under the full D10 model. This is torn-read-free by construction (a lane always clones one immutable `.gen.N`; the symlink flip is atomic; reader-refcount GC defers `rm` while a reader holds `flock -s`) and is the coherence model the merge-spec path (η) will require anyway. It is **gated on a dark-factory change** (§3) because the landed DF consumer does not yet honor the contract. The interim plain-dir base (`_merge-verify/target`, zero DF change, small torn-read window) was **rejected** in favor of never shipping a torn-read window onto the live orchestrator.

## §3 — Pre-conditions / substrate verification (G3)

This is shell/orchestrator/systemd/cross-repo substrate — the `.ri` grammar/semantic gate is N/A. Substrate verified by reading the live code on both sides:

**Verified present (reify):** `provision-warm-lane-fs.sh` (loopback XFS, idempotent, reflink probe P1/P2); `seed-warm-lane.sh` (reflink clone S2, RUSTFLAGS/INVOCATION guards, expects caller to resolve `<base>/target`→`.gen.N` and hold `flock -s` — the D8 seam); `refresh-warm-base.sh` (gen-dir staging + atomic `ln -sfn` flip + reader-refcount GC + inv.9 `--landed-commit` provenance guard — tasks 4661/4669); `warm-lane-preflight.sh` (5 fail-closed checks; base default `<mount>/base/target`, sidecars `<base>.invocation`/`<base>.rustflags`).

**Verified present (dark-factory):** `git.warm_lane_pool: bool` + `git.warm_lane_base_target_dir: str|None` on GitConfig (config.py); pool gated by `harness.py` (`warm_lane_pool_size = max_concurrent_tasks if config.git.warm_lane_pool else 0`); lane lifecycle (ζ #1788 + ν #1820) landed; warm-lane-aware crash recovery (#1794) landed; **no worktree-location containment assumptions** — `worktree_base = (project_root / git.worktree_dir).resolve()` accepts absolute paths, `hooksPath` is relative `'hooks'`, no `.relative_to(project_root)` math (→ Option A is DF-safe).

**Verified ABSENT (the load-bearing gap → DF prerequisite).** The landed dark-factory consumer does **not** implement reify's D8/D10 base contract:
- passes `warm_lane_base_target_path` **raw** to `seed-warm-lane.sh` (git_ops.py ~1048) — no symlink→`.gen.N` resolve;
- **no `flock`** anywhere in the seed/refresh path (no reader-refcount participation);
- invokes `refresh-warm-base.sh <advancing> <base>` with **no `--landed-commit`** (git_ops.py ~1113) — which the reify inv.9 provenance guard *rejects* for a real (non-self-copy) gen-dir advance;
- **no torn-read guard** between a lane's seed-clone and a concurrent base rewrite.
It works only with `base == _merge-verify/target` as a **plain directory** (default config). Therefore the gen-dir base **requires** a dark-factory change before reify can point at it — filed as the R1 prerequisite, not assumed (G3-blocking).

**Host facts.** Root fs is ext4 (`/dev/nvme0n1p5`), no reflink — confirming the XFS loopback is mandatory. A separate ext4 `data_lv` (`/media/leo/data_lv_1`, `/dev/mapper/vgroup0-data1`) holds the orchestrator build worktrees and is a candidate home for the loopback image (more free space than root `/var/lib`). Orchestrator runs as a systemd **--user** unit (`orchestrator-reify.service`, `After=network.target fused-memory.service …`, `Wants=` not `Requires=` for fail-open startup). `provision-warm-lane-fs.sh` currently does an **ephemeral** `losetup`+`mount` — boot-persistence is a gap (R2).

## §4 — Resolved design decisions

- **DA1 — Option A topology.** `worktree_base` lives on the XFS reflink mount; uses the existing `git.worktree_dir` lever; zero DF placement code. (Rationale §2.)
- **DA2 — Path-stable symlink placement.** `<repo>/.worktrees` → `<mount>/worktrees`; `git.worktree_dir: .worktrees` unchanged; tooling path strings preserved. Absolute-path `worktree_dir` is the documented fallback.
- **DA3 — Correct-first gen-dir base.** Activate with the separate D10 generation-dir base at `<mount>/base/target` (`git.warm_lane_base_target_dir` set); no interim torn-read window. Gated on the DF base-contract change (R1).
- **DA4 — base/ as XFS sibling, outside worktree_base.** Same fs as lanes (reflink) but not under `worktree_base` (prune-safe).
- **DA5 — Fail-open boot ordering.** The loopback mount is made boot-persistent and ordered before the orchestrator via `Wants=`/`After=` (mirroring the unit's existing soft-dependency posture): a missing/failed mount degrades to the cold path (inv.6), never blocks orchestrator start.
- **DA6 — DF base contract is the only cross-repo change.** R1 (dark-factory): resolve `<base>/target`→`.gen.N`; hold `flock -s` across the seed cp; pass `--landed-commit <landed_sha>` to `refresh-warm-base.sh`; honor the reader/writer flock protocol (coherence falls out — no separate mutex).

## §5 — Out of scope

- **Merge-speculation (η / #1789) activation** — the K>1 `_spec-K` pool on the path-to-main is the higher-stakes consumer, activated separately. (This PRD's gen-dir base is a *prerequisite* for it, not its activation.)
- **The live deploy itself** — provisioning + migration + restart on the live host is **#4665** (operator-only); this PRD produces the config, scripts, contract, and the documented choreography #4665 runs.
- **Re-opening D2/D9/D10** mechanism decisions.
- **bwrap / sandbox** changes; **sccache** backend; CPU-governance — orthogonal axes.

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Resolution |
|---|---|---|
| Mount ↔ `worktree_base` topology | **reify** | DA1/DA2: symlink `.worktrees`→`<mount>/worktrees`; config + migration script (R3). |
| Generation-dir **base contract** (symlink-resolve + `flock` + `--landed-commit`) | **dark-factory** | R1: implement in `git_ops.py`. Reify's scripts already implement their half; reify holds the contract test (R5); DF holds its integration test. Two-way (H). |
| Boot-persistent loopback mount ordering | **reify** | R2: systemd `.mount` + orchestrator `Wants=`/`After=` wiring. |
| Live deploy choreography | **reify → operator (#4665)** | R6 re-gates #4665; #4665 depends on {R1, R2, R3, R4}. |
| Parent §9.1/§13 reconciliation | **reify** | R6 amends `warm-lane-pool-cow-seeding.md`. |

No new *contested* seam is introduced; R1 is a clean owner assignment (dark-factory owns its consumer code), tracked as a reify→dark_factory external dependency.

## §7 — Boundary-test sketch (G5 / B+H)

High-stakes seam (live-orchestrator worktree substrate + cross-repo + concurrent reader/writer) → contracts + two-way boundary tests.

**Contract** = the D8/D10 base protocol (already prose in `seed-warm-lane.sh`/`refresh-warm-base.sh` headers + PRD §9.3/§9.5). This PRD formalizes the *consumer* obligations (DA6).

**Two-way tests:**
- **reify side (R5)** — `tests/infra/test_warm_base_coherence.sh`: a deterministic concurrency stress proving (a) a reader seeding from the resolved `.gen.N` under `flock -s` **never** observes a torn/mixed generation while `refresh-warm-base.sh` flips the symlink + GCs; (b) GC of a retired gen **defers** while a reader holds its lock; (c) the inv.9 `--landed-commit` provenance guard accepts a clean landed advance and rejects a dirty/HEAD-mismatched one. Faces the DF side.
- **dark-factory side (R1)** — a DF integration test that a lane acquired against a gen-dir base resolves the symlink, holds `flock -s`, and produces a warm (not torn, not cold) `target/`; and that `refresh_warm_base` passes `--landed-commit` so the reify guard is satisfied. Faces the reify side.

## §8 — Decomposition plan (one bullet per task → observable signal)

DAG: **R1 (dark-factory)** ┐ ; **R2**, **R3**, **R4**, **R5** (reify, parallelizable) ; all → **R6 (integration/re-gate)** → consumed by **#4665** (live deploy).

- **R1 — dark-factory: implement the D8/D10 base contract** (`git_ops.py`). Resolve `<base>/target`→concrete `.gen.N`; hold `flock -s <gen>.lock` across the seed `cp`; pass `--landed-commit <landed_sha>` to `refresh-warm-base.sh`. *Filed as a `dark_factory` task; reify R4/R6 + #4665 carry it as `external_deps`.* **Signal:** the DF integration test (§7) is green — a lane seeds warm from a gen-dir base with no torn read, and refresh satisfies reify's inv.9 guard.
- **R2 — reify: boot-persistent loopback mount + ordering.** A systemd `.mount` (or equivalent oneshot) for `/var/lib/reify-warm-lanes.img` (or the data_lv) at `<mount>`, with the orchestrator unit `Wants=`/`After=` it (DA5); wire into `setup-dev.sh`. **Signal:** after `systemctl --user restart` of the mount unit (reboot proxy), `<mount>` is mounted + `cp --reflink=always` probe passes, and `systemctl --user show orchestrator-reify.service -p After,Wants` lists the mount. *Modules:* `scripts/`, a unit file, `scripts/setup-dev.sh`.
- **R3 — reify: worktree_base relocation (symlink) + config knobs.** A tested `scripts/relocate-worktrees-to-warm-lane.sh` (idempotent; symlinks `<repo>/.worktrees`→`<mount>/worktrees`, preserving/relocating `_merge-verify`) + set `git.warm_lane_base_target_dir: <mount>/base/target` in `orchestrator.yaml` (knob present, pool **still off**). **Signal:** after the script runs, `git -C <repo> worktree add` lands a worktree whose `cp --reflink=always` probe passes (on XFS), and `setup-worktree-debug-port.sh` + `land.sh` clean-tree gate still operate against the relocated path. *Modules:* `orchestrator.yaml`, `scripts/relocate-worktrees-to-warm-lane.sh`.
- **R4 — reify: gen-dir base seeding + preflight validation.** A `scripts/seed-warm-base-initial.sh` (or documented step) that cold-builds `_merge-verify` once and runs `refresh-warm-base.sh --landed-commit <sha>` to initialize `<mount>/base/target` as a gen-dir base; validated by `warm-lane-preflight.sh`. *external_dep: R1 (needs the contract to be meaningful end-to-end).* **Signal:** `warm-lane-preflight.sh --mount <mount> --base-dir <mount>/base/target` exits 0 (all 5 checks pass). *Modules:* `scripts/`.
- **R5 — reify: two-way coherence boundary test (H).** `tests/infra/test_warm_base_coherence.sh` per §7 (reify side). **Signal:** the test passes deterministically in `tests/infra/run_all.sh`. *Modules:* `tests/infra/test_warm_base_coherence.sh`.
- **R6 — reify: parent reconciliation + #4665 re-gate (integration gate).** Amend `warm-lane-pool-cow-seeding.md` §9.1/§13 to reference the resolved topology; rewrite #4665's choreography to the A + gen-dir + Correct-first sequence; wire #4665 deps on {R1, R2, R3, R4}; supersede/fold bookmark **#4690**. **Signal:** #4665's description reflects the final operator runbook and its dependency set is wired; parent PRD cross-links this one. *Modules:* `docs/prds/warm-lane-pool-cow-seeding.md`, task state.

**Existing tasks to reconcile (for the decompose session):** **#4690** is the bookmark for this PRD — supersede it (fold into R6 or cancel in its favor). **#4665** is the deploy capstone — do **not** refile it; R6 re-gates its deps (incl. the R1 `dark_factory` external dep) and rewrites its choreography.

## §9 — Open (tactical) questions

1. **Placement final form** — symlink `.worktrees` (DA2, recommended) vs absolute `git.worktree_dir`. Resolve at R3 impl with the tooling-compat probe as the deciding test.
2. **Boot-mount mechanism** — systemd `.mount` with `Options=loop` vs a oneshot `ExecStart=provision-warm-lane-fs.sh`. R2 tactical; either must be ordered-before + fail-open.
3. **Image location + sizing** — `/var/lib` (root nvme) vs the ext4 `data_lv` (`/media/leo/data_lv_1`, more space); and 600 GiB vs a bump now that `worktree_base` (all worktrees) + a two-generation base flip (~2×115 GB transient) + cold-fallback/merge worktrees share the volume. Validate empirically at R4/#4665; size conservatively if the du delta approaches the ceiling. (Capacity floor, not a correctness exactness claim.)
4. **Migration disruptiveness** — whether to relocate existing in-flight worktrees or accept re-dispatch on the cutover restart (#4665 maintenance-window detail).
