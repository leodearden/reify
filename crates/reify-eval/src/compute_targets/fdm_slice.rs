// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline for `fdm::slice` — the PrusaSlicer-subprocess ComputeNode that
//! turns an FDM body + `FDMProcess` into a structured `Toolpath` value (task η /
//! 3789, slice 2 of `docs/prds/v0_5/fdm-as-printed-fea.md`).
//!
//! Mirrors the task-δ split (`as_printed_material.rs`): the pure subprocess core
//! (discover / compose / run / parse) lives in `reify_fdm::slice`; this module
//! holds the eval-side trampoline, the `Toolpath → Value::StructureInstance`
//! marshalling, and the full-reslice-with-cache warm state.
//!
//! When PrusaSlicer is absent from `$PATH` (the W_FDM_SLICER_UNAVAILABLE case,
//! PRD open Q4) the node degrades honestly: it still emits a (degraded/empty)
//! `Toolpath` value plus a single `Severity::Info` diagnostic carrying
//! `DiagnosticCode::FdmSlicerUnavailable` — never an error.
//
// The trampoline + warm-state cache are built across task η steps 15–18; the
// `Toolpath → Value` marshalling below lands first (steps 13–14).

use std::path::Path;

use reify_core::{Diagnostic, DiagnosticCode};
use reify_fdm::{Bead, BeadRole, Layer, Toolpath};
use reify_ir::{OpaqueState, Value};

use super::as_printed_material::structure;
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Marshal a [`Toolpath`] into a `Value::StructureInstance` named `"Toolpath"`
/// whose `beads` / `layers` Lists hold nested `Bead` / `Layer` structures and
/// whose `in_layer_adjacency` / `inter_layer_adjacency` Lists hold `(lo, hi)`
/// index pairs (each a 2-element `Int` List).
///
/// This is the idiomatic, content-hash-deterministic carrier for a structured
/// Rust result (mirrors `as_printed_material`'s `AnisotropicMaterial`
/// marshalling): a [`Toolpath`] holds only order-stable `Vec`s, so the produced
/// Value is byte-stable run-to-run for a given Toolpath.
///
/// Geometry scalars (`width` / `height` / `layer_z` / `nominal_temp` / `speed`
/// and the centerline coordinates) are emitted as **native-unit** `Value::Real`
/// — raw G-code millimetres / mm·min⁻¹, NOT SI-converted. The mm→SI conversion
/// is the downstream θ `FDMPrint` mapping's concern (PRD / `toolpath.rs` module
/// doc); marshalling preserves the parsed values losslessly.
pub fn toolpath_to_value(tp: &Toolpath) -> Value {
    structure(
        "Toolpath",
        vec![
            ("beads", Value::List(tp.beads.iter().map(bead_to_value).collect())),
            ("layers", Value::List(tp.layers.iter().map(layer_to_value).collect())),
            ("in_layer_adjacency", adjacency_list(&tp.in_layer_adjacency)),
            ("inter_layer_adjacency", adjacency_list(&tp.inter_layer_adjacency)),
        ],
    )
}

/// The honest-degradation Toolpath value for the slicer-absent
/// (W_FDM_SLICER_UNAVAILABLE) path: a well-formed `Toolpath` structure with
/// empty `beads` / `layers` / adjacency Lists. Built via [`toolpath_to_value`]
/// on an empty Toolpath so it is field-shape-identical to a real slice result.
//
// Exercised by the unit test now; the production consumer (the slicer-absent
// trampoline arm) lands in step-16, so suppress the non-test dead-code lint
// until then (same idiom as `toolpath::AdjacencyStats`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn degraded_toolpath_value() -> Value {
    toolpath_to_value(&Toolpath {
        beads: Vec::new(),
        layers: Vec::new(),
        in_layer_adjacency: Vec::new(),
        inter_layer_adjacency: Vec::new(),
    })
}

/// Marshal one [`Bead`] into a `Bead` `StructureInstance`.
fn bead_to_value(b: &Bead) -> Value {
    let centerline = Value::List(b.centerline.iter().map(|p| point_raw(*p)).collect());
    structure(
        "Bead",
        vec![
            ("centerline", centerline),
            ("width", Value::Real(b.width)),
            ("height", Value::Real(b.height)),
            ("role", bead_role_value(b.role)),
            ("layer_index", Value::Int(b.layer_index as i64)),
            ("layer_z", Value::Real(b.layer_z)),
            ("nominal_temp", Value::Real(b.nominal_temp)),
            ("speed", Value::Real(b.speed)),
        ],
    )
}

