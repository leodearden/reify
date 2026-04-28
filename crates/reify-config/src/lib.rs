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
        let _raw: ManifestRaw = toml::from_str(s).map_err(|e| ManifestError::Parse(e.to_string()))?;
        // Empty-input case: ManifestRaw::default() yields an empty kernels map.
        // Step-4 will translate the raw entries into typed (KernelId, KernelPin)
        // pairs; for now we always produce an empty manifest, which is enough
        // to pass step-1's empty-input test.
        Ok(Manifest::default())
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
    #[allow(dead_code)] // wired up in step-4
    kernels: BTreeMap<String, KernelPinRaw>,
}

/// Internal serde shape for a single kernel pin.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // wired up in step-4
struct KernelPinRaw {
    version: String,
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
}
