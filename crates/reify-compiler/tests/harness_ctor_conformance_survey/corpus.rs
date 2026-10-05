//! What the survey sweeps: the two corpus halves enumerated from the git index,
//! their broken-enumeration floors, and the parity gate between them.

use reify_test_support::rust_fixture_scan;

use crate::workspace_git::{WORKSPACE_ROOT, git_at_workspace_root, git_is_available};

/// `Some(corpus)` when git can be spawned, `None` when it cannot.
///
/// The gate-resident corpus probes below call this and return early on `None`;
/// the `#[ignore]`d generator calls [`tracked_ri_corpus`] directly, because a
/// generator that cannot enumerate the corpus must fail loudly rather than
/// write a falsely-clean artifact.
fn tracked_ri_corpus_if_git_available() -> Option<&'static [String]> {
    git_is_available().then(tracked_ri_corpus)
}

// ─── step 1/2: corpus enumeration ────────────────────────────────────────────

/// Every TRACKED `.ri` file in the repository, as repo-relative
/// forward-slash paths, sorted and deduplicated.
///
/// Goes through [`scan_tracked_corpus`] — `git ls-files -z` at the workspace
/// root — rather than walking the filesystem, for three reasons:
///
/// 1. The task defines the corpus as "all **tracked** `.ri`", and both the PRD
///    and the capability manifest cite `git ls-files '*.ri'` as the enumerating
///    command — so the survey's denominator is identical to the one the PRD
///    gate reasons about.
/// 2. A filesystem walk would have to exclude `target/` and every other
///    gitignored tree by hand, and would drift from that definition; a
///    build-artifact `.ri` could silently enter the survey.
/// 3. `-z` / NUL splitting means a path containing a space or a newline cannot
///    corrupt the list.
///
/// Panics if git is unavailable or exits non-zero. A silently-empty corpus
/// would render a falsely-clean survey, which is the one failure mode this
/// artifact must never have.
///
/// Enumerated ONCE per process, behind the same `OnceLock` that
/// `stdlib_structure_defs` and `fea_owned_defs` use: the tracked corpus
/// cannot change while the test binary runs, and the gate-resident probes
/// below plus the generator would otherwise spawn a `git ls-files` subprocess
/// and re-sort ~700 paths each time.
pub(crate) fn tracked_ri_corpus() -> &'static [String] {
    static CORPUS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CORPUS.get_or_init(scan_tracked_ri_corpus)
}

/// The uncached enumeration behind [`tracked_ri_corpus`].
fn scan_tracked_ri_corpus() -> Vec<String> {
    scan_tracked_corpus("*.ri")
}

/// A BROKEN-ENUMERATION floor for the `.ri` half, deliberately far below the
/// live count (701 measured 2026-09-16) rather than just under it.
///
/// A constant rather than a literal at the assertion site because
/// [`CorpusHalf::floor`] reads it too: the gate-resident probe and the
/// corpus-parity gate must red at the SAME threshold, or one of them is
/// describing a corpus the other would accept.
const RI_CORPUS_FLOOR: usize = 100;

/// A BROKEN-ENUMERATION floor for the Rust-host half, deliberately far below
/// the live count (1,870 of 1,932 tracked `.rs`, measured 2026-09-16 — every
/// one under `crates/`, the 62 exclusions being the two out-of-scope roots).
///
/// Same reasoning as the `.ri` floor below: it catches a wrong root, a wrong
/// pathspec or a silent git failure, and must NOT red the merge gate when a
/// test-consolidation task legitimately deletes a few hundred host files. The
/// artifact header carries the live count.
///
/// What this floor structurally CANNOT catch is a NARROWED host predicate: the
/// file-NAME scope this half started with admitted 1,307 of the same 1,932
/// tracked files and cleared 300 exactly as comfortably as the correct scope
/// does. A narrowing is caught instead by
/// [`rust_fixture_scan::is_inline_fixture_host`]'s own path-shape contract and
/// by the per-[`HostShape`] live members required of
/// [`tracked_rust_hosts_reach_the_named_site_host_and_every_shape`] and
/// `INLINE_FIXTURE_PINNED_HOSTS`.
const RUST_HOST_CORPUS_FLOOR: usize = 300;

