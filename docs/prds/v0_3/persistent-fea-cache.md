# PRD: Persistent Cross-Session FEA Cache

Status: stub — deferred, candidate v0.3.x. Tier-2 ergonomic improvement. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Persist ComputeNode FEA results to disk keyed by input hash, so that closing and reopening a project, or re-running a CI build, doesn't re-pay the full FEA cost. Architecture supports this; concrete wiring isn't yet scoped.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships in-process ComputeNode caching: cache hits within a single Reify process return instantly. But the cache is shed on engine restart. Practical consequences:

- A user opens a saved design — first interaction triggers full mesh + solve, even though nothing has changed since they last looked at it.
- A CI build runs FEA against the same fixture geometry every time — paying full cost on every build.
- Auto-resolve loops that completed yesterday have to re-explore the same parameter space today, even though they'd hit cache for every step.

For interactive smoothness this is a real friction. The v0.3 FEA in-process cache addresses the within-session case; this PRD extends to cross-session.

## Why deferred (and separate from FEA PRD)

- v0.3 FEA PRD scope is already substantial; pulling persistent cache in muddies the kernel-vs-storage separation.
- Concrete decisions needed on storage format, lifecycle, GC — none load-bearing for v0.3 release.
- Best designed against actual v0.3 FEA usage patterns, not speculatively.
- Touches engine startup / shutdown surface — adjacent concerns warrant their own PRD.

## Sketch of approach

Three pieces:

1. **Serialisation** — `ElasticResult` (displacement field, stress field, scalars) serialised to a compact disk format. Field data is the bulk; tetrahedral nodal data + connectivity. Likely a binary format with optional zstd compression.
2. **Storage layout** — per-project cache directory (`.reify-cache/fea/`) with content-hash-keyed entries. Cache directory `.gitignore`'d by default; opt-in shared cache via project setting.
3. **Lifecycle** — load on cache lookup; write on solve completion; GC on size cap or LRU; invalidate on engine version bump (cache key includes engine version hash so version-incompatible entries miss cleanly rather than poison).

User-visible: cache is automatic, lives in a known directory, can be cleared via `reify cache clear` or by deleting the directory. Verbose mode shows cache hit/miss stats.

## Pre-conditions for activating

- v0.3 FEA kernel shipped (concrete consumer to validate against).
- ComputeNode in-memory caching working end-to-end (this PRD just extends storage tier).
- Engine version-hashing convention established (or established as part of this PRD).

## Open design questions

- **Storage format** — sqlite? jsonb? per-result binary file? Lean: per-result binary file (simple, easy to GC, easy to share). Sqlite is overkill for blob storage; jsonb is too verbose for field data.
- **Per-project vs. shared cache** — per-project (`.reify-cache/` in project root) is the safe default; users opt into a shared system-wide cache if they want cross-project hits. Or always per-project, with shared considered later?
- **GC policy** — LRU with size cap? Time-based eviction? Manual only? Lean: LRU with default 5GB cap, configurable.
- **Concurrency** — multiple Reify processes against one cache directory: file-locking, atomic write-then-rename, or ignore (last-writer-wins)? Lean: atomic write-then-rename (simple, correct, no locking complexity).
- **Cache invalidation on engine version** — engine version hash in cache key. Version bump → all entries miss cleanly. Or migrate? Lean: just miss, let users re-populate. Simpler and safer.
- **Cache for non-FEA ComputeNodes** — should this be FEA-specific or generic? Lean: generic from day one — any ComputeNode benefits, FEA is just the first heavy consumer.
- **Cache distribution** — for CI / team workflows, can the cache be checked into git or shipped via artifact storage? Useful but probably v0.4+ feature.

## Out of scope for this PRD

- Distributed / networked cache (S3-backed, team-shared remote) — useful for teams; v0.4+ add-on.
- Cache for non-FEA outputs — out of scope for v0.3.x but the implementation should be generic enough not to preclude.
- Cache invalidation on dependency changes (e.g. material database update) — handled via input-hash composition, not a separate concern.
- Cache encryption / access control — out of scope; .gitignore'd cache is local-disk only.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-fea.md`** — adds persistence layer beneath the existing in-memory ComputeNode cache. No changes to FEA kernel; only to engine cache storage.
- **Orthogonal to `mesh-morphing.md`** — morphed meshes are NOT persisted; the persistent cache stores from-scratch results only. The morph is path-dependent (different morph-source meshes can produce different valid morph results for the same target geometry), but the persistent cache key is path-independent — caching morph results would either contaminate the cache with path-dependent state or fragment it with a "morph provenance" key dimension. The two layers compose by being orthogonal: persistent cache covers cross-session exact hits; morph covers within-session incremental updates from the most-recent in-memory mesh. Resolved 2026-05-04 alongside the mesh-morphing PRD.
- **Composes with `fea-gui-rendering.md`** — opening a saved project hits cache → first-frame stress contour appears immediately.
- **Generalises naturally to other ComputeNode kinds** — once persistent cache exists for FEA, it can extend to future CFD / EM / CAM ComputeNodes.
