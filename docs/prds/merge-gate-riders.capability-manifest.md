# Capability manifest — merge-gate-riders (decompose 2026-07-19)

Per-leaf capability→evidence bindings (mechanized G3+G6). All bindings **PASS**; no `declared-only` / `test-only` / `producer-downstream` / `producer-absent` / `fixture-ERROR` / `bound≤floor` / `rejection-absent` findings. Anchors verified against main `fc47d707b3`..`310c1abc1b` at author/decompose time (yaml anchors re-based after config batch `bc235a616a`).

## α — dark_factory:2832 (DF sequential_lint_first knob)

| Capability | Evidence |
|---|---|
| Sequential branch exists, hard-coded test→lint→type order | grep: DF `orchestrator/src/orchestrator/verify.py` sequential branch (`await _run_or_skip_timed(test/lint/type)` ~:3713-3722) — wired on DF main |
| `CheckRun.skipped` constructor exists for the short-circuit shape | grep: DF `verify.py` `CheckRun.skipped(label)` (used for `cmd is None`) |
| Module-config precedent for per-project verify bools | grep: DF `config.py` `concurrent_verify` module override `:1893` / global `:2355` |
| Lint/type legs cannot classify env_transient (preserved invariant) | DF `verify.py` PYTEST-only env_transient narrowing comment (~:3760-3775) |

## β — reify:5271 (yaml flip + seam guard)

| Capability | Evidence |
|---|---|
| `sequential_lint_first` knob | **producer: dark_factory:2832 — upstream dep (wired)** |
| `lint_command` = full-workspace clippy strict-superset, separate process | grep: `dark-factory-orchestrator.yaml` lint_command + three-separate-processes comment (~:138-159) |
| `concurrent_verify: false` | grep: `dark-factory-orchestrator.yaml` (~:166) |
| Sanctioned restart path | `scripts/orchestrator-redeploy-restart.sh` exists (header = contract) |
| Yaml-contract test precedent (Open Q3 host) | `tests/infra/test_release_mode_in_test_command.sh` exists |
| Reify plan shape untouched (K2) | `tests/infra/test_verify_failfast_order.sh`, `test_occt_flock_gate.sh` Tests 17/17b `:260-294` — stay green unmodified |

## γ — reify:5273 (run_all content-addressed skip engine)

| Capability | Evidence |
|---|---|
| Merge-tier run_all plan line + narrow-env idiom | grep: `scripts/verify.sh` `REIFY_AUDIT_NO_COLD_BUILD=1 REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 … run_all.sh` (~:1575-1605) |
| run_all member loop / pool + Phase-2.5 structure | `tests/infra/run_all.sh` (pool phases; ledger default `:549-558`) |
| Inclusion map exists but is UNSOUND for skipping (documented, not reused) | `scripts/verify-pipeline-infra-tests.txt` header ("self-maintaining for additions" — incomplete per-test sets by design) |
| Ambient-vars set-equality guard seam | `tests/infra/run-all-ambient-vars.manifest` + `test_run_all_ambient_isolation.sh` (task 5152) |
| INV-5 current statement (to be restated as INV-5′ same-diff) | `docs/prds/run-all-pool-contention-tiering-fix.md:126`; `tests/infra/test_run_all_tiering.sh:4-6` |
| Content-addressed comparison substrate | git tree diff (`git diff --quiet <sha> HEAD -- <paths>`) — trivial |
| Durable-state env precedent | yaml `REIFY_RUN_ALL_FLAKY_LEDGER` row (landed `bc235a616a`) |
| State path is git-ignored (never tracked) | `run_all.sh` ledger comment (`data/verify-logs/` via `.git/info/exclude`) |

## δ — reify:5276 (rider-2 activation)

| Capability | Evidence |
|---|---|
| Skip engine + state schema | **producer: reify:5273 — upstream dep (wired)** |
| `verify_env` injection seam | grep: `dark-factory-orchestrator.yaml` verify_env block (~:164) |
| Sanctioned restart path | `scripts/orchestrator-redeploy-restart.sh` |
| Wall-baseline for the measured delta | survey: run_all infra ~390s (green run 5163) — measured, not asserted as a bound |

## ε — reify:5279 (release-pass delta-skip mechanism)

| Capability | Evidence |
|---|---|
| Release-pass emission seam | grep: `scripts/verify.sh` `add_test_passes` release branch (~:1218-1252, `_RELEASE_ALL_FLAGS`) |
| Declared release-sensitive set reader | grep: `scripts/release-scope-lib.sh` `release_declared_set` |
| Shared reverse-closure primitive (no duplicate) | grep: `scripts/occt-scope-lib.sh:63` `_reify_compile_closure` (dev-dep-accurate; task 4938 semantics) |
| C4 global / C5 fail-wide conventions | grep: `scripts/affected-crates-lib.sh:42-66` `_is_global` / `_is_noncrate` / ALL fallback |
| Release lib prebuild stays (run_all consumes `target/release/reify`) | grep: `scripts/verify.sh:1575` `cargo build --release -p reify-cli` + sha-stamp guard |
| C2 contract + guard (amended same-diff) | `docs/prds/verify-scope-contract.md:37`; `scripts/verify.sh:608-616`; `tests/infra/test_verify_scope.sh:740-870` (NOT edited — task 5166 owns MG-B5 region) |
| Merge-delta context exists on both merge paths | DF merge lane speculative merge commit (first-parent = main); `hooks/pre-merge-commit` MERGE_HEAD |
| Hermetic delta-override precedent | `REIFY_AFFECTED_CRATES_OVERRIDE` (test_verify_scope MG-* fixtures) |
| New-infra-test registration seam | `tests/infra/run-all-classification.manifest` (row same-diff; gate-test registration rule) |
| Rejection-style assertions (background-never-skips, knob-off-pass-present) | produced RED-first inside ε's own new drift test — the rejection mechanism ships with the capability, same diff |

## ζ — reify:5280 (sweep-gated activation)

| Capability | Evidence |
|---|---|
| Skip mechanism + frozen marker string | **producer: reify:5279 — upstream dep (wired)** |
| Background main-tip sweep exists, full both-profile, never narrows | `scripts/verify.sh:564-567` role-scoped heavy-exclusion; `test_verify_scope.sh` BG-B6b scenarios (task 5210, landed) |
| Sweep-health: honest categorization | **producer: dark_factory:2821 (in-progress, live) — upstream external dep (wired)** |
| Sweep-health: collateral-shape ENV_TRANSIENT extension | **producer: dark_factory:2831 (pending, live) — upstream external dep (wired)** |
| Trailing-window green evidence source | `data/orchestrator/runs.db` (survey's own mining substrate) |
| Coverage claim stated conditional (G6) | PRD §5.2/§5.3 — "nothing lost" holds ONLY with the sweep backstop; ζ escalates instead of activating if alerting is not live |

## Milestone wiring

5254 (program release gate) depends on β(5271), δ(5276), ζ(5280), dark_factory:2832 — wired 2026-07-19. Intra-batch: 5271→dark_factory:2832; 5276→5273; 5280→{5279, dark_factory:2821, dark_factory:2831}.
