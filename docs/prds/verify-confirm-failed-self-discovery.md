# PRD: `verify.sh test --confirm-failed` — reify-self-discovered failure confirmation for the DF offline lane

**Status: authored 2026-07-24 (task 5368, re-scoped design-only 2026-07-24 by the L2 escalation-watcher, esc-5368-2 option B — produce a design, do not implement blind). Evidence base: this document's own §3 (live empirical verification against the installed `cargo-nextest 0.9.136`, AND live reading of dark-factory's already-landed consumer code, `main@e534de4553` in `/home/leo/src/dark-factory`). Program milestone: none on the reify side; on the DF side this is the missing cross-project precondition `docs/prds/offline-deep-test-lane-worker.md` calls **ζ**'s dependency (informally "ζ's job" in `offline_lane.py`'s own comments — see §1). Shape: cross-repo protocol seam, reusing `verify-retry-failed-only`'s subset-consumption primitive under opposite subset-construction ownership (self-discovered, not DF-supplied) — output-contract-first, unlike its sibling.**

Cross-repo pattern (CLAUDE.md): **reify ships the primitive, dark-factory wires the invocation.** Unusually for this pattern, **DF's side of the invocation is already fully built and already calling this flag in production** (§1) — reify is the last missing leaf.

---

## §1 — Consumer + user/operator-observable surface (G1)

**This is not a hypothetical future consumer — it is live, already-landed code calling an interface that does not yet exist on reify's side:**

`dark-factory` `orchestrator/src/orchestrator/offline_lane.py:955-992`, method `_default_confirmation_run` (the default implementation of the injectable `ConfirmationRunner` seam, task 1954/β3, already on `dark-factory main`), **today** spawns:

```
argv = ['scripts/run-offline-deep.sh', '--test-threads=1', '--confirm-failed']
env  = {**os.environ, 'DF_VERIFY_ROLE': 'offline'}
# stdout=PIPE, stderr=STDOUT (merged)
```

`run-offline-deep.sh` forwards this verbatim to `verify.sh test --test-threads=1 --confirm-failed` (§3) — which reify does not yet recognize, so this call **currently** returns `verify.sh`'s 59-line `usage()` dump on the merged stdout+stderr stream. `offline_lane.py`'s module-level `_parse_confirmed_failures` (lines 122-159) already carries a **defensive guard, landed as task 5308**, that detects `Usage:`/`Options:`/`verify.sh: ERROR` marker lines and rejects the whole output to `[]` rather than mis-parsing usage text as failing-test IDs — this guard is precisely what stopped the original false-premise mis-scrape from recurring, and it is *why* task 5368 exists as a clean design task instead of an emergency bugfix.

**The live consequence today:** `_handle_red_run` (`offline_lane.py:580-635`, task 1954/β3, also already landed) treats an empty `confirmed` list as "intermittent nondeterminism — log only, no fix task" (`offline_lane.py:620-627`). Because `--confirm-failed` doesn't exist yet, **every** numeric offline-lane red run is *currently* silently swallowed as if it were a flake — the entire confirm→fingerprint→dedup→file-fix-task→escalate pipeline (β3, fully built and unit-tested on the DF side) has never been able to fire for a genuine regression caught by the offline lane. This PRD is not merely closing a documentation gap; it is unblocking an already-built, currently-dormant DF capability.

**Consumers of the reify primitive:**
- (a) `offline_lane.py:_default_confirmation_run` (above) — the numeric/heavy-suite confirmation seam. **In scope.**
- (b) `offline_lane.py:_default_infra_confirmation_run` (`:1027+`) — a *sibling*, unrelated mechanism: re-runs `tests/infra/run_all.sh --scope host-infra` wholesale and parses `RESULT: FAIL (<name>)` lines via `_parse_infra_failures`. Does not call `verify.sh --confirm-failed` at all. **Out of scope**, named only to avoid confusion.
- (c) `offline_lane.py:_default_confirm_command` (task 2789, generic per-project commands) — also unrelated, not reify-specific. **Out of scope.**
- (d) The manual/operator bridge — `run-offline-deep.sh`'s own header: "the manual bridge/operator entry point during the Part-B window."
- (e) `docs/design/offline-deep-test-lane.md` §7.1's "Confirmation re-run" commitment (2026-06-09) and its DF-side decomposition `docs/prds/offline-deep-test-lane-worker.md` (β1-β3, δ, ζ) — this PRD is the reify-local leaf both of those depend on.