/// Marshal one [`Layer`] into a `Layer` `StructureInstance`.
fn layer_to_value(l: &Layer) -> Value {
    let bead_indices = Value::List(
        l.bead_indices
            .iter()
            .map(|&i| Value::Int(i as i64))
            .collect(),
    );
    structure(
        "Layer",
        vec![
            ("index", Value::Int(l.index as i64)),
            ("z", Value::Real(l.z)),
            ("bead_indices", bead_indices),
        ],
    )
}

/// Map a [`BeadRole`] to its `BeadRole::<Variant>` enum [`Value`]. The variant
/// names match the stdlib `BeadRole` enum (`fdm_slice.ri`, step-20) and the
/// `reify_fdm::slice::serialize_toolpath_canonical` role spelling.
fn bead_role_value(role: BeadRole) -> Value {
    let variant = match role {
        BeadRole::Perimeter => "Perimeter",
        BeadRole::SolidInfill => "SolidInfill",
        BeadRole::SparseInfill => "SparseInfill",
        BeadRole::Bridge => "Bridge",
        BeadRole::Support => "Support",
    };
    Value::Enum {
        type_name: "BeadRole".to_string(),
        variant: variant.to_string(),
    }
}

/// Marshal a list of `(lo, hi)` bead-index adjacency pairs into a `List` of
/// 2-element `Int` `List`s.
fn adjacency_list(pairs: &[(usize, usize)]) -> Value {
    Value::List(
        pairs
            .iter()
            .map(|&(lo, hi)| Value::List(vec![Value::Int(lo as i64), Value::Int(hi as i64)]))
            .collect(),
    )
}

/// A native-unit 3-D position `Value::Point` of bare `Value::Real` millimetre
/// coordinates (no SI conversion — see [`toolpath_to_value`]).
fn point_raw(p: [f64; 3]) -> Value {
    Value::Point(vec![Value::Real(p[0]), Value::Real(p[1]), Value::Real(p[2])])
}

// ── ComputeNode trampoline ──────────────────────────────────────────────────

/// `@optimized("fdm::slice")` ComputeNode trampoline.
///
/// Discovers a PrusaSlicer binary on `$PATH` (the production discovery step),
/// then delegates to [`fdm_slice_dispatch`] with the resolved binary. Splitting
/// the resolved-binary out as an explicit [`fdm_slice_dispatch`] parameter is the
/// **race-free test seam**: unit tests force the slicer-absent / stub-slicer
/// paths by passing `slicer_bin` directly, never by mutating `$PATH` via
/// `env::set_var` (which the codebase forbids — process-global env writes race
/// across the test harness's threads).
pub fn fdm_slice_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let slicer = reify_fdm::discover_slicer(&path_var, reify_fdm::DEFAULT_SLICER_NAMES);
    fdm_slice_dispatch(
        value_inputs,
        realization_inputs,
        slicer.as_deref(),
        prior_warm_state,
        cancellation,
    )
}

/// The core `fdm::slice` dispatch, parameterised on the **already-resolved**
/// slicer binary (`slicer_bin`) so tests can inject `None` / a stub without
/// touching `$PATH`.
///
/// - `slicer_bin == None` → the W_FDM_SLICER_UNAVAILABLE path (PRD open Q4):
///   degrade honestly to a [`degraded_toolpath_value`] (empty `Toolpath`) plus a
///   single `Severity::Info` [`Diagnostic`] coded
///   [`DiagnosticCode::FdmSlicerUnavailable`] — never an error, so the graph
///   stays live and the "FDMSlice on a body emits a Toolpath" signal holds.
/// - `slicer_bin == Some(_)` → the present-slicer path (subprocess run with
///   cooperative cancellation + reslice-with-cache warm state) lands in step-18.
pub(crate) fn fdm_slice_dispatch(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    slicer_bin: Option<&Path>,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let Some(_bin) = slicer_bin else {
        return ComputeOutcome::Completed {
            result: degraded_toolpath_value(),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: vec![
                Diagnostic::info(
                    "fdm_slice: no PrusaSlicer binary found on $PATH; emitting an empty \
                     Toolpath. Install PrusaSlicer (or put it on $PATH) to produce a real \
                     toolpath.",
                )
                .with_code(DiagnosticCode::FdmSlicerUnavailable),
            ],
        };
    };

    // Present-slicer path: spawn the subprocess with cooperative cancellation and
    // a reslice-with-cache warm state. Completed in step-18, which consumes the
    // settings from `value_inputs`, the body realization from
    // `realization_inputs`, the cache key vs `prior_warm_state`, and the
    // cancel-poll from `cancellation`.
    let _ = (value_inputs, realization_inputs, prior_warm_state, cancellation);
    todo!("fdm::slice present-slicer path: run_slicer + FdmSliceCacheKey warm state, step-18 #3789")
}

