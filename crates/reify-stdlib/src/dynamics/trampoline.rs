//! Pure, dependency-light cache-key half of the `inverse_dynamics` ComputeNode
//! trampoline (task RBD-ι; `docs/prds/v0_3/rigid-body-dynamics.md` §6/§7.7,
//! `docs/prds/v0_3/compute-node-contract.md` §4 GR-002).
//!
//! Mirrors the modal `free_vibration` split (`modal/trampoline.rs`): the
//! `Value`-holding warm-state cache (`InverseDynamicsCache`) and the engine-
//! facing `ComputeFn` live in `reify-eval` (`dynamics_ops.rs`), because
//! `ComputeOutcome` / `CancellationHandle` / `RealizationReadHandle` /
//! `OpaqueState` are `reify-eval` (resp. `reify-ir`) types and the dependency
//! graph `reify-eval → reify-expr → reify-stdlib` forbids `reify-stdlib` from
//! depending on `reify-eval`. This module holds only the pure
//! [`InverseDynamicsCacheKey`] that cache is keyed on plus the
//! [`body_solid_hashes`] invalidation record — both expressed over
//! `reify-ir::Value` + `reify-core::ContentHash`, no new deps.
//!
//! The key captures EXACTLY the inputs that determine the trajectory-level
//! `List<List<JointForce>>` result: the mechanism content hash (geometry,
//! topology, and the per-body `solid` mass sources), the trajectory content
//! hash (every sample's `q` / `q̇` / `q̈`), and the gravity vector hash. A full
//! key match certifies the cached result for reuse (a cache HIT). The
//! companion [`body_solid_hashes`] records each body's `solid` content hash so
//! the warm state can observe "the MassProperties only changed when a body
//! solid changed" at body granularity.

use reify_core::ContentHash;
use reify_ir::Value;

/// The result-determining inputs of a trajectory-level `inverse_dynamics`
/// solve, used to decide whether a cached `InverseDynamicsCache`
/// (`reify-eval`'s `dynamics_ops`) can be reused for a new call.
///
/// Three [`ContentHash`]es:
/// - `mech_hash` — the mechanism `Value` (`Value::content_hash`), which folds
///   in geometry, topology, and every body's `solid` mass source.
/// - `traj_hash` — the trajectory `Value`, which folds in every sample's
///   `q` / `q̇` / `q̈`.
/// - `gravity_hash` — the SI gravity vector (`combine_all` of `of_u64` over each
///   component's [`f64::to_bits`]), so a future per-mechanism gravity override
///   invalidates the cache without a key-shape change.
///
/// A full per-field match certifies the cached `List<List<JointForce>>` for
/// reuse (a cache HIT). Compared via [`matches`](InverseDynamicsCacheKey::matches)
/// — per-field `ContentHash` equality. `Copy`/`Debug` but deliberately NOT
/// `PartialEq` (the single comparison path is `matches`, mirroring
/// `ModalCacheKey`); the underlying `Value::content_hash` canonicalizes `NaN`
/// and preserves `-0.0`, so comparison is collision-free and deterministic.
#[derive(Clone, Copy, Debug)]
pub struct InverseDynamicsCacheKey {
    /// Content hash of the mechanism `Value` (`mech.content_hash()`).
    pub mech_hash: ContentHash,
    /// Content hash of the trajectory `Value` (`traj.content_hash()`).
    pub traj_hash: ContentHash,
    /// Content hash of the SI gravity vector (bitwise, per component).
    pub gravity_hash: ContentHash,
}

impl InverseDynamicsCacheKey {
    /// Build a key from the trajectory-solve inputs: the mechanism and
    /// trajectory `Value`s (hashed via [`Value::content_hash`]) and the SI
    /// gravity vector (each component hashed bitwise via [`ContentHash::of_u64`]
    /// over [`f64::to_bits`], combined order-dependently with
    /// [`ContentHash::combine_all`]).
    pub fn from_inputs(mech: &Value, traj: &Value, gravity: [f64; 3]) -> Self {
        let gravity_hash =
            ContentHash::combine_all(gravity.iter().map(|g| ContentHash::of_u64(g.to_bits())));
        Self {
            mech_hash: mech.content_hash(),
            traj_hash: traj.content_hash(),
            gravity_hash,
        }
    }

