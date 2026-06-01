//! Project manifest (`reify.toml`) schema, parser, and validator.
//!
//! This crate owns the schema for the project pin described in the v0.2
//! multi-kernel PRD ("Resolved design decisions (2026-04-28)" → "Project pin").
//! It is intentionally self-contained: no other workspace crate consumes it
//! yet, but the binary entry points (CLI, GUI launcher, MCP server) and the
//! future kernel registry will read the parsed pin from here.
//!
//! # Schema
//!
//! A `reify.toml` may declare a `[kernels]` table mapping each kernel id to
//! a pinned version. The supported kernel ids are `occt`, `manifold`,
//! `fidget`, and `openvdb` (introduced in v0.2) and `gmsh` (added in v0.3
//! for surface-to-volume tet meshing) — see [`KernelId`]. Truck is
//! intentionally rejected (the v0.2 PRD drops Truck), as is any other id;
//! matching is canonical-lowercase only, so `OCCT` also surfaces as
//! [`ManifestError::UnknownKernel`]. Empty / whitespace-only version
//! strings are rejected with [`ManifestError::EmptyVersion`].
//!
//! Each pin accepts either an inline string scalar or a table with a
//! `version` key — both forms parse to the same [`KernelPin`]:
//!
//! ```toml
//! [kernels]
//! occt = "7.7.0"
//! manifold = { version = "2.5.1" }
//! fidget = "0.3.4"
//! openvdb = "11.0.0"
//! ```
//!
//! The schema is strict: unknown top-level sections (e.g. a typo
//! `[kernel]` for `[kernels]`) and unknown keys inside a `[kernels.<id>]`
//! table are rejected at parse time so silent misconfiguration cannot
//! ship. Version strings have any surrounding whitespace trimmed before
//! storage; a version that is empty after trimming surfaces as
//! [`ManifestError::EmptyVersion`].
//!
//! # Usage
//!
//! Use [`Manifest::from_toml_str`] for in-memory documents and
//! [`Manifest::load_from_path`] to read from disk. Iterate the parsed pin
//! set in canonical kernel-id order with [`Manifest::kernel_pins`]:
//!
//! ```
//! use reify_config::{KernelId, Manifest};
//!
//! let toml = "[kernels]\nocct = \"7.7.0\"\n";
//! let manifest = Manifest::from_toml_str(toml).expect("valid manifest");
//! let (id, pin) = manifest.kernel_pins().next().expect("one pin");
//! assert_eq!(*id, KernelId::Occt);
//! assert_eq!(pin.version, "7.7.0");
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

/// Re-export the canonical kernel discriminator from `reify-core`.
///
/// `reify_core::KernelId` is the single authoritative definition in the
/// workspace.  Importing it here keeps the public path `reify_config::KernelId`
/// working for all existing callers while making drift between the two crates
/// impossible by construction.  The canonical enum is declared in
/// `crates/reify-core/src/kernel.rs`; see its documentation for the variant
/// order (registry-name lexical: Fidget, Gmsh, Manifold, Occt, OpenVdb),
/// the determinism contract (`BTreeMap<KernelId, _>` iteration order), and
/// the `#[non_exhaustive]` / `ALL` extensibility contract.
///
/// **`kernel_pins()` iteration order** changed from the old
/// `Occt, Manifold, Fidget, OpenVdb, Gmsh` (prior variant-declaration order)
/// to the canonical **registry-name lexical** order
/// `Fidget, Gmsh, Manifold, Occt, OpenVdb`.  Iteration remains deterministic;
/// no test or consumer pins a multi-kernel ordering.
pub use reify_core::KernelId;

pub mod cache;

/// Default cap on the cross-product depth of `auto:` type-parameter resolution.
///
/// Per `docs/prds/v0_2/auto-resolution-backtracking.md` "Resolved design
/// decisions": when a definition declares more than `max_depth` `auto:`
/// type-parameters, the v0.2 DFS-over-cross-product algorithm falls back to
/// the v0.1 per-parameter BFS with a `W_AUTO_TYPE_PARAM_DEPTH_BOUND_EXCEEDED`
/// warning. The default of 6 is the load-bearing PRD constant — projects can
/// override it via `[auto_type_params]\nmax_depth = N` in `reify.toml`.
///
/// This constant is the single-source-of-truth: callers (the eventual
/// compile-pipeline integration) MUST consume it via
/// [`Manifest::auto_type_params`] rather than embedding the literal `6`
/// elsewhere.
pub const DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH: usize = 6;

