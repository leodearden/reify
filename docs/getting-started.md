# Getting Started with Reify

This walks through editing your first `.ri` file, re-running it, and opening the GUI. Assumes you've already finished the `Install` section in the top-level [README](../README.md).

## 1. Sanity check

```sh
./target/release/reify check examples/m5_geometry.ri
```

You should see:

```
warning: W_MODULE_DECL_MISSING: file has no top-of-file `module` declaration; expected `module m5_geometry` (spec §7.1)
  OK Flange#constraint[0]
  OK Flange#constraint[1]
All constraints satisfied.
```

The warning is harmless: the example has no `module` line.

## 2. Build a STEP file

```sh
./target/release/reify build examples/m5_geometry.ri -o /tmp/flange.step
```

Open `/tmp/flange.step` in any STEP viewer (FreeCAD, KiCad's 3D viewer, Online ones). It's a simple cylindrical flange — 50 mm radius, 10 mm tall.

## 3. Edit a parameter

`examples/m5_geometry.ri` looks like:

```
structure def Flange {
    param radius : Length = 50mm
    param height : Length = 10mm
    param hole_count : Int = 6
    param bolt_circle_radius : Length = 35mm
    param hole_radius : Length = 3mm

    constraint radius > bolt_circle_radius
    constraint hole_count > 0

    let body = cylinder(radius, height)
}
```

Try changing `radius : Length = 50mm` to `200mm`. Re-run `reify build` and open the new STEP — bigger flange. Try setting `radius` smaller than `bolt_circle_radius` — `reify check` will tell you which constraint failed.

The `mm` suffix is meaningful: the unit is part of the type. Try `radius : Length = 50` (no unit) — `reify check` rejects the file before checking anything, and exits 1:

```
error: argument 'radius' has type 'Int' but param 'radius' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m`
```

### An unfinished input is indeterminate, not a failure

Delete the default instead (`param radius : Length`). `radius` now has no value, so the constraint that reads it cannot be decided either way:

```
  INDETERMINATE Flange#constraint[0]: undefined inputs: Flange.radius
  OK Flange#constraint[1]
warning: constraint Flange#constraint[0] indeterminate: undefined inputs: Flange.radius
error: failed to compile geometry operation: argument 'radius' for cylinder is unresolved (Undef)
error: all geometry operations failed; no geometry output produced
No constraints violated (1 indeterminate).
```

Plain `reify check` still exits 0. An unbound parameter is an *expected* cause of indeterminacy, because the design is deliberately unfinished (INV-SF-4 in [design-invariants](legibility/design-invariants.md)). The geometry errors are the cylinder that cannot be built without a radius. `reify check --strict` fails, exit 1, on any INDETERMINATE constraint:

```
Strict check failed: 1 constraint(s) INDETERMINATE:
  Flange#constraint[0]: undefined inputs: Flange.radius
```

### Let the solver choose a value

Write `param radius : Length = auto` to let the solver pick `radius` from the constraints. With only `constraint radius > bolt_circle_radius`, `reify check` refuses, exit 1, and names the missing bound:

```
error: strict auto parameter resolution is not uniquely determined: no constraint bounds `Flange.radius` above. Only the constraints declared in `Flange` take part in its solve, and they bound it only below, so a solve could only return the edge of the solver's internal default domain — add the missing bound in `Flange`, give `Flange` an objective, or declare it `auto(free)`
```

Add an upper bound, `constraint radius < 80mm`, and it resolves. `reify check` prints three `OK` lines and `All constraints satisfied.`, and `reify eval` prints `Flange.radius = 0.0575 m`. With no objective, the solver picks the centre of the (35mm, 80mm) bracket; a `minimize` or `maximize` objective picks a different point.

`check` (including `--strict`), `test`, `build`, `eval` / `run`, `report`, `explain` and the GUI viewport all resolve `auto`. The editor (language-server diagnostics and hover) and the GUI's definition-preview pane do not yet. Every place `auto` may appear, and what to do when it does not resolve: the `solving` chunk of the language reference ([`crates/reify-mcp/src/tools/chunks/solving.md`](../crates/reify-mcp/src/tools/chunks/solving.md)) and [spec §9.3](reify-language-spec.md#93-auto-resolution).

## 4. Open the GUI

```sh
scripts/run-gui.sh examples/m5_geometry.ri
```

The GUI shows a 3D viewer, an outline of the design, parameter sliders, and a constraints panel. Edit a parameter — the geometry updates live.

If you want hot-reload of frontend changes (e.g. you're poking at the SolidJS code), use `scripts/run-gui-dev.sh` instead. It also opens an MCP debug listener on `127.0.0.1:${REIFY_DEBUG_PORT:-3939}` for instrumented automation. Set `REIFY_DEBUG_PORT` to a different port to avoid collisions when running concurrent worktrees.

## 5. Try other examples

The 50+ files under `examples/` exercise different language features:

| File | What it shows |
|------|---------------|
| `m5_geometry.ri` | Simplest cylinder; the smoke-test baseline |
| `continuous_cost_min.ri` | Letting the solver size a dimension (`auto`) under a cost objective |
| `bracket.ri` | A more elaborate parametric part |
| `m8_units.ri` | Dimensional analysis examples |
| `m8_tolerancing.ri` | Tolerance propagation |
| `m9_constraint_def.ri` | Defining custom constraints |
| `m11_field_calculus.ri` | Field operations (gradient, divergence) |
| `large_assembly.ri` | Multi-part assembly |
| `kinematic/` | Kinematic linkages (closed chains, joints) |
| `topology_selectors/` | Persistent face/edge naming |
| `multi_load_bracket.ri` | Multi-load-case FEA: 3 load cases, stress envelope, LRFD superposition |

If one fails to parse or check, that's a real bug — please report it (see [README "Feedback"](../README.md#feedback)).

## 6. Next reading

- [Language spec](reify-language-spec.md) — full grammar and semantics
- [Stdlib reference](reify-stdlib-reference.md) — built-in geometry, types, traits
- [Implementation architecture](reify-implementation-architecture.md) — how the engine fits together
- [Multi-load-case FEA](stdlib/multi-load-fea.md) — multi-case solver, stress envelopes, `linear_combine` superposition

## Common issues

**`cargo build` fails on `reify-kernel-occt`** — OCCT 7.8 is missing or wrong version. Re-run `scripts/setup-dev.sh`; it adds the FreeCAD PPA and pins to 7.8.

**GUI fails to launch with a webkit/gtk linker error** — Tauri 2 webview deps missing. `scripts/setup-dev.sh` installs them; re-run it.

**`reify build` hangs for minutes** — first run on a fresh checkout includes a tree-sitter generate (60s) and OCCT C++ compile (a few minutes). Subsequent runs are fast.

**`reify check` reports "Parse error"** — the `.ri` file uses a feature that's not yet stable. Try `examples/m5_geometry.ri` to confirm your install is healthy, then file an issue with the failing file.