/// Every tracked `*.rs` that can host an inline Reify fixture, as repo-relative
/// forward-slash paths, sorted and deduplicated — the second corpus half
/// (task #7543).
///
/// The `.ri` half enumerates FILES whose whole content is Reify; this half
/// enumerates files that may CARRY Reify inside a raw-string literal, which
/// `git ls-files -- '*.ri'` cannot reach. That is every tracked `.rs` under
/// `crates/`, not a test-file subset: a production `src/*.rs` with a
/// `#[cfg(test)] mod tests` carries fixtures like any other, and deciding
/// scope by file NAME is what once hid 563 of these hosts. Both halves come
/// from the same primitive, and both are cached behind the same `OnceLock` for
/// the same reason [`tracked_ri_corpus`] is.
pub(crate) fn tracked_rust_hosts() -> &'static [String] {
    static HOSTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    HOSTS.get_or_init(scan_tracked_rust_hosts)
}

/// The uncached enumeration behind [`tracked_rust_hosts`].
///
/// The same git-index primitive as the `.ri` half, filtered by the SHARED
/// [`rust_fixture_scan::is_inline_fixture_host`] predicate — the one place that
/// decides what an in-scope host is, so the enumeration and the walker cannot
/// disagree. The filter is a path-shape test only; whether a host actually
/// carries Reify is decided per literal, at extraction. The primitive's
/// non-empty assertion covers a broken `*.rs` enumeration; a post-FILTER collapse (every host rejected) is the
/// corpus-parity gate's business, which runs before any sweep.
fn scan_tracked_rust_hosts() -> Vec<String> {
    scan_tracked_corpus("*.rs")
        .into_iter()
        .filter(|p| rust_fixture_scan::is_inline_fixture_host(std::path::Path::new(p)))
        .collect()
}

/// The SINGLE git-index primitive both corpus halves are enumerated through.
///
/// Routing both halves through one primitive is what makes the corpus-parity
/// gate's shared floor structural: neither half can acquire its own
/// enumeration strategy, its own sort order, or its own failure policy.
///
/// A filesystem walk was rejected for the same reason `git ls-files` was chosen
/// for the `.ri` half: it would admit UNTRACKED files, which no commit
/// reproduces — and the artifact is stamped against a commit.
///
/// Panics if git is unavailable, exits non-zero, or reports nothing. A
/// silently-empty corpus would render a falsely-clean survey, which is the one
/// failure mode this artifact must never have.
fn scan_tracked_corpus(pathspec: &str) -> Vec<String> {
    let out = git_at_workspace_root(&["ls-files", "-z", "--", pathspec])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot run `git ls-files` in {WORKSPACE_ROOT}: {e}"
            )
        });
    assert!(
        out.status.success(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '{pathspec}'` in {WORKSPACE_ROOT} \
         exited {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );

    let stdout = String::from_utf8(out.stdout)
        .expect("ctor_conformance_corpus_survey: `git ls-files` emitted non-UTF-8 paths");
    let mut paths: Vec<String> = stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    assert!(
        !paths.is_empty(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '{pathspec}'` returned nothing in \
         {WORKSPACE_ROOT} — a silently-empty corpus would render a falsely-clean survey"
    );
    paths
}

/// `Some(hosts)` when git can be spawned, `None` when it cannot — the
/// [`tracked_ri_corpus_if_git_available`] idiom for the second half.
fn tracked_rust_hosts_if_git_available() -> Option<&'static [String]> {
    git_is_available().then(tracked_rust_hosts)
}

#[test]
fn tracked_rust_hosts_clears_the_broken_enumeration_floor() {
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    assert!(
        hosts.len() >= RUST_HOST_CORPUS_FLOOR,
        "tracked Rust host corpus must have >= {RUST_HOST_CORPUS_FLOOR} entries — a floor \
         that catches a BROKEN enumeration (wrong root, wrong pathspec, silent git failure), \
         not a legitimate shrink; the artifact header carries the live count. Got {}",
        hosts.len()
    );
}

