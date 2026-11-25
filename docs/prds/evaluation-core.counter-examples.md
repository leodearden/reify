# Evaluation core: counter-examples

**Status:** part of `docs/prds/evaluation-core.md`. Dated record, 2026-11-25.

The contract of the evaluation core (`docs/evaluation-core-contract.md`) was attacked by nine critic seats over four rounds while it was being drafted. Each finding that broke something came with the smallest model its author could build. This file holds every one of those that bears on the core crate, restated with the crate's toy kinds (PRD section 5.4). The critic reports held 167 findings: 101 bear on the core and are merged here into 74 tests; the other 66 concern the language, later designs, the migration or the wording of claims, and are not repeated. Five more tests, at the end, come from the review of the PRD itself and from the design of the instance kinds.

How to use it:

- **Each heading is the name of a test.** It is listed under the first leaf whose prerequisites can make all of it pass (PRD section 12), and keeps that name. A part of a test that an earlier leaf can already pass may be written there; the leaf named here owns the whole.
- **"What the contract requires" is the test's assertion.** It was derived by tracing the model through the contract's text. Where the text does not settle the rule that produces the answer, the entry says so and the PRD's section 13 lists the point; the required answer is still the one a fresh evaluation of the same snapshot gives.
- **"What went wrong in the draft reviewed"** is history: it describes a draft of the contract that no longer exists, and is kept so the reason for the test is not lost.
- If a leaf finds that the contract's text and an entry here disagree, PRD section 6 applies.

Kinds of test: a *named regression* is one test with the model as written; a *counter assertion* asserts on the core's counters, never on time; *the random-sequence generator must reach it* means a fixed seed of the generator produces a case of this shape, and the entry is also kept as a named test; a *compile-time check* is a signature that makes the faulty program impossible to write.

## Leaf β: Structure by declaration

### `a_body_learns_its_declaration_only_through_recorded_reads`

- **Kind of test:** named regression. **Contract:** 2.1, 2.2. **Found by:** round 1, soundness (L7).
- **Model:** A toy list cell that reads bindings arg0, arg1, ... until one answers absent; two declarations with 2 and 3 bound args, same recipe. A cell that reads binding 'arg' and falls back to binding 'default' when 'arg' answers absent.
- **Sequence:** evaluate list [a, b] at path P1 → evaluate list [a, b, c] at path P2 (same recipe) → evaluate cell A, then cell B (same recipe) → repeat on a fresh core in the opposite order
- **What went wrong in the draft reviewed:** run took a payload per node and nothing said a body learns its declaration only through recorded reads, so a body could branch on its declaration's shape without a recorded read, and a lookup could return another node's result.
- **What the contract requires:** [a, b, c] answers [a, b, c] and [a, b] answers [a, b] in either order; A and B each get their own value; no lookup returns a result filed for a node whose declaration differs.
- **Note for the leaf:** leaf β has no store, so the last clause holds trivially there; from leaf δ, where results are found by lookup, the same test checks it.

## Leaf δ: The store

### `a_body_never_observes_an_unready_read`

