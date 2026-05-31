# PRD: `imported` Field Source (OpenVDB / CSV / HDF5)

Status: **OpenVDB consumer arm folded into v0.3 via `docs/prds/v0_3/multi-kernel-phase-3.md` Phase 4 task θ** (2026-05-12 contested-ownership disposition, GR-003).
Historical: deferred to v0.2 per 2026-04-26 decision; design resolved 2026-04-28 — see "Resolved design decisions" below. **v0.2 scope narrowed to OpenVDB only**; HDF5 and CSV deferred to a follow-on PRD.

**Cross-reference — active owner (GR-003):** `docs/prds/v0_3/multi-kernel-phase-3.md`
consumes this PRD's OpenVDB arm (symmetrical with multi-kernel-phase-3.md §11 "consumes" row):
— §4 — cache-key composition / GR-003 wiring point: `RealizationCacheKey.options_hash` includes `MeshToVoxelOptions`; every call site passes actual `repr_kind` (not hard-coded `ReprKind::BRep`).
— §8 Phase 4 task θ — operative consumer-arm slice: `engine_eval.rs:621` `CompiledFieldSource::Imported` arm replaced, routing through `reify-kernel-openvdb::ingest::load_field_from_path` (GR-003 resolution per 2026-05-12 disposition).
The 5-task OpenVDB decomposition in this PRD is superseded by task θ. HDF5/CSV continues under `docs/prds/v0_3/imported-field-source-hdf5-csv.md` (multi-kernel-phase-3.md §11). See "Decomposition plan" below.

**Cross-reference — std.fields / stdlib-reconstruction (Slice C task ν):** `docs/prds/v0_6/stdlib-reconstruction.md`
The `std.fields` stdlib module (`crates/reify-compiler/stdlib/fields.ri`) is the documented packaging surface for the built-in `Field<D,C>` parametric type that `imported` fields realize as (see "Lowering to internal `sampled` representation" below). `Field<D,C>` is a built-in parametric type that resolves without import; `std.fields` packages and documents the prelude field operators (gradient/divergence/curl/laplacian/sample).

## Goal

Implement the `imported` source kind for `field def` declarations as specified in `docs/reify-language-spec.md` §3.5 and §4.1.4 (the `Field` source-kind table). v0.1 ships `analytical`, `sampled`, and `composed`; v0.2 adds `imported`, which reads field data from external files — primarily OpenVDB grids, CSV tables, and HDF5 datasets.

## Background

The spec (§4.1.4 source table) defines four source kinds for fields:

| Source kind  | Meaning |
|-------------|---------|
| `analytical` | Closed-form expression, given as a lambda |
| `sampled`    | Discrete samples on a grid/mesh, with interpolation |
| `composed`   | Combination of other fields via arithmetic, logic, or conditional |
| `imported`   | External data file (OpenVDB, CSV, HDF5, etc.) |

`imported` is the boundary case — it lets designers consume field data produced outside Reify (FEA stress fields, MRI scans, density maps, weather data, etc.) without re-deriving them. v0.1 ships the first three because they are pure-Reify constructions; `imported` requires file-format ingestion infrastructure that doesn't fit v0.1's scope.

## Why deferred

- OpenVDB ingestion depends on the OpenVDB kernel adapter, which is part of multi-kernel dispatch (`multi-kernel.md`) — circular dependency unless deferred together.
- HDF5 and CSV ingestion is straightforward in isolation but requires decisions about: schema declaration syntax (how does a user say "this CSV has columns x, y, z, temperature"?), unit assignment to imported numeric data (no embedded dimension info in CSV/HDF5), interpolation policy across irregular sample sets, and provenance tracking through Input occurrences (§14.5 boundary contract).
- v0.1 users with imported-data needs can construct a `sampled` field and load the data programmatically through host-language integration — clunky but functional.

## Sketch of approach

The `imported` source attaches to a `field def` like the existing source kinds:

```
field def fea_stress : Point3<Length> -> Tensor<2, 3, Pressure> {
    source = imported {
        path = "fea_results.vdb"
        format = OpenVDB
        grid = "vonMises"
        units = MPa
        interpolation = trilinear
    }
}
```

Three format families to support:

- **OpenVDB** — sparse volumetric grids. Most natural fit for Reify's field type; OpenVDB grids carry their own coordinate transform and sparsity structure. Requires the OpenVDB kernel from `multi-kernel.md`.
- **HDF5** — structured arrays with named datasets and attributes. Schema is largely self-describing; need a small ingest layer to map dataset path + axes attributes to the Reify field domain.
- **CSV** — tabular point samples, scattered or gridded. Requires explicit schema declaration in source (column-to-axis mapping, units, interpolation) since CSV carries no metadata.

