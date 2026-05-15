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
            eprintln!(
                "reify cache import: warning: skipping entry {stem}: \
                 engine-version mismatch (expected {}, got {})",
                ENGINE_VERSION_HASH,
                String::from_utf8_lossy(&header.engine_version_hash),
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
