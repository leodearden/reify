//! Scenario K10, contract 2.2 O4: the programs fail with the errors PRD 5.2 states, and the
//! loopholes compile so that closing one is noticed. A leaf that changes Kind, Cx, Section or
//! Produced updates the programs and their .stderr only where wording or position moved.

#[test]
fn a_body_cannot_wait_inside_a_section_or_carry_the_resource_out_of_one() {
    let programs = trybuild::TestCases::new();
    programs.compile_fail("tests/must_not_compile/*.rs");
    programs.pass("tests/loopholes/*.rs");
}
