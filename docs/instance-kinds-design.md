# Instances on the evaluation core: the design

**Status:** RULED. Leo ruled on every decision of Part 14 on 2026-11-25 and 26; each ruling is marked where it applies. The PRD built from this design is `docs/prds/instances-on-the-core.md`, which he approved on 2026-11-26. This is draft 5 of the design as it stood when he finished ruling on it, landed with that PRD and carrying the amendments listed below. Three rounds of critics read drafts 1 to 3. Nothing written since draft 3 was reviewed by a critic as part of this document; the PRD's section 13 says what its own review covered. The seven rules of Part 15.1 are settled by building them under the PRD's catalogue of models ("LGTM"), and the leaf that builds each rule rewrites its text here to say what was built.

**Who builds what from here.** The PRD builds Parts 2 to 8, 10 and 11, rules A, B, D, E and F, and the half of rule C that the instance kinds decide. Where the PRD and this document differ, the PRD is right. Part 9 and rule G are the session layer's (task 10551). What Part 6 leaves to a solve is the solve design's (task 10610). Shapes, and an `Output` in a purpose, are the geometry and compute design's (task 10611). Part 16 lists the hand-overs in full. A statement about "today" describes the old engine on the date given and is not maintained.

**Amendments made when it landed, 2026-11-26.** Each is also marked in place with "Amended".

