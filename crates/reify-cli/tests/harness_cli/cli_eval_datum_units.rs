//! End-to-end CLI tests for the construction-datum LENGTH gate, across BOTH
//! halves of that family: units-length ε (task 5746, R11 / decision D4 of
//! `docs/prds/v0_6/units-length-gate-completion.md`) gates the `plane_*` /
//! `axis_*` producers, and units-length η (task 6591) gates the five
//! construction-datum constructors beside them — `midplane`, `axis_through`,
//! `plane_through`, the arity-2 `offset` and `frame_at`.
//!
//! One module for the whole gate, so a reader finds its entire user-observable
//! contract in one place. The two halves share every mechanism below; only the
//! fixtures differ.
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

/// Every BARE construction-datum POSITION operand exits NONZERO with the shared
/// coded units Error on stderr, naming the builtin and the argument the author
/// actually wrote (units-length η, task 6591).
///
/// All five rows MEASURED as exit-0 before this gate: four of them printed a
/// bare-origin datum, and `offset` printed a silent `undef`. That is the whole
/// point of asserting at this altitude — a `Value::Undef` prints `undef` and
/// exits 0, so without the classifier arm the gate is invisible to the author.
///
/// One CONTIGUOUS anchor per builtin (name + argument + expected + got). A
/// POINT/ORIGIN operand is named as the WHOLE parameter rather than per
/// coordinate: the decoder both the gate and the classifier read has already
/// required the three components to share one dimension, so all three offend
/// identically and one edit-build cycle fixes the one line the author wrote.
#[test]
fn eval_bare_datum_constructors_exit_nonzero_with_units_errors() {
    let path = common::fixture_path("datum_units_eta_bare.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a non-LENGTH datum position is an Error (not a Warning), so reify eval \
         should exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for anchor in [
        "midplane: a argument expects Length, got Int",
        "axis_through: a argument expects Length, got Int",
        "plane_through: a argument expects Length, got Int",
        // `frame_at` is DELIBERATELY ABSENT from this list, and the gap is an
        // evaluator one rather than a gate one — see
        // `eval_bare_frame_at_gate_fires_but_its_diagnostic_is_not_reachable`
        // directly below, which pins what IS observable plus the reason.
        // `offset`'s row is the DELTA, not the plane: its plane is a LENGTH
        // `plane_xy(5mm)` and only the bare `2.0` offends. It is the most
        // natural authoring error in the family — a forgotten unit on the
        // offset — and the one that used to be reinterpreted as 2 METRES.
        "offset: delta argument expects Length, got Real",
    ] {
        assert!(
            stderr.contains(anchor),
            "stderr should carry the η units rejection `{anchor}`; got: {stderr}"
        );
    }
    // Checked SEPARATELY so a drop of just the hint is distinguishable from a
    // reword of the base message.
    assert!(
        stderr.contains("pass a dimensioned length such as `5mm`"),
        "stderr should carry the migration hint; got: {stderr}"
    );
    // Guards the fixture's `module datum_units_eta_bare` decl: without it,
    // W_MODULE_DECL_MISSING re-supplies anchor substrings for free and weakens
    // every assertion above (the trap task 6155 documented).
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "datum_units_eta_bare.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// `frame_at`'s gate FIRES at the CLI — its cell goes `undef` — but its
/// diagnostic is NOT reachable from any `.ri` source today, so this row asserts
/// the former and records the latter rather than asserting a flip that cannot
/// happen.
///
/// MEASURED, not assumed. `self.x` / `self.z` are the ONLY `.ri` route to a
/// `Value::Direction` (there is no free-function Direction constructor), and an
/// INLINE `self.<datum>` projection is still `Value::Undef` during the pass that
/// runs `emit_undef_builtin_diagnostics`. The call therefore hits the strict
/// undef-ARGUMENT short-circuit and never dispatches its builtin; a later pass
/// re-evaluates the projection and produces the real value. The missing
/// `OpContractViolation` note is the discriminator: `push_op_contract_failure`
/// sits in the SAME eval arm as the diagnostics hook, so its absence shows the
/// arm was never reached — not that `geometry_diagnose` returned `None`. The
/// unit rows in reify-stdlib pin that it does not: `frame_at`'s exact message,
/// severity and code are asserted there.
///
/// Let-binding the projections is NOT a workaround and was measured too: it
/// breaks the SUCCESS path as well (`frame_at(point3(1mm, 2mm, 3mm), sx, sz)`
/// is `undef` while the inline form builds a Frame), so it would trade a
/// missing diagnostic for a broken constructor.
///
/// The gap is NOT an η artifact — task δ's long-landed gate shows the same
/// shape, `mirror(box(...), self.xy_plane)` reporting "expected a Plane value,
/// got undef" inline while the let-bound form resolves. It is filed as
/// follow-up work as task #7765 (spawned from this one); the fixture already
/// carries the call, so when that lands only this assertion changes — to the
/// real stderr anchor
/// `frame_at: o argument expects Length, got Int`.
#[test]
fn eval_bare_frame_at_gate_fires_but_its_diagnostic_is_not_reachable() {
    let path = common::fixture_path("datum_units_eta_bare.ri");
    let (_, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        stdout.contains("DatumUnitsEtaBare.f = undef"),
        "frame_at's LENGTH gate must still FIRE on a bare origin, even though its          diagnostic cannot reach stderr;
stdout: {stdout}
stderr: {stderr}"
    );
}

/// The δ-REACHABILITY control, carried by the same fixture: task δ's
/// CONSUMER-side gate stays live and reachable from real `.ri` source after all
/// five producers above are gated.
///
/// Task ε recorded, in two places, that these five are "exactly the producers
/// that keep δ's consumer gate live and reachable from real `.ri` source" — so
/// closing them looks like it retires one of decision D4's two ends. It does
/// not: `frame3` validates that its origin is a 3-component `Value::Point` and
/// never its dimension, and `Frame.xy_plane` clones that origin verbatim, so a
/// bare-origin Plane is still constructible and `mirror` still rejects it by
/// coordinate.
///
/// This is pinned as RUNTIME BEHAVIOUR rather than as prose on purpose. A
/// measurement recorded only in a doc comment goes stale silently with nothing
/// to catch it — the failure task 5746's review named when it deleted a
/// changelog paragraph from `make_plane`'s doc. The day task #7625 gates
/// `frame3`, this assertion fails loudly and D4's second end must be revisited
/// deliberately.
#[test]
fn eval_bare_datum_fixture_keeps_deltas_consumer_gate_reachable() {
    let path = common::fixture_path("datum_units_eta_bare.ri");
    let (_, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        stderr.contains("mirror: ox argument expects Length, got Int"),
        "δ's consumer-side decode_plane gate must still fire through frame3's \
         ungated origin, naming the coordinate;\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// The INSEPARABLE control: the same five datum calls with DIMENSIONED literals
/// still exit 0 and print their values.
///
/// Without it, every row above could pass for the wrong reason — a gate that
/// rejected EVERYTHING would satisfy them perfectly.
///
/// Every coordinate is DISTINCT and NON-ZERO wherever the builtin carries one
/// through (task 5746 review round 1: an all-zeros fixture cannot tell a correct
/// implementation from a hardcoded one). These constructors are exactly the kind
/// that could pass a zeroed control while dropping or transposing a coordinate:
/// `axis_through` / `plane_through` clone their first point VERBATIM, `frame_at`
/// clones its origin, and `offset` sums 5mm + 3mm = 8mm — a value no single
/// input spells.
///
/// The rows also carry decision D3's scope lock end to end: all-LENGTH origins
/// beside synthesized normals and directions that stay dimensionless, built by a
/// `frame_at` whose x/z are still bare `self.x` / `self.z` Directions.
///
/// The printed forms were measured against the real `target/debug/reify` binary
/// before being pinned; the value printer's number formatting is the drift-prone
/// part.
#[test]
fn eval_dimensioned_datum_constructors_exit_0_and_print_length_origins() {
    let path = common::fixture_path("datum_units_eta_controls.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "dimensioned construction-datum constructors must still build;\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "axis(point(0.001 m, 0.002 m, 0.003 m), vec(0, 0, 1))",
        "plane(point(0.001 m, 0.002 m, 0.003 m), vec(0, 0, 1))",
        "frame(point(0.004 m, 0.005 m, 0.006 m), [1, 0, 0, 0]q)",
        "plane(point(0 m, 0 m, 0.008 m), vec(0, 0, 1))",
        "plane(point(0 m, 0 m, 0.005 m), vec(0, 0, 1))",
    ] {
        assert!(
            stdout.contains(expected),
            "stdout should print `{expected}` — an all-LENGTH origin beside a \
             bare normal/direction; got: {stdout}"
        );
    }
}
