# Capability manifest — warm-lane pool activation seam

Mechanizes G3 + G6 for `docs/prds/warm-lane-pool-activation-seam.md` (decompose 2026-06-19).
One block per **leaf**, binding each capability the task's user-observable signal asserts to
evidence. Any FAIL value blocks the batch. **Substrate is shell / orchestrator.yaml / systemd /
cross-repo — the `.ri` grammar/semantic gate is N/A** (host-checks only, same as the parent
`warm-lane-pool-cow-seeding.md`). All evidence was re-verified against live source on 2026-06-19.

Evidence vocabulary used here:
- `grep:<file>:<line> wired` — symbol/flag present on main at the named line.
- `host-cap:<facility>` — a host facility (systemd unit, mountpoint, util-linux flock, XFS reflink) whose existence is host-observable, not code-resident.
- `producer:R<N> upstream` — the capability is delivered by an upstream task in the transitive dep closure (DAG-direction verified upstream, not downstream).
- `done:#NNNN` — capability shipped by an already-`done` parent-PRD task.

---

## R1 — dark-factory: implement the D8/D10 base contract (`git_ops.py`)

**Signal:** DF integration test green — a lane acquired against a gen-dir base resolves the
`<base>/target` symlink to its `.gen.N`, holds `flock -s`, and produces a warm (not torn, not cold)
`target/`; and `refresh_warm_base` passes `--landed-commit` so reify's inv.9 guard is satisfied.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `seed-warm-lane.sh` accepts a resolved concrete `.gen.N` base + a held `flock -s` (the D8 seam reify ships) | `grep:scripts/seed-warm-lane.sh:10-14 wired` (D8 seam header: caller MUST resolve `<base>/target`→`.gen.N` + hold `flock -s` across the `cp`) | PASS |
| `refresh-warm-base.sh` honors `--landed-commit` provenance (inv.9) | `grep:scripts/refresh-warm-base.sh:243 wired` (`--landed-commit <sha> is required`); `:252` HEAD-mismatch rejection | PASS |
| gen-dir base model (`.gen.N` staging + atomic `ln -sfn` flip + reader-refcount GC) reify ships | `grep:scripts/refresh-warm-base.sh:347-376 wired` | PASS |
| DF consumer entry points exist to modify (`_seed_warm_lane`, `refresh_warm_base`) | `grep:orchestrator/src/orchestrator/git_ops.py:1029,1067 wired` (the two methods R1 edits; currently pass base raw / omit `--landed-commit` — the gap R1 fills) | PASS |

R1 **is** the producer of its own signal (it writes the DF half). No capability is `producer-downstream`. The reify-side contract it builds against is present-on-main (rows 1–3). No FAIL.

---

## R2 — reify: boot-persistent loopback mount + ordering

**Signal:** after `systemctl --user restart` of the mount unit (reboot proxy), `<mount>` is mounted +
`cp --reflink=always` probe passes, and `systemctl --user show orchestrator-reify.service -p After,Wants`
lists the mount.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| reflink-capable XFS loopback provisioning (idempotent, mandatory probe) | `done:#4659` (`scripts/provision-warm-lane-fs.sh` — α, present) | PASS |
| systemd `--user` `.mount`/oneshot unit + `Wants=`/`After=` ordering | `host-cap:systemd-user-unit` (DA5; orchestrator already a `--user` unit with `Wants=`/`After=` soft-dep posture — PRD §3 host facts) | PASS |
| `setup-dev.sh` wiring point (host-once, like `build-manifold-deps.sh`) | `grep:scripts/setup-dev.sh wired` (present; host-once hook) | PASS |

No FAIL. **Consumer:** R6 (integration gate). `metadata.files: []` (new unit file path + `setup-dev.sh` edit + a script — multi-file, one new → tight-or-empty defers to `[]`).

---

## R3 — reify: worktree_base relocation (symlink) + config knob

**Signal:** after the relocation script runs, `git -C <repo> worktree add` lands a worktree whose
`cp --reflink=always` probe passes (on XFS), and `setup-worktree-debug-port.sh` + `land.sh`'s
clean-tree gate still operate against the relocated path.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `git.worktree_dir` lever + `.resolve()` follows a symlink onto XFS (DF accepts the relocated path) | `grep:orchestrator/src/orchestrator/git_ops.py:481 wired` (`worktree_base = (project_root / git.worktree_dir).resolve()`; PRD §3 confirms no `.relative_to(project_root)` containment math → Option A DF-safe) | PASS |
| `git.warm_lane_base_target_dir` config knob (set to `<mount>/base/target`, pool still OFF) | `grep:orchestrator/src/orchestrator/config.py warm_lane_base_target_dir wired` (GitConfig field, PRD §3 verified-present) | PASS |
| `setup-worktree-debug-port.sh` + `land.sh` operate against the relocated `.worktrees` path string (preserved by the symlink — DA2) | `grep:scripts/setup-worktree-debug-port.sh wired`, `grep:scripts/land.sh wired` (both present; DA2 keeps `<repo>/.worktrees` path string stable) | PASS |

No FAIL. **Consumer:** R6. `metadata.files: ["scripts/relocate-worktrees-to-warm-lane.sh", "orchestrator.yaml"]` (lock-charter-guard exit 0; the new relocation script + the single config file R3 writes).

---

## R4 — reify: gen-dir base seeding + preflight validation

**Signal:** `warm-lane-preflight.sh --mount <mount> --base-dir <mount>/base/target` exits 0 (all 5
checks: mounted, reflink-capable, base present, invocation match, RUSTFLAGS match).

