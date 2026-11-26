# Reify evaluation core: contract

**Status:** PROVISIONAL. This is the contract of the new evaluation core (`docs/prds/evaluation-core.md` builds it). It succeeds sections 2 to 6 of `docs/reify-implementation-architecture.md`, which describe a design that was never built in that form.

**What "provisional" means here.** The rules of sections 4.4, 5.7, 6, 7.1, 7.6 and 7.7 were last changed after their most recent review and have been checked by hand only. The PRD builds them first, under a test that compares every incremental answer with a fresh evaluation over random sequences of edits and trials (invariant G0, section 11). Where that test contradicts this text, the text is amended in the same change that fixes the code. The provisional status is removed by the PRD's closing leaf, when every leaf has landed.

**Provisional sections:** 4.4, 5.7, 6, 7.1, 7.6, 7.7.

**The crate** that implements Part A is `reify-eval-core`.

**Statements about the engine as built** ("today ...", "run: ...") are provenance, not maintained claims. They were read at main `6ef80d5e32` (2026-11-24), or observed with a command-line binary built about 170 commits earlier.

- **Part A** is the contract.
- **Part B** is eleven worked examples. The node kinds in them are illustrations, not part of the contract.
- **Part C** records the rulings this contract rests on, and what is left to later designs.
- **Part D** lists what was left out on purpose, each with what should prompt taking it up.
- **Part E** says where the contract follows the original design and the literature, and where it departs.

**Words.**

- **Node**: one computation at one place.
- **Path**: where a node is. A root, then steps down from it.
- **Root**: a starting place, named by the session: "definition `Bracket` of module `m`, viewed as a design".
- **Recipe**: what kind of computation a node is. It says nothing about where.
- **Declaration**: what a parent says about a child: a recipe plus bindings.
- **Binding**: one named input of a node. It points at a place relative to the declaring node, or carries a constant, or names a snapshot input.
- **Outcome**: what a computation returned: a value, a failure, or "absent".
- **Result**: an outcome that has been stored.
- **Content**: a hash that stands for a result.
- **Key**: what a result is stored under.
- **Deterministic**: said of a result that is a function of its key alone, bit for bit.
- **Snapshot**: an immutable set of inputs.
- **Store**: results by key. One store, shared by every snapshot.
- **Index**: per snapshot, the answer at each path asked for, and what it read.
- **Answer**: what the core returns for a path in a snapshot: an outcome and how fresh it is.
- **Unknown**: something whose value a solver chooses (`auto`).
- **Trial**: one candidate assignment of values to unknowns, tried by a solver.
- **View**: the state as it would be under a trial. Section 6.

---

# Part A. The contract

## 1. The model in one page

1. **There is one mechanism.** Every computed thing is the result of a node: a value, a verdict, the set of nodes inside an instance, a solve, a shape, a mesh, an FEA field. The core runs, stores and invalidates all of them the same way. It has no list of node kinds and does not depend on the language.

2. **A snapshot holds inputs and nothing else.** Compiled source, settings, the content of imported files. It is immutable. Nothing computed is ever written into a snapshot.

3. **A node body learns things only by asking the core, and waits for the answer.** Every read is recorded. What a node read is its dependency list. Nothing else is.

4. **A result is stored under what was computed and the content of what was read.** Not under the path and not under the snapshot. Two instances that compute the same thing from the same contents find the same stored result. So do two snapshots.

5. **A result never says where it is.** It refers to other things only relative to itself. Paths are attached when a result is shown.

6. **Nodes create nodes.** A result declares children; a running body can declare a child too. A path is resolved by reading its parent. Nothing is elaborated until a path through it is asked for.

7. **There is one entry point.** A consumer hands the core a snapshot and a demand: the paths it wants, each with a priority. The core brings up to date those and what they read.

8. **A solver's "what if" uses the same engine.** A view substitutes values at some paths; reading through it is ordinary evaluation.

9. **Two identities, each with its own job.** The path does the jobs that need a place: looking something up, selection, display, finding what was there last time, and being the thing a solver chooses. The content decides whether work can be reused. Every node has both.

## 2. Nodes

### 2.1 What a node is

A node is a **path**, a **recipe** and **bindings**.

- The **path** is a root followed by steps: `root / bracket / holes / [2] / diameter`.
- The **recipe** is the node's kind, the version of that kind's code, and the versions of any engine outside the language that the kind calls. It is hashed. It contains no path, no name of a definition and no source text.
- The **bindings** name the node's inputs. Each is: a place relative to the node that declared this one, which is that node's own place or one of its own bindings, followed by steps down; a constant; or a snapshot input. No input is attached to a path. The declaring node does not read a place by binding a child to it. Whoever declares a node sets its bindings. A body cannot see its bindings except by reading through them, and reading a binding that was not set answers "absent".

A node's source (an expression, a constraint body, an operation list) is not in the recipe. It is a snapshot input the node reads through a binding. That makes a source edit and a value edit the same event.

### 2.2 What a node kind supplies

Rust-shaped pseudo-code. Names are placeholders.

```
trait Kind {
    type Out;                          // this kind's result type

    const ID: KindId;                  // both enter every key of this kind
    const VERSION: u32;                // bumped when the body's meaning changes

    // The body: a task. It waits wherever it reads.
    // It returns its outcome and says whether that outcome is deterministic (5.6),
    // or returns a failure outside the inputs (9).
    async fn run(cx: &Cx) -> Produced<Self::Out>;

    // The content of a result, where hashing it is cheap. None: not sensibly hashable (5.2).
    fn content(out: &Self::Out) -> Option<Hash>;

    // The children this result declares, if any (section 7).
    fn declares(out: &Self::Out, step: &Step) -> Option<Declaration>;
    fn steps(out: &Self::Out) -> impl Iterator<Item = Step>;

}
```

A kind that computes a plain value from an expression supplies `run` and `content`.

**Obligations on a body.** These are the whole of what makes a kind safe to add.

- **O1. It learns only through `cx`.** No global, no thread-local, no clock, no file, no environment variable, no engine-level table, and nothing about its own declaration except through reads.
- **O2. It repeats itself.** It makes one request at a time. Run again from the start and given the same answers, it makes the same requests in the same order. What it was given from last time (5.7) may change what it asks through a view, never its other requests. The core may drop a body in mid-run and run it again. A body that makes a second request before the first is answered ends with a failure.
- **O3. Its result contains no path** and no address inside a live kernel (5.4).
- **O4. It holds nothing shared while it waits.** A kernel is lent only inside a synchronous closure, and a body cannot wait inside one. The compiler checks that the closure does not wait, that nothing borrowed from the kernel leaves it, that it does not take the body's handle with it, and that a body holds across a wait nothing that cannot move between threads, such as a standard lock guard. Two things the compiler cannot check are obligations: a resource type does not hand out shared ownership of itself, and a section does not block on anything that a body or the host would have to supply.
- **O5. It stops.** It bounds its own work, and running out is an outcome like any other.
- **O6. It does not change what it read.** A result is immutable once returned, including through a kernel. The core cannot enforce this. Evaluating the same demands in another order, on a fresh core, exposes it.
- **O7. `content` covers everything a reader can see of the result.** If part of a result should not disturb its readers when it changes, it is a separate node.
- **O8. It says truthfully whether its result is deterministic.**
- **O9. If it opens a view, it makes every other read first.** A read outside the view after that ends its run with a failure.
- **O10. What it makes inside a resource and keeps, it wraps** (`hold`, 2.3), so that it is released when nothing holds it. The core cannot check this.

### 2.3 What the core supplies to a running body

```
impl Cx {
    async fn read<T>(&self, r: &Ref) -> Outcome<T>;                    // recorded; waits until ready
    async fn read_all<T>(&self, rs: &[Request]) -> Vec<Outcome<T>>;    // several refs or child declarations at once, recorded in the order given

    // Declare a child of this node at `step` and read it (7.1). Recorded.
    async fn child<T>(&self, step: Step, d: Declaration) -> Outcome<T>;

    // A view: the state as it would be if `with` held (section 6). Read through it like anything else.
    // A read through it also says whether the answer depended on `with`.
    fn suppose(&self, with: &[(Ref, Outcome)]) -> View;

    // Run a synchronous section that asks nothing of the core, with exclusive use of a
    // resource: a kernel, a solver library, a worker thread. Waits for the resource.
    // The section gets a small handle: cancelled, progress, emit, hold. None of them waits.
    // hold(thing) wraps a thing that lives inside the resource. When the last holder of the
    // wrapper lets go of it, the thing is handed back to that resource's thread to be released.
    async fn exclusive<R, T>(&self, f: impl FnOnce(&mut R, &Section) -> T) -> T;

    // The last final result at this path, what it read, and any state kept with it (5.7).
    fn previous<T, S>(&self) -> Option<Previous<T, S>>;
    fn keep<S>(&self, state: S);              // state to keep beside this run's result; not part of its content

    fn emit(&self, partial: Out);             // publish a partial result while still working (9)
    fn progress(&self, event: Progress);      // tell whoever watches this path; never recorded or stored
    fn cancelled(&self) -> bool;
}
```

A **ref** names what a body wants: a binding, optionally followed by steps down. "My scope's `width`" is the binding `scope` then the step `width`. A body computes refs from what it has read, so a ref can depend on data (`holes[i]`). A ref never goes up and is never an absolute path. The core turns it into a path.

