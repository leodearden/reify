# Capability manifest — tensegrity-membrane.md

Mechanizes G3 + G6 per leaf (and per capability-asserting intermediate). Built at decompose time 2026-06-08; binds every asserted symbol / field / fixture / numeric bound to evidence so a dispatch-time architect diffs intent against substrate instead of re-deriving the check. **No binding resolves to a FAIL value** (`declared-only | test-only | producer-absent | producer-downstream | fixture-ERROR | bound≤floor`) → batch clears the gate.

Reify evidence forms (overlay): empty-value sentinel = `Value::Undef`; production entry paths = reify-eval dispatch tables + `engine_eval.rs`/`engine_build.rs`, `compute_targets/*.rs`, the GUI `MeshData` path; grammar gate = `tree-sitter parse --quiet` 0-ERROR; numeric floors = the G6 domain hazards.

Grammar-gate fixture (covers α + γ + δ syntax): `/tmp/prd-gate-fixtures/tensegrity-membrane-1.ri` — `tree-sitter parse --quiet` **EXIT 0, 0 ERROR nodes** (`Membrane` struct def, `Tensegrity(... surfaces: [[Int,Int,Int],…])` ctor, `tensegrity_surfaces(p)` call, `form.surface_stresses` access). `Pressure` dimension + `Pa` literal confirmed: `units.ri:51` (si_units generates Pa/kPa/MPa/GPa), `materials_mechanical.ri:76` (`youngs_modulus : Pressure`).

---

## α — `surfaces` group + `Membrane` structure + surface geometry emission

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `param surfaces : List<List<Int>>` on `Tensegrity` | grammar-fixture | fixture parses 0-ERROR; identical parse-shape to landed `struts`/`cables` (`stdlib/tensegrity.ri:76-77`) | PASS |
| `structure def Membrane { thickness; material; prestress: Pressure = 0Pa }` | grammar + SIR | fixture parses; `Pressure`/`Pa` confirmed; SIR ctor path landed (GR-001), same as landed `Strut`/`Cable` (`tensegrity.ri:39,55`) | PASS |
| `tensegrity_surfaces(t)` builtin | capability→producer (anti-orphan) | THIS task; wired into eval dispatch exactly as the landed `tensegrity_wires` builtin (the documented sibling seam, `tensegrity.ri:19-22`) | PASS (deliverable) |
| `TensegritySurface` facet record populated | field-population | THIS task's builtin constructs real facet records (3 indices + coords + `kind:"membrane"`), mirroring the landed `TensegrityWire` (`tensegrity.ri:100`) — non-`Undef` | PASS (deliverable) |

## β *(leaf)* — GUI membrane surface styling

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| viewport renders membrane surface keyed on `kind:"membrane"` | capability→producer (DAG-direction) | tag emitted by α (**upstream**); signal = screenshot diff via debug MCP (overlay viewport signal type) | PASS (producer-upstream) |

## γ — anchored isotropic NFDM form-find (extend `solver::form_find`)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `solver::form_find` ComputeNode target | capability→producer (anti-orphan) | wired on main: `crates/reify-eval/src/compute_targets/form_find.rs` registered (landed T1a/3794); THIS task extends it with surface assembly | PASS |
| `FormFindResult.surface_stresses` populated | field-population | THIS task's trampoline writes real per-triangle σ (non-`Undef`) on the production path, mirroring the landed `member_forces` population | PASS (deliverable) |
| equilibrium residual ~1e-9 | numeric floor | `D x = P` is a **linear** solve → machine-precision is the honest floor; asserting 1e-9 for a linear solve is at-identity, not below-floor | PASS |
| recovered shape vs analytic catenoid | numeric floor (anti-floor) | cotangent-Laplacian discretization floor = **O(h²)**; signal asserts a **mesh-convergence bound** (`bound > O(h²)` at example mesh), explicitly **NOT** an exact shape frozen into a RED test. Hypar-exactness intuition flagged false (PRD §4). | PASS (bound>floor) |

## δ — combined struts+cables+membrane form-find (extend `solver::form_find_free`)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| combined `D` = line FD + Σ σ_T L_T | capability→producer | THIS task; builds on landed `form_find_free` (3795, `crates/reify-solver-elastic/src/form_find_free.rs`) — NFDM contributions add to the same matrix | PASS |
| signed `member_forces` + `surface_stresses` (struts q<0, cables q>0, σ>0) | field-population | populated signed reals on production path (mirror landed `member_forces`) | PASS (deliverable) |

## ε *(leaf)* — anisotropic warp/weft NFDM extension

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| per-triangle material frame + 2×2 stress assembly | capability→producer | THIS task; depends δ (**upstream**) | PASS (producer-upstream) |
| principal-direction alignment + residual (NOT exact shape) | numeric floor | residual 1e-9 = linear-solve floor; alignment is a qualitative/direction assertion — **deliberately avoids** asserting an exact anisotropic shape (no clean closed form) | PASS |

## ζ — dedicated CST membrane element (`K_e` + membrane `K_g`)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| CST membrane `K_e` strain-displacement core | capability→producer (grep) | `accumulate_membrane_k` **exists**, `crates/reify-solver-elastic/src/shell_assembly.rs:230` (private); THIS task surfaces it as a public 3-DOF element | PASS |
| membrane `K_g(σ)` | capability→producer | THIS task; assembles via the landed GR-024 path, mirroring the bar `K_g` (`geometric_stiffness/bar.rs`) | PASS |
| in-plane patch test 1e-9 | numeric (exactness identity) | a CST **exactly** reproduces constant strain — the existing `shell_membrane_patch_test_…` (`shell_assembly.rs:1323`) already proves 1e-9. Configuration that earns exactness: constant strain field on a CST. | PASS |
| pretensioned-membrane-under-pressure deflection | numeric floor (anti-floor) | CST membrane transverse response floor = **O(h²)**; signal asserts a **mesh-convergence bound** of the `σ t ∇²w=−p` closed form, NOT an exact number | PASS (bound>floor) |

## η — membrane load analysis (form-found pavilion under load)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| deflections + membrane-stress deltas populated | field-population (FEA hot-zone) | THIS task writes real sampleable values on the production path — the signal **explicitly requires non-`Undef`** result fields (guards against the `ElasticResult.{stress,displacement}=Undef` / esc-2962-33 shape) | PASS (deliverable) |
| tension-only active-set reuse for slack patches | capability→producer (DAG-direction) | T3b / **task 3798** (pending, **upstream** — wired dep); membrane reuses its active-set loop, not a new solver | PASS (producer-upstream, gated) |

## θ *(leaf — integration gate, B+H critical)*

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `examples/tensegrity_pavilion.ri` form-finds AND carries load | capability→producer (DAG-direction) | form-find from δ, load from η, viewport from β — **all upstream**; both result fields populated | PASS (producer-upstream) |
| §8 boundary-test table (both-ways) passes in CI | end-to-end | names the PRD §8 sketch as its observable signal (closes G2); CI example = overlay signal type | PASS |