**User/operator-observable surfaces:**
- **The primary, load-bearing surface is `--confirm-failed`'s combined stdout+stderr text**, because `_default_confirmation_run` pipes both streams together and `_parse_confirmed_failures` treats every non-blank line as a confirmed-still-failing test ID (§3, §4). This is *not* a console-log nicety — it is the literal data contract an already-deployed parser depends on.
- Secondarily, the process exit code (kept for scriptability/operator/boundary-test use — §4.3 — but **not currently inspected** by `_default_confirmation_run`, which only reads `stdout`, never `proc.returncode`; confirmed by reading `offline_lane.py:990-992`).
- Once live: a `pending` fix task with failing-test IDs + suspect commit range in `metadata`, and an `escalate_info`, appearing after a *confirmed* (not merely first-observed) numeric red — directly observable via `get_tasks`/`get_pending_escalations` (β3's own already-written signal, `offline-deep-test-lane-worker.md:254-258`).

This PRD introduces no new engine seam — verify-pipeline plumbing only.

---

## §2 — Problem statement (cross-reference, not restatement — G7)

`docs/design/offline-deep-test-lane.md` §7.1 committed the offline lane to a confirmation re-run before declaring red. DF's engineers then **built the entire consuming pipeline ahead of the reify-side flag existing** — β1 (trigger), β2 (singleton worker + recording call), β3 (confirmation call + fingerprint + dedup + fix-task + escalation), δ (dedicated warm worktree) are all already on `dark-factory main`, each citing the reify-side runner as a precondition (`reify:A5` = `run-offline-deep.sh`, already landed on reify `main`). `β3`'s confirmation call assumes `--confirm-failed` exists. It does not (`git grep` on this worktree is clean; the `*)` unknown-arg arm at `verify.sh:508-511` proves it). The result, live today: `--confirm-failed` calls fail closed into a defensive `[]`, and β3 never fires (§1).

**Why can't `retry_failed_only` (task 5287, already landed on reify `main`) serve this need verbatim?** Its subset-*construction* is entirely DF-owned (`verify-retry-failed-only.md` §8 D2: DF parses per-test results from its own merge-gate attempt and writes a filter file + sets `REIFY_VERIFY_RETRY_*` envs) — correct for the merge gate, where `merge_shadow.py` already holds a rich per-test map from the same attempt lineage. `offline_lane.py`'s `_default_confirmation_run` instead sends a **bare two-flag argv, no filter file, no envs beyond the role** — DF chose not to build a second per-test-parsing/filter-writing path for the offline lane, and instead wrote a parser (`_parse_confirmed_failures`) that expects reify to self-discover *and report back* the failing set directly as its output. `--confirm-failed` therefore shifts subset-**construction** ownership to reify while reusing `retry_failed_only`'s subset-**consumption** primitive — and, newly significant here, also owns **result reporting** in a shape `retry_failed_only` never had to (that mechanism reports through DF's own per-test parse of a normal verify run; `--confirm-failed` must report through its *own* process's stdout, because that is the only channel `_default_confirmation_run` reads).

**Why can't nextest itself enumerate "the tests that just failed"?** `verify-retry-failed-only.md` §3.1 empirically falsified nextest's experimental record/`-R` round-trip as non-operational in the installed `cargo-nextest 0.9.136`. Inherited unchanged here (§3.1) — not re-derived.

---

## §3 — Substrate (G3 — re-verified live at author time against `main@9ee917c112` (reify) and `main@e534de4553` (dark-factory), `cargo-nextest 0.9.136`)

| Capability | Status | Evidence |
|---|---|---|
| `--confirm-failed` unrecognized on reify | **confirmed absent** | `verify.sh:508-511` `*)` arm → `ERROR — unknown argument`, `usage()` (`verify.sh:334-336`, prints lines 2-59), exit 64. |
| DF's `_default_confirmation_run` already calls it | **confirmed live** | `offline_lane.py:981` `argv = [_RUN_OFFLINE_DEEP_SCRIPT, '--test-threads=1', '--confirm-failed']`; `:987-989` `stdout=PIPE, stderr=STDOUT` (merged); `:990-992` return is `_parse_confirmed_failures(text)` — **`proc.returncode` is never read**. |
| DF's output parser contract | **confirmed live** | `offline_lane.py:132-159` `_parse_confirmed_failures`: splits `text` on lines, strips each, drops blanks, returns the rest verbatim as the confirmed-still-failing id list — **unless** any line matches `_VERIFY_USAGE_MARKER_RE` (`:129`, a whole line exactly `Usage:`/`Options:`, or a line starting `verify.sh: ERROR`), in which case the **entire** output is rejected to `[]` (task 5308 guard). |
| DF's red-path consumption of the (possibly empty) list | **confirmed live** | `offline_lane.py:618-627` (`_handle_red_run`): empty ⇒ log "intermittent nondeterminism," return, **no task, no escalation**; non-empty ⇒ `compute_failing_test_set_fingerprint` → update-or-file a `pending` fix task → `escalate_info` (`:629-635`). Both call sites and the fingerprint/dedup/file machinery are already landed (task 1954/β3). |
| Numeric recording call → confirm call sequencing | **confirmed live, same worktree, immediate** | `offline_lane.py:502-516` `_run_once`: `wt = reset_persistent_offline_deep_worktree(head)`; `rc, _ = suite_runner(wt, head, threads)` (the *recording* call, plain `run-offline-deep.sh --test-threads=N`, no `--confirm-failed`); `if rc != 0: await self._handle_red_run(wt, head)` → invokes `confirmation_runner(wt, head)` = `_default_confirmation_run` on the **same** `wt`/`head`, synchronously, before anything else can touch this single-consumer, single-flight worktree. Tree-drift between recording and confirm is therefore a defensive edge case, not a normal-path concern (§5, §6). |
| `emit_nextest_pass` single filterset-construction site | **exists** | `verify.sh:1313` (function), exact-match `-E 'test(=id) | …'` builder at `:1429-1469`, composed with any active heavy-filter via `&` intersection |
| `REIFY_VERIFY_RETRY_*` consumption pipeline end-to-end | **exists** | env-var contract `verify.sh:148-186`; tree-pin eligibility precompute + loud drift fallback `:1563-1584`; per-profile filter-file precedence + loud no-subset/too-large fallback `:1362-1416`; size ceiling `_RETRY_MAX_SUBSET` (default 5000) `:687` |
| Attempt-0 tree-OID sidecar | **exists, but merge-role-gated only** | `verify.sh:1663-1664`: stamped **only** when `DF_VERIFY_ROLE = "merge"`. Never fires for `DF_VERIFY_ROLE=offline` — only the *pattern* is reusable, not the file. |
| Plan executor: verbose per-command tracing + stop on first non-zero | **exists — directly conflicts with the stdout-purity requirement below** | `verify.sh:2278-2316`: every command is echoed (`echo "verify.sh: + $_cmd" >&2`, `:2293`) before running, and a failing command exits immediately. Both properties are wrong for `--confirm-failed`'s output contract (§4, §5) — the echo line alone would already corrupt `_parse_confirmed_failures`. |
| `gen-nextest-config.sh` per-run full-copy + anchored sed | **exists** | Copies `.config/nextest.toml` verbatim, rewrites only the `occt max-threads` literal (`:132-140`); the seam for adding a `[profile.default.junit]` block. |
| `.config/nextest.toml` JUnit config | **absent → new** | No `[profile.*.junit]` table exists today. |
| nextest 0.9.136 JUnit: CLI flag | **confirmed absent (empirical)** | `cargo nextest run --help` (via the `env cargo` skim-hook bypass — CLAUDE.md's cargo/rustc condensation-wrapper gotcha) has no `--junit` option; only experimental `--message-format {human,libtest-json,libtest-json-plus}`. JUnit is config-file-only. |
| nextest 0.9.136 JUnit: config key + resolved path | **verified live** | `[profile.default.junit]\npath = "probe-junit.xml"` in a scratch config copy, run via `cargo nextest run -p reify-config --config-file <copy>`, produced `target/nextest/default/probe-junit.xml`. |
| JUnit `testcase/@name` **==** the exact-match `test(=<id>)` filterset target | **verified live, both directions** | `-E 'test(=cache::tests::default_cache_dir_treats_empty_xdg_as_unset)'` (bare name) ran exactly 1 test; `-E 'test(=reify-config::cache::tests::…)'` (package-prefixed) matched **0** ("no tests to run"). Manifest-writer needs zero transformation. |
| Failed vs. passed `<testcase>` JUnit shape | **verified live** | Scratch 2-test crate: passing = empty `<testcase/>`; failing carries `<failure message="…" type="…">…</failure>`. |
| nextest process exit codes | **verified live** | No filter, 1/3 failing: exit **100** — a "clean, JUnit-complete" failure. Any other exit code (outer `timeout --kill-after=60`'s 124, a signal, OOM) means the JUnit report cannot be trusted as complete. |
| nextest default fail-fast behavior | **verified live — a real, pre-existing under-capture bug for the offline role** | 3 tests, `--test-threads=1`, no `--no-fail-fast`: only **1/3 tests run** ("2/3 tests were not run due to test failure"), not even attributed as "skipped." `emit_nextest_pass`'s `cmd=` (`verify.sh:1472`) carries no fail-fast flag at all today, so it inherits nextest's fail-fast default. **`DF_VERIFY_ROLE=offline` recording passes already under-capture failures today**, independent of this PRD. |
| `--test-threads=N` (task 5264) composability | **exists, orthogonal** | `verify.sh` usage `:45-52`. DF's own confirm call already passes `--test-threads=1` (§1) — the "isolated/serial" requirement `offline-deep-test-lane.md` §7.1 names is **already satisfied by the caller**, no reify-side coupling needed. |
| Exit-code reuse hazard (cautionary precedent) | **exists** | `docs/prds/test-run-concurrency-semaphore.md:6,22`: exit 75 was *assumed* to trigger a DF requeue and did not — `_classify_failure` fell it through to `unknown_test_failure`/BLOCKED, needing a premise-correction PRD. Informs §6: never assume a *new* exit code is inspected by a specific caller without checking — and here, checking shows `_default_confirmation_run` inspects **stdout content, not exit code, at all** (above). |
| `run-offline-deep.sh` forwarding | **exists, already forward-compatible** | `run-offline-deep.sh:24,56`: forwards trailing args verbatim to `verify.sh test`. Zero changes needed. |

### §3.1 — Inherited: nextest record/`-R` remains non-operational (cross-reference, not re-derived)

`verify-retry-failed-only.md` §3.1's falsification applies verbatim — same tool, same version, same worktree. Not re-verified here.

---

## §4 — The confirm contract (H component — what crosses the seam)

### §4.1 — Recording run (today's existing pass, plus two additions)

Unchanged at the call site: `DF_VERIFY_ROLE=offline scripts/run-offline-deep.sh --test-threads=N` (exactly `offline_lane.py`'s `_default_run_suite`, already landed, unmodified). Two internal additions:

1. **JUnit capture, unconditional.** `gen-nextest-config.sh`'s generated per-run copy always appends `[profile.default.junit]\npath = "reify-confirm.xml"` (resolves to `target/nextest/<profile>/reify-confirm.xml`, §3). Unconditional (every role), not gated behind a new flag — cheap, structural, keeps `--confirm-failed` usable from any role, and (newly relevant) the *confirm* run reuses the identical capture+extraction logic (§4.2), so having only one code path for "derive the failed set from a JUnit file" is itself a soundness win, not just convenience.
2. **Manifest-write step, fused *inline* into the same PLAN command `emit_nextest_pass` builds** — forced by the executor's stop-on-first-failure semantics (§3: a recording pass *with* failures exits 100, and the executor stops there; a later, separate `add "…"` manifest-writer — mirroring the existing merge-only sidecar's "final line, full-pass-only" placement, `verify.sh:1649-1665` — would never run on exactly the runs that need it). Shape (illustrative, not literal code): run nextest, capture `rc=$?`; **iff** `rc` is 0 or 100 (§3's clean-vs-unclean distinction) **and `--no-fail-fast` (item 3, below) was actually active for this invocation**, extract every `<testcase>` with a `<failure>`/`<error>` child's bare `name` attribute (one per line, deduplicated) into the profile's confirm-manifest path under `target/`, stamp a confirm-owned tree-OID sidecar (same JSON shape as the existing attempt-0 sidecar, distinct path — §6.4); on any other `rc`, **or when `--no-fail-fast` was not active for this invocation**, write nothing — leaving any pre-existing manifest for this profile untouched — and re-exit with the original `rc`. **This second condition is the structural fix for a coherence gap surfaced in review:** manifest completeness (§5.3) depends on `--no-fail-fast`, which is scoped to the offline role only (item 3, below; §6.3), while `--confirm-failed` itself is deliberately not role-gated (§6.7) — and the 0-or-100 exit-code test alone cannot detect fail-fast truncation, since a fail-fast pass that stops after its first failure still exits 100, indistinguishable by exit code from a genuinely complete single-failure run (§3). Gating the *write* on `--no-fail-fast` having actually been active — rather than relying on the convention that only the offline role ever reaches this code — means a fail-fast (non-offline) recording pass writes **no manifest at all**, so a later `--confirm-failed` degrades to the safe B5 vacuous/absent-manifest path (§9.1) instead of silently trusting a truncated failed-set. Leaving a pre-existing manifest untouched rather than deleting it is intentional: an earlier no-fail-fast recording may already have produced a complete, tree-pinned manifest for the *same* (unchanged) tree, and a later fail-fast pass on that tree should not discard still-valid state; the tree-pin check (§4.2 step 2) is what guards against any staleness this could otherwise introduce. This step is **entirely internal to the recording pass's own PLAN command** and does not touch that pass's own stdout/console output — the stdout-purity constraint (§4.2) applies only to the *confirm* run, not the recording run, since only `_default_confirmation_run` (not `_default_run_suite`) does line-by-line stdout parsing (§3).
3. **Prerequisite: the offline role's nextest invocation gains `--no-fail-fast`.** Empirically necessary (§3, §5) — without it, later-scheduled tests after an early failure are not even attributed as "skipped," they simply never run, so the manifest silently under-captures. Scoped to `DF_VERIFY_ROLE=offline` only, joining its existing single-profile/heavy-filter/idle-priority role-scoped defaults — not a global change to task/merge/background's intentional fail-fast posture. This is independently correct even without `--confirm-failed`: `offline-deep-test-lane.md` §7.2's dedup fingerprint already needs the complete failed set, and today's fail-fast recording pass cannot reliably supply one.

### §4.2 — Confirm run (`verify.sh test --confirm-failed`) — the output-contract-first mechanism

New boolean flag → `CONFIRM_FAILED=1`, parsed alongside `--narrow`/`--include-infra`/`--print-plan` (`verify.sh:496-501` convention). Valid only for `action ∈ {test, all}` (exit 64 otherwise, mirroring the existing strict-validation style).

**Central constraint (new, load-bearing, discovered by reading the live consumer — §1/§3): `--confirm-failed`'s combined stdout+stderr must contain *only* a newline-delimited list of confirmed-still-failing bare test IDs, or nothing at all.** No executor command echoes, no nextest console/progress chatter, no PSI/compile-gate diagnostics, no honest markers — `_parse_confirmed_failures` treats every non-blank line as one confirmed test ID (§3). This rules out simply running `--confirm-failed` through the normal verbose PLAN-array executor (`verify.sh:2278-2316`, which unconditionally echoes `verify.sh: + $_cmd`) — `--confirm-failed` needs its **own, narrow, dedicated code path**, not a variant PLAN entry, structured as:

1. Resolve the confirm-manifest + confirm-sidecar paths for the requested profile(s) (mirroring `_ATTEMPT_SIDECAR_PATH`'s override-env convention, distinct path — §6.4).
2. Tree-pin check: current `git rev-parse HEAD:` against the confirm-sidecar's stamped OID — reuses the *existing* precompute logic at `verify.sh:1563-1584` (§6.1: self-drive the same `REIFY_VERIFY_RETRY_*` inputs the merge-gate path already consumes, rather than new logic).
3. If the manifest is absent/empty/unreadable, or the resolved subset is empty → print **nothing**, exit 0 (§4.3 row 1). This already matches `_parse_confirmed_failures`'s `[]` outcome exactly — no special marker is needed for this case.
4. If the tree has drifted → print a line beginning `verify.sh: ERROR — confirm refused: tree drift …` and exit 64. This deliberately **reuses the exact, already-existing `verify.sh: ERROR` banner convention** (`verify.sh:509` etc.) specifically *because* `_VERIFY_USAGE_MARKER_RE` (§3) already treats any such line as "reject the whole output to `[]`" — **zero DF-side change is required** for this case to degrade safely into DF's existing "no confirmed failures" path. (§6.8 discusses the known limitation this implies, and names a DF-side follow-up.)
5. Otherwise, run nextest against exactly the manifest's subset (reusing the self-driven `REIFY_VERIFY_RETRY_*` pipeline — §6.1, `emit_nextest_pass`'s existing filterset builder, tree-pin guard, size ceiling, all verbatim) **with its own stdout/stderr redirected to a private log under `target/`, not to `verify.sh`'s own stdout** — and with the SAME JUnit capture mechanism as §4.1. After the pass completes, derive the confirmed-still-failing set from *this run's own* JUnit file (identical extraction logic to §4.1's manifest-writer — one shared helper, not two implementations) and print exactly those bare test names, one per line, to stdout. Exit 0 if the set is empty (all now pass) — note this is a **second** path to the same "print nothing, exit 0" observable as step 3, semantically distinct (genuinely confirmed clean vs. nothing was ever recorded) but intentionally **not** distinguished on the wire, since `_default_confirmation_run` cannot tell them apart either way (§6.2) — or exit 100 if the set is non-empty (still-failing IDs were printed).

Composes orthogonally with `--test-threads=N` (task 5264) exactly as DF's existing call already does (`--test-threads=1`, §1, §3) — no new coupling code.

### §4.3 — Contracts (output-first, exit-code second)

**Primary — stdout+stderr content (what `_default_confirmation_run` actually reads):**

| Scenario | Manifest state | stdout+stderr content | `_parse_confirmed_failures` result |
|---|---|---|---|
| No prior recording, or recording found 0 failures, or recording didn't exit cleanly | absent/empty/unreadable | *(nothing)* | `[]` — DF logs intermittent-nondeterminism, no task |
| Recorded N≥1; tree matches; all N now pass | N ids, tree-pinned | *(nothing)* | `[]` — same as above (§6.2: deliberately indistinguishable from the vacuous case, on the wire, from a bare-list contract) |
| Recorded N≥1; tree matches; M≥1 of N still fail | N ids, tree-pinned | exactly the M still-failing bare test names, one per line | `[test_a, test_b, …]` — DF fingerprints, dedups, files/updates a `pending` fix task, `escalate_info` |
| Recorded N≥1; tree does **not** match current HEAD tree | stale | one line: `verify.sh: ERROR — confirm refused: tree drift — …` | `[]` (caught by the pre-existing task-5308 guard, §4.2 step 4) — a known, accepted limitation (§6.8) |

**Secondary — process exit code (for the boundary test, operator/manual use, and possible future DF enhancement — §11; not currently read by `_default_confirmation_run`):** 0 for the first three "nothing printed" / "clean" outcomes' underlying success paths, nextest's own 100 when the still-failing list is non-empty, 64 for the tree-drift `ERROR` line (matching the existing arg-error convention's own exit code) and for a malformed invocation (e.g. `--confirm-failed` with action ∉ {test, all}).

### §4.4 — No honest marker (deliberate divergence from `retry_failed_only`)

`retry_failed_only` emits `@@REIFY_RETRY_SCOPE@@` (§4.4 of that PRD) so DF's *mining* never mistakes a narrowed retry for a full green gate. `--confirm-failed` emits **no** analogous marker: any additional line would corrupt the stdout-purity contract (§4.2), and it would be redundant — `offline_lane.py` already logs its own pass/fail/duration independently on the DF side (`:510-513`, `logger.info('offline-lane: numeric sub-run head=%s status=%s duration=%.1fs', …)`), unaffected by anything `verify.sh` prints. This is a considered omission, not an oversight.

---

## §5 — Soundness invariants (encode as PRD-level, tested at the seam)

1. **Stdout purity (new, primary).** `--confirm-failed`'s combined stdout+stderr contains only the derived id list, an `ERROR —` line, or nothing — never executor echoes, nextest console chatter, or diagnostic prose. Violating this silently corrupts an already-deployed parser with garbage "confirmed" test IDs, which could file a fix task against a nonsense name. This is the single most safety-critical invariant in this PRD, since it protects a live consumer, not a hypothetical one.
2. **Tree-pinned.** Reuses the existing precompute (`verify.sh:1563-1584`) against a confirm-owned sidecar. In the normal call path (§3) recording and confirm run back-to-back in the same single-consumer worktree, so drift is a defensive edge case, not a common path.
3. **Complete failed-set via `--no-fail-fast`, now enforced at the write gate, not merely by convention.** Verified necessary, not merely prudent (§3, §4.1.3) — without it, a recording pass with early failures silently omits later tests from the manifest. Because `--no-fail-fast` is scoped to the offline role (§4.1.3, §6.3) while `--confirm-failed` itself is not role-gated (§6.7), the manifest-write step (§4.1.2) additionally checks that `--no-fail-fast` was actually active before writing — a fail-fast recording pass writes no manifest at all, rather than an under-captured one, so this invariant cannot be silently violated merely because a non-offline caller happens to reach the same code path.
4. **JUnit-complete-or-nothing.** The manifest (and, symmetrically, the confirm run's own derivation) is trusted only when the underlying nextest pass's exit code is 0 or 100 (§3) **and** — for the manifest-write side specifically — invariant 3's `--no-fail-fast`-active gate also holds; the exit-code test alone is not sufficient, since a fail-fast pass truncated after its first failure can still exit 100, indistinguishable by exit code from a genuinely complete single-failure run.
5. **Exact-match only, deduplicated.** Manifest/derived-list lines are `testcase/@name` verbatim, never package/binary-prefixed (verified not to match, §3). A same-name collision across binaries is a pre-existing ambiguity inherited from `retry_failed_only`'s own `test(=<id>)` convention, not introduced here.
6. **Loud where it is safe to be loud, silent where the consumer requires it.** The tree-drift case is intentionally routed through the pre-existing `verify.sh: ERROR` convention specifically because that is the one form of "loud" the live consumer already safely discards (§4.2 step 4) — a genuinely new, unrecognized loud marker would instead corrupt it (invariant 1). This is the one place this PRD's "loud never silent" instinct is deliberately bent to fit an already-deployed, unchangeable-by-this-task consumer; §6.8/§11 name the honest limitation.
7. **Byte-identical when inactive.** `--confirm-failed` unset ⇒ no new env resolution, no manifest read, plan identical to today. JUnit capture (§4.1.1) is the one unconditional addition and is a pure side-channel file write, invisible to the normal plan/output.

---

## §6 — Resolved design decisions

1. **`--confirm-failed` self-drives the existing `REIFY_VERIFY_RETRY_*` pipeline for subset *selection*, rather than inventing a parallel one.** Internally resolves the same env-equivalents (`SCOPE=failed_only`, per-profile filter-file path, tree OID, sidecar path) DF would set for `retry_failed_only`, pointed at self-written state (§4.2 step 2). Zero new *consumption* code for "which tests run."
2. **A genuinely-confirmed-clean outcome and a nothing-was-recorded outcome are deliberately indistinguishable on the wire.** Both print nothing and both parse to DF's `[]`. Distinguishing them would require a channel `_default_confirmation_run` does not read (it only inspects `stdout`, never `proc.returncode`, §1/§3) — so building that distinction now would add reify-side complexity with no consumer able to observe it. If DF later wants the distinction, it is a joint reify+DF follow-up (§11), not something reify can unilaterally surface.
3. **The recording run's `--no-fail-fast` is a discovered prerequisite, scoped to the `offline` role, not a new global default.** Necessary for manifest completeness (§5.3); scoping avoids changing merge/task/background's intentional fail-fast posture. The manifest-write step (§4.1.2) enforces this scoping structurally, not just conventionally — it checks `--no-fail-fast` was actually active before writing, rather than assuming only the offline role ever reaches this code (§6.7).
4. **The confirm-owned sidecar and manifest are new, distinct paths under `target/`, not the existing `reify-verify-attempt.json`.** That file is merge-role-gated and never written for `DF_VERIFY_ROLE=offline` (§3) — only its *pattern* is reused.
5. **The manifest-write step (recording) is fused inline into the same PLAN command as the nextest pass; the confirm run bypasses the PLAN-array executor entirely.** Two different mechanisms for two different reasons: the recording-run fix is forced by stop-on-first-failure sequencing (§4.1.2); the confirm-run fix is forced by stdout purity (§4.2, §5.1) — the executor's own per-command echo is itself a contamination source, not merely a sequencing hazard.
6. **`--test-threads=1`/"isolated-serial" needs no reify-side coupling.** DF's already-deployed call already supplies `--test-threads=1` (§1) — `offline-deep-test-lane.md` §7.1's requirement is satisfied entirely by the caller, composing with the pre-existing, orthogonal `--test-threads=N` flag (task 5264).
7. **`--confirm-failed` is not role-gated — soundness instead comes from the write side, not a read-side check.** Any caller may invoke the flag; DF's offline lane is simply its first and, at present, only real caller. A non-offline caller's recording pass simply never produces a usable manifest (the §4.1.2 write-gate skips the write whenever `--no-fail-fast` wasn't active), so a following `--confirm-failed` lands on the safe B5 vacuous path (§9.1) with no special-casing needed on the confirm side itself.
8. **Tree-drift is reported via the pre-existing `verify.sh: ERROR —` banner, not a novel sentinel exit code, *because* the live consumer's parser already special-cases that exact banner and nothing else.** This is a reversal of this PRD's own earlier draft instinct (a fresh, distinct non-zero exit code) once the live consumer's actual code was read: `_default_confirmation_run` never inspects `proc.returncode` (§1/§3), so a new exit code would be invisible to it — while the `verify.sh: ERROR` banner is *already* on its reject-to-`[]` list (task 5308), giving zero-DF-change-required safe degradation today. **Known, accepted limitation:** this collapses "cannot confirm (tree drift)" and "genuinely confirmed clean (flake)" into the same DF-observed outcome (`[]`, logged as intermittent nondeterminism) — acceptable because (a) tree drift is a defensive edge case in the normal same-worktree/same-head call sequence (§3), and (b) it is strictly better than today's status quo, where *every* confirmation call collapses to `[]` unconditionally (§1/§2). A richer, three-state DF-side contract is named as a follow-up, not solved here (§11).
9. **A partial confirm's per-test detail is exactly the printed id list — no separate structured channel.** Unlike `retry_failed_only` (where DF's own `parse_per_test_results` reads a normal verify run's output), `--confirm-failed` *is* the structured channel: its whole design is organized around producing precisely the list `_parse_confirmed_failures` already knows how to consume.
10. **No honest marker is emitted for `--confirm-failed` (§4.4).** Diverges from `retry_failed_only`'s convention deliberately — see rationale there.

---

## §7 — Out of scope

- Any change to `offline_lane.py`, `_parse_confirmed_failures`, `ConfirmationRunner`, or any other DF-side code — all of it is **already landed** and is treated here as a fixed, read-only contract this PRD designs *against*, not a co-evolving surface. (This is a stronger form of "out of scope" than a typical cross-repo PRD, precisely because the DF side is finished, not merely planned.)
- The DF-side three-state-contract enhancement named in §6.8/§11 — a real, identified possible improvement, explicitly **not** designed here; it would need its own (joint) proposal.
- Any change to the merge/task/background roles' fail-fast behavior.
- `retry_failed_only`'s own contract, tables, or mechanism — cross-referenced throughout, never restated (G7).
- `_default_infra_confirmation_run` / `_default_confirm_command` and their mechanisms (§1b/c) — unrelated seams, not touched.
- Any new capability-manifest / decompose-time artifact — generated at `/prd decompose` time, not authoring time (this task is design-only).

---

## §8 — Cross-repo / cross-PRD seam ownership (G4)

**reify owns (ships the primitive) — the only outstanding work in this seam:**

| Leaf | Mechanism |
|---|---|
| α | Offline role gains `--no-fail-fast`; `gen-nextest-config.sh`'s generated copy gains unconditional `[profile.default.junit]` |
| β | Recording-run manifest-writer: inline-fused JUnit→exact-ID extraction + confirm-owned tree-OID sidecar stamp (§4.1) |
| γ | `--confirm-failed` flag + dedicated (non-PLAN-executor) code path: self-driven `REIFY_VERIFY_RETRY_*` subset selection, stdout-pure id-list/ERROR-line output, exit codes (§4.2-4.3) |
| δ | reify boundary test (`tests/infra/test_verify_confirm_failed.sh`) exercising §9.1, asserting stdout purity byte-for-byte against fixtures, + same-diff drift-guard registrations |
| ε | Operator-observable e2e demo, ideally run **against a real (or faithfully mocked) `_default_confirmation_run` call shape** — see §9.2 |

**DF owns — already shipped, nothing new to file:**

`offline_lane.py`'s `_default_confirmation_run`, `_parse_confirmed_failures`, `_handle_red_run`, the fingerprint/dedup/file/escalate machinery (β3, task 1954), and the recording→confirm sequencing (β2, task 1953) are **all already on `dark-factory main`**. This PRD's α-ε leaves are sufficient, alone, to make β3 start firing for real — no DF-side task is required to close the loop, a materially simpler seam than a typical "reify ships, DF wires" pattern (§1). The **one** DF-side item this PRD identifies but does not scope is the optional §6.8/§11 three-state-contract enhancement, which is genuinely new DF-side work if ever pursued — named as a follow-up candidate (§10), not a dependency of α-ε.

**Seam table:**

| Seam | Direction | Mechanism | Owner | Status |
|---|---|---|---|---|
| Recording-run JUnit capture + manifest | reify internal | α, β | reify | designed here |
| `--confirm-failed` output contract | reify → DF (already-deployed parser) | γ, matching `_parse_confirmed_failures` exactly | reify (γ) — DF side is a **fixed target**, not a co-deliverable | designed here |
| Two-call sequence | DF → reify CLI | `_default_run_suite` + `_default_confirmation_run`, `offline_lane.py:925-992` | dark-factory | **already shipped** |
| Failure-handling pipeline | DF (already consumes, currently starved of real input) | `_handle_red_run`, `offline_lane.py:580-635` | dark-factory | **already shipped, dormant pending this PRD's leaves landing** |
| shared consumption primitive | this PRD reuses ← produced by | `emit_nextest_pass` / `REIFY_VERIFY_RETRY_*` | `verify-retry-failed-only` (already landed, task 5287) | **exists** |
| three-state contract (empty / confirmed-clean / cannot-confirm) | possible future DF enhancement | not designed here | dark-factory (hypothetical) | **not queued — §11** |

**Reciprocal-ownership check:** `offline-deep-test-lane-worker.md` (DF's own decomposition) names its β2 dependency as cross-project precondition `reify:A5` (the runner, `run-offline-deep.sh`, already landed) and refers to the still-missing confirm-flag work only informally, in code comments, as "ζ's job" — DF's PRD does not itself claim ownership of the confirm-flag's implementation. No contested claim; this PRD is the reify-local leaf DF's own documentation is waiting on.

---

## §9 — Boundary-test sketch (two-way; faces both producer and consumer)

### §9.1 — reify side (verify.sh looks *inward* at what it recorded and reports)

| Scenario | Precondition | Postcondition |
|---|---|---|
| B1 recording captures complete failed set | scratch fixture, 3 tests, 2 failing, offline role's `--no-fail-fast` | manifest contains exactly 2 exact IDs; JUnit `failures="2"` |
| B2 confirm reports exactly the still-failing subset | manifest = 2 IDs, sidecar OID matches, both still fail | stdout is exactly 2 lines, each a bare test name, nothing else — byte-for-byte, no executor echoes, no nextest banner |
| B3 confirm reports partial reproduction | of the 2, 1 now passes | stdout is exactly 1 line (the still-failing one) |
| B4 confirm reports full reproduction-clear | both of the 2 now pass | stdout is **empty** (zero bytes, or whitespace-only) |
| B5 vacuous, absent manifest | no prior recording run for this profile | stdout **empty**; exit 0 |
| B6 vacuous, empty manifest | recording run found 0 failures | stdout **empty**; exit 0 (same observable as B5, deliberately, §6.2) |
| B7 tree-drift refusal | sidecar tree_oid ≠ current HEAD tree | stdout is exactly one line matching `^verify\.sh: ERROR\b` — the exact regex DF's live guard checks; exit 64 |
| B8 unclean recording produces no manifest | recording pass killed (simulated timeout) mid-run | no manifest written; a later confirm behaves exactly as B5 |
| B9 stdout-purity regression guard | any confirm invocation, pass or fail | assert **zero** lines in the captured output fail to match either "a bare exact-match-able test name" or the B7 ERROR-line pattern — this is the invariant-1 (§5) regression test, and the single highest-value assertion in δ |
| B10 byte-identical when inactive | `--confirm-failed` unset | `--print-plan` output identical to pre-this-PRD baseline except the added (unconditional) JUnit config line |
| B11 fail-fast recording writes no manifest (write-gate) | scratch fixture, 3 tests, 2 failing, `--no-fail-fast` **not** active (default fail-fast posture) | recording pass exits 100 as usual, but no manifest is written for this profile; a pre-existing manifest for the same tree OID (if any) is left untouched, not overwritten — proves the §4.1.2 write-gate discriminates on fail-fast activity, not on exit code alone |

### §9.2 — DF side (already-shipped code, exercised as a fixed consumer — not new DF work)

These are **not** new DF leaves (§8) — they are what δ/ε should replay against a faithful reproduction of the already-landed consumer, to prove the two sides actually fit together:

| Scenario | Precondition | Postcondition |
|---|---|---|
| C1 `_parse_confirmed_failures` accepts a real confirm output | feed a real B3-shaped stdout capture through DF's actual (or a byte-identical vendored copy of) `_parse_confirmed_failures` | returns exactly the still-failing id list, no garbage entries |
| C2 `_parse_confirmed_failures` safely discards a tree-drift ERROR line | feed a real B7-shaped stdout capture through the same parser | returns `[]` |
| C3 `_handle_red_run` fires end-to-end | a real (or faithfully mocked) confirmed-still-failing list reaches `_handle_red_run` | a `pending` fix task + `escalate_info` appear, exactly per β3's own already-written signal (`offline-deep-test-lane-worker.md:254-258`) |

---

## §10 — Decomposition plan (one bullet = one leaf; signals sketched, finalized at decompose)

reify leaves filed here (`project_root=/home/leo/src/reify`); no DF leaves are required to close this seam (§8). `scripts/verify.sh` / `gen-nextest-config.sh` touch the verify pipeline → full `--scope all` gate.

**reify:**
- **α — offline-role `--no-fail-fast` + unconditional JUnit capture.** `metadata.files: ["scripts/verify.sh", "scripts/gen-nextest-config.sh"]`. Signal: `DF_VERIFY_ROLE=offline verify.sh test --print-plan` shows `--no-fail-fast`; a fixture run with an early failure still runs later tests and produces a complete JUnit file.
- **β — recording-run manifest-writer.** `metadata.files: ["scripts/verify.sh"]`. Signal: a scratch fixture with N failures, `--no-fail-fast` active, produces a manifest with exactly N exact IDs and a tree-pinned sidecar; the manifest is written even though the recording pass's own exit code is 100. Negative signal (the write-gate, §4.1.2): the identical fixture run *without* `--no-fail-fast` active — same exit code 100 — writes no manifest, and leaves any pre-existing manifest for that profile untouched, proving the gate discriminates on fail-fast activity rather than exit code alone. Depends on α.
- **γ — `--confirm-failed` flag + dedicated stdout-pure code path.** `metadata.files: ["scripts/verify.sh"]`. Signal: given β's manifest, `--confirm-failed --test-threads=1` (DF's exact argv, §1) prints exactly the still-failing subset as a bare newline list and nothing else; an absent/empty manifest prints nothing; a mutated-tree fixture prints exactly one `verify.sh: ERROR —` line. Depends on β.
- **δ — reify boundary test + same-diff drift-guard registrations (INTEGRATION-GATE, reify terminal).** Adds `tests/infra/test_verify_confirm_failed.sh` exercising §9.1 (B1-B11, with B9's stdout-purity assertion and B11's write-gate assertion as the headline checks) **and** its classification-manifest + wallclock-upper-bounds registrations in the same diff. Depends on α, β, γ.
- **ε — operator-observable e2e demo (operational).** Deliberately fail a scoped offline-role test; run the exact DF call shape (`run-offline-deep.sh --test-threads=N` then `run-offline-deep.sh --test-threads=1 --confirm-failed`); observe the confirm call's stdout is exactly the failing test's bare name; fix the test; re-run confirm, observe empty stdout. If feasible at decompose, additionally drive `offline_lane.py`'s real `_default_confirmation_run`/`_handle_red_run` against a reify checkout carrying γ (§9.2 C1-C3) — the strongest possible signal that the seam actually closes, not just that each side is independently plausible. Depends on δ.

**Follow-up candidate (not a dependency of α-ε, not filed by this task):**
- A DF-side three-state `ConfirmationRunner` contract (§6.8/§11) — would need a joint reify+DF proposal; recorded here so it is not lost, not scoped or filed.

**Dependency edges to wire at decompose:** α → β → γ → δ → ε. No cross-project edge is required for α-ε to be complete and useful (§8) — a first for this PRD's cross-repo pattern.

---

## §11 — Open (tactical) questions

- **The three-state contract (§6.2/§6.8/§8/§10 follow-up).** `_default_confirmation_run`'s `list[str]` return type cannot currently distinguish "confirmed clean" from "cannot confirm" from "nothing was ever recorded" — all three collapse to `[]` today, an accepted-for-now limitation (§6.8) that is strictly better than the current status quo (§1/§2) but not ideal. Revisit if tree-drift or manifest-absence turn out to be more common in practice than §3's analysis (same-worktree, same-head, immediate sequencing) predicts.
- **Simultaneous `--confirm-failed` + externally-set `REIFY_VERIFY_RETRY_SCOPE`.** Not resolved here whether this should be a hard validation error (exit 64) or a defined precedence — recommend erroring loudly at decompose (ambiguous double-drive of one consumption pipeline), left to γ's implementation. Not currently reachable by DF's own call shape (§1 sets no such env), so low urgency.
- **JUnit `name`-attribute stability as a regression-guarded contract.** §3's empirical mapping was a one-time live probe this session, not (yet) an automated check. δ's B1-B3 exercise the real mapping end-to-end and function as the regression guard; consider a dedicated, minimal drift test if nextest version bumps become routine.
- **`gen-nextest-config.sh`'s JUnit output path collision across concurrent verify.sh processes.** The generated *config* temp file is per-process (`mktemp`); the JUnit *output* path (`target/nextest/<profile>/reify-confirm.xml`) is fixed, not per-process. The offline lane's dedicated single-consumer worktree (§3, `offline-deep-test-lane.md` §8) and the test-run semaphore bound this in practice — likely a non-issue, but worth a `--profile`-qualified filename at implementation time if any doubt remains.
- **Where nextest's own console output goes during a confirm run (§4.2 step 5).** This PRD specifies "a private log under `target/`, not verify.sh's own stdout" without pinning the exact path/rotation/cleanup policy — left to γ's implementation; any reasonable choice satisfies the stdout-purity invariant (§5.1) as long as it is *not* the stream `--confirm-failed` returns to its caller.
- **Whether a `--profile both` recording needs both per-profile manifests before a same-profile-scoped `--confirm-failed` is meaningful.** The offline role defaults to `release`-only (`verify.sh:550-554`) and DF's own call never passes `--profile`, so this is not on the critical path; recommend allowing a confirm to consult only the profile(s) requested, consistent with `retry_failed_only`'s own independent per-profile resolution, but not fully specified here.
