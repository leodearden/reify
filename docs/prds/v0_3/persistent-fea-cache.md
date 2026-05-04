# PRD: Persistent Cross-Session FEA Cache

Status: resolved 2026-05-04 — deferred, candidate v0.3.x. Tier-2 ergonomic improvement. Filed 2026-05-02 from FEA PRD spillover; design fully resolved 2026-05-04.

## Goal

Persist ComputeNode FEA results to disk keyed by input hash, so that closing and reopening a project, or re-running a CI build, doesn't re-pay the full FEA cost. Also extends the FEA PRD's "cache as determinism anchor" guarantee across sessions.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships in-process ComputeNode caching: cache hits within a single Reify process return instantly. But the cache is shed on engine restart. Practical consequences:

- A user opens a saved design — first interaction triggers full mesh + solve, even though nothing has changed since they last looked at it.
- A CI build runs FEA against the same fixture geometry every time — paying full cost on every build.
- Auto-resolve loops that completed yesterday have to re-explore the same parameter space today, even though they'd hit cache for every step.

For interactive smoothness this is a real friction. The v0.3 FEA in-process cache addresses the within-session case; this PRD extends to cross-session.

**Determinism implication.** The FEA PRD names the cache as the determinism anchor: within a cache lifespan, repeated calls return bit-identical bytes regardless of cold-start non-determinism in the mesher/solver. Persistent cache extends that guarantee across sessions — once a parameter trajectory has been explored, repeated runs visit the same trajectory. This is load-bearing for reproducible auto-resolve runs, not just a perf win.

## Why deferred (and separate from FEA PRD)

- v0.3 FEA PRD scope is already substantial; pulling persistent cache in muddies the kernel-vs-storage separation.
- Concrete decisions needed on storage format, lifecycle, GC — none load-bearing for v0.3 release.
- Best designed against actual v0.3 FEA usage patterns, not speculatively.
- Touches engine startup / shutdown surface — adjacent concerns warrant their own PRD.

## Resolved design

### Storage format

- **Per-result binary file**, not sqlite or jsonb. Field data is megabytes of dense f64 per entry; blob-in-sqlite is anti-pattern at that size.
- **Filesystem layout:** `<cachedir>/<engine_version_hash>/<input_hash[0:2]>/<input_hash>.bin` plus `<input_hash>.meta` sidecar. Git-style 2-level sharding from day one — cheap insurance against directory-scan slowdown at 10k+ entries.
- **Encoding:** bincode for headers, raw f64 slabs for field data, zstd compression on the body.
- **Header schema** (small, read without decompressing body): `format_version`, `engine_version_hash` echo, `solve_time_ms`, `byte_size`, `written_at`, `input_hash` echo. **Format-version is separate from engine-version-hash** — engine bumps invalidate result semantics; format bumps invalidate on-disk layout. Don't conflate.
- **Sidecar `.meta` file** holds last-access mtime (touched atomically on read; survives `noatime`/`relatime` mounts that would corrupt `atime`-based tracking).

### Storage location

- **Shared default:** `~/.cache/reify/fea/` (XDG_CACHE_HOME respected). Cache keys are content-hash by construction, so cross-project sharing is safe and amortizes warm-up across projects, branches, and CI runners.
- **Override:** `--cache-dir` CLI flag, `REIFY_CACHE_DIR` env var. Same on-disk layout regardless of root.
- **Local-disk only.** NFS/SMB shared cache directories are unsupported in v0.3.x — atomic-rename semantics are murky on network filesystems. Future work.

### GC policy

- **Cost-aware LRU eviction.** Score = `last_access_age / solve_time_ms`. Cheap-and-old evicts before expensive-and-recent. Cost-weighting is nearly free given `solve_time_ms` is in the header anyway.
- **Default cap: 25 GB.** Configurable via `REIFY_CACHE_MAX_BYTES` env var or user/project config.
- **Trigger:** opportunistic on write (check size after write, evict over cap). Plus explicit `reify cache gc` for manual sweeps. Don't GC on read — too chatty.

### Concurrency

