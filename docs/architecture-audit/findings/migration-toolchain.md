# Audit: `reify migrate` and `#version` Migration Toolchain

**PRD path:** `docs/prds/v0_2/migration-toolchain.md`
**Auditor:** audit-migration-toolchain
**Date:** 2026-05-12
**Mechanism count:** 0 (skipped)
**Gap count:** 0 (skipped)

## Skipped: purely process

The audit-brief (§Boundaries) names this PRD by example as one that may be skipped:
"Skip the inventory if PRD is purely a process doc (e.g. migration-toolchain)."

Rationale, verified by reading the PRD end-to-end:

- The PRD is explicitly **deferred to v0.2+** (header line 3). The 2026-04-28 review
  notes v0.2 has shaped up with no language-surface breaks, so even v0.2 likely won't
  trigger activation — "Likely no first migration step needed until v0.3+ ships a
  breaking change."
- All runtime mechanisms the PRD describes (`reify migrate` CLI subcommand,
  version-gated grammar dispatch, per-version migration rewriter modules, migration
  guide docs) are intentionally not built yet. The PRD itself is a placeholder
  awaiting two open prerequisites (grammar-dispatch infra decision, migration
  ownership decision — listed under "Pre-conditions for activating").
- The one mechanism that **is** live today — parser acceptance of the
  `#version(...)` pragma as an advisory no-op — is by design. The PRD states: "v0.1
  ships the pragma syntax and accepts it but treats it as advisory; v0.2+ activates
  it." Quick code check confirms the pragma is stored
  (`crates/reify-compiler/src/module_pragmas.rs`,
  `crates/reify-compiler/tests/pragma_compile_tests.rs`) without semantic
  enforcement, which matches the PRD's deferred-by-design contract. There is no
  runtime contract to verify or contradict.
- No fictional mechanism is implied as currently present. The PRD does not assume any
  consumer code calls `reify migrate` today; no downstream PRDs depend on the
  rewriter or version-gated parser existing in v0.1.

Because every runtime mechanism the PRD describes is explicitly future / deferred,
there is nothing to classify against the WIRED/PARTIAL/TODO/FICTION/DRIFT/ORPHAN
catalog. The PRD's purpose is process / scope documentation, not specification of
behavior that should be present now.

## Cross-PRD breadcrumbs

- The pragma-parsing no-op is shared with whatever PRD originally documented v0.1
  pragma syntax (spec §14.2 referenced, not a sibling PRD). No active gap.
- If a future v0.3 breaking change activates this PRD, the gap register will need
  to track: (a) version-gated grammar dispatch mechanism, (b) `reify migrate`
  CLI entry point, (c) per-version rewriter module pattern. None are gaps **today**.

