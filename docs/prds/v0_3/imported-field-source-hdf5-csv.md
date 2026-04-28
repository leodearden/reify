# PRD: `imported` Field Source — HDF5 and CSV (v0.3 follow-on)

Status: deferred to v0.3 per 2026-04-28 decision. Sibling task: 2392 (v0.2 OpenVDB-only narrowing).

## Goal

Extend the v0.2 `imported` source kind (currently OpenVDB-only after the 2026-04-28 narrowing) to cover HDF5 datasets and CSV tables, completing the three-format spread originally envisaged in `docs/reify-language-spec.md` §4.1.4. The target is the same `source = imported { ... }` syntax block established in v0.2 so the user-facing surface is uniform across all three formats.

## Background

The v0.2 PRD (`docs/prds/v0_2/imported-field-source.md`) was narrowed on 2026-04-28 to OpenVDB-only because OpenVDB's semantics align natively with Reify's field type: OpenVDB grids carry an embedded coordinate transform, embedded sparsity structure, and format-native metadata that maps directly onto Reify's sample-buffer model without user-supplied schema.

HDF5 and CSV lack those native handles:

- **OpenVDB** — grid carries its own coordinate transform and sparsity; no schema declaration needed from the user.
- **HDF5** — structured arrays with named datasets and attributes. Schema is largely self-describing, but the axis-array names, dataset path, and unit attributes must be mapped explicitly to Reify's domain/codomain.
- **CSV** — tabular point samples (scattered or gridded). Carries no metadata at all; the user must supply column-to-axis mapping, units, and interpolation policy inline.

HDF5 and CSV therefore require a new schema-declaration design surface that wasn't worth designing speculatively in v0.2 and that benefits from real user demand to anchor correctly.

## Why deferred

1. **v0.2 partially serves users already.** The OpenVDB importer covers the FEA/density-map use cases where files already carry grid metadata. Users with CSV/HDF5 data can reach for the v0.1 `sampled` source with host-language loading as a fallback (clunky but functional).
2. **Four interlocking design questions.** Schema declaration syntax, scattered-vs-gridded handling, HDF5 dataset-path mapping, and unit-metadata conventions interact enough that answering any one of them well requires anchoring against the others. Rushing the design risks producing a surface that is inconsistent or that forces a breaking change.
3. **Real demand not yet documented.** The concrete user need (FEA round-trip, MRI/CT, CFD output, lab spreadsheet import) is anticipated but has not yet been formally filed; v0.3 gates on documenting at least one concrete case, per the pre-conditions below.

## Sketch of approach

### 1. Schema declaration syntax

Leading candidate: inline `schema` block alongside the existing `path`/`format`/`units`/`interpolation` keys.

```
field def lab_pressure : Point3<Length> -> Pressure {
    source = imported {
        path    = "measurements.csv"
        format  = CSV
        schema  = { x: Length(mm), y: Length(mm), z: Length(mm), value: Pressure(MPa) }
        interpolation = nearest_neighbour
    }
}
```

The typed column identifiers reuse the existing unit-literal grammar (see `docs/prds/money-dimension.md` and `crates/reify-compiler/stdlib/units.ri`) — no new parser surface for units. For HDF5 the `schema` block is optional (attributes are read from the file) but may override specific columns.

**Missing/malformed rows:** lead candidate is row-level diagnostic + skip-with-warning; the row is dropped from the sample set and a count is accumulated in the file-level provenance record. Silent discard is not acceptable; silent abort is too harsh for a format where single bad rows are common.

**Schema mismatch:** compile-time diagnostic (e.g. `E_FIELD_IMPORTED_SCHEMA_MISMATCH`) when the declared schema is incompatible with the runtime file structure. Exact mnemonic is deferred to implementation but must integrate with the existing `FieldImportedV02` deferral pattern so there is one coherent imported-field error taxonomy.

### 2. Scattered vs gridded samples

CSV may carry either scattered point clouds or raster-order grids; HDF5 is typically gridded but can carry irregular meshes. v0.3 default policy mirrors v0.2's sampled-field fallback:

- **Scattered data** → nearest-neighbour lookup with a per-query warning when the query point falls further than the median inter-sample spacing from its nearest neighbour (proximity threshold configurable, default 2× median spacing).
- **Gridded data** (raster-order CSV, uniform-axis HDF5) → trilinear or tricubic interpolation, identical to the existing `sampled` and OpenVDB paths.

Proper scattered interpolation (kriging, RBF, IDW with adaptive radius) is post-v0.3 and called out in "Out of scope" below. The v0.3 nearest-neighbour fallback is explicitly a compatibility floor, not a recommended production approach for scattered data.

### 3. HDF5 dataset path mapping

Extend the `imported` block with three optional HDF5-specific keys:

```
source = imported {
    path             = "cfd_output.h5"
    format           = HDF5
    dataset          = "/fields/vonMises"
    axis_arrays      = ["/coords/x", "/coords/y", "/coords/z"]
    value_attribute  = "vonMises"
    units            = MPa
    interpolation    = trilinear
}
```

- `dataset` selects the named dataset within the HDF5 file hierarchy (HDF5's hierarchical-named-dataset model; path separator is `/`).
- `axis_arrays` points at coordinate arrays for non-uniform grids; omitted for uniform grids where axis spacing is encoded in the dataset attributes.
- `value_attribute` selects among multi-component datasets where the scalar/tensor of interest is one named component.

When `dataset` is omitted, the ingest layer looks for a root-level dataset named `"data"` or the single dataset present (error if ambiguous).

### 4. Unit metadata conventions

Reify recognises the CF Metadata Conventions (Climate-and-Forecast, the de facto standard for scientific HDF5 and NetCDF) `units` attribute on the value dataset. Resolution order:

1. **Explicit `units = ...` in the `imported` block** — authoritative override; always wins.
2. **CF `units` attribute on the HDF5 dataset** — used when no explicit declaration is present.
3. **`schema = { ..., value: Pressure(MPa) }` column type** — for CSV, the schema declaration is the unit source.
4. **Neither present** → compile-time diagnostic; the user must declare units. No silent dimensionless fallback — this upholds the boundary-contract provenance requirement from `docs/reify-language-spec.md` §14.5.

Conflict between an explicit `units` declaration and a CF attribute → compile-time diagnostic with a note showing both values. The declared value wins but the conflict is surfaced so the user can verify intent.

## Pre-conditions for activating

- v0.2 OpenVDB importer ships and stabilises; the `imported` source kind is exercised in at least one production example so the base-case error taxonomy is settled before HDF5/CSV extend it.
- At least one concrete user need is formally documented: FEA round-trip from HDF5 solver output; MRI/CT scan import; CFD pressure-field CSV from a lab instrument; density table from spreadsheet export.
- The per-purpose tolerance contract (`docs/prds/v0_2/per-purpose-tolerance.md`) is at design-spec stage so imported-data tolerance promises have a stable home in the language.

## Out of scope for this PRD

- Streaming / lazy loading for very large HDF5 datasets (post-v0.3 optimization).
- Write-back to HDF5 or CSV (Reify is the consumer, not the producer, of these formats).
- Format auto-detection from file extension (v0.3 keeps explicit `format = ...` for parity with v0.2 OpenVDB; surprises from auto-detection outweigh the convenience).
- Advanced scattered interpolation: kriging, RBF, IDW with adaptive radius — separate post-v0.3 feature.
- HDF5 SWMR (Single-Writer Multiple-Reader) or parallel-IO modes.
- CSV variants beyond RFC 4180 with header row: TSV, quoted multiline, non-UTF-8 encodings.
- Network-fetched files (`file://` only, matching v0.2 policy).