- **Atomic write-then-rename.** Tempfile MUST live in the cache directory (`<cachedir>/.tmp.<random>`) so atomic rename works on the same filesystem. No locks.
- **Duplicate work is acceptable.** Two processes solving the same input both write; last writer wins, both produce identical bytes (deterministic solve under `#deterministic`, identical-up-to-tolerance otherwise). Not worth a lock.
- **Stale tempfile cleanup** on engine startup: `<cachedir>/.tmp.*` older than 1 hour swept.

### Cache invalidation on engine version

- **Single canonical engine-version-hash** baked at build time. Cache uses this single hash in keys; bumps invalidate cleanly via miss with no migration code.
- **Composition of the engine-version-hash is implementation-internal.** Any change capable of affecting result values must contribute (FEA solver version, mesher version, material database, stdlib FEA code, tolerance-equivalence impl, etc.) — but the cache only sees the single canonical hash. Concrete composition is punted to the implementation task.
- **Engine version bump → all entries miss.** Old entries naturally evicted by LRU. Migration of physical-simulation results is too easy to get subtly wrong.

### Generic vs FEA-specific

- **Opt-in `PersistentlyCacheable` trait.** ElasticResult implements it. Other ComputeNode outputs (e.g. progressive-solve in-flight state, anything holding OS handles) are free not to opt in.
- Trait methods: `serialize_to_writer`, `deserialize_from_reader`, `format_version`, plus access to `solve_time_ms` for cost-weighted GC.
- Generic from day one — when v0.4 CFD/EM/CAM kernels arrive, they implement the same trait without touching the cache layer.

### Cheap-result threshold

- Some ComputeNode results are cheap enough that disk write/read costs more than recompute saves.
- **Approach:** instrument cache-load + cache-write times in early integration runs against real FEA workloads; measure crossover point empirically; set the skip threshold from data. No guessed default. Threshold lives in cache config, tunable per-machine if needed.

### CLI surface

- `reify cache stats` — entry count, total size, hit rate, top-N largest entries.
- `reify cache clear` — empty entire cache. `--engine-version <hash>` clears a single engine-version subdir.
- `reify cache gc` — force LRU eviction to under cap.
- `reify cache export <hash>` — emit tarball of one entry to stdout.
- `reify cache import` — ingest tarball from stdin.

Export/import is a primitive that unblocks every distribution story (git-LFS, S3, CI artifact, scp) without committing to one. Distributed/networked cache (S3-backed, team-shared remote) remains v0.4+.

## Pre-conditions for activating

- v0.3 FEA kernel shipped (concrete consumer to validate against).
- ComputeNode in-memory caching working end-to-end (this PRD extends the storage tier under it).
- Engine-version-hash convention established as part of this PRD.

## Out of scope for this PRD

- Distributed / networked cache (S3-backed, team-shared remote) — useful for teams; v0.4+ add-on.
- NFS/SMB shared-filesystem cache directories — atomic-rename semantics unreliable; deferred.
- Cache for non-FEA outputs in v0.3.x — trait is generic but no other consumers in scope.
- Cache invalidation on dependency changes (e.g. material database update) — handled via input-hash composition, not a separate concern.
- Cache encryption / access control — out of scope; cache is local-disk.
- Migration of cache entries across engine versions — invalidate-by-miss is the policy.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-fea.md`** — adds persistence layer beneath the existing in-memory ComputeNode cache. No changes to FEA kernel; only to engine cache storage.
- **Extends "cache as determinism anchor" across sessions** — FEA PRD's bit-identical-output guarantee inside one cache lifespan now also holds across engine restarts. Reproducibility of auto-resolve trajectories survives reboots.
- **Orthogonal to `mesh-morphing.md`** — morphed meshes are NOT persisted; the persistent cache stores from-scratch results only. The morph is path-dependent (different morph-source meshes can produce different valid morph results for the same target geometry), but the persistent cache key is path-independent — caching morph results would either contaminate the cache with path-dependent state or fragment it with a "morph provenance" key dimension. The two layers compose by being orthogonal: persistent cache covers cross-session exact hits; morph covers within-session incremental updates from the most-recent in-memory mesh. Resolved 2026-05-04 alongside the mesh-morphing PRD.
- **Composes with `fea-gui-rendering.md`** — opening a saved project hits cache → first-frame stress contour appears immediately.
- **Generalises naturally to other ComputeNode kinds** — trait is opt-in; future CFD / EM / CAM ComputeNodes can implement it without touching this PRD's core.
