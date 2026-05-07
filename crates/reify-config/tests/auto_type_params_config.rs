//! Integration tests for the `[auto_type_params]` table in `reify.toml`.
//!
//! Pairs with the `AutoTypeParamsConfig` schema added in
//! `crates/reify-config/src/lib.rs`. These tests pin the public contract for
//! task 2659 (`docs/prds/v0_2/auto-resolution-backtracking.md`):
//!
//! - The default `max_depth` is `6` when no `[auto_type_params]` section is
//!   declared.
//! - The same `6` is exposed as the public constant
//!   `reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH` (the load-bearing
//!   single-source-of-truth for the v0.2 backtracking depth bound).
//! - A custom `max_depth` value round-trips through serde.
//! - `max_depth = 0` is rejected with a typed
//!   `ManifestError::InvalidAutoTypeParamConfig` error (every search must
//!   visit at least one parameter).
//! - Unknown keys inside `[auto_type_params]` are surfaced as
//!   `ManifestError::Parse(_)` (strict schema; mirrors the convention on
//!   `[kernels]` / `[kernels.<id>]`).

use reify_config::{
    DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE, DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH, Manifest,
    ManifestError,
};

/// When a manifest has no `[auto_type_params]` section, the parsed
/// `AutoTypeParamsConfig` falls back to the PRD-decided default of 6.
///
/// Also pins that the public constant
/// `reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH` equals 6 — the
/// single-source-of-truth that the compiler crate's eventual call-site will
/// consume via `Manifest::auto_type_params().max_depth`.
#[test]
fn default_max_depth_is_six_when_section_omitted() {
    let manifest =
        Manifest::from_toml_str("").expect("empty manifest must parse to defaults");
    assert_eq!(
        manifest.auto_type_params().max_depth,
        6,
        "default max_depth must be 6 (PRD-decided v0.2 default)"
    );
    assert_eq!(
        DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH, 6,
        "DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH constant must equal 6"
    );
}

/// A `[auto_type_params]` table with `max_depth = N` overrides the default
/// of 6. Pins the serde wiring (`AutoTypeParamsRaw` is read into
/// `AutoTypeParamsConfig`) and that the parsed value flows through to the
/// public accessor.
#[test]
fn custom_max_depth_round_trips() {
    let manifest = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 8\n")
        .expect("manifest with auto_type_params section must parse");
    assert_eq!(
        manifest.auto_type_params().max_depth,
        8,
        "parsed max_depth must override the default of 6"
    );
}

/// A `[auto_type_params]` table with `max_cross_product_size = N` overrides
/// the default of 100,000. Pins the serde wiring (`AutoTypeParamsRaw` is read
/// into `AutoTypeParamsConfig`) and that the parsed value flows through to
/// the public accessor — mirrors `custom_max_depth_round_trips` for the
/// task 2662 cross-product hard cap.
#[test]
fn custom_max_cross_product_size_round_trips() {
    let manifest =
        Manifest::from_toml_str("[auto_type_params]\nmax_cross_product_size = 200000\n")
            .expect("manifest with auto_type_params section must parse");
    assert_eq!(
        manifest.auto_type_params().max_cross_product_size,
        200_000,
        "parsed max_cross_product_size must override the default of 100,000"
    );
}

/// When a manifest has no `[auto_type_params]` section, the parsed
/// `AutoTypeParamsConfig` falls back to the PRD-decided default of 100,000
/// for `max_cross_product_size`.
///
/// Also pins that the public constant
/// `reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE` equals
/// 100_000 — the single-source-of-truth that the compiler crate's eventual
/// call-site will consume via
/// `Manifest::auto_type_params().max_cross_product_size`. Mirrors
/// `default_max_depth_is_six_when_section_omitted` for the task 2662
/// cross-product hard cap.
#[test]
fn default_max_cross_product_size_is_100k_when_section_omitted() {
    let manifest =
        Manifest::from_toml_str("").expect("empty manifest must parse to defaults");
    assert_eq!(
        manifest.auto_type_params().max_cross_product_size,
        100_000,
        "default max_cross_product_size must be 100,000 (PRD-decided v0.2 default)"
    );
    assert_eq!(
        DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE, 100_000,
        "DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE constant must equal 100,000"
    );
}