#[test]
fn tracked_rust_hosts_entries_all_end_in_dot_rs() {
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let bad: Vec<&String> = hosts.iter().filter(|p| !p.ends_with(".rs")).collect();
    assert!(
        bad.is_empty(),
        "every host entry must end in '.rs', got {} that do not: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn tracked_rust_hosts_all_satisfy_the_shared_host_predicate() {
    // The enumeration and the walker must agree on what an in-scope host IS,
    // and `is_inline_fixture_host` is the one place that decides — so this
    // asserts the filter was actually applied, not merely declared.
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let bad: Vec<&String> = hosts
        .iter()
        .filter(|p| !rust_fixture_scan::is_inline_fixture_host(std::path::Path::new(p)))
        .collect();
    assert!(
        bad.is_empty(),
        "every host entry must satisfy `rust_fixture_scan::is_inline_fixture_host`, \
         got {} that do not: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn tracked_rust_hosts_is_sorted_and_deduplicated() {
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let mut expected = hosts.to_vec();
    expected.sort();
    expected.dedup();
    assert_eq!(
        hosts,
        expected.as_slice(),
        "tracked_rust_hosts must return a sorted, deduplicated list"
    );
}

#[test]
fn tracked_rust_hosts_paths_are_repo_relative_forward_slash() {
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    for p in hosts {
        assert!(
            !p.starts_with('/') && !p.starts_with("./") && !p.contains('\\'),
            "host entries must be repo-relative forward-slash paths, got {p:?}"
        );
    }
}

#[test]
fn tracked_rust_hosts_reach_the_named_site_host_and_every_shape() {
    let Some(hosts) = tracked_rust_hosts_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    assert!(
        hosts.iter().any(|p| p == NAMED_SITE_HOST),
        "the host of the two sites #7543's VERIFY criterion names must be enumerated"
    );
    // The LIVE counterpart to `inline_fixture_pinned_hosts_name_all_four_
    // enumeration_shapes`: that one pins the pin list, this one pins what the
    // git-index enumeration actually returns, so a re-narrowed host predicate
    // reds here even if the pin list is left alone.
    for shape in <HostShape as strum::IntoEnumIterator>::iter() {
        assert!(
            hosts.iter().any(|p| host_shape(p) == shape),
            "the {shape:?} host shape must be enumerated; {} hosts enumerated",
            hosts.len()
        );
    }
}

#[test]
fn tracked_rust_hosts_is_disjoint_from_the_ri_corpus() {
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }
    let ri: std::collections::BTreeSet<&String> = tracked_ri_corpus().iter().collect();
    let overlap: Vec<&String> = tracked_rust_hosts()
        .iter()
        .filter(|p| ri.contains(*p))
        .collect();
    assert!(
        overlap.is_empty(),
        "the two corpus halves must be disjoint — a member surveyed twice would \
         be double-counted in the artifact; got {overlap:?}"
    );
}

#[test]
fn both_corpus_halves_come_from_the_same_git_primitive() {
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }
    // Exercising the generalized seam with each pathspec is what makes the
    // parity gate's shared floor STRUCTURAL: neither half can acquire its own
    // enumeration strategy without this failing.
    assert_eq!(
        scan_tracked_corpus("*.ri"),
        tracked_ri_corpus(),
        "the `.ri` half must be `scan_tracked_corpus(\"*.ri\")` verbatim"
    );
    let hosts_via_seam: Vec<String> = scan_tracked_corpus("*.rs")
        .into_iter()
        .filter(|p| rust_fixture_scan::is_inline_fixture_host(std::path::Path::new(p)))
        .collect();
    assert_eq!(
        hosts_via_seam,
        tracked_rust_hosts(),
        "the host half must be `scan_tracked_corpus(\"*.rs\")` filtered through \
         the shared host predicate, and nothing else"
    );
}

