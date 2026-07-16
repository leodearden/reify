//! End-to-end options-threading proof for `isosurface(..., iso: X)` (task ε,
//! 5003).
//!
//! PRD `docs/prds/v0_3/voxel-to-mesh-surfacing.md` task ε (Phase-3
//! hardening): proves a NON-DEFAULT `iso:` on the `isosurface(...)` builtin
//! measurably changes the surfaced mesh END-TO-END, guarding PRD D4's
//! options-threading path against the C-10 "declared-but-unexercised" shape.
//!
//! Three tests build a narrow-band 20mm-box fixture (via the shared
//! `iso_source` template, so the fixture shape is single-sourced) with
//! distinct `iso:` values:
//!
//! - `iso_option_changes_surfaced_mesh`: the options-threading proof
//!   itself. Two IN-BAND, non-empty builds (`iso: 0mm` vs `iso: 3mm`) must
//!   differ in triangle count — agreement would mean `iso:` never reached
//!   marching cubes. A third leg (`iso:` omitted entirely) must match the
//!   explicit `iso: 0mm` build's triangle count EXACTLY, pinning the
//!   "omitted == `iso: 0mm`" equivalence that no other test exercises
//!   (`voxel_to_mesh_e2e.rs` only ever builds the no-argument form).
//!   Triangle count is the only comparison signal in this file — see that
//!   test's doc comment for why bounding box isn't used.
//! - `iso_option_out_of_band_surfaces_empty_mesh`: a SEPARATE regression
//!   guard for `realize_mesh_from_voxel_with_options`'s documented
//!   `Ok(empty)` no-crossing contract, kept apart so a future legitimate
//!   narrow-band change fails only this guard, not the options-threading
//!   proof above.
//! - `iso_example_fixture_surfaces_nonempty`: validates that the committed
//!   example fixture (not just the inline sources above) builds and
//!   surfaces a non-empty terminal mesh.
//!
//! ## Coverage scope
//!
//! This entire file is `#[cfg(has_openvdb)]`-gated, and every `#[test]`
//! additionally skips at runtime via `occt_available_or_skip` when OCCT is
//! unavailable — so in a non-OpenVDB build, or an OpenVDB build running
//! without OCCT, this binary contributes ZERO test signal. The
//! options-threading proof this file exists to provide is only real in a
//! lane where both kernels are present. This mirrors
//! `voxel_to_mesh_e2e.rs`'s existing skip convention (not a new gap
//! introduced here); it's called out explicitly so a merge-gating lane
//! change doesn't silently drop this guard.
//!
//! ## Reuse
//!
//! - Linker anchors, `OCCT_AVAILABLE` runtime gate,
//!   `Engine::with_registered_kernel` + `ensure_openvdb_kernel()` pairing,
//!   `snapshot()` terminal-by-index + `tessellate_realizations()`
//!   terminal-mesh extraction: `crates/reify-eval/tests/voxel_to_mesh_e2e.rs`.
//! - Runtime-read-fixture RED mechanism (`std::fs::read_to_string` of a
//!   `CARGO_MANIFEST_DIR`-relative example path via `.expect(...)`, NOT
//!   `include_str!`, so a missing fixture is a clean test panic rather than
//!   a compile error): `voxel_to_mesh_e2e.rs`'s
//!   `voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal`.

// Anchor: force the linker to include the reify_kernel_occt rlib
// unconditionally so its `inventory::submit!` registration fires at binary
// startup, regardless of cfg(has_occt). Mirrors the anchor in
// crates/reify-eval/tests/voxel_to_mesh_e2e.rs.
extern crate reify_kernel_occt as _;

// Anchor: same rationale, for reify_kernel_openvdb. Gated on `has_openvdb`
// because the whole test below only makes sense (and only compiles its
// OpenVDB-touching calls) under that cfg. Mirrors the anchor in
// crates/reify-eval/tests/voxel_to_mesh_e2e.rs.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

/// Terminal-mesh aggregate signal returned by [`surface_shell_stats`]: a
/// triangle count. Triangle count is a PROXY for "the surfaced mesh
/// changed" — two genuinely distinct isocontours could in principle
/// marching-cubes to the same triangle count while differing in vertex
/// positions. A bounding-box fallback was considered to close that gap but
/// deliberately dropped: comparing vertex positions across two
/// INDEPENDENTLY-CONSTRUCTED engines (as every test in this file does)
/// would trade that gap for a cross-build floating-point determinism
/// assumption this file declines to make (see
/// `iso_option_changes_surfaced_mesh`'s doc comment). Triangle count is
/// therefore the sole comparison signal throughout this file.
#[cfg(has_openvdb)]
#[derive(Debug, Clone)]
struct ShellStats {
    triangle_count: usize,
    /// `message` of each `Severity::Error` diagnostic from the build, if
    /// any. Populated (not asserted on) by `surface_shell_stats` so each
    /// call site can assert with a message tied to its own context — see
    /// `assert_no_build_errors` and `iso_option_out_of_band_surfaces_empty_mesh`.
    error_diagnostics: Vec<String>,
}

