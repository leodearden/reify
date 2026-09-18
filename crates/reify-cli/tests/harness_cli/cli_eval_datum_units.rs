//! End-to-end CLI tests for the construction-datum LENGTH gate (units-length ε,
//! task 5746, R11 / decision D4 of
//! `docs/prds/v0_6/units-length-gate-completion.md`).
//!
//! This is the only altitude that proves the gate is REACHABLE. `make_plane` /
//! `make_axis` return `Value::Undef` for a bare offset/origin, and a `Value::Undef`
//! on its own prints `undef` and exits 0 — measured against `target/debug/reify`
//! before `geometry::diagnose` grew its two arms:
//!
//! ```text
//! DatumUnitsPlaneBare.p = undef
//! note: DatumUnitsPlaneBare.p is undef (because: op contract failed (OpContractViolation))
//! exit 0
//! ```
//!
//! `push_op_contract_failure` writes `undef_causes`, not the diagnostics sink, and
//! the CLI's exit gate is a pure `Severity::Error` fold — so without the classifier
//! arm the gate is invisible to the author. These rows pin the 0 -> 1 flip.
//!
//! The assertions anchor on the message HEAD only (builtin + argument +
//! expectation + `got` shape). Byte-identity with `ArgRejection::message` is
//! pinned at the unit level by reify-stdlib's
//! `length_rejection_wording_is_the_shared_arg_rejection_template`, so repeating
//! the whole sentence here would buy nothing and duplicate a contract that
//! already has an owner.

use crate::common;

/// A bare `plane_xy(0.0)` offset exits NONZERO with the shared coded units Error
/// on stderr.
#[test]
fn eval_bare_plane_offset_exits_nonzero_with_a_units_error() {
    let path = common::fixture_path("datum_units_plane_bare.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a non-LENGTH plane offset is an Error (not a Warning), so reify eval should \
         exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // One CONTIGUOUS anchor: builtin name + argument name + expected + got.
    // `offset` is the argument the author actually wrote, and the vocabulary
    // `make_plane` itself uses — not the synthesized origin coordinate.
    assert!(
        stderr.contains("plane_xy: offset argument expects Length, got Real"),
        "stderr should carry the ε units rejection naming the builtin, the argument \
         and the offending shape; got: {stderr}"
    );
    // Checked SEPARATELY so a drop of just the hint is distinguishable from a
    // reword of the base message.
    assert!(
        stderr.contains("pass a dimensioned length such as `5mm`"),
        "stderr should carry the migration hint; got: {stderr}"
    );
    // Guards the fixture's `module datum_units_plane_bare` decl: without it,
    // W_MODULE_DECL_MISSING ("expected `module datum_units_plane_bare`")
    // re-supplies the "plane" substring for free and weakens the anchor above
    // (the trap task 6155 documented for affine_scale_dim.ri).
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "datum_units_plane_bare.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// A bare `axis_x(point3(0.0, 0.0, 0.0))` origin exits NONZERO with the shared
/// coded units Error on stderr, naming all three coordinates in ONE message.
///
/// One message, not three: the decoder both the gate and the classifier read has
/// already required the three components to share one dimension, so when this
/// fires all three positions offend identically — and one edit-build cycle fixes
/// the one line the author wrote.
#[test]
fn eval_bare_axis_origin_exits_nonzero_with_a_units_error() {
    let path = common::fixture_path("datum_units_axis_bare.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a non-LENGTH axis origin is an Error (not a Warning), so reify eval should \
         exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("axis_x: ox/oy/oz argument expects Length, got Real"),
        "stderr should carry the ε units rejection naming the builtin, all three \
         origin coordinates and the offending shape; got: {stderr}"
    );
    assert!(
        stderr.contains("pass a dimensioned length such as `5mm`"),
        "stderr should carry the migration hint; got: {stderr}"
    );
    // Guards the fixture's `module datum_units_axis_bare` decl, for the reason
    // its plane sibling above states.
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "datum_units_axis_bare.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// The INSEPARABLE control: the same two datum calls with DIMENSIONED literals
/// still exit 0 and print their values.
///
/// Without it, both rows above could pass for the wrong reason — a `make_plane` /
/// `make_axis` that rejected EVERYTHING would satisfy them perfectly. It is also
/// the end-to-end proof of the two dimension claims the gate makes: the plane's
/// single LENGTH offset MIRRORS into the whole origin triple (`0 m` in the two
/// SYNTHESIZED slots, not a bare `0`), while the synthesized unit normal /
/// direction stays dimensionless (`vec(0, 0, 1)` / `vec(1, 0, 0)`) — decision
/// D3's scope lock.
///
/// The plane offset is NON-ZERO on purpose. `make_plane` writes the gated offset
/// into `origin_si[offset_index]` and zeros the rest, so a zero offset yields
/// three indistinguishable zeros and this row could not tell a correct
/// `offset_index` from a wrong or hardcoded one. (The suite is not blind to that
/// either way — `decode_plane_producer_round_trip_plane_{xy,xz,yz}` drives all
/// three names at distinct offsets — but a control that cannot fail for the
/// reason it names is not much of a control.)
///
/// The printed forms were measured against the real `target/debug/reify` binary
/// before being pinned; the value printer's number formatting is the drift-prone
/// part.
#[test]
fn eval_dimensioned_datums_exit_0_and_print_length_origins() {
    let path = common::fixture_path("datum_units_controls.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "dimensioned datum constructors must still build;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("plane(point(0 m, 0 m, 0.005 m), vec(0, 0, 1))"),
        "stdout should print the plane with the 5mm offset in the z slot, an \
         all-LENGTH origin and a bare normal; got: {stdout}"
    );
    assert!(
        stdout.contains("axis(point(0 m, 0 m, 0 m), vec(1, 0, 0))"),
        "stdout should print the axis with an all-LENGTH origin and a bare direction; \
         got: {stdout}"
    );
}
