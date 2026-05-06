//! Persistent FEA cache directory + max-bytes resolver.
//!
//! Implements the resolver described in
//! `docs/prds/v0_3/persistent-fea-cache.md` "Storage location": given a
//! layered set of inputs (CLI flag, env vars, user config, project
//! config, `$HOME`, `$XDG_CACHE_HOME`), pick the cache directory and
//! the max-bytes cap and report which layer each came from.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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

/// Parsed `[cache]` section from a `~/.config/reify/config.toml` or
/// `<project>/.reify/config.toml` document.
///
/// Both fields are optional so the resolver can layer user, project, and
/// default values without confusing "not set" with "set to None". A
/// document with no `[cache]` section, an empty `[cache]` section, or
/// `[cache]` with only one of the two fields populated all parse — the
/// resolver decides what to do with the absent field.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    /// Cache directory override declared in the config file. `None` means
    /// the field was absent (or `[cache]` itself was absent). Stored as
    /// `PathBuf` for consistency with the resolver's output type.
    pub dir: Option<PathBuf>,
    /// Cache max-bytes override declared in the config file. `None` means
    /// the field was absent.
    pub max_bytes: Option<u64>,
}

/// Parse a config-file document (`~/.config/reify/config.toml` or
/// `<project>/.reify/config.toml`) into a [`CacheConfig`].
///
/// The schema is just the `[cache]` table with optional `dir` and
/// `max_bytes` keys. Both files share this schema; the resolver picks
/// which file is the user-level vs project-level override.
///
/// An empty input (or one with no `[cache]` table) parses to
/// [`CacheConfig::default()`].
pub fn parse_cache_config(s: &str) -> Result<CacheConfig, CacheError> {
    // Render `toml::de::Error` to a string instead of wrapping the type
    // directly — its `Display` impl already includes line/column context,
    // and storing the rendered form keeps the toml-crate type out of
    // `CacheError`'s public surface (matching the `ManifestError::Parse`
    // convention).
    let raw: ConfigFileRaw =
        toml::from_str(s).map_err(|e| CacheError::Parse(e.to_string()))?;
    let cache = raw.cache.unwrap_or_default();
    Ok(CacheConfig {
        dir: cache.dir.map(PathBuf::from),
        max_bytes: cache.max_bytes,
    })
}

/// Layered inputs to [`resolve_cache`].
///
/// All fields are borrowed so the resolver is a pure function with no
/// hidden environment access — callers in the binary entry points pass
/// `std::env::var(...).ok().as_deref()` for the env-var fields and the
/// already-parsed user/project [`CacheConfig`]s, keeping this library
/// deterministic and trivially unit-testable.
///
/// Precedence is fixed (highest first):
///   1. [`Self::cli_dir`]
///   2. [`Self::env_dir`]
///   3. [`Self::user_config`]'s `dir`
///   4. [`Self::project_config`]'s `dir`
///   5. Default (`$XDG_CACHE_HOME/reify/fea` or `$HOME/.cache/reify/fea`)
///
/// `max_bytes` follows the same precedence minus the CLI layer (per the
/// PRD, there is no `--cache-max-bytes` flag).
pub struct CacheResolverInputs<'a> {
    /// `--cache-dir <path>` from the CLI parser, when the eventual
    /// consumer command provides one. Highest-precedence dir layer.
    pub cli_dir: Option<&'a Path>,
    /// `REIFY_CACHE_DIR` from the process environment, already lifted
    /// to `Option<&str>` (typically via `std::env::var(...).ok().as_deref()`).
    /// Empty-string is treated as unset (XDG / POSIX convention).
    pub env_dir: Option<&'a str>,
    /// `REIFY_CACHE_MAX_BYTES` from the process environment, already
    /// lifted to `Option<&str>`. Empty-string is treated as unset.
    pub env_max_bytes: Option<&'a str>,
    /// Parsed `~/.config/reify/config.toml`, if present.
    pub user_config: Option<&'a CacheConfig>,
    /// Parsed `<project>/.reify/config.toml`, if present.
    pub project_config: Option<&'a CacheConfig>,
    /// `$HOME` — used to construct the default cache root when neither
    /// CLI / env / config layers provide a directory and `xdg_cache_home`
    /// is also unset.
    pub home: &'a Path,
    /// `$XDG_CACHE_HOME` from the process environment, already lifted
    /// to `Option<&str>`. Empty-string is treated as unset (per the
    /// XDG Base Directory spec).
    pub xdg_cache_home: Option<&'a str>,
}