#[cfg(test)]
mod tests {
    // `super::*` re-exports the module's `reify_fdm::{Bead, BeadRole, Layer,
    // Toolpath}` + `reify_ir::Value` imports alongside `toolpath_to_value`.
    use super::*;

    /// A hand-built 2-bead / 2-layer Toolpath with one in-layer and one
    /// inter-layer adjacency pair — the marshalling fixture for
    /// [`toolpath_to_value`]. `toolpath_to_value` is a pure projection of the
    /// struct, so the (otherwise odd) shared `(0, 1)` pair on both adjacency
    /// lists is fine: the test distinguishes the two lists by field name.
    fn sample_toolpath() -> Toolpath {
        let bead0 = Bead {
            centerline: vec![[0.0, 0.0, 0.2], [10.0, 0.0, 0.2]],
            width: 0.45,
            height: 0.2,
            role: BeadRole::Perimeter,
            layer_index: 0,
            layer_z: 0.2,
            nominal_temp: 210.0,
            speed: 1800.0,
        };
        let bead1 = Bead {
            centerline: vec![[0.0, 0.0, 0.4], [10.0, 0.0, 0.4], [10.0, 5.0, 0.4]],
            width: 0.50,
            height: 0.2,
            role: BeadRole::SolidInfill,
            layer_index: 1,
            layer_z: 0.4,
            nominal_temp: 215.0,
            speed: 2400.0,
        };
        Toolpath {
            beads: vec![bead0, bead1],
            layers: vec![
                Layer {
                    index: 0,
                    z: 0.2,
                    bead_indices: vec![0],
                },
                Layer {
                    index: 1,
                    z: 0.4,
                    bead_indices: vec![1],
                },
            ],
            in_layer_adjacency: vec![(0, 1)],
            inter_layer_adjacency: vec![(0, 1)],
        }
    }

