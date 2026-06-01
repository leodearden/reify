# P5 H1/H2 live-corpus FP validation: keep both Medium, harden H1, defer promotion

**Date:** 2026-06-01
**Status:** decision — closed
**Scope marker:** task 4141; follow-up of task 4140 (H1 + H2 heuristic implementation).
Cross-ref: `docs/architecture-audit/f-infra-design.md` §5 P5; `crates/reify-audit/src/p5_phantom_done.rs`.

---

## 0. Purpose

Task 4140 added two new P5 heuristics at `Severity::Medium`:

- **H1** (`check_tests_assert_empty`, `Pattern::P5TestsAssertEmpty`): flags a done task whose commit diff adds a test fn with a placeholder-marker name (`placeholder`, `stub`, `not_yet`, `notyet`, `todo`, `unimplemented`) AND a vacuous empty assertion in the fn body.
- **H2** (`check_live_path_stranded`, `Pattern::P5LivePathStranded`): flags a done task whose changed capability symbols have no live non-test callers in the cross-crate call graph (via `JCodemunchOps::get_changed_symbols` + `find_references`).

This PRD records:
1. The live-corpus H1 false-positive count and FP classes.
2. Why H2 cannot be live-validated against the current corpus.
3. The in-scope H1 precision hardening applied in task 4141 (three-signal gate).
4. The **keep-both-Medium** promotion decision for H1 and H2, with explicit deferral rationale.
5. The promotion criteria a future task must meet.
6. Filed follow-up tasks.

---

## 1. What "promotion to High" means mechanically

The D-1 pre-done hook is `reify-audit --task <id> --pre-done`. Its exit code is computed by `high_severity_exit_code` (`crates/reify-audit/src/bin/reify-audit.rs:154–159`), which counts **only** `Severity::High` findings:

```rust
fn high_severity_exit_code(findings: &[Finding]) -> u8 {
    let count = findings.iter().filter(|f| f.severity == Severity::High).count();
    count.min(254) as u8
}
```

A non-zero exit code from this function aborts the done-flip. Therefore:

- **`Severity::Medium`** = visible in `/audit` reports but **non-blocking** for done-flips.
- **`Severity::High`** = auto-blocking: any done-flip for a task with an H1 or H2 High finding is rejected by the pre-done hook.

Promotion to High requires a clean, NON-vacuous live validation sweep before it can be justified as non-blocking in practice. Unjustified High findings would create false done-flip rejections.

---

## 2. H1 live-corpus FP analysis

### 2.1 Sweep methodology

H1 uses only `RealGitOps` (git diff over the live repo), so it genuinely executes over the live corpus without jcodemunch. A pre-run corpus grep was performed to surface candidate FP fn names.

### 2.2 FP count and classes

The live corpus contains approximately:
- **53 test fns** containing the substring `"stub"` as a **domain noun** (not a placeholder-test marker).
- **25 test fns** containing the substring `"placeholder"` as a **domain noun** (not a placeholder-test marker).

Several of these co-occur with empty or zero-valued assertions in their fn bodies, which means H1's pre-4141 two-gate (name-marker AND body-empty-assertion) **fires on them as false positives**.

### 2.3 Concrete FP classes

**Class A — kernel-module noun ("stub"):**
Pattern: `fn stub_kernel_export_returns_error()` with `assert_eq!(result, 0)`.
Location: `crates/reify-kernel-occt/src/stubs.rs` and related test modules.
"stub" names the product module (a kernel stub / shim layer), not a not-yet-implemented test. The empty/zero assertion is a legitimate success-code check.

**Class B — geometry-sentinel noun ("placeholder"):**
Pattern: `fn tessellate_sentinel_placeholder_continues_independent_ops()` with `assert!(result.is_empty())`.
Location: `crates/reify-eval/tests/geometry_error_handling.rs`.
"placeholder" describes a sentinel value in the geometry kernel (a placeholder geometric object used during error recovery paths), not a placeholder-test marker. The fn legitimately asserts that the sentinel path produces an empty geometry list.

**Class C — path/template noun ("placeholder"):**
Pattern: `fn compile_pipe_omits_path_placeholder()` with a vacuous assertion.
"placeholder" names a template slot in a pipe descriptor, not an unimplemented test.

### 2.4 FP reducibility

These FPs are **partly irreducible at the fn-name level**: the marker word is genuinely used as a domain noun, and no simple rename disambiguates without changing the corpus. The FP class is therefore non-zero and expected to persist in future sweeps.

### 2.5 H1 TP rate

The genuine incident pattern verified in task 4140 and preserved through task 4141:
- `activate_expands_geometric_params_placeholder_to_empty_list` — carries "placeholder" (marker) AND "empty" (empty-intent noun) in its name, and asserts `is_empty()`. This is a genuine placeholder test masking an unimplemented capability.

