//! Shared validation-message constants for geometry kernel operations.
//!
//! Both `reify-kernel-fidget` and `reify-kernel-occt` must emit byte-identical
//! error messages for Sphere radius and Box dimension validation so that
//! callers (tests, log parsers, UI) can match on a single string regardless of
//! which kernel is active.  The constants here are the single source of truth:
//!
//! - Fidget production site: `crates/reify-kernel-fidget/src/kernel.rs` —
//!   `execute(Sphere)` and `execute(Box)` arms.
//! - OCCT production site: `crates/reify-kernel-occt/src/lib.rs` —
//!   `OcctKernel::execute` `Sphere` and `Box` arms.
//!
//! Every test that asserts the error message should `assert_eq!` against these
//! constants rather than using substring containment, so that message drift
//! between the two kernels is caught at compile time rather than by accident.
//!
//! # The C4 kernel LENGTH tripwire
//!
//! This module also owns the **kernel LENGTH tripwire** mandated by contract
//! C4 of `docs/prds/v0_6/units-length-gate-completion.md`: the classifier
//! [`check_length_field`] and the two shared message formatters
//! [`non_length_kernel_field_message`] and
//! [`non_numeric_kernel_field_message`].  They are a third member of the same
//! single-source-of-truth family as the constants above, and are governed by
//! the same rule — both kernels emit the *same* string, produced here, so a
//! test can `assert_eq!` against one source rather than substring-matching two
//! drifting literals.
//!
//! ## (a) It is a TRIPWIRE, not a gate
//!
//! PRD D5 / ratified decision 4.  A violation is *reported*, never rejected:
//! [`check_length_field`] returns a message and the caller's accept/reject
//! disposition is unchanged.  Two independent reasons a hard kernel gate is
//! not available:
//!
//! 1. Hundreds of legitimate kernel-side fixtures pass a bare `Value::Real`.
//!    Measured at commit `15885c5a1b` (the base of task #5751): **539**
//!    `Value::Real(` / `Value::Int(` occurrences inside
//!    `crates/reify-kernel-occt/src/lib.rs`'s own `mod tests`; **39 of the 52**
//!    `crates/reify-kernel-occt/tests/harness_occt/*.rs` modules; **43** across
//!    the 17 in-crate `crates/reify-kernel-fidget/src/kernel.rs` tests; and
//!    **106** `.rs` files workspace-wide mention both `GeometryOp::` and
//!    `Value::Real(`.  Rejecting those is exactly the breakage D5 forbids.
//! 2. A kernel error carries neither a source span nor an argument name, so a
//!    gate could not produce an actionable diagnostic even when it fired.
//!
//! ## (b) It is the SECOND, INDEPENDENT detection layer
//!
//! The first layer is the closure guard (leaf ι), which is **not yet landed**.
//! The two are deliberately independent: the closure guard reasons about where
//! a value came from, this tripwire observes what actually arrived at the
//! kernel boundary.  Neither subsumes the other, and this one keeps working if
//! the first is bypassed or has a hole.
//!
//! **Emission stays in the kernels.**  This module owns the pure
//! classifier/formatters/arming state only; each kernel emits its own
//! `tracing::warn!` whose `target:` names the emitting crate, per the house
//! pattern at `crates/reify-kernel-gmsh/src/repair.rs:135-143` and
//! `crates/reify-kernel-manifold/src/kernel.rs:1159-1167`.  That keeps
//! `reify-ir` — a dependency of 13+ crates — free of a `tracing` edge.
//!
//! ## (c) C2 corollary: what this tripwire is actually watching for
//!
//! On `main` today, **no route constructs a [`crate::geometry::GeometryOp`]
//! outside `compile_geometry_op`**: there is no serde deserialization path
//! into `GeometryOp`, and the one other construction site
//! (`crates/reify-kernel-occt/src/handle.rs:887`,
//! `OcctKernelHandle::extrude_with_history`) is not live.  A route that did so
//! would be out of contract, and this tripwire is the **runtime detector** if
//! one ever appears.
//!
//! ## (d) A fired tripwire means an EVAL-LAYER hole, not a kernel bug
//!
//! The gate that *should* have caught a bare value first is
//! `required_length_value` / `required_length_values` at
//! `crates/reify-eval/src/geometry_ops.rs:482`/`:523`.  Start a diagnosis
//! there — the kernel is the *detector*, not the defect.
//!
//! ## (e) The five deliberately ungated OCCT fields
//!
//! `OcctKernel::execute` has 46 numeric-extraction sites, split 46 = 41 + 3 + 2.
//! The 41 LENGTH-semantic ones go through the kernel's `extract_length_f64`;
//! these five stay on the context-free `extract_f64`, each marked at its call
//! site with a `// not length-semantic:` comment:
//!
//! - `HalfSpace`'s `nx` / `ny` / `nz` — components of a **dimensionless unit
//!   normal**, not lengths.  Gating them would fire on every correct call.
//! - `CircularPattern.angle` and `Draft.angle` — **ANGLE**, which is the
//!   surface of PRD 3, not this one.
//!
//! Fidget's four sites (`Sphere.radius`, `Box.width`/`height`/`depth`) are all
//! LENGTH-semantic, so it has no ungated site.
//!
//! `occt_non_length_fields_stay_ungated` is the anti-over-reach control that
//! keeps a blanket conversion of all 46 sites from passing, and
//! `occt_every_length_field_is_gated` is the completeness check over all 41
//! gated pairs.
//!
//! ## Contract
//!
//! `docs/prds/v0_6/units-length-gate-completion.md` — C4 (the tripwire itself),
//! D5 (detector-never-gate), boundary rows 13 (debug assertion names op kind
//! AND field) and 14 (release build reports without changing accept/reject).