#[test]
fn tracked_ri_corpus_clears_the_broken_enumeration_floor() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    // Why the floor is where it is: see `RI_CORPUS_FLOOR`. The corpus is
    // expected to churn in BOTH directions, and a fixture-consolidation task
    // that legitimately deletes a few dozen `.ri` has nothing to do with this
    // survey and must not red the merge gate with a message that reads like a
    // defect.
    //
    // The NAME is scoped to exactly that floor and no further. An earlier name
    // ("…is_non_empty_and_covers_the_whole_tracked_tree") also claimed the
    // COVERAGE half, which is asserted in a different test entirely
    // (`tracked_ri_corpus_reaches_outside_examples`) — so a reader scanning the
    // test list, or triaging a red, was told this test proved something it did
    // not. The test name is what shows up in `cargo test` output; coverage
    // ownership stays with the test that actually asserts it. The live count
    // belongs in the artifact this module generates, which states it as a
    // measured header field.
    assert!(
        corpus.len() >= RI_CORPUS_FLOOR,
        "tracked .ri corpus must have >= {RI_CORPUS_FLOOR} entries — a floor that catches a \
         BROKEN enumeration (wrong root, wrong pathspec, silent git failure), not a legitimate \
         shrink; the artifact header carries the live count. Got {}",
        corpus.len()
    );
}

#[test]
fn tracked_ri_corpus_entries_all_end_in_dot_ri() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let bad: Vec<&String> = corpus.iter().filter(|p| !p.ends_with(".ri")).collect();
    assert!(
        bad.is_empty(),
        "every corpus entry must end in '.ri', got {} that do not: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn tracked_ri_corpus_is_sorted_and_deduplicated() {
    // Determinism: the artifact must be byte-reproducible, which requires the
    // enumeration itself to be a total order with no repeats.
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let mut expected = corpus.to_vec();
    expected.sort();
    expected.dedup();
    assert_eq!(
        corpus,
        expected.as_slice(),
        "tracked_ri_corpus must return a sorted, deduplicated list"
    );
}

// DELIBERATELY ABSENT: a gate-resident "every corpus entry resolves to an
// existing file on disk" probe.
//
// That is a property of the WORKING TREE, not of anything this module decides.
// `git ls-files` reports the INDEX, so an engineer who has `rm`'d a tracked
// `.ri` locally, or is mid-`git mv`, without staging the deletion would get a
// red `reify-compiler` suite pointing at the survey module with no connection to
// what they were doing. The three probes that remain
// (`…_all_end_in_dot_ri`, `…_is_sorted_and_deduplicated`,
// `…_paths_are_repo_relative_forward_slash`) are pure properties of the
// enumeration and carry no such coupling.
//
// The behaviour that actually matters when a member is missing is that the
// sweep RECORDS it rather than dying, and that IS gate-resident: see
// `survey_corpus_records_a_read_error_rather_than_panicking` and
// `survey_corpus_records_unsurveyable_members_instead_of_dropping_them`. Each
// unreadable member lands in `SurveyRun::not_surveyed` with reason
// `read-error`, and the rendered artifact lists it by name — which is the
// honest disclosure a stale tree deserves, not a merge-gate red.

#[test]
fn tracked_ri_corpus_reaches_outside_examples() {
    // The landed `discover_ri_files()` walk is rooted at `examples/` and would
    // miss ~399 of the ~660 tracked files. Widening the root IS β.
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    assert!(
        corpus
            .iter()
            .any(|p| p.starts_with("crates/reify-compiler/stdlib/")),
        "corpus must include stdlib members (β is not examples-scoped); \
         first 5 entries: {:?}",
        &corpus[..corpus.len().min(5)]
    );
    assert!(
        corpus.iter().any(|p| p.starts_with("examples/")),
        "corpus must still include the examples/ tree"
    );
    let non_examples = corpus
        .iter()
        .filter(|p| !p.starts_with("examples/"))
        .count();
    // Same reasoning as the corpus floor above: the load-bearing assertions are
    // the two structural `starts_with` probes, which hold at any size. This
    // number only has to be large enough to catch an enumeration that collapsed
    // back to the examples-scoped walk β exists to widen (399 measured at plan
    // time), and small enough that a legitimate fixture cull is not a merge-gate
    // red.
    assert!(
        non_examples >= 50,
        "the non-examples half is the point of β — a collapse back to the \
         examples-scoped walk must red, a legitimate fixture cull must not \
         (399 measured at plan time), got {non_examples}"
    );
}

