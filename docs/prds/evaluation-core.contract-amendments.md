# Evaluation core contract: amendments made with the PRD

**Status:** approved by Leo on 2026-11-25 ("LGTM" to the PRD's decision D10; amendment 24 separately, as decision D11) and applied to the contract. Kept as the record of what changed and why. Number 9 was not proposed and is left to a leaf; number 15 is covered by 23. Each closes a point that the contract's text leaves open (PRD section 13.2). Amendments 8a to 21 were found by tracing the critics' counter-examples through the text; 23 to 29 by compiling the body interface; 30 to 32 by the Instance design session and the critic of the PRD. None changes a decision recorded in Part C of the contract. Numbers match PRD section 13; the points of 13.1 are left to the leaves and have no entry here. This file has had one adversarial review, which withdrew or narrowed five of its entries; a second check then changed six more in small ways. The forms here have been checked only by their author since.

When approved, each is applied to both copies of the contract (`contract-draft.md` and `evaluation-core-contract.md`) and listed in the contract's amendment log.

Each entry gives the section, the text as it stands, the text proposed, and the model that showed the gap.

---

## 8a. A read of a path whose own body is running (7.7)

**As it stands:** "It is found on the waits: a body that is waiting counts as in progress. A chain of waits that passes through a lookup is not a cycle: that lookup misses and its body runs. Only a ring of running bodies is a cycle."

**Proposed:** "It is found on the waits: a body that is waiting counts as in progress. A path that is being answered in a snapshot has at most one run at a time, and a read of that path waits on that run. A chain of waits that passes through a lookup is not a cycle: that lookup misses and its body runs. Only a ring of running bodies is a cycle."

Nothing else in 7.7 or in section 10 changes. An earlier form of this amendment also narrowed the word "lookup"; the critic of the PRD showed that would drop the rule for a lookup that waits on a read leading back to it, so it was withdrawn.

**Model:** `x = y + 1`, `y = x + 1`, nothing stored. Read one way, both answer "cycle". Read the other way, each read of the other is a lookup that misses and runs the body again, without end.

## 9. A child whose parent's new answer no longer declares it, or newly declares it (7.1)

Not proposed now. It is point 9 of PRD section 13.1 and is left to leaf μ₂. An earlier form proposed re-checking "every child that the earlier answer recorded declaring"; the critic of the PRD showed that misses a child that appears at a path already answered "absent" while its parent's content is unchanged.

## 10. The same step declared twice in one run (7.1)

**Proposed, added after "at a step it chooses":** "Within one run, declaring a step again with the same declaration names the same child. Declaring it with a different declaration ends the run with a failure."

**Model:** a body that declares a child in a loop and builds the step without the iteration index.

## 11. Which view answers depended on a substitution (6, 2.3)

**As it stands (6):** "The core tracks which answers depended on a substitution, and the caller can ask."

**Proposed (6):** "The core tracks which answers depended on a substitution. A read through a view returns that fact with the outcome."

**Proposed (2.3):** a view's `read` and `read_all` return, with each outcome, whether it depended on a substitution of that view.

**Model:** a solve that must tell a constraint its trials can move from one they cannot. The body interface of 2.3 has no call for it.

## 12. A partial result and its readers (5.2, 9)

**As it stands (9):** "An answer computed from an intermediate input is shown and marked intermediate. When the input becomes final, its readers are checked again, in that snapshot and in any made from it."

**Proposed (9):** "An answer computed from an intermediate input is shown and marked intermediate. It is never made final by comparison. When the input becomes final, every reader that read it while it was intermediate is answered again from the start, in that snapshot and in any made from it."

**Proposed (5.2), a new last bullet:** "**A partial result** has no content. Nothing is filed under one."

**Model:** an FEA whose result is not hashed by value publishes a partial field; a reader computes a peak of 350 MPa from it. The final field gives 420 MPa. If the partial took the kind's content (the hash of the key), it would equal the final's, a re-check by content would pass, and 350 MPa would become final.

## 13. Reading something that failed for a reason outside the inputs (9)

**As it stands:** "A failure that does not follow from the inputs is not an outcome: cancelled, out of memory, a request's budget exhausted. Nothing is stored for it. Nothing is stored or kept, either, for anything that read it, directly, through other nodes or through a view."

**Proposed, added:** "A read of something that failed this way does not return: the reading body is dropped. A body whose own work fails this way (a section that ran out of memory) returns that in place of an outcome. Each demanded path that needed it is reported to the host as pending, with the error; the other paths of the request are not affected, unless the failure was the request's own budget."

**Proposed (2.2):** what `run` returns is an outcome with its determinism claim, or a failure outside the inputs.

**Model:** an FEA runs out of memory; a constraint reads it. `cx.read` returns an outcome, and the text says this is not one. What the constraint's body receives, and what the host is told, are not stated.

## 14. Watching progress (2.4, 9)

**As it stands (9):** "**Progress** (an iteration count, a residual) goes through `cx.progress` to whoever watches that path and is kept nowhere."

**Proposed (9):** "**Progress** (an iteration count, a residual) goes through `cx.progress` to every live request whose demand needs that path, and is kept nowhere."

**Proposed (2.4):** a request (amendment 32) receives answers as they become available, and progress events for every path its demand needs, each tagged with its snapshot.

**Model:** the host interface of 2.4 has no way to watch a path.

## 15. A value produced in a section and still held when the body is dropped (2.5)

Covered by amendment 23.

**Model:** a body makes a shape in one section, then waits on a read, and is dropped while waiting. Each tick of a slider would leave a shape in the kernel.

## 16. When the store trims (5.5, 7.6)

**As it stands (5.5):** "After that it stays until trimmed for space, and results reached only from a solver's trials are trimmed first."

**Proposed (5.5):** "After that it stays until trimmed for space, and results reached only from a solver's trials are trimmed first. The store trims when a request's memory budget (7.6) would otherwise be exceeded. That request ends with an error only when trimming everything that may be trimmed is not enough. A result that can no longer be found, because its key holds the content of a trimmed result that carried a nonce, is trimmed with it."

The sentence before it ("A result is kept while ... another stored result refers to it") is unchanged. An earlier form of this amendment changed "refers to it" to "was computed from it"; the critic of the PRD showed that contradicts 5.2, which expects a result to be trimmed and made again while results computed from it remain.

**Model:** three hundred ticks of a slider, three shapes each. Whether memory stays bounded is not fixed by the text. Separately: a mesh carrying a nonce is trimmed while a field computed from it stays, and can never be found again.

## 17. A warm start and whether a solve succeeds (5.6)

**Proposed, added to the second bullet of the list:** "The core cannot check this from outside. It is tested per kind, by solving cold and warm."

## 18. A result changed through a kernel after it was returned (O6)

**Proposed, added to O6:** "The core cannot enforce this. Evaluating the same demands in another order, on a fresh core, exposes it."

**Model:** a reader tessellates a stored shape in place at a fine tolerance; a later export at a coarse tolerance then depends on that earlier view.

## 19. Wording

- **4.4.** "An answer given in a snapshot is that snapshot's answer for good" becomes "A final answer given in a snapshot is that snapshot's answer for good". An intermediate answer becomes final later in the same snapshot.

## 20. Settings of the core itself (7.6)

**As it stands:** "The core holds no limit as a constant. There are two sorts, and they live in different places."

**Proposed, added after the two sorts:** "**Settings of the core itself** are given when a core is built: how many alternative records of what a view-opener's trials rested on the store keeps for the same earlier reads (section 6), and how many of the bodies that claimed determinism the debug mode runs again (5.6). They never change an answer that is deterministic."

**Why:** section 6 says "The store may keep a few alternatives", and 5.6 says the debug mode "re-runs a sample". Each is a number, and neither is an answer-changing limit nor a budget of one request.

## 21. The debug mode and O2 (5.6)

**Proposed, added after "re-runs a sample of bodies that made one and compares the bits":** "The same re-run is given the answers the first run was given, and fails if the body makes different requests (O2)."

## 22. The provisional list, the amendment log, and who removes the status (header of the repository copy)

The repository copy gains an explicit list of provisional sections and an amendment log, both empty of history at landing. Its header sentence "The provisional mark on a section is removed by the PRD leaf that makes its tests pass" becomes "The provisional status is removed by the PRD's closing leaf, when every leaf has landed": later leaves keep exercising each of the six sections, so no mark comes off early. Leaf ξ₃ (the last of the three mechanisms) and leaf φ each record a wide run of the random-sequence test in the amendment log.

---

# What the Rust probe forces

The contract says of its pseudo-code: "Rust-shaped pseudo-code. Names are placeholders." These are the places where more than a name changes. The probe is described in PRD section 5.2.

## 23. Release (2.2, 2.3, 2.5, 5.5)

**As it stands (2.2):** "// Release what a result owns outside ordinary memory (a kernel shape)." followed by "`fn release(out: Self::Out, resources: &mut Resources);`"

**As it stands (5.5):** "When a result is dropped the core calls the kind's `release`."

**Proposed (2.2):** the `release` function is removed from what a kind supplies.

**Proposed (2.3), added to what a section's handle offers:** "`hold(thing)` wraps a thing that lives inside the resource. When the last holder of the wrapper lets go of it, the thing is handed back to that resource's thread to be released."

**Proposed (5.5):** "What a result holds inside a resource is released, on that resource's thread, when the last holder of the result lets go of it: the store, a body that read it, or a host that kept an answer."

**Proposed (2.5):** "If it is dropped while a section it gave to `exclusive` is running, the core releases what that section produces" becomes "If it is dropped while a section it gave to `exclusive` is running, or while it still holds something a section made, that thing is released as 5.5 says."

**Why:** a result is shared between the store and every body reading it, so the core cannot hand it to `release` by value; and release has to run on the resource's thread. With one mechanism, three cases are covered that the text covers unevenly: a result the store drops, a section whose body was dropped, and a body dropped later while still holding what a section made.

**Proposed (5.5), two more sentences:** "The store owns results" becomes "The store owns its entries; a result is shared with whoever is reading it". "A shape's lifetime therefore belongs to its store entry" becomes "A shape therefore lives until its store entry is dropped and no reader or host still holds the result".

**Proposed (5.7), added:** "The pointer the core keeps for `previous` does not keep a result alive. If the result was trimmed, `previous` gives nothing."

**Proposed (2.2), a new obligation:** "**O10. What it makes inside a resource and keeps, it wraps.** The core cannot check this."

**What changes in behaviour:** release happens when the last holder lets go. That can be later than when the store drops its entry: a host that keeps answers, or a body that waits for a long time, keeps kernel memory that trimming and a request's memory budget cannot reclaim. The memory budget counts what the store holds.

## 24. Where a root's declaration comes from (2.1, 3.1, 7.5)

**As it stands (7.5):** "**A root is named by the session**: a module and a definition. Two definitions with the same text are two roots that share their results."

**Proposed (7.5), added:** "A root's declaration is a snapshot input. A demand names the root; a name with no such input answers \"absent\"."

**Proposed (3.1), added to the list of inputs:** "**Roots.** One input per root: its declaration."

**Why:** the contract does not say where a root's declaration comes from. Any other source (a call that registers roots on the core) would let an answer depend on something outside the snapshot. What roots exist is still for the instance design.

## 25. A read at the wrong type (2.3)

**Proposed, added:** "A read names the type it expects. If the answer is a value of another type, the reader is given a failure that names both."

## 26. Seeding (2.4)

**As it stands:** "`core.seed(key, result)`  // tests only: start a store holding given results (used by G0)"

**Proposed:** a core may be built on a store that already holds given results, and a core's results can be handed to such a store (both used by G0). Nothing is seeded into a core after its first evaluation.

## 27. Whose thread runs the bodies (10)

**As it stands:** "**First executor: one thread runs every body.** A body runs until it waits; then the next ready body runs."

**Proposed, added:** "The core starts no thread for this. The host calls the core to take a turn, and the calling thread runs the bodies. The only threads the core starts are those of resources."

## 28. A body that visibly breaks O2 (2.2)

**Proposed, added to O2:** "A body that makes a second request before the first is answered ends with a failure."

A body that waits on something other than the core is also caught, but whether it is caught depends on timing, so that failure is one outside the inputs (section 9) and is not stored.

## 29. What the compiler checks of O4 (2.2)

**As it stands:** "**O4. It holds nothing shared while it waits.** A kernel is lent only inside a synchronous closure, and a body cannot wait inside one. The compiler checks this."

**Proposed:** "**O4. It holds nothing shared while it waits.** A kernel is lent only inside a synchronous closure, and a body cannot wait inside one. The compiler checks that the closure does not wait, that nothing borrowed from the kernel leaves it, that it does not take the body's handle with it, and that a body holds across a wait nothing that cannot move between threads, such as a standard lock guard. Two things the compiler cannot check are obligations: a resource type does not hand out shared ownership of itself, and a section does not block on anything that a body or the host would have to supply."

## 30. A binding can be passed along (2.1)

**As it stands:** "Each is: a place relative to the node that declared this one; a constant; a snapshot input; or a snapshot input attached to this node's own path."

**Proposed:** "Each is: a place relative to the node that declared this one, which is that node's own place or one of its own bindings, followed by steps down; a constant; a snapshot input; or a snapshot input attached to this node's own path. The declaring node does not read a place by binding a child to it."

**Proposed (4.2), added where the routine reads a declaration:** "Turning a ref into a path reads each declaration it passes through. Those reads are recorded in the index entry, with the reads of 4.2, and are not part of the key."

**Why the second sentence:** without it, re-pointing a passed-along binding leaves the child stale. The declarer that passes the binding on reads nothing and keeps its content, and the child's own declaration is unchanged (test `repointing_a_passed_along_binding_reaches_the_child`, leaf μ₁).

**Why the first:** asked by the Instance design session (their request 1), and needed by the toy kinds too. A parameter evaluates its argument in the scope that wrote it, so an instance must pass on a binding it was given. The core turns the binding into a path when it resolves the child, so the declarer reads nothing, and the child's declaration still holds no path.

## 31. A view inside a view: the case the text means to allow (6)

**As it stands:** "Anything it read before opening it is simply an input: if that input depends on the outer substitution, the node is a different computation under each outer trial, and it follows the outer one. But if, through its own view, it reads a place whose answer depends on the outer substitution, the two view-openers depend on each other, and that is a cycle (7.7)"

**Proposed:** "Anything it read before opening it is simply an input: if that input depends on the outer substitution, the node is a different computation under each outer trial, and it follows the outer one, and the answers do not depend on which of the two is asked for first. If neither of two view-openers took in anything the other decides before opening its view, and each reads the other's unknown through its own view, they depend on each other, and that is a cycle (7.7)"

**Status:** this states the property only, and names the one case that is certainly a cycle. Whether a view-opener that took in part of an outer substitution may meet another part of it through its view is left to leaf λ; the test `inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution` requires that it may. The rule the core uses to tell the cases apart is point 6 of PRD section 13.1 and is settled by leaf λ under test. An earlier form of this amendment gave a rule (dependence per part of the outer substitution); the critic of the PRD showed that rule gives a cycle where none is needed, so it was withdrawn.

**Model:** `let c = Slot(width: w)`, where `w` is an unknown of the enclosing instance and `Slot` has an unknown `depth`. `Slot`'s solve reads `width`, opens its view, and its constraint `depth >= width` reads `width` again through the view. Read to the letter, today's text makes that a cycle. Found by two critics and, separately, by both designers of the instance kinds.

## 32. What makes a request and a snapshot live (2.4, 4.4)

**As it stands (2.4):** "`core.evaluate(snapshot, demand, budget)    -> answers as they become available, each tagged with its snapshot`"

**Proposed (2.4):** `core.evaluate(snapshot, demand, budget)` returns a request. Answers and progress arrive for it as they become available. The host can replace a request's demand and can end it. Several requests may be live at once. `core.trim()` asks the store to trim now.

**Proposed (4.4), added to the first bullet:** "A request is live from `evaluate` until the host ends it or a budget does, whether or not everything it demanded has been answered. A snapshot is live while the host holds it or a live request is on it. What a new snapshot inherits is taken when the first request is made on it, from what is kept at that moment on the snapshot it was edited from. Ending that snapshot's requests afterwards drops only the work nothing else needs."

**Why:** 4.4 ("what the live requests on a snapshot demand"), 5.5 ("a live snapshot's index") and 10 ("a snapshot nobody wants any more") all rest on what "live" means, and the text does not say. If a request ended when its evaluation completed, nothing would be kept at the next edit and nothing would ever be inherited. And a host following section 10 (abandon the earlier tick) must be able to do so without losing what the new tick inherits: it makes the new request first.

---

# Recorded, not proposed here

- **Section 8, "It also means two solves never nest through the core"** contradicts the last bullet of section 6 as the PRD's leaf λ builds it. It is the solve's text, and is restated when Leo has ruled on the Instance design session's question about how groups are formed (below).

## Applied on Leo's rulings in the instance design session

Three changes to text about node kinds, asked for by the session that designed the instance kinds. Leo ruled on each there on 2026-11-25 ("LGTM" to that design's decisions 1, 6 and 3), and they are in the contract:

- **Section 8, "Who decides an unknown".** "Every constraint that reads the result bears on it, wherever that constraint lives" became: the `auto` belongs to the instance of the definition whose text contains it; every constraint inside that instance that reads the result bears on it; a constraint outside it is checked against the result and cannot change it. Example 11's matching sentence follows.
- **5.4, "Values", with 5.3.** A structure value is its type's qualified name and type arguments, the value of every parameter that is present, and every value decided at or below its instance, by relative path. So a definition's name is in the content of its values, and in a definition's list of members where a sub or a constructor call names its definition; 5.3 now says so. Renaming a definition changes the content of its values and of what holds them. It does not change shapes, meshes or anything that holds no such value.
- **7.3 and Example 3.** A guarded member reads its own guard first: absent under a false guard, a failure "presence undetermined" under an undefined one. A guarded sub is the instance itself; there is no separate node in front of it. The instance that contains it reads no guard.

- **Example 8** (2026-11-26). The list of refused gestures no longer has "an instance built inside an expression" or "a value given by `auto`". Leo: "I don't see why this should be refused", then "Agreed" to dropping the entry. The example now states his rule for which parameters get a direct control (every one unless what is written for it where the instance is declared is a formula; a literal is edited in place, `auto` and `undef` are literals reached by a toggle, and where nothing is written the control adds an argument and leaves the definition's default alone), and the list reads: a formula argument, an element of a list collection, a definition in an imported module, a value whose unit cannot be written back exactly, a required parameter of a root.
- **7.5 and 3.1** (2026-11-26). A root's declaration names its definition and carries no arguments. Leo, asked whether it could carry a value for a root's required parameter: "No. This adds a semantically critical value that lives outside the source. The source must remain the canonical sole definition of the design. If the source can't represent it we either improve the expressive power of the source (judiciously - the bar for expanding the language should be high to prevent accretion) or we refuse it." This bounds amendment 24; it is the first row of the contract's amendment log.

- **Purposes: section 8, 3.1 and Example 9** (2026-11-26). Leo reversed the purpose half of his ruling of the day before. A purpose only checks ("the purpose is a predicate that checks that a design is fit for a particular purpose"); an objective written in a purpose is a compile error; a purpose is declared and then applied in source ("a declare then apply pattern (similar to the declare then instantiate for structures), so that the source can express application"), and a session's toggle is the uncommitted form of that edit. A constraint in the braces after a sub still bears on that instance's own `auto`. In the contract: section 8 no longer counts a purpose's constraints as inside an instance and says a purpose only checks; 3.1 no longer lists which purposes are active among the settings; Example 9 is rewritten around a purpose applied in source. Second row of the amendment log.
- **2.1 and 7.3: no input is attached to a path** (2026-11-26; ruled in the session that owns the core's PRD). 2.1 listed "a snapshot input attached to this node's own path" as a kind of binding, and 7.3 spoke of "the settings attached to its path". Purpose activation was the only use the instance kinds had for it, and that is now source; it also let a session change an answer at a place from outside the source. Asked whether to remove it from the core, Leo: "do delete it". 2.1 now lists three kinds of binding and says "No input is attached to a path"; 7.3 says an instance reads only its own presence guards and the source of its definition. This narrows amendment 30. Third row of the amendment log. Task #10617 removes the kind from the crate and its test bed; the PRD's toy `scope` (5.4) and the catalogue entry `equal_roots_share_results_but_not_place` are restated without it (PRD decision D13).

## Still requested by the Instance design session, not applied

- **Section 8, the third planning rule**: "Groups whose problems depend on each other's unknowns are one group" conflicts with the ownership ruling when the unknowns have different owners. Their model is `Beam { param len; param k = auto; constraint k == 2mm; constraint len + k == 50mm }` under `Asm { sub a : Beam { len = auto } }`, with answer `len = 48 mm`, `k = 2 mm`. They state the required answer and leave the method to the solve design. The core carries the model as a test of nested views (`inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve`, leaf λ).
- **Example 5's hand-down node stays.** They withdrew the request to remove it. How objectives travel is the solve design's.
- **A constraint in a sub's braces bears on that instance's own `auto`.** Leo ruled in that session on 2026-11-25 that a constraint in a sub's braces, and a purpose's constraints, bear on the instance's own `auto` ("this is what aligns best with the purpose of the braces on a sub: customizing the instance that is being created"). The purpose half was reversed the next day (above, "Purposes"), and Example 9 was rewritten with it. The braces half stands: section 8 says what counts as inside an instance, and its first bullet says "by the constraints inside that instance" where it said "by the definition's own constraints".
- **2.5, the expression evaluator's adapter**: only the condition of an `if` or a `match` decides whether something is read; both operands of `and`, `or` and `implies` and every element of a quantifier are read. That agrees with 2.5 as written, so nothing changes in the contract. Its cost is in the language: a ring of dependencies through an operand that could not affect the answer is a cycle. They are putting it to Leo. The adapter is not in the core crate.
- **A read of something that is not there** gives undefined with a cause, an index out of range included. Leo ruled this in their session; it changes the language spec's section 3.4, not the contract. For the core, such a path answers "absent", and a trial that reads past the end of a collection whose count is on trial learns from the view whether that answer depended on the substitution.