/// Error message emitted when a Sphere `radius` value fails the
/// finite-and-strictly-positive check.
///
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE: &str =
    "sphere radius must be a finite positive value";

/// Error message emitted when any Box dimension (`width`, `height`, or `depth`)
/// fails the finite-and-strictly-positive check.
///
/// Note the plural "values": all three dimensions are validated in a single
/// combined check, so a single message covers any dimension failure.
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE: &str =
    "box dimensions must be finite positive values";

// ── C4 kernel LENGTH tripwire: classifier + shared message formatters ────────

use crate::value::Value;
use reify_core::DimensionVector;

/// Classify a [`Value`] arriving at a **length-semantic kernel field**.
///
/// Returns `None` when `v` carries [`DimensionVector::LENGTH`] — the only
/// in-contract shape for a length field.  Otherwise returns `Some(msg)`, where
/// `msg` is [`non_length_kernel_field_message`]'s output and therefore names
/// BOTH the op kind and the field (boundary rows 13/14; C4 forbids a bare
/// `"expected numeric value"` here).
///
/// A bare `Value::Real` / `Value::Int` is DIMENSIONLESS under
/// [`Value::dimension`], so it classifies as a violation — which is exactly the
/// case the tripwire exists to catch.
///
/// This is a **detector, never a gate**: the caller must report the message and
/// then proceed with the disposition it would have had anyway.  See the module
/// docs for the D5 rationale and the C2 corollary.
///
/// `op_kind` should come from [`crate::geometry::GeometryOp::kind_name`] so it
/// stays correct as variants are added; `field` is the literal field name at
/// the call site.
pub fn check_length_field(op_kind: &str, field: &str, v: &Value) -> Option<String> {
    if v.dimension() == DimensionVector::LENGTH {
        return None;
    }
    let msg = non_length_kernel_field_message(op_kind, field, &got_label(v));
    // The opt-in debug assertion. Compiled out entirely in release, so C4's
    // "never changes release accept/reject behaviour" holds by construction —
    // and it panics with *this* `msg`, so the assertion text and the release
    // diagnostic text are one string, never two literals free to drift.
    #[cfg(debug_assertions)]
    if length_tripwire_assert_armed() {
        panic!("{msg}");
    }
    Some(msg)
}

