//! The PROCESS-BOUNDARY half of PRD `docs/prds/v0_6/units-length-gate-completion.md` §6.
//!
//! Every leaf of that PRD tested its own gate IN-PROCESS, at `Engine::build` against a
//! `MockGeometryKernel`. Nothing pinned the signal the PRD's §1 headline promises the
//! user: `$ reify eval bare.ri` → a units diagnostic, `$ echo $?` → 1. A severity
//! downgrade, a swallowed build result at `main.rs`, or a diagnostic-renderer regression
//! would leave every leaf test green while that user-observable signal vanished. This
//! file exists so that cannot happen: each test spawns the real `reify` binary and
//! asserts the exit code and the stderr a user would actually see.
//!
//! Each source is written to a `tempfile::tempdir()` rather than to the flat
//! `tests/fixtures/` tree, so a bare-number source sits beside the assertion it feeds and
//! this file can be read in isolation. The file stem MUST match the source's `module`
//! declaration: on a mismatch the CLI reports `E_MODULE_PATH_MISMATCH` and the test
//! measures that instead of the units gate (the trap documented at
//! `harness_cli/cli_check.rs:796-797`).
//!
//! ROWS §6 NAMES THAT THIS FILE DELIBERATELY DOES NOT RE-ASSERT. Each already has a
//! shipped, non-vacuous, gate-resident test; a second copy here would be duplication, not
//! coverage. They enter the suite through `units_length_boundary_ledger.rs`, which binds
//! every §6 row to the test that discharges it.
//!
//! - Row 8 — `harness_cli/cli_affine_eval.rs::eval_affine_translate_mass_exits_1_with_a_units_error`,
//!   which is already end to end at this same process boundary.
//! - Row 7's `Scalar{DIMENSIONLESS}` twin —
//!   `reify-eval/src/geometry_ops/tests.rs::compile_geometry_op_apply_transform_translation_follows_the_three_state_contract`,
//!   because that value shape is not expressible from `.ri` source.
//! - Row 1's `DiagnosticCode` clause —
//!   `harness_geometry/primitive_profile_length_units_e2e.rs::bare_box_dimensions_drop_the_op_with_a_coded_error`,
//!   because the code is not observable here: `reify eval` has no structured-diagnostics
//!   flag (`Usage: reify eval [--explain-undef] [--verbose] [--cache-dir <path>] <file>`)
//!   and its renderer prints no code prefix for these errors.

use crate::common;
use reify_core::units::LENGTH_MIGRATION_HINT;
use std::process::ExitStatus;

/// A `.ri` source together with the file stem its `module` declaration must match.
///
/// The two travel as ONE value because they are not independent: on a mismatch the CLI
/// reports `E_MODULE_PATH_MISMATCH` and the row measures that instead of the units gate.
/// Two loose `&str` arguments leave that agreement to each call site; one struct makes it
/// a property of the fixture, which is also what lets a fixture be shared by a row and by
/// the D9 invariant below without either copying the other's bytes.
struct RiSource {
    stem: &'static str,
    source: &'static str,
}

impl RiSource {
    /// Run `reify eval` over this source.
    fn eval(&self) -> (ExitStatus, String, String) {
        self.run("eval")
    }

    /// Run `reify check` over this source — the twin row 9 needs.
    fn check(&self) -> (ExitStatus, String, String) {
        self.run("check")
    }

    /// Write the source as `<stem>.ri` into a fresh temp dir, run `reify <subcommand>`
    /// over it and return `(status, stdout, stderr)`.
    ///
    /// The single spawn site — the same shape `tests/common/mod.rs` uses for its own
    /// `spawn_reify`. A fresh dir per call, so two rows may share one fixture (and so one
    /// stem) without colliding.
    fn run(&self, subcommand: &str) -> (ExitStatus, String, String) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join(format!("{stem}.ri", stem = self.stem));
        std::fs::write(&path, self.source).expect("failed to write temp module");
        common::run_with_args(&[subcommand, path.to_str().expect("temp path is UTF-8")])
    }
}

/// The bare-primitive source. ONE copy, read by §6 row 1, row 9 and the D9 invariant —
/// three assertions about the same bytes, which is only true if they are the same bytes.
const BARE_BOX: RiSource = RiSource {
    stem: "bare_box_dimensions",
    source: r#"module bare_box_dimensions

structure def S {
    let b = box(20, 20, 10)
    param geometry : Solid = b
}
"#,
};

