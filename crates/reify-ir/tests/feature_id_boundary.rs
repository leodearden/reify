//! Cross-cutting boundary/regression gate for the structured `FeatureId` /
//! `Value::Feature` contract (P1 ε, task #4810).
//!
//! This is the B+H integration gate over already-landed, green foundation
//! work: P1 α (`FeatureId`, #4806) and P1 γ (`Value::Feature`/`Type::Feature`,
//! #4808). It is a committed CHARACTERIZATION suite, not a RED-first driver —
//! a failing row here means a genuine cross-cutting contract regression in
//! α/γ, never a bug to silently patch upstream from this file.
//!
//! Rows covered here (see `docs/prds/naming-convergence/P1-structured-featureid-feature-value.md`):
//! B1, B2, B3, B4, B5, and the value-model half of B9.
//!
//! Ownership notes (asserted through PUBLIC seams only — integration tests
//! cannot reach `pub(crate)`/private items):
//! - B5's raw-bytes golden is owned by `feature_id_content_hash_bytes_golden`
//!   in `crates/reify-ir/src/geometry.rs` (needs the `pub(crate)`
//!   `FeatureId::content_hash_bytes`). This file asserts B5's
//!   PUBLIC-observable consequence via `Value::Feature(_).content_hash()`.
//! - B9's exhaustiveness half (`assert_all_value_variants_listed` /
//!   `assert_all_type_variants_listed`) is owned by
//!   `crates/reify-eval/tests/m8_m11_regression_checkpoint.rs`.

use reify_core::{RealizationNodeId, Type};
use reify_ir::{FeatureId, Value};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

/// B1: a `Realization` root built via `From<&RealizationNodeId>` round-trips
/// losslessly through `entity()`/`index()`/`Display`.
#[test]
fn b1_realization_round_trips_losslessly() {
    let rn = RealizationNodeId::new("Foo", 3);
    let f = FeatureId::from(&rn);
    assert_eq!(f.entity(), "Foo");
    assert_eq!(f.index(), 3);
    assert_eq!(f.to_string(), "Foo#realization[3]");
}

/// B2: `derived_mid_surface` preserves the realization root's `entity()`/
/// `index()` while changing the `Display` grammar to append `/mid_surface`.
#[test]
fn b2_derived_preserves_root() {
    let root = FeatureId::realization("Foo", 3);
    let d = FeatureId::derived_mid_surface(&root);
    assert_eq!(d.entity(), "Foo");
    assert_eq!(d.index(), 3);
    assert_eq!(d.to_string(), "Foo#realization[3]/mid_surface");
}

/// B3 (premise I2): `Display` is the exact inverse of `FromStr` over a
/// matrix of {Realization, single Derived, doubly-nested Derived} shapes,
/// and `TryFrom<&str>` / `.parse()` agree with `FromStr`.
#[test]
fn b3_display_from_str_round_trip_matrix() {
    let cases = [
        FeatureId::realization("Foo", 3),
        FeatureId::derived_mid_surface(&FeatureId::realization("Foo", 3)),
        FeatureId::derived_mid_surface(&FeatureId::derived_mid_surface(&FeatureId::realization(
            "Foo", 3,
        ))),
    ];
    for x in cases {
        let s = x.to_string();
        assert_eq!(FeatureId::from_str(&s), Ok(x.clone()), "from_str({s:?})");
        assert_eq!(
            FeatureId::try_from(s.as_str()),
            Ok(x.clone()),
            "try_from({s:?})"
        );
        assert_eq!(s.parse::<FeatureId>(), Ok(x), "parse({s:?})");
    }
}

/// B4 (premise I3): the pinned malformed-string set is rejected by both
/// `FromStr` and `TryFrom<&str>`. Mirrors the exact set pinned in-crate by
/// `feature_id_from_str_rejects_malformed` (`reify-ir/src/geometry.rs`),
/// which additionally pins the specific `FeatureIdParseError` variants.
#[test]
fn b4_malformed_strings_rejected() {
    for bad in [
        "",                       // empty
        "Foo",                    // root not realization-shaped
        "Foo#realization[]",      // empty index
        "Foo/mid_surface",        // root not realization-shaped
        "x#realization[0]/bogus", // unknown derived step
    ] {
        assert!(FeatureId::from_str(bad).is_err(), "expected Err for {bad:?}");
        assert!(FeatureId::try_from(bad).is_err(), "expected Err for {bad:?}");
    }
}

