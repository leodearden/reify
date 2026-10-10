//! Activation inventory: which workspace members turn a provider's feature on,
//! and from which manifest site.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

#[derive(Debug)]
pub enum ManifestError {
    Io { path: PathBuf, source: io::Error },
    Toml { path: PathBuf, message: String },
    UnsupportedMembersGlob { entry: String },
    MissingMember { dir: PathBuf },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Toml { path, message } => write!(f, "{}: {message}", path.display()),
            Self::UnsupportedMembersGlob { entry } => write!(
                f,
                "workspace members entry `{entry}` is a glob; list members explicitly"
            ),
            Self::MissingMember { dir } => {
                write!(f, "workspace member {} has no Cargo.toml", dir.display())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepTable {
    pub kind: DepKind,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationSite {
    Dependency {
        table: DepTable,
        key: String,
        inherited_from_workspace: bool,
    },
    FeatureTable {
        feature: String,
    },
}

impl fmt::Display for DepTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = DEPENDENCY_TABLE_NAMES
            .iter()
            .find_map(|(name, kind)| (*kind == self.kind).then_some(*name))
            .unwrap_or_default();
        match &self.target {
            Some(cfg) => write!(f, "target.'{cfg}'.{table}"),
            None => f.write_str(table),
        }
    }
}

impl fmt::Display for ActivationSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dependency {
                table,
                key,
                inherited_from_workspace,
            } => {
                write!(f, "[{table}] {key}")?;
                if *inherited_from_workspace {
                    f.write_str(" (feature set in [workspace.dependencies])")?;
                }
                Ok(())
            }
            Self::FeatureTable { feature } => write!(f, "[features] {feature}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    pub consumer: String,
    pub manifest: PathBuf,
    pub site: ActivationSite,
}

#[derive(Debug)]
pub struct Member {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: PathBuf,
    pub lib_root: PathBuf,
    pub declared_features: BTreeSet<String>,
    pub default_features: BTreeSet<String>,
    features: BTreeMap<String, Vec<String>>,
    dependencies: Vec<DependencyEntry>,
}

#[derive(Debug)]
struct DependencyEntry {
    table: DepTable,
    key: String,
    resolved: ResolvedDep,
}

#[derive(Debug)]
struct ResolvedDep {
    provider: String,
    own_features: BTreeSet<String>,
    workspace_features: BTreeSet<String>,
}

#[derive(Debug)]
pub struct Workspace {
    members: Vec<Member>,
}

impl Workspace {
    pub fn load(root: &Path) -> Result<Workspace, ManifestError> {
        let root_manifest = root.join("Cargo.toml");
        let root_table = read_manifest(&root_manifest)?;
        let workspace = section(&root_table, "workspace");
        let workspace_deps = workspace.and_then(|w| section(w, "dependencies"));
        let members = member_entries(&root_manifest, workspace)?
            .iter()
            .map(|entry| Member::load(&root.join(entry), workspace_deps))
            .collect::<Result<_, _>>()?;
        Ok(Workspace { members })
    }

    pub fn member(&self, package: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == package)
    }

    pub fn activations(&self, provider: &str, feature: &str) -> Vec<Activation> {
        self.members
            .iter()
            .flat_map(|member| {
                member
                    .activation_sites(provider, feature)
                    .map(|site| Activation {
                        consumer: member.name.clone(),
                        manifest: member.manifest.clone(),
                        site,
                    })
            })
            .collect()
    }
}

