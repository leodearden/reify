//! Project manifest (`reify.toml`) schema, parser, and validator.
//!
//! This crate owns the schema for the project pin described in the v0.2
//! multi-kernel PRD ("Resolved design decisions (2026-04-28)" → "Project pin").
//! It is intentionally self-contained: no other workspace crate consumes it
//! yet, but the binary entry points (CLI, GUI launcher, MCP server) and the
//! future kernel registry will read the parsed pin from here.
//!
//! See the doc comment on [`Manifest`] (added in later steps) for the on-disk
//! schema and worked examples.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;

/// Parsed project manifest.
///
/// Carries the set of pinned kernels declared by the project. The map is a
/// `BTreeMap` so iteration order is deterministic and stable across runs —
/// the PRD frames "determinism follows from the pin" as a load-bearing
/// invariant for the v0.2 multi-kernel design.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    kernels: BTreeMap<KernelId, KernelPin>,
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
            let id = KernelId::from_str(&raw_id)
                .map_err(|_| ManifestError::UnknownKernel(raw_id.clone()))?;
            let version = raw_pin.into_version();
            if version.trim().is_empty() {
                return Err(ManifestError::EmptyVersion(id));
            }
            kernels.insert(id, KernelPin { version });
        }
        Ok(Manifest { kernels })
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
}

/// Identifier for a kernel supported by Reify v0.2.
///
/// Truck is intentionally absent: the v0.2 PRD ("Truck dropped from v0.2")
/// rejects truck as an unknown kernel id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KernelId {
    Occt,
    Manifold,
    Fidget,
    OpenVdb,
}

/// A pinned kernel version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPin {
    /// The pinned version, stored as a raw string. The exact comparison
    /// policy (semver, ABI version, three-part numeric, …) is a registry
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
    /// A `[kernels]` entry used a key that is not one of the four
    /// kernel ids supported in v0.2 (occt, manifold, fidget, openvdb).
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
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Parse(msg) => {
                write!(f, "failed to parse reify.toml: {}", msg)
            }
            ManifestError::UnknownKernel(key) => {
                write!(
                    f,
                    "unknown kernel id '{}' in [kernels] (expected one of: occt, manifold, fidget, openvdb)",
                    key
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
#[derive(Debug, Default, Deserialize)]
struct ManifestRaw {
    #[serde(default)]
    kernels: BTreeMap<String, KernelPinRaw>,
}

/// Internal serde shape for a single kernel pin.
///
/// Accepts either an inline string scalar (`occt = "7.7.0"`) or a table
/// (`[kernels.occt]\nversion = "7.7.0"`). The inline form is the
/// recommended spelling; the table form is accepted for forward
/// compatibility with future per-kernel options.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KernelPinRaw {
    Inline(String),
    Table { version: String },
}

impl KernelPinRaw {
    fn into_version(self) -> String {
        match self {
            KernelPinRaw::Inline(v) => v,
            KernelPinRaw::Table { version } => version,
        }
    }
}

impl FromStr for KernelId {
    type Err = UnknownKernelId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "occt" => Ok(KernelId::Occt),
            "manifold" => Ok(KernelId::Manifold),
            "fidget" => Ok(KernelId::Fidget),
            "openvdb" => Ok(KernelId::OpenVdb),
            _ => Err(UnknownKernelId),
        }
    }
}

impl fmt::Display for KernelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            KernelId::Occt => "occt",
            KernelId::Manifold => "manifold",
            KernelId::Fidget => "fidget",
            KernelId::OpenVdb => "openvdb",
        };
        f.write_str(s)
    }
}

/// Returned by `KernelId::from_str` when the string is not a canonical
/// kernel id. Currently only used internally; consumers see the typed
/// `ManifestError::UnknownKernel` variant (added in step-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownKernelId;

impl fmt::Display for UnknownKernelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown kernel id")
    }
}

impl std::error::Error for UnknownKernelId {}

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
    fn kernel_id_round_trips_for_all_four_v0_2_kernels() {
        let cases: &[(KernelId, &str)] = &[
            (KernelId::Occt, "occt"),
            (KernelId::Manifold, "manifold"),
            (KernelId::Fidget, "fidget"),
            (KernelId::OpenVdb, "openvdb"),
        ];
        for &(id, canonical) in cases {
            let displayed = format!("{}", id);
            assert_eq!(
                displayed, canonical,
                "Display for {:?} must be canonical lowercase",
                id
            );
            let parsed: KernelId = displayed.parse().expect("Display output must parse back");
            assert_eq!(parsed, id, "round-trip must yield the original KernelId");
        }
    }

    #[test]
    fn kernel_id_from_str_rejects_unknown_strings() {
        let invalid = [
            "",       // empty
            " occt",  // leading whitespace
            "occt ",  // trailing whitespace
            "Occt",   // capitalised
            "OCCT",   // upper-case
            "Fidget", // capitalised
            "truck",  // dropped from v0.2
            "open_vdb",
            "open-vdb",
        ];
        for s in invalid {
            assert!(
                s.parse::<KernelId>().is_err(),
                "expected '{}' to be rejected by KernelId::from_str",
                s
            );
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

    #[test]
    fn multiple_kernel_pins_iterate_in_kernel_id_order() {
        // Non-canonical text order in the TOML source.
        let toml = "[kernels]\n\
                    fidget = \"0.3.4\"\n\
                    occt = \"7.7.0\"\n\
                    openvdb = \"11.0\"\n\
                    manifold = \"2.5\"\n";
        let manifest = Manifest::from_toml_str(toml).expect("four-pin TOML should parse");
        let ids: Vec<KernelId> = manifest.kernel_pins().map(|(id, _)| *id).collect();
        // BTreeMap iteration follows the derived `Ord` on `KernelId`, which is
        // the variant declaration order.
        assert_eq!(
            ids,
            vec![
                KernelId::Occt,
                KernelId::Manifold,
                KernelId::Fidget,
                KernelId::OpenVdb,
            ]
        );
    }
}
