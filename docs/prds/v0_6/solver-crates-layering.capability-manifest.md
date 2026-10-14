# Capability manifest — solver-crates-layering

PRD: `docs/prds/v0_6/solver-crates-layering.md`. The machine-readable twin is
`docs/prds/v0_6/solver-crates-layering.capability-manifest.yaml`. Its delivered checks are copied
into producer metadata by `commit_planning`.

**Code anchors** were verified against main `8e6ffcac9d` (2026-10-12). Every mechanical check
below was run against `main` at authoring time and has sound polarity: each `present` check has no
match today and each `absent` check matches today.

Bindings use these evidence forms:

- `grep:` — read at main;
- `producer:` — a task or leaf that delivers the capability;
- `G6` — a numeric premise.

Two verdicts are **OPEN**. In each case the binding is the leaf's own work product, so neither
blocks queueing.

| leaf | capability | evidence | verdict | delivered check |
|---|---|---|---|---|
| α | ratchet-host shape exists | grep:`crates/reify-eval/tests/version_id_discipline_gate.rs` (matcher + seeded self-tests + real-tree walk) | PASS | path present `crates/reify-solver-elastic/tests/adoption_ratchets.rs` |
| α | baselines reproduce | re-measured at `8e6ffcac9d`: sign rule 6, anchor partition 3, upward imports 8, `det_bb` 2, `5.0 / 6.0` 2, G 16, `td[i][j] =` 5, `MpcRow {` literals 2, `.data[` 72 | PASS | manual |
| β | `elements/shell_frame.rs` | producer: this leaf; `elements/mod.rs` exists | PASS | path present |
| β | primitives leave shell_assembly | grep: `pub struct ShellFrame` in `shell_assembly.rs` | PASS | grep absent |
| β | no upward imports from lower layers | grep: `use crate::shell_assembly::` in `degenerate_shell`, `membrane_cst`, `geometric_stiffness/membrane`, `shell_kinematics` | PASS | grep absent |
| γ | `ShellSection` | no section type at main | PASS | grep present `struct ShellSection` |
| γ | `IsotropicElastic::shear_modulus` | the method is absent; 16 inline formulas | PASS | grep present in `constitutive.rs` |
| γ | second KAPPA gone | grep: fn-local `const KAPPA` in `shell_result.rs` | PASS | grep absent |
| γ | scaled blocks gone | grep: `td[i][j] = ` has 5 production sites and 0 in tests | PASS | grep absent |
| γ | bool flags gone | grep: `use_ans_membrane: bool` | PASS | grep absent |
| γ | guard numeric premise | G6: the degenerate copy's guard already ships; current MITC3+ fixtures are well-conditioned; BT6 covers the singular case | PASS | manual |
| δ | `AssemblyElement` fields private | grep: `pub connectivity: &'a [usize]`. After #8373 the `dofs_per_node()` derivation is single, but the checks are still asymmetric. | PASS | grep absent |
| δ | callers moved first | producer: #8375 (upstream edge) | PASS | manual |
| ε | `ElementStiffness` storage private | grep: `pub data: Vec<f64>`; 72 `.data[` sites | PASS | grep absent |
| ε | no external readers | measured: none outside reify-solver-elastic | PASS | manual |
| ζ | copied helpers gone | grep: `fn member_force_delta` in `tensegrity_load.rs` | PASS | grep absent |
| ζ | thin call | `membrane_load_analysis(` is absent from `tensegrity_load.rs` | PASS | grep present |
| ζ | types alias cleanly | read: the Options and Error types are identical; the Solve types differ by `member_slack` and the surface fields | PASS | manual |
| ζ | empty-patch equivalence | the finding asserts it; the skeptic verified only the shared lines and the fork | **OPEN** | manual |
| η | `ForceDensityProblem` | no validated type; 3 unchecked `is_anchor[a] = true` | PASS | grep present |
| η | `MemberKind::admits` | no `impl MemberKind`; 6 sign copies | PASS | grep present `fn admits(` |
| η | `map_ff_error` gone | grep: present in `form_find.rs` | PASS | grep absent |
| η | user surface byte-stable | the trampoline's `describe()`; `tests/golden/tensegrity_t_prism.txt`; `cli_tensegrity_e2e.rs` | PASS | manual |
| θ | `MpcRow` fields private | grep: `pub dofs: Vec<usize>` | PASS | grep absent |
| θ | doctest literal gone | grep: `MpcRow { dofs` in the `lib.rs` crate doc | PASS | grep absent |
| ι | `tests/common/fea_harness.rs` | `tests/common/mod.rs` exists (92 lines) | PASS | path present |
| ι | per-file `dedup_bcs` gone | grep: 5 top-level definitions | PASS | grep absent (5 files) |
| ι | determinism rebuilt first | producer: #8375 | PASS | manual |
| ι | conflicting-BC fixtures | unmeasured; the leaf runs the suites and fixes any real fixture defect | **OPEN** | manual |
| κ | `GeometryError::BackendUnavailable` | the variant is absent; no exhaustive match outside Display | PASS | grep present |
| κ | substring parsers gone | grep: 2 `msg.contains(…STUB_UNAVAILABLE_MARKER)` | PASS | grep absent |
| κ | stub arm compilable | `build.rs` omits `has_gmsh` when `GMSH_LIB_DIR` is empty | PASS | manual |
| λ | `TetSpatialIndex<'a>` | no lifetime today; the only users are in `resample.rs` | PASS | grep present |
| μ | `tests/qoi.rs` | the record's h14 partition: tests use only crate-root re-exports | PASS | path present |
| μ | inline module gone | grep: `#[cfg(test)]` appears once in `qoi.rs` | PASS | grep absent |
| ν | slack predicate exported | `reify-eval/Cargo.toml` has reify-constraints as a normal dependency (task 4386) | PASS | grep present `pub fn has_inequality_slack` |
| ν | eval mirrors gone | grep: top-level `fn has_inequality_slack` and `fn objective_is_money` in `engine_eval.rs` | PASS | grep absent |
| ν | signed-contribution primitive | the fold is written 4× (solver, registry, cpsat, engine_eval) | PASS | grep present |
| ν | constraints ratchet host | producer: this leaf | PASS | path present |
| ξ | `crate::trial` | #8581's measurement: imports only `reify_expr` / `reify_ir` | PASS | path present `src/trial.rs` |
| ξ | `solver/derive` | `effective_bounds` needs only `dimension_of` | PASS | path present |
| ξ | in-flight edits first | producers: #6655, #7492, #7420 | PASS | manual |
| ο | `solver/core` | read: floor_infeasible_diagnostic → constraints_witness → core → floor_infeasible_diagnostic (D17) | PASS | path present |
| ο | core renamed | grep: `fn solve_core_with_sd_tolerance`; all 5 callers pass `NM_SD_TOLERANCE` | PASS | grep absent |
| π | inline test module gone | grep: `mod tests {` in `solver.rs` (7,046 lines) | PASS | grep absent |
| π | uniqueness layer | the record's partition: downward-only | PASS | path present `solver/uniqueness.rs` |
| ρ | terminal status | the overlay's freeze shape | PASS | manual |

The adopted task #8528 (ShellStress, fk-824124d1e74e) is not a batch block: it was filed by the
run and carries no manifest block of its own.
