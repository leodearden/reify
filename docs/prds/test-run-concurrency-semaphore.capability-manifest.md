# Capability manifest — test-run-concurrency-semaphore

Mechanizes G3 (substrate) + G6 (premise) per leaf. Any FAIL binding blocks queueing.
This is an **infrastructure** PRD (shell + nextest-config); the reify product-signal
forms (field-population `Value::Undef`, grammar fixtures) are mostly N/A — the
operative evidence forms are **wired-on-main** (the mechanism is in the verify
pipeline's execute path, not test-only) and **no-numeric-floor** (config integers,
not accuracy bounds; the one numeric premise is the RSS headroom inequality).

## Per-leaf bindings

### α — held-slot test-run semaphore lib
- **Capability:** an N-slot counting semaphore that holds a slot for a command's full duration, merge-exempt, exit-75 on deadline, FD not leaked to children.
- **Substrate (G3):** `flock`+`timeout` present (`cargo-test-occt-gated.sh:100-109`); `DF_VERIFY_ROLE` present (verify.sh:288). **PASS.**
- **Wired-on-main evidence:** the lib is *consumed* by β in `verify.sh`'s execute path (not test-only). At α's own close the evidence is the behavioral test `tests/infra/test_test_run_semaphore.sh` exercising the real lib (process serialization / FD-non-leak / exit-code — real behavior, not synthetic-input unit assertion). **PASS** (anti-orphan satisfied by β being queued as the consumer with a dep edge).
- **Grammar-fixture:** N/A (no novel syntax). **PASS.**
- **Numeric floor:** N/A (no accuracy bound). **PASS.**

### β — wire into verify.sh + uniform merge exemption
- **Capability:** verify.sh test phase acquires/holds the slot around test passes only; merge (queue + local) exempt; exit-75 propagated.
- **Substrate (G3):** verify.sh `add_test_passes()` execute path exists (verify.sh ~:632+); `hooks/pre-merge-commit:37` is the local merge call site; orchestrator `test_command` already calls `verify.sh test`. **PASS.**
- **Wired-on-main evidence:** grep must show the acquire/release in verify.sh's **execute** branch wrapping the nextest passes (not only in `--print-plan`), and `DF_VERIFY_ROLE=merge` set in `hooks/pre-merge-commit`. Production entry = the verify pipeline the orchestrator runs on every task. **PASS** (verified at decompose against the landed diff).
- **Grammar / numeric:** N/A. **PASS.**

### γ — occt cap 4→24, env-driven
- **Capability:** occt nextest test-group runs at 24 (env-overridable); RSS worst case bounded.
- **Substrate (G3):** `nextest --config 'test-groups.occt.max-threads=N'` accepted by 0.9.136 (verified empirically 2026-06-10); `.config/nextest.toml:20` literal is the edit site. **PASS.**
- **Numeric floor (G6):** the only numeric premise — RSS headroom. Floor = host RAM 125 GiB. Assert worst case `2 runs × 24 × 2 GiB = 96 GiB < 125 GiB`. Empirically backed: the 2026-06-10 measurement observed ~98 GiB free during a single 32-wide run, so a 24-wide × 2-run ceiling is safe with margin. `bound (96) < floor (125)` ⇒ **PASS.** (Note: the "cap raise sometimes helps throughput" claim is an explicit conjecture, NOT asserted as a number by any leaf signal → no false premise frozen into a RED test.)
- **Wired-on-main:** `.config/nextest.toml` + the `--config` flag in verify.sh's emitted plan. **PASS.**
- **Coupling guard:** γ carries a dependency on β (cap raise unsafe until the hard run-bound is live). **PASS.**

### δ — contract surfacing (orchestrator.yaml + CLAUDE.md)
- **Capability:** the semaphore's env/exemption/exit-75 contract is documented; confirms no functional orchestrator change.
- **Substrate (G3):** the contracts being documented already hold (verify.sh:161/228; orchestrator.yaml:31-37). **PASS.**
- **Wired-on-main / grammar / numeric:** docs leaf — evidence is the committed doc diff. **PASS.**

### ε — integration gate (critical leaf)
- **Capability:** end-to-end proof through real `verify.sh` that the composed system bounds task concurrency, exempts merge, propagates exit-75, and runs cap=24 with compiles outside the gate.
- **Substrate (G3):** `tests/infra/run_all.sh` auto-discovers `test_*.sh` and is run by verify.sh. **PASS.**
- **Wired-on-main evidence (anti-fake-done):** the signal is a behavioral e2e test driving the production `scripts/verify.sh` (not the lib in isolation, not synthetic input) — observes real serialization timing, real exit codes, real `--print-plan` content. This is the C-as-integration-gate that proves the chain, addressing the C-02/C-07 fake-done-leaf pattern. **PASS.**
- **Grammar / numeric:** N/A. **PASS.**

## Summary
All bindings **PASS**. No FAIL. Batch clear to queue. The single numeric premise (RSS
headroom) is validated with empirical margin; all other leaves are mechanism/wiring/docs
with behavioral (not synthetic-input) signals.
