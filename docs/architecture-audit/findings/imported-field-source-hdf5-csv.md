# Audit: `imported` Field Source — HDF5 and CSV (v0.3 follow-on)

**PRD path:** `docs/prds/v0_3/imported-field-source-hdf5-csv.md`
**Auditor:** audit-imported-field-source-hdf5-csv
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 17

## Top concerns

- **Base case is FICTION-in-disguise.** v0.2 OpenVDB decomp tasks 2665–2669 are all marked `done` with merge commits, but the `Imported` arm of `elaborate_field` still returns `Value::Undef` and the compiler's `compile_field` unconditionally emits `DiagnosticCode::FieldImportedV02` (a `Severity::Error`) for any `imported` block. The smoke test `imported_field_smoke_pins_v02_deferral_pipeline` pins the deferral as the *current contract*. The v0.3 PRD's pre-condition "v0.2 OpenVDB importer ships and stabilises" is therefore not actually met — and the v0.3 PRD has no mechanism that exists in code yet because none of the v0.2 plumbing is hooked end-to-end. (The OpenVDB ingest module exists in `reify-kernel-openvdb::ingest`, the provenance builder exists in `reify-eval::field_import_provenance`, the content-hasher exists in `reify-eval::engine_eval::hash_imported_file_content` — but nothing calls them from the field-elaboration path.)
- **Parser silently drops every new key the PRD proposes.** `FieldSource::Imported` in `reify-syntax/src/lib.rs:720` is a fixed-shape struct with only `path: Option<String>`, `format: Option<String>`, `grid: Option<String>`. The doc-comment is explicit that unknown keys (`units`, `interpolation`, and by extension `schema`, `dataset`, `axis_arrays`, `value_attribute`) are silently dropped at parse time with no extras field. The PRD's syntax sketches will parse without error but every new key vanishes — wrong-type and unknown-key diagnostics are explicitly out of scope per the AST design note.
- **Schema-block syntax has no grammar precedent.** The PRD shows `schema = { x: Length(mm), y: Length(mm), z: Length(mm), value: Pressure(MPa) }`. Reify map literals use `map { k => v, ... }` (`tree-sitter-reify/grammar.js:811`); the colon-separated curly-brace form is not a parsed expression shape. The `Length(mm)` form (dimension applied to a unit) is also not a known expression — `Length`/`Pressure` are *dimensions*, not callables, and `mm` is a unit declared as `pub unit mm : Length` in `crates/reify-compiler/stdlib/units.ri`.
- **No HDF5 or CSV crate dependency exists yet.** A workspace-wide grep of `Cargo.toml` files turns up no `hdf5`, `netcdf`, or `csv` crates. The ingest infrastructure is one OpenVDB-specific module (`reify-kernel-openvdb::ingest`); no kernel/adapter shape generalises across formats today.

## Mechanisms

### M-001: `imported { ... }` block produces a usable `Value::Field` lambda

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes runtime mechanism that code does not provide; compile-time deferral diagnostic in its place)
- **Evidence:** `crates/reify-compiler/src/functions.rs:399-411` (unconditional `FieldImportedV02` error + `CompiledFieldSource::Imported` placeholder); `crates/reify-eval/src/engine_eval.rs:621` (`Imported => Arc::new(Value::Undef)`); `crates/reify-eval/tests/imported_field_e2e.rs:60-123` (`imported_field_smoke_pins_v02_deferral_pipeline` pins the deferral as current contract — comments line 11-19 spell out what would need to change when the glue task lands)
- **Blocks:** every downstream consumer of `imported` fields (v0.3 HDF5/CSV PRD, FEA solver output ingestion, etc.); the v0.3 PRD's first listed pre-condition ("v0.2 OpenVDB importer ships and stabilises … exercised in at least one production example")
- **Note:** This is the base-case wiring v0.3 *extends*. v0.2 decomposition tasks 2665–2669 are all `done` with merge commits, but the eval-side glue task that calls `ingest::lower_to_sampled` from `elaborate_field` was never landed. The v0.3 PRD inherits an unimplemented mechanism.

### M-002: Extended `imported`-block grammar accepting new keys (`schema`, `dataset`, `axis_arrays`, `value_attribute`, `units`, `interpolation`)