A read returns an outcome and nothing about freshness. A body cannot behave differently because an input is still being refined. A read names the type it expects; if the answer is a value of another type, the reader is given a failure that names both.

### 2.4 What the core supplies to its host

```
core.snapshot(inputs)                      -> Snapshot
snapshot.edit(changed inputs)              -> Snapshot
core.evaluate(snapshot, demand, budget)    -> a request. Answers arrive as they become available, and progress for
                                              every path its demand needs, each tagged with its snapshot
request: replace its demand; end it        // several requests may be live at once
core.trim()                                // ask the store to trim now
core.reads(snapshot, path)                 -> what this path's answer read      // "why"
core.readers(snapshot, path)               -> what read this path               // "what depends on this"
core.counters()                            -> body runs per kind, store lookups, paths resolved, index entries touched
a core may be built on a store that already holds results, and a core's results can be handed to such a store
                                           // used by G0; nothing is seeded after a core's first evaluation
```

`evaluate` is the one entry point. A demand is a set of paths, each with a priority. The counters are part of the contract; the growth tests of section 11 are written against them.

### 2.5 Waiting

A read waits until its answer is ready. The body is suspended, holding only its own state, and the executor runs other bodies meanwhile. No work is repeated.

- **A body makes one request at a time.** `read_all` is how to ask for several at once: a sum over ten thousand elements asks for all of them in one request.
- **Cancelling a body is dropping it.** Nothing it did is stored. It can be run again from the start, which O2 guarantees gives the same thing. If it is dropped while a section it gave to `exclusive` is running, or while it still holds something a section made, that thing is released as 5.5 says.
- **Synchronous leaf code keeps working.** The expression evaluator returns a plain value with no failure path, and a missing cell reads as `undef`, the same as a real `undef`. Its adapter runs it against the answers it has, notes what was missing, waits for those, and runs it again. A pass that met a missing answer declares no child, so nothing is ever made from a stand-in value. So the evaluator is run once per round of refs that could only be computed from earlier answers. That is the adapter's own business and no other kind is shaped by it. The evaluator must first read through one interface. Today it reads a public map directly at six sites, hands the whole map to a selector hook, copies the map to build scopes in five places, and reaches state through five other channels.
- **A solver library that calls back synchronously** runs on a thread of its own, outside any pool a body can wait for, with a bridge that turns each callback into a request the solve body waits on. A library that holds a process-wide lock across its callbacks cannot be hosted this way.

## 3. State

### 3.1 A snapshot

A snapshot is an immutable map from input key to value. The inputs:

- **Source.** One input per unit of compiled source. A unit is one thing that becomes one node's source: a member's default or body, a constraint's expression, a geometry operation list, each argument where an instance is declared, its guard, its placement, a function body. A definition's list of members is a unit of its own, so changing a literal does not change the list, and a declaring scope binds an argument without reading it.
- **Settings** that change results: tolerance classes, the default solver, which kernels and outside tools are available and their versions, and the limits of 7.6. Which purposes are applied, and to what, is not a setting: a purpose is applied in source (Example 9).
- **Imported files**, by content.
- **Roots.** One input per root: its declaration, which names a definition and carries no arguments (7.5).

**A unit of source is identified by its meaning, not its text.** Its hash is that of the compiled tree after parsing and name resolution: no comments, no whitespace, no positions. An edit that changes only spelling changes no input. Positions for diagnostics are kept beside the units in a table that no node reads.

Not in a snapshot: any computed value, any solved value, any verdict, any shape, any dependency graph, what is demanded. A snapshot's identity is the content of its inputs. Two snapshots with the same inputs are the same snapshot.

Snapshots differ only by inputs. Substituting a value at a path exists only inside a solver's view (section 6).

### 3.2 "Current", previews and commits

"Current" is not a core concept. A thin session layer above the core holds it.

- **Current** is the snapshot whose inputs are exactly what loading the source text and the settings produces. That is an invariant. It is what the built engine gets today by reloading.
- **A preview is the edit a commit would make, not yet written to disk.** While a slider is held, the session compiles the exact text the commit would write and evaluates `current.edit(that input)`.
- **A commit** writes that same edit into the `.ri` file and makes the previewed snapshot current.

A preview and its commit have the same inputs, so they are the same snapshot. After a commit there is nothing to compute.

This keeps the ruling of 2026-07-06 that the `.ri` source is the canonical truth for every mutation. That ruling is about where truth lives. It did not require a commit to be a reload from scratch.

## 4. Demand and evaluation

### 4.1 Demand

A demand is a set of paths, each with a priority. It is given with each request and is not part of a snapshot. Nothing is evaluated that is not demanded or read by something demanded. A demand may name a path that does not exist yet; resolving it is what creates it.

What the consumers ask for:

- **`eval`**: the values at given paths, or all under the root.
- **`check`**: the root's summary, which reads every constraint and every declared body under it.
- **`build`**: the root's summary and an export.
- **The GUI**: what it displays, at high priority; and the root's summary, at low priority. The second can be switched off for a very large design; it is on by default. The status it shows is one of: all satisfied, some violated, still checking, checking off. "All satisfied" appears only when the whole design has been checked.

These are four demands on one entry point, not four procedures. An answer cannot depend on which was used.

### 4.2 How a path gets its answer

Recursive for exposition. The executor uses its own queue, not the machine stack.

```
answer(S, p):
    if p is kept up to date in S:          return S.index[p]          // 4.4
    d = declaration of p                   // a recorded read of p's parent's result, asking for the step.
                                           // "absent" is an answer, and is indexed with these reads too.
    // Look for a stored result: start at the recipe, then follow the contents of what was read.
    k = hash(d.recipe)
    loop:
        match store[k]:
            "next it read r":   c = content(answer(S, place(d, p, r)));   k = hash(k, r, c)
            a result:           S.index[p] = it, with the reads;   return it
            nothing:            break
    // Nothing stored for these contents. Run the body, recording what it reads.
    out, reads = run the body               // each read waits on answer(S, ...)
    file out in the store along the chain of (r, content) in reads
    S.index[p] = out, with the reads;   return out
```

If following a stored chain leads back to the node being looked up, the stored result does not apply and the body is run. Only a body that reaches itself while running is a cycle (7.7).

Turning a ref into a path reads each declaration it passes through. Those reads are recorded in the index entry, with the reads of the routine above, and are not part of the key.

The routine shows the plain case. Four things are left out of it and specified where noted: the set recorded for a body that opened a view is the last step of its chain (section 6); a child that a body declared gets its declaration from the read its parent recorded, not from the parent's result (7.1); limits are checked when a path is first answered in a snapshot (7.6); and a lookup that meets the same computation already in progress waits for it (section 10).

Three properties follow from this one routine.

- **Early cutoff.** A node whose inputs have the same contents as before has the same key. Its result is found and its body does not run.
- **Change and change back is free.** So is a guard toggled off and on.
- **Identical instances share.** Two bolts with the same argument contents follow the same chain. There is no instancing feature.

### 4.3 How demand reaches elaboration

Resolving a path reads the parent's result, which reads its parent's, up to the root. Each is an ordinary node evaluation, found in the store if an identical instance was elaborated before. Asking for `root / frame / legs / [3] / foot / area` elaborates the root, the frame, the collection, its element 3 and the foot. Nothing beside them.

### 4.4 After a change

Each snapshot has an index: for each path answered in it, the answer and what it read. A final answer given in a snapshot is that snapshot's answer for good, since the snapshot cannot change. The question is what a new snapshot may take from its parent.

- **The kept set** is what the live requests on a snapshot demand, and everything those answers read. A new snapshot inherits the kept set's entries, shared in structure. A request is live from `evaluate` until the host ends it or a budget does, whether or not everything it demanded has been answered. A snapshot is live while the host holds it or a live request is on it. What a new snapshot inherits is taken when the first request is made on it, from what is kept at that moment on the snapshot it was edited from. Ending that snapshot's requests afterwards drops only the work nothing else needs.
- **An inherited entry is unconfirmed until the core has confirmed it.** The core works outward from every input that has changed since the entry was last confirmed, re-checks each kept node that read something changed, and goes on to that node's readers only if the node's content changed. An entry is confirmed once nothing it could depend on remains to be checked. Until then a read of it waits. "Kept up to date" in the routine above means confirmed.
- **Unfinished work is inherited too.** If a snapshot is replaced before all its entries were confirmed, the snapshot made from it starts from the inputs that were still outstanding as well as its own. This is the normal case when edits arrive faster than evaluation.
- **The core never brings a node up to date on behalf of a reader whose earlier reads have changed.** It re-checks that reader first. Otherwise it would compute a branch the reader is about to leave: an FEA behind an `if` that has just gone the other way. An entry it passes over for this reason is removed from the index.
- **Everything else is not inherited.** If such a path is asked for later, it is answered by the routine above, through the store. That costs work in proportion to the part of what it depends on that was not being kept.
- **A path that stops being demanded** leaves the kept set.

So after an edit the work is bounded by the kept nodes that read something whose content really changed. One exception is honest to state: when a scope's or a collection's result changes, the core compares one declaration per child.

Within one snapshot, a final answer at a path stays that path's answer. Every reader in a snapshot sees the same one, which matters for results that are not deterministic.

