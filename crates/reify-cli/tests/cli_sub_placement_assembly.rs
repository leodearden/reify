//! CLI end-to-end gate for the multi-level assembly example (task 3908).
//!
//! Runs `reify build examples/sub_placement_assembly.ri -o out.step` through the
//! real binary and asserts the §8.3 export boundary condition on the committed
//! multi-level example:
//!
//! - Exit 0 and "Wrote" in stdout (clean build).
//! - Exactly **3** `MANIFOLD_SOLID_BREP(` entities in the STEP output (arm,
//!   motor, shaft) — the aux fixture is excluded because `default_visible ==
//!   false` (§8.3: only product solids are exported).
//! - The source file uses declarative `at` placement and contains **zero**
//!   `translate(self.` manual-lift expressions (§0: the ceremony is gone).
//!
//! This is an ACTIVE (non-ignored) end-to-end gate: the CLI binary links with
//! OCCT, so `reify build` exercises the full placement+surfacing+export path.
//! Capability is delivered by T7/3905; this test locks the committed example.

mod common;

/// End-to-end gate: `reify build examples/sub_placement_assembly.ri` must
/// produce a STEP file with exactly 3 product solids (aux fixture excluded).
///
/// Also verifies the no-manual-lift structural invariant: the example source
/// uses `at` placement and contains zero `translate(self.` expressions.
#[test]
fn build_sub_placement_assembly_three_product_solids_aux_excluded() {
    let result = common::run_build_at(&common::example_path("sub_placement_assembly.ri"));

    // (a) Clean exit.
    assert!(
        result.status.success(),
        "reify build should exit 0 for sub_placement_assembly.ri.\n\
         stdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );

    // (b) "Wrote" confirmation in stdout.
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote'; got: {}",
        result.stdout
    );

    // (c) Output STEP file exists on disk.
    assert!(
        result.output_path.exists(),
        "geometry file should be written for sub_placement_assembly.ri"
    );

    // (d) Exactly 3 product solids in the STEP (arm + motor + shaft).
    //     Aux fixture must be absent (default_visible==false → excluded from export).
    let step_bytes = std::fs::read(&result.output_path).expect("failed to read exported STEP file");
    let step_str = String::from_utf8(step_bytes).expect("STEP output must be valid UTF-8");

    let solid_count = step_str.matches("MANIFOLD_SOLID_BREP(").count();
    assert_eq!(
        solid_count, 3,
        "exported STEP must contain exactly 3 product solids (aux fixture excluded);\n\
         got {solid_count} MANIFOLD_SOLID_BREP entities.\n\
         (4 → aux not excluded; 2 or 1 → motor/shaft not exported at composed coords)"
    );

    // (e) No-manual-lift structural acceptance: the example uses `at` placement
    //     and contains zero `translate(self.` lift expressions (§0 invariant).
    //
    //     Strip comment lines before checking so that comment prose such as
    //     "Shaft at depth 2" or "placed at +100 mm Y" cannot satisfy the
    //     positive assertion in the absence of real DSL placement clauses.
    let example_source = std::fs::read_to_string(common::example_path("sub_placement_assembly.ri"))
        .expect("examples/sub_placement_assembly.ri must be readable");

    let code_only: String = example_source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        code_only.contains(" at "),
        "example code must contain at least one `at` placement clause (declarative placement); \
         got code lines:\n{code_only}"
    );
    assert!(
        !code_only.contains("translate(self."),
        "example code must not contain `translate(self.` manual-lift expressions \
         (§0: the ceremony is gone)"
    );
}