/// The bare-modify source. ONE copy, read by §6 row 4 and by D9.
const BARE_FILLET: RiSource = RiSource {
    stem: "bare_fillet_radius",
    source: r#"module bare_fillet_radius

structure def S {
    let b = fillet(box(10mm, 10mm, 10mm), 1)
    param geometry : Solid = b
}
"#,
};

/// The bare-transform source. ONE copy, read by §6 row 7 and by D9.
const BARE_TRANSFORM: RiSource = RiSource {
    stem: "bare_transform_translation",
    source: r#"module bare_transform_translation

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let moved = apply_transform(b, transform3(orient_identity(), vec3(5, 0, 0)))
    param geometry : Solid = moved
}
"#,
};

/// Assert that `stderr` carries the ONE units-rejection line `ArgRejection::message`
/// (`crates/reify-ir/src/arg_acceptance.rs:211-222`) produces for `builtin`'s `arg`.
///
/// Built from the template rather than hand-spelled per row, and from the real
/// [`LENGTH_MIGRATION_HINT`] const rather than a copy of its text. Hand-spelling the hint
/// once per row would be exactly the lockstep duplication D9 exists to prevent: a reword
/// of the hint must break this suite in ONE place, not in every row that quotes it.
///
/// A SUBSTRING check, deliberately: a row's claim is that the user is told this, wherever
/// the renderer puts it. The stricter claim — that the line carries this and nothing else,
/// identically on every route — is [`every_units_rejection_uses_the_one_wording_template`]'s,
/// and it is stricter precisely because it compares whole lines.
fn expect_length_rejection(stderr: &str, builtin: &str, arg: &str, got: &str) {
    let expected = length_rejection_line(builtin, arg, got);
    assert!(
        stderr.contains(&expected),
        "stderr should carry the units rejection `{expected}`; got: {stderr}"
    );
}

/// The ONE wording template, and the file's only copy of it.
///
/// Its single producer in the tree is `ArgRejection::message`. Every consumer here —
/// each row's [`expect_length_rejection`] and the cross-route D9 invariant below — reads
/// the shape from here and the hint from the real const, so a reword breaks this suite in
/// one place rather than once per row.
fn length_rejection_line(builtin: &str, arg: &str, got: &str) -> String {
    format!("{builtin}: {arg} argument expects Length, got {got}; {LENGTH_MIGRATION_HINT}")
}

/// §6 row 1 — a bare-`Int` primitive dimension is rejected at the process boundary,
/// naming EVERY offending argument rather than only the first.
///
/// All three of `width`, `height` and `depth` are asserted: a gate that fired on one
/// position and silently passed the other two would still exit 1, so the exit code alone
/// cannot distinguish a whole gate from a third of one.
#[test]
fn bare_box_dimensions_exit_1_naming_every_rejected_argument() {
    let (status, stdout, stderr) = BARE_BOX.eval();

    assert!(
        !status.success(),
        "`reify eval` on bare box dimensions must exit nonzero — this IS the §1 \
         headline signal;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        expect_length_rejection(&stderr, "box", arg, "Int");
    }
}

/// §6 row 2 (control) — the dimensioned spelling still exits 0 AND still realizes the
/// same SI volume the pre-gate build produced.
///
/// The volume is the load-bearing half. Exit 0 alone would also be produced by a gate
/// that accepted the LENGTH and then re-scaled it; `20mm × 20mm × 10mm` is 4·10⁻⁶ m³
/// before the gate and must remain so after it.
///
/// Compared as a NUMBER, through [`printed_magnitude`], for the reason row 6 states at
/// length: pinning the printed spelling would red this row on a benign change to the
/// value printer instead of on the re-scaling it exists to catch.
#[test]
fn dimensioned_box_exits_0_with_the_pre_gate_si_volume() {
    let (status, stdout, stderr) = RiSource {
        stem: "dimensioned_box",
        source: r#"module dimensioned_box

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let v = volume(b)
    param geometry : Solid = b
}
"#,
    }
    .eval();

    assert!(
        status.success(),
        "the dimensioned control must still exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("expects Length"),
        "a dimensioned length must not be rejected; got: {stderr}"
    );
    let v = printed_magnitude(&stdout, "S.v", "m^3");
    assert!(
        (v - 4e-6).abs() < 1e-12,
        "the gate must not have re-scaled an ACCEPTED length: 20mm × 20mm × 10mm is \
         4e-6 m³, the pre-gate SI baseline; got {v} m³ from: {stdout}"
    );
}