#[test]
fn tracked_ri_corpus_paths_are_repo_relative_forward_slash() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    for p in corpus {
        assert!(
            !p.starts_with('/') && !p.starts_with("./") && !p.contains('\\'),
            "corpus entries must be repo-relative forward-slash paths, got {p:?}"
        );
    }
}

// ─── corpus parity: neither half may narrow without the gate seeing it ───────

/// The two enumerated corpus halves the survey sweeps.
///
/// `EnumIter` is load-bearing exactly as it is on `Owner`: [`corpus_parity`]
/// iterates the DECLARATION rather than a local literal, so a future third
/// half cannot be added to the survey and left silently unwired in the gate.
/// `strum` is already a `[dev-dependencies]` entry of this crate, so this costs
/// no new dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
pub(crate) enum CorpusHalf {
    /// Tracked `.ri` files, whose whole content is Reify source.
    TrackedRi,
    /// Tracked `.rs` files that may CARRY Reify inside a raw-string literal.
    InlineRustHost,
}

impl CorpusHalf {
    /// This half's BROKEN-ENUMERATION floor.
    pub(crate) fn floor(self) -> usize {
        match self {
            CorpusHalf::TrackedRi => RI_CORPUS_FLOOR,
            CorpusHalf::InlineRustHost => RUST_HOST_CORPUS_FLOOR,
        }
    }

    /// The file extension every member of this half must carry.
    fn extension(self) -> &'static str {
        match self {
            CorpusHalf::TrackedRi => ".ri",
            CorpusHalf::InlineRustHost => ".rs",
        }
    }

    /// How this half is named in a parity failure and in the artifact header.
    pub(crate) fn label(self) -> &'static str {
        match self {
            CorpusHalf::TrackedRi => "tracked .ri corpus",
            CorpusHalf::InlineRustHost => "inline Rust fixture hosts",
        }
    }
}