- **State:** FICTION
- **Failure mode:** F1 (parser silently drops unknown keys; PRD assumes they round-trip)
- **Evidence:** `crates/reify-syntax/src/lib.rs:702-724` — `FieldSource::Imported` is a fixed three-field struct (`path`, `format`, `grid`) with explicit doc-comment that unknown keys and type-mismatched values are silently dropped at parse time; `crates/reify-syntax/tests/field_tests.rs:220-249` (`parse_imported_field_extra_keys_do_not_break_known_keys`) pins this behaviour. Tree-sitter grammar at `tree-sitter-reify/grammar.js:203-208` accepts arbitrary `field_config_entry` repeats but the AST layer flattens to the three fields.
- **Blocks:** All v0.3 HDF5/CSV functionality.
- **Note:** Migrating to a `Vec<(String, Expr)>`-based shape (like `Sampled`) would break "all `FieldSource::Imported { path, .. }` match sites" per the syntax-crate doc note. v0.3 will need that migration plus new key-validation in `compile_field`.

### M-003: `format = HDF5` and `format = CSV` recognised as valid format values

- **State:** FICTION
- **Failure mode:** F1 (no format-dispatch surface in code; v0.2 silently accepts any identifier as `format`)
- **Evidence:** No `FieldFormatKind` or equivalent enum exists. `FieldImportProvenance.format` is a free-form `String` (`crates/reify-types/src/provenance.rs:21`). The string `"OpenVDB"` appears in tests only (`crates/reify-types/src/provenance.rs:65`, `crates/reify-eval/src/field_import_provenance.rs:142`). No registry, no validation, no dispatch.
- **Blocks:** All v0.3 format-specific behaviour.
- **Note:** The PRD assumes a format discriminator drives ingestion routing. The current shape stores the format string in provenance only; there is no consumer that branches on its value.

### M-004: HDF5 file ingestion (read dataset, allocate sample buffer, lower to `SampledField`)

- **State:** FICTION
- **Failure mode:** F1 (no HDF5 crate dependency, no module, no FFI)
- **Evidence:** Workspace-wide grep for `hdf5`/`netcdf` in `Cargo.toml` files returns no matches. The only ingest module is `crates/reify-kernel-openvdb/src/ingest.rs` and it is OpenVDB-shape-specific (`OpenVdbGridSource`, `OpenVdbGridKind`, `OpenVdbInterpolation`).
- **Blocks:** All HDF5 functionality in this PRD.
- **Note:** Greenfield. Requires a new crate dependency, a new ingest module, and (probably) FFI to the C HDF5 library or a pure-Rust hdf5-metno binding.

### M-005: CSV file ingestion (parse rows, scattered/gridded classification, lower to `SampledField`)

- **State:** FICTION
- **Failure mode:** F1 (no CSV crate dependency, no module)
- **Evidence:** Workspace-wide grep for `csv` crate in `Cargo.toml` returns no matches. No CSV parser, no row-level error accumulator.
- **Blocks:** All CSV functionality in this PRD.
- **Note:** Greenfield. RFC 4180 with header row is the stated scope; bespoke parsing or `csv = "1"` from crates.io would both work but neither has been decided/landed.

### M-006: Inline `schema = { col: TypeOfDimension(unit_name), ... }` block syntax

- **State:** FICTION
- **Failure mode:** F1 (no grammar precedent; parser would need an entirely new sub-grammar)
- **Evidence:** Map literals in Reify use `map { k => v, ... }` (`tree-sitter-reify/grammar.js:811`), not `{ k: v }`. `field_config_entry` accepts a single expression on the RHS (`grammar.js:190-194`). No `record_literal` / `typed_field_list` / similar construct exists in the grammar.
- **Blocks:** CSV import (mandatory) and HDF5 column-override (optional per PRD).
- **Note:** This is a new design surface, not a small extension of an existing one — comparable in cost to the original `imported`-block grammar.

### M-007: `Length(mm)` / `Pressure(MPa)` typed-column expression form

- **State:** FICTION
- **Failure mode:** F1 (the expression `Length(mm)` is not a known Reify expression — `Length` is a dimension type, not a callable)
- **Evidence:** `crates/reify-compiler/stdlib/units.ri:14-19` declares `Length` as a dimension via `pub unit m : Length`. There is no `fn Length(unit: Unit) -> ...` declaration anywhere in the stdlib. The PRD claims this "reuses the existing unit-literal grammar (see `docs/prds/money-dimension.md` and `crates/reify-compiler/stdlib/units.ri`) — no new parser surface for units", but the cited references describe unit declarations (`pub unit mm : Length`), not a type-applied-to-unit expression syntax.
- **Blocks:** Per-column unit declaration for CSV schemas.
- **Note:** Either an extant grammar surface needs to be cited that this auditor missed, or the PRD's "no new parser surface" claim is incorrect.

