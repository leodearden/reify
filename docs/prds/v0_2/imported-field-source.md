# PRD: `imported` Field Source (OpenVDB / CSV / HDF5)

Status: deferred to v0.2 per 2026-04-26 decision.

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
- OpenVDB kernel work is in scope or already underway (`multi-kernel.md`).
- A concrete user need for imported field data has been documented (FEA round-trip, scan-to-CAD, etc.).
- Host-language integration story is settled — imported files are read at evaluation time, which means I/O happens in the runtime, with all the failure modes that implies (file not found, schema mismatch, unit conflicts).

## Out of scope for this PRD

- Streaming/lazy loading for very large grids (post-v0.2 optimization).
- Write-back to imported formats (Reify is the consumer, not the producer, of these files).
- Format auto-detection from file extension (v0.2 requires explicit `format = ...`).
- Network-fetched fields (file:// only for v0.2).
