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
    let raw: ConfigFileRaw = toml::from_str(s).map_err(|e| CacheError::Parse(e.to_string()))?;
    // Reject semantically nonsensical values before lifting to the public
    // type. This mirrors the `deny_unknown_fields` philosophy: loud
    // misconfiguration over silent fall-through.
    if let Some(ref c) = raw.cache {
        if c.dir.as_deref() == Some("") {
            return Err(CacheError::EmptyDir);
        }
        if c.max_bytes == Some(0) {
            return Err(CacheError::ZeroMaxBytes);
        }
    }
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
    // dir-resolution ladder, highest-precedence first. The PRD pins
    // CLI > env > user-config > project-config > default.
    let (dir, dir_source) = if let Some(cli) = inputs.cli_dir {
        (cli.to_path_buf(), CacheDirSource::CliFlag)
    } else if let Some(env) = inputs.env_dir.filter(|s| !s.is_empty()) {
        // Empty-string env vars are treated as unset (XDG / POSIX
        // convention) — fall through to the next layer rather than
        // forcing the cache to "" (CWD).
        (PathBuf::from(env), CacheDirSource::EnvVar)
    } else if let Some(user_dir) = inputs.user_config.and_then(|c| c.dir.as_ref()) {
        (user_dir.clone(), CacheDirSource::UserConfig)
    } else if let Some(project_dir) = inputs.project_config.and_then(|c| c.dir.as_ref()) {
        (project_dir.clone(), CacheDirSource::ProjectConfig)
    } else {
        (
            default_cache_dir(inputs.home, inputs.xdg_cache_home),
            CacheDirSource::Default,
        )
    };
    // max_bytes ladder, parallel to dir but minus the CLI layer (the PRD
    // does not define a CLI flag for max-bytes).
    let (max_bytes, max_bytes_source) =
        if let Some(env) = inputs.env_max_bytes.filter(|s| !s.is_empty()) {
            // Empty-string env vars are treated as unset (XDG / POSIX
            // convention, matching env_dir). On parse failure surface
            // `CacheError::InvalidMaxBytes` so callers can render the
            // offending input back to the user.
            let n: u64 = env
                .parse()
                .map_err(|_| CacheError::InvalidMaxBytes(env.to_string()))?;
            if n == 0 {
                return Err(CacheError::ZeroEnvMaxBytes);
            }
            (n, CacheMaxBytesSource::EnvVar)
        } else if let Some(user_n) = inputs.user_config.and_then(|c| c.max_bytes) {
            (user_n, CacheMaxBytesSource::UserConfig)
        } else if let Some(project_n) = inputs.project_config.and_then(|c| c.max_bytes) {
            (project_n, CacheMaxBytesSource::ProjectConfig)
        } else {
            (DEFAULT_CACHE_MAX_BYTES, CacheMaxBytesSource::Default)
        };
    Ok(CacheResolution {
        dir,
        max_bytes,
        dir_source,
        max_bytes_source,
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
    /// `REIFY_CACHE_MAX_BYTES` was set to a value that did not parse as
    /// `u64` (e.g. `"not-a-number"`, `"-5"`). The wrapped string is the
    /// offending input verbatim, so the caller can quote it back to the
    /// user.
    InvalidMaxBytes(String),
    /// `[cache].dir` is set to the empty string `""` in the config file.
    /// An empty-string path is meaningless (it resolves to CWD on most
    /// filesystems) and almost certainly a typo or misconfigured variable.
    /// Unlike `REIFY_CACHE_DIR=""` which is treated as unset (POSIX/XDG
    /// convention — empty env vars are often spuriously set in shell
    /// pipelines), an empty-string `dir` in a TOML config file is a hard
    /// error, because a TOML file entry is always intentional.
    /// Remove the key to fall through to the next layer.
    EmptyDir,
    /// `[cache].max_bytes` is set to `0` in the config file.
    /// A zero-byte cache cap is meaningless — a cache of zero bytes cannot
    /// store anything and will immediately evict every entry. This value
    /// is almost certainly a misconfiguration. Remove the key to fall
    /// through to the next layer or set it to a positive integer.
    ZeroMaxBytes,
    /// `REIFY_CACHE_MAX_BYTES` is set to `0` in the process environment.
    /// A zero-byte cache cap is meaningless — a cache of zero bytes cannot
    /// store anything and will immediately evict every entry. Unset the
    /// variable to fall through to the next layer or use a positive integer.
    ZeroEnvMaxBytes,
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Parse(msg) => write!(f, "failed to parse cache config: {}", msg),
            CacheError::Io(err) => write!(f, "failed to read cache config: {}", err),
            CacheError::InvalidMaxBytes(input) => {
                write!(f, "REIFY_CACHE_MAX_BYTES is not a valid u64: '{}'", input)
            }
            CacheError::EmptyDir => write!(
                f,
                "[cache].dir is set to the empty string; \
                 remove the key to fall through to the next layer"
            ),
            CacheError::ZeroMaxBytes => write!(
                f,
                "[cache].max_bytes is set to 0; \
                 remove the key to fall through to the next layer \
                 or use a positive integer"
            ),
            CacheError::ZeroEnvMaxBytes => write!(
                f,
                "REIFY_CACHE_MAX_BYTES is set to 0; \
                 unset the variable to fall through to the next layer \
                 or use a positive integer"
            ),
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

    /// `[cache].dir = ""` must surface as a parse error. An empty-string
    /// `dir` is meaningless (it resolves to CWD on most filesystems) and
    /// almost certainly a typo or misconfigured variable. This mirrors the
    /// env-var path that filters `is_empty()` and falls through, keeping
    /// TOML and env-var semantics in sync.
    #[test]
    fn parse_cache_config_rejects_empty_dir() {
        let err = parse_cache_config("[cache]\ndir = \"\"\n")
            .expect_err("[cache].dir = \"\" should be rejected");
        match err {
            CacheError::EmptyDir => {}
            other => panic!("expected CacheError::EmptyDir, got {:?}", other),
        }
    }

    /// `[cache].max_bytes = 0` must surface as a parse error. A zero-byte
    /// cap is meaningless (a zero-byte cache cannot store anything) and
    /// is almost certainly a misconfiguration. Remove the key to fall
    /// through to the next layer or use a positive integer.
    #[test]
    fn parse_cache_config_rejects_zero_max_bytes() {
        let err = parse_cache_config("[cache]\nmax_bytes = 0\n")
            .expect_err("[cache].max_bytes = 0 should be rejected");
        match err {
            CacheError::ZeroMaxBytes => {}
            other => panic!("expected CacheError::ZeroMaxBytes, got {:?}", other),
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

    #[test]
    fn resolve_cache_env_dir_beats_configs() {
        // env_dir set, no CLI, both config layers also set: env wins.
        let user = CacheConfig {
            dir: Some(PathBuf::from("/u")),
            max_bytes: None,
        };
        let project = CacheConfig {
            dir: Some(PathBuf::from("/p")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: Some("/env"),
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("env-beats-configs resolve");
        assert_eq!(resolved.dir, PathBuf::from("/env"));
        assert_eq!(resolved.dir_source, CacheDirSource::EnvVar);
    }

    /// Empty-string env vars are treated as unset (XDG / POSIX
    /// convention). `REIFY_CACHE_DIR=""` falls through to the next
    /// layer rather than forcing the cache to `""` (CWD).
    #[test]
    fn resolve_cache_empty_env_dir_falls_through_to_user_config() {
        let user = CacheConfig {
            dir: Some(PathBuf::from("/u")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: Some(""),
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("empty-env resolve");
        assert_eq!(resolved.dir, PathBuf::from("/u"));
        assert_eq!(resolved.dir_source, CacheDirSource::UserConfig);
    }

    #[test]
    fn resolve_cache_cli_beats_env_when_both_set() {
        // Regression-pin: CLI > env in the precedence chain.
        let inputs = CacheResolverInputs {
            cli_dir: Some(Path::new("/cli")),
            env_dir: Some("/env"),
            env_max_bytes: None,
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("cli+env resolve");
        assert_eq!(resolved.dir, PathBuf::from("/cli"));
        assert_eq!(resolved.dir_source, CacheDirSource::CliFlag);
    }

    #[test]
    fn resolve_cache_user_config_dir_used_when_only_layer_set() {
        let user = CacheConfig {
            dir: Some(PathBuf::from("/u")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("user-only resolve");
        assert_eq!(resolved.dir, PathBuf::from("/u"));
        assert_eq!(resolved.dir_source, CacheDirSource::UserConfig);
    }

    #[test]
    fn resolve_cache_project_config_dir_used_when_only_layer_set() {
        let project = CacheConfig {
            dir: Some(PathBuf::from("/p")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: None,
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("project-only resolve");
        assert_eq!(resolved.dir, PathBuf::from("/p"));
        assert_eq!(resolved.dir_source, CacheDirSource::ProjectConfig);
    }

    #[test]
    fn resolve_cache_user_config_beats_project_config() {
        let user = CacheConfig {
            dir: Some(PathBuf::from("/u")),
            max_bytes: None,
        };
        let project = CacheConfig {
            dir: Some(PathBuf::from("/p")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("user-beats-project resolve");
        assert_eq!(resolved.dir, PathBuf::from("/u"));
        assert_eq!(resolved.dir_source, CacheDirSource::UserConfig);
    }

    /// User config is `Some` but its `dir` field is `None` — must not be
    /// treated as "user config supplies a dir". Falls through to project.
    #[test]
    fn resolve_cache_user_config_with_no_dir_falls_through_to_project() {
        let user = CacheConfig {
            dir: None,
            max_bytes: Some(123),
        };
        let project = CacheConfig {
            dir: Some(PathBuf::from("/p")),
            max_bytes: None,
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("user-no-dir resolve");
        assert_eq!(resolved.dir, PathBuf::from("/p"));
        assert_eq!(resolved.dir_source, CacheDirSource::ProjectConfig);
    }

    #[test]
    fn resolve_cache_max_bytes_env_beats_configs() {
        let user = CacheConfig {
            dir: None,
            max_bytes: Some(999),
        };
        let project = CacheConfig {
            dir: None,
            max_bytes: Some(555),
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: Some("1024"),
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("env-max-bytes resolve");
        assert_eq!(resolved.max_bytes, 1024);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::EnvVar);
    }

    #[test]
    fn resolve_cache_max_bytes_user_only() {
        let user = CacheConfig {
            dir: None,
            max_bytes: Some(999),
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("user-max-bytes resolve");
        assert_eq!(resolved.max_bytes, 999);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::UserConfig);
    }

    #[test]
    fn resolve_cache_max_bytes_project_only() {
        let project = CacheConfig {
            dir: None,
            max_bytes: Some(555),
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: None,
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("project-max-bytes resolve");
        assert_eq!(resolved.max_bytes, 555);
        assert_eq!(
            resolved.max_bytes_source,
            CacheMaxBytesSource::ProjectConfig
        );
    }

    #[test]
    fn resolve_cache_max_bytes_user_beats_project() {
        let user = CacheConfig {
            dir: None,
            max_bytes: Some(999),
        };
        let project = CacheConfig {
            dir: None,
            max_bytes: Some(555),
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("user-beats-project max_bytes resolve");
        assert_eq!(resolved.max_bytes, 999);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::UserConfig);
    }

    /// `REIFY_CACHE_MAX_BYTES=""` is treated as unset and falls through
    /// (XDG / POSIX convention, mirroring REIFY_CACHE_DIR).
    #[test]
    fn resolve_cache_max_bytes_empty_env_falls_through() {
        let user = CacheConfig {
            dir: None,
            max_bytes: Some(999),
        };
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: Some(""),
            user_config: Some(&user),
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("empty-env-max-bytes resolve");
        assert_eq!(resolved.max_bytes, 999);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::UserConfig);
    }

    #[test]
    fn resolve_cache_invalid_max_bytes_not_a_number() {
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: Some("not-a-number"),
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let err = resolve_cache(&inputs).expect_err("non-numeric env should fail");
        match err {
            CacheError::InvalidMaxBytes(s) => assert_eq!(s, "not-a-number"),
            other => panic!("expected CacheError::InvalidMaxBytes, got {:?}", other),
        }
    }

    /// Negative numbers don't parse as `u64` — the offending input must
    /// surface via `InvalidMaxBytes`, not as a silent overflow / wrap.
    #[test]
    fn resolve_cache_invalid_max_bytes_negative() {
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: Some("-5"),
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let err = resolve_cache(&inputs).expect_err("negative env should fail");
        match err {
            CacheError::InvalidMaxBytes(s) => assert_eq!(s, "-5"),
            other => panic!("expected CacheError::InvalidMaxBytes, got {:?}", other),
        }
    }

    /// `REIFY_CACHE_MAX_BYTES=0` must be rejected with `ZeroEnvMaxBytes` —
    /// a zero-byte cap is meaningless regardless of which layer sets it,
    /// mirroring the parse-time rejection of `[cache].max_bytes = 0`.
    #[test]
    fn resolve_cache_env_max_bytes_zero_is_rejected() {
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: Some("0"),
            user_config: None,
            project_config: None,
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let err = resolve_cache(&inputs).expect_err("REIFY_CACHE_MAX_BYTES=0 should fail");
        match err {
            CacheError::ZeroEnvMaxBytes => {}
            other => panic!("expected CacheError::ZeroEnvMaxBytes, got {:?}", other),
        }
    }

    /// Display rendering must include both the offending input (so the
    /// user can spot the typo) and the env-var name (so the user knows
    /// where to look). Mirrors the `ManifestError::InvalidAutoTypeParamConfig`
    /// rendering style.
    #[test]
    fn invalid_max_bytes_display_mentions_input_and_variable() {
        let err = CacheError::InvalidMaxBytes("not-a-number".to_string());
        let rendered = format!("{}", err);
        assert!(
            rendered.contains("not-a-number"),
            "Display must include offending input: {}",
            rendered
        );
        assert!(
            rendered.contains("REIFY_CACHE_MAX_BYTES"),
            "Display must include the env-var name: {}",
            rendered
        );
    }

    /// Pin the rendering of `EmptyDir`: the formatted message must contain
    /// the exact substring `"[cache].dir"` so users can locate the
    /// misconfiguration. Using `"[cache].dir"` rather than bare `"dir"`
    /// avoids incidental passes on words like `"directory"` or `"redirect"`.
    /// Runtime-behavior check on diagnostic quality, mirroring
    /// `invalid_max_bytes_display_mentions_input_and_variable`.
    #[test]
    fn empty_dir_display_mentions_offending_key() {
        let err = CacheError::EmptyDir;
        let rendered = format!("{}", err);
        assert!(
            rendered.contains("[cache].dir"),
            "EmptyDir Display must contain '[cache].dir': {}",
            rendered
        );
    }

    /// Pin the rendering of `ZeroMaxBytes`: the formatted message must
    /// identify the offending `[cache].max_bytes` key so users can locate
    /// the misconfiguration. Runtime-behavior check on diagnostic quality.
    #[test]
    fn zero_max_bytes_display_mentions_offending_key() {
        let err = CacheError::ZeroMaxBytes;
        let rendered = format!("{}", err);
        assert!(
            rendered.contains("max_bytes"),
            "ZeroMaxBytes Display must mention 'max_bytes': {}",
            rendered
        );
        assert!(
            rendered.contains("[cache]"),
            "ZeroMaxBytes Display must identify it as a [cache] key: {}",
            rendered
        );
    }

    /// End-to-end shape: parse_cache_config + resolve_cache compose
    /// cleanly, user beats project, and the source-of-truth tags are
    /// available for the `reify cache stats` diagnostics use case.
    #[test]
    fn resolve_cache_round_trips_parse_cache_config_outputs() {
        let user = parse_cache_config("[cache]\ndir = \"/u\"\nmax_bytes = 100\n")
            .expect("user config parses");
        let project = parse_cache_config("[cache]\ndir = \"/p\"\nmax_bytes = 50\n")
            .expect("project config parses");
        let inputs = CacheResolverInputs {
            cli_dir: None,
            env_dir: None,
            env_max_bytes: None,
            user_config: Some(&user),
            project_config: Some(&project),
            home: Path::new("/h"),
            xdg_cache_home: None,
        };
        let resolved = resolve_cache(&inputs).expect("round-trip resolve");
        assert_eq!(resolved.dir, PathBuf::from("/u"));
        assert_eq!(resolved.max_bytes, 100);
        assert_eq!(resolved.dir_source, CacheDirSource::UserConfig);
        assert_eq!(resolved.max_bytes_source, CacheMaxBytesSource::UserConfig);
        // The source enums are exposed for diagnostics: pin that they
        // are derivable (Debug + Copy) so callers can render them
        // without re-walking the precedence chain.
        let _: CacheDirSource = resolved.dir_source;
        let _: CacheMaxBytesSource = resolved.max_bytes_source;
        let _ = format!("{:?}", resolved.dir_source);
        let _ = format!("{:?}", resolved.max_bytes_source);
    }
}