/// `max_depth = 0` is semantically meaningless: every search must visit at
/// least one parameter. Pin the typed
/// `ManifestError::InvalidAutoTypeParamConfig { field: "max_depth", value: 0 }`
/// rejection at parse time so misconfiguration cannot ship as a silent
/// no-op (DFS would never run, BFS would always run for any param count).
#[test]
fn zero_max_depth_rejected_with_typed_error() {
    let err = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 0\n")
        .expect_err("max_depth = 0 must be rejected");
    match err {
        ManifestError::InvalidAutoTypeParamConfig { field, value } => {
            assert_eq!(
                field, "max_depth",
                "field must identify the offending manifest key"
            );
            assert_eq!(value, 0, "value must carry the offending value");
        }
        other => panic!(
            "expected ManifestError::InvalidAutoTypeParamConfig {{ field: \"max_depth\", value: 0 }}, got {:?}",
            other
        ),
    }
}

/// `max_cross_product_size = 0` is semantically meaningless: every search
/// must visit at least one leaf assignment. Pin the typed
/// `ManifestError::InvalidAutoTypeParamConfig { field: "max_cross_product_size", value: 0 }`
/// rejection at parse time so misconfiguration cannot ship as a silent
/// no-op (DFS would always fall back to BFS unconditionally for any
/// non-empty params slice). Mirrors `zero_max_depth_rejected_with_typed_error`
/// for the task 2662 cross-product hard cap.
#[test]
fn zero_max_cross_product_size_rejected_with_typed_error() {
    let err = Manifest::from_toml_str("[auto_type_params]\nmax_cross_product_size = 0\n")
        .expect_err("max_cross_product_size = 0 must be rejected");
    match err {
        ManifestError::InvalidAutoTypeParamConfig { field, value } => {
            assert_eq!(
                field, "max_cross_product_size",
                "field must identify the offending manifest key"
            );
            assert_eq!(value, 0, "value must carry the offending value");
        }
        other => panic!(
            "expected ManifestError::InvalidAutoTypeParamConfig {{ field: \"max_cross_product_size\", value: 0 }}, got {:?}",
            other
        ),
    }
}

/// Pin the `Display` rendering for `ManifestError::InvalidAutoTypeParamConfig`.
///
/// The rendered string must be `"auto_type_params.<field> must be > 0; got <value>"`
/// — this is the unchanged user-facing diagnostic wording, now produced by a
/// single format arm for all `[auto_type_params]` knobs rather than per-variant
/// arms. Two field labels are checked to cover both current knobs.
#[test]
fn invalid_auto_type_param_config_display_renders_field_and_value() {
    let err_depth = ManifestError::InvalidAutoTypeParamConfig {
        field: "max_depth",
        value: 0,
    };
    assert_eq!(
        format!("{}", err_depth),
        "auto_type_params.max_depth must be > 0; got 0",
        "Display for max_depth knob must render expected string"
    );

    let err_cap = ManifestError::InvalidAutoTypeParamConfig {
        field: "max_cross_product_size",
        value: 0,
    };
    assert_eq!(
        format!("{}", err_cap),
        "auto_type_params.max_cross_product_size must be > 0; got 0",
        "Display for max_cross_product_size knob must render expected string"
    );
}

/// Unknown keys inside `[auto_type_params]` are surfaced as
/// `ManifestError::Parse(_)` rather than silently dropped. Pins the
/// strict-schema convention shared with `[kernels.<id>]` (the v0.2
/// determinism load-bearer cannot tolerate silent typos like `min_depth`
/// being parsed as the default).
#[test]
fn unknown_field_in_auto_type_params_rejected() {
    let err = Manifest::from_toml_str("[auto_type_params]\nmax_depth = 6\nfoo = 1\n")
        .expect_err("unknown field in auto_type_params must be rejected");
    match err {
        ManifestError::Parse(_) => {}
        other => panic!("expected ManifestError::Parse(_), got {:?}", other),
    }
}