/// Returns `true` (and the caller should proceed) if OCCT is available;
/// otherwise emits a skip message naming `test_name` and returns `false` (the
/// caller should `return` immediately). Factors out the OCCT-availability
/// skip preamble that was previously copy-pasted verbatim into all three
/// `#[test]` functions below.
#[cfg(has_openvdb)]
fn occt_available_or_skip(test_name: &str) -> bool {
    if reify_kernel_occt::OCCT_AVAILABLE {
        true
    } else {
        eprintln!(
            "skipping {test_name}: OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        false
    }
}

/// Builds `source` with a real OCCT + OpenVDB engine and returns the
/// terminal realization's surfaced-mesh [`ShellStats`] for `entity`. A
/// fresh `Engine` per call avoids cross-build cache state, so counts
/// compared across calls are each independently honest. Mirrors the
/// engine-construction and terminal-extraction sequence in
/// `voxel_to_mesh_e2e.rs::voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal`.
///
/// `Severity::Error` diagnostics are collected into
/// [`ShellStats::error_diagnostics`] rather than asserted on here, so each
/// call site can fail with a message tied to its own context (see
/// `assert_no_build_errors`) — this also catches a silently degraded build
/// masquerading as a 0-triangle `Ok(empty)` result. A terminal `MeshSurface`
/// entry is asserted to EXIST regardless of emptiness, distinguishing
/// "present but empty" (the documented `Ok(empty)` contract) from "absent"
/// (a real defect): `surface_subtree`'s `Ok(mesh)` handling
/// (`crates/reify-eval/src/geometry_ops.rs`) pushes a `MeshSurface`
/// unconditionally on a successful `kernel.tessellate`, never conditioned
/// on non-emptiness.
#[cfg(has_openvdb)]
fn surface_shell_stats(source: &str, entity: &str) -> ShellStats {
    use reify_core::Severity;
    use reify_ir::ExportFormat;
    use reify_test_support::parse_and_compile_with_stdlib;

    // parse_and_compile_with_stdlib (reify-test-support/src/helpers.rs)
    // already panics with a parse- or compile-specific message ("parse
    // errors: ..." / "compile errors: ...") on any error-severity
    // diagnostic — it never returns a module carrying one. So a malformed
    // inline source below fails loudly right here, never silently as the
    // generic "expected at least 1 realization node" panic further down.
    let compiled = parse_and_compile_with_stdlib(source);

    // Same pairing the fixed `cmd_build` uses: single-pick OCCT default +
    // lazily-acquired OpenVDB (adds ONLY the openvdb adapter, leaves
    // default_kernel_name == "occt").
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let acquired = engine.ensure_openvdb_kernel();
    assert!(
        acquired,
        "ensure_openvdb_kernel() must return true under cfg(has_openvdb) \
         (the OpenVDB adapter must be present in the kernel registry)"
    );

    let _eval = engine.eval(&compiled);
    let build = engine.build(&compiled, ExportFormat::Stl);

    let error_diagnostics: Vec<String> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.message.clone())
        .collect();

    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == entity)
        .collect();
    assert!(
        !nodes.is_empty(),
        "expected at least 1 realization node for entity {entity}; got none"
    );
    nodes.sort_by_key(|(id, _)| id.index);
    let (terminal_id, _terminal_node) = *nodes.last().expect("nodes is non-empty (asserted above)");
    let terminal_path = terminal_id.to_string();

    let tess = engine.tessellate_realizations(&compiled);
    let terminal_mesh = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a MeshSurface entry at \
                 entity_path {terminal_path:?} for entity {entity} (a \
                 no-error build's terminal realization always pushes one via \
                 surface_subtree's Ok(mesh) handling, even for an Ok(empty) \
                 mesh); got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });

    ShellStats {
        triangle_count: terminal_mesh.mesh.indices.len() / 3,
        error_diagnostics,
    }
}

/// Asserts `stats` (from [`surface_shell_stats`]) carries no
/// `Severity::Error` build diagnostics — the common "must not be silently
/// degraded" case. `iso_option_out_of_band_surfaces_empty_mesh` doesn't use
/// this: it asserts on `error_diagnostics` directly so a future contract
/// shift there names itself instead of reporting this generic message.
#[cfg(has_openvdb)]
fn assert_no_build_errors(stats: &ShellStats, entity: &str) {
    assert!(
        stats.error_diagnostics.is_empty(),
        "build of entity {entity} must have no error-severity diagnostics \
         (an OpenVDB-registration or dispatch failure would silently \
         degrade the isosurface build and masquerade as an Ok(empty) \
         0-triangle result); got: {:?}",
        stats.error_diagnostics
    );
}

