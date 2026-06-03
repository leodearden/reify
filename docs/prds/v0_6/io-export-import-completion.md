# PRD: std.io export/import completion (§9 `std.io.formats`)

**Milestone:** v0_6 · **Status:** active (decompose-ready) · **Date:** 2026-06-03
**Approach:** B + H (contract + two-way boundary tests) — multi-crate export/import seam.
**Closes:** `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` cluster **P15 io-export-import** (5 of 6 rows; the 6th, `Scalar<Money>`-degrades, is a shared type-resolver gap left to the resolver cluster).
**Source doc:** `docs/reify-stdlib-reference.md` §9.

---

## 1. Goal — what a user observes

Today `reify build foo.ri -o foo.step` is the *only* working export, and the **CLI file
extension** (not the design) picks the format. STL is declared in the Rust `ExportFormat`
enum but every kernel rejects it; 3MF/Display have no code path; the documented
`std.io.formats` occurrences (`STEPOutput`/`STLOutput`/`ThreeMFOutput`/`DisplayOutput`/`STEPInput`/
`PointCloudInput`) and their enums/struct exist in **no `.ri` file**; STEP/PointCloud **import**
does not exist.

After this PRD lands, a user can:

1. `reify build widget.ri -o widget.stl` → a valid **binary STL** of the realized solid.
2. `reify build widget.ri -o widget.3mf` → a valid **3MF** package.
3. **Declare exports in the design** and have the build *drive them*:
   ```reify
   structure def Widget {
       let part = box(10mm, 20mm, 5mm)
       sub rough = STLOutput(subject: part, resolution: 0.2mm, path: "rough.stl")
       sub master = STEPOutput(subject: part, version: STEPVersion.AP203, path: "master.step")
   }
   ```
   `reify build widget.ri` (no `-o`) writes **both** `rough.stl` and `master.step` *next to
   the design file* — the **occurrence type** picks the format and the `path` param picks the
   filename, **not** the CLI extension.
4. Select a STEP schema: `STEPOutput(... version: STEPVersion.AP203 ...)` → the written STEP's
   `FILE_SCHEMA` header says AP203 (vs the AP214 default).
5. **Import** STEP geometry: `let g = step_import("incoming.step")` realizes a solid usable by
   any downstream op (boolean, query, re-export).

Out of this PRD but **stubbed + tracked** (forward-stub sibling PRDs committed beside this one):
PointCloud import, `DisplayOutput`→viewport drive, and `Buy`/`Discard`/`Provenance` lifecycle
(BOM/cost) eval. See §8.

---

## 2. Background — substrate reality (verified 2026-06-03)

Verified against `target/release/reify` and the source tree; file:line below.

**Export pipeline (partly built):**
- `ExportFormat = {Step, Stl, Obj}` — `crates/reify-ir/src/geometry.rs:1479`. Only `Step` is
  implemented; `OcctKernel::export` rejects everything else with
  `FormatError("unsupported export format")` — `crates/reify-kernel-occt/src/lib.rs:2982-2985`;
  `ManifoldKernel::export` is a stub (`kernel.rs:469-476`).
- Format chosen by **file extension** — `crates/reify-cli/src/main.rs:541-548`.
- **Tessellation egress already exists** and is production-proven (the GUI renders every solid as
  a mesh): `OcctKernel::tessellate` (`lib.rs:2989-3006`) → C++ `tessellate_shape`
  (`occt_wrapper.cpp:4328+`, `BRepMesh_IncrementalMesh`) → `Mesh { vertices, indices, normals }`
  (`geometry.rs:1485-1494`). `ManifoldKernel::tessellate` likewise.