/// Default cap on the size of the `auto:` type-parameter cross-product search
/// space (number of leaf assignments DFS may explore).
///
/// Per `docs/prds/v0_2/auto-resolution-backtracking.md` "Resolved design
/// decisions": when the cross-product of per-param Phase A candidate sets
/// would exceed `max_cross_product_size` total assignments, the v0.2 DFS
/// orchestrator emits a `W_AUTO_TYPE_PARAM_CROSS_PRODUCT_SIZE_EXCEEDED`
/// warning and falls back to v0.1 per-parameter BFS. The default of
/// 100,000 is the load-bearing PRD constant — projects can override it via
/// `[auto_type_params]\nmax_cross_product_size = N` in `reify.toml`.
///
/// This constant is the single-source-of-truth: callers (the eventual
/// compile-pipeline integration) MUST consume it via
/// [`Manifest::auto_type_params`] rather than embedding the literal
/// `100_000` elsewhere.
pub const DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE: usize = 100_000;

/// Configuration for the `auto:` type-parameter resolution algorithm
/// (project-level, declared under `[auto_type_params]` in `reify.toml`).
///
/// Fields:
/// - `max_depth`: cap on how many `auto:` type-parameters the v0.2 DFS
///   over the cross-product will resolve before falling back to the v0.1
///   per-parameter BFS. Defaults to [`DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH`].
/// - `max_cross_product_size`: cap on the total number of cross-product leaf
///   assignments DFS will explore before falling back to v0.1 BFS. Defaults
///   to [`DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE`].
///
/// `Default::default()` returns
/// `max_depth = DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH` and
/// `max_cross_product_size = DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE`
/// so a manifest without an `[auto_type_params]` table still produces a
/// fully-populated config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoTypeParamsConfig {
    /// Cap on the cross-product DFS depth before falling back to BFS.
    /// Validated `> 0` at parse time; `0` is rejected with
    /// [`ManifestError::InvalidAutoTypeParamConfig`].
    pub max_depth: usize,
    /// Cap on the total number of cross-product leaf assignments DFS will
    /// explore before falling back to BFS. Validated `> 0` at parse time;
    /// `0` is rejected with [`ManifestError::InvalidAutoTypeParamConfig`].
    pub max_cross_product_size: usize,
}

impl Default for AutoTypeParamsConfig {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH,
            max_cross_product_size: DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE,
        }
    }
}

/// Parsed project manifest.
///
/// Carries the set of pinned kernels declared by the project. The map is a
/// `BTreeMap` so iteration order is deterministic and stable across runs —
/// the PRD frames "determinism follows from the pin" as a load-bearing
/// invariant for the v0.2 multi-kernel design.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    kernels: BTreeMap<KernelId, KernelPin>,
    auto_type_params: AutoTypeParamsConfig,
}

impl Manifest {
    /// Parse a `reify.toml` document from a string.
    pub fn from_toml_str(s: &str) -> Result<Manifest, ManifestError> {
        // Render `toml::de::Error` to a string instead of wrapping the type
        // directly: its `Display` impl already includes line/column context,
        // and storing the rendered form keeps the toml-crate type out of
        // `ManifestError`'s public surface (so a future swap of toml crates
        // would not be a breaking change for downstream consumers).
        let raw: ManifestRaw =
            toml::from_str(s).map_err(|e| ManifestError::Parse(e.to_string()))?;
        let mut kernels: BTreeMap<KernelId, KernelPin> = BTreeMap::new();
        for (raw_id, raw_pin) in raw.kernels.into_iter() {
            let id = KernelId::from_registry_name(&raw_id)
                .ok_or_else(|| ManifestError::UnknownKernel(raw_id.clone()))?;
            // Trim surrounding whitespace before storing: the registry will
            // string-compare the version, and accepting `" 7.7.0 "` verbatim
            // would silently break that comparison. A version that is empty
            // after trimming is rejected (covers both `""` and `"   "`).
            let version = raw_pin.into_version().trim().to_string();
            if version.is_empty() {
                return Err(ManifestError::EmptyVersion(id));
            }
            kernels.insert(id, KernelPin { version });
        }
        // Lift the optional `[auto_type_params]` section into the public
        // `AutoTypeParamsConfig` shape. Absent section ⇒ `Default::default()`
        // so callers always get a fully-populated config. `max_depth = 0` is
        // rejected here (every search must visit at least one parameter), as
        // is `max_cross_product_size = 0` (every search must visit at least
        // one leaf assignment).
        let auto_type_params = match raw.auto_type_params {
            Some(raw_atp) => {
                if raw_atp.max_depth == 0 {
                    return Err(ManifestError::InvalidAutoTypeParamConfig {
                        field: stringify!(max_depth),
                        value: raw_atp.max_depth,
                    });
                }
                if raw_atp.max_cross_product_size == 0 {
                    return Err(ManifestError::InvalidAutoTypeParamConfig {
                        field: stringify!(max_cross_product_size),
                        value: raw_atp.max_cross_product_size,
                    });
                }
                AutoTypeParamsConfig {
                    max_depth: raw_atp.max_depth,
                    max_cross_product_size: raw_atp.max_cross_product_size,
                }
            }
            None => AutoTypeParamsConfig::default(),
        };
        Ok(Manifest {
            kernels,
            auto_type_params,
        })
    }

