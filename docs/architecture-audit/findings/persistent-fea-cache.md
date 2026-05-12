# Audit: Persistent Cross-Session FEA Cache

**PRD path:** `docs/prds/v0_3/persistent-fea-cache.md`
**Auditor:** audit-persistent-fea-cache
**Date:** 2026-05-12
**Mechanism count:** 21
**Gap count:** 13

## Top concerns

- The lower half (foundation: trait, header, atomic I/O, engine-version-hash, dir resolution) is fully WIRED in `crates/reify-eval/src/persistent_cache.rs` (3071 LOC) and `crates/reify-config/src/cache.rs` (1001 LOC), but **none of it is called from anywhere** outside its own module tests. There is zero call-site coupling to the engine. The PRD's headline benefit ("open project → first interaction returns instantly") is unreachable until task 2974 lands.
- The integration task 2974 (ComputeNode → persistent-cache lookup/write hooks) is `pending` and gated on task 2924 (FEA engine integration of ComputeNode itself, also `pending`), which is in turn gated on 9 prereq tasks 3377-3385 (mixed done/pending/deferred). The persistent-cache "extends FEA cache as determinism anchor across sessions" promise depends on a ComputeNode dispatch path that has not yet shipped — this PRD's resolved design assumes a substrate (in-memory ComputeNode cache, OpaqueState attachment, `solve_elastic_static` registration) that doesn't fully exist yet.
- `compute_cache_key` (in-memory key composer, `crates/reify-eval/src/compute_cache_key.rs`) does NOT fold in `engine_version_hash`. Task 2974's description states the persistent key = `hash(in-memory-key, engine_version_hash)` — the composition happens in 2974, not in `compute_cache_key`. Watch in Phase 3 whether the in-memory cache could go stale across engine bumps within a single long-lived process (today's compute_cache_key would alias).
- CLI surface (`reify cache stats|clear|gc|export|import`, tasks 2976/2977) is entirely absent from `crates/reify-cli/src/main.rs`. Task 2976 is `pending`, 2977 is `in-progress`. PRD's documentation deliverable (2981) is downstream of both.

## Mechanisms

### M-001: `PersistentlyCacheable` opt-in trait

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:399-442` (trait body); `:697-822` (`impl PersistentlyCacheable for ElasticResult`); `:364-372` compile-time assertion; task 2969 done (commit `5c92298fe3`)
- **Blocks:** —
- **Note:** Object-unsafe by design; uses `impl Write / impl Read` generics for zstd encoder monomorphization. Co-located with `ElasticResult` in `reify-eval` because `reify-stdlib → reify-eval` would form a dep cycle (documented at `:9-18`).

### M-002: `ElasticResult` value type with field arrays

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:444-457`; six fields (`displacement: Vec<f64>`, `stress: Vec<f64>`, `max_von_mises`, `converged`, `iterations`, `solve_time_ms`); compile-time assertions; round-trip test at `:2710`
- **Blocks:** —
- **Note:** Container exists in Rust but is NOT yet produced or consumed by any solver dispatch path; the kernel solver returns its own result type that hasn't been bridged into this container yet (gate is 2924).

### M-003: On-disk filesystem layout (2-level git-style sharding)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:652-695` (`shard_dir`, `entry_meta_path`, `entry_bin_path`); task 2972 done (commit `aebbbafe43`)
- **Blocks:** —
- **Note:** `<cachedir>/<engine_version_hash>/<input_hash[0:2]>/<input_hash>.{bin,meta}` exactly per PRD.

### M-004: Bincode entry header (format_version, hash echoes, byte_size, written_at, solve_time_ms)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:150-330` (`CacheEntryHeader` + `ENTRY_FORMAT_VERSION=1` + `ENTRY_HEADER_ENCODED_LEN=92`); `:332` `ENGINE_VERSION_HASH` const
- **Blocks:** —
- **Note:** Format-version is intentionally separated from engine-version-hash per PRD §"Header schema"; both checked on read with header echoes for corruption detection.

