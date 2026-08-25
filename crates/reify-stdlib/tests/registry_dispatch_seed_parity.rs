//! The eval seam of the builtin-signature registry (task #6001 α,
//! `docs/prds/v0_6/builtin-signature-registry.md` §7.3(3), invariant I-REG-2).
//!
//! Two layers, deliberately separated — the same split
//! `reify-compiler`'s `tests/harness_builtin_registry/registry_seed_result_types.rs`
//! uses on the compiler side:
//!
//! **(a) The registry path itself.** Every probe below routes through
//! `reify_stdlib::__registry_dispatch_for_test`, the `test-support`-gated shim
//! over `registry_dispatch::dispatch(EvalBuiltinId, &[Value])`. Taking an
//! `EvalBuiltinId` — not a `&str` — is what makes this test observe that eval
//! dispatch is genuinely KEYED ON THE REGISTRY, rather than merely observe
//! that `eval_builtin` still works (which it would even if the string matchers
//! survived untouched). This layer is the RED signal: neither the shim nor the
//! `registry_dispatch` module exists yet, so the binary does not compile.
//!
//! **(b) Observational inertness.** The same probes re-run through the public
//! `reify_stdlib::eval_builtin(name, args)` must return the IDENTICAL `Value`,
//! so hoisting the registry to the front of the 26-arm dispatch chain changes
//! which layer resolves the name but nothing a `.ri` author can observe.
//!
//! # Why the expected values are spelled out rather than captured
//!
//! A parity test that compares `dispatch(id, args)` against
//! `eval_builtin(name, args)` and nothing else is vacuous once both route
//! through the same code. So each probe carries a HAND-WRITTEN expected
//! `Value` reproducing what the pre-registry string path returned. Where the
//! arithmetic is exactly representable in `f64` the expectation is a literal
//! (`parse_length("12mm")` → `12.0 * 0.001`; von Mises of a uniaxial 100 MPa
//! window → `√(0.5·2·(10⁸)²)` = exactly `1e8`, since `1e16` and `1e8` are both
//! exact doubles; `safety_factor` → `250e6 / 1e8` = exactly `2.5`). Where it is
//! NOT — `max_shear` and `principal_stresses` both run the iterative
//! `compute_eigenvalues_3x3` — the expectation is built from the crate's own
//! PUBLIC kernel (`reify_stdlib::compute_max_shear_3x3` /
//! `compute_eigenvalues_3x3`), which is honest about what this test pins:
//! α does not touch a kernel body, only the key dispatch is routed on, so the
//! assertion that earns its keep is "dispatch reaches THE SAME kernel and
//! wraps its result identically". No tolerance is used anywhere — every
//! comparison is `Value`'s bit-exact `PartialEq`.

use reify_builtins::{BindingKind, BuiltinId, EvalBuiltinId, lookup, rows};
use reify_core::DimensionVector;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_stdlib::__registry_dispatch_for_test as dispatch;
use reify_stdlib::{compute_eigenvalues_3x3, compute_max_shear_3x3, eval_builtin};

/// The `StructureTypeId` sentinel `analysis::stress_invariants` mints for
/// registry-free instances (`crates/reify-stdlib/src/analysis.rs:20`).
const REGISTRY_FREE_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

/// Uniaxial 100 MPa stress magnitude. Chosen because `1e8` and `(1e8)² = 1e16`
/// are both exact doubles, so the von Mises and safety-factor expectations
/// below need no tolerance.
const SIGMA: f64 = 100e6;

