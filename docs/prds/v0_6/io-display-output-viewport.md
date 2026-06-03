# PRD (forward-stub): `DisplayOutput` → viewport drive

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub · **Date:** 2026-06-03
**Parent:** `io-export-import-completion.md` §8 (deferred row 2). **Tracker:** task κ.

## Why deferred

The parent PRD declares the **`DisplayOutput`** occurrence + **`DisplayStyle`** structure as
type-checked surface, and the export driver *recognizes but skips* `DisplayOutput` with an
`I_DISPLAY_OUTPUT_DEFERRED` info diagnostic. Actually **driving the viewport** from a
`DisplayOutput` (subject → a chosen pane, with `style` = color/opacity/wireframe) is a **GUI seam**
the export PRD does not own.

## Substrate gap (verified 2026-06-03)

- The GUI mesh→viewport path (`gui/src-tauri/src/engine.rs:2124` `tessellate_snapshot` → `MeshData`,
  `types.rs:248`) carries **no per-mesh style** — color/opacity/wireframe are **client-side Three.js**,
  not on the backend `MeshData`.
- There are **two cached viewports** (`design-main`, `def-preview`), **not** arbitrary numbered
  **panes**. `DisplayOutput.pane : Int` has no backend target.
- Contested with in-flight GUI-rendering work (per overlay G4 known-pairs spirit) — resolve seam
  ownership with the GUI PRD before activating.

## Sketch (when activated)

1. Decide pane semantics: map `pane : Int` to existing/new viewport slots, or a tiled multi-pane.
2. Thread `DisplayStyle` (color/opacity/wireframe) onto `MeshData` (backend) → Three.js scene.
3. Replace the driver's `I_DISPLAY_OUTPUT_DEFERRED` skip with a real GUI dispatch
   (engine-integration-norm seam — likely a new viewport-drive seam → author a norm extension, G4).

## Pre-conditions for activating

- Parent `io-export-import-completion.md` landed (`DisplayOutput`/`DisplayStyle` surface +
  driver recognition point).
- GUI-rendering seam ownership resolved (who owns per-mesh style + panes).

## Decomposition (when activated — not filed now)

α `MeshData` style fields (backend) · β pane model · γ driver → viewport dispatch (replaces skip) ·
δ Three.js style application (frontend) · ε boundary test via reify-debug MCP (mesh count / style delta).