---

## 3. H1 precision hardening: three-signal gate

### 3.1 Change applied (task 4141 step-2)

`check_tests_assert_empty` was hardened from the **two-gate**:
```
(a) name contains PLACEHOLDER_MARKERS  AND  (b) body has EMPTY_ASSERTION_PATTERNS
```
to a **three-signal gate**:
```
(a) name contains PLACEHOLDER_MARKERS  AND
(c) name contains EMPTY_INTENT_NAME_TOKENS  AND
(b) body has EMPTY_ASSERTION_PATTERNS
```

`EMPTY_INTENT_NAME_TOKENS = ["empty", "none", "nil", "zero", "vacuous", "nothing", "no_"]`

These tokens were chosen to avoid substring collisions with common identifiers:
- `"nil"` is not in `"until"`, `"sentinel"`, or `"tessellate"`.
- `"none"` is not in `"independent"`, `"continues"`, or `"canonical"`.
- `"no_"` (with trailing underscore) matches `"no_results"`, `"no_items"` but not `"independent"`, `"canonical"`, `"cannot"`.

### 3.2 Effect on FP classes

| FP class | Pre-4141 (two-gate) | Post-4141 (three-signal) |
|---|---|---|
| `stub_kernel_export_returns_error` | **fires** (FP) | suppressed — no empty-intent token |
| `tessellate_sentinel_placeholder_continues_independent_ops` | **fires** (FP) | suppressed — no empty-intent token |
| `compile_pipe_omits_path_placeholder` | **fires** (FP) | suppressed — no empty-intent token |
| `activate_expands_geometric_params_placeholder_to_empty_list` | fires (TP) | **still fires** — "placeholder" (marker) + "empty" (intent) |

### 3.3 Recall tradeoff

A masking test whose fn name carries a PLACEHOLDER_MARKERS token but **lacks** any EMPTY_INTENT_NAME_TOKENS token would be missed by the three-signal gate. This is the correct precision/recall bias for a `Severity::Medium` (non-blocking) signal: we prefer fewer false blocks over complete recall. The tradeoff is documented in the `EMPTY_INTENT_NAME_TOKENS` comment in `p5_phantom_done.rs`.

Broader detector tuning (word-boundary marker matching, the corpus FP tail from other marker words) is filed as a follow-up (§6.1).

---

## 4. H2 un-validatability finding

### 4.1 H2 data source

H2 (`check_live_path_stranded`) iterates symbols from `ctx.jcodemunch.get_changed_symbols()`. With `NoopJCodemunchOps`, this returns `vec![]` → H2 iterates nothing → **zero H2 findings regardless of real stranding**.

### 4.2 Evidence: jcodemunch seam construction

`crates/reify-audit/src/bin/reify-audit.rs`:

**`needs_jcodemunch` (L433–443):**
```rust
fn needs_jcodemunch(args: &Args) -> bool {
    if args.pre_done { return false; }
    args.pattern.as_deref().is_none_or(|p| {
        pattern_selects(p, "P1")
            || pattern_selects(p, "PDEAD")
            || pattern_selects(p, "PUNTESTED")
            || pattern_selects(p, "PLAYER")
    })
}
```
P5 is absent from this list → `needs_jcodemunch` returns `false` for `--pattern P5` → `NoopJCodemunchOps` is always used for P5-only runs.

**Seam construction (L547–567):**
```rust
let jcodemunch: Box<dyn JCodemunchOps> =
    if args.no_jcodemunch || !needs_jcodemunch(&args) {
        Box::new(NoopJCodemunchOps)
    } else {
        match RealJCodemunchOps::new(...) {
            Ok(r) => Box::new(r),
            Err(e) => {
                eprintln!("reify-audit: jcodemunch unreachable ...");
                Box::new(NoopJCodemunchOps)  // fail-soft
            }
        }
    };
```

For a **default sweep** (no `--pattern`): `needs_jcodemunch` returns `true` → attempts `RealJCodemunchOps`. However, `jcodemunch-serve` is **not wired in reify** (the slice-2 PRD `reify-audit-p1-jcodemunch-substrate.md` covers this; the service is not yet deployed). `RealJCodemunchOps::new` fails → fail-softs to `NoopJCodemunchOps`.

**Conclusion:** A zero-finding H2 sweep is **vacuous** — it cannot distinguish "no stranded paths" from "jcodemunch substrate unavailable". Such a sweep cannot justify a `Medium → High` promotion.

---

## 5. Promotion decision: keep both H1 and H2 at Severity::Medium

### 5.1 H1 decision

**H1 stays `Severity::Medium`.** Promotion deferred pending a fresh NON-vacuous post-refinement validation sweep.