A debug mode re-derives every answer by following its chain and compares. The two must agree.

## 5. Results: storage and validity

### 5.1 The rule

> A result is stored under a hash of its recipe and the content of each thing it read, in the order read.
> A stored result applies to a node if, reading the same things in the same order, the node finds the same contents now.

For a node whose reads do not depend on values, which is most nodes, the key is exactly a recursive hash: the recipe and the keys or values of the inputs, down to source and literals. For a node whose reads depend on values, the store is a decision tree: at each step it says which read came next, given the contents so far.

One keying rule for every kind of node. It replaces at least seven keying schemes in the built engine.

### 5.2 The content of an outcome

- **A value that is cheap to hash**: the hash of the value, tagged with its type. A scalar with its determinacy, a verdict with its diagnostics, a list, a geometry description, the children an instance declares.
- **A value that is not** (a shape, a mesh, an FEA field): the hash of the key it was produced under, plus a nonce if it is not deterministic.
  - A deterministic result has the same content wherever and whenever it is made. Sharing by inputs survives a trim and reaches across processes with no coordination.
  - A nonce is unique to one computation: a count, the host, the process. If such a result is trimmed and made again, it has a new content, and nothing computed from the old one can be mistaken for something computed from the new.
  - The content is fixed when the result is produced and travels with it. A reader that only passes the value along needs nothing more; a reader that looks inside it is making a recorded read of it.
- **A failure**: the hash of its structured description, which contains no path.
- **Absent**: a constant.
- **A partial result** has no content. Nothing is filed under one.

The first gives early cutoff. The second gives none through a shape: if the inputs to a shape change and the shape comes out the same, what reads it still runs. Section 3.6 of the original design has the same property.

### 5.3 What enters a key

- The kind's id and version, and the version of each outside engine it calls.
- The contents read. These include the node's source, the functions that source calls, and any setting it reads (representation, tolerance class, kernel preference, solver choice, a limit).

Not the path. Not any name of a definition, except where a name is part of a content that was read: a structure value names its type (5.4), and a definition's list of members names the definition of each sub and of each constructor call. Not bindings. Not the snapshot. Not freshness. Not the time. Not what was there last time.

### 5.4 A result must not contain where it is

- **Values.** No path inside a value. A structure value is its type's qualified name and type arguments, the value of every parameter that is present, and every value decided at or below its instance, by relative path. It holds nothing derived and nothing about how a value was reached.
- **Geometry.** A shape is in its instance's own frame and the parent places it.
- **Parts of a shape.** A face, an edge, a mesh node's attachment to a face, the target of a load: named relative to the description of the shape. Never by a path, a definition name, or a number a kernel handed out.
- **Verdicts and diagnostics.** They name what they are about by ref, and a position in source by its place in the declaration's tree, not by a byte offset. The path and the text position are attached when shown.

The check is mechanical: evaluate the same declaration at two different paths and compare. The second place is evaluated after unrelated kernel work, so a leaked kernel number shows. Every kind gets this test.

What breaks this rule in kept code today. Each is work for a node-kind design, not for the core:

- a closure value hashes the absolute ids of what it captured;
- a geometry value and a selector hash the name of their definition, and the FEA cache key includes it (run: renaming a structure, with identical inputs, turns a cache hit into a miss);
- every compiled reference is the definition's name plus a member, so every compiled hash contains the name;
- values and results carry kernel handle numbers: a load's target faces, and a volume mesh's record of which face each node lies on. The content hash leaves the number out while FEA reads it;
- a verdict's id, the list of undefined inputs in an indeterminate verdict, and diagnostic text all carry cell ids, and attribution is by string replacement on the message;
- a structure value carries the byte position of its constructor call;
- `self.children` evaluates to a list of path strings;
- the role and provenance names of faces and edges live in one table per build, not per body.

### 5.5 Who owns a result

The store owns its entries; a result is shared with whoever is reading it. A result is kept while a live snapshot's index points at it, while an evaluation is reading it, or while another stored result refers to it. After that it stays until trimmed for space, and results reached only from a solver's trials are trimmed first. The store trims when a request's memory budget (7.6) would otherwise be exceeded. That request ends with an error only when trimming everything that may be trimmed is not enough. A result that can no longer be found, because its key holds the content of a trimmed result that carried a nonce, is trimmed with it. What a result holds inside a resource is released, on that resource's thread, when the last holder of the result lets go of it: the store, a body that read it, or a host that kept an answer.

A shape therefore lives until its store entry is dropped and no reader or host still holds the result, and ten thousand identical bolts own one shape. The store never has to be cleared for correctness.

This needs something no kernel offers today: freeing one shape. The only release is a whole-kernel reset on reload, and under this contract there is no reload. Without it memory grows for the whole session. It is a precondition of the geometry kinds.

### 5.6 Determinism

A result is **deterministic** if it is a function of its key alone, bit for bit: the same contents read always give it. A body says so with each result (O8). The same kind can produce both: a solve started from points derived from what it read is deterministic, and the same solve started from last time's answer is not. The language already uses the word this way, in the `#deterministic` pragma and `ElasticOptions.deterministic`.

A result is treated as not deterministic unless its body claims otherwise, and never when the body used `previous`. Expressions are deterministic. A kernel operation is if the kernel is, which one probe supports; the PRD lists the test that would settle it. The core cannot check a claim of determinism from outside, so the debug mode re-runs a sample of bodies that made one and compares the bits. The same re-run is given the answers the first run was given, and fails if the body makes different requests (O2).

For a result that is not deterministic, any result the body returns for a key is acceptable for that key, and these rules hold:

- **At most one result per key is live**, and within a snapshot an answered path keeps its answer.
- **What a body was given from last time may change which acceptable result it returns, never its class.** Whether a solve succeeded, and whether a strict unknown was judged uniquely determined, must be computed from points derived from what the body read before opening any view. One critic's probe shows the kept dimensional solver does not meet this: the same problem gave 8, gave 2, or failed as "not uniquely determined", depending only on its starting geometry. The core cannot check this from outside. It is tested per kind, by solving cold and warm.
- **A result does not show how it was reached.** No iteration count, no "was warm started". The built FEA result has both.
- **A body may return the last final result at its path when the new one is within its tolerance.** The result keeps its content, so its readers are not disturbed. The built FEA "significance filter" does this.

### 5.7 What was there last time

The best starting point for a node is what that node did last time. That is found through the index, not kept in a store of its own.

- **`cx.previous()`** gives the last final result at this path and what that result read. "Last" is the most recent final answer the core gave at this path in any snapshot of the session, outside any view; the core keeps one pointer per path for it. The pointer does not keep a result alive: if the result was trimmed, `previous` gives nothing. Once the body has made a read in this run, it can ask whether that read's content differs from last time. That is the "what changed" the original design passed to a warm start, limited to what the body reads anyway.
- **A kind may keep extra state beside a result** with `cx.keep`: a solver's last point, a solution vector. It is immutable, is not part of the result's content, is trimmed with the result, and comes back through `previous`.
- **A body that used `previous` returns a result that is not deterministic.** If its value is not hashed by value, it carries a nonce (5.2). A result returned again under 5.6 keeps its content, and the state kept beside it becomes the returning run's.
- **None of it enters a key**, and a body must give an acceptable result with or without it.

Choosing the stored result for the *nearest* inputs, not the last one at this path, is left out on purpose (Part D).

## 6. Views

Inside a body, `cx.suppose(with)` gives a view: the state in which the paths in `with` have the given outcomes and everything else is as it is. Reading through a view is ordinary evaluation, and waits like any other read.

- **A substitution is a statement about a place.** It applies at one path. Two things with equal content at different paths are different things that happen to share stored results. After a substitution at one of them, whatever depends on it reads a different content, gets different keys, and no longer shares with the other.
- **So anything a view may substitute must have exactly one path.** Every sub, every element of a collection, and every node a body declares has one. A structure value that has flowed from where it was made has none; Example 11 sets out the choice that leaves.
- **Results computed under a view are stored like any others.** A solver's trials reuse each other's work wherever contents agree, and the final state finds what the trial at the final point computed, provided the solve substitutes outcomes with exactly the content its result will give. Nothing is promoted or copied.
- **An answer that did not depend on anything substituted is an answer of the surrounding state too.** The core tracks which answers depended on a substitution. A read through a view returns that fact with the outcome.
- **Structure follows.** If a substituted value controls structure, the view has the other structure. A trial with `count := 4` has four elements. A substitution at a path that does not exist in the view answers "absent".
- **What a view's evaluation rested on is recorded for the caller.** While a view read is evaluated, every node and every snapshot input involved either depended on a substitution or did not. Each one that did not, and was read by something that did or by the caller, is recorded for the caller with its content: a node by its place relative to the caller, an input by its key. That includes the source of a cell that lies between an unknown and a constraint. Together these are everything outside the substitution that the trials rested on. They form the last step of the caller's chain (O9). A stored result applies if every member has the same content now. Members are checked in the order they were first read, stopping at the first that differs. The store may keep a few alternatives for the same earlier reads.
- **A view inside a view.** A node evaluated under a view may open a view of its own. What matters is what it reads through its own view. Anything it read before opening it is simply an input: if that input depends on the outer substitution, the node is a different computation under each outer trial, and it follows the outer one, and the answers do not depend on which of the two is asked for first. If neither of two view-openers took in anything the other decides before opening its view, and each reads the other's unknown through its own view, they depend on each other, and that is a cycle (7.7): both get that failure, described the same way whichever was asked for first. Without this the answer would depend on the order. So an instance with a solve of its own, whether a sub or a structure value, can be given an argument that an enclosing solve is still choosing, and an element of a collection whose count is on trial can have a solve of its own.