Each `imported` field is realized as a `sampled` field internally — the import layer reads the file once, allocates the sample buffer, and the rest of the field machinery (interpolation, gradient, composition) treats it identically to a `sampled` field. The distinction is at the source-declaration level only.

Imported fields participate in the Input-occurrence boundary contract (§14.5): the imported file's path, format, ingestion timestamp, and asserted tolerance/units are recorded as provenance so downstream tools can reason about the boundary condition.

Caching: imported data is content-hashed on file contents (not path) so the evaluation graph correctly invalidates when the file changes.

## Pre-conditions for activating

- v0.1 alpha has shipped and field-using examples are stable.
- OpenVDB kernel work is underway: `multi-kernel.md` Phases 1+2 shipped 2026-04-28; OpenVDB consumer wiring is Phase 4 task θ in `docs/prds/v0_3/multi-kernel-phase-3.md`.

## Resolved design decisions (2026-04-28)

**v0.2 scope narrowed to OpenVDB only.** The original PRD listed three formats (OpenVDB, HDF5, CSV); v0.2 ships only OpenVDB. Reasoning: OpenVDB's semantics align natively with Reify's field type (sparse volumetric grids, embedded coordinate transform, embedded grid metadata — no schema declaration needed). HDF5 and CSV require explicit schema-declaration design surface (column-to-axis mapping, unit annotation syntax, interpolation policy for irregular sample sets, error reporting on schema mismatch) that isn't worth designing speculatively. Deferred to a follow-on PRD when concrete user demand emerges.

**Unit handling for OpenVDB.** OpenVDB grids carry user metadata; Reify reads any unit annotation and validates against the field declaration's type (`Length`, `Pressure`, etc.). Mismatch → error at ingestion. No new syntax in the `field def` block.

**Interpolation.** OpenVDB grids declare their own interpolation implicitly (linear / quadratic / staggered, depending on grid type). Reify uses the grid's declared interpolation; no override knob for v0.2.

**Provenance via Input occurrence (§14.5).** Imported fields participate in the existing Input boundary contract. Provenance fields recorded: file path, format, content hash, ingestion timestamp, declared tolerance. Mechanical extension of existing infrastructure — no new design.

**Cache invalidation by content hash.** The source file is content-hashed on read; the hash is part of the realization cache key. File-content change → cache invalidation; file-path change with same content → cache hit. Standard pattern; no surprises.

**Lowering to internal `sampled` representation.** Once ingested, an `imported` field is indistinguishable from a `sampled` field at the field-machinery level (interpolation, gradient, composition all work the same). The distinction lives at the source-declaration level only. Concretely, the realized value is a `Field<D,C>` — the built-in parametric type that resolves without import (not an alias declared by any module). The `Field<D,C>` type and the prelude field operators (gradient/divergence/curl/laplacian/sample) are packaged and documented by the `std.fields` stdlib module (`crates/reify-compiler/stdlib/fields.ri`).

## Decomposition plan (5 tasks, gated on 2295's OpenVDB sub-kernel)

> **Superseded (2026-05-12).** The OpenVDB consumer-arm work (tasks 1–5 below) has been re-homed as task θ in `docs/prds/v0_3/multi-kernel-phase-3.md` §8 Phase 4, per the 2026-05-12 contested-ownership disposition for GR-003. The 5-task list below is preserved for historical reference only — do not implement these tasks separately; implement task θ instead. The HDF5/CSV follow-on continues under `docs/prds/v0_3/imported-field-source-hdf5-csv.md` (listed in multi-kernel-phase-3.md §11 as blocked-on-θ).

1. **`imported` source-kind grammar + parsing** — extends the `field def` source enum with the variant, parses `path` / `format = OpenVDB` / `grid = "..."` block.
2. **OpenVDB ingestion** — file read, sample-buffer allocation, unit metadata validation, lowering to internal `sampled` representation.
3. **Provenance recording on Input occurrence** — adds file path, content hash, ingestion timestamp, declared tolerance fields.
4. **Cache invalidation on file content change** — content-hash the source file; integrate hash into the realization cache key.
5. **End-to-end smoke test + diagnostic coverage** — file-not-found, grid-not-in-file, unit mismatch, malformed file, etc.

## Out of scope for this PRD

- HDF5 and CSV imported fields — deferred to a follow-on PRD with v0.3+ priority. Tracker filed separately.
- Streaming/lazy loading for very large grids (post-v0.2 optimization).
- Write-back to imported formats (Reify is the consumer, not the producer, of these files).
- Format auto-detection from file extension (v0.2 requires explicit `format = OpenVDB`).
- Network-fetched fields (file:// only for v0.2).
- Scattered-sample interpolation (kriging, RBF, etc.) — separate feature when CSV/HDF5 land.
