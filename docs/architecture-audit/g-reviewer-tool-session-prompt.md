# G (global reviewer tool) design+implement session — start prompt

Paste the block below into a fresh Claude Code session in this repo. Self-contained.

This is portfolio approach G: a **corpus-level** lint that catches "producer with zero production callers" at review time, never inside an individual task worktree (anti-gaming).

G is **complementary** to F (audit cadence + tracking infra) and to the PRD-decomposition skill. G operates at a different point in the lifecycle — it runs as part of `/review`, the orchestrator's periodic review, or on demand, and produces a corpus-wide report.

Unlike F (which is a session pair), G is a single session: design + implement together. Scope is tight enough.

---

## Paste this block

```
You are designing and implementing a corpus-level reviewer tool
that detects "producer with zero production callers" patterns
across the Reify codebase. Portfolio approach G — runs at /review
or orchestrator-periodic-review level, never per-task.

The anti-gaming property is essential: narrowly-focused task
implementers and stewards can game per-task gates (the audit
showed this happens — producer tasks close with unit tests
passing but no production caller). G runs at corpus level, so
no individual task can fool it.

## DELIVERABLES

  a. Tool implementation. Either:
     - Rust binary at `crates/reify-audit/` (new crate; standalone
       analyzer) — if the analysis fits well as a static analysis
     - Or shell+ripgrep+rust-analyzer script at
       `scripts/audit-orphan-producers.sh` — if simpler
     - Or extension to an existing skill / dev tool
     Pick whichever is more sustainable; design decision in
     conversation.
  b. Integration: invoke from `/review` skill as a phase OR as a
     `cargo xtask` (existing tooling pattern in the repo?) OR
     standalone. Pick one or two.
  c. A small report fixture: run the tool against the current
     codebase and capture the output at
     `docs/architecture-audit/g-tool-baseline-report.md`. This is
     the regression baseline for future runs.
  d. Test on at least 3 known orphan-producer cases from the audit
     (cluster C-04, C-10, C-43 are candidates per
     phase-3-files-synthesis.md) to confirm the tool detects them.
  e. Memory entry pointing at the tool + how to invoke it.

## REQUIRED READING

  1. docs/architecture-audit/phase-3-files-synthesis.md §1
     (clusters C-04, C-10, C-14, C-25, C-43 — known
     orphan-producer cases the tool must detect)
  2. docs/architecture-audit/phase-3-scaffold-pattern-critique.md
     §3 approach G + §1.3 Type A definition
  3. ~/.claude/projects/-home-leo-src-reify/memory/preferences_implementation_chain_portfolio.md
     (G's scope: corpus-level, anti-gaming, runs at review time)
  4. CLAUDE.md (project root — existing tooling patterns)
  5. List existing skills via system reminders; read /review
     in detail (G integrates into it)
  6. Look at `crates/` for existing audit/analysis crates and
     `scripts/` for analyzer scripts — match the existing style
  7. Inspect a few representative orphan cases via grep:
       - `selector_vocabulary_v2.rs` (C-10 — pub fns not in
         dispatch table)
       - `warm_state.rs` `drain_events` (C-43 — pub fn with
         zero non-test callers)
       - `reify-doc-tool` `build_doc_model` (C-25 — module exists,
         CLI uses stub)

## OPEN QUESTIONS TO RESOLVE

  Q-G-1. **Detection algorithm.** Static cross-crate
     reachability? Symbol-level call-graph from rust-analyzer?
     ripgrep-based heuristic ("function defined but only
     referenced in same module")? Each has tradeoffs.
     Simpler is better.
  Q-G-2. **What counts as a "production caller"?** Inside
     `crates/` but outside the defining module? Excludes
     `#[cfg(test)]`? Excludes example bins? Define precisely.
  Q-G-3. **Allow-list shape.** Some pub fns are legitimately
     library API surface even with no in-tree caller. How does
     the tool distinguish "intentionally exported for downstream
     consumers" from "orphan"? Annotation? Allowlist file?
     Convention (`pub(crate)` for non-API)?
  Q-G-4. **Output shape.** Report listing offenders ranked by
     severity? Per-crate summary? JSON for tooling? Markdown
     for humans?
  Q-G-5. **Invocation surface.** `cargo audit-orphans`?
     `reify-audit orphans`? `/review`'s "Phase 4 orphan-producer
     sweep"? All three?
  Q-G-6. **First-slice scope.** Probably just `crates/reify-*`
     in scope, exclude examples, exclude test fixtures, exclude
     vendored code. Confirm.
  Q-G-7. **False-positive handling.** When the tool reports a
     false positive (legitimate API surface), the fix is in the
     allow-list — but how is the allow-list maintained? Inline
     annotation on the fn? Separate file?
  Q-G-8. **Future extensions.** G's first version detects
     Type A orphans. Type B (consumer-with-stub) and Type C
     (both-built-not-bridged) are harder — should the tool's
     architecture leave room? Or is "Type A only" a stable
     scope?

## CONVERSATIONAL STYLE

- Leo wants terse, technical responses.
- Use AskUserQuestion for crisp design choices.
- Implementation should target the SIMPLEST mechanism that catches
  the 3 known cases + at least 5 others from the audit corpus.
  Resist over-engineering.
- Anti-gaming property is non-negotiable: G must run at corpus
  level, never inside a per-task worktree. If a design choice
  would let a task-level invocation game it, reject the choice.

## SESSION END

Stop when:
  1. Tool implementation at the agreed path runs successfully
     against the current codebase.
  2. Baseline report at
     `docs/architecture-audit/g-tool-baseline-report.md` captured.
  3. Test on 3+ known orphan cases confirms detection.
  4. Integration into /review or cargo xtask (or both) wired.
  5. Memory entry written.
  6. Hand-back paragraph to Leo with: invocation command, baseline
     orphan count, expected maintenance load.

Do NOT:
  - Try to detect Type B / Type C patterns in this first slice.
    Those are F's territory or a future G extension.
  - Modify gap-register.md or any audit artifact except to add
    the baseline report.
  - Commit unless Leo explicitly asks.

Hard cap: 150k tokens.
```

---

## Notes for Leo

- This is a single design+implement session because G's scope is tight (Type A only, corpus-level only).
- Expected session length: 90–180 minutes depending on whether the implementation lands as a Rust crate or a script.
- Run any time after F is at least designed (F's tracking infra may eat G's output, or vice versa — better if both designs are visible together).
- The baseline report this session produces will be useful as a regression marker: future runs should see the orphan count go down as the FEA stack unblocks and consumers wire up.
