//! The gate: judge every registered (provider, feature) pair against the
//! workspace's activations of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::gated_surface::{GatedSurface, ScanError, ShapeViolation};
use crate::manifest::{Activation, ManifestError, Member, Workspace};
use crate::usage::{UsageError, first_use};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredPair {
    pub provider: &'static str,
    pub feature: &'static str,
}

impl fmt::Display for RegisteredPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.feature)
    }
}

/// A registry row that would make the gate vacuous for its pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryDefect {
    ProviderNotMember,
    FeatureNotDeclared,
    FeatureEnabledByDefault,
    NoGatedSurface,
    NoActivations,
}

#[derive(Debug, PartialEq)]
pub enum Finding {
    DeadActivation {
        pair: RegisteredPair,
        activation: Activation,
    },
    InadmissibleShape {
        pair: RegisteredPair,
        violation: ShapeViolation,
    },
    RegistryDefect {
        pair: RegisteredPair,
        defect: RegistryDefect,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeadActivation { pair, activation } => write!(
                f,
                "dead feature activation: {} activates {pair} at {} {} but names none of the \
                 items {} gates on `{}`; drop the activation or use a gated item",
                activation.consumer,
                activation.manifest.display(),
                activation.site,
                pair.provider,
                pair.feature,
            ),
            Self::InadmissibleShape { pair, violation } => write!(
                f,
                "inadmissible gate shape: {}:{}: {} exposes {pair} surface through {} without \
                 naming a gated item, so liveness cannot be judged by name; gate a named item \
                 instead, or drop the pair from REGISTRY",
                violation.file.display(),
                violation.line,
                pair.provider,
                violation.shape,
            ),
            Self::RegistryDefect { pair, defect } => {
                write!(f, "registry defect for {pair}: ")?;
                describe_defect(f, *pair, *defect)?;
                f.write_str("; fix the REGISTRY row so the gate is not vacuous for it")
            }
        }
    }
}

fn describe_defect(
    f: &mut fmt::Formatter<'_>,
    pair: RegisteredPair,
    defect: RegistryDefect,
) -> fmt::Result {
    let RegisteredPair { provider, feature } = pair;
    match defect {
        RegistryDefect::ProviderNotMember => write!(f, "{provider} is not a workspace member"),
        RegistryDefect::FeatureNotDeclared => {
            write!(f, "{provider} declares no feature `{feature}`")
        }
        RegistryDefect::FeatureEnabledByDefault => write!(
            f,
            "`{feature}` is a default feature of {provider}, so every dependent turns it on"
        ),
        RegistryDefect::NoGatedSurface => {
            write!(f, "{provider} gates no public item on `{feature}`")
        }
        RegistryDefect::NoActivations => write!(f, "no workspace member activates {pair}"),
    }
}

#[derive(Debug)]
pub enum CheckError {
    Manifest(ManifestError),
    Scan(ScanError),
    Usage(UsageError),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(e) => e.fmt(f),
            Self::Scan(e) => e.fmt(f),
            Self::Usage(e) => e.fmt(f),
        }
    }
}

impl From<ManifestError> for CheckError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

impl From<ScanError> for CheckError {
    fn from(e: ScanError) -> Self {
        Self::Scan(e)
    }
}

impl From<UsageError> for CheckError {
    fn from(e: UsageError) -> Self {
        Self::Usage(e)
    }
}

pub fn check(root: &Path, registry: &[RegisteredPair]) -> Result<Vec<Finding>, CheckError> {
    let workspace = Workspace::load(root)?;
    let mut findings = Vec::new();
    for &pair in registry {
        findings.extend(judge(&workspace, pair)?);
    }
    Ok(findings)
}

/// Registry defects first, then shape violations, then dead activations.
fn judge(workspace: &Workspace, pair: RegisteredPair) -> Result<Vec<Finding>, CheckError> {
    let defect = |defect| Finding::RegistryDefect { pair, defect };
    let Some(provider) = workspace.member(pair.provider) else {
        return Ok(vec![defect(RegistryDefect::ProviderNotMember)]);
    };
    if !provider.declared_features.contains(pair.feature) {
        return Ok(vec![defect(RegistryDefect::FeatureNotDeclared)]);
    }
    let surface = GatedSurface::scan(&provider.lib_root, pair.feature)?;
    let activations = workspace.activations(pair.provider, pair.feature);
    let defects = [
        (
            provider.default_features.contains(pair.feature),
            RegistryDefect::FeatureEnabledByDefault,
        ),
        (surface.evidence.is_empty(), RegistryDefect::NoGatedSurface),
        (activations.is_empty(), RegistryDefect::NoActivations),
    ];
    let mut findings: Vec<Finding> = defects
        .into_iter()
        .filter(|(present, _)| *present)
        .map(|(_, kind)| defect(kind))
        .collect();
    findings.extend(
        surface
            .violations
            .into_iter()
            .map(|violation| Finding::InadmissibleShape { pair, violation }),
    );
    if !surface.evidence.is_empty() {
        findings.extend(dead_activations(
            provider,
            pair,
            &surface.evidence,
            activations,
        )?);
    }
    Ok(findings)
}