## 7. Dynamic structure

### 7.1 Nodes that create nodes

Two ways. Both give the new node a path and a declaration.

- **A result declares children.** It is part of the declaring node's result, so it is content: two identical instances declare the same children.
- **A running body declares a child** with `cx.child(step, declaration)`, at a step it chooses. Within one run, declaring a step again with the same declaration names the same child; declaring it with a different declaration ends the run with a failure. That is how an expression gets an FEA result for these arguments, a measurement of this shape, or an instance of a definition built in the middle of the expression. The step is the chain of call sites and iteration indices from the body's own expression to the place, not the argument values. So each call has its own path, and the path stays the same when the arguments change. The declaration is recorded with the read, so an identical parent elsewhere can be found without running its body, and so the child can be reached by path from outside. The child exists as long as its parent's current answer records declaring it. When a body runs again, the core re-checks every child its last run declared, whatever became of the parent's content.

Sharing does not depend on which way a node was created. It comes from the storage rule.

The core does not build a graph, compare two graphs or reconcile them.

### 7.2 Re-elaboration

A declaring node is an ordinary node. If something it read changes, it is evaluated again. Each child reads its own declaration from the parent's result, and that read has its own content. If the parent's result changed and this child's declaration did not, the child and everything under it are untouched.

### 7.3 What should read structure-controlling values

To keep an instance's elaboration from depending on values, each value-controlled piece of structure can be its own node:

- a guarded member reads its own guard first: under a false guard its answer is "absent", and under an undefined one it is a failure, "presence undetermined" (counted with undetermined verdicts, not as an error). A guarded sub is the instance itself, not a node in front of it;
- a collection is a node whose result declares its elements, and which reads the count;
- declarations with one name under mutually exclusive guards (the arms of a `match`) are one such node, which reads the guards and chooses the declaration, so the name has one path.

An instance then reads only its own presence guards and the source of its definition. The instance that contains a guarded member reads no guard. This is a choice for the instance kinds. The core permits either.

### 7.4 Removal

There is no removal operation. A path whose parent no longer declares it resolves to "absent", and a consumer that still demands it is told so. Stored results stay until trimmed, which is why a guard toggled off and on costs nothing the second time.

### 7.5 Identity

- **A path is stable across re-elaboration.** Same root, same steps, same node. Demand, selection and what was there last time survive.
- **A root is named by the session**: a module and a definition. Two definitions with the same text are two roots that share their results. A root's declaration is a snapshot input. A demand names the root; a name with no such input answers "absent". A root's declaration names its definition and carries no arguments: the source is the sole definition of a design, so a value the source does not hold is not part of one.
- **Collection elements** are steps by position or by key. The core does not care which, but each element has exactly one step; the other form of address is a lookup.
- **A child a body declares inside an iteration** keeps its path only while the order of the iteration holds. Filtering or reordering the list moves it.

### 7.6 Limits

The core holds no limit as a constant. There are two sorts, and they live in different places.

**Limits that change answers** are snapshot settings: a design fails at one value and succeeds at another.

- **Depth of a path**, and **children per node**, counting both the children a result declares and those a body declares. The core checks both when a path is first answered in a snapshot, against the snapshot's settings, whether or not the result is in the store. Exceeding one is a failure at that path with one diagnostic. It is a fact about a path in a snapshot and is never stored: the same instance can be legal at depth 3 and too deep at depth 65. When one of these settings changes, the core treats as changed every path whose check now comes out differently.
- **A body's own work** (O5), where the kind makes it configurable. The body reads the setting, so it is in the key.
- **Where they are set:** a default in the binary; a project value in a `[limits]` table of `reify.toml`; a per-file value by pragma in the file of the design being evaluated. The effective value and where it came from are shown on request. Today's depth of 64 is the default.

**Budgets that are about this machine and this session** are given with each request: paths resolved, memory for the store, time. Exhausting one ends the request with an error, without stopping work that another live request is waiting on. They never enter a key and nothing is stored for them.

**Settings of the core itself** are given when a core is built: how many alternative records of what a view-opener's trials rested on the store keeps for the same earlier reads (section 6), and how many of the bodies that claimed determinism the debug mode runs again (5.6). They never change an answer that is deterministic.

(Run: today a recursion that exhausts the 10,000-node budget prints one error per frontier node, 34 of them at depth 20, and a partial tree.)

### 7.7 Cycles

If a running body requires its own path's answer, every node on the cycle gets the failure "cycle", described in a form that does not depend on where it was entered. That includes a node reached again under a view it opened itself. Like depth, a cycle is a fact about paths and is not stored. It is found on the waits: a body that is waiting counts as in progress. A path that is being answered in a snapshot has at most one run at a time, and a read of that path waits on that run. A chain of waits that passes through a lookup is not a cycle: that lookup misses and its body runs. Only a ring of running bodies is a cycle. Two view-openers that depend on each other (section 6) are one.

Reporting possible cycles before anything runs is a check for the language, which knows what an expression might read. The core does not offer it.

## 8. The solve

The solve is a node. The core has nothing for it that other nodes do not use.

**One solve per coupled group.** A group is a set of unknowns that must be chosen together, with the constraints and objective that bear on them. Its solve lives at the lowest scope that contains all of its unknowns and all of those constraints: `scope / solves / [group]`. A scope with three independent groups has three solves; one failing does not touch the others, and an edit to one group's constraint re-solves that group only.

This follows the direction of 2026-11-19 as task #10296 records it: key the node "by solve unit, which is one template scope OR a merged cross-scope cluster, never by scope alone". The task adds that because membership of a cluster depends on run-time values, "the PRD must say how node identity survives a cluster re-forming". Here a group is named by its first unknown and its home is a place in the instance tree. When groups re-form, results are found by content wherever the problem is unchanged, and what is lost is the previous starting point for the unknowns that moved.

**How a solve runs.**

- It **reads its problem as data**: which unknowns it owns, and the source of the constraints and objective. Every kept solver needs the expressions themselves. One pattern-matches them into geometric primitives. One splits each comparison into two sides to form residuals, derives interval boxes and builds extra objectives. One scans them for enumeration domains.
- It **evaluates trials through a view**, substituting every unknown it owns. Before that it reads declarations, source, and what its instance is given from outside: its arguments. Everything else it reads through the view. Whatever lies between the unknowns and the constraints is evaluated by the core under the trial, whatever kind of node it is. So a constraint over a volume, a mass or an FEA result can take part in a solve with no special case in the core. A solver that assigns unknowns one at a time gives the unassigned ones an outcome that says so.
- Its **result** is a map from each unknown to its value. An unknown is any node with one path: a scalar, an enumeration, a pose frame. Each solved thing is a node that reads the solve and takes its part. Its readers depend on the solve and are evaluated once.
- **Re-solving after an edit** is not a case. Anything the solve read, directly or through its trials, invalidates its stored result when it changes. That covers the two triggers the built engine's hand-written rule missed: a guard flip, and an edit to an objective's inputs.
- **A failed solve** is a stored result. Each unknown it owned is then undetermined, with that cause. There is no "keep the previous value" as state, because an answer cannot depend on history. The previous value can still be shown, marked pending.

**Who decides an unknown.** Ruled by Leo on 2026-11-25.

- **Each written `auto` is one decision, made where it is written.**
- **Written in a definition**, it is decided inside each instance of that definition, by the constraints inside that instance. That is the same for a sub and for a structure value. A constraint outside is checked against the result and cannot change it; its verdict names the unknown it could not reach.
- **Written at the use site**, it is the creator's decision: it belongs to the instance of the definition whose text contains it. Every constraint inside that instance whose verdict depends on the result, through any chain of reads, bears on it. A constraint outside it is checked against the result and cannot change it.
- **What counts as inside an instance**: the constraints of its definition and of everything below it, and a constraint written in the braces after a sub, which is a member of that sub's instance.
- **A purpose only checks.** An applied purpose sits inside the applying instance by path, but its constraints are verdicts against the result and are never part of a solve's problem. A purpose holds no objective (Example 9).
- In Leo's words: "let and sub should behave the same way (in how they handle auto and constraints). Objectives will probably have to differ."

**How groups are formed** is the job of ordinary nodes: what unknowns exist below a scope, which constraints bear on which, which groups live here. Three rules bind them. They are what lets the core give answers that do not depend on order.

- **Planning reads declarations only, never values.** Structure under a guard or a count is reported as "present if", with the guard or count as data. A group formed this way can be larger than the values require; the solve reads the guard through its view and leaves out what is absent. Without this rule a plan could read a value that reads a solve that reads the plan.
- **A scope's own result does not read its planning nodes.** They, and the node that declares one solve per group, are fixed children of the scope.
- **Groups whose problems depend on each other's unknowns are one group.** If the plan misses that, the two solves end up inside each other's trials, which is a cycle (section 6): both fail, the same way whichever is asked for first. A missed coupling is a reported defect of the plan, never a silent answer. It also means two solves never nest through the core: if the solve design wants "the child optimises for each trial of the parent", that is one solve node doing both, by the plan's choice.

