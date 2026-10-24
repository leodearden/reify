//! Parsing of `reify` subcommand arguments into typed values, plus the usage
//! banners those parsers print when the arguments are malformed.

use std::process::ExitCode;

use reify_compiler::cfg::CfgSet;

/// One per-param binding parsed from a `--purpose` flag value.
///
/// `param` is the per-param name in the multi-pair form (`p:A`), or `None`
/// in the single-pair form (`name=entity`). `entity` is the structure ref.
#[derive(Debug, PartialEq)]
pub(crate) struct PurposeBinding {
    pub(crate) param: Option<String>,
    pub(crate) entity: String,
}

/// A single `--purpose <value>` activation: a purpose name and its bindings.
#[derive(Debug, PartialEq)]
pub(crate) struct PurposeActivation {
    pub(crate) name: String,
    pub(crate) bindings: Vec<PurposeBinding>,
}

/// Parse a `--purpose <value>` flag value.
///
/// Grammar:
/// - single-pair: `name=entity` → one binding `{ param: None, entity }`.
/// - multi-pair:  `name=p:A,q:B` → ordered bindings, each `{ param: Some(p), entity: A }`.
///
/// Errors on: missing `=`, empty name, empty binding list, empty segment
/// (e.g. trailing `,`), malformed `p:` / `:e` (empty side of `:`), or
/// multi-segment values where any segment lacks its `param:` name.
pub(crate) fn parse_purpose_flag(value: &str) -> Result<PurposeActivation, String> {
    let (name, rest) = value
        .split_once('=')
        .ok_or_else(|| format!("--purpose value '{}' is missing '='", value))?;
    if name.is_empty() {
        return Err(format!(
            "--purpose value '{}' has an empty purpose name",
            value
        ));
    }
    if rest.is_empty() {
        return Err(format!("--purpose value '{}' has no binding", value));
    }

    let mut bindings: Vec<PurposeBinding> = Vec::new();
    for segment in rest.split(',') {
        if segment.is_empty() {
            return Err(format!(
                "--purpose value '{}' has an empty binding segment",
                value
            ));
        }
        let binding = match segment.split_once(':') {
            Some((param, entity)) => {
                if param.is_empty() || entity.is_empty() {
                    return Err(format!(
                        "--purpose value '{}' has a malformed binding segment '{}'",
                        value, segment
                    ));
                }
                PurposeBinding {
                    param: Some(param.to_string()),
                    entity: entity.to_string(),
                }
            }
            None => PurposeBinding {
                param: None,
                entity: segment.to_string(),
            },
        };
        bindings.push(binding);
    }

    // Multi-binding values must use named bindings (per-param `p:E` form) so
    // each binding knows which purpose param it targets. Allowing
    // `name=A,B` would silently rely on positional order against a
    // user-declared param list, which is too brittle.
    if bindings.len() >= 2 && bindings.iter().any(|b| b.param.is_none()) {
        return Err(format!(
            "--purpose value '{}' has multiple bindings but at least one is missing its 'param:' name",
            value
        ));
    }

    Ok(PurposeActivation {
        name: name.to_string(),
        bindings,
    })
}

/// One parsed `--cfg <value>` argument.
///
/// - `Flag(name)` — a bare boolean flag (`--cfg debug`).
/// - `KeyValue { key, value }` — a `key=value` entry (`--cfg target=wasm`,
///   `--cfg feature=x`). An empty `value` is permitted (`--cfg target=`),
///   matching `CfgSet`'s kv empty-string semantics.
#[derive(Debug, PartialEq)]
enum CfgArg {
    Flag(String),
    KeyValue { key: String, value: String },
}

/// Parse a single `--cfg <value>` flag value into a [`CfgArg`].
///
/// Grammar:
/// - no `=` → bare flag; the value must be non-empty (`""` is an error).
/// - `key=value` → key/value entry; the key must be non-empty (`=v` is an
///   error). The value may be empty (`target=` yields an empty-string value).
///
/// Mirrors [`parse_purpose_flag`]'s error-message style.
fn parse_cfg_flag(value: &str) -> Result<CfgArg, String> {
    match value.split_once('=') {
        None => {
            if value.is_empty() {
                return Err("--cfg value is empty".to_string());
            }
            Ok(CfgArg::Flag(value.to_string()))
        }
        Some((key, val)) => {
            if key.is_empty() {
                return Err(format!("--cfg value '{}' has an empty key", value));
            }
            Ok(CfgArg::KeyValue {
                key: key.to_string(),
                value: val.to_string(),
            })
        }
    }
}