impl Member {
    fn load(dir: &Path, workspace_deps: Option<&Table>) -> Result<Member, ManifestError> {
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            return Err(ManifestError::MissingMember {
                dir: dir.to_path_buf(),
            });
        }
        let table = read_manifest(&manifest)?;
        let name = section(&table, "package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| schema_error(&manifest, "expected a [package] name"))?
            .to_string();
        let lib_path = section(&table, "lib")
            .and_then(|lib| lib.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("src/lib.rs");
        let features = feature_table(&table);
        Ok(Member {
            name,
            dir: dir.to_path_buf(),
            lib_root: dir.join(lib_path),
            declared_features: features
                .keys()
                .filter(|f| *f != "default")
                .cloned()
                .collect(),
            default_features: features
                .get("default")
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
            features,
            dependencies: dependency_entries(&table, workspace_deps),
            manifest,
        })
    }

    fn activation_sites<'a>(
        &'a self,
        provider: &'a str,
        feature: &'a str,
    ) -> impl Iterator<Item = ActivationSite> + 'a {
        let dependency_sites = self
            .dependencies
            .iter()
            .filter(move |entry| entry.resolved.provider == provider)
            .filter_map(move |entry| entry.activation_site(feature));
        dependency_sites.chain(feature_table_activations(
            &self.features,
            &self.dependencies,
            provider,
            feature,
        ))
    }
}

impl DependencyEntry {
    fn activation_site(&self, feature: &str) -> Option<ActivationSite> {
        let inherited = self.resolved.workspace_features.contains(feature);
        (inherited || self.resolved.own_features.contains(feature)).then(|| {
            ActivationSite::Dependency {
                table: self.table.clone(),
                key: self.key.clone(),
                inherited_from_workspace: inherited,
            }
        })
    }
}

