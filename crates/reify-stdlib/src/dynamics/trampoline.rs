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

#[cfg(test)]
mod tests {
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::collections::BTreeMap;

    use super::InverseDynamicsCacheKey;

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
}