Rationale:
- The pre-4141 H1 FP rate is non-zero and partly irreducible.
- Task 4141 applied a precision hardening (three-signal gate) that reduces the FP class substantially.
- **Promoting on the same sweep that applied the refinement is circular**: the AS-IS FP count describes the PRE-refinement detector. The refined detector must be re-validated by an independent sweep before any promotion is justified.
- The promotion criteria (§6) must be met by a dedicated follow-up task.

### 5.2 H2 decision

**H2 stays `Severity::Medium`.** Promotion deferred pending the real `JCodemunchOps` implementation.

Rationale:
- H2's only data source is `JCodemunchOps::get_changed_symbols`, which is `NoopJCodemunchOps` for all current P5 runs.
- With a noop data source, H2 cannot produce any findings — it is structurally un-validatable.
- Promoting H2 to High while it is inert would place it in the auto-block set without any live-corpus evidence, creating false done-flip blocks as soon as the real jcodemunch substrate lands.

---

## 6. Promotion criteria for future tasks

A future task may promote H1 or H2 to `Severity::High` when ALL of the following are met:

### H1 promotion criteria

1. **Non-vacuous live sweep**: the sweep uses `RealGitOps` over the live corpus (same as task 4141); no MockGitOps substitution.
2. **Post-refinement sweep**: the sweep runs AFTER the three-signal gate (task 4141 step-2) has been in production for at least one cycle of done-task commits.
3. **Measured FP rate ≤ 5%**: across all H1 findings in the sweep, ≤5% are confirmed FPs via the TP/FP classification methodology used in task 4141.
4. **Written evidence in docs/prds/**: a PRD section recording the FP count, TP count, and the sweep's fn-name/file evidence must be filed before or alongside the High-flip commit.
5. **All existing tests still pass**: the `h1_domain_noun_placeholder_in_sentinel_not_flagged` and `h1_domain_noun_stub_kernel_not_flagged` regression guards from task 4141 must remain green.

### H2 promotion criteria

1. **Real jcodemunch substrate wired**: `needs_jcodemunch` returns `true` for P5 runs (i.e., the jcodemunch slice-2 work from `reify-audit-p1-jcodemunch-substrate.md` has landed and the `jcodemunch-serve` unit is running).
2. **Non-vacuous live H2 sweep**: `get_changed_symbols` returns a non-empty symbol set for at least one done task's commit range.
3. **Measured FP rate ≤ 5%**: same methodology as H1.
4. **H2 breadcrumb updated**: the H2 doc-comment in `p5_phantom_done.rs` is updated to record the live-sweep evidence (file:line of the first confirmed TP finding).
5. **Written evidence in docs/prds/**: recorded in a PRD section.

---

## 7. Filed follow-up tasks

The following follow-up tasks were filed (via `fused-memory submit_task`) as part of task 4141:

### 7.1 Real jcodemunch JCodemunchOps impl / H2 data source

Title: "P5 H2: wire real JCodemunchOps for P5 runs (H2 live-corpus data source)"
Context: H2's `check_live_path_stranded` is structurally inert because `needs_jcodemunch` returns `false` for P5 runs (→ `NoopJCodemunchOps`). The slice-2 jcodemunch substrate (`reify-audit-p1-jcodemunch-substrate.md`) landing is a prerequisite; once the substrate is wired, extend `needs_jcodemunch` to include P5 and run the H2 promotion sweep.

### 7.2 H2 silent-vacuity breadcrumb

Title: "P5 H2: add breadcrumb log line when H2 sweep is vacuous (NoopJCodemunchOps)"
Context: When `check_live_path_stranded` runs with `NoopJCodemunchOps`, it produces zero findings silently — operationally indistinguishable from "no stranded symbols found". Add a `log::debug!` or audit-report annotation indicating "H2 vacuous (NoopJCodemunchOps)" so operators reading `/audit` output understand why H2 is silent.

### 7.3 Fresh H1 re-validation sweep after refinement

Title: "P5 H1: post-refinement live-corpus re-validation sweep and promotion decision"
Context: Task 4141 applied the three-signal gate to H1. A dedicated follow-up task must run the refined detector over the live corpus, classify the H1 findings as TP/FP, verify FP rate ≤ 5%, and file the promotion evidence per §6 criteria. This task is the gate to flipping H1 from Medium to High.

---

## 8. Summary

| Heuristic | Severity | Status | Blocking for D-1? | Next action |
|---|---|---|---|---|
| H1 `P5TestsAssertEmpty` | Medium | Active, three-signal gate applied | No | Re-validation sweep (§7.3) → promote if FP ≤ 5% |
| H2 `P5LivePathStranded` | Medium | Active, structurally inert (NoopJCodemunchOps) | No | Wire real jcodemunch (§7.1) → non-vacuous sweep → promote |
