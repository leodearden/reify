use crate::common;

/// End-to-end CLI regression for task 4145: `reify eval` surfaces geometry-query
/// value cells (`mass`, `centroid`) for a `Physical` structure.
///
/// Before the kernel-backed build() path was gated into cmd_eval, both cells
/// printed "undef" because `Engine::new(None) + eval()` never runs the
/// `run_post_processes`/`post_process_geometry_queries` geometry pipeline.
///
/// Exit status is asserted unconditionally (exit 0 for a clean file regardless
/// of OCCT availability). Mass/centroid value assertions are gated on
/// `reify_kernel_occt::OCCT_AVAILABLE` so stub-mode CI runners skip cleanly.
#[test]
fn eval_spec_shape_physical_surfaces_mass_and_centroid() {
    let path = common::example_path("spec-shape-physical.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — this is a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval spec-shape-physical.ri should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Geometry assertions only when the OCCT kernel is compiled in.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mass/centroid assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // --- mass ---
    // Expect a line like "Bracket.mass = 0.0471 kg" (or similar SI display).
    // The analytic value is V * density = 6e-6 m³ * 7850 kg·m⁻³ = 0.0471 kg;
    // the committed golden pins 4.710000e-2 [kg] via the same build() path.
    // We allow a small band [0.046, 0.048] to accommodate f64 display rounding.
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("Bracket.mass"))
        .unwrap_or_else(|| {
            panic!("expected a 'Bracket.mass' line in stdout.\nstdout: {stdout}\nstderr: {stderr}")
        });

    // RHS is everything after the '='
    let mass_rhs = mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("Bracket.mass line has no '=': {mass_line}"));

    assert_ne!(
        mass_rhs, "undef",
        "Bracket.mass must not be undef (kernel path must have fired).\nline: {mass_line}"
    );

    assert!(
        mass_rhs.contains("kg"),
        "Bracket.mass RHS must contain 'kg' (MASS dimension).\nline: {mass_line}"
    );

    // The leading token of the RHS should be the numeric SI value.
    // Value::Display for a Scalar emits "<si_value> <dimension>".
    let numeric_token = mass_rhs.split_whitespace().next().unwrap_or("");
    let mass_kg: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse Bracket.mass numeric token as f64: {numeric_token:?}\nline: {mass_line}"
        )
    });
    assert!(
        (0.046..=0.048).contains(&mass_kg),
        "Bracket.mass = {mass_kg} kg is outside the expected band [0.046, 0.048] kg.\nline: {mass_line}"
    );

    // --- centroid ---
    // Expect a line like "Bracket.centroid = point(0, 0, 0)" (from the golden).
    // We only pin non-undef and the "point(" prefix — not exact coordinates —
    // because cmd_eval renders via raw Value::Display which may have f64 jitter.
    let centroid_line = stdout
        .lines()
        .find(|l| l.contains("Bracket.centroid"))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'Bracket.centroid' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let centroid_rhs = centroid_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("Bracket.centroid line has no '=': {centroid_line}"));

    assert_ne!(
        centroid_rhs, "undef",
        "Bracket.centroid must not be undef.\nline: {centroid_line}"
    );

    assert!(
        centroid_rhs.starts_with("point("),
        "Bracket.centroid RHS must start with 'point('.\nline: {centroid_line}"
    );

    // --- no error diagnostics ---
    // spec-shape-physical.ri is a clean file; stderr should contain no
    // "Error:" lines from the engine.
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}