    /// `true` iff every field hash equals `other`'s — i.e. a cached result
    /// built for `other` may be reused for `self` (a cache HIT). Per-field
    /// `ContentHash` equality is symmetric and collision-free.
    pub fn matches(&self, other: &InverseDynamicsCacheKey) -> bool {
        self.mech_hash == other.mech_hash
            && self.traj_hash == other.traj_hash
            && self.gravity_hash == other.gravity_hash
    }
}

#[cfg(test)]
mod tests {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::collections::BTreeMap;

    use super::{body_solid_hashes, InverseDynamicsCacheKey};

    /// Default gravity used across the cache-key fixtures: SI `[0, 0, −9.81]`
    /// (matches `reify_stdlib::dynamics::rnea::default_gravity()`).
    const G: [f64; 3] = [0.0, 0.0, -9.81];

    /// A registry-free `Value::StructureInstance` with `type_name` + fields,
    /// mirroring the eval-side `mint_instance` shape.
    fn instance(type_name: &str, fields: Vec<(String, Value)>) -> Value {
        let fields: PersistentMap<String, Value> = fields.into_iter().collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: type_name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// A `solid` value (stand-in `MassProperties`) carrying a single mass cell,
    /// so distinct masses produce distinct content hashes.
    fn solid(mass: f64) -> Value {
        instance("MassProperties", vec![("mass".to_string(), Value::Real(mass))])
    }

    /// A minimal mechanism `Value::Map` (`kind="mechanism"`) whose `bodies` list
    /// holds one body `Map` per supplied solid mass (in order). Matches the
    /// eval-side mechanism shape the trampoline reads (`body.solid` source).
    fn mechanism(masses: &[f64]) -> Value {
        let bodies: Vec<Value> = masses
            .iter()
            .enumerate()
            .map(|(i, &m)| {
                let body: BTreeMap<Value, Value> = [
                    (Value::String("id".to_string()), Value::Int(i as i64)),
                    (Value::String("solid".to_string()), solid(m)),
                ]
                .into_iter()
                .collect();
                Value::Map(body)
            })
            .collect();
        let mech: BTreeMap<Value, Value> = [
            (
                Value::String("kind".to_string()),
                Value::String("mechanism".to_string()),
            ),
            (Value::String("bodies".to_string()), Value::List(bodies)),
        ]
        .into_iter()
        .collect();
        Value::Map(mech)
    }

    /// A minimal `MotionTrajectory` instance with one motionless
    /// `TrajectorySample` per supplied joint position `q` (vels/accels zeroed).
    fn trajectory(qs: &[f64]) -> Value {
        let samples: Vec<Value> = qs
            .iter()
            .map(|&q| {
                instance(
                    "TrajectorySample",
                    vec![
                        ("t".to_string(), Value::Real(0.0)),
                        ("values".to_string(), Value::List(vec![Value::Real(q)])),
                        ("vels".to_string(), Value::List(vec![Value::Real(0.0)])),
                        ("accels".to_string(), Value::List(vec![Value::Real(0.0)])),
                    ],
                )
            })
            .collect();
        instance(
            "MotionTrajectory",
            vec![
                ("mechanism".to_string(), Value::Real(0.0)),
                ("samples".to_string(), Value::List(samples)),
            ],
        )
    }

    // ── step-1: InverseDynamicsCacheKey::from_inputs / matches ──────────────────

    /// (a) Two keys built from identical `(mech, traj, gravity)` match — the
    /// cache-HIT condition.
    #[test]
    fn cache_key_matches_identical_inputs() {
        let mech = mechanism(&[1.0]);
        let traj = trajectory(&[-std::f64::consts::FRAC_PI_6]); // −30°
        let a = InverseDynamicsCacheKey::from_inputs(&mech, &traj, G);
        let b = InverseDynamicsCacheKey::from_inputs(&mech, &traj, G);
        assert!(a.matches(&b), "identical (mech, traj, gravity) must match");
    }

    /// (b) A different mechanism `Value` (here a body's solid mass) must NOT
    /// match — the cached result was computed for different inertial inputs.
    #[test]
    fn cache_key_differs_on_mechanism() {
        let traj = trajectory(&[-std::f64::consts::FRAC_PI_6]);
        let a = InverseDynamicsCacheKey::from_inputs(&mechanism(&[1.0]), &traj, G);
        let b = InverseDynamicsCacheKey::from_inputs(&mechanism(&[2.0]), &traj, G);
        assert!(!a.matches(&b), "a different mechanism must MISS");
        assert!(!b.matches(&a), "matches() must be symmetric");
    }

    /// (c) A different trajectory `Value` (here a sample's joint position) must
    /// NOT match.
    #[test]
    fn cache_key_differs_on_trajectory() {
        let mech = mechanism(&[1.0]);
        let a = InverseDynamicsCacheKey::from_inputs(&mech, &trajectory(&[-0.5]), G);
        let b = InverseDynamicsCacheKey::from_inputs(&mech, &trajectory(&[0.5]), G);
        assert!(!a.matches(&b), "a different trajectory must MISS");
    }

    /// (d) A change to ANY gravity component must NOT match — gravity is folded
    /// into the key so a future override invalidates correctly.
    #[test]
    fn cache_key_differs_on_each_gravity_component() {
        let mech = mechanism(&[1.0]);
        let traj = trajectory(&[-0.5]);
        let base = InverseDynamicsCacheKey::from_inputs(&mech, &traj, G);
        for axis in 0..3 {
            let mut g2 = G;
            g2[axis] += 1e-6;
            let other = InverseDynamicsCacheKey::from_inputs(&mech, &traj, g2);
            assert!(
                !base.matches(&other),
                "a change to gravity component {axis} must MISS"
            );
        }
    }

    // ── step-3: body_solid_hashes ───────────────────────────────────────────────

    /// (a) One `ContentHash` per body, in `bodies` order, each equal to that
    /// body's `solid.content_hash()`.
    #[test]
    fn body_solid_hashes_one_per_body_in_order() {
        let masses = [1.0, 2.0, 3.0];
        let mech = mechanism(&masses);
        let hashes = body_solid_hashes(&mech);
        assert_eq!(hashes.len(), masses.len(), "one hash per body");
        for (i, &m) in masses.iter().enumerate() {
            assert_eq!(
                hashes[i],
                solid(m).content_hash(),
                "hash[{i}] must equal body[{i}].solid.content_hash()"
            );
        }
    }

    /// (b) Changing one body's solid `Value` changes only that entry.
    #[test]
    fn body_solid_hashes_changes_only_the_touched_body() {
        let base = body_solid_hashes(&mechanism(&[1.0, 2.0, 3.0]));
        let changed = body_solid_hashes(&mechanism(&[1.0, 2.5, 3.0]));
        assert_eq!(base.len(), 3);
        assert_eq!(changed.len(), 3);
        assert_eq!(base[0], changed[0], "body 0 solid unchanged → same hash");
        assert_ne!(base[1], changed[1], "body 1 solid changed → different hash");
        assert_eq!(base[2], changed[2], "body 2 solid unchanged → same hash");
    }

    /// (c) A non-mechanism `Value` yields an empty `Vec` (a bare scalar and a
    /// non-mechanism `StructureInstance` are both rejected).
    #[test]
    fn body_solid_hashes_empty_for_non_mechanism() {
        assert!(
            body_solid_hashes(&Value::Real(1.0)).is_empty(),
            "a bare scalar is not a mechanism"
        );
        assert!(
            body_solid_hashes(&instance("MotionTrajectory", vec![])).is_empty(),
            "a non-mechanism StructureInstance is not a mechanism Map"
        );
    }
}