/// §6 row 3 — D1: a bare `0` is NOT special-cased, so it is rejected with exactly the
/// wording any other bare number gets.
///
/// `0` is the tempting exemption (it is dimensionally harmless), and an exemption is the
/// one shape that would let a bare-number habit survive the gate.
#[test]
fn a_bare_zero_is_rejected_like_any_other_bare_number() {
    let (status, stdout, stderr) = RiSource {
        stem: "bare_zero_box",
        source: r#"module bare_zero_box

structure def S {
    let b = box(0, 0, 0)
    param geometry : Solid = b
}
"#,
    }
    .eval();

    assert!(
        !status.success(),
        "D1: a bare zero must be rejected, not exempted;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        expect_length_rejection(&stderr, "box", arg, "Int");
    }
}

/// §6 row 4 — a bare `fillet` radius is rejected with a UNITS diagnostic that REPLACES
/// the span-less kernel failure the PRD's §2 probe recorded.
///
/// The negative half is the row's real claim. Before the gate, a bare radius reached
/// OCCT and surfaced as `BRepFilletAPI_MakeFillet failed` — unattributable, with no span
/// and no argument name. Asserting only the new message would leave a regression that
/// merely PREPENDED a units line to the old kernel failure indistinguishable from the
/// fix, so the old failure's absence is asserted too.
#[test]
fn a_bare_fillet_radius_replaces_the_span_less_occt_failure() {
    let (status, stdout, stderr) = BARE_FILLET.eval();

    assert!(
        !status.success(),
        "a bare fillet radius must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "fillet", "radius", "Int");
    assert!(
        !stderr.contains("BRepFilletAPI_MakeFillet"),
        "the bare radius must never reach OCCT: the span-less kernel failure is \
         REPLACED by the units diagnostic, not accompanied by it; got: {stderr}"
    );
}

/// §6 row 5a — the PLANE-VALUE form. The plane's own offset is rejected, and `mirror`
/// then fails attributably on the resulting `undef` rather than silently building a
/// mirror plane 10 METRES out.
///
/// Split from the scalar form below because the two travel different routes: this one
/// rejects inside `plane_yz` and propagates `undef` into `mirror`, while the scalar form
/// rejects at `mirror`'s own origin slots. One test asserting a loose `contains("mirror")`
/// would pass on either and so pin neither.
#[test]
fn a_bare_plane_offset_is_rejected_and_mirror_fails_attributably() {
    let (status, stdout, stderr) = RiSource {
        stem: "bare_mirror_plane_offset",
        source: r#"module bare_mirror_plane_offset

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10))
    param geometry : Solid = m
}
"#,
    }
    .eval();

    assert!(
        !status.success(),
        "a bare plane offset must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "plane_yz", "offset", "Int");
    assert!(
        stderr.contains("mirror: expected a Plane value, got undef"),
        "the consuming `mirror` must fail attributably on the resulting undef rather \
         than silently mirroring about a 10-metre plane; got: {stderr}"
    );
}

/// §6 row 5b — the SCALAR form keeps its pre-existing per-component wording. The
/// no-regression half of row 5.
///
/// `ox` is asserted by name: a loose `contains("mirror")` would also be satisfied by the
/// plane-value route's `expected a Plane value` message, so it could not tell a surviving
/// per-component gate from a collapsed one.
#[test]
fn the_scalar_mirror_origin_keeps_its_per_component_wording() {
    let (status, stdout, stderr) = RiSource {
        stem: "bare_mirror_scalar_origin",
        source: r#"module bare_mirror_scalar_origin

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, 10, 0, 0, 1, 0, 0)
    param geometry : Solid = m
}
"#,
    }
    .eval();

    assert!(
        !status.success(),
        "the scalar mirror origin must still exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "mirror", "ox", "Int");
}