/// Builds the shared `IsoKnob` fixture source (`param size: Length = 20mm`,
/// a `box(size, size, size)` solid, `isosurface(solid<arg>)`), varying only
/// the `iso:` argument text — e.g. `", iso: 3mm"`, `", iso: 0mm"`, or `""`
/// to omit `iso:` entirely. Single-sources the fixture shape so a future
/// edit (e.g. the box size) can't drift out of sync across call sites.
#[cfg(has_openvdb)]
fn iso_source(arg: &str) -> String {
    format!(
        "structure IsoKnob {{ param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid{arg}) }}"
    )
}

/// Two IN-BAND, non-empty builds (`iso: 0mm` vs `iso: 3mm`, generated from
/// the shared [`iso_source`] template so they're identical apart from the
/// `iso:` argument) must differ in triangle count. Comparing two non-empty
/// results — rather than non-empty vs empty — is the stronger proof: since
/// neither can collapse to empty, a difference can only mean `iso:`
/// genuinely moved the isocontour, not that a build merely fell outside the
/// narrow band. Triangle count is the only signal compared here: vertex
/// positions are `f32`s produced by two INDEPENDENTLY-CONSTRUCTED engines,
/// so comparing them (e.g. a bounding box) would encode a cross-build
/// floating-point determinism assumption this proof doesn't need to make
/// (see [`ShellStats`]'s doc comment).
///
/// A third source OMITS `iso:` entirely and must match the explicit
/// `iso: 0mm` build's triangle count EXACTLY — pinning the "omitted ==
/// `iso: 0mm`" equivalence, which no other test exercises
/// (`voxel_to_mesh_e2e.rs` only ever builds the no-argument form). The
/// identical-`GeometryOp` argument (see the assertion site below) already
/// guarantees both builds are the same computation, so the triangle-count
/// exact-match alone pins the equivalence.
///
/// See `iso_option_out_of_band_surfaces_empty_mesh` for the separate
/// `Ok(empty)` contract guard.
#[cfg(has_openvdb)]
#[test]
fn iso_option_changes_surfaced_mesh() {
    if !occt_available_or_skip("iso_option_changes_surfaced_mesh") {
        return;
    }

    let source_zero = iso_source(", iso: 0mm");
    let source_inband = iso_source(", iso: 3mm");
    let source_omitted = iso_source("");

    let zero = surface_shell_stats(&source_zero, "IsoKnob");
    let inband = surface_shell_stats(&source_inband, "IsoKnob");
    let omitted = surface_shell_stats(&source_omitted, "IsoKnob");
    assert_no_build_errors(&zero, "IsoKnob");
    assert_no_build_errors(&inband, "IsoKnob");
    assert_no_build_errors(&omitted, "IsoKnob");

    assert!(
        zero.triangle_count > 0,
        "iso: 0mm must surface a non-empty mesh (a closed 20mm box's \
         narrow-band SDF crosses iso=0 at its boundary, per δ/5002); got \
         {} triangles",
        zero.triangle_count
    );
    assert!(
        inband.triangle_count > 0,
        "iso: 3mm must also surface a non-empty mesh (in-band for a 20mm \
         box's ~0.01m narrow-band half-width, mirroring the committed \
         example fixture examples/multi_kernel/voxel_to_mesh_iso.ri); got \
         {} triangles",
        inband.triangle_count
    );
    // eval::geometry_ops defaults a missing `iso`/`adaptive` to the same
    // values the explicit `iso: 0mm` (no `adaptive:`) form resolves to, so
    // both compute the IDENTICAL GeometryOp::Surface — an exact match is
    // expected, not merely a close one.
    assert_eq!(
        omitted.triangle_count, zero.triangle_count,
        "omitting `iso:` entirely (`isosurface(solid)`) must surface the \
         same triangle count as the explicit default `iso: 0mm` — the two \
         are documented to be equivalent (see this test's doc comment); \
         got {} (no `iso:` argument) vs {} (`iso: 0mm`)",
        omitted.triangle_count, zero.triangle_count
    );
    assert_ne!(
        zero.triangle_count, inband.triangle_count,
        "iso: 0mm and iso: 3mm — both IN-BAND and both non-empty — must \
         surface a DIFFERENT triangle count. This IS the options-threading \
         proof, driven by a genuine isocontour shift rather than by either \
         build collapsing to empty, and it holds for ANY live `iso:` \
         regardless of the exact narrow-band width; equal counts would mean \
         the `iso:` option never reached marching cubes (D4 \
         options-threading collapsed to default)"
    );
}

