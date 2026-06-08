# Getting Started with Reify

This walks through editing your first `.ri` file, re-running it, and opening the GUI. Assumes you've already finished the `Install` section in the top-level [README](../README.md).

## 1. Sanity check

```sh
./target/release/reify check examples/m5_geometry.ri
```

You should see:

```
  OK Flange#constraint[0]
  OK Flange#constraint[1]
All constraints satisfied.
```

## 2. Build a STEP file

```sh
./target/release/reify build examples/m5_geometry.ri -o /tmp/flange.step
```

Open `/tmp/flange.step` in any STEP viewer (FreeCAD, KiCad's 3D viewer, Online ones). It's a simple cylindrical flange — 50 mm radius, 10 mm tall.

## 3. Edit a parameter

`examples/m5_geometry.ri` looks like:

```
structure def Flange {
    param radius : Scalar = 50mm
    param height : Scalar = 10mm
    param hole_count : Int = 6
    param bolt_circle_radius : Scalar = 35mm
    param hole_radius : Scalar = 3mm

    constraint radius > bolt_circle_radius
    constraint hole_count > 0

    let body = cylinder(radius, height)
}
```

Try changing `radius : Scalar = 50mm` to `200mm`. Re-run `reify build` and open the new STEP — bigger flange. Try setting `radius` smaller than `bolt_circle_radius` — `reify check` will tell you which constraint failed.

The `mm` suffix is meaningful: parameters are dimensioned. Try `radius : Scalar = 50` (no unit) — `reify check` will flag a dimensional mismatch against `bolt_circle_radius`'s 35mm.

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
| `bracket.ri` | A more elaborate parametric part |
| `m8_units.ri` | Dimensional analysis examples |
| `m8_tolerancing.ri` | Tolerance propagation |
| `m9_constraint_def.ri` | Defining custom constraints |
| `m11_field_calculus.ri` | Field operations (gradient, divergence) |
| `large_assembly.ri` | Multi-part assembly |
| `kinematic/` | Kinematic linkages (closed chains, joints) |
| `topology_selectors/` | Persistent face/edge naming |

If one fails to parse or check, that's a real bug — please report it (see [README "Feedback"](../README.md#feedback)).

## 6. Next reading

- [Language spec](reify-language-spec.md) — full grammar and semantics
- [Stdlib reference](reify-stdlib-reference.md) — built-in geometry, types, traits
- [Implementation architecture](reify-implementation-architecture.md) — how the engine fits together

## Common issues

**`cargo build` fails on `reify-kernel-occt`** — OCCT 7.8 is missing or wrong version. Re-run `scripts/setup-dev.sh`; it adds the FreeCAD PPA and pins to 7.8.

**GUI fails to launch with a webkit/gtk linker error** — Tauri 2 webview deps missing. `scripts/setup-dev.sh` installs them; re-run it.

**`reify build` hangs for minutes** — first run on a fresh checkout includes a tree-sitter generate (60s) and OCCT C++ compile (a few minutes). Subsequent runs are fast.

**`reify check` reports "Parse error"** — the `.ri` file uses a feature that's not yet stable. Try `examples/m5_geometry.ri` to confirm your install is healthy, then file an issue with the failing file.
