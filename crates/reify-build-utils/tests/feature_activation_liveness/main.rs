//! Feature-activation liveness gate: a registered (provider, feature) pair may
//! be activated only by a crate that names one of the items the provider gates
//! on that feature.

use std::path::{Path, PathBuf};

mod detector;
mod fixture;
mod gated_surface;
mod manifest;
mod usage;

#[test]
fn registered_feature_activations_are_live() {
    assert!(!REGISTRY.is_empty(), "an empty REGISTRY gates nothing");
    let root = workspace_root();
    let findings = detector::check(&root, REGISTRY).unwrap_or_else(|e| {
        panic!(
            "feature-activation liveness gate cannot read {}: {e}",
            root.display()
        )
    });
    let report: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert!(
        findings.is_empty(),
        "{} feature-activation liveness finding(s):\n{}",
        findings.len(),
        report.join("\n")
    );
}

/// The runtime CARGO_MANIFEST_DIR first: a test binary CoW-seeded from
/// another lane has that lane's path baked into env!().
fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(Path::parent)
        .expect("reify-build-utils sits two levels below the workspace root")
        .to_path_buf()
}
