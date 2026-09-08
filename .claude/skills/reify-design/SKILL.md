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
- **Best-practices corpus:** `examples/best_practices/` — small single-idiom exemplars, one per easy-to-get-wrong construct, each stating the anti-pattern it replaces. Catalogued in `examples/best_practices/INDEX.md`; **grep that index before probing the language**. Everything there is compile-gated, so the idioms are known to work.
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

    param fillet_radius : Length = auto

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
- **Member kinds:** `param` (public input), `let` (derived), `constraint` (predicate), `sub` (sub-entity instance), `port`, `connect a.port <-> b.port`, `type` (alias), `meta { ... }` (informational only, no constraint participation).
- **`auto` is a binding VALUE, not a declaration keyword.** Solver-determined members are ordinary `param`/`let` whose value is `auto`: `param fillet_radius : Length = auto`, `let r : Length = auto`, `param s : Real = auto(free)`. There is no `auto` member kind — `auto fillet_radius : Length` is a **parse error**. `auto` is also illegal in operand position (`x + auto`), which raises `E_AUTO_NOT_AT_BINDING_SITE`. Use `auto(free)` when the constraints admit more than one solution: strict `auto` runs a uniqueness re-solve and goes `undef` if it cannot converge.

### Probe-verified idioms — index (2026-07-24)

One line per idiom. Worked, compile-gated exemplars live in `examples/best_practices/` —
**read the exemplar rather than re-probing**. Second-level index with the full list:
`examples/best_practices/INDEX.md`.

- **Unary minus** works everywhere: `-x`, never `0mm - x`. → `negation.ri`
- **Hollow/centered primitives**: `tube(outer_r, inner_r, h)`, `cylinder_centered` — don't
  difference two cylinders. (`tube` is base-at-z=0 and needs `inner_r < outer_r`;
  `box_centered` is an op-identical alias for `box`.) → `hollow_primitives.ri`
- **Symmetric parts**: `mirror` returns a reflected copy — `let twin = union(g, mirror(g, plane_yz(0mm)))`.
  Plane ctors take exactly one offset arg; the 7-arg scalar form needs a *dimensioned*
  origin (`0mm`, never bare `0`), and `reify check` will not tell you when it doesn't.
  → `symmetry_mirror.ri`
