---
name: reify-design
description: Author and iterate on parametric engineering designs in the Reify DSL (`.ri` files). Use this skill whenever the user asks to design, model, sketch, refine, or modify a parametric part or assembly in Reify — e.g. "design a flange", "model a bracket in reify", "add a fillet to this .ri file", "tweak the parameters", "iterate on this design", "make me a parametric X", or just opens a `.ri` file and asks to extend or refactor it. Trigger even if the user doesn't say "reify" explicitly, as long as a `.ri` file or parametric-design intent is in scope. Do NOT use for Rust kernel work, compiler/solver internals, or anything under `crates/` — this is a design-author skill, not a Reify-maintainer skill.
---

# Reify Design

Help the user author parametric designs in `.ri` files. Be terse and focus on getting the design right and iterating fast — skip recapping basic syntax unless the user asks.

## Where things live

All paths below are relative to the Reify repo root (find it via the working directory or `git rev-parse --show-toplevel` if unsure).

- **Language reference (authoritative):** `crates/reify-mcp/src/tools/chunks/*.md` — one file per topic: `structures`, `syntax`, `types`, `units`, `geometry`, `traits`, `parameters`, `constraints`, `enums`, `fields`, `occurrences`, `connect`, `collections`, `functions`, `guards`, `purposes`, `stdlib`. Read the relevant chunk when unsure — these are the same chunks the in-GUI assistant exposes via `reify_language_reference`.
- **Examples:** `examples/*.ri`. Canonical patterns:
  - `m5_geometry_flange.ri` — `structure def ... : Rigid`, params with units, `cylinder` / `circular_pattern` / `difference`
  - `m8_units.ri`, `m8_materials.ri`, `m8_tolerancing.ri` — units, materials, tolerances
  - `m10_geometric_types.ri`, `m10_combined.ri` — geometric types in use
  - `bearing_auto_seal.ri` — `auto` + constraint-driven sizing
  - `dimensional_chains.ri`, `pattern_composition.ri` — composition idioms
  - `m5_connect_chain.ri` — `connect a.port <-> b.port`
- **Embedded GUI prompt (reference only):** `gui/sidecar/src/system-prompt.ts`. Slightly stale vs. the chunks — trust the chunks where they disagree (e.g. `structure def` vs `structure`, `and/or/not` vs `&&/||/!`).

## Reify syntax — lean cheatsheet

```reify
structure def Bracket<M: Material> : Rigid {
    param thickness : Length = 5mm
    param width     : Length = 80mm
    param material  : M

    sub rib : Rib { height = thickness * 0.8 }

    let volume = thickness * width * width
    let body   = box(width, width, thickness)

    constraint thickness > 1mm
    constraint thickness < width / 4

    auto fillet_radius : Length

    port mount : MechanicalPort { direction = in }
}
```

Things that are easy to get wrong (the embedded GUI prompt has old forms — these are right):

- **Declaration keyword:** `structure def Name`, `enum def Name`, `trait def Name`. Not bare `structure Name`.
- **Identifiers:** `snake_case` for params/lets/ports/subs/values, `PascalCase` for structures/traits/types.
- **Logic ops:** `and`, `or`, `not`, `implies`. Not `&&`, `||`, `!`.
- **Conditional:** `if cond then a else b`. Not `if c { a } else { b }`.
- **Quantities:** number + unit, no space — `80mm`, `90deg`, `2.5kg`, `1.5e-3m`. *Always* units on physical quantities.
- **Ranges:** `2mm..5mm`, `0deg..<360deg`, `>2mm`, `<=100MPa`.
- **Specials:** `undef` (not yet decided), `auto` (solver decides), `some(v)` / `none`.
- **Member kinds:** `param` (public input), `let` (derived), `auto` (solver-determined), `constraint` (predicate), `sub` (sub-entity instance), `port`, `connect a.port <-> b.port`, `type` (alias), `meta { ... }` (informational only, no constraint participation).

## Workflow

### 1. Read before writing

When extending an existing `.ri` file:
1. Read it.
2. If the GUI is already up with debug enabled, call `mcp__reify-debug__engine_state` to see current diagnostics.
3. If unsure about a syntax form, grep `examples/` (`rg 'circular_pattern' examples/`) or read the relevant chunk in `crates/reify-mcp/src/tools/chunks/`.
4. Prefer editing existing files over creating new ones. Start a new file only when the user is genuinely starting a fresh design.

### 2. Iterate visually

Reify ships a GUI with a debug MCP for visual verification. Two launch scripts (both auto-set `LD_LIBRARY_PATH` for OCCT's bundled libs):

- **Dev (HMR + debug MCP):** `scripts/run-gui-dev.sh <file.ri>` — vite on `:1420`, debug MCP on `127.0.0.1:${REIFY_DEBUG_PORT:-3939}`. Set `REIFY_DEBUG_PORT` per worktree to avoid port collisions. Use this when iterating.
- **Release:** `scripts/run-gui.sh <file.ri>` — what end users will see.

If `reify` is built: `reify gui --debug <file.ri>` (alias `reify gui-debug <file.ri>`).

When a GUI is running with `REIFY_DEBUG=1`, the `mcp__reify-debug__*` tools are available:

| Tool | Use for |
|------|---------|
| `health` | Confirm the listener is up |
| `open_file` | Switch the GUI to a `.ri` file |
| `editor_content` / `type_in_editor` | Read or replace the editor buffer |
| `engine_state` / `mesh_stats` | What did the engine actually evaluate? Errors, mesh sizes |
| `viewport_state` / `set_camera` / `fit_to_view` | Frame the viewport |
| `screenshot` | Capture the viewport (html-to-image over Three.js) |
| `wait_for_idle` | Block until the engine settles after an edit |
| `select_entity` / `list_elements` / `dom_query` | Inspect the rendered scene |
| `store_state` | Snapshot the Solid store |
| `set_test_mode` | Disable transitions for stable screenshots |
| `keyboard` / `click_element` | Drive the UI |

Typical loop: edit `.ri` → `wait_for_idle` → check `engine_state` and `mesh_stats` for errors → `screenshot` for visual confirmation → adjust.

If the GUI isn't running and the iteration is non-trivial, ask the user whether to start `scripts/run-gui-dev.sh` rather than launching unprompted — it's a foreground process tied to the terminal.

### 3. Design quality

- **Always units** on physical quantities. `param width : Length = 80mm`, never `param width = 80`.
- **Pair parameters with constraints** that express their valid range — minimum wall, fillet ≥ tool radius, hole-to-edge clearance, aspect ratios. New `param` without a `constraint` is usually a smell.
- **Use `auto`** for values the solver should determine (fillet sized by stress, fit driven by tolerances). Use `constraint` to express the relationships the solver must satisfy.
- **Trait conformance** (`: Rigid`, `: Physical`, `: MaterialSpec`) requires the structure to declare the trait's required members — see the `traits` chunk and `m5_geometry_flange.ri` for the concrete pattern (Material struct, density, moment_of_inertia, etc.).
- **Sub-component composition** is preferred over monolithic geometry when a feature has independent meaning. Use `sub`, `connect`, and ports rather than embedding everything in one structure's `let body = ...`.

## What this skill is *not* for

Don't trigger this skill for:

- Editing Rust code under `crates/` (kernel, eval, compiler, FEA solver, MCP server, GUI Tauri shell)
- Building / testing the Reify toolchain itself
- Investigating compiler bugs, solver convergence, kernel issues, or task / orchestrator workflow

Those are Reify-maintainer tasks. This skill is purely about authoring `.ri` source as a designer.
