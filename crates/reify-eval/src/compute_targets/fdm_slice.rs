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
/// # Units: the DSL-visible surface is SI and dimensioned
///
/// THIS FUNCTION IS THE UNIT-REGIME BOUNDARY, and this is its canonical
/// statement — `reify_fdm::Toolpath` stays in native G-code millimetres /
/// mm·min⁻¹ / °C (see that struct's docs for why), and the projection built
/// here converts, for EVERY dimensional field, because a half-SI surface would
/// leave the rule unstatable:
///
/// - `width` / `height` / `layer_z` / `Layer.z` → `Length` (SI metres)
/// - the centerline → `Point3<Length>`, the shape `resolve_point3_length_arg`
///   requires of any point passed to a geometry builtin
/// - `speed` → `Velocity` (m·s⁻¹, from mm·min⁻¹)
/// - `nominal_temp` → `Temperature` (K, from °C via the +273.15 the language
///   itself declares for `degC`)
///
/// `layer_index` / `index` / `bead_indices` stay `Int`: dimensionless by
/// nature. Because each field's declared type now names its own unit, there is
/// no carve-out left to remember or to document. The `.ri` half of the
/// contract is `crates/reify-compiler/stdlib/fdm_slice.ri`, whose declared
/// field types must agree with the list above; `fdm_slice_e2e.rs`'s
/// `stdlib_bead_and_layer_fields_declare_the_si_dimensioned_regime` is what
/// keeps the two in agreement.
///
/// Each conversion is spelled by the native-unit constructor it needs
/// (`super::length_mm` / `point3_length_mm` / `velocity_mm_per_min` /
/// `temperature_deg_c`), so no factor is written at a call site here. The
/// OUTBOUND direction is separate and deliberately unshared: the PrusaSlicer
/// boundary scales m→mm by an explicit `* 1000.0` (`read_slice_settings`, and
/// the STL write reached from `export_body_stl`).
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
    let centerline = Value::List(
        b.centerline
            .iter()
            .map(|p| super::point3_length_mm(*p))
            .collect(),
    );
    structure(
        "Bead",
        vec![
            ("centerline", centerline),
            ("width", super::length_mm(b.width)),
            ("height", super::length_mm(b.height)),
            ("role", bead_role_value(b.role)),
            ("layer_index", Value::Int(b.layer_index as i64)),
            ("layer_z", super::length_mm(b.layer_z)),
            ("nominal_temp", super::temperature_deg_c(b.nominal_temp)),
            ("speed", super::velocity_mm_per_min(b.speed)),
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
            ("z", super::length_mm(l.z)),
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
    Value::enum_unit("BeadRole", variant)
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
            structured_detail: vec![],
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
                structured_detail: vec![],
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
                None => completed_no_cache(value, cost_per_byte),
            }
        }
        // Cancellation: the engine's Cancelled arm already leaves the prior cache +
        // output VCs intact — the trampoline only signals the outcome.
        Err(SliceError::Cancelled) => ComputeOutcome::Cancelled,
        // A genuine slicer / parse / io failure surfaces as Failed (an Error
        // diagnostic); SlicerUnavailable never reaches here (bin is Some).
        Err(e) => ComputeOutcome::Failed {
            diagnostics: vec![Diagnostic::error(format!("fdm_slice: {e}"))],
            structured_detail: vec![],
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
        structured_detail: vec![],
    }
}