| Capability asserted | Evidence | Verdict |
|---|---|---|
| 5-check preflight (`<mount>/base/target` default, `.invocation`/`.rustflags` sidecars) | `done:#4661` (`scripts/warm-lane-preflight.sh` — γ; `grep:scripts/warm-lane-preflight.sh:18-22,93` 5 checks + default base-dir) | PASS |
| initialize `<mount>/base/target` as a gen-dir base via `refresh-warm-base.sh --landed-commit <sha>` (cold-build `_merge-verify` once) | `done:#4661` (`scripts/refresh-warm-base.sh` gen-dir staging + `--landed-commit`); R4 ships the `seed-warm-base-initial.sh` wrapper that calls it | PASS |
| an ephemerally-provisioned reflink mount to validate against | `done:#4659` (`provision-warm-lane-fs.sh`; R4 does not need R2's *boot-persistence* to run preflight — a mount from α suffices) | PASS |
| (sequencing) DF consumer honors the gen-dir base end-to-end | `producer:R1 upstream` (external_dep `dark_factory:R1`; DAG-direction upstream ✓). **Note:** R4's *signal* (preflight exit 0) is producible from R4's own reify scripts + α **without** R1 — preflight never exercises the DF consumer. R1 is a sequencing dep so R4 does not stand up a gen-dir base no consumer can read; it is not a signal-capability gap. | PASS |

No FAIL. **Consumer:** R6. `metadata.files: []` (PRD frames it as "a `scripts/seed-warm-base-initial.sh` **or documented step**" — extent uncertain → tight-or-empty defers to `[]`).

---

## R5 — reify: two-way coherence boundary test (H, reify side)

**Signal:** `tests/infra/test_warm_base_coherence.sh` passes deterministically in `tests/infra/run_all.sh`.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| reader seeds from resolved `.gen.N` under `flock -s` and **never** observes a torn/mixed generation during a concurrent symlink-flip + GC | `grep:scripts/refresh-warm-base.sh:347-376 wired` (atomic `ln -sfn` flip + reader-refcount `flock` GC); `host-cap:util-linux-flock` | PASS |
| GC of a retired gen **defers** while a reader holds its lock | `grep:scripts/refresh-warm-base.sh:373-376 wired` (`flock -n -x` held across the `rm` so a reader's `flock -s` blocks the removal) | PASS |
| inv.9 `--landed-commit` guard **accepts** a clean landed advance and **rejects** a dirty / HEAD-mismatched one (G6 branch-4 rejection) | `rejection-check: grep:scripts/refresh-warm-base.sh:243 (missing `--landed-commit` → exit non-zero), :252 (HEAD mismatch → exit non-zero)` — rejection mechanism present and fires | PASS |
| `tests/infra/run_all.sh` harness to host the test | `grep:tests/infra/run_all.sh wired` (present) | PASS |

No FAIL. **Consumer:** R6 (and faces the DF side R1). `metadata.files: ["tests/infra/test_warm_base_coherence.sh"]` (lock-charter-guard exit 0; the single new test file).

---

## R6 — reify: parent reconciliation + #4665 re-gate (integration gate)

**Signal:** #4665's description reflects the final operator runbook (A + gen-dir + Correct-first
sequence) and its dependency set is wired; parent PRD `warm-lane-pool-cow-seeding.md` §9.1/§13
cross-links this PRD.

| Capability asserted | Evidence | Verdict |
|---|---|---|
| parent PRD §9.1/§13 editable to reference the resolved topology | `grep:docs/prds/warm-lane-pool-cow-seeding.md wired` (present; §9.1/§13 are the implicit-choreography sections this PRD closes) | PASS |
| #4665 (deploy capstone) + #4690 (bookmark) exist to re-gate / supersede | `done`-adjacent task state: #4665 `deferred` (deps `[4663,4667,4690]`), #4690 `blocked` — both confirmed live via `get_task` | PASS |
| all reify leaves + DF contract upstream of the gate | `producer:R2,R3,R4,R5 upstream` (intra-batch) + `producer:R1 upstream` (`dark_factory:R1` external_dep) — DAG-direction upstream ✓ | PASS |

No FAIL. **Consumer:** #4665 (out-of-batch live-deploy capstone). `metadata.files: ["docs/prds/warm-lane-pool-cow-seeding.md"]` (lock-charter-guard exit 0; the single parent-PRD doc R6 amends; the rest of R6's deliverable is task-state mutation, not file writes).

---

## Summary

| Leaf | Capabilities | FAIL bindings | metadata.files |
|---|---|---|---|
| R1 (dark_factory) | 4 | 0 | `[]` (git_ops.py + new DF integration test — DF architect acquires) |
| R2 | 3 | 0 | `[]` |
| R3 | 3 | 0 | `scripts/relocate-worktrees-to-warm-lane.sh`, `orchestrator.yaml` |
| R4 | 4 | 0 | `[]` |
| R5 | 4 | 0 | `tests/infra/test_warm_base_coherence.sh` |
| R6 | 3 | 0 | `docs/prds/warm-lane-pool-cow-seeding.md` |

**Zero FAIL bindings → batch clears the manifest gate.** No numeric-floor, grammar-fixture, or
field-population checks apply (shell/systemd/cross-repo substrate; the §9 Q3 capacity question is a
capacity floor, not a correctness exactness claim, per the PRD). The one rejection assertion (R5 row 3,
inv.9 guard) binds to a present, firing rejection mechanism.
