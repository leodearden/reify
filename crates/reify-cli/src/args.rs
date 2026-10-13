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