/// Which input layer supplied the resolved cache directory.
///
/// Returned alongside the resolved path so callers can render
/// diagnostics (e.g. `using cache at /foo (REIFY_CACHE_DIR)` /
/// `... (default)`) without re-walking the precedence chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheDirSource {
    /// `--cache-dir` CLI flag.
    CliFlag,
    /// `REIFY_CACHE_DIR` env var.
    EnvVar,
    /// User-level config (`~/.config/reify/config.toml`).
    UserConfig,
    /// Project-level config (`<project>/.reify/config.toml`).
    ProjectConfig,
    /// Hard-coded default (`$XDG_CACHE_HOME/reify/fea` or
    /// `$HOME/.cache/reify/fea`).
    Default,
}

/// Which input layer supplied the resolved cache max-bytes cap.
///
/// No `CliFlag` variant because the PRD does not define a CLI flag for
/// max-bytes (only the env var and config-file overrides exist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMaxBytesSource {
    /// `REIFY_CACHE_MAX_BYTES` env var.
    EnvVar,
    /// User-level config (`~/.config/reify/config.toml`).
    UserConfig,
    /// Project-level config (`<project>/.reify/config.toml`).
    ProjectConfig,
    /// Hard-coded default ([`DEFAULT_CACHE_MAX_BYTES`]).
    Default,
}

/// Output of [`resolve_cache`]: the resolved cache directory and
/// max-bytes cap, with each value's source-of-truth tagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheResolution {
    /// Absolute (or caller-supplied) cache directory.
    pub dir: PathBuf,
    /// On-disk size cap (bytes).
    pub max_bytes: u64,
    /// Which input layer supplied [`Self::dir`].
    pub dir_source: CacheDirSource,
    /// Which input layer supplied [`Self::max_bytes`].
    pub max_bytes_source: CacheMaxBytesSource,
}

/// Resolve the cache directory and max-bytes cap from layered inputs.
///
/// See [`CacheResolverInputs`] for the precedence policy. The function
/// is pure and side-effect-free: it never reads the process environment
/// or the filesystem itself.
pub fn resolve_cache(inputs: &CacheResolverInputs<'_>) -> Result<CacheResolution, CacheError> {
    // Default branch only for now — later steps (12 / 14 / 16 / 18) wire
    // in the CLI / env / config layers in precedence order.
    let _ = inputs.cli_dir;
    let _ = inputs.env_dir;
    let _ = inputs.env_max_bytes;
    let _ = inputs.user_config;
    let _ = inputs.project_config;
    let dir = default_cache_dir(inputs.home, inputs.xdg_cache_home);
    Ok(CacheResolution {
        dir,
        max_bytes: DEFAULT_CACHE_MAX_BYTES,
        dir_source: CacheDirSource::Default,
        max_bytes_source: CacheMaxBytesSource::Default,
    })
}

/// Read and parse a cache config document (`~/.config/reify/config.toml`
/// or `<project>/.reify/config.toml`) from `path`.
///
/// Filesystem errors (missing file, permissions, …) surface as
/// [`CacheError::Io`]; parse-time errors surface via the same variants
/// as [`parse_cache_config`] (mirrors the
/// `Manifest::load_from_path` shape).
pub fn load_cache_config_from_path(path: &Path) -> Result<CacheConfig, CacheError> {
    let contents = std::fs::read_to_string(path).map_err(CacheError::Io)?;
    parse_cache_config(&contents)
}