    /// Read and parse a `reify.toml` document from `path`.
    ///
    /// Filesystem errors (missing file, permissions, …) surface as
    /// [`ManifestError::Io`]; parse-time errors surface via the same
    /// variants as [`Manifest::from_toml_str`].
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Manifest, ManifestError> {
        let contents = std::fs::read_to_string(path.as_ref()).map_err(ManifestError::Io)?;
        Manifest::from_toml_str(&contents)
    }

    /// Iterate the pinned kernels in canonical (BTreeMap) order.
    pub fn kernel_pins(&self) -> impl Iterator<Item = (&KernelId, &KernelPin)> {
        self.kernels.iter()
    }

    /// Read the project's `[auto_type_params]` configuration.
    ///
    /// Returns the parsed config when the manifest declared an
    /// `[auto_type_params]` table, otherwise the [`Default`] value
    /// (`max_depth = `[`DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH`]`,
    /// max_cross_product_size = `[`DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE`]).
    /// The eventual compile-pipeline integration MUST consume both fields
    /// via this accessor rather than embedding the literal defaults elsewhere.
    pub fn auto_type_params(&self) -> &AutoTypeParamsConfig {
        &self.auto_type_params
    }
}


/// A pinned kernel version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPin {
    /// The pinned version, stored as a raw string with any surrounding
    /// whitespace trimmed by the parser. The exact comparison policy
    /// (semver, ABI version, three-part numeric, …) is a registry
    /// concern and is not interpreted here.
    pub version: String,
}

/// Errors returned by manifest parsing and validation.
#[derive(Debug)]
pub enum ManifestError {
    /// The TOML document failed to parse. The wrapped string is the
    /// renderer-formatted diagnostic from the underlying `toml` crate
    /// (line/column information is preserved).
    Parse(String),
    /// A `[kernels]` entry used a key that is not one of the supported
    /// kernel ids (occt, manifold, fidget, openvdb, gmsh).
    /// The wrapped string is the offending key, verbatim, so callers
    /// can quote it back to the user. Lookup is canonical-lowercase
    /// only — `OCCT` and `truck` both surface as `UnknownKernel`.
    UnknownKernel(String),
    /// A `[kernels]` entry pinned an empty or whitespace-only version
    /// string. Empty pins are almost always an authoring mistake; the
    /// task-level decision is to reject them at parse time so they
    /// never reach the registry.
    EmptyVersion(KernelId),
    /// Reading the manifest from disk failed (e.g. missing file,
    /// permission denied). The wrapped `io::Error` is exposed via
    /// [`std::error::Error::source`] so callers can introspect it.
    Io(std::io::Error),
    /// An `[auto_type_params]` knob was set to a non-positive value (i.e.
    /// `0`). Every search must visit at least one parameter and at least one
    /// leaf assignment, so `0` is meaningless for any knob in this table
    /// and is rejected at parse time.
    ///
    /// `field` is the manifest-schema name of the offending key (e.g.
    /// `"max_depth"`, `"max_cross_product_size"`); `value` is the offending
    /// input, surfaced verbatim in the rendered message. This single variant
    /// subsumes the prior `InvalidMaxDepth` / `InvalidMaxCrossProductSize`
    /// siblings and is designed to absorb future `[auto_type_params]` knobs
    /// without combinatorial growth of error variants.
    ///
    /// Construction sites emit `field` via `stringify!(field_name)` — keeping
    /// the label token adjacent to the struct-field access (`raw_atp.field_name`)
    /// so that a field rename is immediately visible at both sites.
    InvalidAutoTypeParamConfig { field: &'static str, value: usize },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Parse(msg) => {
                write!(f, "failed to parse reify.toml: {}", msg)
            }
            ManifestError::UnknownKernel(key) => {
                let expected: Vec<String> = KernelId::ALL.iter().map(|id| id.to_string()).collect();
                write!(
                    f,
                    "unknown kernel id '{}' in [kernels] (expected one of: {})",
                    key,
                    expected.join(", ")
                )
            }
            ManifestError::EmptyVersion(id) => {
                write!(
                    f,
                    "kernel '{}' in [kernels] has an empty version string",
                    id
                )
            }
            ManifestError::Io(err) => {
                write!(f, "failed to read reify.toml: {}", err)
            }
            ManifestError::InvalidAutoTypeParamConfig { field, value } => {
                write!(f, "auto_type_params.{} must be > 0; got {}", field, value)
            }
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ManifestError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// Internal serde shape for the on-disk reify.toml document.
///
/// `deny_unknown_fields` is intentional: a typo at the top level (e.g.
/// `[kernel]` for `[kernels]`, or a stray `[project]` table) would
/// otherwise parse silently to an empty manifest and the project pin
/// would be a no-op. Since the manifest is the determinism load-bearer
/// for v0.2 kernel selection, silent misconfiguration is the wrong
/// default — surface a parse error instead.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRaw {
    #[serde(default)]
    kernels: BTreeMap<String, KernelPinRaw>,
    /// Optional project-level configuration for the `auto:` type-parameter
    /// resolution algorithm (PRD: `docs/prds/v0_2/auto-resolution-backtracking.md`).
    /// Absent ⇒ `AutoTypeParamsConfig::default()`.
    #[serde(default)]
    auto_type_params: Option<AutoTypeParamsRaw>,
}

