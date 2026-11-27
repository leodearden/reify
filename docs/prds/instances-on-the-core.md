# Instances on the core

**Status:** APPROVED by Leo on 2026-11-26. He ruled on every decision; section 7 gives each ruling. It was decomposed the same day into 33 tasks, with one more outside the plan: section 12.2 gives each leaf's task id, and section 12.5 lists what the decomposition changed in this text. Two rounds of critics read drafts 1 and 2 and three tracers traced the catalogue; section 13 says what they found and what changed.

**Design:** `docs/instance-kinds-design.md` ("the design"), which lands in the same commit as this PRD (decision D11). Leo ruled on every decision in its Part 14. This PRD builds what the design specifies and does not restate it. "Design 6.3" below means section 6.3 of it; "rule A" to "rule G" are the seven mechanical rules of its Part 15.1.

**Contract:** `docs/evaluation-core-contract.md` ("the contract"). "Contract 7.1" means section 7.1 of it. The core crate is `reify-eval-core`, built by `docs/prds/evaluation-core.md` ("the core PRD"). A core task is always cited by its number. The Greek labels in sections 10 to 12 are this PRD's own leaves; the core PRD uses the same letters for other things.

**Catalogue:** `docs/prds/instances-on-the-core.corpus.md` lists every behavioural model this PRD is built under: a small `.ri` source and its expected answers by path.

**Code anchors:** by symbol or file. Statements about the workspace were verified at main `a78117af69` (2026-11-26) by the seats named in section 8; re-locate at implementation time.

## 1. Goal

The first real node kinds on the new evaluation core: the kinds that evaluate an instance of a `structure def`, whether it is a root, a `sub`, a collection element, or what a constructor call such as `let c = Plate()` makes. With them come the compiler output they read, the typed path that names every answer, and a corpus of small `.ri` models with expected answers that is written before the kinds and that the kinds are built under.

This is step 3 of the migration outline in the core PRD. When this PRD closes, no Reify surface uses the new engine yet: the command line, the language server and the GUI switch in one later release (task 10551).

What can be observed when it lands:

- Every entry of the catalogue that needs only the instance kinds passes in the merge gate. Each entry is a `.ri` source compiled by the real compiler and evaluated by the real kinds on the real core, with its answers compared by path.
- Each of rules A, B, D, E and F has its models named in the catalogue and passing, and the design's text of each rule says what was built. Of rule C, the part the instance kinds decide is built; the rest is the solve design's. Rule G is the GUI's and waits for the session layer.
- Every catalogue entry that waits for the solve kinds, the geometry kinds or the session layer is listed with the task that owns it and is counted on every run, so that none is forgotten.
- Each defect of the old engine that the design removes (design Part 13) is an entry that passes.
- A report printed from the core's counters: body runs, index entries and stored results on models with 100, 1,000 and 10,000 instances and values. The design leaves the cost per instance unmeasured (15.2).
- A list of the tracked `.ri` sources that stop compiling under the rulings, and why, for the switch.
- `reify check` on a file that applies a purpose in source says, with a coded error, that the line needs the new engine. That is the one thing a user of today's surfaces sees (decision D5).

## 2. Background

On 2026-11-24 Leo decided to build a new evaluation core beside the old engine, with instances as the first node kinds on it. The core's contract was ruled on 2026-11-25 and its PRD was decomposed into 26 tasks the same day; three have landed and a fourth is in progress.

On 2026-11-25 and 26 a design session worked out with him how instances are evaluated on that core. He ruled on every decision. The rulings this PRD is built on, in his words where the design quotes him:

- **One kind makes every instance.** A sub and a structure value "should behave the same way (in how they handle auto and constraints)". They differ only in what holds them.
- **An `auto` belongs to the instance of the definition whose text contains it.** A constraint bears on it only if the constraint is inside that instance. A constraint outside is checked against the result.
- **A constraint written in the braces after a sub bears on that instance's own `auto`**: "this is what aligns best with the purpose of the braces on a sub: customizing the instance that is being created." Inside the braces a bare name is the instance's own member first, then the declaring definition, and `self` is the parent.
- **A purpose only checks**: "the purpose is a predicate that checks that a design is fit for a particular purpose." An objective inside a purpose is a compile error. A purpose is declared, then applied in source, "so that the source can express application".
- **The source is the sole definition of a design.** "The source must remain the canonical sole definition of the design. If the source can't represent it we either improve the expressive power of the source ... or we refuse it".
- **Reading something that is not there gives undefined.** A member under a false guard is absent. An undefined guard gives the state the design calls presence undetermined.
- **Only the condition of an `if` or a `match` decides whether something is read.** Both operands of `and`, `or` and `implies` are read: "less surprising for people without a code/CS logic background".
- **A structure value carries its type's name, its parameters and every value decided at or below it.**
- **A definition's constraints are checked on every instance**: "yes, all instances".
- **`check` fails by default on an undetermined verdict.** The session layer owns that rule. This PRD owes it one thing: each undetermined verdict is reported so that a deliberately partial design can be told from any other cause.

Two facts from the design session shape the order of work here.

**Seven mechanical rules were never stable on paper.** Three rounds of critics each found new counter-examples in the same seven rules, in the previous round's fixes: how the expression evaluator reads (A), which made instances a summary reads (B), what makes a trial point infeasible (C), the default roots (D), one name declared under several guards (E), the kinds of step in a path and their text (F), and the parameter row's control (G). Leo agreed to settle them by building them under the critics' models ("LGTM"). So the models come first.

**Nothing written after the design's third draft had been reviewed.** That includes what his rulings of 2026-11-26 changed: the control model of its Part 9, names in a sub's braces, and the whole of Part 10 on purposes, which was rewritten after he reversed the purpose half of decision 4. Part 9 belongs to the session layer. This PRD's review read the others against the rest of the design; section 13 records what it found.

## 3. Scope

In scope, in four parts:

1. **The compiler's second output** (design Part 11, items 1 to 7), emitted beside the output the old engine reads. It includes the new member form that applies a purpose, and the members written in a sub's braces.
2. **Typed paths** and their one text form (design Part 8, rule F).
3. **The kinds:** definition table, member table, instance, cell with its expression-evaluator adapter, constraint, collection, as-value, summary, and an applied purpose as an ordinary member. An unknown's cell and its seam to a solve are built and tested against a small solver written for the tests.
4. **The corpus**, written first, and its runner.

Not in scope, with the seam to each named in section 9:

- **The solve kinds** (task 10610). How solves are grouped, how an objective reaches an unknown, and the real solvers. This PRD defines where an unknown gets its value from, and what an instance tells a solve without being evaluated, and tests both with a solver that tries a list of candidates.
- **The geometry and compute kinds** (task 10611). A geometry member is a member whose kind another PRD supplies. So a tool body read from a value (`tool.body`) and a constraint over geometry are specified by the design and become testable only when those kinds exist. Their catalogue entries wait, each naming task 10611.
- **The session layer, the GUI and the agent tools** (task 10551). The design's Part 9 and rule G are input to that PRD. So are: what `check` does with an undetermined verdict; what a file with no root exits with; the note on a verdict that depends on another instance's unknown (design 6.5); the positions an edit gesture needs (design Part 11, item 8), which have no reader until there are gestures; and the user documentation of every change this PRD makes to the language (decision D6).
- **Any file of the old evaluation orchestration.** The held list is `docs/notes/old-evaluation-core-hold.toml`. No task of this PRD edits a file on it.

## 4. Who consumes this

`/prd`'s first gate asks that every mechanism name a consumer: another PRD, or a surface a user can observe. Until the switch no surface runs these kinds, and the PRDs that will consume them (solve, geometry, session) are not authored. Each has a filed milestone task. The core PRD met the same gate, and Leo allowed it an exception (its decision D1). This PRD asks for the same basis, with one difference that makes it less of an exception (decision D1 here):

1. **Within this PRD every mechanism is consumed by the corpus.** A catalogue entry is real `.ri` source, compiled by the real compiler and evaluated by the real kinds through a small host, with its answers compared by path. That is a fixture that exercises the new path and runs in the merge gate, which the second gate accepts as a leaf's signal. It is not a unit test on synthetic input: the only thing missing between an entry and a user is the surface.
2. **Each mechanism's later consumer is a filed task**, and this PRD's leaves get dependency edges to those tasks when it is decomposed (section 11).
3. **The risk the gate guards against is a producer nobody wires in.** Here the wiring is the session-layer PRD, whose milestone already depends on this PRD's close. One producer of the design had no consumer at all until then, the position table, and it has been moved to that PRD.

| Mechanism | Design | Consumer in this PRD | Later consumer |
|---|---|---|---|
| The second compiler output: definition list, member tables, units of source, local references | Part 11 | the kinds, through the corpus host | the session layer loads source into a snapshot (10551); the solve design plans from it (10610) |
| Typed paths and their text | Part 8, rule F | every expectation in the corpus names its answer by path text | the command line, GUI and agent tools (10551) |
| Definition table, member table | Part 2 | the instance kind | solve planning (10610) |
| Instance, cell, constraint, collection | Part 2, 4, 5 | the corpus | everything |
| A structure value: making, as-value, opening | Part 7 | the corpus | tool bodies and results of analyses (10611) |
| Summary, and why a verdict is undetermined | Part 2, decision 12 | the corpus | `check` and `build` (10551) |
| The evaluator adapter | rule A | the cell and the constraint | every kind that evaluates an expression (10610, 10611) |
| An unknown's cell; what an instance tells a solve | 6.2, 6.4 | a solver in this crate's tests | the solve kinds (10610) |
| An applied purpose | Part 10 | the corpus | the GUI's purpose selector and `--purpose`, as the uncommitted form of adding the line (10551) |
| The default roots | Part 3, rule D | the corpus host | the session declares roots (10551) |
| The input that says which kind evaluates each kind of member | Part 17 | a stand-in kind in the tests | geometry and compute members (10611); solve nodes (10610) |

## 5. Sketch of approach

### 5.1 Where the code lives

| What | Where | Why there |
|---|---|---|
| The kinds, the host's helpers, the corpus and its runner | a new crate, `reify-eval-instances` | It depends on the core, the evaluator and the second output's types, and on no kernel. Later kind crates (solve, geometry) depend on it. |
| The second output and its types | a new crate, `reify-eval-source` (decision D11) | A task that edits it verifies it and the kinds crate, not the workspace. It calls the expression compiler and the scopes of `reify-compiler`, which one early leaf makes callable from outside. |
| The typed path and its text; this PRD's diagnostic codes | `reify-core` | The compiler, the kinds and every surface need the path, and a diagnostic will carry one after the switch. |
| The evaluator's two changes | `reify-expr` | It is the evaluator. |

A new crate is registered in the four places the core PRD lists (its section 5.3). Both are put in the `engine` stratum of `scripts/crate-strata.json`; that is a choice, and it means the solve and geometry kind crates must be in that stratum or a later one. Nothing in the old engine's dependency closure depends on either, so the generated list `crates/reify-eval/engine_hash_closure.txt` does not change (seat `repo-guards`). No file under `crates/reify-eval` changes.

Files are small, one concept each, and a file imports only from layers below it. `reify-eval-instances` has five layers:

| Layer | Files under `src/` | Holds |
|---|---|---|
| 1. Results and names, as data | `cause` `cell_result` `verdict` `presence` `summary_result` `structure_value` `failure` `kind_ids` `member_kinds` `problem` | what each kind returns; the names of the kinds, as constants that import no kind; the two tables an instance is given. Nothing here contains a path |
| 2. The seam to the core | `core_seam` `step_map` `unit_key` | the one file that builds what a body returns and calls `read_all`; typed step to core step and back; how a unit of source is keyed as an input |
| 3. The evaluator adapter | `adapter/reader` `adapter/rounds` `adapter/make` `adapter/open` `adapter/state_tests` | rule A |
| 4. The kinds | `definition_table` `member_table` `instance` `cell` `unknown` `constraint` `arguments_verdict` `collection` `forall_block` `as_value` `made_list` `problem_node` `summary` | one kind to a file |
| 5. The host's helpers | `host/core_calls` `host/load` `host/demand` `host/answer` | the one file that calls the core's host interface; compiler output to snapshot inputs; path text to demand; an answer in a form no engine type leaks into |

A purpose instance is the instance kind over a purpose's member table, so it has no file of its own. A guard's condition, a count and a placement are cells.

### 5.2 The compiler's second output

**What exists.** The compiler's output is `CompiledModule`, wrapped per program in `CompiledProgram`. Every entry function goes through `compile_with_prelude_context_checked_with_config`. Its expressions are `CompiledExpr` trees whose references are absolute ids made of a definition's name and a member. A constructor call is compiled by copying the callee's defaults and `let`s into the caller. Members written in a sub's braces, other than assignments, are dropped. The old engine, the command line, the language server and the GUI all read this output (seat `compiler-output`).

**What is built.** A second lowering from the syntax tree, in the crate `reify-eval-source`, behind an entry of its own. It is called only by code of the new engine, so today's surfaces pay nothing for it and see nothing of it. It does not transform `CompiledModule` and adds no field to it.

It emits three things:

- **The definition list**: every definition and every purpose the entry file reaches, imports and the standard library included, by qualified name, with one entry per specialisation of a generic definition. An `occurrence def` is lowered like a `structure def`.
- **A member table per definition** (design Part 11, item 3), with no bodies: each member's name, kind and type; its units of source; its guards; for a sub or a constructor call, the definition named and each argument with its guard; a collection's count and keys; the members written in a sub's braces; each applied purpose with what its parameters are bound to; the unknowns, named or not; the objectives; which constraints are a purpose's; which units of source may make instances; what its constraints can read (section 5.6); and what each definition uses, for the default roots.
- **The units of source.** A unit is one expression or list that one node reads: a member's default or body, a constraint's expression, an argument, a guard, a placement, a geometry operation list, an objective. It is a `CompiledExpr` tree, compiled by the existing `compile_expr` under a scope that mints references local to the unit, plus a table that says what each reference is: the instance itself, the scope that wrote an argument or a sub's braces, or a binder, then typed steps. A unit's content is a hash of the tree, that table, and the content of every function body it calls, with a rule for a function that reaches itself and a version for built-in code (items 1, 2 and 5). It contains no name of a definition and no position. So two definitions with the same text have units of equal content, renaming a definition changes no unit, and editing a function's body changes the units that call it.

**Every member form is in the table from the first leaf.** A form whose lowering is not built yet is emitted as a member with its kind and no units, and the instance answers `no-kind-supplied` for it. A member under a guard is a failure of the same code until the leaf that builds guards lands. Nothing is dropped and nothing is treated as present by default.

**How it reuses the compiler.** Name resolution, type checking and unit checking are fused with emission today: `compile_entity` is one function of about 3,800 lines. The second lowering reuses `compile_expr` whole. Where it needs a piece that `compile_entity` has inline, the leaf lifts that piece into a function both call, without changing what it does. Where lifting would mean restructuring `compile_entity`, the leaf copies the piece into the new crate and adds it to the list in section 16 of what the switch deletes. No leaf changes what the existing entry returns.

**What it needs of shared code**, each an addition that changes nothing the old lowering does:

- The parser keeps what it drops today: the guard on an assignment in a sub's braces (`lower_sub` discards it), and a port's guard.
- The expression compiler and the scopes of `reify-compiler` become callable from another crate. They are `pub(crate)` today. This changes visibility and nothing else, and the new items are hidden from the crate's documentation.
- `compile_expr` takes a mode, introduced in one edit. In the old mode it does exactly what it does today. In the new mode a reference is local to the unit, a read of `self.sub.member` included, and a constructor call compiles to "make an instance of this definition, with these arguments, at this site" (item 7). Leaf γ₃ in section 12.2 lists everything the new mode does.
- No new variant of `CompiledExprKind`, and no edit to `reify-ir`: 69 production files match on that type, some of them held, and the crate is hashed into the engine version. The make-an-instance form is carried by the existing constructor node and a table beside the unit, keyed by the node's place in the tree.

**Generic definitions.** Today an explicit type argument is recorded and never specialised, and an `auto` type argument makes a renamed copy of the definition. The second output has one member table per pair of definition and type arguments, with the types substituted, for the specialisations reached from the entry file. A type argument written `auto` stays the compiler's choice. If the choice fails, the sub's member table entry says so and the sub's answer is a failure (design 6.3).

**Connections, ports, placements, geometry members.** A `connect` compiles to ordinary members of the declaring instance: a compatibility constraint, a connector sub when a type is named, and a record in the member table. A port is a named group of cells, and its guard is honoured. A placement is a cell. A geometry member is a member with its unit of source and that unit's hash, of a kind this PRD does not supply. The alignment that places one part against another, and `relate` and `chain` where they place parts, are geometry's (10611); the member table records them and nothing here evaluates them.

**One rule waits for another task.** A construction that omits a required argument is a compile error by the ruling recorded in task 7874. The standard library's placeholder bodies omit arguments today, and that task rewrites them. Until it lands the second output does not enforce the rule, and the entries that pin it wait.

### 5.3 Paths

An id is a typed path: a root (a module and a definition), then steps. One module turns a path into text and back, and no other code splits an id string. The text must be one-to-one, which is a test, not an argument.

The starting form of the text, to be settled by building (rule F):

