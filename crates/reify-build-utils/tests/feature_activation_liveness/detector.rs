//! The gate: judge every registered (provider, feature) pair against the
//! workspace's activations of it.

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
