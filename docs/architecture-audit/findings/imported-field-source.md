# Audit: `imported` Field Source (OpenVDB / CSV / HDF5)

**PRD path:** `docs/prds/v0_2/imported-field-source.md`
**Auditor:** audit-imported-field-source
**Date:** 2026-05-12
**Mechanism count:** 13
**Gap count:** 10

## Top concerns

- **Wire site is missing.** Every supporting infrastructure piece exists (parser ✓, AST variant ✓, compiler arm ✓, provenance builder ✓, content-hash helper ✓, cache side-table ✓, OpenVDB ingestion `read_vdb_file` ✓, in-memory `lower_to_sampled` ✓, e2e deferral pin ✓), but `engine_eval::elaborate_field`'s `CompiledFieldSource::Imported` arm still returns `Value::Undef`. None of the supporting helpers are called in production. PRD task 5 (#2669) is the wire site — pending.
- **Deferral diagnostic still hard-coded.** `reify-compiler/src/functions.rs:399-411` emits `DiagnosticCode::FieldImportedV02` unconditionally on every `imported` field, so even with all the underlying infra wired the language surface still rejects the program with `Severity::Error`. Removing this is part of the task 5 work surface but is a distinct cross-crate change with no flag/feature gate.
- **`reify-eval` does not depend on `reify-kernel-openvdb`.** Neither `reify-eval/Cargo.toml` nor `reify-compiler/Cargo.toml` declares a dep on `reify-kernel-openvdb`. Task 5 must add this dep (or invert the call direction). Today `read_vdb_file` is unreachable from the engine, which is why all the scaffolding compiles with `dead_code` allows.
- **Content-hash composition for the realization cache is stubbed.** `compile_field` emits `ContentHash::of(&[0u8])` as the `source_hash` for `CompiledFieldSource::Imported` (functions.rs:432). The path/format/grid strings parsed in the AST are *not* threaded through to the compiled content hash, so two `imported` fields differing only in their `path` produce identical compiled-field hashes today — a real cache-correctness hazard once any wiring lands (separate from the runtime file-content hash held in `CacheStore::imported_file_hashes`).

## Mechanisms

### M-001: `imported { path = … format = X grid = "…" }` grammar + parse

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `tree-sitter-reify/grammar.js` (`field_source_imported`); `crates/reify-syntax/src/ts_parser.rs:761-808`; `crates/reify-syntax/src/lib.rs:695-725`; `crates/reify-syntax/tests/field_tests.rs:111-345`, `crates/reify-syntax/tests/edge_case_tests.rs:84-95`. Task 2665 (done).
- **Blocks:** —
- **Note:** Parse-time uses typed `Option<String>` for `path`/`format`/`grid` and silently drops unknown keys + wrong-type values (intentional, documented). Wrong-type values cannot be distinguished from absent values at compile time — see ts_parser.rs:790-803 design note.

### M-002: Compiler validation of `imported` source (presence + diagnostic)

- **State:** PARTIAL
- **Failure mode:** F3 (PRD assumes "deferral diagnostic" while task 2666 is pending; the diagnostic blocks the surface even when downstream infra is present)
- **Evidence:** `crates/reify-compiler/src/functions.rs:399-411`; `crates/reify-compiler/tests/field_compile_tests.rs:674-708`. `DiagnosticCode::FieldImportedV02` always fires.
- **Blocks:** Tasks 2666, 2667, 2668, 2669 (without removing this error, end-to-end use is impossible).
- **Note:** Per PRD's narrowed scope (OpenVDB-only), once task 5 wires the eval/ingestion path, this `Severity::Error` should be removed (or downgraded to a per-format gate). It is presently the canonical "this feature does not work yet" signal and a test pins it (imported_field_e2e.rs:60-79).

### M-003: `imported` field is lowered to `CompiledFieldSource::Imported` (compiler IR)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:92-100`; `crates/reify-compiler/src/functions.rs:410`; round-trip pinned by `compile_field_imported_emits_v02_deferral_diagnostic`.
- **Blocks:** —
- **Note:** `CompiledFieldSource::Imported` carries no payload (the parsed path/format/grid are discarded between parser and compiler). Any wiring task will need to extend this variant with the parsed strings or re-parse from the syntax AST — small refactor, but not present.

### M-004: Content-hash composition for `Imported` source includes import params

