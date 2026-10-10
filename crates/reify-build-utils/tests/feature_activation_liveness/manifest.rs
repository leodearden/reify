//! Activation inventory: which workspace members turn a provider's feature on,
//! and from which manifest site.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fixture::Fixture;

    fn root_manifest(members: &[&str], workspace_deps: &str) -> String {
        let list: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
        format!(
            "[workspace]\nmembers = [{}]\n\n[workspace.dependencies]\n{workspace_deps}\n",
            list.join(", ")
        )
    }

    fn package(name: &str, rest: &str) -> String {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n{rest}\n")
    }

    const PROVIDER_FEATURES: &str = "[features]\nf = []\ng = []\n";

    fn provider_and_consumer(workspace_deps: &str, consumer_rest: &str) -> Fixture {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["prov", "cons"], workspace_deps));
        fx.write("prov/Cargo.toml", &package("prov", PROVIDER_FEATURES));
        fx.write("cons/Cargo.toml", &package("cons", consumer_rest));
        fx
    }

    fn load(fx: &Fixture) -> Workspace {
        Workspace::load(fx.root()).expect("fixture workspace loads")
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn dep_site(kind: DepKind, target: Option<&str>, key: &str, inherited: bool) -> ActivationSite {
        ActivationSite::Dependency {
            table: DepTable {
                kind,
                target: target.map(str::to_string),
            },
            key: key.to_string(),
            inherited_from_workspace: inherited,
        }
    }

    fn sites(activations: &[Activation]) -> Vec<(String, ActivationSite)> {
        activations
            .iter()
            .map(|a| (a.consumer.clone(), a.site.clone()))
            .collect()
    }

    #[test]
    fn members_are_discovered_by_package_name_with_lib_root() {
        let fx = Fixture::new();
        fx.write(
            "Cargo.toml",
            &root_manifest(&["crates/alpha", "crates/beta"], ""),
        );
        fx.write("crates/alpha/Cargo.toml", &package("alpha-pkg", ""));
        fx.write(
            "crates/beta/Cargo.toml",
            &package("beta", "[lib]\npath = \"lib/custom.rs\"\n"),
        );
        let ws = load(&fx);

        let alpha = ws.member("alpha-pkg").expect("alpha-pkg is a member");
        assert_eq!(alpha.name, "alpha-pkg");
        assert_eq!(alpha.dir, fx.root().join("crates/alpha"));
        assert_eq!(alpha.manifest, fx.root().join("crates/alpha/Cargo.toml"));
        assert_eq!(alpha.lib_root, fx.root().join("crates/alpha/src/lib.rs"));

        let beta = ws.member("beta").expect("beta is a member");
        assert_eq!(beta.lib_root, fx.root().join("crates/beta/lib/custom.rs"));

        assert!(ws.member("alpha").is_none(), "lookup is by package name, not dir");
    }

    #[test]
    fn declared_and_default_features_are_read_from_the_features_table() {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["prov", "bare"], ""));
        fx.write(
            "prov/Cargo.toml",
            &package("prov", "[features]\ndefault = [\"f\"]\nf = []\ng = [\"f\"]\n"),
        );
        fx.write("bare/Cargo.toml", &package("bare", ""));
        let ws = load(&fx);

        let prov = ws.member("prov").unwrap();
        assert_eq!(prov.declared_features, names(&["f", "g"]));
        assert_eq!(prov.default_features, names(&["f"]));

        let bare = ws.member("bare").unwrap();
        assert!(bare.declared_features.is_empty());
        assert!(bare.default_features.is_empty());
    }

    #[test]
    fn workspace_inherited_dev_dependency_with_member_features_is_an_activation() {
        let fx = provider_and_consumer(
            "prov = { path = \"prov\" }",
            "[dev-dependencies]\nprov = { workspace = true, features = [\"f\"] }\n",
        );
        let ws = load(&fx);

        assert_eq!(
            ws.activations("prov", "f"),
            vec![Activation {
                consumer: "cons".to_string(),
                manifest: fx.root().join("cons/Cargo.toml"),
                site: dep_site(DepKind::Dev, None, "prov", false),
            }]
        );
    }

    #[test]
    fn normal_build_and_target_tables_are_each_detected() {
        let fx = Fixture::new();
        fx.write(
            "Cargo.toml",
            &root_manifest(&["prov", "normal", "build", "targeted"], ""),
        );
        fx.write("prov/Cargo.toml", &package("prov", PROVIDER_FEATURES));
        let entry = "prov = { path = \"../prov\", features = [\"f\"] }";
        fx.write(
            "normal/Cargo.toml",
            &package("normal", &format!("[dependencies]\n{entry}\n")),
        );
        fx.write(
            "build/Cargo.toml",
            &package("build", &format!("[build-dependencies]\n{entry}\n")),
        );
        fx.write(
            "targeted/Cargo.toml",
            &package(
                "targeted",
                &format!("[target.'cfg(unix)'.dev-dependencies]\n{entry}\n"),
            ),
        );
        let ws = load(&fx);

        assert_eq!(
            sites(&ws.activations("prov", "f")),
            vec![
                ("normal".to_string(), dep_site(DepKind::Normal, None, "prov", false)),
                ("build".to_string(), dep_site(DepKind::Build, None, "prov", false)),
                (
                    "targeted".to_string(),
                    dep_site(DepKind::Dev, Some("cfg(unix)"), "prov", false)
                ),
            ]
        );
    }

    #[test]
    fn renamed_dependency_is_attributed_to_its_package() {
        let fx = provider_and_consumer(
            "",
            "[dev-dependencies]\nalias = { package = \"prov\", path = \"../prov\", features = [\"f\"] }\n",
        );
        let ws = load(&fx);

        assert_eq!(
            sites(&ws.activations("prov", "f")),
            vec![("cons".to_string(), dep_site(DepKind::Dev, None, "alias", false))]
        );
        assert!(ws.activations("alias", "f").is_empty());
    }

    #[test]
    fn workspace_inherited_rename_resolves_package_from_the_root_entry() {
        let fx = provider_and_consumer(
            "alias = { package = \"prov\", path = \"prov\" }",
            "[dependencies]\nalias = { workspace = true, features = [\"f\"] }\n",
        );
        let ws = load(&fx);

        assert_eq!(
            sites(&ws.activations("prov", "f")),
            vec![("cons".to_string(), dep_site(DepKind::Normal, None, "alias", false))]
        );
    }

    #[test]
    fn feature_carried_by_the_workspace_entry_is_marked_inherited() {
        let fx = provider_and_consumer(
            "prov = { path = \"prov\", features = [\"f\"] }",
            "[dev-dependencies]\nprov.workspace = true\n",
        );
        let ws = load(&fx);

        assert_eq!(
            sites(&ws.activations("prov", "f")),
            vec![("cons".to_string(), dep_site(DepKind::Dev, None, "prov", true))]
        );
    }

    #[test]
    fn features_table_dependency_feature_entries_are_activations() {
        let fx = provider_and_consumer(
            "",
            "[dependencies]\n\
             prov = { path = \"../prov\", optional = true }\n\
             alias = { package = \"prov\", path = \"../prov\", optional = true }\n\n\
             [features]\n\
             own = []\n\
             x = [\"prov/f\"]\n\
             y = [\"prov?/f\"]\n\
             z = [\"alias/f\"]\n\
             only-dep = [\"dep:prov\"]\n\
             bare = [\"own\"]\n\
             other = [\"prov/g\"]\n",
        );
        let ws = load(&fx);

        let expected: Vec<(String, ActivationSite)> = ["x", "y", "z"]
            .iter()
            .map(|f| {
                (
                    "cons".to_string(),
                    ActivationSite::FeatureTable {
                        feature: f.to_string(),
                    },
                )
            })
            .collect();
        assert_eq!(sites(&ws.activations("prov", "f")), expected);
    }

    #[test]
    fn entries_that_do_not_turn_the_feature_on_yield_nothing() {
        let fx = Fixture::new();
        fx.write(
            "Cargo.toml",
            &root_manifest(&["prov", "plain", "versioned", "different"], ""),
        );
        fx.write("prov/Cargo.toml", &package("prov", PROVIDER_FEATURES));
        fx.write(
            "plain/Cargo.toml",
            &package("plain", "[dependencies]\nprov = { path = \"../prov\" }\n"),
        );
        fx.write(
            "versioned/Cargo.toml",
            &package("versioned", "[dependencies]\nprov = \"0.1\"\n"),
        );
        fx.write(
            "different/Cargo.toml",
            &package(
                "different",
                "[dev-dependencies]\nprov = { path = \"../prov\", features = [\"g\"] }\n",
            ),
        );
        let ws = load(&fx);

        assert!(ws.activations("prov", "f").is_empty());
        assert_eq!(
            sites(&ws.activations("prov", "g")),
            vec![("different".to_string(), dep_site(DepKind::Dev, None, "prov", false))],
            "the fixture's other feature is seen, so the empty result above is not vacuous"
        );
    }

    #[test]
    fn self_dev_dependency_is_an_activation_by_the_provider_itself() {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["prov"], ""));
        fx.write(
            "prov/Cargo.toml",
            &package(
                "prov",
                &format!(
                    "{PROVIDER_FEATURES}\n[dev-dependencies]\nprov = {{ path = \".\", features = [\"f\"] }}\n"
                ),
            ),
        );
        let ws = load(&fx);

        assert_eq!(
            sites(&ws.activations("prov", "f")),
            vec![("prov".to_string(), dep_site(DepKind::Dev, None, "prov", false))]
        );
    }

    #[test]
    fn members_glob_is_refused() {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["crates/*"], ""));
        fx.write("crates/a/Cargo.toml", &package("a", ""));

        let err = Workspace::load(fx.root()).unwrap_err();
        assert!(
            matches!(&err, ManifestError::UnsupportedMembersGlob { entry } if entry == "crates/*"),
            "{err:?}"
        );
    }

    #[test]
    fn member_without_a_manifest_is_refused() {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["ghost"], ""));
        fx.write("ghost/src/lib.rs", "");

        let err = Workspace::load(fx.root()).unwrap_err();
        let ghost = fx.root().join("ghost");
        assert!(
            matches!(&err, ManifestError::MissingMember { dir } if *dir == ghost),
            "{err:?}"
        );
    }
}
