//! The items a provider gates on one feature, read from its module tree.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::fixture::Fixture;

    fn crate_with(files: &[(&str, &str)]) -> Fixture {
        let fx = Fixture::new();
        for (rel, contents) in files {
            fx.write(rel, contents);
        }
        fx
    }

    fn lib_root(fx: &Fixture) -> PathBuf {
        fx.root().join("src/lib.rs")
    }

    fn evidence(files: &[(&str, &str)]) -> BTreeSet<String> {
        let fx = crate_with(files);
        GatedSurface::scan(&lib_root(&fx), "f")
            .expect("fixture crate scans")
            .evidence
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    const TRAP_CRATE: &[(&str, &str)] = &[
        (
            "src/lib.rs",
            "#[cfg(any(test, feature = \"f\"))]\npub mod probe;\npub mod graph;\npub mod invariants;\n",
        ),
        ("src/probe.rs", "pub fn inner() {}\n"),
        ("src/graph.rs", "pub fn build() {}\n"),
        ("src/invariants.rs", "pub fn check() {}\n"),
    ];

    #[test]
    fn module_gate_covers_only_the_module_it_is_attached_to() {
        let evidence = evidence(TRAP_CRATE);
        assert!(evidence.contains("probe"), "{evidence:?}");
        for ungated in ["graph", "invariants", "build", "check"] {
            assert!(
                !evidence.contains(ungated),
                "{ungated} leaked: {evidence:?}"
            );
        }
    }

    #[test]
    fn items_inside_a_gated_module_inherit_its_gate() {
        assert_eq!(evidence(TRAP_CRATE), names(&["probe", "inner"]));
    }

    #[test]
    fn gated_method_of_an_ungated_inherent_impl_is_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "pub struct Engine;\n\
             impl Engine {\n\
                 #[cfg(feature = \"f\")]\n\
                 pub fn hook(&self) {}\n\
                 pub fn plain(&self) {}\n\
             }\n",
        )]);
        assert_eq!(evidence, names(&["hook"]));
    }

    #[test]
    fn only_a_positive_mention_of_the_feature_gates() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(not(feature = \"f\"))]\npub fn off() {}\n\
             #[cfg(feature = \"other\")]\npub fn other() {}\n\
             #[cfg(not(any(unix, feature = \"f\")))]\npub fn nested_off() {}\n\
             #[cfg(all(unix, feature = \"f\"))]\npub fn both() {}\n\
             #[cfg(any(test, feature = \"f\"))]\npub fn either() {}\n\
             #[cfg(not(not(feature = \"f\")))]\npub fn double_negated() {}\n\
             #[cfg(feature = \"f\")]\npub fn direct() {}\n",
        )]);
        assert_eq!(
            evidence,
            names(&["both", "either", "double_negated", "direct"])
        );
    }

    #[test]
    fn only_public_gated_positions_are_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nfn hidden() {}\n\
             #[cfg(feature = \"f\")]\npub(crate) fn crate_only() {}\n\
             pub struct Holder {\n\
                 #[cfg(feature = \"f\")]\n    pub slot: u8,\n\
                 #[cfg(feature = \"f\")]\n    hidden_slot: u8,\n\
                 pub plain: u8,\n\
             }\n\
             pub enum Mode {\n\
                 #[cfg(feature = \"f\")]\n    Probe,\n\
                 Normal,\n\
             }\n",
        )]);
        assert_eq!(evidence, names(&["slot", "Probe"]));
    }

    #[test]
    fn gated_reexport_contributes_its_leaf_names_rename_aware() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "mod inner {\n    pub struct A;\n    pub struct B;\n}\n\
             #[cfg(feature = \"f\")]\npub use inner::{A, B as C};\n",
        )]);
        assert_eq!(evidence, names(&["A", "C"]));
    }

    #[test]
    fn gated_macro_rules_is_evidence() {
        let evidence = evidence(&[(
            "src/lib.rs",
            "#[cfg(feature = \"f\")]\nmacro_rules! m {\n    () => {};\n}\n",
        )]);
        assert_eq!(evidence, names(&["m"]));
    }

    #[test]
    fn out_of_line_modules_resolve_in_both_layouts_and_under_inline_modules() {
        let gated = |name: &str| format!("#[cfg(feature = \"f\")]\npub fn {name}() {{}}\n");
        let evidence = evidence(&[
            (
                "src/lib.rs",
                "pub mod a;\npub mod c;\npub mod x {\n    pub mod y;\n}\n",
            ),
            ("src/a.rs", "pub mod b;\npub mod z {\n    pub mod w;\n}\n"),
            ("src/a/b.rs", &gated("in_b")),
            ("src/a/z/w.rs", &gated("in_w")),
            ("src/c/mod.rs", "pub mod d;\n"),
            ("src/c/d.rs", &gated("in_d")),
            ("src/x/y.rs", &gated("in_y")),
        ]);
        assert_eq!(evidence, names(&["in_b", "in_w", "in_d", "in_y"]));
    }

    #[test]
    fn path_attribute_resolves_relative_to_the_declaring_file() {
        let evidence = evidence(&[
            ("src/lib.rs", "pub mod a;\n"),
            (
                "src/a.rs",
                "#[path = \"elsewhere/impl_a.rs\"]\npub mod renamed;\n",
            ),
            (
                "src/elsewhere/impl_a.rs",
                "#[cfg(feature = \"f\")]\npub fn via_path() {}\n",
            ),
        ]);
        assert_eq!(evidence, names(&["via_path"]));
    }

    #[test]
    fn path_attribute_inside_an_inline_module_is_refused() {
        let fx = crate_with(&[
            (
                "src/lib.rs",
                "mod outer {\n    #[path = \"p.rs\"]\n    mod p;\n}\n",
            ),
            ("src/p.rs", ""),
        ]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let lib = lib_root(&fx);
        assert!(
            matches!(&err, ScanError::UnsupportedPath { declared_in, line: 2 } if *declared_in == lib),
            "{err:?}"
        );
    }

    #[test]
    fn unresolved_module_is_an_error_naming_its_declaring_file() {
        let fx = crate_with(&[("src/lib.rs", "pub fn ok() {}\nmod missing;\n")]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let lib = lib_root(&fx);
        assert!(
            matches!(
                &err,
                ScanError::UnresolvedModule { declared_in, line: 2, module }
                    if *declared_in == lib && module == "missing"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn unparsable_module_file_is_an_error_naming_that_file() {
        let fx = crate_with(&[
            ("src/lib.rs", "mod broken;\n"),
            ("src/broken.rs", "pub fn (\n"),
        ]);
        let err = GatedSurface::scan(&lib_root(&fx), "f").unwrap_err();
        let broken = fx.root().join("src/broken.rs");
        assert!(
            matches!(&err, ScanError::Parse { path, .. } if *path == broken),
            "{err:?}"
        );
    }
}
