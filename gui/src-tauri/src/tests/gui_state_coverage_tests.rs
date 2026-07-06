// INV-GUI-1 interim enforcement (warn-mode): every `GuiState` field must be
// either classified with a sync mechanism or named on the known-stale
// allowlist. See docs/prds/v0_6/gui-state-sync.md §8 L1.
//
// This lint is interim scaffolding — L5 (per the PRD) retires it wholesale
// once the derive-based mechanism lands. Everything it needs (checker fn,
// classification table, allowlist, fixture, tests) lives in this one file so
// deletion is a single `rm` + removing its `mod` line from tests/mod.rs.
//
// Tests are progressively added across steps 1–10 of the plan. This file is
// intentionally sparse at pre-1 — content grows with each step.
