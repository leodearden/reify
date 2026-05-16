# FEA Cache

**Applies to:** Reify v0.3
**Status:** Shipped — `reify cache` subcommands are live as of v0.3.
**Audience:** Users who want to understand, configure, or share the persistent FEA result cache.
**Not a PRD:** For design rationale and acceptance criteria see [`docs/prds/v0_3/persistent-fea-cache.md`](prds/v0_3/persistent-fea-cache.md).

---

## Where the cache lives

By default the FEA cache is stored at:

```
~/.cache/reify/fea/
```

This follows the [XDG Base Directory spec](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html): when `$XDG_CACHE_HOME` is set and non-empty, the cache root becomes `$XDG_CACHE_HOME/reify/fea` instead. An empty-string `$XDG_CACHE_HOME` is treated as unset and falls through to `$HOME/.cache/reify/fea`.

Cache keys are content-addressed (xxhash3-128 of the input parameters and mesh). This means the cache directory is **safe to share across projects, branches, worktrees, and CI runners** — concurrent writers will never corrupt each other, and last-writer-wins is byte-identical for deterministic inputs.

Inside the cache directory each Reify engine version gets its own subdirectory, named by the engine-version hash (a 32-char lowercase hex string). The layout is:

```
~/.cache/reify/fea/
  <engine-version-hash>/
    <shard-prefix>/
      <input-hash>.bin
      <input-hash>.meta
```

---

## Environment variables

| Variable | Effect | Default |
|---|---|---|
| `REIFY_CACHE_DIR` | Override the cache directory. Empty-string treated as unset (falls through). | `$XDG_CACHE_HOME/reify/fea` or `~/.cache/reify/fea` |
| `REIFY_CACHE_MAX_BYTES` | Override the on-disk size cap (bytes). `0` is rejected. Empty-string treated as unset. | `26843545600` (25 GiB) |

**Precedence ladder for cache directory** (highest first):

1. `--cache-dir <path>` CLI flag (`stats`/`clear`/`gc` only — `export`/`import` do not accept this flag)
2. `REIFY_CACHE_DIR` env var
3. `dir` in `~/.config/reify/config.toml` `[cache]` section (user config)
4. `dir` in `<project>/.reify/config.toml` `[cache]` section (project config)
5. Default (`$XDG_CACHE_HOME/reify/fea` or `~/.cache/reify/fea`)

**Precedence ladder for max-bytes cap** (highest first — note: no `--cache-max-bytes` CLI flag):

1. `REIFY_CACHE_MAX_BYTES` env var
2. `max_bytes` in `~/.config/reify/config.toml` `[cache]` section (user config)
3. `max_bytes` in `<project>/.reify/config.toml` `[cache]` section (project config)
4. Default (25 GiB)

**Config file schema** (either `~/.config/reify/config.toml` or `<project>/.reify/config.toml`):

```toml
[cache]
dir = "/path/to/cache"      # optional
max_bytes = 53687091200     # optional; 50 GiB example
```

Unknown keys in `[cache]` or unknown top-level sections are a hard error (typo detection).

---

## CLI

The `stats`, `clear`, and `gc` subcommands accept `--cache-dir <path>` to override the resolved cache directory for that invocation. The `export` and `import` subcommands do not accept `--cache-dir`; they resolve the cache directory from the `REIFY_CACHE_DIR` env var and config layers only. To redirect `export` or `import` to a non-default cache directory, set `REIFY_CACHE_DIR` for the invocation:

```sh
REIFY_CACHE_DIR=/mnt/fast-disk/reify-fea reify cache export a3f1e2d4c5b6a7f8e9d0c1b2a3f4e5d6 > entry.tar
REIFY_CACHE_DIR=/mnt/fast-disk/reify-fea reify cache import < entry.tar
```

### `reify cache stats [--cache-dir <path>]`

Print a summary of the current cache state. Aggregates across **all** engine-version subdirs so you can see disk usage from older engine versions too.

Example output:

```
Cache directory: /home/user/.cache/reify/fea
Entry count: 142
Total size: 1073741824
Top 5 largest entries:
  a3f1e2d4c5b6a7f8e9d0c1b2a3f4e5d6  8388608
  b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6  4194304
  ...
Note: hit rate is per-process and only reflects the current process so far; cross-session aggregates are not tracked.
```

Byte counts are bare integers (no unit suffix) for machine-parsing compatibility.

### `reify cache clear [--cache-dir <path>] [--engine-version <hash>] --yes`

Delete cache entries. The `--yes` flag is **required** (this is a destructive operation with no undo).

- Without `--engine-version`: removes all engine-version subdirectories under the cache root. Only subdirectories whose name is a 32-char lowercase hex string are touched — stray non-cache files are left alone.
- With `--engine-version <hash>`: scopes the wipe to a single engine-version subdir. The hash must be exactly 32 lowercase hex digits.

Both forms are idempotent: clearing an already-empty cache exits successfully.

### `reify cache gc [--cache-dir <path>]`

Force LRU eviction of the **live engine version's** subdir down to the configured cap (`REIFY_CACHE_MAX_BYTES` / config / default 25 GiB). Entries that were used least recently are evicted first.