- **State:** FICTION
- **Failure mode:** F4 (stub value masquerading as content addressing — `ContentHash::of(&[0u8])`)
- **Evidence:** `crates/reify-compiler/src/functions.rs:432`. Comment: no per-field path/format/grid signal. Two distinct imported fields → identical source_hash.
- **Blocks:** Realization-cache correctness for PRD task 5 (#2669); persistent-fea-cache PRD (depends on stable content hashes).
- **Note:** Decision-pending. Compiler-side content hash composition for `Imported` must include the literal path/format/grid strings (or whatever payload the IR carries). Today it is a placeholder `[0u8]` constant.

### M-005: OpenVDB in-memory lowering to `SampledField`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-openvdb/src/ingest.rs:296-434` (`lower_to_sampled`); covered by `tests/ingest_tests.rs` and internal mod tests (units, axis-length, degenerate-axis, bounds checks all wired); `KNOWN_UNITS` table at line 251-268.
- **Blocks:** —
- **Note:** Solid coverage of error surface (UnitMismatch / UnknownUnit / UnsupportedCodomain / EmptyGrid / DataShapeMismatch / InvalidSpacing / AxisLengthMismatch / InvalidBounds / DegenerateAxis / ExcessiveAxisLength / OverflowingAxisLength). Interpolation map (Quadratic→Cubic, Staggered→Linear) wired with `InterpolationDeferred` warnings.

### M-006: OpenVDB `.vdb` file-read (FFI path)

- **State:** PARTIAL
- **Failure mode:** F2 (cfg-gated on `has_openvdb`; stub mode returns `IngestError::FfiNotImplemented`)
- **Evidence:** `crates/reify-kernel-openvdb/src/ingest.rs:541-651` (`read_vdb_file`); `build.rs:86` (`cargo:rustc-cfg=has_openvdb`); `tests/ffi_smoke_tests.rs` (two-arm split). 2645 (OpenVDB sub-kernel adapter) marked done at dd2496a934.
- **Blocks:** Task 2666 end-to-end testing in CI environments where `/opt/reify-deps` is absent.
- **Note:** Real FFI body lands behind `cfg(has_openvdb)`. Workspaces missing OpenVDB get a structured `FfiNotImplemented` error rather than a panic. CI status w.r.t. `has_openvdb` not verified in this audit (see Cross-PRD breadcrumbs).

### M-007: Wire site — `elaborate_field`'s `CompiledFieldSource::Imported` arm

- **State:** FICTION
- **Failure mode:** F1 (PRD-stated runtime behavior; engine returns `Value::Undef`)
- **Evidence:** `crates/reify-eval/src/engine_eval.rs:621` (`reify_compiler::CompiledFieldSource::Imported => Arc::new(Value::Undef)`); pinned by `crates/reify-eval/tests/imported_field_e2e.rs:96-122` (asserts `lambda == Value::Undef`). All supporting helpers (`hash_imported_file_content` line 672 marked `#[allow(dead_code, reason = "wired into elaborate_field by PRD task 5")]`, `field_import_provenance::build_field_import_provenance`, `CacheStore::imported_file_hash_changed`) exist but have zero non-test call sites.
- **Blocks:** Task 2669 (#2669, "end-to-end smoke test"); the PRD-promised user surface.
- **Note:** This is the keystone gap. The infra is built; it must be plumbed into one match arm plus dependency edits.

### M-008: `reify-eval` → `reify-kernel-openvdb` dependency edge

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes engine can call OpenVDB ingestion; no dep declared)
- **Evidence:** `crates/reify-eval/Cargo.toml` carries no `reify-kernel-openvdb` entry (verified by grep on Cargo.toml files). `read_vdb_file` is therefore unreachable from `elaborate_field`.
- **Blocks:** Task 2666 / 2669 wire-up.
- **Note:** Adding this edge may have build-graph implications (reify-kernel-openvdb depends on the OpenVDB C++ FFI; reify-eval is a fast inner-loop crate). Architecturally a registry/dispatcher indirection may be preferable (matches the multi-kernel dispatch pattern); flagging only.

### M-009: Provenance recording via `FieldImportProvenance` on Input occurrence

- **State:** PARTIAL
- **Failure mode:** F2 (builder ready; no production caller)
- **Evidence:** `crates/reify-types/src/provenance.rs:6-33` (struct, all 5 fields); `crates/reify-eval/src/field_import_provenance.rs:65-89` (`build_field_import_provenance`, with cross-extractor Gate 4 filter); rich unit tests at lines 91-183. No non-test call site (grep confirms). Task 2667 pending.
- **Blocks:** Task 2667; arch §14.5 promise for imported fields.
- **Note:** Builder doc explicitly says "Task 5 of the decomposition plan will call this builder from `elaborate_field`'s `CompiledFieldSource::Imported` arm once the end-to-end wiring lands" (field_import_provenance.rs:13-17). Provenance is also not threaded into the `Value::Field` variant — `Value::Field` has no provenance slot today.

### M-010: Content-hash of source-file bytes (cache key contribution)

- **State:** PARTIAL
- **Failure mode:** F2 (helper exists; not called)
- **Evidence:** `crates/reify-eval/src/engine_eval.rs:645-681` (`hash_imported_file_content`, dead_code allowed by `#[allow(dead_code, reason = "wired into elaborate_field by PRD task 5")]`); `CacheStore::record_imported_file_hash` / `get_imported_file_hash` / `imported_file_hash_changed` at cache.rs:319-385 also unused outside their own unit tests (cache.rs:3727+). Path-content separation correctly implemented (path string not mixed into hash domain), pinned by `hash_imported_file_content` tests.
- **Blocks:** Task 2668; PRD acceptance property "file-path change with same content → cache hit".
- **Note:** The side-table `imported_file_hashes: HashMap<String, ContentHash>` lives in `CacheStore` and is keyed by literal user-supplied path string — see growth-policy commentary on cache.rs:237-247 (monotonic growth until `clear()`).

### M-011: Cache invalidation on file content change via `imported_file_hash_changed`

- **State:** PARTIAL
- **Failure mode:** F2 (predicate exists; no consumer)
- **Evidence:** `crates/reify-eval/src/cache.rs:360-385` — 3-branch predicate (cold/equal/changed) is implemented and tested in isolation; nothing in eval invokes it. Doc explicitly names `elaborate_field` as the future wire site. Realization-cache key (`field.content_hash`) does *not* include the file-byte hash today (see M-004).
- **Blocks:** Task 2668; persistent-fea-cache PRD's content-addressed assumptions for imported fields.
- **Note:** Even once `imported_file_hash_changed` is wired into elaborate_field, this signals invalidation but does not by itself participate in cross-engine cache-key composition unless M-004 is also resolved.

### M-012: `Value::Field { source: FieldSourceKind::Imported }` → `SampledField` runtime value

- **State:** FICTION
- **Failure mode:** F1 (PRD says imported is lowered to internal `sampled` representation; runtime produces `Value::Undef`)
- **Evidence:** `engine_eval.rs:621` produces `Value::Undef` lambda; `FieldSourceKind::Imported` is recognised on the source-kind side (line 634). PRD: "Each `imported` field is realized as a `sampled` field internally — the import layer reads the file once, allocates the sample buffer, and the rest of the field machinery treats it identically".
- **Blocks:** Any consumer that samples / gradients / composes an imported field (`reify-expr` field reduction code path comments at field_reductions.rs:105 acknowledge "Imported fields carry Value::Undef in their lambda").
- **Note:** Downstream samplers must already special-case Value::Undef in the lambda position or they will produce wrong results. The PRD's "indistinguishable from a sampled field" property is not yet user-observable.

### M-013: End-to-end smoke + diagnostic coverage (file-not-found, grid-not-in-file, unit mismatch, malformed file)

- **State:** PARTIAL
- **Failure mode:** F2 (deferral pinned end-to-end; real-path diagnostic surface only covered at kernel-OpenVDB layer)
- **Evidence:** `crates/reify-eval/tests/imported_field_e2e.rs` pins the v0.2 deferral state explicitly (test name `imported_field_smoke_pins_v02_deferral_pipeline`); ingest_tests.rs covers ingestion-layer diagnostics. No end-to-end test that wires parse → compile → eval → file-read → SampledField → query. Task 2669 pending.
- **Blocks:** —
- **Note:** Test file documents (lines 11-19) exactly what to update once the deferral lifts. Good "what to do next" breadcrumb but no live coverage of the integrated path.

## Cross-PRD breadcrumbs

- **`persistent-fea-cache` PRD** depends on content-addressed cache keys; M-004's stub content hash for `Imported` would let two imports with different paths collide in any persistent cache. Cross-cite.
- **`multi-kernel` PRD** (`v0_2/multi-kernel.md`) owns the OpenVDB sub-kernel adapter (#2645, done) and is the natural home for the dispatcher indirection that M-008 might use instead of a direct `reify-eval → reify-kernel-openvdb` edge.
- **`structural-analysis-fea` / `multi-load-case-fea`** PRDs invoke `FieldImportProvenance` only indirectly (via the arch §14.5 Input-occurrence contract). They would be the first downstream consumers if/when imported fields go live for FEA stress-field round-tripping (a worked-example use case in the PRD itself).
- **Tolerance-promise machinery** (`tolerance_promise.rs`, `tolerance_combine.rs`) shares the Gate 4 filter with `build_field_import_provenance` — the auditor flags this as a small but real cross-module coupling concentrated on `Option<f64>` declared-tolerance fields. Not a gap; informational.
- **`imported-field-source-hdf5-csv.md`** is the follow-on PRD (HDF5/CSV deferred); auditor did not chase. Same wire-site question reappears there.
