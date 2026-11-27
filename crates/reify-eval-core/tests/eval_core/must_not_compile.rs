//! Scenario K10, contract 2.2 O4 (docs/prds/evaluation-core.md): the programs fail with the
//! errors PRD 5.2 states, and the loopholes compile so that closing one is noticed. A change that
//! moves the recorded output, an interface edit or a toolchain bump alike, re-records it with
//! TRYBUILD=overwrite only where each program still fails with its 5.2 error (PRD 12.1, rule 10).

#[test]
fn a_body_cannot_wait_in_a_section_or_carry_the_resource_out_but_two_loopholes_compile() {
    let must_not_compile = glob_of_exactly("tests/must_not_compile", 6);
    let loopholes = glob_of_exactly("tests/loopholes", 2);
    let programs = trybuild::TestCases::new();
    programs.compile_fail(must_not_compile);
    programs.pass(loopholes);
}

/// trybuild passes a glob that matches no file, so this fails unless `directory` holds exactly
/// `count` programs.
fn glob_of_exactly(directory: &str, count: usize) -> String {
    let paths: std::io::Result<Vec<_>> = std::fs::read_dir(directory)
        .and_then(|entries| entries.map(|entry| Ok(entry?.path())).collect());
    let found = paths
        .unwrap_or_else(|error| panic!("reading {directory}: {error}"))
        .iter()
        .filter(|program| {
            program
                .extension()
                .is_some_and(|extension| extension == "rs")
        })
        .count();
    assert_eq!(
        found, count,
        "{directory} holds {found} programs; PRD 12.2 gives it {count}"
    );
    format!("{directory}/*.rs")
}