/// End-to-end CLI regression for task 4168 (geom-transform ε): `arbitrary_pattern`'s
/// LIST form — a `List<Transform<3>>` carrying a full per-instance rigid transform
/// (rotation + translation) — must be honored end-to-end through `reify eval`,
/// additively alongside the existing scalar-triple (translation-only) form
/// (superseding the phantom-partial task 323).
///
/// examples/pattern_arbitrary_transforms.ri unions `box(2mm,2mm,10mm)` with a
/// single Y-90-rotated copy (zero translation) via a `Physical`-conforming
/// structure (mirrors spec-shape-physical.ri), so `reify eval` surfaces
/// `mass = volume(geometry) * material.density` as a real `Scalar<Mass>`.
///
/// Exit status is asserted unconditionally (exit 0 for a clean file regardless
/// of OCCT availability). The non-undef / numeric-band mass assertions are
/// gated on `reify_kernel_occt::OCCT_AVAILABLE`, mirroring
/// `eval_spec_shape_physical_surfaces_mass_and_centroid` (without OCCT,
/// `volume(geometry)` cannot be dispatched, so `mass` is legitimately undef —
/// same status as `Rigid.moment_of_inertia`, see structural_physical.ri).
/// The band is chosen to fall strictly between the rotation-honored union
/// value (~5.652e-4 kg) and the value a rotation-dropped regression would
/// produce (~3.14e-4 kg, i.e. the single un-rotated box — task 323's
/// translation-only behaviour), so this test would catch the per-instance
/// rotation silently being dropped anywhere in the compiler/eval/kernel chain.
///
/// RED (pre-step-10): examples/pattern_arbitrary_transforms.ri does not exist
/// yet, so `run_subcommand` invokes `reify eval` on a nonexistent path, which
/// fails to parse/compile and exits non-zero — failing the unconditional
/// `status.success()` assertion below.
#[test]
fn eval_pattern_arbitrary_transforms_honors_rotation() {
    let path = common::example_path("pattern_arbitrary_transforms.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — this is a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval pattern_arbitrary_transforms.ri should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Geometry assertions only when the OCCT kernel is compiled in.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mass assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Expect a line like "ArbitraryTransformBracket.mass = 5.652000e-4 kg".
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("ArbitraryTransformBracket.mass"))
        .unwrap_or_else(|| {
            panic!(
                "expected an 'ArbitraryTransformBracket.mass' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let mass_rhs = mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("ArbitraryTransformBracket.mass line has no '=': {mass_line}"));

    assert_ne!(
        mass_rhs, "undef",
        "ArbitraryTransformBracket.mass must not be undef (kernel path must have fired).\nline: {mass_line}"
    );

    assert!(
        mass_rhs.contains("kg"),
        "ArbitraryTransformBracket.mass RHS must contain 'kg' (MASS dimension).\nline: {mass_line}"
    );

    let numeric_token = mass_rhs.split_whitespace().next().unwrap_or("");
    let mass_kg: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse ArbitraryTransformBracket.mass numeric token as f64: {numeric_token:?}\nline: {mass_line}"
        )
    });

    // Rotation-honored union: 7.2e-8 m^3 * 7850 kg/m^3 = 5.652e-4 kg. This band
    // comfortably contains that value while excluding the rotation-dropped
    // value (~3.14e-4 kg, the single un-rotated box).
    assert!(
        (0.00050..=0.00062).contains(&mass_kg),
        "ArbitraryTransformBracket.mass = {mass_kg} kg is outside the rotation-honored band \
         [0.00050, 0.00062] kg — expected ~5.652e-4 kg for box(2,2,10) unioned with its \
         Y-90-rotated copy. A value near 3.14e-4 kg would indicate the per-instance rotation \
         was dropped (task 323's translation-only regression).\nline: {mass_line}"
    );

    // --- no error diagnostics ---
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}