/// Row-major 3×3 window of the uniaxial tensor, as the kernels see it.
const UNIAXIAL_WINDOW: [f64; 9] = [SIGMA, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

/// Build a 3×3 `Value::Tensor` whose elements all carry `dim`.
fn dimensioned_matrix(rows_f64: &[[f64; 3]; 3], dim: DimensionVector) -> Value {
    Value::Tensor(
        rows_f64
            .iter()
            .map(|row| {
                Value::Tensor(
                    row.iter()
                        .map(|&v| Value::Scalar {
                            si_value: v,
                            dimension: dim,
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The uniaxial 100 MPa stress tensor every analysis probe is fed.
fn uniaxial_stress() -> Value {
    dimensioned_matrix(
        &[[SIGMA, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
        DimensionVector::PRESSURE,
    )
}

fn pressure(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::PRESSURE,
    }
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

/// One (id, name, args, expected) probe: the same call expressed as an
/// `EvalBuiltinId` for the registry path and as a `&str` for the public path.
struct Probe {
    id: EvalBuiltinId,
    name: &'static str,
    args: Vec<Value>,
    expected: Value,
    /// Short label naming the disposition under test, for assertion messages.
    what: &'static str,
}

/// Every seed probe: both `parse_*` dispositions, all five analysis kernels.
fn probes() -> Vec<Probe> {
    let stress = uniaxial_stress();

    // `unit_symbol_to_si("mm")` is the exact literal 0.001, and
    // `parse_length_value` computes `num * factor` — so `12.0 * 0.001`
    // reproduces the kernel's f64 bit-for-bit.
    let twelve_mm = Value::Scalar {
        si_value: 12.0 * 0.001,
        dimension: DimensionVector::LENGTH,
    };

    let eigs = compute_eigenvalues_3x3(&UNIAXIAL_WINDOW)
        .expect("uniaxial 3×3 window has real eigenvalues");

    // I1 = trace = σ; I2 = I3 = 0 for a rank-1 diagonal tensor.
    let dim = DimensionVector::PRESSURE;
    let dim2 = dim.mul(&dim);
    let dim3 = dim2.mul(&dim);
    let invariant_fields: PersistentMap<String, Value> = [
        (
            "i1".to_string(),
            Value::Scalar {
                si_value: SIGMA,
                dimension: dim,
            },
        ),
        (
            "i2".to_string(),
            Value::Scalar {
                si_value: 0.0,
                dimension: dim2,
            },
        ),
        (
            "i3".to_string(),
            Value::Scalar {
                si_value: 0.0,
                dimension: dim3,
            },
        ),
    ]
    .into_iter()
    .collect();

    vec![
        Probe {
            id: EvalBuiltinId::ParseLength,
            name: "parse_length",
            args: vec![string("12mm")],
            expected: Value::Option(Some(Box::new(twelve_mm.clone()))),
            what: "parse_length recognises a length",
        },
        Probe {
            id: EvalBuiltinId::ParseLength,
            name: "parse_length",
            args: vec![string("bogus")],
            expected: Value::Option(None),
            what: "parse_length declines malformed input",
        },
        Probe {
            id: EvalBuiltinId::ParseLengthR,
            name: "parse_length_r",
            args: vec![string("12mm")],
            expected: Value::Enum {
                type_name: "Result".to_string(),
                variant: "Ok".to_string(),
                payload: vec![("value".to_string(), twelve_mm)],
            },
            what: "parse_length_r Ok disposition",
        },
        Probe {
            id: EvalBuiltinId::ParseLengthR,
            name: "parse_length_r",
            args: vec![string("bogus")],
            expected: Value::Enum {
                type_name: "Result".to_string(),
                variant: "Err".to_string(),
                payload: vec![(
                    "error".to_string(),
                    string("could not parse 'bogus' as a length"),
                )],
            },
            what: "parse_length_r Err disposition",
        },
        Probe {
            id: EvalBuiltinId::VonMises,
            name: "von_mises",
            args: vec![stress.clone()],
            // √(0.5·((σ−0)² + 0 + (0−σ)²)) = √(σ²) = σ, exactly.
            expected: pressure(SIGMA),
            what: "von_mises of a uniaxial window",
        },
        Probe {
            id: EvalBuiltinId::MaxShear,
            name: "max_shear",
            args: vec![stress.clone()],
            expected: pressure(compute_max_shear_3x3(&UNIAXIAL_WINDOW)),
            what: "max_shear of a uniaxial window",
        },
        Probe {
            id: EvalBuiltinId::PrincipalStresses,
            name: "principal_stresses",
            args: vec![stress.clone()],
            expected: Value::List(eigs.iter().map(|&e| pressure(e)).collect()),
            what: "principal_stresses of a uniaxial window",
        },
        Probe {
            id: EvalBuiltinId::SafetyFactor,
            name: "safety_factor",
            args: vec![stress.clone(), pressure(250e6)],
            // yield / von_mises = 250e6 / 1e8 = 2.5, exactly; dimensionless,
            // so `from_real_scalar` yields a bare `Value::Real`.
            expected: Value::Real(2.5),
            what: "safety_factor of a uniaxial window",
        },
        Probe {
            id: EvalBuiltinId::StressInvariants,
            name: "stress_invariants",
            args: vec![stress],
            expected: Value::StructureInstance(Box::new(StructureInstanceData {
                type_id: REGISTRY_FREE_TYPE_ID,
                type_name: "StressInvariants".to_string(),
                version: 1,
                fields: invariant_fields,
            })),
            what: "stress_invariants of a uniaxial window",
        },
    ]
}

/// Every `BindingKind::EvalBuiltin` row, derived from the registry rather than
/// restated — mirroring `units.rs`'s disjointness list, so a τ row added later
/// is covered here automatically instead of silently escaping the sweep.
fn eval_rows() -> Vec<&'static reify_builtins::BuiltinRow<BuiltinId>> {
    rows()
        .iter()
        .filter(|r| r.binding == BindingKind::EvalBuiltin)
        .collect()
}

// ── (a) value-for-value parity through the registry path ────────────────────

#[test]
fn registry_dispatch_reproduces_the_legacy_string_path_values() {
    for probe in probes() {
        assert_eq!(
            dispatch(probe.id, &probe.args),
            probe.expected,
            "registry dispatch changed the value of {}",
            probe.what
        );
    }
}

// ── (b) row-derived entry-point agreement ───────────────────────────────────

#[test]
fn every_eval_row_resolves_exactly_at_its_declared_arity() {
    for row in eval_rows() {
        for argc in 0..=3usize {
            let resolved = lookup(row.name, argc).and_then(BuiltinId::as_eval_builtin);
            if row.arity.matches(argc) {
                assert!(
                    resolved.is_some(),
                    "row '{}' declares {:?} but lookup(name, {argc}) did not \
                     resolve to an EvalBuiltinId",
                    row.name,
                    row.arity
                );
            } else {
                assert!(
                    resolved.is_none(),
                    "row '{}' declares {:?} yet lookup(name, {argc}) resolved \
                     to {resolved:?} — a non-declared arity must not resolve",
                    row.name,
                    row.arity
                );
            }
        }
    }
}

#[test]
fn every_eval_row_is_reachable_from_a_probe() {
    // Guards the probe table against silently going stale: a seed row with no
    // probe would let a dispatch regression through unobserved.
    let probed: Vec<EvalBuiltinId> = probes().into_iter().map(|p| p.id).collect();
    for row in eval_rows() {
        let id = row
            .id
            .as_eval_builtin()
            .expect("an EvalBuiltin-bound row narrows to EvalBuiltinId");
        assert!(
            probed.contains(&id),
            "seed row '{}' ({id:?}) has no probe in this test",
            row.name
        );
    }
}

// ── (c) public-path parity: the hoist is observationally inert ──────────────

#[test]
fn eval_builtin_returns_the_same_value_as_the_registry_path() {
    for probe in probes() {
        let via_public = eval_builtin(probe.name, &probe.args);
        assert_eq!(
            via_public, probe.expected,
            "eval_builtin('{}') changed the value of {}",
            probe.name, probe.what
        );
        assert_eq!(
            via_public,
            dispatch(probe.id, &probe.args),
            "eval_builtin('{}') and registry dispatch disagree for {}",
            probe.name,
            probe.what
        );
    }
}

// ── (d) no shadowing: a non-declared arity still yields Undef ───────────────

#[test]
fn eval_builtin_yields_undef_at_a_non_declared_arity() {
    // Routing arity through `lookup` instead of through `helpers::unary` /
    // `binary`'s argc guard moves WHICH layer declines, not WHAT is observed:
    // an unresolved name falls through to `eval_builtin`'s terminal
    // `Value::Undef`, exactly what the old `None` fall-through produced. This
    // also pins that no OTHER member of the 26-arm chain claims a seed name —
    // if one did, it would answer here with something other than `Undef`.
    let filler = Value::Real(1.0);
    for row in eval_rows() {
        for argc in 0..=3usize {
            if row.arity.matches(argc) {
                continue;
            }
            let args = vec![filler.clone(); argc];
            let result = eval_builtin(row.name, &args);
            assert!(
                result.is_undef(),
                "eval_builtin('{}') with {argc} arg(s) must be Undef (the row \
                 declares {:?}), got {result:?}",
                row.name,
                row.arity
            );
        }
    }
}