- **Bolt circles**: `circular_pattern(hole, axis_z(point3(…)), n, 360deg)` — angle is the TOTAL
  sweep (step = total/count). Never construct geometry inside `generate` lambdas: silent `undef`
  under a green `check` (#5385); lambdas may only compute `point3`/scalar values.
  → `bolt_circle.ri`
- **Imports** resolve under `reify check` + GUI; `eval`/`build` are still single-file — keep
  eval/build entry files self-contained.
- **Interference/clearance oracle exists — run it before shipping an assembly**:
  `intersects(a, b)`/`distance(a, b)` on let-bound geometry (low ceremony), or
  `mechanism`/`snapshot`/`min_clearance` (assembly-grade). Eval/build only — `reify check`
  reports these INDETERMINATE, which is expected. → `clearance_oracle.ri`; assembly-grade
  worked example: `examples/tolerancing/vc_bolt_pattern_clearance.ri`.
- **Discrete choices**: until CP-SAT is wired, `param s : Real = auto(free)` +
  `constraint s*s == 1` (s = ±1). Note `auto` is a binding *value* — `auto s : Real` is a
  parse error — and strict `auto` goes `undef` here because two roots defeat the uniqueness
  re-solve. → `discrete_choice.ri`
- **Turning an arc-measure ratio into an angle — and why the `* 1rad`**:
  `let theta : Angle = (s/r) * 1rad` enters Angle, `theta / 1rad` leaves; arc length is
  `r * theta / 1rad`. No-space literal only (`1 rad` is a parse error). Not optional:
  `let theta : Angle = s / r` and `let arc : Length = r * theta` are hard errors; unannotated
  `let arc = r * theta` silently yields `m·rad`. Arc-measure ratios only — a *trigonometric*
  ratio needs no crossing: `atan`/`atan2`/`asin`/`acos` and the `angle`/`angle_between_surfaces`
  queries return `Angle`, and annotated `let bad : Angle = atan(o/a) * 1rad` is a hard error
  (declares `rad`, computes `rad^2`) — unannotated, or inside the call as `atan((o/a) * 1rad)`,
  it is silent instead. Both readings typecheck, so the wrong one is silent. `omega = 2*pi * f * 1rad` is a
  separate class (2π rad/cycle; no `cycle` unit). → `angle_crossings.ri`

A green `reify check` is weaker than it looks: geometry-argument dimension errors and
wrong-arity datum constructors (`plane_yz(0mm, 0mm)`, `axis_z(vec3(…))`) produce no
check-time diagnostic at all — the first silently fails at build, the second evaluates to
`undef`. Run `reify eval` before believing a geometry expression is right.

## Workflow

### 1. Read before writing

When extending an existing `.ri` file:
1. Read it.
2. If the GUI is already up with debug enabled, call `mcp__reify-debug__engine_state` to see current diagnostics.
3. If unsure about a syntax form, grep `examples/` (`rg 'circular_pattern' examples/`) or read the relevant chunk in `crates/reify-mcp/src/tools/chunks/`.
4. Prefer editing existing files over creating new ones. Start a new file only when the user is genuinely starting a fresh design.

### 2. Iterate visually

Reify ships a GUI with a debug MCP for visual verification. Two launch scripts (both auto-set `LD_LIBRARY_PATH` for OCCT's bundled libs, prepend `/opt/reify-deps/tbb-pin` ahead of any inherited `LD_LIBRARY_PATH`, and default `WEBKIT_DISABLE_DMABUF_RENDERER=1`; both refuse fast with no display — `REIFY_GUI_SKIP_PREFLIGHT=1` bypasses):

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

### 4. Session wrap — graduate your probes

At the end of a design session, spend a couple of minutes turning what you
learned about the *language* into something the next session can grep.

**Why this is worth doing.** The printer_v01 dogfood session spent ~40% of its
CLI verification runs (13 of 19) on probe files interrogating language
semantics rather than the design itself, and the 2026-07-24 probe wave wrote
~25 more. Nearly every one of those findings would have been a single grep away
if a prior session had preserved its probes. This step is how that stops
repeating.

1. **Identify this session's probes.** A probe is a throwaway `.ri` file
   written purely to interrogate language or stdlib semantics — "does `mirror`
   take a plane or an axis?", "what arity does `tube` want?". The user's actual
   design files are **never** graduated, no matter how instructive they were.

2. **Discard what's already covered.** Grep `examples/best_practices/INDEX.md`
   first. If the idiom is already there, the probe has served its purpose —
   delete it. That grep is the entire point of the index; do not skip it and
   add a near-duplicate exemplar.

3. **Minimise what's left.** For each genuinely new finding, reduce the probe
   to the smallest file that still demonstrates the idiom, then:
   - rename it to an idiom-descriptive `snake_case` name (`symmetry_mirror.ri`,
     not `probe3.ri`);
   - add a `module <file_stem>` decl — the module path **must** match the file
     stem or you get `E_MODULE_PATH_MISMATCH`;
   - add a header comment stating the idiom **and the anti-pattern it
     replaces** — the anti-pattern is what makes it findable by someone who
     doesn't yet know the right answer;
   - drop it in `examples/best_practices/`.

4. **Add its INDEX.md row — not optional.** `examples_smoke.rs`'s
   `best_practices_index_matches_corpus_directory` fails the build if a corpus
   file has no index entry, or an entry names a missing file. File and row land
   in one commit.

5. **Verify before you commit:**
   ```sh
   cargo test -p reify-compiler --test harness_compilation_surface examples_smoke::
   ```
   Also run `reify eval` on the new file, not just `reify check` — check is
   silent about several classes of geometry error (see the note at the end of
   the idiom index).

**A file that cannot reach a clean compile must NOT be added.** The corpus is
compile-gated by construction, and an exemplar that doesn't work is worse than
no exemplar. Do not add a `SKIP_SET` entry to get one in. If the finding is
that something is *broken*, that is a bug report, not an exemplar.

## What this skill is *not* for

Don't trigger this skill for:

- Editing Rust code under `crates/` (kernel, eval, compiler, FEA solver, MCP server, GUI Tauri shell)
- Building / testing the Reify toolchain itself
- Investigating compiler bugs, solver convergence, kernel issues, or task / orchestrator workflow

Those are Reify-maintainer tasks. This skill is purely about authoring `.ri` source as a designer.
