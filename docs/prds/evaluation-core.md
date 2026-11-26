# Evaluation core

**Status:** APPROVED by Leo on 2026-11-25, decision by decision (section 7). It was decomposed the same day into 26 tasks; section 12.2 gives each leaf's task id, and section 12.5 lists what the decomposition changed in this text. One adversarial review has been folded in, and a second seat checked the fold; what that second check found is folded in too, and has been checked only by the author.

**Contract:** `docs/evaluation-core-contract.md`, which lands in the same commit as this PRD. This PRD builds what Part A of that document specifies and does not restate it. "Contract 4.4" below means section 4.4 of it.

**Code anchors:** by symbol or file. Statements about the workspace were verified at main `61b77ee152` (2026-11-25); re-locate at implementation time.

## 1. Goal

A new crate holds the core of Reify's new evaluation engine: the one mechanism that runs, stores and invalidates every computed thing. It is generic over what a node computes and depends on no Reify crate. This PRD is step 1 of the migration outline and stops there. When it closes, the crate passes its own invariants on toy node kinds, and no Reify surface uses it yet.

What can be observed when it lands:

- The crate's tests run in the merge gate and hold the invariants of contract section 11 (G0 to G4), the two-places test of 5.4 for every toy kind, and the debug mode of 4.4.
- Each counter-example the contract's critics built is a named test of the crate (`docs/prds/evaluation-core.counter-examples.md` lists 74, and five added in review).
- A growth report printed from the counters of 2.4: body runs, store lookups, paths resolved and index entries touched at N = 100, 1,000 and 10,000 on the synthetic models of section 11, plus a measurement at 10⁵ to 10⁶ cells (the contract's open item on cost per node).
- The contract's "provisional" status comes off in the closing leaf, after a recorded wide run of the random-sequence test with everything built, each of the six sections it covers having stood or been amended with Leo's approval (section 6).

## 2. Background

After a study of the engine as built against the architecture Leo designed (`docs/reify-implementation-architecture.md`), he decided to replace the engine's core with a new one built beside the old, to hold work on the old core meanwhile, and to prioritise the new one. He asked for the core's contract first, kept small, and for instances to follow as the first node kinds on it.

The contract was then drafted, attacked by critics over four rounds and discussed with him. He ruled on sixteen decisions; they are tabled in Part C of the contract. The ones this PRD is built on:

- A result is stored under what was computed and the content of what was read, never under a path.
- A node body is a task that waits for its reads, and can be run again.
- The core is its own crate with no Reify dependency, in small files.
- No limit is a constant in the core.
- There is one entry point: evaluate a demand in a state.
- The new engine is built to full coverage beside the old one before any surface switches.
- Tests are lean: invariants and counter-examples, not a wall of unit tests.

Two facts from the design session shape the order of work.

**Three mechanisms carried every late fault.** Each of the four critic rounds found three or four faults that gave wrong answers. After the first round, every one was in the same three places:

1. what a solver's trial evaluations are recorded as resting on (contract 6);
2. what a new snapshot may inherit from the one before it (contract 4.4);
3. nodes that a body declares in mid-run (contract 7.1).

Each fix was a new rule, and each new rule was where the next round found its faults. The last fixes have not been reviewed. The design session recommended building those three on toy kinds under a mechanical test in place of a fifth round of reading, and Leo agreed.

**The Rust shape was unproven.** Nobody had written the body interface in Rust: a body as an async task, a shared handle to the core, an exclusive resource lent inside a synchronous closure, kinds held behind one interface. Section 5.2 records what a probe written for this PRD established.

## 3. Scope

In scope: the crate; its toy kinds; its tests; the contract document's home in the repository; a note in the architecture document.

Not in scope, with the seam to each named in section 9:

- changes to code that is kept (migration outline step 2);
- the instance, solve, geometry and compute node kinds (steps 3 to 5);
- the session layer, the surfaces and the retirement of the old engine (step 6);
- the review of the tasks held behind #10510;
- any file under `crates/reify-eval`, with one possible exception: a generated list there, if section 5.3's check says it must be regenerated.

Left out of the core on purpose, as the contract's Part D lists: starting from the nearest earlier result, bodies on several threads, a store that outlives the process, limits by pragma inside one definition, sharing index entries between identical instances, and hints about what a body will read. None is built here and none is precluded.

## 4. Who consumes this

`/prd`'s first gate asks that every mechanism name a consumer: another PRD, or a surface a user can observe. This crate has neither, by design: Leo ruled that the core comes first and stays small, and the instance kinds that will be its first real consumer are being designed in a parallel session and have no PRD yet. So the gate is passed on a stated basis, for Leo to confirm (decision D1):

1. **Within this PRD every mechanism has a consumer in the crate's own tests**: a toy kind that uses it through the public interface, or the test harness acting as host. A mechanism with no such consumer is not built here.
2. **Each mechanism's later consumer is named by family**, from the contract's worked examples.
3. **The risk the gate guards against is a producer nobody ever wires in.** Here that risk is carried by the migration outline: steps 3 to 6 are the wiring, each a PRD of its own. If a later design needs something different from the core, the change comes back as an amendment to the contract.

| Mechanism | Contract | Consumer in this PRD | Later consumer |
|---|---|---|---|
| A body as a waiting task; `read`, `read_all` | 2.3, 2.5 | `cell` | every kind |
| An exclusive resource lent to a synchronous section | 2.3, 10, O4 | `shape`, `measure` | realize, measure, tessellate, compute; solver libraries |
| Results stored by recipe and contents read | 4.2, 5.1 to 5.3 | every toy kind; `cell` with refs computed from data | every kind |
| Structure by declaration: a result declares children | 7.1 to 7.4 | `scope`, `guard`, `collection` | instance scope, guarded member, collection |
| A child declared by a running body | 7.1 | `call` | compute, measure, the instance behind a structure value |
| Views | 6 | `solve` | the solve kinds |
| Snapshot, edit, and what a new snapshot inherits | 3.1, 4.4 | the test harness, as host | the session layer |
| Demand with priority; scheduling; abandoning work | 4.1, 10 | the test harness | the session layer and the GUI |
| Outcome and freshness; partial results; progress | 9 | `slow` | compute, the GUI's display |
| Determinism, the nonce, `previous`, `keep` | 5.2, 5.6, 5.7 | `warm` | solve, compute, realize |
| Ownership, trimming, release when the last holder lets go | 5.5 | `shape` | the geometry kinds |
| Limits as settings; budgets with a request | 7.6 | `scope` and `collection` (depth, children), `solve` (trials), the harness (budgets) | instance kinds, the session layer |
| Cycles | 7.7 | `cell`, `solve` | every kind |
| `reads` and `readers` | 2.4 | the test harness | "why" and "what depends on this" in the GUI and agent tools |
| Counters | 2.4 | the growth tests | the measurements the contract leaves open |
| The debug mode | 4.4, 5.6 | the random-sequence test | every later kind's tests |
| `seed` | 2.4 | the fresh-evaluation comparison of G0 | the same, for later kinds |

A leaf's signal in this PRD is therefore a named test that runs in the merge gate, or a number read from the counters. `/prd` normally rejects "a test passes on synthetic input" as a leaf's signal, because such a test can pass while the product's wiring is absent. Here there is no product wiring in scope, and the tests are the specification's acceptance. That is the same exception, and D1 covers both.

## 5. Sketch of approach

### 5.1 What the crate implements

Part A of the contract, sections 2 to 7 and 9 to 11. Section 8 (the solve) is not core: the crate has nothing for a solve that other nodes do not use, and the toy `solve` is there only to exercise views. Parts B to E of the contract bind later designs.

### 5.2 The Rust shape

The contract's pseudo-code was never compiled. A probe written for this PRD compiled the four unproven pieces together, on the workspace's pinned toolchain (rustc 1.96.0, edition 2024, stable features only), with std and `xxhash-rust` as its only library dependencies. It is about 1,100 lines of source and 700 of tests, kept at `/home/leo/.claude/spawn-briefs/reify-evaluation-core-contract/probe/` on the workstation, and is the starting point of leaf α. It is not committed: a tracked Rust `tests/` directory outside `crates/` fails a merge-gate ratchet (`tests/infra/test_no_new_wallclock_rust_deadlines.sh`), and the listing below is the durable record. The author of this PRD ran its tests again: 20 integration tests, 1 doctest and 5 must-not-compile doctests, all passing.

**What it established.**

- **A body is a future, and it holds nothing of the core's.** The handle a body is given (`Cx`) owns a one-slot mailbox and a cancel flag. A read puts one request in the slot and suspends. The executor serves the request, puts the reply in the slot and queues the body again. So "one request at a time" (O2) is a data structure, and a body cannot keep hold of the core while it waits, because it never has it.
- **Kinds behind one interface.** A kind author writes a trait with `type Out` and an `async fn run`. The core stores each kind in an erased form that boxes the future. Bodies are `Send`, so running them on several threads later breaks no kind.
- **Results are typed at each read, over a store that erases them.** A kind declares `type Out`. `cx.read::<T>()` returns an outcome holding a shared pointer to a `T`. The core itself has no type parameter. A read at the wrong type gives the reader a failure outcome that names both types; it is not a panic.
- **An exclusive resource lives alone on a thread of its own.** `core.add_resource::<R>(make)` runs `make` on a new thread, so a kernel need not be `Send`. `cx.exclusive(section)` posts a synchronous closure to that thread and waits for what it returns. While a section runs, the executor runs other bodies. Shown with a barrier, with no sleep and no clock.
- **The executor is the crate's own**, about 400 lines: a queue ordered by priority and then arrival, no recursion, and the wait relation in one place. A chain of 100,000 dependent cells runs on the default 2 MiB test stack. Neither `futures`' pool nor a tokio runtime offers priority order, dropping by request and a view of who waits for whom without a second structure beside it.
- **The core starts no thread for bodies.** The host calls `turn()` (run what is ready, never block), `wait()` (block until one section finishes) or `run()` (both, until idle). The thread that calls is the one thread that runs every body. A test is then exactly the order of its calls.

**What the compiler rejects** (each a program that fails to compile, with the first error of rustc 1.96.0):

| Program | Error |
|---|---|
| a section that waits | E0728: `await` is only allowed inside `async` functions and blocks |
| a section that returns a reference into the resource | lifetime may not live long enough |
| a section that returns a future borrowing the resource | lifetime may not live long enough |
| a section that takes the body's `Cx` with it | E0521: borrowed data escapes outside of function |
| a kind whose `run` holds a standard lock guard across a read (it must be a kind's `run`: the trait requires that future to be `Send`, and a free `async fn` that holds the guard compiles) | future cannot be sent between threads safely |
| an async closure given as a section | E0308: mismatched types, "one type is more general than the other", from the bound of `exclusive` itself (`F: FnOnce(&mut R, &Section<R>) -> T`) |

The first row fails whatever `exclusive` accepts: the compiler rejects `await` in a closure that is not async before it looks at any signature. The sixth fails only while the signature of `exclusive` holds, so it is the one that would catch a section allowed to wait. It was not in the probe. The review of leaf χ compiled it against the crate, and it was added on Leo's ruling (decision D9).

**What the compiler cannot reject.** Two programs compile that break the intent of O4: a resource type that hands out shared ownership of itself (a section returns a clone of an `Arc` of the resource, and the body keeps it across waits), and a section that blocks on a channel only a body or the host would fill. Both become stated obligations: on whoever adds a resource type, and on whoever writes a section (section 13, point 29). `trybuild` runs a program it expects to compile, so each of the two is written to do nothing when run: the section that blocks on a channel is in a function that takes the receiver as a parameter and that `main` never calls.

**Where the probe departs from the contract's pseudo-code.** Each is in the list of proposed amendments (section 13.2).

- **Release.** `Kind::release(out, resources)` cannot be written: a result is shared, so the core cannot hand it over by value, and release must run on the resource's thread. The probe replaces it with one mechanism. A section wraps a thing that lives inside the resource (`section.hold(thing)`), and when the last holder drops the wrapper, the thing is handed back to its resource's thread to be released. That covers a result the store drops, a section whose body was dropped, and a body dropped later while still holding what a section made. The last of these the contract does not cover today.
- **`exclusive` has no failure path.** A missing resource or a section that panics cannot resume the body with a value. The core ends the node with a failure outside the inputs (contract 9) and drops the body.
- **`emit`** cannot name `Out` on a `Cx` that is not typed by its kind. It becomes `emit::<T>` with a check at run time.
- **Seeding** is an argument when a core is built (a starting store), not a call on a running core.
- **What a body returns** gains a third case beside an outcome: a failure outside the inputs (point 13). The probe's `Produced` does not have it yet; leaf α adds it.

**What the probe does not show.** It has no snapshot, store, index or view. For each, it only shows that the shape has a place. Three things it flagged for the leaves:

- A store lookup must wait for the content of what the chain says was read next. So the executor has waiters that are not bodies, and the wait relation must cover both. An unconfirmed inherited entry is a third sort of waiter.
- Every read costs a suspend and a second poll even when the answer is ready. If the measurement of leaf φ shows that too dear, serving ready reads in the first poll means giving `Cx` a way to reach the index.
- A result that owns a long chain of other results is dropped by Rust recursively, on the machine stack. That is a constraint on a kind's result type.

**The interface, as compiled.** Names follow the contract. `Hash128` replaces the pseudo-code's `Hash`, which collides with `std::hash::Hash` wherever it is derived (code-quality heuristic 1, informative names).

```rust
pub trait Kind: 'static {
    type Out: Value;
    const ID: KindId;
    const VERSION: u32;
    fn run(cx: &Cx) -> impl Future<Output = Produced<Self::Out>> + Send;   // written `async fn run` by a kind
    fn content(out: &Self::Out) -> Option<Content>;
}

impl Cx {                                    // Send + Sync, not Clone
    pub async fn read<T: Value>(&self, r: &Ref) -> Outcome<T>;
    pub async fn read_all<T: Value>(&self, rs: &[Ref]) -> Vec<Outcome<T>>;   // leaf θ widens this to refs or child declarations, as contract 2.3 has it
    pub async fn exclusive<R, T, F>(&self, section: F) -> T
    where R: 'static, T: Send + 'static, F: FnOnce(&mut R, &Section<R>) -> T + Send + 'static;
    pub fn cancelled(&self) -> bool;
}

impl<R: 'static> Section<R> {                // nothing here waits
    pub fn cancelled(&self) -> bool;
    pub fn progress(&self, note: impl Any + Send);
    pub fn hold<P: Release<R>>(&self, thing: P) -> Held<R, P>;
}
pub trait Release<R>: Send + 'static { fn release(self, resource: &mut R); }

pub enum Outcome<T: ?Sized> { Value(Arc<T>), Failure(Failure), Absent }
pub struct Produced<T: ?Sized> { pub outcome: Outcome<T>, pub deterministic: bool }

impl Core {                                  // Send; driven through &mut self by one thread
    pub fn add_resource<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static);
    pub fn turn(&mut self) -> Turn;          // run what is ready; never blocks
    pub fn wait(&mut self) -> Turn;          // block until one section finishes, then take a turn
    pub fn run(&mut self);
    pub fn drain(&mut self);                 // not in the probe: run until no section is running, nothing a section
                                             // produced is waiting to be delivered, and no release is waiting to run
    pub fn counters(&self) -> &Counters;
}
```

The listing is the probe's. Leaf α builds it without `VERSION`, `content` and the field `deterministic`, which leaves δ and ο add (section 12.2). The rest of contract 2.3 (`child`, `suppose`, `previous`, `keep`, `emit`) is added by the leaf that implements it, in this shape: a call that needs an answer is one more request through the mailbox, and a call that only tells the core something is buffered in it.

**The host's half, not compiled.** The probe stands in for it with calls that declare and demand single paths. The crate's form (section 13, points 26 and 32):

```rust
impl Core {
    pub fn snapshot(&mut self, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Snapshot;
    pub fn evaluate(&mut self, snapshot: &Snapshot, demand: Demand, budget: Budget) -> Request;
    pub fn replace_demand(&mut self, request: &Request, demand: Demand);
    pub fn end(&mut self, request: Request);          // a budget can also end a request
    pub fn take_events(&mut self) -> Vec<Event>;      // answers and progress, each tagged with request and snapshot
    pub fn answer<T: Value>(&self, snapshot: &Snapshot, path: &Path) -> Answer<T>;
    pub fn reads(&self, snapshot: &Snapshot, path: &Path) -> Vec<Read>;
    pub fn readers(&self, snapshot: &Snapshot, path: &Path) -> Vec<Path>;
    pub fn trim(&mut self);
    pub fn into_store(self) -> Store;                 // a core's results, to build another core on
    pub fn selected_store(&self, keep: impl FnMut(&StoredResult) -> bool) -> Store;   // leaf δ: a new store sharing only the results `keep` selects
}
impl Snapshot { pub fn edit(&self, changed: impl IntoIterator<Item = (InputKey, Option<Input>)>) -> Snapshot; }
```

A request is live until the host ends it or a budget does. A core may have several live requests from the first leaf; until leaf τ they are served in order of arrival. A snapshot is live while the host holds it or a live request is on it. `edit` is a pure function of the snapshot. What a new snapshot inherits is taken when the first request is made on it, from what is kept on the snapshot it was edited from at that moment. So a host that wants inheritance makes the new request before it ends the old one, and ending the old one afterwards drops only the work nothing else needs. These calls are the crate's public interface, not a test-only one: the test harness is a host, and a session layer will need the same calls. `selected_store` leaves the core as it was; a `StoredResult` shows `keep` the id of the result's kind and, from leaf ο, whether the result is deterministic.

### 5.3 Dependencies and files

**Dependencies.** std; `xxhash-rust` (already a workspace dependency), used in one file; and a persistent map for a snapshot's inputs and its index, used in two files (decision D5). Tests add nothing, except as decision D9 says. Leaf α declares them all, because an edit to the root `Cargo.toml` or `Cargo.lock` makes a task's verify build the whole workspace. Wherever the order of a map's entries can reach an answer, a counter or the order of work, the crate uses an ordered map or a fixed hasher, so that two runs of a test do the same thing.

**Registration.** A new crate is registered in four places, all by leaf α: `members` and `[workspace.dependencies]` in the root `Cargo.toml`; `Cargo.lock`; the `spine` stratum of `scripts/crate-strata.json` (a crate that depends on no other workspace crate); and a `workspace-feature-union.workspace = true` line in the crate's own manifest. This is the set the newest leaf crate needed (`reify-version`, commit `5e15d946d2`). That crate has no third-party dependency. One that has may also need the generated part of `crates/workspace-feature-union/Cargo.toml` regenerated, and with it the generated list `crates/reify-eval/engine_hash_closure.txt`, if the new packages change which features an existing package is built with (`docs/notes/workspace-feature-union.md`). Whether they do is checked before the batch is filed (section 8). If they do, leaf α regenerates both with the script that owns them; that is the one case in which this PRD touches a file under `crates/reify-eval`, and it is a generated list, not engine code. Section 8 records what the check found: the first file gains one line and the list does not change.

**Files.** One concept to a file, and a file imports only from files in the layers below it or earlier in its own. The merge gate enforces the direction: two top-level modules that refer to each other fail it (`module-sccs-only-shrink`).

| Layer | Files under `src/` | Holds |
|---|---|---|
| 1. Values that cross every boundary | `hash` `step` `path` `reference` `failure` `outcome` `produced` `answer` `recipe` `input` `declaration` `demand` `budget` `counters` | the contract's vocabulary, as data |
| 2. The body interface | `cancel` `job` `held` `event` `section` `substitution` `previous` `mailbox` `view` `cx` `kind` `erased_kind` `kinds` | what a kind sees and supplies |
| 3. State | `snapshot` `limits` `key` `nonce` `content` `store` `trim` `path_table` `index` `kept` `confirm` `rested_on` | inputs, results and what each snapshot answered; no knowledge of scheduling |
| 4. Execution | `ready` `task` `waits` `cycle` `resource` `requests` `resolve` `record` `in_progress` `lookup` `limit_check` `serve/read` `serve/child` `serve/section` `serve/view_read` `finish` `executor` | running bodies and serving their requests |
| 5. The host | `host` `builder` `rederive` `lib` | what a host calls; the debug mode |

Sixty files. Two places hold more than one thing, for a stated reason: `index` holds reads and readers, which are one relation seen from two ends and must have one owner; `held` holds the `Release` trait and the guard it parameterises.

`executor`, `serve/*` and `finish` are where every structure meets, and they are where leaves will contend for a file. Each serve function takes the structures it touches as explicit parameters and imports only downward, so that the split is by kind of request and is not one unit cut into files (code-quality heuristic 14, no cheating).

`lib.rs` gains a line with every new file. That line is the one edit every leaf shares.

**Tests** live under the crate's `tests/` directory with the toy kinds, and can name only public items. So "tests drive public seams" is something the compiler enforces here. They are one test binary with modules, as the workspace's consolidated crates are, because every file directly under `tests/` is a separate link.

### 5.4 Toy kinds

A toy design is described entirely by snapshot inputs, as the contract requires of a real one (2.1: a node's source is a snapshot input it reads through a binding). The toy kinds interpret those inputs. Each kind is here because some part of the core has no other consumer; the last column says which.

| Toy kind | What it computes | What it is the consumer of |
|---|---|---|
| `cell` | Arithmetic over refs. A ref may be computed from an earlier answer (`elems[i]`). Division by zero is a failure. | `read`, `read_all`; reads that depend on values, so the store's decision tree; failures and "blocked by" |
| `scope` | Its result declares a fixed list of members, read from a source input. | a result that declares children; path resolution |
| `guard` | A scope that reads a condition first. Under a false condition its own answer is "absent"; otherwise it declares its members. The instance kinds read their guards this way. | "absent" as a node's own answer; a path under an absent node; structure controlled by a value |
| `collection` | Reads a count. Its result declares that many elements. | structure controlled by a value; one declaration comparison per child |
| `call` | A cell whose body declares a child at a call-site step, with a declaration built from argument values, optionally inside a loop. It may declare a child of its own recipe. | `cx.child`; recursion through body-declared children |
| `solve`, `solved` | `solve` reads its problem as data, tries candidates in a fixed order through a view, and returns a map from unknown to value. `solved` reads a solve and takes its part. A `solve` may start from `previous`. | `suppose`; the record of what trials rested on; structure under a view; a view inside a view |
| `shape`, `measure` | `shape` makes an opaque result inside an exclusive section on a toy kernel that hands out handle numbers from a counter and counts what it holds. From leaf ο it has a mode in which its result is not deterministic. `measure` reads a shape inside a section and returns a number. | `exclusive`, `hold`; content of a result that is not hashed by value; release; the two-places test |
| `warm` | Iterates from the state it kept last time. Not deterministic, by way of `previous`. | `previous`, `keep`, the nonce, returning last time's result within tolerance |
| `slow` | Publishes partial results, reports progress and polls `cancelled`. | `emit`, `progress`, `cancelled`; freshness |

The brief for this PRD listed six kinds. `guard`, `call` and `slow` are added because the counter-examples need them and because `cx.child`, `emit` and "absent" would otherwise have no consumer.

The toy kinds live under the crate's `tests/` directory, not in `src/`. They are then written against the public interface by construction, which is what a real kind will have (code-quality stance on tests: a test that reaches a module's internals is an interface-design smell). A counter-example that needs something the nine kinds do not do (a faulty kind, a kind that claims determinism falsely, a planning node that reads declarations only, a shape made from another shape, a variant of `solve` that substitutes differently) uses a variant or a kind of its own, written in the test against the same public interface. Such a kind is not added to the generator or the reference interpreter.

### 5.5 Test architecture

**The reference interpreter.** A direct recursive interpreter of a toy design: given a snapshot's inputs and a path, it computes the outcome from the meaning of the toy kinds, with an environment of substitutions for solver trials. It has no store, no index, no executor and no sharing, and it shares no code with the core. It is the contract's sections 6 and 7 written out as a function, including the rule that makes two view-openers which depend on each other a cycle. Writing it is expected to surface ambiguities in those sections; each is an amendment under section 6.

**Random sequences.** A generator, seeded and deterministic, produces a toy design and a sequence of host events:

- edit an input, starting from any earlier snapshot, not only the latest (a preview branches);
- evaluate a demand (a set of paths with priorities) to completion;
- drive a request for a number of turns, stop, and edit again, with the stopped request either left live or ended (edits arriving faster than evaluation). A turn runs every body that is ready, so a request is unfinished after one only while a section it needs has not been waited for; a design used for this event has a `shape`;
- evaluate a demand under a budget that ends it early, then edit again;
- stop demanding a path;
- trim the store;
- change a limit setting.

Solver trials are not host events. They happen inside `solve` bodies, and the generator reaches them by generating designs that contain solves over cells, guards, collections and other solves.

**What is checked after every evaluation (G0).**

1. Every final answer equals the reference interpreter's outcome for that snapshot and path, where the interpreter is handed the non-deterministic results the core's answers rest on and computes everything else itself. Separately, each non-deterministic result is within its kind's tolerance of the value the interpreter computes cold. Each toy kind that can return a result that is not deterministic states its tolerance with its meaning in the interpreter; for the toy `shape` it is that a `measure` of the two results gives the same number.
2. Every final answer equals the answer of a fresh core evaluating the same snapshot, seeded with the non-deterministic results the session's answers rest on. This is G0 as the contract states it. The fresh core is built on a store that `selected_store` makes from the session's core: until leaf ο every result is treated as deterministic and it selects none; from leaf ο it selects the results that are not deterministic.
3. The debug mode is on: the core re-derives each answer it gives from the index by following its stored chain, and the two agree.
4. The order check: a fresh core asked for the same paths in a different order gives the same answers. This needs no interpreter.

Check 1 catches a fault that a fresh evaluation and an incremental one would both commit. Check 2 catches dependence on history. Check 3 localises a fault in what was inherited to the moment it happens. Check 4 catches dependence on the order of requests.

For one rule the interpreter is not an independent witness: when two nodes that open views depend on each other, the interpreter has to decide "cycle" by a rule, and it will be the rule the core uses. There check 4 is the witness, since a wrong rule shows as answers that differ with the order of requests.

**Failing cases.** A failing case is shrunk (events removed, the design reduced) and printed as a literal that can be pasted in as a named test, with its seed. Every failure found while building the crate becomes a named regression test in the same change that fixes it.

**No property-testing library.** The generator and the shrinker are written in the crate's tests, on a small fixed-seed generator, as the workspace's existing randomised tests are (`XorShift64Star` in the literal round-trip tests of `reify-compiler`). `proptest` and `quickcheck` are not in `Cargo.lock`. The gate runs a fixed list of seeds; an environment variable widens the run outside the gate, and the last leaf of the three mechanisms records such a run (section 12). No test asserts a wall-clock bound. Three things could still make two runs differ, and each is closed: the order in which two resource threads finish (the harness waits for every running section before it takes a turn); a release that is still on its way to a resource thread (the harness drains before it counts); and the order of a hash map's entries (section 5.3).

**Reaching the faults.** A random design seldom has the shape a fault needs. So each leaf of the three mechanisms names the shapes its rule is about, and its test counts the generated cases of each shape at the gate's seeds and fails on a zero (section 12.1, rule 9). A shape is a property of the design and the events, not of the core's inner state: the reference interpreter records what each answer read, and the test finds the shape from that.

**Named regressions.** `docs/prds/evaluation-core.counter-examples.md` holds every counter-example the critics built that bears on the core: its smallest model, what went wrong in the draft it was found in, and what the contract now requires. Each is a named test. Section 12 lists them by leaf.

**Growth (G1 to G4).** Tests on the synthetic models of contract section 11 at N = 100, 1,000 and 10,000. They assert on the counters only, never on time.

**Two places (5.4).** For every toy kind, the same declaration is evaluated at two different paths in two fresh cores, the second after unrelated work on the toy kernel, and the two results are compared by the toy kind's own equality on their values, not by content: the content of an opaque deterministic result is the hash of its key, which is the same at both places whatever the result holds. In one core, the second place must cost no body run.

## 6. When a test contradicts the contract

The rules of contract sections 4.4, 5.7, 6, 7.1, 7.6 and 7.7 have been checked by hand only. The contract's own header says what happens when a test contradicts the text: the text is amended in the same change that fixes the code. This section is that rule in operational form. It applies to every leaf.

A leaf that cannot make a test pass while following the text does this and nothing else:

1. It keeps the failing case as a named test, in its smallest form.
2. It writes the amendment on its branch: the changed rule in `docs/evaluation-core-contract.md`, and a row in that document's amendment log giving the section, the old text, the new text and the test that forced it.
3. It stops and escalates to Leo with the model, the old text, the proposed text, and what else the change touches. It does not merge.
4. When Leo approves, the code, the test and the amended text land in one merge. If he does not, he says what to do instead.

A leaf never weakens a test to fit the code, special-cases the failing model, or departs from the text without an amendment.

What counts as a contradiction:

- Following the text gives an answer that differs from a fresh evaluation, or depends on the order of requests.
- The text does not say what should happen.
- The reference interpreter and the core disagree, and each follows a defensible reading of the text.
- Following the text breaks a growth invariant.

The same route applies to the rest of the contract's text. The decisions recorded in Part C of the contract are Leo's rulings: if the work shows one cannot stand, the escalation carries the evidence and the question, not a proposed rewrite.

Points known before any code is written are listed in section 13.

## 7. Decisions

Each is stated with what it costs. Leo ruled on D1, D2, D3, D5, D8, D9, D10 and D11 on 2026-11-25; his words are quoted under each. The others are engineering choices made here from the evidence; they stand unless he says otherwise.

### D1. On what basis the consumer gate is passed (RULED)

Section 4. The crate's consumers in this PRD are its own toy kinds and test host, and a leaf's signal is a named test. *Cost:* the crate can close with every test passing while nothing in the product uses it; the only guard against that is the migration outline's later steps. *Alternative:* hold this PRD until the instance kinds have one. That would put the three fragile mechanisms behind a design that is still in session, which is the opposite of Leo's "core first".

**Leo:** "Agreed. You're allowed an exception to that rule on this occasion."

### D2. The crate's name (RULED: `reify-eval-core`)

**Leo:** "How about reify-eval-core (It's the core of the Reify evaluation subsystem)". Adopted. The crate is `reify-eval-core`, at `crates/reify-eval-core`, with Rust path `reify_eval_core`.

The draft had recommended `reify-evalcore` and set `reify-eval-core` aside because it reads as a part of the old engine crate `reify-eval`. On his reading the name says what the crate is to the subsystem, and later crates of the new engine can follow it. What it costs is the same as any name with that prefix:

- A plain-text search for `reify-eval` finds it too. So do four assertions in `tests/infra` that look for `-p reify-eval` without an anchor, and any rule written as the prefix `crates/reify-eval` without its slash. All of those already match `reify-eval-fea-tests`.
- No script, nextest filter or stratum rule treats `reify-eval-*` as a family (checked at main `61b77ee152`): `reify-eval-fea-tests` is named exactly wherever it appears.
- The hold on old-engine work (task 10510) lists files under `crates/reify-eval/src/` by name, so it does not catch the new crate. A later mechanical re-run of that rule must match those files, not the prefix.

### D3. Where the contract lives, and the architecture document (RULED)

- The contract goes to `docs/evaluation-core-contract.md`, beside the architecture document and the language specification. It is a standing normative document that later PRDs will cite, not a PRD.
- This PRD goes to `docs/prds/evaluation-core.md`, the place for foundations tied to no milestone.
- `docs/reify-implementation-architecture.md` gets one sentence, added to the end of the existing first paragraph of section 2, saying that sections 2 to 6 describe a design never built in that form and that the contract succeeds them. The sections stay. The old engine's code cites them by number, and three files of `reify-eval` cite the document by line range (`cache.rs`, `freshness_walk.rs` and one test), so the sentence is added without adding a line. They are replaced when the old engine is retired.
- Once the contract is in the repository, that copy is the only maintained one. The working draft under `~/.claude/spawn-briefs/` keeps its history (Parts D, E and G) and gets a note at its head pointing at the repository copy.

*Cost:* until retirement the repository holds two descriptions of evaluation, one marked as superseded.

**Leo:** "LGTM"

### D4. The hash (decided here: the core's own type over the same function)

The core has its own `Hash128`, a 128-bit value computed with xxh3-128 through `xxhash-rust`, imported by one file. A host supplies only leaf contents (a kind's `content`, an input's content) and the core does all the combining, so a host needs the width and not the function: `Hash128::from_u128(content_hash.0)` from `reify_core::ContentHash`. *Cost:* two hash newtypes in the workspace, joined by one conversion where a Reify kind meets the core. *Rejected:* a hasher type parameter (it would appear on every key, store and index type and buys nothing); a small shared crate (it would be a Reify crate under the core, which Leo's ruling excludes, and only wraps `xxhash-rust`).

### D5. The persistent map (RULED: `imbl`)

A snapshot's inputs and its index need point lookup, insert, remove, a clone that shares with its parent, and "is this the same map". The probe ran those on both candidates.

| | `im` 15.1.0 | `imbl` 7.0.2 |
|---|---|---|
| In the workspace today | yes (`reify-ir`); in `Cargo.lock` and in the agents' package cache | no: five new `Cargo.lock` entries as the probe's build resolved them (`imbl`, `imbl-sized-chunks`, `archery`, and newer versions of `rand_xoshiro` and `rand_core`), and one download |
| Maintained | no. Its repository was archived on 2026-05-03, and the RustSec advisory "im is unmaintained" (https://rustsec.org/advisories/RUSTSEC-2026-0248, read 2026-11-25) names `imbl` as the replacement. | yes. It is the fork of `im`, with the same interface and fixes to the ordered map and the vector. |

Recommended: `imbl`, used only inside `snapshot.rs` and `index.rs`. The index is the structure the correctness test rests on. *Cost and risk:* since 2026-08-01 no commit on main has added a third-party package to `Cargo.lock` (every commit touching it was checked), so whether a sandboxed task can download one is untested, and leaf α is the first leaf. So the packages are fetched into the agents' package cache before the batch is filed (section 8); α then downloads nothing. *Fallback with no new package:* `im`, which passes the same tests; moving later is a two-file change.

**Leo:** "LGTM"

Separately, and not part of this PRD: the rest of the workspace depends on the archived crate through `reify-ir`. On Leo's instruction ("File it") that is task #10538, which should run after leaf α; whoever files α adds the edge.

### D6. The executor and exclusive resources (decided here)

As section 5.2: the crate's own executor; the host's thread runs every body; each resource alone on its own thread, keyed by its Rust type. *Cost:* about 400 lines of scheduling code to own. A host that must deliver an edit while its thread is blocked in `wait()` needs a handle that makes `wait()` return; leaf τ builds it. One kernel per Rust type means a second instance of a kernel needs a second type or an id; nothing here needs one.

### D7. The toy kinds (decided here)

The nine of section 5.4. Three more than the brief's six (`guard`, `call`, `slow`), each because a call of the body interface or a group of counter-examples would otherwise have no consumer.

### D8. Priority (RULED)

Section 11: file at `high`, pin the path from α to π after the existing pins. *Alternative:* pin ahead of the existing six. It changes nothing while those six depend on #9739, and would matter only on the day that gate lifts.

**Leo:** "LGTM"

### D9. How the gate checks what must not compile (RULED: a small leaf beside the main chain)

The property "a body cannot hold a kernel while it waits" is enforced by the signatures of `run` and `exclusive`. The direct evidence is a program that fails to compile. Two facts about this repository constrain how that evidence is kept:

- no gate runs doctests (the merge gate runs nextest, which does not), so a must-not-compile doctest would be a check that nothing runs;
- on stable Rust such a doctest passes for any compile error at all, a typo included.

| Option | Evidence in the gate | Cost |
|---|---|---|
| 1. `trybuild` in one test of the crate (recommended), as leaf χ beside the chain | each program fails to compile with the expected message | a new test-only dependency, fetched beforehand like D5's; one test that runs a nested compile; untested in this gate, which is why it is its own leaf and holds up only the acceptance leaf |
| 2. No gate check | none; the probe's result, recorded in section 5.2, stands as dated evidence | a later change that loosens a signature would not be caught |
| 3. Make the gate run this crate's doctests | fails for any compile error | an edit to the verify pipeline while it is being ported; still does not pin the reason |

If option 1 cannot be made to run in the gate, leaf χ stops and says so, and Leo chooses between 2 and 3 then. The must-not-compile programs are not written as doctests under any option.

**Leo:** "LGTM"

**Found at leaf χ on 2026-11-26, after the ruling.** Three things the table above did not have.

- The expected-output files of option 1 end in `.stderr`, an extension the lock-charter guard's list must gain before such a file can be tracked (section 8). That is a change in two repositories, and it went to Leo, who ruled on 2026-11-26: "Widen it." The widening is task #10618 here and task 11872 in dark-factory; leaf χ depends on the first.
- The test's first run in a build directory compiles the crate's dependencies again, in a directory of its own. It was measured once at 798 s, with the machine's load average between 155 and 308, against the default limit of 1,200 s for one test; the next run took 18 s. If the gate ends the test at that limit, that goes to Leo under the sentence above.
- The first program of section 5.2, a section that waits, fails with E0728 whatever `exclusive` accepts: the compiler rejects `await` in a closure that is not async before it looks at any signature. So that program is evidence about the language, not about this crate. A sixth program, an async closure that `exclusive`'s own bound rejects, would fail only while the signature holds. Task #10596 put it to Leo, who ruled on 2026-11-26: "Yes, add & amend." It is the sixth row of the table in section 5.2. Task #10619 adds the program after leaf χ, and leaf φ depends on it.

### D10. How the open points of section 13 are closed (RULED)

- The nine points of 13.1 (rules inside the three mechanisms): the leaf named finds the rule under test and brings it to Leo as an amendment. *Cost:* up to six leaves on the critical path may each stop once for a ruling.
- The 23 rows of 13.2: Leo approves the proposed sentences now, with this PRD, and they are applied to the contract before it lands. 9 of them are design choices and 2 more are forced by the compiler; the table says which. 10 touch a provisional section and may be amended again by the leaf that tests them. *Cost:* these sentences are new. One adversarial review withdrew or narrowed five of them; a second check then changed six more in small ways. *Alternative:* leave them all to the leaves, which adds up to 23 stops.
- Approval can be by the whole table or row by row.

**Leo:** "LGTM". Taken as approval of the whole table. Row 24 (a root's declaration is a snapshot input) is decision D11, which he approved separately. The sentences are in the contract.

### D11. Roots (RULED; sent to the Instance design session)

The contract says a root is "named by the session" and leaves what the roots are to the instance design. The core needs only the mechanical half: where a root's declaration comes from. Proposed: it is a snapshot input. A demand names a root; the core reads the declaration from the snapshot; a name with no such input answers "absent". Declaring or changing a root is then an edit like any other, and the rule that an answer depends on the snapshot alone has no exception. *Cost:* one more kind of input in contract 3.1, and so which roots exist becomes part of what "current" means in 3.2: the session derives the root inputs when it loads the source. A demand that names a root nobody declared answers "absent", like any path that does not exist, and a host that misspells a root is told nothing more. What roots exist stays with the instance design; that session has said it needs no common parent above two roots.

**Leo:** chose "Yes, a snapshot input" when asked directly, over "given with each request" and "wait for the instance design".

**Bounded by a later ruling of his (2026-11-26, in the instance design).** A root's declaration names a definition and carries no arguments: "The source must remain the canonical sole definition of the design." Contract 7.5 says so. For the core nothing changes: a root's declaration is a recipe and bindings like any other, and what a host may put in one is the host's rule. The toy designs of this PRD are not Reify designs; their roots bind the inputs they read.

### D12. Smaller choices made here

- A read at the wrong type, and a body that makes two requests at once, each give a failure outcome. Each follows from the key, so it is stored like any failure. One faulty kind cannot take a session down.
- A body that waits on something other than the core is caught only when the thing it waited on was not ready, so that failure is one outside the inputs and is not stored.
- A missing resource, and a body or a section that panics, are failures outside the inputs (contract 9): nothing is stored.
- The probe stays outside the repository (section 5.2). Leaf α's task names its path on the workstation. *Cost:* a task dispatched on another host would not find it and would rebuild from the listing in 5.2.

### D13. An input attached to a node's own path (RULED: removed)

The contract as landed let a binding name "a snapshot input attached to this node's own path": a value the session hangs on a place. Its one use among Reify's kinds was to say which purposes are active, and Leo ruled on 2026-11-26 that a purpose is applied in source. It was also a way to change an answer at a place from outside the source, which the principle of his ruling under D11 excludes for a Reify design. Leaf β had built the kind, and leaf γ's toy `scope` and test bed used it.

**Leo**, asked on 2026-11-26 whether to remove it from the core: "do delete it".

Contract 2.1 and 7.3 are amended (third row of the contract's amendment log). The toy `scope` reads its members and nothing else (section 5.4). The catalogue entry `equal_roots_share_results_but_not_place` is restated with two roots whose declarations are equal. Task #10617 removes the kind from the crate and its test bed. It runs after leaf δ, and leaves ε and η depend on it, so that no later leaf builds on the kind. It is not a leaf: it adds no named test, and the manifest is unchanged. *Cost:* one more task on the path from α to π. The tests of the test bed that exist only for the kind are deleted or restated, and the generator loses one of its two ways of changing what a scope declares; the other, an edit to the scope's list of members, stays.

### D14. What a kind outside the crate needs that the toy kinds did not (decided here, 2026-11-26)

The session writing the PRD for the instance kinds read the crate as landed and the store leaf's branch. It found three things that the contract has and that no leaf was named to build, because no toy kind needed them, and two things the interface lacked. None changes the contract.

| What | Why it was missing | Built by |
|---|---|---|
| A binding to a constant (contract 2.1) | no leaf named it, though leaf θ's toy `call` builds a declaration from argument values | leaf θ |
| `steps` of the kind interface (contract 2.2): the steps a result declares | the limit on children per node counts them, and leaf η's text did not say so | leaf η |
| A structured description on a kind's failure (contract 9 and 5.2) | the toy `cell` fails in one way and needs only a code | task #10621 |
| Reading the root and the steps of a path, and the parts of a ref, from outside the crate | no test took a path apart | task #10621 |
| A space on a step, so that a member's name, a call-site step and a child every node of a kind has are different steps when their text is equal | the toy kinds have only named members and indexed elements | task #10621; leaf θ puts the steps of body-declared children in a space of their own (section 15, 5) |

**"Presence undetermined" stays a failure.** Contract 7.3 says a guarded member whose guard is undefined answers with that failure; Leo ruled it with the instance design. The instance PRD session asked whether it could be a value that declares no children instead, because a kind's failure on the store leaf's branch carries a static code and their kind must name the undefined guard by ref. The failure is right for more than its wording. A path below a failed node answers that node's failure (leaf β built it so), so everything that reads below such a member learns that presence is undetermined, and why. Below a value that declares nothing it would read "absent", as it does under a false guard, and the two cases could not be told apart there. Task #10621 gives the failure its description. The contract does not yet say what a path below a failed node answers; that sentence is put to Leo.

**A crate outside this one may implement kinds before the interface stops changing** (rule 11 of section 12.1). The instance kinds can start after leaf δ. What changes after δ, as far as this PRD knows. For a kind: leaf θ widens `read_all`; leaf ο adds `deterministic` to what a body returns, and from ο a result is shared as a deterministic one only if its body says so (contract 5.6); leaf σ adds `emit` and `progress`, which break nothing. For a host: leaf η gives `evaluate` a budget of paths, leaves ρ and τ change budgets and demand, and leaf σ makes a pending answer carry the last result. *Cost:* the tasks of those leaves gain a file outside the crate, and their own verify then builds that crate too. The alternative, every instance kind waiting for leaf ο, puts them behind eleven more leaves. A leaf that stops on a contradiction (section 6) may change the interface in a way this list does not have. The instance PRD's session plans two such files, `crates/reify-eval-instances/src/core_seam.rs` for what its kinds build and call, and `crates/reify-eval-instances/src/host/core_calls.rs` for its host's calls, and will confirm the paths when its PRD is decomposed (the crate's name is not yet ruled). It says every result built through the first is claimed deterministic, and that file's head will say so for each helper. It will ask that leaf ο's task switch on the debug mode's re-run of determinism claims in the second.

**The toy solver is not offered outside this crate's tests.** The toy kinds live in the crate's test binary. A PRD that needs a solve kind before the solve PRD exists writes a small one in its own tests against the public view interface, from leaf ι on.

## 8. Preconditions, and what was verified

Nothing in any Reify crate is assumed. What is assumed, with how each was checked (workspace facts at main `61b77ee152`; read again at main `db25257628` when the batch was filed, and each still held):

| Assumed | Checked how |
|---|---|
| The Rust shape of section 5.2 compiles on the pinned toolchain (`rust-toolchain.toml`: 1.96.0) | compiled and tested in the probe; re-run by the author of this PRD |
| `xxhash-rust` with `xxh3` | a workspace dependency; the probe built with it offline |
| A persistent map with the five operations of D5 | both candidates passed the probe's test of them |
| A dispatched task can add a new third-party package | **not verified**, and avoided. On 2026-11-25 the session that filed the batch fetched the packages of D5 and D9 into the agents' package cache (`CARGO_HOME` in `verify_env` of `dark-factory-orchestrator.yaml`, which was `/tmp/reify-agent-cargo-home`), with Leo's leave. With `imbl = "=7.0.2"` and `trybuild = "=1.0.125"` (the newest of each that day) declared, the workspace's `Cargo.lock` at main `db25257628` gains seven packages: `imbl` 7.0.2, `imbl-sized-chunks` 0.2.0, `archery` 1.2.3, `rand_core` 0.9.5, `rand_xoshiro` 0.7.0, `trybuild` 1.0.125 and `target-tuple` 1.0.2. Everything else the two need is already locked. All seven are in the cache: resolving the workspace against it downloaded nothing. The cache is under `/tmp`; if it has been cleared since, leaf α needs the network |
| Adding those packages does not change the features an existing package is built with | **checked on 2026-11-25, and one package changes.** On a scratch copy of main `db25257628` holding the new crate's manifest and its registrations, `python3 scripts/workspace-feature-union.py check` failed with 38 findings, all the same: `imbl` depends on `wide` with its default features, so `--workspace` resolves `wide` 0.7.33 as `{default,std}` while every `-p <member>` resolves `{std}`. The same check passes on unmodified main. `generate` repairs it with one line in the generated region of `crates/workspace-feature-union/Cargo.toml` (`wide = { version = "0.7", features = ["std"] }`), which also adds `wide` to that crate's entry in `Cargo.lock`; the check then passes with 39 members. The list in `crates/reify-eval/engine_hash_closure.txt`, regenerated with the command in its header, came out unchanged (205 names), so no file under `crates/reify-eval` changes. `wide` is built once more with the wider feature set, and what depends on it with it. Leaf α's task says this and has it run both commands again, since main moves |
| A new crate needs the four registrations of section 5.3, and for a crate with third-party dependencies possibly the two generated files named there | the three newest leaf-crate commits (`5e15d946d2`, `27ed2334da`, `0bbd2da11f`) and a search for every script and guard that lists crates |
| A task that touches only the new crate verifies that crate alone; a merge verifies the workspace | `decide_scope` in `scripts/verify.sh`; `scripts/reify_path_attribution.py` |
| Integration tests under a crate's `tests/` run in the gate with no registration, if they are not heavy and assert no wall-clock bound | `.config/nextest.toml` (default 120 s period, 10 periods); the overlay's "Gate-test drift-guard registration" |
| No gate runs doctests | a search of `scripts/verify.sh` for `--doc` found comments only |
| A tracked file with a new extension passes the repository's guards | **not checked before filing.** `trybuild` reads each program's expected compiler output from a file named after the program and ending in `.stderr`. The lock-charter guard's list of extensions (`scripts/lock-charter-guard.sh --list-extensions`) has no `stderr`, so `tests/infra/test_lock_charter_guard.sh` fails once such a file is tracked. Found at leaf χ (#10554). Widening the list is a lockstep change here and in dark-factory's copy of it. Leo ruled on 2026-11-26: "Widen it." Leaf χ waits for that change, task #10618 |
| Clippy runs with `-D warnings` on all targets; there is no rustfmt gate | `scripts/verify.sh`; `CLAUDE.md` |
| The module-cycle ratchet runs at merge for every crate, and at task level only when `reify-build-utils` is affected | `scripts/assert-module-scc.py --help`; `docs/legibility/design-invariants.md` |
| A test path written in prose is checked against the repository | `tests/infra/test_cited_test_paths_resolve.sh`. This PRD and its catalogue name tests, never test files |
| A crate whose Rust files contain `#[cfg(not(debug_assertions))]`, `cfg!(debug_assertions)` or `cfg!(not(debug_assertions))` outside a comment, or `cfg_attr(debug_assertions, ignore` or `should_panic` must be listed in `scripts/release-sensitive-crates.txt` in the same change (found when the batch was filed) | `release_sensitive_set` in `scripts/release-scope-lib.sh`; `tests/infra/test_release_scoped_scope.sh`. The debug mode of contract 4.4 is therefore a setting given when a core is built, never one of those spellings |
| A directory named `tests`, `benches` or `examples` that holds tracked Rust and is not one of the roots the wall-clock ratchet scans fails that ratchet (found when the batch was filed) | `tests/infra/test_no_new_wallclock_rust_deadlines.sh`. The crate keeps its tests under `crates/reify-eval-core/tests` and has no `src/tests`, `benches` or `examples` directory |

Activation: Leo's approval, then the two checks above, made on 2026-11-25 when the batch was filed. No task of another PRD must land first.

## 9. Seams with other work

This PRD owns the core crate and the contract document. It owns no file under `crates/reify-eval`. Its neighbours do not exist as PRDs yet; each row says who will own the seam when they do.

| Neighbour | Direction | What crosses the seam | Owner of the seam | State |
|---|---|---|---|---|
| Instance kinds on the core (migration step 3). Being designed with Leo in the Instance design session. | consumes | The kind interface and `Cx`; declarations and bindings; what a snapshot input is. | This PRD owns the interface. The instance PRD owns its kinds, the roots, and the compiler change that gives each unit of source a content (contract 3.1). | in draft since 2026-11-26; decision D14 records what it asked of this crate |
| Solve kinds (step 4). Replaces the solve-node milestone #10296 and takes in #8422. | consumes | Views. A bridge for a solver library that calls back synchronously (contract 2.5). | This PRD owns views. The bridge is not built here: no toy kind needs it. The solve PRD owns it, as an addition to this crate. Section 5.2 says what this crate keeps open for it. | not authored |
| Geometry and compute kinds (step 5) | consumes | `exclusive`, `release`, content of results not hashed by value, `emit`, `progress`. | This PRD owns the calls. The geometry PRD owns freeing one shape in a kernel, which contract 5.5 names as a precondition of the geometry kinds, not of the core. | not authored |
| Session layer, surfaces, retirement (step 6) | consumes | `snapshot`, `edit`, `evaluate`, demand and budgets, `reads`, `readers`. | This PRD owns the calls. The session PRD owns "current", previews and commits, and where limit settings are read from (`reify.toml`, a pragma, the binary's defaults). The core holds no default. | not authored |
| Changes to kept code (step 2) | none | Nothing. No change to kept code has to land before this crate can be built. | The Held-task review session sorts the held tasks; some of them are these changes. | in progress elsewhere |
| The old engine, `crates/reify-eval` | none | Nothing. This PRD changes no engine code there. Section 5.3 names one generated list it may have to regenerate. | | |
| `docs/reify-implementation-architecture.md` sections 2 to 6 | supersedes | A note at the head of section 2 pointing at the contract. | This PRD adds the note. Replacing the sections is left to the retirement PRD: the old engine's code cites them by section number and by line, and those citations stay true only while the text stays. | with this PRD |
| `reify_core::ContentHash` | none | Nothing crosses. Section 7 (D4) says how a host converts. | | |

**Open items of the design record that this PRD does not settle** are listed in section 16, each with its owner.

**The children a body declared.** A host gets them from `core.reads` (leaf υ), which lists each child a path's answer declared with its step. A body has no read for them in this PRD. A kind whose readers need the list puts it where a reader can read it: in its own result, or, so that readers of its value are not disturbed, in a small child it declares whose declaration carries the list. The Instance design session asked for a core read instead. That would be a second sort of dependence, on what a node recorded and not on its content, inside the mechanism of contract 7.1; it is left out until a kind shows the alternative is not enough.

**Dependence per answer, not per cause.** A read that a view-opener makes through its view returns whether that answer depended on the substitution. A node evaluated under the view that did not open it gets no such flag for its own reads. A solve that must tell "this input was undefined because of the trial" from "this answer depended on the trial" gets there in two steps: the result names the cause of its undefined value by ref, and the solve reads that ref through its view. The Instance design session raised the point; it needs no change to the core, and a model where it is not enough would come back as an amendment.

Two demands from later designs that today's contract does not meet. Neither is needed by this PRD; both are recorded so the design that needs them brings an amendment:

- **Planning cannot see children that a body declares.** The contract says planning reads declarations only, never values (section 8). A child declared by a running body exists only as a read recorded by a run of that body, and the body reads values. So a plan cannot learn what unknowns lie inside an instance built in the middle of an expression without running that expression. Owner: the instance and solve designs.
- **A solve that couples two roots.** A ref never goes up and a binding is relative to the declarer, so a node that reads and substitutes in two session roots needs a common parent. Nothing says the session may declare one. Owner: the instance design (what the roots are).

## 10. Boundary scenarios

The crate has two boundaries: with a node kind, and with a host. In this PRD the far side of each is a stand-in (the toy kinds; the test harness). These scenarios are what a real kind and a real host will rely on, written from their side. Leaf φ gives each a test of its own name, with every toy kind present; the last column names the leaf that builds what the row guards. By rule 1 of section 12.1 a row may already pass before that leaf, by the slow path.

**Facing a node kind.**

| # | Scenario | Given | Then | Built by |
|---|---|---|---|---|
| K1 | A body reads something not yet computed | a `cell` whose input is another `cell` | the body is suspended once and resumed with the outcome; it runs once | β |
| K2 | A body asks for many things at once | a `cell` summing a 10,000-element `collection` with one `read_all` | one body run; the reads are recorded in the order given | β for the body run; υ for the order, which a test can see only through `reads` |
| K3 | A body reads through a binding that was not set | a `cell` reading binding `arg` | the read answers "absent" and is recorded | β |
| K4 | A result declares a child and something else reads it | a `scope` with a member; a sibling reads `scope / member` | the path resolves by reading the scope's result | β |
| K5 | A stored result is found at another place | the same declaration at two paths | the second costs no body run and equals the first | δ |
| K6 | A body's result is a failure that follows from its inputs | a `cell` dividing by a `cell` that is zero | the failure is stored and found again; a reader returns "blocked by" with the ref | δ |
| K7 | A body declares a child in mid-run | a `call` with one call site, its argument edited | the child's path is the same before and after the edit | θ |
| K8 | A body supposes values and reads through the view | a `solve` over a `cell`, a `guard` and a `collection` count | reads through the view see the substitution; structure follows it; the surrounding state is untouched | ι |
| K9 | A body borrows an exclusive resource | a `shape` on the toy kernel | the section runs on the kernel's thread; the executor runs other bodies meanwhile | α |
| K10 | A body tries to wait inside a section, or to carry the resource out of one | (source text) | it does not compile | χ |
| K11 | A body is dropped while its section runs | a `shape` whose request is ended | what the section produced is released; nothing is stored | α |
| K12 | A body starts from what it did last time | a `warm` node after an input change | `previous` returns the last final result at its path and its kept state | π |
| K13 | A body publishes a partial result | a `slow` node read by a `cell` | the reader's answer is intermediate and is not stored; it runs again when the input is final | σ |
| K14 | A kind claims determinism falsely | a toy kind that returns a counter and claims determinism | the debug mode reports it | ο |
| K15 | A result holds something its place gave it | a `shape` that copies a kernel handle number into its result | the two-places test fails for that kind | δ |

**Facing a host.**

| # | Scenario | Given | Then | Built by |
|---|---|---|---|---|
| H1 | Two snapshots with equal inputs | an edit and its reversal | they are the same snapshot; evaluating the second costs no body run | δ |
| H2 | A demand names a path that does not exist | a path under an absent `guard` | the answer is "absent" | β |
| H3 | Nothing unasked is computed | N parts, one demanded | body runs and paths resolved do not grow with N | β |
| H4 | An edit to one input | N parts all demanded; one part's parameter edited | the work does not grow with N | μ₁ |
| H5 | An edit arrives before the last evaluation finished | an evaluation left unfinished, then an edit | every answer equals a fresh evaluation's | ξ₁ |
| H6 | A demanded path stops being demanded, is edited around, and is demanded again | | its answer equals a fresh evaluation's | ξ₂ |
| H7 | A request runs out of its budget | a paths budget smaller than the demand needs | one error for the request; nothing stored for it; a second request is not stopped | η |
| H8 | A limit setting changes | a design legal at depth 3, with the depth setting lowered to 2 and raised again | the failure appears at exactly the paths too deep, and goes when the setting is raised | ξ₃ |
| H9 | Two demands of different priority | what is displayed, and a summary | among bodies ready together, what the first needs runs first | τ |
| H10 | "Why" and "what depends on this" | any answered path with a deterministic result | the debug mode's re-run of its body asks for exactly what `reads` lists and gives its result; `readers` is the inverse of `reads` | υ |
| H11 | The same snapshot, evaluated fresh and after a history | any sequence of section 5.5 | the answers are equal (G0) | γ onward |

## 11. Tasks, holds and priority

- **No hold applies.** No task of this PRD depends on #10510 (the holder that parks work on the old core) or on #9739 (the gate that parks the backlog behind the alpha release). The tasks are new and touch no old-core file. A task filed now is not parked unless an edge is added, so nothing has to be released.
- **Nothing from the held set has to land first.** The crate uses no Reify code. No change to kept code is a precondition of any leaf.
- **Priority** (PROPOSED, decision D8). Every leaf is filed at priority `high`. After the batch is committed, each leaf on the path from α to π is pinned with `set_task_priority_override(pinned=True, pin_order=N)`, in dependency order, after the six pins that exist today. Those six (tasks 9100, 9101, 9103, 9104, 9108 and 9112, leaves of the verify-pipeline port) each depend on #9739, so the new chain would in practice be first. A pinned task still waits for its dependencies; pinning only decides which ready task takes the next free slot.
- **Cost on the machine.** Every leaf but α touches only files inside the new crate, so its task-level verify builds and tests that crate alone (rule 11 of section 12.1 makes an exception of a leaf that must update a crate outside). The merge gate builds the whole workspace for every merge, as it does for any task. α also edits the root `Cargo.toml` and `Cargo.lock`, which makes its task-level verify a full-workspace one. All third-party dependencies are declared by α for that reason. The new ones (decisions D5 and D9) are fetched before the batch is filed, so no leaf downloads anything.

## 12. Decomposition plan

Twenty-six leaves. Labels are Greek, with a subscript where one mechanism is cut into rule families; each leaf's task id follows its label. *New files* are named from the table of section 5.3. *Edits* names the existing files a leaf changes. `tests/` files are not listed.

### 12.1 Rules that hold for every leaf

1. **The crate is correct after every leaf.** A mechanism that is not built yet is replaced by the slow, plainly correct behaviour: run the body again, or answer the path through the store. It is never replaced by an approximation. The same holds for whatever touches the unbuilt part: a reader of something that is not inherited is re-checked by answering that thing afresh; a lookup that waited for another run resumes its own lookup afterwards and never takes that run's result on trust. So the random-sequence test passes after every leaf, and a leaf that stops on a contradiction (section 6) leaves a working crate behind it.
2. **The interface grows with its consumers.** A call of contract 2.2 to 2.4 enters the crate in the leaf that implements it, together with the toy kind that uses it. Nothing is declared ahead of what reads it.
3. **Each leaf extends the test bed in the same change.** A leaf that adds a toy kind adds that kind's meaning to the reference interpreter and its fragment to the generator, and switches the generator feature on. From then on every later leaf is tested with it.
4. **A new concept is a new file.**
5. **Before asking for a merge**, a leaf runs `python3 scripts/assert-module-scc.py check`. Two top-level modules of the crate that refer to each other fail the merge gate, and the task-level verify does not run that check for this crate.
6. **Section 6 applies.** A test that contradicts the contract stops the leaf and goes to Leo.
7. **No limit is a constant** (contract 7.6). A bound on anything arrives in a snapshot setting, with a request, or in the settings a core is built with. A test supplies the value it needs.
8. **No test asserts a wall-clock bound, and no test depends on timing.** Growth is asserted on counters. A test that uses a resource thread waits for it through the core (`wait`, or the drain call of leaf α), never by sleeping.
9. **Each leaf of phase 3 shows that the generator reaches its mechanism.** The leaf's signal names the shapes its rule is about, each as a property of the design and the events that the reference interpreter can detect; the test counts how many generated cases have each shape at the gate's seeds and fails if a count is zero.
10. **The must-not-compile programs keep failing for their stated reasons.** A leaf that changes the interface the programs of leaf χ are written against (the kind interface, `Cx`, `Section`, what a body returns) updates those programs, and their expected compiler output where only its wording or position moved, so that each still fails with the error section 5.2 names, or still compiles. It never changes which error a program is expected to fail with.
11. **A change to what a crate outside this one is compiled against keeps the whole workspace compiling.** Once such a crate implements a kind or drives a core as its host, a leaf that changes a signature it is written against updates that crate in the same change, in the file the crate keeps for the purpose, and the leaf's task names that file. For a kind that is the kind interface, `Cx`, what a body returns, and how a declaration or an input is made. For a host it is the calls of contract 2.4: budgets, demand, answers. The merge gate builds the whole workspace, so a leaf that did not could not land. A leaf that only adds to the interface edits nothing outside. No such crate exists yet (decision D14).

### 12.2 The leaves

Signals are named tests that run in the merge gate. A name in `code font` is a test in `docs/prds/evaluation-core.counter-examples.md`, which gives its model and what the contract requires; it is listed under the first leaf whose prerequisites can make all of it pass. "G0 with X" means: the random-sequence test of section 5.5, with the checks and host events the crate has by that leaf, passes at the gate's fixed seeds with generator feature X switched on.

**Phase 1. The first real piece.**

- **α (#10553). The crate, the body interface and the executor.**
  Starts from the probe (section 5.2). Registers the crate and declares every third-party dependency (section 5.3). Builds the kind interface and its erased form, the mailbox and the handle a body holds, `read` of a snapshot input, `exclusive` with `hold`, the executor with its ready queue, resource threads, the counters for body runs and sections, a snapshot of inputs, and `evaluate` over roots (decision D11) returning a request that the host can end; several requests may be live, served in order of arrival. Ending a request drops the bodies only it wanted. A drain call runs until no section is running, nothing a section produced is waiting to be delivered, and no release is waiting to run. A body or a section that panics, and a section for a resource nobody added, end the node with a failure outside the inputs and hang nothing; after a section panics its resource is dead, and later sections for it fail the same way. Adds the census row of section 14. Leaves out of the probe what nothing reads yet: a kind's `VERSION` and `content` (leaf δ) and the determinism claim (leaf ο). The probe's host calls (declaring and demanding single paths, abandoning, absolute bindings, taking progress, a content by path) give way to the host's half of section 5.2, and its tests of later leaves go to those leaves: the ring of waiting bodies to ε, priority order to τ, the chain of 100,000 cells and `read_all` to β, equal results sharing one content to δ. Until leaf β adds `demand`, `evaluate` takes the roots to answer, and until leaf η adds `budget` it takes no budget. At this leaf a body reads only snapshot inputs, so the signal is written that way: the wrong type is a snapshot input read at another type, and a body holds what a section made while it waits on a second section. Toy kinds: a root `cell` over inputs, and `shape` on the toy kernel.
  *New files:* layer 1 except `step`, `demand`, `budget`, `recipe`; layer 2 except `substitution`, `previous`, `view`; `snapshot`; `ready`, `task`, `resource`, `requests`, `serve/read`, `serve/section`, `executor`; `host`, `builder`, `lib`.
  *Signal:* a second body runs to completion while a first waits on a section that is still running on the resource's thread, shown with a barrier and no sleep; a request ended while its body's section runs, and one ended while its body holds what a section made, each leave the toy kernel holding nothing after a drain; a read at the wrong type gives the reader a failure outcome naming both types; a body that makes two requests at once ends with a failure; a section that panics ends its node with a failure outside the inputs, the host receives that error as an event for the demanded path that needed it, and no later `turn`, `wait` or `run` hangs; the core can be moved to another thread; a section sees `cancelled` when its request is ended, and its `progress` notes reach the host, without waiting.
  *Prereqs:* none. *Unlocks:* β, χ.

- **χ (#10554). Programs that must not compile** (decision D9). Beside the chain.
  One test that compiles the programs of section 5.2 and expects each to fail with its stated error (five land with this leaf; task #10619 adds the sixth, decision D9), and the two loophole programs and expects them to compile, so that a change which closes a loophole is noticed too.
  *Signal:* that test, in the merge gate (boundary scenario K10). If the gate cannot run it, the leaf stops and reports what it found.
  *Prereqs:* α. *Unlocks:* φ.

- **β (#10555). Structure by declaration.**
  Paths, steps, refs, declarations and bindings, including a binding passed along (section 13, point 30). A result that declares children. Resolving a path by reading its parent. "Absent" as an answer. The index of one snapshot: within a snapshot a path is answered once. `read_all`. `edit`, with no reuse between snapshots yet: each snapshot is evaluated from nothing. A body that read something which failed outside the inputs is dropped, and the host receives the error as an event for each demanded path that needed it (section 13, point 13; from leaf σ that path's answer also shows as pending). Toy kinds: `cell` with refs (including refs computed from data), `scope`, `guard`, `collection`.
  If no body can run and some are waiting, the request ends with an error that names them. Leaf ε replaces that with the cycle rule.
  *New files:* `step`, `demand`, `path_table`, `index`, `waits`, `resolve`, `record`, `finish`. *Edits:* `path`, `reference`, `declaration`, `mailbox`, `cx`, `kind`, `erased_kind`, `serve/read`, `executor`, `host`, `counters` (paths resolved, which G3 asserts on).
  *Signal:* a chain of 100,000 dependent cells is evaluated on the default test-thread stack; `a_body_learns_its_declaration_only_through_recorded_reads`; a child bound to one of its declarer's own bindings reads the place that binding names, and the declarer reads nothing by binding it: with only the declarer demanded, the body of the bound place runs zero times; G3 (N parts, one demanded: body runs and paths resolved do not grow with N); G4 (a body that makes n reads runs once).
  *Prereqs:* α. *Unlocks:* γ.

**Phase 2. The test bed, then the store.**

- **γ (#10556). The reference interpreter and random sequences.**
  Test code only, and the largest piece of it: the reference interpreter for the kinds of β, with a memo per snapshot so that it is not exponential; the generator of designs and of host events (edit from any earlier snapshot; evaluate a demand; drive a request for k turns and then edit); the shrinker; the printer that turns a failing case into a test. Check 1 of section 5.5, and the order check. The generator makes no cyclic design until leaf ε.
  *Signal:* G0 with `cell`, `scope`, `guard`, `collection`. A wrong rule injected into the toy `cell` (a cell whose source hashes to a multiple of ten adds one), by registering in the core under test a faulty variant of `cell` in its place and never by a global or thread-local switch, is caught, shrunk until the shrinker can remove nothing more, and printed as a test. The printed text is compared with a committed copy, and that copy is a test of the crate: it builds the case, runs it with the faulty `cell` registered, and expects the check to report it.
  *Prereqs:* β. *Unlocks:* δ.

- **δ (#10557). The store.**
  Results filed under the recipe and the contents read, in order (contract 4.2, 5.1 to 5.3): a kind's `VERSION` and `content` enter here. The decision tree for reads that depend on values. Content of an outcome that is cheap to hash, of a failure, of "absent", and of a deterministic result that is not hashed by value (the hash of its key). A failure that follows from the inputs is a stored result, and a reader of it returns "blocked by"; a failure outside the inputs is stored nowhere. A core built on a store that already holds results, the call that hands a core's results to such a store (section 13, point 26), and `selected_store`, which makes a store of only the results a caller selects (section 5.2). Until leaf ο no body can say whether its result is deterministic, and every toy kind built by then is, so every result is treated as deterministic and shared; from leaf ο the default of contract 5.6 takes effect. From this leaf a new snapshot starts with an empty index and is answered through the store. Check 2 of section 5.5. The two-places test of contract 5.4, written once over the table of toy kinds so that every later kind is covered when it is registered. Toy kind: `measure`.
  *New files:* `recipe`, `key`, `content`, `store`, `lookup`. *Edits:* `kind`, `erased_kind`, `finish`, `serve/read`, `builder`, `host`, `counters`.
  *Signal:* G0 with edits; after an edit, body runs are at most the number of nodes whose read contents changed (early cutoff); an edit and its reversal cost no body run; N identical instances cost the body runs of one when evaluated one after another (G2); a kind registered with the same id and a higher `VERSION`, in a core built on the store of a core that used the first, finds none of the first kind's results; a cell that read a shape whose section panicked is stored nowhere, and an identical cell elsewhere runs its own body; `values_of_different_types_never_share_a_content`; `a_body_never_observes_an_unready_read`; `two_places_check_catches_resource_minted_id_in_result`; `order_check_detects_result_mutated_through_exclusive_resource`; the same child declaration reached through two declarers one after the other, whose passed-along binding names places of equal content, costs one body run; the two-places test for `cell`, `scope`, `guard`, `collection`, `shape`, `measure`.
  *Prereqs:* γ. *Unlocks:* ε, η.

- **ε (#10558). Cycles.**
  A ring of running bodies gives every node on it the failure "cycle", described the same way wherever it was entered. Nothing is stored for it (contract 7.7). Found on the waits. A path being answered in a snapshot has at most one run at a time, and a read of that path waits on that run (section 13, point 8a). A chain of waits that passes through a lookup is not a cycle: that lookup misses and its body runs.
  *New files:* `cycle`. *Edits:* `waits`, `executor`, `failure`, `lookup`.
  *Signal:* G0 with cyclic designs; `two_cell_ring_answers_cycle_on_both_whatever_is_demanded_first`; `conditional_mutual_reference_without_a_ring_of_running_bodies_is_not_a_cycle`; `a_binding_to_a_sibling_place_is_not_a_read_by_the_declaring_scope`.
  *Prereqs:* δ. *Unlocks:* θ.

- **η (#10559). Limits as settings, and the paths budget.**
  Depth of a path (the number of steps below its root, counting steps a body declared) and children per node, checked when a path is first answered in a snapshot, against snapshot settings; never stored (contract 7.6). The kind interface gains `steps` here (contract 2.2), the steps a result declares: the limit on children per node counts them, and the toy kinds that declare children supply it (decision D14). A budget of paths resolved, given with a request: exhausting it ends the request with an error, and nothing is stored or kept for anything that met it.
  *New files:* `limits`, `limit_check`, `budget`. *Edits:* `serve/read`, `requests`, `host`, `counters`, `kind`, `erased_kind`.
  *Signal:* G0 with limit settings in the generated designs, and with the host events that change a limit setting and that end a request by its budget (until leaf μ₁ each snapshot is answered afresh through the store, so neither needs inheritance); `depth_limit_failure_is_a_fact_about_the_path_not_shared_by_content`; `shared_exponential_tree_ends_by_paths_budget_not_node_budget`.
  *Prereqs:* δ. *Unlocks:* θ.

**Phase 3. The three mechanisms.** One rule family per leaf. Until the leaf that inherits it lands, a kind of node is simply not inherited (rule 1).

- **θ (#10560). Children declared by a running body** (contract 7.1; mechanism 3).
  `cx.child`. The declaration is recorded with the read, so an identical parent elsewhere is found without running its body and the child can be reached by path from outside. The child exists while its parent's current answer records declaring it. Children per node counts both sorts. Toy kind: `call`. A declaration built from argument values needs a binding to a constant (contract 2.1), and it enters here: a value with a content, held in the declaration and part of the declaration's content; a read through it answers the value, and steps below it name nothing. The form of a step for a child a body declares is chosen here (section 15, 5), in a space of its own (decision D14).
  *New files:* `serve/child`. *Edits:* `mailbox`, `cx`, `record`, `resolve`, `lookup`, `limit_check`, `declaration`, `step`.
  *Signal:* G0 with `call`, reaching a body that declares a child on one branch only and a child declared inside a loop; `children_declared_in_a_loop_get_distinct_paths`; `one_inner_call_site_reached_from_two_outer_calls_gets_two_paths`; `body_declared_child_is_shared_and_resolvable_without_rerunning_its_parent`; `unbounded_body_declared_recursion_stops_at_the_depth_limit_with_one_failure`; one `read_all` of three child declarations on three resources starts all three children before the parent's single wait completes (the part of `read_all_of_child_declarations_starts_every_child_before_waiting` that needs no view of what was recorded; leaf υ owns that test).
  *Prereqs:* ε, η, task #10621. *Unlocks:* ι.

- **ι (#10561). Views** (contract 6; mechanism 1, first part).
  `cx.suppose`, reading through a view, a substitution at one path, structure under a view, which answers depended on a substitution, and the rule that a body which has opened a view reads nothing outside it afterwards (O9). **In this leaf the result of a body that opened a view is not stored**, and such a body runs again whenever it is asked for in a new snapshot. A result computed under a view is stored only when it did not depend on the substitution. Toy kinds: `solve`, `solved`.
  *New files:* `substitution`, `view`, `serve/view_read`. *Edits:* `mailbox`, `cx`, `record`, `finish`, `waits`, `cycle`.
  *Signal:* G0 with solves over cells, guards, collections and calls, one view deep, reaching a trial that changes a guard and a trial that changes a count; `a_substitution_under_an_absent_parent_answers_absent`; `an_unassigned_unknown_inside_its_own_solve_never_reaches_the_solve`; `solve_reached_again_under_its_own_view_is_a_cycle_and_terminates`; `read_outside_view_after_opening_it_fails_the_run`; `view_reports_which_answers_depended_on_substitution`.
  *Prereqs:* θ. *Unlocks:* κ.

- **κ (#10562). What a view-opener's trials rested on** (contract 6, sixth bullet; mechanism 1, second part).
  The record of everything outside the substitution that the trials rested on, as the last step of the body's chain. Storing a view-opener's result and finding it again. Results computed under a view are stored like any others. From this leaf the last clause of `read_outside_view_after_opening_it_fails_the_run` can fail, since view-openers are now stored, and that test asserts it.
  *New files:* `rested_on`. *Edits:* `record`, `store`, `lookup`, `finish`, `serve/view_read`, `builder` (how many alternatives the store keeps is a setting, point 20).
  *Signal:* G0 as for ι, with the same answers and with view-openers now found in the store, reaching an edit that changes a rested-on member, one that changes nothing a solve rested on, and one after which a member depends on a substituted path; `solve_rests_on_ordinary_values_its_constraints_read_under_trial`; `rests_on_member_with_changed_content_invalidates_stored_solve`; `solve_rests_on_inputs_its_trial_nodes_read_after_edit_and_across_places`; `rests_on_check_evaluates_members_as_ordinary_answers_of_the_snapshot`; `rests_on_check_stops_at_first_changed_member_in_first_read_order`; `rests_on_check_that_waits_on_a_running_reader_misses_instead_of_cycling`; `stored_solve_is_not_found_when_a_rested_on_member_comes_to_depend_on_a_substituted_path`; `final_state_finds_trial_results_when_solver_substitutes_final_content`; `change_and_change_back_refinds_a_view_opening_node`.
  *Prereqs:* ι. *Unlocks:* λ.

- **λ (#10563). A view inside a view** (contract 6, last bullet, and 7.7; mechanism 1, third part).
  A node evaluated under a view opens a view of its own. Two view-openers that depend on each other are a cycle, described the same way whichever is asked for first.
  *Edits:* `substitution`, `serve/view_read`, `rested_on`, `cycle`.
  *Signal:* G0 with nested solves under the order check, reaching an inner solve that follows an outer trial and a pair that depend on each other; `mutually_dependent_solves_fail_as_one_cycle_whatever_the_demand_order`; `solves_coupled_through_arguments_are_a_plain_ring`; `inner_solve_with_outer_dependent_inputs_follows_each_outer_trial_without_cycle`; `inner_solve_follows_outer_trial_when_its_view_reads_another_part_of_the_outer_substitution`; `element_solve_independent_of_the_count_answers_under_every_count_trial`; `inner_solve_infeasible_under_all_but_one_outer_trial_is_found_by_the_outer_solve`; `guard_that_reads_an_unknown_is_evaluated_per_trial_not_a_cycle`.
  *Prereqs:* κ. *Unlocks:* μ₁.

- **μ₁ (#10564). What a new snapshot inherits** (contract 4.4; mechanism 2, first part).
  The kept set: what live requests demand and what those answers read. A request stays live, and its demand kept, until the host or a budget ends it; what a new snapshot inherits is taken when the first request is made on it (section 5.2). Inherited entries, unconfirmed until confirmed. Working outward from the inputs that changed and stopping where content is unchanged. Never bringing a node up to date on behalf of a reader whose earlier reads have changed. Turning a ref into a path is recorded as a read of each declaration it passed through, in the index entry and not in the key (point 30). The debug mode of 4.4, which is check 3 of section 5.5. **In this leaf a new snapshot inherits nothing when, at its first request, a request on the snapshot it was edited from is live and not yet answered in full (leaf ξ₁ builds that case), or when its limit settings differ from that snapshot's (leaf ξ₃ builds that case); and a node whose answer declared children from its body, anything at or under such a child, and a node that opened a view, are not inherited**, and whatever read one of them is re-checked by answering it afresh. The leaf adds the counter of index entries touched, on which G1 is asserted. The kept set, the confirmation and the order of re-checking are one algorithm and cannot land apart; the debug mode is that algorithm's check and has nothing to check before it.
  *New files:* `kept`, `confirm`, `rederive`. *Edits:* `index`, `requests`, `resolve`, `executor`, `host`, `counters`.
  *Signal:* G0 with inheritance on and the debug mode on, reaching an edit whose change stops at an unchanged intermediate and an edit that makes a reader leave a branch; G1's first three cases at N = 100, 1,000 and 10,000 (one part's own parameter; a value N parts read, changed so that an intermediate result stays the same; a scope with N children gaining a member and a collection going from N to N + 1); `work_after_an_edit_stops_where_content_is_unchanged`; `scope_gaining_a_member_compares_one_declaration_per_child_and_touches_nothing_below`; `absent_answers_are_indexed_with_their_ancestor_reads`; `repointing_a_passed_along_binding_reaches_the_child`; with `r = if detailed then big else estimate` and an edit that flips `detailed` and changes what `big` read, `big`'s body runs zero times.
  *Prereqs:* λ. *Unlocks:* μ₂.

- **μ₂ (#10565). Inheriting a node whose body declared children** (contract 4.4 with 7.1; mechanisms 2 and 3 together).
  *Edits:* `kept`, `confirm`, `serve/child`.
  *Signal:* G0 as for μ₁ with such nodes inherited, reaching a child that disappears and a child that appears while its parent's content is unchanged; `child_a_rerun_body_stops_declaring_becomes_absent`; `child_of_a_parent_answered_from_the_store_is_absent_when_not_declared`; `child_appearing_at_a_path_already_answered_absent_is_found`; `branch_left_by_an_edit_is_not_evaluated_before_its_reader_is_rechecked`.
  *Prereqs:* μ₁. *Unlocks:* ν.

- **ν (#10566). Inheriting a node that opened a view** (contract 4.4 with 6; mechanisms 1 and 2 together).
  A kept view-opener is inherited and confirmed against what its trials rested on.
  *Edits:* `kept`, `confirm`, `rested_on`.
  *Signal:* G0 as for μ₂ with solves inherited, reaching an edit after which something a kept solve's trials rested on comes to read that solve's unknown; G1's fourth case (N independent solve groups under one scope, one edited: one solve runs); `edit_that_routes_a_rested_on_member_through_the_unknown_is_a_miss_not_a_cycle`; `independent_solve_groups_fail_and_resolve_independently`.
  *Prereqs:* μ₂. *Unlocks:* ξ₁.

- **ξ₁ (#10567). Inheriting from a snapshot whose work was unfinished** (contract 4.4, third bullet).
  The next snapshot starts from the inputs still outstanding as well as its own. This happens only when the first request on the new snapshot is made while a request on the snapshot it was edited from is still live and unfinished. A request ended by its budget is not live and keeps nothing (leaf η), so it leaves no unfinished work to inherit; the generator's budget event still runs at this leaf, and shows only that nothing wrong is inherited.
  *Edits:* `kept`, `confirm`, `requests`.
  *Signal:* G0 with evaluations left unfinished (the host stops driving a request, leaves it live, and makes the first request on a snapshot edited from that one), and edits made from any earlier snapshot, reaching a path that the new request demands and that the earlier request had not answered when the new request was made; `edit_made_while_the_parent_is_still_evaluating_never_inherits_a_stale_answer`; `entry_inherited_from_an_unfinished_walk_is_reconfirmed_before_it_is_served`.
  *Prereqs:* ν. *Unlocks:* ξ₂.

- **ξ₂ (#10568). A changed demand** (contract 4.4, sixth bullet).
  A request's demand can be replaced. A path that stops being demanded leaves the kept set.
  *Edits:* `kept`, `requests`, `host`.
  *Signal:* G0 with demand changes, reaching a path that is dropped from the demand, has what it read edited, and is demanded again; `path_dropped_from_demand_is_not_inherited_stale`.
  *Prereqs:* ξ₁. *Unlocks:* ξ₃.

- **ξ₃ (#10569). A changed limit setting** (contract 7.6).
  It reaches every path whose check now comes out differently.
  With this leaf the three mechanisms are complete. It runs the random-sequence test at a width the gate does not (the environment variable of section 5.5) and records in the contract's amendment log how many sequences and events ran without a failure, as a row whose Section is "none", whose Change gives the seeds and the number of sequences and events, and whose Forced by is "wide run, no failure". It removes no provisional mark: later leaves still exercise those sections.
  *Edits:* `limit_check`, `confirm`.
  *Signal:* G0 with every host event of section 5.5 that the crate has by this leaf (all but trimming the store, which arrives with leaf ρ), reaching a limit change that makes a legal path too deep, one that makes a too-deep path legal, and a change of the children-per-node setting that moves a node across it; `changing_a_limit_setting_reaches_every_path_whose_check_changes`; the recorded wide run.
  *Prereqs:* ξ₂. *Unlocks:* ζ.

**Phase 4. What is built on them.**

- **ζ (#10570). Joining a run in flight** (contract 10).
  The same computation is not run twice at once. A lookup that finds a run in progress whose reads so far have the same contents waits for it, unless that run is waiting on the lookup, and then resumes its own lookup. It comes before ο because with results that are not deterministic it is a matter of correctness: one key must not get two live results.
  *New files:* `in_progress`. *Edits:* `lookup`, `waits`, `executor`.
  *Signal:* N identical instances demanded together cost the body runs of one (G2 under concurrent demand), for cells and for solves; `first_evaluation_of_a_fold_over_n_elements_runs_its_body_once`; `inner_solve_independent_of_the_outer_trial_runs_once`; two identical solves demanded together, whose problems read the same contents but whose places differ in something only their trials read, each get their own answer; G0 unchanged.
  *Prereqs:* ξ₃. *Unlocks:* ο.

- **ο (#10571). Determinism and the nonce** (contract 5.2, 5.6).
  A body says whether its result is deterministic, and from this leaf the default of contract 5.6 holds: a result is treated as not deterministic unless its body claims otherwise (until now every result was treated as deterministic, leaf δ). A result that is not, and is not hashed by value, carries a nonce in its content. At most one result per key is live. The debug mode re-runs a sample of bodies that claimed determinism, giving them the answers the first run had, and fails on different bits or different requests (O2, O8). How many it re-runs is a setting (point 20). Toy kind: `shape` gains its non-deterministic mode.
  *New files:* `nonce`. *Edits:* `produced`, `content`, `store`, `rederive`, `kind`, `builder`.
  *Signal:* G0 with a non-deterministic kind, compared as section 5.5 says; `seeded_reader_of_nondet_result_not_found_after_remake`; `debug_mode_catches_a_false_determinism_claim`; `answers_downstream_of_a_nondeterministic_result_equal_a_fresh_evaluation_seeded_with_it`.
  *Prereqs:* ζ. *Unlocks:* π.

- **π (#10572). What was there last time** (contract 5.7).
  `cx.previous`, `cx.keep`, one pointer per path that does not keep a result alive, "what changed" for reads made in this run, returning last time's result when the new one is within tolerance. A body that used `previous` returns a result that is not deterministic. Toy kind: `warm`; `solve` gains a start from `previous`.
  *New files:* `previous`. *Edits:* `mailbox`, `cx`, `path_table`, `store`, `finish`.
  *Signal:* G0 with `warm` and warm-started solves; `previous_is_last_final_answer_outside_views_and_changed_covers_only_reads_made`; `previous_found_at_body_declared_child_after_argument_change`; `kept_state_reaches_next_run_at_same_path_after_input_change`; `returning_the_previous_result_keeps_its_content_and_spares_its_readers`; `warm_started_hashable_result_with_unchanged_value_keeps_content`; `warm_start_never_changes_whether_a_solve_succeeds`; `warm_started_trials_file_an_alternative_rested_on_set_beside_the_cold_one`; `identical_lookups_in_flight_run_the_body_once`; `equal_roots_share_results_but_not_place`.
  *Prereqs:* ο. *Unlocks:* ρ, σ, τ, υ.

- **ρ (#10573). Ownership, trimming and release** (contract 5.5).
  The store keeps a result while a live snapshot's index points at it, an evaluation is reading it, or another stored result refers to it. Trimming for space, results reached only from trials first. What a result holds inside a resource is released once, when its last holder lets go (point 23). A request's memory budget for the store, against which everything the store holds counts, whichever request it was made for; and a call that asks the store to trim.
  *New files:* `trim`. *Edits:* `store`, `held`, `budget`, `host`.
  *Signal:* G0 with trim events; after every event and a drain, the toy kernel holds exactly what the store, the running bodies and the test still hold; `results_reached_only_from_trials_are_trimmed_first_and_released`; `trimmed_shapes_released_once_and_slider_session_stays_bounded`; `every_shape_produced_for_a_dropped_body_is_released`; `trimming_a_nonce_result_leaves_its_dependents_reclaimable`; `looking_inside_a_passed_opaque_value_is_a_recorded_read`; `a_nondeterministic_answer_is_pinned_for_every_reader_in_its_snapshot`; `remade_nondeterministic_result_gets_new_content_and_downstream_reruns`.
  *Prereqs:* π. *Unlocks:* φ.

- **σ (#10574). Freshness and progress** (contract 9).
  `cx.emit`, intermediate and pending answers, a pending answer carrying the last result its path had, readers answered again when an input becomes final. `cx.progress` reaching every live request whose demand needs the path. Toy kind: `slow`, with a switch that makes a run fail outside the inputs.
  *Edits:* `answer`, `mailbox`, `cx`, `section`, `event`, `index`, `finish`, `host`.
  *Signal:* G0 with `slow`, comparing final answers only; a pending answer carries the last final result of its path; `exclusive_section_handle_offers_cancelled_progress_emit_without_waiting`; `reader_of_an_emitted_partial_is_recomputed_when_the_input_becomes_final`; `intermediate_answer_becomes_final_in_its_snapshot_and_descendants`; `a_transient_failure_does_not_survive_into_a_later_snapshot`; `progress_reaches_watcher_and_never_changes_result_or_key`.
  *Prereqs:* π. *Unlocks:* φ.

- **τ (#10575). Scheduling and budgets** (contract 10, 7.6).
  Work ordered by the priority of the demand that needs it; a body that higher-priority work waits on is raised to that priority. Work for a snapshot nobody wants is dropped; a section already running is told through `cancelled`; work a live request waits on is not dropped. A time budget, measured on a clock the host supplies: the budget carries the clock and the core reads the time only through it, so a test's clock is a counter the test advances. A handle that makes a blocked `wait()` return, so a host can deliver an edit.
  *Edits:* `ready`, `waits`, `requests`, `budget`, `executor`, `host`.
  *Signal:* among bodies ready at the same time, one that a higher-priority demand needs runs first, and a low-priority body that high-priority work is waiting on runs before other low-priority bodies; a request the host ends, after making one on a newer snapshot, starts no further bodies, and bodies another live request needs still complete; a second thread uses the handle while the host is in `wait()`, joined by a barrier, and `wait()` returns; `budget_exhaustion_is_one_request_error_and_stores_nothing`; `cold_reads_between_nodes_of_one_kind_do_not_deadlock_and_identical_runs_are_shared`; `solver_budget_exhaustion_is_stored_and_request_budget_exhaustion_is_not`; G0 with two live requests.
  *Prereqs:* π. *Unlocks:* φ.

- **υ (#10576). What an answer read, and what read it** (contract 2.4).
  `core.reads(snapshot, path)`, which includes each child the answer declared, and `core.readers(snapshot, path)`. The form of `Read` is this leaf's: `reads` lists, in the order they were made, the paths the answer read, the declaration reads made while turning a ref into a path, and each child the answer declared with its step; for a node that opened a view, its reads before the view and then the members of what its trials rested on (contract 6). The reference interpreter records reads in the same form.
  *Edits:* `index`, `host`.
  *Signal:* on generated designs, with the debug mode re-running every body that claimed determinism (leaf ο), each re-run asks for exactly what `reads` lists, in that order, and reproduces its result; `readers` is the inverse of `reads` over the answered paths; for a node that opened no view, `reads` equals the reads the reference interpreter made; `read_all_of_child_declarations_starts_every_child_before_waiting`, whose last clause (the children are recorded in the order given) needs `reads`.
  *Prereqs:* π. *Unlocks:* φ.

**Phase 5. Acceptance and close.**

- **φ (#10577). Section 11 at size, and the boundary scenarios.** The integration gate of this PRD.
  The growth tests G1 to G4 on every synthetic model of contract section 11 at N = 100, 1,000 and 10,000, with every toy kind present. One test per boundary scenario of section 10, K1 to K15 and H1 to H11, each named for its scenario. K10's test is leaf χ's test, renamed `scenario_k10_…` by this leaf; no second nested compile is written. A measurement outside the gate: counters and a rough time at 10⁵ and 10⁶ cells, the per-trial cost of the toy solver through a view, and the index's size for a design of many paths over few contents. The results are committed as `docs/prds/evaluation-core.growth-report.md`. A wide run of the random-sequence test with everything built, recorded in the contract's amendment log.
  *Signal:* the growth tests and the 26 scenario tests pass in the gate; the report is in the repository and gives, for each model and each N, the four counters; the recorded wide run.
  *Prereqs:* χ, ρ, σ, τ, υ. *Unlocks:* ω.

- **ω (#10578). Close.**
  Removes the contract's provisional status and its list of provisional sections, each of the six having stood through every leaf or been amended with Leo's approval; checks the amendment log is complete: every commit that changed Part A of the contract after it landed (`git log` on the contract file) has its row, and every wide run of leaves ξ₃ and φ is recorded; stamps this PRD and its manifest SHIPPED with the landed task ids, and brings the census row of section 14 up to date.
  *Signal:* the committed headers.
  *Prereqs:* every other leaf.

### 12.3 Order

```
α → β → γ → δ → ε ─┬→ θ → ι → κ → λ → μ₁ → μ₂ → ν → ξ₁ → ξ₂ → ξ₃ → ζ → ο → π ─┬→ ρ ─┐
│           └→ η ──┘                                                          ├→ σ ─┤
│                                                                             ├→ τ ─┤
│                                                                             └→ υ ─┴→ φ → ω
└→ χ ───────────────────────────────────────────────────────────────────────────────┘
```

Eighteen leaves lie one after another from α to π, and the longest path has twenty-one. That is the price of rule 1 and of building the three mechanisms one rule family at a time. Merging neighbours (ι with κ, μ₁ with μ₂, the three ξ) would save one merge each and give up the isolation that lets a contradiction stop one rule family only.

The four leaves after π have no order among themselves, but each edits `host`, and the orchestrator runs one task at a time on a file. Expect them to land one after another in whatever order the scheduler picks.

Three tasks are not leaves and are not drawn. Task #10617 (decision D13) depends on δ, and ε and η depend on it. Task #10619 (decision D9) depends on χ, and φ depends on it. Task #10621 (decision D14) depends on δ, and θ and ω depend on it.

### 12.4 The manifest

The capability manifest and its machine-readable twin are written when the batch is decomposed, after Leo approves this PRD, and land in the same commit as the task ids. They are `docs/prds/evaluation-core.capability-manifest.md` and `docs/prds/evaluation-core.capability-manifest.yaml`. Each leaf's checks are the presence of its named tests in the crate. So that the manifest can check for them, the 26 scenario tests of leaf φ are named `scenario_k01_…` to `scenario_k15_…` and `scenario_h01_…` to `scenario_h11_…`, each followed by words for its scenario.

### 12.5 What the decomposition changed in this document

Made on 2026-11-25 by the session that filed the tasks. None changes the design.

- **A test moved from leaf θ to leaf υ**, here and in the catalogue: `read_all_of_child_declarations_starts_every_child_before_waiting`. Its last clause says the children are recorded in the order given, and a test can see what was recorded only through `core.reads`, which leaf υ adds. Leaf θ keeps the part it can pass, as a clause of its signal.
- **Leaf ξ₃'s signal** said "every host event of section 5.5". Trimming the store is one of them and its call arrives with leaf ρ, so the signal now excepts it.
- **Leaf β's edits** gained `counters`: its signal asserts on paths resolved, and leaf α builds only the counters for body runs and sections.
- **Scenario K2** (section 10): the order of the recorded reads is visible from leaf υ.
- **Section 8** records the two checks made before filing and two workspace facts found then. Section 5.3 points at the result.
- **Section 14** names the four invariants it had left unnamed.
- **Section 12.2 and the manifest** carry the task ids; 12.4 names the manifest's paths and the prefix of leaf φ's scenario tests.

A review made while filing also found places where a leaf's texts leave an agent something to work out; they went to the session that owns the contract and this PRD, and are not changes made here.

### 12.6 What the review of the filed batch changed

Made on 2026-11-25 by the session that owns this PRD, from the review made while filing: 140 points, each checked against the text. No test was renamed and none moved to another leaf, so the manifest is unchanged.

- **Leaf ξ₁** is reached only when the first request on a new snapshot is made while a request on the snapshot it was edited from is live and unfinished. A request ended by its budget keeps nothing and is no longer one of its ways. Its two catalogue entries are restated to do that, with a section held on a barrier to keep a request unfinished.
- **Leaf μ₁** inherits nothing in the two cases leaves ξ₁ and ξ₃ build, in place of "every evaluation runs to completion".
- **A store of chosen results** (`selected_store`, section 5.2) arrives with leaf δ. Check 2 of section 5.5 and one catalogue entry need it.
- **Leaf δ** treats every result as deterministic until leaf ο, where the default of contract 5.6 takes effect.
- **Rule 10 of section 12.1** keeps leaf χ's programs failing for their stated reasons as later leaves change the interface.
- **Leaf ε's edits gained `lookup`, and leaf ι's gained `waits` and `cycle`.** The tasks' own file lists were left as filed: a task takes a file when it first edits it.
- **Section 10's last column** names the leaf that builds what a row guards. It was headed "First passes in", which rule 1 makes untrue for several rows.
- **Every catalogue entry is self-contained**: models that referred to something outside the repository are restated with the toy kinds, and a kind used by one test alone is written in that test (section 5.4).
- **Smaller points**: what leaf α drops from the probe; how the host learns of a failure outside the inputs before leaf σ; the two-places test compares values, not contents; how a wide run is recorded; the form of what `reads` returns; the clock of a time budget; the form of leaf γ's printed test; one more tactical question (section 15, 7).

## 13. Points the contract does not yet settle

Found by tracing each counter-example through today's text (three seats, one per group of critic reports), by the Rust probe, and by the critic of this PRD. In every case the answer a correct core must give is known and is pinned by a named test; what is open is the rule or the wording that produces it. None reopens a ruling recorded in Part C of the contract.

Each point is handled in one of two ways (decision D10):

- **Left to the leaf.** The PRD gives the property and the test. The leaf finds the rule under the random-sequence test and brings it to Leo as an amendment under section 6. These are the rules inside the three mechanisms, where writing a rule on paper is what the design session advised against.
- **Proposed with this PRD.** A sentence was proposed for each, so that no leaf has to stop for it. Leo approved them on 2026-11-25 and they are in the contract. The exact text of each, with its model, is in `docs/prds/evaluation-core.contract-amendments.md`. Where such a sentence falls inside one of the six provisional sections, the last column says so: it may be amended again by the leaf that tests it.

### 13.1 Left to the leaf

| # | What is open | Smallest model | Required answer | Leaf |
|---|---|---|---|---|
| 1 | **The order in which kept nodes are re-checked.** The text says to work outward from changed inputs, and also never to bring a node up to date on behalf of a reader whose earlier reads have changed. It gives no order that satisfies both. The design record lists this as unsettled. | `r = if detailed then big else estimate`. One edit flips `detailed` and changes what `big` read. | `big`'s body runs zero times. | μ₁ |
| 2 | **Whether the three demands on inheritance can be met together:** right answers (G0), work bounded as G1 says, and point 1. Marking everything downstream of a change breaks G1. Checking everything demanded from the top breaks G1. Re-checking strictly bottom-up breaks point 1. | G1's second case with point 1's model added. | All three hold. If they cannot, the leaf stops with the measurements and Leo chooses which to relax; the later leaves do not depend on which. | μ₁ |
| 3 | **An inherited entry whose confirmation needs the body that is waiting on it.** The text makes a read of an unconfirmed entry wait. Here that wait never ends, and it is not a cycle by 7.7. | Unknown `x` in [1, 8]. `m = if flag then 2x else 5`. Constraint `m <= 10`. Edit `flag` from false to true with `x` kept. | `x` solved in [1, 5], as a fresh evaluation gives. | ν |
| 4 | **How a rested-on snapshot input is named.** The text records it "by its key". A lookup at another place then checks the first place's input and finds a result that is wrong for it. Contract section 6 still says "an input by its key": that sentence is known to be wrong here, and leaf κ builds the required answer of this row, not that sentence. | Two definitions whose constraints and objective have equal content. `y = u * 2` in one and `y = u * 4` in the other. `u` unknown, minimised. | The second gets its own solve (2.5 mm, not 5 mm). | κ |
| 5 | **How a rested-on node above the caller is named.** "By its place relative to the caller" has no spelling for a place above it, since a ref never goes up. That sentence of contract section 6 is known to be incomplete here, and leaf κ builds the required answer of this row. | As point 4. | As point 4. | κ |
| 6 | **A nested view-opener that took in the outer substitution before opening its view.** The plain words of section 6's last bullet make it a cycle. The same bullet's last sentence, and the contract's Example 11, say it follows the outer trial. Contract section 8 still says "two solves never nest through the core": that sentence is known to be wrong for this case (section 13.2, the conflict in section 8), and leaf λ builds the nesting this row requires. | `Slot(width: w)` with its own unknown `depth`, inside a solve over `w`. A second model: an outer solve over `w` and `v`; the inner reads `w` before its view and something that depends on `v` through it. | No cycle in either model; the same answer in either order of request. Two solves that each read the other's unknown only through their views still both fail as a cycle. | λ |
| 7 | **How many alternative records of "what trials rested on" the store keeps, and which is returned when several apply.** | A solve whose trials read `a` then `b` cold and `b` then `a` warm. | Either stored result is acceptable. How many are kept is a setting (point 20). | κ, π |
| 8 | **A stored view-opener is found again when a rested-on member keeps its content but has come to depend on a substituted path.** It shows only when a view substitutes at a path whose own answer does not read the view-opener, which section 6 allows. Found by the critic of this PRD. | Cell `n = 4`. `z = if g then 5 else n + 1`. `c = (z <= 3)`. A solve tries `n := 1..8` and returns the first with `c` true. With `g` true it fails. Edit `g` to false: outside the view `z` is still 5. | The solve runs again and returns 1. | κ |
| 9 | **A child under a node whose new answer was found in the store, or whose content did not change.** The text re-checks children only "when a body runs again", and only those the earlier answer declared. | `ok = if flag then <child>.peak < 400 else true`, the child's path demanded. `flag` goes false, true, false: the child stays present. `flag` goes false, true with the child's path already answered "absent": it stays absent. | Absent in the first; the child's result in the second. | μ₂ |

Two candidate rules, offered to the leaves and not reviewed. For point 6: a view-opener that depended on the outer substitution before opening its view follows the outer trial, whatever part of that substitution it meets again through its view. Both designers of the instance kinds met this point independently. For point 8: a stored result applies only if no member depends on a substituted path now. Restricting where a view may substitute would also close it, but would rule out models the catalogue requires.

### 13.2 Proposed with this PRD, and approved

| # | What was open | The sentence | Kind | In a provisional section |
|---|---|---|---|---|
| 8a | Reading a path whose own body is running. One sentence of 7.7 says cycle; read with section 10, another says the lookup misses and the body runs again, which would never end. | One sentence added: a path being answered in a snapshot has at most one run at a time, and a read of that path waits on that run. Nothing else in 7.7 or 10 changes. (ε) | omission | 7.7 |
| 10 | One run declares the same step twice. | The same declaration twice is one child. Two different declarations at one step is a failure of that run. (θ) | omission | 7.1 |
| 11 | "The caller can ask" which view answers depended on a substitution, but no call exists. | A read through a view returns that fact with the outcome. (ι) | omission | 6 |
| 12 | The content of a partial result, and what "checked again" means when its input becomes final. If a partial shared its final's content, a reader's intermediate answer could become final unchanged. | An intermediate answer is never made final by comparison. When an input becomes final, every reader that read it while intermediate is answered again from the start. (σ) | omission | no |
| 13 | What a body receives when what it read failed for a reason outside the inputs (out of memory, a budget). The text says such a failure "is not an outcome", and a read returns an outcome. | The read does not return: the reading body is dropped and nothing is stored for it. The host is told the error for each demanded path that needed it. A body whose own work fails this way returns that in place of an outcome. (β, δ; shown as a pending answer from σ) | design choice | no |
| 14 | A host has no way to watch a path's progress. | Progress events arrive with the answers, for every live request whose demand needs that path, and are never stored. (σ) | omission | no |
| 16 | When trimming happens, and what becomes of a result whose key holds the content of a trimmed result that carried a nonce. | The store trims what nothing live points at when a request's memory budget would be exceeded, and ends the request only if that is not enough. A result that can no longer be found, because its key holds a content that no longer exists, is trimmed with what it was computed from. (ρ) | design choice | 7.6 |
| 17 | A warm start must not change whether a solve succeeds (5.6). The core cannot check that. | It stays an obligation on the kind, tested per kind by solving cold and warm. (π) | wording | no |
| 18 | "A result is immutable once returned, including through a kernel" (O6) cannot be enforced by the core. | It stays an obligation on the kind. Evaluating the same demands in another order on a fresh core exposes a result that an earlier reader changed. (δ) | wording | no |
| 19 | Wording. 4.4: an answer is a snapshot's answer "for good", yet an intermediate answer later becomes final. | "A final answer". | wording | 4.4 |
| 20 | "The store may keep a few alternatives" (6) and "re-runs a sample" (5.6) are numbers with no home. | Settings of the core itself, given when it is built. They never change a deterministic answer. (κ, ο) | design choice | 7.6 |
| 22 | The repository copy of the contract has no list of provisional sections beyond one sentence, and no amendment log. | It gains both. The closing leaf removes the provisional status. (ω) | wording | no |
| 21 | Nothing checks O2 ("it repeats itself"). | The debug mode's re-run is given the answers the first run had, and fails if the body makes different requests. (ο) | omission | no |
| 23 | **Release.** `Kind::release(out, resources)` cannot be written: a result is shared, and release must run on the resource's thread. | A thing that lives inside a resource is wrapped when it is made, and handed back to that resource's thread when its last holder drops it: the store, a body, or a host that kept an answer. This also covers a body dropped while it holds what a section made. The pointer kept for `previous` does not keep a result alive. (α, ρ) | forced by the compiler; the mechanism is a design choice | 5.7, by its last sentence |
| 24 | Where a root's declaration comes from. | It is a snapshot input (decision D11). (α) | design choice | 7.5 is not provisional |
| 25 | A read at the wrong type. | The reader is given a failure naming both types. (α) | design choice | no |
| 26 | `core.seed` is drawn as a call on a running core. | A core may be built on a store that already holds results, and a core's results can be handed to such a store. Nothing is seeded after a core's first evaluation. (δ) | design choice | no |
| 27 | Section 10 says one thread runs every body and does not say whose. | The host's calling thread. The core starts threads only for resources. (α) | design choice | no |
| 28 | A body that makes a second request before the first is answered. | It ends with a failure. (α) | omission | no |
| 29 | "The compiler checks this" (O4) is true of less than it says. | O4 names what the compiler checks, and adds two obligations it cannot check: a resource type does not hand out shared ownership of itself, and a section does not block on anything a body or the host would have to supply. (α) | forced by the compiler | no |
| 30 | Whether a declaration may bind a child's input to one of the declarer's own bindings. Asked by the Instance design session. | Yes, and the declarer reads nothing by doing so. Turning a ref into a path is then a recorded read of each declaration it passes through, kept in the index entry and not in the key; without that, re-pointing the binding leaves the child stale. (β, μ₁) | design choice | 4.4, by its second half |
| 31 | The last bullet of section 6 forbids, read to the letter, the nested case it means to allow (point 6). | Only the property: a view-opener that depended on the outer substitution before opening its view follows the outer trial, and the answers do not depend on which is asked for first. The rule stays with leaf λ. | wording | 6 |
| 32 | What makes a request and a snapshot live, which 4.4, 5.5 and 10 all rest on, and the host calls the tests need. | `evaluate` returns a request. It is live until the host or a budget ends it, and its demand can be replaced. A snapshot is live while the host holds it or a live request is on it. What a new snapshot inherits is taken when the first request is made on it. The host can ask the store to trim. (α, μ₁, ξ₂, ρ) | design choice | 4.4 |

Number 9 is in 13.1, and number 15 is covered by point 23.

**A conflict in section 8, recorded and not proposed.** "It also means two solves never nest through the core" contradicts the last bullet of section 6 as leaf λ builds it, where an instance's own solve runs under an outer trial. Section 8 is the solve's, not the core's, and the Instance design session has put a related question to Leo (how groups are formed when unknowns have different owners). The sentence is restated when that is ruled.

**Passed to other sessions, not settled here.**

- When the expression evaluator's adapter may declare a child. Read literally, the text never lets it declare one; read loosely, a child can be declared from a stand-in value, which is the fault the rule was written to prevent. The adapter is not in this crate. Sent to the Instance design session.
- The two demands of section 9 that the contract does not meet (planning and body-declared children; a solve across two roots). Sent to the Instance design session.

**Received from the Instance design session** (`instance-design/contract-requests.md` in the design directory, two versions): seven requests and one later addition. Two are answered by points 6 and 30. One asks for a read of "the children this node's body declared"; section 9 says how that is met without a new read. Three change text about node kinds and wait on Leo's rulings in that session; they are recorded at the end of the amendments file and are not proposed here. One was a question about how inputs are keyed, since settled there. The addition is a model in which an inner solve is infeasible under every outer trial but one; it is a test of leaf λ, and the question it raises about how groups are formed belongs to the solve design.

## 14. Design invariants

Walked against `docs/legibility/design-invariants.md`.

- **`module-sccs-only-shrink`.** The crate's top-level modules must form a layered order with no two referring to each other. Section 5.3 gives the order. Rule 5 of section 12.1 makes each leaf check it before merging, because the task-level verify does not.
- **`nothing-vacuous-and-unowned`.** Two surfaces here could hold something declared and inert.
  - *The calls of the kind interface and of `Cx`.* Rule 2 of section 12.1 closes that: nothing is declared before the leaf that implements it.
  - *The obligations on a body, O1 to O10.* A kind can break one silently. What checks each: O4's lending the compiler (leaf χ keeps the evidence); O9 the core, which fails the run; O2 and O8 the debug mode's sampled re-run; O3 the two-places test. Convention only: O1, O5, O6, O7, the two obligations that point 29 adds to O4, and O10, to wrap what is made inside a resource (point 23). Leaf α adds a census row to that document saying so, and leaf ω brings it up to date.
- **`placeholders-owned-and-loud`.** The crate has no placeholder. A `TODO` in it cites a live task in the form the PTODO ratchet reads.
- **`diagnostics-carry-codes`.** The core emits no diagnostic text. A failure is a structured value (code-quality heuristic 12, structured data instead of meaningful strings). A kind attaches its own diagnostics to its results.
- The silent-failure invariants about `undef`, the angle family and the declared-surface family concern the language and its kinds. They do not apply to this crate. Nor do the four it leaves unnamed, walked when the batch was filed: `error-severity-exits-nonzero` (the crate has no process and no exit status), `declared-intent-consumed-or-diagnosed` and `indeterminate-attributable-transient` (it holds no declaration of design intent and gives no "indeterminate" verdict; a request that ends early ends with an error, and a path that does not exist answers "absent", as decision D11 records), and `parse-is-value-faithful` (it has no parser). What corresponds to `undef-has-provenance` here is contract section 9: a body that reads a failed input returns "blocked by the failure at this ref", and the chain to the root cause is the recorded reads.

## 15. Open questions (tactical; decided by the leaf named)

1. **How the toy designs are written down** (a builder in Rust, or a small text form). Suggested: a builder, since the generator and the shrinker work on the structure. Leaf γ.
2. **How a waiter that is not a body is represented** in the wait relation (section 5.2). Leaf δ.
3. **The seeds the gate runs, and how many events each.** Suggested: enough for every counter-example marked "the generator must reach it" to be reached at a fixed seed, within a tenth of the default test timeout on a loaded machine. Leaves γ and μ₁.
4. **Whether a ready read is served in the first poll.** Only if leaf φ's measurement shows the second poll matters. Leaf φ reports; no leaf changes it without Leo's say, because it gives a body a way to reach the index.
5. **The form of a step for a child a body declares** (the chain of call sites and iteration indices). Leaf θ. Since task #10621 (decision D14) a step is a key in a space: a static name chosen by the kind that declares the child, which the core compares and never interprets, so that steps with equal keys in different spaces are different steps. A step made by name or by index alone is in a default space. Required of the form leaf θ chooses: the steps of body-declared children are in a space of their own, and a host can still read a step's space and key.
6. **Where the generic part of the random-sequence checker lives.** It stays under `tests/` because nothing outside the crate uses it yet. A later PRD for real kinds may lift it into the crate; it is written free of toy types so that it can be.
7. **The unit of a request's memory budget for the store.** Contract 7.6 says "memory for the store" and names no unit. Suggested: each kind reports a size for a result, one unit when it reports none, and the budget is a sum of those. That report is a new item of the kind interface, entering with this leaf, and so an amendment to contract 2.2 under section 6. A count of stored entries would need no amendment and is not memory. Leaf ρ.

## 16. Open items carried from the design record

The contract's working draft lists eleven things that were not settled. The copy that lands in the repository does not carry that list, and points here for two of them. What becomes of each:

| Item of the design record | Here | Owner if not here |
|---|---|---|
| The Rust shape of the body interface | settled by the probe (section 5.2) and leaf α | |
| Views, and keeping the kept set up to date: rules without an algorithm | the leaves of phase 3 | |
| Cost per node | measured by leaf φ at 10⁵ and 10⁶ cells | |
| The trial loop's cost through a view | measured by leaf φ for the toy solver | a real solver: the solve kinds |
| The index at very large sizes | measured by leaf φ | sharing index entries between identical instances: left out on purpose (contract Part D) |
| Long work that cannot be interrupted: a section started for low-priority work delays urgent work on the same resource by its own length | not settled. The core only tells a running section it is cancelled | the session layer and the geometry kinds; it needs a measurement on a real design |
| Kernel repeatability. The test that would settle it: tessellate one shape fine and then coarse, and compare with a fresh coarse one | not settled. Until it is, a kernel kind's results carry a nonce | the geometry kinds |
| Source inputs: a content per unit of compiled source | not here | the compiler, with the instance kinds |
| Names for parts of a shape without a kernel number | not here | the geometry design |
| The old engine's tests | not here | the retirement PRD |
| The probes of today's engine ran on an old binary | not here | whoever next relies on one |