| Step | Text | Example |
|---|---|---|
| Root | the definition's name; the entry file's module is not written. How another module is written is open (section 15). | `Asm` |
| Member by name | `.name` | `Asm.thin.t` |
| Element by position, by key | `[n]`, `["key"]` | `Asm.vents[2]`, `Asm.vents["intake"]` |
| Summary; an instance read as a value; a member table | `/summary`, `/as-value`, `/defs[Name]` | `Asm.b/summary` |
| A constraint; one declared per element by a `forall` block | `/constraint[k]`, then `[i]` or `["key"]` | `Asm.b/constraint[0]` |
| A constraint, a `let`, or a guard's condition written in a sub's braces | `/braces/constraint[k]`, `/braces.name`, `/braces/guard[k]` on the sub | `Rig.motor/braces/constraint[0]` |
| An applied purpose, and a constraint of it | `/purpose[k]`, then `/constraint[j]` | `Asm/purpose[0]/constraint[0]` |
| The built-in verdict on an instance's arguments | `/arguments` | `Asm.b/arguments` |
| A guard's condition; a collection's count; a sub's placement | `/guard[k]`, `/count[name]`, `/at[name]` on the declaring instance | `Asm/count[vents]` |
| An unknown with no name of its own | `/auto[member.argument]` on its owner, the member with its key for a keyed entry; inside an expression, `/auto[member#chain.argument]`; a placement, `/auto[name.at]`; a connector's parameter, `/auto[connect[k].parameter]` | `Asm/auto[a.len]`, `M/auto[vents["a"].w]` |
| A made instance; the list of what a cell made | `/made[chain]`, `/made` | `Asm.c/made[0]` |
| A cell's value opened as an instance | `/open`, `/open[chain]` | `Asm.c/open.area` |
| What an instance tells a solve; the solve design's nodes | `/problem`, `/solves` | `Asm.a/problem` |

Its final form, settled by building, is rule F of docs/instance-kinds-design.md.

A guard's cell, a count's cell and a placement's cell are children of the declaring instance, not of what they belong to: a collection must read its count before it has a result to declare anything with. A member's name is written after a dot and everything else after a slash, and no identifier contains a slash. So a sub named `summary` and an instance's summary cannot be confused. On the core, a step is a key in a space (core task 10621): members by name are in one space and the fixed children in another.

`k` for a constraint is its position among the constraints of the instance's member table that give a verdict: those inherited from traits first, in the order the traits are listed, then the definition's own in source order. `k` for an applied purpose is its position among the applications in the member table, in source order, and a purpose applied under a guard keeps its position when it is absent. A count constraint (`xs.count == n`) is the source of the count's cell and gives no verdict. A chain is the call sites and iteration indices from the member's expression to the call, and the forms combine: an `auto` in a call inside a sub's argument is `/auto[member.argument#chain.argument]`. For a value opened in mid-expression it ends with the position of the member read among the unit's member reads, so that two reads in one expression open two instances.

A label is not an id. A person sees `Asm.c` for the made instance and a constraint's own text for a verdict; a tool gets the id and the label beside it. Labels are the session layer's.

This replaces what task 8704 planned. That task requires its printer to give today's strings byte for byte and has two kinds of step. Nothing in this PRD re-routes the old engine's id strings: its files are held, and they go at the switch.

### 5.4 The kinds

The kinds are the eight of the design's Part 2 table. This section adds only what the PRD has to fix so that they can be built on the core as it is.

**Results.** Each kind's result is plain data with no path in it (contract 5.4):

- **A cell** returns a value. When the value is undefined, the result also carries two things. One is its cause: a root cause the cell knows from its own declaration (no value was given; the source is the literal `undef`; an operation had no value; the solve that decides it found nothing) or the refs it read that were undefined or not there. The other is the cause's class, which is "partial" when every root cause behind it is a root's parameter with no value or a literal `undef`, and "other" otherwise. A cell computes its class from the classes of every undefined thing its unit read, so nobody has to follow refs to find it. That is conservative: one "other" among them makes the result "other", so the class can change when an expression is split into two `let`s. An undefined value itself carries neither (design Part 2). A parameter's cell knows it is at a root by a binding to a small snapshot input that only a root's declaration sets.
- **A constraint** returns a verdict, refs to what it is about, and, when undetermined, the refs it read that were undefined and the class. No measured value.
- **An instance** returns present, with its members, or absent. Every guarded member, whatever its kind, reads its guards first, the outermost first, and stops at the first that is not true: under a false guard its answer is absent; under an undefined guard it is the failure "presence undetermined" (contract 7.3), whose description holds a ref to the guard that was undefined and that guard's class; under a guard that itself failed it is blocked by that failure. A collection whose count is undefined answers "presence undetermined" in the same way, and so does a `forall` block over it. As the core is built, a path below a failed node answers that node's failure. So a read of the member, or of anything below it, receives "presence undetermined", and the adapter turns that one failure into an undefined value with that cause and class, as the design's rule on reading what is not there requires. A read below an absent member receives "absent" and becomes undefined with the class "other". The two can be told apart, which `present(...)` and the summary need.
- **A failure** has a code and a description of the kind's own type, which may hold refs and holds no path (core task 10621). `cycle`, `limit` and `blocked` come from the core. `presence-undetermined`, `ambiguous-declaration` (rule E; it names the declarations that collided), `type-argument-unresolved` and `no-kind-supplied` are this crate's. Only "presence undetermined" is turned into undefined by a reader. The others block whatever reads them and cannot be caught by an expression.

**Fixed children of an instance:** `defs` (at a root only), `summary`, `as-value`, `arguments`, `problem`, and `solves`, whose kind the solve design supplies.

**What declares what.** An instance reads source only to declare its children: its definition's member table, and, when its declarer wrote braces after it, the list of members written there, which the declarer passes as a binding and which reads "absent" otherwise. A root's own member table is an input its declaration binds, and the session sets that; every other instance reads its table through the definition table, which the root declares as its child `defs` and passes along. An instance declares one child per member of either list, and gives each its bindings: the unit or units of source the member reads; its own scope; the scope that wrote each argument, passed along from its own declarer (contract 2.1); the definition table. Whatever is written in the braces gets both scopes, an assignment's right side and a guard's condition included: a bare name is first a `let` of the same braces, then a member the instance's definition declares, whether or not it is present in this instance, and it is compiled to a reference into the instance (a member found this way that is a `let` without `pub` is a compile error, and the name is not looked for further: decision D13, item 14); every other name, and `self`, is a reference into the scope that wrote the braces. Which it is, is decided by the compiler, from the two member tables. The condition of a guard written in the braces is a cell of the sub's instance, and a parameter given by a guarded assignment reads that cell first. In the call form of a sub, `sub p = Plate(t: t / 2)`, an argument is the caller's expression, as in any constructor call. A purpose instance's subject parameters are bound to places the same way: the applying instance's own place, then steps down.

**Which kind evaluates a member.** The member table says what kind of member each is: a cell, a constraint, a sub, a collection, an applied purpose, a geometry member, an objective. Which recipe evaluates each is a snapshot input the host supplies, a small table that the root binds and every instance is passed. It is an input, and so part of what a result is stored under, because a table held by the engine itself would be something a body learns outside its reads (contract 2.2, O1). The rows for geometry members and for `solves` are empty until those PRDs fill them. A member whose row is empty answers `no-kind-supplied`, and whatever reads it is blocked. Nothing is skipped in silence.