This is a no-op when the cache is already under cap. It operates only on the current engine version's subdir — to reclaim disk from older engine versions, use `reify cache clear --engine-version <hash>`.

Example output:

```
Evicted entries: 12
Evicted bytes: 104857600
Remaining bytes: 1073741824
```

### `reify cache export <hash>`

Write a single cache entry to **stdout** as a tar archive. The hash must be exactly 32 lowercase hex digits (the `input_hash` from `reify cache stats`).

```sh
reify cache export a3f1e2d4c5b6a7f8e9d0c1b2a3f4e5d6 > entry.tar
```

The tar contains `<hash>.bin` (the FEA result) and, if present, `<hash>.meta` (an LRU sidecar). The meta sidecar is regenerated with local timestamps on import, so source-machine clock skew does not affect the LRU ordering on the destination.

Exit codes: `0` on success, non-zero if the hash is absent from the local cache or malformed.

### `reify cache import`

Read a cache tar archive from **stdin** and ingest the entries into the local cache.

```sh
reify cache import < entry.tar
```

Entries whose embedded engine-version hash does not match the live engine version are **warn-and-skip**: a warning is printed to stderr and the entry is not written, but the overall command exits successfully. This prevents a mismatched entry from poisoning the local cache while still allowing a mixed-version tarball to partially import.

---

## Distribution recipes

### CI: warm cache from artifact storage

Before the build, import a previously exported cache artifact:

```sh
# Warm the cache from the artifact store.
reify cache import < cache.tar

# Run the build (cache hits avoid re-running FEA).
reify build examples/assembly.ri -o /tmp/assembly.step

# Export new entries and push back to the artifact store.
reify cache export <hash> > cache.tar
# (replace <hash> with the entry you want to cache; use `reify cache stats` to list hashes)
```

Because entries are content-addressed, pushing an artifact with the same key as one already in the store is always safe — the content is byte-identical.

### Team: share via scp or rsync

Copy the local FEA cache to a teammate's machine:

```sh
# scp (entire cache)
scp -r ~/.cache/reify/fea peer:~/.cache/reify/

# rsync (incremental, safe to run repeatedly)
rsync -av ~/.cache/reify/fea/ peer:~/.cache/reify/fea/
```

Concurrent writes from multiple machines are safe — entries are content-addressed, so two machines writing the same hash produce byte-identical files. Last-writer-wins is correct by design for deterministic inputs.

### git-LFS: per-project opt-in

Individual cache entries (`.bin` files) can be committed through git-LFS for per-project storage:

```sh
git lfs track "*.bin"
git add .gitattributes
git add .cache/reify/fea/<engine-version>/<shard>/<hash>.bin
git commit -m "cache: add FEA result for <hash>"
```

**This is not recommended for most workflows.** Repository size grows quickly, there is no cost-aware eviction (entries accumulate indefinitely unless manually deleted), and entries are tied to the project repository rather than shared across all projects that use the same analysis parameters. Use scp/rsync or CI artifact storage instead.

---

## Caveats

### Local disk only

NFS and SMB-mounted cache directories are **not supported in v0.3.x**. The FEA cache relies on atomic-rename semantics (`rename(2)`) to guarantee that entries are never partially written. On many network filesystems atomic rename is either not guaranteed or not implemented correctly, which can result in corrupted cache entries. Mount the cache directory on a local filesystem.

### Engine-version bump → cache miss (expected)

Each Reify build embeds a canonical engine-version hash. When the engine version changes — due to a Reify upgrade, a development rebuild with different compilation flags, or a code change to the FEA kernel — every cached result from the old version will **miss** on lookup. The old entries remain on disk (under their own engine-version subdir) until `reify cache gc` or `reify cache clear` is run.

This is intentional, not a bug. Migrating physics-simulation results across engine versions is too easy to get subtly wrong: a result that was valid under one set of numerical parameters or element formulations may not be valid under another. The per-engine-version subdir structure enforces a hard miss rather than risking a stale result being silently accepted.

---

## Determinism implication

The FEA cache extends Reify's "cache as determinism anchor" guarantee across sessions. Once a parameter trajectory has been explored and cached, subsequent re-runs of the same design follow the same trajectory — the same constraint-solving path, the same solver decisions, the same results.

This has a practical consequence: **cold-start re-runs may differ from warm-start re-runs.** After `reify cache clear`, an engine-version bump, or a fresh checkout, the solver starts from scratch. Trajectories that were cached may be re-explored in a different order (depending on the solver's internal traversal), and — for non-deterministic or numerically sensitive designs — may produce different intermediate states. Once the cache is warm again, the trajectory stabilises.

This is load-bearing for reproducible auto-resolve runs, not just a performance optimisation. If you need a run to be reproducible, ensure the cache is warm before starting it (e.g. by importing a known-good cache artifact).

---

## References

- [`docs/prds/v0_3/persistent-fea-cache.md`](prds/v0_3/persistent-fea-cache.md) — PRD with design rationale and acceptance criteria.
- `crates/reify-config/src/cache.rs` — config resolver implementation (env vars, precedence).
- `crates/reify-cli/src/cache.rs` — CLI subcommand dispatcher.
- `crates/reify-eval/src/persistent_cache.rs` — on-disk cache entry format and LRU eviction.
