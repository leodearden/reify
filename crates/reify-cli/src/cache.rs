//! `reify cache` subcommand dispatcher.
//!
//! Two sub-subcommands today:
//! - `cache export <hash>` — writes a single cache entry as a tar archive on stdout
//! - `cache import` — reads a tar archive from stdin into the local cache
//!
//! Sibling task 2976 (`cache stats/clear/gc`) will extend this module with
//! additional sub-subcommands; the dispatcher is structured for that.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use reify_config::cache::{CacheError, CacheResolverInputs, resolve_cache};
use reify_eval::persistent_cache::{
    CacheEntryHeader, ENGINE_VERSION_HASH, ENTRY_HEADER_ENCODED_LEN, entry_bin_path,
    entry_meta_path, shard_dir, write_sidecar,
};

/// Upper bound on a single tar entry's body size (header + compressed body).
/// 256 MiB is the workstation-scale ceiling — an `ElasticResult` uncompressed
/// body caps at ~256 MiB per `persistent_cache.rs` (2 × MAX_F64_ELEMENTS × 8
/// bytes for displacement+stress) and the compressed body is bounded below
/// that.  Defends against a tar-bomb that claims a huge size in its header.
const IMPORT_ENTRY_MAX_BYTES: usize = ENTRY_HEADER_ENCODED_LEN + 256 * 1024 * 1024;

/// Usage line printed to stderr for any `reify cache` dispatcher error.
const CACHE_USAGE: &str = "Usage: reify cache (export <hash>|import)";

/// Usage line for `reify cache export` argument errors.
const EXPORT_USAGE: &str = "Usage: reify cache export <hash>";

/// Usage line for `reify cache import` argument errors.
const IMPORT_USAGE: &str = "Usage: reify cache import";

/// Staged-entry value for the import walk: `(bin_bytes, meta_bytes)`.  Either
/// may be absent depending on the tar order or whether the producer chose to
/// include the sidecar.  Factored out to satisfy `clippy::type_complexity` on
/// the `HashMap<String, _>` in `cmd_cache_import`.
type StagedEntry = (Option<Vec<u8>>, Option<Vec<u8>>);

/// Top-level `cache` subcommand dispatcher.
///
/// `args` is everything after `cache` on the command line, i.e. for
/// `reify cache export foo` we receive `["export", "foo"]`.
pub fn cmd_cache(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("export") => cmd_cache_export(&args[1..]),
        Some("import") => cmd_cache_import(&args[1..]),
        _ => {
            eprintln!("{CACHE_USAGE}");
            ExitCode::FAILURE
        }
    }
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

    let cache_root = match resolve_cache_root() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("reify cache export: {e:?}");
            return ExitCode::FAILURE;
        }
    };

    let bin_path = entry_bin_path(&cache_root, ENGINE_VERSION_HASH, hash);
    if !bin_path.exists() {
        eprintln!("reify cache export: no such cache entry: {hash}");
        return ExitCode::FAILURE;
    }
    let meta_path = entry_meta_path(&cache_root, ENGINE_VERSION_HASH, hash);

    // Build the tar over a stdout lock.  Tar entry names are flat
    // `<hash>.bin` / `<hash>.meta`; the sharded directory layout is
    // reconstructed on import from the bin's `CacheEntryHeader` echo fields.
    // See plan.json "Tar entry layout" design decision for rationale.
    let stdout = std::io::stdout();
    let mut builder = tar::Builder::new(stdout.lock());
    if let Err(e) = builder.append_path_with_name(&bin_path, format!("{hash}.bin")) {
        eprintln!("reify cache export: {e}");
        return ExitCode::FAILURE;
    }
    // The sidecar is recoverable per persistent_cache.rs (the read path
    // recreates it on a cache hit), so absence is non-fatal — we just
    // export what we have.
    if meta_path.exists()
        && let Err(e) = builder.append_path_with_name(&meta_path, format!("{hash}.meta"))
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
    let env_dir = std::env::var("REIFY_CACHE_DIR").ok();
    let env_max_bytes = std::env::var("REIFY_CACHE_MAX_BYTES").ok();
    let xdg_cache_home = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").unwrap_or_default();

    let inputs = CacheResolverInputs {
        cli_dir: None,
        env_dir: env_dir.as_deref(),
        env_max_bytes: env_max_bytes.as_deref(),
        user_config: None,
        project_config: None,
        home: Path::new(&home),
        xdg_cache_home: xdg_cache_home.as_deref(),
    };
    resolve_cache(&inputs).map(|r| r.dir)
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
            eprintln!("reify cache import: {e:?}");
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
            eprintln!(
                "reify cache import: warning: skipping entry {stem}: \
                 engine-version mismatch (expected {}, got {})",
                ENGINE_VERSION_HASH,
                String::from_utf8_lossy(&header.engine_version_hash),
            );
            continue;
        }

        let input_hash_str = match std::str::from_utf8(&header.input_hash) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "reify cache import: warning: skipping entry {stem}: \
                     non-utf8 input_hash echo: {e}"
                );
                continue;
            }
        };

        let sd = shard_dir(&cache_root, ENGINE_VERSION_HASH, input_hash_str);
        if let Err(e) = std::fs::create_dir_all(&sd) {
            eprintln!("reify cache import: shard dir create error: {e}");
            return ExitCode::FAILURE;
        }
        let bin_path = entry_bin_path(&cache_root, ENGINE_VERSION_HASH, input_hash_str);

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
        let meta_path = entry_meta_path(&cache_root, ENGINE_VERSION_HASH, input_hash_str);
        if let Err(e) = write_sidecar(&meta_path) {
            eprintln!("reify cache import: sidecar write error: {e}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
