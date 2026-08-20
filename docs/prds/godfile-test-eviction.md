# PRD — God-file decomposition, stage 1: evict embedded test modules

**Status:** active. Version-agnostic build-hygiene infrastructure (root `docs/prds/`).
**Date:** 2026-07-06. Stage 1 of Leo's ratified 3-stage god-file decomposition plan
(hotspot-program batch; survey: `docs/notes/bug-hotspot-survey-2026-07-05.md` §H2/§H3).
**Approach: bare B** (no new mechanism/contract introduced — see §5 G5 rationale).

**One-line goal:** evict the `#[cfg(test)]` test modules embedded in
`crates/reify-eval/src/geometry_ops.rs` and `crates/reify-eval/src/engine_build.rs` into
sibling test files, with zero semantic change and exact test-name/count parity, so these
two files stop being the mandatory merge-contention point for all geometry/build work.

---

## 1. Background

`docs/notes/bug-hotspot-survey-2026-07-05.md` ranks `geometry_ops.rs` and
`engine_build.rs` as hotspots #3 and #2 respectively: both are god-files that every
geometry- or build-touching task must lock, and — per the survey's §H2/§H3 proposal C5 /
(5) — the ~10.7k and ~22.3k lines of embedded tests are most of each file's bulk. Leo's
ratified 3-stage plan (verbatim, spawn brief 2026-07-06):

> (1) evict embedded tests FIRST — this PRD; (2) semantic extractions — owned by the
> kernel-seam-contracts and engine-build-hardening PRDs (concurrent sessions, not yet
> authored as of this writing); (3) module splits along the survey's seam maps —
> deferred bookmark here. "Do it as soon as possible consistent with good
> quality/correctness. There are no quiet times. Set the wide lock split task(s) as high
> or critical priority and let module lock parking push them through when they hit the
> front of the queue."

This PRD is **stage 1 only**. It is deliberately a **small number of deliberately
wide-lock tasks** — the opposite of the project's usual tight-lock discipline
(`task-lock-charter-lifecycle.md`) — because the entire point is to let orchestrator
module-lock parking serialize every other geometry/build task behind these two CRITICAL
tasks until they land, then get out of the way.

### 1.1 Re-verified line boundaries (G3/G6 premise re-check — the survey's numbers are stale by design)

The survey's line anchors are from 2026-07-05; both files grow weekly (INV-META-1 /
brief §G3 instruction: "re-verify"). Re-verified directly against `main` @ `4d696e63cb`
(2026-07-06) via an AST-aware brace-matcher that skips string/char literals and
comments (naive `{`/`}` counting is unsound here — the files contain raw-string test
fixtures with literal braces):

**`geometry_ops.rs`** — 32,802 lines total, 356 `#[test]` fns, **exactly one**
`#[cfg(test)] mod tests { ... }` block, lines 10329–32802, extending to the file's
literal EOF. Production code: lines 1–10328 (10,328 lines). No other `cfg(test)` marker
exists anywhere earlier in the file (confirmed by an unanchored scan). No external file
references `geometry_ops::tests` (one comment in `engine_build.rs:19159` cites a test
name inside it for documentation purposes only — not a code dependency).

**`engine_build.rs`** — 21,998 lines total, 107 `#[test]` fns, **seven** separate
`#[cfg(test)]` module blocks (not one contiguous block — this corrects the survey's
implicit "one blob" framing):

| Module | Lines | Count |
|---|---|---|
| `mod tests` | 10938–19642 | 8,705 |
| `mod populate_local_feature_tests` | 19650–19861 | 212 |
| `mod dispatch_volume_mesh_tests` | 19865–20134 | 270 |
| `mod p2_substitution_diagnostic_tests` | 20205–20337 | 133 |
| `mod mixed_region_tests` | 20341–21558 | 1,218 |
| `mod post_process_mechanism_mass_props_tests` | 21566–21747 | 182 |
| `mod diagnose_topology_correspondence_drops_tests` | 21756–21998 (EOF) | 243 |
| **Total test lines** | | **10,963** |