/// Errors returned by cache-config parsing and loading.
#[derive(Debug)]
pub enum CacheError {
    /// The TOML document failed to parse, or an unknown section / key was
    /// rejected by the strict schema. The wrapped string is the renderer-
    /// formatted diagnostic from the underlying `toml` crate (line/column
    /// information is preserved).
    Parse(String),
    /// Reading the cache config from disk failed (e.g. missing file,
    /// permission denied). The wrapped `io::Error` is exposed via
    /// [`std::error::Error::source`] so callers can introspect it.
    /// Mirrors `ManifestError::Io`.
    Io(std::io::Error),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Parse(msg) => write!(f, "failed to parse cache config: {}", msg),
            CacheError::Io(err) => write!(f, "failed to read cache config: {}", err),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// On-disk shape for the cache config file (`~/.config/reify/config.toml`
/// or `<project>/.reify/config.toml`).
///
/// `deny_unknown_fields` is intentional: a typo at the top level (e.g.
/// `[caceh]` for `[cache]`) would otherwise parse silently to an empty
/// config and the cache override would be a no-op. Silent
/// misconfiguration is the wrong default for an override mechanism —
/// surface a parse error instead.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFileRaw {
    #[serde(default)]
    cache: Option<CacheConfigRaw>,
}

/// On-disk shape for the `[cache]` section.
///
/// `deny_unknown_fields` is intentional: a typo on a key (e.g. `dirr`
/// for `dir`) would otherwise be silently dropped and the override
/// would not apply. Mirroring the `ManifestRaw` strict-schema
/// convention.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheConfigRaw {
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    max_bytes: Option<u64>,
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

    /// Unknown keys inside `[cache]` (e.g. a typo `dirr` for `dir`) must
    /// surface as a parse error — silently accepting them would let a
    /// misconfiguration ship without warning, defeating the point of the
    /// override.
    #[test]
    fn parse_cache_config_rejects_unknown_field_in_cache_table() {
        let err = parse_cache_config("[cache]\nfoo = \"bar\"\n")
            .expect_err("unknown field in [cache] should be rejected");
        match err {
            CacheError::Parse(_) => {}
            // CacheError currently has only Parse, but match exhaustively
            // anyway so future variants force this test to be revisited.
            #[allow(unreachable_patterns)]
            other => panic!("expected CacheError::Parse(_), got {:?}", other),
        }
    }

    /// Unknown top-level sections (e.g. a typo `[caceh]` for `[cache]`)
    /// must surface as a parse error rather than parsing silently to
    /// `CacheConfig::default()`. Mirrors the `ManifestRaw` convention.
    #[test]
    fn parse_cache_config_rejects_unknown_top_level_section() {
        let err = parse_cache_config("[unknown]\nx = 1\n")
            .expect_err("unknown top-level section should be rejected");
        match err {
            CacheError::Parse(_) => {}
            #[allow(unreachable_patterns)]
            other => panic!("expected CacheError::Parse(_), got {:?}", other),
        }
    }

    #[test]
    fn resolve_cache_all_defaults() {
        // When every layer is absent, the resolver falls through to the
        // PRD-decided default cache directory (`$HOME/.cache/reify/fea`)
        // and the default max-bytes cap (25 GiB), and reports both
        // sources as `Default`.
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("all-defaults resolve");
        assert_eq!(resolved.dir, PathBuf::from("/h/.cache/reify/fea"));
        assert_eq!(resolved.max_bytes, DEFAULT_CACHE_MAX_BYTES);
        assert_eq!(resolved.dir_source, CacheDirSource::Default);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::Default);
    }

    #[test]
    fn resolve_cache_cli_dir_used_when_only_layer_set() {
        let inputs = CacheResolverInputs {
            cli_dir: Some(Path::new("/cli")),
            env_dir: None,
            env_max_bytes: None,
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("cli-only resolve");
        assert_eq!(resolved.dir, PathBuf::from("/cli"));
        assert_eq!(resolved.dir_source, CacheDirSource::CliFlag);
    }

    #[test]
    fn resolve_cache_cli_beats_all_other_layers() {
        // Pin the precedence policy: CLI flag is the highest layer and
        // wins over env vars and both config layers.
        let user = CacheConfig {
            dir: Some(PathBuf::from("/u")),
            max_bytes: None,
        };
        let project = CacheConfig {
            dir: Some(PathBuf::from("/p")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: Some(Path::new("/cli")),
            env_dir: Some("/env"),
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("cli-wins resolve");
        assert_eq!(resolved.dir, PathBuf::from("/cli"));
        assert_eq!(resolved.dir_source, CacheDirSource::CliFlag);
    }
}