fn read_manifest(path: &Path) -> Result<Table, ManifestError> {
    let text = fs::read_to_string(path).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    text.parse::<Table>().map_err(|e| ManifestError::Toml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn schema_error(path: &Path, message: &str) -> ManifestError {
    ManifestError::Toml {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn section<'a>(table: &'a Table, name: &str) -> Option<&'a Table> {
    table.get(name).and_then(Value::as_table)
}

fn strings(value: Option<&Value>) -> impl Iterator<Item = String> + '_ {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
}

fn member_entries(
    root_manifest: &Path,
    workspace: Option<&Table>,
) -> Result<Vec<String>, ManifestError> {
    let entries = workspace
        .and_then(|w| w.get("members"))
        .and_then(Value::as_array)
        .ok_or_else(|| schema_error(root_manifest, "expected a [workspace] members array"))?;
    entries
        .iter()
        .map(|entry| {
            let entry = entry
                .as_str()
                .ok_or_else(|| schema_error(root_manifest, "members entries must be strings"))?;
            if entry.contains(['*', '?', '[']) {
                return Err(ManifestError::UnsupportedMembersGlob {
                    entry: entry.to_string(),
                });
            }
            Ok(entry.to_string())
        })
        .collect()
}

fn feature_table(manifest: &Table) -> BTreeMap<String, Vec<String>> {
    section(manifest, "features")
        .into_iter()
        .flatten()
        .map(|(name, enables)| (name.clone(), strings(Some(enables)).collect()))
        .collect()
}

const DEPENDENCY_TABLE_NAMES: [(&str, DepKind); 3] = [
    ("dependencies", DepKind::Normal),
    ("dev-dependencies", DepKind::Dev),
    ("build-dependencies", DepKind::Build),
];

fn dependency_tables(manifest: &Table) -> impl Iterator<Item = (DepTable, &Table)> {
    let targeted = section(manifest, "target")
        .into_iter()
        .flatten()
        .filter_map(|(cfg, scope)| scope.as_table().map(|scope| (cfg, scope)))
        .flat_map(|(cfg, scope)| kinded_tables(scope, Some(cfg)));
    kinded_tables(manifest, None).chain(targeted)
}

fn kinded_tables<'a>(
    scope: &'a Table,
    target: Option<&'a str>,
) -> impl Iterator<Item = (DepTable, &'a Table)> + 'a {
    DEPENDENCY_TABLE_NAMES
        .iter()
        .filter_map(move |(name, kind)| {
            let table = DepTable {
                kind: *kind,
                target: target.map(str::to_string),
            };
            section(scope, name).map(|deps| (table, deps))
        })
}

fn dependency_entries(manifest: &Table, workspace_deps: Option<&Table>) -> Vec<DependencyEntry> {
    dependency_tables(manifest)
        .flat_map(|(table, deps)| {
            deps.iter().map(move |(key, value)| DependencyEntry {
                table: table.clone(),
                key: key.clone(),
                resolved: resolve_entry(key, value, workspace_deps),
            })
        })
        .collect()
}

/// The provider is the entry's `package` (or the key); a `workspace = true`
/// entry also inherits `package` and `features` from the root entry.
fn resolve_entry(key: &str, value: &Value, workspace_deps: Option<&Table>) -> ResolvedDep {
    let spec = value.as_table();
    let inherits = spec
        .and_then(|s| s.get("workspace"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let root_spec = workspace_deps
        .filter(|_| inherits)
        .and_then(|deps| deps.get(key))
        .and_then(Value::as_table);
    let package = [spec, root_spec]
        .into_iter()
        .flatten()
        .find_map(|s| s.get("package").and_then(Value::as_str));
    ResolvedDep {
        provider: package.unwrap_or(key).to_string(),
        own_features: strings(spec.and_then(|s| s.get("features"))).collect(),
        workspace_features: strings(root_spec.and_then(|s| s.get("features"))).collect(),
    }
}

/// Splits a `[features]` value `dep/feat` or `dep?/feat`; `dep:x` and bare
/// feature names enable no dependency feature.
fn dependency_feature(enable: &str) -> Option<(&str, &str)> {
    let (dep, feature) = enable.split_once('/')?;
    Some((dep.strip_suffix('?').unwrap_or(dep), feature))
}

fn feature_table_activations<'a>(
    features: &'a BTreeMap<String, Vec<String>>,
    dependencies: &'a [DependencyEntry],
    provider: &'a str,
    feature: &'a str,
) -> impl Iterator<Item = ActivationSite> + 'a {
    let names_provider = move |key: &str| {
        dependencies
            .iter()
            .any(|entry| entry.key == key && entry.resolved.provider == provider)
    };
    features
        .iter()
        .filter(move |(_, enables)| {
            enables
                .iter()
                .filter_map(|e| dependency_feature(e))
                .any(|(dep, enabled)| enabled == feature && names_provider(dep))
        })
        .map(|(name, _)| ActivationSite::FeatureTable {
            feature: name.clone(),
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fixture::{Fixture, package_manifest as package, root_manifest};

    const PROVIDER_FEATURES: &str = "[features]\nf = []\ng = []\n";

    fn provider_and_consumer(workspace_deps: &str, consumer_rest: &str) -> Fixture {
        let fx = Fixture::new();
        fx.write(
            "Cargo.toml",
            &root_manifest(&["prov", "cons"], workspace_deps),
        );
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

        assert!(
            ws.member("alpha").is_none(),
            "lookup is by package name, not dir"
        );
    }

    #[test]
    fn declared_and_default_features_are_read_from_the_features_table() {
        let fx = Fixture::new();
        fx.write("Cargo.toml", &root_manifest(&["prov", "bare"], ""));
        fx.write(
            "prov/Cargo.toml",
            &package(
                "prov",
                "[features]\ndefault = [\"f\"]\nf = []\ng = [\"f\"]\n",
            ),
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
                (
                    "normal".to_string(),
                    dep_site(DepKind::Normal, None, "prov", false)
                ),
                (
                    "build".to_string(),
                    dep_site(DepKind::Build, None, "prov", false)
                ),
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
            vec![(
                "cons".to_string(),
                dep_site(DepKind::Dev, None, "alias", false)
            )]
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
            vec![(
                "cons".to_string(),
                dep_site(DepKind::Normal, None, "alias", false)
            )]
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
            vec![(
                "cons".to_string(),
                dep_site(DepKind::Dev, None, "prov", true)
            )]
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
            vec![(
                "different".to_string(),
                dep_site(DepKind::Dev, None, "prov", false)
            )],
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
            vec![(
                "prov".to_string(),
                dep_site(DepKind::Dev, None, "prov", false)
            )]
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
