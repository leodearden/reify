# PRD — Unified warm-lane pool: CoW-seed task-dispatch worktrees AND merge-speculation slots

**Status:** active · version-agnostic infrastructure foundation · authored 2026-06-17 · this is **Phase 6** of `docs/prds/warmer-builds-merge-verify.md` (generalized to a *unified* pool per the §10 "Future-lever note"), unblocked now both gating triggers hold (Phase 1 κ landed; de-risking spike #4641 done).
**Source design:** `docs/design/warmer-builds-merge-verify.md` §10 (Phase 6 sketch) + the warmer-builds **PRD** §10 "Future-lever note"; **de-risking spike memo** `docs/design/phase6-xfs-reflink-cow-spike-results.md` (#4641, 2026-06-17 — the empirical basis for every "spike-proven" claim below). Re-locate every cited symbol at implementation time — `main` moves fast; the line is a hint, cite-by-symbol.
**Scope guard (inherited, load-bearing):** warmth never narrows the gate. The merge gate stays full-scope, full-correctness (verify-scope-contract **C2**; `scripts/verify.sh` force-`--scope all` for `DF_VERIFY_ROLE=merge`, drift-tested #4059). This PRD makes builds *start warm*, never *cover less*.

---

## 1. Goal — one CoW warm-lane pool that starts every concurrent build warm

Phase 1 (κ, `dark_factory:1692`, **landed**) warms only the *serial* merge-verify lane (`_merge-verify`, reset-in-place, `git.persistent_merge_worktree: true` in `orchestrator.yaml:234`). Every **other** lane on the box still builds cold from an empty `target/`:

- **The N concurrent task-dispatch worktrees.** N = the orchestrator's `max_concurrent_tasks` (read once at startup; currently **24**, was 48 — a tunable knob, *not* a hardcoded constant). Each dispatched task agent today gets a fresh `git worktree add` with an empty `target/` and pays a full cold dependency compile before its first scoped verify.
- **Future merge-speculation slots.** When the merge train runs at depth K>1 (Lever C, `dark-factory/plans/merge-throughput-multihost-verify-prd.md`), each speculative verify slot would today be cold too.

A *task* worktree's `target/` measures ~115–177 GB on disk; N cold full targets is both a disk problem and a per-dispatch cold-build tax.

**The mechanism is spike-proven (#4641, 2026-06-17):** a `cp -a --reflink=always` clone of a warm `target/` on XFS-reflink transfers build warmth faithfully and **path-independently** — a seeded lane ran the full merge gate in **9:31 vs 22:29 cold** (~58% total, ~70% / ~12.6 min off compile-link, ~904 of ~940 unit-compiles skipped), additive to sccache, and the CoW substrate is stable over reset-in-place cycles (no fragmentation accumulation, no space leak, no perf drift — Q2 SAFE).

**User-observable end state (consumers: the orchestrator task-dispatch path + the merge train + every human/agent waiting on a build):**

| Lane class | Today (cold) | With the warm-lane pool |
|---|---|---|
| Task-dispatch worktree, first verify | full cold dependency compile | CoW-warm — recompiles only the task delta closure |
| Merge-speculation slot (K>1, when Lever C lands) | full cold per slot | CoW-warm per slot; `main` advance stays strictly serial+ordered |
| Disk for N+K warm lanes | N+K × ~115 GB full targets | one ~115 GB base + deltas-only per lane (CoW shared extents) |

All wall-time figures are **expectations, never frozen RED-test thresholds** — inheriting the warmer-builds PRD §9/G6 convention: each task asserts a *measured improvement direction + a recorded delta vs a same-box cold control*, never a guessed minute-count.

## 2. Background — why the spike unblocks this, and what it simplified

The warmer-builds design (§10) and PRD (§10 Future-lever note) left Phase 6 behind two gates: **(1)** Phase 1 κ must land (the warm `target/` is the *seed source* — the review is incoherent before it exists), and **(2)** a de-risking spike must answer §10.7's make-or-break questions. Both now hold:

- **κ landed and live** — `PERSISTENT_MERGE_WORKTREE_NAME = '_merge-verify'`, reset-in-place, prune-exemption, and the safety-valve are present in DF `git_ops.py`; `git.persistent_merge_worktree: true` for reify. The rolling warm base this PRD seeds from **exists on disk**.
- **Spike #4641 answered §10.7** — Q1 PROMISING (the 58% measurement above), Q2 SAFE. The desirability bookmark **#4469** ("Phase 6 desirability review", deferred) had exactly these two triggers; authoring this PRD *is* that review resolved positively (the L2 gated review `esc-4642-47` is its consumer).

**Two spike findings materially simplify the design vs the §10.2 sketch:**

1. **cargo's freshness/metadata hash is PATH-INDEPENDENT** (spike §4/§6.1, decisive control: a bare `cargo build --workspace` in the original seed path vs a renamed CoW clone produced a **byte-identical** Fresh/miss profile — 383 == 383 Fresh units — and the *same* requested unit hashes, e.g. `reify-audit-498551377d43003e`). **Consequence:** the design §10.2 *vector-a* `--remap-path-prefix` machinery and *vector-b* bind-mount-to-one-canonical-path machinery are **unnecessary** for warmth to transfer. A lane at its own distinct path just works. Fixed-path-per-lane is retained below only for the *secondary* sccache-hit-rate + landlock-scoping benefits, **not** as a cargo-correctness requirement.
2. **The only load-bearing seed↔lane constraints are RUSTFLAGS consistency + matching `verify.sh` invocation + mtime ordering** (spike §2/§8). Keep `RUSTFLAGS` identical (default `""`) between base build and lane builds (a path-remap would invalidate the very fingerprints CoW preserves); seed the base with the *same* `verify.sh` invocation the lane runs (else invocation-mismatch re-materializes units); make lane sources older than seeded artifacts (mtime). These are the contract's hard preconditions (§10).

## 3. Sketch of approach — one rolling base, a pool of CoW lanes, two consumers

```
   Phase-1 κ warm merge target/  ──(reflink, atomic, on every main advance)──▶  rolling warm BASE
   (_merge-verify, at head)                                                     (on XFS-reflink volume)
                                                                                       │
                                       cp -a --reflink=always  (4–5 s, deltas-only)    │
                          ┌────────────────────────────────────────────────────────────┤
                          ▼                                                              ▼
          TASK-DISPATCH lanes  _lane-0 … _lane-{N-1}                 MERGE-SPECULATION slots  _spec-0 … _spec-{K-1}
          (N = max_concurrent_tasks @ startup)                       (K = _MERGE_AHEAD_BOUND, when Lever C lands)
          • reset-in-place per task base                             • reset-in-place per candidate merge commit
          • RELAXED correctness (not on path to main;               • STRICT correctness (on path to main):
            a stale-fingerprint false-green is caught                 inherits Phase-1 §10 invariants; `main`
            by the serial merge gate downstream)                      advances strictly serial+ordered via CAS
```

**Substrate (G3 / D2):** a **loopback XFS-reflink image** on the existing ext4 `data_lv` (the spike-proven route — `/var/lib/reify-xfs-spike.img` reflink worked), mounted under the worktree base. No LVM surgery, no data-LV shrink, fully reversible.

**Split (G4 / D8 — the canonical "reify ships primitives, dark-factory wires consumers" seam):**

| Side | Deliverables |
|---|---|
| **reify** (this batch) | provisioning script · CoW-clone+mtime+RUSTFLAGS-guard helper · base-refresh+defrag-signal helper · preflight guard · the end-to-end **integration-gate harness** (the G2/G5 leaf) · `orchestrator.yaml` knobs + docs (the consumer contract) |
| **dark-factory** (`dark_factory:` tasks, depend cross-project on reify's contract) | task-dispatch wiring (allocate-from-pool + seed instead of cold `git worktree add`) · merge-speculation-slot wiring (coordinated with Lever C) · `.mcp.json` re-provision per lane · landlock re-scoping · pool allocate/reset/release lifecycle |

## 4. Resolved design decisions

- **D1 — Unified pool serves *both* task dispatch and merge speculation** *(Leo, 2026-06-17)*. One rolling warm base (the Phase-1 at-head merge `target/`), CoW-cloned into a pool of fixed-path lanes. Task-dispatch lanes run under the relaxed correctness regime (off the path to main); merge-speculation slots run under the strict regime (on the path to main, Lever C). Chosen over either consumer alone because both populations build cold today and both seed from the *same* base — splitting them into two PRDs would duplicate the substrate + clone + refresh machinery.
- **D2 — Loopback XFS image, not a carved LV** *(Leo, 2026-06-17)*. `mkfs.xfs -m reflink=1,bigtime=1` on a preallocated file on the ext4 `data_lv` (6.0 TB free), mounted via loop under the worktree base. Spike-proven; non-disruptive (no VG-extent dependency, no data-LV shrink); reversible (`umount`+`losetup -d`+`rm`). The §10.6 "carve an LV" route is deferred — migrate only if loop overhead ever measurably bites (it did **not** in the spike).
- **D3 — No path-remap, no bind-mount-to-canonical-path** *(spike §4/§6.1)*. cargo freshness is path-independent; a renamed clone is byte-identical. Design §10.2's vector-a/vector-b machinery is dropped. Fixed-path-per-lane is kept for the sccache-hit-rate bonus (§1.2 of the design) + landlock-scope simplicity, **not** for cargo correctness.
- **D4 — RUSTFLAGS + `verify.sh`-invocation consistency is the load-bearing seed↔lane constraint** *(spike §2/§8)*. `seed-warm-lane.sh` **asserts** the lane build env's `RUSTFLAGS` equals the base build's (default `""`) and **fails closed** on mismatch — a silent mismatch would cold-rebuild the whole workspace and erase the win. The base is built with the *same* `verify.sh` invocation the consuming lane runs.
- **D5 — mtime normalization for fresh-checkout lanes; reset-in-place sidesteps it** *(spike §2)*. A fresh `git worktree add` stamps sources at wall-clock *now* (newer than seeded artifacts → cargo rebuilds everything). The clone helper stamps all sources (excluding `target/`, `.git`) to a fixed old epoch (`2020-01-01`) then `touch`es the delta. Reset-in-place lanes (`git clean -xfd -e target` re-touches only changed files) need no global stamp.
- **D6 — Single rolling base, refreshed on every main advance; defrag decoupled from freshness** *(PRD §10 Future-lever note + spike §7)*. Refresh the base by atomic reflink-rename of the *advancing* lane's at-head `target/` (metadata-only, seconds). XFS refcounting means an in-flight clone is independent the instant it is taken and old-base extents free only when the last clone releases them — **no drain protocol**. Fragmentation is reset **on a signal** (`xfs_bmap` extent counts), not a fixed cadence, by promoting the invariant-6 safety-valve **cold** build's `target/` as a fresh *contiguous* base (≈free — already scheduled, and doubles as the correctness check).
- **D7 — B+H: full contract (§9) + two-way boundary-test sketch (§10)** *(Leo, 2026-06-17)*. The seam crosses repos and the merge-speculation half sits on the path to `main`; specifying signatures + boundary scenarios up front lands the integration gate (δ) as a first-class task rather than letting it starve under the narrow-lock orchestrator.
- **D8 — reify ships primitives + the integration-gate harness; dark-factory wires the consumers** *(established pattern)*. Mirrors `setup-worktree-debug-port.sh`'s "G4 provisioning seam" and the cpu-governance α/β/γ↔ζ split (CLAUDE.md). The DF wiring tasks (ζ task-dispatch, η merge-speculation) are filed against `project_root=/home/leo/src/dark-factory` and depend cross-project on reify's ε contract.
- **D9 — Pool size derives from `max_concurrent_tasks` at startup** *(Leo, 2026-06-17)*. N = the orchestrator's configured task-concurrency cap read **once at startup** (`orchestrator.yaml:11`, currently 24), **not** a hardcoded constant — if the cap is retuned (it was 48→24 on 2026-06-04), the pool tracks it. Total lanes = N (task) + K (merge-spec, = `_MERGE_AHEAD_BOUND`).

## 5. Pre-conditions for activating

- **Phase 1 κ (`dark_factory:1692`) — LANDED and ON.** Hard prerequisite (the seed source). Verified: knob `true`, `_merge-verify` on disk. ✔
- **De-risking spike #4641 — DONE.** Q1 PROMISING, Q2 SAFE. ✔ (Both #4469 triggers satisfied.)
- **Merge-speculation half (η) coordinates with Lever C** (`dark-factory/plans/merge-throughput-multihost-verify-prd.md`). η is gated on Lever C's `_speculation_slot`/CAS-advance pipeline existing; until it lands, η is filed but blocked-on-consumer (the task-dispatch half ζ ships independently first — it is the lower-stakes, immediately-available consumer).
- **XFS-reflink substrate provisioned** (α) before any seeding (β/γ/δ).

## 6. Substrate verification (G3) — no novel `.ri` substrate; verified by direct host checks + the spike

This is shell/XFS/systemd/orchestrator infrastructure — **no `.ri` grammar surface**, so the grammar gate / `prd-decompose-verify.mjs` workflow is **N/A** (same as the cpu-load-admission-control PRD). G3 is discharged by **direct host checks**, all already passing (most *by the spike itself*):

| Capability | Phase | Evidence |
|---|---|---|
| `cp -a --reflink=always` clones a `target/` tree on XFS-reflink (deltas-only, shared extents) | α/β | **spike-proven** memo §3 (4–5 s for 72 GB; `filefrag` `shared` flag) |
| cargo freshness hash is path-independent (warmth transfers across the path boundary) | β/δ | **spike-proven** memo §4/§6.1 (383==383 Fresh, identical unit hashes) |
| mtime-normalization makes a seeded fresh-checkout lane skip the rebuild | β/δ | **spike-proven** memo §2/§5 |
| XFS-reflink reset-in-place is fragmentation/space/perf-stable over cycles | γ | **spike-proven** memo §7 (Q2 SAFE) |
| `mkfs.xfs -m reflink=1,bigtime=1` on a loopback image over ext4 mounts + reflinks | α | **spike-proven** (the spike ran on `/var/lib/reify-xfs-spike.img`, `reflink=1 bigtime=1`) |
| Phase-1 warm `_merge-verify` base exists to seed from | β/γ | verified: `orchestrator.yaml:234` + `git_ops.py` 1692 landed; `_merge-verify` on disk |
| ext4 `data_lv` has free space for the loopback image | α | verified: `df` → 6.0 TB free on `/media/leo/data_lv_1` |
| DF task-dispatch worktree provisioning seam exists to re-wire | ζ | `git_ops.py` `_create_merge_worktree`/`create_worktree` family present (the create-worktree path κ already touched) |
| DF `_MERGE_AHEAD_BOUND` / Lever C `_speculation_slot` seam | η | Lever C PRD present in `dark-factory/plans/`; pipeline pending |
| `xfs_bmap` / `losetup` / `fallocate` host tooling | α/γ | standard on the 6.x kernel; `xfsprogs` installed (per PRD §10 note) |

No FAIL bindings. The capability manifest (`docs/prds/warm-lane-pool-cow-seeding.capability-manifest.md`) records each leaf's bindings to these host-check evidences.

## 7. Cross-PRD / cross-repo relationship (G4)

The genuine seam is reify-primitives ↔ dark-factory-wiring (the D8 "reify ships, DF wires" split — *no* reciprocal "the other owns it"). The merge-speculation half coordinates with Lever C.

| Other | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/warmer-builds-merge-verify.md` (Phases 0–5) | this PRD **is** its Phase 6 (generalized) | rolling warm base = Phase-1 κ's `_merge-verify` `target/`; this PRD's §10 amends its "Future-lever note" | this PRD; companion θ amends the parent | this PRD |
| dark-factory task-dispatch (`git_ops.py` `create_worktree` path) | DF **consumes** reify's clone/seed primitives | allocate-from-pool + `seed-warm-lane.sh` replaces cold `git worktree add` for task lanes | **ζ (dark_factory task)**; depends on reify ε + β | queued (this batch) |
| dark-factory `plans/merge-throughput-multihost-verify-prd.md` (Lever C) | η **plugs the pool into** Lever C's K>1 speculative slots | `_speculation_slot` CoW-seed + strict serial+ordered CAS-advance preserved | **η (dark_factory task)**; coordinated-with / gated-on Lever C | queued, blocked-on-Lever-C |
| `dark_factory:1692` (Phase 1 κ) | this PRD **builds on** it | the warm `_merge-verify` `target/` is the seed source; D6 base-refresh hooks κ's advance | κ (**done/landed**) | built-on |
| `setup-worktree-debug-port.sh` (esc-4202-61 hygiene) | ζ **re-runs it per pooled lane** | per-lane `.mcp.json` debug-port re-provision on lane (re)assignment | reify ships the script; ζ invokes it | built-on |
| #4469 (Phase 6 desirability bookmark, deferred) | this PRD **resolves** it positively | the desirability decision | companion θ marks it done/superseded | resolved by this PRD |
| `esc-4642-47` (L2 gated review → "/prd Phase 6") | this /prd session **is** its consumer | the review gate | this PRD | consuming |

## 8. (no "why deferred" — this PRD is active)

Every task is shippable now except η, which is blocked-on-consumer (Lever C). The §5 dependency set is the only sequencing.

## 9. Contract — seam signatures + invariants (the B+H §, D7)

An implementer must be able to build the producer side from this section without further discussion. All scripts follow the repo's stdout-contract convention (resolved value on stdout, diagnostics on stderr; mirror `setup-worktree-debug-port.sh`).

### 9.1 Provisioning primitive — `scripts/provision-warm-lane-fs.sh`

```
provision-warm-lane-fs.sh [--size-gib <N>] [--img <path>] [--mount <dir>]
  defaults: --size-gib 600  --img /var/lib/reify-warm-lanes.img  --mount <worktree_base>/warm-lanes
  IDEMPOTENT: if <img> exists, is mounted at <mount>, and a reflink probe passes → print <mount>, exit 0 (no-op).
  else: fallocate <img>; mkfs.xfs -m reflink=1,bigtime=1 <img>; losetup + mount at <mount>;
        run a `cp --reflink=always` probe inside <mount> — exit non-zero if it fails ("Operation not supported").
  STDOUT: the resolved mount dir (bare path). STDERR: all diagnostics.
  Wired into setup-dev.sh (host-once, like build-manifold-deps.sh).
```
*Invariant P1:* never reformat a populated image (guard on existing XFS magic). *Invariant P2:* the probe is mandatory — a non-reflink mount must fail loudly, never silently fall back to cold copies.

### 9.2 Clone + warmth-transfer primitive — `scripts/seed-warm-lane.sh`

```
seed-warm-lane.sh <base_target_dir> <lane_dir> (--fresh-checkout | --reset-in-place)
  1. ASSERT env RUSTFLAGS == the base's recorded RUSTFLAGS (default "") → else exit non-zero (D4, fail-closed).
  2. cp -a --reflink=always <base_target_dir> <lane_dir>/target   (deltas-only; ~seconds).
  3. --fresh-checkout: find <lane_dir> -path target -prune -o -path .git -prune -o -exec touch -d 2020-01-01 (D5);
     --reset-in-place: no global stamp (git clean -xfd -e target already moved only changed-file mtimes).
  STDOUT: nothing on success (or the lane_dir). STDERR: diagnostics. Exit: 0 on faithful warm seed.
```
*Invariant S1:* the base must have been built with the **same** `verify.sh` invocation the lane will run (D4) — the helper records/checks an invocation fingerprint stamped beside the base. *Invariant S2:* `--reflink=always` (not `auto`) — a silent non-reflink full copy is a provisioning error, not a slow path.

### 9.3 Base-refresh + defrag-signal — `scripts/refresh-warm-base.sh`

```
refresh-warm-base.sh <advancing_target_dir> <base_dir> [--check-frag]
  on main advance (quiescent moment only — base target/ consistent, never mid-build, §10.4):
    cp -a --reflink=always <advancing_target_dir> <base_dir>.new && atomic rename → <base_dir>   (D6).
  --check-frag: report xfs_bmap extent counts; if over a threshold, signal a contiguous re-seed is due
    (promote the next invariant-6 safety-valve COLD build's target as the fresh base — ≈free).
  No drain protocol (XFS refcount frees old extents on last clone release).
```

### 9.4 Preflight guard — `scripts/warm-lane-preflight.sh`

```
warm-lane-preflight.sh   (mirrors check-manifold-deps.sh; first step of a pooled verify)
  fails with a clear, actionable message (not a cryptic later error) if:
    the volume is unmounted / not reflink-capable, the base is missing or invocation-mismatched,
    or RUSTFLAGS would differ between base and lane.
```

### 9.5 Pool lifecycle contract (consumed by dark-factory ζ/η)

A lane is `FREE` or `ASSIGNED`. The DF wiring implements:

```
acquire_lane(role ∈ {task, merge-spec}) -> lane_dir
    pick a FREE lane of the role's pool; if its target/ is empty/cold, seed-warm-lane.sh from the current base.
reset_lane(lane_dir, target_commit)
    git reset --hard <target_commit> && git clean -xfd -e target        (determinism invariant, §10 inv.1)
release_lane(lane_dir)
    ASSIGNED -> FREE; target/ RETAINED warm for the next assignment.
```
**Invariants (all MUST hold):**
1. **Reset determinism** — after `reset_lane`, the source tree is bit-identical to a fresh checkout of `<target_commit>`; `target/` retained; correctness rests on cargo's own fingerprinting recompiling exactly the changed crates + reverse-dep closure (exactly how local dev reuses `target/`).
2. **One consumer per lane at a time** — concurrent cargo against a single `target/` is forbidden (κ's invariant 3, replicated per lane).
3. **Fixed path per lane** — stable `_lane-K` / `_spec-K` paths (sccache-hit + landlock-scope benefit; D3 says cargo doesn't *require* it, but stability is free and beneficial).
4. **Merge-spec strict regime** — `main` advances strictly serial + ordered via CAS even at K>1 (Lever C's contract); a warm/cold safety-valve divergence is a **hard alarm**, never a silent pass (κ invariant 6, replicated).
5. **Task-lane relaxed regime** — off the path to main; a stale-fingerprint false-green is caught by the downstream serial merge gate. Still requires inv.1 so the agent's verify is meaningful.
6. **Pool-exhaustion fallback** — if no FREE lane, fall back to a cold ephemeral `git worktree add` (never block/deadlock the scheduler).
7. **Per-lane `.mcp.json` re-provision** — on (re)assignment, ζ runs `setup-worktree-debug-port.sh` so the lane's debug port is correct (esc-4202-61 hygiene); landlock re-scopes writes to the lane path.

## 10. Boundary-test sketch (two-way; the B+H §, closes G2 for δ/ζ/η)

| # | Scenario | Preconditions | Postconditions (asserted) | Faces |
|---|---|---|---|---|
| B1 | Provision idempotency | volume already mounted + reflink-capable | 2nd `provision-warm-lane-fs.sh` exits 0, no reformat, prints the same mount, probe passes | reify (α) |
| B2 | Non-reflink fails loud | image on a non-reflink FS | `provision` exits non-zero on the probe (never silent cold-copy fallback) | reify (α) |
| B3 | **CoW seed skips the rebuild** | warm base + `--fresh-checkout` lane + representative reify-gcode delta | scoped `verify.sh` recompiles only the delta closure; **measured wall improvement vs a same-box cold control** (direction + recorded delta, no frozen threshold); exit 0; identical test pass-set | reify (δ, the gate) |
| B4 | Path-independence holds | base built at path A, lane at path B | cargo Fresh-unit count in the lane == in-place control; no broad rebuild (spike §4 replicated as a regression test) | reify (δ) |
| B5 | RUSTFLAGS-mismatch guard | lane env `RUSTFLAGS` ≠ base | `seed-warm-lane.sh` exits non-zero with an actionable message; **no** silent cold rebuild | reify (β) |
| B6 | Base refresh on advance | main advances; new at-head target | base reflinked + atomically renamed; in-flight clones unaffected (independent); `df` flat; old extents free on last release | reify (γ) |
| B7 | Reset-in-place stability | one lane, K reset cycles | extent counts bounded (binary ≤2, rlibs 1, untouched shared), no space leak, no per-cycle drift (spike §7 as a CI-able shorter loop) | reify (γ) |
| B8 | Task-lane dispatch warm | DF allocates a task lane for a dispatched agent | journal shows the agent's first verify warm (delta vs cold baseline); `.mcp.json` port re-provisioned; landlock scoped to the lane path | dark-factory (ζ) |
| B9 | Merge-spec slot correctness | K>1 speculative verifies in parallel (Lever C live) | each verifies its candidate warm; `main` advances strictly serial + ordered (CAS); safety-valve from-scratch verify agrees, else hard alarm | dark-factory (η) |
| B10 | Pool exhaustion fallback | all lanes `ASSIGNED` | a new request falls back to a cold ephemeral worktree; scheduler never blocks | dark-factory (ζ) |

δ (reify integration gate) realizes B1–B7; ζ realizes B8/B10; η realizes B9.

## 11. Decomposition plan — task DAG with observable signals (G2)

Greek labels; task IDs assigned at decompose. All wall-time signals are *measured improvement direction + recorded delta vs a same-box cold control* — never a frozen minute-threshold (G6, inheriting the parent PRD's convention).

- **α — reify · provision the XFS-reflink loopback volume.** `scripts/provision-warm-lane-fs.sh` (§9.1) + `setup-dev.sh` wiring. *Intermediate* (unlocks β/γ/δ). **Signal:** on a clean box the script yields a mounted reflink-capable XFS volume and a `cp --reflink=always` probe passes; a 2nd run is an idempotent no-op (B1/B2). *Modules:* `scripts/`, `setup-dev.sh`.
- **β — reify · CoW clone + warmth-transfer helper.** `scripts/seed-warm-lane.sh` (§9.2) with the RUSTFLAGS-consistency fail-closed guard + mtime normalization. *Intermediate* (unlocks δ + DF ζ). *(depends_on α.)* **Signal:** seeds a lane from a base in ~seconds (deltas-only `du`), a `--fresh-checkout` lane's scoped verify recompiles only the delta closure (B3 mechanism), and a RUSTFLAGS mismatch exits non-zero (B5). *Modules:* `scripts/`.
- **γ — reify · base refresh + defrag signal + preflight guard.** `scripts/refresh-warm-base.sh` (§9.3) + `scripts/warm-lane-preflight.sh` (§9.4). *Intermediate* (unlocks δ + DF η). *(depends_on α.)* **Signal:** base atomically reflinked to a new head with in-flight clones unaffected and `df` flat (B6); a K-cycle reset-in-place loop shows bounded extents / no space leak (B7); `xfs_bmap` over threshold signals a contiguous re-seed. *Modules:* `scripts/`.
- **δ — reify · END-TO-END INTEGRATION GATE (the C-as-integration-gate leaf).** `tests/infra/test_warm_lane_pool.sh`: provision → seed a lane from the warm base → run a representative scoped `verify.sh` → assert warm-skip + recorded cold-vs-warm wall delta + identical pass-set + path-independence (B3/B4/B7). *Leaf.* *(depends_on α, β, γ.)* **Signal:** the harness runs green in CI (`tests/infra`) and records a measured cold-vs-warm delta proving warmth transfer on a real delta. *Modules:* `tests/infra/`, `scripts/`.
- **ε — reify · pool knobs + consumer contract docs.** `orchestrator.yaml` warm-lane-pool knobs (pool sizes derived from `max_concurrent_tasks` + `_MERGE_AHEAD_BOUND` per D9; substrate image/mount paths; defrag threshold) + a CLAUDE.md "warm-lane pool" section documenting the §9.5 lifecycle contract DF consumes. *Intermediate* (the DF-facing contract). *(depends_on δ — prove the mechanism before publishing the contract.)* **Signal:** knobs present and documented; the §9.5 acquire/reset/release contract is written down for DF to implement against. *Modules:* `orchestrator.yaml`, `CLAUDE.md`, `docs/`.
- **ζ — dark-factory · wire the TASK-dispatch consumer.** Replace cold `git worktree add` in the task-dispatch path with allocate-from-pool + `seed-warm-lane.sh`; per-lane `.mcp.json` re-provision (`setup-worktree-debug-port.sh`); landlock re-scope; pool allocate/reset/release lifecycle; pool-exhaustion cold fallback. *Leaf (DF-side).* *(depends_on ε contract + β, cross-project.)* **Signal:** the orchestrator journal shows a dispatched task agent's first verify warm (delta vs cold baseline) and the lane allocate/release lifecycle observable (B8/B10). *Repo:* dark-factory.
- **η — dark-factory · wire the MERGE-SPECULATION consumer (Lever C K>1).** CoW-seed the `_speculation_slot` verify worktrees from the rolling base; refresh the base on main advance via `refresh-warm-base.sh`; preserve strict serial+ordered CAS-advance; safety-valve divergence alarm. *Leaf (DF-side).* *(depends_on ε + γ + Lever C pipeline; blocked-on-consumer until Lever C lands.)* **Signal:** K>1 speculative verifies run warm in parallel, `main` advances strictly serial+ordered, and a safety-valve from-scratch verify agrees (B9). *Repo:* dark-factory.
- **θ — reify · companion correction-tasks.** Amend `warmer-builds-merge-verify.md` §10 + the Future-lever note (Phase 6 landed; the D3 path-independence simplification supersedes §10.2's remap/bind-mount); update design §10.6 (loopback chosen over LV, spike-validated); resolve bookmark **#4469** (done/superseded by this PRD); add the CLAUDE.md warm-lane-pool section if ε didn't fully cover it. *Leaf.* *(depends_on δ — don't rewrite the record before the gate proves the mechanism.)* **Signal:** the parent PRD/design prose reflects landed Phase 6 and #4469 is flipped out of deferred. *Modules:* `docs/`, task state.

**DAG:** α → {β, γ}; {α, β, γ} → δ; δ → ε; δ → θ; ε + β → ζ; ε + γ + (Lever C) → η. ζ and η are `dark_factory:` tasks with cross-project edges to ε (and β/γ).

## 12. Out of scope

- **Narrowing the merge-gate scope** — FORBIDDEN (C2 / §1). Warmth never trades coverage.
- **Carved-LV substrate** — deferred (D2); loopback is the chosen, spike-proven route. Migrate only on measured loop-overhead pain.
- **`--remap-path-prefix` / bind-mount-to-canonical-path** — dropped (D3); cargo freshness is path-independent. Resurrect only if a future toolchain change reintroduces path-sensitivity (regression-guarded by B4).
- **Global `CARGO_INCREMENTAL=1`** — still mutually exclusive with sccache; the per-lane CoW target reuses sccache-warmed rlibs, so incremental stays out (the parent PRD's δ measured it lane-scoped for the *merge* lane only).
- **btrfs-snapshot substrate** — the spike validated XFS-reflink and found it SAFE; btrfs is the parent design's noted worst-case for this rewrite-heavy workload. Not pursued unless XFS-reflink ever regresses.
- **Lever C itself (the K>1 train pipeline)** — owned by `dark-factory/plans/merge-throughput-multihost-verify-prd.md`; η only *plugs the pool into* it.
- **A′ coalescence** (`coupling-tolerant-train-former`) — a partial substitute for the merge-speculation pool (batching vs parallelizing); orthogonal, separately owned.

## 13. Open questions (tactical — surfaced, not blocking)

1. **Loopback image size.** 600 GiB default (≈115 GB base + N+K deltas + headroom). **Suggested:** size to base + (N+K) × measured-mean-delta × safety factor; confirm against live `du` of a few seeded lanes. Decide during α.
2. **Defrag-signal threshold.** The `xfs_bmap` extent count at which γ triggers a contiguous re-seed. **Suggested:** start generous (the spike saw the binary plateau at 2 extents over 5 cycles — fragmentation was a non-issue), tighten only if a real lane reused heavily shows drift. Decide during γ.
3. **Task-lane base-staleness re-seed cadence.** How stale a task lane's `target/` may drift (across many reset-in-place assignments) before re-CoW from a fresher base. **Suggested:** re-seed lazily on a measured rebuild-cost signal, not a fixed cadence (reset-in-place keeps lanes *warmer*, not staler, by construction). Decide during ζ.
4. **Whether task lanes need the per-lane `.mcp.json` port at all on every assignment vs once.** **Suggested:** re-provision on each (re)assignment (cheapest correct option; esc-4202-61). Decide during ζ.
5. **Safety-valve cadence for merge-spec lanes vs the existing κ safety valve.** η inherits κ's every-Nth-land + nightly; confirm K>1 doesn't need a tighter cadence. Decide during η.
