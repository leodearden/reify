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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reify_core::{Diagnostic, DiagnosticCode};
use reify_fdm::{
    Bead, BeadRole, InfillPattern, Layer, SliceError, SliceSettings, Toolpath, infill_pattern_arg,
    serialize_toolpath_canonical, slice_body,
};
use reify_ir::{OpaqueState, Value};

use super::as_printed_material::{field_int, field_real, field_scalar, struct_data, structure};
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
///
/// The production consumer is `fdm_slice_dispatch`'s slicer-absent (`slicer_bin
/// == None`) arm, so this is live in non-test builds.
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
    let Some(bin) = slicer_bin else {
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

    // ── present-slicer path: compose settings, key the reslice cache, run ───────
    let settings = read_slice_settings(value_inputs);
    // A body realization handle is REQUIRED to key the reslice cache: the content
    // hash is the only distinguishing input between two bodies under identical
    // settings. Collapsing to a 0 sentinel when absent would alias distinct
    // realization-less bodies and confuse a genuine content_hash == 0 realization.
    // A realization-less dispatch is therefore NON-CACHEABLE: no HIT lookup and
    // no warm-state donation. The slicer still runs and cost_per_byte is reported.
    let cache_key: Option<FdmSliceCacheKey> =
        realization_inputs.first().map(|h| FdmSliceCacheKey {
            body_hash: h.content_hash.0,
            settings_hash: settings_hash(&settings),
        });

    // Cache HIT: a prior warm state keyed identically → reuse the cached Toolpath
    // value and skip the subprocess entirely (the η "full-reslice-with-cache"
    // reuse). The Arc makes the re-donation an O(1) refcount bump.
    if let Some(key) = cache_key.as_ref()
        && let Some(cache) = prior_warm_state.and_then(|s| s.downcast_ref::<FdmSliceCache>())
        && cache.key == *key
    {
        let cost = hit_cost(cache);
        return completed_with_cache(cache.clone(), cost);
    }

    // Cache MISS: export the body realization to a temp STL (the slicer's input
    // model), then slice + parse with cooperative cancellation.
    let (_body_dir, body_path) = match export_body_stl(realization_inputs) {
        Ok(p) => p,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "fdm_slice: failed to export the body to an STL for slicing: {e}"
                ))],
            };
        }
    };

    let cancel_poll = || cancellation.is_cancelled();
    let start = Instant::now();
    match slice_body(
        Some(bin),
        &body_path,
        &settings,
        &cancel_poll,
        SLICE_CANCEL_GRACE,
    ) {
        Ok(toolpath) => {
            // cost_per_byte = measured wall-clock / serialized Toolpath size.
            let elapsed = start.elapsed().as_secs_f64();
            let serialized_len = serialize_toolpath_canonical(&toolpath).len();
            let value = toolpath_to_value(&toolpath);
            let cost_per_byte =
                (elapsed > 0.0 && serialized_len > 0).then(|| elapsed / serialized_len as f64);
            match cache_key {
                Some(key) => {
                    let cache = FdmSliceCache {
                        key,
                        result: Arc::new(value),
                    };
                    completed_with_cache(cache, cost_per_byte)
                }
                None => ComputeOutcome::Completed {
                    result: value,
                    new_warm_state: None,
                    cost_per_byte,
                    diagnostics: Vec::new(),
                },
            }
        }
        // Cancellation: the engine's Cancelled arm already leaves the prior cache +
        // output VCs intact — the trampoline only signals the outcome.
        Err(SliceError::Cancelled) => ComputeOutcome::Cancelled,
        // A genuine slicer / parse / io failure surfaces as Failed (an Error
        // diagnostic); SlicerUnavailable never reaches here (bin is Some).
        Err(e) => ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!("fdm_slice: {e}"))],
        },
    }
}

// ── present-slicer helpers: settings, cache key, warm state, STL export ─────────

/// SIGTERM→grace→SIGKILL window forwarded to [`slice_body`] for cooperative
/// cancellation (the child is always reaped — no orphan/zombie).
const SLICE_CANCEL_GRACE: Duration = Duration::from_millis(500);