**The summary** reads each verdict of its instance, the verdict on arguments, each cell, the cells that are not members (a guard's condition, a count, a placement, an unknown with no name), and the summary of each child instance, element and purpose instance, and of each instance any of those cells made. It does not read `defs`, `as-value`, `problem` or `solves`. It returns counts and refs in four groups: constraints; everything at or below a purpose instance applied at or below this instance, whatever group it had in the summary it came from; the built-in verdicts on arguments; and cells, members and instances that failed or whose presence is undetermined. In each: what is violated, what is undetermined with its class, what failed. An expectation's three counts are the sums over the groups, and an expectation may name a group. A member whose presence is undetermined counts once as undetermined and never as failed, and nothing inside it is counted. A cell that fails counts whether or not a verdict reads it, because `check` reads "every constraint and every declared body" (contract 4.1); otherwise a ring of `let`s that nothing reads would pass `check`.

**The built-in verdict on arguments** (design Part 2) is the fixed child `arguments`. Every instance has one; with no arguments it is satisfied. It is undetermined when the value of some argument is undefined and that argument is not the literal `undef`. When an argument's cell is a failure it is `blocked`. It never takes part in a solve.

**Telling a partial design from any other cause** needs no walk. Every undetermined verdict in a summary carries its class, and so does a member whose presence is undetermined. For an explanation that names the root causes by path, a host follows what an answer read, with the core's `reads` (task 10576); that is the session layer's.

**A structure value** is held in the value type the evaluator already passes around: the type's qualified name with its type arguments; each present parameter as a field; and the values decided at or below, by relative path, in one reserved field that takes part in equality and hashing. That is meant to need no edit of `reify-ir`; if it needs one, the leaf says so and the cost in decision D11 applies. **A value read from an instance** (`as-value`) leaves out absent parameters and carries a parameter whose presence is undetermined as undefined. It never fails because of one. `self.children`, `self.members` and `self.descendants` are read by the adapter from the instance's own list of members and each present member's value; if any member's presence is undetermined, the list is undefined (design Part 5).

**A cause across a constructor call.** A made instance is declared from argument values, and a value carries no cause. So the adapter passes, beside each argument's value, the class of its cause when it is undefined, and whether it was the literal `undef`. The made instance's parameter cell takes its class from that. What this does not reach is an undefined parameter read back out of a value: `constraint d.t >= 3mm` over `let d = Plate(t: u)` reads a defined value with an undefined field, and a value that has flowed through another cell or a list has nothing beside it. Decision D10(h) says what class such a read gets. **Through a function the class does travel.** A function is entered whatever its arguments (decision D10(d)), and a parameter of a function keeps, beside the value it is bound to, what the adapter would have passed with that argument: the class of its cause, and whether it was the literal `undef`. So a constructor call inside a function answers as the same call written inline.

**Tests every kind gets.** The core PRD obliges each later kind to pass the two-places test of contract 5.4 and to make truthful claims of determinism. The instance kinds open no view and never use what was there last time, so every result of theirs is deterministic. Each kind whose result declares children supplies `steps` to the kind interface, so that the limit on children per node counts them (core task 10559). Section 5.8 says how the two-places test is run over the whole corpus.

### 5.5 The expression evaluator

The evaluator is kept code that the old engine also calls. Its entry is `eval_expr(expr, &EvalContext)`. `EvalContext` is a concrete structure: a value is read by a direct lookup in a map of cell ids, and binders (a function's parameters, a lambda's, a quantifier's variable) use the same map with made-up names. `and`, `or` and `implies` skip the right operand when the left decides; `forall` and `exists` stop at the first element that decides (seat `evaluator-and-values`).

The design needs two changes (decision D4 says how they land).

**1. Every read goes through one interface.** The store reads (about five sites in `lib.rs` and two in `dual_eval.rs`) become calls of one reader, and the places that copy the map to build a scope (about ten) become a chain of local scopes. The old engine's context implements the reader over its map. Nothing it computes changes.

**2. A reader may answer "not here yet", and under such a reader everything outside an arm is read.** This is rule A. It is built differently from the design's wording, which makes "not here yet" a value of its own that every operation absorbs; the leaf rewrites the rule to say what was built (section 6).

- A new entry beside `eval_expr` walks the tree and returns either a value or "pending". `eval_expr` keeps its signature, because held files call it. Pending is not a value: no built-in ever receives one, so the code that handles an undefined value (4,587 occurrences of `Value::Undef` or `is_undef()` in 157 source files, inline tests included) is untouched. A node whose operand is pending is pending, after its other operands have been evaluated, so that everything outside an arm is asked for in one round.
- An `if` or a `match` whose condition is pending evaluates no arm.
- Under a reader that can answer "not here yet", both operands of `and`, `or` and `implies` are evaluated, and so is every element of a quantifier. Under the old engine's reader, which never says "not here yet", the operators skip exactly as they do today.
- A constructor call with a pending argument makes nothing. The list of what a cell made is declared after the last round.
- The adapter runs the evaluator, waits in one request for everything that was missing, and runs it again. Diagnostics from every round but the last are discarded.

**What the old engine's tests would see: nothing.** Its context never answers "not here yet", so its results, its diagnostics and what it skips are as today. The six tests that pin skipping with a panic sentinel and the four that pin the dual evaluator's branch record pass unchanged. One test must be measured again: the 3 MiB frame budget, because the walk's frame changes.

**No function is made to diverge.** A function body can call only functions declared above it, so no source can recurse through a skipped operand, or through a function entered with an undefined argument. A scan found none among 1,008 tracked `.ri` files and 1,052 sources embedded in Rust source.

**Six places where something other than an `if` or a `match` decides what runs today** (seat `evaluator-and-values`). Under the new reader:

| Today | Under the new reader |
|---|---|
| A user function called with an undefined argument is not entered | it is entered, and each operation inside propagates (decision D10(d), as Leo ruled it). Under the old engine's reader it is not entered, as today |
| A built-in called with an undefined argument is not run | unchanged: a built-in is one operation, and one that is not run makes nothing |
| `map_or` evaluates only one of its default and its function; `map_err` evaluates its function only for an error | the default is evaluated whichever way the value falls, like a logical operator's operand, so an instance made in it is made. The function is still applied only when it has something to apply to. The result does not change |
| `flat_map` stops at the first element whose result is not a list | every element is evaluated; the result is as today |
| `worst_case` stops at the first key that is not a string | every key is evaluated; the result is as today |
| A method call with arguments returns undefined without evaluating its object | the object is evaluated |

A quantifier over an empty collection evaluates no body, which is what its meaning says.

**What leaf ζ₂ (#10641) built beyond the table** (section 6). The results do not change anywhere.

- Pending is counted by the round's reader, not returned by the walk. The entry is `eval_expr_or_pending(expr, ctx, reader)`, which gives `Evaluated::Value` or `Evaluated::Pending`; the reader is the trait `PendingReader`, whose answer is `CellRead::Here(value)` or `CellRead::NotHereYet`. The walk reads undefined in place of a pending cell and reads on. The points that decide what is read or run (an `if` or `match` condition, a quantifier's collection, a built-in, entering a function, applying a combinator's function) check whether a read among their operands was answered "not here yet". The pure operations are not among them: the operators and the option-recovery combinators (`unwrap_or`, `ok_or`, ...) run on the stand-in, and the round's value is discarded.
- A method call with arguments evaluates its arguments as well as its object.
- The ad-hoc selectors evaluate every argument: `@point` all three coordinates, where today it stops at the first that is not a length, and `@face` and `@edge` theirs, which today they never evaluate.
- `map_or` and `map_err` evaluate their function expression too, whichever way the value falls, so a lambda's captures are read.
- A user function is not entered with a pending argument: the call is pending.
- Compute dispatch for an `@optimized` function and the solve that `solve_load_cases` runs each act as one operation, like a built-in: they run only when every argument is defined and none is pending (`ComputeDispatch::dispatch` takes determined arguments). With an undefined argument an `@optimized` function's body is evaluated instead, and `solve_load_cases` is undefined, as today.
- An operand evaluated only for the cells it reads (the right operand of a logical operator its left decides, a method call's arguments, the argument of `map_or` or `map_err` that the value does not use) pushes to the context's diagnostic and undef-cause sinks like any evaluation. So the last round can carry a diagnostic or an `OpContractFailed` cause that the old engine never produces on the same source, as in `false and fits_build_volume(..)`. Whether to detach the sinks for such operands is for the adapter, leaf ε₂ (#10645).
- The selector hook (`SymbolicSelectorCtorFn`, whose contract takes a map) is handed the local binders but no store cell under the new entry, because the new engine's store is not a map. A selector call whose arguments read store cells can therefore be undefined where the old entry builds a selector. Leaf ε₂ (#10645) owns the fix.

**Making an instance from an expression.** In the new mode a constructor node asks the reader to make an instance: it gives the definition, the arguments it has evaluated, and the site. The site is the chain of call sites and iteration indices from the cell's own expression to the constructor (contract 7.1), which the walk keeps as it enters calls and iterations. The adapter declares the child with `cx.child` and reads its `as-value`.

**The tests of state.** `determined(x)` asks whether `x`'s value is defined; it is built with the first cells, and in the new mode its argument may be a path through a sub, where today the compiler asks for a direct reference to a cell. `present(x)` asks whether the thing at `x` is there (design Part 4): true, false, or undefined when its presence is undetermined. Its argument is an instance or a member, not a cell's value, and it compiles to a read whose outcome the adapter tests, so it needs no new kind of node. `constrained(p)` is decision D3. `partially_determined(p)` is "constrained and not determined", as the spec defines it; today it is always false.

### 5.6 An unknown and its seam to a solve

This is where the ruling on a sub's braces meets the ruling on purposes. Two rounds of review each found faults in the mechanism the draft before had written down for it. So this section first states what must be true, each with the entries or scenarios that pin it. That part is fixed. It then gives one way to build it, which is a starting form: section 6 says how a leaf may build it differently.

**What must be true.**

1. Every unknown is a cell of its owner's own scope, with its guards. An `auto` written as an argument is an unknown of the instance that wrote it, at a step of its own, and is passed to the child as an ordinary argument (design 6.2).
2. An unknown's cell reads its guards first. When its member is absent, so is the unknown, and a solve leaves it out (scenario S3).
3. An unknown's value comes from a node whose kind the solve design supplies. For each unknown that node answers with a result: decided, with the value; or not decided, with the reason. The cell returns the value, or undefined with the cause "the solve that decides it found nothing", class "other" (contract section 8: each unknown a failed solve owned is then undetermined). With no kind supplied the cell is blocked by `no-kind-supplied` (S7).
4. The solve design can let a container decide a sub's unknown without this crate changing. A value has no container, so an instance a constructor call made, or a value opened, cannot be reached that way.
5. A solve can learn an instance's problem without evaluating a cell (S1): its unknowns with the guards each is present under; which `auto` parameters the use gives, and under which guards; its constraints, those written in the braces included; its objectives; and what its constraints can read from outside, through any chain of its own cells: which of its arguments, and which names of the scope that wrote its braces.
6. A solve reads only the arguments its constraints read (design 6.4, requirement 3). `sub b : Plate { note = self.b.area / 40mm }` is not a cycle when no constraint of `Plate` reads `note`; `sub a : Plate { floor = self.a.t / 2 }` is one when a constraint does, directly or through a `let` (S10, and the two entries on an argument cycle).
7. A parameter the use gives is not an unknown. Given under a guard, it is the argument where the guard holds and an unknown where it does not (S8).
8. A constraint that is violated whatever the unknown is does not bear on it. It is a violated verdict and the unknown is still decided.
9. A candidate under which a constraint that bears is undetermined is not taken (S9, design 6.4's second model).
10. Nothing at or below a purpose instance is part of the problem of an owner outside that purpose instance (S4). The exclusion is by path, so that a constraint definition a purpose applies, and an instance a purpose's expression makes, are covered. An instance that a purpose's expression makes still decides its own unknowns from its own constraints.
11. The built-in verdict on arguments never bears.
12. A constraint in a sub's braces bears on the instance's own unknowns (S5).
13. An instance with unknowns of its own follows its creator's trial, with no cycle and the same answer whichever is asked for first (S6).

**One way to build it.**

- An instance has two more fixed children. `problem` reads source only: the definition's member table, the braces list, and the list of arguments its declarer wrote. It returns what item 5 lists for its own instance. It lists its child instances as data (definition, guards, count, braces, arguments) and does not read them: reading a guarded sub's node would run its guard, which may read the very unknown being decided. Its list of outside reads covers the constraints of instances below it as far as the member tables say, and is conservative for a unit of source that may make instances. `solves` is the solve design's node. The instance gives it two bindings: its own scope, and the `solves` of its declarer, passed along, which is absent at a root and for a made or opened instance.
- An unknown's cell reads its own step under its owner's `solves`.

**What tests it.** The design's Part 17 says the seam is tested "against the core crate's toy solver". That cannot be done: the core's toy kinds live in its test binary, which no other crate can use (the Core PRD session confirmed this). So this crate's tests have a small solver of their own, written against the core's public view interface (decision D8). Its starting form:

- It reads its owner's `problem`. Before it opens a view it reads what that list says the constraints read from outside, and the guard of each parameter that is given under a guard; it does not substitute at a parameter that is given.
- It tries the candidates the entry gives, in order, as tuples: the k-th candidate takes the k-th value of each unknown's list. It treats the present unknowns of one owner as one group.
- Under each candidate it reads, through a view that substitutes at the unknowns' cells, the verdict of each constraint its `problem` lists, the same for each present instance below, and the verdicts of whatever instances those constraints and the cells they read made. It does not read the owner's summary, which would read every cell, an argument that no constraint reads included.
- The core says of each read through a view whether its answer depended on the trial. A candidate is taken when no constraint that depended on the trial is violated, undetermined or failed. If no candidate passes, it answers "not decided".

That is enough to show what the instance kinds must show. It shows nothing about how a real solver chooses, and it does not tell a point that is infeasible from a model that is at fault, which is rule C's other half and the solve design's. So the catalogue tags an entry with what it needs of a solver:

- **forced**: every value that satisfies the constraints that bear gives the answers the entry pins, and no constraint that might bear is undetermined for a reason that does not depend on the trial. Any correct solver gives those answers. These run here.
- **infeasible**: no value does. These run here.
- **nested**: an owner with an unknown contains, at any depth, another instance with an unknown of its own. These run here once the core has a view inside a view.
- **real**: the answer depends on an objective, on the default objective, on how solves are grouped, or on what a solver does with a constraint that is undetermined. These wait for task 10610.

**What goes to the solve design** (task 10610), with this section: the thirteen items above; the design's 6.4 list "what is available without evaluating anything", restated as item 5, because it was written before the ruling on the braces; and four questions with a catalogue entry each, marked open. They are the three of design 6.4, and one the braces ruling raises: is what a braces constraint reads of its writer's scope read before the sub's solve opens its view, like an argument, or through it? With `let half = self.m.d / 2` and `sub m : Motor { constraint d == 10mm   constraint d > self.half }` the first is a cycle and the second gives 10 mm. The contract's section 8 names only arguments.

### 5.7 What changes in the language, and when

The rulings change what compiles and what a program means. Decision D5 proposes one rule for when: **every change takes effect on the second output only, and the existing compile is unchanged until the switch.** The table lists the changes.

| Change | Design | Today (old binary of 2026-11-23, or code read at main) |
|---|---|---|
| A purpose is applied by a member line, `purpose Name(args)` (decision D2) | Part 10 | does not parse |
| An objective, or an `auto`, inside a purpose is a compile error | Part 10 | an objective is accepted; six tracked purposes have one |
| A purpose's parameter of kind Constraint or Field is refused at its declaration | 15.2 | any kind compiles, a kind that does not exist included |
| A purpose's subject, or a `connect` end, that is a value held in a cell is a compile error | decision 9 | compiles |
| A purpose's body may apply a constraint definition | Part 10 | at a purpose's top level it parses as a call of an unknown function and warns; in a `where` block it is refused |
| Members written in a sub's braces are members of that instance (table below) | Part 4 | all but assignments are dropped with no diagnostic |
| A bare name in the braces is the instance's own member first; `self` is the parent | Part 4 | an assignment's right side is compiled in the parent's scope |
| The guard on an assignment in the braces (`t = 6mm where thick`) is honoured | Part 4, 6.2 | dropped by the parser |
| A read of a member that may be absent compiles wherever it is written, and is undefined where the member is absent. A guard's condition and an applied purpose are read the same way. The spec's reference-safety rule (6.3, 8.10) goes (the end of decision D13) | Part 4 and decision 3, as amended by Leo's ruling of 2026-11-26 | a warning for a guarded `let` or `param`; nothing for a guarded sub |
| A guard that reads what it guards is a compile error | Part 4 | silent |
| A read, from another instance, of a member that is not there gives undefined | decision 3 | a compile error ("unknown member") for a guarded sub |
| A sub inside a `where` block, and subs in the arms of a `match`, exist under their guards | Part 4 | a sub in a `where` block is "not yet supported"; a guarded sub written on its own line is never elaborated (task 8414) |
| One name declared as a collection and as a single sub is a compile error; two declarations present at once is a failure | rule E | silent: no diagnostic |
| A collection's count that reads its own elements is a compile error | decision 10 | silent, no elements |
| `auto` as a constructor argument, where the call is evaluated exactly once | decision 5 | a type error for a length; a silent undefined for a plain number |
| `auto` in an arm, a lambda, a function body or a quantifier's body is a compile error | 6.3 | the same two outcomes |
| `present(x)` | decision 3 | an unresolved function |
| A user function is entered whatever its arguments, and each operation inside propagates (decision D10(d)) | 6.4 | not entered when an argument is undefined: its result is undefined and nothing inside it is made (spec 9.2.7) |
| A read of a `let` that is not `pub` from another definition's text is a compile error; the braces of a sub and the body of a purpose are another definition's text (decision D13, item 14) | spec 7.4 and 8.3 | compiles; 9 tracked files rely on it |
| `determined`, `constrained` and `partially_determined` of a path through a sub | 15.2 | the compiler asks for a direct reference to a cell |
| `self.children`, `self.members`, `self.descendants`, and a bare collection, are the values of present members | decision 11 | lists of path strings, absent subs included |
| An `auto` type argument the compiler cannot choose is a failure at the sub | 6.3 | a compile error |
| `tool.body` on a value | 7.5 | a compile error; testable when the geometry kinds exist |

**What a sub's braces may hold.** The grammar accepts every member form there. On the second output each is compiled or is a coded error; none is dropped.

| Written in the braces | On the second output |
|---|---|
| `name = expr`, with an optional `where` | an argument, written by the parent |
| `constraint`, an applied constraint definition, a `forall` block of constraints | a member of the sub's instance; it bears on the instance's own unknowns |
| `let`, without `auto` | a member of the sub's instance (decision D10(f) says who can read it) |
| `where` block | a guard on the members inside it |
| `minimize`, `maximize` | recorded in the instance's `problem` as an objective; what it does is the solve design's |
| `connect`, `chain`, `relate` | as in a definition's body (leaf ο) |
| `let x : T = auto` | a compile error: write the `let` in the parent and pass it. The `auto` is in the parent's text and the member would be the sub's, so it has no single owner |
| `purpose Name(args)` | a compile error: write the line in the parent, with the sub as its subject. Inside the braces `self` is the parent, so nothing there names the instance |
| `param`, `port`, `sub`, `sketch` | a compile error, as today |
| `fn`, `meta`, `match` with declarations | a compile error |
| an `auto` as a constructor argument inside any of these | a compile error, for the reason given for `let x : T = auto` |

An annotation on a member written in the braces is treated as on any member. A constraint in the braces of a list sub is a member of every element, once the grammar accepts braces there (section 12.5).

The design writes a guarded assignment in the braces as a block, `where thick { t = 6mm }`. That form does not parse: a guarded block cannot hold an assignment. The form that parses is `t = 6mm where thick`, and it is the one this PRD builds.

The language spec is amended by a leaf of this PRD, because the rulings are made and the spec is where the language is defined (section 12, leaf χ). The sentences to amend are listed in the task of leaf χ; among them are the spec's 3.4 (an index out of range), 5.2 (identity), 6.4 (the same name under guards must be provably exclusive), 8.6 (`self` in a specialization) and 4.4 (a purpose's objectives and activation). Leo's rulings of 2026-11-26 add 9.2.7 (`undef` through a function call) and the reference-safety rule of 6.3 and 8.10, which is deleted.

### 5.8 The corpus and its runner

**The catalogue comes with this PRD; the files come with the first leaf.** The catalogue is a markdown document in `docs/`, which no gate reads. The first leaf turns each entry into two files with one stem, `<name>.ri` and `<name>.toml`, under `crates/reify-eval-instances/tests/corpus/<theme>/`. From then on the files are the maintained copy and the catalogue is frozen with a header that says so.

It cannot be files from the start. A tracked `.ri` file under `crates/` must parse today, or sit in a tree that the parse guard is told not to sweep, and the entries that apply a purpose do not parse until the grammar leaf lands. The first leaf registers the tree with that guard, as the spec-conformance fixture tree is registered, and its own validity test applies the same checks to the files that are meant to pass them.

**An entry's expectations** are a TOML file. Its keys are closed, and an unknown key fails the loader:

```toml
tests = "A definition's constraints are checked on every instance"
rule = ["decision 7"]
provenance = ["design 14.7", "task 5765"]
needs = ["instance", "constraint", "summary"]

[[expect]]
path = "Asm.b.length"
value = "-5 mm"

[[expect]]
path = "Asm.b/constraint[0]"
verdict = "violated"

[[expect]]
path = "Asm/summary"
summary = { violated = 1, undetermined = 0, failed = 0 }
```

**As built (α, #10634).** The keys are the ones `tests/corpus_runner/entry.rs` and `expectation.rs` accept. Every entry also carries `answer_basis`, the prose that says how its answers were reached; the loader requires it, so the example above, which omits it, would be refused. Three prose keys are optional: `today` and `note` hold the catalogue's Today and Note bullets, and `not_pinned` the paragraph three entries keep after their table on what they do not pin. The keys also differ from the example in these ways. `rule` and `provenance` are verbatim strings, not lists: 21 Rule and 25 Provenance bullets of the catalogue hold a `;` inside parentheses or quotes, so splitting them would lose text. Presence is one key, `presence = "present"`, `"absent"` or `"undetermined"`. The file's roots row (`roots = [...]`) and the compile rows (`compile = "ok"`, or `compile_error` with its message and an optional `code`) carry no `path`. A multi-file entry is a directory `<name>/` beside `<name>.toml`, which names the entry file in `entry_file`; the directory is the import root. Candidates, settings and edits are tables (`[[candidates.unknown]]` with `path` and `values`, under a `[candidates]` that may carry a `note`; `[settings]` with `path_depth`, `[[edits]]` with `definition`, `replace` and `by`), and a row read after an edit carries `after_edit`.

The forms are: a value; a verdict; present, absent or presence undetermined; a failure by its code; a summary's counts; the class of an undetermined verdict, or of a member whose presence is undetermined (partial, or other); that two paths answer the same, for where the design requires agreement and no ruling has fixed the value; the default roots of the file; and a compile outcome (accepted, or an error by its code). A value is written with its unit in the catalogue's answer notation, which is not Reify source, and the runner parses it into a type of its own (section 15, item 3).

An expectation is what the host is answered at that path, not what a reader would compute from it. A path below an absent member answers "absent", and a path below a member of undetermined presence answers "presence undetermined". What a reader makes of either is pinned at the reader's own path.

An entry may also carry: a limit that changes answers (the depth of a path, the children per node); the candidates the test solver tries for each unknown, taken in step, the k-th value of each together; and a sequence of edits with expectations after each, each edit applying to the text the one before it left. For an entry with edits the runner compares the edited session's answers with a fresh evaluation's.

**What an entry needs.** `needs` is a list from a fixed vocabulary. Each word belongs to exactly one leaf of this PRD, or to one other task:

| Word | Built by |
|---|---|
| `two-files` | α: the runner takes an entry made of several source files |
| `paths` | β |
| `grammar-purpose` | γ₁: the source applies a purpose and parses only once that leaf lands |
| `compile-error` | δ₂: the second output reports an error by its code. A rejection entry also carries the word of the leaf that builds its rule |
| `instance`, `eval-reads` | ε₂ (with `determined`) |
| `constraint`, `summary` | η |
| `guard` | θ₁ |
| `one-name` | θ₂ |
| `collection` | ι |
| `value` | κ₁ |
| `made` | κ₂ |
| `open` | κ₃ |
| `braces` | λ |
| `unknown`, `solve-forced`, `solve-infeasible` | μ₁ |
| `nested-solve` | μ₂: an owner with an unknown that contains, at any depth, another instance with an unknown of its own; `auto` as a constructor argument |
| `purpose` | ν |
| `roots` | ξ |
| `connect` | ο |
| `generic` | π |
| `state-tests` | ρ: `constrained`, `partially_determined` |
| `edits` | σ |
| `solve-real` | task 10610; waits |
| `geometry` | task 10611; waits |
| `session` | task 10551; waits |
| `indexed-subs` | task 5482; waits |
| `required-arguments` | task 7874; waits |

The runner holds one list of the words built so far, and one table of who owns each of the others. An entry runs when every word it needs is built. Each leaf's signal is that adding its word makes its entries run and pass. The validity test fails on a word outside the vocabulary, and on a count of waiting entries, per owner, that differs from a committed number, so an entry cannot stop running without the gate noticing. No entry is an ignored test.

**How an entry is checked.**

1. Its source is compiled through the second output. An entry that expects a compile error passes when the error with that code is reported, and fails when the source is accepted: a rejection that does not fire is a failure, never a skip.
2. A small host loads the output into a snapshot. It declares as roots the definitions the entry's expected paths start at. For an entry that needs `roots` it declares the default roots instead, so that such an entry can also pin "absent" under a definition that is not one.
3. The host asks the core for the paths the expectations name and for each root's summary, and each answer is compared with its expectation through a type of the runner's own. No type of the engine appears in an expectation, so the same files can later be run through the command line.
4. The entry is evaluated a second time on a fresh core with its paths asked for in the reverse order, and the answers must agree.

**The two-places run** is part of the runner from the first kind leaf. For every entry that compiles and whose root can be held as a sub, which is one whose every parameter has a default, the runner also evaluates a generated definition that holds that root as a sub. Every answer under the sub must equal the answer under the root, and the second must cost no body run of any member; the instance's own node is not counted, because a root is given different bindings. This is contract 5.4's test for every instance kind, over the whole corpus, at no cost in entries.

**A disputed entry does not stop a leaf.** Section 6 says what a leaf does when it believes an expected answer is wrong. The entry goes on a committed list of disputed entries, with the escalation that asks Leo; the runner does not run it; and the integration leaf fails while that list is not empty.

**The relation to the spec-conformance suite.** `docs/prds/v0_6/spec-conformance-suite.md` plans a corpus of `.ri` fixtures keyed to the spec's clauses and run through the real drivers. Its fixture format and harness are not built (its task for them is deferred), and its tasks wait on the alpha gate, which this PRD's tasks must not. So this corpus has its own small format now. It is kept independent of the engine so that, after the switch, its entries can become conformance fixtures: each already names the rule it tests and pins answers a driver can print. That move is recorded for task 10551 (section 9).

**Lean.** The five drafts of the catalogue held 249 entries. They were merged to one entry per distinct behaviour, and the review added the ones it found missing (section 13 gives the counts). An entry pins only the answers it is about. There are no variants that differ only in numbers.

## 6. How the seven rules are settled, and when a test contradicts a ruling

Five sorts of statement meet a test here, and each has its own route. "Escalates" means the task raises an escalation to Leo through the orchestrator, as any task does. In every route the leaf goes on with the rest of its work: a model it cannot make pass goes on the list of disputed entries, which the integration leaf requires to be empty.

**A ruling of Leo's, or the design's text in Parts 1 to 13.** A leaf never changes either and never weakens a test to fit the code. If the work shows one cannot stand, the leaf keeps the failing model as a named entry on the disputed list, escalates with the evidence and the question, and proposes no rewrite.

**The wording of rules A to F** (design 15.1). Leo agreed that these are settled by building. A leaf that builds one may change its wording so that it says what was built, provided every catalogue entry for that rule passes with its expected answers unchanged. It records the final wording in the design document, with the entries that decided it, in the same change. It does not escalate for this.

**This PRD's own mechanism** (sections 5.2 to 5.6: what a node is called, what it reads, how the test solver walks). Two rounds of review found faults in it each time, so it is a starting form and not a specification. A leaf may build it differently, provided every catalogue entry and every scenario of section 10 passes with its expected answers unchanged, the requirements of section 5.6 hold, and nothing in the contract is broken. It records what it built in this PRD's section 5 in the same change. It does not escalate for this.

**An expected answer in the catalogue.** The answers were traced by hand and never run, so some will be wrong. A leaf that believes one is wrong does not edit it. It moves the entry to the disputed list, with the answer the catalogue gives, the answer it believes right, and the sentence of the design each rests on, and escalates. If two entries cannot both pass under any rule, both go on the list.

**The contract.** As the core PRD's section 6 says: the leaf writes the amendment on its branch, escalates, and does not merge that part until Leo approves.

The points known before any code is written are in section 16.

## 7. Decisions

Each is stated with what it costs, as it was put to Leo. **He ruled on 2026-11-26.** He took every recommendation but four, and each of those is marked where it stands: D6 (granted, with the obligation recorded on a task), D9 (task 9024 is cancelled), D10(d) (he took the alternative) and D13 item 14 (an error, where the draft left today's behaviour). On D13 items 5 and 7 he asked a question, and on the answer he ruled that a read of a member that may be absent compiles wherever it is written (the end of D13). Where a decision below says "ruled" and nothing more, he took the recommendation.

### D1. On what basis the consumer gate is passed

**Ruled 2026-11-26.**

Section 4. *Recommended:* the corpus is the consumer inside this PRD, each leaf's signal is named catalogue entries passing in the merge gate, and each later consumer is a filed task that gets a dependency edge. *Cost:* until the switch, a user cannot run a design on the new kinds; a fault that only a real design shows is found later. *Alternative:* add a small developer command that prints the new engine's answers for a file, so that any design can be tried before the switch. It costs one leaf and a second entry point that the session layer would replace. I recommend against it here and for it as the first thing the session-layer PRD builds: running the tracked `.ri` files on both engines and comparing is the natural acceptance test of the switch.

### D2. The syntax for applying a purpose, and where the session puts the line

**Ruled 2026-11-26.**

**The form.** *Recommended:* a member line `purpose Name(args)`, with arguments as in a constructor call, and an optional `where`:

```
purpose robust(subject : Structure) { constraint subject.t >= 3mm }

structure def Rig {
    sub bracket : Bracket
    purpose robust(self.bracket)
}
```

It mirrors `constraint def Name` and `constraint Name(args)`: the keyword that declares the thing applies it. It cannot be confused with the declaration, which appears only at the top level of a file and has a body. The subject is `self` for the applying instance, or a sub or an element of it at any depth (`self.frame.bracket`, `self.vents[2]`). An element's index or key must be a literal, because an instance declares its children from source alone. A whole collection is not a subject; a purpose over every element takes the container and uses `forall`.

*Cost:* a grammar rule in every member list, the same rule in the GUI editor's grammar, and a new kind of member in the syntax tree, which about fifteen files match on. A child's parameter could no longer be named `purpose` in a sub's braces; no tracked file has one. The grammar change was reasoned about and not generated: if it cannot be made without a conflict, the leaf stops and asks.

*Alternatives, each tested against today's grammar:*

- `constraint robust(subject: self.bracket)` parses today, as an applied constraint definition. I recommend against it. A constraint bears on an `auto` and a purpose never does, so one keyword would mean two things, decided by what the name resolves to.
- `check robust(self.bracket)` reads well and needs the same grammar work. It adds a keyword, and loses the parallel with `constraint`.

**In a sub's braces, in a purpose's body and in a trait the line is a compile error.** In the braces the message says to write it in the parent with the sub as subject. Inside the braces `self` is the parent, so `purpose robust(self)` there would apply to the parent, and nothing there names the instance itself. *Alternative:* give the braces a way to name the instance. That is new syntax for something the parent can already say.

**Where the session's uncommitted edit puts the line.** *Recommended:* at the end of the body of the viewed root's definition, with the subject written as its path from that root. Applying `robust` to the bracket while viewing `Rig` adds `purpose robust(self.bracket)` to `Rig`. *Why:* it changes this design and nothing else. The contract's Example 9 and the design's say "the definition that holds" the subject; read as the definition that directly declares it, that would apply the purpose to every use of that definition in every design. A purpose toggled on a member that is under a guard is written under the same guard; without it the purpose's verdicts are undetermined wherever the member is absent (the end of decision D13). The gesture, and the position it writes at, are the session layer's.

### D3. What `constrained(p)` reads

**Ruled 2026-11-26.**

The spec says: "the parameter has at least one constraint applied". Today it reads the cell's state at that moment, and gives false for a solved `auto` (run). The design requires an answer that a made instance and its opened value agree on, which rules out any answer taken from how a cell got its value.

*Recommended:* true when some constraint that is present, written in the text of the definition of the instance that holds `p` or of a definition below it, can read `p` according to the source. "Can read" is computed by the compiler into the member table: a constraint reads `p` directly, through a `let`, or through an argument into a sub or a constructor call whose own constraints can read that parameter, a call reached through a function's body included. Presence is read from guards at evaluation, so an undefined guard on the only such constraint gives undefined. A constraint that a definition writes in the braces of its own sub is part of that definition's text and counts. A constraint that the declarer of the instance writes in its braces does not count, and neither does a purpose's. `partially_determined(p)` is then "constrained and not determined", as the spec has it.

*Why the declarer's braces do not count.* Draft 1 of this PRD, following a seat's proposal, counted every constraint inside the instance. The review found that this fails the design's own requirement:

```
structure def Plate { param t : Length = 3mm   pub let c = constrained(t) }
structure def Asm   { sub p : Plate { constraint t >= 1mm }   let e = self.p   let x = e.c }
```

`Asm.p.c` would be true. `e` is the plate as a value, a value carries no braces, so `e.c` would be false. With the declarer's braces left out, both are false, and the answer is a function of the value alone.

*It is feasible.* It needs one relation in the member table, which section 5.6 needs anyway, and one small node per use, which reads guards and nothing else.

*Cost:* it changes what today's sources mean, at the switch. There are nine uses in five tracked `.ri` files, two of them in the standard library's determinacy purposes and the rest in examples and a fixture, and the comments beside several say "true iff `p` is auto". Under this reading `param x : Length = auto` with no constraint is not constrained, and `param y : Length = 5mm` with `constraint y > 3mm` is. And a parameter constrained only from its declarer's braces reports false.

*Alternative:* true when `p`'s declaration is `auto`. It keeps today's comments true. But an opened value has nothing left to decide, so it would answer false where the made instance answers true, unless a value carried how it was reached, which the ruling on values excludes.

### D4. How the two evaluator changes land

**Ruled 2026-11-26.**

Section 5.5. *Recommended:* both land in `reify-expr` as one evaluator with two readers. The old engine's reader never answers "not here yet" and skips operands as today. The new engine's reader may, and under it everything outside an arm is read. *Cost:* about 75 functions in six files of `reify-expr` take the evaluation context, 34 of them in a file of about 10,000 lines that other work also edits; how many change is for the leaf to count; the frame-budget test is measured again; until the switch the evaluator carries a choice the old engine never takes. *What the old engine's tests see:* nothing (section 5.5).

*Alternatives:* (a) a second evaluator for the new engine: no risk to the old one, and two copies of 10,000 lines to keep in step. (b) Change the operators for every caller now: simpler code, but the old engine's results change wherever a skipped operand had an effect (a diagnostic, a kernel query, an FEA run), and ten tests that pin skipping break. (c) Make "not here yet" a value, as the design's rule A words it: every built-in, and code across 157 source files that touches an undefined value, would have to learn it.

### D5. When the language changes take effect

**Ruled 2026-11-26.**

Section 5.7. *Recommended:* every change takes effect on the second output only. The existing compile is unchanged until the switch, with one exception that cannot be avoided: the parser is shared, so the new member form parses for both. The existing compile then reports a coded error at that line: the application needs the new engine. *Why an error and not a warning:* a file that applies a purpose would otherwise pass `check` with the purpose unchecked.

*What follows:*

- No tracked file changes meaning or stops compiling before the switch. In particular the six tracked purposes that hold an objective, at least eleven more in sources embedded in Rust tests, and the example in the documentation chunk on purposes all keep compiling.
- The switch has a list of sources to change, and a leaf of this PRD produces it by running the second output over every tracked `.ri` file. It is handed to task 10551.
- The language spec is amended here. *Cost:* until the switch the spec describes, in these places, an engine that is not the one running. It already does in others.
- Until the switch the two lowerings disagree on what compiles, by design.

*One thing this leaves as it is.* On the old engine a `let` or a `constraint` in a sub's braces is still dropped with no diagnostic, and so is the guard on an assignment there: a false constraint in the braces reports "All constraints satisfied". Task 8195 exists to make that loud, as a coded warning in the existing compile; an error would refuse source that the ruling makes valid. The Held-task review session's record says Leo approved the warning on 2026-11-26 and that tasks 8071 and 8203 were folded into 8195 on his instruction. It waits on the alpha gate and is not this PRD's (decision D9).

*Alternative to the whole:* land each new compile error in the shared compile now. It gives today's users the diagnostics early. It also breaks the files above before anything can evaluate their replacements.

### D6. The documentation rule: an exception

The project's rule is that a PRD which changes the language carries, as its own leaves, the documentation chunks, a worked example, the design cheatsheet's index line, and a check that an author can find the feature. This PRD changes the language and has none of those leaves. *Recommended:* grant the exception and move the four to the session-layer PRD, with the list of chunks this PRD's seats found (purposes, structures, parameters, solving, guards, constraints). *Why:* those documents tell an author and an agent what the running tool does. Until the switch the running tool does none of this, and a chunk that taught `purpose Name(args)` would teach a line that today's `check` refuses. *Cost:* the requirement lives in a task's text for some time, and the editor's grammar accepts a form no document mentions. *Alternative:* one sentence in the purposes chunk now, saying the form exists and takes effect with the new engine.

**Ruled 2026-11-26: granted, on a condition.** Leo: "Yes, either add documentation obligation to an existing milestone that will depend on the filed tasks from this PRD or file it in a new milestone or task." The obligation goes onto task 10551, the milestone for the session-layer PRD, which gets this PRD's close leaf as a dependency at decomposition (section 11). It is written into that task's list of recorded requirements in the step that adds the edge: the four items above with the chunks named, the language changes they must cover (section 5.7's tables), and that they land with the switch.

### D7. How the corpus is run before any surface exists

**Ruled 2026-11-26.**

Section 5.8. *Recommended:* a catalogue in `docs/` with this PRD; files under the new crate from the first leaf; expectations in TOML; a runner in the crate's tests; entries that wait for other work counted with their owners. *Cost:* a second corpus format beside the one the spec-conformance suite plans, until the switch; two files per entry, 382 for the 191 entries the catalogue holds. *Alternative:* write the entries as conformance fixtures now. That suite's format is unbuilt and its tasks wait on the alpha gate.

### D8. The seam to a solve is tested by a solver in this crate's tests

**Ruled 2026-11-26.**

Section 5.6. The design's Part 17 says "the core crate's toy solver". That cannot be used from another crate. *Recommended:* a solver of about a hundred lines in this crate's tests, against the core's public views. *Cost:* the leaf waits for core tasks 10561 and 10562, and its nested case for 10563. *Alternatives:* move the core's toy kinds into a support crate (four registrations, and it moves files that pending core tasks edit); or leave the seam untested until the solve PRD (then the ownership rulings have no test here).

### D9. Existing tasks this PRD replaces or leaves

Read from the task store on 2026-11-26. This session has written to none of them. **Ruled 2026-11-26:** as in the table, and task 9024 is cancelled.

| Task | What it plans | This PRD |
|---|---|---|
| 8704 | one grammar for instance paths, printing today's strings | **Replaced** by the path leaf. Cancel with that leaf as successor, after decomposition. |
| 8202 | lower a sub's braces constraints by pushing them to the parent | **Replaced.** Its lowering contradicts the ruling that such a constraint is inside the sub's instance. |
| 8195 (with 8071 and 8203, now folded into it) | a `let`, a `constraint`, a `connect` in a sub's braces: reject loudly, or lower | **Half replaced.** Lowering them is this PRD's. Saying so on the old engine until the switch is task 8195's, as a coded warning (decision D5). "Reject" is excluded as an end state by the ruling. |
| 7717 | a structured pair across the GUI boundary | **Not taken in.** The pair becomes a typed path, and that boundary is the session layer's (10551). |
| 9024 | a structured subject on constraint diagnostics, in the old pipeline | **Not needed by this PRD.** A verdict on the new engine names its subject by ref. **Ruled 2026-11-26: "cancel it."** Cancelled that day by the Held-task review session, which first removed the dependency on it from its five open dependents (5973, 8612, 9025, 9028, 9032) and narrowed task 9025 to the part that does not need the subject type. How the language server selects a verdict's diagnostic on the new engine is recorded on task 10551. |
| 7874 | a required argument is always required | **Relied on, no edge.** It waits on the alpha gate. Entries that assume it wait, tagged. |
| 5482 | indexed subs | **Relied on for per-element arguments, no edge.** Entries wait, tagged. |
| 8732 | one table of which members each context compiles | **Not taken in.** It will need a row for the applied purpose. |

**The program this PRD succeeds.** `docs/prds/v0_6/gui-purpose-surface.md` planned purpose activation held on the session. None of its leaves has landed. Under the rulings:

- **Dropped:** the activation seam, purpose intent held on the session, the applied-epoch and staleness fields and their UX.
- **Kept in a new form, built here:** applying a purpose as a line in source; compile errors on that line in place of the typed activation errors (an unknown purpose, a missing or unknown parameter, a subject that is not there to be named); a purpose's verdicts as children of the applying instance, found by path and counted in its summary; any number of applications; removing the line being the same as never having written it.
- **Kept in a new form, the session layer's (10551):** the GUI's purpose selector and the agent tool, as the uncommitted form of adding the line; `--purpose` on the command line, if it survives; how a purpose's rows are shown.

The Held-task review session proposes the closure of that program's tasks to Leo; this PRD is the document it would name as successor.

### D10. Eight points the design left undesigned, or that the review found

**Ruled 2026-11-26:** (a), (b), (c), (e), (f), (g) and (h) as recommended; on (d) Leo took the alternative.

**(a) An argument given to a parameter that is absent.** A parameter can be under a guard. `sub c : Case { needs_fan = big   fan_rpm = 1200 }` gives an argument to `fan_rpm`, which exists only when there is a fan. *Recommended:* the built-in verdict on the instance's arguments is violated, and its message says to put the assignment under the same guard (`fan_rpm = 1200 where big`). When the parameter's presence is undetermined, the verdict is undetermined with the guard's class. *Why:* an argument that silently does nothing is a declared intent that nothing consumes. *Cost:* a design that passes an argument "in case" must guard it. In a constructor call an argument cannot be guarded, so `Case(needs_fan: big, fan_rpm: 1200)` has to become two calls in an `if`. *Alternative:* ignore it and report it in the summary as a note that fails nothing.

**(b) An `Output` inside a purpose.** The spec allows it. The grammar does not accept a sub at the top level of a purpose's body. No tracked purpose holds an `Output`; one fixture has a sub there, which the parser skips with no diagnostic. *Recommended:* not accepted in this PRD; the question goes to the geometry and compute design (10611), which owns outputs. *Cost:* none today.

**(c) What the depth limit counts.** The core counts the steps of a path. A level of recursion through subs is one step; through values it is two (the made instance, then the member). So under one setting a recursion through values gets about half the depth. *Recommended:* leave it, and say so where the limit is documented. *Why:* the limit exists to stop runaway structure, and a step is what the core can count without knowing what a kind means by a level. *Alternative:* count levels of instances. That is a change to the contract and to the core's limits task.

**(d) A function called with an undefined argument.** The spec says `undef` propagates strictly through a function call (9.2.7), and today's engine does not enter the function (run). So with `u` undefined, today:

```
structure def Plate { param t : Length   constraint t >= 3mm   pub let w = 40mm }
fn mk(t : Length) -> Plate { Plate(t: t) }

let d = Plate(t: u)     // a plate whose t is undefined; d.w is 40 mm; two undetermined verdicts
let e = mk(u)           // today: undefined; e.w is undefined; nothing is made, no verdict
```

The design says "inline or wrapped, the same answer" (6.4). The draft recommended keeping the spec's rule, on the ground that the alternative changes what every function means, and gave the alternative as: enter a function whatever its arguments, and let each operation inside propagate.

**Ruled 2026-11-26: the alternative.** Leo: "there are almost no functions in the wild so it doesn't matter whether we change them. Now is the best possible time." So, on the new engine:

- `mk(u)` answers as `Plate(t: u)` does: a plate whose `t` is undefined, `e.w` 40 mm, and the same two undetermined verdicts with the same class.
- `fn five(x : Length) -> Length { 5mm }` gives 5 mm for an undefined `x`, where the spec and today's engine give undefined.
- A parameter of a function keeps what came with the argument it was bound to: the class of its cause, and whether it was the literal `undef` (section 5.4). So `mk(undef)` does not trip the built-in verdict on arguments, as `Plate(t: undef)` does not.
- A built-in called with an undefined argument is still not run. A built-in is one operation.
- No function diverges by this: a body can call only functions declared above it (section 5.5).

*What it costs:* the spec's 9.2.7 is amended (leaf χ). The change is made under the new reader only (decision D4), so the old engine's answers stay as they are until the switch. It removes a place where something other than an `if` or a `match` decided whether a user's function is read. The catalogue entry is `undefined-argument-constructor-versus-function-wrapper`.

**(e) Which parameters of a purpose are subjects.** A subject is bound to a place; any other parameter is a value. *Recommended:* a parameter whose declared type is a structure, a trait or `Structure` is a subject, and its argument must name an instance: `self`, or a sub or element below it. A value held in a cell is refused (decision 9 of the design). Any other parameter is a value, written as an expression. A purpose with no parameter is applied as `purpose name()`. *Cost:* a purpose cannot take a structure value, such as a material or a load case, as an ordinary parameter; it reads it from its subject. No tracked purpose takes one; the form compiles today. *Alternative:* decide by how the argument is written at each application. Then one purpose would be compiled once per combination of place and value arguments.

**(f) A `let` written in a sub's braces: who can read it.** The spec says it is "scoped to the specialization". Task 8071 offered making it readable from the parent. *Recommended:* the spec's reading. The `let` is a member of the sub's instance that the other members in the braces can read, and nothing else can. A name that repeats a member of the sub's definition is a compile error. *Why:* a value carries no braces, so `let e = self.s` could never have `e.zz`, and a sub and its value would differ in what can be read. *Cost:* to publish a derived value of a sub, the parent writes its own `let`. *Alternative:* readable from the parent as `self.s.zz`, and a compile error on a value.

**(g) Names in the call form of a sub.** Your ruling on names is for the braces: a bare name is the instance's own member first. A sub can also be written as a call. *Recommended:* in the call form an argument is the caller's expression, as in every constructor call.

```
structure def Plate { param t : Length = 3mm }
structure def Asm {
    param t : Length = 10mm
    sub p = Plate(t: t / 2)      // reads Asm's t: 5 mm, as let c = Plate(t: t / 2) does
    sub q : Plate { t = t / 2 }  // reads the plate's own t: a cycle, as ruled
}
```

*Cost:* the two ways of writing one sub differ in what a bare name means, where the design speaks of "a sub's argument, either form". *Alternative:* the braces rule in both forms. Then `sub p = Plate(t: t / 2)` and `let c = Plate(t: t / 2)` differ, which is the difference between a sub and a value you ruled against.

**(h) An undefined parameter read back out of a value.** A deliberately partial design is one whose undetermined verdicts all trace to a root's parameter with no value or to a literal `undef`. A value carries nothing about how it was reached, so the trace stops at one:

```
structure def Plate { param t : Length   constraint t >= 3mm }
structure def Asm   { param u : Length   let d = Plate(t: u)   constraint d.t <= 8mm }
```

With `Asm` as a root, the plate's own constraint is undetermined and is known to be partial, because the class is passed with the argument. `Asm`'s constraint reads `d.t` out of the value. *Recommended:* it is partial when the cell that holds the value made it itself, as `d` did, because that cell knows the class of each argument it passed; it is "other" when the value has flowed through another cell or a list. *Cost:* a partial design that passes such a value around fails `check` even with the flag that allows partial designs; reading through a sub does not. *Alternative:* "other" always, which is simpler and fails more partial designs. Giving a value the class of each undefined field inside it would make equal values differ, which the ruling on values excludes.

*One thing the ruling on (d) changes here. It is this PRD's reading, reported to Leo, and not his ruling.* As put to him, (h) also gave "other" to a value that came out of a function. Under (d) as ruled, `let d = mk(u)` makes its plate inside a function that the cell `d` called: that cell made the value and passed the arguments, so a read of `d.t` is partial, as for the inline call. Otherwise the wrapped form and the inline form would differ in a verdict's class. The catalogue entry `undefined-parameter-read-back-out-of-value` pins the three cases: made by the cell, made in a function the cell called, and flowed through another cell.

### D11. Where things live

**Ruled 2026-11-26.**

- **The kinds crate is `reify-eval-instances`**, after `reify-eval-core`.
- **The second output is a crate of its own, `reify-eval-source`.** *Why:* the orchestrator verifies a task on the crates it touches and what depends on them. Eleven other crates depend on `reify-compiler`, the old engine, the command line and the GUI among them, so a leaf that adds lowering code inside it verifies most of the workspace. Thirteen leaves of this PRD add to the second output. In a crate of their own they verify two crates. *Cost:* one early leaf makes the expression compiler and the scopes of `reify-compiler` callable from outside, hidden from its documentation; that surface is wider than anyone would design, and it goes when the old lowering does. A second crate to register. *Alternative:* a module inside `reify-compiler`. No public surface, and eleven more near-workspace verifies. Twenty of thirty leaves would then touch a shared crate; with the crate, nine do.
- **The design lands in the repository** as `docs/instance-kinds-design.md`, in the commit that lands this PRD, as the contract did. It is draft 5 as ruled, with a header that says which parts later PRDs own, and with the amendments of section 13. Later PRDs cite it. *Cost:* it holds dated statements about the old engine, which are provenance and are not maintained.
- **The typed path, and this PRD's diagnostic codes, are in `reify-core`**, added by one leaf. *Cost:* that crate is hashed into the engine version, so the edit discards stored FEA results once, as every edit to that crate does today.
- **`reify-ir` is not edited.** A structure value as ruled is held in the value type that crate already has (section 5.4). If a leaf finds it cannot be, it escalates: that crate is hashed into the engine version too.

### D12. Priority

**Ruled 2026-11-26.**

*Recommended, as for the core:* every leaf at priority `high`; the leaves on the longest path pinned after the core's pins. No leaf depends on the old-core holder (10510) or on the alpha gate (9739). *Cost on the machine:* nine leaves edit a crate that much of the workspace depends on, and verify accordingly: α and δ (the workspace manifest), β (`reify-core`), γ₁ and γ₂ (the parser), γ₃ (`reify-compiler`), ζ₁, ζ₂ and κ₂ (`reify-expr`). χ edits the spec alone. The other twenty edit the two new crates only. Leaf γ₃ lists everything this PRD needs of the expression compiler; each thing found missing later costs one more near-workspace verify. From leaf ε₁ on, every core task's verify includes this crate's tests. The merge gate verifies the workspace for every leaf, as for any task.

### D13. Smaller choices made here: say if any is wrong

**Ruled 2026-11-26:** as listed, except item 14, which Leo changed, and items 5 and 19, which changed with his ruling on the question below the list. The list shows each item as ruled.

1. A summary counts a cell that fails, whether or not a verdict reads it.
2. A count constraint (`xs.count == n`) is the source of the count and gives no verdict. A collection with none, or with more than one present at once, is a coded error or a named failure, by the leaf that builds collections.
3. An explicit `undef` argument does not trip the built-in verdict on arguments. An argument that evaluates to undefined does, and so does an argument whose own guard is undefined; the parameter is then undefined. This is a reading of the design's "every argument I was given is defined", made so that a deliberately partial design can be told apart.
4. The built-in verdict on arguments never takes part in a solve.
5. **Changed by Leo's ruling (below the list).** A guard's condition is a declaration under the guards that enclose it, and not under the guard it states. A guard that reads a guarded member compiles; where that member is absent the guard is undefined, and what it guards is of undetermined presence. An applied purpose is read the same way: `purpose p(self.fan)` with `fan` under a guard compiles, and its verdicts are undetermined where there is no fan.
6. The same name under guards the compiler cannot prove exclusive compiles, and rule E gives the answers at run time. The spec's 6.4 is amended.
7. A read, from another instance, of a member that only some `match` arms' definitions have compiles and is undefined where the chosen arm lacks it.
8. A ring of `let`s inside one definition, and `t = t / 2` in a sub's braces, compile and are a `cycle` failure at run time.
9. Default roots: a use inside a function body or a purpose's body does not count; a constructor call inside a lambda in a member's expression does; a definition that is the type of a member counts as used even when the value comes from a function; a generic definition whose type parameter has no default is never a default root, the rule's last clause included, so a file that holds nothing else has no root.
10. A purpose's body may apply a constraint definition (`constraint Name(args)`), as the design's Part 10 assumes. Today, at a purpose's top level the line parses as a call of an unknown function and warns, and inside a `where` block it is refused.
11. A reader that is given the failure "presence undetermined" turns it into an undefined value whose cause is the undefined guard. It is the only failure a reader turns into a value.
12. An undefined result carries the class of its cause, partial or other, computed from everything undefined that its unit read.
13. The static error for recursion with no guard (`termination.rs`) stays.
14. **Changed by Leo's ruling.** A read of a `let` that is not `pub`, from another definition's text, is a compile error. Leo: "No, trying is an error (default private visibility)." It is the spec's own rule (7.4 and 8.3: parameters, subs and ports are read from outside; a `let` and a constraint are private unless marked `pub`), which today's compiler does not enforce: such a read compiles today, and 9 tracked files rely on it. Like every change here it takes effect on the second output (decision D5), and those files go on leaf τ's list. Two places where this is the PRD's reading of the ruling: the braces of a sub are the declarer's text, so a bare name there that is a private `let` of the sub's definition is a compile error, and it does not fall through to the declarer's member of that name; and a purpose's body reads only the `pub` members of its subject. A path is not source: a host reads a private `let` by path like any cell, and a value carries it.
15. Under the new reader `flat_map` and `worst_case` evaluate every element, `map_or` evaluates its default whichever way the value falls, and a method call evaluates its object. Their results do not change.
16. A value read from an instance carries a parameter of undetermined presence as undefined; the value still exists.
17. A guard that fails blocks the member it guards. Rule E's "otherwise its presence is undetermined" is for guards that are undefined, not for guards that failed.
18. `present(x)` is undefined when the presence of `x` is undetermined.
19. **Changed by Leo's ruling (below the list).** In a sub's braces a bare name that the sub's definition declares under a guard is the instance's member, present or not. A constraint in the braces that reads it with no guard of its own compiles, and is undetermined where the member is absent.
20. A parameter with no default that is given only under a guard (`len = 20mm where long`) is a compile error, with the rule of task 7874.
21. `let x : T = auto` written in a sub's braces is a compile error, and so is `auto` as a constructor argument inside a member written there. The `auto` is in the parent's text and the member would be the sub's, so it has no single owner. The way to write it is a `let` in the parent, passed in.
22. `auto` inside a purpose is a compile error, like an objective: a purpose decides nothing.
23. A purpose line inside a purpose's body or a trait is a compile error.
24. A constraint in the braces of a list sub is a member of every element. (Braces on a list sub do not parse today, so no leaf builds this: section 12.5.)

**Leo's question on items 5, 7 and 19 (2026-11-26), and his ruling.** Leo: "These seem like inconsistent handling of reading guarded members. Since unguarded reads of guarded members from outside are allowed and may get undefined, what would happen if we simply allowed a read of a maybe-absent member, regardless of where it's from? Is the safety worth the inconsistency?"

*What the items rest on.* Item 7 follows the rule he ruled in the design: reading something that is not there gives undefined (decision 3b). Items 5 and 19 extend line 1 of that same decision, which kept one compile error out of the spec's reference-safety rule (6.3 and 8.10): inside one definition, a read of a guarded member from a declaration with no guard of its own, outside any arm of an `if` or a `match` (design Part 4, "The compiler still reports the plain mistake"). So the question is whether to keep that line. Today's compiler gives a warning for a guarded `let` or `param` and nothing for a guarded sub.

*What would happen.*

```
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
    constraint self.fan.flow > 5                      // 1
    constraint not needs_fan or self.fan.flow > 5     // 2
    let f = self.fan.flow                             // 3
    sub duct : Duct where self.fan.flow > 5           // 4
    purpose quiet(self.fan)                           // 5
}
```

| Line | As drafted | Allowed everywhere (as ruled), in a `Case` with no fan |
|---|---|---|
| 1 | compile error | undetermined, class "other". `check` fails with or without the flag for partial designs, and the verdict names `fan` and the guard that removed it |
| 2 | compile error, although the same line written in a parent of `Case` compiles and is satisfied (design 3b, line 4) | satisfied |
| 3 | compile error | undefined. Nothing fails unless a verdict reads `f`, as for the same read from a parent |
| 4 | compile error | the guard is undefined, so the presence of `duct` is undetermined and `check` fails. Written `where needs_fan and self.fan.flow > 5` it is absent |
| 5 | compile error | every verdict of the purpose is undetermined, class "other" |

In a `Case` with a fan every line answers the same either way.

*Recommended: allow it everywhere.* Three reasons.

1. The error is not exact, and cannot be made exact without proving things about guards. It refuses line 2, which is right by the spec's own three-valued logic. It also refuses a read of a member that is always there: a name declared under `where n < 3` and again under `where n >= 3` counts as guarded, while the same two declarations written as one `where`/`else` do not (the entry now named `separate-wheres-read-without-guard`, which pinned that compile error until the ruling).
2. It is the one static rule about presence in a language where everything else about presence is decided when the design is evaluated, and it needs its own definitions of what counts as being under a guard (items 5 and 19 exist only for that). It protects the reader who needs it least, the author of the definition, who can see the guard. The same mistake made from a parent compiles.
3. Nothing it catches becomes silent. A constraint that reads an absent member fails `check` under either flag and says why. In the GUI it shows at that constraint with the rest of the design evaluated, where a compile error stops the whole file. A `let` that reads one is undefined, which is what a reader outside the definition already gets.

*What it costs.* When the mistake is found. The compile error finds it in every configuration, before anything runs. Without it, it is found the first time the design is evaluated in a configuration where the member is absent. The one exposed is the author of a library definition who never evaluates that configuration: its user then meets an undetermined verdict inside the library.

**Ruled 2026-11-26: allowed everywhere.** To the recommendation Leo answered "Agreed". It amends line 1 of the design's decision 3b and its Part 4.

*What it changed here:* items 5 and 19 above, and item 7 stays. The row of section 5.7 is "compiles; undefined where the member is absent". Leaf χ deletes the spec's reference-safety rule. Leaf θ₁ builds no compile check for it. Four catalogue entries pin run-time answers where they pinned a compile error: `unguarded-read-in-own-definition-is-undefined-where-absent`, `braces-unguarded-read-of-guarded-member-is-undetermined`, `separate-wheres-read-without-guard` and `guarded-own-sub-as-purpose-subject-undetermined-when-absent`. Two things stay compile errors: a guard that reads the very member it guards, and a name that no configuration declares.

## 8. Preconditions, and what was verified

Verified at main `a78117af69` by reading code, by the grammar gate, or by running the prebuilt binary of 2026-11-23. Nothing was built. The seat reports are under `/home/leo/.claude/spawn-briefs/reify-evaluation-core-contract/instance-prd/seats/` on the workstation; the facts this PRD rests on are restated here so that a task does not need them.

| Assumed | Checked how |
|---|---|
| A kind can be written outside the core crate against its public interface | read: `Kind`, `Cx`, `Declaration`, the bindings and `Outcome` are public (seat `core`). **Seven core tasks change that interface.** By the Core PRD session's message of 2026-11-26, 10557 is done, 10617 is in progress and 10621 is pending; the others were pending when read: 10557 (a required `VERSION`, `content`, `Input::of`), 10617 (the attached binding goes), 10621 (a failure's description, a step's space, public accessors), 10559 (`steps`; a budget on `evaluate`), 10560 (`read_all`, `cx.child`, constant bindings), 10571 (what a body returns), 10574 (`emit`, `progress`, a pending answer). Section 9 says how this PRD lives with that. |
| A declaration can carry a value | **not built.** Contract 2.1 lists a constant binding. The Core PRD session answered on 2026-11-26 that core task 10560 builds it. Needed here from the made-instances leaf on. |
| A step can tell a member named `summary` from the fixed child | **not as built**: a step is a name or an index. Core task 10621, filed on this PRD's request, makes a step a key in a space that the declaring kind names. It was `deferred` when first read (filed in planning mode); the Core PRD session reported it pending on 2026-11-26. |
| A kind's failure can carry a ref | **not as built** on the store's branch (a static code). Core task 10621 builds a description of the kind's own type. |
| A path below a failed node answers that node's failure | read by the Core PRD session in `declaration_of`. **Ruled by Leo on 2026-11-26** in that session's thread, as that session reported it: the contract's section 4.3 gains "A path below a node whose answer is a failure answers that failure." The change was in the merge queue when reported and not yet on main. Task 10621 pins it with a test. Section 5.4 relies on it. |
| A host can take a core path apart | **not as built** (the accessors are private). Core task 10621 makes them public. |
| The limit on children per node counts the children a result declares | from core task 10559, which adds `steps` to the kind interface with a default of none |
| The core's views can be used from another crate's tests | the interface is specified public in the core PRD; it arrives with task 10561 |
| `reify_ir::Value` and `CompiledExpr` can be a core result (the core requires `Send + Sync`) | read: no `Rc`, `RefCell` or raw pointer in `value.rs` or `expr.rs`. **Not compiled.** The first kind leaf compiles it; if it fails, the leaf escalates. |
| `compile_expr` and the scopes can be called from another crate | read: they are `pub(crate)`. Leaf γ₃ makes them public. **How many items that is was not counted.** If it cannot be done without changing what they do, the leaf escalates and the fallback is decision D11's alternative. |
| The syntax tree has what the second output needs | read and run: **not all.** It drops the guard on an assignment in a sub's braces and a port's guard. Leaf γ₂ adds them. |
| `purpose Name(args)` can be a member | grammar gate: **does not parse today.** The rule was not generated. Leaf γ₁ adds it; no parser state admits both it and the declaration (reasoning). |
| The subject forms parse (`self`, `self.bracket`, `self.vents[2]`), with a trailing `where` | grammar gate: yes |
| Each rejection the design relies on | run and read, 39 rows (seat `rejections-and-spec`): three exist and fire today (an untyped `let x = auto`; `param`, `port` and `sub` in a sub's braces; recursion with no guard). Every other is built here and is pinned by an entry that fails if the source is accepted. |
| No source recurses through a skipped operand | read and scanned: none can |
| `toml` is a workspace dependency | read: root `Cargo.toml` |
| A corpus under the new crate is not compiled or evaluated by any other gate | read: two repository-wide gates walk `crates/**/*.ri` (the parse guard in `reify-syntax`, the bare-scalar guard in `reify-cli`). Both run at merge and not at task level, so a file that does not parse is green on the task and red at merge. Leaf α registers the tree with the parse guard and runs the bare-scalar check in its own validity test. |
| An expectation file may be `.toml` | run: the lock-charter guard's list of extensions has it |
| A diagnostic in a crate's source must carry a code | read: the uncoded-diagnostics ratchet. `DiagnosticCode` is non-exhaustive, and adding a variant obliges nothing else. Leaf β declares every code this PRD needs, in one edit. |
| The old engine's tests do not see the evaluator change | reasoning over code read; leaf ζ₂'s signal is that the workspace's tests pass with no expectation edited |
| Two top-level modules of a new crate that refer to each other fail the merge gate | read: the module-cycle ratchet. The task-level verify does not run it for these crates, so each leaf runs it before asking for a merge. |

Activation: Leo approved on 2026-11-26. Leaves α, β, χ, γ₁, γ₂, γ₃, ζ₁, ζ₂, δ₁ and δ₂ wait for no core task. The first leaf on the core, ε₁, waits for core tasks 10557, 10617, 10621, 10558 and 10559. No task behind the alpha gate or the old-core holder must land first.

## 9. Seams with other work

| Neighbour | Direction | What crosses the seam | Owner | State |
|---|---|---|---|---|
| The core (`docs/prds/evaluation-core.md`) | consumes | The kind interface, declarations and bindings, `cx.child`, views, limits, cycles; the host's calls. | The core PRD owns the interface. **While it still grows:** this crate keeps every construction of what a body returns and every call of `read_all` in `core_seam`, and every call of the core's host interface in `host/core_calls`. Kinds written in this crate's tests build their results through `core_seam` too. A core task that changes a signature either file is written against updates that file in the same change. The Core PRD session agreed on 2026-11-26. It is rule 11 of the core PRD, on main since `4f334348e2`, and covers a host outside the crate as well as a kind; that PRD's decision D14 names the two files as planned, to be confirmed by this session at decomposition. It is in the texts of tasks 10560 and 10571. It is that session's choice and not a ruling: Leo can instead tell this PRD to wait for the determinism task (10571), which puts every instance kind behind that task and everything before it on the core's chain. | agreed; both paths to be confirmed at decomposition, for every core task whose edits include the host's file (by a critic's reading of the core PRD: 10559, 10564, 10568, 10573, 10574, 10575, 10576). Core task 10571 builds the sampled re-run of determinism claims and does not switch it on in this crate; task #10667 of this PRD does (section 12.5). |
| The contract | relies on | 7.3 as it stands. One sentence that Leo ruled into its section 4.3 on 2026-11-26: a path below a failed node answers that failure. Two sentences of its section 8 are the solve design's to restate: the third planning rule, and "before that it reads ... its arguments", which the design narrows to the arguments its constraints read. Its 2.5 says a missing cell reads as `undef` to the evaluator; decision D4 builds "not here yet" as something other than a value. Its 9 lists a failed solve among failures, where its 8 makes the unknown undetermined; this PRD's cell follows 8, and what the solve's own node answers is the solve design's. | The Core PRD session owns the text. | the sentence is ruled; it was in the merge queue when reported |
| Solve kinds (task 10610) | produces for | An instance's `problem`; an unknown's cell and the two bindings of `solves`; the class and refs on an undetermined verdict; the three groups of a summary; the row for `solves` in the table of member kinds. | This PRD owns the seam and its tests. The solve PRD owns grouping, objectives, the real solvers, rule C's other half, and every catalogue entry tagged `solve-real`. | not authored; leaf μ₁ becomes a dependency of 10610 |
| Geometry and compute kinds (task 10611) | produces for | The row for geometry members in the table of member kinds; a geometry member's unit of source and its hash; that only places are enumerated for drawing and export; a value made by built-in code is made where it is returned (rule B); `tool.body` through an opened value; an `Output` in a purpose. | This PRD owns the table and the stand-in test. The geometry PRD owns the kinds and every entry tagged `geometry`. | not authored |
| Session layer (task 10551) | produces for | The second output's entry; `default_roots`; loading into a snapshot; paths and their text; summaries with classes. | This PRD owns each. The session PRD owns "current", gestures and their refusals (rule G), the position table, `check`'s exit rule, labels, the note of design 6.5 and every explanation by path, the documentation of the language changes (decision D6: written onto task 10551 at decomposition, as Leo ruled), the list of sources that stop compiling at the switch, the move of this corpus into the conformance suite, the bill-of-materials report, and task 7717. One thing the gesture must know: a purpose toggled on a member that is under a guard is written under the same guard, or its verdicts are undetermined wherever the member is absent. | not authored; this PRD's close leaf becomes a dependency of 10551 and of 10552 |
| The old engine, `crates/reify-eval` | none | Nothing. | | |
| The existing compile | shares | The parser and `compile_expr`. Additions only (section 5.2). | This PRD, for those additions. | |
| The spec-conformance suite (`docs/prds/v0_6/spec-conformance-suite.md`) | future home | The corpus's entries, as fixtures, after the switch. | The session PRD owns the move. | recorded |
| The purpose program (`docs/prds/v0_6/gui-purpose-surface.md`) | succeeds | Decision D9. | The Held-task review session proposes the closure. | reported |

**Two things the design asks of code this PRD does not own.**

- **A built-in that is handed a structure value and reads a `let` of its definition.** Today a value carries its definition's `let`s as fields. Under the ruling it carries nothing derived. A scan of non-test Rust found one such reader among 112 functions that read a field of a structure value by a name written in code: the bill-of-materials report (`Engine::build_bom_report`) reads `line_cost`, a `let` of the trait `Costed`, and falls back to the unit cost with no warning when it is missing. On the new engine that report reads the cell `line_cost` at each place's path. It is a surface and belongs to the session layer (10551). Several values are made by Rust code with no `.ri` definition at all (a carried topology, a shell-extraction result); what the ruling means for those is the geometry and compute design's (10611).
- **What a solve does** with an unknown whose guard is undefined, with a constraint that is undetermined, and across a recursion (design 6.4's three questions); and whether the guard of an argument is read before a solve's view or through it, which decides whether one model is a cycle. The catalogue has an entry for each, marked open, owned by 10610.

## 10. Boundary scenarios

What the neighbours will rely on, written from their side. Each is a test named `scenario_<id>`, written by the leaf in its last column.

**Facing the compiler.**

| # | Scenario | Given | Then | Leaf |
|---|---|---|---|---|
| C1 | A unit's content does not depend on its definition's name | two definitions with the same text | every unit of one has the same content as the matching unit of the other | δ₂ |
| C2 | A change of spelling changes nothing | a file, and the same file with comments and spacing changed | the same definition list, member tables and unit contents | δ₂ |
| C3 | An edit to one literal changes one unit; an edit to a function's body changes the units that call it | a default edited; a function body edited | one unit's content changes, and the member table does not; the callers' units change | δ₂ |
| C4 | The existing compile is undisturbed | every leaf that edits `reify-syntax` or `reify-compiler` | no expectation of an existing test is edited, and the new mode of `compile_expr` is off unless the second output sets it | γ₁, γ₂, γ₃ |
| C5 | A rejection that the catalogue pins fires | each entry that expects a compile error | the error with that code; accepting the source fails the entry | each leaf, by the runner; the seeded failure of the runner itself, δ₁ |
| C6 | Nothing is dropped | a definition with a member form whose lowering is not built | the member is in the table with its kind; its answer is `no-kind-supplied` | δ₁ for the table, ε₁ for the answer |

**Facing a solve.**

| # | Scenario | Given | Then | Leaf |
|---|---|---|---|---|
| S1 | The problem is readable without evaluating | an instance's `problem`, one with a guarded sub and a collection | its unknowns with guards, its constraints, its objectives, what its constraints read from outside, its children as data; no cell, guard or count is evaluated to produce it | μ₀ |
| S2 | A substitution at an unknown's cell reaches everything that reads it | the test solver's trial | constraints at and below the owner see the trial's value; the surrounding state does not | μ₁ |
| S3 | An unknown that is absent is left out | an unknown under a false guard | it is not decided; the others are | μ₁ |
| S4 | A purpose's constraint never bears | a purpose that the only feasible value violates | the unknown is decided as without the purpose; the purpose's verdict is violated | ν |
| S5 | A constraint in a sub's braces bears | braces that force a value the definition alone would not choose | that value | μ₁ |
| S6 | An instance with unknowns of its own follows its creator's trial | a made instance given an argument on trial; a sub whose braces constraint reads the parent's unknown | no cycle; the same answer whichever is asked for first | μ₂ |
| S7 | No solver supplied | an unknown, and an empty row for `solves` | the unknown is blocked by `no-kind-supplied` | μ₁ |
| S8 | A parameter the use gives is not an unknown | an `auto` parameter given by the use under a guard | an unknown where the guard is false, the argument where it is true | μ₁ |
| S9 | A candidate that leaves a constraint undetermined is not taken | design 6.4's second model, with candidates 10 mm and 48 mm | 48 mm | μ₂ |
| S10 | A solve does not read an argument its constraints do not read | `Motor` with an `auto` and a parameter `note` no constraint reads, given `note = w` by a parent whose `w` is an `auto` | both decided, no cycle | μ₂ |

**Facing geometry.**

| # | Scenario | Given | Then | Leaf |
|---|---|---|---|---|
| G1 | A member of a kind nobody supplied is loud | a definition with a geometry member, no geometry kinds | `no-kind-supplied` at that member; its readers blocked; the instance's other members answered | ε₂ |
| G2 | A supplied kind is declared like any member | a stand-in kind in the tests, entered in the table of member kinds | the instance declares it with the member's unit of source and scope bound | ο |
| G3 | A value's member is reached by opening | a stand-in member on a definition, read as `tool.body` on a value | the read resolves to the member under `open` | κ₃ |

**Facing a host.**

| # | Scenario | Given | Then | Leaf |
|---|---|---|---|---|
| H1 | An answer is named by text | any path of the catalogue | the text parses to the path and prints back the same | β |
| H2 | A root nobody declared | a demand under a definition that is not a root | "absent" | ε₁ |
| H3 | Naming a root is an edit | a definition used by a sibling, then declared as a root | its answers at its own defaults | ξ |
| H4 | Why a verdict is undetermined | a root's parameter with no value; an explicit `undef`; a division by zero; a read of an absent member; a member whose guard is a root's parameter with no value; the first again through a made instance | partial, partial, other, other, partial; the class survives the constructor call | η; the two guard cases in θ₁; the made instance in κ₂ |
| H5 | An edited session equals a fresh one | each entry with edits | every answer after each edit equals a fresh evaluation's | σ |
| H6 | The same instance in two places | every entry whose root can be held as a sub | equal answers; no body run of a member in the second | ε₂ |

## 11. Tasks, holds and priority

- **No hold applies.** No task of this PRD depends on the old-core holder (10510) or on the alpha gate (9739), directly or through another task. That is why tasks 7874, 5482 and 8732 get no edge.
- **Edges added when this PRD is decomposed:** leaf μ₁ as a dependency of task 10610, if it is still pending; the close leaf as a dependency of tasks 10552 and 10551, and then 10551's `deferred_watch` is cleared, as its text asks.
- **Edges onto the core's tasks:** in section 12.3.
- **Replaced tasks:** decision D9. The Held-task review session proposes each cancellation to Leo once the successor leaf has an id. Task 9024 was cancelled on his ruling of 2026-11-26, by that session, after its five dependents were settled.
- **The documentation obligation** (decision D6, as ruled): written onto task 10551 in the step that makes the close leaf its dependency.
- **Notes onto other tasks, at decomposition and with Leo's approval:** the waiting entries and the restated list of design 6.4 onto task 10610; the waiting entries onto task 10611; onto task 10551, what section 9 gives it.
- **Priority:** decision D12.

## 12. Decomposition plan

Thirty-three leaves. *New files* are under `crates/reify-eval-instances/src/` unless a path is given; `tests/` files are not listed. *Edits* names the existing files a leaf changes. Each leaf's task id follows its label. Three of the plan's thirty leaves were split at decomposition (section 12.5).

### 12.1 Rules that hold for every leaf

1. **The corpus is the acceptance.** A leaf's signal is that the entries which need its word run and pass, in both orders of request. It adds its word to the runner's list in the same change. An entry that needs words from two leaves neither of which waits for the other first runs when the later of the two lands, whichever that is; the leaf that lands later makes it pass or disputes it (section 6).
2. **Never an approximation.** A member whose kind is not built yet answers `no-kind-supplied`. Nothing is skipped, defaulted or answered from the definition's defaults.
3. **Section 6 applies.** A leaf may reword a rule it builds; it never edits an expected answer or a ruling. A disputed entry goes on the list.
4. **The existing compile's output does not change** (scenario C4).
5. **No file on the held list is edited.** A leaf that finds it must escalates.
6. **A new concept is a new file.** Every diagnostic has a code; the codes are declared by leaf β. A leaf's file list names files and never a directory.
7. **No test asserts a wall-clock bound.** Growth is asserted on the core's counters.
8. **Two files are the only ones that touch the core's interface where it still grows.** `core_seam` is the only file that builds what a body returns or calls `read_all`; `host/core_calls` is the only file that calls the core's host interface. A core task's agent will edit them knowing nothing else about this crate, so the head of each says what each helper promises, and a helper's signature is the contract with the rest of the crate. One promise is the claim of determinism: every result an instance kind returns is deterministic. Kinds written in this crate's tests build their results through `core_seam` too, and the test solver states its own claim there. Leaf ε₁ creates both files; a later leaf adds a helper only when it uses a call of the core for the first time.
9. **Before asking for a merge**, a leaf runs `python3 scripts/assert-module-scc.py check`.
10. **Tests are lean.** The catalogue is the test of behaviour. A leaf adds a test of its own only for what an entry cannot express: a property over generated inputs, a boundary scenario of section 10, or a seeded failure of the runner itself.
11. **Each kind whose result declares children supplies `steps`.**

### 12.2 The leaves

Leo's approval of this PRD rules its decisions, so no leaf lists a ruling as a prerequisite.

**Phase 0. The corpus and the path, before any kind.**

- **α (#10634). The crate, the corpus files and the runner's frame.**
  Registers `reify-eval-instances` and declares every dependency that crate will need. Turns each entry of the catalogue into its two files. Builds the expectation loader with a closed set of keys, the needs vocabulary and its owner table, the list of disputed entries, and a validity test. The test checks that every entry loads; that every word is in the vocabulary; that each source is of the kind its entry says (it parses; it waits for the purpose grammar and does not parse today; it is meant never to parse); that a source which parses has no bare `Scalar`; and that the count of waiting entries per owner equals the committed number. Registers the tree with the parse guard. Adds the census row the project's rule on declared exceptions asks for. Freezes the catalogue with a header.
  *Edits:* root `Cargo.toml`, `Cargo.lock`, `scripts/crate-strata.json`; the parse guard's list of trees in the tests of `reify-syntax`.
  *Signal:* the validity test in the merge gate prints every entry with "waiting" and its owner; an expectation with an unknown key, given to the loader as a string, is refused by name.
  *Prereqs:* none.

- **β (#10635). Typed paths and their text** (rule F), and this PRD's diagnostic codes. Replaces task 8704.
  The path type and its kinds of step; one module that prints and parses. A round-trip test over generated paths: every kind of step, names that look like fixed children, keys that hold a dot, a slash, a bracket or a quote. The validity test of α gains: every path in the corpus parses and prints back the same. Declares, in one edit of `crates/reify-core/src/diagnostics.rs`, the code of every compile error this PRD adds: those of section 5.7, of decisions D2, D5 and D10, and of the list D13, the read of a private `let` among them.
  *New files:* `crates/reify-core/src/instance_path.rs`, `crates/reify-core/src/instance_path_text.rs`.
  *Signal:* scenario H1; the corpus's paths in the merge gate; rule F's final text in the design.
  *Prereqs:* α.

- **χ (#10636). The spec says what was ruled.** Beside the chain.
  Amends `docs/reify-language-spec.md` for each sentence the rulings contradict (section 5.7), keeping each anchor: 9.2.7 among them, where a user function is now entered whatever its arguments (decision D10(d)), and the reference-safety rule of 6.3 and 8.10, which is deleted (the end of decision D13). Adds `present`, the applied purpose and the rules for a sub's braces, and says in 7.4 that the braces of a sub and the body of a purpose read only what is visible from outside (decision D13, item 14). The spec is read by two compiled checks that run only at merge, the keyword test of `reify-syntax` and the anchor lint, so this is a task through the merge queue and not a docs commit.
  *Signal:* the amended sentences on main; both checks pass.
  *Prereqs:* none.

**Phase 1. Shared code: the parser, the compiler's seam, the evaluator.**

- **γ₁ (#10637). The member form that applies a purpose** (decision D2).
  The grammar rule, the generated parser and its corpus; the GUI editor's grammar and its test; the syntax tree's new member and its lowering; an arm for it wherever the existing code matches on a member exhaustively; a coded error from the existing compile wherever the form is written (a definition's body, a `where` block, a sub's braces, a purpose's body, a trait). It removes the waiting tag from the corpus entries that now parse and changes the committed count.
  *Edits:* the grammar under `tree-sitter-reify`; `gui/src/editor/reify.grammar`; `crates/reify-ast/src/decl.rs`; `crates/reify-syntax/src/ts_parser.rs`; the files of `reify-compiler`, `reify-lsp` and `gui/src-tauri` that match on a member. It verifies the whole workspace and the GUI's lane.
  *Signal:* `reify check` on a file that applies a purpose prints the coded error; the corpus entries that waited for grammar parse, and the validity test says so; the editor's grammar test accepts the form; scenario C4.
  *Prereqs:* α, β.

- **γ₂ (#10638). The parser keeps two guards it drops.**
  The guard on an assignment in a sub's braces, and a port's guard. Additions to the syntax tree; the existing lowering ignores them.
  *Edits:* `crates/reify-ast/src/decl.rs`, `crates/reify-syntax/src/ts_parser.rs`, and the lines that build or match a sub's or a port's declaration.
  *Signal:* a parsed sub with `t = 6mm where thick` carries the guard; scenario C4.
  *Prereqs:* none.

- **γ₃ (#10639). The compiler's seam.**
  The expression compiler and the scopes become callable from another crate. `compile_expr` gains its mode, off by default. In the new mode: a reference is local to the unit, a read of `self.sub.member` included; a constructor call compiles to a request to make an instance at a site; a member read whose receiver is a cell or a sub compiles to a reference, also when the member may not be there; `auto` is accepted as a constructor argument where design 6.3 allows it; `present(...)` resolves; `determined`, `constrained` and `partially_determined` take a path through a sub; `self.children`, `self.members`, `self.descendants` and a bare collection are typed as values; a geometry member of a value can be read (`tool.body`); and a name can be resolved against two scopes, for a sub's braces.
  *Edits:* `crates/reify-compiler/src/expr.rs`, `scope.rs`, `lib.rs`, and the files whose items become public.
  *Signal:* for each behaviour, a test in `reify-compiler` that compiles a small source in the new mode and in the old; scenario C4. It unlocks δ₁ and δ₂ and has no entry of its own.
  *Prereqs:* none.

- **ζ₁ (#10640). The evaluator reads through one interface** (section 5.5, change 1).
  *Edits:* `crates/reify-expr/src/lib.rs`, `dual_eval.rs`.
  *Signal:* the workspace's tests pass with no expectation edited; no read of the value map remains outside the reader.
  *Prereqs:* none.

- **ζ₂ (#10641). "Not here yet", and reading everything outside an arm** (section 5.5, change 2; rule A).
  A new entry that can return pending. `eval_expr` keeps its signature. Under a reader that can answer "not here yet", a user function is entered whatever its arguments (decision D10(d)); under the old engine's reader it is not entered when one is undefined, as today.
  *Signal:* with a test reader in `reify-expr`'s own tests: a quantifier over 10,000 elements asks for all of them in one round; an `if` with a pending condition evaluates no arm; the two operand orders of each logical operator ask for the same refs; a function whose body ignores its parameter returns its value for an undefined argument, and returns undefined under a reader that never says "not here yet". The workspace's tests pass with no expectation edited, the ten tests that pin skipping among them. The frame-budget test's new figure is recorded.
  *Prereqs:* ζ₁.

**Phase 2. The first slice.**

- **δ₁ (#10642). The second output: the crate, the definitions and the member tables.**
  Registers `reify-eval-source` and makes `reify-eval-instances` depend on it. The entry, the output's types, the definition list with imports and the standard library. Every member form is in a definition's member table with its kind and its declared type, and no units yet; the members a definition inherits from its traits are in it, the traits' first; a `let` or a parameter of a geometry type has the kind of a geometry member and is never a cell. The corpus runner's compile step.
  *Edits:* root `Cargo.toml`, `Cargo.lock`, `scripts/crate-strata.json`, `crates/reify-eval-instances/Cargo.toml`.
  *Signal:* scenario C6's first half over every catalogue source that parses: a member form not yet lowered is in the table with its kind. Scenario C5: a seeded entry whose source is accepted where an error is expected fails the runner.
  *Prereqs:* α, γ₃.

- **δ₂ (#10643). The second output: units of source.**
  The scope that mints local references, with γ₃'s steps turned into typed paths. Lowered in full here: parameters, `let`s, constraints, applied constraint definitions, subs with their arguments (with the braces' rule for a name on an assignment's right side), the members inherited from traits, occurrence definitions, and what each definition uses. Content by meaning, with callees folded in. The compile error for a read of a private `let` from another definition's text (decision D13, item 14), in the one place where a name is resolved against another definition's members, so that the braces and purposes get it without code of their own.
  *Signal:* scenarios C1, C2 and C3 over every catalogue source that parses and is not a rejection; the entry `private-let-read-from-another-definition-rejected`.
  *Prereqs:* δ₁, β, γ₃, γ₂.

- **ε₁ (#10644). The host and the instance.**
  Loads the output into a snapshot. `core_seam`, `host/core_calls`, `step_map`, the table of member kinds, the definition table, the member table and the instance kind; the host's demand and answer. The runner evaluates an entry and compares its answers, in both orders of request. No member has a kind yet, so every member answers `no-kind-supplied`.
  *Signal:* scenario H2, and C6's second half.
  *Prereqs:* α, δ₁; core 10557, 10617, 10621, 10558, 10559.

- **ε₂ (#10645). The cell and the evaluator's adapter.**
  A parameter evaluates its argument in the scope that wrote it, or its default in its own instance. `determined`. The adapter's rounds. `unit_key`. The runner's two-places run.
  *Signal:* the entries that need `instance` and `eval-reads` and nothing later pass: a sub's argument, a default, an argument that reads the parent, a ring of `let`s as a cycle. Scenarios G1 and H6. (A root's parameter with no default first runs with η and ξ, and a recursion stopped by the depth limit with θ₁.)
  *Prereqs:* ε₁, δ₂, ζ₂.

- **η (#10646). Constraints, the summary, and why a verdict is undetermined.**
  The constraint kind; the built-in verdict on arguments; the cause and its class on a cell's result; the summary and its four groups.
  *Signal:* the entries that need `constraint` and `summary` and nothing later pass. Scenario H4's first three cases. (The design's decision 7 model, a `Bar` given a negative length that is violated at `Asm.b` where today it is "All constraints satisfied", first runs at κ₂.)
  *Prereqs:* ε₂.

**Phase 3. One capability at a time.**

- **θ₁ (#10647). Guards and presence.**
  The second output gains guards, guard cells, guarded assignments (with the braces' rule for a name in the guard) and `present`. A member reads its guards first; absent; presence undetermined; reading what is not there gives undefined, with the ref and the class recorded. The compile error for a guard that reads what it guards. A read of a member that may be absent compiles wherever it is written (the end of decision D13), so there is no other compile check of presence.
  *Signal:* the entries that need `guard` pass, with both operand orders of each. Scenario H4's absent member and its member of undetermined presence.
  *Prereqs:* η, γ₂.

- **θ₂ (#10648). One name under several guards** (rule E).
  `where`/`else`, the arms of a `match`, separate guards. One node per name, which chooses the declaration; `ambiguous-declaration`; the compile error for two kinds of node under one name.
  *Signal:* the entries that need `one-name` pass; rule E's final text in the design.
  *Prereqs:* θ₁.

- **ι (#10649). Collections.**
  List and keyed subs, the count's cell, elements by position and key, a `forall` block as one constraint per element, an index computed from data, a count that reads its own elements as a compile error.
  *Signal:* the entries that need `collection` and nothing later pass: an undefined count, an index out of range in both operand orders. A test of the leaf's own for the limit on children per node. (A count read from a sibling sub first runs at κ₁ and κ₃.)
  *Prereqs:* θ₁; core 10560, for a `forall` over a range, whose index is a value.

- **κ₁ (#10650). An instance read as a value.**
  `as-value`: the type's name, every present parameter, every value decided at or below, held as section 5.4 says. `let e = self.a`. `self.children`, `self.members`, `self.descendants` and a bare collection as values of present members. Equality by content.
  *Signal:* the entries that need `value` pass.
  *Prereqs:* η, ι.

- **κ₂ (#10651). Instances made by an expression** (rule B).
  The second output's units carry the table of constructor sites. The evaluator's walk keeps the site chain, and asks the reader to make an instance and to open a value. The adapter declares the child, passing each argument's value and the class of its cause, and reads its value; a parameter of a function keeps the class of the argument it was bound to, so that a wrapped call answers as an inline one (decision D10(d)); the list of what a cell made; the summary reads that list, exactly once.
  *Edits:* `crates/reify-expr/src/lib.rs`.
  *Signal:* the entries that need `made` and nothing later pass. Scenario H4 through a made instance, a call wrapped in a function included. Rule B's final text in the design. (The entries for a value made through a function and through a lambda, one made in a sub's argument counted once, a selecting built-in, two calls of one function, and a function called with an undefined argument also need `open` or `roots`, and first run at κ₃ or with ξ.)
  *Prereqs:* κ₁; core 10560.

- **κ₃ (#10652). Opening a value.**
  A member read on a value is a ref through `open`. One opened instance per holding cell; a value opened in mid-expression.
  *Signal:* the entries that need `open` pass, with the made instance's verdicts counted once and the opened one's never. Scenario G3.
  *Prereqs:* κ₂.

- **λ (#10653). Members written in a sub's braces.**
  The table of section 5.7: each form is compiled or is a coded error. A bare name is a `let` of the braces first, then the instance's own member, then the declaring definition; `self` is the parent. A name that resolves to a `let` without `pub` of the sub's definition is the compile error leaf δ₂ built, and does not fall through to the declaring definition.
  *Signal:* the entries that need `braces` and no solver pass: a false constraint in the braces is violated at the sub, where today the file reports all satisfied; the entry built so that a wrong resolution of a name flips its verdict; `t = t / 2` is a cycle; the entry `braces-read-of-private-let-rejected`.
  *Prereqs:* θ₁, γ₂.

- **μ₀ (#10654). What an instance tells a solve, from source alone** (section 5.6, items 5 and 11). Added at decomposition.
  The second output gains the unknowns, named or not, with their guards, the parameters given under a guard, the objectives, what a definition's constraints can read, and the compile errors for where `auto` may not be written. The `problem` node.
  *Signal:* scenario S1. The rejections fire by code in a test of the second output. The restated list of design 6.4 in the design.
  *Prereqs:* η, θ₁, λ, ι.

- **μ₁ (#10655). An unknown's cell and its seam to a solve** (section 5.6). A dependency of task 10610.
  The unknown's cell; the two bindings of `solves`; the summary reading the unknowns that have no name; the test solver.
  *Signal:* the entries that need `unknown`, `solve-forced` or `solve-infeasible` and nothing later pass. Scenarios S2, S3, S5, S7 and S8.
  *Prereqs:* μ₀, η, θ₁, λ; core 10561 and 10562.

- **μ₂ (#10656). An owner inside an owner.**
  An instance with an unknown of its own inside an owner that has one: a sub or a made instance given an argument on trial; a braces constraint that reads the parent's unknown; `auto` as a constructor argument.
  *Signal:* the entries that need `nested-solve` pass. Scenarios S6, S9 and S10.
  *Prereqs:* μ₁, κ₂; core 10563.

- **ν (#10657). Purposes, declared and applied.**
  The second output gains purposes in the definition list, the applied purpose in the applying definition's member table, and the compile errors of section 5.7 and decisions D2 and D10(e): an objective or an `auto` in a purpose, a parameter of kind Constraint or Field, a subject that is a value, a line in a sub's braces. A purpose's body that reads a `let` without `pub` of its subject is the compile error leaf δ₂ built (entry `purpose-read-of-private-let-rejected`). The instance declares one purpose instance per application, with its subjects bound to places. Everything below a purpose instance is kept out of the problem of every owner outside it.
  *Signal:* the entries that need `purpose` pass: a purpose that only checks, with a forced unknown so that bearing would show; a subject that is absent; one purpose applied twice; a value as subject refused; an objective refused; a line in a sub's braces refused. Scenario S4.
  *Prereqs:* γ₁, η, θ₁, λ, μ₁.

- **ξ (#10658). The default roots** (rule D).
  `default_roots` over the second output; a file with several roots; a file with none.
  *Signal:* the entries that need `roots` pass, one per clause. Scenario H3. Rule D's final text in the design.
  *Prereqs:* δ₂, ε₂.

- **ο (#10659). Connections, ports, placements, and a geometry member's source.**
  A `connect` as ordinary members; a port as a named group with its guard; an end that is a value refused; a placement as a cell, `at auto` and a connector's `auto` parameter as unknowns; a geometry member's unit and hash in the table; the same inside a sub's braces, where a connector sub the compiler makes is allowed though a written `sub` is not. The existing compiler's connector naming is counted by a ratchet, so the leaf does not copy it.
  *Signal:* the entries that need `connect` pass. Scenario G2.
  *Prereqs:* θ₁, λ, γ₂, μ₁.

- **π (#10660). Generic definitions.**
  One member table per specialisation reached; a type argument that could not be chosen is a failure at the sub.
  *Signal:* the entries that need `generic` pass.
  *Prereqs:* δ₂, η.

- **ρ (#10661). `constrained` and `partially_determined`** (decision D3).
  The relation "this constraint can read this parameter" is the one leaf μ₁ puts in the member table.
  *Signal:* the entries that need `state-tests` pass, with a sub, a made instance and an opened value agreeing.
  *Prereqs:* θ₁, κ₃, μ₁.

- **σ (#10662). Edits.**
  The runner's edit sequences. Adding an argument changes a member table and re-declares one child; editing a default re-runs only the cells that read it.
  *Signal:* scenario H5 and the entries that need `edits` pass. They pass by the slow path before core task 10564 lands, and exercise what a new snapshot inherits from the day it does.
  *Prereqs:* η.

**Phase 4. Integration and close.**

- **τ (#10663). The sources that stop compiling at the switch.**
  Runs the second output over every tracked `.ri` file and records which stop compiling, and why.
  *Signal:* the list, committed under `docs/` and named in task 10551.
  *Prereqs:* every leaf that adds to the second output: δ₁, δ₂, θ₁, θ₂, ι, κ₂, κ₃, λ, μ₀, μ₁, μ₂, ν, ο, π, ρ.

- **υ (#10664). What an instance costs.**
  The core's counters on generated models with 100, 1,000 and 10,000 instances, and the same of values.
  *Signal:* the report, committed under `docs/`; a test that N identical instances cost the body runs of one.
  *Prereqs:* ι, κ₃; core 10564, for the count of index entries.

- **φ (#10665). The whole corpus.** The integration gate of this PRD.
  Every entry whose needs are built passes. The list of disputed entries is empty. The waiting entries per owner are committed and named in tasks 10610, 10611 and 7874. Editing a default re-runs only the cells that read it, asserted on the core's counters. The runner turns on the core's debug mode. Every scenario of section 10 exists under its name.
  *Signal:* those, in the merge gate.
  *Prereqs:* every leaf above but χ; core 10564.

- **ω (#10666). Close.** Stamps this PRD and its manifest with the landed task ids, the frozen-record header, and the list of what stays maintained: the design document, the corpus tree, and the manifest. A dependency of tasks 10552 and 10551.
  *Prereqs:* every other leaf.

### 12.3 Order

| Leaf | Waits for, in this PRD | Waits for, in the core |
|---|---|---|
| α, χ, γ₂, γ₃, ζ₁ | nothing | |
| β | α | |
| γ₁ | α, β | |
| ζ₂ | ζ₁ | |
| δ₁ | α, γ₃ | |
| δ₂ | δ₁, β, γ₃, γ₂ | |
| ε₁ | α, δ₁ | 10557, 10617, 10621, 10558, 10559 |
| ε₂ | ε₁, δ₂, ζ₂ | |
| η | ε₂ | |
| ξ | δ₂, ε₂ | |
| θ₁ | η, γ₂ | |
| θ₂ | θ₁ | |
| ι | θ₁ | 10560 |
| κ₁ | η, ι | |
| κ₂ | κ₁ | 10560 |
| κ₃ | κ₂ | |
| λ | θ₁, γ₂ | |
| μ₀ | η, θ₁, λ, ι | |
| μ₁ | μ₀, η, θ₁, λ | 10561, 10562 |
| μ₂ | μ₁, κ₂ | 10563 |
| ν | γ₁, η, θ₁, λ, μ₁ | |
| ο | θ₁, λ, γ₂, μ₁ | |
| π | δ₂, η | |
| ρ | θ₁, κ₃, μ₁ | |
| σ | η | |
| τ | δ₁, δ₂, θ₁, θ₂, ι, κ₂, κ₃, λ, μ₀, μ₁, μ₂, ν, ο, π, ρ | |
| υ | ι, κ₃ | 10564 |
| φ | every leaf above but χ | 10564 |
| ω | every other leaf | |

What the core tasks give: 10557 the store; 10617 removes the attached binding; 10621 a step's space, a failure's description and public accessors for a path; 10558 cycles; 10559 limits and `steps`; 10560 children declared by a body, and constant bindings; 10561 views; 10562 storing the result of a body that opened a view, without which the two-places run would see a solve run twice; 10563 a view inside a view; 10564 what a new snapshot inherits, and with it the debug mode. None of this PRD's leaves waits for the core's later tasks on determinism or scheduling. The one task outside the plan does (section 12.5), and the close leaf waits for it.

The longest path inside this PRD has fourteen leaves: α, β, δ₂, ε₂, η, θ₁, ι, κ₁, κ₂, κ₃, ρ, τ, φ, ω. Two more are as long: through δ₁ in β's place, and through γ₃ and δ₁ in the places of α and β. In practice the core sets the pace: ι and κ₂ wait for 10560, μ₁ for 10562, μ₂ for 10563 and φ for 10564, which lie one after another on the core's own chain.

Leaves that edit one file land one after another, in whatever order the scheduler picks: γ₁ and γ₂ (the syntax tree and the parser); ζ₁, ζ₂ and κ₂ (`crates/reify-expr/src/lib.rs`); and, in the two new crates, the leaves that extend one kind or one lowering file.

### 12.4 The manifest

The capability manifest and its sidecar are written at decomposition. Each leaf's bindings are of three sorts: a capability the leaf builds, bound to a check that its word is in the runner's list; a core capability, bound to the core task that builds it; and a rejection, bound to the entry that fails if the source is accepted.

### 12.5 What the decomposition changed

The plan above was filed on 2026-11-26 by a team: three seats drafted the task records, one the capability manifest, and two critics read the batch. Where a task differs from the text that Leo approved, this section says so, and the task is right. Nothing here changes a ruling or an expected answer.

**Three leaves were split, so there are thirty-three.** None lengthens the longest path.

- **δ became δ₁ and δ₂**, and **ε became ε₁ and ε₂**, each cut at "the table, then the units". δ₁ builds the crate, the definition list and every member table with kinds and no units; δ₂ builds the units, content by meaning, the private-`let` error and what each definition uses. ε₁ builds the host, the seam files, the tables and the instance kind, with every member answering `no-kind-supplied`; ε₂ builds the cell, the evaluator's adapter, `determined` and the two-places run. Each was more than one task could hold: ε alone had seventeen new files against five core interfaces that are still moving. Every leaf that waited for δ waits for δ₂, and every leaf that waited for ε waits for ε₂.
- **μ₁ became μ₀ and μ₁.** μ₀ builds what a solve can read from source alone: the second output's record of unknowns, objectives and what constraints can read, the compile errors for where `auto` may not be written, and the `problem` node, with scenario S1. μ₁ keeps the unknown's cell, the two bindings of `solves`, the test solver and the three words. μ₀ waits for ι, because S1 and a keyed entry's unknown need collections.
- **Elsewhere in this text**, "leaf δ" means δ₁ for the member table, a member's kind, the definition list and the runner's compile step, and δ₂ for a unit, a reference, content, the private-`let` check and what a definition uses. "Leaf ε" means ε₁ for the host, the seam files, the tables and the instance kind, and ε₂ for a cell, the adapter, `determined` and the two-places run.

**Edges added.** δ₂ waits for γ₂, so that a guarded assignment in a sub's braces is never lowered as an unguarded one. υ waits for core task 10564, which adds the count of index entries its report needs.

**A leaf's signal is what the runner applies.** Thirty-one entries need words from two leaves neither of which waits for the other, for example the default roots and guards. Such an entry first runs when the later of the two lands, and that leaf makes it pass. So each task states its signal as: when the leaf adds its word, every entry all of whose words are then built runs and passes. Four signals in section 12.2 named behaviours whose entries run later, and are reworded there: ε's (a root's parameter with no default; a recursion stopped by the depth limit), η's (the design's decision 7 model), ι's (a count read from a sibling sub) and κ₂'s (five of its six examples).

**What a leaf builds that section 12.2 did not say.**

- δ₁ puts the members a definition inherits from its traits in its member table, the traits' first, and gives a `let` or a parameter of a geometry type the kind of a geometry member. Two catalogue entries rest on these and no leaf had them.
- δ₁ adds the edge from `reify-eval-instances` to `reify-eval-source` and the runner's compile step, which α cannot: the crate does not exist when α runs. δ₁ also writes scenario C5 as a seeded failure of the runner.
- θ₁ builds, on the second output, the compile error for a recursive sub with no guard (decision D13, item 13), and decision D10(a): the verdict on arguments is violated when an argument is given to an absent parameter.
- A made instance decides its own unknowns from whichever of κ₃ and μ₁ lands second; μ₂ keeps only what follows an outer trial.
- ι writes one test of its own for the limit on children per node. The catalogue has no entry for it, because the contract does not say at which path the failure is reported.
- φ asserts, on the core's counters, that editing a default re-runs only the cells that read it and that adding an argument re-declares one child. σ cannot: before core task 10564 a new snapshot inherits nothing.
- τ's list has two parts: the tracked sources that stop compiling on the second output, with the error; and the sources whose meaning the rulings change with no compile error, found by six syntactic tests (a use of `constrained` or `partially_determined`; a file that declares a function; a use of `self.children`, `self.members` or `self.descendants`; a sub's braces that hold anything but an unguarded assignment; a braces assignment whose right side has a bare name the sub's definition declares; a guarded sub, a sub in a `where` block or in the arms of a `match`).
- γ₃ gives the steps of a reference as written in the source; δ₂ turns them into typed paths. γ₃ does not wait for β.
- α also passes a third repository-wide sweep over tracked `.ri` files, `member_continuation_sweep`, which section 8 does not name, and edits the record of unswept trees in `docs/prds/v0_6/syntax-lowering-contract.md`. β also edits `crates/reify-core/tests/api_surface.rs`, which pins that crate's public interface.

**The corpus.**

- Three files hold the runner's state, all created by α in `crates/reify-eval-instances/tests/corpus/`: `built.toml` (the words built), `waiting.toml` (the number of waiting entries per owner; an entry is counted under every owner of a word it needs that is not built) and `disputed.toml`.
- The catalogue gives each expected compile error in words. α keeps the words; β writes each code into the corpus when it declares the codes, and from then on an expectation of a compile error with no code fails the validity test. Two codes have fixed names because other tasks rely on them: `PurposeApplicationNeedsNewEngine` and `PrivateLetReadFromAnotherDefinition`.
- Twenty-two rejections that section 5.7 and decisions D2 and D13 require had no catalogue entry: eight forms in a sub's braces (λ), an `auto` in a `match` arm, a lambda or a quantifier's body (μ₀), nine about purposes (ν), a recursive sub with no guard (θ₁) and the seeded one of scenario C5 (δ₁). Most need the purpose grammar and could not be parsed when the catalogue was written. The leaf that builds each rejection adds its entry, a source and the expected error code, in the same change. That is the one case in which a leaf adds an entry.
- The two-places run covers every entry that runs, expects no compile error, sets no limit and whose root has a default for every parameter. It counts body runs only for members whose answer is stored: the core stores nothing for `cycle` or `limit`, so a member that answers one of them, or is blocked by one, may run again in the second place, and for those only the answers are compared.
- Three entries were corrected before filing: `constrained-ignores-purpose-constraint` gained the word `state-tests`, `definition-objective-on-use-site-auto-open` gained `nested-solve`, and three entries' notes named the wrong leaf for `determined`.
- A scenario's test is named with a lower-case id (`scenario_s1`), because an upper-case one fails the lint. C4 is three tests, one per leaf that writes it, and C6 is two.

**One task outside the plan: #10667.** It switches on the core's sampled re-run of determinism claims for the corpus runner, and fixes what the re-run finds. Section 9 expected core task 10571 to switch it on in this crate. The Core PRD session left it to this PRD, so that a false claim in an instance kind cannot block a core task. It waits for φ and for core task 10571, and the close leaf waits for it. So the close leaf, and no other, waits for one of the core's later tasks; tasks 10551 and 10552 wait for the core's close task in any case.

**Decision D13, item 24, cannot be built today.** Braces on a list sub do not parse: `sub vents : List<Vent> { constraint w >= 3mm }` is a syntax error. The item stands as what the form means when the grammar gains it, with indexed subs (task 5482). No leaf builds it and no entry pins it.

**Decision D13, item 14, for a subject of type `Structure`.** A purpose's body reads only parameters, subs, ports and `pub` `let`s of its subject. Where the subject's type is a definition, a read of a private `let` is an error at the purpose's declaration. Where it is `Structure` or a trait, the read is resolved when the purpose is applied, and the error is at the application line. This is the lead's reading of the ruling.

**Checks not run.** The overlay's substrate-verification workflow probes premises through the `reify` binary, and no leaf's signal can be observed that way before the switch, so it was not run; section 8 says what the authoring seats ran instead. The documentation leaves the docs-truth rule asks for are not in the batch (decision D6).

**Verify cost.** ρ may have to edit `crates/reify-expr/src/lib.rs`; if it does it verifies most of the workspace, which decision D12 did not count. δ₁ takes δ's place among the leaves that edit the workspace manifest.

## 13. What the review found

Drafts 1 and 2 were each read by a design critic and by a check of every stated fact; draft 1 also by a critic of the task breakdown. The catalogue was merged from five drafts by a planning seat, traced entry by entry by three tracers, and corrected by three more seats. The reports are under `instance-prd/critics/` on the workstation.

**The first design critic** (21 findings) found the faults that matter in the seam to a solve, which is where the ruling on the braces meets the ruling on purposes and where nothing had been read before:

- Draft 1's test solver took a candidate when no constraint was violated, so it took one that left a constraint undetermined. On the design's own model (6.4, the beam feasible for one length) it would have answered 10 mm where the design says 48 mm.
- Draft 1 said a solve reads its problem from the owner's member table. After the ruling on the braces, three parts of an instance's problem are written at the use.
- A solve must know, before it opens its view, what its constraints read from outside. The design's rule named arguments only; a constraint in the braces reads the parent directly.
- A seat's reading of `constrained(p)`, which draft 1 recommended, failed the design's own requirement for a sub with a constraint in its braces. Decision D3 now recommends the narrower reading and shows the model.
- Draft 1 had no place for "the solve found nothing" among a cell's causes, and could not tell a partial design from another cause inside a made instance, because a value carries no cause.
- A purpose applied in a sub's braces had no meaning, and most member forms the braces accept had no stated fate.
- The position table had no reader in this PRD. It moved to the session layer.

**The tracers** traced all 139 merged entries by hand: 136 reproduced, and three did not (a path that should have gone through `open`; an edit sequence whose second edit was traced from the original text; a path below a member of undetermined presence, which answers that failure and not undefined). They found no wrong answer in the 38 entries on a sub's braces, purposes and unknowns. They found the faults in this PRD's text instead: the same faults in the test solver, from the other side; that a candidate must not be rejected by a constraint that is violated whatever the trial; that everything below a purpose must be kept out of a solve by path, or a constructor call inside a purpose's constraint lets the purpose change a solved value; that a summary must read the cells that are not members, because a guard's condition can make an instance; and that many entries' tags did not match the leaves; the seats that then corrected the catalogue changed about 130 tags.

**The second design critic** read draft 2, where those faults had been answered with new mechanism: 24 findings, five of them faults in the answers. The solver as rewritten read the owner's whole summary and so read arguments no constraint reads; an instance's `problem` could not be followed into a guarded sub without running its guard; the class of a cause had no carrier through a value; "everything below a purpose never bears" also stopped an instance made inside a purpose from deciding its own unknowns; and a leaf waited for a core task that does not yet store a solve's result.

**What that pattern means, and what draft 3 does about it.** The design session met the same thing: the choices about meaning held up under each round, and the mechanical rules grew a new fault with each fix. So draft 3 states the seam to a solve as thirteen things that must be true, each pinned by entries or scenarios, and gives the mechanism as a starting form that a leaf may build differently (sections 5.6 and 6). It does not claim the starting form is right. It claims the entries are, and they were traced three times.

**Decisions that came out of the review.** Two of draft 1's "smaller choices" were decisions of substance and are now D10(d) and (e). The merge's challenge to the path of a `let` in a sub's braces is D10(f). The second critic's are D10(g) and (h), and items 21 to 24 of D13. The tracers' cases with no answer are the path exclusion of section 5.6, the reworded D3, two rows of section 5.7, and items 16 to 20 of D13.

**The fact checks** (17 and 20 findings) found one quotation of Leo that the design carries only in a shortened form, two words in quotation marks that nobody had said, a wrong count of the longest chain twice over, a task (8303) that had been amended that day, and several counts stated more firmly than their sources. All are corrected in this draft.

**The breakdown critic** (25 findings) found that the words entries are gated by did not match the leaves, so most leaves' signals could not have been produced; that five signals needed something a later leaf builds; that draft 1 undercounted, at five of twenty-seven, the leaves that verify most of the workspace, which were eighteen; and that the arrangement with the core covered a body's calls and not the host's. The vocabulary is now one word per leaf, the signals are moved, the second output is a crate of its own, and `host/core_calls` is the second seam file.

**The catalogue.** 249 drafted entries were merged to 139. The merge found four places where two drafts disagreed and settled each from the design's text, not by majority. The gaps the merge, the tracers and the critics found added 49. After Leo's rulings it holds 191 entries: 176 need only what this PRD builds, 10 wait for the solve kinds, 3 for the geometry kinds and 2 for task 7874. 123 answers are traced, 62 are stated by the design, the contract or a ruling, and 6 are open, each owned by task 10610. The entries added after the tracing have been through the grammar and through the seat that wrote them, and not through a second tracer.

**Leo's rulings of 2026-11-26, and what draft 4 changed for them.** Seven entries that had waited for a ruling now pin their answers: two on `constrained` and one on a purpose's constraint (D3), an argument to an absent parameter (D10(a)), a function called with an undefined argument (D10(d), rewritten for the alternative he took), a `let` in a sub's braces read from the parent (D10(f)), and a parameter read back out of a value (D10(h)). Three entries were added for the read of a private `let` (D13, item 14). Four entries that pinned the compile error for an unguarded read of a guarded member were rewritten to pin the run-time answers (the end of D13). A scan of the catalogue's sources found no other entry that reads a private `let` from another definition. The text changed in sections 5.4, 5.5 and 5.7, in decisions D5, D6, D9, D10 and D13, in sections 8, 9 and 11, and in leaves β, χ, ζ₂, δ, κ₂ and θ₁. The six new or changed sources were parsed with the binary of 2026-11-23.

**The decomposition** was read by two critics, whose findings are in the tasks and in section 12.5. The text that changed after their reading (the split of δ and ε, and the corrections to forty-odd records) was checked by scripts for labels, dependencies and names, and by no further reader.

**Not reviewed.** Draft 3's own changes were checked against the second round's findings by one seat and were not read adversarially again. The changes made for Leo's rulings of 2026-11-26 were made by the lead and checked by one seat before decomposition.

**Amendments to the design document, landed with it:** decision 3b's line 1 and Part 4's "The compiler still reports the plain mistake", which Leo's ruling of 2026-11-26 removes; 6.4's requirement 1, which now holds for an undefined argument as well (decision D10(d)); 6.4's list of what is available without evaluating; rule A's wording on "not here yet"; Part 4's form of a guarded assignment in the braces; Part 10's sentence that a purpose can apply a constraint definition, which is true only once this PRD lands; 15.2's statement that one fixture has an `Output` in a purpose, which a scan did not find; Part 17's "the core crate's toy solver".

## 14. Design invariants

The project's invariants (`docs/legibility/design-invariants.md`), walked against this sketch. The ones that bear:

| Invariant | Where it bears | How it is met |
|---|---|---|
| An undefined value has a cause that can be found | cells and verdicts | a cell's result names a root cause or the refs it read, and its class |
| An undetermined verdict can be attributed | the summary | each one is reported with refs and its class |
| A declared intent is consumed or diagnosed | members in a sub's braces; an argument to an absent parameter; an applied purpose on the old engine; a member whose kind is not supplied; a member form not yet lowered | each is evaluated, or is a verdict, a coded error or a named failure |
| A placeholder is owned and loud | entries that wait; disputed entries; empty rows of the table of member kinds | each waiting entry names its owner and is counted; the disputed list blocks the integration leaf; an empty row is a failure at the member |
| Diagnostics carry codes | every new compile error | leaf β declares the codes |
| Nothing is vacuous and unowned | the runner | the validity test fails on a waiting count that differs from the committed one, and on a word outside the vocabulary; leaf α adds the census row |
| Module cycles only shrink | the two new crates | layers import downward only; kinds are named by constants in a file that imports no kind |

## 15. Open questions (tactical; decided by the leaf named)

1. **Lift or copy** each piece of `compile_entity` the second lowering needs (δ₁ and later): section 5.2 gives the rule.
2. **Which spaces the steps use** (ε₁): one for members by name, one for the fixed children, one per collection for its keys, or fewer. The core compares a space and never interprets it.
3. **How a value is written in an expectation and compared** (α): as a Reify literal evaluated by the kept evaluator, or parsed by the runner. Decided by α (#10634): in the catalogue's answer notation (`8 mm`, `Plate{t: 3 mm}`, `undefined`), parsed by the runner when the entry loads into a type of its own; how it is compared with an answer is fixed by the first leaf that compares a value.
4. **One test per entry, or one per theme** (α): the runner must name each failing entry either way. Decided by α (#10634): one test over the whole corpus, which collects every problem, each naming its entry, fails once at the end, and prints each entry's state.
5. **The key of a unit of source as a snapshot input** (ε₂): one function builds it; nothing parses it back.
6. **Where the module is written in a path's text, and how a quote inside a key is escaped** (β). Decided by β (#10635): a module other than the entry file's is written as its dotted path, then `::`, then the definition (`std.units::Plate`), and the entry file's module is not written; inside a key `\"` is a quote and `\\` a backslash, and there is no other escape. The full text is rule F of docs/instance-kinds-design.md.
7. **Whether `relate` and `chain` are constraints or placements** (ο): the leaf reads how the existing lowering treats each; whatever places a part goes to task 10611. And whether an end of a `connect` written in a sub's braces may name a sibling of the sub.

## 16. What is not settled, and what would settle it

- **What a solve does** in the three cases of design 6.4, and whether what a braces constraint reads of its writer's scope is read before the sub's solve opens its view or through it (section 5.6). Owner: task 10610. The catalogue has an open entry for each.
- **The mechanism of sections 5.4 to 5.6.** It is a starting form. Known not to be settled: when the test solver reads the guards that decide which unknowns are present; how the list of what constraints read from outside is closed over an instance that a cell makes; how each of several constructor calls in one expression gets the class of its own arguments; how a binder carries the class of what it was bound to, which decision D10(d) requires of a function's parameter and which is not decided for a lambda's or a quantifier's; how many spaces a step needs. Each is settled by the leaf that builds it, under the entries (section 6).
- **Whether an objective written in a definition reaches an unknown owned by another instance** (contract Example 11, forms (3) and (4): `Plate(t: auto)` and its constraint). Owner: task 10610.
- **Whether the grammar takes `purpose Name(args)` without a conflict.** Settled by generating it, in leaf γ₁.
- **How wide the public surface of `reify-compiler` has to be** for the second output to live outside it. Settled by leaf γ₃; the fallback is in decision D11.
- **Cost.** Nothing in this area has been measured: no time, no memory, no count of index entries. Leaf υ's report is the first measurement.
- **Every observed behaviour of today's engine** came from a binary built on 2026-11-23, behind main.
- **What the switch deletes that this PRD duplicates.** Any piece of `compile_entity` that a leaf copies is listed here by that leaf, with task 10551 as the owner of its removal.
  *δ₁ copied:* the always-Indeterminate `ConstraintChecker` the second output passes to the program compile (`IndeterminateChecker` in `crates/reify-eval-source/src/program_modules.rs`) is a copy of `compile_builder::auto_type_param_phase::CompileTimeIndeterminateChecker`, which reify-compiler keeps `pub(crate)`.
  The `#no_prelude` test (`has_no_prelude` in `crates/reify-eval-source/src/module_context.rs`) is a copy of the one in `compile_builder::pre_pass::effective_prelude`.
  The parse of a stdlib module and the modules it binds against (`parse_module` and `imports_of` in `crates/reify-eval-source/src/stdlib_modules.rs`) copy the recipe of `stdlib_topo::compile_against_imports`: parse with the enum names of the modules its `CompiledModule.imports` names, and bind against those modules.
  The scope a definition's units compile in (`crates/reify-eval-source/src/definition_scope.rs`) is γ₃'s test helper in `crates/reify-compiler/tests/harness_compilation_surface/source_unit_mode_tests.rs`, promoted, which fills a `CompilationScope` from a `TopologyTemplate`.
  *δ₁ reads, not copied:* the second output takes declared types, the geometry classification of a `let` and the `TopologyTemplate`s its unit scopes are built from, from the existing compile's `CompiledProgram` and the cached stdlib build, and binds sub and trait names with `name_env::bind` over them, so the switch must keep that producer or replace it before it deletes the old lowering.
