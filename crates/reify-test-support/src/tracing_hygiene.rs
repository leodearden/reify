//! Construct lint: every tracing subscriber in the workspace must come from
//! this crate's auto-priming constructors ([`crate::CapturingSubscriberBuilder`],
//! [`crate::CountingSubscriberBuilder`], [`crate::warn_counting_subscriber`] and
//! their wrappers). A hand-rolled subscriber bypasses
//! [`crate::prime_tracing_callsite_cache`] and re-opens the intermittent
//! zero-event flake that tasks 6273 and 5624 closed. That function's docs are
//! the authoritative account of the mechanism.
//!
//! # Scope: the whole workspace, production code included
//!
//! The ratchet scans every `.rs` file, `src/` as well as `tests/`. No path
//! rule can tell test code from production code, because in-crate
//! `#[cfg(test)] mod tests` blocks live under `src/`. Covering production code
//! is also deliberate: a `tracing_subscriber` global default installed in any
//! process that runs tests breaks priming's "nothing else installs a global
//! default" precondition. Production logging setup that never runs in a test
//! process, such as a binary's `main`, opts out one line at a time with a
//! trailing `// tracing-hygiene:allow — <reason>` comment. Test code never
//! opts out; it extends the constructors instead.
//!
//! The scanner and collector here are pure: which directories may define a
//! subscriber is policy, and it lives in this module's workspace ratchet test.

use crate::ignore_hygiene::walk_rs_files;
use std::fmt;
use std::path::{Path, PathBuf};

/// Matches every rustfmt-produced `Subscriber` trait-impl header, including a
/// split generic list whose `> Trait for Type` tail stays on one line.
const TRAIT_IMPL_NEEDLE: &str = "Subscriber for ";

/// Matches any use of the `tracing-subscriber` crate, whose subscribers never prime.
const TRACING_SUBSCRIBER_CRATE_NEEDLE: &str = "tracing_subscriber::";

/// A line carrying this marker is exempt. It is reserved for production
/// logging setup; see the module docs.
const ALLOW_MARKER: &str = "tracing-hygiene:allow";

/// Which hand-rolled subscriber construct a source line carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriberConstruct {
    /// An `impl ... Subscriber for ...` header.
    TraitImpl,
    /// A `tracing_subscriber::` path.
    TracingSubscriberCrate,
}

/// One hand-rolled subscriber construct found in a single source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandRolledSubscriberSite {
    /// 1-based line number.
    pub line: usize,
    pub construct: SubscriberConstruct,
    /// The trimmed source line.
    pub text: String,
}

/// A [`HandRolledSubscriberSite`] located in a workspace file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSubscriberSite {
    /// Relative to the workspace root passed to
    /// [`collect_workspace_hand_rolled_subscriber_sites`].
    pub path: PathBuf,
    pub site: HandRolledSubscriberSite,
}

impl fmt::Display for WorkspaceSubscriberSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {:?}: {}",
            self.path.display(),
            self.site.line,
            self.site.construct,
            self.site.text
        )
    }
}

/// Scan one Rust source text for hand-rolled subscriber constructs, returning
/// every hit in source order.
///
/// Whole-line `//` comments (`///`, `//!` and regular `//`) are skipped: a
/// commented-out subscriber has no runtime consequence. Lines carrying the
/// `tracing-hygiene:allow` marker are skipped too. Known limits of this
/// line-oriented scan: `/* */` block comments and string literals are not
/// excluded, and any other trailing `// ...` on a code line is still scanned.
pub fn find_hand_rolled_subscriber_sites(source: &str) -> Vec<HandRolledSubscriberSite> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .filter(|(_, line)| !line.contains(ALLOW_MARKER))
        .filter_map(|(idx, line)| {
            let construct = if line.contains(TRAIT_IMPL_NEEDLE) {
                SubscriberConstruct::TraitImpl
            } else if line.contains(TRACING_SUBSCRIBER_CRATE_NEEDLE) {
                SubscriberConstruct::TracingSubscriberCrate
            } else {
                return None;
            };
            Some(HandRolledSubscriberSite {
                line: idx + 1,
                construct,
                text: line.trim().to_owned(),
            })
        })
        .collect()
}