// ── arming state for the opt-in debug assertion ──────────────────────────────

thread_local! {
    /// Whether the debug assertion is armed **on this thread**.
    ///
    /// THREAD-LOCAL, deliberately — not a global `AtomicBool`. `cargo test`
    /// runs tests in parallel threads inside one process, so a process-global
    /// arm would let one test's deliberate injection panic an unrelated
    /// thread's legitimate bare-`Value` op, producing order-dependent flakes
    /// across the ~1500 legacy fixtures. Thread-local scoping makes the arm
    /// invisible outside the test that took it.
    ///
    /// The consequence is that an arm does NOT cross a kernel-thread boundary
    /// (e.g. `OcctKernelHandle`'s worker thread), so an injection test must
    /// drive the kernel on the test thread — which every existing in-crate
    /// occt test already does.
    static ASSERT_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Is the [`check_length_field`] debug assertion armed on this thread?
///
/// Always `false` unless a [`LengthTripwireAssertGuard`] from
/// [`arm_length_tripwire_assert`] is alive on this thread. Reads correctly in
/// both profiles; in release the assertion it guards is compiled out, so an
/// armed release build still only *reports*.
pub fn length_tripwire_assert_armed() -> bool {
    ASSERT_ARMED.with(std::cell::Cell::get)
}

/// RAII arm for the [`check_length_field`] debug assertion.
///
/// Restores the previous arm state on `Drop`, so nesting composes and a
/// `#[should_panic]` unwind still cleans up.
#[must_use = "the assertion is disarmed as soon as the guard is dropped"]
pub struct LengthTripwireAssertGuard {
    prev: bool,
}

impl Drop for LengthTripwireAssertGuard {
    fn drop(&mut self) {
        ASSERT_ARMED.with(|c| c.set(self.prev));
    }
}

/// Arm the [`check_length_field`] debug assertion for the current thread until
/// the returned guard is dropped.
///
/// # The default is DISARMED, and why
///
/// C4's letter asks for an assertion "under `cfg(debug_assertions)`". A
/// *default-armed* assertion is not implementable: it would panic the exact
/// fixtures PRD D5 exists to protect. Measured at commit `15885c5a1b`, the
/// base of task #5751 —
///
/// - **539** `Value::Real(` / `Value::Int(` occurrences inside
///   `crates/reify-kernel-occt/src/lib.rs`'s own `mod tests` (which begins at
///   `:4887`; e.g. `make_box_20_10_5`, which feeds three bare `Value::Real`s
///   straight into `GeometryOp::Box`);
/// - **39 of the 52** `crates/reify-kernel-occt/tests/harness_occt/*.rs`
///   modules contain `Value::Real(`;
/// - **43** occurrences across the **17** in-crate
///   `crates/reify-kernel-fidget/src/kernel.rs` tests;
/// - **106** `.rs` files workspace-wide mention both `GeometryOp::` and
///   `Value::Real(` — among them
///   `reify-eval/tests/harness_kernel_realization/*`,
///   `reify-kernel-conformance`,
///   `reify-test-support/src/{fixtures,mocks,kernel_assertions}.rs` and
///   `gui/src-tauri/src/tests/engine_tests.rs`.
///
/// A default-armed assertion would panic every fixture that feeds a bare
/// `Value::Real` into a length-semantic `GeometryOp` field, which is exactly
/// the breakage D5 forbids. Keying the default on `cfg(test)` does not help
/// either: every external test binary links the kernel libs WITHOUT
/// `cfg(test)` and would be armed regardless.
///
/// The in-repo precedent for the identical call is
/// `crates/reify-kernel-gmsh/src/repair.rs:126-129`: "Rather than a
/// `debug_assert!` (which hard-crashes debug/test builds and CI), we emit a
/// `tracing::warn!` so the concern stays visible in any build without crashing
/// tests."
///
/// **The detector is never dormant.** The RELEASE diagnostic half is
/// unconditional in both profiles — every kernel length site reports a
/// violation naming op kind and field whether or not the assertion is armed.
/// Arming only escalates that report to a panic, and only for the arming
/// thread, so a test can assert the tripwire fires without a subscriber.
pub fn arm_length_tripwire_assert() -> LengthTripwireAssertGuard {
    let prev = ASSERT_ARMED.with(|c| c.replace(true));
    LengthTripwireAssertGuard { prev }
}

/// The shared diagnostic for a **non-LENGTH** value at a length-semantic kernel
/// field.
///
/// Mirrors the wording shape of `reify-eval`'s
/// `arg_acceptance.rs:152` template (`"{builtin}: {arg_name} argument expects
/// {expected}, got {got}"`) so kernel-layer and eval-layer unit diagnostics
/// read alike.  The wording shape only is mirrored — `arg_acceptance` lives in
/// `reify-eval` and the adapter → eval dependency direction is deliberately
/// inverted (documented in both kernel `Cargo.toml`s), so the code cannot be
/// shared.
///
/// Both kernels MUST emit exactly this string; tests `assert_eq!` against it.
pub fn non_length_kernel_field_message(op_kind: &str, field: &str, got: &str) -> String {
    format!("kernel length tripwire: {op_kind}.{field} expects Length, got {got}")
}

/// The length-field replacement for the legacy context-free
/// `"expected numeric value"` kernel error.
///
/// Emitted as the `Err` payload when a length-semantic field receives a value
/// that `Value::as_f64` cannot read at all.  C4 requires this string to name
/// the op kind and the field — "never a bare `expected numeric value`" — so the
/// failure is attributable without a source span.
///
/// Changing an error *message* is not changing accept/reject behaviour: the
/// same inputs are still `Err`, only the string improves.
pub fn non_numeric_kernel_field_message(op_kind: &str, field: &str) -> String {
    format!("kernel length tripwire: {op_kind}.{field} expects Length, got a non-numeric value")
}

/// Name the observed value for the `got` slot of
/// [`non_length_kernel_field_message`].
///
/// For a [`Value::Scalar`] the *dimension* is what matters — a
/// `Scalar<Mass>` at a length field is a different bug from a bare `Real` — so
/// the label carries it, via [`DimensionVector::canonical_name`] when the
/// dimension is a named singleton and the raw exponent rendering
/// ([`DimensionVector`]'s `Display`, e.g. `"m·kg^-1"`, `"dimensionless"`)
/// otherwise.
///
/// There is no `Value::type_name()` in `reify-ir`, and `reify-eval`'s
/// `value_short_label` is unreachable across the inverted adapter → eval
/// dependency edge, so this is a small local match.  The trailing wildcard
/// keeps a newly added `Value` variant from breaking the build; it can only
/// ever be reached by a value that is already a tripwire violation.
fn got_label(v: &Value) -> String {
    match v {
        Value::Bool(_) => "Bool".to_string(),
        Value::Int(_) => "Int".to_string(),
        Value::Real(_) => "Real".to_string(),
        Value::String(_) => "String".to_string(),
        Value::Scalar { dimension, .. } => match dimension.canonical_name() {
            Some(name) => format!("Scalar<{name}>"),
            None => format!("Scalar<{dimension}>"),
        },
        Value::Undef => "Undef".to_string(),
        Value::List(_) => "List".to_string(),
        Value::Point(_) => "Point".to_string(),
        Value::Vector(_) => "Vector".to_string(),
        Value::Direction { .. } => "Direction".to_string(),
        Value::Enum { .. } => "Enum".to_string(),
        Value::Option(_) => "Option".to_string(),
        Value::GeometryHandle { .. } => "GeometryHandle".to_string(),
        other => format!("{}", ValueVariantName(other)),
    }
}

/// `Display` shim naming a [`Value`] variant not enumerated in [`got_label`].
///
/// Renders the leading identifier of the value's `Debug` form, which is the
/// variant name for every `Value` variant, without dragging the (potentially
/// very large) payload into a diagnostic string.
struct ValueVariantName<'a>(&'a Value);

