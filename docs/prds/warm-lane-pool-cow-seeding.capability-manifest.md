# Capability manifest — warm-lane-pool-cow-seeding

Mechanizes G3 + G6 for `docs/prds/warm-lane-pool-cow-seeding.md`. Substrate is shell/XFS/systemd/orchestrator — **no `.ri` grammar surface**, so the grammar gate / `prd-decompose-verify.mjs` is **N/A** (same posture as `cpu-load-admission-control.capability-manifest.md`). Evidence forms here are **direct host checks** and **spike #4641 measurements** (`docs/design/phase6-xfs-reflink-cow-spike-results.md`), not grammar fixtures. Re-run the host checks at decompose; any that regress block the batch.

Legend: PASS evidence per binding; a FAIL value (`producer-absent`, `declared-only`, `bound≤floor`, `producer-downstream`, …) blocks queueing.

---

## α — provision XFS-reflink loopback volume *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `mkfs.xfs -m reflink=1,bigtime=1` on a loopback image mounts + reflinks | spike ran on `/var/lib/reify-xfs-spike.img` with `reflink=1 bigtime=1` (memo §1) | PASS (spike-proven) |
| ext4 `data_lv` has free space for the image | `df /media/leo/data_lv_1` → 6.0 TB free | PASS (host-check) |
| `fallocate`/`losetup`/`xfs_bmap` present on 6.x | standard; `xfsprogs` installed (parent PRD §10 note) | PASS |
| consumer: β/γ/δ seed/refresh against this volume | β/γ/δ in this batch depend_on α | PASS (downstream consumers in-batch) |

## β — CoW clone + warmth-transfer helper *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `cp -a --reflink=always` clones a `target/` tree (deltas-only, shared extents) | memo §3 (4–5 s for 72 GB; `filefrag` `shared`) | PASS (spike-proven) |
| cargo freshness hash is path-independent (warmth transfers across the path boundary) | memo §4/§6.1 decisive control: 383==383 Fresh, identical unit hashes in-place vs renamed clone | PASS (spike-proven) — **the load-bearing G6 premise** |
| mtime normalization makes a fresh-checkout lane skip the rebuild | memo §2/§5 | PASS (spike-proven) |
| RUSTFLAGS-mismatch guard fires (fail-closed) | β implements the assert; rejection-mechanism is **this task's own deliverable** (B5), verified by the δ gate observing a non-zero exit on mismatch | PASS (rejection-mechanism built+observed in δ) |
| seed source (Phase-1 warm base) exists | `orchestrator.yaml:234` + `dark_factory:1692` landed; `_merge-verify` on disk | PASS (host-check) |

## γ — base refresh + defrag signal + preflight guard *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| atomic reflink-rename refresh leaves in-flight clones independent | memo §3 (shared-extent independence) + XFS refcount semantics (parent §10.4) | PASS (spike-proven + design-cited) |
| reset-in-place is fragmentation/space/perf-stable over cycles | memo §7 (Q2 SAFE: binary ≤2 extents, rlibs 1, no leak, no drift) | PASS (spike-proven) |
| `xfs_bmap` extent counts are a usable defrag signal | memo §7 used `filefrag`/extent counts; `xfs_bmap` is the XFS-native equivalent | PASS (host-check at decompose) |

## δ — END-TO-END INTEGRATION GATE *(leaf — the G2/G5 integration-gate)*

| Capability asserted by the signal | Required from | Evidence | Verdict |
|---|---|---|---|
| provision → seed → scoped verify runs end-to-end and **skips the rebuild** | α + β + γ (all upstream prereqs) | spike §6 proved the seeded-vs-cold skip on the full merge command; δ replicates on a scoped task delta | PASS (capabilities all in δ's upstream dependency closure) |
| "measured cold-vs-warm wall delta" (no frozen threshold) | δ's own harness | signal asserts **improvement direction + recorded delta** vs a same-box cold control — G6 branch-1 numeric-floor N/A (no absolute bound frozen) | PASS (G6-clean; inherits parent PRD §9 convention) |
| identical test pass-set warm vs cold | cargo correctness | memo §6 (17 317+8 907 identical passes warm vs cold) | PASS (spike-proven) |
| path-independence regression (B4) | cargo freshness | memo §4 | PASS |

*DAG-direction check:* every capability δ's signal needs is delivered by α/β/γ which are **upstream** of δ — no `producer-downstream`. PASS.

## ε — pool knobs + consumer contract docs *(intermediate)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `max_concurrent_tasks` readable to size the pool (D9) | `orchestrator.yaml:11` (=24, tunable) | PASS (host-check) |
| the §9.5 lifecycle contract is implementable by DF | §9.5 gives acquire/reset/release signatures + 7 invariants | PASS (contract specified) |
| consumer: ζ/η implement against this contract | ζ/η depend_on ε cross-project | PASS |

## ζ — dark-factory TASK-dispatch wiring *(leaf, DF repo)*

| Capability asserted by the signal | Required from | Evidence | Verdict |
|---|---|---|---|
| task-dispatch path can be re-wired to allocate-from-pool + seed | DF `git_ops.py` create-worktree family | present (κ already touched this path); ζ owns the change | PASS (producer = ζ itself; seam exists) |
| reify clone primitive callable from DF | β (`seed-warm-lane.sh`) upstream | β in this batch, cross-project edge | PASS (upstream) |
| per-lane `.mcp.json` re-provision | `setup-worktree-debug-port.sh` | reify ships it (CLAUDE.md); ζ invokes | PASS (wired-on-main script) |
| "agent first verify warm (delta vs baseline)" | ζ wiring + β seed | end-to-end capability all upstream of / within ζ | PASS (no downstream dependency) |

## η — dark-factory MERGE-SPECULATION wiring *(leaf, DF repo; blocked-on-consumer)*

| Capability asserted by the signal | Required from | Evidence | Verdict |
|---|---|---|---|
| `_speculation_slot` K>1 pipeline to plug the pool into | Lever C (`dark-factory/plans/merge-throughput-multihost-verify-prd.md`) | PRD present in `plans/`; **pipeline pending** | PASS-as-prerequisite — η is **blocked-on-consumer** until Lever C lands; edge wired, not a fiction |
| "`main` advances strictly serial+ordered (CAS)" | Lever C's CAS-advance contract (**upstream/coordinated**, not downstream) | owned by Lever C; η depends on it | PASS (capability is upstream — G6 branch-3 satisfied, not inverted) |
| base refresh on advance | γ (`refresh-warm-base.sh`) upstream | γ in this batch | PASS (upstream) |
| safety-valve divergence alarm (negative assertion: a warm/cold mismatch must FIRE) | κ's safety-valve mechanism, replicated | κ landed; η replicates the alarm and B9 observes it firing on an injected divergence | PASS (rejection-mechanism inherited + observed) |

## θ — companion correction-tasks *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| parent PRD/design + #4469 are editable records | files exist on main; #4469 is a live deferred task | PASS |
| no code capability asserted (prose + task-state only) | — | PASS (documentation leaf) |

---

**Summary:** no FAIL bindings. The single load-bearing G6 premise (warmth transfers path-independently through CoW) is **spike-proven** (memo §4/§6.1), not assumed. η is correctly **blocked-on-consumer** (Lever C), with its cross-PRD edge wired rather than treated as a present capability. No frozen numeric thresholds (all wall-time signals assert improvement-direction + recorded delta).