### M-005: Sidecar `.meta` mtime tracking under `noatime`/`relatime` mounts

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:52-148` (`SIDECAR_MAGIC_BYTE=0xCA`, `read_sidecar_mtime`, `touch_sidecar`, `write_sidecar`); `:756` round-trip test
- **Blocks:** —
- **Note:** Touches `.meta` mtime on read so the `.bin` mtime preserves `written_at`; uses `std::fs::FileTimes::set_modified` (stable 1.75+); tolerates absent sidecar (ENOENT → `Ok(())`).

### M-006: Atomic write-then-rename via in-shard tempfile + dir fsync

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:872-955` (`write_entry`); `:921` body `sync_all`; `:936` shard-dir fsync after rename; concurrent-writer test at `:2804`
- **Blocks:** —
- **Note:** Tempfile lives in `<cachedir>/.../shard/` so atomic rename works on the same FS; sidecar written after rename + dir fsync to avoid orphan sidecars on writer crash.

### M-007: Read path with header verification + sidecar touch + corruption-as-miss

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/persistent_cache.rs:956+` (`read_entry`); corruption tests at `:2880`, `:2924`, `:2972`, `:3000`
- **Blocks:** —
- **Note:** Returns `Ok(None)` on all corruption / mismatch / partial-write cases, never propagates as error — matches PRD "treat as miss" policy.

### M-008: Canonical build-time `ENGINE_VERSION_HASH`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/build.rs:54-87`; `crates/reify-eval/src/engine_hash_algo.rs:1-384` (shared algorithm); task 2970 done (commit `95b3d3c6af`); 5 contributor categories enumerated; directory-level `cargo:rerun-if-changed` directives for new-file detection
- **Blocks:** —
- **Note:** PRD-line-59 "materials database" contributor deferred until materials live in a versioned source file (build.rs `:35`). Cache only sees one canonical hash; composition is intentionally opaque.

### M-009: Cache directory resolution (XDG default + CLI/env/config overrides)

- **State:** PARTIAL
- **Failure mode:** F2 (mechanism implemented but not wired to a consumer)
- **Evidence:** `crates/reify-config/src/cache.rs:1-1001`: `default_cache_dir`, `CacheConfig`, `parse_cache_config`, `resolve_cache`, `CacheError`, `load_cache_config_from_path` — all defined; tested in `crates/reify-config/tests/cache_resolution.rs`. **BUT:** no `use reify_config::cache::` outside tests; no callers; `REIFY_CACHE_DIR` / `REIFY_CACHE_MAX_BYTES` env vars are not read by any consumer. Task 2971 marked done (commit `687c6a95f1`).
- **Blocks:** 2974 (integration), 2976 (CLI), 2978 (startup sweep)
- **Note:** Library exists; no engine call-site invokes it. Task 2971 is closed but practical effect is zero until a consumer is wired. PRD §"Cache directory resolution" also calls for a "best-effort NFS-detection warning at startup" — no `statvfs` or FS-type check is present in `cache.rs`.

### M-010: NFS/SMB network-FS detection warning at startup

- **State:** TODO
- **Failure mode:** F4 (declared in PRD + task description but not implemented)
- **Evidence:** PRD line 42 "Local-disk only"; task 2971 description "Detect at startup; emit warning if cache dir appears to be on a network FS (best-effort heuristic, e.g. statvfs FS type), but do not refuse"; no `statvfs` / `nix::sys::statvfs` / `MagicNumber` reference in repo (`grep -rn "statvfs\|network\|nfs" crates/reify-config/src/cache.rs` finds nothing).
- **Blocks:** —
- **Note:** Cosmetic-but-load-bearing for the "atomic rename semantics murky on NFS" failure mode. Task 2971 is marked done despite this requirement being part of its body.

### M-011: ComputeNode persistent-cache lookup + write integration

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes the layer exists at runtime; engine has zero call-sites)
- **Evidence:** `grep PersistentlyCacheable\|read_entry\|write_entry` outside `persistent_cache.rs` returns only doc references in `progressive.rs:102`. `engine_eval.rs`, `graph.rs`, `dirty.rs`, `engine_admin.rs`, `engine_edit.rs` contain no persistent-cache calls. Task 2974 pending; deps 2924 + 2969 + 2973 (2924 pending).
- **Blocks:** 2979 (cheap-result threshold), 2980 (cross-session tests), 2981 (docs)
- **Note:** The PRD's headline value proposition (cross-session hits) is unreachable until this lands. Task description calls for the persistent-key composition to be `hash(in-memory-key, engine_version_hash)` — this composition logic does not exist anywhere yet (`compute_cache_key.rs` does not fold engine-version-hash). See M-012.