    /// Read a named field of a [`Value::StructureInstance`], panicking with a
    /// helpful message if `v` is not a structure or the field is absent-shaped.
    fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
        match v {
            Value::StructureInstance(d) => d.fields.get(key),
            other => panic!("expected a StructureInstance, got {other:?}"),
        }
    }

    /// Unwrap a [`Value::List`] to its element slice.
    fn as_list(v: &Value) -> &[Value] {
        match v {
            Value::List(items) => items,
            other => panic!("expected a List, got {other:?}"),
        }
    }

    /// The top-level value is a `StructureInstance` named `Toolpath` carrying a
    /// `beads` List of 2 and a `layers` List of 2 `Layer` structures.
    #[test]
    fn toolpath_to_value_yields_named_toolpath_structure() {
        let v = toolpath_to_value(&sample_toolpath());

        match &v {
            Value::StructureInstance(d) => assert_eq!(d.type_name, "Toolpath"),
            other => panic!("expected a Toolpath StructureInstance, got {other:?}"),
        }

        let beads = as_list(field(&v, "beads").expect("beads field present"));
        assert_eq!(beads.len(), 2, "two beads");
        for b in beads {
            match b {
                Value::StructureInstance(d) => assert_eq!(d.type_name, "Bead"),
                other => panic!("expected a Bead StructureInstance, got {other:?}"),
            }
        }

        let layers = as_list(field(&v, "layers").expect("layers field present"));
        assert_eq!(layers.len(), 2, "two layers");
        match &layers[0] {
            Value::StructureInstance(d) => assert_eq!(d.type_name, "Layer"),
            other => panic!("expected a Layer StructureInstance, got {other:?}"),
        }
        assert_eq!(field(&layers[0], "index"), Some(&Value::Int(0)));
        assert_eq!(field(&layers[0], "z"), Some(&Value::Real(0.2)));
        let bead_indices = as_list(field(&layers[1], "bead_indices").expect("bead_indices"));
        assert_eq!(bead_indices.len(), 1);
        assert_eq!(bead_indices[0], Value::Int(1), "layer 1 owns bead 1");
    }

    /// Each marshalled bead carries its role (as a `BeadRole` enum value), its
    /// geometry scalars (native mm / mm·min⁻¹, NOT SI-converted — θ owns that),
    /// its integer layer index, and its centerline polyline as a List.
    #[test]
    fn bead_fields_carry_role_geometry_and_centerline() {
        let v = toolpath_to_value(&sample_toolpath());
        let beads = as_list(field(&v, "beads").unwrap());

        // role enum mapping: BeadRole::Perimeter -> BeadRole::Perimeter.
        assert_eq!(
            field(&beads[0], "role"),
            Some(&Value::Enum {
                type_name: "BeadRole".to_string(),
                variant: "Perimeter".to_string(),
            }),
            "Perimeter maps to the BeadRole::Perimeter enum value"
        );
        assert_eq!(field(&beads[0], "width"), Some(&Value::Real(0.45)));
        assert_eq!(field(&beads[0], "height"), Some(&Value::Real(0.2)));
        assert_eq!(field(&beads[0], "layer_index"), Some(&Value::Int(0)));
        assert_eq!(field(&beads[0], "layer_z"), Some(&Value::Real(0.2)));
        assert_eq!(field(&beads[0], "nominal_temp"), Some(&Value::Real(210.0)));
        assert_eq!(field(&beads[0], "speed"), Some(&Value::Real(1800.0)));

        let cl0 = as_list(field(&beads[0], "centerline").expect("centerline field"));
        assert_eq!(cl0.len(), 2, "bead 0 has two centerline points");

        // The second bead's distinct role maps through too.
        assert_eq!(
            field(&beads[1], "role"),
            Some(&Value::Enum {
                type_name: "BeadRole".to_string(),
                variant: "SolidInfill".to_string(),
            }),
            "SolidInfill maps to the BeadRole::SolidInfill enum value"
        );
        let cl1 = as_list(field(&beads[1], "centerline").unwrap());
        assert_eq!(cl1.len(), 3, "bead 1 has three centerline points");
    }

    /// The two adjacency lists are marshalled into distinctly-named fields, each
    /// holding `(lo, hi)` index pairs as 2-element Int lists.
    #[test]
    fn adjacency_pairs_marshalled_into_named_lists() {
        let v = toolpath_to_value(&sample_toolpath());

        let in_layer = as_list(field(&v, "in_layer_adjacency").expect("in_layer_adjacency"));
        assert_eq!(in_layer.len(), 1, "one in-layer pair");
        let p = as_list(&in_layer[0]);
        assert_eq!(p.len(), 2, "a pair is a 2-element list");
        assert_eq!(p[0], Value::Int(0));
        assert_eq!(p[1], Value::Int(1));

        let inter_layer =
            as_list(field(&v, "inter_layer_adjacency").expect("inter_layer_adjacency"));
        assert_eq!(inter_layer.len(), 1, "one inter-layer pair");
        let q = as_list(&inter_layer[0]);
        assert_eq!(q[0], Value::Int(0));
        assert_eq!(q[1], Value::Int(1));
    }

    /// Marshalling the same Toolpath twice yields structurally-equal Values —
    /// the Value-level determinism the content-hash cache key relies on.
    #[test]
    fn marshalling_is_deterministic() {
        let tp = sample_toolpath();
        assert_eq!(
            toolpath_to_value(&tp),
            toolpath_to_value(&tp),
            "two marshallings of the same Toolpath are structurally equal"
        );
    }

    /// The slicer-absent degraded value is a well-formed `Toolpath` structure
    /// (same field shape as a real slice) with empty bead / layer / adjacency
    /// Lists — the honest-degradation payload for W_FDM_SLICER_UNAVAILABLE.
    #[test]
    fn degraded_toolpath_value_is_empty_but_well_formed() {
        let v = degraded_toolpath_value();
        match &v {
            Value::StructureInstance(d) => assert_eq!(d.type_name, "Toolpath"),
            other => panic!("expected a Toolpath StructureInstance, got {other:?}"),
        }
        for key in [
            "beads",
            "layers",
            "in_layer_adjacency",
            "inter_layer_adjacency",
        ] {
            assert_eq!(
                as_list(field(&v, key).unwrap_or_else(|| panic!("{key} field present"))).len(),
                0,
                "{key} is empty in the degraded value"
            );
        }
    }

    /// step-15 RED: the slicer-absent trampoline path. With the slicer forced
    /// absent (`slicer_bin = None` — the race-free function-parameter seam, since
    /// the codebase forbids `env::set_var` test seams), `fdm_slice_dispatch`
    /// returns `Completed` with a degraded (empty) Toolpath value and *exactly
    /// one* `Severity::Info` diagnostic coded `FdmSlicerUnavailable` — never an
    /// error (PRD open Q4). Fails to compile until step-16 adds the dispatch
    /// seam + the `FdmSlicerUnavailable` DiagnosticCode.
    #[test]
    fn slicer_absent_dispatch_degrades_with_info_diagnostic() {
        use crate::{CancellationHandle, ComputeOutcome};
        use reify_core::{DiagnosticCode, Severity};

        // value_inputs/realization_inputs are unused on the absent path (the
        // dispatch short-circuits on `slicer_bin == None`); pass placeholders
        // shaped like the real [body, FDMProcess, FDMSliceOptions] arity.
        let value_inputs = [Value::Undef, Value::Undef, Value::Undef];
        let outcome =
            fdm_slice_dispatch(&value_inputs, &[], None, None, &CancellationHandle::new());

        match outcome {
            ComputeOutcome::Completed {
                result,
                new_warm_state,
                cost_per_byte,
                diagnostics,
            } => {
                assert_eq!(
                    result,
                    degraded_toolpath_value(),
                    "the absent path emits the degraded Toolpath value"
                );
                assert!(new_warm_state.is_none(), "no warm state on the absent path");
                assert!(cost_per_byte.is_none(), "no cost on the absent path");
                assert_eq!(diagnostics.len(), 1, "exactly one diagnostic");
                assert_eq!(
                    diagnostics[0].severity,
                    Severity::Info,
                    "W_FDM_SLICER_UNAVAILABLE is informational, never an error"
                );
                assert_eq!(
                    diagnostics[0].code,
                    Some(DiagnosticCode::FdmSlicerUnavailable),
                    "carries the FdmSlicerUnavailable code"
                );
            }
            other => panic!("expected Completed (degraded), got {other:?}"),
        }
    }

    // ── step-17: present-slicer path — cancellation + warm-state cache ───────────
    //
    // Injected stub "slicers" (CI-portable, no live PrusaSlicer): a `#!/bin/sh`
    // script stands in for the binary, passed straight to the race-free
    // `fdm_slice_dispatch` seam. The stub ignores the body STL the trampoline
    // exports and drives only the outcome a test needs — a long sleeper (the
    // cancellation poll) or a fixture-emitting `cp` (the warm-state cache).

    /// Absolute path to the committed ζ PrusaSlicer-vocabulary fixture in the
    /// sibling `reify-fdm` crate (the same fixture `reify-fdm`'s own slice tests
    /// drive their stub slicer with). Canonicalized so the `..` is resolved before
    /// it is baked into the `#!/bin/sh` stub.
    #[cfg(unix)]
    fn fixture_gcode_path() -> std::path::PathBuf {
        let rel = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../reify-fdm/tests/fixtures/prusaslicer_bracket.gcode");
        std::fs::canonicalize(&rel)
            .unwrap_or_else(|e| panic!("canonicalize fixture {}: {e}", rel.display()))
    }

    /// Write a `#!/bin/sh` stub "slicer" with `body`, mark it +x, return its path.
    #[cfg(unix)]
    fn write_stub_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write stub");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod +x stub");
        path
    }

    /// A stub body: append one byte to `counter` (the run-count seam), then copy
    /// the committed fixture to whatever `-o <path>` the composed args carry, and
    /// exit 0 — a successful slice that records that it ran.
    #[cfg(unix)]
    fn emit_fixture_counting_body(fixture: &Path, counter: &Path) -> String {
        format!(
            "echo x >> '{c}'\nout=\"\"\nprev=\"\"\nfor a in \"$@\"; do\n  \
             if [ \"$prev\" = \"-o\" ]; then out=\"$a\"; fi\n  prev=\"$a\"\ndone\ncp '{f}' \"$out\"\n",
            c = counter.display(),
            f = fixture.display(),
        )
    }

    /// The `[body, FDMProcess, FDMSliceOptions]` value-input placeholder triple.
    /// The stub slicer ignores the composed settings and `read_slice_settings`
    /// falls back to defaults for `Undef`, so bare `Undef`s exercise the dispatch
    /// path without a full stdlib FDMProcess (settings determinism is what the
    /// cache key needs, and identical inputs → identical settings → identical key).
    #[cfg(unix)]
    fn undef_inputs() -> [Value; 3] {
        [Value::Undef, Value::Undef, Value::Undef]
    }

    /// One realization handle carrying a fixed content hash (the body-hash half of
    /// the `FdmSliceCacheKey`) and no mesh content (the trampoline exports an empty
    /// STL, which the stub ignores).
    #[cfg(unix)]
    fn body_handle(hash: u128) -> RealizationReadHandle {
        use reify_core::{ContentHash, RealizationNodeId};
        RealizationReadHandle::new(RealizationNodeId::new("body", 0), ContentHash(hash), None)
    }

    /// step-17(a) RED: a pre-cancelled dispatch against a long-sleeper stub slicer
    /// returns `ComputeOutcome::Cancelled` promptly — the `|| is_cancelled()` poll
    /// reaches `run_slicer`, which SIGTERM→reaps the child (no orphan). Fails until
    /// step-18 wires the present-slicer path (today it hits `todo!`).
    #[cfg(unix)]
    #[test]
    fn present_slicer_precancelled_returns_cancelled() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub_script(dir.path(), "sleeper.sh", "exec sleep 30");

        let cancel = CancellationHandle::new();
        cancel.cancel(); // pre-cancelled: the poll fires on the first run_slicer tick.

        let inputs = undef_inputs();
        let realizations = [body_handle(0x1111)];
        let start = Instant::now();
        let outcome = fdm_slice_dispatch(&inputs, &realizations, Some(&stub), None, &cancel);
        let elapsed = start.elapsed();

        assert!(
            matches!(outcome, ComputeOutcome::Cancelled),
            "a pre-cancelled dispatch must return Cancelled, got {outcome:?}"
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "cancellation must be prompt (≪ the 30s sleeper), took {elapsed:?}"
        );
    }

    /// step-17(b) RED: a fresh dispatch (no prior warm state) runs the stub slicer
    /// once and returns `Completed` with a donated warm state + positive
    /// `cost_per_byte`; a second dispatch with that warm state + identical inputs
    /// HITs the cache, reuses the Toolpath value, and does NOT re-run the slicer
    /// (the run-count seam stays at 1). Fails until step-18.
    #[cfg(unix)]
    #[test]
    fn present_slicer_warm_state_cache_reuses_toolpath() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counter = dir.path().join("run-count");
        let stub = write_stub_script(
            dir.path(),
            "ok-slicer.sh",
            &emit_fixture_counting_body(&fixture_gcode_path(), &counter),
        );

        let inputs = undef_inputs();
        let realizations = [body_handle(0x2222)];
        let never = CancellationHandle::new();

        // ── dispatch 1: cache MISS — runs the slicer, donates warm state ─────────
        let (result1, warm) = match fdm_slice_dispatch(
            &inputs,
            &realizations,
            Some(&stub),
            None,
            &never,
        ) {
            ComputeOutcome::Completed {
                result,
                new_warm_state,
                cost_per_byte,
                ..
            } => {
                assert!(
                    cost_per_byte.is_some_and(|c| c > 0.0),
                    "a fresh slice reports a positive cost_per_byte, got {cost_per_byte:?}"
                );
                let beads = as_list(field(&result, "beads").expect("beads field"));
                assert!(!beads.is_empty(), "the fixture slice has beads");
                (
                    result,
                    new_warm_state.expect("a fresh slice donates warm state"),
                )
            }
            other => panic!("dispatch 1 expected Completed, got {other:?}"),
        };
        let runs_after_first = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        assert_eq!(runs_after_first, 1, "the slicer ran exactly once on the MISS");

        // ── dispatch 2: cache HIT — prior warm state + identical inputs ──────────
        let result2 = match fdm_slice_dispatch(
            &inputs,
            &realizations,
            Some(&stub),
            Some(&warm),
            &never,
        ) {
            ComputeOutcome::Completed { result, .. } => result,
            other => panic!("dispatch 2 expected Completed, got {other:?}"),
        };
        let runs_after_second = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        assert_eq!(runs_after_second, 1, "the cache HIT must NOT re-run the slicer");
        assert_eq!(result1, result2, "the HIT returns the cached Toolpath value");
    }
}
