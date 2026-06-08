# Capability manifest — io-export-import-completion

Mechanizes G3 + G6 per leaf (gates.md). Each binding: capability → evidence. Any binding resolving
to `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor`
**blocks the batch**. Empty-value sentinel = `Value::Undef`. Grammar evidence = `reify check` on a
committed fixture (real binary; tree-sitter CLI treated as stale per memory). All file:line verified
2026-06-03 against `target/release/reify` + source tree.

Verdict: **all bindings PASS** — no FAIL, no blocker.

---

## α — std.io.formats declarative surface (intermediate)

| Capability | Evidence | Verdict |
|---|---|---|
| `occurrence def` parses + type-checks | grammar-fixture `/tmp/prd-gate-fixtures/io-formats-real.ri` → `reify check` exit 0 (export surface); grammar.js:489 `occurrence_definition` | PASS |
| qualified enum default `= STEPVersion.AP214` / `= OutputFormat.STL` | same fixture, no error on those lines | PASS |
| `subject : Solid` resolves | `type_resolution.rs:563` `"Solid" => Type::Geometry`; fixture compiles | PASS |
| `constraint determined(subject)` (direct ref) | fixture compiles (only member-access `subject.geometry` fails — not used) | PASS |
| `STEPInput : Input` w/ concrete `source`/`provenance`/`Provenance(...)` defaults | fixture `/tmp/prd-gate-fixtures/io-stepinput.ri` → "All constraints satisfied" | PASS |
| **NOT** using `= undef` on occurrence params | confirmed `unresolved name: undef` on occurrence param → design avoids it (concrete defaults) | PASS (avoided) |
| consumer named (intermediate) | unlocks δ (driver), ε (version), ζ (import) — all in batch | PASS |

## β — STL writer + kernel export() wiring (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| `Mesh { vertices, indices, normals }` to serialize | `reify-ir/src/geometry.rs:1485-1494` wired (produced by tessellate) | PASS (grep wired) |
| tessellation egress on a solid | `OcctKernel::tessellate` `reify-kernel-occt/src/lib.rs:2989-3006` → `occt_wrapper.cpp:4328` `tessellate_shape` (BRepMesh) — production path (GUI uses it) | PASS (grep wired) |
| `ExportFormat::Stl` exists to dispatch on | `reify-ir/src/geometry.rs:1481` | PASS |
| kernel `export()` dispatch site (Stl arm to add) | `reify-kernel-occt/src/lib.rs:2964-2987` (the `_ =>` reject at :2982 is what we replace) | PASS (site exists, wired into build via :2498) |
| CLI `.stl → ExportFormat::Stl` already maps | `reify-cli/src/main.rs:541-548` | PASS |
| **G6:** binary STL size = 84+50·N, N>0, AABB ≈ box | tessellate(box) works today; size is STL-spec identity; producible from β alone (no downstream dep) | PASS (premise true + producible) |

## γ — 3MF writer + ExportFormat::ThreeMF (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| `ExportFormat` is extensible (add `ThreeMF`) | `reify-ir/src/geometry.rs:1479` enum; exhaustive matches enumerated in §7.1 | PASS |
| demanded-repr arm to add (`ThreeMF→Mesh`) | `engine_build.rs:1643-1646` `demanded_reprs_for_template` match | PASS (site exists) |
| `zip` container dep | new `Cargo.toml` dep (queued in-task, std OPC packaging) | PASS (additive dep, in leaf scope) |
| Mesh source (shared w/ β) | as β | PASS |
| **G6:** valid OPC zip, `<triangle>` count = N | 3MF/OPC fixed schema + tessellation; producible from β+γ | PASS |
| **G6:** `W_3MF_NO_MATERIALS` when materials requested w/o data | warning is observable; no false "materials written" claim (honest gate, not silent no-op) | PASS |
| prereq | β upstream (DAG-direction OK) | PASS |

## δ — Output-occurrence export driver + relative paths + --out-dir (leaf, H gate)

| Capability | Evidence | Verdict |
|---|---|---|
| enumerate `: Output` occurrence **instances** in realized snapshot | occurrence eval → value cells (`reify-eval/tests/occurrence_eval.rs`); template/trait-bound metadata in `CompiledModule.templates`; **driver is NEW code composing existing wired substrate** | PASS (substrate wired; new walk) |
| recognize Output + read its params (precedent to extend) | `extract_output_tolerance_bound` `tolerance_combine.rs:129-211` (task #5 done) reads Output template + tolerance; extend to read `format`/`path` | PASS (grep wired, extend) |
| resolve `subject` → geometry handle | `surface_export_bodies` `geometry_ops.rs:5143` + `named_steps`/`GeomRef::Sub` threading `engine_build.rs:2211` (task #3441) | PASS (grep wired) |
| kernel `export()` for Stl/Step | β (Stl) + existing Step (`lib.rs:2976`) — **upstream** | PASS (β upstream) |
| design-file dir known at build time | CLI has the `<file>` arg (`main.rs` build path); join is deterministic | PASS |
| **G6:** DSL drives format+path w/o `-o`/extension | every required capability (α surface, β STL, value-map, surface_export_bodies) is **upstream or existing** — none owned by a downstream task | PASS (no DAG inversion) |
| prereqs | α, β upstream | PASS |

## ε — STEPVersion → STEP schema (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| STEP writer to parameterize | `occt_wrapper.cpp:4269-4290` `export_step` (STEPControl_Writer) | PASS (grep wired) |
| OCCT schema selection | `Interface_Static::SetCVal("write.step.schema", …)` is a real OCCT static param (AP203/AP214 first-class) | PASS |
| AP242 honesty | best-effort `"AP242DIS"`; fallback → `W_STEP_AP242_FALLBACK` warning (no silent lie) | PASS (degradation documented) |
| version param reaches export | via δ driver (reads `version` off STEPOutput) | PASS (δ upstream) |
| **G6:** `FILE_SCHEMA` contains AP203 | OCCT emits the schema id in the STEP header; observable by grep | PASS |
| prereqs | α, δ upstream | PASS |

## ζ — STEP import: OCCT reader FFI + step_import builtin + STEPInput (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| OCCT `STEPControl_Reader` available | OCCT is linked (export uses STEPControl_Writer, `occt_wrapper.cpp:120`); reader is the same lib — **new FFI fn, not new dep** | PASS |
| geometry-import eval seam | **NEW** — no B-rep import seam today (only field import `engine_eval.rs:816`); the builtin returns a `KernelHandle` exactly as geometry-constructor builtins do | PASS (new seam, substrate for it exists) |
| `step_import` as a builtin | `reify-stdlib` `eval_builtin` registry (the geometry-constructor home) | PASS (site exists) |
| STEPInput surface | declared in α (upstream) | PASS (α upstream) |
| **G6:** round-trip AABB within 1e-6 m (NOT byte/topology exact) | OCCT read/write precision ≪ 1e-6 m on a bounding box; bound is on AABB (B-rep-stable), **not** exact round-trip — avoids the exactness trap (cf. trajectory-spline esc-3770-1) | PASS (premise achievable) |
| prereq | α upstream | PASS |

---

## Deferred trackers (ι, κ, λ) — filed `deferred`, NOT in the pending flip

Each is a forward-stub tracker pointing at its committed stub PRD; no capability bindings asserted
(no leaf signal until activated). They exist so the deferred scope is tracked, not dropped.