/// Read the mechanically-relevant [`SliceSettings`] off the `FDMProcess` value
/// (`value_inputs[1]`, mirroring the stdlib `fdm_slice(body, process, options)`
/// arg order). Missing / `Undef` fields fall back to conventional defaults so a
/// partial process still yields a deterministic, sliceable profile.
fn read_slice_settings(value_inputs: &[Value]) -> SliceSettings {
    let process = value_inputs.get(1).and_then(struct_data);
    SliceSettings {
        // `field_scalar` yields the SI-metre magnitude of the `Length` field
        // (`0.2mm` -> 0.0002 m); `SliceSettings.layer_height` is documented in mm
        // and passed verbatim to PrusaSlicer `--layer-height` (which expects mm),
        // so convert m -> mm (×1000). The `.unwrap_or(0.2)` Undef fallback is
        // already mm, so the real-process and Undef paths now agree.
        layer_height: process
            .and_then(|p| field_scalar(p, "layer_height"))
            .map(|m| m * 1000.0)
            .unwrap_or(0.2),
        // Undef-path fallbacks mirror the stdlib `FDMProcess` defaults
        // (walls = 3, top_bottom_layers = 4) so an Undef process yields the same
        // profile `FDMProcess()` would, all in one consistent (mm) unit system.
        walls: process.and_then(|p| field_int(p, "walls")).unwrap_or(3).max(0) as u32,
        top_bottom_layers: process
            .and_then(|p| field_int(p, "top_bottom_layers"))
            .unwrap_or(4)
            .max(0) as u32,
        infill_density: process
            .and_then(|p| field_real(p, "infill_density"))
            .unwrap_or(0.2),
        infill_pattern: process
            .map(read_infill_pattern)
            .unwrap_or(InfillPattern::Gyroid),
    }
}

/// Map the `FDMProcess.infill_pattern` enum value to an [`InfillPattern`];
/// unknown / absent → `Gyroid` (mirrors `as_printed_material::read_pattern`).
fn read_infill_pattern(process: &reify_ir::StructureInstanceData) -> InfillPattern {
    match process.fields.get("infill_pattern") {
        Some(Value::Enum { variant, .. }) => match variant.as_str() {
            "Cubic" => InfillPattern::Cubic,
            "Grid" => InfillPattern::Grid,
            "Triangular" => InfillPattern::Triangular,
            "Honeycomb" => InfillPattern::Honeycomb,
            _ => InfillPattern::Gyroid,
        },
        _ => InfillPattern::Gyroid,
    }
}

/// Deterministic hash of the slicing-relevant settings — the "composed-settings
/// hash" half of [`FdmSliceCacheKey`]. Uses the canonical `infill_pattern_arg`
/// spelling + bit-exact `f64`s so identical settings hash identically
/// (`DefaultHasher` is fixed-seeded, so this is stable within a process — all the
/// cache key needs, since the key is recomputed-and-compared in the same run).
fn settings_hash(s: &SliceSettings) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.layer_height.to_bits().hash(&mut h);
    s.walls.hash(&mut h);
    s.top_bottom_layers.hash(&mut h);
    s.infill_density.to_bits().hash(&mut h);
    infill_pattern_arg(s.infill_pattern).hash(&mut h);
    h.finish()
}

/// Content-hash cache key for a `fdm::slice` dispatch: the body realization's
/// content hash plus the composed-settings hash. Identical `(body, settings)` →
/// identical key → a warm-state cache HIT that skips the subprocess (PRD η).
///
/// NOTE: `value_inputs[2]` (`FDMSliceOptions`, currently just `target_fidelity`)
/// is deliberately EXCLUDED from the key — and is safe to exclude — ONLY because
/// `target_fidelity` is presently an inert no-op placeholder that does not reach
/// the slicer. The moment any options field actually influences slicer output, it
/// MUST be folded into `settings_hash` (or otherwise into this key); otherwise a
/// HIT keyed only on `(body, process)` would silently return a Toolpath sliced at
/// the wrong fidelity.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FdmSliceCacheKey {
    body_hash: u128,
    settings_hash: u64,
}

/// Warm-state cache entry for a completed slice: the key it was computed for plus
/// the marshalled Toolpath `Value` (behind an `Arc` so a HIT re-donation is an
/// O(1) refcount bump). Modelled on `trajectory_ops::ComputeResultCache<K>`.
#[derive(Clone)]
struct FdmSliceCache {
    key: FdmSliceCacheKey,
    result: Arc<Value>,
}