### M-008: Scattered vs gridded sample classification at ingestion time

- **State:** FICTION
- **Failure mode:** F1 (no classification surface; current `SampledField` is grid-only)
- **Evidence:** `crates/reify-types/src/sampled.rs` (referenced indirectly via `SampledField`/`SampledGridKind` imports in `reify-kernel-openvdb/src/ingest.rs:30-32`) — only structured-grid kinds (`Regular1D/2D/3D` via `OpenVdbGridKind`). No `Scattered` variant exists.
- **Blocks:** Most CSV import (scattered point clouds) and irregular HDF5 meshes.
- **Note:** The `SampledField` data model would need a scattered-points variant, or scattered data would need a separate `Value::Field` carrier shape.

### M-009: Nearest-neighbour lookup with proximity-threshold warning for scattered queries

- **State:** PARTIAL
- **Failure mode:** F2 (nearest-neighbour interpolation exists on uniform grids but no scattered-point query path)
- **Evidence:** `crates/reify-expr/src/interp.rs:52-68` defines `InterpolationMethod::NearestNeighbor`, but the implementation operates on a grid (`fn locate_cell` at `interp.rs:115`). No KD-tree / spatial index / scattered-NN code exists. No "query further than median spacing → warning" surface.
- **Blocks:** Scattered CSV consumption.
- **Note:** Existing NN supports the uniform-grid case only. Scattered NN requires entirely new query infrastructure.

### M-010: Median inter-sample spacing precomputation for scattered-data proximity-warning threshold

- **State:** FICTION
- **Failure mode:** F1 (no precomputation surface for scattered statistics)
- **Evidence:** No `median_spacing`, no `MedianInterSampleSpacing`, no scattered-sample statistics module exists in `reify-types`, `reify-expr`, or `reify-kernel-*`. The proximity-warning policy is described in the PRD but has no implementation footprint.
- **Blocks:** The PRD's "2× median spacing" warning policy.
- **Note:** Independent of the NN-query infrastructure but needs to happen at ingest time so per-query overhead stays low.