/// Each consumer is scanned once per pair, however many sites activate it.
fn dead_activations(
    provider: &Member,
    pair: RegisteredPair,
    evidence: &BTreeSet<String>,
    activations: Vec<Activation>,
) -> Result<Vec<Finding>, CheckError> {
    let mut liveness: BTreeMap<String, bool> = BTreeMap::new();
    let mut findings = Vec::new();
    for activation in activations {
        let live = match liveness.get(&activation.consumer) {
            Some(&live) => live,
            None => {
                let live = names_evidence(provider, &activation, evidence)?;
                liveness.insert(activation.consumer.clone(), live);
                live
            }
        };
        if !live {
            findings.push(Finding::DeadActivation { pair, activation });
        }
    }
    Ok(findings)
}

/// The consumer's package dir is its manifest's dir. A provider activating
/// itself is judged without its own lib source, where every mention is a
/// definition or sits under cfg(test).
fn names_evidence(
    provider: &Member,
    activation: &Activation,
    evidence: &BTreeSet<String>,
) -> Result<bool, CheckError> {
    let package_dir = activation.manifest.parent().unwrap_or(Path::new(""));
    let exclude = if activation.consumer == provider.name {
        provider.lib_root.parent()
    } else {
        None
    };
    Ok(first_use(package_dir, exclude, evidence)?.is_some())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::fixture::{Fixture, package_manifest, root_manifest};
    use crate::gated_surface::Shape;
    use crate::manifest::{ActivationSite, DepKind, DepTable};

    const PAIR: RegisteredPair = RegisteredPair {
        provider: "prov",
        feature: "f",
    };
    const PROVIDER_FEATURES: &str = "[features]\nf = []\ng = []\n";
    const PROVIDER_LIB: &str = "#[cfg(feature = \"f\")]\npub fn hook() {}\n";
    const DEV_ACTIVATION: &str =
        "[dev-dependencies]\nprov = { workspace = true, features = [\"f\"] }\n";
    const CALLS_HOOK: &str = "#[test]\nfn t() {\n    prov::hook();\n}\n";

    /// A workspace of `prov` (manifest body `provider_rest`) plus consumers,
    /// each given as (name, manifest body).
    fn workspace(provider_rest: &str, consumers: &[(&str, &str)]) -> Fixture {
        let fx = Fixture::new();
        let mut members = vec!["prov"];
        members.extend(consumers.iter().map(|(name, _)| *name));
        fx.write(
            "Cargo.toml",
            &root_manifest(&members, "prov = { path = \"prov\" }"),
        );
        fx.write("prov/Cargo.toml", &package_manifest("prov", provider_rest));
        fx.write("prov/src/lib.rs", PROVIDER_LIB);
        for (name, rest) in consumers {
            fx.write(&format!("{name}/Cargo.toml"), &package_manifest(name, rest));
            fx.write(&format!("{name}/src/lib.rs"), "");
        }
        fx
    }

    fn run(fx: &Fixture, registry: &[RegisteredPair]) -> Vec<Finding> {
        check(fx.root(), registry).expect("fixture workspace checks")
    }

    fn dead(pair: RegisteredPair, root: &Path, consumer: &str, kind: DepKind) -> Finding {
        Finding::DeadActivation {
            pair,
            activation: Activation {
                consumer: consumer.to_string(),
                manifest: root.join(consumer).join("Cargo.toml"),
                site: ActivationSite::Dependency {
                    table: DepTable { kind, target: None },
                    key: "prov".to_string(),
                    inherited_from_workspace: false,
                },
            },
        }
    }

    fn defect(pair: RegisteredPair, defect: RegistryDefect) -> Finding {
        Finding::RegistryDefect { pair, defect }
    }

    #[test]
    fn an_activation_whose_consumer_only_mentions_the_item_in_a_comment_is_dead() {
        let fx = workspace(PROVIDER_FEATURES, &[("dead", DEV_ACTIVATION)]);
        fx.write(
            "dead/tests/t.rs",
            "// prov::hook() is not called here\nfn t() {}\n",
        );
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![dead(PAIR, fx.root(), "dead", DepKind::Dev)]
        );
    }

    #[test]
    fn an_activation_whose_consumer_names_a_gated_item_is_live() {
        let fx = workspace(PROVIDER_FEATURES, &[("live", DEV_ACTIVATION)]);
        fx.write("live/tests/t.rs", CALLS_HOOK);
        assert_eq!(run(&fx, &[PAIR]), vec![]);
    }

    #[test]
    fn a_self_activation_is_judged_without_the_providers_own_lib_source() {
        let provider = format!(
            "{PROVIDER_FEATURES}\n[dev-dependencies]\nprov = {{ path = \".\", features = [\"f\"] }}\n"
        );
        let fx = workspace(&provider, &[]);
        fx.write(
            "prov/src/lib.rs",
            &format!("{PROVIDER_LIB}pub fn caller() {{\n    hook();\n}}\n"),
        );
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![dead(PAIR, fx.root(), "prov", DepKind::Dev)]
        );

        fx.write("prov/tests/t.rs", CALLS_HOOK);
        assert_eq!(run(&fx, &[PAIR]), vec![]);
    }

    #[test]
    fn an_inadmissible_shape_is_a_finding_even_when_every_activation_is_live() {
        let fx = workspace(PROVIDER_FEATURES, &[("live", DEV_ACTIVATION)]);
        fx.write("live/tests/t.rs", CALLS_HOOK);
        fx.write(
            "prov/src/lib.rs",
            &format!("{PROVIDER_LIB}#[cfg_attr(feature = \"f\", derive(Debug))]\npub struct S;\n"),
        );
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![Finding::InadmissibleShape {
                pair: PAIR,
                violation: ShapeViolation {
                    file: fx.root().join("prov/src/lib.rs"),
                    line: 3,
                    shape: Shape::CfgAttr,
                },
            }]
        );
    }

    #[test]
    fn a_provider_that_is_not_a_member_is_a_registry_defect() {
        let fx = workspace(PROVIDER_FEATURES, &[]);
        let ghost = RegisteredPair {
            provider: "ghost",
            feature: "f",
        };
        assert_eq!(
            run(&fx, &[ghost]),
            vec![defect(ghost, RegistryDefect::ProviderNotMember)]
        );
    }

    #[test]
    fn an_undeclared_feature_is_a_registry_defect() {
        let fx = workspace(PROVIDER_FEATURES, &[]);
        let nope = RegisteredPair {
            provider: "prov",
            feature: "nope",
        };
        assert_eq!(
            run(&fx, &[nope]),
            vec![defect(nope, RegistryDefect::FeatureNotDeclared)]
        );
    }

    #[test]
    fn a_default_enabled_feature_is_a_registry_defect() {
        let fx = workspace(
            "[features]\ndefault = [\"f\"]\nf = []\n",
            &[("live", DEV_ACTIVATION)],
        );
        fx.write("live/tests/t.rs", CALLS_HOOK);
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![defect(PAIR, RegistryDefect::FeatureEnabledByDefault)]
        );
    }

    #[test]
    fn a_provider_without_gated_public_items_is_a_registry_defect() {
        let fx = workspace(PROVIDER_FEATURES, &[("live", DEV_ACTIVATION)]);
        fx.write("live/tests/t.rs", CALLS_HOOK);
        fx.write(
            "prov/src/lib.rs",
            "pub fn hook() {}\n#[cfg(feature = \"f\")]\nfn private_hook() {}\n",
        );
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![defect(PAIR, RegistryDefect::NoGatedSurface)]
        );
    }

    #[test]
    fn a_pair_nobody_activates_is_a_registry_defect() {
        let fx = workspace(PROVIDER_FEATURES, &[("bystander", "")]);
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![defect(PAIR, RegistryDefect::NoActivations)]
        );
    }

    #[test]
    fn pairs_are_judged_independently_and_reported_in_registry_order() {
        let both = "[dev-dependencies]\nprov = { workspace = true, features = [\"f\", \"g\"] }\n";
        let fx = workspace(PROVIDER_FEATURES, &[("dead", both), ("half", both)]);
        fx.write(
            "prov/src/lib.rs",
            &format!("{PROVIDER_LIB}#[cfg(feature = \"g\")]\npub fn other() {{}}\n"),
        );
        fx.write("half/tests/t.rs", CALLS_HOOK);
        let g = RegisteredPair {
            provider: "prov",
            feature: "g",
        };
        assert_eq!(
            run(&fx, &[g, PAIR]),
            vec![
                dead(g, fx.root(), "dead", DepKind::Dev),
                dead(g, fx.root(), "half", DepKind::Dev),
                dead(PAIR, fx.root(), "dead", DepKind::Dev),
            ]
        );
    }

    #[test]
    fn a_dead_consumer_activating_from_two_tables_has_one_finding_per_site() {
        let twice = format!(
            "[dependencies]\nprov = {{ workspace = true, features = [\"f\"] }}\n\n{DEV_ACTIVATION}"
        );
        let fx = workspace(PROVIDER_FEATURES, &[("twice", &twice)]);
        assert_eq!(
            run(&fx, &[PAIR]),
            vec![
                dead(PAIR, fx.root(), "twice", DepKind::Normal),
                dead(PAIR, fx.root(), "twice", DepKind::Dev),
            ]
        );
    }
}