/// Separate regression guard for `realize_mesh_from_voxel_with_options`'s
/// documented `Ok(empty)` no-crossing contract (`kernel_real.rs`), kept
/// apart from the options-threading proof above so a future legitimate
/// narrow-band change fails only this guard. `iso: 10m` is >=500x the 20mm
/// box and well past `MeshToVoxelOptions::honest_floor`'s narrow-band LOWER
/// BOUND (`narrow_band * h >= longest_extent/2`), so it is expected to
/// surface an EMPTY mesh; the fallback below (fewer triangles than an
/// in-band build) keeps that expectation from being a hard requirement, so
/// a legitimate band widening doesn't force an edit here — only a genuine
/// options-threading regression (`iso:` silently ignored) would surface the
/// SAME count as in-band.
///
/// Asserts on `ShellStats::error_diagnostics` directly (rather than going
/// through `assert_no_build_errors`) so a future contract shift to an error
/// diagnostic (instead of `Ok(empty)`) names itself, rather than reporting
/// the generic "silently degraded build" wording shared with other tests.
#[cfg(has_openvdb)]
#[test]
fn iso_option_out_of_band_surfaces_empty_mesh() {
    if !occt_available_or_skip("iso_option_out_of_band_surfaces_empty_mesh") {
        return;
    }

    let source_outband = iso_source(", iso: 10m");
    let outband = surface_shell_stats(&source_outband, "IsoKnob");

    // Inspected directly here (not via assert_no_build_errors) so that if a
    // future change makes an out-of-band iso: emit an error diagnostic
    // instead of the documented Ok(empty) result, the failure names that
    // contract shift specifically rather than reporting a generic
    // "silently degraded build".
    assert!(
        outband.error_diagnostics.is_empty(),
        "iso: 10m produced error-severity diagnostics {:?} instead of the \
         documented Ok(empty) no-crossing result \
         (realize_mesh_from_voxel_with_options, kernel_real.rs) — the \
         no-crossing contract itself appears to have changed from \
         returning an empty mesh to emitting an error, which is the \
         regression this test guards against",
        outband.error_diagnostics
    );
    // Primary check: the documented Ok(empty) contract. Fallback (only
    // built if the primary check would otherwise fail): a legitimate
    // narrow-band widening should still surface far fewer triangles than an
    // in-band build, decoupling this guard from the band's exact width
    // while still catching a genuine options-threading regression — see the
    // doc comment above.
    if outband.triangle_count != 0 {
        let source_inband = iso_source(", iso: 3mm");
        let inband = surface_shell_stats(&source_inband, "IsoKnob");
        assert!(
            outband.triangle_count < inband.triangle_count,
            "iso: 10m must surface either an EMPTY mesh (the documented \
             Ok(empty) no-crossing contract, kernel_real.rs) or, failing \
             that, far fewer triangles than an in-band iso: 3mm build — got \
             {} triangles (out-of-band, iso: 10m) vs {} (in-band, iso: 3mm), \
             which does not decrease and so looks like iso: was not \
             threaded through at all",
            outband.triangle_count, inband.triangle_count
        );
    }
}

/// The committed example fixture `examples/multi_kernel/voxel_to_mesh_iso.ri`
/// (`iso: 3mm`, in-band for a 20mm box) must build and surface a non-empty
/// terminal mesh.
///
/// Reads the fixture at runtime via `std::fs::read_to_string`, NOT
/// `include_str!`, so a missing/unreadable fixture fails the test cleanly
/// instead of breaking the whole test binary's compile.
///
/// Overlaps in literal with `iso_option_changes_surfaced_mesh`'s `inband`
/// leg deliberately: that test proves the VALUE is live, this one validates
/// the COMMITTED FILE itself (e.g. a typo introduced editing the checked-in
/// `.ri`), so a fixture-specific regression fails with a message pointing
/// straight at `voxel_to_mesh_iso.ri`.
#[cfg(has_openvdb)]
#[test]
fn iso_example_fixture_surfaces_nonempty() {
    if !occt_available_or_skip("iso_example_fixture_surfaces_nonempty") {
        return;
    }

    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/multi_kernel/voxel_to_mesh_iso.ri"
    ))
    .expect(
        "examples/multi_kernel/voxel_to_mesh_iso.ri must exist \
         (task 5003 step-2 creates this fixture)",
    );

    let stats = surface_shell_stats(&source, "VoxelToMeshIso");
    assert_no_build_errors(&stats, "VoxelToMeshIso");
    assert!(
        stats.triangle_count > 0,
        "examples/multi_kernel/voxel_to_mesh_iso.ri (iso: 3mm, in-band for \
         a 20mm box) must surface a non-empty terminal mesh; got {} triangles",
        stats.triangle_count
    );
}