impl FdmSliceCache {
    /// Coarse heap-size estimate (the flat key + the marshalled Toolpath tree).
    fn estimated_size_bytes(&self) -> usize {
        std::mem::size_of::<FdmSliceCacheKey>() + value_size_estimate(self.result.as_ref())
    }
}

/// `cost_per_byte` for a cache HIT re-donation: the inverse heap size (the
/// `trajectory_ops::completed_donating` convention for a cheap reuse). The fresh
/// MISS path uses the measured wall-clock / serialized-size cost instead.
fn hit_cost(cache: &FdmSliceCache) -> Option<f64> {
    let size = cache.estimated_size_bytes();
    (size > 0).then(|| 1.0 / size as f64)
}

/// Build a `Completed` outcome donating `cache` as the node's warm state, with
/// the given `cost_per_byte`. One deep clone for the output value cell; the
/// warm-state copy reuses the same `Arc<Value>`.
fn completed_with_cache(cache: FdmSliceCache, cost_per_byte: Option<f64>) -> ComputeOutcome {
    let result = cache.result.as_ref().clone();
    let size = cache.estimated_size_bytes();
    ComputeOutcome::Completed {
        result,
        new_warm_state: Some(OpaqueState::new(cache, size)),
        cost_per_byte,
        diagnostics: Vec::new(),
    }
}

/// Coarse heap-size estimate of a `Value` tree (mirrors
/// `trajectory_ops::value_size_estimate`; kept local — that copy is private to
/// `trajectory_ops`).
fn value_size_estimate(v: &Value) -> usize {
    let base = std::mem::size_of::<Value>();
    match v {
        Value::String(s) => base + s.len(),
        Value::List(items) | Value::Point(items) | Value::Vector(items) => {
            base + items.iter().map(value_size_estimate).sum::<usize>()
        }
        Value::StructureInstance(d) => {
            base + d.type_name.len()
                + d.fields
                    .iter()
                    .map(|(k, val)| k.len() + value_size_estimate(val))
                    .sum::<usize>()
        }
        _ => base,
    }
}