- The terminal-repr router **already** maps `Stl/Obj → ReprKind::Mesh`
  (`demanded_reprs_for_template`, `engine_build.rs:1643-1646`); the STEP export body-walk
  `surface_export_bodies` exists (`geometry_ops.rs:5143`, task **#3905** done). **No STL, 3MF, OBJ,
  or glTF serializer exists anywhere.** The single gap for STL/3MF is the *serializer*, not the
  mesh source or the routing.

**"Output drives export" is ~15% built:** `extract_output_tolerance_bound`
(`tolerance_combine.rs:129-211`, task **#5** done) recognizes an Output **template** by a
`RepresentationWithin(subject, tol)` constraint and reads only its **tolerance**. It does **not**
read a `format`, does **not** enumerate Output **instances**, and `build(module, format)` is
**single-format / single-output** (`engine_build.rs:2106`, output is one `Option<Vec<u8>>`).

**`occurrence def` is real** (`grammar.js:489`, `EntityKind::Occurrence`, runtime-evaluated —
`reify-eval/tests/occurrence_eval.rs`) but produces **no export side-effect** — instances just
evaluate to value cells.

**io.ri** (`crates/reify-compiler/stdlib/io.ri`): declares `Source/Sink/Input/Buy/Output/Discard/
Costed`, `Provenance`, enums `DiscardReason/DisposalMethod/OutputFormat{STEP,STL,ThreeMF,Display}`.
**Zero occurrences**; no `STEPVersion`/`PointCloudFormat`/`DisplayStyle`/`PointCloud`. Its header
comment claiming `= undef` is unsupported is **stale** (task **#3918** landed `undef`).

**Import:** no STEP reader (`STEPControl_Reader` absent), no `PointCloud` type anywhere, no
B-rep geometry-import eval seam — only a *field*-import seam (`engine_eval.rs:816-849` →
OpenVDB `read_vdb_file`, task #3439 pending). A geometry import needs a **new** seam.

**Types:** `Solid → Type::Geometry` (`type_resolution.rs:563`). **`Structure` and bare
`Geometry` do NOT resolve as param types** (`reify check` → `unresolved type`). `PointCloud`
absent.

---

## 3. G3 grammar gate — empirically validated (real `reify check`)

Fixtures in `/tmp/prd-gate-fixtures/io-*.ri`. Results drive the §4 design:

| Fragment | Verdict |
|---|---|
| `occurrence def X : Output { … }` | ✅ parses + type-checks |
| `enum STEPVersion { AP203, AP214, AP242 }` | ✅ |
| `param version : STEPVersion = STEPVersion.AP214` (qualified enum default) | ✅ |
| `param format : OutputFormat = OutputFormat.STL` | ✅ |
| `param path : String` / `= "out.stl"` | ✅ |
| `param style : DisplayStyle = DisplayStyle()` (struct-ctor default) | ✅ |
| `constraint determined(subject)` (direct ref) | ✅ |
| `let part = box(…)` + `sub o = STLOutput(subject: part, resolution: 0.2mm, path: "o.stl")` | ✅ "All constraints satisfied" |
| `param subject : Structure` / `param subject : Geometry` | ❌ `unresolved type` → **use `Solid`** |
| `param style : DisplayStyle = undef` (occurrence param) | ❌ `unresolved name: undef` → **concrete default, no `undef`** |
| `constraint determined(subject.geometry)` (member access) | ❌ `member access not yet supported` → **drop; use `determined(subject)`** |
| `sub part = box(…)` | ❌ parse error — `sub` binds entities → **bind geometry with `let`** |
| `STEPInput : Input` with no source/provenance | ❌ inherited `source`/`provenance` required → **supply concrete defaults** (validated, §4) |

**No new grammar work is required.** Every novel fragment either parses today or is replaced by an
existing-grammar idiom. `grammar_confirmed = true` for every leaf.

---

## 4. Sketch of approach + resolved design decisions

### 4.1 The `std.io.formats` declarative surface (task α)

Append to `crates/reify-compiler/stdlib/io.ri` (exact validated shapes):

```reify
enum STEPVersion { AP203, AP214, AP242 }

structure def DisplayStyle {
    param opacity   : Real = 1.0
    param wireframe : Bool = false
}

occurrence def STEPOutput : Output {
    param subject : Solid
    param path    : String
    param version : STEPVersion  = STEPVersion.AP214
    param format  : OutputFormat = OutputFormat.STEP
    constraint determined(subject)
}
occurrence def STLOutput : Output {
    param subject    : Solid
    param path       : String
    param resolution : Length       = 0.1mm
    param format     : OutputFormat = OutputFormat.STL
    constraint determined(subject)
}
occurrence def ThreeMFOutput : Output {
    param subject          : Solid
    param path             : String
    param include_materials: Bool         = true
    param include_colors   : Bool         = true
    param format           : OutputFormat = OutputFormat.ThreeMF
    constraint determined(subject)
}
occurrence def DisplayOutput : Output {
    param subject : Solid
    param pane    : Int          = 0
    param style   : DisplayStyle = DisplayStyle()
    param format  : OutputFormat = OutputFormat.Display
}
occurrence def STEPInput : Input {
    param source     : String   = ""
    param provenance : Provenance = Provenance(
        source_tool: "step-import", source_version: "",
        timestamp: "", tolerance_guarantee: 0.001mm)
    param version : STEPVersion = STEPVersion.AP214
}
```

**Resolved deviations from §9 (forced by validated substrate, all documented in the io.ri
header):**
- **`subject : Solid`** everywhere (§9 uses `Structure`/`Geometry`, which don't resolve as param
  types). `Solid → Type::Geometry` is exactly an exportable realized-geometry handle.
- **`path : String` added** to each export occurrence (the chosen output-path policy — §4.3). §9
  occurrences have no filename; the build driver needs one.
- **format pinned by a qualified-enum default** per occurrence type (§9 leaves it unset; an
  unset required `format` is awkward and the occurrence *type* already implies the format).
- **No `= undef`** on occurrence params (trap: trait-only). Optional params get concrete defaults
  (`resolution = 0.1mm`, `style = DisplayStyle()`, STEPInput `source = ""` + a default `Provenance`).
- **`constraint determined(subject)`** (direct ref) replaces §9's `determined(subject.geometry)`
  (member access unsupported).
- **`PointCloudInput` / `PointCloudFormat` are NOT declared here** — they need a `PointCloud` type
  that does not exist; the whole point-cloud import path is the deferred sibling PRD (§8). Declaring
  them now would add orphan surface (the very `declared-only` defect the register flags).

### 4.2 STL + 3MF serializers (tasks β, γ)

Both are **pure functions over the existing `Mesh`** — no kernel change beyond an `export()` arm:
- `write_stl_binary(&Mesh, &mut dyn Write)` (default) + `write_stl_ascii(&Mesh, …)` in
  `reify-ir` (sibling of `Mesh`). Binary STL = 80-byte header + u32 count + 50·N bytes.
- `write_3mf(&Mesh, ThreeMfOptions, &mut dyn Write)` — a ZIP container (`zip` crate) holding
  `[Content_Types].xml`, `_rels/.rels`, `3D/3dmodel.model` (the OPC/3MF core XML mesh). `Obj`
  stays declared-but-unimplemented (out of §9; no demand).
- Kernel `export()` gains `Stl | ThreeMF => { let m = self.tessellate(handle, tol)?; write_*(&m, w) }`
  for **both** OCCT and Manifold. `tol` from the demanded tolerance (STL `resolution`).
- New `ExportFormat::ThreeMF` variant + its `demanded_reprs` arm (`ThreeMF → Mesh`) + CLI
  `.3mf → ThreeMF` extension.
- **`include_materials`/`include_colors` honestly gated:** the mesh egress carries no per-body
  material/color today (GUI styling is client-side). When either is `true` but no material/color
  data is present, emit geometry-only and a **`W_3MF_NO_MATERIALS`** warning. (Real material/color
  egress is future work; the param is wired to a real, observable no-op-with-warning, not silently
  ignored.)

### 4.3 The Output-occurrence export driver (task δ — the H integration-gate)

A new build entrypoint:
```
build_outputs(&mut self, module) -> Vec<ExportArtifact>     // ExportArtifact { path, format, bytes, diagnostics }
```
Contract (full signatures/invariants in §7):
1. **Enumerate** every occurrence **instance** in the realized snapshot conforming to `: Output`
   (trait-bound conformance, not name match — so user-defined Output occurrences work too).
2. For each, **resolve params from the value map**: `subject` → geometry handle (via the existing
   `named_steps`/`GeomRef::Sub` threading + `surface_export_bodies` handle map), `format` (occurrence
   `format` param), `path`, and format-specific params (`resolution`→tessellation tol,
   `version`→STEP schema, `include_*`→3MF opts).
3. **Path resolution (per the user's directive):** a **relative** `path` resolves against the
   **directory of the design `.ri` file**, *not* the process CWD. Absolute paths verbatim. The CLI
   `--out-dir DIR` flag, when given, overrides the base directory for relative paths (CI escape
   hatch). `DisplayOutput` is recognized but **skipped with an info diagnostic** (`I_DISPLAY_OUTPUT_DEFERRED`)
   — its viewport drive is the deferred sibling PRD (§8); recognizing-but-deferring keeps the
   surface honest.
4. **Emit** one file per Output occurrence via the (now STL/3MF/STEP-capable) kernel `export()`.
   Deterministic order (occurrence declaration order). A failed export emits a diagnostic and
   continues — one bad Output never aborts the others.
5. **Reuse + extend** `extract_output_tolerance_bound` (task #5) to also read `format`/`path`, so
   the new driver and the existing tolerance pipeline share one Output-recognition path.

**Back-compat / mode selection (resolved):** `reify build f.ri -o out.stl` keeps the existing
**imperative single-output** path (extension-driven, `build(module, format)`) unchanged. With **no
`-o`**, the **declarative** `build_outputs` driver runs and emits every Output occurrence. The two
modes are mutually exclusive per invocation; `--out-dir` only affects the declarative mode.

### 4.4 STEP version → schema (task ε)

`STEPOutput.version` threads to OCCT `export_step`, which sets
`Interface_Static::SetCVal("write.step.schema", "AP203"|"AP214"|"AP242")` before transfer. AP203/AP214
are first-class in OCCT `STEPControl_Writer`; **AP242 is best-effort** (OCCT's `"AP242DIS"` schema
value) and falls back to AP214 with a `W_STEP_AP242_FALLBACK` warning if the linked OCCT rejects it
(documented honest degradation, not a silent lie).

### 4.5 STEP import (task ζ — minimal, the new geometry-import seam)

- C++ `import_step(path) -> shape` (OCCT `STEPControl_Reader`, `TransferRoots`, `OneShape`) + the
  Rust FFI bridge in `reify-kernel-occt`.
- A `step_import(path: String) -> Solid` builtin (a new **geometry-import eval seam** — the first
  B-rep import; mirrors how geometry-constructor builtins return a realized handle, and how the
  field-import seam records provenance). The realized handle is a normal `Geometry`, usable by any
  downstream op or export.
- `STEPInput` occurrence (declared in α) is the declarative wrapper; its `source` feeds
  `step_import`. **Import is not byte-exact** (B-rep is re-read, re-tessellated) — round-trip is
  asserted on **bounding box within tolerance**, never byte/topology equality (G6 §6).

---

## 5. Pre-conditions for activating

- **Geometry realization is real.** The export/import slices test against `box(…)`, which
  realizes today, so this PRD is **not hard-blocked**. The in-flight **geometry-primitive-constructors**
  PRD enriches the set of exportable solids; this PRD consumes whatever realizes. Soft dependency —
  noted, not a hard `depends_on`.
- **`undef` first-class** (task #3918, landed) — already satisfied; no `= undef` is used here anyway.
- No new grammar work (§3). No new substrate prerequisite tasks.

---

## 6. G6 — premise validity per leaf signal

| Leaf | Asserted premise | Basis (achievable / true / producible from this leaf's deps) |
|---|---|---|
| β STL | binary STL, tri-count N>0, size = 84+50·N | `tessellate(box)` works today; size is the STL spec identity. Producible from β alone. |
| γ 3MF | valid ZIP/OPC with `<triangle>` count = N | 3MF/OPC is a fixed schema; `zip` crate + tessellation. Producible from β+γ. |
| γ materials gate | `W_3MF_NO_MATERIALS` when `include_materials` ∧ no material | warning is observable; no false "materials written" claim. |
| δ driver | DSL occurrence drives **format+path** with no `-o`/extension | requires α (surface) + β (STL) + the value-map/`surface_export_bodies` substrate — all upstream/existing. **No** capability owned by a downstream task. |
| δ relative path | output at `<design-dir>/o.stl`, not CWD | path join is deterministic; the design-file dir is known at build. |
| ε STEP version | written `FILE_SCHEMA` contains `AP203` | OCCT `write.step.schema` is a real static param; AP203/AP214 first-class, AP242 best-effort w/ fallback warning. |
| ζ STEP import | re-exported AABB matches fixture dims within **1e-6 m** | OCCT reader is standard; bound is on bounding-box (a B-rep-stable quantity), **not** byte/topology equality — avoids the round-trip-exactness trap. |

No numeric-method *floor* is asserted (no FEA/solver bound). The only tolerance, ζ's 1e-6 m AABB
match, is a geometric round-trip stability bound, comfortably above OCCT's STEP read/write precision.

---

## 7. Contract (B + H)

### 7.1 Seam: kernel `export()` (extended)
```rust
fn export(&mut self, handle: GeometryHandleId, format: ExportFormat,
          writer: &mut dyn std::io::Write) -> Result<(), ExportError>;
```
Invariants: `Step` unchanged. `Stl | ThreeMF` ⇒ `tessellate(handle, tol)` then serialize; `tol`
= demanded tolerance (STL `resolution`, else kernel default). Unsupported `(format, kernel)` ⇒
`ExportError::FormatError`. `ExportFormat` gains `ThreeMF`; every exhaustive match
(`demanded_reprs_for_template`, CLI extension map, both kernels) is updated.

### 7.2 Seam: mesh serializers (pure, over `Mesh`)
`write_stl_binary` / `write_stl_ascii` / `write_3mf(&Mesh, ThreeMfOptions, &mut Write)` in
`reify-ir`. Invariant: `triangles = indices.len()/3`; binary STL byte-length = `84 + 50*triangles`.
No kernel/global state.

### 7.3 Seam: `build_outputs` driver
```rust
struct ExportArtifact { path: PathBuf, format: ExportFormat, bytes: Vec<u8>, diagnostics: Vec<Diagnostic> }
fn build_outputs(&mut self, module: &CompiledModule, design_dir: &Path,
                 out_dir_override: Option<&Path>) -> Vec<ExportArtifact>;
```
Invariants: one artifact per `: Output` occurrence instance (except `DisplayOutput` → skipped with
`I_DISPLAY_OUTPUT_DEFERRED`); deterministic declaration order; relative `path` joined onto
`out_dir_override.unwrap_or(design_dir)`; per-artifact failure is isolated (diagnostic, not abort);
`subject` resolves to the same handle `surface_export_bodies` would export.

### 7.4 Seam: STEP-import geometry seam
`import_step(path) -> shape` (OCCT FFI) → `step_import(path: String) -> Geometry` builtin → a
`KernelHandle` valid for all downstream ops. Records minimal geometry-import provenance.

### 7.5 Boundary-test sketch (faces producer **and** consumer; = δ's integration-gate signal)

| # | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|
| B1 | STL binary | `box(10,20,5)` realizes | `export().Stl` → 84+50·N bytes; N>0; AABB ≈ (10,20,5) within tess tol |
| B2 | STL ascii | ascii requested | `solid … endsolid` with N facets |
| B3 | 3MF valid | box | unzip → `3D/3dmodel.model`; `<triangle>` count = N |
| B4 | 3MF materials gate | `include_materials=true`, no material | `W_3MF_NO_MATERIALS`; geometry still written |
| B5 | **Driver format+path** | `.ri` w/ `STLOutput(path:"o.stl")`, **no `-o`** | `reify build` writes `<design-dir>/o.stl`; format from occurrence |
| B6 | **Driver multi-output** | `.ri` w/ `STLOutput` + `STEPOutput` | both `o.stl` and `o2.step` written in one build |
| B7 | **Driver relative path** | design at `sub/foo.ri`, `path:"o.stl"` | output at `sub/o.stl`, **not** `cwd/o.stl` |
| B8 | STEP version | `STEPOutput(version: AP203)` | written STEP `FILE_SCHEMA` contains `AP203` |
| B9 | STEP import round-trip | committed `fixture.step` (known dims) | `step_import` → re-export STL; AABB matches within 1e-6 m |
| B10 | Back-compat `-o` | `reify build box.ri -o x.stl` | single STL; imperative path unchanged |

---

## 8. Out of scope — deferred forward-stub sibling PRDs (committed beside this one)

Each ships a stub PRD doc + a **stay-deferred** tracking task (filed `deferred`, *not* flipped to
`pending`):

| Deferred capability | Why deferred | Stub PRD | Tracker |
|---|---|---|---|
| **PointCloud import** (`PointCloud` value type + PLY/PCD/XYZ/LAS readers + `PointCloudInput` + `PointCloudFormat`) | needs a brand-new value type + 4 file readers; orthogonal to B-rep export | `io-import-pointcloud.md` | task ι |
| **`DisplayOutput` → viewport drive** (GUI panes + per-mesh style: color/opacity/wireframe) | GUI seam; no backend per-mesh style or multi-pane exists; contested w/ GUI-rendering work | `io-display-output-viewport.md` | task κ |
| **`Buy`/`Discard`/`Provenance` lifecycle eval** (BOM / cost roll-up / waste report) | no consumer surface today (would be a reporting PRD); producer-orphan if built now | `io-lifecycle-bom-cost.md` | task λ |

Also out of scope: `Obj` export (declared-but-unused, not in §9), `Scalar<Money>` resolver fix
(shared type-resolver gap, owned by the resolver cluster), real 3MF material/color egress.

---

## 9. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometry-primitive-constructors.md` (in-flight) | consumes | realized `Geometry`/`Mesh` handle (export subject) | geometry-primitives owns realization; **this** owns export | soft-dep (box realizes today) |
| `io-display-output-viewport.md` (stub, this batch) | produces | `DisplayOutput` occurrence surface → GUI viewport drive | **the stub PRD** (GUI side) owns the drive | declared here, drive deferred |
| `io-import-pointcloud.md` (stub) | produces | `step_import` geometry-import seam (reused by point-cloud import) | **this** owns the seam; stub adds PointCloud readers atop it | seam here, readers deferred |
| tolerance pipeline (task #5, done) | extends | `extract_output_tolerance_bound` (reads `format`/`path` too) | **this** extends it | wired (no contest) |
| `sub-placement-and-surfacing.md` (task #3905, done) | reuses | `surface_export_bodies` body-walk / `named_steps` | #3905 owns the walk; **this** reuses for the driver | wired (no contest) |

No reciprocal-ownership ambiguity. The export-driver seam is *not* a new engine-integration-norm
§3 seam — it extends the existing `build()` post-realization surface (the §3.2 realization-kind /
terminal-repr dispatch already maps Stl/3MF→Mesh).

---

## 10. Decomposition plan

Greek labels → task IDs at decompose. **Active batch (flip to pending): α β γ δ ε ζ.**
**Deferred trackers (stay deferred): ι κ λ.**

- **α — `std.io.formats` declarative surface.** *Modules:* `reify-compiler/stdlib/io.ri`,
  `tree-sitter-reify/` fixtures. *Intermediate* (unlocks δ, ε, ζ). *Signal:* a committed `.ri`
  fixture instantiating each occurrence (`STEPOutput/STLOutput/ThreeMFOutput/DisplayOutput/STEPInput`
  + `STEPVersion`/`DisplayStyle`) passes `reify check` (exit 0; **zero** `unresolved type`/
  `unresolved name: undef` errors); stale io.ri `undef` header comment removed. *Prereqs:* —.
- **β — STL writer + kernel `export()` wiring.** *Modules:* `reify-ir/src/geometry.rs`,
  `reify-kernel-occt/src/lib.rs`, `reify-kernel-manifold/src/kernel.rs`. *Leaf.* *Signal:*
  `reify build <box>.ri -o /tmp/b.stl` exits 0 and writes a valid binary STL (`84 + 50·N` bytes,
  `N>0`); re-parsing the AABB ≈ box dims. *Prereqs:* —.
- **γ — 3MF writer + `ExportFormat::ThreeMF`.** *Modules:* `reify-ir/src/geometry.rs`,
  `reify-eval/src/engine_build.rs` (demanded-repr arm), `reify-kernel-occt`, `reify-kernel-manifold`,
  `reify-cli/src/main.rs` (`.3mf` ext), `Cargo.toml` (`zip`). *Leaf.* *Signal:*
  `reify build <box>.ri -o /tmp/b.3mf` writes a valid 3MF (unzip → `3D/3dmodel.model` XML with N
  `<triangle>`s); `include_materials=true` w/o material emits `W_3MF_NO_MATERIALS`. *Prereqs:* β.
- **δ — Output-occurrence export driver + design-relative paths + `--out-dir`.** *Modules:*
  `reify-eval/src/engine_build.rs` + new driver, `reify-eval/src/tolerance_combine.rs`
  (read `format`/`path`), `reify-cli/src/main.rs`. *Leaf — H integration-gate (signal = §7.5
  B5/B6/B7).* *Signal:* a `.ri` with `sub o = STLOutput(subject: part, resolution: 0.2mm,
  path:"o.stl")` built via `reify build sub/foo.ri` (no `-o`) writes `sub/o.stl`; a second
  `STEPOutput` in the same design also writes `sub/o2.step` — proving the **DSL** drove format +
  path, not the CLI extension. *Prereqs:* α, β.
- **ε — `STEPVersion` → STEP schema.** *Modules:* `reify-kernel-occt/cpp/occt_wrapper.cpp` +
  `src/lib.rs`, export-options threading (`reify-ir`/`reify-eval`). *Leaf.* *Signal:*
  `STEPOutput(subject: part, version: STEPVersion.AP203, path:"p.step")` (driver-built) writes a
  STEP whose `FILE_SCHEMA` line contains `AP203` (vs `AP214` default); AP242 unsupported →
  `W_STEP_AP242_FALLBACK`. *Prereqs:* α, δ.
- **ζ — STEP import: OCCT reader FFI + `step_import` builtin + `STEPInput`.** *Modules:*
  `reify-kernel-occt/cpp/occt_wrapper.cpp` + `src/lib.rs`, `reify-stdlib` (`eval_builtin`),
  `reify-eval` (geometry-import seam), `reify-compiler` (STEPInput wiring). *Leaf.* *Signal:* a
  committed `fixture.step` (known dims) imported via `let g = step_import("fixture.step")`;
  `reify build` re-exports `g` to STL whose AABB matches the fixture dims within `1e-6 m` (round-trip,
  not byte-exact). *Prereqs:* α.
- **ι — [DEFERRED tracker] PointCloud import.** Stub PRD `io-import-pointcloud.md`. *Stays deferred.*
- **κ — [DEFERRED tracker] `DisplayOutput` → viewport drive.** Stub PRD `io-display-output-viewport.md`.
  *Stays deferred.*
- **λ — [DEFERRED tracker] `Buy`/`Discard`/`Provenance` lifecycle eval.** Stub PRD
  `io-lifecycle-bom-cost.md`. *Stays deferred.*

DAG: α, β roots; γ→β; δ→α,β; ε→α,δ; ζ→α. ι/κ/λ independent + deferred.

---

## 11. Open questions (tactical — deferred, not design-blocking)

1. **STL ascii toggle surface.** Default is binary; §9 `STLOutput` has no ascii flag. Add a
   `binary : Bool = true` param, or leave ascii test-only? *Suggested:* add `binary : Bool = true`
   (cheap, observable). Decide during β.
2. **3MF `zip` crate vs hand-rolled OPC.** *Suggested:* `zip` crate (stored, no compression for
   determinism). Decide during γ.
3. **STEP import provenance depth.** Minimal (`source_tool`/path) now; align with field-import
   provenance richness later. Decide during ζ.
4. **`DisplayOutput` warning vs silent skip in the driver.** *Suggested:* `I_DISPLAY_OUTPUT_DEFERRED`
   info diagnostic (chosen in §4.3). Confirm during δ.
