//! `reify cache` subcommand dispatcher.
//!
//! Two sub-subcommands today:
//! - `cache export <hash>` — writes a single cache entry as a tar archive on stdout
//! - `cache import` — reads a tar archive from stdin into the local cache
//!
//! Sibling task 2976 (`cache stats/clear/gc`) will extend this module with
//! additional sub-subcommands; the dispatcher is structured for that.

use std::process::ExitCode;

/// Usage line printed to stderr for any `reify cache` dispatcher error.
const CACHE_USAGE: &str = "Usage: reify cache (export <hash>|import)";

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

/// Placeholder implementation of `cache export <hash>` — wired in later steps.
fn cmd_cache_export(_args: &[String]) -> ExitCode {
    eprintln!("reify cache export: not yet implemented");
    ExitCode::FAILURE
}

/// Placeholder implementation of `cache import` — wired in later steps.
fn cmd_cache_import(_args: &[String]) -> ExitCode {
    eprintln!("reify cache import: not yet implemented");
    ExitCode::FAILURE
}