/// Build the active [`CfgSet`] from the repeated `--cfg <value>` arguments.
///
/// Starts from [`CfgSet::host_default`] (target = the compiling host's platform)
/// and folds each parsed [`CfgArg`] in order:
/// - `target=<v>` overrides the target;
/// - any other `key=value` is inserted into `kv`;
/// - a bare flag is inserted into `flags`.
///
/// Per PRD §4 D-2, `target` is host-defaulted and overridable ONLY by an explicit
/// `--cfg target=<v>`; bare flags and non-`target` key/values never clear it, so
/// passing a feature flag cannot silently disable platform gating.
pub(crate) fn build_cfg_set(values: &[String]) -> Result<CfgSet, String> {
    let mut cfg = CfgSet::host_default();
    for value in values {
        match parse_cfg_flag(value)? {
            CfgArg::KeyValue { key, value } if key == "target" => {
                cfg.target = Some(value);
            }
            CfgArg::KeyValue { key, value } => {
                cfg.kv.insert(key, value);
            }
            CfgArg::Flag(flag) => {
                cfg.flags.insert(flag);
            }
        }
    }
    Ok(cfg)
}

/// Usage line printed to stderr for any `reify check` usage error.
pub(crate) const CHECK_USAGE: &str = "Usage: reify check [--strict] [--purpose <name>=<binding>]... [--cfg <key=value|flag>]... <file>";

/// Usage line printed to stderr for any `reify explain` usage error.
pub(crate) const EXPLAIN_USAGE: &str = "Usage: reify explain <file>";

/// Parse a single required file-path positional, rejecting unknown `--`-prefixed flags
/// and extra positionals.  Every error prints `usage` to stderr before returning
/// `Err(ExitCode::FAILURE)` — callers can `return` the `Err` value directly.
///
/// The path is returned as an owned [`String`] so it safely outlives the argument slice.
///
/// Used by `cmd_explain` (and available for other no-flag subcommands).  Commands with
/// their own optional flags (e.g. `cmd_eval` with `--explain-undef`) extract those flags
/// first, then delegate the remainder to this helper or handle the tail themselves.
pub(crate) fn parse_single_file_arg(args: &[String], cmd: &str, usage: &str) -> Result<String, ExitCode> {
    let mut file_path: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            flag if flag.starts_with("--") => {
                eprintln!("Error: unknown flag for `{}`: {}", cmd, flag);
                eprintln!("{}", usage);
                return Err(ExitCode::FAILURE);
            }
            path => {
                if file_path.is_some() {
                    eprintln!("Error: unexpected extra positional argument: {}", path);
                    eprintln!("{}", usage);
                    return Err(ExitCode::FAILURE);
                }
                file_path = Some(path.to_string());
            }
        }
    }
    match file_path {
        Some(path) => Ok(path),
        None => {
            eprintln!("{}", usage);
            Err(ExitCode::FAILURE)
        }
    }
}

/// Usage line printed to stderr for any `reify doc` usage error.
const DOC_USAGE: &str = "Usage: reify doc <input.ri> [-o <path>] [--format html|markdown|json] [--split] [--compact]\n       reify doc --stdlib --out <dir>";