/// §6 row 6 (control) — the dimensioned plane offset still exits 0 and still mirrors
/// about the same place.
///
/// WHY THE COMPARAND IS `0.02 m` AND NOT THE PRD's LITERAL `plane_yz(0.01)`: that
/// spelling is now itself rejected (measured: `plane_yz: offset argument expects Length,
/// got Real`), so it cannot be run as a live comparand. The identity the PRD row was
/// always about survives intact — `10mm` IS `0.01 m`, and mirroring a box spanning
/// x ∈ [0, 0.02] about the plane x = 0.01 leaves its centroid at x = 0.02.
///
/// A tolerance rather than a byte-equal match: the printed x is
/// `0.019999999999999997 m`, and pinning that spelling would make the row fail on any
/// benign change to the value printer's float formatting instead of on a geometry change.
/// [`printed_magnitude`] is where that reasoning lives for every control row, row 2
/// included.
#[test]
fn a_dimensioned_plane_offset_mirrors_to_the_same_si_position() {
    let (status, stdout, stderr) = RiSource {
        stem: "dimensioned_mirror_plane",
        source: r#"module dimensioned_mirror_plane

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10mm))
    let c = centroid(m)
    param geometry : Solid = m
}
"#,
    }
    .eval();

    assert!(
        status.success(),
        "the dimensioned control must still exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("expects Length"),
        "a dimensioned plane offset must not be rejected; got: {stderr}"
    );

    let x = printed_magnitude(&stdout, "S.c", "m");
    assert!(
        (x - 0.02).abs() < 1e-9,
        "mirroring about x = 10mm must leave the centroid at x = 0.02 m — the SI \
         identity the PRD's `plane_yz(0.01)` comparand stood for; got {x} m from: {stdout}"
    );
}

/// The magnitude of the first `<number> <unit>` field `reify eval` prints for `binding`.
///
/// ONE extractor for every control row, so no row pins a float's SPELLING. That matters
/// uniformly: `S.v = 0.000004 m^3` and `S.c = point(0.019999999999999997 m, …)` are both
/// the value printer's choice of rendering, and a benign change to it — `4e-6 m^3`, an
/// extra significant figure — must not red a row whose claim is about geometry. Rows
/// compare the number, within a tolerance they choose.
///
/// "First field" is exact rather than vague: for a scalar there is only one, and for a
/// `point(x, y, z)` it is the x component, which is the only component any control row
/// here asks about.
///
/// The CLI's only output is text, so SOME extraction is unavoidable; this is the narrowest
/// one that cannot silently succeed on the wrong line. Every failure path panics with the
/// stdout that produced it, so a printer change surfaces as a readable diagnostic rather
/// than a wrong number.
fn printed_magnitude(stdout: &str, binding: &str, unit: &str) -> f64 {
    let prefix = format!("{binding} = ");
    let rendered = stdout
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no `{prefix}…` line in stdout: {stdout}"));
    let suffix = format!(" {unit}");
    rendered
        .split(['(', ',', ')'])
        .filter_map(|field| field.trim().strip_suffix(&suffix))
        .find_map(|magnitude| magnitude.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("no `<number> {unit}` field in `{rendered}` (stdout: {stdout})"))
}

/// Every stderr line with the renderer's severity prefix stripped.
///
/// What remains is the message its producer built — `ArgRejection::message` for a units
/// rejection — so a whole-line comparison against the template is a claim about that
/// producer and not about how the renderer decorates it.
fn diagnostic_lines(stderr: &str) -> impl Iterator<Item = &str> {
    stderr.lines().map(|line| {
        let line = line.trim();
        line.strip_prefix("error: ")
            .or_else(|| line.strip_prefix("warning: "))
            .unwrap_or(line)
    })
}

