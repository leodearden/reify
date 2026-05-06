//! Persistent FEA cache directory + max-bytes resolver.
//!
//! Implements the resolver described in
//! `docs/prds/v0_3/persistent-fea-cache.md` "Storage location": given a
//! layered set of inputs (CLI flag, env vars, user config, project
//! config, `$HOME`, `$XDG_CACHE_HOME`), pick the cache directory and
//! the max-bytes cap and report which layer each came from.

use std::path::{Path, PathBuf};

/// Default cap on the on-disk size of the FEA cache.
///
/// Per `docs/prds/v0_3/persistent-fea-cache.md` "GC policy", the
/// default is 25 GB. We use the binary-prefix interpretation
/// (25 GiB = `25 * 1024 * 1024 * 1024` bytes), which is standard for
/// filesystem caps.
///
/// This constant is the single source of truth: downstream consumers
/// (the cache stack, the `reify cache stats` command) MUST consume it
/// here rather than embedding the literal elsewhere.
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 25 * 1024 * 1024 * 1024;

/// Sub-path under the cache root where the FEA cache lives.
///
/// Per `docs/prds/v0_3/persistent-fea-cache.md` "Storage location", the
/// shared default is `~/.cache/reify/fea/` — the `reify/fea` portion
/// is appended after the resolved cache root (`$XDG_CACHE_HOME` or
/// `$HOME/.cache`).
pub const DEFAULT_CACHE_SUBPATH: &str = "reify/fea";

/// Resolve the default cache directory from `$HOME` and `$XDG_CACHE_HOME`.
///
/// `xdg_cache_home` follows the XDG Base Directory spec: when it is
/// `Some(non-empty)`, use it directly; otherwise fall through to
/// `<home>/.cache`. Empty-string is treated as unset (matches
/// "If $XDG_CACHE_HOME is either not set or empty, a default equal to
/// $HOME/.cache should be used").
pub fn default_cache_dir(home: &Path, xdg_cache_home: Option<&str>) -> PathBuf {
    let root = match xdg_cache_home {
        Some(s) if !s.is_empty() => PathBuf::from(s),
        _ => home.join(".cache"),
    };
    root.join(DEFAULT_CACHE_SUBPATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_cache_max_bytes_is_25_gib() {
        // PRD: "Default 25 GB" — interpreted as 25 GiB (binary prefix is
        // standard for filesystem caps). Pin the literal so a stray edit
        // to the constant surfaces here.
        assert_eq!(DEFAULT_CACHE_MAX_BYTES, 25u64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn default_cache_subpath_is_reify_fea() {
        assert_eq!(DEFAULT_CACHE_SUBPATH, "reify/fea");
    }

    #[test]
    fn default_cache_dir_uses_home_dot_cache_when_xdg_unset() {
        let dir = default_cache_dir(std::path::Path::new("/h"), None);
        assert_eq!(dir, PathBuf::from("/h/.cache/reify/fea"));
    }

    #[test]
    fn default_cache_dir_uses_xdg_when_set() {
        let dir = default_cache_dir(std::path::Path::new("/h"), Some("/xdg"));
        assert_eq!(dir, PathBuf::from("/xdg/reify/fea"));
    }

    /// Per the XDG Base Directory spec, `$XDG_CACHE_HOME` empty-string is
    /// treated as unset. Falls through to `$HOME/.cache`.
    #[test]
    fn default_cache_dir_treats_empty_xdg_as_unset() {
        let dir = default_cache_dir(std::path::Path::new("/h"), Some(""));
        assert_eq!(dir, PathBuf::from("/h/.cache/reify/fea"));
    }

    #[test]
    fn parse_cache_config_empty_input_returns_default() {
        // Empty document — the `[cache]` table is absent altogether.
        let cfg = parse_cache_config("").expect("empty input should parse");
        assert_eq!(cfg, CacheConfig::default());
        assert_eq!(cfg.dir, None);
        assert_eq!(cfg.max_bytes, None);
    }

    #[test]
    fn parse_cache_config_empty_cache_table_returns_default() {
        // `[cache]` table is declared but has no fields. Should be
        // semantically equivalent to the absent-table case.
        let cfg = parse_cache_config("[cache]\n").expect("empty [cache] should parse");
        assert_eq!(cfg, CacheConfig::default());
    }

    #[test]
    fn parse_cache_config_dir_only() {
        let cfg = parse_cache_config("[cache]\ndir = \"/some/path\"\n")
            .expect("dir-only [cache] should parse");
        assert_eq!(
            cfg,
            CacheConfig {
                dir: Some(PathBuf::from("/some/path")),
                max_bytes: None,
            }
        );
    }

    #[test]
    fn parse_cache_config_max_bytes_only() {
        let cfg = parse_cache_config("[cache]\nmax_bytes = 1024\n")
            .expect("max_bytes-only [cache] should parse");
        assert_eq!(
            cfg,
            CacheConfig {
                dir: None,
                max_bytes: Some(1024),
            }
        );
    }

    #[test]
    fn parse_cache_config_both_fields_round_trip() {
        let cfg = parse_cache_config("[cache]\ndir = \"/c\"\nmax_bytes = 42\n")
            .expect("both-fields [cache] should parse");
        assert_eq!(
            cfg,
            CacheConfig {
                dir: Some(PathBuf::from("/c")),
                max_bytes: Some(42),
            }
        );
    }
}