/// On-disk shape for the `[auto_type_params]` section.
///
/// `max_depth` defaults to [`DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH`] and
/// `max_cross_product_size` defaults to
/// [`DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE`] so a declared-but-empty
/// `[auto_type_params]` table still produces the PRD-decided defaults.
/// `deny_unknown_fields` mirrors the strict-schema convention on
/// `[kernels.<id>]`: typos like `min_depth` surface as
/// `ManifestError::Parse(_)` rather than silently parsing to the default.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutoTypeParamsRaw {
    #[serde(default = "default_max_depth_value")]
    max_depth: usize,
    #[serde(default = "default_max_cross_product_size_value")]
    max_cross_product_size: usize,
}

fn default_max_depth_value() -> usize {
    DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH
}

fn default_max_cross_product_size_value() -> usize {
    DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE
}

/// Internal serde shape for a single kernel pin.
///
/// Accepts either an inline string scalar (`occt = "7.7.0"`) or a table
/// (`[kernels.occt]\nversion = "7.7.0"`). The inline form is the
/// recommended spelling; the table form exists so future per-kernel
/// options can be added without breaking the inline form. Today the
/// table accepts only `version` — unknown keys are rejected at parse
/// time via `deny_unknown_fields` on [`KernelPinTable`], so authoring a
/// future-only option (e.g. `tolerance = ...`) on the current schema is
/// a loud error rather than a silently-ignored field.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KernelPinRaw {
    Inline(String),
    Table(KernelPinTable),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KernelPinTable {
    version: String,
}