/// §6 row 7 — a bare translation component is rejected by NAME, replacing the generic
/// shape message that named nothing.
///
/// Both halves matter. Before the gate, `vec3(5, 0, 0)` failed as
/// `not a valid Transform<3>` — a whole-argument complaint that told the user neither
/// which component was wrong nor that units were the reason. The row's claim is that the
/// generic message was REPLACED, so its absence is asserted alongside the new one.
///
/// ARITY TRAP: `transform3` takes `(orientation, translation)`. A 4-arg spelling never
/// reaches the units gate at all — it fails the generic shape check first, and a fixture
/// written that way would read as a units failure while measuring something else.
///
/// The `Scalar{DIMENSIONLESS}` twin §6 row 7 also names gets NO fixture here, and that
/// is deliberate rather than an omission: ζ probed it and recorded the result at
/// `crates/reify-eval/tests/harness_geometry/transform_translation_length_units_e2e.rs:19-29`
/// — `5mm / 1mm` collapses to `Value::Real`, so no `.ri` source can express the twin, and
/// writing one anyway would exercise the `Real` path twice while claiming to cover it.
/// Its home is the unit row
/// `geometry_ops/tests.rs::compile_geometry_op_apply_transform_translation_follows_the_three_state_contract`,
/// which constructs the value directly; it enters this suite through the ledger.
#[test]
fn a_bare_transform_translation_names_the_component_not_the_shape() {
    let (status, stdout, stderr) = BARE_TRANSFORM.eval();

    assert!(
        !status.success(),
        "a bare translation component must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "apply_transform", "translation.x", "Int");
    assert!(
        !stderr.contains("not a valid Transform<3>"),
        "the generic shape message must be REPLACED by the per-component units \
         diagnostic, not accompanied by it; got: {stderr}"
    );
}

/// §6 row 9 — D8: `reify check` and `reify eval` agree on the SAME bytes.
///
/// One source, two subcommands, one assertion block — and [`BARE_BOX`] really is one
/// source, shared with row 1 rather than re-typed here, so "the same bytes" is a property
/// of the fixture and not of two transcriptions staying in step. `check` is the cheap
/// pre-flight a user reaches for first; if it passed what `eval` rejects, the gate would
/// be advisory rather than binding, and a bare-number habit would survive contact with it.
///
/// This holds today on η's compile slots alone, with no dependency on PRD 2's `reify
/// check` semantics — verified live. Should this row ever come to need those semantics,
/// that is a real dependency edge onto PRD 2's task, never a weakening of the assertion.
#[test]
fn check_and_eval_agree_on_a_bare_primitive_dimension() {
    for (subcommand, (status, stdout, stderr)) in
        [("check", BARE_BOX.check()), ("eval", BARE_BOX.eval())]
    {
        assert!(
            !status.success(),
            "`reify {subcommand}` must exit nonzero on a bare primitive dimension;\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        expect_length_rejection(&stderr, "box", "width", "Int");
    }
}

/// §6 row 9b — the arity-6 `linear_pattern_2d` site is resolved as an ARITY error, and
/// was deliberately NOT given an arity-6 LENGTH slot.
///
/// §6 left this row open ("resolved either way"); the decompose-time correction settled
/// it as the malformed-fixture branch, so the row's postcondition is a PAIR: the arity
/// diagnostic is present AND no spacing slot was minted at that arity. Asserting only the
/// arity message would stay green if someone later added the slot, which is the outcome
/// the row exists to rule out — an arity-6 call has `count2` at index 5, so a slot there
/// would emit a FALSE units rejection on valid code (the reason
/// `builtin_signatures.rs` guards the slots with `arg_count == 11`).
///
/// Compile-layer twin:
/// `harness_compilation_surface/compile_api_tests.rs::compile_linear_pattern_2d_wrong_arity_produces_diagnostic`.
#[test]
fn the_arity_6_linear_pattern_2d_site_is_an_arity_error_not_a_length_slot() {
    let (status, stdout, stderr) = RiSource {
        stem: "arity_6_linear_pattern_2d",
        source: r#"module arity_6_linear_pattern_2d

structure def S {
    let w = box(5mm, 5mm, 5mm)
    let p = linear_pattern_2d(w, 1, 0, 0, 3, 20)
    param geometry : Solid = p
}
"#,
    }
    .eval();

    assert!(
        !status.success(),
        "an arity-6 linear_pattern_2d call must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("linear_pattern_2d() expects 11 arguments, got 6"),
        "the site must resolve as an ARITY error; got: {stderr}"
    );
    for slot in ["spacing1", "spacing2"] {
        assert!(
            !stderr.contains(slot),
            "no `{slot}` LENGTH slot exists at arity 6 — index 5 is `count2` there, so a \
             slot would emit a FALSE units rejection on valid code; got: {stderr}"
        );
    }
}

/// One row of the §6-18 table: a pattern source whose spacing is left `Undef`, the label
/// its diagnostic MUST carry, and the internal nickname it must no longer carry.
struct PatternLabelCase {
    fixture: RiSource,
    unresolved_arg: &'static str,
    builtin: &'static str,
    pre_lambda_nickname: &'static str,
}

/// §6 row 18 — D7: an unresolved pattern spacing names the builtin the `.ri` author
/// actually TYPED, not the compiler's internal `PatternKind` nickname.
///
/// The route is the REACHABLE one. A bare literal in the spacing position is caught
/// earlier by the compile slot and never reaches the `Undef` chokepoint that renders this
/// label, so the source leaves the spacing genuinely unresolved — a `param s : Length`
/// with no default. Each case asserts the typed name positively AND the nickname
/// negatively: `contains("linear_pattern")` alone is satisfied by the nickname spelling
/// too, so only the pair distinguishes the fix from the bug.
///
/// `PatternKind::Circular` and `Arbitrary` still render `circular` and `arbitrary` and are
/// deliberately outside this row: finishing D7 for them is task #6874, which must land the
/// call-site migration in the same diff.
#[test]
fn pattern_spacing_undef_names_the_builtin_the_author_typed() {
    let cases = [
        PatternLabelCase {
            fixture: RiSource {
                stem: "undef_spacing_linear_pattern",
                source: r#"module undef_spacing_linear_pattern

structure def S {
    param s : Length
    let b = box(10mm, 10mm, 10mm)
    let p = linear_pattern(b, 1, 0, 0, 3, s)
    param geometry : Solid = p
}
"#,
            },
            unresolved_arg: "spacing",
            builtin: "linear_pattern",
            pre_lambda_nickname: "for linear ",
        },
        PatternLabelCase {
            fixture: RiSource {
                stem: "undef_spacing_linear_pattern_2d",
                source: r#"module undef_spacing_linear_pattern_2d

structure def S {
    param s : Length
    let b = box(10mm, 10mm, 10mm)
    let p = linear_pattern_2d(b, 1, 0, 0, 3, s, 0, 1, 0, 3, 20mm)
    param geometry : Solid = p
}
"#,
            },
            unresolved_arg: "spacing1",
            builtin: "linear_pattern_2d",
            pre_lambda_nickname: "linear_2d",
        },
    ];

    for case in cases {
        let (status, stdout, stderr) = case.fixture.eval();
        let expected = format!(
            "argument '{arg}' for {builtin} is unresolved (Undef)",
            arg = case.unresolved_arg,
            builtin = case.builtin,
        );

        assert!(
            !status.success(),
            "an unresolved {builtin} spacing must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}",
            builtin = case.builtin,
        );
        assert!(
            stderr.contains(&expected),
            "stderr should carry `{expected}`; got: {stderr}"
        );
        assert!(
            !stderr.contains(case.pre_lambda_nickname),
            "the diagnostic must not fall back to the internal nickname \
             `{nickname}`; got: {stderr}",
            nickname = case.pre_lambda_nickname,
        );
    }
}

/// §6 row 19 — D12: an explicitly passed `iso:` is gated, and its ABSENCE is not.
///
/// The contrast is the row. A gate that fired on the defaulted value too would break
/// every existing `isosurface(solid)` call site, so both halves are needed to tell the
/// intended behaviour from an over-broad one.
///
/// THE SECOND HALF DELIBERATELY DOES NOT ASSERT EXIT 0. Measured: the absent-`iso` form
/// — and the tracked `examples/multi_kernel/voxel_to_mesh.ri` itself — exit 1 under
/// `reify eval` with `no openvdb kernel registered (call ensure_openvdb_kernel())`
/// followed by `GeometryOp::Surface is a Mesh-repr terminal anchor fed by a Voxel→Mesh
/// conversion edge`. That failure is wholly unrelated to units, and no implementer could
/// green an exit-0 assertion here. What D12 actually claims IS assertable at this
/// boundary, and is what this half asserts: the absent argument draws no units rejection.
/// The exits-0 half is discharged with the kernel registered, in
/// `crates/reify-eval/tests/isosurface_iso_option_e2e.rs`.
///
/// AN ABSENCE NEEDS AN ANCHOR. `stderr` not carrying a units rejection is also true of a
/// run that never reached `isosurface` at all — a rename, a changed keyword-argument
/// spelling, a source that stopped parsing — so on its own the second half would pass
/// having measured nothing. Two positive anchors hold it down: the `S.shell` binding
/// appears on stdout, which only happens if the defaulted call evaluated; and the run
/// fails for the openvdb reason quoted above and not some other one.
#[test]
fn the_iso_option_is_gated_but_its_absence_is_not() {
    let (status, stdout, stderr) = RiSource {
        stem: "bare_isosurface_iso",
        source: r#"module bare_isosurface_iso

structure def S {
    param size : Length = 20mm

    let solid = box(size, size, size)
    let shell = isosurface(solid, iso: 5)
}
"#,
    }
    .eval();

    assert!(
        !status.success(),
        "a bare `iso:` must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "isosurface", "iso", "Int");

    let (_, default_stdout, default_stderr) = RiSource {
        stem: "defaulted_isosurface_iso",
        source: r#"module defaulted_isosurface_iso

structure def S {
    param size : Length = 20mm

    let solid = box(size, size, size)
    let shell = isosurface(solid)
}
"#,
    }
    .eval();

    assert!(
        default_stdout.contains("S.shell = <Geometry"),
        "the defaulted `isosurface(solid)` must actually have been evaluated — without \
         this, `no units rejection` would also hold for a run that never reached \
         `isosurface`;\nstdout: {default_stdout}\nstderr: {default_stderr}"
    );
    assert!(
        default_stderr.contains("no openvdb kernel registered"),
        "the defaulted form is expected to fail for the openvdb reason this row documents, \
         not some other one; got: {default_stderr}"
    );
    assert!(
        !default_stderr.contains("iso argument expects Length"),
        "D12: an ABSENT `iso:` takes the documented default and must draw no units \
         rejection at all; got: {default_stderr}"
    );
}

/// One route into the units chokepoint: the fixture that reaches it, and the rejection
/// the user must see when it does.
///
/// The fixture is BORROWED from the row that owns it — no copy — so "the three routes
/// produce identical wording" is a statement about the same three sources the rows
/// assert on.
struct ChokepointRoute {
    fixture: &'static RiSource,
    builtin: &'static str,
    arg: &'static str,
    /// Which PRD leaf owns this route — carried so a divergence report names the owner.
    leaf: &'static str,
}

/// D9 — the invariant no single row can make: three DIFFERENT chokepoint routes produce
/// the ENTIRE rejection line identically.
///
/// It is stronger than the rows in the one way that matters, and it has to be, or it
/// would be three redundant spawns. Each row asks `stderr.contains(template)` — satisfied
/// by any line that carries the template somewhere, alongside anything else. This test
/// asks that a diagnostic line, severity prefix stripped, EQUALS the template: no leading
/// qualifier, no appended suffix, no second hint bolted onto one route's spelling. That
/// is the byte-identical wording D9 promises across PRDs 1/3/5, and the property
/// `ArgRejection::message` (`crates/reify-ir/src/arg_acceptance.rs:211-222`) being the
/// single producer is supposed to deliver.
///
/// The three routes are chosen to be genuinely different paths into that one producer: a
/// primitive (β's raw-`Value` route), a modify (γ's), and a transform (ζ's decoded
/// route). A leaf that hand-rolled its own rejection string on one of them would satisfy
/// its own row and fail here.
///
/// Divergences are COLLECTED rather than asserted one at a time: a template change should
/// report every route it broke, not stop at whichever ran first.
#[test]
fn every_units_rejection_uses_the_one_wording_template() {
    let routes = [
        ChokepointRoute {
            fixture: &BARE_BOX,
            builtin: "box",
            arg: "width",
            leaf: "β (primitive, raw-Value route)",
        },
        ChokepointRoute {
            fixture: &BARE_FILLET,
            builtin: "fillet",
            arg: "radius",
            leaf: "γ (modify)",
        },
        ChokepointRoute {
            fixture: &BARE_TRANSFORM,
            builtin: "apply_transform",
            arg: "translation.x",
            leaf: "ζ (transform, decoded route)",
        },
    ];

    let mut divergent = Vec::new();
    for route in routes {
        let (status, stdout, stderr) = route.fixture.eval();
        assert!(
            !status.success(),
            "the {leaf} route must reach the units chokepoint and exit nonzero;\n\
             stdout: {stdout}\nstderr: {stderr}",
            leaf = route.leaf,
        );
        let expected = length_rejection_line(route.builtin, route.arg, "Int");
        if !diagnostic_lines(&stderr).any(|line| line == expected) {
            divergent.push(format!(
                "  {leaf}: no diagnostic line EQUALS `{expected}`\n    got: {stderr}",
                leaf = route.leaf,
            ));
        }
    }

    assert!(
        divergent.is_empty(),
        "D9: every units rejection must be the ONE line `ArgRejection::message` produces, \
         whole, whatever route reached the chokepoint. Diverging routes:\n{}",
        divergent.join("\n"),
    );
}