/// Walk `workspace_root` with [`walk_rs_files`] under `include`, and collect
/// every hand-rolled subscriber site, sorted by `(path, line)`.
///
/// Per-file I/O errors are silently skipped, the same policy as
/// [`crate::temp_dirs::collect_workspace_unguarded_temp_dirs`]: a file deleted
/// mid-walk by a concurrent build must not become a spurious site.
pub fn collect_workspace_hand_rolled_subscriber_sites(
    workspace_root: &Path,
    include: impl Fn(&Path) -> bool,
) -> Vec<WorkspaceSubscriberSite> {
    let mut sites: Vec<WorkspaceSubscriberSite> = walk_rs_files(workspace_root, include)
        .into_iter()
        .filter_map(|path| {
            std::fs::read_to_string(&path)
                .ok()
                .map(|source| (path, source))
        })
        .flat_map(|(path, source)| {
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .to_path_buf();
            find_hand_rolled_subscriber_sites(&source)
                .into_iter()
                .map(move |site| WorkspaceSubscriberSite {
                    path: rel.clone(),
                    site,
                })
        })
        .collect();
    sites.sort_by(|a, b| (&a.path, a.site.line).cmp(&(&b.path, b.site.line)));
    sites
}

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
    fn skips_only_the_lines_carrying_the_allow_marker() {
        let source = "fn main() {\n\
                      \x20   tracing_subscriber::fmt().init(); // tracing-hygiene:allow — CLI logging\n\
                      \x20   tracing_subscriber::fmt().init();\n\
                      }\n";

        assert_eq!(
            find_hand_rolled_subscriber_sites(source)
                .iter()
                .map(|site| site.line)
                .collect::<Vec<_>>(),
            vec![3],
        );
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
        write(
            "a/src/lib.rs",
            "struct X;\nimpl tracing::Subscriber for X {}\n",
        );
        write(
            "b/tests/t.rs",
            "struct Y;\nimpl tracing::Subscriber for Y {}\n",
        );
        write("c/src/clean.rs", "fn clean() {}\n");

        let filtered =
            collect_workspace_hand_rolled_subscriber_sites(root, |rel| !rel.starts_with("b"));
        assert_eq!(
            filtered.len(),
            1,
            "expected exactly one site, got {filtered:?}"
        );
        assert_eq!(filtered[0].path, PathBuf::from("a/src/lib.rs"));
        assert_eq!(filtered[0].site.line, 2);
        assert_eq!(filtered[0].site.construct, SubscriberConstruct::TraitImpl);

        let everything = collect_workspace_hand_rolled_subscriber_sites(root, |_| true);
        assert_eq!(
            everything.len(),
            2,
            "expected two sites, got {everything:?}"
        );
        assert_eq!(
            everything
                .iter()
                .map(|site| site.path.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([PathBuf::from("a/src/lib.rs"), PathBuf::from("b/tests/t.rs")]),
        );
    }

    /// The crate that owns [`crate::prime_tracing_callsite_cache`], and the
    /// only place allowed to define subscribers. Its constructors prime, and
    /// its own tests (the `*_callsite_race.rs` guards,
    /// `priming_lost_race_diagnostic.rs`'s `Competing`, `tracing_support.rs`'s
    /// `ForwardingSubscriber`) hand-roll deliberately.
    const SUBSCRIBER_OWNER: &str = "crates/reify-test-support";

    /// A GUI-crate file that installs tracing subscribers. Walking it proves
    /// the sweep reaches `gui/src-tauri`, which sits outside `crates/`.
    const REACH_SENTINEL: &str = "gui/src-tauri/src/tests/engine_tests.rs";

    #[test]
    fn workspace_tracing_subscribers_are_built_only_by_reify_test_support() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/reify-test-support has a parent (crates/)")
            .parent()
            .expect("crates/ has a parent (the repo root)");

        crate::workspace_sweep::assert_workspace_rs_walk_is_healthy(repo_root, REACH_SENTINEL);

        let (owner_sites, enforced): (Vec<_>, Vec<_>) =
            collect_workspace_hand_rolled_subscriber_sites(repo_root, |_| true)
                .into_iter()
                .partition(|found| found.path.starts_with(SUBSCRIBER_OWNER));

        let owner_impl_file = Path::new(SUBSCRIBER_OWNER).join("src/tracing_support.rs");
        assert!(
            owner_sites.iter().any(|found| found.path == owner_impl_file
                && found.site.construct == SubscriberConstruct::TraitImpl),
            "positive control failed: the scanner found no TraitImpl site in \
             {owner_impl_file:?}, which defines real subscribers — the scanner \
             no longer recognises the real-world impl shape, so a clean \
             enforced sweep below would be vacuous. Owner sites found: {owner_sites:?}"
        );

        assert!(
            enforced.is_empty(),
            "Found {} hand-rolled tracing subscriber construct(s) outside {SUBSCRIBER_OWNER}:\n  {}\n\n\
             In test code, build the subscriber with reify_test_support's \
             CapturingSubscriberBuilder / CountingSubscriberBuilder / \
             warn_capturing_subscriber / warn_counting_subscriber (/ \
             warn_counting_guard), which prime tracing's process-global \
             callsite-Interest cache; see prime_tracing_callsite_cache. If \
             none fits, extend them in \
             crates/reify-test-support/src/tracing_support.rs rather than \
             hand-rolling.\n\n\
             Production logging setup that never runs in a test process (a \
             binary's main) may instead end each such line with \
             `// {ALLOW_MARKER} — <reason>`; see the \
             reify_test_support::tracing_hygiene module docs.",
            enforced.len(),
            enforced
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }
}
