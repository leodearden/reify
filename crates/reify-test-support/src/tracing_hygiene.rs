#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn trait_impl_site(line: usize, text: &str) -> HandRolledSubscriberSite {
        HandRolledSubscriberSite {
            line,
            construct: SubscriberConstruct::TraitImpl,
            text: text.to_owned(),
        }
    }

    #[test]
    fn flags_a_tracing_subscriber_trait_impl() {
        let source = "use std::fmt;\n\
                      struct Foo;\n\
                      impl tracing::Subscriber for Foo {\n\
                      }\n";

        assert_eq!(
            find_hand_rolled_subscriber_sites(source),
            vec![trait_impl_site(3, "impl tracing::Subscriber for Foo {")],
        );
    }

    #[test]
    fn flags_every_trait_impl_spelling_and_every_occurrence() {
        let source = "impl Subscriber for A {}\n\
                      impl tracing_core::Subscriber for B {}\n\
                      impl<S, F> tracing::Subscriber for C<S, F> {}\n\
                      impl<\n    S,\n\
                      > tracing::Subscriber for D<S> {}\n";

        assert_eq!(
            find_hand_rolled_subscriber_sites(source),
            vec![
                trait_impl_site(1, "impl Subscriber for A {}"),
                trait_impl_site(2, "impl tracing_core::Subscriber for B {}"),
                trait_impl_site(3, "impl<S, F> tracing::Subscriber for C<S, F> {}"),
                trait_impl_site(6, "> tracing::Subscriber for D<S> {}"),
            ],
        );
    }

    #[test]
    fn flags_tracing_subscriber_crate_paths() {
        let source = "use tracing_subscriber::Layer;\n\
                      fn main() {\n\
                      \x20   let s = tracing_subscriber::fmt().finish();\n\
                      }\n";

        let sites = find_hand_rolled_subscriber_sites(source);

        assert_eq!(
            sites
                .iter()
                .map(|site| (site.line, site.construct))
                .collect::<Vec<_>>(),
            vec![
                (1, SubscriberConstruct::TracingSubscriberCrate),
                (3, SubscriberConstruct::TracingSubscriberCrate),
            ],
        );
    }

    #[test]
    fn clears_primed_constructor_usage_and_non_subscriber_impls() {
        let source = "fn t() {\n\
                      \x20   let (subscriber, capture) = reify_test_support::CapturingSubscriberBuilder::new(tracing::Level::DEBUG).target_prefix(\"x\").build();\n\
                      \x20   tracing::subscriber::with_default(subscriber, || {});\n\
                      }\n\
                      fn f() -> (impl tracing::Subscriber + Send + Sync, Capture) {\n\
                      \x20   build()\n\
                      }\n\
                      impl tracing::field::Visit for V {}\n\
                      impl<S: Subscriber> Wrapper<S> {}\n";

        assert_eq!(find_hand_rolled_subscriber_sites(source), Vec::new());
    }

    #[test]
    fn skips_comment_lines() {
        let source = "/// impl tracing::Subscriber for Foo\n\
                      //! use tracing_subscriber::fmt;\n\
                      \x20   // impl tracing::Subscriber for Foo {\n";

        assert_eq!(find_hand_rolled_subscriber_sites(source), Vec::new());
    }

    #[test]
    fn collector_reports_relative_paths_and_honours_include() {
        let guard = crate::temp_dirs::prefixed_tempdir("tracing-hygiene-");
        let root = guard.path();
        let write = |rel: &str, contents: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().expect("fixture path has a parent"))
                .expect("create fixture dir");
            std::fs::write(&path, contents).expect("write fixture file");
        };
        write("a/src/lib.rs", "struct X;\nimpl tracing::Subscriber for X {}\n");
        write("b/tests/t.rs", "struct Y;\nimpl tracing::Subscriber for Y {}\n");
        write("c/src/clean.rs", "fn clean() {}\n");

        let filtered = collect_workspace_hand_rolled_subscriber_sites(root, |rel| !rel.starts_with("b"));
        assert_eq!(filtered.len(), 1, "expected exactly one site, got {filtered:?}");
        assert_eq!(filtered[0].path, PathBuf::from("a/src/lib.rs"));
        assert_eq!(filtered[0].site.line, 2);
        assert_eq!(filtered[0].site.construct, SubscriberConstruct::TraitImpl);

        let everything = collect_workspace_hand_rolled_subscriber_sites(root, |_| true);
        assert_eq!(everything.len(), 2, "expected two sites, got {everything:?}");
        assert_eq!(
            everything
                .iter()
                .map(|site| site.path.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([PathBuf::from("a/src/lib.rs"), PathBuf::from("b/tests/t.rs")]),
        );
    }
}
