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
        let raw: ManifestRaw =
            toml::from_str(s).map_err(|e| ManifestError::Parse(e.to_string()))?;
        let mut kernels: BTreeMap<KernelId, KernelPin> = BTreeMap::new();
        for (raw_id, raw_pin) in raw.kernels.into_iter() {
            // Step-6 will replace this generic Parse fallback with a typed
            // ManifestError::UnknownKernel variant. For now any non-canonical
            // id surfaces as a Parse error so the [kernels] wiring is testable
            // against the canonical-order assertions in step-3.
            let id = KernelId::from_str(&raw_id).map_err(|_| {
                ManifestError::Parse(format!("unknown kernel id: '{}'", raw_id))
            })?;
            kernels.insert(
                id,
                KernelPin {
                    version: raw_pin.into_version(),
                },
            );
        }
        Ok(Manifest { kernels })
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
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Parse(msg) => {
                write!(f, "failed to parse reify.toml: {}", msg)
            }
        }
    }
}

impl std::error::Error for ManifestError {}

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