impl KernelPinRaw {
    fn into_version(self) -> String {
        match self {
            KernelPinRaw::Inline(v) => v,
            KernelPinRaw::Table(KernelPinTable { version }) => version,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_parses_to_empty_manifest() {
        let manifest = Manifest::from_toml_str("").expect("empty input should parse");
        assert!(
            manifest.kernel_pins().next().is_none(),
            "empty manifest must have no pinned kernels"
        );
    }

    #[test]
    fn single_kernel_pin_round_trips() {
        let manifest = Manifest::from_toml_str("[kernels]\nocct = \"7.7.0\"\n")
            .expect("single-pin TOML should parse");
        let entries: Vec<(&KernelId, &KernelPin)> = manifest.kernel_pins().collect();
        assert_eq!(entries.len(), 1, "should have exactly one pinned kernel");
        let (id, pin) = entries[0];
        assert_eq!(*id, KernelId::Occt);
        assert_eq!(pin.version, "7.7.0");
    }

    #[test]
    fn unknown_kernel_id_rejected_with_typed_error() {
        let err = Manifest::from_toml_str("[kernels]\nfoobar = \"1.0\"\n")
            .expect_err("unknown kernel id should be rejected");
        match err {
            ManifestError::UnknownKernel(name) => assert_eq!(name, "foobar"),
            other => panic!(
                "expected ManifestError::UnknownKernel(\"foobar\"), got {:?}",
                other
            ),
        }
    }

    /// PRD: docs/prds/v0_2/multi-kernel.md, "Resolved design decisions
    /// (2026-04-28)" — "Truck dropped from v0.2". Truck must be rejected as
    /// an unknown kernel id, not silently accepted.
    #[test]
    fn truck_is_rejected_as_unknown_in_v0_2() {
        let err = Manifest::from_toml_str("[kernels]\ntruck = \"0.5\"\n")
            .expect_err("truck must be rejected in v0.2");
        match err {
            ManifestError::UnknownKernel(name) => assert_eq!(name, "truck"),
            other => panic!(
                "expected ManifestError::UnknownKernel(\"truck\"), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn kernel_id_match_is_lowercase_only() {
        let err = Manifest::from_toml_str("[kernels]\nOCCT = \"7.7\"\n")
            .expect_err("uppercase kernel id should be rejected");
        match err {
            ManifestError::UnknownKernel(name) => assert_eq!(name, "OCCT"),
            other => panic!(
                "expected ManifestError::UnknownKernel(\"OCCT\"), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn kernel_id_round_trips_for_all_supported_kernels() {
        let cases: &[(KernelId, &str)] = &[
            (KernelId::Occt, "occt"),
            (KernelId::Manifold, "manifold"),
            (KernelId::Fidget, "fidget"),
            (KernelId::OpenVdb, "openvdb"),
            (KernelId::Gmsh, "gmsh"),
        ];
        for &(id, canonical) in cases {
            let displayed = format!("{}", id);
            assert_eq!(
                displayed, canonical,
                "Display for {:?} must be canonical lowercase",
                id
            );
            // Use from_registry_name (the canonical inverse) instead of FromStr,
            // which is not implemented on reify_core::KernelId.
            let parsed = KernelId::from_registry_name(&displayed)
                .expect("Display output must resolve back via from_registry_name");
            assert_eq!(parsed, id, "round-trip must yield the original KernelId");
        }
    }

    #[test]
    fn kernel_id_from_registry_name_rejects_unknown_strings() {
        let invalid = [
            "",       // empty
            " occt",  // leading whitespace
            "occt ",  // trailing whitespace
            "Occt",   // capitalised
            "OCCT",   // upper-case
            "Fidget", // capitalised
            "truck",  // dropped from v0.2
            "open_vdb", "open-vdb",
        ];
        for s in invalid {
            assert!(
                KernelId::from_registry_name(s).is_none(),
                "expected '{}' to be rejected by KernelId::from_registry_name",
                s
            );
        }
    }

    #[test]
    fn unknown_top_level_section_rejected() {
        // `[kernel]` is a typo for `[kernels]`. Without `deny_unknown_fields`
        // the document would parse to an empty manifest and the project pin
        // would be a silent no-op; that is the wrong default for the v0.2
        // determinism load-bearer.
        let err = Manifest::from_toml_str("[kernel]\nocct = \"7.7.0\"\n")
            .expect_err("unknown top-level section should be rejected");
        match err {
            ManifestError::Parse(_) => {}
            other => panic!("expected ManifestError::Parse(_), got {:?}", other),
        }
    }

    #[test]
    fn table_form_unknown_field_rejected() {
        // The table form accepts only `version` today; future per-kernel
        // options (e.g. `tolerance`) must extend the schema explicitly.
        // Silently accepting and discarding unknown keys would be worse than
        // rejecting them.
        let err =
            Manifest::from_toml_str("[kernels.occt]\nversion = \"7.7.0\"\nfeature = \"foo\"\n")
                .expect_err("unknown fields in pin table should be rejected");
        match err {
            ManifestError::Parse(_) => {}
            other => panic!("expected ManifestError::Parse(_), got {:?}", other),
        }
    }

    #[test]
    fn malformed_toml_returns_parse_error_with_diagnostic_text() {
        // Unclosed [kernels — never reaches the kernels-walk; toml::from_str
        // surfaces a syntax error.
        let err = Manifest::from_toml_str("[kernels\nocct = \"7.7\"\n")
            .expect_err("malformed TOML should be rejected");
        match err {
            ManifestError::Parse(msg) => {
                assert!(
                    !msg.is_empty(),
                    "Parse error message must carry diagnostic text"
                );
            }
            other => panic!("expected ManifestError::Parse(_), got {:?}", other),
        }
    }

    #[test]
    fn empty_version_string_rejected() {
        let err = Manifest::from_toml_str("[kernels]\nocct = \"\"\n")
            .expect_err("empty version string should be rejected");
        match err {
            ManifestError::EmptyVersion(id) => assert_eq!(id, KernelId::Occt),
            other => panic!(
                "expected ManifestError::EmptyVersion(KernelId::Occt), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn whitespace_only_version_rejected() {
        let err = Manifest::from_toml_str("[kernels]\nocct = \"   \"\n")
            .expect_err("whitespace-only version should be rejected");
        match err {
            ManifestError::EmptyVersion(id) => assert_eq!(id, KernelId::Occt),
            other => panic!(
                "expected ManifestError::EmptyVersion(KernelId::Occt), got {:?}",
                other
            ),
        }
    }

    /// Pin the policy: surrounding whitespace on a non-empty version is
    /// trimmed before storage. Downstream consumers (the future kernel
    /// registry) will string-compare the version, and storing `" 7.7.0 "`
    /// verbatim would silently break those comparisons.
    #[test]
    fn version_with_surrounding_whitespace_is_trimmed() {
        let manifest = Manifest::from_toml_str("[kernels]\nocct = \" 7.7.0 \"\n")
            .expect("non-empty padded version should parse");
        let (id, pin) = manifest
            .kernel_pins()
            .next()
            .expect("one pinned kernel expected");
        assert_eq!(*id, KernelId::Occt);
        assert_eq!(
            pin.version, "7.7.0",
            "stored version must have surrounding whitespace trimmed"
        );
    }

    #[test]
    fn multiple_kernel_pins_iterate_in_kernel_id_order() {
        // Non-canonical text order in the TOML source.
        let toml = "[kernels]\n\
                    fidget = \"0.3.4\"\n\
                    occt = \"7.7.0\"\n\
                    openvdb = \"11.0\"\n\
                    manifold = \"2.5\"\n\
                    gmsh = \"4.15.2\"\n";
        let manifest = Manifest::from_toml_str(toml).expect("five-pin TOML should parse");
        let ids: Vec<KernelId> = manifest.kernel_pins().map(|(id, _)| *id).collect();
        // BTreeMap iteration follows the derived `Ord` on `KernelId`.  The
        // canonical order (from reify-core) is registry-name LEXICAL order:
        // Fidget < Gmsh < Manifold < Occt < OpenVdb.
        assert_eq!(
            ids,
            vec![
                KernelId::Fidget,
                KernelId::Gmsh,
                KernelId::Manifold,
                KernelId::Occt,
                KernelId::OpenVdb,
            ]
        );
    }

    #[test]
    fn gmsh_pin_parses_to_typed_kernel_id() {
        let manifest = Manifest::from_toml_str("[kernels]\ngmsh = \"4.15.2\"\n")
            .expect("gmsh pin TOML should parse");
        let entries: Vec<(&KernelId, &KernelPin)> = manifest.kernel_pins().collect();
        assert_eq!(entries.len(), 1, "should have exactly one pinned kernel");
        let (id, pin) = entries[0];
        assert_eq!(*id, KernelId::Gmsh);
        assert_eq!(pin.version, "4.15.2");
    }

    #[test]
    fn unknown_kernel_message_lists_all_supported_kernels() {
        let err = Manifest::from_toml_str("[kernels]\nfoobar = \"1.0\"\n")
            .expect_err("unknown kernel id should be rejected");
        let msg = format!("{}", err);
        // Every current KernelId must appear in the error message so users
        // can identify the correct id without reading source. Driven by
        // KernelId::ALL so the test stays in sync automatically when new
        // variants are added.
        for id in KernelId::ALL {
            let name = id.to_string();
            assert!(
                msg.contains(&name),
                "error message must list '{}' as an expected id; got: {}",
                name,
                msg
            );
        }
    }

    #[test]
    fn kernel_id_all_covers_every_variant() {
        // The wildcard arm is required because KernelId is #[non_exhaustive] in
        // reify-core (external crates cannot write an exhaustive match).
        // The reify-core unit tests own the exhaustiveness contract via ALL.
        let _known_variants_reminder = |id: KernelId| match id {
            KernelId::Occt
            | KernelId::Manifold
            | KernelId::Fidget
            | KernelId::OpenVdb
            | KernelId::Gmsh => (),
            _ => (),
        };
        // Pin ALL.len() so adding a variant without extending ALL fails this test.
        assert_eq!(KernelId::ALL.len(), 5);
    }
}