/// End-to-end CLI regression for task 5208: curated 3-arg
/// `fillet(solid, <edge selector>, r)` must be reachable through the **production
/// `.ri` pipeline** — the literal `reify eval` entry point a designer types.
///
/// Before task 5208 the capability was a phantom: the producers (3205 fillet
/// field / 4360 in-loop dispatch / 4362 UnifiedDag default) had landed and the
/// in-process e2e passed, but every passing test drove a FLAT
/// `let b = box(); let e = edges_at_height(b, …); let f = fillet(b, e, 2mm)`
/// program with a *named-let* selector. The shapes a designer actually writes —
/// an **inline** selector expression in the fillet call — died at build time with
/// `error: … [edge selector evaluated to Undef]`, cascading into
/// `unresolvable GeomRef::Sub(<name>)` for any downstream consumer, so
/// `reify eval` exited FAILURE with a wall of geometry-op errors and no mass.
///
/// `examples/topology_selectors/bottom_deck_selectors.ri` (task 5208 prereq-1)
/// exercises all three previously-broken shapes in one designer-authored file:
///   1. three blank-box `let`s filleted via an inline `edges_parallel_to` on that
///      same `let` (the plan-view vertical corners),
///   2. curated fillets applied to a boolean-composed (`difference`) body, and
///   3. four CHAINED curated fillets, each stage a named `let` referencing the
///      prior via an inline `edges_at_height`.
///
/// The structure conforms to `Physical`, so a clean build surfaces
/// `mass = volume(geometry) * material.density` over the final filleted body —
/// the observable designer signal this test pins.
///
/// Exit status and the stderr guards are asserted unconditionally (a clean file
/// must exit 0 whether or not OCCT is compiled in — without OCCT the curated
/// fillet is simply not dispatched, it must not *error*). The mass assertions are
/// gated on `reify_kernel_occt::OCCT_AVAILABLE`, mirroring
/// `eval_spec_shape_physical_surfaces_mass_and_centroid` above.
///
/// Scope note (no lockstep duplication): the strict *directional* geometric
/// invariant for this example — filleted volume strictly less than the un-filleted
/// blank — is pinned in-process by
/// `crates/reify-eval/tests/fillet_curated_edges_e2e.rs`. This CLI test
/// deliberately pins only what is unique to the CLI surface: the process exits 0,
/// stderr carries no geometry-op error, and `mass` reaches stdout as a real
/// positive `Scalar<Mass>` rather than `undef`.
///
/// RED (pre-fix, verified on this example at HEAD~ of the seam fix): `reify eval`
/// exited FAILURE with six `failed to compile geometry operation` /
/// `curated edge selection …` errors and `mass = undef`.
#[test]
fn eval_bottom_deck_selectors_curated_fillets_build_clean() {
    let path = common::example_path("topology_selectors/bottom_deck_selectors.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval topology_selectors/bottom_deck_selectors.ri should exit 0 — curated 3-arg \
         fillet must be reachable through the production .ri pipeline (task 5208).\
         \nstdout: {stdout}\nstderr: {stderr}"
    );

    // No Error-severity diagnostic reached the designer. cmd_eval renders each
    // diagnostic as "<severity>: <message>" on stderr (Severity::Error Displays
    // lowercase "error"), so an Error diagnostic is exactly an "error:" line.
    // Warnings (e.g. "topology correspondence dropped") are expected and allowed.
    let error_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.trim_start().to_ascii_lowercase().starts_with("error:"))
        .collect();
    assert!(
        error_lines.is_empty(),
        "reify eval produced Error diagnostics for a program whose curated fillets must all \
         resolve.\nerror lines: {error_lines:#?}\nstderr: {stderr}"
    );

    // The two specific pre-5208 failure signatures must be absent. The second
    // substring is deliberately the stable prefix "curated edge selection" rather
    // than the full legacy sentence ("… is not yet available on the current
    // build …"), so this guard survives the step-14 reword of that diagnostic
    // into an actionable message.
    for signature in [
        "failed to compile geometry operation",
        "curated edge selection",
    ] {
        assert!(
            !stderr.contains(signature),
            "reify eval stderr must not contain {signature:?} — the curated fillets in \
             bottom_deck_selectors.ri must all resolve.\nstderr: {stderr}"
        );
    }

    // Geometry/mass assertions only when the OCCT kernel is compiled in.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mass assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Expect a line like "BottomDeckSelectors.mass = 1.514227852007 kg".
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("BottomDeckSelectors.mass"))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'BottomDeckSelectors.mass' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let mass_rhs = mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("BottomDeckSelectors.mass line has no '=': {mass_line}"));

    assert_ne!(
        mass_rhs, "undef",
        "BottomDeckSelectors.mass must not be undef — the final curated fillet must produce a \
         real solid for volume() to measure.\nline: {mass_line}"
    );

    assert!(
        mass_rhs.contains("kg"),
        "BottomDeckSelectors.mass RHS must contain 'kg' (MASS dimension).\nline: {mass_line}"
    );

    let numeric_token = mass_rhs.split_whitespace().next().unwrap_or("");
    let mass_kg: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse BottomDeckSelectors.mass numeric token as f64: {numeric_token:?}\nline: {mass_line}"
        )
    });

    assert!(
        mass_kg.is_finite() && mass_kg > 0.0,
        "BottomDeckSelectors.mass = {mass_kg} kg must be finite and strictly positive.\nline: {mass_line}"
    );

    // Sanity band. The hollowed shell measures ≈1.207e-3 m³ before corner
    // rounding; × 1270 kg·m⁻³ (PETG) ≈ 1.53 kg, and the seven fillets remove a
    // little more (measured 1.514 kg). The band is wide enough to absorb OCCT
    // version-to-version fillet jitter while still catching a gross regression
    // (e.g. a cut silently dropped, or the whole shell collapsing to the blank
    // 0.48×0.26×0.08 m box at ≈12.7 kg).
    assert!(
        (1.35..=1.65).contains(&mass_kg),
        "BottomDeckSelectors.mass = {mass_kg} kg is outside the expected band [1.35, 1.65] kg \
         for the seven-fillet hollowed deck.\nline: {mass_line}"
    );
}