/// Report a `reify doc` usage error: the `Error:` line naming the guard, then
/// the [`DOC_USAGE`] banner that tells it apart from a genuine exit-1 failure.
pub(crate) fn doc_usage_error(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("Error: {msg}");
    eprintln!("{DOC_USAGE}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_purpose_flag_accepts_single_pair() {
        // `name=entity` is the single-binding form: one binding with no
        // per-param name and the entity as the structure ref.
        let activation =
            parse_purpose_flag("mfg_ready=Bracket").expect("single-pair form should parse");
        assert_eq!(activation.name, "mfg_ready");
        assert_eq!(activation.bindings.len(), 1);
        assert_eq!(activation.bindings[0].param, None);
        assert_eq!(activation.bindings[0].entity, "Bracket");
    }

    #[test]
    fn parse_purpose_flag_accepts_multi_pair_named_bindings() {
        // `name=p:A,q:B` is the multi-pair form: ordered, each segment carries
        // its per-param name.
        let activation = parse_purpose_flag("fits_within=part:A,envelope:B")
            .expect("multi-pair form should parse");
        assert_eq!(activation.name, "fits_within");
        assert_eq!(activation.bindings.len(), 2);
        assert_eq!(activation.bindings[0].param.as_deref(), Some("part"));
        assert_eq!(activation.bindings[0].entity, "A");
        assert_eq!(activation.bindings[1].param.as_deref(), Some("envelope"));
        assert_eq!(activation.bindings[1].entity, "B");
    }

    #[test]
    fn parse_purpose_flag_rejects_malformed_values() {
        // Missing `=` — no purpose name vs. binding-list separator.
        assert!(parse_purpose_flag("noequals").is_err());
        // Empty purpose name.
        assert!(parse_purpose_flag("=Bracket").is_err());
        // Empty binding list.
        assert!(parse_purpose_flag("mfg_ready=").is_err());
        // Trailing empty segment after a comma (`p=a,`).
        assert!(parse_purpose_flag("p=a,").is_err());
    }

    #[test]
    fn parse_cfg_flag_parses_target_key_value() {
        // `target=wasm` is the key=value form: an explicit platform override.
        assert_eq!(
            parse_cfg_flag("target=wasm"),
            Ok(CfgArg::KeyValue {
                key: "target".to_string(),
                value: "wasm".to_string(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_parses_bare_flag() {
        // A value with no `=` is a bare boolean flag.
        assert_eq!(
            parse_cfg_flag("linux"),
            Ok(CfgArg::Flag("linux".to_string())),
        );
    }

    #[test]
    fn parse_cfg_flag_parses_non_target_key_value() {
        // Any `key=value` (not just `target=`) is a key/value cfg entry.
        assert_eq!(
            parse_cfg_flag("feature=x"),
            Ok(CfgArg::KeyValue {
                key: "feature".to_string(),
                value: "x".to_string(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_allows_empty_value() {
        // `target=` is the explicit empty-value form: the key is present and the
        // value is the empty string, matching cfg.rs's kv empty-string semantics.
        assert_eq!(
            parse_cfg_flag("target="),
            Ok(CfgArg::KeyValue {
                key: "target".to_string(),
                value: String::new(),
            }),
        );
    }

    #[test]
    fn parse_cfg_flag_rejects_empty_key() {
        // `=v` has an empty key — there is no cfg name to set.
        assert!(parse_cfg_flag("=v").is_err());
    }

    #[test]
    fn parse_cfg_flag_rejects_empty_input() {
        // An empty value is neither a flag nor a `key=value` — rejected.
        assert!(parse_cfg_flag("").is_err());
    }

    #[test]
    fn build_cfg_set_empty_is_host_default() {
        // No `--cfg` args ⇒ the host-default active cfg (target = host platform,
        // empty flags/kv), identical to CfgSet::host_default (PRD §4 D-2).
        assert_eq!(
            build_cfg_set(&[]),
            Ok(reify_compiler::cfg::CfgSet::host_default()),
        );
    }

    #[test]
    fn build_cfg_set_target_override_replaces_host() {
        // `--cfg target=wasm` overrides the host-default target.
        let cfg = build_cfg_set(&["target=wasm".to_string()]).expect("valid cfg");
        assert_eq!(cfg.target.as_deref(), Some("wasm"));
    }

    #[test]
    fn build_cfg_set_flag_keeps_host_target() {
        // A bare flag must NOT clear the host-default target (D-2 robustness): a
        // feature flag should never silently disable platform gating.
        let cfg = build_cfg_set(&["feat".to_string()]).expect("valid cfg");
        assert_eq!(cfg.target.as_deref(), Some(std::env::consts::OS));
        assert!(cfg.flags.contains("feat"));
    }

    #[test]
    fn build_cfg_set_non_target_kv_keeps_host_target() {
        // A non-`target` key=value lands in `kv` and leaves the host target intact.
        let cfg = build_cfg_set(&["k=v".to_string()]).expect("valid cfg");
        assert_eq!(cfg.kv.get("k").map(String::as_str), Some("v"));
        assert_eq!(cfg.target.as_deref(), Some(std::env::consts::OS));
    }

    #[test]
    fn build_cfg_set_rejects_malformed_value() {
        // A malformed `--cfg` value (empty key) propagates parse_cfg_flag's error.
        assert!(build_cfg_set(&["=bad".to_string()]).is_err());
    }
}
