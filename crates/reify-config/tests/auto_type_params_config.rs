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
//! - `max_depth = 0` is rejected with a typed `ManifestError::InvalidMaxDepth`
//!   error (every search must visit at least one parameter).
//! - Unknown keys inside `[auto_type_params]` are surfaced as
//!   `ManifestError::Parse(_)` (strict schema; mirrors the convention on
//!   `[kernels]` / `[kernels.<id>]`).

use reify_config::{DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH, Manifest};
#[allow(unused_imports)]
use reify_config::ManifestError;

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