/// Whether every declared corpus half is enumerated well enough to sweep.
///
/// A PURE function over the two enumerations, taken as data: that is what lets
/// the gate be exercised with a half stubbed to the empty set without stubbing
/// anything global, and it is what the generator calls BEFORE any sweep so a
/// narrowed walker fails loudly instead of writing a falsely-thin artifact.
///
/// Checks, in order: every declared [`CorpusHalf`] present in the input; each
/// half at or above its floor; every member carrying its half's extension; and
/// the halves pairwise disjoint. EVERY violation is accumulated into one
/// message rather than returning the first — the same
/// both-directions-reported-together convention
/// `assert_no_unwaived_ctor_conformance_sites` uses, and for the same
/// reason: a caller who fixes the first complaint and re-runs should not
/// discover the second one turn later.
///
/// The floors are BROKEN-ENUMERATION floors (see [`RI_CORPUS_FLOOR`] and
/// [`RUST_HOST_CORPUS_FLOOR`]), set far below the live counts — 701 `.ri` and
/// 1,870 hosts measured 2026-09-16 — and not tracking numbers. A legitimate
/// fixture or test cull must not be a merge-gate red; only an enumeration that
/// broke can get near them. A floor cannot see a NARROWED host predicate at
/// all — see [`RUST_HOST_CORPUS_FLOOR`] for what does.
pub(crate) fn corpus_parity(halves: &[(CorpusHalf, &[String])]) -> Result<(), String> {
    use strum::IntoEnumIterator;
    let mut violations: Vec<String> = Vec::new();

    for declared in CorpusHalf::iter() {
        if !halves.iter().any(|(half, _)| *half == declared) {
            violations.push(format!(
                "{} is declared but absent from the parity input — it would be swept \
                 and rendered without ever being checked",
                declared.label()
            ));
        }
    }

    for (half, members) in halves {
        if members.len() < half.floor() {
            violations.push(format!(
                "{} holds {} member(s), below its broken-enumeration floor of {}",
                half.label(),
                members.len(),
                half.floor()
            ));
        }
        let foreign: Vec<&String> = members
            .iter()
            .filter(|p| !p.ends_with(half.extension()))
            .collect();
        if !foreign.is_empty() {
            violations.push(format!(
                "{} holds {} member(s) not ending in '{}' — a walker that widened into \
                 the wrong pathspec: {:?}",
                half.label(),
                foreign.len(),
                half.extension(),
                &foreign[..foreign.len().min(5)]
            ));
        }
    }

    for (i, (a, a_members)) in halves.iter().enumerate() {
        for (b, b_members) in halves.iter().skip(i + 1) {
            let seen: std::collections::BTreeSet<&String> = a_members.iter().collect();
            let shared: Vec<&String> = b_members.iter().filter(|p| seen.contains(*p)).collect();
            if !shared.is_empty() {
                violations.push(format!(
                    "{} and {} both enumerate {} member(s), which would be surveyed and \
                     counted twice: {:?}",
                    a.label(),
                    b.label(),
                    shared.len(),
                    &shared[..shared.len().min(5)]
                ));
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("; "))
    }
}

/// A well-formed input for every declared [`CorpusHalf`], derived from the enum
/// rather than written out — so a future third half is covered here with no
/// edit, the same reason `Owner::render_order` is derived.
#[cfg(test)]
fn synthetic_halves() -> Vec<(CorpusHalf, Vec<String>)> {
    use strum::IntoEnumIterator;
    CorpusHalf::iter()
        .map(|half| {
            let members = (0..half.floor() + 5)
                .map(|i| format!("synthetic/{half:?}/f{i}{}", half.extension()))
                .collect();
            (half, members)
        })
        .collect()
}

#[cfg(test)]
fn as_parity_input(halves: &[(CorpusHalf, Vec<String>)]) -> Vec<(CorpusHalf, &[String])> {
    halves.iter().map(|(h, m)| (*h, m.as_slice())).collect()
}

#[test]
fn corpus_parity_accepts_two_well_formed_halves() {
    let halves = synthetic_halves();
    assert_eq!(
        corpus_parity(&as_parity_input(&halves)),
        Ok(()),
        "adequately-sized, correctly-extensioned, disjoint halves must pass"
    );
}

#[test]
fn corpus_parity_reds_when_any_half_is_stubbed_to_the_empty_set() {
    use strum::IntoEnumIterator;
    // VERIFY criterion (b) verbatim — and it must hold for EITHER half stubbed,
    // not just the new one, which is why this iterates the enum.
    for stubbed in CorpusHalf::iter() {
        let mut halves = synthetic_halves();
        for (half, members) in halves.iter_mut() {
            if *half == stubbed {
                members.clear();
            }
        }
        let err = corpus_parity(&as_parity_input(&halves))
            .expect_err(&format!("an empty {stubbed:?} half must red the gate"));
        assert!(
            err.contains(stubbed.label()),
            "the failure must NAME the half that collapsed; {stubbed:?} gave {err:?}"
        );
    }
}

#[test]
fn corpus_parity_reds_on_a_half_below_its_floor() {
    use strum::IntoEnumIterator;
    for narrowed in CorpusHalf::iter() {
        let mut halves = synthetic_halves();
        for (half, members) in halves.iter_mut() {
            if *half == narrowed {
                members.truncate(narrowed.floor() - 1);
            }
        }
        let err = corpus_parity(&as_parity_input(&halves)).expect_err(&format!(
            "a below-floor {narrowed:?} half must red the gate"
        ));
        assert!(
            err.contains(narrowed.label()),
            "a non-empty but narrowed half must be named too; {narrowed:?} gave {err:?}"
        );
    }
}

#[test]
fn corpus_parity_reds_when_a_member_carries_the_other_halfs_extension() {
    use strum::IntoEnumIterator;
    // The shape of a walker that widened into the wrong pathspec.
    for wrong in CorpusHalf::iter() {
        let foreign = CorpusHalf::iter()
            .find(|h| h.extension() != wrong.extension())
            .expect("at least two halves with distinct extensions");
        let mut halves = synthetic_halves();
        for (half, members) in halves.iter_mut() {
            if *half == wrong {
                members[0] = format!("synthetic/intruder{}", foreign.extension());
            }
        }
        let err = corpus_parity(&as_parity_input(&halves))
            .expect_err(&format!("a foreign-extension member in {wrong:?} must red"));
        assert!(
            err.contains(wrong.label()) && err.contains(wrong.extension()),
            "the failure must name the half and the extension it broke; got {err:?}"
        );
    }
}

#[test]
fn corpus_parity_reds_when_the_halves_overlap() {
    let mut halves = synthetic_halves();
    let shared = halves[0].1[0].clone();
    halves[1].1.push(shared.clone());
    let err = corpus_parity(&as_parity_input(&halves))
        .expect_err("a member enumerated into both halves must red the gate");
    // The intruder necessarily also breaks the second half's extension rule;
    // `corpus_parity` accumulates every violation, so both are reported and
    // this assertion can still name the overlap specifically.
    assert!(
        err.contains(&shared),
        "the failure must name the doubly-enumerated member; got {err:?}"
    );
}

#[test]
fn corpus_parity_reds_when_a_declared_half_is_absent_from_the_input() {
    use strum::IntoEnumIterator;
    // Derived from `CorpusHalf::iter()`, never a local literal — the same
    // failure mode `Owner::render_order` guards against, for the same reason: a
    // future third half added to the enum and forgotten at the wiring site
    // would otherwise be swept, rendered and never parity-checked.
    for omitted in CorpusHalf::iter() {
        let halves = synthetic_halves();
        let input: Vec<(CorpusHalf, &[String])> = halves
            .iter()
            .filter(|(half, _)| *half != omitted)
            .map(|(half, members)| (*half, members.as_slice()))
            .collect();
        let err = corpus_parity(&input)
            .expect_err(&format!("an unwired {omitted:?} half must red the gate"));
        assert!(
            err.contains(omitted.label()),
            "the failure must name the half nobody wired; got {err:?}"
        );
    }
}

#[test]
fn corpus_parity_holds_over_the_two_live_enumerations() {
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }
    assert_eq!(
        corpus_parity(&[
            (CorpusHalf::TrackedRi, tracked_ri_corpus()),
            (CorpusHalf::InlineRustHost, tracked_rust_hosts()),
        ]),
        Ok(()),
        "the live wiring must satisfy the gate it is checked by"
    );
}

