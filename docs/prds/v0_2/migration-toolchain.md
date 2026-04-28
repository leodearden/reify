# PRD: `reify migrate` and `#version` Migration Toolchain

Status: deferred to v0.2+ per 2026-04-26 decision.
2026-04-28 review: v0.2 has shaped up as runtime/architecture changes (multi-kernel dispatch, per-purpose tolerance, persistent-naming v2) with **no language-surface breaks**. Likely no first migration step needed until v0.3+ ships a breaking change. PRD remains a placeholder; the two open questions (grammar-dispatch infra, migration ownership) are easier to answer when a concrete breaking change is on the table, so we defer resolving them until then.

## Goal

Implement the migration toolchain described in `docs/reify-language-spec.md` §14: a `reify migrate --from <ver> --to <ver>` CLI that transforms `.ri` source files across breaking language changes, plus full activation of the `#version(...)` pragma for version-gated parsing. v0.1 ships the pragma syntax and accepts it but treats it as advisory; v0.2+ activates it.

## Background

Spec §14.3 declares v0.1 a draft specification with no backwards-compatibility guarantees. §14.5 commits the toolchain to providing migration support when breaking changes ship: a diagnostic identifying the affected construct and migration path, plus an automated migration tool where feasible.

`#version(0.1)` is parsed by v0.1 but not enforced — §14.2 explicitly notes "full version-gated parsing is deferred." This is the right call for v0.1: there's nothing to migrate from yet, and version-gated parsing only matters once multiple versions exist in the wild.

The toolchain becomes relevant the first time a breaking change ships in v0.2. At that point users need: a way to tell the parser "this file targets v0.1" (so old grammar/semantics are honored), and an automated rewriter for the cases where the breaking change has a mechanical fix.

## Why deferred

- v0.1 is the first version. There is nothing to migrate from. Building the migration toolchain before any breaking change exists is premature.
- The shape of the toolchain depends entirely on what changes between v0.1 and v0.2. We can't design a generic migration framework without concrete migration cases.
- `#version(0.1)` is already accepted by the parser as a no-op, so v0.2+ can activate it without source-level breaking changes.

## Sketch of approach

The toolchain has two parts that activate together when v0.2 ships its first breaking change:

**Version-gated parsing.** The parser reads the `#version(...)` pragma (or defaults to current toolchain version if absent) and dispatches to a version-specific grammar/elaboration pipeline. v0.1 grammar lives forever as a "v0.1 mode" path; v0.2 grammar is the default. Most files target one version per project, so the per-file dispatch overhead is minimal. The diagnostic for an unsupported version names the version and points at `reify migrate`.

**`reify migrate` rewriter.** For each pair of adjacent versions (`0.1 → 0.2`, `0.2 → 0.3`, ...) the toolchain ships a migration module — a tree-sitter-based rewriter that handles the mechanical breaking changes for that step. Migrations chain: `--from 0.1 --to 0.3` runs `0.1 → 0.2` then `0.2 → 0.3`. Each migration documents what it changes, what it leaves alone, and what manual review is recommended for. Migrations are not lossless on every change — some breaking changes (e.g. semantic changes to `auto` resolution) cannot be fixed mechanically and the migration tool can only flag affected sites.

A migration guide is published with each minor release listing every breaking change, the rewriter coverage, and manual-review instructions. This is committed to the repo at `docs/migrations/0.1-to-0.2.md`, etc.

## Pre-conditions for activating

- v0.2 has at least one concrete breaking change that needs migration (e.g. a syntax change, a removed annotation, a renamed stdlib trait).
- The grammar dispatch infrastructure is decided — is "v0.1 mode" a separate parser, or feature flags in the v0.2 parser?
- A decision has been made on whether migrations are owned by the toolchain repo or shipped as separate cargo crates.

## Out of scope for this PRD

- Any migration content for a 0.1 → 0.2 step (depends entirely on what v0.2 actually changes).
- Bidirectional migration (0.2 → 0.1 downgrade is not in scope; migrations are forward-only).
- Migration of project metadata files (`reify.toml`, etc.) — the spec only covers source files.
- IDE integration of migrations (LSP-level "fix it" actions, post-v0.2).
