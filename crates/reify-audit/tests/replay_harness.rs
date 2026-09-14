//! Pins for `common::git_env::libtest_summary_count` — the single parser every
//! replay child's libtest summary is read through.
//!
//! Its readers are spread across binaries: the replay harness's own
//! non-vacuity checks, reached from `cli.rs`, `real_git_ops.rs`,
//! `ptodo_baseline.rs` and `g_allow.rs`, plus `g_allow.rs`'s deprived-`PATH`
//! and refusal fixtures. A bug in the parser surfaces at each of them as a
//! count mismatch attributed to the child, so the pins live in their own binary
//! rather than beside one reader — retiring that reader would otherwise take
//! the parser's only coverage with it and leave the rest unpinned.
//!
//! Pinned over literal summary lines rather than a spawned child, since these
//! are pure-function properties. The fixtures carry the trailing `finished in`
//! segment this toolchain's libtest actually appends, so they drive the exact
//! shape the real call sites read.

mod common;

use common::git_env::{SummaryField, libtest_summary_count};

/// Each field is read as the number belonging to that field, in both of
/// libtest's verdict shapes.
///
/// There is no "unknown field" case to pin: the field is a [`SummaryField`],
/// so a misspelling no longer type-checks and cannot reach this parser at all.
#[test]
fn libtest_summary_count_reads_the_field_it_was_asked_for() {
    const OK: &str = "test result: ok. 5 passed; 0 failed; 2 ignored; 0 measured; \
                      30 filtered out; finished in 0.01s";
    const FAILED: &str = "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; \
                          3 filtered out; finished in 0.01s";

    // Both verdicts parse: a reader asserting on a FAILING child reads the
    // `FAILED.` shape, which differs from `ok.` before the counts.
    assert_eq!(libtest_summary_count(OK, SummaryField::Passed), Some(5));
    assert_eq!(libtest_summary_count(OK, SummaryField::Failed), Some(0));
    assert_eq!(libtest_summary_count(OK, SummaryField::Ignored), Some(2));
    assert_eq!(libtest_summary_count(FAILED, SummaryField::Passed), Some(0));
    assert_eq!(libtest_summary_count(FAILED, SummaryField::Failed), Some(1));

    // Parsed as a NUMBER: a substring match on `"1 passed"` would read this as
    // `Some(1)` too, silently greening every call site that compares against
    // that.
    const TWENTY_ONE: &str = "test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; \
                              0 filtered out; finished in 0.01s";
    assert_eq!(
        libtest_summary_count(TWENTY_ONE, SummaryField::Passed),
        Some(21)
    );

    // No summary at all: `None`, which the replay turns into its "could not
    // find libtest's `test result:` summary" panic. Now the ONLY cause of a
    // `None`, which is what lets that panic name the child.
    assert_eq!(libtest_summary_count("", SummaryField::Passed), None);
    assert_eq!(
        libtest_summary_count("running 1 test\ntest foo ... ok\n", SummaryField::Passed),
        None
    );
}

/// The LAST `test result:` line wins — the parser's other stated property, and
/// the one that matters in practice: every replay child is spawned with
/// `--nocapture`, so a test's own stdout is interleaved with libtest's and can
/// carry the same prefix. Not hypothetical: assertion messages in these
/// binaries embed a truncated child stdout, summary line and all.
#[test]
fn libtest_summary_count_takes_the_last_summary_line() {
    // A decoy `test result:` line emitted by the test's OWN output under
    // `--nocapture`, ahead of the real summary. Reading the first match would
    // report the decoy's 99.
    let interleaved = concat!(
        "running 1 test\n",
        "some test echoed a captured child summary:\n",
        "test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
         finished in 0.01s\n",
        "test a_replayed_test ... ok\n",
        "\n",
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; \
         finished in 0.01s\n",
    );
    assert_eq!(
        libtest_summary_count(interleaved, SummaryField::Passed),
        Some(1)
    );

    // A nested child's summary reaching the parent through a
    // `--- child stdout ---` block may arrive indented. The parser trims before
    // matching the prefix, so such a line is still a candidate — and being LAST
    // is what decides, not indentation.
    let indented = concat!(
        "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
         finished in 0.01s\n",
        "    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
         finished in 0.01s\n",
    );
    assert_eq!(
        libtest_summary_count(indented, SummaryField::Passed),
        Some(2)
    );
}
