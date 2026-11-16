//! End-to-end CLI tests for the direction-typing β `dir` builtin: `dir(x, y, z)`
//! and `dir(v)` construct unit Directions; a degenerate vector is an
//! `E_DegenerateDirection` error, a dimensioned component a dimension error, and
//! a shape fault a silent `undef`.

use std::process::ExitStatus;

use crate::common;

/// Writes `module <stem>` plus `source` to `<tempdir>/<stem>.ri` (the module
/// name must equal the file stem) and runs `reify <subcommand>` on it.
fn run_source(subcommand: &str, stem: &str, source: &str) -> (ExitStatus, String, String) {
    let dir = tempfile::TempDir::new().expect("create a tempdir");
    let path = dir.path().join(format!("{stem}.ri"));
    std::fs::write(&path, format!("module {stem}\n{source}")).expect("write the .ri source");
    common::run_with_args(&[subcommand, path.to_str().expect("utf-8 tempdir path")])
}

fn in_structure(lets: &str) -> String {
    format!("structure def S {{\n{lets}\n}}\n")
}

#[test]
fn eval_dir_constructs_unit_directions() {
    let path = common::fixture_path("dir_unit.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "dir_unit.ri should evaluate cleanly;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for row in [
        "DirUnit.a = dir(0, 1, 0)",
        "DirUnit.b = dir(0, 1, 0)",
        "DirUnit.c = dir(0, 0, 1)",
        "DirUnit.near = dir(1, 0, 0)",
        "DirUnit.ident = dir(1, 0, 0)",
        "DirUnit.fy = dir(0, 1, 0)",
        "DirUnit.via_fn = dir(0, 1, 0)",
        "DirUnit.via_proj = dir(0, 1, 0)",
    ] {
        assert!(
            stdout.contains(row),
            "stdout should carry `{row}`;\nstdout: {stdout}\nstderr: {stderr}"
        );
    }

    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "dir_unit.ri should check cleanly;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.to_lowercase().contains("unresolved function"),
        "dir must be a known builtin;\nstderr: {stderr}"
    );
}

#[test]
fn eval_degenerate_dir_exits_nonzero_with_e_degenerate_direction() {
    let nan = "    let big = 1e308\n    let inf = big * big\n    let nan = inf - inf\n    \
               let d = dir(nan, 0, 0)";
    for (lets, non_finite) in [
        ("    let d = dir(0, 0, 0)", false),
        ("    let d = dir(1e-13, 0, 0)", false),
        ("    let d = dir(vec3(0.0, 0.0, 0.0))", false),
        (nan, true),
    ] {
        let (status, stdout, stderr) = run_source("eval", "dir_degenerate", &in_structure(lets));
        assert!(
            !status.success(),
            "a degenerate dir must fail eval: {lets}\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            stderr.contains("E_DegenerateDirection") && stderr.contains("dir:"),
            "stderr must name E_DegenerateDirection for {lets}\nstderr: {stderr}"
        );
        if non_finite {
            assert!(
                stderr.contains("non-finite"),
                "a NaN dir must be named non-finite\nstderr: {stderr}"
            );
        }
    }
}

#[test]
fn eval_dimensioned_dir_exits_nonzero_with_a_dimension_error() {
    for lets in [
        "    let d = dir(1mm, 0, 0)",
        "    let d = dir(1mm, 0mm, 0mm)",
        "    let d = dir(vec3(1mm, 0mm, 0mm))",
        "    let d = dir(0mm, 0mm, 0mm)",
    ] {
        let (status, stdout, stderr) = run_source("eval", "dir_dimensioned", &in_structure(lets));
        assert!(
            !status.success(),
            "a dimensioned dir must fail eval: {lets}\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            stderr.contains("dir:") && stderr.contains("dimensionless"),
            "stderr must name the dimension fault for {lets}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_DegenerateDirection"),
            "a dimension fault must not be blamed on degeneracy: {lets}\nstderr: {stderr}"
        );
    }
}

#[test]
fn eval_shape_fault_dir_stays_undiagnosed_undef() {
    let (status, stdout, stderr) = run_source(
        "eval",
        "dir_shape",
        &in_structure("    let d = dir(\"up\")"),
    );
    assert!(
        status.success(),
        "a shape-fault dir is a silent undef, not an error;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("S.d = undef"),
        "the cell must print undef;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DegenerateDirection") && !stderr.contains("dimensionless"),
        "a shape fault must stay undiagnosed;\nstderr: {stderr}"
    );
}

#[test]
fn check_user_fn_direction_slot_rejects_a_vector() {
    let source = format!(
        "fn pick(d: Direction) -> Direction {{ d }}\n{}",
        in_structure("    let p = pick(vec3(0.0, 1.0, 0.0))")
    );
    let (status, stdout, stderr) = run_source("check", "dir_slot", &source);
    assert!(
        !status.success(),
        "a Vector must not fill a Direction slot;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("no matching overload"),
        "stderr must report no matching overload;\nstderr: {stderr}"
    );
}