- **Kind of test:** compile-time check. **Contract:** 2.3, 2.5. **Found by:** round 1, soundness (L18); round 1, whole design (W10 (content must cover what readers see).
- **Model:** Cell that reads a slow shape's metric (its section on the exclusive resource not yet done) and adds 1.
- **Sequence:** evaluate a cell whose input is slow (still running on its resource) when the cell first reads it
- **What went wrong in the draft reviewed:** run returned Result<Outcome, NotReady> and a body could map NotReady to a default and return Ok; nothing made the core discard such a run.
- **What the contract requires:** The cell's result is r's real value + 1; Cx::read's signature has no not-ready return, so the faulty body does not compile.
- **Note for the leaf:** the part that says a faulty body does not compile is a property of the signature of `read`, which has no value for 'not ready'. No program is compiled to show it.

### `values_of_different_types_never_share_a_content`

- **Kind of test:** named regression. **Contract:** 5.2, 6. **Found by:** round 1, soundness (L23).
- **Model:** Two test-local kinds: B returns the Bool true and I returns the Int 1, and each kind's `content` hashes the same bytes, adding no tag of its own. Reader cell R reads binding 'x', bound to B at one path and to I at another.
- **Sequence:** evaluate R where its ref resolves to Bool true → evaluate R at another path where the same ref resolves to Int 1
- **What went wrong in the draft reviewed:** 5.2 defined content for a value and an opaque result only, and each kind hashed its own Out with no type tag.
- **What the contract requires:** R computes from Int 1 and never finds the result filed from Bool true. The type tag is the core's: a kind supplies a content and the core does the combining (PRD decision D4), so the test passes with kinds whose `content` adds no tag.

### `two_places_check_catches_resource_minted_id_in_result`

- **Kind of test:** named regression. **Contract:** 2.2 (O3), 5.4. **Found by:** round 1, geometry and session (G3 (raw kernel handle ids in values and ).
- **Model:** Toy shape S embeds the resource counter value in its result (leaky variant) or a description-relative name (clean variant). Harness: evaluate S's declaration at path p1, do unrelated shape work, evaluate at p2, compare. Leaky fails, clean passes.
- **Sequence:** core 1: evaluate S's declaration at path p1 → core 2, fresh: unrelated shape work on the toy kernel, then evaluate S's declaration at path p2 → compare the two results by value; once with the leaky variant and once with the clean one
- **What went wrong in the draft reviewed:** 5.4 did not list kernel handle ids, and the two-places test ran each place with an empty store and fresh kernel, which mints the same ids both times.
- **What the contract requires:** The two-places harness reports a mismatch for a toy shape kind that embeds a resource-minted counter in its result, and passes for one that names parts relative to its description.
- **Note for the leaf:** where the sequence trims a shape, use a second, fresh core. Trimming arrives with leaf ρ.

### `order_check_detects_result_mutated_through_exclusive_resource`

- **Kind of test:** named regression. **Contract:** 2.2 (O1, O6), 5.3, 5.6. **Found by:** round 1, geometry and session (G4 (realized shape depends on representa).
- **Model:** Toy shape S, on a toy kernel that keeps a marker per shape. Reader F, a test-local variant of `measure`, sets S's marker inside its section and returns 1. Reader E, another, returns 1 if S's marker is set and 0 if it is not.
- **Sequence:** core 1: evaluate demand {F}, then demand {E} → the order check: a fresh core evaluates demand {E} alone
- **What went wrong in the draft reviewed:** The kernel preference and representation were not in the realize key, and nothing forbade a reader from writing onto a stored shape through the kernel.
- **What the contract requires:** The order check reports that E's answer differs: 1 on core 1, 0 on the fresh core. The fault is F's (O6 forbids changing what it read); the core's part is that the order check exposes it (PRD section 13.2, point 18).
- **Note for the leaf:** the debug mode's re-run would see the same changed kernel, so it is not the witness here.

## Leaf ε: Cycles

### `a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope`

- **Kind of test:** named regression. **Contract:** 2.1, 3.1, 8. **Found by:** round 1, solve (S7).
- **Model:** Root, a toy `scope`, declares three members. `plan` is a test-local cell bound to root's own place, which reads root's result and answers the number of members it declares (3). `handdown` is a toy `scope` with a binding 'src' set to the place root/plan; it declares one member, `a`, a cell bound to handdown's binding 'src' that answers what it reads. root's member `a` is a cell whose binding 'owner' is the place root/handdown/a and which answers what it reads through 'owner'. Variant: root is a test-local scope whose body reads plan before it returns its declarations.
- **Sequence:** evaluate demand {root/a} → variant: evaluate demand {root/a}
- **What went wrong in the draft reviewed:** How an unknown learns its owner was unspecified; if ownership was part of the scope's result, scope -> plan -> scope declarations was a cycle.
- **What the contract requires:** root/a answers 3 with no cycle: root reads nothing by binding a child to a sibling place. Variant: root and root/plan answer 'cycle' with one description.

### `two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first`

- **Kind of test:** named regression. **Contract:** 7.7. **Found by:** round 1, soundness (L19); found while cataloguing.
- **Model:** Cells a = b + 1 and b = a + 1.
- **Sequence:** evaluate demand {a} → fresh core: evaluate demand {b} → fresh core: evaluate demand {a, b}
- **What went wrong in the draft reviewed:** Cycle detection looked only at running bodies, and a body waiting for a NotReady input had returned, so the ring was never detected.
- **What the contract requires:** a and b answer 'cycle' with identical descriptions in all three orders; each body runs once; `run()` returns with no budget given, so the ring is found and not ended by a budget.

### `conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle`

- **Kind of test:** named regression. **Contract:** 7.7. **Found by:** round 1, soundness (L9).
- **Model:** Input f; cell a = if f then b else 1; cell b = if f then 2 else a.
- **Sequence:** S1 (f = true): evaluate demand {a, b} → fresh core: demand {b}, then {a} → S2 = S1.edit(f := false); evaluate {a, b} → S3 = S2.edit(f := true); evaluate {a, b}
- **What went wrong in the draft reviewed:** 7.7 let may_read, which contains every possible read, report a cycle before anything ran, so a <-> b was reported as 'cycle' though the reads that close it are never taken together.
- **What the contract requires:** f = true: a = 2, b = 2. f = false: a = 1, b = 1. Never 'cycle', in every order and across both edits.

## Leaf η: Limits and the paths budget

### `depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content`

- **Kind of test:** named regression. **Contract:** 7.6. **Found by:** round 1, soundness (L6).
- **Model:** T is a toy `guard` with bindings d and live: it reads live first and answers absent when it is false; otherwise it declares cells m = d - 1, l = (d > 0) and leaf = (if l then next/leaf else 1), and next, a T whose binding d is the place m and whose binding live is the place l. Root declares deep (T with d = 70, live = true) and shallow (T with d = 6, live = true). The depth of a path is its number of steps below the root, so the T under deep with d = 6 is at depth 65 and reads the same contents as root/shallow. Depth limit is a snapshot setting input.
- **Sequence:** core 1: evaluate demand {root/deep/leaf}, then {root/shallow/leaf} → core 2 (fresh store): shallow first, then deep → edit the depth-limit setting to 72; evaluate → edit it back to 64; evaluate
- **What went wrong in the draft reviewed:** The depth failure was stored like any result, by recipe and contents; whichever of the two T(d = 6) instances came first decided the other's answer.
- **What the contract requires:** In both orders: root/shallow/leaf = 1; the T at depth 65 answers one depth failure with one diagnostic; the leaf cells above it answer 'blocked by the failure at this ref'. Limit 72: everything under deep succeeds and root/deep/leaf = 1. Limit 64 again: the same failure as the first time.

### `shared_exponential_tree_ends_by_paths_budget_not_node_budget`

- **Kind of test:** named regression. **Contract:** 7.6, 4.1, 4.2. **Found by:** round 1, whole design (W16 (dropping the total-node limit: 2^41); round 1, soundness (L17).
- **Model:** T is a toy `guard` with bindings d and live: it reads live first and answers absent when it is false; otherwise it declares cells m = d - 1, l = (d > 0) and sum = (if l then left/sum + right/sum else 1), and two children left and right, each a T whose binding d is the place m and whose binding live is the place l. Root is T with d = 40 and live = true. Demand root/sum with a paths-resolved budget of 10,000: the request ends with a budget error; each kind's body runs about 41 times; the store holds no failure entry.
- **Sequence:** evaluate demand {root/sum} with a paths budget of 10,000 → sum reads every T under it → each level's two children share results by content but each path is answered → paths resolved grows as 2^k
- **What went wrong in the draft reviewed:** The per-request budget counted node evaluations, not path resolutions, so sharing kept it from firing while index entries grew without bound.
- **What the contract requires:** The request ends with one budget error once paths resolved reaches the budget; each kind's body runs about depth + 1 times; nothing about the exhaustion is stored; a later request with a larger budget is not affected by the earlier one.
- **Note for the leaf:** the summary reads its children one at a time. Read together, two identical siblings would both run until leaf ζ joins runs in flight.

## Leaf θ: Children declared by a running body

### `children_declared_in_a_loop_get_distinct_paths`

- **Kind of test:** named regression. **Contract:** 7.1, 7.5. **Found by:** round 2 (N3).
- **Model:** A caller body loops over a list of three values and declares a shape child per element at step (call-site, index).
- **Sequence:** evaluate demand {peaks} with three loads → edit the second load → reorder the list of loads
- **What went wrong in the draft reviewed:** The step named only the place in the expression, so all three declarations had one path in one run: either a later declaration overwrote an answered path or later calls read the first call's answer.
- **What the contract requires:** Three children at three distinct paths; an edit to one load re-runs only that child. Missing: the core's answer when one run declares the same step twice with different declarations (a failure at that path would be the conservative rule).
- **The text does not yet settle this** (partly). See the PRD's list of open points.

### `one_inner_call_site_reached_from_two_outer_calls_gets_two_paths`

- **Kind of test:** named regression. **Contract:** 7.1, 7.5. **Found by:** round 4, logic (F19).
- **Model:** Caller body with two call sites into a helper that declares a shape child at (call-site, inner-site).
- **Sequence:** evaluate demand {r} → edit 120mm to 130mm → filter a list over which children are declared
- **What went wrong in the draft reviewed:** The step listed place, map iteration and recursion level, not the call sites of enclosing functions; 7.5 claimed stability under any re-elaboration.
- **What the contract requires:** Two children at two paths; editing one argument re-runs only that child; after filtering, children are at their new indices and the old paths answer absent.

### `body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent`

- **Kind of test:** counter assertion. **Contract:** 7.1, 4.2, 2.5. **Found by:** round 2 (N10).
- **Model:** Two identical scopes c1 and c2, each with a toy `call` named result that declares a shape child from its arguments; demand both, then demand the second child's path directly.
- **Sequence:** evaluate demand {root/c1/result} → evaluate demand {root/c2/result} (identical arguments) → demand the path of root/c2/result's shape child directly
- **What went wrong in the draft reviewed:** The parent's chain recorded the read of the child but not its declaration, so an identical parent elsewhere had to run its body (G2 fails), the child could not be resolved by path from outside, and placeholder children could be declared.
- **What the contract requires:** One body run of the shape child for the two identical scopes; zero runs of the second `call`'s body; the second shape child's path is answerable by a direct demand.

### `unbounded_body_declared_recursion_stops_at_the_depth_limit_with_one_failure`

- **Kind of test:** named regression. **Contract:** 7.6, 7.1, 2.2. **Found by:** round 1, soundness (L16); round 1, whole design (W21 (small items: read by kind, versions).
- **Model:** (b) V is a toy `call` reading binding n; its body declares child step 'call' of its own recipe with n := n + 1 and reads that child's value. Root declares V with n = 0. Depth limit is a snapshot setting. (a) A test-local kind whose body loops until it has done the work a work-limit setting it reads allows, and then returns the failure 'out of work'.
- **Sequence:** evaluate demand {root/v} for model (b) → evaluate the kind of model (a) at two places with the same setting
- **What went wrong in the draft reviewed:** Deterministic limits covered path depth and children per result only; roots named by content kept depth at 1, and nested views and a body's own work had no deterministic bound.
- **What the contract requires:** (b) Terminates: exactly one depth failure, with one diagnostic, at the first path past the limit; root/v is a failure blocked by it; nothing stored for the depth failure. (a) The kind of model (a) returns a stored 'out of work' failure, found again by content at the second place.

## Leaf ι: Views

### `a_substitution_under_an_absent_parent_answers_absent`

- **Kind of test:** named regression. **Contract:** 6. **Found by:** round 1, soundness (L24).
- **Model:** Root scope declares a toy `guard` thin, which reads its own condition (x > 5) first, answers absent under a false one, and otherwise declares cell t; constraint cell reads thin/t and x; a toy solver body opens the two views.
- **Sequence:** in a toy solver body: suppose([(x, 4), (thin/t, 3)]); read thin, thin/t and the constraint → suppose([(x, 6), (thin/t, 3)]); read the same
- **What went wrong in the draft reviewed:** 4.2 answered a substitution before resolving the declaration, so a substituted path existed in a trial even when its parent was absent.
- **What the contract requires:** x := 4: thin = absent, thin/t = absent (not 3), constraint evaluated on absent by its kind. x := 6: thin present, thin/t = 3.

### `an_unassigned_unknown_inside_its_own_solve_never_reaches_the_solve`

- **Kind of test:** named regression. **Contract:** 8, 7.7, 2.3. **Found by:** round 1, solve (S10).
- **Model:** A test-local variant of the toy solver, owning p and q, that assigns one at a time and substitutes the other with an 'unassigned' outcome: a failure whose description is 'unassigned' (contract 8). Cell c = p or q reads p and then q; it does not short-circuit, and it answers 'blocked by' an operand that is a failure.
- **Sequence:** toy solver trial: suppose([(p, true), (q, unassigned)]); read constraint → faulty toy solver trial: suppose([(p, true)]) only; read constraint
- **What went wrong in the draft reviewed:** The draft did not say what an owned, unsubstituted unknown reads as inside its own solve's view: a nested re-run of the same solve or a cycle.
- **What the contract requires:** With q substituted: c answers 'blocked by' the unassigned q; no read reaches the solve; the read of c through the view reports that it depended on the substitution. Without (the faulty variant substitutes p only): c's read of q reaches the solve, and the solve fails with 'cycle', not a hang or a nested re-solve.

### `solve_reached_again_under_its_own_view_is_a_cycle_and_terminates`

- **Kind of test:** named regression. **Contract:** 7.7, 10. **Found by:** round 2 (L16 (Part 1 re-run, judged PARTLY), part).
- **Model:** Toy solver whose constraint cell reads the solver's own result path.
- **Sequence:** evaluate demand {root/solves/[u]} → a trial evaluates a node that reads root/solves/[u] itself
- **What went wrong in the draft reviewed:** 7.7 spoke only of the same snapshot, so a solve reached again under its own view was not a cycle and opened a new view each time: non-termination.
- **What the contract requires:** Terminates; root/solves/[u] and the reading node answer 'cycle'; nothing is stored; removing the self-read by an edit gives a solved value.

### `read_outside_view_after_opening_it_fails_the_run`

- **Kind of test:** named regression. **Contract:** 2.2 O9, 4.2. **Found by:** round 4, logic (F20); round 2 (N11).
- **Model:** Toy solver variant that reads a cell outside its view after opening it.
- **Sequence:** toy solver opens a view, runs trials, then reads a cell outside the view
- **What went wrong in the draft reviewed:** Two correctness obligations lived outside O1-O8 with no consequence for breaking them.
- **What the contract requires:** The solve's run ends with a failure; that failure is its answer; no ordered read is filed after its set.
- **Note for the leaf:** at leaf ι a view-opener's result is not stored, so the last clause holds trivially; from leaf κ, which stores view-openers, it can fail, and the test asserts it there.

### `view_reports_which_answers_depended_on_substitution`

- **Kind of test:** named regression. **Contract:** 6, 8, 2.3. **Found by:** round 1, whole design (W6 (missed coupling cannot be reported: ).
- **Model:** Toy solver opens view {x := 3}; reads cell y = x + 1 and cell z = 5 through it. Asking the core must answer: y depended on the substitution, z did not.
- **Sequence:** a toy solver body opens the view {x := 3} → it reads y and z through the view; each read returns its outcome and whether it depended on the substitution
- **What went wrong in the draft reviewed:** The draft claimed the core reports a trial that reads another solve's unknown, which needs either kind knowledge in the core or a transitive-read query that Cx lacked.
- **What the contract requires:** y = 4, reported as depending on the substitution; z = 5, reported as not depending on it. A read through a view returns that fact with the outcome (contract 2.3 and 6; PRD section 13.2, point 11).

## Leaf κ: What a view-opener's trials rested on

### `solve_rests_on_ordinary_values_its_constraints_read_under_trial`

- **Kind of test:** named regression. **Contract:** 6, 8. **Found by:** round 2 (N1).
- **Model:** (1) Two toy scopes, each with an unknown u, a cell other (5 and 2), and a toy solver minimising u with u >= 0 and u + other >= 10; variant with other as an argument binding and as a plain member. (2) One scope, cell w, solver minimising t with t >= w / 10; edit w.
- **Sequence:** (1) evaluate demand {root/a1/u, root/a2/u} on a cold store, both orders → (2) evaluate demand {t}; edit w := 60mm; evaluate demand {t}
- **What went wrong in the draft reviewed:** The rests-on set held only reads that did not depend on a substitution; every constraint a solve reads through its view depends on it, so the inputs those constraints read were recorded nowhere.
- **What the contract requires:** (1) root/a1/u = 5 mm, root/a2/u = 8 mm, both constraints satisfied, in one snapshot, in either order. (2) t = 4 mm, then 6 mm after the edit.

### `rests_on_member_with_changed_content_invalidates_stored_solve`

- **Kind of test:** named regression. **Contract:** 6. **Found by:** round 2 (N6).
- **Model:** Cells flag, m = if flag then x else K, c = (m <= 10); toy solver over x reads c through its view; edit K.
- **Sequence:** evaluate the solve with flag = false → edit the else-branch literal of m from 5mm to 15mm → evaluate the solve again
- **What went wrong in the draft reviewed:** The set check's only failure condition was 'now depends on the marker'; unchanged contents were never required.
- **What the contract requires:** After the edit the solve answers a failure (c violated for every x), as a fresh evaluation does.

### `solve_rests_on_inputs_its_trial_nodes_read_after_edit_and_across_places`

- **Kind of test:** named regression. **Contract:** 6, 2.1, 5.3. **Found by:** round 4, logic (F1).
- **Model:** Cells read their formula from a snapshot-input binding. Scope Sa: y's formula input 'A.y' = u*2, constraint input 'A.c' = (y >= 10), objective min u, toy solver. Scope Sb: identical except y's input key 'B.y' = u*4 and constraint input key 'B.c' with the same content as 'A.c'. Edit test: change 'A.y'. Cross test: demand both solves in both orders. Attached-input variant: y also reads an input attached to its own path, different at the two places.
- **Sequence:** evaluate demand {u} → edit the source of y from u * 2 to u * 4 → evaluate demand {u} → cross-place: evaluate two solves whose constraint and objective sources are equal in content but whose y cells read different source units (u * 2 and u * 4), both orders
- **What went wrong in the draft reviewed:** The set recorded only nodes; a node's source, a function body and a setting are snapshot inputs, so an edit to them left the stored solve applicable and the walk never reached it.
- **What the contract requires:** Edit case: u = 5 mm, then 2.5 mm. Cross-place case: the two solves answer 5 mm and 2.5 mm in one snapshot, in either order. Missing: how a rests-on member that is a snapshot input is named so that a lookup at another place checks that place's own input (relative to the caller, as nodes are), not the recording run's key.
- **The text does not yet settle this** (partly). See the PRD's list of open points.

### `rests_on_check_evaluates_members_as_ordinary_answers_of_the_snapshot`

- **Kind of test:** named regression. **Contract:** 6. **Found by:** round 2 (N15).
- **Model:** Toy solver with rests-on member m; edit m to read a toy shape node; lookup; compare m's answer with a fresh evaluation.
- **Sequence:** solve stored with rests-on set {m} → edit so that m now reads an expensive node → lookup the solve
- **What went wrong in the draft reviewed:** Undefined marker semantics.
- **What the contract requires:** Members checked during a lookup get the same answers a fresh evaluation of the snapshot gives; nothing is computed on a stand-in value.

### `rests_on_check_stops_at_first_changed_member_in_first_read_order`

- **Kind of test:** counter assertion. **Contract:** 6. **Found by:** round 4, logic (F8).
- **Model:** Inputs use = true and mesh. K is a toy `shape` made from mesh, and k a `measure` of K; neither depends on u. A toy `guard` gc, whose condition is use, declares the constraint cell c = (u * k >= 10). A toy solver over u, minimising u, has c as its problem. Its trials read gc, which reads use, before they read k, so use is the first member of what they rested on.
- **Sequence:** S0: evaluate the solve → S1 = S0.edit(use := false, mesh changed) → evaluate the solve; count K's body runs
- **What went wrong in the draft reviewed:** The set was unordered, so a check had to answer every member.
- **What the contract requires:** S1: K's body runs 0 times; the solve runs again without the guarded constraint.

### `rests_on_check_that_waits_on_a_running_reader_misses_instead_of_cycling`

- **Kind of test:** named regression. **Contract:** 7.7, 4.2, 6. **Found by:** round 4, logic (F7).
- **Model:** Cells flag, c = if flag then x else 5, cell x reading the toy solver's result; toy solver minimises x with x + c >= 8 through a view.
- **Sequence:** S0: evaluate demand {x, c} → S1 = S0.edit(flag := true); evaluate demand {x, c}
- **What went wrong in the draft reviewed:** 4.2's 'leads back' rule covered only following a stored chain; 7.7 counted waiting bodies as in progress; a wait cycle through a lookup either deadlocked or gave 'cycle' for the whole snapshot.
- **What the contract requires:** S1: x = 4 mm, c = 4 mm; no 'cycle'; terminates.

### `final_state_finds_trial_results_when_solver_substitutes_final_content`

- **Kind of test:** counter assertion. **Contract:** 6. **Found by:** round 4, logic (F17).
- **Model:** Toy solver with an option to substitute (value, determinacy) or value only; expensive toy node between unknown and constraint.
- **Sequence:** solve runs trials; its result is u = 7 mm determined → readers between u and the constraints evaluated in the final state; count body runs
- **What went wrong in the draft reviewed:** Claimed the final state finds what the last trial computed.
- **What the contract requires:** When the toy solver substitutes exactly its final outcome at the final point: 0 body runs of the trial-path nodes in the final state. When it does not: they run once (documented, not a fault).
- **Note for the leaf:** the toy `solved` answers exactly the outcome (value and determinacy) its solve's map holds for its unknown, so a substitution of that outcome has the content the `solved` node's result will have; the option that substitutes the value only gives an outcome of another content.

### `change_and_change_back_refinds_a_view_opening_node`

- **Kind of test:** counter assertion. **Contract:** 6, 4.2. **Found by:** round 2 (N14).
- **Model:** Toy solver whose trial reads cell m; edit m A -> B -> A; and K identical solvers at K places with K different m values.
- **Sequence:** solve with rests-on member m = A → edit m := B → edit m := A → count solver body runs
- **What went wrong in the draft reviewed:** 'A new run replaces the set', so the previous set was lost.
- **What the contract requires:** Change and change back: 0 solver body runs on the way back, while the number of distinct sets under the prefix is within the store's alternative limit (a setting of the core, PRD section 13.2, point 20). K identical solvers at K places with K different values of m, K no more than that limit, evaluated one after another and then again in a snapshot made by an unrelated edit: no solver body runs the second time.

## Leaf λ: A view inside a view

### `guard_that_reads_an_unknown_is_evaluated_per_trial_not_a_cycle`

- **Kind of test:** named regression. **Contract:** 6, 8. **Found by:** round 1, soundness (L3); round 1, whole design (W7 (solved count / guard over an unknown).
- **Model:** Scope G declares: a child scope a with its own toy solver owning len, trying 10, 11, ..., 100 in order, with constraint cells len >= 20 and len <= 100; a toy `guard` p, which reads its condition cell (x > 6.5) first, answers absent under a false one, and otherwise declares the constraint cell a/len == 2 * x; constraint cells x >= 6 and x <= 8; a planning cell, a test-local cell that reads only G's declarations; and a toy solver owning x, trying 8, 7.5, 7, 6.5, 6 in order, that reads the planning cell and then tries each candidate through cx.suppose. A toy solver counts an absent constraint as holding.
- **Sequence:** evaluate demand {x, a.len} → fresh core: evaluate demand {a.len}, then {x}
- **What went wrong in the draft reviewed:** The plan read guards and counts outside any view; the escape for structure controlled by a value on trial held only inside a view, so plan -> guard -> unknown -> solve -> plan was a cycle in the outer snapshot.
- **What the contract requires:** No 'cycle'. a/len = 20, independent of x and of order. x = 6.5: the trials at 8, 7.5 and 7 have p present and a/len == 2 * x false; at 6.5 p is absent. At the solved x, p and its constraint answer absent. Same answers in both demand orders.

### `mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order`

- **Kind of test:** named regression. **Contract:** 6, 7.7, 8. **Found by:** round 1, soundness (L2); round 2 (N4).
- **Model:** Root scope declares scope children a and b; each declares a toy solver owning u with constraints u >= 0 and u + other >= 10 and objective min u; a's binding `other` -> place b/u, b's -> place a/u. Run with two toy-solver variants: (i) reads `other` before cx.suppose; (ii) reads `other` through the view.
- **Sequence:** core 1: evaluate demand {a.u}, then {b.u} → core 2 (fresh store): evaluate demand {b.u}, then {a.u} → core 3 (fresh store): evaluate demand {a.u, b.u} together → edit b's argument to the constant 5mm; evaluate {a.u, b.u}
- **What went wrong in the draft reviewed:** Each trial of a's solve ran b's solve nested inside it. Demanding a.u first gave (0, 10), b.u first gave (10, 0), and with no trial on the final point (0, 0) with both constraints VIOLATED. b's solve recorded different view reads inside a's view than in S, so one recipe and prefix had two 'next read' entries.
- **What the contract requires:** Orders 1-3: a.u and b.u both answer failure 'cycle' with byte-identical descriptions; nothing stored for it; never (0,10), (10,0) or (0,0). After the edit: b.u = 5mm, a.u = 5mm (each minimises u under u + 5 >= 10), equal to a fresh evaluation.

### `solves_coupled_through_arguments_are_a_plain_ring`

- **Kind of test:** named regression. **Contract:** 8, 4.2, 7.7, 10. **Found by:** round 2 (L2 (Part 1 re-run, judged PARTLY)); round 4, logic (F11).
- **Model:** Two toy scopes a and b, each with an unknown u, an argument cell other bound to the sibling's u, and a toy solver that reads its argument first and then minimises u subject to u >= 0 and u + other >= 10 through a view. The test declares one solver per scope (the plan 'missed' the coupling).
- **Sequence:** cold store: evaluate demand {root/a/u}, then demand {root/b/u} → fresh core, cold store: evaluate demand {root/b/u}, then demand {root/a/u} → compare every answer between the two orders
- **What went wrong in the draft reviewed:** Second draft: if the view-inside-a-view condition is checked when the view is opened, b opens its view inside a's trial, solves u_b = 10 - t, files that under its problem reads, and b in S finds it; if checked at every read, the failure is filed under reads that omit the outer substitution and is found outside the view, so a is 'blocked by b' in one order and the reverse in the other.
- **What the contract requires:** In both orders, root/a/solves/[u], root/b/solves/[u], root/a/u, root/b/u, root/a/other, root/b/other all answer the failure 'cycle' with one identical description; no result is stored for any of them; the evaluation terminates; never a value pair.

### `inner_solve_with_outer_dependent_inputs_follows_each_outer_trial_without_cycle`

- **Kind of test:** named regression. **Contract:** 6. **Found by:** round 4, logic (F12); round 4, values (VIEW-NEST (value with an auto built from).
- **Model:** Outer toy solver over count n (integer trials) reading a per-element value; toy collection declaring n scopes with constant binding pitch = 100 / n; each scope has an unknown angle and its own toy solver for a constraint over pitch and angle. Variant: scope cell q = pitch * 2 read by the constraint instead of pitch. Example 11 variant: outer unknown w, child scope Slot(width = w) with its own unknown depth >= width.
- **Sequence:** outer solve on n opens view n := 3 → a trial reads vents/[0]'s solved angle → vents/[0]'s solve reads its declaration and pitch, then opens view angle := a and evaluates its constraint → outer trial n := 4 reads vents/[0]'s solved angle again
- **What went wrong in the draft reviewed:** The exemption 'depending merely for existing does not count' needed a second kind of dependence the core does not record.
- **What the contract requires:** Per section 6's last sentence and Example 11: under n := 3 the inner solve answers the angle satisfying its constraint at pitch 33.3 mm, under n := 4 at 25 mm; neither is an answer of the surrounding state; no 'cycle'. The property is now in contract section 6 (PRD section 13.2, point 31); what the core records to tell it from a genuine mutual dependence (the case of `mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order`) is left to this leaf.
- **The text does not yet settle this** (partly). PRD section 13.1, point 6.

### `element_solve_independent_of_the_count_answers_under_every_count_trial`

- **Kind of test:** named regression. **Contract:** 6, 8. **Found by:** round 2 (N12); round 1, solve (S2).
- **Model:** Outer toy solver over count n (integer trials); collection of n toy scopes, each with an unknown angle and its own toy solver for angle == 30; outer constraint sums a value per element.
- **Sequence:** outer solve on n opens view n := 3 → trial evaluates total_airflow over vents/[0..2], each reading its vent's angle → vents/[i]'s solve opens its own view angle := a → outer trial n := 4
- **What went wrong in the draft reviewed:** The rule failed every inner view-opener whose path or inputs depended on the outer unknown, so Example 4's stronger case failed once Vent had any auto, and (b) and (c) were forbidden without saying so.
- **What the contract requires:** Per section 6's last sentence: under each count trial every vent's solve answers angle = 30deg and total_airflow is evaluated on n such vents; no 'cycle'. The property is now in contract section 6 (PRD section 13.2, point 31), and section 6 governs a nested solve that is not mutual: section 8's sentence that two solves never nest through the core is known to be wrong here (PRD section 13.2, the conflict in section 8). What the core records to tell this case from a mutual dependence is left to this leaf.
- **The text does not yet settle this** (partly). PRD section 13.1, point 6.

## Leaf μ₁: What a new snapshot inherits

### `absent_answers_are_indexed_with_their_ancestor_reads`

- **Kind of test:** named regression. **Contract:** 4.2, 6. **Found by:** round 1, soundness (L8).
- **Model:** (1) Input use_plate; a toy `guard` thin, which reads use_plate first, answers absent when it is false, and otherwise declares cell t = 4; cell tt reads thin/t. (2) Input n = 4; collection vents reads n and declares elements with cell area = 10; cell probe reads vents/[3]/area; a toy solver body that calls suppose([(n, 3)]), reads probe and asks which answers depended on the substitution.
- **Sequence:** S1: evaluate demand {tt}: tt = absent → S2 = S1.edit(use_plate := true); evaluate demand {tt} (with tt kept from S1, and again with it not kept) → in a toy solver body in S: suppose(n := 3); read probe through the view; ask whether probe depended on the substitution → outside the view, evaluate S demand {probe}
- **What went wrong in the draft reviewed:** Only body reads were recorded; ancestor declaration reads and 'absent' answers were not indexed, so (1) tt was not reached when root/thin came into existence and stayed absent, and (2) probe under the view was judged independent of the substitution and leaked into the parent's index.
- **What the contract requires:** (1) tt = 4mm, final, in both variants. (2) probe = absent under the view and reported as depending on the substitution; outside the view probe = 10mm.

### `scope_gaining_a_member_compares_one_declaration_per_child_and_touches_nothing_below`

- **Kind of test:** counter assertion. **Contract:** 11 G1, 3.1. **Found by:** round 2 (N13).
- **Model:** Toy scope with N cell children whose argument values are separate snapshot inputs; a toy collection reading a count cell.
- **Sequence:** root scope with N children gains one member; collection N -> N + 1; one part's parameter edited, at N = 100, 1000, 10000
- **What went wrong in the draft reviewed:** G1 claimed no growth where the text required N comparisons; the argument sat inside the member body the parent reads.
- **What the contract requires:** Gaining a member: index entries touched at most N plus a constant (one per child, whose declaration is compared), and 0 body runs under unchanged children; editing one argument: no growth with N.

### `work_after_an_edit_stops_where_content_is_unchanged`

- **Kind of test:** counter assertion. **Contract:** 4.4, 3.1, 11. **Found by:** round 1, soundness (L13); round 2 (W4 (Part 1 re-run, judged PARTLY)); round 1, whole design (W4 (G1 not delivered; walk without cutof).
- **Model:** Input g; cell h = min(g, 10); N cells p_i = h + i. And a root scope declaring N child cells, each bound to its own argument source input.
- **Sequence:** S1: evaluate demand {p_0..p_(N-1)} for N = 100, 1,000, 10,000 → S2 = S1.edit(g := 30mm); evaluate; read core.counters() → S3: the root scope with N child cells, each bound to its own argument source input, all demanded; edit one cell's argument source; read core.counters()
- **What went wrong in the draft reviewed:** Reach was the full transitive 'who read this' closure from the changed inputs, with no comparison of contents, so G1's lookup and index counters grew with N behind a cutoff; and the root scope read the literal it bound.
- **What the contract requires:** S2: body runs = 1 (h); store lookups and index entries touched bounded by a constant independent of N. S3: the root scope's body does not run; body runs, lookups and entries touched bounded by the edited part's cone, independent of N.

## Leaf μ₂: Inheriting a node whose body declared children

### `child_a_rerun_body_stops_declaring_becomes_absent`

- **Kind of test:** named regression. **Contract:** 7.1. **Found by:** round 4, logic (F4).
- **Model:** Caller cell ok, a toy `call`: ok = if flag then (declare shape child X, read it, compare) else true. The child's path is written ok/X here; its step has the form leaf θ chose (PRD section 15, question 5). Demand ok and ok/X; flip flag.
- **Sequence:** S1: flag = true; evaluate demand {ok, ok/X} → S2 = S1.edit(flag := false); evaluate the same demand
- **What went wrong in the draft reviewed:** The walk stopped at ok because its content was unchanged, and ok/X's own reads (constant bindings) had not changed.
- **What the contract requires:** S2: ok = true; ok/X answers absent.

### `child_of_a_parent_answered_from_the_store_is_absent_when_not_declared`

- **Kind of test:** named regression. **Contract:** 7.1, 4.4, 4.2. **Found by:** found while cataloguing.
- **Model:** As `child_a_rerun_body_stops_declaring_becomes_absent`, but with flag false first so the store already holds ok's no-child result when flag flips back.
- **Sequence:** S0: evaluate demand {ok} → S1 = S0.edit(flag := true); evaluate demand {ok, ok/X} → S2 = S1.edit(flag := false); evaluate demand {ok, ok/X}
- **What went wrong in the draft reviewed:** Not reviewed: the rule is the post-round-4 fix to F4.
- **What the contract requires:** S2: ok = true; ok/X answers absent, as a fresh evaluation of S2 does. Missing: a rule that fires whenever the parent's answer changes which children it records, whether that answer came from a run or from the store.
- **The text does not yet settle this** (undetermined). See the PRD's list of open points.

### `branch_left_by_an_edit_is_not_evaluated_before_its_reader_is_rechecked`

- **Kind of test:** counter assertion. **Contract:** 4.4, 7.1, 4.2. **Found by:** round 1, soundness (L14); round 2 (L14 (Part 1 re-run, judged PARTLY)).
- **Model:** Input load; cell detailed = load > 100; cell r whose body reads detailed and then either declares child step 'fea' (a shape reading binding load) and reads it, or returns load / 2.
- **Sequence:** S1 (load = 150N): evaluate demand {r}: detailed = true; the FEA child runs → S2 = S1.edit(load := 50N); evaluate demand {r}; read the FEA kind's body-run counter
- **What went wrong in the draft reviewed:** Every input of a body's last trace was brought up to date before the body ran, including reads after one whose content had changed, so the untaken branch's expensive node was evaluated (against G3).
- **What the contract requires:** S2: the FEA child's body runs 0 times; r = 25 (load / 2); a later request on S2 that demands the FEA child's path gets 'absent': the entry passed over when r left that branch was removed from S2's index, so it is not served. Missing: a sentence that a kept node reached by the outward work is not re-checked until its readers' earlier reads are known unchanged (equivalently, that kept nodes are re-checked only through their readers, in read order).
- **The text does not yet settle this** (partly). See the PRD's list of open points.

## Leaf ν: Inheriting a node that opened a view

### `independent_solve_groups_fail_and_resolve_independently`

- **Kind of test:** counter assertion. **Contract:** 8, 11. **Found by:** round 1, solve (S4); round 1, whole design (W5 (solve keyed by scope alone re-solves).
- **Model:** Scope S declares a planning node P, a test-local kind that reads the sources of S's constraint cells (snapshot inputs) and nothing computed, and whose result, not its body, declares one toy solver per group of unknowns that a constraint couples. Each group here is one unknown with its bound constraint cells; the unknown is a `solved` node reading its group's solve, and each toy solver tries 0, 1, ..., 10 in order. In the first model there are two groups: a, with 1 <= a <= 5, and c, with c >= 6 and c <= 5, which no value meets.
- **Sequence:** evaluate demand {a, c} → N = 100, 1,000, 10,000 independent groups under S, all demanded; edit the source of one group's bound; read core.counters()
- **What went wrong in the draft reviewed:** One solve node per scope bundled independent components, so one failing strict auto poisoned every auto in its scope, and any edit re-solved every component.
- **What the contract requires:** a = 1; c answers 'blocked by' its solve's failure, and nothing else is affected by it. After editing one of N groups: solver body runs = 1, P's body runs = 1 (it read the edited source), other counters independent of N.

### `edit_that_routes_a_rested_on_member_through_the_unknown_is_a_miss_not_a_cycle`

- **Kind of test:** named regression. **Contract:** 4.2, 4.4, 6, 7.7, 10. **Found by:** round 1, soundness (L1).
- **Model:** Input flag. Toy solver owning x on a fixed grid of trial values; its problem is constraint cells c1 (x >= 1), c2 (x <= 8), c (m <= 10), whose sources are snapshot inputs. Cell m = if flag then 2*x else 5. The solver reads its problem, then for each t reads c1, c2, c through cx.suppose([(x, t)]) and returns the first t satisfying all. Demand x.
- **Sequence:** S1 = snapshot(flag = false); evaluate demand {x}: the solve runs trials x := t and records rested-on set {src(c1), src(c2), c = OK} → S2 = S1.edit(flag := true) → evaluate S2 demand {x}; run in two variants: (A) S1's request on x is still live, so x, solve, c, m are inherited kept entries; (B) S1's request was dropped first, so x is answered through the store routine → compare every answer with a fresh evaluation of S2
- **What went wrong in the draft reviewed:** View reads that did not depend on `with` were recorded as reads of the caller, and the 4.2 lookup replayed them in the outer snapshot, outside the view. In S2 replaying c reaches m -> x -> solve, the node being looked up, and 7.7 answered 'cycle' for {x, solve, m}; the body run fresh reads m through the view and has no cycle. G0 failed for deterministic kinds.
- **What the contract requires:** Both variants: x = the toy solver's solution of {x >= 1, x <= 8, 2x <= 10} (in [1, 5]), final; m = 2x; c = OK; never 'cycle' and never a hang; identical to a fresh evaluation of S2. Missing for variant A: a sentence saying that when a read waits on an unconfirmed inherited entry whose confirmation leads back to the waiting reader, the entry is treated as the 4.2 lookup treats a chain that leads back (it does not apply and its body is run), or that re-checking a kept node is exactly the 4.2 chain walk with its rules.
- **The text does not yet settle this** (partly). See the PRD's list of open points.

## Leaf ξ₁: Inheriting from a snapshot whose work was unfinished

### `edit_made_while_the_parent_is_still_evaluating_never_inherits_a_stale_answer`

- **Kind of test:** named regression. **Contract:** 4.4. **Found by:** round 1, soundness (L12).
- **Model:** Input w; a toy `shape` s whose section waits on a barrier the test holds; k, a `measure` of s, answering 0; cell area = w * 10 + k.
- **Sequence:** C = snapshot(w = 10); evaluate demand {area} on C; one `turn()`: area waits on k, whose section is held → P = C.edit(w := 12); evaluate demand {area} on P while C's request is still live and area is unanswered in C → release the barrier; run until idle → end C's request → variants: P's first request made after C's request has finished; C's request ended before P's first request
- **What went wrong in the draft reviewed:** A child's index was its parent's outside a reach computed once, at creation; entries the parent gained later, including ones copied in from trials, were served in the child for paths the change reaches.
- **What the contract requires:** P: area = 120; C: area = 100; in every variant. What P inherits is taken at its first request, and nothing C answers afterwards is served in P.

### `entry_inherited_from_an_unfinished_walk_is_reconfirmed_before_it_is_served`

- **Kind of test:** named regression. **Contract:** 4.4. **Found by:** round 4, logic (F2).
- **Model:** (1) Inputs a = 1, b = 1, h = 1. A toy `shape` s made from h, whose section waits on a barrier while the test holds it; m, a `measure` of s, answering 0; cells x = a * 10 + m and y = b * 10. (2) Inputs f = true, g = 1; cells n = g * 2 and r = if f then n else 0.
- **Sequence:** (1) S0: evaluate {x, y} to completion, leaving the request live → S1 = S0.edit(a := 2, h := 2); hold the barrier; evaluate {x, y} on S1; one `turn()`: y is answered, and x waits on m, whose new section is held → S2 = S1.edit(b := 2); evaluate {x, y} on S2 while S1's request is still live → release the barrier; run until idle; end the earlier requests → (2) P: evaluate {r}, leaving the request live; S = P.edit(f := false, g := 5); evaluate {r}; a second request on S demands {n}
- **What went wrong in the draft reviewed:** Nothing defined which inherited entries were current; an entry the parent's walk had not reached was inherited as current, and an entry skipped under the reader-first rule became 'kept' when a later request demanded it.
- **What the contract requires:** (1) x = 20, y = 20 in S2. (2) r = 0; n = 10 on the second request; no read ever returns an unconfirmed entry.

## Leaf ξ₂: A changed demand

### `path_dropped_from_demand_is_not_inherited_stale`

- **Kind of test:** named regression. **Contract:** 4.4. **Found by:** round 2 (N2).
- **Model:** Cells g, x = g + 1, y = x * 2; demand changes between requests on the same snapshot.
- **Sequence:** S0: evaluate demand {x, y} → on S0, demand becomes {x} → S1 = S0.edit(g := 5); evaluate demand {x} → evaluate demand {y} in S1 → S2 = S1.edit(an unrelated input); evaluate demand {y} in S2 → variant, a separate model: inputs g = 1 and f = true; cells y = g * 2 and x = if f then 0 else y. S0: evaluate demand {x, y}; on S0 the demand becomes {x}; S1 = S0.edit(g := 5); evaluate demand {x}; S2 = S1.edit(f := false); evaluate demand {x}: x reads y for the first time since the demand dropped it
- **What went wrong in the draft reviewed:** 'Checked in S' was undefined for an entry copied from the parent; copies counted as checked served y = 4.
- **What the contract requires:** y = 12 in S1 and in S2; in the variant, x = 10 in S2, the fresh value of y.

## Leaf ξ₃: A changed limit setting

### `changing_a_limit_setting_reaches_every_path_whose_check_changes`

- **Kind of test:** named regression. **Contract:** 7.6, 4.2. **Found by:** round 4, logic (F3).
- **Model:** T is a toy `guard` with bindings d and live: it reads live first and answers absent when it is false; otherwise it declares cells m = d - 1, l = (d > 0) and leaf = (if l then next/leaf else 1), and next, a T whose binding d is the place m and whose binding live is the place l. Root is T with d = 70 and live = true, so root/leaf reads down to the T with d = 0 at depth 70. The depth limit is a snapshot setting. Separately, a toy `call` declares 100,000 children in a loop, with a children-per-node setting of 1,000.
- **Sequence:** evaluate demand {root/leaf} with depth limit 64 → edit the depth setting to 100; evaluate → edit it to 3; evaluate demand {root/leaf} → evaluate the `call`
- **What went wrong in the draft reviewed:** The core checked limits but no read recorded the setting, so changing it reached no path; a body's children were not counted.
- **What the contract requires:** limit 64: root/leaf answers 'blocked by' the depth failure at depth 65. Limit 100: root/leaf = 1. Limit 3: every path deeper than 3 that is read answers too deep, and root/leaf answers 'blocked by' that failure, not 1. The `call` gets a children-per-node failure at the first child over the limit.

## Leaf ζ: Joining a run in flight

### `first_evaluation_of_a_fold_over_n_elements_runs_its_body_once`

- **Kind of test:** counter assertion. **Contract:** 2.5, 11. **Found by:** round 1, soundness (L15); round 1, whole design (W9 (one NotReady per rerun: quadratic fi); round 2 (W9 (Part 1 re-run, judged PARTLY)).
- **Model:** Collection holes whose count is read from an input n, declaring n elements each with a cell area; cell total reads the count, then read_all over the n element refs.
- **Sequence:** snapshot with n holes, n = 100, 1,000, 10,000; evaluate demand {total}; read core.counters()
- **What went wrong in the draft reviewed:** A body discovered one unpredicted read per run and was re-run after each NotReady, so a first evaluation of a fold was quadratic; no growth test measured a first evaluation.
- **What the contract requires:** total's body runs once; reads O(n); element bodies run at most once each, once in all when the elements are identical.
- **Note for the leaf:** the clause about identical elements costing one element body needs runs in flight to be joined, which is why the test sits here. That the fold's own body runs once is asserted from leaf β (G4).

### `inner_solve_independent_of_the_outer_trial_runs_once`

- **Kind of test:** counter assertion. **Contract:** 8, 6. **Found by:** round 1, solve (S3).
- **Model:** Three nested toy scopes, each with a toy solver owning one unknown, read by a `solved` node. Scope x: unknown len, tried 10, 11, ..., 100 in order, with the constraint cell len >= 25. Scope a declares x and an unknown y, tried 0, 1, ..., 60, with the constraint cells x/len + y == 60 and y >= 0; a's solver reads x/len before it opens its view, since len is x's to decide. Root declares a and an unknown z, tried 20, 21, ..., 30, with the constraint cell a/x/len == z, which root's solver reads through its view.
- **Sequence:** evaluate demand {root/z, root/a/y, root/a/x/len} together; read the per-kind body-run counters
- **What went wrong in the draft reviewed:** The claim rule was not closed under coupling, so ordinary models produced solves nested inside each other's trials rather than 'one after the other'.
- **What the contract requires:** len = 25, y = 35, z = 25. x's solve, a's solve and root's solve each run once, though root's solve makes six trials; answers equal a fresh evaluation in every demand order.

## Leaf ο: Determinism and the nonce

### `seeded_reader_of_nondet_result_not_found_after_remake`

- **Kind of test:** named regression. **Contract:** 5.2, 2.4. **Found by:** round 1, geometry and session (G6 (persistent disk cache aliases a re-m).
- **Model:** A toy `shape` M in its non-deterministic mode (content: the hash of its key K plus a nonce), and F, a `measure` of M.
- **Sequence:** core 1: evaluate demand {F}: M1 under key K, F1 filed under M1's content → `selected_store` on core 1 keeps F1 and not M1 → core 2, built on that store: evaluate demand {F}: M is made again as M2, with a new nonce → F is looked up
- **What went wrong in the draft reviewed:** Content of a key-as-content result was the key alone, so a remade non-deterministic mesh had the same content and a persisted FEA matched it.
- **What the contract requires:** Core 2 finds no stored result for F under M2's content, and F's body runs; F1 is never returned for M2. With M deterministic, core 2 would make M again with the same content and find F1.

### `debug_mode_catches_a_false_determinism_claim`

- **Kind of test:** named regression. **Contract:** 5.6. **Found by:** round 4, logic (F22).
- **Model:** D, a test-local kind that claims its result is deterministic and returns a number that grows by one on each of its runs (it breaks O1 and O8 on purpose); cell F reads D.
- **Sequence:** with the debug mode on and set to re-run every body that claimed determinism: evaluate demand {F} → a fresh core with the debug mode off: evaluate demand {F}
- **What went wrong in the draft reviewed:** The debug mode only followed chains and never re-ran bodies.
- **What the contract requires:** With the debug mode on, its re-run of D returns another number and the false claim is reported; with it off, nothing is reported and the answers are those of the first run.

### `answers_downstream_of_a_nondeterministic_result_equal_a_fresh_evaluation_seeded_with_it`

- **Kind of test:** the random-sequence generator must reach it. **Contract:** 11, 5.6, 10. **Found by:** round 1, soundness (L5); round 2 (N18); round 1, geometry and session (G12 (tolerance not carried through reade).
- **Model:** N, a test-local kind that says its result is not deterministic, reads an input choice and returns 70.00000000000006 when it is 0 and 69.99999999999951 when it is 1; its tolerance is 1e-9 around 70. Cell v = (N <= 70). Cell n = floor(N / 10). A collection declares n elements.
- **Sequence:** S1 (choice = 0): evaluate {v, the collection} → S2 = S1.edit(choice := 1); evaluate {v, the collection} → for each snapshot, compare v and the element count with those of a fresh core evaluating it on a store holding that snapshot's N result, built as section 5.5's check 2 builds it
- **What went wrong in the draft reviewed:** G0 said history never changes an answer and section 10 said order never does; a tolerant result feeding exact kinds makes discrete downstream answers (a verdict, a count, structure) differ exactly between histories, so G0 as written could not hold or be tested.
- **What the contract requires:** For each snapshot: v and the element count equal, exactly, those of the fresh core handed the same N result; each N result is within its tolerance. The two snapshots legitimately give different downstream answers (v false and 7 elements; v true and 6 elements).

## Leaf π: What was there last time

### `equal_roots_share_results_but_not_place`

- **Kind of test:** named regression. **Contract:** 7.5, 1 (item 9), 2.1. **Found by:** round 1, geometry and session (G9 (roots named by content conflate two ).
- **Model:** Two roots L and R of the same toy scope recipe, each with cell w reading a snapshot input attached to its own path (both 10). Evaluate both: R runs no bodies. Change L's attached input to 12: R's answer and counters unchanged.
- **Sequence:** evaluate roots Left and Right (identical content) → edit (or attach a per-path setting to) Left's w → read Right's body
- **What went wrong in the draft reviewed:** Every root was named by the content of its declaration, so equal definitions or equal constructions had one path and any place-based job (preview, selection, previous) moved both.
- **What the contract requires:** Before the edit, Right's evaluation after Left's runs zero bodies. After an input attached to root Left changes, Right's answer is unchanged and its index entries untouched; previous() at Right's paths is Right's own.

### `identical_lookups_in_flight_run_the_body_once`

- **Kind of test:** counter assertion. **Contract:** 10. **Found by:** round 1, soundness (L28).
- **Model:** Root scope declares two identical scope children, each with a toy solver that uses previous; demand both solves at once.
- **Sequence:** evaluate demand {a/solve, b/solve} together; read core.counters()
- **What went wrong in the draft reviewed:** 'At most one evaluation per key' could not prevent duplicates, because a key with recorded reads is known only when the run ends.
- **What the contract requires:** The solve body runs once; a and b answer the same result; G0 holds with that one non-deterministic result.

### `previous_is_last_final_answer_outside_views_and_changed_covers_only_reads_made`

- **Kind of test:** named regression. **Contract:** 5.7. **Found by:** round 4, logic (F9).
- **Model:** Inputs t (the dragged value) and g. Toy shapes A and B, each with a `measure`; count their body runs. A toy solver p that warm-starts from previous() and whose problem reads t and the cells of two toy guards, one on g that reads A's measure and one on not g that reads B's.
- **Sequence:** drag: ticks S1..Sk, each current.edit(t := tick value); p is demanded and runs in each → flip g; p runs and calls previous()
- **What went wrong in the draft reviewed:** 'Last' was undefined across snapshots, and previous() reported changes in reads the new run would not make.
- **What the contract requires:** Tick k's previous() is tick k-1's final result (or the most recent final at that path); after the flip, A's body does not run, and previous() reports as changed only reads the new run makes; answers under a view never become 'last'.

### `previous_found_at_body_declared_child_after_argument_change`

- **Kind of test:** named regression. **Contract:** 7.1, 5.7. **Found by:** round 1, whole design (W2 (warm state kept by path, but expensi).
- **Model:** root/result, a toy `call`, reads input L and declares, via cx.child at a fixed step, a `warm` child N bound to L. Run with L = 1.00, edit L = 1.01; N's body must see previous() = its 1.00 result; then edit back to 1.00: zero N body runs.
- **Sequence:** L = 1.00: root/result declares N; N runs cold and returns final result R1 → edit L to 1.01 → root/result runs again and declares N again → N's body calls cx.previous()
- **What went wrong in the draft reviewed:** The first draft kept warm state by path but named computations by the content of their declaration, so every argument change moved the computation to a fresh path with no history.
- **What the contract requires:** previous() returns R1 (the result for L = 1.00) with its reads; the body can ask which reads changed (L). The new result is marked not deterministic (5.7). Reverting to 1.00 m is a store hit and runs no body.

### `kept_state_reaches_next_run_at_same_path_after_input_change`

- **Kind of test:** named regression. **Contract:** 5.7, 7.1. **Found by:** round 1, geometry and session (G1 (warm state by path; FEA and volume-m).
- **Model:** A, a toy `call`, reads input L and declares a `warm` child N bound to L at a fixed step; N calls cx.keep(L) and on its next run reads previous().state. Edit L 1000 -> 1010: N sees kept state 1000.
- **Sequence:** L = 1000: N runs cold and keeps its state with cx.keep → edit L to 1010 → N misses the store and runs → N's body calls previous()
- **What went wrong in the draft reviewed:** Warm state was kept by path while the computations that use it were roots named by content, which change path exactly when inputs change.
- **What the contract requires:** previous() yields the 1000 result and its kept state; the new result carries a nonce if not hashed by value; the state kept beside the returned result is this run's.

### `returning_the_previous_result_keeps_its_content_and_spares_its_readers`

- **Kind of test:** counter assertion. **Contract:** 5.6, 5.7. **Found by:** round 1, soundness (L22).
- **Model:** F, a test-local variant of the toy `shape` that reads previous and returns it when its input moved less than a tolerance (its result is not deterministic); cell R reads a `measure` of F.
- **Sequence:** S1: evaluate demand {R} → S2 = S1.edit(F's input, within F's tolerance); evaluate; read core.counters()
- **What went wrong in the draft reviewed:** Returning the previous result gave no early cutoff for an opaque kind, because its content was the key it was filed under.
- **What the contract requires:** S2: F's body runs once; the reader's body runs 0 times; the reader's answer is unchanged; F's answer is non-deterministic.

### `warm_started_hashable_result_with_unchanged_value_keeps_content`

- **Kind of test:** counter assertion. **Contract:** 5.7, 5.6. **Found by:** round 4, logic (F10).
- **Model:** Warm-starting toy solver whose answer is unchanged by the edit; cell readers counted.
- **Sequence:** S0: solve returns {u: 5mm} → S1 = edit of an input read through the view; warm solve returns {u: 5mm} again → count body runs of readers of u
- **What went wrong in the draft reviewed:** 5.7 gave every result of a body that used previous a nonce, against 5.2 and 5.6's 'keeps its content'.
- **What the contract requires:** Readers of u: 0 body runs in S1; the solve's result is marked not deterministic.

### `warm_start_never_changes_whether_a_solve_succeeds`

- **Kind of test:** named regression. **Contract:** 5.6, 5.7. **Found by:** round 1, solve (S1).
- **Model:** Toy solver for min (x-2)^2 (x-8)^2 on [0, 10] that descends from its start (previous's point if any, else one derived from the bounds) and decides uniqueness from starts derived from the bounds only.
- **Sequence:** core 1: instance A, cold: demand x → core 2: A with its bounds narrowed to [0, 4]: demand x, so that A's previous lies near 2; then the original bounds: demand x, which core 2 has no result for, so the solve runs from previous → core 3: the same with bounds [6, 10], previous near 8 → core 4: identical instance B with no previous: demand x → compare the outcome variant (value or failure) across all four
- **What went wrong in the draft reviewed:** Warm state by path chose the start; the strictness verdict and value depended on it; the result, a failure included, was stored by content and served to every identical instance.
- **What the contract requires:** For a conforming toy solver: all four runs answer the same outcome variant (here failure 'not uniquely determined'); where the class is success, values may differ only within tolerance. The class rule is an obligation on the kind, tested per kind by solving cold and warm (PRD section 13.2, point 17); the core does not check it.

### `warm_started_trials_file_an_alternative_rested_on_set_beside_the_cold_one`

- **Kind of test:** named regression. **Contract:** 2.2, 6, 5.6. **Found by:** round 1, soundness (L10); round 2 (L10 (Part 1 re-run, judged PARTLY)); round 2 (W8 (Part 1 re-run, judged PARTLY: 'Same ); round 1, whole design (W8 (read order through a view depends on).
- **Model:** Toy solver whose start is previous's point when given, else a fixed cold start, visiting a start-dependent trial sequence; constraint cells c2 and c3 are guarded on x so different trials read different constraints.
- **Sequence:** S1: evaluate demand {solve} (cold, no previous) → S2 = S1.edit(c3's source); evaluate {solve} (warm, previous = run 1) → S3 = S2.edit(revert c3); evaluate {solve} → an identical solve instance with no previous is demanded in S2
- **What went wrong in the draft reviewed:** B2 required the same requests in the same order given the same answers and the store was a decision tree over ordered reads; warm-started trials read in a different order, so one prefix needed two 'next read' entries: an orphaned branch, or a solve that was never reused.
- **What the contract requires:** S2: the solve body runs once and its result is filed beside R1 without displacing it. S3: no solve body run. Every answer equals a fresh evaluation handed the same non-deterministic result. Not determined by the text: which alternative is returned when two apply.

## Leaf ρ: Ownership, trimming and release

### `looking_inside_a_passed_opaque_value_is_a_recorded_read`

- **Kind of test:** named regression. **Contract:** 5.2, 5.6. **Found by:** round 1, soundness (L25).
- **Model:** K, a test-local variant of the toy `shape` in its non-deterministic mode (content: the hash of its key plus a nonce), whose internal order of edges changes with each build; cell b passes K along; a selection cell looks inside it through the toy kernel and names an edge; trimming forced by a small store memory budget. A second run uses K's deterministic mode.
- **Sequence:** evaluate demand {selection} → force K out of the store (small memory budget) → evaluate demand {selection} again, in a new snapshot after an unrelated edit
- **What went wrong in the draft reviewed:** A value could hold a reference to a stored result, and using the referenced result through it was not a recorded read.
- **What the contract requires:** Non-deterministic K: after trim and remake the selection re-runs and names an edge of the new K. Deterministic K: the selection is found and is correct.

### `a_nondeterministic_answer_is_pinned_for_every_reader_in_its_snapshot`

- **Kind of test:** named regression. **Contract:** 4.4, 5.5, 5.6. **Found by:** round 1, soundness (L21).
- **Model:** A `warm` node x (its result is not deterministic); a toy shape made from x; verdict cell reads x; a small store memory budget forces trimming between the two reads.
- **Sequence:** evaluate demand {shape, verdict} with store trimming forced between the two reads, in normal mode and in debug mode
- **What went wrong in the draft reviewed:** The draft said the index was never needed for correctness; without it a non-deterministic result could be trimmed and recomputed between two reads in one snapshot.
- **What the contract requires:** Shape and verdict both read R; debug mode agrees with the index; no R'.

### `remade_nondeterministic_result_gets_new_content_and_downstream_reruns`

- **Kind of test:** named regression. **Contract:** 5.2. **Found by:** round 2 (N5); round 1, whole design (W17 (G0 untestable for exact kinds downs).
- **Model:** nondet toy F (opaque, nonce), deterministic opaque toy shape D reading F, cell I reading D; force-trim F.
- **Sequence:** evaluate demand {I} → drop the snapshot holding F; trim → evaluate demand {I} again
- **What went wrong in the draft reviewed:** The content of a not-exact opaque result was the key it was first stored under, so a remade one took the old content; an exact opaque kind between it and its reader got its old key back.
- **What the contract requires:** I is recomputed from D' (body runs: F 1, D 1, I 1 after the trim); G0's fresh run handed F' gives the same I.

### `results_reached_only_from_trials_are_trimmed_first_and_released`

- **Kind of test:** counter assertion. **Contract:** 5.5, 6. **Found by:** round 1, solve (S16).
- **Model:** Toy solver over t with many trials; each trial reads a constraint cell that reads a shape built from t (with a release counter); small store memory budget.
- **Sequence:** evaluate demand {t, volume} with a small store memory budget; read release calls and live shape count → demand the volume at the solved t
- **What went wrong in the draft reviewed:** Every result computed in a trial was stored, with no trim priority for trial-only results.
- **What the contract requires:** Live shapes stay within the store budget; release calls equal shapes dropped; no dropped shape is one a live index points at; the shape at the solved t is found or rebuilt with equal content.

### `trimmed_shapes_released_once_and_slider_session_stays_bounded`

- **Kind of test:** counter assertion. **Contract:** 5.5, 10, 7.6. **Found by:** round 1, geometry and session (G5 (drop-together rule plus no per-shape).
- **Model:** Chain box(w) -> cut -> fillet: three toy shapes, each made from the one before (test-local variants of `shape`). 300 edits of w, each a new snapshot with its own request; each earlier request is ended and its snapshot dropped.
- **Sequence:** 300 preview snapshots, each producing 3 new shapes → old preview snapshots stop being live → the store trims for space under its memory budget → drain; count the shapes the toy kernel holds and the release calls → end the current request, drop the current snapshot, trim, drain → in a new snapshot with the same inputs, demand the fillet
- **What went wrong in the draft reviewed:** A key-as-content result could be dropped only with its readers, and no rule released a shape at all; reload was the only collector and reload no longer happens.
- **What the contract requires:** After every tick and a drain, the shapes the toy kernel holds are exactly those the store, the running bodies and the test still hold, and the store stays within its memory budget; every dropped shape is released exactly once; a shape shared by many identical instances is released once. After the last trim, with no snapshot live, the kernel holds only what the store still holds; the re-demand remakes box, cut and fillet with the same contents as before, since they are deterministic.

### `every_shape_produced_for_a_dropped_body_is_released`

- **Kind of test:** counter assertion. **Contract:** 2.5, 5.5, 2.2 O4. **Found by:** round 4, logic (F14).
- **Model:** Toy shape kind on an exclusive resource with a live-handle counter and release; drop bodies during and after the section.
- **Sequence:** ticks 1..k, each a new snapshot whose request starts a toy `shape` body that enters an exclusive section → each tick arrives while the previous section runs; the previous request is ended and its body dropped → variant: the section has returned, the body holds the shape and is waiting on another read when it is dropped → drain; count live shapes against stored shape results
- **What went wrong in the draft reviewed:** release was called only for stored results; a section's output for a dropped body leaked.
- **What the contract requires:** After any number of dropped ticks and a drain, live shapes = shapes referenced by stored results; every shape is released exactly once, including one a body held across a later wait when it was dropped (PRD section 13.2, point 23).

### `trimming_a_nonce_result_leaves_its_dependents_reclaimable`

- **Kind of test:** counter assertion. **Contract:** 5.5, 5.2. **Found by:** round 2 (W17 (Part 1 re-run, judged PARTLY)).
- **Model:** Non-deterministic toy shape M; F, a deterministic test-local shape made from M; cell G, a `measure` of F.
- **Sequence:** evaluate demand {G} → end the request and drop every snapshot that indexes them → trim; drain → demand G again
- **What went wrong in the draft reviewed:** The cost of the cascade was not stated; the trimming order ignored it.
- **What the contract requires:** G is recomputed from M' (M, F and G each run once more). After the trim and the drain the toy kernel holds nothing: F's result, whose key holds M's content, was trimmed with M (PRD section 13.2, point 16) and released.

## Leaf σ: Freshness and progress

### `exclusive_section_handle_offers_cancelled_progress_emit_without_waiting`

- **Kind of test:** compile-time check. **Contract:** 2.3. **Found by:** round 4, logic (F16).
- **Model:** Toy shape section that loops, polls cancelled, reports progress and emits.
- **Sequence:** inside an exclusive section, poll cancelled, report progress, emit a partial
- **What went wrong in the draft reviewed:** Signature mismatch.
- **What the contract requires:** A section can see cancellation of its body, report progress and emit a partial without waiting.
- **Note for the leaf:** `cancelled` and `progress` are already in leaf α's signal. This leaf adds `emit`.

### `reader_of_an_emitted_partial_is_recomputed_when_the_input_becomes_final`

- **Kind of test:** named regression. **Contract:** 9, 5.2, 2.3. **Found by:** round 1, soundness (L4); round 1, whole design (W1 (partial result of a key-as-content k).
- **Model:** A `slow` node P publishes a partial 350 and then returns 420. Cell peak reads P; cell verdict = (peak < 400). Drive the executor so peak reads the partial.
- **Sequence:** S: evaluate demand {verdict}; P publishes 350; peak and verdict are computed from it → P returns 420 → S2 = S.edit(an unrelated input); evaluate demand {verdict} → an identical instance of P, peak and verdict is demanded in S2
- **What went wrong in the draft reviewed:** A partial and the final result had the same content (the key), so peak computed from the partial was filed under P's key and served as final in every later snapshot and to identical instances; and nothing re-checked a reader in the same snapshot when its input went from intermediate to final.
- **What the contract requires:** While P is partial: peak = 350 intermediate; verdict true intermediate; nothing stored. After P is final: peak = 420 final and verdict false final, in S, in S2 and in the identical instance (PRD section 13.2, point 12).

### `intermediate_answer_becomes_final_in_its_snapshot_and_descendants`

- **Kind of test:** named regression. **Contract:** 9, 5.7, 4.4. **Found by:** round 2 (N16).
- **Model:** A `slow` node X publishes a partial and then returns a final result; cell R reads X; an unrelated edit falls between the two.
- **Sequence:** S0: a slow node X emits a partial; reader R is shown intermediate → S1 = S0.edit(unrelated) before X finishes → X finishes
- **What went wrong in the draft reviewed:** Unclear which result is 'previous', which snapshots re-check readers of an intermediate, and a blanket 'keeps that answer'.
- **What the contract requires:** R is intermediate in S0 and S1 until X is final, then final in both, equal to a fresh evaluation; previous() never returns an intermediate result.
- **The text does not yet settle this** (partly). See the PRD's list of open points.

### `a_transient_failure_does_not_survive_into_a_later_snapshot`

- **Kind of test:** named regression. **Contract:** 9, 2.3. **Found by:** round 1, soundness (L20); round 2 (L20 (Part 1 re-run, judged PARTLY)).
- **Model:** A `slow` node F whose switch makes its first run fail outside the inputs; cell C reads F and compares it with a bound; then an unrelated input edit.
- **Sequence:** S: evaluate demand {C}; F's run hits a transient failure → S2 = S.edit(unrelated input); evaluate demand {C} → an identical instance of C is demanded in S2
- **What went wrong in the draft reviewed:** 'Nothing is stored' for cancelled, out of memory and budget, but nothing said what a reader of such an answer gets; a reader that returned 'blocked' was a completed run, indexed and inherited by later snapshots.
- **What the contract requires:** S2 and the identical instance: C = the verdict on F's real result; 'blocked' is never found. In S: C's body is dropped, nothing is stored or kept for it, and the host is told the error for C (PRD section 13.2, point 13).

### `progress_reaches_watcher_and_never_changes_result_or_key`

- **Kind of test:** named regression. **Contract:** 2.3, 9, 2.4. **Found by:** round 1, geometry and session (G11 (progress has no channel)).
- **Model:** A `slow` node calls cx.progress(i) each iteration. Core 1: a request demands its path and the host takes the request's events. Core 2, fresh: the same request, and the host never takes events.
- **Sequence:** body runs an iterative loop → every stride it reports {iter, residual} → on core 1 the host takes the events → body finishes
- **What went wrong in the draft reviewed:** The only in-run output was emit(partial: Out), which would make each tick a distinct intermediate result; the thread-local sink was forbidden.
- **What the contract requires:** Core 1's host receives the node's progress events; the result and its content are identical in both cores; store lookups and index entries touched are equal in both. Progress goes to every live request whose demand needs the path (PRD section 13.2, point 14).

## Leaf τ: Scheduling and budgets

### `cold_reads_between_nodes_of_one_kind_do_not_deadlock_and_identical_runs_are_shared`

- **Kind of test:** named regression. **Contract:** 10, 7.7. **Found by:** round 4, logic (F6).
- **Model:** Input width = 50; cells w_eff = min(width, 40) + k and area = w_eff * 10, where k is a `measure`, answering 0, of a toy `shape` whose section waits on a barrier while the test holds it. Two identical toy scopes, each declaring these cells.
- **Sequence:** cold store: evaluate demand {root/area} → fresh core, cold store: evaluate the two identical scopes together; count body runs → fresh core: S1 = snapshot; hold the barrier; evaluate {area} on S1; one `turn()` → S2 = S1.edit(an input area does not read); evaluate {area} on S2: its lookup finds S1's run in progress and waits for it → end S1's request; release the barrier; run until idle
- **What went wrong in the draft reviewed:** 'Two lookups that miss at the same point do not both run; the second waits' matched any two nodes of one kind at the recipe hash, with no exception when the run waits on the lookup.
- **What the contract requires:** area = 400, no 'cycle'; identical scopes: one body run per node; the run S2 waits on is not dropped when S1's request ends, and S2's area = 400.

### `budget_exhaustion_is_one_request_error_and_stores_nothing`

- **Kind of test:** named regression. **Contract:** 7.6. **Found by:** round 1, claims (V2 (node budget exhaustion prints 34 err).
- **Model:** T is a toy `guard` with bindings d and live: it reads live first and answers absent when it is false; otherwise it declares cells m = d - 1, l = (d > 0) and sum = (if l then left/sum + right/sum else 1), and two children left and right, each a T whose binding d is the place m and whose binding live is the place l. Root is T with d = 20 and live = true. Request A demands root/sum with a budget of 10,000 paths and fails with one error. Request B, with an ample budget, live at the same time and sharing work, completes.
- **Sequence:** evaluate requests A and B together → A's budget is exhausted at the frontier → A's one error is reported; B goes on
- **What went wrong in the draft reviewed:** Today's engine reports one error per frontier node (34) and a partial tree; the reviewed draft misquoted the count. The core question is what the new core reports.
- **What the contract requires:** One request-level budget error; no per-path failures stored; a second request with a sufficient budget succeeds with the same body-run counts as a cold one minus store hits; a concurrent second live request's shared work is not stopped.

### `solver_budget_exhaustion_is_stored_and_request_budget_exhaustion_is_not`

- **Kind of test:** named regression. **Contract:** 7.6, 9, 5.6. **Found by:** round 1, solve (S12).
- **Model:** Toy solver with an iteration-limit setting it reads; a problem that needs more iterations than the limit from one start and fewer from another.
- **Sequence:** instance A with previous (warm): demand solve → identical instance B, cold: demand solve → a request with a tiny time budget: demand solve; then a request with ample budget
- **What went wrong in the draft reviewed:** The draft did not distinguish a solver's own iteration budget (a function of problem and start) from the request budget, and let a warm start decide the former.
- **What the contract requires:** A and B give the same outcome variant; a solver's own budget exhaustion (from derived starts) is stored and found by identical instances; the request-budget run stores nothing and the ample-budget request answers the real result.

## Leaf υ: What an answer read, and what read it

### `read_all_of_child_declarations_starts_every_child_before_waiting`

- **Kind of test:** named regression. **Contract:** 2.3. **Found by:** round 4, logic (F21).
- **Model:** A toy `call` declares three shape children in one read_all, each on a resource of its own: three toy kernel types (a resource is keyed by its Rust type, PRD decision D6; for example one kernel type with a const-generic parameter), each with its own shape kind.
- **Sequence:** a body declares three children in one request → measure whether all three child bodies start before any finishes
- **What went wrong in the draft reviewed:** cx.child was the only way to declare and it waited per child.
- **What the contract requires:** All three children are started before the parent's single wait completes; the parent body waits once; the chain records them in the order given.
- **Moved here from leaf θ when the tasks were filed (2026-11-25).** The last clause can be seen only through `core.reads`, which leaf υ adds. Leaf θ writes the part before it, under a name of its own.

## Findings with no test in this crate

- Evaluator adapter declares children from placeholders: the rule for when the expression evaluator's adapter may declare a child: the adapter is not in this crate (seam: instance kinds).
- G4 passes while the evaluator's adapter work grows quadratically: the adapter's own counter: the adapter is not in this crate (seam: instance kinds).
- Bridged solver library threads from a bounded pool deadlock under nesting: a bridged solver library: the bridge is not in this crate (seam: solve kinds).

## Added in review

These have no critic report behind them. Each was found by the review of the PRD, or by the session designing the instance kinds, and is listed in PRD section 13 where a contract point is open.

### `child_appearing_at_a_path_already_answered_absent_is_found`

- **Kind of test:** named regression. **Contract:** 7.1, 4.4. **Leaf:** μ₂. **Found by:** the critic of the PRD.
- **Model:** `ok = if flag then X.peak < 400 else true`, where `X` is a child the body of `ok` declares at a call-site step and `X.peak` is 300.
- **Sequence:** flag false; demand `ok` and the child's path (answered "absent") → edit flag to true; evaluate the same demand.
- **What goes wrong without a rule:** `ok` runs, declares the child and returns true. Its content is unchanged, so nothing downstream is looked at. Its earlier answer declared no child, so no child is re-checked. The child's path stays "absent".
- **What the contract requires:** the child's path answers the child's result, as a fresh evaluation gives.
- **The text does not yet settle this.** PRD section 13, point 9.

### `repointing_a_passed_along_binding_reaches_the_child`

- **Kind of test:** named regression. **Contract:** 2.1, 4.4. **Leaf:** μ₁. **Found by:** the critic of the PRD.
- **Model:** root `R` declares scope `D` with binding `b` set to `R/p1`. `D` declares cell `C` with binding `x` set to `D`'s own binding `b`. `p1 = 5`, `p2 = 7`. `C` returns what it reads through `x`.
- **Sequence:** demand `C` (5) → edit `R`'s source so that `b` is set to `R/p2`; evaluate.
- **What goes wrong without a rule:** `D` is re-checked, read only its own source, and has unchanged content. `C`'s own declaration and `p1` are unchanged. `C` stays 5.
- **What the contract requires:** `C` answers 7. Turning `x` into a path is a recorded read of `D`'s declaration, so `C` is re-checked when that declaration changes.
- **The text does not yet settle this.** PRD section 13, point 30.

### `stored_solve_is_not_found_when_a_rested_on_member_comes_to_depend_on_a_substituted_path`

- **Kind of test:** named regression. **Contract:** 6. **Leaf:** κ. **Found by:** the critic of the PRD.
- **Model:** cell `n = 4`, reading an input. `z = if g then 5 else n + 1`. `c = (z <= 3)`. A toy solve tries `n := 1..8` through a view and returns the first `n` for which `c` is true.
- **Sequence:** `g` true; demand the solve (every trial sees `z = 5`; it fails; `c` is recorded as rested on, false) → edit `g` to false; demand the solve.
- **What goes wrong without a rule:** outside the view `z = n + 1 = 5` and `c` is false, the same content as recorded, so the stored failure applies.
- **What the contract requires:** the solve runs again and returns 1.
- **The text does not yet settle this.** PRD section 13, point 8.

### `inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution`

- **Kind of test:** named regression. **Contract:** 6, 7.7. **Leaf:** λ. **Found by:** the critic of the PRD.
- **Model:** an outer solve over `w` and `v`, one of whose constraints reads the inner solve's unknown, so that the inner solve is evaluated under each outer trial. The inner solve reads `width = w` before opening its view. Through its view, one of its constraints reads a cell that depends on `v`, which nothing it read before its view depended on.
- **Sequence:** demand the outer unknowns first, then the inner one, on a fresh core; then the other order on another fresh core.
- **What the contract requires:** no cycle, and the same answers in both orders. Asked for at the top, the inner solve's read of `width` makes the outer solve finish first; under an outer trial the inner solve follows it.
- **The text does not yet settle this.** PRD section 13, point 6.

### `inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve`

- **Kind of test:** named regression. **Contract:** 6. **Leaf:** λ. **Found by:** the session designing the instance kinds.
- **Model:** an inner scope with input `len` bound from outside, unknown `k`, and constraints `k == 2` and `len + k == 50`. An outer scope whose unknown is that `len`. The inner solve reads `len` before its view. The outer solve's trials read the inner unknown and treat a failed inner solve as a trial that does not hold.
- **Sequence:** demand `len` then `k` on a fresh core; then `k` then `len` on another.
- **What the contract requires:** `len = 48` and `k = 2` in both orders. The inner solve is infeasible under every outer trial but one; that is not a cycle and not a silent wrong answer.
