# Capability manifest — continuous closed-form cost minimisation

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/continuous-cost-minimisation.md`
(committed on main, `ba1bb92551`). Each binding maps a leaf's asserted capability
to **executed** evidence (captured command output), with a PASS / DEFERRED verdict.
Any FAIL binding blocks the batch. Authored at decompose time, 2026-06-24.

**Leaf task IDs:** α `4789` · β `4790` · γ `4791` · δ `4793` · ε `4795`.

**Probe environment:** `target/release/reify` (release binary, built 2026-06-24);
`tree-sitter` (`~/.cargo/bin/tree-sitter`), CWD `tree-sitter-reify/` for grammar
probes. `LD_LIBRARY_PATH` includes the OCCT/`/opt/reify-deps` shared libs.

---

## Scope note — substrate vs. deliverable (why no FAILs)

This PRD reuses a **shipped** pipeline (`minimize` → `MinimizeDecl` →
`ObjectiveTerm`/`ObjectiveSet{WeightedSum}` → `DimensionalSolver` Nelder-Mead
penalty solve) and adds three NEW mechanisms (the Money-triggered robustness
floor, the `cost_robustness_tradeoff` special-form, the tolerance-tied margin).
Decompose-time verification asserts only the **assumed substrate** (grammar +
the shipped solve pipeline + the λ-anchor objectives) — NOT the tasks' own
deliverables, which by definition do not exist yet. The single rejection-shaped
assertion in the PRD (`cost_robustness_tradeoff(<non-money>, 2.0)` → diagnostics)
is **γ's deliverable**, explicitly NOT an assumed-substrate rejection premise
(§4: "its semantics are new work owned by this PRD"; §6 branch 4). It is therefore
verified at γ's leaf signal, never as a decompose-time substrate probe — avoiding
the inverse of the 4575 silent-accept trap (asserting a not-yet-built rejection
fires today would be a category error, not a gate).

## Numeric-floor note (G6)

The headline comparison premise — "resolved value sits ≥ margin off the binding
constraint" — is **not** an accuracy bound against a numeric floor (§6 branch 1).
It is a **structural** property of the synthesised `slack_i ≥ m` constraint,
achievable by construction whenever the floor is feasible (§3 feasibility caveat →
distinct diagnostic, α's invariant iii). No numeric floor (P1-tet bending lock,
Duhamel `O((ΩΔt)²)`, etc.) applies. There is no closed-form-exactness (branch 2)
premise in this PRD.

---

## Grammar evidence (anti-mismatch)

§4 establishes **no novel syntax** — all v1 surfaces use shipped grammar. Two
committed fixtures capture the v1 surfaces; both parse with **0 ERROR nodes**.

| Fixture (committed) | Surface | `tree-sitter parse --quiet` |
|---|---|---|
| `tests/prd-gate/fixtures/cost_min_money_objective.ri` | `minimize <Money·t>` over an auto `Length` param + inequality | **exit 0** (0 ERROR) |
| `tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri` | `minimize cost_robustness_tradeoff(<money-expr>, 0.3)` | **exit 0** (0 ERROR) |

---

## Per-leaf capability bindings

### α `4789` — robustness floor + configurable default margin + floor-infeasibility diagnostic

| Capability asserted | Evidence form | Result |
|---|---|---|
| Money `minimize` over own auto param resolves via the shipped `DimensionalSolver` (the pipeline the floor extends) | **wired-on-main / executed**: `reify eval cost_min_money_objective.ri` → `CostMinPart.thickness = 0.000001 m`, **exit 0** — Money objective drives the auto param, same pipeline as `objective_set_weighted.ri` (calibration). | **PASS** |
| Floor synthesis `slack_i ≥ m` for Money objectives | **anti-orphan**: extends shipped `DimensionalSolver` problem assembly at the catalogued §3.5 ConstraintSolver seam — no orphan `pub fn` in a `kernel-*` crate. Floor is α's deliverable; structural-by-construction (no numeric floor). | **PASS** (deliverable correctly scoped) |
| Floor-infeasibility surfaces a **distinct** diagnostic (not bare "infeasible") | **executed baseline**: a Money objective driving `thickness` *toward* a strict `> 1mm` lower bound resolves `infeasible` today (`solve failed: infeasible`) — the boundary-parking fragility the floor fixes; α adds the distinct diagnostic. | **PASS** (deliverable; baseline captured) |
| Per-purpose tolerance scope exposes a usable per-constraint margin | **investigation (queued)** — α reports the finding; gates δ. Not assumed; configurable default ships regardless (§2.3). | **DEFERRED** (α's own finding) |

### β `4790` — end-to-end Money-objective floor + canonical example

| Capability asserted | Evidence form | Result |
|---|---|---|
| Money objective + inequality resolves the auto param OFF the boundary, info diagnostic present | depends on α's floor (intra-batch dep `4790→4789`); the underlying Money solve pipeline is **shipped** (α evidence above). `examples/continuous_cost_min.ri` is β's deliverable (does not exist yet — correctly not probed). | **PASS** (substrate shipped; off-boundary value is α+β's deliverable) |
| Grammar of the example surface | `cost_min_money_objective.ri` parses (grammar table above). | **PASS** |

### γ `4791` — `cost_robustness_tradeoff(cost_expr, λ)` special-form

| Capability asserted | Evidence form | Result |
|---|---|---|
| `minimize cost_robustness_tradeoff(<money>, λ)` parses as an ordinary call in objective position | **executed grammar**: `cost_robustness_tradeoff_form.ri` → `tree-sitter parse --quiet` **exit 0** (0 ERROR). | **PASS** |
| λ=0 anchor ≡ Chebyshev-centre objective | **wired-on-main**: `build_centrality_objective` shipped at `crates/reify-constraints/src/solver.rs:497`, wired at `:814`. | **PASS** |
| λ=1 anchor ≡ pure-cost penalty solve | **wired-on-main**: the shipped `minimize`→`DimensionalSolver` penalty solve (α evidence). | **PASS** |
| Special-form recognition/typing + normalised two-anchor blend | γ's deliverable. **executed baseline**: `reify eval cost_robustness_tradeoff_form.ri` → `cost_robustness_tradeoff` evaluates to **undef** (unknown builtin, solver no-progress) — confirms the semantics are genuinely absent (pre-γ baseline). | **PASS** (deliverable; absence confirmed) |
| `cost_robustness_tradeoff(<non-money>, 2.0)` → named diagnostics (non-Money arg, λ∉[0,1]), no panic | **γ's deliverable, NOT assumed substrate** (§4/§6 branch 4). Verified at γ's leaf signal via the negative-assertion sentinel (rejection must fire, exit_code 1) once γ ships — never as a decompose-time substrate probe. | **PASS** (deliverable; correctly fenced from the substrate gate) |

### δ `4793` — tolerance-tied margin (conditional on α's finding)

| Capability asserted | Evidence form | Result |
|---|---|---|
| Per-purpose tolerance scope supplies a per-constraint margin (replacing the configurable default) | **gated on α's investigation** — the availability of the margin source is the very thing α reports. No decompose-time substrate premise. If α reports it cannot, δ drops to a deferred follow-up (configurable default remains). | **DEFERRED** (conditional; `margin_for` contract stable across source, §8.1) |

### ε `4795` — companion docs + successor cross-links (terminal task)

| Capability asserted | Evidence form | Result |
|---|---|---|
| Doc section cross-references the four named successor PRDs | **file-existence (executed `git cat-file -e HEAD:…`)**: `whole-model-objective-coupling.md` ✓, `material-waste-cost-minimisation.md` ✓, `multi-aspect-objective-units-coherence.md` ✓ on main; `discrete-cost-minimisation.md` (PRD 2) is **named-but-not-yet-authored** — §5 marks it "queued (stub / spawned)" as a sibling spawned by a separate /prd session. The cross-reference is a valid forward pointer to a named successor (the parent PRD §0.1/§5 already references it the same way); the file existing is not ε's precondition. | **PASS** (3 on-main + 1 named-forward-ref) |
| Doc section itself | ε's deliverable (not probed). | **PASS** (deliverable) |

---

## Batch verdict

No FAIL bindings. Two DEFERRED bindings (α's tolerance-scope investigation; δ's
conditional margin source) are gated-by-design, not gaps. The decompose-mode
substrate-verification workflow (`scripts/prd-decompose-verify.mjs`) was run over
the substrate-scoped leaf signals before the batch was flipped `deferred → pending`.