1. Decision 3b, line 1, and Part 4: the compile error for an unguarded read of a guarded member inside one definition is removed. Leo ruled on 2026-11-26 that a read of a member that may be absent compiles wherever it is written (PRD, the end of decision D13).
2. Part 6.4, requirement 1, holds for an undefined argument as well: a user function is entered whatever its arguments (PRD decision D10(d), Leo's ruling of 2026-11-26).
3. Part 6.4, "What is available without evaluating anything": restated in the PRD's section 5.6, item 5. The list here was written before the ruling on a sub's braces.
4. Part 6.2: a guarded assignment in a sub's braces is written `t = 6mm where thick`. The block form does not parse.
5. Rule A: "not here yet" is built as an outcome of the evaluator's walk and not as a value (PRD section 5.5).
6. Part 10: a purpose's body accepts an applied constraint definition only once the PRD's purposes leaf lands.
7. Part 15.2: no tracked source has an `Output` in a purpose. A scan found none.
8. Part 17: the seam to a solve is tested by a small solver in the instance crate's own tests. The core's toy solver cannot be used from another crate.
9. Points this document left open that Leo ruled with the PRD are in the PRD's section 7 and are not restated here: what `constrained(p)` reads (D3), the syntax for applying a purpose (D2), the eight points of D10, and a `let` without `pub` (D13, item 14).

**How to read it.** Part 14 holds the decisions, each with its own model, and can be read first. Parts 1 to 13 are the design those decisions rest on. Part 15 is in two halves: seven mechanical rules that three rounds on paper kept finding new counter-examples in, and what is not designed. The seven are stated in their current best form, and should be settled by building them under the critics' models, not by a ruling on their wording.

**What it is.** A design for the first node kinds on the new evaluation core: how a `sub` and a structure value are evaluated. It sits beside `evaluation-core-contract.md` ("the contract") and must live inside it.

**Other work it refers to.**

- **The solve design**: a later design for how solves are formed and how objectives travel. Leo left both to it.
- **The geometry design**: a later design for shapes, meshes and placement.
- **The core-PRD session**: the session writing the PRD for the core crate. It owns the contract's text.
- **The held-task review**: the session sorting the tasks held while the new core is built.

**Grounds.** "Today" means the code at main `61b77ee152` as a seat read it, or a run of the prebuilt binary, which is about 225 commits older. Each such statement comes from a report in `seats/`. Counts over the `.ri` files are from regex scanners over the 996 tracked `.ri` files, so they are approximate and leave out sources embedded in Rust tests.

**Leo's words** appear in quotation marks only where they are his. Everything else said about a ruling is my reading of it.

## Words

From the contract, in a phrase each: a **node** is one computation at one path; a **root** is where a path starts; a **binding** is a named input a node was given; a **ref** is a relative address a node reads through; the **store** keeps results by content, and may **trim** one for space and compute it again; a **view** is the state as it would be under a **trial**, one candidate assignment a solver tries; a **setting** is an input of the session that is not source, such as a limit; a **verdict** is a constraint's result.

This design adds:

- **Definition**: a `structure def`. **Entry file**: the file a command or the GUI was given.
- **Instance**: one use of a definition at one path. A root, a sub, a collection element, and what a constructor call makes.
- **Cell**: a node that evaluates one expression: a `param`, a `let`, a guard's condition, a collection's count, a placement.
- **Unit of source**: one compiled expression or list that a node reads as its source (the contract's term).
- **Undefined** is said of a value: the language's `undef`. **Undetermined** is said of a verdict, or of whether a member is present: it cannot be decided because something it reads is undefined. The command line prints such a verdict as INDETERMINATE.
- **Inside an instance**: at a path below it. Its members, its subs and everything they contain, and the instances its cells make.
- **Place**: a root, a sub of a place, an element of a collection of a place, and their members. A sub inside an instance that a constructor call made is not a place.
- **Owner** of an unknown: the instance that decides it.
- **Bears on**: a constraint bears on an unknown when it takes part in choosing that unknown's value. A constraint that does not bear on it is only **checked** against the result.
- **The default objective**: what chooses among feasible values when no objective is written. Today it picks the middle of the feasible range, and the models below assume that. How it reaches an unknown is the solve design's.
- **Member table**: a definition's members, read from source alone. (Drafts 1 and 2 called it a "shape", which the contract uses for a solid.)
- **Made instance**: the instance a constructor call creates.
- **Opened value**: an instance built from a structure value, with nothing left to decide.
- **The braces**: the body written after a sub, as in `sub a : Beam { len = 4mm }`. The spec calls it a specialization scope.
- **Evaluated exactly once**: said of a constructor call that runs once whenever its member is evaluated. It is not inside a lambda, a function body or a quantifier's body, and not in an arm of an `if` or a `match`.

Path notation: `root/a/len` (names), `root/vents/[2]`, `root/vents/["intake"]`, `root/defs/[Plate]` (a member table), `root/c/made[0]` (the instance made by the first constructor call in cell `c`), `root/c/open` (the value of `c`, opened), `root/a/as-value` (an instance read as a value).

---

## 1. The design in eight rules

1. **One kind makes every instance.** A root, a sub, a collection element and a made instance are the same kind of node. They differ only in what their creator gives them.
2. **A cell is one unit of source evaluated in a scope.** A parameter evaluates the argument's source in the scope that wrote the argument, or else its own default in its own instance. There are no separate argument nodes.
3. **An `auto` belongs to the instance of the definition whose text contains it.** A constraint bears on it only if that constraint is inside the owner. No constraint outside the owner does. Whether an *objective* outside can reach it is left to the solve design, as the contract leaves it.
4. **What holds an instance decides what it takes part in.** A sub is contained in, placed by and listed by the instance that declares it, wherever that instance is. A value is held by a cell and is none of those. Being drawn in the viewport, exported or listed in the bill of materials needs a place.
5. **A structure value is content:** its type, every parameter's value, and every value decided inside it. It names no path. Reading a member builds an instance from it with nothing left to decide.
6. **Each guarded member checks its own guard.** Under a false guard it is absent. The instance containing it reads no guard.
7. **A member table is source only.** Whatever needs a definition's members without evaluating anything reads it.
8. **A path is typed, and its text form must be one-to-one.** One module turns a path into text and back. What a person sees beside it is a label, not an id.

---

## 2. The node kinds

| Kind | Reads | Result | Declares |
|---|---|---|---|
| **definition table** | The list of definitions of every module the root reaches | Their qualified names | One member table per definition, and per specialisation of a generic one |
| **member table** | One definition's list of members | Each member's kind, its units of source, its guards, and for a sub or a constructor call the definition it names and its arguments | Nothing |
| **instance** | Its guards, if it is declared under any; then its member table | Present with its members; or absent; or presence undetermined | One child per member, an applied purpose included. Fixed children: `as-value`, `summary`, and the solve design's nodes |
| **cell** | Its guards. Then the first of these that applies: a value it was given; the argument's source, evaluated in the scope that wrote it; its own source, evaluated in its instance. If that source is `auto`, the solve that decides it | A value with its determinacy, or a failure | From its body: an instance per constructor call reached, an opened value per member read of a computed value, and one small child listing the instances it made. From its result: `open`, when its value is a structure value |
| **constraint** | Its guards; its source; values | A verdict, and refs to what it is about. Not the measured values | As a cell's body does |
| **collection** | Its guards; then the count cell (a list) or nothing more (a keyed collection's keys are in the member table) | The element steps | One instance per element, given its index or key |
| **as-value** | Every parameter of its instance and every unknown decided at or below it | A structure value | Nothing |
| **summary** | Each verdict of its instance; the summary of each child instance, element, made instance and purpose instance | Counts, and refs to what is violated, undetermined or failed | Nothing |

None of these opens a view or uses what was there last time. Only a solve does, and the solve kinds are the solve design's.

**One member table per definition.** A root declares one definition table. It covers every module the root reaches, imports and the standard library included, by qualified name, with one entry per specialisation of a generic definition. Every instance is passed the definition table and reads its own definition's entry. What a use gives is in the *declaring* instance's member table, in the entry for that sub. The tables of a recursive definition form a loop, not an infinite tree.

**Where a definition's name appears.** In the definition table's steps, in a member table where a sub or a constructor call names its definition, and in a structure value's type. Nowhere else. So renaming a definition re-runs the member tables and instances that name it and what holds its values, but nothing they contain: those are found by content. The contract's 5.3 says no name of a definition enters a key; this needs amending (Part 16).

**Why presence is read by the member itself.** The contract's illustration has a separate "present-if" node. For `root/thin/t` to resolve, the node at `root/thin` must declare `t`, so it must be the instance. The spec says the same in its own terms: a `where` block is a guard on each declaration inside it (6.3).

**Parameters need no argument node.** The parameter's cell is given the argument's source and a binding to the scope that wrote it. That needs a declaration to be able to pass one of the declarer's bindings on to a child. The core-PRD session proposed that sentence for the contract, and reports that Leo has approved it with the core PRD's other amendments.

**The verdict holds no measured value.** Contract Example 2 leaves this to the kind. A verdict that held the measured volume would disturb its summary on every edit that keeps it satisfied. A panel or a tool that wants the measured values asks for them separately.

**What a cell can return.** A value, which may be undefined, as in the language today. Or a failure: a cycle, a limit, a kernel error, or being blocked by another failure. A failure cannot be caught by an expression. An undefined value can be tested with `determined`.

**Why a value is undefined (changed in draft 4).** An undefined *value* carries no cause. A cause that named what was read would put a path inside a value, and two equal values would differ by it. But a cell's or a constraint's *result* that is undefined or undetermined also names, by ref, the undefined things it read. The reason shown to a person is found by following those refs back to what was not there. A solve can follow them too, which rule C of Part 15 needs.

**An argument that is undefined is reported (draft 4).** Leo's ruling recorded in task 9345 keys that report on the root cause, not on where the instance sits, and tasks 9252 and 9253 ask that an undefined constructor argument is never silent. So each instance has one built-in verdict, "every argument I was given is defined". When one is not, that verdict is undetermined and its cause is the root cause.

**Finding the instances a cell made.** The core gives the list of children a body declared to its host program, not to another body. So a cell that made instances declares one small child that holds the list, and a summary reads that child. Three rounds found faults in which lists a summary reads; the current form is in Part 15, rule B.

**How the expression evaluator reads.** The evaluator runs with the answers it has and runs again when more arrive. Three rounds found faults here too; the current form is in Part 15, rule A. One part of it is a choice about the language and is listed in Part 14 (decision 12): only the condition of an `if` or a `match` decides whether something is read. Both operands of `and`, `or` and `implies`, and every element of a quantifier, are read.

---

## 3. Roots

**Today.** The command line, the language server and the GUI evaluate every definition in a file as a design at its own defaults. The command line takes no definition's name; the GUI's definition preview evaluates one. A definition meant only as a child prints its defaults and verdicts as if it were a design, and where a parameter has no default (91 of 1,434 definitions outside the standard library) it shows undefined values and undetermined verdicts. Geometry already uses a different rule: the viewport and export start from the definitions no other definition uses as a non-collection `sub`. Under that rule independent definitions are drawn overlapped at the origin, and a definition used only through `let c = Child(...)` is exported alone at its defaults (run). A direct import's `pub` definitions are evaluated as roots of the importing file; the code calls that merge interim.

**The design.**

- A root is an instance of a definition with no arguments, named by the session. Evaluating a definition "on its own" is not a mode. It is that instance.
- Any definition can be named as a root: `--root Bolt` on the command line, "view as design" in the GUI, a tool's request. The session declares the root, and that declaration is an input of the snapshot, so naming a new root is an edit and a path under a root that was never declared answers "absent". The core-PRD session reports that Leo ruled this on 2026-11-25 ("a root's declaration is a snapshot input"). Which roots the session declares by default is what this part proposes.
- **By default the roots are the definitions of the entry file that nothing else in that file uses.** The exact meaning of "uses" is Part 15, rule D. In outline: definitions that use one another are judged as a group; a definition is used when another names it as a sub's type, in a constructor call, or as the type of a member; a use by a test does not count; a `@test` definition is not itself a default root.
- When in doubt the rule makes a definition a root. An extra root costs some output. A design that silently is not checked costs more.
- Imported definitions are not roots.
- One rule for evaluation, checking, drawing and export. The geometry-only rule goes.
- **With several roots,** `check` checks each. `build -o` and export need one: with several and none named, the command fails and lists them. The GUI views one root at a time: the first in the file, then whichever the user last chose for that file. Its status line names that root and says how many other roots the file has and how many of them have a violation.

**A parameter with no default** is undefined at a root, which the spec calls "a legitimate, partially-specified design" (9.2), and its verdicts are undetermined. Leo's ruling recorded in task 7874 makes an omitted required argument a compile error at a construction, and the task's record keeps a root's parameters apart from that.

**What it costs.**

- `eval` and `check` stop printing child-only definitions at their own defaults, and every id changes from a definition's name to a path. A seat found between 40 and 125 test lines in about 25 files that pin the values of a definition that is used as a child. It scanned three test directories and rates the count low-confidence; tests built through the builder interface and expectations that only count verdicts were not scanned.
- A definition used by a sibling is no longer checked at its own defaults unless it is named.
- A definition used only as a value stops being exported alone.
- Several roots stays common in fixtures. A census under a different rule from this one found 270 files outside the standard library with several definitions: 171 with exactly one that no sibling instantiates, and 99 with several. No count was made under this rule.

**Alternatives.** Every definition is a root, as today: cheap on the new core, because equal instances share work, but it keeps the noise and offers an agent `Child.t` as the thing to edit. Or check every definition and draw only the default roots: it keeps today's verdict coverage and removes the overlapped drawing, at the price of two rules. Or a marker in source: explicit, but it needs grammar or a loader, and files with several candidates still need a default.

---

## 4. Guards and alternatives

**Today.** A guarded sub on a definition that is not recursive is never created, even when its guard is true; the code comment says a guard on a sub is meaningful only for recursion (task 8414). A guarded `let` or `param` is a cell holding undefined, while a guarded constraint is absent. Guards are silently discarded on a port, on a geometry `let`, on a collection's count constraint, and on a member inside a guarded block. A guard inside a child is decided by the definition's default values. The arms of a `match` that declare subs are never created. A seat found nothing that rejects a guard that reads the thing it guards. A read of a guarded member from outside its guard gets a warning, where the spec says error.

**The design.**

- **`where` means presence for every member kind.** The spec: "When false, the entity does not exist in the evaluation graph." A member under a false guard is **absent**: its path answers "absent", it has no value and no verdict, its verdicts are not counted, and `self.children` and the like skip it.
- **A guard's condition is a cell of the declaring instance**, read by each member it governs. A nested guard reads the outer one first, so an inner condition is never evaluated under a false outer one.
- **A guard is read per instance.** A guard inside a child follows that instance's arguments.
- **One name declared under several guards is one node.** The arms of a `where`/`else` and of a `match` are the common case. The details, with undefined guards, are Part 15, rule E.
- **An undefined guard is not "false".** The member's presence is **undetermined**. The summary counts it with the undetermined verdicts, and `check --strict` fails on it as on any of those. A design with `param cooled : Bool` and `sub fan : Fan where cooled` is a legitimate partial design. Today an undefined recursion guard stops silently.
- **A guard may read anything a cell may read, except what it guards.** That includes a solved value: the member is then present in some trials and absent in others, which the core supports. The compiler rejects the direct case, such as a guard on `fan` that reads `fan.flow`. An indirect one, such as a guard that reads `self.children.count`, is reported by the core as a cycle.
- **Recursion needs no mechanism of its own.** `sub left : Tree where depth > 0 { depth = self.depth - 1 }` is a guarded sub whose guard is read per level. The limit on depth is the core's setting for the depth of a path.
- **A guarded collection is absent, not empty**, when its guard is false.

**Reading what is not there.** One rule covers a member that is absent, a member whose presence is undetermined, an element past the end of a collection, and anything below any of those:

> Reading something that is not there gives undefined.

- A verdict that reads it is undetermined, and when it is shown it says what was read and why that is not there. Nothing is silent: today the same reads give undefined with no reason found.
- **It composes with the language's logic.** The spec makes `and`, `or` and `implies` three-valued, and says they are "commutative with respect to `undef`", "not imperative short-circuit evaluation" (9.2.3). So when a parent `Top` has `sub c : Case` and the case has no fan, `constraint not self.c.needs_fan or self.c.fan.flow > x` in `Top` is satisfied whichever way round it is written.
- **The compiler still reports the plain mistake.** Inside one definition, a read of a guarded member from a declaration that has no guard of its own, outside any arm of an `if` or a `match`, is a compile error. The spec's reference-safety rule says more than this (the reader's guard must imply the member's); this is the part of it the compiler can decide without proving anything. A name declared in every arm of a `where`/`else`, or of a `match` that covers every case, is not a guarded member. Inside `Case` itself, then, the constraint above is an error, and is written `constraint self.fan.flow > x where needs_fan`. **Amended 2026-11-26:** Leo ruled that this compile error goes. A read of a member that may be absent compiles wherever it is written, and inside `Case` the constraint above is satisfied as it is in `Top` (PRD, the end of decision D13).
- Everything else is left to run time: a reader under some other guard, a read inside an arm, a read across an instance.
- **A test for "is it there".** `determined(self.c.fan.flow)` is false when there is no fan. It is also false when there is a fan whose `flow` is undefined for another reason, such as a solve that failed, so `if determined(self.c.fan.flow) then ... else 0` would hide that. A test of presence alone, `present(self.c.fan)`, would not. Adding it is a language change (decision 3).
- Draft 2 made such a read a failure. The second round showed that the answer then depended on the order of a constraint's operands.

**What is written in a sub's braces.** The spec lets them set parameters and add a `constraint`, a `let`, a `connect` and a `where` (8.7). Today only parameter assignments are compiled. A `let`, a `constraint`, a `connect`, a `relate` and a `where` written there are dropped with no diagnostic: a false constraint in a sub's braces still reports "All constraints satisfied" (run). An objective gets a warning and is dropped.

In this design they are compiled, **as members of the instance the braces follow** (ruled by Leo, decision 4). A constraint written there is inside that instance, so it bears on the instance's own unknowns like any constraint of its definition. If it cannot be met together with the definition's own constraints, the instance's solve fails as unsatisfiable (decision 4). An assignment is an argument: it is the declaring instance's, and a guarded assignment (`where thick { t = 6mm }`) is an argument present under that guard, with the default otherwise.

**Names inside the braces** (ruled by Leo, 2026-11-26: "Agreed"). A bare name is the instance's own member if it has one by that name; otherwise it is looked up in the declaring definition. `self` is the declaring instance, the parent. With `Plate { param t : Length = 3mm }` and `Asm { param t : Length = 10mm   sub p : Plate { t = self.t / 2   constraint t >= 4mm } }`, `p.t` is 5 mm and the constraint reads the plate's `t`. This changes one thing from today: `t = t / 2` in the braces reads the parent's `t` now and would read the plate's own, which is a cycle and is reported. A scan of the tracked `.ri` files found 10 assignments in 22 braces, none with a bare name on the right that the child also has.

**An argument that depends on what it helps decide is a cycle (changed in draft 4).** A solve reads only the arguments that its own constraints and objective read. If one of those arguments depends on what that solve decides, there is a cycle and the core reports it. `sub a : Plate { note = self.a.area / 40mm }` is not one, as long as no constraint of `Plate` reads `note`. A placement is not an argument a child's solve reads, so a mate that places a part from that part's own size is not one either. Draft 3 said a solve reads all its arguments, which made both of those cycles. No sub in the tracked `.ri` files outside the standard library has an argument that reads the sub itself or a sibling's member (0 of 478, a critic's scan).

---

## 5. Collections

**Today.** A list sub's elements are built from the count as it stood before any solve, so a solved count creates no elements. The count constraint `xs.count == n` is compiled into a hidden count cell. All elements of a list sub get the same, empty arguments. A keyed sub has its keys and per-key arguments in source. The indexed form `sub ps[i in 0..<n] = P(...)` is parsed and rejected (task 5482). An index out of range gives undefined with no message; the spec says it is a failure (3.4). A bare reference to a collection gives its first member's values, where the spec says a list of the elements.

**The design.**

- **The collection is the node at the sub's name.** It reads its count cell and declares one instance per element. A keyed collection reads only source, and each key's entry in the member table carries that key's arguments.
- **An element's step is its position or its key, and that is its identity.** When the count changes, element `i` is the same node while `i` is below the count; elements come and go at the end. Selection and "what was there last time" follow the step. A design that needs identity to survive an insertion uses keys.
- **A solved count works by dependency order.** The collection reads the count, which reads the unknown, which reads its solve. There is no return to structure after solving, because there are no phases.
- **A count may read anything except its own elements.** The direct case is a compile error whose message gives the way out: `let proto = Bolt()` and read `proto.rated_load`. That costs nothing while elements take no arguments, because equal instances share work. Once elements take arguments (task 5482) the arguments must be written again for `proto`, and a purpose active on an element does not reach `proto`.
- **`xs[i]` with a computed `i` is a ref computed from data.** An index out of range reads something that is not there, by the rule of Part 4. That departs from the spec's "failure" (3.4), for the same reason: `i < xs.count and xs[i].area > 0mm^2` should not depend on which operand is written first.
- **An undefined count** makes the collection's elements undetermined, as an undefined guard does a member.
- **A `forall` block that declares constraints gives one constraint per element**, declared by a node that reads the collection. So an edit to one element re-checks one constraint, and a count that is an expression works (today `count == n*2` gives no per-element verdicts). A quantifier inside an expression stays one value.
- **A bare collection is a list of its elements as values.** `self.children`, `self.members` and `self.descendants` list present members only, as values. If a member's presence is undetermined, the list is undefined. Today these are lists of path strings and include absent subs. What an objective over such a list reaches is the solve design's.

---

## 6. Unknowns: who owns an `auto`, and where it may be written

Leo ruled, of a sub and a value: "let and sub should behave the same way (in how they handle auto and constraints). Objectives will probably have to differ." And he answered "Yes" to: a structure value is closed, naming it changes nothing, and each written `auto` is one decision made where it is written.

### 6.1 One rule for constraints

> An `auto` belongs to the instance of the definition whose text contains it. A constraint bears on it only if the constraint is inside that instance. A constraint outside that instance is checked against the result.

- `param t : Length = auto` written in `Plate` belongs to each `Plate` instance.
- `sub a : Beam { len = auto }` written in `Asm` belongs to the `Asm` instance. `Beam`'s own constraints on `len` are inside that `Asm` instance, so they bear on it.

**"Bears on" follows every chain of reads.** A constraint inside the owner bears on the unknown if its verdict depends on the unknown's value through any chain: through a cell, through an argument into a child, through a child's own solve. It makes no difference how the instance holding the constraint came to exist: a sub, a level of a recursion, a collection element, a constructor call written inline, or one reached through a function.

**The meaning, as far as constraints go.** Each instance chooses its own unknowns from the constraints inside it, given its arguments. Its parent sees only the result, as a function of what the parent passes in. How the solve design computes that is its choice: an inner solve under each outer trial, or one solve that keeps the two sets of constraints apart. The answers must be the same.

**Where this meaning has no answer.** Two siblings, each given the other's result: `sub a : Beam { len = self.b.k }` and `sub b : Beam { len = self.a.k }`, where `k` is `Beam`'s own unknown and a constraint of `Beam` ties it to `len`. Each must be decided before the other. That is a cycle and is reported as one. One solve over both would find an answer. This is a cost of ownership (decision 1).

**Objectives are not settled here.** The spec lets a child with no objective inherit its container's, and lets `minimize cost(self.descendants)` choose a child's `auto` (10.5, 10.8). Both happen today: two examples have a parent's cost objective choose a child's quantity, and three files rely on inheritance (`seats/reliance.md`). Leo expects objectives to differ between a sub and a value. If the solve design lets a container's objective reach a sub's own unknown, that unknown is decided above its owner, and its cell reads the deciding solve through a binding passed down. The kinds allow it. A made or opened instance is given no such binding, because a value has no container.

### 6.2 An `auto` argument is an unknown of the instance that wrote it

`sub a : Beam { len = auto }` gives `Asm` an unknown with no name of its own, passed to `a` as an ordinary argument. Today the compiler makes it a cell in the parent's member list under the child's id, with no expression; it is not passed as an argument. The same form covers a constructor argument, a keyed entry's argument, `at auto`, and a connector's parameter.

- So every unknown is a cell of its owner's own scope, listed in the owner's member table.
- **Such an unknown is under the same guards as the member it was written on.** When the sub is absent, so is the unknown, and a solve leaves it out. When the sub's presence is undetermined, so is the unknown's.
- **A definition's own `auto` parameter is an unknown only where the use does not give it.** With a guarded assignment (`where thick { t = 6mm }`; **amended 2026-11-26:** the form that parses is `t = 6mm where thick`), it is given when `thick` holds and an unknown otherwise. The parameter's cell reads the guard first.

### 6.3 Where `auto` may be written

Two rules:

1. **An `auto` is the whole value of something the source declares:** a parameter's default, a typed `let`, a placement, or an argument of a sub, a keyed entry, a connector, an element of a collection, or a constructor call that is evaluated exactly once. There is one unknown for each instance of that declaration.
2. **An unknown exists whenever its declaration does.** So an `auto` cannot sit where evaluation may skip it (an arm of an `if` or a `match`) or repeat it an undeclared number of times (a lambda, a function body, a quantifier's body). What may not exist is written under `where`. What exists many times is written as a collection, whose count is declared.

| Position | Today | This design |
|---|---|---|
| A `param` default | An unknown | An unknown of each instance, where the use does not give the parameter |
| A typed `let` (`let u : Length = auto`) | An unknown. The untyped form is rejected | The same |
| A sub's argument, either form | A cell the parent owns | An unknown of the declaring instance |
| `at auto` | A placement specification of its own kind | The same rule |
| A connector's parameter | A cell of the definition holding the `connect` | The same rule |
| A keyed entry's argument | Not established | The same rule, one per key |
| A constructor argument in a call evaluated exactly once | A type error for a length; a **silent undefined** for a plain number, with `check --strict` reporting all satisfied (run) | An unknown of the instance whose member it is. `let d = Plate(t: auto)` means `let u : Length = auto` and `let d = Plate(t: u)`. The typed two-line form runs today |
| Inside a lambda or a function body | The same two outcomes (run) | A compile error: write a typed `let` outside and pass it in |
| In an arm of an `if` or a `match`; in a quantifier's body | Not probed | A compile error: put the member under `where`, or use a collection |
| An argument written for a collection's elements | No source form yet (task 5482) | One unknown per element, owned by the declaring instance |
| A type argument (`Bearing<auto: Seal>`) | Chosen by the compiler | Unchanged, outside the core. A choice that fails gives the sub a failure outcome that says so. Task 6815 records that today it can panic a debug build (not checked in code) |

Of 209 uses of `auto` in the tracked `.ri` files, none is in a constructor argument inside an expression (census), so nothing existing changes.

### 6.4 What this design gives the solve design, and what it asks of it

**What is available without evaluating anything.** **Amended 2026-11-26:** this list was written before the ruling on a sub's braces; the PRD's section 5.6, item 5, restates it.

- Every definition's member table. The unknowns a definition declares are in it, with their guards.
- Which members are applied purposes. Their constraints are checked and are never part of a solve's problem (Part 10).

**What is available only through a view.** The instances that exist under a trial, their verdicts, and which answers depended on the trial's values. The member tables cannot give this in two cases: a recursive sub (the tables loop; the depth is a value), and an instance made inside a function body or a lambda.

**What the instance rules require.** Each with the smallest model that showed it.

1. **Inline or wrapped, the same answer.** With `Plate { param t : Length   constraint t >= 3mm }`, `fn mk(t) = Plate(t: t)` and `let u : Length = auto   constraint u >= 1mm   minimize u`: `let d = Plate(t: u)` and `let d = mk(u)` must both give 3 mm. A difference here is of the kind Leo called "confusingly inconsistent". **Amended 2026-11-26:** the same holds when the argument is undefined: a user function is entered whatever its arguments (PRD decision D10(d)).
2. **An inner instance that is feasible for only some arguments.** `Beam { param len : Length   param k : Length = auto   constraint k == 2mm   constraint len + k == 50mm }` under `Asm { sub a : Beam { len = auto } }`. The answer is `len` = 48 mm and `k` = 2 mm. A solve that tries values of `len` and runs `Beam`'s solve under each finds `Beam` infeasible everywhere but one point. The solve design must find the point or report the coupling. It must not return "undetermined" in silence.
3. **A solve reads only the arguments its own constraints and objective read** (Part 4), and never a placement.
4. **An unknown that is absent in the final state is left out,** and does not fail the others in its group.
5. **Unknowns are present "if".** Under a guard, per element of a count, or only where a parameter was not given.

**Three questions this design hands over unanswered.**

- **What makes a trial point infeasible.** Part 15, rule C.
- **What a group does when a constraint that might bear on it is undetermined.** With `use_b` undefined, `sub b : Beam { len = auto }` under `where use_b`, and `constraint x == 2 * self.b.len`: is `x` solved from its other constraints, or undetermined?
- **Recursion.** `Top { let s : Length = auto   sub tree = RecTree(span: s)   minimize s }`, where each level of `RecTree` requires `span >= 10mm` and passes `span / 2` down two levels. Rule 6.1 read plainly gives 40 mm, as three hand-written levels would. The spec says "Resolution does not cross recursive instance boundaries" (8.9); on one critic's reading that gives 1 mm with the tree's constraints violated. The contract leaves "whether a group may cross the boundary of a recursive instance" to the solve design. The second answer makes a recursion of depth three differ from three levels written out.

**What follows for the contract.** Section 8's third planning rule ("groups whose problems depend on each other's unknowns are one group") was written before the ownership ruling. For unknowns of different owners it has to be restated; model 2 is the case to restate it against.

### 6.5 The note on a verdict that depends on someone else's unknown

When a violated or undetermined verdict depends on an unknown that another instance owns, it gets a note. For `constraint c.t > 5mm` over `let c = Plate()`: "`c.t` is decided inside `Plate`, where its `auto` is written. `Asm` affects it only through the arguments it gives. To choose it where the plate is made, write `Plate(t: auto)` there."

- The note names the place in source where the instance was made, which need not be the definition the constraint is in. With `Widget { param p : Plate   constraint p.t > 5mm }` and `Asm { let c = Plate()   sub d = Widget(c) }`, the constraint is `Widget`'s and the call to change is in `Asm`.
- It says what the change costs: an unknown moved to the use is no longer chosen by the definition's own objective unless the solve design says so, and may then have nothing bounding it.
- It is not stored in the value: equal values would then differ by where they came from. It is computed when the verdict is shown, by following the recorded reads back from the verdict.

---

## 7. Structure values

Leo: "Agreed. I'm convinced. Values must stay." He also said "Strongly agreed. Moreover, I think this is the right answer." to one implementation of "what an instance of this definition is" yielding both the sub form and the value form, with the difference confined to role.

**Today.** A constructor call copies the callee's parameter defaults and `let` expressions into the caller. Constraints, unknowns, guarded members, subs, ports and geometry members are dropped. A named argument aimed at an `auto` parameter is stored as a field called `__arg0`. A sub read as a value (`let e = self.a`) is a copy taken before the solve: with `a.len` solved to 50 mm, `e.len` reads undefined (run; a seat found no later refresh in the code).

### 7.1 What a value carries

Its type (the qualified name and type arguments), the value of every parameter that is present, and the value of every unknown decided at or below its instance, each by its relative path. Nothing derived, and nothing about how a value was reached: `Plate(t: 3mm)` and a `Plate()` whose `t` was solved to 3 mm are equal.

- **The type's name, not the definition's meaning.** The language is nominally typed: two definitions with equal text are different types, and a set of values behind one trait must not merge them. It also means an edit to one constraint of `Plate` does not change every `Plate` value. The cost is that renaming a definition changes the content of its values.
- **Everything decided at or below.** A nested instance decides its own unknowns, and a solve need not be deterministic. If a value carried only what its own instance owns, an opened value would decide the nested ones again and could disagree with the instance it came from. Carrying every decided value makes the two agree in every member. The cost: a value is as large as the number of unknowns inside its instance, and a list such as `self.descendants` on a large design is correspondingly large.
- **A solve that fails inside a value** leaves the parameter it was to decide undetermined. The value still exists.

### 7.2 Making one, and reading a member

- **Making.** The cell's body declares an instance at the constructor call, `root/c/made[0]`, with the arguments it has evaluated, and reads that instance's `as-value`. That instance is the real one: it has one path, its unknowns are decided there, and its verdicts are the ones that count.
- **Reading a parameter** is a field read of the value.
- **Reading any other member.** `c.area` compiles to the ref `c / open / area`. The cell `c` builds one instance from its value, with nothing left to decide. There is one per holding cell, at a stable path, shared by every reader, and every result in it is found in the store. One thing can still make it differ from the made instance: a test of how a value was reached (Part 15.2, on `constrained`).
- **A value in mid-expression** (`mk(1mm).area`, `xs[0].area`) is opened by the reading body at that read.
- **A sub read as a value** is its `as-value`, taken after its solves. `let e = self.a` then agrees with `self.a`.

### 7.3 Which instance is shown, selected and counted

- **Its verdicts are counted once,** at the made instance. `let c = Plate(t: 1mm)` read in three places is one violation. An opened value's verdicts are never counted.
- **Shown:** both, at their own paths (Part 9). A cell that holds a value shows the value's members through `open`. A cell that made instances also lists them, with their verdicts.
- A function called from five cells that each make a `Plate` gives five made instances and one run of each body.

### 7.4 How a sub and a value differ

| | A sub | A value | Why |
|---|---|---|---|
| Parameters, unknowns, constraints, guards, its own subs | By the instance kind | The same kind | No difference |
| Contained in and placed by its declaring instance; listed by `self.children` | Yes | No | What holds it |
| Drawn in the viewport, exported, in the bill of materials | Yes, if it is a place and not `aux` | Only where consumed | What holds it |
| An applied purpose | A member of the definition, checked on the instance | The same | No difference |
| An end of a `connect` | Yes, from the instance that declares it | A compile error that says to write a sub | What holds it |
| Named as a purpose's subject by its container | Yes | Refused: a value held in a cell is not a subject (decision 9) | What holds it |
| A gesture on a literal argument, or one that adds an argument | Edits the sub's declaration | Edits the call | No difference |
| A body in braces, whose constraints bear on the instance's own unknowns | Yes | No: a constructor call has arguments only | Ruled (decision 4): the braces customise the instance being created. A value has no syntax for it |
| A container's objective | The solve design's | Never reaches in | Leo expects this one |
| Its path when the source around it is edited | Stable | `made[k]` can move when the cell's expression is edited | A sub has a name; a call has a position |
| Bare-trait arguments checked; type arguments accepted | Yes | Not today (spec 4.9; a parse error) | Existing gaps, not this design's |

**Equality is by content.** For structure values that is already what sets do: two equal values are one member, and so are two distinct subs with equal values (run). `==` on structures and on geometry is rejected by the compiler today. The spec's "identity for structure references" and "same evaluation graph node" (5.2) describe nothing the language does and would be amended. Whether `==` becomes legal on structures is a language question this design does not need answered. Geometry equality by content is the geometry design's to confirm: today a geometry handle's equality includes a reference that names its definition.

**No `connect` in the tracked `.ri` files ends on a `let`-bound value** (0 of 68 endpoints). No `.ri` file activates a purpose at all: activations are in Rust tests and name definitions, with one sub path. Both forms compile today, so sources outside that population could break.

### 7.5 Tool bodies

Leo's ruling of July 2026, in its document's words: a `let`-constructed instance "is a *value*: its geometry members never auto-join the parent's rendered/exported solid set; they realize on demand where consumed."

Here a geometry member is a member like any other. `tool.body` is the description at `root/tool/open/body`, and `difference(plate, tool.body)` takes it by content. Drawing, export, mass and the bill of materials enumerate places, so the tool's body is neither drawn nor exported unless asked for. The experience the ruling protects follows from what holds the value. Today `tool.body` does not exist on a value and the form is a compile error.

### 7.6 A definition's constraints become checked on every instance

Today they are checked once, at the definition's defaults. A seat measured what changes, by building a copy of each definition with each site's arguments as its defaults and running `check` on the old binary.

| | Sites | Satisfied | Violated | Undetermined | Not evaluated |
|---|---|---|---|---|---|
| Subs | 225 | 199 | 5 | 6 | 15 |
| Values | 242 | 214 | 1 | 4 | 23 |

Population: every place in the tracked `.ri` files that instantiates a definition with a constraint of its own or inherited through a trait.

- **Among the sites that could be evaluated, no real design and no example gains a violated verdict.** That is 126 sites in real designs and 153 in examples; 8 and 16 more could not be evaluated.
- **Newly violated: 2 sites**, both test fixtures. One was written to show this defect; the other already fails through a constraint of its own and gains one more line. Two further sites come out violated only because the old binary leaves their subject undefined.
- **Newly undetermined: 4 sites**, all standard-library stub results (`ElasticResult()`, `BucklingResult()`). Task 7874 names these stubs and says their bodies are rewritten to omit nothing. Whether the stubs are evaluated at all on main was not established.
- The other non-satisfied sites repeat a verdict their definition already prints.
- **38 sites were not evaluated:** 8 in the printer design, which crashes the old binary; 5 generic; 1 that passes `auto`; 13 whose argument values could not be read back; 11 in files that fail to check today.
- **In sources embedded in Rust tests,** the seat found about 254 such sites and probed 204: 160 satisfied, none violated, 44 that do not compile.
- **Parameters left unset.** Of all 1,254 instance sites, 37 leave a parameter with no default unsupplied; 9 of those have a constraint that reads it. Task 7874 makes each omission a compile error except for four standard-library parameters it gives defaults to. Which of the 37 those are was not established.

Every `check` of a file with constrained instances will print more lines, one verdict per instance.

---

## 8. Paths

**Today.** Ids are pairs of strings, printed as `Entity.member`. That print is not one-to-one: `Asm.thin.t` is refused as ambiguous, because the dots could split two ways. Instance paths are built with `format!` at 8 or more sites and id strings of all kinds are split by hand at 16 or more sites in Rust, and in 5 frontend files. No typed instance path is in use.

**The design.**

- **An id carries a typed path:** a root (a module and a definition, with a separator between them that a module's own dots cannot be mistaken for), then steps.
- **The text form spells every step and must be one-to-one.** `Asm.thin.t`, `Asm.vents[2].area`, `Asm.vents["intake"].area`, `Asm.c/made[0].t`, `Asm.c/open.area`. One module owns it, both ways, and no other code splits an id string. The kinds of step and the text for each are Part 15, rule F.
- **A label is not an id.** A person sees `Asm.c` for the made instance and the constraint's own text for a verdict. A tool gets the id, and the label beside it.

Tasks 8704 (a typed instance-path grammar) and 7717 (structured ids across the GUI boundary) already plan parts of this.

---

## 9. What the GUI and the agent tools show

Leo's primary use, in his words: "the GUI, both for interactive use and as a sort of REPL for agentic use."

**Today.** The tree has every definition as a root. Property rows are built from definition cells, so a selected instance shows the definition's defaults. The write-back addresses a definition's name and a parameter's name; it cannot address a sub's argument, and the parsed source keeps no position at which to add one. Of the agent tools, the one that sets a parameter takes `Entity.member` only.

**The design.**

- **The tree is rooted at the viewed root.** Under an instance: its parameters, lets, constraints with their verdicts, subs, and collections that expand to elements. A member that is absent is listed greyed, with the guard that removed it; it has no value. One whose presence is undetermined is listed as such and is not drawn.
- **A cell that holds a structure value expands to that value's members**, marked as a value, its body not drawn by default. **A cell that made instances also lists them**, each with its verdicts. Where a cell made exactly one instance and holds its value, the two are shown as one.
- **Property rows are the selected instance's own values**, read at its path.
- **A parameter row has a direct control unless what is written for that parameter, where the instance is declared, is a formula** (Leo's proposal of 2026-11-26, as restated and agreed). A literal is edited in place; `auto` and `undef` are literals reached by a toggle; where nothing is written, the control adds an argument. A control never destroys an expression somebody wrote.

  | Written at the use | The row shows | Control |
  |---|---|---|
  | A literal | Slider and value box | Live |
  | `auto` | The decided value, read-only, and which instance decides it | Toggle |
  | `undef` | Empty | Toggle |
  | Nothing, and the parameter has a default | The default's value, read-only, and whether it is a literal, a formula or `auto` | Toggle |
  | A formula | The value and the formula, read-only | None; edit it in source |

  The toggle cycles value, `auto`, `undef`, default. Switching to "value" starts from the value shown. In the "default" state the slider is live when the definition's default is a literal, so a drag adds an argument, as the contract's Example 8 says; when the default is a formula or `auto` the box is read-only and the toggle is the way to "value". The fourth state is called "default", not "absent", which in this design means a member removed by a false guard. A parameter with no default has no "default" state, since omitting a required argument is a compile error.
  - A formula *default* can be overridden at a use: from "default" the toggle adds an argument and leaves the definition's formula alone. A formula written as the *argument* has no toggle, because any other state would delete it.
  - `auto` at the use and `auto` in the definition are different decisions with different owners (Part 6). The row names who decides.
  - **A literal in a constructor call is editable** (Leo: "I don't see why this should be refused"; agreed 2026-11-26). A literal has one place in source; editing it moves every instance that text makes, and the preview and the tool's answer say which.
  - The control sits on the row where the literal is written. A row that merely holds a value made elsewhere (`d.p.t`, with `sub d = Widget(c)`) is a read-only box that links to where it was made.
  - **At a root** there is no argument. For a parameter with a default the row edits the definition's default and says what else moves. **A root's required parameter has no control**: Leo ruled on 2026-11-26 that such a value may not live in the session ("The source must remain the canonical sole definition of the design. If the source can't represent it we either improve the expressive power of the source ... or we refuse it"), and writing a default would make the parameter optional at every use. The source can already say it with a definition that uses the part: `structure def BoltM8 { sub bolt : Bolt { d = 8mm } }`.
  - The control per type (a number with a unit, a Bool, an enum, an Int) and where a slider's range comes from are for the GUI's PRD.
- **Before an edit, a tool can ask what it would move:** the instances whose values would change, as paths. When the declaring definition is used several times, an edit moves every use, and no edit moves only one of them. The preview shows that to a person; the tool's answer says it to an agent.
- **Selection is a path.** A click on a body selects its instance. A click on a verdict selects the instance that owns it.
- **Verdicts are listed per instance.** "All satisfied" appears only when the whole of the viewed root has been checked (contract 4.1), and the status line says what the other roots show (Part 3).
- **The agent tools use the same paths**, and "set this parameter" makes the same edit as the gesture, with the same refusals. A tool that explains a verdict returns the measured values, the note of 6.5, and where in source a change would take effect.

---

## 10. Purposes and connections

**Today.** A purpose is activated by name, with a string for each subject, from one command (`check --purpose`). No `.ri` file can say that a purpose applies. A purpose can be active once: a second activation on another subject is dropped with no message (run). The subject's kind is not checked; a kind that does not exist compiles. A purpose's `minimize` is stored and never read. Of 42 purposes in the tracked `.ri` files, 37 have one subject, 1 has two and 4 have none; 6 hold an objective; none has a constraint or a field as subject.

**Ruled by Leo, 2026-11-26.** Three rulings, which replace the purpose half of decision 4:

- **A purpose only checks.** "the purpose is a predicate that checks that a design is fit for a particular purpose." Its constraints never take part in choosing an unknown. This reverses his ruling of the day before for purposes; the ruling for a sub's braces stands.
- **An objective inside a purpose is a compile error** that says to write it in the definition.
- **A purpose is declared, then applied in source** ("a declare then apply pattern (similar to the declare then instantiate for structures), so that the source can express application"). The exact syntax is for the PRD; the model is `constraint def Name { ... }` applied by `constraint Name(args)`, which the language already has.

**The design that follows.**

- **An applied purpose is a member of the definition it is written in.** The instance declares one purpose instance for it, from its member table, with each of the purpose's parameters bound to a place: the instance itself, or an instance it contains. `subject.t` is a ref through that binding.
- **Its constraints are verdicts and nothing else.** They are counted in the summary of the instance that applies the purpose. A solve's problem never includes them, although they are inside that instance by path. The member table marks them, so whatever forms a solve can leave them out without evaluating anything.
- **A requirement that should shape the design is a constraint, not a purpose.** It is written where the unknown is owned: in the definition, in the braces where a sub is declared, or in the container after moving the `auto` up to it. A `constraint def` gives it a name to reuse, and a purpose can call the same definition to check it (**amended 2026-11-26:** accepted in a purpose's body once the PRD's purposes leaf lands).
- **A purpose may be applied any number of times.** Each application is a line.
- **The session's toggle is an uncommitted source edit.** Switching a purpose on in the GUI, or `--purpose` on the command line, is the uncommitted form of adding the line that applies it, as a slider is of editing a literal. So `check file.ri` runs the purposes the source applies, with no flag, and which purposes are active is no longer a setting.
- **A subject that is not there.** The purpose's constraints then read something that is not there and are undetermined, shown with the reason (Part 4).
- **A subject is the applying instance or an instance it contains.** A value held in a cell is refused (decision 9). A subject that is a constraint or a field is refused for now (Part 15.2).
- **Nothing about objectives is left here.** How the default objective, which the spec calls "a default purpose" (10.7), reaches an unknown is the solve design's.
- **A tolerance requirement** arrives today through a purpose (`RepresentationWithin(subject, 1mm)`). Whether that drives the tolerance a shape is made at, or only checks it, is the geometry design's question.

**`connect` needs no kind of its own.** It compiles to ordinary members of the declaring instance: a compatibility constraint, a connector sub when a type is named, alignment constraints, and a record in the member table. An end must be a port of the declaring instance itself or of one of its subs or elements. The spec's "owned by the nearest common ancestor" equals the declaring instance for ends on different subs or on the instance's own ports.

---

## 11. What the compiler must emit

Today every compiled reference is a string containing the definition's name, a sub's hash is the hash of its source text, a call's hash contains the callee's name and its arguments but not its body, and a constructor call copies the callee's members into the caller.

In the order they would have to land, as far as a seat could tell:

1. **A hash of its own for what has none today:** a geometry operation list, an objective, an `auto` placement, ports and connections.
2. **A sub's hash built from its compiled parts**, not its text.
3. **A member table per definition**, without bodies: each member's name, kind, type and units of source; its guards; for a sub or a constructor call, the definition named and each argument with its guard; a collection's count cell and keys; the unknowns with no name of their own; which units of source may make instances.
4. **A rule for what a sub's braces may hold** and whose constraints they are (decision 4), before those members are compiled.
5. **A call's hash that folds in the callee's body**, with a rule for recursion and a version for built-in code.
6. **References local to the unit of source:** the instance itself, the scope that wrote an argument, or a binder, then steps. Never a definition's name plus a member.
7. **A constructor call compiled to "make an instance of this definition, with these arguments, at this site."** A member read whose receiver is a cell or a sub compiled to a ref.
8. **Positions in a side table**, keyed by unit of source and place in its tree, with a position for each argument and for where a new one would go. The byte position inside a structure value goes.

**How to land it.** The compiler emits this as a second output beside the one the old engine reads, so the old engine is not edited. That costs two outputs until the old engine is retired.

---

## 12. The contract's examples, worked

A critic traced each of these against draft 3's kinds and found them to hold.

**Example 3 (a guard).** `root/thin` is an instance declared under the guard `use_plate`. It reads `use_plate`, then `Plate`'s member table. Set `use_plate` false: `root/thin` is absent, and so is `root/thin/t`. Set it true: everything is found. `big` and its constraint read the guard cell for `n > 1`. Change `n` from 2 to 3: the guard cell runs and is still true, so nothing that read only the guard runs. `Plate`'s `constraint t >= 3mm` is a constraint under `root/thin`, reading this instance's `t`.

**Example 4 (a solved count).** Ask for `root/a0`. It reads `root/vents/[0]/area`. Resolving that reads the collection `root/vents`, which reads its count cell, which reads `root/n`. `n` is an unknown of `root`, so its cell reads the solve that decides it. That solve returns 4, the collection declares four elements, and `a0` is 10 mm. Whatever forms the solve must not read `root/vents` outside a view, or it would read the count, then `n`, then itself; `root`'s member table tells it the vents are "one per element of this count" without evaluating anything. With the stronger constraint over the vents, the trial `n := 3` gives a collection of three, read through the view.

**Example 5 (two coupled siblings).** `len = auto` on `a` and on `b` are two unknowns of `root`. Six constraints bear on them: two of `Asm`'s, two of `Beam`'s, two of `Rod`'s, all inside `root`. The solve tries values for the two and returns 70 mm and 50 mm. `root/a/len` reads its argument, which is `root`'s unknown. `Beam`'s bounds are verdicts under `root/a`, checked at 70 mm. If `auto` is written in `Beam` itself, each beam owns its `len` and decides it from its own bounds; `Asm`'s two constraints are then checked against the results and carry the note of 6.5.

**Example 8 (a slider and its commit).** Dragging `b`'s width edits the literal in `sub b : Peg { w = 20mm }`. That argument is the source of the cell `root/b/w`, which runs again, and everything displayed that reads it follows. If `b` has no argument, the drag adds one: `Pair`'s member table changes, `root` runs again and declares `b` with an argument, and the cell `root/b/w`, which had been reading its default, now reads the argument and runs. Later ticks change only the argument. `Peg`'s default is untouched, so `a` does not move. Release writes the same text. This holds given that an instance passes on a binding for every argument it could be given.

**Example 9 (a purpose).** Applying `thin_enough` to a bracket is one line in the source of the definition that holds the bracket, or the session's uncommitted edit that adds that line. That definition's member table gains a member. Its instance runs again and declares one more child, the purpose instance, with `subject` bound to the bracket. The purpose's constraint is an ordinary verdict that reads `subject.t`. No solve reads it. The instance's other children are untouched, because their declarations did not change. (The contract's Example 9 has activation as a setting attached to a path and lets a purpose take part in solving; both change with Leo's rulings of 2026-11-26.)

**Example 10 (one literal).** Editing `Peg`'s default for `w` changes one unit of source. Every `w` cell that reads the default runs; the ones given an argument never read it and do not.

**Example 11 (structure values).** `let c = Plate()`: the cell `root/c` makes `root/c/made[0]`, whose `t` is its own unknown, decided inside as 3 mm; the cell's value is `Plate{t: 3 mm}`. `constraint c.t > 5mm` reads the field: violated, with the note. `let d = Plate(t: auto)`: an unknown of `root`; `Plate`'s constraints at `root/d/made[0]` are inside `root` and depend on it, so they bear on it. `sub s : Plate` gives the same answers as `c`, through the same kinds. With `sub x = Widget(c)`, `sub y = Fidget(c)` and `let a = c.area`: one made instance, its verdicts counted once, and three opened values that all find stored results.

**An argument on trial.** `let c = Slot(width: w)`, `w` an unknown of `root`. Under the trial `w := x`, `root/c` makes its instance with the value `x`. `Slot`'s own solve reads that argument, opens its view on `depth`, and returns. It follows the outer solve. A critic traced the same answer in either order of request; that rests on how the core PRD resolves its open point on a view inside a view.

---

## 13. The defects that disappear, and what each rests on

| Defect today | What removes it | What that rests on |
|---|---|---|
| A definition's constraints are not checked on its instances (tasks 5765, 6609, 6926) | A constraint is a member, and an instance declares its members | For a sub, nothing more. For a value, also rules A and B of Part 15 |
| A child's own `auto` is never decided per instance (9604) | An unknown is a cell of its owner; each instance has its own | The solve kinds, which are not designed here |
| A guarded sub is never created (8414) | Presence is an instance's first read | Nothing more |
| A count that reads a sibling sub gives no elements (6604); a solved count gives none | The collection reads the count, which reads whatever it needs | For a solved count, a solver that tries integer values, which no kept solver does |
| A guard inside a child is decided by the definition's defaults (7992) | A guard is a cell of the instance | A parent reading the child's guarded member rests on the rule for reading what is not there |
| A sub read as a value is stale after the solve (6340) | The value is read from the instance, after its solves | Nothing more |
| A recursive sub inside an instance is not elaborated (10499) | Recursion is a guarded sub | Nothing more |
| The GUI cannot address an instance's parameter (7717) and shows the definition's defaults for it | Rows are read at the instance's path, and a path's text is one-to-one | Rule F of Part 15, and positions for arguments in the parsed source |
| A warm edit evaluates instance cells in the definition's scope (9818, 10081) | There is no definition-level cell, and no second routine | The core's guarantee that an incremental answer equals a fresh one, and compiler items 3, 6 and 7 |
| A `let`, a constraint or a `where` in a sub's braces is dropped | They are compiled as members of the instance | Name resolution in the braces (Part 15.2), and the compiler's rule (item 4) |

**Against the held defect records.** The held-task review sent 81 open defect records, one testable sentence each. A critic checked **draft 1** against them, reading 12 task records and judging 69 from their sentences: 16 removed by a rule, 14 partly, 8 for the solve design, 17 for the geometry design, 22 for the core or the session layer, 4 not addressed. The check was not run again. Of the four not addressed, both pairs now have a rule: a type argument that fails to resolve (6.3), and an argument that evaluates to undefined (Part 2).

---

## 14. Decisions for Leo

**1 to 6 are real choices.** 7 to 11 follow from them or from the spec and need a yes or a no. 12 is a list of smaller rules to veto. Each model is complete as written. Where a model gives a solved value, it assumes the default objective picks the middle of the feasible range, as it does today.

### 1. Who owns an `auto`

**RULED by Leo, 2026-11-25: "LGTM"** to the recommendation, put to him with the question whether his 25 November ruling answers task 9374 for constraints.

**Model.**

```
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm  { sub a : Beam { len = auto } }
structure def TopS { sub asm : Asm      constraint self.asm.a.len == 70mm }
structure def TopV { let asm = Asm()    constraint asm.a.len == 70mm }
```

In `TopV`, `asm` is a value. Leo's ruling that a value is closed means nothing outside `Asm` chooses its unknowns: `Asm` decides `a.len` alone, 55 mm, and the constraint is violated.

**Recommended.** `TopS` gives the same, 55 mm and violated. The `auto` is in `Asm`'s text, so it belongs to the `Asm` instance, and `TopS`'s constraint is outside that instance. (`Beam`'s bounds are in `Beam`'s text but inside the `Asm` instance, which is why they bear.) This is the only reading under which a sub and a `let` handle `auto` and constraints the same way. It changes the contract's wording "wherever that constraint lives" to "wherever inside the creator", so I am asking, not assuming. (Neither path compiles on the old binary, so this is a model of the rule and not of a measured change.)

**An earlier ruling it touches.** Task 9374 is a pending decision gate. It records Leo's ruling of 18 October on exactly this shape: a parent's constraint on a child's own `auto` is "Undrivable now, drivable later via whole-model-objective-coupling". I read the ruling of 25 November as answering the gate for constraints: a constraint outside the owning instance is checked and never chooses. "Via an objective" stays open, with the solve design. If that is right, task 9374 can close on it.

**Cost.**

- To choose `a.len` from `TopS`, `Asm` passes it through: `structure def Asm { param a_len : Length = auto   sub a : Beam { len = a_len } }`, and `TopS` writes `sub asm : Asm { a_len = auto }`.
- A child that is feasible for only some arguments makes the parent's solve a search for where (6.4, model 2).
- Two instances that each need the other's own result have no answer, where one solve over both would find one. Model: `Beam { param len : Length   param k : Length = auto   constraint k == 100mm - len / 2 }` and `Asm { sub a : Beam { len = self.b.k }   sub b : Beam { len = self.a.k } }`. Under ownership: a cycle, reported. One solve over both: 66.7 mm each.
- **Existing sources.** Of 55 places in the tracked `.ri` files where `auto` is written as an argument, none is read by a constraint or an objective above the definition that wrote it. Two test fixtures have a parent's constraint on a child's own `auto`; today both give undefined and an undetermined verdict, and under this rule the child decides and the parent's constraint is checked. Sources embedded in Rust tests were not counted.

**Alternative.** An `auto` written on a sub is open to every ancestor whose constraints read it. Then `TopS` gives 70 mm and `TopV` 55 mm.

### 2. Roots

**RULED by Leo, 2026-11-25: "LGTM".**

**Recommended.** By default the roots are the definitions of the entry file that nothing else in that file uses; any definition can be named instead; `build -o` with several roots and none named fails and lists them.

**Model.**

```
structure def Bolt    { param d : Length = 6mm   constraint d >= 3mm }
structure def Bracket { sub b : Bolt { d = 8mm } }
```

Recommended: one root, `Bracket`, and one verdict, at `Bracket.b`, checked at 8 mm (given decision 7). Today: two roots and one verdict, for a `Bolt` at its own 6 mm that nothing uses; the bolt in the bracket is not checked at all. Alone in a file, `Bolt` is the root in both.

**Cost.** Part 3. Adding one `sub` line that uses a definition removes that definition's own root and verdicts. How many definitions stop being roots was not counted under this rule; under a nearby one, 314 of 783 definitions in files with several are used by a sibling.

**Alternatives.** Every definition, as today: both roots above, with `Bolt` checked twice. Or check every definition and draw only the default roots: the same two verdicts, one body drawn. Or a marker in source.

**Made in passing, say if wrong:** imported definitions are never roots; a `@test` definition is not a default root and is never drawn or exported by default; the GUI views the first root until the user chooses.

### 3. What is not there

**RULED by Leo, 2026-11-25: "LGTM"** to the recommendation as put to him: absent, not undefined; reading what is not there gives undefined; presence undetermined; the `present(...)` addition; an index out of range gives undefined.

**Model for all three.**

```
structure def Fan  { param d : Length = 40mm }
structure def Case { param needs_fan : Bool = false   sub fan : Fan where needs_fan }
structure def Top  { sub c : Case }
```

- **3a. A member under a false guard is absent, not an undefined member.** Recommended: `Case` has no `fan`; the GUI lists it greyed and `self.children` does not include it. Today a guarded `let` is a member holding undefined, and a guarded sub is never created whatever its guard.
- **3b. Reading something that is not there gives undefined.** Under the recommendation:
  1. In `Case`: `let f = self.fan.d` is a compile error, as the spec states. Today a warning. **Amended 2026-11-26:** by Leo's later ruling it compiles, and `f` is undefined where there is no fan (PRD, the end of decision D13).
  2. In `Case`: `let f = if needs_fan then self.fan.d else 0mm` is 0 mm.
  3. In `Top`: `let f = self.c.fan.d` compiles. `f` is undefined, and shown with the reason: "`c.fan` is absent because `needs_fan` is false".
  4. In `Top`: `constraint not self.c.needs_fan or self.c.fan.d > 30mm` is satisfied, whichever way round it is written.
  5. `vents[7].area` on a collection of three is undefined, shown with "index 7 of `vents`, which has 3". This departs from the spec's 3.4, which says failure.

  *Alternative A:* a failure at the reader, which no expression can catch. Lines 3 and 4 fail. It is louder, and nothing can then test for presence.
  *Alternative B:* the spec's rule applied to every read inside a definition. Line 2 becomes a compile error too. The spec says nothing I can find about a read across an instance.

  *With it, a language addition:* `present(self.c.fan)`, a test of presence alone. `determined(self.c.fan.d)` also answers false for a fan that is there with `d` undefined for another reason, so using it as the test hides that case.

  *Existing sources:* the tracked `.ri` files have 4 guarded subs, 10 guarded blocks and 3 other guarded declarations, in 15 files, none in a real design or the standard library. A search by name found no read of a named guarded member anywhere. It did not cover `where` blocks inside purposes or sources in Rust tests.
- **3c. An undefined guard gives "presence undetermined".** *Model:* `structure def Case { param cooled : Bool   sub fan : Fan where cooled }`, viewed as a root, so `cooled` is undefined. Recommended: the fan's presence is undetermined, `check` says so, and `check --strict` fails. *Alternative:* treat it as false, which hides the fan and its constraints. Today that is what a recursive sub's guard does; a guarded sub that is not recursive is never created at all.

### 4. What may add a constraint that bears on an instance's own `auto`

**The purpose half of this ruling was REVERSED by Leo on 2026-11-26: a purpose only checks** (Part 10). In the models below the purpose gives 1 mm and a violated verdict. The braces half stands.

**RULED by Leo, 2026-11-25: "Bears for both the braces and a purpose."** His reason: "this, Because this is what aligns best with the purpose of the braces on a sub: customizing the instance that is being created." So in the models below the braces give 17.5 mm and the purpose gives 3 mm. This is not what the draft recommended (the braces only check).

**What a conflict is.** He added: "A conflicting constraint is an error, unsatisfiable, unless that conflicts with some other design decision we've already made?" It does not conflict. Where the constraint bears on an `auto`, as with `sub motor : Motor { constraint shaft_d > 25mm }` against `Motor`'s own `shaft_d <= 20mm`, the instance's solve fails as unsatisfiable; by the contract's section 8 a failed solve is a result, the unknown is undetermined with that cause, and nothing falls back to an earlier or default value. Where nothing is being chosen, as with `{ shaft_d = 12mm   constraint shaft_d > 15mm }`, there is no solve and the constraint is violated. Both fail `check`.

As put to Leo on 2026-11-25: two things attach a constraint to an instance from outside its definition, the braces after a sub and a purpose made active on it. (Since 2026-11-26 a purpose is applied in source and only checks.)

**Model for the braces.**

```
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Rig   { sub motor : Motor { constraint shaft_d > 15mm } }
```

**Model for a purpose.**

```
structure def Bracket { param t : Length = auto   constraint t >= 1mm   constraint t <= 10mm   minimize t }
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
```

with `robust` made active on a `Bracket`.

| | The braces | The purpose |
|---|---|---|
| **Only checks** | `Motor` decides alone: 12 mm. The constraint is violated, with the note of 6.5. To get more than 15 mm, `Rig` writes `{ shaft_d = auto   constraint shaft_d > 15mm }`, which gives 17.5 mm | `Bracket` decides alone: 1 mm. The purpose is violated, with the note |
| **Bears** | 17.5 mm | 3 mm |

- **"Only checks" for both** is the plainest reading of Leo's "let and sub should behave the same way (in how they handle auto and constraints)" and of the contract's text that such an `auto` is decided "by the definition's own constraints". A purpose could then affect a solve only through its objective, if the solve design allows that.
- **"Bears" for both** is what the spec's sentence that a sub's body "Can set parameters and add constraints on the instance" (8.7) suggests, and what the contract's Example 9 expects of a purpose. Then `sub motor : Motor { constraint shaft_d > 15mm }` and `sub motor : Motor` with `constraint self.motor.shaft_d > 15mm` beside it give different solved values.
- **Recommended, held less firmly than the others: the braces only check, and a purpose bears.** The reason to give for the difference: under this reading a constraint in the braces is a member of the declaring instance, `Rig`, so it is outside the `Motor` instance, and decision 1 already says a constraint outside an instance only checks. A purpose is a setting that someone switches on for this very instance in order to make the design fit it.

**Either way:** the verdict is listed at the sub's path. Under "only checks" the constraint is a member of `Rig`; under "bears" it is a member of this `Motor`.

**This depends on decision 1**, and on which scope a name in the braces resolves in, which Leo ruled on 2026-11-26 (Part 4): the model's bare `shaft_d` names the child's parameter. An objective in the braces (the spec's `sub bracket : Bracket { minimize mass }`) goes to the solve design.

**Existing sources:** two fixtures have a constraint in a sub's braces, dropped today. No `.ri` file activates a purpose.

### 5. `auto` in a constructor argument, and for a collection's elements

**RULED by Leo, 2026-11-25: "LGTM".**

**In a constructor argument.**

```
structure def Plate { param t : Length   constraint t >= 3mm }
structure def Asm   { let d = Plate(t: auto)   constraint d.t <= 8mm }
```

*Recommended:* allowed where the call is evaluated exactly once. `Asm` gets one unknown; `Plate`'s constraint and `Asm`'s both bear on it; 5.5 mm. Elsewhere (under a lambda, in a function body, in an arm of an `if`) it is a compile error. *Alternative:* never allowed there, and the user writes `let u : Length = auto` and `let d = Plate(t: u)`, which gives the same and runs today. Either way today's outcome goes: a type error for a length, and for a plain number a silent undefined. Spec 2.9 changes. None of the 209 uses of `auto` in the tracked `.ri` files is in this position.

**For a collection's elements.** *Recommended:* one unknown per element. *Model,* in words, because no source form exists until indexed subs (task 5482) land: two vents each given `area = auto`, with a constraint that the first is twice the second and that they sum to 9 mm². Per element: 6 and 3. One shared unknown: infeasible. A shared unknown stays writable under this rule (a typed `let` outside, passed to each), and the reverse is not true. This reads "each written `auto` is one decision" as one decision per element, a reading of a phrase Leo said "Yes" to. It can wait for task 5482.

### 6. What a value carries

**RULED by Leo, 2026-11-25: "LGTM".**

```
structure def Plate { param t : Length = auto   constraint t >= 3mm   minimize t }
structure def A { param v : Length = 1mm }
structure def B { param v : Length = 1mm }
```

*Recommended:* a value carries its type's name, its parameters, and every value decided at or below it. So `Plate(t: 3mm)` equals a `Plate()` whose `t` was solved to 3 mm, and `A()` does not equal `B()`. *Cost:* renaming a definition changes the content of its values and re-runs what holds them; a value is as large as the number of unknowns inside it. *Alternative:* the definition's meaning in place of its name, as the contract now says; then `A()` equals `B()`, and editing a constraint of `Plate` changes every `Plate` value. Existing sources were not counted; sets already merge equal values today.

### 7. Constraints checked on every instance

**RULED by Leo, 2026-11-26: "yes, all instances".**

```
structure def Bar { param length : Length = 10mm   constraint length > 0mm }
structure def Asm { sub b : Bar { length = 0mm - 5mm } }
```

*Confirm.* Today: "All constraints satisfied", because `Bar` is checked once at 10 mm. Under the design: violated at `Asm.b`. A seat measured 467 sites in the tracked `.ri` files that instantiate a definition with a constraint. Among those it could evaluate, no real design and no example gains a violated verdict; two test fixtures do; four standard-library stub results become undetermined, which task 7874 already covers. 38 sites could not be evaluated, 8 of them in the printer design. Results returned by the solvers will be checked against their definitions' constraints for the first time. For values this rests on rules A and B of Part 15.

### 8. The GUI

**Leo, 2026-11-26: "LGTM, except" two points, under discussion.** (1) On a literal in a constructor call: "I don't see why this should be refused. Explain & discuss". (2) On a value from a formula or a solve: "I don't think a value set by a formula gets a slider, just a read only value box. Proposal: only scalar literals get a direct control. auto is a literal, as is any explicit constant, or undef. The control becomes a value slider, value box, and a cyclic toggle (auto/undef for values with no default, auto/undef/absent for values with a default)." The gesture table of rule G is to be rewritten once this is settled.

**Settled 2026-11-26.** (1) Agreed: a literal in a constructor call is editable, and the contract's example drops that refusal. (2) The control model of Part 9 is agreed (his points "1-3 LGTM"). He rejected a root's value for a required parameter living in the session: "No. This adds a semantically critical value that lives outside the source." Part 9 has the result.

```
structure def Peg  { param w : Length = 10mm }
structure def Pair { sub a : Peg { w = 10mm }   sub b : Peg   let c = Peg(w: 30mm) }
```

*Confirm* Part 9. Viewing `Pair`: the tree shows `a`, `b` and the cell `c`; selecting `b` shows `w = 10 mm, from Peg's default`. Dragging `a.w` edits the literal in `Pair`. Dragging `b.w` adds `{ w = ... }` to `sub b`, as the contract's Example 8 says. Three things go beyond that example:

- dragging `c`'s `w` edits the literal in the constructor call, which the example refuses;
- viewing `Peg` itself as a root, dragging `w` edits `Peg`'s default, though a root has no place of declaration;
- where a value comes from a formula or a solve, a drag is refused and a separate action, "set a value here", adds an argument that overrides it.

This depends on decision 2 (the viewed root).

### 9. A `connect` end and a purpose's subject must be places

**RULED by Leo, 2026-11-26: "Agreed".**

```
structure def Asm { let s = Shaft()   let h = Hub()   connect s.end -> h.bore }
```

*Confirm.* This compiles today and would be a compile error that says to write subs. None of the 68 `connect` endpoints in the tracked `.ri` files is on a value. A purpose made active on a `let`-bound value gives an undetermined verdict today; a value held in a cell cannot be named as a subject.

### 10. Collections

**RULED by Leo, 2026-11-26: "LGTM."**

*Confirm* two rules. An element's identity is its position or its key; that is what it is today, so nothing changes. A count may not read its own elements: with `sub bolts : List<Bolt>`, `constraint bolts.count == ceil(load / bolts[0].rated_load)` is a compile error that says to write `let proto = Bolt()` and read `proto.rated_load`. Today it gives undefined silently. All 21 count constraints in the tracked `.ri` files use a literal.

### 11. Values where the language has identity or paths today

**RULED by Leo, 2026-11-26: "LGTM."**

*Confirm.* Equality of structure values is by content, and the spec's two sentences on identity (5.2) are amended; a set of two equal values has one member today and still would. `self.children`, `self.members` and `self.descendants` give the values of present members, where today they give path strings and include absent subs; 16 lines in 8 tracked files use them. What an objective over `self.descendants` reaches, `cost(self.descendants)` included, stays with the solve design.

### 12. Smaller rules the design states: say if any is wrong

**Leo, 2026-11-26: "LGTM, except" three, under discussion:** why both operands of `and`, `or` and `implies` are read; whether a `forall` should give one verdict per element ("In my understanding of normal use 'For all x in X, P(x)' evaluates to a bool not |X| bools"); and what the built-in verdict for an undefined argument means in practice. The other ten stand.

**Settled 2026-11-26.** Reading both operands: "confirmed - mostly because it's less surprising for people without a code/CS logic background". `forall`: "Your rec LGTM": both forms stay as the spec has them (the expression form is one Bool and one verdict; the statement form declares one constraint per element). An argument that is undefined because it read something not there, or from an operation with no value: "Agreed", undetermined. His ruling recorded in task 9345 stands for the rest: no error for a root's parameter with no value or an explicit `undef`. **`check` and undetermined verdicts (ruled 2026-11-26: "12.3 Agreed", and of failing by default, "I think it should").** `check` fails by default when any verdict is undetermined. A flag allows a design that is deliberately partial: undetermined only because a root's parameter has no value or an argument is an explicit `undef`. Undetermined for any other reason (a read of something not there, a divide by zero, a failed solve) fails with or without the flag. It takes effect at the switch to the new engine and governs `build` and what the agent tools report as success. The session layer owns the rule (task 10551). What this design owes it: a summary reports each undetermined verdict so that the two kinds can be told apart, by following the refs to its undefined inputs.

- **Only an `if` or a `match` selects what is read** (confirmed). Both operands of `and`, `or` and `implies`, and every element of a quantifier, are read, as the spec's 9.2.3 implies. So a ring of dependencies through an operand that could not affect the answer is a cycle. Today the evaluator skips a right operand once the left decides, and such a model gets an answer.
- A guard may read a solved value.
- More than one declaration of a name present at once is a failure.
- An argument that depends on what it helps decide is a cycle (Part 4).
- An undefined count makes a collection's elements undetermined.
- A `forall` block that declares constraints gives one verdict per element.
- A failure (a cycle, a limit, a kernel error) cannot be caught by an expression.
- A verdict holds no measured value.
- An undefined argument is reported once, by a built-in verdict of the instance it was given to.
- `tool.body` on a value becomes legal.
- A purpose can be applied any number of times; each application is a line in source.
- A file with no root: `check` says so and lists its definitions.
- A path has one text form, spelled in full for tools, with a shorter label for people.

---

## 15. Not settled

### 15.1 Seven rules to settle by building

**Leo, 2026-11-25: "LGTM"** to settling these by building them under the critics' models.

Each of these is a property over every expression or every file. Each of the three rounds found new counter-examples in them, in the fixes of the round before. They are given here in their current form. They belong in the PRD's behavioural corpus with the models from all three rounds, and their wording should follow the tests.

**A. How the expression evaluator reads.** **Built 2026-11-27** by leaf ζ₂ (#10641), as the PRD's section 5.5 records. The entries that decide it, `evaluator-reads/cycle-through-and-operand-cannot-be-caught`, `evaluator-reads/ring-through-unneeded-operand-is-cycle` and `evaluator-reads/quantifier-later-element-read-is-cycle`, first run at leaf ε₂ (#10645), which confirms this text.

- "Not here yet" is an answer of the reader, not a value. The evaluator's entry `eval_expr_or_pending` returns a value or pending, and an evaluation is pending when any read it made was answered "not here yet".
- The walk reads on past a pending read, so a node whose operand is pending is pending after its other operands are evaluated. No built-in is run, no function entered and no function applied with a pending operand.
- Only the condition of an `if` or a `match` decides whether something is read. Such a node whose condition is pending evaluates no arm, and a quantifier whose collection is pending evaluates no body. Otherwise every operand, quantifier element and argument is evaluated, so a quantifier over ten thousand elements asks for all of them in one round, as the contract's growth requirements need.
- Under the old engine's reader, which never answers "not here yet", everything is evaluated and skipped as before.
- A child is declared only from values that contain no stand-in, and the list of made instances is declared after the last pass. Leaf κ₂ (#10651) builds this.
- *Why draft 3's rule was not taken:* it skipped an operand once a stand-in could decide it; a critic showed that costs one pass per element and makes cycles depend on operand order.

**B. Which made instances a summary reads.**

- A unit of source may make instances if it contains a constructor call, a call to a user function, a subexpression of function type, or a call to a built-in that constructs a structure value. A built-in that only selects or passes a value along (`xs[0]`, `filter`, `max_by`) makes nothing.
- A value that built-in code constructs is made where it is returned, so its definition's constraints are checked.
- Exactly one summary reads a cell's list: that of the instance the cell belongs to. For a parameter cell evaluating an argument, the mark arrives with the argument.
- *Open:* draft 3 had both the declaring and the receiving instance read a parameter's list, which counted a violation twice.

**C. What makes a trial point infeasible.**

- A constraint that is undefined under a trial makes that point infeasible when the undefined input it read depended on the trial's values: an element past the end of a collection whose count is on trial.
- When the undefined input did not depend on the trial, the constraint is undetermined and is reported as a fault of the model, not as a bound the unknown cannot meet.
- The spec's 8.9 calls a read past the end an error of the definition; under a trial this differs.
- *How a solve tells them apart.* The core says, for each read a solve makes through its view, whether that answer depended on the trial's values. A constraint's answer as a whole usually does, so that alone does not separate the cases. The undetermined verdict names its undefined inputs by ref (Part 2); the solve reads each of those through its view and gets that answer's flag, following a chain ref by ref. The core-PRD session confirmed this needs no change to the core.
- *Open:* what a solve concludes when an input is absent at every trial because of the unknown's bounds is the solve design's. None of this has been built.

**D. The default roots, exactly.**

- Group the entry file's definitions that use one another. A group is used if a definition outside it, other than a test, names one of its members as a sub's type, an element's type, in a constructor call, as the type of a member (written or inferred), or as a type argument.
- A use in a function body does not count, so a definition constructed only inside functions stays a root.
- A `@test` definition is not a default root.
- If that leaves no root in a file that has definitions other than tests, every one of those is a root.
- *Open:* each round found a file the then-current rule got wrong.

**E. One name declared under several guards.**

- It is present, with the declaration whose guard is true, only when every other guard is false.
- If two are true it is a failure that names them. If none is true and all are false it is absent. Otherwise its presence is undetermined.
- The declarations must give the same kind of node: a collection and a single sub under one name is a compile error.
- Reading a member that only some arms' definitions have is governed by the spec's narrowing (6.4) at compile time and gives undefined at run time.

**F. The kinds of step in a path, and their text.** **Settled 2026-11-27, by building.**

A path is a root, then a flat sequence of steps, and any step may follow any other except as the first rule below says. A member's name follows a dot and everything else follows a slash. No name holds a dot or a slash, and a bracket's content is read as a whole, so each step's text ends where the next begins.

| Step | Text | Example |
|---|---|---|
| Root: a definition of the entry file's module, which is not written | the name | `Asm` |
| Root: a definition of another module | the module's dotted path, `::`, the name | `std.units::Plate` |
| A member, by name | `.name` | `Asm.thin.t` |
| An element, by position or by key | `[n]`, `["key"]` | `Asm.vents[2]`, `Asm.vents["intake"]` |
| The instance's summary | `/summary` | `Asm.b/summary` |
| The instance read as a value | `/as-value` | `Asm.a/as-value` |
| A definition's member table, named as a root is | `/defs[root]` | `Asm/defs[Plate]`, `Asm/defs[lib::Motor]` |
| The `k`th constraint that gives a verdict; a `forall` block's constraint for one element adds an element step | `/constraint[k]` | `Asm.b/constraint[0]`, `Asm/constraint[1][3]` |
| What is written in a sub's braces, followed by the step to a constraint, a `let` or a guard there | `/braces` | `Rig.motor/braces/constraint[0]`, `Asm.s/braces.zz`, `Asm.p/braces/guard[0]` |
| The `k`th applied purpose | `/purpose[k]` | `Asm/purpose[0]/constraint[0]` |
| The built-in verdict on an instance's arguments | `/arguments` | `Asm.b/arguments` |
| The `k`th guard's condition | `/guard[k]` | `Case/guard[0]` |
| A collection's count | `/count[name]` | `Asm/count[vents]` |
| A sub's placement | `/at[name]` | `Asm/at[a]` |
| An unknown with no name of its own, by its site (below) | `/auto[site]` | `Asm/auto[a.len]` |
| The list of what a cell made | `/made` | `Asm.c/made` |
| A made instance, by the chain of call sites and iteration indices from the cell's expression to the call | `/made[i,j,...]` | `Asm.c/made[0]`, `Asm.c/made[0,2]` |
| A cell's value opened as an instance; for a value opened in mid-expression, by its chain | `/open`, `/open[i,j,...]` | `Asm.c/open.area`, `Asm.m/open[0].area` |
| What an instance tells a solve | `/problem` | `Asm.a/problem` |
| The solve design's nodes | `/solves` | `Asm.a/solves` |

An unknown's site is an anchor, then at least one segment:

| Part | Text | Example |
|---|---|---|
| Anchor: a member, or a keyed collection's entry | `name`, `name["key"]` | `Asm/auto[a.len]`, `M/auto[vents["a"].w]` |
| Anchor: the `k`th `connect` | `connect[k]` | `Asm/auto[connect[0].p]` |
| Segment: the argument the unknown fills | `.argument` | `Asm/auto[d.p#0.t]` |
| Segment: the chain to a call inside the expression, always followed by the argument it fills | `#i,j,...` | `Asm/auto[both#1.t]` |
| Segment: the anchor sub's placement | `/at` | `Asm/auto[a/at]` |

- A key is written in quotes. Inside it `\"` is a quote and `\\` a backslash, and there is no other escape: `Asm.k["say \"hi\" \\ bye"]`. A `]`, a dot or a slash inside a key is plain text: `Asm.ducts["a.b/c]"].area`. A member anchor's element is only ever a key, so `connect[0]` is always a `connect`.
- Numbers are plain decimal, with no sign and no leading zero, and fit in 64 bits. There is no whitespace anywhere. The parser accepts only this text, so printing a path and parsing its text give each other back exactly.
- Two rules make the text one-to-one. No element step comes directly after a bare `/made` or `/open`: the made list's instances are named by their chain, and an opened value is an instance, not a collection, so `/made[0]` is always a made instance. The placement's unknown is `/auto[name/at]`, not the starting form's `/auto[name.at]`, because `at` can also be the name of an argument.
- A member named like a fixed child is written after a dot, so `Asm.summary.area` is a member and `Asm/summary` the summary; `Asm.braces.x` is a member's member and `Asm/braces.x` a `let` in braces. No word is reserved: `at` and `connect` are valid names.
- The steps are a free sequence, not a grammar of which step may follow which, because `Bracket/purpose[0]/constraint[0]/made[0]/constraint[0]` names a constraint of an instance made under a constraint (`purposes/instance-made-in-purpose-constraint-does-not-bear`).
- Decided by building: the round trip over generated paths and `scenario_h1` in `crates/reify-core/tests/instance_path_text.rs`, and every path of the corpus parsed at load. The entries that decided it: `paths/path-text-round-trips-one-to-one` (a key holding a dot, a slash and `]`; a sub named `summary`), `paths/unnamed-steps-name-their-nodes`, `paths/keyed-entry-autos-have-distinct-paths`, `paths/mid-expression-opens-numbered-by-member-read`, `paths/made-open-as-value-paths`, `paths/constraint-braces-purpose-paths-by-position`, `paths/braces-guard-and-definition-guard-distinct-paths` and `purposes/instance-made-in-purpose-constraint-does-not-bear`.
- The type is `crates/reify-core/src/instance_path.rs`; its text, both ways, is `crates/reify-core/src/instance_path_text.rs`.

**G. The parameter row's control.** Settled by Leo on 2026-11-26 and moved to Part 9. What remains to settle by building: the refusals at the edges (an element of a list, which has no arguments of its own until task 5482; an imported definition; a unit that cannot be written back exactly; an argument assigned under a guard in the braces) and the positions the parser must keep to add an argument.

### 15.2 Not designed, and what would settle each

- **What `partially_determined` and `constrained` mean.** Today they read "the solver is in progress" and "auto or provisional". A body on the core cannot see the first. The second must be answered from the declaration, not from how the cell got its value, or a made instance and its opened value disagree (a critic's model: `pub let via_solver = constrained(t)`). *Settle:* a ruling on what determinacy a value carries, with the expression kinds.
- **A purpose whose subject is a constraint or a field** stays refused. Leo, 2026-11-26: "Acceptable, especially for now. Not a long term ruling, especially for fields - I can imagine wanting to apply a purpose to a field. I can't currently make sense of a purpose on a constraint".
- **The syntax for applying a purpose**, and where the session's uncommitted edit puts the line. *Settle:* in the PRD; the model is `constraint Name(args)`.
- **An `Output` inside a purpose.** The spec allows it, and one fixture has one. Not examined. **Amended 2026-11-26:** a scan found no tracked source with an `Output` in a purpose; one fixture has a sub there, which the parser skips.
- **Cost per instance in the index.** An instance per site, an opened value per holding cell, and values as large as what was decided inside them. *Settle:* count index entries on a model with ten thousand values, with the core's counters.
- **What the depth limit counts.** A `made` or `open` step is a step, so the same limit allows different recursion depths through different constructs. *Settle:* with the core PRD.
- **When alternatives switch, "what was there last time" comes from another definition.** *Settle:* with the solve and geometry designs, which are its only readers.
- **Generic definitions** (agreed 2026-11-26: "lgtm"). One member table per specialisation. A type argument written `auto` stays the compiler's choice. The compiler part of the PRD designs the rest.
- **An argument given to a parameter that is absent.** A value does not carry it. Whether giving one is an error is not said.
- **Ports** (agreed 2026-11-26: "lgtm"). Named groups of cells in an instance. A guard on a port is honoured, where today it is dropped. A port of a value reads like any member. The compiler's handling was not examined.
- **Derived subs** (`mirror of`) have no consumer today (task 6616). Not designed.
- **`occurrence def`** becomes a template like a structure today and cannot be constructed as a value. Assumed to be the same instance kind. Not examined.
- **A module-level `field def`** goes to the geometry and compute design, and derived subs stay with task 6616 (Leo, 2026-11-26: "yes, unless we find a good reason to change that later").
- **Whether a `let` is readable from outside without `pub`.** The spec says no and the compiler allows it. No task for it was found, and none was searched for. It is a compile-time check and does not touch the kinds.
- **Positions for the edit gesture.** The parser keeps no position for an argument's name or for where a new argument would go.
- **Every run was on a binary about 225 commits behind main.**

---

## 16. What goes to other designs and sessions

**To the core-PRD session:** `contract-requests.md` (two versions) and a third note. Answered: a declaration may pass a binding along; the list of made instances is content in a small child, with no new core read; an inner solve following an outer one through its arguments is agreed and is a named test. Sent since: one member table per definition, reached through a passed binding; definition names in member tables, which touches 5.3; section 8's third planning rule needs restating for unknowns of different owners; 5.4 should say a value carries every value decided at or below; this design and the spec's 3.4 differ on an index out of range. Also sent: rule A's "everything outside an arm is asked for in one round", which it found to agree with the contract as written; and rule C's question, which it answered (Part 15.1). It reports three rulings of Leo's in its own thread: the crate is `reify-eval-core`; the core PRD's proposed contract sentences are approved; a root's declaration is a snapshot input. The core PRD is approved and its filing has gone to a further session, listed in `sessions.md`; contract changes still go to the core-PRD session. Waiting on Leo here: what a structure value is (5.4), the wording of section 8 and Example 5, Example 3.

**To the held-task review:** `for-held-task-review.md`: the kept-code changes of Part 11 and of rules A and B, and that this design asks for what tasks 8704, 7717 and 9024 (a structured subject on constraint diagnostics) plan. To add: task 9374 may close on decision 1.

**For the solve design, whose home is now task 10610** (the milestone "Design the solve on the new evaluation engine and author its PRD", filed 2026-11-26 by the core-PRD session on Leo's instruction; it points at Part 6.4 of this file by path and carries this list in its details, and it replaces the old solve-node milestone 10296). If Part 6.4 or this list changes in a way the solve design must know, the change goes into that task's details. The list: Part 6.4 in full, rule C, and 6.1's case with no answer. Also: objectives are members of member tables, and how they travel is the solve design's; two examples and three other files rely today on a container's objective reaching a child's unknown; a value is never reached into; a sub's braces may hold an objective; a purpose may not, and a purpose's constraints are never part of a solve's problem (Leo, 2026-11-26).

**For the geometry design.**

- A geometry member is a cell whose value is a description, in its definition's frame.
- Only places are enumerated for drawing, export, mass and the bill of materials.
- A tolerance requirement arrives today through a purpose and applies to everything below its subject. A purpose now only checks and is applied in source, so whether such a requirement drives the tolerance a shape is made at, or only checks it, is yours. Either way it has to flow down through declarations.
- Whether geometry compares by content (7.4).
- A value that built-in code constructs is made where it is returned (rule B); the results of an FEA are such values.

---

## 17. A proposal for the PRD

One PRD, "instances on the core", after the core's own. (The core crate is `reify-eval-core`; the core-PRD session reports that Leo ruled on the name on 2026-11-25.) In four parts:

1. **Compiler output** (Part 11), emitted beside today's.
2. **Typed paths** and their one text form (Part 8, rule F), in the shared crate, taking in tasks 8704 and 7717, with a test that the text form is one-to-one.
3. **The kinds:** definition table, member table, instance, cell with its evaluator adapter, constraint, collection, as-value, summary. The unknown cell and its seam to a solve are built and tested against the core crate's toy solver (**amended 2026-11-26:** against a small solver in the instance crate's own tests; the core's cannot be used from another crate). The real solve arrives with the solve PRD.
4. **A behavioural corpus, written first:** `.ri` files with expected answers by path, independent of any engine's result types. The contract's examples, every model in this document and in the nine critics' reports, and the held defect records. Rules A to G of Part 15 are settled against it.

Not in it: the solve kinds, the geometry kinds, the session layer and the GUI. Part 9 and rule G are input to that later PRD.

**Two edges to add when this PRD is decomposed** (asked by the core-PRD session, 2026-11-26; both tasks read with `get_task`):

- The leaf that builds the unknown cell and its seam to a solve becomes a dependency of task 10610, if 10610 is still pending: `add_dependency(id="10610", depends_on=<that leaf>)`. Today 10610 depends only on the core leaf for a view opened under another view (task 10563).
- This PRD's close leaf becomes a dependency of task 10552, the capstone of the new-evaluation-engine program, which asks that of every PRD of the program.

**What authoring this PRD depends on** (Leo asked on 2026-11-26 whether its details depend on open questions outside this session; if not: "when we're done here, /reflect, then /spawn fable, x-high /team (/prd that)"). Nothing outside this session blocks authoring it:

- *The core.* Its PRD is approved and decomposed, and the contract is in the repository (`docs/evaluation-core-contract.md`). This PRD's leaves take dependency edges on core leaves when it is decomposed. That is ordering, not an open question.
- *The solve design* (task 10610) comes after. This PRD defines the seam: an unknown's cell reads the solve that decides it through a binding. It tests the seam with the core's toy solver (amended as above).
- *The geometry and compute design.* This PRD treats a geometry member as a member whose kind another PRD supplies, and its corpus is models without geometry. So tool bodies and constraints over geometry are specified here and become testable only when the geometry kinds exist. It is owned by task 10611, the milestone "Design the geometry and compute kinds on the new evaluation engine and author their PRD", filed 2026-11-26 by the core-PRD session on Leo's instruction. Its details carry this design's Part 16 list "For the geometry design"; a change to that list goes into that task's details. Task 10550 (free one shape in the kernel interface) is a precondition of the geometry kinds.
- *The session layer* (task 10551) declares roots and owns what `check` does with an undetermined verdict. This PRD tests with a minimal host.

Nothing has been filed from this session.

---

## 18. How this draft came about

**The team.** Nineteen seats, about 5.7M tokens:

| Wave | Seats | Tokens |
|---|---|---|
| Facts and a second design | Roots, structure, values, compiler output, GUI and paths, purposes and connections, what starts failing, task store, an independent second design | about 2.9M |
| Review of draft 1 | Logic, language, claims, held defects | about 1.1M |
| Review of draft 2 | Logic, language, claims, a count of what existing sources rely on | about 1.1M |
| Review of draft 3 | Logic, claims and decisions | about 0.6M |

Their reports are in `seats/` and `critics/`. The lead's positions before any seat reported are in `lead-positions-before-seats.md`.

**Where the two designs agreed without seeing each other:** one instance kind; no argument nodes; ownership as one rule; what a value carries; one opened value per holding cell; presence read by the member; the default for roots; where a purpose lives.

**What this design took from the second design:** a member table as a node; `as-value` as a node; an `auto` argument as an unknown of the writer; the note computed from recorded reads; equality by content; a place required for `connect`; reading something absent as undefined.

**Where they still differ:** the second design allows a purpose's subject to be a constraint or a field now, and makes `==` on structures legal. It put the list of made instances in the cell's own result.

**What each review changed.**

- *The first:* presence undetermined in place of a failure; one member table per definition; Part 6.4 rewritten from claims about how solves are grouped into requirements on the solve design; the sub's braces raised to a decision; thirty-three statements of fact corrected, three of them paraphrases presented as Leo's words.
- *The second* (16, 13 and 13 faults from its three critics): reading what is not there gives undefined, not a failure; a value carries everything decided at or below; the braces and purposes as one decision; a count showing that the ownership rule and the absent-member rule change no `auto` argument and no guarded read in the tracked `.ri` files.
- *The third* (19 faults from its logic critic, 11 from its fact check): only an `if` or a `match` selects what is read; a solve reads only the arguments its constraints read; an undefined value carries no cause; a `present` test; the earlier ruling in task 9374; every decision given a complete model.

**Why there is no fourth round.** The third logic critic was asked which rules are stable. Its answer: the choices about meaning (who owns an `auto`; absent members; what a value carries; places; collections) follow from traced models and are ready for a decision. Seven mechanical rules are properties over all expressions or all files, and each round found new counter-examples in exactly those. They are Part 15.1. The contract session reached the same point after four rounds and chose to build the fragile parts under test. I recommend the same here.

**Not reviewed.** Draft 4's changes: Part 2's two new paragraphs (why a value is undefined is found when shown; an undefined argument's built-in verdict), the rule for arguments in Part 4, the `present` test, rules A to G as restated, and every model in Part 14.