// ─── the inline half's named site host, and the path shapes hosts take ───────

/// The Rust test host that carried the inline sites task #7543's VERIFY
/// criterion names, until δ fixed them in f247bade44
/// (`PRE_DELTA_PURPOSE_FIXTURES_HOST` keeps a verbatim pre-δ copy).
///
/// A named constant rather than a literal in two places: the inline pin
/// `INLINE_FIXTURE_PINNED_HOSTS` lists it
/// and `inline_fixture_pinned_hosts_name_all_four_enumeration_shapes` requires
/// it,
/// and a pin that drifted from its own requirement would still pass.
pub(crate) const NAMED_SITE_HOST: &str =
    "crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs";

/// The four PATH SHAPES an in-scope host can take, as a total classification.
///
/// Total and mutually exclusive, so "the pin names a live member of each" is a
/// statement about the whole scope rather than about a hand-picked subset.
/// Only the first is reachable by `reify_test_support::ignore_hygiene::
/// walk_test_rs_files`, and only the first two survived the host predicate's
/// original `tests`-directory-or-`tests.rs` clause — which is why a
/// re-narrowing shows up HERE and nowhere else: the corpus-parity floor is
/// cleared just as comfortably by a predicate admitting 1,307 of the 1,932
/// tracked `.rs` as by the one admitting all 1,870 under `crates/`.
///
/// `EnumIter` is load-bearing for the same reason it is on [`CorpusHalf`]: both
/// shape gates iterate the DECLARATION, so a fifth shape cannot be added here
/// and left unpinned by a hand-maintained list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
pub(crate) enum HostShape {
    TestsDirectory,
    SrcTestsRs,
    SrcStarTestsRs,
    ProductionSrc,
}

pub(crate) fn host_shape(rel: &str) -> HostShape {
    let path = std::path::Path::new(rel);
    let file = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    if path.components().any(|c| c.as_os_str() == "tests") {
        HostShape::TestsDirectory
    } else if file == "tests.rs" {
        HostShape::SrcTestsRs
    } else if file.ends_with("_tests.rs") {
        HostShape::SrcStarTestsRs
    } else {
        HostShape::ProductionSrc
    }
}