Critically, **live production code sits between two of these blocks**: lines
~20135–20204 hold `pub(crate) fn p2_substitution_diagnostic(...)` (currently
`#[allow(dead_code)]`, wiring pending task #4744) plus its doc comment — this is
production code, not test code, and **must not be moved**. The other five inter-block
gaps are banner-comment-only (verified by inspection). Production total:
21,998 − 10,963 = **11,035 lines**.

This structural fact is good news for the brief's conditional per-responsibility split
request for `engine_build.rs` (§2 task 2 below): the file **already has** 6
purpose-named sibling test modules plus the general `mod tests` — splitting along those
existing boundaries is zero-reclassification-risk (the boundaries are the crate authors'
own, not this PRD's guess).

---

## 2. Consumers (G1)

No orphan-producer risk — the brief pre-answers this and it holds up under inspection:

1. **Every contributor/agent landing geometry or build work.** The deliverable *is*
   merge-contention relief: these two files are locked by every task touching geometry
   ops or the build/tessellate pipeline. Observable via `get_scheduler_state` /
   `get_scheduler_events` — dispatchable-concurrency on tasks touching these files rises
   once the file sizes (and hence the *reason* other PRDs' tasks over-declare them) drop.
2. **Stage 3 (deferred bookmark, task γ below)** — consumes the pre-shaped test layout
   directly; its production-code module split can proceed without also having to
   relocate ~33k lines of test code in the same pass.
3. **kernel-seam-contracts and engine-build-hardening PRDs** (concurrent sessions, not
   yet authored) — referenced, not filed against, per G4 (§4).

**G1 engine-integration sub-check:** N/A. This PRD introduces no new engine
seam/dispatch/hook — it moves existing `#[cfg(test)]` code verbatim. No new consumer
plugs into `engine-integration-norm.md` §3 because nothing new is produced for the
*engine* to consume; the consumer is the *build/orchestrator/human* surface described
above.

---

## 3. Substrate verification (G3)

**No novel substrate assumed — G3 is a no-op in the usual "does this API/grammar/flag
exist" sense.** The one substrate question that *does* matter here — "can this test code
be relocated with zero semantic change?" — was verified directly (not assumed) and is
recorded per-leaf in the companion capability manifest
(`docs/prds/godfile-test-eviction.capability-manifest.md`):

- Rust privacy: an item is visible in its defining module **and all descendant
  modules**, regardless of file layout. A `#[cfg(test)] mod tests;` pointing at a
  sibling file (`geometry_ops/tests.rs`) is exactly as much a descendant of
  `geometry_ops` as an inline `mod tests { ... }` — `use super::*;` resolves identically
  either way. No visibility annotation needs to change.
- The workspace is edition `2024` (`Cargo.toml:41`), which supports the mod.rs-free
  `file.rs` + `file/` sibling-directory module layout needed here. No crate in the
  workspace has used this exact pattern before (checked); it is standard, unexotic
  modern Rust, introduced here because no in-crate precedent existed to defer to.
- `cargo-nextest 0.9.136` is on `PATH` and `cargo check -p reify-eval --lib` succeeds
  today (56.5s) — the parity check in §6 is meaningful against a green baseline.

The D3 Enumerator/Prover/Adversary workflow (`scripts/prd-decompose-verify.mjs`,
overlay-mandated for behavioral/numeric/grammar capability claims) was **not invoked**:
this PRD asserts no such claims. Its only "capability" is a structural fact about
existing source (verified above by direct AST-aware inspection, not narrative
assertion) — recorded as an explicit conservative choice in §9.

---

## 4. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| kernel-seam-contracts (not yet authored) | downstream consumer | `geometry_ops.rs` production-code semantic extraction (stage 2) | kernel-seam-contracts | referenced only — not filed against; real `add_dependency` edge to be added once its extraction task exists |
| engine-build-hardening (not yet authored) | downstream consumer | `engine_build.rs` production-code semantic extraction (stage 2) | engine-build-hardening | referenced only — not filed against; real `add_dependency` edge to be added once its extraction task exists |
| eval-substrate (mentioned in the hotspot-program batch's common context; not yet authored) | potential co-toucher | no shared mechanism — only a shared file surface (rebase risk, not an ownership seam) | n/a | advisory note only (§7) |
| this PRD's own stage-3 bookmark (task γ, §6) | intra-PRD | module split along the survey's seam maps | this PRD (deferred leaf, do-not-implement) | queued deferred, depends on α+β |

No reciprocal-ownership ambiguity: kernel-seam-contracts and engine-build-hardening do
not exist yet, so there is no PRD text anywhere claiming this PRD's territory. This PRD
claims none of theirs — stage 2 (semantic extraction of *production* code) is explicitly
out of scope here (§7).

---

## 5. Approach (G5)

**Bare B.** Blast radius is 1 crate (`reify-eval`), mechanism count is ~0 (no new
type/fn/API — a pure relocation), no load-bearing seam is touched (production code is
byte-for-byte untouched), and while 3 other PRDs eventually reference the resulting
layout, none of them consume a *contract* this PRD defines — they consume a smaller
file to work in. No B+H contract/boundary-test section is warranted; the "boundary" here
is `cargo nextest list` parity, which is the G2 signal itself, not a producer/consumer
seam needing a two-way test.

---

## 6. Resolved design decisions

**Target file layout — Rust 2018+ mod.rs-free sibling-directory convention** (chosen
because no in-crate precedent exists to defer to; see §3):

`geometry_ops.rs` (production only, ~10,328 lines) ends with:
```rust
#[cfg(test)]
mod tests;
```
and the entire former body of `mod tests { ... }` (verbatim, including its `use`
statements) becomes the top-level content of the new file
`crates/reify-eval/src/geometry_ops/tests.rs` (~22,474 lines).

`engine_build.rs` (production only, ~11,035 lines, `p2_substitution_diagnostic` **left
in place, untouched, at its current textual position**) has each of its 7 existing
`#[cfg(test)] mod X { ... }` blocks replaced **in place** by a one-line
`#[cfg(test)] mod X;` declaration, each pointing at a new sibling file:

- `crates/reify-eval/src/engine_build/tests.rs`
- `crates/reify-eval/src/engine_build/populate_local_feature_tests.rs`
- `crates/reify-eval/src/engine_build/dispatch_volume_mesh_tests.rs`
- `crates/reify-eval/src/engine_build/p2_substitution_diagnostic_tests.rs`
- `crates/reify-eval/src/engine_build/mixed_region_tests.rs`
- `crates/reify-eval/src/engine_build/post_process_mechanism_mass_props_tests.rs`
- `crates/reify-eval/src/engine_build/diagnose_topology_correspondence_drops_tests.rs`

This *is* the "per-responsibility split" the brief conditionally asked for on
`engine_build.rs` — achieved for free because the responsibility boundaries already
exist as named modules in the source (§1.1), so no reclassification judgment call is
needed. For `geometry_ops.rs` no such pre-existing internal module structure exists (its
356 tests are a flat list inside one `mod tests`); see §9 for why this PRD does **not**
attempt to invent a responsibility split there.

**Banner/doc comments** immediately preceding each `#[cfg(test)]` marker move together
with their test content into the new file (they document the tests, not the production
file). This is a formatting-only judgment call, not a semantic one — an implementer may
adjust without violating the "no semantic change" mandate.

**metadata.files — deliberately wide, overriding the project's usual tight-or-empty rule.**
The overlay's default decompose-mode rule (`.claude/skills/prd/project.md` §"metadata.files
authoring rule") is tight-or-empty-never-a-directory. Leo's brief explicitly overrides
this for these two tasks: *"declare the god-file itself + the new test-file paths. That
is the point — Leo wants module-lock parking to serialize everything else behind them."*
This PRD honors the override **without** violating the underlying mechanism the rule
protects against (never declaring a directory): every entry below is a concrete `.rs`
file path, verified against `scripts/lock-charter-guard.sh check` (all ACCEPT, exit 0).
The wideness comes from the *count* of named files, not from a directory wildcard.

---

## 7. Out of scope for this PRD

- **Any change to production code or test *content*.** No test is rewritten, renamed,
  reordered relative to its siblings, or have its assertions touched. The `#[test]` fn
  count and every fn name must be byte-identical before/after within its module.
- **Stage 2** (semantic extraction of production code in either file) — owned by
  kernel-seam-contracts / engine-build-hardening (§4), not filed here.
- **Stage 3** (module split of the now-test-free production code along the survey's
  seam maps) — filed here only as a **deferred, do-not-implement bookmark** (task γ,
  §6.3 below), per the brief's explicit instruction.
- **Any co-touching by the eval-substrate PRD** (or any other concurrent PRD queuing
  work against these two files) — not managed here beyond the rebase-friendliness note
  attached to tasks α/β (§6.3): a wholesale verbatim text relocation rebases trivially;
  any conflicting branch should take the moved location.
- Splitting `geometry_ops.rs`'s single flat `mod tests` into multiple per-responsibility
  files — deferred; see §9 open question 1.

---

## Decomposition plan

Three tasks: two CRITICAL leaf tasks (α, β) that do the actual eviction, and one
deferred, unscheduled bookmark (γ) for stage 3. This is the entire batch — "few tasks
here is correct" per the brief.

### α — geometry_ops.rs test eviction (LEAF, priority CRITICAL)

**Modules touched:** `crates/reify-eval/src/geometry_ops.rs`,
`crates/reify-eval/src/geometry_ops/tests.rs` (new).

**Mechanical procedure:**
1. Re-locate the current `#[cfg(test)] mod tests { ... }` boundaries by search (line
   numbers in §1.1 are a 2026-07-06 snapshot — the file grows weekly; do not trust them
   blindly, confirm the block still runs to literal EOF and is still the only
   `cfg(test)` marker in the file).
2. Capture the pre-move baseline: `cargo nextest list -p reify-eval --lib 2>&1 | sort`
   (full sorted list — cheap enough not to filter) and the `#[test]` count
   (`grep -c '#\[test\]' crates/reify-eval/src/geometry_ops.rs`, expect 356 as of
   2026-07-06, re-count at execution time).
3. Create `crates/reify-eval/src/geometry_ops/tests.rs` containing the exact former body
   of `mod tests { ... }` (everything between its opening and matching closing brace),
   verbatim, as the file's top-level content (no wrapping `mod tests { }`, no `#[cfg(test)]`
   inside the file — the attribute lives on the declaration).
4. Replace the original block in `geometry_ops.rs` with:
   ```rust
   #[cfg(test)]
   mod tests;
   ```
5. `git diff` on `geometry_ops.rs` must show **only** the test-block deletion + the
   2-line replacement — zero production-line changes. If it shows anything else, stop.
6. Re-run the nextest capture from step 2 against the new tree; the sorted list and
   count must be **identical**.
7. `cargo build -p reify-eval` and the project's full verify pipeline
   (`scripts/verify.sh --scope all --profile both`) must be green.
   `bash scripts/verify-pipeline-guard.sh requires-full-gate crates/reify-eval/src/geometry_ops.rs crates/reify-eval/src/geometry_ops/tests.rs`
   is expected to exit 1 (neither file is in the verify-pipeline manifest — confirmed
   2026-07-06) — this only affects the merge-worker's trivial-pass eligibility, not
   whether tests run; the full suite still executes as normal.

**Authorized exception (Leo, 2026-07-06 — resolves esc-5026-4).** One moved test,
`compile_geometry_op_has_no_nested_per_kind_match`, byte-depends on the *pre*-eviction
layout: it `include_str!`s `geometry_ops.rs` and locates the production/test split via the
literal `"\n#[cfg(test)]\nmod tests {"`, then asserts the production region has no per-kind
behavioral match arms. The eviction rewrites that byte sequence to `mod tests;`, so a
*pure* verbatim move would make `.find` return `None`, panic the `.expect`, and turn verify
RED — the "verbatim move + no test rewrites + verify stays green" constraints are jointly
unsatisfiable for this one test. The sanctioned deviation is a **single-line update to that
boundary literal** (`mod tests {` → `mod tests;`) plus its matching `.expect` message. The
scanned production region (everything before the declaration) is **byte-identical** to
before, so the guard's invariant is fully preserved; it is not weakened. All 355 other
tests move byte-for-byte. This is the **only** permitted departure from step 3's "verbatim"
mandate.

**Rebase note (carry verbatim in the task, per the brief):** "wholesale test move —
rebases over this are trivial (test text relocated verbatim); if your branch conflicts,
take the moved location."

**User-observable signal (G2):** `cargo nextest list -p reify-eval --lib` produces an
identical sorted test-name set and count (356 as of this writing, re-verify at dispatch)
before and after; `wc -l crates/reify-eval/src/geometry_ops.rs` drops from 32,802 to
~10,328 (production-only, exact production line range preserved — this is the
CLI-output-difference / operator-visible-metric class of G2 signal, adapted for a
pure-refactor PRD where "nothing observably changes for a user" *is* the point); the
full verify pipeline stays green.

**metadata.files:** `["crates/reify-eval/src/geometry_ops.rs", "crates/reify-eval/src/geometry_ops/tests.rs"]`
(wide-lock, deliberate — §6).

### β — engine_build.rs test eviction (LEAF, priority CRITICAL)

**Modules touched:** `crates/reify-eval/src/engine_build.rs`, plus 7 new sibling files
under `crates/reify-eval/src/engine_build/` (§6).

**Mechanical procedure:** same shape as α, generalized to 7 blocks, with one hazard
called out explicitly:

1. Re-locate all 7 `#[cfg(test)]` module blocks by search — do not assume a single
   contiguous span (unlike `geometry_ops.rs`, this file interleaves test blocks with
   real production code).
2. Capture the pre-move baseline exactly as in α step 2 (`cargo nextest list -p
   reify-eval --lib`, `#[test]` count expect 107 as of 2026-07-06).
3. **Hazard: `pub(crate) fn p2_substitution_diagnostic(...)` (currently
   `#[allow(dead_code)]`, pending task #4744) sits between the
   `dispatch_volume_mesh_tests` and `p2_substitution_diagnostic_tests` blocks (§1.1).
   This is production code — do NOT move it, do not treat it as part of either
   neighboring test module's content.** Its position relative to the surrounding
   `mod X;` declarations may shift by a few lines as the blocks around it become
   one-liners, but its body must be byte-identical and it must remain a normal item in
   `engine_build.rs`, not relocated to any new file.
4. For each of the 7 blocks, create the corresponding sibling file (§6 layout) with the
   block's exact former body as top-level content, and replace the block in
   `engine_build.rs` with `#[cfg(test)] mod <name>;`.
5. `git diff` on `engine_build.rs` must show only: 7 test-block-body deletions, 7
   one-line `mod X;` replacements, and nothing else — `p2_substitution_diagnostic`'s
   body must appear as unchanged context, not as a diff hunk.
6. Re-run the nextest capture; sorted list and count (107) must be identical.
7. Same build/verify-pipeline-guard checks as α step 7, run against
   `engine_build.rs` + all 7 new files.

**Rebase note (carry verbatim in the task):** same as α.

**User-observable signal (G2):** `cargo nextest list -p reify-eval --lib` identical
sorted set/count (107) before/after; `wc -l crates/reify-eval/src/engine_build.rs` drops
from 21,998 to ~11,035; `p2_substitution_diagnostic`'s body diffs as unchanged; full
verify pipeline green.

**metadata.files:** `["crates/reify-eval/src/engine_build.rs", "crates/reify-eval/src/engine_build/tests.rs", "crates/reify-eval/src/engine_build/populate_local_feature_tests.rs", "crates/reify-eval/src/engine_build/dispatch_volume_mesh_tests.rs", "crates/reify-eval/src/engine_build/p2_substitution_diagnostic_tests.rs", "crates/reify-eval/src/engine_build/mixed_region_tests.rs", "crates/reify-eval/src/engine_build/post_process_mechanism_mass_props_tests.rs", "crates/reify-eval/src/engine_build/diagnose_topology_correspondence_drops_tests.rs"]`
(wide-lock, deliberate — §6).

### γ — Stage 3 bookmark: production module splits (DEFERRED — do NOT implement)

**Priority: low** (conservative default for a deferred, unscheduled bookmark — see §9
open question 2). **Depends on α and β** (real intra-batch edges — the pre-shaped test
layout is this task's whole premise). **Stays `deferred`** — filed via
`planning_mode=True` like its siblings but deliberately **excluded** from the
`commit_planning` flip, per this project's established bookmark pattern
(`preferences_bookmark_task_pattern`).

**Scope (when eventually activated — NOT now):** module splits along the survey's seam
maps:
- `geometry_ops.rs` → `op_compile` / `query_dispatch` / `kinematic` / `selector_build` /
  `topology_selector` / `arg_resolve` / `reply_decode` / `ad_hoc` / `surfacing` (survey
  §H3 structure table).
- `engine_build.rs` → `engine_realize` / `engine_tessellate` / `engine_post_process` /
  `cross_sub_realization` (survey §H2), following the crate's existing `engine_<verb>.rs`
  convention established by task #2032 (`engine_eval.rs` / `engine_edit.rs` /
  `engine_purposes.rs` / `engine_constraints.rs` / `engine_admin.rs`, done 2026-04-21).

**Real dependency wiring still pending:** kernel-seam-contracts and
engine-build-hardening (§4) do not exist yet. When those PRDs are authored and
decomposed, add real `add_dependency` edges from this bookmark to their production
semantic-extraction leaf tasks (this bookmark's split can't proceed sensibly until
production code has already been re-shaped by stage 2). Do **not** invent placeholder
edges now.

**metadata.files:** `[]` (no work performed; defer entirely to the architect when this
bookmark is eventually activated).

---

## 8. Priority rationale

α and β: **CRITICAL**, per the brief's explicit, doubly-stated instruction ("Set the
wide lock split task(s) as high or critical priority and let module lock parking push
them through when they hit the front of the queue" + task-level "priority: CRITICAL"
labels in the brief's own scope section). γ: **low** — it is an inert bookmark with an
unmet real-world prerequisite (stage-2 PRDs not yet authored); nothing is lost by it
sitting at the back of the deferred pile since it never enters the schedulable queue in
this PRD's decomposition anyway.

## 9. Open questions (tactical — not decided in this session, resolved conservatively for now)

1. **Should `geometry_ops.rs`'s single flat `mod tests` (356 tests, no pre-existing
   internal module structure) also get a per-responsibility split, mirroring what
   `engine_build.rs` gets for free?** Not attempted here. Reasoning: unlike
   `engine_build.rs`, there is no pre-existing module boundary to follow — any split
   would require reading and classifying all 356 tests by which of the survey's 9
   production seams (`op_compile`/`query_dispatch`/etc.) they exercise, which is a
   judgment call, not a mechanical fact, and directly conflicts with this task's "PURE
   MECHANICAL MOVE: no semantic changes" mandate. **Conservative resolution:** single
   wholesale sibling file (`geometry_ops/tests.rs`) now; defer the responsibility split
   to stage 3, where it can be done consistently alongside (and validated against) the
   production-code split it's meant to pre-shape, using the *actual* post-split module
   boundaries rather than a guess made before they exist.
2. **γ's priority.** The brief doesn't state a priority for the stage-3 bookmark (only
   for α/β). **Conservative resolution:** `low`, since it's an inert, unscheduled
   placeholder with a real dependency (stage-2 PRDs) that doesn't exist yet — raising its
   priority would have no effect (it never reaches `pending` in this batch) and could
   mislead a future reader of the task list into thinking it's more actionable than it
   is.
3. **Exact banner-comment handling at each of the 7 `engine_build.rs` split points**
   (move with the test content vs. leave a one-line pointer comment next to the `mod X;`
   declaration). Left to the implementer (§6) — purely cosmetic, does not affect the
   G2 signal.
4. **Should α/β be flagged `complexity: "simple"`** (single-agent fast path, skips the
   architect+implementer split)? **Conservative resolution: no.** Despite being
   mechanically well-specified, correctly executing a 22k/11k-line relocation and
   verifying byte-for-byte parity (especially β's interspersed-production-code hazard)
   warrants the normal architect+implementer flow rather than a single-pass "quick fix."