### M-012: In-memory ComputeNode cache key folding engine-version-hash

- **State:** DRIFT
- **Failure mode:** F3 (PRD/task implies a contract that the implementation does not reflect)
- **Evidence:** `crates/reify-eval/src/compute_cache_key.rs:91-160` composes `combine_all([target_hash, value_bucket, realization_bucket, options_hash])` with NO engine-version-hash term. Task 2974 description: persistent-key = `hash(in-memory-key, engine_version_hash)` — so engine_version_hash is added at the *persistent* layer, not in `compute_cache_key`. The PRD itself is silent on which layer adds engine_version_hash, so the drift is between task 2974's text and the implementation surface — not necessarily PRD vs code.
- **Blocks:** —
- **Note:** Phase-3 question: should in-memory cache also key on engine-version-hash? In a long-lived process where the version cannot change, no. Across a hypothetical hot-reload it would matter. Documenting because the PRD-implied "engine bump → all entries miss" guarantee is currently delegated entirely to the on-disk path; in-memory entries from prior versions in the same process would not naturally invalidate.

### M-013: Cost-aware LRU eviction with 25 GB cap (opportunistic-on-write)

- **State:** TODO
- **Failure mode:** F4 (named in PRD + task; implementation in progress, not landed)
- **Evidence:** Task 2975 `in-progress`. `grep "fn evict\|cost_aware\|lru\|LruGc"` in `persistent_cache.rs` returns nothing; only `crates/reify-eval/src/warm_pool.rs` has an LRU (for warm-state, not for cache entries). `DEFAULT_CACHE_MAX_BYTES = 25 * 1024 * 1024 * 1024` exists at `crates/reify-config/src/cache.rs:24` but no GC code references it.
- **Blocks:** 2976 (CLI gc subcommand), 2980 (cross-session tests)
- **Note:** Eviction score is `last_access_age_seconds / max(solve_time_ms, 1)` per task description. Per-engine-version-subdir scope. Until this lands, the cache grows without bound — significant for a 25 GB default on a typical dev machine.

### M-014: `reify cache stats / clear / gc` CLI subcommands

- **State:** TODO
- **Failure mode:** F4 (PRD §"CLI surface" enumerates them; task 2976 pending)
- **Evidence:** `grep "cache stats\|cache gc\|cache clear" crates/reify-cli/src` returns no source matches; `crates/reify-cli/src/main.rs` defines no `cache` subcommand. Task 2976 pending, deps 2973 + 2975.
- **Blocks:** 2980 (cross-session tests rely on `cache stats`/`clear`), 2981 (docs)
- **Note:** PRD §"CLI surface" itemizes 5 subcommands (stats, clear, gc, export, import). None exist in `reify-cli`. Cargo.toml of `reify-cli` does not depend on `reify-config` either, so even import wiring is not in place.

### M-015: `reify cache export / import` tarball primitive

- **State:** TODO
- **Failure mode:** F4 (PRD line 78-79; task 2977 `in-progress` but no implementation present)
- **Evidence:** Task 2977 `in-progress`; `grep "cache export\|cache import\|tar"` in `crates/reify-cli/src/` returns nothing matching this surface. Mentioned only in PRD doc.
- **Blocks:** 2981 (docs), and unblocks every team-distribution story (git-LFS, S3, CI artifact)
- **Note:** Validation contract per task: import with mismatched engine-version-hash must warn-and-skip, not fail or pollute. None of that policy is enforced anywhere yet.

### M-016: Engine startup sweep (stale `.tmp.*` + orphan engine-version dirs)

- **State:** TODO
- **Failure mode:** F4 (PRD §"Concurrency" + §"Stale tempfile cleanup"; task 2978 pending)
- **Evidence:** Task 2978 pending; `grep "engine_admin\|startup_sweep\|stale_tempfile\|orphan_engine"` in `crates/reify-eval/src/engine_admin.rs` returns nothing. `engine_admin.rs` does not call into `persistent_cache::*`.
- **Blocks:** —
- **Note:** 1-hour `.tmp.*` threshold + 30-day orphan-version threshold. Without this, every crashed write leaves a tempfile that lives forever, and every dev's accumulating engine versions stay on disk.

### M-017: Cheap-result skip threshold (measure-don't-guess)

