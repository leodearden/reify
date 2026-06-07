//! `reify cache` subcommand dispatcher.
//!
//! Two sub-subcommands today:
//! - `cache export <hash>` — writes a single cache entry as a tar archive on stdout
//! - `cache import` — reads a tar archive from stdin into the local cache
//!
//! Sibling task 2976 (`cache stats/clear/gc`) will extend this module with
//! additional sub-subcommands; the dispatcher is structured for that.
//!
//! ## Partial-batch import semantics
//!
//! `cache import` processes staged entries in `HashMap` (non-deterministic)
//! order and individual placement is atomic per entry (tempfile+persist).
//! Filesystem I/O failures during the second loop (tempfile create/write/
//! sync/persist, `create_dir_all`, `write_sidecar`) abort the batch with
//! `ExitCode::FAILURE` *after* any earlier entries have already landed.
//! On-disk state remains internally consistent (no half-written `.bin`s),
//! but operators cannot tell from the exit code alone which entries
//! succeeded.  This is acceptable for the v0.3 single-entry distribution
//! workflow; if multi-entry batches become common, a per-entry summary
//! and continue-on-error policy would replace the early-return pattern.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use reify_config::cache::{CacheError, CacheResolverInputs, resolve_cache};
use reify_eval::persistent_cache::{
    CacheEntryHeader, ENGINE_VERSION_HASH, ENTRY_HEADER_ENCODED_LEN, entry_bin_path,
    entry_meta_path, evict_over_cap, shard_dir, write_sidecar,
};

/// Upper bound on a single tar entry's body size (header + compressed body).
/// 256 MiB is the workstation-scale ceiling — an `ElasticResult` uncompressed
/// body caps at ~256 MiB per `persistent_cache.rs` (2 × MAX_F64_ELEMENTS × 8
/// bytes for displacement+stress) and the compressed body is bounded below
/// that.  Defends against a tar-bomb that claims a huge size in its header.
const IMPORT_ENTRY_MAX_BYTES: usize = ENTRY_HEADER_ENCODED_LEN + 256 * 1024 * 1024;

/// Upper bound on the number of tar entries we will buffer during an import.
///
/// 2977's stated distribution mode is single-entry (one `<hash>.bin` plus an
/// optional sidecar), so 1024 entries leaves multiple orders of magnitude of
/// headroom for any plausible legitimate batch while bounding the worst-case
/// memory footprint at `IMPORT_ENTRY_MAX_COUNT * IMPORT_ENTRY_MAX_BYTES`
/// (~256 GiB nominal, but reached only by an adversarial tar that simulta-
/// neously declares the maximum entry size for every entry).  Without this
/// cap a tar with many large entries could drive the staging `HashMap` to
/// OOM before any per-entry decode/validation runs.  Reaching the cap is a
/// hard FAILURE — distribution-tarballs honest about their format will never
/// trip it, and an unbounded stream is treated as malformed.
const IMPORT_ENTRY_MAX_COUNT: usize = 1024;

/// Usage line printed to stderr for any `reify cache` dispatcher error.
const CACHE_USAGE: &str = "Usage: reify cache (export <hash>|import|stats|clear|gc)";

/// Usage line for `reify cache export` argument errors.
const EXPORT_USAGE: &str = "Usage: reify cache export <hash>";

/// Usage line for `reify cache import` argument errors.
const IMPORT_USAGE: &str = "Usage: reify cache import";