/// B5: structural content-hash identity, asserted at the PUBLIC
/// `Value::content_hash()` seam (tag 31) rather than via the `pub(crate)`
/// `content_hash_bytes` (owned by `feature_id_content_hash_bytes_golden`).
/// Covers equal-structure ⇒ equal hash, determinism, differ-by-entity/
/// index/wrapping ⇒ different hash, and — the key structural-identity
/// guarantee — that the hash is NOT simply the `Display` string's hash.
#[test]
fn b5_content_hash_is_structural_not_display() {
    let a = Value::Feature(FeatureId::realization("Foo", 3));
    let b = Value::Feature(FeatureId::realization("Foo", 3));
    assert_eq!(
        a.content_hash(),
        b.content_hash(),
        "equal structure must hash equal"
    );
    assert_eq!(
        a.content_hash(),
        a.content_hash(),
        "content_hash must be deterministic across calls"
    );

    let different_entity = Value::Feature(FeatureId::realization("Bar", 3));
    let different_index = Value::Feature(FeatureId::realization("Foo", 4));
    let derived = Value::Feature(FeatureId::derived_mid_surface(&FeatureId::realization(
        "Foo", 3,
    )));
    assert_ne!(a.content_hash(), different_entity.content_hash());
    assert_ne!(a.content_hash(), different_index.content_hash());
    assert_ne!(a.content_hash(), derived.content_hash());

    // Structural identity, NOT the Display string: hashing the FeatureId's
    // rendered string as a plain Value::String must NOT collide with
    // hashing the structured Value::Feature.
    let fid = FeatureId::realization("Foo", 3);
    let as_feature = Value::Feature(fid.clone());
    let as_string = Value::String(fid.to_string());
    assert_ne!(
        as_feature.content_hash(),
        as_string.content_hash(),
        "Value::Feature must hash its structure, not its Display string"
    );
}

/// B9 (value-model half): `FeatureId` is usable as a `HashMap` key —
/// mirroring its real production use in
/// `reify_eval::topology_attribute_propagation` and `engine_build`'s
/// per-feature indices — and `Value::Feature` is usable as a `BTreeMap`
/// key, mirroring `Value::Map`'s real `BTreeMap<Value, Value>` backing
/// store. `Value` as a whole has no `Hash` impl (by design: `Value::Map`
/// is a `BTreeMap`, not a `HashMap`), so the `HashMap` assertion
/// deliberately binds the bare `FeatureId`, not `Value::Feature`.
#[test]
fn b9_feature_id_and_value_feature_usable_as_map_keys() {
    let fid_a = FeatureId::realization("Foo", 3);
    let fid_b = FeatureId::derived_mid_surface(&fid_a);

    let mut by_hashmap: HashMap<FeatureId, &str> = HashMap::new();
    by_hashmap.insert(fid_a.clone(), "root");
    by_hashmap.insert(fid_b.clone(), "derived");
    assert_eq!(by_hashmap.get(&fid_a), Some(&"root"));
    assert_eq!(by_hashmap.get(&fid_b), Some(&"derived"));

    let mut by_btreemap: BTreeMap<Value, &str> = BTreeMap::new();
    by_btreemap.insert(Value::Feature(fid_a.clone()), "root");
    by_btreemap.insert(Value::Feature(fid_b.clone()), "derived");
    assert_eq!(
        by_btreemap.get(&Value::Feature(fid_a.clone())),
        Some(&"root")
    );
    assert_eq!(by_btreemap.get(&Value::Feature(fid_b)), Some(&"derived"));
}

/// B9 (value-model half, cont'd): `Value::Feature.try_infer_type()` maps to
/// `Some(Type::Feature)`, and `Type::Feature`'s `Display` is `"Feature"`.
#[test]
fn b9_try_infer_type_and_type_feature_display() {
    let v = Value::Feature(FeatureId::realization("Foo", 3));
    assert_eq!(v.try_infer_type(), Some(Type::Feature));
    assert_eq!(format!("{}", Type::Feature), "Feature");
}
