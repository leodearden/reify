// Integration tests for task 4327 (PRD undef-self-describing δ):
// `reify eval` emits a complete cause-set note for each undef output cell.
//
// S1 clause 1: a partial design (Tube with unbound params) → exit 0 +
//   stderr contains `note: Tube.wall_thickness is undef` with BOTH causes
//   listed (Tube.outer_diameter, Tube.wall_ratio) — proves the COMPLETE set
//   is emitted, not just the first cause (B1).
//
// S1 clause 2: a fully-determined design → exit 0 + stderr contains NO
//   `note: ... is undef` line — proves silence on a clean design.

mod common;

/// Test A (S1 clause 1): partial design → undef note with COMPLETE cause set.
///
/// Runs `reify eval examples/undef_self_describing.ri` and asserts:
/// - exit code 0 (undef output is not an error — PRD §9.2)
/// - stderr contains the subject line `note: Tube.wall_thickness is undef`
/// - stderr contains both `Tube.outer_diameter` and `Tube.wall_ratio`
///   together with `unbound` (proving the complete root-cause set, not
///   first-only).
#[test]
fn eval_undef_emits_note_with_complete_cause_set() {
    let path = common::example_path("undef_self_describing.ri");
    let (status, _stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval must exit 0 on a partial design (undef is not an error)\nstderr:\n{stderr}"
    );

    assert!(
        stderr.contains("note: Tube.wall_thickness is undef"),
        "stderr must contain the subject line `note: Tube.wall_thickness is undef`\nstderr:\n{stderr}"
    );

    // Both root causes must appear in the because-clause — B1: complete set.
    assert!(
        stderr.contains("Tube.outer_diameter") && stderr.contains("unbound"),
        "stderr must contain `Tube.outer_diameter` and `unbound` in the cause set\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Tube.wall_ratio") && stderr.contains("unbound"),
        "stderr must contain `Tube.wall_ratio` and `unbound` in the cause set\nstderr:\n{stderr}"
    );
}

/// Test B (S1 clause 2): fully-determined design → zero undef notes.
///
/// Runs `reify eval` on the determined fixture (all params have defaults)
/// and asserts:
/// - exit code 0
/// - stderr contains NO line that is both a note-line AND contains `is undef`
///   (silence for a fully-determined design).
#[test]
fn eval_determined_emits_no_undef_notes() {
    let path = common::fixture_path("undef_self_describing_determined.ri");
    let (status, _stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval must exit 0 on a fully-determined design\nstderr:\n{stderr}"
    );

    // No note line should mention "is undef" for a fully-determined design.
    let has_undef_note = stderr
        .lines()
        .any(|line| line.contains("note:") && line.contains("is undef"));
    assert!(
        !has_undef_note,
        "stderr must contain NO `note: ... is undef` lines for a fully-determined design\nstderr:\n{stderr}"
    );
}