### M-011: Trilinear / tricubic interpolation for gridded HDF5/CSV data

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-expr/src/interp.rs:52-68` defines `InterpolationMethod::Linear` (trilinear in 3D) and `Cubic` (tricubic in 3D), both implemented. This path is already exercised by `sampled` fields and the OpenVDB-ingest lowering reuses it.
- **Blocks:** N/A
- **Note:** Only piece of the PRD that lands on extant, working machinery — but a gridded HDF5/CSV import still needs the upstream key parsing and format-specific ingest before it can reach this code.

### M-012: HDF5 hierarchical dataset path resolution (`/fields/vonMises`)

- **State:** FICTION
- **Failure mode:** F1 (no HDF5 reader exists; see M-004)
- **Evidence:** Same evidence as M-004 (no HDF5 crate or module).
- **Blocks:** HDF5 import for any file with non-default dataset placement.
- **Note:** Strictly subordinate to M-004 but called out separately because the PRD makes a specific design commitment to HDF5 path-separator semantics and default-dataset auto-pick (`"data"` or single-dataset).

### M-013: HDF5 `axis_arrays` reading for non-uniform-grid coordinate axes

- **State:** FICTION
- **Failure mode:** F1 (no HDF5 reader; `SampledField` representation may also lack non-uniform-spacing axis support)
- **Evidence:** No HDF5 crate (M-004 evidence). `OpenVdbGridSource.spacing: Vec<f64>` (`reify-kernel-openvdb/src/ingest.rs:73`) is one spacing scalar per axis (uniform grid) — non-uniform-axis spacing is not in scope of the v0.2 lowered shape.
- **Blocks:** HDF5 import for non-uniform grids.
- **Note:** May require a new `SampledField` representation that carries per-axis sample positions, not just spacing.

### M-014: HDF5 `value_attribute` selector for multi-component datasets

- **State:** FICTION
- **Failure mode:** F1 (no HDF5 reader; no multi-component selection surface)
- **Evidence:** No HDF5 crate (M-004 evidence). `SampledField` codomain is scalar/vector/tensor per its `Type`, but no component-selection wiring at ingest.
- **Blocks:** Multi-component HDF5 datasets (common for CFD output).
- **Note:** Strictly subordinate to M-004.

### M-015: CF Metadata Conventions `units` attribute reading from HDF5 dataset

- **State:** FICTION
- **Failure mode:** F1 (no HDF5 reader; no CF conventions parser)
- **Evidence:** Same as M-004. No reference to CF / Climate-and-Forecast conventions anywhere in the codebase.
- **Blocks:** HDF5 unit auto-discovery (the PRD's preferred path when no explicit `units = ...` is given).
- **Note:** The PRD also specifies a conflict diagnostic between explicit and CF-derived units — additional surface that has no current home.

### M-016: Row-level malformed-row diagnostic + skip-with-warning + skip-count provenance

- **State:** FICTION
- **Failure mode:** F1 (no CSV reader; no row-skip provenance field on `FieldImportProvenance`)
- **Evidence:** `crates/reify-types/src/provenance.rs:17-33` — `FieldImportProvenance` has five fields (`path`, `format`, `content_hash`, `ingestion_timestamp_secs`, `declared_tolerance_si`). No `rows_skipped` / `malformed_row_count` field. No CSV reader exists (M-005).
- **Blocks:** CSV import.
- **Note:** The PRD calls out silent discard as not acceptable. The implementation needs both a diagnostic-emission path at ingest and an extension to `FieldImportProvenance`.

### M-017: Schema-mismatch compile-time diagnostic (`E_FIELD_IMPORTED_SCHEMA_MISMATCH` or successor)

- **State:** FICTION
- **Failure mode:** F1 (diagnostic code does not exist; no schema-validation surface)
- **Evidence:** `crates/reify-types/src/diagnostics.rs:253` defines `FieldImportedV02` (deferral marker) but no schema-mismatch variant. The PRD explicitly defers the exact mnemonic but commits to integration with the "existing `FieldImportedV02` deferral pattern so there is one coherent imported-field error taxonomy" — and that pattern is itself the placeholder error for the un-landed v0.2 wiring.
- **Blocks:** Compile-time validation of CSV schemas against runtime file structure.
- **Note:** Validation requires reading enough of the file at compile time (or first eval) to compare against the declared schema — design implications around when the file is opened.

### M-018: Unit resolution order (explicit `units` > CF attribute > `schema` column type > error)

- **State:** FICTION
- **Failure mode:** F1 (none of the three units sources exists — no explicit-units parsing on imported block, no CF reader, no schema block)
- **Evidence:** Compile-side `FieldSource::Imported` drops `units` (M-002). No CF reader (M-015). No `schema` grammar (M-006). The resolution-order policy is described in the PRD with no implementation footprint.
- **Blocks:** Boundary-contract provenance (the PRD ties unit declaration to spec §14.5 — no silent dimensionless fallback allowed).
- **Note:** Implements the boundary-contract requirement that imported data carry meaningful units. Coupled to the per-purpose tolerance contract (PRD lists per-purpose tolerance as a v0.3 pre-condition).

## Cross-PRD breadcrumbs

- **v0.2 base PRD (`docs/prds/v0_2/imported-field-source.md`)** — the base case this PRD extends has decomp tasks 2665–2669 marked `done`-with-merge-commits but the eval-side glue is not actually wired (see M-001). The "Resolved design decisions" + decomp plan + tasks-done state is misleading: a downstream auditor reading the v0.2 PRD would reasonably conclude OpenVDB import works, but the smoke test pins `Value::Undef` as the actual output.
- **v0.3 FEA PRD (`docs/prds/v0_3/structural-analysis-fea.md`)** — explicitly lists `v0.2 imported-field-source has shipped` as a pre-condition (`structural-analysis-fea.md:102`). If M-001 is correct, that pre-condition is not met for the FEA PRD either.
- **`docs/prds/money-dimension.md`** — cited by the v0.3 PRD as providing the unit-literal grammar that `Length(mm)` reuses. Audit of that PRD would clarify whether the syntax exists or whether the v0.3 PRD's reference is incorrect (M-007).
- **`docs/prds/v0_2/per-purpose-tolerance.md`** — listed as a pre-condition (status `deferred to v0.2`). Coupling point for tolerance promises on imported data.
- **`docs/prds/v0_2/multi-kernel.md`** — names OpenVDB ingestion as the key motivator for the OpenVDB sub-kernel; cited by `multi-kernel.md:60` ("OpenVDB unblocks the `imported` field source").
