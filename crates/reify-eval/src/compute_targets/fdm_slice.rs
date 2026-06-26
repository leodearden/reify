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
// The implementation is built incrementally across task η steps 13–18; this
// placeholder keeps the module well-formed before the first RED test lands.

#[cfg(test)]
mod tests {
    use super::*;
    use reify_fdm::{Bead, BeadRole, Layer, Toolpath};
    use reify_ir::Value;

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
}