impl std::fmt::Display for ValueVariantName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dbg = format!("{:?}", self.0);
        let name: &str = dbg
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if name.is_empty() {
            f.write_str("value")
        } else {
            f.write_str(name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use reify_core::DimensionVector;

    /// The five non-LENGTH values a kernel length field can receive.
    ///
    /// `Real`/`Int` are the bare-literal case the C4 tripwire exists to
    /// detect; `Scalar{MASS}` is a *dimensioned but wrong* value;
    /// `String`/`Undef` are the non-numeric cases that also fail
    /// `Value::as_f64`.
    fn non_length_values() -> Vec<Value> {
        vec![
            Value::Real(1.0),
            Value::Int(1),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::String("x".into()),
            Value::Undef,
        ]
    }

    /// The legacy context-free kernel error string that C4 forbids on a
    /// length field ("never a bare `expected numeric value`").
    const LEGACY_BARE: &str = "expected numeric value";

    /// A properly dimensioned LENGTH value is not a violation.
    #[test]
    fn dimensioned_length_value_is_not_a_violation() {
        assert_eq!(
            check_length_field("Fillet", "radius", &Value::length(0.001)),
            None
        );
    }

    /// Every non-LENGTH value is a violation whose message names BOTH the op
    /// kind and the field — boundary rows 13/14's message contract, stated
    /// once and profile-independently.
    #[test]
    fn every_non_length_value_is_a_violation_naming_op_kind_and_field() {
        for v in non_length_values() {
            let msg = check_length_field("Fillet", "radius", &v)
                .unwrap_or_else(|| panic!("expected a violation for {v:?}"));
            assert!(
                msg.contains("Fillet"),
                "message for {v:?} does not name the op kind: {msg}"
            );
            assert!(
                msg.contains("radius"),
                "message for {v:?} does not name the field: {msg}"
            );
        }
    }

    /// C4: the length-field diagnostic is never the bare legacy string.
    #[test]
    fn tripwire_message_is_never_the_bare_legacy_string() {
        for v in non_length_values() {
            let msg = check_length_field("Fillet", "radius", &v)
                .unwrap_or_else(|| panic!("expected a violation for {v:?}"));
            assert_ne!(msg, LEGACY_BARE);
            assert!(
                !msg.contains(LEGACY_BARE),
                "message for {v:?} still carries the bare legacy string: {msg}"
            );
        }
        assert!(!non_numeric_kernel_field_message("Fillet", "radius").contains(LEGACY_BARE));
    }

    /// Both formatters name op kind and field, and `check_length_field`'s
    /// message is EXACTLY `non_length_kernel_field_message`'s output — the
    /// `assert_eq!`-against-a-single-source doctrine this module's docs
    /// mandate, so kernel-side drift is a compile/test failure, not an
    /// accident of substring containment.
    #[test]
    fn formatters_name_op_kind_and_field_and_anchor_the_classifier() {
        let non_length = non_length_kernel_field_message("Extrude", "distance", "Real");
        assert!(non_length.contains("Extrude"), "{non_length}");
        assert!(non_length.contains("distance"), "{non_length}");

        let non_numeric = non_numeric_kernel_field_message("Extrude", "distance");
        assert!(non_numeric.contains("Extrude"), "{non_numeric}");
        assert!(non_numeric.contains("distance"), "{non_numeric}");

        // Single-source anchor: the classifier returns the formatter's own
        // output verbatim.
        assert_eq!(
            check_length_field("Extrude", "distance", &Value::Real(1.0)),
            Some(non_length_kernel_field_message("Extrude", "distance", "Real"))
        );
    }

    /// Table-driven over four distinct (op kind, field) pairs, so a formatter
    /// that hardcodes a single op or field cannot pass.
    #[test]
    fn formatters_are_parameterised_over_op_kind_and_field() {
        let pairs = [
            ("Box", "width"),
            ("Fillet", "radius"),
            ("Extrude", "distance"),
            ("Shell", "thickness"),
        ];
        for (op_kind, field) in pairs {
            let msg = check_length_field(op_kind, field, &Value::Real(1.0))
                .unwrap_or_else(|| panic!("expected a violation for {op_kind}.{field}"));
            assert!(msg.contains(op_kind), "{op_kind}.{field}: {msg}");
            assert!(msg.contains(field), "{op_kind}.{field}: {msg}");
            assert_eq!(
                msg,
                non_length_kernel_field_message(op_kind, field, "Real"),
                "classifier message drifted from the shared formatter"
            );
            let non_numeric = non_numeric_kernel_field_message(op_kind, field);
            assert!(non_numeric.contains(op_kind), "{op_kind}.{field}: {non_numeric}");
            assert!(non_numeric.contains(field), "{op_kind}.{field}: {non_numeric}");
        }

        // Distinct pairs must produce distinct messages — a formatter that
        // ignores its arguments would collapse them.
        let a = non_length_kernel_field_message("Box", "width", "Real");
        let b = non_length_kernel_field_message("Shell", "thickness", "Real");
        assert_ne!(a, b);
    }

    /// The `got` label distinguishes the observed variant, and names the
    /// dimension for a dimensioned-but-wrong `Scalar`.
    #[test]
    fn violation_message_labels_the_observed_value() {
        let real = check_length_field("Box", "width", &Value::Real(1.0)).unwrap();
        let int = check_length_field("Box", "width", &Value::Int(1)).unwrap();
        assert_ne!(
            real, int,
            "Real and Int must not share a `got` label: {real}"
        );

        let mass = check_length_field(
            "Box",
            "width",
            &Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        )
        .unwrap();
        assert!(
            mass.contains("Mass"),
            "a dimensioned-but-wrong Scalar must name its dimension: {mass}"
        );
    }

    // ── the opt-in debug assertion and its RAII arming guard ─────────────────

    /// Boundary row 13, half 1: ARMED, in a debug build, a violation PANICS
    /// with a message naming the OP KIND.
    ///
    /// The `#[cfg(debug_assertions)]` attribute is MANDATORY on a
    /// `#[should_panic]` test for a debug-only assertion: in release the assert
    /// is compiled out and an ungated test would falsely pass. This is the
    /// in-repo rule documented at `crates/reify-kernel-occt/src/lib.rs:5822-5835`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Fillet")]
    fn armed_debug_assertion_panics_naming_the_op_kind() {
        let _g = arm_length_tripwire_assert();
        let _ = check_length_field("Fillet", "radius", &Value::Real(1.0));
    }

    /// Boundary row 13, half 2: the twin pinning the FIELD NAME.
    ///
    /// `should_panic(expected = ...)` takes a single substring, so both halves
    /// of "names BOTH the op kind and the field name" need their own test.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "radius")]
    fn armed_debug_assertion_panics_naming_the_field() {
        let _g = arm_length_tripwire_assert();
        let _ = check_length_field("Fillet", "radius", &Value::Real(1.0));
    }

    /// DEFAULT IS DISARMED, in every profile.
    ///
    /// This is the property that keeps the ~1500 legacy bare-`Value` kernel
    /// fixtures green (PRD D5): unarmed, a violation is *reported*, never
    /// raised.
    #[test]
    fn unarmed_violation_reports_and_never_panics() {
        assert!(!length_tripwire_assert_armed());
        let msg = check_length_field("Fillet", "radius", &Value::Real(1.0))
            .expect("a bare Real at a length field is a violation");
        assert!(msg.contains("Fillet"), "{msg}");
        assert!(msg.contains("radius"), "{msg}");
    }

    /// The guard restores the PREVIOUS arm state on `Drop`, so an armed scope
    /// cannot leak into the rest of the thread — including out of a
    /// `#[should_panic]` unwind.
    #[test]
    fn guard_restores_previous_state_on_drop() {
        assert!(!length_tripwire_assert_armed());
        {
            let _g = arm_length_tripwire_assert();
            assert!(length_tripwire_assert_armed());
        }
        assert!(!length_tripwire_assert_armed());

        // ...and the unarmed behaviour is genuinely back.
        let msg = check_length_field("Fillet", "radius", &Value::Real(1.0));
        assert!(msg.is_some());
    }

    /// Nesting composes: the inner guard's `Drop` restores the OUTER arm,
    /// not the process default.
    #[test]
    fn nested_guards_compose() {
        assert!(!length_tripwire_assert_armed());
        let outer = arm_length_tripwire_assert();
        assert!(length_tripwire_assert_armed());
        {
            let _inner = arm_length_tripwire_assert();
            assert!(length_tripwire_assert_armed());
        }
        assert!(
            length_tripwire_assert_armed(),
            "inner guard's Drop must restore the OUTER arm, not the default"
        );
        drop(outer);
        assert!(!length_tripwire_assert_armed());
    }

    /// **Boundary row 14 — the release contract.**
    ///
    /// In a release build the panic arm is compiled out entirely, so even an
    /// ARMED violation must return `Some(msg)` naming op kind and field rather
    /// than panicking. C4: the tripwire never changes release accept/reject
    /// behaviour.
    ///
    /// This runs for real on the merge gate, which forces `--profile both`
    /// (`scripts/verify.sh`, `DF_VERIFY_ROLE=merge`).
    #[cfg(not(debug_assertions))]
    #[test]
    fn release_armed_violation_reports_and_never_panics() {
        let _g = arm_length_tripwire_assert();
        assert!(length_tripwire_assert_armed());
        let msg = check_length_field("Fillet", "radius", &Value::Real(1.0))
            .expect("a bare Real at a length field is a violation in release too");
        assert!(msg.contains("Fillet"), "{msg}");
        assert!(msg.contains("radius"), "{msg}");
    }

    /// Control: the tripwire must not fire on CORRECT input, armed or not.
    /// Profile-independent — a debug build would panic here if it did.
    #[test]
    fn armed_length_value_is_still_not_a_violation() {
        let _g = arm_length_tripwire_assert();
        assert_eq!(
            check_length_field("Fillet", "radius", &Value::length(0.001)),
            None
        );
    }

    /// **Cross-kernel byte-identity (step-9D).**
    ///
    /// `reify-ir` cannot depend on either kernel crate, so the identity is
    /// established structurally: there is exactly ONE producer of the
    /// diagnostic, and both kernels are required to emit its output verbatim.
    /// The per-kernel half of this claim lives in
    /// `fidget_length_field_enumeration_is_complete` and
    /// `occt_every_length_field_is_gated`, each of which `assert_eq!`s the
    /// captured message against `check_length_field`'s return for the same
    /// `(op_kind, field, value)`.
    ///
    /// Here we pin the properties that make that possible: the message is a
    /// pure function of `(op_kind, field, value)` with no kernel identity in
    /// it, so "what fidget emits" and "what occt emits" are the same `String`
    /// by construction rather than by two literals happening to agree.
    #[test]
    fn diagnostic_is_kernel_independent_so_both_kernels_emit_one_string() {
        for (op_kind, field) in [("Sphere", "radius"), ("Box", "width")] {
            let v = Value::Real(1.0);

            // Two independent evaluations stand in for the two kernels'
            // call sites; both go through the single shared producer.
            let as_fidget_would = check_length_field(op_kind, field, &v);
            let as_occt_would = check_length_field(op_kind, field, &v);
            assert_eq!(as_fidget_would, as_occt_would);

            let msg = as_fidget_would.expect("a bare Real is a violation");

            // No kernel identity leaks into the shared string — that is what
            // keeps the two emissions byte-identical. Kernel attribution is
            // carried by the `tracing` event's `target:`
            // (`reify_kernel_{fidget,occt}::length_tripwire`), not by the text.
            for kernel_token in ["fidget", "Fidget", "occt", "Occt", "OCCT"] {
                assert!(
                    !msg.contains(kernel_token),
                    "the shared diagnostic must not name a kernel ({kernel_token}): {msg}"
                );
            }

            // ...and it is a pure function of its inputs.
            assert_eq!(msg, non_length_kernel_field_message(op_kind, field, "Real"));
        }
    }
}
