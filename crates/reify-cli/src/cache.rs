//! `reify cache` subcommand dispatcher.
//!
//! Two sub-subcommands today:
//! - `cache export <hash>` — writes a single cache entry as a tar archive on stdout
//! - `cache import` — reads a tar archive from stdin into the local cache
//!
//! Sibling task 2976 (`cache stats/clear/gc`) will extend this module with
//! additional sub-subcommands; the dispatcher is structured for that.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use reify_config::cache::{CacheError, CacheResolverInputs, resolve_cache};
use reify_eval::persistent_cache::{ENGINE_VERSION_HASH, entry_bin_path, entry_meta_path};

/// Usage line printed to stderr for any `reify cache` dispatcher error.
const CACHE_USAGE: &str = "Usage: reify cache (export <hash>|import)";

/// Usage line for `reify cache export` argument errors.
const EXPORT_USAGE: &str = "Usage: reify cache export <hash>";

/// Usage line for `reify cache import` argument errors.
const IMPORT_USAGE: &str = "Usage: reify cache import";

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
/// cache.  This step drives the tar reader and surfaces parse errors; the
/// per-entry write body lands in step-12.
fn cmd_cache_import(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("{IMPORT_USAGE}");
        return ExitCode::FAILURE;
    }

    // Resolve the cache root up front even though step-10 doesn't write
    // anything yet — surfaces config errors before we start reading stdin.
    let _cache_root = match resolve_cache_root() {
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
    for entry_result in entries {
        match entry_result {
            Ok(_entry) => {
                // TODO(step-12): decode header, validate, atomic-rename.
            }
            Err(e) => {
                eprintln!("reify cache import: tar entry decode error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
