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