**What the contract does not decide.** These belong to the solve design that follows, and the core supports either answer to each:

- How an objective reaches unknowns it was not written beside. The spec lets a child with no objective inherit its container's, and lets a cost over `self.descendants` join what is contained. A structure value is not contained, so on this a sub and a value will probably differ, as Leo expects. The built engine joins groups through objectives today.
- What objective governs a group that spans a child with an objective of its own.
- Whether a group may cross the boundary of a recursive instance. The spec says resolution does not.
- A limit on a group's size. The built engine falls back above 12 unknowns.
- What a solver does with a constraint whose guard reads an unknown. The core makes that constraint present or absent in each trial. The kept dimensional solver has no treatment of an implication (#6572), and today's two paths disagree: a full evaluation leaves guarded constraints out (run: the solver picked 7 mm, which switched on a guarded constraint that then failed), and the warm edit includes both arms.
- Unknowns inside structure that another unknown of the same group controls.

**What the kept solvers lack for this**, as new leaf work: a strictness verdict that does not depend on the starting point (5.6); a best point on failure, which no solver returns today; one problem that mixes pose and scalar unknowns (#6677, not landed); integer trials for a count, since an engine-made integer unknown is routed to the real-valued solver today.

Unknowns that are types (`Bearing<auto: Seal>`) are resolved by the compiler, per definition, and stay outside the core.

## 9. Outcome and freshness

Two things, kept separate. They are orthogonal.

**Outcome**: what the computation returned.

- a value;
- a failure, with a structured description: a kernel error, a failed solve, a body's own limit;
- absent.

A failure that follows from the inputs is a result. It is stored under its key and found again. A body that reads a failed input normally returns "blocked by the failure at this ref", also a stored result; the chain to the root cause is the recorded reads. A graph failure is not a language value and an expression cannot catch it, as the original design requires.

A failure that does not follow from the inputs is not an outcome: cancelled, out of memory, a request's budget exhausted. Nothing is stored for it. Nothing is stored or kept, either, for anything that read it, directly, through other nodes or through a view. Otherwise one bad moment would be found by every identical instance and inherited by every later state. A read of something that failed this way does not return: the reading body is dropped. A body whose own work fails this way (a section that ran out of memory) returns that in place of an outcome. Each demanded path that needed it is reported to the host as pending, with the error; the other paths of the request are not affected, unless the failure was the request's own budget.

**Freshness**: how far the core has got for this path in this snapshot.

- **final**;
- **intermediate**: the body published a partial result and is still working, or something it read was intermediate;
- **pending**: not computed yet, with the last result this path had, if any, for display.

Only final answers are stored. An answer computed from an intermediate input is shown and marked intermediate. It is never made final by comparison. When the input becomes final, every reader that read it while it was intermediate is answered again from the start, in that snapshot and in any made from it. Freshness is never in a key.

**Diagnostics** are part of a result. **Progress** (an iteration count, a residual) goes through `cx.progress` to every live request whose demand needs that path, and is kept nowhere. A journal of events may be kept beside the core as history. The built one has no consumer and is never cleared.

## 10. Scheduling

- **First executor: one thread runs every body.** A body runs until it waits; then the next ready body runs. Work is queued, not recursed, so a long chain of dependencies cannot overflow the machine stack. The core starts no thread for this: the host calls the core to take a turn, and the calling thread runs the bodies. The only threads the core starts are those of resources.
- **Heavy synchronous work runs elsewhere and is waited for.** A section given to `cx.exclusive` runs on the thread of the resource it borrows: the kernel's own thread, a worker for an FEA or a solver library. The executor thread stays free. The kernels need the exclusivity anyway: OCCT runs on one thread, and gmsh and the SolveSpace library each sit behind a process-wide lock.
- **Work is ordered by the priority of the demand that needs it.** What the user is looking at comes before the root's summary.
- **Work for a snapshot nobody wants any more is dropped.** A slider that has moved on abandons the bodies waiting for the earlier tick. A long section already running is told through `cancelled`. Kernel calls cannot be interrupted, so such a section delays more urgent work on the same resource by its own length. The PRD lists this as an open item.
- **The same computation is not run twice at once**, on one thread or many. A lookup that finds a run in progress whose reads so far have the same contents as its own waits for that run, unless that run is waiting on it. Work that a live request is waiting on is not dropped.
- **In parallel.** Bodies interact only through `cx`. Results and snapshots are immutable. A parallel executor gives the same answers for deterministic results. For one that is not, which acceptable result is stored can depend on timing, as it can on history.
- **Not built now, and not precluded:** running bodies themselves on several threads, keeping a long task running when its inputs move, evaluating what changed but is not demanded, more than one host.

## 11. The growth requirements, as invariants

Stated against the counters of 2.4, on synthetic models, for shape of growth and not for speed.

- **G1. Work follows the change.** In a model of N parts, all demanded, change one input. Body runs, store lookups and index entries touched are bounded by the kept nodes that read something whose content changed, plus one declaration comparison per child where a scope's or a collection's result changed. Test cases, each at N = 100, 1,000, 10,000: one part's own parameter (no growth with N); a value that N parts read, changed so that an intermediate result stays the same (no growth); a root scope with N children gaining one member, and a collection going from N to N + 1 (at most N comparisons, and nothing under an unchanged child touched); N independent solve groups under one scope, one edited (one solve runs; the scope's planning node runs once, and the counters cannot see that its own work grows with the scope).
- **G2. Identical instances share.** N instances with equal argument contents: body runs inside an instance equal those for one instance, whatever N is. Bookkeeping per path remains (Part D, sharing index entries). Sharing does not reach unknowns that one solve couples: N coupled unknowns are one N-dimensional solve.
- **G3. Nothing unasked.** N parts, one demanded: body runs and paths resolved do not grow with N.
- **G4. No work is repeated to wait.** A body that makes n reads runs once. The exception is the expression evaluator, which its adapter runs once per round of refs that could only be computed from earlier answers. That has its own counter.

And the correctness statement they rest on:

- **G0. An answer depends on the snapshot and on nothing else, except through a result that is not deterministic.** After any sequence of edits and trials, every answer equals the answer of a fresh evaluation that is handed the same non-deterministic results. Separately, each such result is within its kind's tolerance of one computed cold.

The core crate carries these tests itself, on toy kinds, before any Reify node kind exists.

---

# Part B. Eleven worked examples

The node kinds named here are illustrations for the instance design that follows this contract. They are not part of the contract. Each example ends with what in the core it uses. Snippets marked "checked" were parsed and run on the prebuilt binary.

Kinds used: **scope** (an instance of a definition; its result declares its members), **cell** (a `param` or `let`), **constraint**, **collection**, the planning kinds and **solve**, **realize** (a shape for a geometry description), **measure**, **tessellate**, **compute** (an FEA or similar), **summary** (the verdicts under a scope).

## Example 1. A value cell

```
structure def Clamped {
    param width : Length = 50mm
    let w_eff = if width > 40mm then 40mm else width
    let area = w_eff * 10mm
}
```
(checked)

Edit `50mm` to `60mm`. The source input for `width`'s default changes. `root/width` read it, so it runs: 0.06 m. `root/w_eff` read `width`, so it runs: 0.04 m, the same content as before. The core stops there. `root/area` is not looked at. Two body runs.

Uses: recorded reads, the storage rule, 4.4.

## Example 2. A constraint, and one that reads geometry

```
structure def Plate {
    param t : Length = 5mm
    param w : Length = 40mm
    let body = box(w, w, t)
    constraint t >= 3mm
    constraint volume(body) <= 10000mm^3
}
```
(checked)

Each constraint is a node whose result is a verdict. The first reads `t`. `body` is a cell whose value is a geometry description: `box(40 mm, 40 mm, 5 mm)`, small and hashable. `root/body/brep` is a realize node; it reads the description and borrows the kernel. `volume(body)` is a measure node that reads the shape and returns a scalar. The second constraint reads that scalar.

Edit `w` to 41 mm. The description changes, a new shape is made, the volume is 8405 mm³, the constraint runs and is still satisfied. Whether the scope's summary is then looked at depends on what a verdict's result holds. If it holds only "satisfied", its content is unchanged and the core stops. If it also holds the measured value, the summary runs. O7 makes that a choice the kind's designer has to make in the open.

There is one routine. `check`, `build`, the GUI and a preview all read the same node. Today five routines produce verdicts and which run depends on the entry point (run: the same conformance constraint is VIOLATED under `check` and OK under `check --purpose`).

Uses: recorded reads, `exclusive`, O7.

## Example 3. An instance scope with a guard

```
structure def Asm {
    param use_plate : Bool = true
    param n : Int = 2
    sub thin : Plate where use_plate { t = 4mm }
    where n > 1 {
        let big = n * 10
        constraint big > 5
    }
}
```
(checked)

`root/thin` is the instance of `Plate`. It reads `use_plate` first. When that is false its answer is "absent"; otherwise it declares its members, with `t` bound to this argument. `root/thin/t` resolves through it.

- Set `use_plate` false: `root/thin` runs and is absent. A viewport still demanding its mesh is answered "absent". Set it true again: everything is found.
- Change `n` from 2 to 3: the guard `n > 1` runs and is still true, so nothing that read only the guard runs. `big` and its constraint read `n`, so they run.
- `Plate`'s own `constraint t >= 3mm` is the node `root/thin/constraint[0]`, reading this instance's `t`. Today it is checked once, at the definition's default, and never on an instance (run).

Today a guarded sub on an ordinary definition is never created, even when its guard is true (run), against the language spec's rule that `where` controls structural presence for every entity.

Uses: a result that declares children, "absent" as an answer.

## Example 4. A collection whose count is solved

```
structure def Manifold {
    param n : Int = auto
    sub vents : List<Vent>
    constraint n >= 4
    constraint n <= 4
    constraint vents.count == n
    let a0 = vents[0].area
}
```
(checked)

`root/a0` reads `root/vents/[0]/area`. Resolving that path reads the collection node `root/vents`, which reads `n`, which reads the solve for `n`'s group. The solve reads its problem (the two bounds, as data) and returns 4. The collection declares four elements, element 0 exists, and `a0` is 10 mm. Each of those reads waits for the one before it. Today the count is solved and no element is created (run: `a0 = undef`).

There is no going back to structure after solving, because there are no phases. It is dependency order.

A stronger case, which the original design ruled out as a cycle: `constraint total_airflow(vents) >= required`, with `n` unknown. Under a trial with `n := 3` the collection, read through the view, declares three vents and the constraint is evaluated on them. So the core gives this model a meaning. No kept solver can use it yet: none produces integer trials for a count.

Uses: a view, structure under a view.

## Example 5. A solve over two coupled sibling instances

```
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Rod  { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm {
    sub a : Beam { len = auto }
    sub b : Rod  { len = auto }
    constraint self.a.len + self.b.len == 120mm
    constraint self.a.len - self.b.len == 20mm
}
```
(checked)

Each scope has a few fixed children that do the planning. They read declarations only, and the scope's own result does not read them.

- **What is unknown below here** flows up. `root/a`'s report says "`len`, with two bounds on it". `root`'s report collects its children's.
- **Which groups live here.** `root`'s grouping node sees that the two constraints of `Asm` tie `a.len` and `b.len` together. The lowest scope holding both unknowns and both constraints is `root`. A fixed child, `root/solves`, reads the grouping and declares `root/solves/[a.len]`.
- **What was solved above** flows down. `root` has a hand-down node per child, holding the part of its solves that concerns that child, and binds it into the child's declaration.

`root/solves/[a.len]` reads the six constraints as data, runs trials through a view with both unknowns substituted, and returns `{a.len: 70 mm, b.len: 50 mm}`. `root/a/len` reads what was handed down and takes 70 mm. `Beam`'s bounds are verdict nodes under `root/a`, checked at 70 mm. Today they are not applied to the instance (run, with the two constants changed to 200 mm and 100 mm: the solve returned 150 mm against the bound of 100 mm).

If `len = auto` is written in `Beam` itself instead of at the use site, each beam decides its own `len` from its own bounds, and `Asm`'s two constraints are checked against the results, not used to choose them. Today that form gives the instances no `len` at all (run).

Uses: nothing the core does not already have. The planning nodes are ordinary.

## Example 6. A realization whose parent composes its children

```
structure def Peg { param w : Length = 10mm   let body = box(w, w, 20mm) }
structure def Pair {
    sub a : Peg { w = 10mm } at transform3(orient_identity(), vec3(0mm, 0mm, 0mm))
    sub b : Peg { w = 20mm } at transform3(orient_identity(), vec3(50mm, 0mm, 0mm))
    let total_vol = volume(self.a.body) + volume(self.b.body)
}
```
(checked)

There is one realize node per instance, representation and tolerance class, as in the original design: `root/a/body/brep`, `root/a/body/mesh[class]`. Each reads its body's description and the settings it needs. The stored result is filed by content, so two pegs with equal descriptions share one shape and one mesh, each in the peg's own frame.

The parent's displayable geometry is a node whose result is a list: (a, its transform), (b, its transform). Making it needs no kernel.

- Add a third `Peg` with `w = 10mm` somewhere else: one more list entry. The shape and the mesh are found.
- Move `b`: the list changes. No kernel work.
- Change `b`'s `w`: one new shape, one new mesh. `root/b/body/brep` can start from the shape it made last time. `a` is untouched.

Today both instance bodies carry the definition's label, and a sub with arguments is re-executed uncached.

One thing this example does not settle. A parent operation such as `union(self.a.body, self.b.body)` reads the children's bodies. Today it gets them unplaced (run: the union of two pegs 100 mm apart has the volume of one). The spec's idiom places a child body by hand, with `translate(self.inner.body, ...)`, and does not say what happens when the sub has an `at`. Either answer fits the contract: placed, the parent's description contains the child's description and its transform. Which is right is a question for the geometry design.

Uses: the storage rule, 5.4, `previous`.

## Example 7. An expensive computation

```
    let result = solve_elastic_static(material, length, width, height, [tip_load], [mount], ElasticOptions())
    let peak = result.max_von_mises
    constraint peak < 400MPa
```
(checked; this form of the call builds its own box mesh from the three dimensions)

Evaluating `result`, the evaluator reaches the call with its arguments evaluated. Its adapter declares a child at that place in the expression: a compute node with those argument values. The cell waits. The compute node runs its heavy part on a worker thread: it takes the last solution from `previous`, reports progress, polls `cancelled`, and may publish partial fields. When it finishes, `result` goes on.

- While it runs, the answers at `result`, `peak` and the constraint are pending, each with its last result for display. If the FEA publishes a partial field, `peak` is computed from it and shown as intermediate. Nothing computed from a partial is stored.
- Edit an unrelated parameter: nothing here runs.
- Change `length`: the compute node has the same path and a new declaration. It runs, starting from the last solution, and its result carries a nonce because of that. Change it back: found.
- Rename the structure: found. Today that is a miss (run).
- Two identical cantilevers: one FEA.
- Inside a solve, equal trial points share one FEA. Today the per-trial FEA has no cache.

The other form of the call takes a body. There the compute node also reads the body's volume mesh, itself a node, and its loads name faces relative to the body's description. Not run here.

Uses: waiting, a child declared by a body, `previous`, freshness, `progress`.

## Example 8. A slider preview and its commit to source

`Pair` from Example 6. The user drags `b`'s width.

**Hold.** The edit a commit would make is to the literal in `sub b : Peg { w = 20mm }`. Each tick the session compiles that argument with the new literal and evaluates the demand in `current.edit(it)`. Everything displayed that depends on it is evaluated by the same routine as a load: the description, the shape, the mesh, `total_vol`, every verdict. A verdict cannot be computed from a stale value, because there is no other way to compute it. That is the defect observed on the held joint slider today. The root's summary follows at low priority and is abandoned if the next tick arrives first.

**Release.** The session writes the same literal into the file. The snapshot is the one already evaluated. The root's summary finishes.

- If `b` had no argument (`sub b : Peg`), the edit would add one: `sub b : Peg { w = 22mm }`. It would not rewrite `Peg`'s default, which would move `a` as well.
- The argument for `b` lives in `Pair`. If `Pair` is itself used twice, the gesture moves `b` in both. The preview shows both moving, because the preview is the edit. The session can say so when the drag starts.
- A parameter has a direct control unless what is written for it, where the instance is declared, is a formula. A literal is edited in place; `auto` and `undef` count as literals, reached by a toggle. Where nothing is written the control adds an argument and leaves the definition's default alone, whether that default is a literal or a formula. A literal has one place in source wherever it is written, a constructor call inside an expression included; editing it moves every instance that text makes, and the preview shows which.
- The gesture is refused for an argument that is a formula (it is edited in source, so that a formula is not silently replaced), an element of a list collection (it has no arguments of its own), a definition in an imported module, a value whose unit cannot be written back exactly, and a required parameter of a root (a root's declaration carries no arguments, 7.5).

Uses: `edit`, demand priorities.

## Example 9. A purpose being applied

```
structure def Bracket { param t : Length = 5mm   constraint t > 1mm }
purpose thin_enough(subject : Structure) { constraint subject.t < 4mm }
```
(checked)

A purpose is a predicate: it checks that a design is fit for something. It is declared once and applied in source, by one line in the definition that holds its subject. The form of that line is for the PRD of the instance kinds. A session's toggle, or `check --purpose`, is the uncommitted form of adding that line, as a slider is of editing a literal (Example 8).

Applying a purpose therefore changes one source input: that definition's list of members. Its instance runs again and declares one more child, the purpose's instance, with each of the purpose's parameters bound to a place below. The children it already had are untouched, because their declarations did not change.

The purpose's constraints are then ordinary nodes, and their verdicts appear in that instance's summary. No solve reads them: a purpose only checks, and an objective written in a purpose is a compile error that says to write it in the definition. Today a purpose's objective is recorded and never read, activation writes into the installed graph in place, and neither the GUI nor any agent tool can activate one.

A purpose over two things in different roots, or over a constraint, needs more than this example shows. That is for the instance design.

Uses: `edit`, a result that declares children.

## Example 10. An edit to one literal in the source

Edit `param w : Length = 10mm` to `12mm` in the text of `Peg`. The compiler runs. One source input has different content. From there it is Example 1, plus one new shape and mesh per distinct description.

- An edit to one constraint's body changes that source input. The constraint runs once for each distinct set of contents its instances have. Nothing else runs.
- A comment or whitespace edit changes no input, once the compiler hashes meaning and not text. Today the hash of a sub declaration is its source text.

Today every text edit recompiles the module, rebuilds the graph, recomputes every ordinary cell, frees every kernel shape and re-realizes every body.

Uses: `edit`. A source edit and a value edit are the same event.

## Example 11. Structure values

A structure value is what a constructor call in an expression gives: `let c = Plate()`. It is data. It flows: into another `let`, a list, a function, a field of another value.

**How it works, whichever way the choice below goes.**

- The cell that holds the constructor call declares a child for the instance, at that place in the expression. That child is the instance. It has one path, it is what the GUI shows and selects, and its verdicts are the ones that count.
- The value the cell then holds is the type's name, the value of every parameter, and every value decided at or below the instance. Nothing in it is still to be decided.
- A cell elsewhere that holds the value and needs a member declares a child of its own from the value, with every parameter given. There is nothing left to solve in it and everything in it is found in the store. Those children are mechanism: not shown and not counted.
- The same goes for a value taken from a place, such as `let e = self.a`: it carries `a`'s parameters as they came out.

**The choice.** The instance at the constructor has one path, so an unknown in it could be chosen from outside. Whether it may be is a choice, not a consequence of anything in Part A.

```
structure def Plate {
    param t : Length = auto
    constraint t >= 3mm
    minimize t
    pub let area = t * 40mm
}
structure def Asm {
    let c = Plate()              // (1)
    constraint c.t > 5mm         // (2)
    let d = Plate(t: auto)       // (3)
    constraint d.t > 5mm         // (4)
    sub s : Plate                // (5)
    constraint self.s.t > 5mm    // (6)
}
```
(not checked; form (3) does not compile today)

| | A. Inert, as built | B. Closed: solved inside | C. A `let` of a constructor is a place |
|---|---|---|---|
| (1) `c.t` | undefined; `Plate`'s constraint is never checked | 3 mm, solved by `Plate`'s own constraint and objective | an unknown at the constructor's path |
| (2) | indeterminate | violated: checked against 3 mm, with a diagnostic naming the closed unknown | same as (6) |
| (3), (4) `d.t` | no owner | `Asm`'s unknown, at the call site; `Plate`'s constraints bear on it | as B |
| (5), (6) `s.t` | undefined | 3 mm and violated, the same as (1), (2): a sub and a value handle `auto` and constraints the same way | an unknown the creator's solve may reach |

- **Under the language spec's default, B and C give the same answers on every line.** The default is that a child's own unknowns are solved first and the parent takes them as given. B and C differ only if a parent's constraints are allowed to steer a child's own unknowns, which Leo has ruled they are not, for a sub as for a value.
- **C costs more than it looks.** A function that returns `Plate()` stops being a function of its arguments, or C applies only when the constructor is the whole right-hand side of a `let`, and then `let c = Plate()` differs from `let c = id(Plate())`. Leo on that second form, 2026-11-25: "Absolutely not - confusingly inconsistent." Equality and sets of values must choose between content and place.
- **C makes a name change the meaning.** Leo's hunch, 2026-11-25: `let c = Plate()` followed by `sub d = Widget(c)` "should be semantically identical to" `sub d = Widget(Plate())`. Under B that holds however often `c` is used. Under C it holds only while `c` is used once. With `sub d = Widget(c)` and `sub e = Fidget(c)`, C has one plate that both must suit; written out as `Widget(Plate())` and `Fidget(Plate())`, C has two, each suited to one.
- **Ruled by Leo on 2026-11-25: B.** ("Yes.") In words: a structure value is closed, naming it changes nothing, and each written `auto` is one decision made where it is written. The reasoning offered: in the spec, what steers a child from outside travels along containment: an objective is inherited from the nearest container, and a cost over `self.descendants` joins what is contained. A value is not contained. So "nothing outside steers a value" is a difference of role, which is where Leo wants the difference between a sub and a value confined. Functions that return values stay functions, and equality stays by content, as it is today: a set of two equal values has one member (run).

**An instance built from something on trial.** `let c = Slot(width: w)`, where `w` is an unknown of `Asm` and `Slot` has an unknown `depth` of its own. Under each trial of `w` the instance is given a different `width`. `Slot`'s solve reads its argument first and then runs its own trials, so it is a different computation under each outer trial and it follows the outer solve (section 6). Asked for in the other order the result is the same. The same holds for `sub c : Slot { width = w }`.

**Each written `auto` is one decision, made where it is written.** That is the one thing in this area that is not a value, and it is what gives B the behaviour C offers.

- `auto` written in the definition is decided inside, by the definition's own rules, the same for every `Plate()`.
- `auto` written at the use site (form (3)) is decided once, at that call, by everything inside the creator that reads the result. With `let c = Plate(t: auto)`, `sub d = Widget(c)` and `sub e = Fidget(c)`, there is one unknown, and the constraints inside `Widget` and `Fidget` both bear on it.
- So an `auto` may be written only where it is evaluated once. Inside a lambda or a function body one written `auto` would be many decisions, and planning, which reads declarations only, could not count them. There it is a compile error that says to use a sub or a collection. No source in the repository writes `auto` in a constructor argument outside a sub declaration today.

**What the repository's sources say** (a census by pattern, so approximate): 769 constructor calls in expression position, in 226 of 996 files. They construct 166 definitions. None of those definitions has an unknown, a guarded member or an objective. They are data: materials, loads, options. So the choice between B and C changes no existing model. One thing does change under either: 21 of those definitions have constraints, used at 150 call sites, and today none of those constraints is checked on a value.

Uses: a child declared by a body, the storage rule, a view.

---

# Part C. Rulings, and what is left to later designs

## Ruled

Quoted where short. Dates are 2026-11-25.

| | Decision | Leo |
|---|---|---|
| **1** | Results are filed by recipe and contents read, never by path | "yes" |
| **2** | A snapshot holds inputs only; the solve is a producer | "I think so" |
| **3** | Structure by declaration, resolved on demand; no graph build or diff | "I think so" |
| **4** | Every body can be run again; amended: a body is a task that waits for its reads | "yes", then "Yes" to the amendment |
| **5** | Outcome and freshness are separate | "yes (They are orthogonal)" |
| **6** | Some results are not deterministic, and the contract says so | "yes" |
| **7** | The GUI demands what is displayed at high priority and the root's summary at low priority, with a toggle that is on by default | "Yes" |
| **8** | One solve per coupled group, at the lowest enclosing scope | "yes." |
| **9** | A preview is the uncommitted form of a source edit | "LGTM" |
| **10** | The core is its own crate with no Reify dependency | "Yes. As usual, keep the files small to further reduce lock contention." |
| **11** | No limit is a constant. Limits that change answers are snapshot settings (binary default, `reify.toml`, per-file pragma); machine and session budgets come with each request | "Limits as config only", then "Yes" |
| **12** | Build the new engine to full coverage, then switch every surface in one release. One entry point, with `eval`, `check` and `build` as demands | "LGTM", then "Yes" |
| **13** | The first store is in memory only; the FEA disk cache comes back later | "acceptable"; the recovery rule "might need adjustment based on other changes we make" |

Also ruled in discussion:

- **Structure values are closed** (Example 11, option B). Leo first: "I think so. I'm finding it a bit slippery to reason out". Then, after the three options were set out: "Yes". His hunch on the way: `let c = Plate()` followed by `sub d = Widget(c)` "should be semantically identical to" `sub d = Widget(Plate())`; and on a version of option C limited to a constructor that is the whole right-hand side: "Absolutely not - confusingly inconsistent."
- **A sub and a value handle `auto` and constraints the same way** (section 8, "Who decides an unknown"). Leo: "let and sub should behave the same way (in how they handle auto and constraints). Objectives will probably have to differ."
- **Who owns an `auto`, what a structure value carries, and what is not there** were ruled in the session that designed the instance kinds, each "LGTM" to that design's recommendation (2026-11-25): an `auto` belongs to the instance of the definition whose text contains it, and only a constraint inside that instance bears on it (section 8); a structure value is its type's name and arguments, its parameters, and every value decided at or below its instance (5.4, with the exception this makes in 5.3); a guarded member reads its own guard first and is absent under a false one (7.3, Example 3). Later the same day he ruled there that a constraint written in the braces after a sub, and a purpose's constraints, bear on the instance's own `auto`: "this is what aligns best with the purpose of the braces on a sub: customizing the instance that is being created." He reversed the purpose half the next day (below); the braces half stands. How objectives travel, and how groups are formed when unknowns have different owners, are still the solve design's.
- **The source is the sole definition of a design.** Asked in the same session, on 2026-11-26, whether a root's declaration could carry a value for a root's required parameter, Leo: "No. This adds a semantically critical value that lives outside the source. The source must remain the canonical sole definition of the design. If the source can't represent it we either improve the expressive power of the source (judiciously - the bar for expanding the language should be high to prevent accretion) or we refuse it." So a root's declaration names a definition and carries no arguments (7.5). The same day he agreed that editing a literal in a constructor call inside an expression is not refused ("I don't see why this should be refused"), and settled which parameters get a direct control (Example 8). That principle collided with purposes as this contract had them: which purposes were active was a session setting, and a purpose's constraints bore on its subject's own `auto`, so an active purpose changed solved values from outside the source. The same day he reversed the purpose half of the ruling of the day before. A purpose only checks: "the purpose is a predicate that checks that a design is fit for a particular purpose." An objective written in a purpose is a compile error. A purpose is declared, then applied in source: "a declare then apply pattern (similar to the declare then instantiate for structures), so that the source can express application." (Section 8, 3.1, Example 9.) With purposes applied in source, none of Reify's kinds read an input attached to a node's own path, a kind of binding that 2.1 had offered and 7.3 had mentioned, and one that let a session change an answer at a place from outside the source. Asked the same day, in the session that wrote the core's PRD, whether to remove it from the core, Leo: "do delete it". (2.1, 7.3.)
- **The next step is the PRD for the core crate**, with the three fragile mechanisms built and tested first, authored by a separate session run as a team. Leo: "/spawn (fable, x-high) (/team (/prd this))".

Also agreed in discussion:

- **The content of a result too big to hash** is the hash of its key plus a nonce when it is not deterministic (5.2). Leo proposed it.
- **"Deterministic"**, not "exact" (5.6). Leo proposed it.
- **Source is identified by meaning**, at the level of the compiled tree, not text (3.1). Leo proposed it.
- **What was there last time is reached through the index** (5.7). "Yes."
- **The old engine's tests** are not to be preserved for their own sake: "Beware test bloat." "My strong lean is towards doing it right going forwards rather than preserving code over the short term. Design the engine result types based on the needs of the engine and the project."

One consequence of Decision 13, given 5.2: a result can be persisted together with every nonce-carrying result it read, which is simpler than the rule first proposed.

## Tentative

- **Decisions 2 and 3** are "I think so".
- Nothing else. Whether a sub handles `auto` the way a value does stood here; it is now ruled, above.

## Left to later designs, each needing a ruling then

- **The solve design:** the points listed in section 8, of which how objectives reach a sub's own unknowns is the first.
- **The geometry design:** what `self.a.body` means inside a parent operation when `a` has a placement; how to name "that bit" of a shape. Leo sees the second as the representational layer of a problem that concerns him at the UX layer, geometry selectors, and has no further contribution yet.
- **The instance design:**
  - what the roots are; whether a definition is evaluated on its own; guards per instance; what the GUI shows for an instance; purposes over more than one subject;
  - following from the ruling on values: that a `connect` endpoint and a purpose's subject must be places, since both compile between values today; that equality of values and of geometry is by content, where the spec now says "identity" and "same evaluation graph node" (a third point, that a child declared for a value reads no setting attached to a path, fell away on 2026-11-26, when the input attached to a path was removed);
  - that a definition's constraints become checked on values, at the 150 call sites the census found.

---

# Part D. Left out on purpose

Each with what should prompt taking it up.

- **Starting from the nearest earlier result, not the last one at this path.** It needs a second index beside the exact key: per recipe, the numeric inputs each stored result read, searched for the nearest. It must not be done by making the hash preserve distance, which would fight the hash's job as identity. Three things keep the door open at no cost: nothing from last time enters a key; the core chooses the starting point and hands it over; stored results keep the numeric values they read. *Take it up when:* scrubbing a slider back and forth, or solver trials with geometry in the loop, show starting points being missed.
- **Bodies on several threads.** *Take it up when:* a real design shows the one executor thread, not the kernels, as the limit.
- **A store that outlives the process.** *Take it up when:* the loss of the FEA disk cache (Decision 13) costs more than it is worth, or a second machine is wanted.
- **Limits by pragma inside one definition**, not per file. *Take it up when:* a design needs a deep limit for one recursive definition and not the rest.
- **Sharing index entries between identical instances** (item 8). *Take it up when:* a measurement shows per-path bookkeeping dominating.
- **Hints about what a body will read**, so inputs can be started early. Dropped from this draft: with waiting bodies and one thread they buy nothing. *Take it up when:* bodies run on several threads.

---

# Part E. Where this follows and where it departs

| Point | Original design | This contract | Literature |
|---|---|---|---|
| Dependencies | Reads recorded during evaluation | Follows | Adapton; Shake; Salsa |
| Static analysis | An optimisation for simple cases | The language's business; not in the core | |
| Immutable snapshots | Yes; the snapshot holds values and the graph | Follows on immutability. Departs: inputs only | |
| Provenance on snapshots | Edit, elaboration, resolution, merge | Dropped. A snapshot is its inputs and a link to its parent | |
| Result key | (path, hash of inputs) | Departs: recipe and contents read, no path | Constructive traces (Bazel, CloudBuild). Nix and Bazel both lost sharing to locations embedded in results |
| Hashing | 128-bit, floats by their bits, a match is presumed equal | Follows | |
| Validity | Replay the recorded reads | Follows, and makes it the storage rule | Verifying traces (Shake) |
| Early cutoff | Compare outputs, per kind | Follows, by value hash in the key; none through a shape, as in the original | The 2018 taxonomy's "deep" traces (Buck, input-addressed Nix) cannot cut off |
| What changed | Dirty cone forward, demand cone backward, intersect, four priority classes | Work outward from the change over the kept set, stopping where content is unchanged. Priority from the demand | Incremental's stabilisation; Adapton's demanded graph |
| Not-ready input | Tasks on an async runtime; nested solves suspend | Follows: a body waits. The expression evaluator's adapter is the exception | Shake and Buck2 suspend; Bazel Skyframe reruns and warns the naive form is quadratic |
| Kernel across a wait | Not addressed | Cannot be written | |
| Structure | A node returns a subgraph; diff and reconcile. "No separate phases" in the structural document; two phases in the architecture document | Declarations, resolved on demand, no phases, no diff | Salsa tracked structs; Skyframe keys |
| Identity | Path | Path for place, content for reuse; every node has both | Adapton with names: content alone cannot reuse a changed computation |
| Realization | One node per (instance, representation, tolerance); parent composes | Follows. Tolerance in classes; "a tighter result may satisfy a looser request" is not offered | |
| Solve | First document: an edge, written as a new snapshot. Architecture: a state transition | An edge, with no write into any snapshot | No engine fits a numeric solve as a graph cycle; iteration inside one node |
| Solve identity | One per scope, decomposed inside | One per coupled group, homed at a scope | |
| Trials | Child snapshots, same engine | Follows, as views; structure may differ per trial | No single-engine equivalent found |
| Coupled scopes | Reported; the designer widens the scope | Can be solved together; when, is left to the solve design | |
| Determinacy | Four states; "only increases" | A cell's value carries its determinacy; "only increases" is dropped, since a solve can be redone | |
| Freshness | Four states, one marker | Departs: outcome and freshness | Buck2 and Bazel keep transient failures out of the cache |
| Gating ("only on final inputs") | Per-node policy | Left to the executor; a body cannot see freshness | |
| Warm starting | Previous state and "what changed" passed to the body; pools; three tiers | The previous result and what changed, through the index; extra state kept beside a result. Pools not built. Tiers 2 and 3 left out with a trigger | Skyframe's per-key state: discardable, never part of the value |
| Demand | Always-demanded set; the solve demanded by a special rule | Follows; the special rule goes, because readers pull the solve | Incremental's observers |
| Entry points | Not specified | One | |
| Diagnostics | Results plus an event journal; aggregation by journal query | Results; aggregation by a summary node; journal optional | |
| Limits on recursion | None | Settings, checked per path | |
| Concurrency | Async pool from the start | Async from the start; one executor thread first, heavy sections on resource threads | |

**The Salsa decision of March 2026** rejected adopting Salsa, with trial snapshots as the one blocking gap. Under this contract that ground stands and is stronger, since views and previews are both central. Of the others: the ground that parameters are mutable targets of resolution falls away. The freshness ground is weaker. The content-hash ground, rated minor in March, now counts against Salsa, because results by content are not native to it. That last grade is my judgement.

---

# Amendment log

Each change to Part A after this document landed is listed here: the section, the text before, the text after, the test that forced it, and the task that made it. Leaves ξ₃ and φ of `docs/prds/evaluation-core.md` also record here each wide run of the random-sequence test.

The text as landed includes the sentences approved with the PRD on 2026-11-25. They are listed, each with the model behind it, in `docs/prds/evaluation-core.contract-amendments.md`.

| Date | Section | Change | Forced by | Task |
|---|---|---|---|---|
| 2026-11-26 | 7.5, 3.1 | Added: a root's declaration names its definition and carries no arguments. Nothing was removed | Leo's ruling of that date, recorded in Part C: the source is the sole definition of a design | none (a docs commit) |
| 2026-11-26 | 8, 3.1 | Section 8: removed "and the constraints of a purpose attached to it" from what counts as inside an instance; added that a purpose only checks, its constraints are never part of a solve's problem, and it holds no objective. 3.1: removed "which purposes are active and on what" from the settings; added that a purpose is applied in source | Leo's ruling of that date, recorded in Part C: a purpose only checks and is applied in source | none (a docs commit) |
| 2026-11-26 | 2.1, 7.3 | 2.1: removed "or a snapshot input attached to this node's own path" from the kinds of binding; added "No input is attached to a path." 7.3: "An instance then reads only its own presence guards, the source of its definition, and the settings attached to its path" became "An instance then reads only its own presence guards and the source of its definition." | Leo's ruling of that date, recorded in Part C: "do delete it" | #10617 removes the kind from the crate |
