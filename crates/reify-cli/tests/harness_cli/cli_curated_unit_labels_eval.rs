//! End-to-end acceptance for task #6674: the `reify eval` cell renders its unit
//! from the curated unit-ladder registry, and nothing but the unit moves.
//!
//! Driven through the real CLI rather than `impl Display for Value` in-process,
//! so the whole eval print path — `Value` → `format!("{}", v)` → the
//! `<lhs> = <rhs>` line — is under test, not just the formatter.
//!
//! The positive half is the two dimensions #6674 curates (Frequency → "Hz",
//! Stiffness → "N/m"). The negative half is the acceptance's other clause:
//! Length and Angle keep BOTH their raw-SI labels and their raw-SI magnitudes,
//! because this path never consults `resolve_display`. The Angle witness
//! matters specifically because no committed golden exercises Angle, so this
//! fixture is its only one.
//!
//! RED until step-6 creates crates/reify-cli/tests/fixtures/curated_unit_labels.ri.

use crate::common;

#[test]
fn eval_curated_unit_labels_renders_registry_units_without_rescaling() {
    let path = common::fixture_path("curated_unit_labels.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval curated_unit_labels.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // The two dimensions #6674 adds to the curated set.
    for expected in ["= 50 Hz", "= 1000 N/m"] {
        assert!(
            stdout.contains(expected),
            "stdout should contain {expected:?} (a curated coherent-SI unit label);\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // Unchanged neighbours: same labels AND same magnitudes as before #6674.
    for expected in ["= 0.003 m", "= 0.5 rad"] {
        assert!(
            stdout.contains(expected),
            "stdout should contain {expected:?} — Length and Angle rendering is \
             unchanged by #6674;\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // The negative half: the composed base-SI forms the curated labels replace
    // must be gone.
    for gone in ["s^-1", "kg\u{00b7}s^-2"] {
        assert!(
            !stdout.contains(gone),
            "stdout must NOT contain the composed base-SI form {gone:?} — it is \
             what the curated label replaces;\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    // Standing fence: this path must never acquire resolve_display's
    // rescaling — no scaled rung, and no engineering notation.
    for gone in ["3 mm", "28.6478897565 deg", "\u{00d7}"] {
        assert!(
            !stdout.contains(gone),
            "stdout must NOT contain {gone:?} — the eval cell renders the raw SI \
             magnitude, never a rescaled one;\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}