/// Build a `Completed` outcome WITHOUT donating warm state (the realization-absent
/// path). Mirrors [`completed_with_cache`] so both success arms stay in sync if
/// future changes add diagnostics or new fields to `ComputeOutcome::Completed`.
fn completed_no_cache(result: Value, cost_per_byte: Option<f64>) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte,
        diagnostics: Vec::new(),
        structured_detail: vec![],
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
            // The metre→millimetre conversion now lives INSIDE `write_stl_binary`
            // (task #6187), which takes metres and emits millimetres, so this
            // caller hands it the mesh unscaled: a 10mm part is still presented
            // as 10mm, consistent with the layer_height mm contract. Do NOT
            // re-apply a ×1000 here — that would make this path 1,000,000×.
            // `export_body_stl_scales_metres_to_millimetres` is the guard.
            reify_ir::write_stl_binary(mesh, &mut f)?;
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
    use reify_core::DimensionVector;

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

    /// Assert that `v` is a `Value::Scalar` carrying exactly `dimension` and an
    /// `si_value` equal to `expected_si` to a 1e-12 **relative** tolerance.
    ///
    /// The dimension is checked as well as the magnitude: a magnitude-only
    /// assertion would let a right-number/wrong-dimension Scalar through, which
    /// is precisely the defect class this surface's conversion has to rule out.
    ///
    /// Relative rather than `assert_eq!` because `Value::Scalar` equality is
    /// bitwise over f64. Every expected value here is ONE f64 operation from a
    /// fixture literal, so its relative error is ≤ 2^-53 ≈ 1.11e-16 plus ~1e-16
    /// of literal representation error — 1e-12 clears that by >3000x while
    /// still failing any real unit error (the smallest of which is 1000x).
    fn assert_scalar(v: &Value, expected_si: f64, dimension: DimensionVector, what: &str) {
        match v {
            Value::Scalar {
                si_value,
                dimension: d,
            } => {
                assert_eq!(
                    *d, dimension,
                    "{what}: expected dimension {dimension:?}, got {d:?}"
                );
                let tol = expected_si.abs() * 1e-12;
                assert!(
                    (si_value - expected_si).abs() <= tol,
                    "{what}: expected si_value ~= {expected_si} (tol {tol}), got {si_value}"
                );
            }
            other => panic!("{what}: expected a dimensioned Value::Scalar, got {other:?}"),
        }
    }

    /// [`assert_scalar`] specialised to `DimensionVector::LENGTH` — the check
    /// every marshalled geometry field (and every centerline coordinate) must
    /// satisfy, in SI metres.
    fn assert_length(v: &Value, expected_m: f64, what: &str) {
        assert_scalar(v, expected_m, DimensionVector::LENGTH, what);
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
        // 0.2 mm -> 2.0e-4 m, Length-dimensioned (the DSL-visible surface is SI).
        assert_length(
            field(&layers[0], "z").expect("layer z field"),
            2.0e-4,
            "layer 0 z (0.2 mm)",
        );
        let bead_indices = as_list(field(&layers[1], "bead_indices").expect("bead_indices"));
        assert_eq!(bead_indices.len(), 1);
        assert_eq!(bead_indices[0], Value::Int(1), "layer 1 owns bead 1");
    }

    /// Assert that `v` is a `Value::Point` of EXACTLY three LENGTH-dimensioned
    /// `Value::Scalar` components, returning their SI-metre magnitudes — the
    /// shape `resolve_point3_length_arg` (`geometry_ops.rs`) requires of any
    /// point fed to a geometry builtin. Bare-`Real` components fail it
    /// (returning None + a Warning), so a centerline built from them is a dead
    /// end in the language; this is the property that makes `Bead.centerline`
    /// actually usable.
    ///
    /// The single shape check for marshalled centerline points: callers that
    /// know the expected coordinates use [`assert_point3_length`], callers that
    /// only bound them (the end-to-end SI-envelope check) use this directly.
    fn point3_length_coords(v: &Value, what: &str) -> [f64; 3] {
        match v {
            Value::Point(coords) => {
                assert_eq!(coords.len(), 3, "{what}: expected exactly 3 components");
                let mut out = [0.0_f64; 3];
                for (i, c) in coords.iter().enumerate() {
                    match c {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert_eq!(
                                *dimension,
                                DimensionVector::LENGTH,
                                "{what} component {i}: expected LENGTH, got {dimension:?}"
                            );
                            out[i] = *si_value;
                        }
                        other => panic!(
                            "{what} component {i}: expected a dimensioned Scalar, got {other:?}"
                        ),
                    }
                }
                out
            }
            other => panic!("{what}: expected a Value::Point, got {other:?}"),
        }
    }

    /// [`point3_length_coords`] plus an expected-value check on each coordinate
    /// (to [`assert_scalar`]'s relative tolerance).
    fn assert_point3_length(v: &Value, expected_m: [f64; 3], what: &str) {
        let coords = point3_length_coords(v, what);
        for (i, (c, e)) in coords.iter().zip(expected_m.iter()).enumerate() {
            let what_i = format!("{what} component {i}");
            let tol = e.abs() * 1e-12;
            assert!(
                (c - e).abs() <= tol,
                "{what_i}: expected si_value ~= {e} (tol {tol}), got {c}"
            );
        }
    }

    /// Each marshalled bead carries its role (as a `BeadRole` enum value), its
    /// integer layer index, its centerline polyline as a List of
    /// `Point3<Length>`, and every dimensional scalar as an SI, dimensioned
    /// `Value::Scalar` — Length (m), Velocity (m·s⁻¹) and Temperature (K),
    /// converted from the Rust struct's native G-code mm / mm·min⁻¹ / °C at
    /// this marshalling boundary.
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
                payload: vec![],
            }),
            "Perimeter maps to the BeadRole::Perimeter enum value"
        );
        // Geometry: native mm in the Rust struct -> SI metres here (x 1e-3).
        assert_length(
            field(&beads[0], "width").expect("width field"),
            4.5e-4,
            "bead 0 width (0.45 mm)",
        );
        assert_length(
            field(&beads[0], "height").expect("height field"),
            2.0e-4,
            "bead 0 height (0.2 mm)",
        );
        assert_eq!(field(&beads[0], "layer_index"), Some(&Value::Int(0)));
        assert_length(
            field(&beads[0], "layer_z").expect("layer_z field"),
            2.0e-4,
            "bead 0 layer_z (0.2 mm)",
        );
        // The SI regime is TOTAL, not Length-only: a half-converted surface
        // would leave the rule unstatable and every field a thing to look up.
        //
        // 1800 mm·min⁻¹ = 1.8 m·min⁻¹ = 0.03 m·s⁻¹ (÷ 60_000).
        assert_scalar(
            field(&beads[0], "speed").expect("speed field"),
            0.03,
            DimensionVector::VELOCITY,
            "bead 0 speed (1800 mm/min)",
        );
        // 210 °C = 483.15 K. The +273.15 offset is not a free choice — it is the
        // offset the language itself declares for degC (stdlib/units.ri,
        // `pub unit degC : Temperature = 1 offset 273.15`). A Temperature-
        // dimensioned Scalar carries kelvin, so a design author writing
        // `bead.nominal_temp > 200degC` only gets the right answer in K.
        assert_scalar(
            field(&beads[0], "nominal_temp").expect("nominal_temp field"),
            483.15,
            DimensionVector::TEMPERATURE,
            "bead 0 nominal_temp (210 degC)",
        );

        let cl0 = as_list(field(&beads[0], "centerline").expect("centerline field"));
        assert_eq!(cl0.len(), 2, "bead 0 has two centerline points");
        // [0, 0, 0.2] mm and [10, 0, 0.2] mm -> metres.
        assert_point3_length(&cl0[0], [0.0, 0.0, 2.0e-4], "bead 0 centerline point 0");
        assert_point3_length(&cl0[1], [1.0e-2, 0.0, 2.0e-4], "bead 0 centerline point 1");

        // The second bead's distinct role maps through too.
        assert_eq!(
            field(&beads[1], "role"),
            Some(&Value::Enum {
                type_name: "BeadRole".to_string(),
                variant: "SolidInfill".to_string(),
                payload: vec![],
            }),
            "SolidInfill maps to the BeadRole::SolidInfill enum value"
        );
        let cl1 = as_list(field(&beads[1], "centerline").unwrap());
        assert_eq!(cl1.len(), 3, "bead 1 has three centerline points");
        // Every point of every bead carries the Point3<Length> shape, not just
        // the first bead's: [0,0,0.4], [10,0,0.4], [10,5,0.4] mm -> metres.
        assert_point3_length(&cl1[0], [0.0, 0.0, 4.0e-4], "bead 1 centerline point 0");
        assert_point3_length(&cl1[1], [1.0e-2, 0.0, 4.0e-4], "bead 1 centerline point 1");
        assert_point3_length(&cl1[2], [1.0e-2, 5.0e-3, 4.0e-4], "bead 1 centerline point 2");
    }

    /// The feedrate conversion DIVIDES by 60_000 rather than multiplying by a
    /// rounded reciprocal — asserted bitwise, because that is the only way to
    /// observe the difference.
    ///
    /// `1800.0 * (1.0 / 60_000.0)` is 0.030000000000000002; `1800.0 / 60_000.0`
    /// is exactly 0.03. The gap is ~7e-17 relative, so
    /// [`assert_scalar`]'s 1e-12 tolerance (sized to catch unit errors, the
    /// smallest of which is 1000x) cannot see it and neither could any
    /// tolerance-based check. Hence `assert_eq!` on the f64 here: it is the
    /// guard that keeps a round feedrate round through the marshalling
    /// boundary, so a design author's `bead.speed == 30mm/s` is not defeated by
    /// a representation artefact.
    #[test]
    fn speed_conversion_divides_rather_than_multiplying_a_reciprocal() {
        let v = toolpath_to_value(&sample_toolpath());
        let beads = as_list(field(&v, "beads").unwrap());
        let speed = match field(&beads[0], "speed").expect("speed field") {
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("speed must be a dimensioned Scalar, got {other:?}"),
        };
        assert_eq!(
            speed, 0.03,
            "1800 mm/min must marshal to exactly 0.03 m/s; got {speed:?} \
             (multiplying by a rounded 1.0/60_000.0 reciprocal yields \
             0.030000000000000002)"
        );
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
                ..
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

    /// END-TO-END unit-regime pin: G-code **text** → the ζ parser →
    /// [`toolpath_to_value`] → DSL `Value`, over the whole path rather than from a
    /// hand-built [`Bead`].
    ///
    /// Every other regime test in this module starts from `sample_toolpath()` (a
    /// hand-built struct) or from a compile-only `.ri` snippet, so none of them can
    /// see a COMPOUNDING error between the parser's native millimetres and this
    /// module's conversion — a parser that silently pre-scaled, or a `MM_TO_M`
    /// applied twice, reads identically at those seams. This test drives the
    /// production [`fdm_slice_dispatch`] through the module's stub-slicer seam (a
    /// `#!/bin/sh` that `cp`s the committed ζ fixture to the composed `-o` path), so
    /// the Values asserted below are the ones a design author actually receives.
    ///
    /// Deliberately NOT a live PrusaSlicer run: no slicer is on `$PATH` here (which
    /// is why `real_slicer_build_is_deterministic_verify_and_lock` skips), and a real
    /// slice's widths and heights come from PrusaSlicer's own config rather than from
    /// anything the `.ri` declares, so its expected values would not be derivable.
    /// Every expectation below IS derivable — one f64 operation from a literal in
    /// `crates/reify-fdm/tests/fixtures/prusaslicer_bracket.gcode`:
    ///
    /// - every `;WIDTH:` in the fixture is `0.45` → `width` == 4.5e-4 m
    /// - every `;HEIGHT:` is `0.2` → `height` == 2.0e-4 m
    /// - `;Z:0.2` / `;Z:0.4` → `layers[0].z` == 2.0e-4 m, `layers[1].z` == 4.0e-4 m
    /// - `M109 S210` holds for every bead (the `M104 S0` sits after the last
    ///   extrusion) → `nominal_temp` == 483.15 K
    /// - every pen-down travel carries `F9000` → `speed` == 0.15 m·s⁻¹
    /// - the `G1 X0 Y0 F9000` before the first `;TYPE:External perimeter` extrude →
    ///   that bead's first centerline point == `[0, 0, 2.0e-4]` m
    #[cfg(unix)]
    #[test]
    fn gcode_text_marshals_into_the_si_regime_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let counter = dir.path().join("run-count");
        let stub = write_stub_script(
            dir.path(),
            "si-regime-slicer.sh",
            &emit_fixture_counting_body(&fixture_gcode_path(), &counter),
        );

        let inputs = undef_inputs();
        let realizations = [body_handle(0x6301)];
        let never = CancellationHandle::new();

        let result = match fdm_slice_dispatch(&inputs, &realizations, Some(&stub), None, &never) {
            ComputeOutcome::Completed { result, .. } => result,
            other => panic!("the stub-slicer dispatch expected Completed, got {other:?}"),
        };
        // The stub really ran: these Values came from parsing the fixture TEXT, not
        // from the empty `degraded_toolpath_value` (whose fields would vacuously pass
        // every per-bead assertion below).
        let runs = std::fs::read_to_string(&counter)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        assert_eq!(runs, 1, "the stub slicer ran exactly once");

        let beads = as_list(field(&result, "beads").expect("beads field"));
        let layers = as_list(field(&result, "layers").expect("layers field"));

        // Counts read off the fixture, not guessed: two `;LAYER_CHANGE` blocks, and
        // per layer four structural bead runs — `External perimeter` and `Perimeter`
        // (one role, but the travel between them breaks the run) plus the infill
        // pass's two extrudes, which a travel likewise separates. The
        // `;TYPE:Skirt/Brim` run contributes NOTHING: `role_from_prusaslicer_type`
        // maps it to `None`, so its extrusions are skipped.
        assert_eq!(
            layers.len(),
            2,
            "the fixture has two `;LAYER_CHANGE` layers"
        );
        assert_eq!(beads.len(), 8, "four structural bead runs per layer");

        // Per-bead invariants over EVERY bead rather than index-by-index pins: the
        // fixture is uniform in width / height / temperature / feedrate, so a
        // conversion that missed one bead — or one field — surfaces here.
        for (i, b) in beads.iter().enumerate() {
            assert_length(
                field(b, "width").expect("width field"),
                4.5e-4,
                &format!("bead {i} width (;WIDTH:0.45)"),
            );
            assert_length(
                field(b, "height").expect("height field"),
                2.0e-4,
                &format!("bead {i} height (;HEIGHT:0.2)"),
            );
            assert_scalar(
                field(b, "nominal_temp").expect("nominal_temp field"),
                483.15,
                DimensionVector::TEMPERATURE,
                &format!("bead {i} nominal_temp (M109 S210 → 210 °C)"),
            );
            assert_scalar(
                field(b, "speed").expect("speed field"),
                0.15,
                DimensionVector::VELOCITY,
                &format!("bead {i} speed (F9000 mm·min⁻¹)"),
            );

            // `layer_z` agrees with the owning layer's `;Z:` in the same regime.
            let layer_index = match field(b, "layer_index").expect("layer_index field") {
                Value::Int(n) => *n,
                other => panic!("bead {i} layer_index must be an Int, got {other:?}"),
            };
            let expected_layer_z = if layer_index == 0 { 2.0e-4 } else { 4.0e-4 };
            assert_length(
                field(b, "layer_z").expect("layer_z field"),
                expected_layer_z,
                &format!("bead {i} layer_z (layer {layer_index})"),
            );

            let centerline = as_list(field(b, "centerline").expect("centerline field"));
            assert!(
                centerline.len() >= 2,
                "bead {i} must be a real polyline, got {} points",
                centerline.len()
            );
            for (k, p) in centerline.iter().enumerate() {
                let coords = point3_length_coords(p, &format!("bead {i} centerline point {k}"));
                // The fixture's part occupies 0..10 mm in X/Y and 0.2..0.4 mm in Z, so
                // in SI every coordinate lies within [0, 1.1e-2] m. Read as millimetres
                // the same points are 0..10 — 1000x outside this envelope — which makes
                // the bound the regime assertion at scale; it simultaneously excludes
                // the skirt's x/y = -1 mm, the only negative coordinates in the file.
                for (axis, c) in coords.iter().enumerate() {
                    assert!(
                        (0.0..=1.1e-2).contains(c),
                        "bead {i} centerline point {k} axis {axis}: {c} m falls outside the \
                         fixture's SI envelope [0, 1.1e-2] m — either the mm→m conversion did \
                         not happen (0..10 read as mm) or a sacrificial Skirt/Brim extrusion \
                         (x/y = -1 mm) leaked through"
                    );
                }
            }
        }

        // The Skirt/Brim run really produced no bead, so the FIRST bead is the
        // External perimeter that follows it — pinned by its role and by the pen-down
        // point `G1 X0 Y0 F9000` leaves it at, on the `;Z:0.2` layer.
        assert_eq!(
            field(&beads[0], "role"),
            Some(&Value::Enum {
                type_name: "BeadRole".to_string(),
                variant: "Perimeter".to_string(),
                payload: vec![],
            }),
            "the first bead is the External perimeter (the preceding Skirt/Brim is skipped)"
        );
        let cl0 = as_list(field(&beads[0], "centerline").expect("centerline field"));
        assert_point3_length(
            &cl0[0],
            [0.0, 0.0, 2.0e-4],
            "first bead pen-down point (G1 X0 Y0, `;Z:0.2`)",
        );
        assert_point3_length(
            &cl0[1],
            [1.0e-2, 0.0, 2.0e-4],
            "first bead first extrude endpoint (G1 X10 Y0)",
        );

        // Layer Z in the same SI regime as the beads sitting on it, and the layers
        // partition the beads (so the 8 above are all reachable from a `Layer`).
        assert_eq!(field(&layers[0], "index"), Some(&Value::Int(0)));
        assert_length(
            field(&layers[0], "z").expect("layer z field"),
            2.0e-4,
            "layer 0 z (;Z:0.2)",
        );
        assert_length(
            field(&layers[1], "z").expect("layer z field"),
            4.0e-4,
            "layer 1 z (;Z:0.4)",
        );
        let owned: usize = layers
            .iter()
            .map(|l| as_list(field(l, "bead_indices").expect("bead_indices field")).len())
            .sum();
        assert_eq!(owned, beads.len(), "the layers partition every bead");
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

    /// REVIEW-FIX (blocking issue 2/2, robustness_unit_mismatch): the STL
    /// PrusaSlicer consumes must carry MILLIMETRES, since the binary-STL
    /// convention is mm. A 0.01 m (= 10 mm) triangle must appear as ~10.0 in the
    /// written coordinates, not 0.01.
    ///
    /// The ×1000 conversion has since MOVED into `reify_ir::write_stl_binary`
    /// (task #6187), so `export_body_stl` hands it the SI-metre mesh unscaled
    /// and these assertions are unchanged. That makes this test the
    /// DOUBLE-SCALE guard: re-introducing a caller-side ×1000 makes the path
    /// 1,000,000× and lands here as `max_coord = 10000`.
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