- **State:** TODO
- **Failure mode:** F4 (PRD §"Cheap-result threshold"; task 2979 pending; transitively gated on M-011 which is FICTION)
- **Evidence:** Task 2979 pending; deps include 2974. No `cheap_result_skip_threshold_ms`, `load_time_ms`, or instrumentation histograms in the codebase (`grep` returns zero matches).
- **Blocks:** —
- **Note:** Task explicitly says "instrument cache.load_time_ms / cache.write_time_ms / compute.solve_time_ms, run representative workload, set threshold from data". Currently un-instrumented and un-measured. Reads are never skipped — only writes.

### M-018: Cross-session integration tests + reproducibility guarantee under `#deterministic`

- **State:** TODO
- **Failure mode:** F4 (task 2980 pending; gated on every other gap above)
- **Evidence:** Task 2980 pending; no `crates/reify-eval/tests/persistent_cache_integration.rs` (only the unit tests in `persistent_cache.rs` itself); the 6 test scenarios called out by 2980 (cross-session hit, engine-version invalidation, concurrent writers, cost-aware eviction, partial-write recovery, determinism anchor across sessions) have no end-to-end coverage.
- **Blocks:** 2981 (docs)
- **Note:** The "cache as determinism anchor across sessions" promise — PRD's main extension over the in-process FEA cache — is unverified end-to-end. Until M-011 lands there is no consumer to exercise it.

### M-019: User-facing cache documentation (env vars + CLI + distribution recipes)

- **State:** TODO
- **Failure mode:** F4
- **Evidence:** Task 2981 pending (deps 2974+2976+2977); no `docs/cache.md` exists; CLAUDE.md has no cache section.
- **Blocks:** —
- **Note:** Downstream of CLI + integration; cannot complete until upstream gaps close.

### M-020: ElasticResult bridge from solver kernel output → `PersistentlyCacheable` container

- **State:** PARTIAL
- **Failure mode:** F2
- **Evidence:** `ElasticResult` struct + `impl PersistentlyCacheable` both at `crates/reify-eval/src/persistent_cache.rs:450` and `:697`; round-trip tested. BUT no code path in `reify-solver-elastic` produces this exact container — solver internally uses different result types (`crates/reify-solver-elastic/src/result.rs`, `interpolation.rs`); bridge from solver kernel output → `ElasticResult` is unimplemented.
- **Blocks:** 2924, 2974
- **Note:** Container exists but has no producer; trait impl will not be exercised until task 2924 wires the kernel output into it.

### M-021: Genericity of `PersistentlyCacheable` for v0.4 CFD/EM/CAM kernels

- **State:** ORPHAN
- **Failure mode:** F4
- **Evidence:** PRD line 66 promises "Generic from day one — when v0.4 CFD/EM/CAM kernels arrive, they implement the same trait without touching the cache layer"; trait is technically generic (`impl Write/Read`, no FEA-specific items); but ZERO other impls exist (only `ElasticResult`); no compile-time or test check that a hypothetical second impl wouldn't break dispatch.
- **Blocks:** —
- **Note:** Future-proofing claim, not a present gap; flagged ORPHAN since the genericity assertion is unexercised by any second consumer.

## Cross-PRD breadcrumbs

- **`structural-analysis-fea.md` (v0.3 FEA PRD)** — Provides the in-memory ComputeNode cache layer that this PRD persists beneath. Gate is task 2924 (pending, plus 9 prereq tasks 3377-3385). M-011 cannot land until ComputeNode dispatch ships.
- **`compute-node-infrastructure.md`** — Owns `compute_cache_key.rs` (P3.4) and the upstream `options_hash` filtering contract (`ElasticOptions::cacheable_hash`). M-012's drift question routes back here for the question "where should engine_version_hash enter the key chain".
- **`mesh-morphing.md`** — PRD §"Relationship" calls out explicit non-coupling: morphed meshes are NOT persisted. This is a design boundary, not a gap. No mechanism to audit here.
- **`fea-gui-rendering.md`** — "Opening a saved project hits cache → first-frame stress contour appears immediately" composes with M-011 + M-018. Gate is the same.
- **`structural-analysis-shells.md`** — Transitively gated: shells PRD's results would also implement `PersistentlyCacheable` per the "generic from day one" promise (M-001), but no shells-side `impl` exists yet. Not in this PRD's scope; flagged only because the PRD claims genericity.

## Skip note

Not skipped — this is an engineering PRD with concrete on-disk + API surface.
