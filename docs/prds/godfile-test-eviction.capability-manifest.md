# Capability manifest — God-file decomposition, stage 1: evict embedded test modules

Mechanizes G3 + G6 per leaf for `docs/prds/godfile-test-eviction.md`. Built at decompose
time (2026-07-06) **by direct AST-aware source inspection + a live build check**, not by
the `scripts/prd-decompose-verify.mjs` D3 Enumerator/Prover/Adversary workflow: that
workflow exists to adjudicate *narrative* premises baked into a RED test (a numeric
bound, a grammar fragment, an end-to-end capability claim). This PRD asserts none of
those — its only substantive claim is a structural fact about existing source
(`geometry_ops.rs` / `engine_build.rs`'s test-code boundaries), which is directly
inspectable and was inspected. Evidence forms used below: `inspect:<file>:<line-range>`
(direct read/grep/AST-scan at HEAD `4d696e63cb`, 2026-07-06) and `lang:<rule>` (Rust
language-semantics fact, not code-specific). Any FAIL value blocks the batch — there are
none below.

---

## α — geometry_ops.rs test eviction — leaf

Signal: `cargo nextest list -p reify-eval --lib` sorted output identical (356 tests,
same names) before/after the move; `wc -l` on `geometry_ops.rs` drops to the
production-only range; full verify pipeline green.

| Capability | Evidence | Verdict |
|---|---|---|
| Test code in `geometry_ops.rs` is confined to exactly one `#[cfg(test)] mod tests { ... }` block extending to the file's literal EOF (a lossless verbatim cut is possible) | `inspect:crates/reify-eval/src/geometry_ops.rs` — AST-aware brace-matcher (skips string/char literals, immune to the raw-string-with-braces false positive that broke naive counting) found exactly one column-0 `#[cfg(test)]` marker at line 10329, closing brace at line 32802 = the file's last line. Unanchored scan confirms zero `cfg(test)` markers anywhere earlier (any indentation). | **PASS** |
| No production code is embedded inside the test block | Same brace-matched scan — by construction, everything strictly between the block's opening and matching closing brace is inside `mod tests`; nothing after line 32802 exists to leak. | **PASS** |
| No external consumer imports `geometry_ops::tests` by path (extraction cannot break a caller) | `inspect: grep -rn "geometry_ops::tests" crates --include=*.rs` → 1 hit, `engine_build.rs:19159`, a **prose comment** citing a test name for documentation, not a code path reference. | **PASS** |
| Moving the module to a sibling file preserves visibility (no `pub(crate)`/`pub` annotations need to change) | `lang: an item is visible in its defining module and all descendant modules, independent of whether the descendant is inline or file-backed`. `geometry_ops::tests` is a direct child module either way. | **PASS** |
| The mod.rs-free `file.rs` + `file/` sibling-directory layout is available in this workspace | `inspect:Cargo.toml:41` → `edition = "2024"` (supports this layout; no in-crate precedent existed to defer to, so this PRD establishes it — noted in the PRD as a resolved design decision, not a gap). | **PASS** |
| Baseline is green (the parity check is meaningful, not vacuously true against a broken build) | `inspect: cargo check -p reify-eval --lib` → succeeded, 56.53s, 2026-07-06. | **PASS** |
| `cargo nextest` is available to produce the pre/post list | `inspect: cargo-nextest 0.9.136` on `PATH`. | **PASS** |

**Verdict: all PASS. No blocking bindings.**

---

## β — engine_build.rs test eviction — leaf

Signal: `cargo nextest list -p reify-eval --lib` sorted output identical (107 tests,
same names) before/after the move; `wc -l` on `engine_build.rs` drops to the
production-only range; `p2_substitution_diagnostic`'s body diffs as unchanged context;
full verify pipeline green.

| Capability | Evidence | Verdict |
|---|---|---|
| Test code in `engine_build.rs` is confined to exactly 7 named `#[cfg(test)]` module blocks (not a single blob, correcting the survey's implicit framing) | `inspect:crates/reify-eval/src/engine_build.rs` — AST-aware scan found exactly 7 column-0 `#[cfg(test)]` markers (lines 10938, 19650, 19865, 20205, 20341, 21566, 21756), each brace-matched to a precise close (19642, 19861, 20134, 20337, 21558, 21747, 21998=EOF). Unanchored scan confirms zero `cfg(test)` markers anywhere before line 10938. | **PASS** |
| No production code is embedded *inside* any of the 7 blocks | Same brace-matched scan — content strictly between each block's own open/close brace is inside that module by Rust scoping; the concern is code *between* blocks, handled below. | **PASS** |
| Production code sitting *between* two test blocks is correctly identified and excluded from the move | `inspect:crates/reify-eval/src/engine_build.rs:20135-20204` — read directly: `pub(crate) fn p2_substitution_diagnostic(...)` (`#[allow(dead_code)]`, doc comment references task #4744) sits between `dispatch_volume_mesh_tests` (closes 20134) and `p2_substitution_diagnostic_tests` (opens 20205). Flagged explicitly as a MUST-NOT-MOVE hazard in the PRD task description (§6.3 step 3). | **PASS (hazard identified and gated in the task text, not silently correct)** |
| The other 5 inter-block gaps contain no hidden production code | `inspect:` gaps at 19642–19650, 19861–19865, 20337–20341, 21558–21566, 21747–21756 — each read directly; all are banner/doc comments only. | **PASS** |
| Moving each module to its own sibling file preserves visibility | `lang:` same rule as α — `engine_build::<name>` is a direct child module of `engine_build` either way; `p2_substitution_diagnostic` being `pub(crate)` (not even private) makes this even less constrained for its own test module. | **PASS** |
| Baseline is green | Same `cargo check -p reify-eval --lib` run as α (single crate, both files compile in the same pass). | **PASS** |

**Verdict: all PASS. No blocking bindings.**

---

## γ — Stage 3 bookmark — deferred, do-not-implement

No substantive capability bindings apply: this task performs no work and is
deliberately excluded from the `commit_planning` flip (stays `deferred`). Its only
premise — "α and β together produce a pre-shaped, test-free-of-clutter production file
for stage 3 to split" — is true by construction once α and β land (real
`add_dependency` edges γ→α, γ→β wired at decompose time; see PRD §6.3). No FAIL
bindings possible because no signal is being asserted as complete-able today.

---

## Summary

| Leaf | Bindings | FAIL count | Blocks batch? |
|---|---|---|---|
| α | 7 | 0 | No |
| β | 6 | 0 | No |
| γ | n/a (deferred, no work) | 0 | No (not scheduled) |

Batch clears the capability manifest gate. Proceeding to file via
`submit_task(planning_mode=True)`.