/// Export the body realization (`realization_inputs[0]`) to a temp **binary STL**
/// — the model file the slicer consumes. Writes the surface mesh when present;
/// otherwise a minimal empty (zero-triangle) STL (the slicer-stub tests ignore
/// the model, and a real slicer simply yields an empty toolpath for empty input).
/// Returns the owning `TempDir` (kept alive until slicing finishes) + the path.
fn export_body_stl(
    realization_inputs: &[RealizationReadHandle],
) -> std::io::Result<(tempfile::TempDir, PathBuf)> {
    use std::io::Write as _;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("reify-body.stl");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&path)?);
    match realization_inputs.first().and_then(|h| h.surface_mesh()) {
        Some(mesh) => {
            // Reify geometry is in SI metres, but PrusaSlicer (and the binary-STL
            // convention it follows) interprets STL coordinates as millimetres.
            // Scale a copy of the mesh m -> mm (×1000) so a 10mm part is presented
            // as 10mm, not 0.01mm — consistent with the layer_height mm contract.
            // `indices` are unchanged; `normals` are unit directions needing no
            // scaling (write_stl_binary recomputes per-facet normals from the
            // scaled vertices and never reads `mesh.normals`).
            let scaled = reify_ir::Mesh {
                vertices: mesh.vertices.iter().map(|&v| v * 1000.0).collect(),
                indices: mesh.indices.clone(),
                normals: mesh.normals.clone(),
            };
            reify_ir::write_stl_binary(&scaled, &mut f)?;
        }
        // Minimal valid binary STL: 80-byte header + a u32 zero triangle count.
        None => {
            f.write_all(&[0u8; 80])?;
            f.write_all(&0u32.to_le_bytes())?;
        }
    }
    f.flush()?;
    Ok((dir, path))
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

    /// Two **independently-constructed but equal** Toolpaths marshal to
    /// structurally-equal Values — the run-to-run Value determinism the
    /// content-hash cache key relies on. Built as two separate allocations (not
    /// `f(x) == f(x)` on one instance) so a captured heap pointer or an
    /// insertion-order-dependent field map would actually break the assertion.
    #[test]
    fn marshalling_is_deterministic() {
        let a = sample_toolpath();
        let b = sample_toolpath();
        assert_eq!(
            toolpath_to_value(&a),
            toolpath_to_value(&b),
            "two independently-built equal Toolpaths marshal to equal Values"
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

    /// A pre-cancelled dispatch against a long-sleeper stub slicer returns
    /// `ComputeOutcome::Cancelled` promptly — the `|| is_cancelled()` poll reaches
    /// `run_slicer`, which SIGTERM→reaps the child (no orphan).
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

    /// A fresh dispatch (no prior warm state) runs the stub slicer once and returns
    /// `Completed` with a donated warm state + positive `cost_per_byte`; a second
    /// dispatch with that warm state + identical inputs HITs the cache, reuses the
    /// Toolpath value, and does NOT re-run the slicer (the run-count seam stays at 1).
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

    /// REVIEW-FIX (blocking issue 1/2, robustness_unit_mismatch): `read_slice_settings`
    /// must convert the `Length` field's SI-metre magnitude to millimetres. A real
    /// `FDMProcess` has `layer_height = 0.2mm` → a `Length` Scalar with `si_value
    /// 0.0002` (m); PrusaSlicer `--layer-height` (and `SliceSettings`' documented mm
    /// contract) expect mm, so the read must yield 0.2 mm, NOT the raw 0.0002.
    /// Platform-independent — no `#[cfg(unix)]` gate.
    #[test]
    fn read_slice_settings_converts_layer_height_metres_to_mm() {
        use reify_core::DimensionVector;
        // `0.2mm` evaluates to a Length Scalar of si_value 0.0002 m; `field_scalar`
        // ignores the dimension, so LENGTH is just for realism.
        let process = structure(
            "FDMProcess",
            vec![(
                "layer_height",
                Value::Scalar {
                    si_value: 0.0002,
                    dimension: DimensionVector::LENGTH,
                },
            )],
        );
        let settings = read_slice_settings(&[Value::Undef, process, Value::Undef]);
        assert_eq!(
            settings.layer_height, 0.2,
            "0.0002 m must convert to 0.2 mm, not stay 0.0002"
        );
        // …and the composed slicer arg is the mm value, not the raw metre value.
        let args =
            reify_fdm::compose_slicer_args(&settings, std::path::Path::new("/tmp/out.gcode"));
        let idx = args
            .iter()
            .position(|a| a == "--layer-height")
            .expect("--layer-height present in composed args");
        assert_eq!(
            args[idx + 1], "0.2",
            "--layer-height must be 0.2 (mm), not 0.0002; got {args:?}"
        );
        // The Undef-process fallback is mm-consistent with the converted real path.
        let undef = read_slice_settings(&[Value::Undef, Value::Undef, Value::Undef]);
        assert_eq!(undef.layer_height, 0.2, "Undef fallback stays 0.2 mm");
    }

    /// REVIEW-FIX (blocking issue 2/2, robustness_unit_mismatch): `export_body_stl`
    /// must scale the SI-metre surface mesh to millimetres (×1000) before writing the
    /// STL PrusaSlicer consumes, since the binary-STL convention is mm. A 0.01 m
    /// (= 10 mm) triangle must appear as ~10.0 in the written coordinates, not 0.01.
    /// Platform-independent — no `#[cfg(unix)]` gate.
    #[test]
    fn export_body_stl_scales_metres_to_millimetres() {
        use crate::engine_compute::RealizedContent;
        use reify_core::{ContentHash, RealizationNodeId};
        // A single right triangle spanning 0.01 m = 10 mm in SI-metre mesh coords.
        let mesh = reify_ir::Mesh {
            vertices: vec![0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0],
            indices: vec![0, 1, 2],
            normals: None,
        };
        let handle = RealizationReadHandle::new(
            RealizationNodeId::new("body", 0),
            ContentHash(0),
            Some(RealizedContent::SurfaceMesh(Arc::new(mesh))),
        );
        let (_dir, path) = export_body_stl(&[handle]).expect("export_body_stl writes the STL");
        let bytes = std::fs::read(&path).expect("read the written STL");

        // Binary STL: 80-byte header, u32 little-endian triangle count, then a
        // 50-byte record per triangle (12-byte facet normal + 9×f32 vertices + 2).
        let tri_count = u32::from_le_bytes(bytes[80..84].try_into().unwrap());
        assert_eq!(tri_count, 1, "exactly one triangle written");
        let mut max_coord = 0.0f32;
        for i in 0..9 {
            let off = 84 + 12 + i * 4;
            let c = f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
            max_coord = max_coord.max(c.abs());
        }
        assert!(
            (max_coord - 10.0).abs() < 1e-3,
            "metre→mm ×1000 scaling: max written coord should be ~10.0 mm, got {max_coord}"
        );
    }

    // ── step-1 (task #4874): realization-absent cache-key collision tests ─────────

    /// A present-slicer dispatch with an empty `realization_inputs` slice (no body
    /// realization handle) runs the slicer and returns a real Toolpath value, but
    /// MUST NOT donate a warm state — there is no content hash to key the reslice
    /// cache, so caching would alias distinct realization-less bodies.
    ///
    /// On HEAD this fails: the `unwrap_or(0)` sentinel causes the dispatch to donate
    /// a warm state keyed `(body_hash=0, settings_hash)`, which is indistinguishable
    /// from a genuine `content_hash == 0` body.
    #[cfg(unix)]
    #[test]
    fn realization_absent_present_slicer_donates_no_warm_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counter = dir.path().join("run-count");
        let stub = write_stub_script(
            dir.path(),
            "ok-slicer.sh",
            &emit_fixture_counting_body(&fixture_gcode_path(), &counter),
        );

        let inputs = undef_inputs();
        let none: [RealizationReadHandle; 0] = [];
        let never = CancellationHandle::new();

        let outcome = fdm_slice_dispatch(&inputs, &none, Some(&stub), None, &never);
        match outcome {
            ComputeOutcome::Completed {
                result,
                new_warm_state,
                cost_per_byte,
                ..
            } => {
                assert!(
                    new_warm_state.is_none(),
                    "a realization-less dispatch must NOT donate a warm state (collision-unsafe)"
                );
                assert!(
                    cost_per_byte.is_some_and(|c| c > 0.0),
                    "the slicer still ran and reported a cost_per_byte, got {cost_per_byte:?}"
                );
                let beads = as_list(field(&result, "beads").expect("beads field"));
                assert!(!beads.is_empty(), "the fixture slice still produces beads");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// A warm state donated by a `content_hash == 0` body realization must NOT be
    /// served to a realization-less dispatch — the cache key's `body_hash=0` is a
    /// legitimate hash, not the "absent" sentinel. The realization-less dispatch must
    /// MISS and re-run the slicer (run-count advances from 1 to 2).
    ///
    /// On HEAD this fails: `unwrap_or(0)` aliases the realization-less key to the
    /// hash-0 warm state → a wrong cache HIT that does NOT re-run the slicer.
    #[cfg(unix)]
    #[test]
    fn realization_absent_does_not_hit_zero_hash_warm_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counter = dir.path().join("run-count");
        let stub = write_stub_script(
            dir.path(),
            "ok-slicer.sh",
            &emit_fixture_counting_body(&fixture_gcode_path(), &counter),
        );

        let inputs = undef_inputs();
        let never = CancellationHandle::new();

        // ── dispatch 1: body with content_hash == 0; donates warm state ──────────
        let realizations = [body_handle(0)];
        let warm = match fdm_slice_dispatch(&inputs, &realizations, Some(&stub), None, &never) {
            ComputeOutcome::Completed { new_warm_state, .. } => {
                new_warm_state.expect("a hash-0 realization must donate warm state")
            }
            other => panic!("dispatch 1 expected Completed, got {other:?}"),
        };
        let count_after_first = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        assert_eq!(count_after_first, 1, "slicer ran exactly once on dispatch 1");

        // ── dispatch 2: no realization; prior warm state is the hash-0 donation ──
        // Must MISS (non-cacheable) and re-run the slicer, NOT alias hash-0 key.
        let none: [RealizationReadHandle; 0] = [];
        match fdm_slice_dispatch(&inputs, &none, Some(&stub), Some(&warm), &never) {
            ComputeOutcome::Completed { .. } => {}
            other => panic!("dispatch 2 expected Completed, got {other:?}"),
        }
        let count_after_second = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        assert_eq!(
            count_after_second, 2,
            "realization-less dispatch must NOT hit the hash-0 warm state; slicer must re-run"
        );
    }
}