/// Render a 32-byte echo field (e.g. `header.engine_version_hash`) as a
/// stderr-safe lowercase-hex string.  Used in warn-and-skip error messages
/// where the field's bytes might be operator-hostile: a maliciously crafted
/// tar entry could place terminal escapes, NULs, or other control bytes in
/// the 32-byte echo, and `String::from_utf8_lossy` only replaces invalid
/// UTF-8 — it passes valid-but-non-printable ASCII (NUL, ESC, etc.) through
/// unchanged.  Hex-encoding is unambiguous and safe to splat into any log.
fn hex_encode_32(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Staged-entry value for the import walk: `(bin_bytes, meta_bytes)`.  Either
/// may be absent depending on the tar order or whether the producer chose to
/// include the sidecar.  Factored out to satisfy `clippy::type_complexity` on
/// the `HashMap<String, _>` in `cmd_cache_import`.
type StagedEntry = (Option<Vec<u8>>, Option<Vec<u8>>);

/// True iff `s` is exactly 32 ASCII lowercase hex digits (`[0-9a-f]{32}`).
///
/// Stem-shape gate for the import tar walk.  Production cache filenames are
/// 32 lowercase hex chars by construction (the `input_hash` is the
/// xxhash3-128 hex of the cache key); any other shape on a tar entry is
/// either malformed or a path-traversal attempt, both of which we warn-and-
/// skip rather than feeding into `shard_dir` + `format!("{stem}.bin")`.
fn is_32_lowercase_hex(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Top-level `cache` subcommand dispatcher.
///
/// `args` is everything after `cache` on the command line, i.e. for
/// `reify cache export foo` we receive `["export", "foo"]`.
pub fn cmd_cache(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("export") => cmd_cache_export(&args[1..]),
        Some("import") => cmd_cache_import(&args[1..]),
        Some("stats") => cmd_cache_stats(&args[1..]),
        Some("clear") => cmd_cache_clear(&args[1..]),
        Some("gc") => cmd_cache_gc(&args[1..]),
        _ => {
            eprintln!("{CACHE_USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// `reify cache stats` — print the resolved cache directory, the entry count,
/// the total `.bin` byte footprint, the top-N largest entries, and a hit-rate
/// caveat sentence.  Aggregates across ALL engine-version subdirs (per the
/// design decision: stats is observability and must surface stale-engine
/// entries so an operator can decide when to wipe an old version by hand).
fn cmd_cache_stats(args: &[String]) -> ExitCode {
    let (cli_dir, rest) = match parse_cache_dir_flag(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("reify cache stats: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !rest.is_empty() {
        eprintln!("Usage: reify cache stats [--cache-dir <path>]");
        return ExitCode::FAILURE;
    }
    let cache_root = match resolve_cache_root_with_cli(cli_dir.as_deref()) {
        Ok((p, _)) => p,
        Err(e) => {
            eprintln!("reify cache stats: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut entries = match collect_cache_entries(&cache_root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("reify cache stats: {e}");
            return ExitCode::FAILURE;
        }
    };
    let total_bytes: u64 = entries.iter().map(|(_, sz)| *sz).sum();
    let entry_count = entries.len();

    println!("Cache directory: {}", cache_root.display());
    println!("Entry count: {entry_count}");
    // Bare-integer byte counts match `cmd_cache_gc`'s `Evicted bytes`/
    // `Remaining bytes` lines — pick one convention for machine-parsing now
    // so a future `--human` flag can do unit formatting separately, and
    // operators don't have to remember which surface adds the ` B` suffix.
    println!("Total size: {total_bytes}");

    // Top-N largest entries.  Sort descending by byte size, then take up to
    // STATS_TOP_N rows.  When the cache has fewer entries than the cap (or
    // is empty), the section header still prints — keeps the output schema
    // stable and discoverable.  Row format: `  <hash>  <bytes>`; the
    // trailing numeric token is the parseable byte size.
    entries.sort_by_key(|b| std::cmp::Reverse(b.1));
    println!("Top {STATS_TOP_N} largest entries:");
    for (hash, sz) in entries.iter().take(STATS_TOP_N) {
        println!("  {hash}  {sz}");
    }

    // Hit-rate caveat.  Per the design decision, hit-rate is not tracked
    // across processes — surface a one-sentence note rather than a stale
    // number.
    println!(
        "Note: hit rate is per-process and only reflects the current process \
         so far; cross-session aggregates are not tracked."
    );
    ExitCode::SUCCESS
}

/// Top-N cap for the `cache stats` "largest entries" section.  Pinned at 5
/// per the design decision (smallest fixed N that surfaces a useful "what's
/// eating disk?" signal without producing an unbounded report).
const STATS_TOP_N: usize = 5;

/// Usage line for `reify cache clear` argument errors.
const CLEAR_USAGE: &str =
    "Usage: reify cache clear [--cache-dir <path>] [--engine-version <hash>] --yes";

/// Usage line for `reify cache gc` argument errors.
///
/// The `(live engine version only)` qualifier mirrors the design decision that
/// `gc` operates only on the live `ENGINE_VERSION_HASH` subdir — cross-version
/// reclaim is the startup-sweep concern called out in the PRD, not an operator
/// surface here.  Surfacing the scope in the usage banner prevents the
/// post-upgrade "I ran gc and disk usage didn't drop" confusion noted in the
/// amendment-pass review.
const GC_USAGE: &str = "Usage: reify cache gc [--cache-dir <path>] (live engine version only)";

/// `reify cache gc` — force LRU eviction down to the configured cache cap.
///
/// Per the design decision, gc operates ONLY on the live `ENGINE_VERSION_HASH`
/// subdir (cross-version GC is a separate startup-sweep concern, scheduled in
/// the PRD).  No-op when the cache is already under cap; useful when the cap
/// was lowered manually via `REIFY_CACHE_MAX_BYTES` or a config file edit.
fn cmd_cache_gc(args: &[String]) -> ExitCode {
    let (cli_dir, rest) = match parse_cache_dir_flag(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("reify cache gc: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !rest.is_empty() {
        eprintln!("{GC_USAGE}");
        return ExitCode::FAILURE;
    }
    let (cache_root, max_bytes) = match resolve_cache_root_with_cli(cli_dir.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("reify cache gc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = match evict_over_cap(&cache_root, ENGINE_VERSION_HASH, max_bytes) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("reify cache gc: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Evicted entries: {}", report.evicted_count);
    println!("Evicted bytes: {}", report.evicted_bytes);
    println!("Remaining bytes: {}", report.remaining_bytes);
    ExitCode::SUCCESS
}

/// `reify cache clear` — empty the cache (or one engine-version subdir when
/// `--engine-version <hash>` is given).  Requires `--yes` consent (project
/// guidance: destructive ops must confirm) and validates the engine-version
/// hash via [`is_32_lowercase_hex`] as a defense-in-depth path-traversal
/// guard.  Filesystem mutation lands in step-10/12; this commit only wires
/// the dispatcher arm and the `--yes` refusal.
fn cmd_cache_clear(args: &[String]) -> ExitCode {
    let (cli_dir, rest) = match parse_cache_dir_flag(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("reify cache clear: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut yes = false;
    let mut engine_version: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        match a {
            "--yes" => {
                yes = true;
                i += 1;
            }
            "--engine-version" => {
                if i + 1 >= rest.len() {
                    eprintln!("reify cache clear: --engine-version requires a value");
                    eprintln!("{CLEAR_USAGE}");
                    return ExitCode::FAILURE;
                }
                engine_version = Some(rest[i + 1].clone());
                i += 2;
            }
            flag if flag.starts_with("--") => {
                eprintln!("reify cache clear: unknown flag: {flag}");
                eprintln!("{CLEAR_USAGE}");
                return ExitCode::FAILURE;
            }
            _ => {
                eprintln!("reify cache clear: unexpected positional argument: {a}");
                eprintln!("{CLEAR_USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    if !yes {
        eprintln!("reify cache clear: refusing to clear without --yes (destructive operation)");
        eprintln!("{CLEAR_USAGE}");
        return ExitCode::FAILURE;
    }
    let cache_root = match resolve_cache_root_with_cli(cli_dir.as_deref()) {
        Ok((p, _)) => p,
        Err(e) => {
            eprintln!("reify cache clear: {e}");
            return ExitCode::FAILURE;
        }
    };
    // --engine-version <hash>: scope the wipe to a single engine-version
    // subdir. Validate the hash via is_32_lowercase_hex BEFORE joining onto
    // cache_root — defense-in-depth against path-traversal payloads
    // (`../foo`, etc.) on the same surface as a hostile script.
    if let Some(hash) = engine_version {
        if !is_32_lowercase_hex(&hash) {
            eprintln!("reify cache clear: --engine-version must be 32 lowercase hex digits");
            return ExitCode::FAILURE;
        }
        let target = cache_root.join(&hash);
        // Idempotent contract: a non-directory or absent target is a no-op
        // SUCCESS.  This mirrors the bulk-clear branch's `if !path.is_dir() {
        // continue }` guard (cache.rs:348-350) which silently skips stray
        // regular files.  `Path::is_dir()` returns false for both a missing
        // path and a non-directory (regular file / symlink-to-file), so this
        // single guard covers both cases without surfacing ENOTDIR as a generic
        // I/O FAILURE.  Edge: `is_dir()` also returns false when `stat` itself
        // fails (e.g. EACCES on the parent directory, transient FS error); in
        // that case the guard silently reports SUCCESS without having removed an
        // existing directory.  This is consistent-by-design with the bulk-clear
        // branch (which uses the identical predicate at cache.rs:358) and with
        // the esc-2976-107 idempotent contract.  The `NotFound` arm below is
        // retained as TOCTOU defense for the race where the directory is removed
        // concurrently between this check and `remove_dir_all`.
        if !target.is_dir() {
            return ExitCode::SUCCESS;
        }
        match std::fs::remove_dir_all(&target) {
            Ok(()) => return ExitCode::SUCCESS,
            // No-op SUCCESS when the target subdir doesn't exist (idempotent).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!(
                    "reify cache clear: failed to remove {}: {e}",
                    target.display()
                );
                return ExitCode::FAILURE;
            }
        }
    }
    // Tolerate the root not existing — a clear of an empty cache is an
    // idempotent no-op SUCCESS.
    let read_root = match std::fs::read_dir(&cache_root) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("reify cache clear: {e}");
            return ExitCode::FAILURE;
        }
    };
    for entry in read_root {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("reify cache clear: {e}");
                return ExitCode::FAILURE;
            }
        };
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        // Defense-in-depth: only remove subdirs whose name passes the
        // 32-lowercase-hex predicate (the engine-version naming
        // invariant).  An operator who points --cache-dir at a shared
        // filesystem with adjacent unrelated state should not have
        // those entries silently nuked.
        if !is_32_lowercase_hex(name) {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        if let Err(e) = std::fs::remove_dir_all(&path) {
            eprintln!(
                "reify cache clear: failed to remove {}: {}",
                path.display(),
                e
            );
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Walk `cache_root/<engine_version_subdir>/<shard>/*.bin` across ALL
/// engine-version subdirs and return `(input_hash_stem, byte_size)` tuples.
///
/// Per the stats design decision, this aggregates across every engine-version
/// directory it finds (so operators can spot stale-engine bloat).  Shape gates:
/// * engine-version subdirs are filtered through [`is_32_lowercase_hex`] —
///   stray non-cache directories under a misconfigured `--cache-dir` are
///   silently skipped (matches the `clear` defense-in-depth predicate).
/// * `.bin` files prefixed with `.tmp.` (in-flight tempfile writes) are
///   skipped.
/// * `.meta` sidecars are skipped (`.bin` is the canonical entry).
///
/// Returns `Ok(vec![])` when `cache_root` does not exist (treat as "empty").
///
/// Concurrency tolerance: stats is an observability surface and must survive
/// concurrent mutation (a parallel `cache clear` / `gc`, or a long-lived
/// process running `evict_over_cap` in the background).  Any `NotFound` error
/// encountered mid-walk — directory snapshot taken before a concurrent
/// `remove_dir_all`, or a `.bin` evicted between `read_dir` and `metadata` —
/// is treated as "the entry disappeared, skip it" rather than a hard failure.
/// Other I/O errors still propagate.  This mirrors `evict_over_cap`'s already-
/// tolerant walk semantics.
fn collect_cache_entries(cache_root: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let mut out: Vec<(String, u64)> = Vec::new();
    let read_root = match std::fs::read_dir(cache_root) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for ev_entry in read_root {
        let ev_entry = match ev_entry {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let ev_path = ev_entry.path();
        if !ev_path.is_dir() {
            continue;
        }
        let ev_name = match ev_path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if !is_32_lowercase_hex(ev_name) {
            continue;
        }
        let shard_iter = match std::fs::read_dir(&ev_path) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for shard_entry in shard_iter {
            let shard_entry = match shard_entry {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            let shard_path = shard_entry.path();
            if !shard_path.is_dir() {
                continue;
            }
            let file_iter = match std::fs::read_dir(&shard_path) {
                Ok(it) => it,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            for file_entry in file_iter {
                let file_entry = match file_entry {
                    Ok(e) => e,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                let file_path = file_entry.path();
                if file_path.extension().and_then(|s| s.to_str()) != Some("bin") {
                    continue;
                }
                let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
                    Some(s) => s,
                    None => continue,
                };
                if file_name.starts_with(".tmp.") {
                    continue;
                }
                let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_owned(),
                    None => continue,
                };
                let size = match file_entry.metadata() {
                    Ok(m) => m.len(),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                out.push((stem, size));
            }
        }
    }
    Ok(out)
}

/// `reify cache export <hash>` — writes a single cache entry to stdout as a
/// tar archive.  Tar emission lands in step-8; this step probes for the
/// entry's existence and short-circuits on miss.
fn cmd_cache_export(args: &[String]) -> ExitCode {
    if args.len() != 1 {
        eprintln!("{EXPORT_USAGE}");
        return ExitCode::FAILURE;
    }
    let hash = &args[0];

    // Hash-shape gate: production cache filenames are 32 lowercase hex
    // chars by construction (the `input_hash` is the xxhash3-128 hex of
    // the cache key).  Without this gate a user-supplied hash flows into
    // `shard_dir` (persistent_cache.rs), whose `&input_hash[..2]` panics
    // in release builds when the input is shorter than 2 bytes or when
    // byte 2 is not a UTF-8 char boundary (e.g. `reify cache export ""`
    // or `reify cache export a` or a 2-byte multibyte char).  Reject
    // here with a usage-style error before reaching the slice.  Mirrors
    // the import-side `is_32_lowercase_hex` defense.
    if !is_32_lowercase_hex(hash) {
        eprintln!("reify cache export: hash must be 32 lowercase hex digits");
        return ExitCode::FAILURE;
    }

    let cache_root = match resolve_cache_root() {
        Ok(p) => p,
        Err(e) => {
            // Use `Display` (`{e}`) not `Debug` (`{e:?}`): `CacheError`
            // implements `fmt::Display` (reify-config/src/cache.rs) with a
            // user-facing message like "failed to parse cache config: ...";
            // the `Debug` form would print the bare enum-variant shape.
            eprintln!("reify cache export: {e}");
            return ExitCode::FAILURE;
        }
    };

    let bin_path = entry_bin_path(&cache_root, ENGINE_VERSION_HASH, hash);
    // Open the bin file ONCE up front (before tar header construction) so the
    // export sees a stable file descriptor for the whole emission.  This
    // closes the TOCTOU race that would otherwise exist between the
    // `bin_path.exists()` probe and the tar reader's open: when sibling task
    // 2976 (cache stats/clear/gc) lands and adds concurrent eviction, an
    // export interleaved with a GC sweep could otherwise observe a missing
    // file mid-emission, producing a half-written tar on stdout.  Opening
    // up front means an inflight export holds an unlinked-but-still-alive
    // inode; `tar::Builder::append_file` reads from the handle, not the
    // path.
    let mut bin_file = match std::fs::File::open(&bin_path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("reify cache export: no such cache entry: {hash}");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("reify cache export: {e}");
            return ExitCode::FAILURE;
        }
    };
    let meta_path = entry_meta_path(&cache_root, ENGINE_VERSION_HASH, hash);
    // Open the sidecar opportunistically: absence is non-fatal (the read
    // path recreates the sidecar on cache hit per persistent_cache.rs), so
    // a concurrent eviction between the bin open above and this open just
    // means we export the bin alone.  Any non-NotFound error is bubbled.
    let mut meta_file: Option<std::fs::File> = match std::fs::File::open(&meta_path) {
        Ok(f) => Some(f),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("reify cache export: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Build the tar over a stdout lock.  Tar entry names are flat
    // `<hash>.bin` / `<hash>.meta`; the sharded directory layout is
    // reconstructed on import from the bin's `CacheEntryHeader` echo fields.
    // See plan.json "Tar entry layout" design decision for rationale.
    let stdout = std::io::stdout();
    let mut builder = tar::Builder::new(stdout.lock());
    if let Err(e) = builder.append_file(format!("{hash}.bin"), &mut bin_file) {
        eprintln!("reify cache export: {e}");
        return ExitCode::FAILURE;
    }
    if let Some(ref mut mf) = meta_file
        && let Err(e) = builder.append_file(format!("{hash}.meta"), mf)
    {
        eprintln!("reify cache export: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = builder.finish() {
        eprintln!("reify cache export: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Resolve the cache root via [`reify_config::cache::resolve_cache`] using the
/// environment-variable layer plus `$HOME` / `$XDG_CACHE_HOME` defaults.
///
/// Config-file layers are deliberately not plumbed in for 2977: sibling task
/// 2976 (cache stats/clear/gc CLI) will fold those in when it lands.  Both
/// `export` and `import` use this helper so the precedence is identical.
fn resolve_cache_root() -> Result<PathBuf, CacheError> {
    resolve_cache_root_with_cli(None).map(|(dir, _)| dir)
}

/// Run the persistent-cache stale-tempfile + orphan-directory sweep at
/// CLI startup.
///
/// Called once from [`main`](crate) before the command dispatcher so every
/// engine-using subcommand (`check`, `build`, `test`, `lsp`, `mcp-server`)
/// inherits the cleanup for free without per-command wiring.
///
/// The sweep is best-effort: resolver errors (e.g. `REIFY_CACHE_MAX_BYTES`
/// parse failure) are logged at `tracing::debug!` level and the sweep is
/// skipped — matching the GUI's policy so both entry points behave identically
/// on bad env.  The returned `SweepReport` is discarded — callers get the same
/// "never fails startup" contract documented on
/// [`reify_eval::sweep_persistent_cache_at_startup`].
pub(crate) fn run_startup_sweep() {
    match resolve_cache_root() {
        Ok(cache_root) => {
            let _ = reify_eval::sweep_persistent_cache_at_startup(&cache_root);
        }
        Err(e) => {
            tracing::debug!("persistent-cache sweep skipped — resolver error: {e}");
        }
    }
}

/// Resolve both the cache root AND the max-bytes cap, honouring the optional
/// `--cache-dir` CLI override.
///
/// `cmd_cache_gc` needs the cap to call [`evict_over_cap`], and a future
/// `cmd_cache_stats` extension that surfaces "% of cap used" will need it
/// too.  Wrapping `resolve_cache` once here keeps the env-var plumbing in a
/// single place — `resolve_cache_root` becomes a thin facade for callers that
/// only need the dir.
fn resolve_cache_root_with_cli(cli_dir: Option<&Path>) -> Result<(PathBuf, u64), CacheError> {
    let env_dir = std::env::var("REIFY_CACHE_DIR").ok();
    let env_max_bytes = std::env::var("REIFY_CACHE_MAX_BYTES").ok();
    let xdg_cache_home = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").unwrap_or_default();

    let inputs = CacheResolverInputs {
        cli_dir,
        env_dir: env_dir.as_deref(),
        env_max_bytes: env_max_bytes.as_deref(),
        user_config: None,
        project_config: None,
        home: Path::new(&home),
        xdg_cache_home: xdg_cache_home.as_deref(),
    };
    resolve_cache(&inputs).map(|r| (r.dir, r.max_bytes))
}

/// Strip a `--cache-dir <path>` flag from `args` and return the parsed path
/// plus the remaining args (in original order).
///
/// Hand-rolled per-handler parsing keeps the flag schema local to each
/// sub-subcommand and matches the existing `cmd_gui` / `cmd_doc` patterns
/// in `main.rs`.  Unknown `--`-prefixed tokens are NOT rejected here — the
/// caller is responsible for its own typo gate so it can use a
/// subcommand-specific usage banner.
fn parse_cache_dir_flag(args: &[String]) -> Result<(Option<PathBuf>, Vec<String>), String> {
    let mut cache_dir: Option<PathBuf> = None;
    let mut rest: Vec<String> = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--cache-dir" {
            if i + 1 >= args.len() {
                return Err("--cache-dir requires a value".to_owned());
            }
            // Reject repeated `--cache-dir` rather than silently
            // last-write-wins.  A copy-paste mistake like
            // `reify cache stats --cache-dir /a --cache-dir /b` would
            // otherwise resolve to `/b` with no operator-visible cue;
            // refusing it surfaces the typo at parse time.
            if cache_dir.is_some() {
                return Err("--cache-dir specified more than once".to_owned());
            }
            cache_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok((cache_dir, rest))
}

/// `reify cache import` — reads a cache tarball from stdin into the local
/// cache.  Tar entries are accumulated into a `HashMap<stem, (bin, meta)>`
/// keyed on the file stem (the input hash); after the walk we decode each
/// `.bin`'s `CacheEntryHeader`, reconstruct the destination shard path from
/// the header's echo fields, and atomic-rename the `.bin` into place via
/// `tempfile::persist`.  The `.meta` body is ignored — `write_sidecar`
/// stamps a fresh single-byte payload with destination-clock mtime so the
/// LRU heuristic isn't polluted by the source machine's clock.
///
/// Engine-version-mismatch warn-and-skip lands in step-14.
fn cmd_cache_import(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("{IMPORT_USAGE}");
        return ExitCode::FAILURE;
    }

    let cache_root = match resolve_cache_root() {
        Ok(p) => p,
        Err(e) => {
            // Use `Display` (`{e}`) not `Debug` — same rationale as the
            // export site above.
            eprintln!("reify cache import: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Materialize the live `ENGINE_VERSION_HASH` as a fixed-size byte array
    // ONCE up front (it's a `const &str` so this is a constant value across
    // the import), and convert via a runtime check rather than `expect`: a
    // future refactor that changes the constant's length (or its byte width
    // via non-ASCII) should produce a controlled diagnostic, not panic
    // every `cache import` invocation.  The `engine_version_hash_const_is_32_chars`
    // test in persistent_cache.rs pins the invariant — this guard is for
    // the case where that test is removed or its invariant is broken.
    let evh_bytes: [u8; 32] = match ENGINE_VERSION_HASH.as_bytes().try_into() {
        Ok(b) => b,
        Err(_) => {
            eprintln!(
                "reify cache import: internal error: ENGINE_VERSION_HASH is not 32 bytes (len={})",
                ENGINE_VERSION_HASH.len()
            );
            return ExitCode::FAILURE;
        }
    };

    let stdin = std::io::stdin();
    let mut archive = tar::Archive::new(stdin.lock());
    let entries = match archive.entries() {
        Ok(it) => it,
        Err(e) => {
            eprintln!("reify cache import: tar archive parse error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // (stem → (bin_bytes, meta_bytes)). We tolerate either ordering of bin/meta
    // in the tar and only act on stems that have a `.bin` after the walk.
    let mut staged: HashMap<String, StagedEntry> = HashMap::new();
    for entry_result in entries {
        let mut entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("reify cache import: tar entry decode error: {e}");
                return ExitCode::FAILURE;
            }
        };

        let entry_path = match entry.path() {
            Ok(p) => p.into_owned(),
            Err(e) => {
                eprintln!("reify cache import: tar entry path error: {e}");
                return ExitCode::FAILURE;
            }
        };
        // Tar-slip defense: reject `..` or absolute paths.  Our own export
        // emits flat names, so anything else is suspect — bail rather than
        // attempt to interpret.
        if entry_path.is_absolute() || entry_path.components().count() != 1 {
            eprintln!(
                "reify cache import: rejecting tar entry with traversal-shaped path: {}",
                entry_path.display()
            );
            return ExitCode::FAILURE;
        }
        let stem = match entry_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => {
                eprintln!(
                    "reify cache import: rejecting tar entry with non-utf8 stem: {}",
                    entry_path.display()
                );
                return ExitCode::FAILURE;
            }
        };
        // Stem-shape gate (defense-in-depth against path traversal): any stem
        // that is not exactly 32 lowercase hex digits is either malformed or
        // an attempt to smuggle traversal bytes into `shard_dir` /
        // `entry_bin_path`.  Warn-and-skip; do NOT enter the staged map.
        if !is_32_lowercase_hex(&stem) {
            eprintln!(
                "reify cache import: warning: skipping tar entry with non-hex stem: {}",
                entry_path.display()
            );
            continue;
        }
        let ext = entry_path.extension().and_then(|s| s.to_str());

        let mut buf = Vec::new();
        // Use `take` to cap the body read at IMPORT_ENTRY_MAX_BYTES + 1 — if we
        // hit the +1 byte the entry exceeded the budget.
        let cap = IMPORT_ENTRY_MAX_BYTES as u64 + 1;
        if let Err(e) = entry.by_ref().take(cap).read_to_end(&mut buf) {
            eprintln!("reify cache import: tar entry body read error: {e}");
            return ExitCode::FAILURE;
        }
        if buf.len() > IMPORT_ENTRY_MAX_BYTES {
            eprintln!(
                "reify cache import: tar entry {} exceeds {IMPORT_ENTRY_MAX_BYTES} byte cap",
                entry_path.display()
            );
            return ExitCode::FAILURE;
        }

        // Cap entry-count BEFORE inserting into the staging map — a tar
        // claiming `IMPORT_ENTRY_MAX_COUNT` entries each at the body cap
        // would otherwise drive memory to OOM before any per-entry decode
        // runs.  We don't bother to be clever here: hitting the cap is a
        // hard FAILURE and we abandon the import (no partial cache writes
        // because writes only happen in the second loop after the walk
        // finishes).  Bounded by stem-uniqueness: the cap counts distinct
        // stems, not raw tar entries, so a producer that sends both `.bin`
        // and `.meta` for each of N stems counts as N (not 2N).
        if !staged.contains_key(&stem) && staged.len() >= IMPORT_ENTRY_MAX_COUNT {
            eprintln!("reify cache import: tar archive exceeds {IMPORT_ENTRY_MAX_COUNT}-entry cap");
            return ExitCode::FAILURE;
        }
        let slot = staged.entry(stem).or_insert((None, None));
        match ext {
            Some("bin") => slot.0 = Some(buf),
            Some("meta") => slot.1 = Some(buf),
            _ => {
                // Unknown extension — log and skip rather than fail.  Future
                // distribution-format additions may include sidecar files we
                // don't recognise yet; we acknowledge them by skipping.
                eprintln!(
                    "reify cache import: skipping unrecognised entry {}",
                    entry_path.display()
                );
            }
        }
    }

    for (stem, (bin_opt, _meta_opt)) in staged {
        let Some(bin_bytes) = bin_opt else {
            eprintln!("reify cache import: warning: stem {stem} has no .bin entry, skipping");
            continue;
        };

        let header = match CacheEntryHeader::read_from(&mut Cursor::new(&bin_bytes)) {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "reify cache import: warning: skipping entry {stem}: \
                     header decode failed: {e}"
                );
                continue;
            }
        };
        if let Err(e) = header.verify_format_version() {
            eprintln!(
                "reify cache import: warning: skipping entry {stem}: \
                 incompatible header format: {e}"
            );
            continue;
        }

        // Engine-version gate (PRD warn-and-skip semantics): bins whose
        // header's `engine_version_hash` doesn't match the LIVE
        // ENGINE_VERSION_HASH are version-incompatible with this binary's
        // FEA engine, so we'd be poisoning the cache by accepting them.
        // The check happens BEFORE any `fs::*` call so a mismatched entry
        // leaves zero filesystem residue (the integrity invariant called
        // out in the plan's Design Decisions).
        if &header.engine_version_hash[..] != ENGINE_VERSION_HASH.as_bytes() {
            // Hex-render the echo field rather than `from_utf8_lossy` — a
            // hostile tar could place terminal escapes / NULs in the 32-byte
            // header field that lossy UTF-8 would pass through verbatim to
            // an operator's stderr (and any log it gets piped into).
            eprintln!(
                "reify cache import: warning: skipping entry {stem}: \
                 engine-version mismatch (expected {}, got {})",
                ENGINE_VERSION_HASH,
                hex_encode_32(&header.engine_version_hash),
            );
            continue;
        }

        // `stem` is provably 32 lowercase ASCII hex from the tar-walk gate
        // above, so it converts to a [u8; 32] infallibly. We hand both
        // `stem_bytes` and the hoisted `evh_bytes` (computed once at the top
        // of `cmd_cache_import` via a runtime check, not `expect`) to
        // `verify_field_echoes` — the same helper `read_entry` uses for
        // on-disk corruption detection — to confirm the bin's internal echo
        // fields agree with the tar-layer stem and the live engine.
        // Engine-version equality was already verified above; the only
        // remaining failure mode here is a stem-vs-header input_hash mismatch
        // (corrupted or tampered echo, including path-traversal payloads in
        // `header.input_hash`).
        let stem_bytes: [u8; 32] = stem
            .as_bytes()
            .try_into()
            .expect("stem validated as 32 hex bytes by tar-walk gate");
        if let Err(e) = header.verify_field_echoes(&evh_bytes, &stem_bytes) {
            eprintln!(
                "reify cache import: warning: skipping entry {stem}: \
                 header echo does not match tar-entry stem: {e}"
            );
            continue;
        }

        // Path construction now uses the validated `stem` (NEVER
        // `header.input_hash`), so `&stem[..2]` is provably a 2-char hex
        // shard name and `format!("{stem}.bin")` is a single-segment
        // filename — no `..`, `/`, `\`, or NUL bytes can appear.
        let sd = shard_dir(&cache_root, ENGINE_VERSION_HASH, &stem);
        if let Err(e) = std::fs::create_dir_all(&sd) {
            eprintln!("reify cache import: shard dir create error: {e}");
            return ExitCode::FAILURE;
        }
        let bin_path = entry_bin_path(&cache_root, ENGINE_VERSION_HASH, &stem);

        // Atomic-rename via tempfile-in-shard — mirrors `write_entry`'s
        // pattern (persistent_cache.rs).  Skipping the post-persist directory
        // fsync is intentional (see Design Decisions in plan.json).
        let mut tmp = match tempfile::Builder::new().prefix(".tmp.").tempfile_in(&sd) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("reify cache import: tempfile create error: {e}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(e) = tmp.write_all(&bin_bytes) {
            eprintln!("reify cache import: tempfile write error: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(e) = tmp.as_file().sync_all() {
            eprintln!("reify cache import: tempfile sync error: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(persist_err) = tmp.persist(&bin_path) {
            eprintln!("reify cache import: persist error: {}", persist_err.error);
            return ExitCode::FAILURE;
        }

        // Recreate the sidecar via `write_sidecar` rather than streaming the
        // tar's `.meta` bytes verbatim — see Design Decisions: the `.meta`
        // body is just a single magic byte, and we want destination-clock
        // mtime for the LRU heuristic, not the source machine's mtime.
        let meta_path = entry_meta_path(&cache_root, ENGINE_VERSION_HASH, &stem);
        if let Err(e) = write_sidecar(&meta_path) {
            eprintln!("reify cache import: sidecar write error: {e}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