/// Back-compat guard (CLI level): task 4168 additively wired a LIST form into
/// `arbitrary_pattern` alongside the existing scalar-triple (translation-only)
/// form; this guards that the triple form did not regress into a *crash* under
/// the real kernel-backed `reify eval` path.
///
/// NOTE (esc-4168-114 scope ruling): this test deliberately does NOT assert
/// `reify eval pattern_composition.ri` exits 0. That would be a false premise —
/// pattern_composition.ri uses a `Length` param (`w`) as the *target* of its
/// pattern ops (`arbitrary_pattern(w, …)`, `linear_pattern_2d(w, …)`), which is
/// not a geometry expression. Every SingleTarget geometry op in
/// crates/reify-compiler/src/geometry.rs whose target arg fails to resolve to
/// geometry falls back to `GeomRef::Step(0)`, which is unresolvable in a fresh
/// realization and surfaces as a graceful Error diagnostic (non-zero exit) once
/// executed against a real kernel. This is a PRE-EXISTING, cross-cutting
/// compiler gap (byte-identical compiled output before/after task 4168), tracked
/// separately as a follow-up task; the fix (a compile-time diagnostic, mirroring
/// the task-3395/3418 precedent for Conditional/Match targets) touches ~20 shared
/// call sites and is out of scope for task 4168's additive list-form feature.
///
/// The actual translation-triple back-compat is guarded at the compile level by
/// compile_api_tests.rs::compile_arbitrary_pattern_produces_realization (which
/// asserts the triple form still lowers to a realization). Here we only assert
/// the CLI does not *crash* (no panic / signal kill) on the triple form.
#[test]
fn eval_pattern_composition_triple_form_does_not_crash() {
    let path = common::example_path("pattern_composition.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // The process must exit gracefully — a normal exit code (0 or a diagnostic
    // non-zero), never a signal kill / abort. `status.code()` is `None` only
    // when the child was terminated by a signal (e.g. a Rust panic aborting).
    assert!(
        status.code().is_some(),
        "reify eval pattern_composition.ri was killed by a signal (panic/abort) — \
         the triple form must degrade gracefully, not crash.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Guard against a *new* triple-form regression specifically: task 4168 must
    // not introduce an arbitrary_pattern arg-parsing/decoding panic. A Rust
    // panic in the CLI prints "panicked at" to stderr.
    assert!(
        !stderr.contains("panicked at"),
        "reify eval pattern_composition.ri panicked — the triple form must not \
         panic under the additive list-form wiring.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
