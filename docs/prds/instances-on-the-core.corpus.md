# Instances on the core: the catalogue of behavioural models

**Status:** landed with the PRD `docs/prds/instances-on-the-core.md`, which Leo approved on 2026-11-26. 191 entries. It is the input of that PRD's first leaf.

**What this is.** Each entry is a small `.ri` source and the answers it must give, by path. The first task of the PRD turns each entry into two files, `<name>.ri` and `<name>.toml`, under `crates/reify-eval-instances/tests/corpus/<theme>/`. From then on those files are the maintained copy and this document is a frozen record.

**Where the entries came from.** Five seats drafted 249 entries from the design's own models (its Parts 3 to 14), the contract's worked examples, the nine critics' reports on the design's drafts, and the 81 held defect records of the old engine. A planning seat merged them to one entry per distinct behaviour: 139 kept and 110 dropped as duplicates, with the dropped entries' sources named in the kept entry's Provenance. 52 more were added after the review of the PRD, the tracing of the catalogue and Leo's rulings. Every source was parsed with the grammar on main. The ones that need `grammar-purpose` parse once their `purpose Name(args)` lines are taken out.

**Expected answers are under Leo's final rulings** (design draft 5), not under the draft a model was first written against. Where the design does not determine an answer, the entry's basis is `open`: it says what the question is and who owns it, and it pins nothing for the affected paths. Some answers rest on a reading the PRD's lead made to keep the catalogue consistent. Those readings are the appendix of this document, and an entry that rests on one cites it as "decisions" or "readings" with its letter and number. The PRD's section 7 put them to Leo, who ruled on 2026-11-26; the appendix says which he changed and which are still under a question of his.

## How to read an entry

- **Tests:** the behaviour the entry pins.
- **Rule:** the rule of the design it tests.
- **Needs:** what must be built before the entry can run (table below).
- **Answer basis:** `traced` (traced by hand under the final rulings), `stated` (the design or the contract states the answer), or `open`.
- **Today:** what the old engine does, where a seat observed it on the binary of 2026-11-23 or read it in code.
- The table gives each pinned path and its expected answer.

| Need | Built by | Entries |
|---|---|---|
| `two-files` | leaf α (the runner) | 1 |
| `paths` | leaf β | 11 |
| `grammar-purpose` | leaf γ₁ | 19 |
| `compile-error` | leaf δ₂, with the leaf that builds the rule | 49 |
| `instance` | leaf ε₂ | 157 |
| `eval-reads` | leaf ε₂ | 11 |
| `constraint` | leaf η | 88 |
| `summary` | leaf η | 75 |
| `guard` | leaf θ₁ | 60 |
| `one-name` | leaf θ₂ | 10 |
| `collection` | leaf ι | 23 |
| `value` | leaf κ₁ | 33 |
| `made` | leaf κ₂ | 43 |
| `open` | leaf κ₃ | 32 |
| `braces` | leaf λ | 25 |
| `unknown` | leaf μ₁ | 61 |
| `solve-forced` | leaf μ₁ | 36 |
| `solve-infeasible` | leaf μ₁ | 3 |
| `nested-solve` | leaf μ₂ | 11 |
| `purpose` | leaf ν | 22 |
| `roots` | leaf ξ | 31 |
| `connect` | leaf ο | 2 |
| `generic` | leaf π | 3 |
| `state-tests` | leaf ρ | 3 |
| `edits` | leaf σ | 1 |
| `solve-real` | task 10610, waits | 10 |
| `geometry` | task 10611, waits | 3 |
| `required-arguments` | task 7874, waits | 2 |

**Counts.** 176 entries need only what this PRD builds. 10 wait for the solve kinds (task 10610) and 3 for the geometry kinds (task 10611); 2 wait for task 7874. Basis: 123 traced, 62 stated, 6 open.

## The entries for each mechanical rule

- **Rule A** (how the expression evaluator reads): `absent-member-read-across-instance-undefined`, `guarded-reads-elsewhere-left-to-run-time`, `index-past-end-reads-undefined`, `cycle-through-and-operand-cannot-be-caught`, `determined-of-stand-in-declares-once`, `or-operand-constructor-call-makes-instance`, `undefined-argument-instance-made-in-either-order`, `ring-through-unneeded-operand-is-cycle`, `stand-in-inside-container-declares-no-child`, `unwrap-or-default-read-makes-instance`, `quantifier-later-element-read-is-cycle`.
- **Rule B** (which made instances a summary reads): `constraints-checked-on-every-sub-and-value`, `instances-made-via-function-and-lambda-counted`, `argument-made-instance-counted-once`, `instance-made-in-count-cell-counted`, `selecting-builtin-makes-no-instance`, `two-calls-one-function-two-instances`, `inline-or-wrapped-constructor-same-answer`, `value-verdicts-counted-once-at-made-instance`, `nested-and-listed-made-instances-counted-once`, `instance-made-in-guard-condition-counted`, `or-operand-constructor-call-makes-instance`, `unwrap-or-default-read-makes-instance`.
- **Rule C** (what makes a trial point infeasible): `index-past-end-under-trial-infeasible`, `trial-independent-undefined-input-is-model-fault`, `input-absent-at-every-trial-left-open`, `index-past-end-on-trial-rejects-candidate`.
- **Rule D** (the default roots): `a-sub-type-use-removes-default-root`, `unrelated-definitions-are-each-a-root`, `collection-element-type-counts-as-use`, `constructor-call-counts-as-use`, `inferred-member-type-from-function-counts-as-use`, `function-body-construction-does-not-count-as-use`, `file-of-only-tests-has-no-root`, `recursive-groups-judged-as-one`, `mutually-typed-pair-are-both-roots`, `imported-definitions-are-never-roots`, `test-use-through-function-does-not-count`, `member-type-and-type-argument-count-as-uses`, `test-definitions-and-test-uses-ignored`, `construction-in-purpose-body-does-not-count`, `constructor-in-member-lambda-counts-as-use`, `generic-without-type-default-is-not-root`, `generic-only-file-has-no-root`, `imported-definition-checked-on-each-instance`, `guarded-sub-and-block-per-instance`, `recursion-guard-read-per-level`.
- **Rule E** (one name declared under several guards): `collection-and-sub-under-one-name-rejected`, `where-else-name-is-one-unguarded-member`, `match-arm-subs-created-per-instance`, `name-under-overlapping-guards-all-cases`, `let-and-sub-under-one-name-rejected`, `nested-where-else-arms-give-one-member`, `separate-wheres-read-without-guard`, `arm-only-member-read-across-instance`, `unguarded-and-guarded-declaration-of-one-name`, `one-name-with-failed-guard-is-blocked`.
- **Rule F** (the kinds of step in a path, and their text): `undefined-argument-reported-by-arguments-verdict`, `forall-block-gives-one-verdict-per-element`, `keyed-forall-verdicts-addressed-by-key`, `two-calls-one-function-two-instances`, `two-autos-in-one-expression-two-unknowns`, `path-text-name-index-key-resolves-instance-values`, `constraint-braces-purpose-paths-by-position`, `made-open-as-value-paths`, `unnamed-steps-name-their-nodes`, `path-text-round-trips-one-to-one`, `constraint-identity-is-its-position`, `keyed-entry-autos-have-distinct-paths`, `braces-guard-and-definition-guard-distinct-paths`, `mid-expression-opens-numbered-by-member-read`.
- **Rule G** (the parameter row's control) is the GUI's. It has no entry here and waits for the session layer (task 10551).


---

## Roots (rule D)

### a-sub-type-use-removes-default-root

- Tests: A definition that another definition of the entry file uses as a sub's type is not a default root, and its constraint is checked on the instance at the argument it was given.
- Rule: decision 2; decision 7; rule D; Part 3
- Provenance: design Part 14 decision 2 (model); Part 3. design decision 2 (its model); claims-3 probe d2.ri; language-1 L4 model 2; second-design Q1.
- Needs: roots, instance, constraint
- Answer basis: stated: decision 2, "one root, `Bracket`, and one verdict, at `Bracket.b`, checked at 8 mm". `Bolt.d` is a path under a root that was never declared, which "answers 'absent'" (Part 3).
- Today: two roots, and `Bolt` checked once at 6 mm; the bolt in the bracket is not checked (run, `p10.ri`: `OK Bolt#constraint[0]`, `Bolt.d = 0.006 m`).

```ri
structure def Bolt    { param d : Length = 6mm   constraint d >= 3mm }
structure def Bracket { sub b : Bolt { d = 8mm } }
```

| Path | Expect |
|---|---|
| (file) | roots Bracket |
| Bracket.b.d | value 8 mm |
| Bracket.b/constraint[0] | verdict satisfied |
| Bolt.d | absent |

### unrelated-definitions-are-each-a-root

- Tests: Definitions that do not use one another are each a default root, and each is evaluated as its own instance.
- Rule: rule D; Part 3
- Provenance: design Part 3 ("With several roots, `check` checks each"); decision 2 ("Alone in a file, `Bolt` is the root")
- Needs: roots, instance, constraint
- Answer basis: traced. Neither definition names the other, so each is its own unused group. Each root is evaluated at its defaults: d = 2 mm, and `d >= 3mm` is violated at each.

```ri
structure def Bolt { param d : Length = 2mm   constraint d >= 3mm }
structure def Nut  { param d : Length = 2mm   constraint d >= 3mm }
```

| Path | Expect |
|---|---|
| (file) | roots Bolt, Nut |
| Bolt/constraint[0] | verdict violated |
| Nut/constraint[0] | verdict violated |

### collection-element-type-counts-as-use

- Tests: Naming a definition as the element type of a collection sub counts as a use.
- Rule: rule D (clause "an element's type")
- Provenance: design Part 15.1 rule D
- Needs: roots
- Answer basis: stated: rule D lists "an element's type" among uses. Only the roots row is pinned, and the second output records what each definition uses (leaf δ₂), so the entry does not wait for collections.
- Today: every definition in the file is a root (design Part 3).

```ri
structure def Bolt { param d : Length = 6mm }
structure def Rack { sub bolts : List<Bolt>   constraint bolts.count == 2 }
```

| Path | Expect |
|---|---|
| (file) | roots Rack |

### constructor-call-counts-as-use

- Tests: A definition used only through a constructor call in another definition is not a default root.
- Rule: rule D (clause "in a constructor call"); Part 3
- Provenance: design Part 3 ("a definition used only through `let c = Child(...)` is exported alone", as today); rule D
- Needs: roots
- Answer basis: stated: rule D lists a constructor call among uses. Only the roots row is pinned.
- Today: `Plate` is evaluated as a root, and geometry export exports it alone at its defaults (design Part 3, run).

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm   { let c = Plate() }
```

| Path | Expect |
|---|---|
| (file) | roots Asm |

### inferred-member-type-from-function-counts-as-use

- Tests: A member whose inferred type is a definition uses that definition, even when the only constructor call is inside a function, so a type annotation does not change the root set.
- Rule: rule D (clauses "as the type of a member (... inferred)" and "a use in a function body does not count")
- Provenance: critic logic-3 R13 (`let b : Bracket = mk_bracket()` against the same line without the annotation); design Part 15.1 rule D. language-1 L4 model 4. logic-3 R13 (a type annotation changed the root set).
- Needs: roots, instance, made, open
- Answer basis: traced. The call inside `mk` is a use in a function body and does not count (decisions D22). `Asm.b` has inferred type `Plate`, which counts: decisions D24, a definition that is the type of a member, written or inferred, counts as used even when the value comes from a function. So `Plate`'s group is used; the only root is `Asm`. Writing `let b : Plate = mk()` gives the same. `mk()` makes `Plate(t: 2mm)` inside `mk`'s body (`Asm.b/made[0,0]`), and `b` holds its value. A cell declares no member `t`, so a read of the value's `t` is written through `/open` (decisions A): `Asm.b/open.t` is 2 mm.

```ri
structure def Plate { param t : Length = 3mm }
fn mk() -> Plate = Plate(t: 2mm)
structure def Asm { let b = mk() }
```

| Path | Expect |
|---|---|
| (file) | roots Asm |
| Asm.b/open.t | value 2 mm |

### function-body-construction-does-not-count-as-use

- Tests: A definition constructed only inside a function body, and held in no member of its type, stays a default root.
- Rule: rule D (clause "a use in a function body does not count")
- Provenance: design Part 15.1 rule D; critic logic-2 N17(b); critic language-1 L4 item 4
- Needs: roots
- Answer basis: stated: rule D, "a definition constructed only inside functions stays a root". `Asm.a` has type `Length`, so no member names `Plate`.

```ri
structure def Plate { param t : Length = 3mm }
fn plate_t() -> Length = Plate(t: 2mm).t
structure def Asm { let a = plate_t() }
```

| Path | Expect |
|---|---|
| (file) | roots Plate, Asm |

### file-of-only-tests-has-no-root

- Tests: A file whose only definitions are tests has no default root.
- Rule: rule D (clause "a `@test` definition is not a default root"; the fallback applies only to "a file that has definitions other than tests")
- Provenance: design Part 15.1 rule D; decision 12 ("A file with no root"). design decision 12 list (a file with no root); rule D. logic-2 N17(a).
- Needs: roots
- Answer basis: stated: rule D. What `check` prints for it is the session's (decision 12).

```ri
@test
structure def TestNothing { constraint 1mm < 2mm }
```

| Path | Expect |
|---|---|
| (file) | roots (none) |

### recursive-groups-judged-as-one

- Tests: Definitions that use one another are judged as one group: a self-recursive definition used by nothing else is a root, and a mutually recursive pair used from outside contributes no root.
- Rule: rule D (clause "group the entry file's definitions that use one another"); Part 4 (recursion is a guarded sub)
- Provenance: critic logic-1 L11 and language-1 L4 item 3 (self-recursive `RecTree` dropped); critic logic-2 N17(c) (`F`, `E`, `D`); design Part 15.1 rule D. language-1 L4 model 3 (RecTree in examples/integration_corner_cases.ri; a mutually recursive pair). logic-2 N17(c).
- Needs: roots, instance, guard
- Answer basis: traced. Groups: {Tree}, {E, D}, {F}. No definition outside {Tree} names `Tree`: root. `F` names `E`, so {E, D} is used: no root from it. Nothing names `F`: root. Below `F`: `F.e.n` is 1, so `F.e.d` is present with `m = 0`; its guard `m > 0` is false, so `F.e.d.e` is absent.

```ri
structure def Tree { param depth : Int = 1   sub left : Tree where depth > 0 { depth = self.depth - 1 } }
structure def F { sub e : E }
structure def E { param n : Int = 1   sub d : D where n > 0 { m = 0 } }
structure def D { param m : Int = 0   sub e : E where m > 0 { n = 0 } }
```

| Path | Expect |
|---|---|
| (file) | roots Tree, F |
| F.e.d.m | value 0 |
| F.e.d.e | absent |

### mutually-typed-pair-are-both-roots

- Tests: Two definitions that each have a parameter of the other's type form one unused group, so both are roots and the file is not left with none.
- Rule: rule D (grouping clause; fallback clause)
- Provenance: critic logic-3 R13 ("Two definitions that each have a parameter of the other's type leave a file with no root"); design Part 15.1 rule D. logic-3 R13; logic-1 L11.
- Needs: roots
- Answer basis: traced for the row: `A` names `B` and `B` names `A`, so they are one group; nothing outside names either, so both are roots. The grouping clause decides the row, not the fallback clause. Open: whether rule D's fallback clause ("if that leaves no root ... every one of those is a root") is ever reached (readings D26). No pinned row depends on it; rule D's final text is written by leaf ξ.
- Note: the fallback clause appears unreachable (decisions D26). Every file with a non-test definition has at least one unused group, except one whose only non-test definitions are generics with no default for a type parameter, and decisions D25 says such a file has no root, the fallback included (entry `generic-only-file-has-no-root`). This is the only entry in the catalogue that carries the question, and the question pins no row.

```ri
structure def A { param b : B }
structure def B { param a : A }
```

| Path | Expect |
|---|---|
| (file) | roots A, B |

### imported-definitions-are-never-roots

- Tests: The `pub` definitions of an imported module are never default roots of the entry file.
- Rule: Part 3 ("Imported definitions are not roots"); decision 2 ("made in passing")
- Provenance: design Part 3; Part 14 decision 2
- Needs: roots, two-files
- Answer basis: stated: Part 3. The `(file)` row is the entry file's, `main.ri`.
- Today: a direct import's `pub` definitions are evaluated as roots of the importing file (design Part 3, seat report `roots.md`).

```ri
// file: lib.ri
module lib
pub structure def Motor { param d : Length = 8mm }

// file: main.ri (the entry file)
module main
import lib
structure def App { param x : Length = 1mm }
```

| Path | Expect |
|---|---|
| (file) | roots App |

### root-parameter-without-default-is-undefined

- Tests: At a root, a parameter with no default is undefined and the verdicts that read it are undetermined, with no error.
- Rule: Part 3 ("A parameter with no default is undefined at a root"); decision 12 (task 9345 ruling)
- Provenance: design Part 3; spec 9.2; task 7874 (a root's parameters keep today's unbound meaning). language-1 L19; language-2 B2.
- Needs: roots, instance, constraint, summary
- Answer basis: stated: Part 3, "A parameter with no default is undefined at a root ... and its verdicts are undetermined". The root is given no argument, so its built-in argument verdict is satisfied (decisions B5) and the summary counts the one constraint.

```ri
structure def Bolt { param d : Length   constraint d >= 3mm }
```

| Path | Expect |
|---|---|
| (file) | roots Bolt |
| Bolt.d | value undefined |
| Bolt/constraint[0] | verdict undetermined |
| Bolt/summary | summary violated 0 undetermined 1 failed 0 |

### test-use-through-function-does-not-count

- Tests: a definition constructed only in a function body and used only by a test stays a root, and a `@test` definition is not a default root.
- Rule: rule D (second and third bullets; a use by a test does not count); decision 2 (made in passing); decisions D22.
- Provenance: logic-2 N17(b) (proposed counting a function-body use by its callers; not taken: draft 5 counts no use in a function body); logic-1 L11 (the same proposal; not taken); logic-2 N17(e) and logic-3 R13 (`@test` definitions became default roots in draft 3; taken: not a default root); logic-2 doubts (spec 12.1).
- Needs: roots
- Answer basis: traced. `Column` is named in `fixture`'s body, which does not count, and as the inferred type of `TestColumn.col`, a test, which does not count either. So `Column` is a root. `TestColumn` is `@test`: not a default root.
- Today: run `testroot.ri`: both `Column` and `TestColumn` are printed.

```ri
structure def Column {
    param w : Length = 50mm
    constraint w > 0mm
}
fn fixture() -> Column { Column(w: 60mm) }
@test structure def TestColumn {
    let col = fixture()
    constraint col.w < 100mm
}
```

| Path | Expect |
|---|---|
| (file) | roots Column |

### member-type-and-type-argument-count-as-uses

- Tests: a definition named only as a parameter's type, or only as a type argument in a member's type, is used and is not a root.
- Rule: rule D (first bullet: "as the type of a member" and "as a type argument"); decisions D24.
- Provenance: logic-2 N17(d) (under draft 2 the data definition became the only root; moot cause removed: draft 5 has no default-parameter bullet); logic-3 R13 (`M6` in `Joint<M6>` became a root; taken: type arguments count). design rule D; logic-3 R13. logic-3 R13 (M6 in Joint<M6>).
- Needs: roots
- Answer basis: traced. `Beam` names `Stock` as `mat`'s type and `M6` as a type argument in `fasteners`' type, so both are used. `Beam` is used by nothing: the only root.

```ri
structure def Stock {
    param e : Length = 1mm
}
structure def M6 {
    param d : Length = 6mm
}
structure def Beam {
    param mat : Stock
    param fasteners : List<M6>
}
```

| Path | Expect |
|---|---|
| (file) | roots Beam |

### test-definitions-and-test-uses-ignored

- Tests: a `@test` definition is not a default root, and its use of a definition does not stop that definition being a root.
- Rule: rule D; decision 2; Part 3.
- Provenance: language-1 L4 model 1 (the spec's own form, spec 2651-2658); claims-3 probe `t1.ri`; language-1 probe `t1.ri`. Taken: rule D's "a use by a test does not count" and "a `@test` definition is not a default root". Also, from the merged entries: language-1 L4 item 1; logic-3 R13; spec 12.1.
- Needs: roots, instance, constraint
- Answer basis: traced. Entry file holds `Column` and `TestColumn`. Rule D: `TestColumn` is a test, so its use of `Column` does not count; `Column` is unused, so a root. `TestColumn` is a `@test` definition, so not a default root, and a path under an undeclared root answers absent (Part 3). `Column.w` is 50 mm, so its constraint holds.
- Today: `check` checks both, printing `OK Column#constraint[0]` and `OK TestColumn#constraint[0]` as two designs (run `roots_test.ri`); `build -o` on language-1's `t1.ri` exported the two test fixtures overlapped and not the design (report: language-1 L4).

```ri
structure def Column { param w : Length = 50mm   constraint w > 0mm }
@test structure def TestColumn { sub col : Column { w = 60mm }   constraint self.col.w < 100mm }
```

| Path | Expect |
|---|---|
| (file) | roots Column |
| Column/constraint[0] | verdict satisfied |
| TestColumn.col.w | absent |

### construction-in-purpose-body-does-not-count

- Tests: a definition constructed only inside a purpose's body is still a default root, even where the purpose is applied.
- Rule: rule D; decisions D22 ("A use inside a function body or a purpose body does not count")
- Provenance: appendix D22; PRD D13 item 9. The function-body half is `function-body-construction-does-not-count-as-use`; this is the purpose-body half.
- Needs: roots, purpose, grammar-purpose
- Answer basis: stated: decisions D22. Trace: the only place `Plate` is named is `Plate()` in `fits`'s body, which does not count, so `Plate`'s group is unused: a root. Nothing names `Asm`: a root. A purpose is not a structure definition and is never a root.
- Note: the member line `purpose fits(self)` is the applied form of decisions E33 and parses once leaf γ₁ lands. With it taken out, the source parses on main.
- Today: run `g03s.ri` (the application line taken out): `Asm.w` and `Plate.t` are both printed, as two designs.

```ri
structure def Plate {
    param t : Length = 3mm
}
purpose fits(subject : Structure) {
    constraint subject.w > Plate().t
}
structure def Asm {
    param w : Length = 5mm
    purpose fits(self)
}
```

| Path | Expect |
|---|---|
| (file) | roots Plate, Asm |

### constructor-in-member-lambda-counts-as-use

- Tests: a constructor call inside a lambda written in a member's expression counts as a use, while one inside a function body does not, so only the definition built inside a function stays a default root.
- Rule: rule D; decisions D23 and D22
- Provenance: appendix D23; PRD D13 item 9. The lambda and `generate` form is the one `instances-made-via-function-and-lambda-counted` already uses.
- Needs: roots
- Answer basis: traced. `Plate(...)` is inside the lambda in `f`'s expression, part of that member's source: a use (D23). `Bolt(...)` is only in `bolt_d`'s body: not a use (D22). No member's type names either definition (`f` is `Int -> Length`, `ts` is `List<Length>`, `d` is `Length`), so D24 decides nothing here. Roots: `Bolt` and `Asm`, compared as a set (decisions B10).
- Today: run `g04.ri`: `Bolt.d` and `Plate.t` are printed as designs beside `Asm`.

```ri
structure def Plate {
    param t : Length = 3mm
}
structure def Bolt {
    param d : Length = 6mm
}
fn bolt_d() -> Length { Bolt(d: 8mm).d }
structure def Asm {
    let f = |i : Int| Plate(t: i * 1mm).t
    let ts = generate(2, f)
    let d = bolt_d()
}
```

| Path | Expect |
|---|---|
| (file) | roots Bolt, Asm |

### generic-without-type-default-is-not-root

- Tests: a generic definition whose type parameter has no default is not a default root, and a definition that satisfies its bound without being named stays one.
- Rule: rule D; decisions D25
- Provenance: appendix D25; PRD D13 item 9; the side question of `failed-type-argument-choice-is-sub-failure`.
- Needs: roots, generic
- Answer basis: stated: decisions D25. Trace: `Bearing<T: Seal>` has no default for `T`: not a default root. `Bearing` names the trait `Seal`, not `ORing`, and nothing else names `ORing`: a root. The file keeps a root, so rule D's fallback clause is not reached. A generic definition whose every type parameter has a default is not covered by D25 and is not modelled.
- Today: run `g05.ri`: `Bearing.n` and `ORing.t` are both printed.

```ri
trait Seal {}
structure def ORing : Seal {
    param t : Length = 3mm
}
structure def Bearing<T: Seal> {
    param n : Int = 1
}
```

| Path | Expect |
|---|---|
| (file) | roots ORing |

### generic-only-file-has-no-root

- Tests: a file whose only definition is a generic with no default for its type parameter has no default root; rule D's fallback clause does not make it one.
- Rule: rule D (its fallback clause); decisions D25 ("the rule's last clause included. A file that holds nothing else has no root"); decisions D26
- Provenance: trace-structure counter-example 1 (the fallback is reachable through D25's case, and would then contradict it); appendix D25; PRD D13 item 9.
- Needs: roots, generic
- Answer basis: stated: readings D25, ruled by Leo on 2026-11-26 as PRD D13 item 9. Trace: clause 1 gives one group, {Box}, which nothing uses. `Box` has no default for `T`, so it is not a default root, and D25 says the fallback clause does not make it one. There is no test. So the file has no root. Read the other way, the fallback would make `Box` a root with no specialisation reached from the entry file, so no member table, and the reading avoids that.
- Today: run `/tmp/instance-prd/fix-structure/genonly.ri`: `Box.n = 1`, evaluated as a root.

```ri
trait Seal {}
structure def Box<T: Seal> { param n : Int = 1 }
```

| Path | Expect |
|---|---|
| (file) | roots (none) |


---

## Instances and arguments

### omitted-required-argument-is-compile-error

- Tests: Omitting a required argument where a sub is declared, or in a constructor call, is a compile error, while an explicit `undef` is accepted.
- Rule: Part 3 (task 7874 ruling); Part 9 ("omitting a required argument is a compile error")
- Provenance: design Part 3; task 7874 (pending; covers `sub x : T` and bare `sub x : T`). language-1 L19; failing.md M5 and probe p/a5.ri.
- Needs: compile-error, made, required-arguments
- Answer basis: stated: task 7874's ruling, cited by design Part 3 and Part 9. The error names `d`. No leaf of this PRD builds the rejection, so the entry waits for task 7874 (PRD D9: "relied on, no edge"); `made` because the second row is about a constructor call's site in the second output (leaf κ₂).
- Today: the omitted parameter is undefined, "because: Req.w unbound", and `check` exits 0 (task 7874 record, run on 2026-09-30).

```ri
structure def Bolt { param d : Length   constraint d >= 3mm }
structure def Asm  { sub a : Bolt   sub b : Bolt { d = undef }   let c = Bolt() }
```

| Path | Expect |
|---|---|
| (compile) | error: the sub `a` omits the required argument `d` of `Bolt` (nothing is reported for `b`) |
| (compile) | error: the call Bolt() omits the required argument d of Bolt |

---

### let-cells-evaluate-from-parameter-defaults

- Tests: a `let` with an `if` and a `let` over it evaluate per instance from the parameter's default.
- Rule: contract Example 1; Part 2 (cell).
- Provenance: contract Part B Example 1.
- Needs: instance
- Answer basis: traced. `width` = 50 mm > 40 mm, so `w_eff` = 40 mm; `area` = 40 mm × 10 mm = 400 mm². Clamped is the only definition, so it is the root.
- Today: `w_eff` = 0.04 m (the contract marks the example "checked").

```ri
structure def Clamped {
    param width : Length = 50mm
    let w_eff = if width > 40mm then 40mm else width
    let area = w_eff * 10mm
}
```

| Path | Expect |
|---|---|
| Clamped.w_eff | value 40 mm |
| Clamped.area | value 400 mm^2 |

The example's point, that an edit of 50 mm to 60 mm runs two bodies and stops, is about the core's incremental behaviour and is not pinned here (left out, core).

### argument-cycle-only-when-solve-reads-it

- Tests: an argument that a child's solve reads and that reads what that solve decides is a cycle; an argument the solve does not read is not.
- Rule: decision 12 ("An argument that depends on what it helps decide is a cycle"); Part 4; Part 6.4 item 3.
- Provenance: design Part 4 ("`sub a : Plate { note = self.a.area / 40mm }` is not one, as long as no constraint of `Plate` reads `note`"). critic logic-3 R12. logic-3 R12.
- Needs: instance, unknown, solve-forced
- Answer basis: traced. In `a`, `Plate`'s constraint reads `floor`, so `a`'s `problem` lists `floor` among what its constraints read from outside, and the solve reads it before it opens its view (PRD 5.6). `floor` reads `a.t`, which reads that solve: a cycle. In `b`, the solve reads `floor` = 1 mm, not `note`; `t` = 1 mm; `note` = `b.area / 40mm` = 1 mm. The assignments are arguments only, so the entry does not need the braces leaf (decisions F).
- Today: `Asm.a.floor` and `Asm.a.area` undef, and `Asm.b.note` undef, silently (run, `argcyc.ri`).
- Candidates: `Asm.b.t`: 2 mm, 1 mm. A reading in which `t == floor` did not bear would take 2 mm. Under 2 mm the constraint is violated and depended on the trial; under 1 mm it is satisfied, and nothing else depends on the trial.

```ri
structure def Plate {
    param t : Length = auto
    param floor : Length = 1mm
    param note : Length = 0mm
    constraint t == floor
    pub let area = t * 40mm
}
structure def Asm {
    sub a : Plate { floor = self.a.t / 2 }
    sub b : Plate { note = self.b.area / 40mm }
}
```

| Path | Expect |
|---|---|
| Asm.a.t | failure cycle |
| Asm.b.t | value 1 mm |
| Asm.b.note | value 1 mm |

### argument-cycle-holds-through-let-chain

- Tests: an argument that a child's constraint reads only through a `let` of the child is still read by the child's solve before its view, so an argument that reads what that solve decides is a cycle; inserting a `let` does not turn the cycle into a value.
- Rule: decision 12 ("An argument that depends on what it helps decide is a cycle"); Part 4; Part 6.4 item 3; PRD 5.6 ("what its constraints can read from outside, through any chain of its own cells")
- Provenance: trace-structure counter-example 2 (the same model as `argument-cycle-only-when-solve-reads-it` with `let f = floor` inserted).
- Needs: instance, unknown
- Answer basis: traced. `Plate`'s constraint reads `f`, and `f` reads the argument `floor`, so `a`'s `problem` lists `floor` among what its constraints read from outside (through a chain of its own cells, PRD 5.6), and the solve reads `floor` before it opens its view. `floor` reads `self.a.t`, which reads that solve: a cycle at `Asm.a.t`. Under the other reading, where the solve reads only the arguments a constraint reads directly, `floor` would be read through the view: under the trial `t := x`, `f` = x / 2, and `x == x / 2` would take 0 mm, the first candidate.
- Today: run `/tmp/instance-prd/fix-structure/argletcyc.ri`: `Asm.a.floor` and `Asm.a.f` undef, `Asm.a.t` not printed.
- Candidates: `Asm.a.t`: 0 mm, 1 mm. The wrong reading takes 0 mm; the pinned answer is a cycle, so no candidate is tried.

```ri
structure def Plate {
    param t : Length = auto
    param floor : Length = 1mm
    let f = floor
    constraint t == f
}
structure def Asm {
    sub a : Plate {
        floor = self.a.t / 2
    }
}
```

| Path | Expect |
|---|---|
| Asm.a.t | failure cycle |

### undefined-argument-reported-by-arguments-verdict

- Tests: an argument that is undefined because it read something not there is reported once, by the `/arguments` verdict of the instance it was given to, and nowhere else; an explicit `undef` is not reported by that verdict.
- Rule: decision 12 ("An undefined argument is reported once, by a built-in verdict of the instance it was given to"; ruling 12.3); Part 2 ("An argument that is undefined is reported"); seat-context ruling 9; rule F.
- Provenance: design Part 2; design decision 12. task 9253.
- Needs: instance, guard, summary
- Answer basis: traced. `c.fan` is absent; `p`'s argument reads it. The argument's cell `Top.p.t` is the reader, so its undefined value is pinned there (decisions B1): `p.t` and `p.a` are undefined and `Top.p/arguments` is undetermined. `q` is given the literal `undef`, which does not make `/arguments` undetermined (decisions B5, following design 14.12, task 9345): satisfied. `Plate` has no constraint and `c.fan` is absent, not undetermined, so by B4 the summary has one undetermined verdict. Decisions B5 is the lead's reading, not a ruling of Leo's.

```ri
structure def Fan   { param d : Length = 40mm }
structure def Case  { param needs_fan : Bool = false   sub fan : Fan where needs_fan }
structure def Plate { param t : Length   let a = t * 2 }
structure def Top {
    sub c : Case
    sub p : Plate { t = self.c.fan.d }
    sub q : Plate { t = undef }
}
```

| Path | Expect |
|---|---|
| Top.p.t | value undefined |
| Top.p.a | value undefined |
| Top.p/arguments | verdict undetermined |
| Top.q/arguments | verdict satisfied |
| Top/summary | summary violated 0 undetermined 1 failed 0 |

### argument-to-absent-parameter-violates-arguments-verdict

- Tests: a parameter under a false guard is absent at the instance, and the instance's value does not carry it, even when the use gives it an argument; the argument is not dropped in silence: the instance's verdict on its arguments is violated.
- Rule: Part 15.2 ("An argument given to a parameter that is absent"); Part 4; Part 7.1; PRD D10(a), ruled by Leo on 2026-11-26 as recommended (readings B6).
- Provenance: design Part 15.2. logic-3 R18.
- Needs: instance, guard, value, constraint, summary
- Answer basis: traced for the first two rows; stated for the verdict: PRD D10(a) as Leo ruled it. The `/arguments` verdict is violated, and its message says to put the assignment under the same guard (`t = 8mm where thick`). It is the only verdict below `Asm` that is not satisfied.

```ri
structure def Plate {
    param thick : Bool = false
    param t : Length = 6mm where thick
}
structure def Asm { sub p : Plate { t = 8mm } }
```

| Path | Expect |
|---|---|
| Asm.p.t | absent |
| Asm.p/as-value | value Plate { thick: false } |
| Asm.p/arguments | verdict violated |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### imported-definition-checked-on-each-instance

- Tests: an imported standard-library definition is not a root, its member table is found through the definition table, and its constraints are checked on each instance, sub or value.
- Rule: rule D; decision 2 ("imported definitions are never roots"); Part 2 (the definition table covers every module the root reaches, by qualified name); decision 7.
- Provenance: logic-2 N2 (taken: Part 2's definition table).
- Needs: roots, instance, constraint, summary, made, open
- Answer basis: traced. `DimensionalTolerance` (in `std.tolerancing`, read at main) has `constraint upper_deviation >= lower_deviation`. Both the sub and the made instance `Shaft.tol/made[0]` have 0 mm against 0.1 mm: violated at each, the made instance's counted once (decisions B4). `tolerance_band` is 0 − 0.1 mm. `tol.tolerance_band` opens the value through the standard library's member table.
- Today: run `tol.ri`: `Shaft.band = -0.0001 m`; `check` prints "All constraints satisfied."

```ri
import std.tolerancing.DimensionalTolerance
structure def Shaft {
    sub dia : DimensionalTolerance {
        nominal = 8mm
        upper_deviation = 0mm
        lower_deviation = 0.1mm
    }
    let tol = DimensionalTolerance(nominal: 8mm, upper_deviation: 0mm, lower_deviation: 0.1mm)
    let band = tol.tolerance_band
}
```

| Path | Expect |
|---|---|
| (file) | roots Shaft |
| Shaft.dia.tolerance_band | value -0.1 mm |
| Shaft.dia/constraint[0] | verdict violated |
| Shaft.band | value -0.1 mm |
| Shaft/summary | summary violated 2 undetermined 0 failed 0 |

---

### member-chain-through-nested-subs-resolves

- Tests: a `let` that reads a member of a sub, which itself reads a member of its own sub, resolves at every level of nesting.
- Rule: Part 1 rule 1 (one kind makes every instance); Part 2 (cell)
- Provenance: task 5401 (the task 5360 reproduction it carries)
- Needs: instance
- Answer basis: traced.
  `Parent.m.k.off` = 10 mm × 2 = 20 mm. `Parent.m.relay` reads it, and `Parent.echo` reads `Parent.m.relay`. Each is an ordinary cell of its own instance.
- Today: `Parent.echo = undef` and `Parent.m.relay = undef`, with no diagnostic (task 5401 record, probe of 2026-07-23 at 0408733da2; tasks 5360 and 6937 may since have changed this).

```ri
structure def Kid { param w : Length = 10mm   pub let off = w * 2 }
structure def Mid { sub k = Kid()   pub let relay = self.k.off }
structure def Parent { sub m = Mid()   let echo = self.m.relay }
```

| Path | Expect |
|---|---|
| Parent.m.relay | value 20 mm |
| Parent.echo | value 20 mm |

### failed-type-argument-choice-is-sub-failure

- Tests: when the compiler cannot choose a strict `auto` type argument (two candidates fit), the sub reports a failure that says so, and nothing panics or goes silent.
- Rule: Part 6.3 (table, row "A type argument"); decisions B2; decisions D25; PRD 5.7 (row "An `auto` type argument the compiler cannot choose is a failure at the sub")
- Provenance: task 6815; task 6851 (its fixture, with `param seal : T` replaced by a defaulted `Int` parameter. Under task 7874's ruling the original omits a required argument, which is a compile error for a different reason)
- Needs: compile-error, instance, generic
- Answer basis: traced.
  Part 6.3's table: "A choice that fails gives the sub a failure outcome that says so". Decisions B2 names that kind `type-argument-unresolved`, at the sub's own path.
  `GraphiteSeal` and `ORingSeal` both satisfy `Seal`, so the choice for `Assembly.b` fails: `Assembly.b` is `failure type-argument-unresolved`.
  Because the failure is the sub's outcome, the file compiles on the second output and evaluation reaches the sub (the record's E_AUTO_TYPE_PARAM_AMBIGUOUS no longer stops evaluation there).
  Side question (can a generic definition be a default root?): decisions D25, not while its type parameter has no default, so `Bearing<T: Seal>` is not one. The file's default roots would be `GraphiteSeal`, `ORingSeal` and `Assembly`; no roots row is pinned, and the host declares `Assembly` because the expected path starts there (PRD 5.8).
- Today: a debug build panics with "unrepresentable cell_type"; `check` and `eval` exit 1 on the compile error (task 6851 record).

```ri
trait Seal {}
structure def GraphiteSeal : Seal { param t : Length = 2mm }
structure def ORingSeal : Seal { param t : Length = 3mm }
structure def Bearing<T: Seal> { param n : Int = 1 }
structure def Assembly { sub b = Bearing<auto: Seal>() }
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Assembly.b | failure type-argument-unresolved |

### argument-cell-failure-blocks-arguments-verdict

- Tests: when an argument's cell is a failure, the instance's built-in verdict on its arguments is the failure `blocked`, not undetermined.
- Rule: decisions B5 ("When an argument's cell is a failure, the verdict is `failure blocked`"); decisions B4; decisions C14; contract 7.7; PRD 5.4 (the summary reads the verdict on arguments)
- Provenance: appendix B5; the ring of `let`s of `let-cycle-fails-ring-blocks-readers`.
- Needs: compile-error, instance, summary
- Answer basis: traced. The ring of `let`s compiles (decisions C14). `x` and `y` are on the ring: cycle. `Asm.p.t` evaluates `x` in `Asm`'s scope: blocked. `p`'s argument verdict reads that cell, a failure: blocked (B5). The summary reads every child its instance declares, the verdict on arguments included, and counts as failed every node whose own answer is a failure, `blocked` included (decisions B4; PRD 5.4): `x`, `y`, `p.t` and `p/arguments`, failed 4.
- Today: run `g09.ri`: "circular let-binding dependency in template Asm: [x, y]", and `Asm.p.t = undef`.

```ri
structure def Plate {
    param t : Real = 1.0
}
structure def Asm {
    let x = y * 2.0
    let y = x * 2.0
    sub p : Plate {
        t = x
    }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.x | failure cycle |
| Asm.p.t | failure blocked |
| Asm.p/arguments | failure blocked |
| Asm/summary | summary violated 0 undetermined 0 failed 4 |

### private-let-read-from-another-definition-rejected

- Tests: a `let` that is not `pub` cannot be read from another definition's text, through a sub or through a value; a `pub let` can.
- Rule: spec 7.4 and 8.3 ("`let` bindings and constraints are private (unless marked `pub`)"); PRD D13 item 14, ruled by Leo on 2026-11-26; readings E38.
- Provenance: Leo's ruling on the PRD's D13 item 14. Draft 3 had left today's behaviour as it is.
- Needs: compile-error
- Answer basis: stated: the spec's rule, which Leo ruled is to be enforced. `area` is a private `let` of `Plate`. `a` reads it through the sub and `b` through a value: each is a compile error that names the `let` and its definition. `w` reads a `pub let` and is no error.
- Today: run `priv1.ri`: compiles; `Asm.a = 0.00012 m^2`, `Asm.b = 0.00008 m^2`.

```ri
structure def Plate {
    param t : Length = 3mm
    let area = t * 40mm
    pub let width = 40mm
}
structure def Asm {
    sub p : Plate
    let a = self.p.area
    let c = Plate(t: 2mm)
    let b = c.area
    let w = self.p.width
}
```

| Path | Expect |
|---|---|
| (compile) | error: `a` reads `area`, a `let` of `Plate` that is not `pub` |
| (compile) | error: `b` reads `area`, a `let` of `Plate` that is not `pub` |


---

## Constraints and the summary

### constraints-checked-on-every-sub-and-value

- Tests: a definition's constraints are checked on every instance, in the three sub and value forms, at that instance's arguments.
- Rule: decision 7; Part 7.6; Part 7.2 to 7.3; rule B.
- Provenance: design decision 7 (`Bar`/`Asm`); failing.md (its headline and probe `p/a1.ri`, from which this model is cut); `tests/prd-gate/fixtures/ivf_override_violates_constraint.ri` (failing.md M3). Taken. Also, from the merged entries: design decision 7 (model with a value added); Part 7.6. design Part 13 row 1. tasks 5765, 6609, 6926 (constraint half).
- Needs: instance, constraint, summary, made
- Answer basis: stated (decision 7: "violated at `Asm.b`"; 7.2: a made instance's verdicts are the ones that count). `s`, `s2`, `c` have t 1 mm: constraint[0] violated, constraint[1] (20 > 1) satisfied. `d` has t 5, w 1: constraint[0] satisfied, constraint[1] violated. Four violated, the made instances' counted once each, at the made instance (decisions B4); every `/arguments` verdict is satisfied, since every argument is defined. `sub s2 = Plate(t: 1mm)` is a sub in the call form, not a made instance.
- Today: "All constraints satisfied", checking `Plate` once at its defaults (run `every_inst.ri`).

```ri
structure def Plate { param t : Length = 5mm   param w : Length = 20mm   constraint t > 2mm   constraint w > t }
structure def Asm {
    sub s : Plate { t = 1mm }
    sub s2 = Plate(t: 1mm)
    let c = Plate(t: 1mm)
    let d = Plate(w: 1mm)
}
```

| Path | Expect |
|---|---|
| Asm.s/constraint[0] | verdict violated |
| Asm.s/constraint[1] | verdict satisfied |
| Asm.s2/constraint[0] | verdict violated |
| Asm.c/made[0]/constraint[0] | verdict violated |
| Asm.d/made[0]/constraint[0] | verdict satisfied |
| Asm.d/made[0]/constraint[1] | verdict violated |
| Asm/summary | summary violated 4 undetermined 0 failed 0 |

### partial-design-told-from-other-undetermined-causes

- Tests: each undetermined verdict says whether it is undetermined only because the design is deliberately partial (a root's parameter with no value, an argument written as the literal `undef`) or for another cause (a read of something not there, a division by zero).
- Rule: seat-context ruling 9; design 14.12 ("12.3 Agreed"); PRD 5.4 ("Telling a partial design from any other cause"); decisions B5 and B8
- Provenance: PRD boundary scenario H4; the two classes of spec 9.2's cause tracer (`Unbound` and `UserUndef` expected; the rest unexpected).
- Needs: instance, constraint, summary, guard
- Answer basis: traced.
  `constraint[0]` reads `u`, a root's parameter with no value: partial.
  `constraint[1]` reads `p.t`, whose source is the literal `undef`: partial. That argument does not trip `p`'s argument verdict (decisions B5).
  `constraint[2]` reads `c.fan.d`; `fan` is absent (`needs_fan` false), and the read across an instance compiles and gives undefined (Part 4): other.
  `constraint[3]` reads `10mm / k` with `k` = 0 mm, an operation with no value: other. Summary: the four, undetermined 4.
- Today: run `g13.ri`: "error: unknown member 'fan' on sub 'c'". Run `divz.ri`: `10mm / k` with `k = 0 m` is undef "because: op contract failed (OpContractViolation)".

```ri
structure def Fan {
    param d : Length = 40mm
}
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
}
structure def Plate {
    param t : Length
}
structure def Top {
    param u : Length
    param k : Length = 0mm
    sub c : Case
    sub p : Plate {
        t = undef
    }
    constraint u > 1mm
    constraint self.p.t > 1mm
    constraint self.c.fan.d > 1mm
    constraint 10mm / k > 1
}
```

| Path | Expect |
|---|---|
| Top/constraint[0] | undetermined-because partial |
| Top/constraint[1] | undetermined-because partial |
| Top/constraint[2] | undetermined-because other |
| Top/constraint[3] | undetermined-because other |
| Top/summary | summary violated 0 undetermined 4 failed 0 |

### partial-cause-carried-across-constructor-call

- Tests: a verdict inside a made instance whose argument is undefined because a root's parameter has no value keeps the class partial, and one whose argument is undefined for another cause keeps the class other: the class crosses the constructor call beside the argument's value.
- Rule: seat-context ruling 9; PRD 5.4 ("A cause across a constructor call"); decisions B5, B8 and C19 (a constructor call with an undefined argument makes an instance whose parameter is undefined)
- Provenance: the brief of seat fix-structure (PRD 5.4's paragraph had no entry); `partial-design-told-from-other-undetermined-causes` for the classes.
- Needs: instance, constraint, summary, made
- Answer basis: traced. `u` is a root's parameter with no value. `Plate(t: u)` makes `Top.c/made[0]` with `t` undefined (readings C19). A value carries no cause, so the adapter passes the class of `u`'s cause, partial, beside the value (PRD 5.4), and the made instance's parameter cell takes it: `constraint[0]` reads `t` and is undetermined, partial. Its `/arguments` verdict is undetermined (the argument is undefined and is not the literal `undef`, decisions B5), and it read the same cause: partial. In `d`, `10mm / k` with `k` = 0 has no value: other, at both verdicts. Summary of `Top`: the two made instances' verdicts, counted once each at the made instance (decisions B4): undetermined 4. `Top` is a root given no argument: its own `/arguments` is satisfied.
- Today: run `/tmp/instance-prd/fix-structure/partial_call.ri`: `Top.c = Plate { t: undef }`, `Top.d = Plate { t: undef }`; `check` reports one indeterminate verdict, at `Plate#constraint[0]`, the definition's defaults.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Top {
    param u : Length
    param k : Real = 0.0
    let c = Plate(t: u)
    let d = Plate(t: 10mm / k)
}
```

| Path | Expect |
|---|---|
| Top.c/made[0].t | value undefined |
| Top.c/made[0]/constraint[0] | undetermined-because partial |
| Top.c/made[0]/arguments | undetermined-because partial |
| Top.d/made[0]/constraint[0] | undetermined-because other |
| Top.d/made[0]/arguments | undetermined-because other |
| Top/summary | summary violated 0 undetermined 4 failed 0 |

### undefined-parameter-read-back-out-of-value

- Tests: a verdict that reads an undefined parameter back out of a structure value is partial when the cell that holds the value made it, in its own expression or in a function it called, and other when the value has flowed through another cell.
- Rule: PRD 5.4 ("A cause across a constructor call"); PRD D10(h), ruled by Leo on 2026-11-26 as recommended; PRD D10(d), as ruled, for the wrapped call; decision 12 of the design (telling a deliberately partial design from any other cause).
- Provenance: the second design critic's finding F3 on draft 2 of the PRD. The rows for `e` and `f` were added when Leo ruled.
- Needs: instance, value, constraint, summary, made, open
- Answer basis: stated: PRD D10(h) as Leo ruled it. `u` is a root's parameter with no value, so it is undefined with the class partial. `d` makes a plate and passes `u`: the plate's `t` is undefined, and the class travels with the argument, so the plate's own constraint and its verdict on arguments are partial. `Asm`'s first constraint reads `d.t` out of the value; the cell `d` made the value itself and knows the class of each argument it passed: partial. `e` holds the same value and made nothing, so the second constraint's read of `e.t` is other. `f` made its plate inside a function it called. Under D10(d) a function is entered and the wrapped call answers as the inline one, so the third constraint is partial: this row is the lead's reading of D10(h) after the ruling on D10(d), reported to Leo. Summary of `Asm`: its three constraints, and two verdicts at each of the two made instances: undetermined 7.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
fn mk(t : Length) -> Plate { Plate(t: t) }
structure def Asm {
    param u : Length
    let d = Plate(t: u)
    constraint d.t <= 8mm
    let e = d
    constraint e.t <= 8mm
    let f = mk(u)
    constraint f.t <= 8mm
}
```

| Path | Expect |
|---|---|
| Asm.u | value undefined |
| Asm.d/made[0]/constraint[0] | undetermined-because partial |
| Asm.d/made[0]/arguments | undetermined-because partial |
| Asm/constraint[0] | undetermined-because partial |
| Asm/constraint[1] | undetermined-because other |
| Asm.f/made[0,0]/constraint[0] | undetermined-because partial |
| Asm/constraint[2] | undetermined-because partial |
| Asm/summary | summary violated 0 undetermined 7 failed 0 |


---

## Guards and presence

### false-guard-makes-every-member-kind-absent

- Tests: Under a false guard a sub, a `let` and a constraint are all absent, with no value and no verdict, and the verdicts inside the absent sub are not counted.
- Rule: decision 3 (3a); Part 4
- Provenance: design Part 14 decision 3 (model, 3a); Part 4 ("`where` means presence for every member kind"). second-design Q3 and probe g1.ri; claims-1 part D item 10.
- Needs: instance, guard, constraint, summary, roots
- Answer basis: stated: decision 3a and Part 4 ("its path answers 'absent', it has no value and no verdict, its verdicts are not counted"; a path below an absent member is absent, Part 12 Example 3).
- Today: `Fan`'s constraint is reported violated at `Fan`'s own defaults, and `Case.k` is a cell holding undefined (run, `p8.ri`).

```ri
structure def Fan  { param d : Length = 40mm   constraint d >= 50mm }
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
    let k = 5mm where needs_fan
    constraint 1mm > 2mm where needs_fan
}
```

| Path | Expect |
|---|---|
| (file) | roots Case |
| Case.fan | absent |
| Case.fan.d | absent |
| Case.k | absent |
| Case/constraint[0] | absent |
| Case/summary | summary violated 0 undetermined 0 failed 0 |

### present-tells-absent-from-undefined

- Tests: `present(...)` tells an absent member from a present one whose value is undefined, which `determined(...)` cannot.
- Rule: decision 3 (the `present` addition); Part 4 ("A test for 'is it there'")
- Provenance: design Part 14 decision 3; Part 4; critic logic-3 R5. language-2 B5(a). logic-3 R5.
- Needs: instance, guard
- Answer basis: stated: Part 4 and decision 3b, "`determined(self.c.fan.flow)` is false when there is no fan. It is also false when there is a fan whose `flow` is undefined ... `present(self.c.fan)` would not". The `determined` rows need no word of their own: `determined` is leaf ε₂'s, built with the first cells (readings F); `present` is leaf θ₁'s. The source has no purpose line and parses today (tracer, run `present.ri`).
- Today: "warning: unresolved function: present" (run, `p2.ri`).

```ri
structure def Fan  { param d : Length = undef }
structure def Case { param needs_fan : Bool = false   sub fan : Fan where needs_fan }
structure def Top {
    sub a : Case
    sub b : Case { needs_fan = true }
    let pa = present(self.a.fan)
    let pb = present(self.b.fan)
    let da = determined(self.a.fan.d)
    let db = determined(self.b.fan.d)
}
```

| Path | Expect |
|---|---|
| Top.pa | value false |
| Top.pb | value true |
| Top.da | value false |
| Top.db | value false |

### present-of-undetermined-presence-is-undefined

- Tests: `present(...)` of a member whose presence is undetermined is undefined, not false, so an `if` on it takes no arm and an undecided member does not read as an absent one.
- Rule: readings E29; decision 3 (3c); ruling 9 of `seat-context.md` (a summary must tell an undetermined verdict from any other cause); ruling 6 (only the condition of an `if` decides what is read)
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N2; readings E29, which answers it.
- Needs: instance, guard
- Answer basis: traced. `Top.x` is a root parameter with no value, so it is undefined; the argument `cooled = x` resolves `x` to `Top.x` (`Case` has no member `x`). `Top.c.fan` reads its guard `cooled`, undefined: `presence-undetermined`. `present(self.c.fan)` is therefore `value undefined` (readings E29). `r`'s `if` condition is undefined, so no arm is taken and `r` is undefined. Had `present` given false, `r` would be 0 mm and a design whose fan is undecided would read as one with no fan.

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case { param cooled : Bool   sub fan : Fan where cooled }
structure def Top {
    param x : Bool
    sub c : Case { cooled = x }
    let p = present(self.c.fan)
    let r = if present(self.c.fan) then self.c.fan.d else 0mm
}
```

| Path | Expect |
|---|---|
| Top.c.fan | presence-undetermined |
| Top.p | value undefined |
| Top.r | value undefined |

### guard-reading-own-member-rejected

- Tests: A guard that directly reads the member it guards is rejected by the compiler.
- Rule: Part 4 ("A guard may read anything a cell may read, except what it guards")
- Provenance: design Part 4
- Needs: compile-error, guard
- Answer basis: stated: Part 4, "The compiler rejects the direct case, such as a guard on `fan` that reads `fan.flow`".
- Today: nothing rejects such a guard (design Part 4, seat report).

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case { sub fan : Fan where self.fan.d > 30mm }
```

| Path | Expect |
|---|---|
| (compile) | error: the guard of `fan` reads `fan` |

### guarded-sub-and-block-per-instance

- Tests: a guarded sub and a guarded block are present or absent per instance, the sub reads its argument, and the child definition's constraint is checked on the instance.
- Rule: contract Example 3; Part 4; Part 12 (Example 3); decision 7; rule D.
- Provenance: contract Part B Example 3; design Part 12 "Example 3". The wrapper `Top` is added so that both values of the guard appear in one file. contract Example 3 (Plate and a second root added); second-design Q2.
- Needs: instance, guard, constraint, summary, roots
- Answer basis: traced. `on`: `use_plate` true, so `thin` present with `t` = 4 mm, `4 mm ≥ 3 mm` satisfied; `n` = 2 > 1, so `big` = 20 and `20 > 5` satisfied. `off`: `use_plate` false, so `thin` and everything below it absent; `n` = 1, so the block's `big` and constraint absent. `Plate` and `Asm` are used by another definition of the file, so the one root is `Top` (rule D).
- Today: a guarded sub on a definition that is not recursive is never created; `Plate`'s constraint is checked once at the definition's default (contract Example 3, run).

```ri
structure def Plate { param t : Length = 5mm   constraint t >= 3mm }
structure def Asm {
    param use_plate : Bool = true
    param n : Int = 2
    sub thin : Plate where use_plate { t = 4mm }
    where n > 1 {
        let big = n * 10
        constraint big > 5
    }
}
structure def Top {
    sub on : Asm
    sub off : Asm { use_plate = false   n = 1 }
}
```

| Path | Expect |
|---|---|
| (file) | roots Top |
| Top.on.thin.t | value 4 mm |
| Top.on.thin/constraint[0] | verdict satisfied |
| Top.on.big | value 20 |
| Top.on/constraint[0] | verdict satisfied |
| Top.off.thin | absent |
| Top.off.thin.t | absent |
| Top.off.big | absent |
| Top.off/constraint[0] | absent |
| Top/summary | summary violated 0 undetermined 0 failed 0 |

The example's incremental claim (changing `n` from 2 to 3 reruns the guard and nothing that read only the guard) is the core's, not pinned.

### guard-reads-own-solved-unknown

- Tests: a guard may read a solved value.
- Rule: decision 12 ("A guard may read a solved value"); Part 4.
- Provenance: design decision 12 list; design Part 4. design Part 4.
- Needs: instance, guard, constraint, unknown, solve-forced
- Answer basis: traced. `t` ∈ [5 mm, 7 mm] for any solver, so `t > 4mm` holds and `rib` is present. The value of `t` is not forced, but every feasible value gives the pinned answers (forced in the sense of PRD 5.6), so only presence and `rib.h` are pinned. Under the candidates (readings E35): at 4 mm `constraint[0]` is violated and read the trial, so 4 mm is rejected (had it been taken, the guard would be false and `rib` absent); 6 mm violates nothing.
- Today: unknown for this model (the old binary violates a bound by a rounding residual in a pinched variant, `solvedguard.ri`, run).
- Candidates: Panel.t: 4 mm, 6 mm

```ri
structure def Rib { param h : Length = 2mm }
structure def Panel {
    param t : Length = auto
    constraint t >= 5mm
    constraint t <= 7mm
    sub rib : Rib where t > 4mm
}
```

| Path | Expect |
|---|---|
| Panel.rib | present |
| Panel.rib.h | value 2 mm |

### guard-on-port-honoured-per-instance

- Tests: a guard on a port is honoured per instance.
- Rule: Part 15.2 (ports, "A guard on a port is honoured, where today it is dropped"); Part 4.
- Provenance: design Part 15.2; design Part 4 (Today: guards silently discarded on a port).
- Needs: instance, guard, connect
- Answer basis: stated (Part 15.2). The compiler's handling of ports was not examined by the design, so this is a statement of the design, not a trace. Ports are leaf ο's (readings F, `connect`).
- Today: the guard on a port is silently discarded (design Part 4, report).

```ri
trait Flow {}
structure def Vent { param has_inlet : Bool = false   port inlet : in Flow where has_inlet {} }
structure def Asm {
    sub v : Vent
    sub w : Vent { has_inlet = true }
}
```

| Path | Expect |
|---|---|
| Asm.v.inlet | absent |
| Asm.w.inlet | present |

### recursion-guard-read-per-level

- Tests: a self-recursive guarded sub reads its guard per level and stops where it is false, and a self-recursive definition with no outside user is a default root.
- Rule: Part 4 (recursion is a guarded sub; a guard is read per instance); rule D; decision 2.
- Provenance: logic-1 L1, instance half (taken: member tables of a recursive definition form a loop, Part 2); logic-1 holds-up "Recursion at the level of instances"; logic-1 L11 (taken: rule D judges definitions that use one another as a group). design Part 13 row (task 10499); Part 4 recursion and names in the braces.
- Needs: instance, guard, constraint, summary, roots
- Answer basis: traced. RecTree is used only by itself, so its group has no outside user and it is a root (rule D). Root: depth 2 > 0, so `child` is present with depth 1, span 50 mm (`self` in the braces is the parent). Level 1: 1 > 0, so `child.child` is present with depth 0, span 25 mm. Level 2: 0 > 0 is false, so `child.child.child` is absent. Spans 100, 50, 25 mm all satisfy `span >= 10mm`.
- Today: run `rec.ri`: `RecTree.child.child.span = 0.025 m`, with "recursive structure cycle detected" as a warning.

```ri
structure def RecTree {
    param depth : Int = 2
    param span : Length = 100mm
    sub child : RecTree where depth > 0 {
        depth = self.depth - 1
        span = self.span / 2
    }
    constraint span >= 10mm
}
```

| Path | Expect |
|---|---|
| (file) | roots RecTree |
| RecTree.child.depth | value 1 |
| RecTree.child.child.depth | value 0 |
| RecTree.child.child.span | value 25 mm |
| RecTree.child.child.child | absent |
| RecTree/summary | summary violated 0 undetermined 0 failed 0 |

### undefined-guard-presence-counted-undetermined

- Tests: an undefined guard gives presence undetermined, a member list holding such a member is undefined, and the summary counts the presence with the undetermined verdicts.
- Rule: Part 4 (an undefined guard is not "false"); Part 5 ("If a member's presence is undetermined, the list is undefined"); decision 3c.
- Provenance: logic-1 L17 (taken: presence undetermined in place of a failure); logic-3 R19 (taken: the list rule restored in Part 5); logic-2 Part C on decision 3c (the root needed naming under draft 2's default-parameter bullet; draft 5 has no such bullet, so `Asm` is a default root). design decision 3c (its model). claims-3 probe d3c.ri; language-1 L12; language-2 B6.
- Needs: instance, guard, value, constraint, summary
- Answer basis: traced. `use_plate` has no default and `Asm` is a root, so it is undefined. `thin`'s guard is undefined: presence undetermined. `w` reads an undefined `if` condition: undefined; its constraint is undetermined. `self.children` holds a member of undetermined presence, so it is undefined (readings E30), and so is its count. `determined` of an undefined value is false. Summary (readings B4, E28): the constraint and `thin`'s presence; nothing is failed. `self.children` is leaf κ₁'s (`value`), not a collection's; `determined` is leaf ε₂'s, built with the first cells.

```ri
structure def Plate {
    param t : Length = 3mm
}
structure def Asm {
    param use_plate : Bool
    sub thin : Plate where use_plate
    let w = if use_plate then 4mm else 2mm
    constraint w > 1mm
    let n = self.children.count
    let known = determined(self.n)
}
```

| Path | Expect |
|---|---|
| Asm.thin | presence-undetermined |
| Asm.w | value undefined |
| Asm/constraint[0] | verdict undetermined |
| Asm.n | value undefined |
| Asm.known | value false |
| Asm/summary | summary violated 0 undetermined 2 failed 0 |

### read-below-undetermined-presence-gives-undefined

- Tests: a read below an instance whose presence is undetermined gives undefined, and the verdict that reads it is undetermined, not failed.
- Rule: Part 4 ("Reading something that is not there gives undefined", which covers "a member whose presence is undetermined ... and anything below"); decision 3; readings B1, B4, B5 and E28.
- Provenance: logic-2 N5 (taken in outcome: logic-3 Part A row N5 "Closed in outcome").
- Needs: instance, guard, constraint, summary
- Answer basis: traced. `cooled = x` is an argument; the bare `x` is not a member of `Case`, so it is `Top.x`, undefined at the root. `Top.c.fan`'s guard is undefined: presence undetermined. `Top.c.fan.flow` is below it, so the host asking for that path is answered `presence-undetermined` (readings B1); the reader `Top.fl` turns that into undefined, and that is pinned at `Top.fl`. `Top`'s constraint reads it: undetermined. `c`'s argument `x` is undefined and is not the literal `undef`, so `Top.c/arguments` is undetermined (readings B5). Summary (readings B4, E28): `Top/constraint[0]`, `Top.c/arguments`, and one for `fan`'s undetermined presence (its own constraint not counted): undetermined 3; `fl` is a cell with an undefined value, not a failure, and no verdict. `Top` is a root and is given no arguments, so `Top/arguments` is satisfied. The braces hold only an argument assignment, so the entry does not need `braces` (readings F).

```ri
structure def Fan {
    param flow : Length = 2mm
    constraint flow > 0mm
}
structure def Case {
    param cooled : Bool
    sub fan : Fan where cooled
}
structure def Top {
    param x : Bool
    sub c : Case {
        cooled = x
    }
    let fl = self.c.fan.flow
    constraint self.c.fan.flow > 1mm
}
```

| Path | Expect |
|---|---|
| Top.c.fan | presence-undetermined |
| Top.c.fan.flow | presence-undetermined |
| Top.fl | value undefined |
| Top/constraint[0] | verdict undetermined |
| Top.c/arguments | verdict undetermined |
| Top/summary | summary violated 0 undetermined 3 failed 0 |

### inner-guard-unread-under-false-outer

- Tests: an inner guard is never read under a false outer one, so an undefined inner guard under a false outer guard gives absent, not presence undetermined.
- Rule: Part 4 ("A nested guard reads the outer one first, so an inner condition is never evaluated under a false outer one").
- Provenance: logic-1 holds-up "Nested guards". design Part 4.
- Needs: instance, guard, summary
- Answer basis: stated (Part 4). `w`: outer `on` is false, so absent whatever `u` is. `w2`: outer `on2` is true and inner `u` is undefined, so presence undetermined, counted once.

```ri
structure def Asm {
    param on : Bool = false
    param on2 : Bool = true
    param u : Bool
    where on {
        where u {
            let w = 1mm
        }
    }
    where on2 {
        where u {
            let w2 = 1mm
        }
    }
}
```

| Path | Expect |
|---|---|
| Asm.w | absent |
| Asm.w2 | presence-undetermined |
| Asm/summary | summary violated 0 undetermined 1 failed 0 |

### guard-reads-another-instances-solved-value

- Tests: a guard may read a value another instance's solve decides, without a cycle.
- Rule: Part 4 ("A guard may read anything a cell may read, except what it guards. That includes a solved value"); decision 12.
- Provenance: logic-1 holds-up "A guard that reads another instance's solved value: no cycle".
- Needs: instance, guard, constraint, unknown, solve-forced
- Answer basis: traced. `a.t` is `a`'s unknown, forced to 4 mm by its two bounds. `extra`'s guard reads it after that solve: 4 mm > 3 mm, so `extra` is present. Nothing `a`'s solve reads depends on `extra`. Under the candidates (readings E35): at 3 mm `Asm.a/constraint[0]` (`t >= 4mm`) is violated and read the trial, so 3 mm is rejected; a solver that took it would make the guard false and `extra` absent. 4 mm violates nothing.
- Candidates: Asm.a.t: 3 mm, 4 mm

```ri
structure def Plate {
    param t : Length = auto
    constraint t >= 4mm
    constraint t <= 4mm
}
structure def P {
    param v : Length = 1mm
}
structure def Asm {
    sub a : Plate
    sub extra : P where self.a.t > 3mm
}
```

| Path | Expect |
|---|---|
| Asm.a.t | value 4 mm |
| Asm.extra | present |

### guard-reading-children-reported-as-cycle

- Tests: a guard that reads the member list containing what it guards compiles and is reported by the core as a cycle.
- Rule: Part 4 ("An indirect one, such as a guard that reads `self.children.count`, is reported by the core as a cycle").
- Provenance: logic-1 L19 (taken: Part 4 now says the indirect cases reach the core). design Part 4. language-1 L18.
- Needs: instance, guard, value, compile-error
- Answer basis: stated (Part 4). `extra`'s presence reads its guard; the guard reads `self.children`, which needs `extra`'s presence (readings E30: the list reads every member's presence). `a` and `b` are not on the ring. `self.children` is leaf κ₁'s (`value`), not a collection's.

```ri
structure def P {
    param v : Length = 1mm
}
structure def Asm {
    sub a : P
    sub b : P
    sub extra : P where self.children.count > 2
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.extra | failure cycle |
| Asm.a.v | value 1 mm |

### absent-member-read-across-instance-undefined

- Tests: reading an absent member across an instance compiles and gives undefined; `and`, `or` and `implies` read both operands, so a guarding disjunct gives the same verdict in either order; a constraint under the member's own guard is absent with it.
- Rule: decision 3b (lines 3 and 4); decision 12 (both operands read); Part 15 rule A; ruling 5; ruling 6; Part 4.
- Provenance: design decision 3b; language-1 L1 (its read 2, from another instance, with `{ needs_fan = true }`); language-2 B3; claims-3 consistency finding (the `or` line is satisfied only across an instance; inside `Case` it is entry unguarded-read-in-own-definition-is-undefined-where-absent, which since Leo's ruling of 2026-11-26 answers the same). Taken. Also, from the merged entries: design decision 3b.3. decision 3b.4; spec 9.2.3; logic-3 R2. logic-2 N7; logic-3 holds-up.
- Needs: instance, guard, eval-reads, constraint, summary, compile-error
- Answer basis: stated (decision 3b.3: "`f` is undefined"; 3b.4: "satisfied, whichever way round it is written"; Part 4: "is written `constraint self.fan.flow > x where needs_fan`"). Kleene: `true or undefined` is true, `false implies undefined` is true. `h` has its fan, 40 mm. Summary (readings B4, B5): four satisfied verdicts, a satisfied `/arguments` on every instance, `Top.c/constraint[0]` absent and not counted.
- Today: compile errors "unknown member 'fan' on sub 'c'" and "on sub 'h'" (run `cross_read.ri`).

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
    constraint self.fan.d > 30mm where needs_fan
}
structure def Top {
    sub c : Case
    sub h : Case { needs_fan = true }
    let f = self.c.fan.d
    let g = self.h.fan.d
    constraint not self.c.needs_fan or self.c.fan.d > 30mm
    constraint self.c.fan.d > 30mm or not self.c.needs_fan
    constraint self.c.needs_fan implies self.c.fan.d > 30mm
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Top.f | value undefined |
| Top.g | value 40 mm |
| Top/constraint[0] | verdict satisfied |
| Top/constraint[1] | verdict satisfied |
| Top/constraint[2] | verdict satisfied |
| Top.c/constraint[0] | absent |
| Top.h/constraint[0] | verdict satisfied |
| Top.c.fan | absent |
| Top/summary | summary violated 0 undetermined 0 failed 0 |

### unguarded-read-in-own-definition-is-undefined-where-absent

- Tests: inside one definition, a declaration with no guard of its own that reads a guarded member compiles; where the member is absent the read is undefined, a constraint guarded by an `or` is satisfied, and a constraint that reads it plainly is undetermined and not partial.
- Rule: decision 3b (reading something that is not there gives undefined); Leo's ruling of 2026-11-26 on the PRD's D13 (a read of a member that may be absent compiles wherever it is written), which amends decision 3b line 1 and Part 4; readings C15.
- Provenance: design decision 3b.1 and Part 4, which made this a compile error until Leo ruled; claims-3 probe `d3b.ri` and its consistency finding; language-1 L1. Also, from the merged entries: spec 6.3 and 8.10. logic-3 R3. tasks 9538 and 7489 (probe 2); critic defects-1. Renamed from `unguarded-read-in-own-definition-is-compile-error`, and the second constraint added, when he ruled.
- Needs: compile-error, instance, guard, constraint, summary
- Answer basis: traced. `needs_fan` is false, so `Case.fan` is absent. `f` reads `self.fan.d`, which is not there: undefined. The first constraint is `true or undefined`, which is true (spec 9.2.3). The second reads the same absent member with nothing beside it: undetermined, and the cause is a read of something not there, so other. `Fan` is used by `Case`, so `Case` is the root.
- Today: run `own_read.ri`: compiles with no diagnostic; `Case.f = undef`; `OK Case#constraint[0]`; `INDETERMINATE Case#constraint[1]: undefined inputs: Case.fan.d`.

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
    let f = self.fan.d
    constraint not needs_fan or self.fan.d > 30mm
    constraint self.fan.d > 30mm
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Case.fan | absent |
| Case.f | value undefined |
| Case/constraint[0] | verdict satisfied |
| Case/constraint[1] | undetermined-because other |
| Case/summary | summary violated 0 undetermined 1 failed 0 |

### guarded-reads-elsewhere-left-to-run-time

- Tests: a read of a guarded member inside an arm of an `if`, or from a declaration under some other guard, compiles; at run time it gives the arm's value or undefined.
- Rule: decision 3b (line 2); Part 4 ("Everything else is left to run time"); Part 15 rule A; ruling 5; ruling 6.
- Provenance: design decision 3b.2; language-1 L1 (its `if` idiom, probe `g1.ri`) and its "implies is not defined" point (probe `g3.ri`); language-2 B4 and probe `n4.ri`. Taken: the design dropped draft 2's compile error for a reader under another guard. Also, from the merged entries: decision 3b.2. logic-2 N7 (second half); logic-3 R4 (second half).
- Needs: instance, guard, eval-reads, compile-error
- Answer basis: traced. `fan` is absent (`needs_fan` false). `f`: the `if` condition is false, the else arm gives 0 mm. `c`: `n > 1` is true, the then arm reads `self.fan.d`, which is not there: undefined. `b` has a guard of its own (`n > 1`, true), so it compiles and is present, and reads undefined.
- Today: `Case.f = 0 m`, `Case.c = undef`, `Case.b = undef`, no diagnostic (run `arm_read.ri`).

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case {
    param needs_fan : Bool = false
    param n : Int = 2
    sub fan : Fan where needs_fan
    let f = if needs_fan then self.fan.d else 0mm
    let c = if n > 1 then self.fan.d else 0mm
    where n > 1 { let b = self.fan.d }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Case.f | value 0 mm |
| Case.c | value undefined |
| Case.b | value undefined |

### guard-decides-presence-for-sub-collection-constraint

- Tests: `where` decides presence for a plain sub, a list sub and a constraint alike: a true guard makes the member present, a false guard makes it absent (a guarded collection is absent, not empty), and an undefined guard makes its presence undetermined.
- Rule: decision 3 (3a, 3c); Part 4; contract Example 3
- Provenance: task 8414 (its own model, `thin` with `t = 1mm`); task 9029 (a guarded constraint under a true, false and undefined guard); task 8450 (its guarded-constraint half. Its geometry half is task 10611's); design Part 12, Example 3. design Part 13 row (task 8414). design Part 4 (a guarded collection is absent, not empty).
- Needs: instance, guard, collection, constraint
- Answer basis: traced.
  `on`: `use_plate` is true, so `thin` is present with `t` = 1 mm, and the guarded constraint is present and reads 1 mm, so it is violated. `use_list` is false, so `xs` is absent.
  `off`: `use_plate` is false, so `thin` and the guarded constraint are absent. `use_list` is true, so `xs` has two elements at their default of 100 mm.
  `unk`: `use_plate` is an explicit `undef`, so the presence of `thin` and of the guarded constraint is undetermined. `xs` is absent, because `use_list` keeps its default, false.
  `Top.n` reads the count of `Top.on.xs`, which is absent; reading what is not there gives undefined.
  The constraint `self.thin.t >= 2mm` is `Asm`'s first verdict-bearing constraint, so it is `constraint[0]`. The count constraint `xs.count == 2` is the source of the count cell and has no `constraint[k]` (readings A, last paragraph).
  A guarded constraint under an undefined guard answers `presence-undetermined`, like every guarded member (readings E28).
- Today: a guarded plain sub on a structure that is not recursive is never created, even with its guard true, and a guarded list sub is created whatever its guard (task 8414 record, release binary at 9eae7048a4).

```ri
structure def Plate { param t : Length = 5mm }
structure def Bar { param length : Length = 100mm }
structure def Asm {
    param use_plate : Bool = true
    param use_list : Bool = false
    sub thin : Plate where use_plate { t = 1mm }
    constraint self.thin.t >= 2mm where use_plate
    sub xs : List<Bar> where use_list
    constraint xs.count == 2 where use_list
}
structure def Top {
    sub on : Asm
    sub off : Asm { use_plate = false   use_list = true }
    sub unk : Asm { use_plate = undef }
    let n = self.on.xs.count
}
```

| Path | Expect |
|---|---|
| Top.on.thin | present |
| Top.on.thin.t | value 1 mm |
| Top.on/constraint[0] | verdict violated |
| Top.on.xs | absent |
| Top.off.thin | absent |
| Top.off/constraint[0] | absent |
| Top.off.xs[1].length | value 100 mm |
| Top.unk.thin | presence-undetermined |
| Top.unk/constraint[0] | presence-undetermined |
| Top.unk.xs | absent |
| Top.n | value undefined |

### child-guard-follows-instance-arguments

- Tests: a guard inside a child follows that instance's own arguments, not the definition's defaults; a guarded member read from the parent gives its value when present and undefined when absent; and a guarded `let` is computed from the instance's own parameters.
- Rule: Part 4 ("A guard is read per instance"; reading what is not there); decision 3 (3a, 3b line 3)
- Provenance: task 7992 (with its amendment of 2026-11-19, which retires the rule that a guarded member takes the template's value); design Part 13 (task 7992 row). design Part 4; Part 13 (tasks 7992, 8414). design Part 13 row (task 7992).
- Needs: instance, guard
- Answer basis: traced.
  `Test.s` is given `base` = 5 mm and keeps `on` = true, so `g` = 3 mm and `q` = 5 mm × 3 = 15 mm, computed from the instance's own `base` and not from the default of 2 mm.
  `Test.t` is given `on` = false, so its `g` is absent, although `Inner`'s default is true.
  `self.t.g` is a read across an instance, so it compiles (Part 4). It reads something that is not there, which gives undefined.
- Today: no `Test.s.g` cell exists, and `self.s.g` is rejected with "unknown member 'g' on sub 's'" (task 7992 record, debug binary at bc4ac07135). Since task 10015, a guarded member at an instance reads the template's value.

```ri
structure def Inner {
    param on : Bool = true
    param base : Length = 2mm
    where on {
        param g : Length = 3mm
        pub let q = base * 3
    }
}
structure def Test {
    sub s = Inner(base: 5mm)
    sub t = Inner(on: false)
    let gs = self.s.g
    let qs = self.s.q
    let gt = self.t.g
}
```

| Path | Expect |
|---|---|
| Test.s.g | value 3 mm |
| Test.s.q | value 15 mm |
| Test.t.g | absent |
| Test.gs | value 3 mm |
| Test.qs | value 15 mm |
| Test.gt | value undefined |

### recursion-unfolds-inside-instance-per-level

- Tests: a recursive guarded sub unfolds inside a sub instance, one level per guard; its plain subs exist at every level; the first level whose guard is false is absent; and each level's constraints are checked with that level's values.
- Rule: Part 4 ("Recursion needs no mechanism of its own"); decision 7; Part 13 (task 10499 row)
- Provenance: task 10499 (shapes a and b); task 9541 and task 9547 (their presence half: a grandchild whose guard is false is absent, so there is nothing to draw or export. Their export half is task 10611's)
- Needs: instance, guard, constraint, summary
- Answer basis: traced.
  `Root.r` has n = 3. Its guard 3 > 0 holds, so `child` exists with n = 2, then n = 1, then n = 0.
  The level with n = 0 has a false guard, so its `child` is absent.
  `tag` is a plain sub at every level, with k = 7.
  `n <= 1` is violated at n = 3 and n = 2 and satisfied at 1 and 0, so `Root/summary` counts 2 violations. Every level's arguments are defined, so no `/arguments` verdict is undetermined, and nothing fails (readings B4, B5). `Node` is used by `Root`, so `Root` is the only root.
- Today: no `Root.r.child` cells at any depth, only a generic recursion warning, and `check` prints OK for `Node#constraint[0]`. Plain subs below the root level of a recursion are not created (task 10499 record, release binary at 9eae7048a4).

```ri
structure def Tag { param k : Int = 7 }
structure def Node {
    param n : Int = 0
    constraint n <= 1
    sub tag : Tag
    sub child = Node(n: n - 1) where n > 0
}
structure def Root { sub r = Node(n: 3) }
```

| Path | Expect |
|---|---|
| Root.r.child.n | value 2 |
| Root.r.child.child.n | value 1 |
| Root.r.child.child.child.n | value 0 |
| Root.r.child.child.child.child | absent |
| Root.r.child.child.tag.k | value 7 |
| Root.r/constraint[0] | verdict violated |
| Root.r.child.child/constraint[0] | verdict satisfied |
| Root/summary | summary violated 2 undetermined 0 failed 0 |

### recursion-guard-reads-guarded-let

- Tests: a recursion guard that reads a `where`-guarded `let`, written inside the same guarded block, unfolds exactly as the same guard with a literal does, and the `let` is evaluated per level.
- Rule: Part 4 (a nested guard reads the outer one first; guards per instance; recursion); decision 3
- Provenance: task 10150
- Needs: instance, guard
- Answer basis: traced.
  At S, `on` is true, so `lim` = 0 and the guard 2 > 0 holds: `S.child` exists with depth 1.
  At `S.child`, `on` is true (its default), `lim` = 0 and 1 > 0, so `S.child.child` exists with depth 0.
  At `S.child.child`, 0 > 0 is false, so its `child` is absent.
  The guard reads `lim` and is nested inside `lim`'s own guard `on`, so it is never read where `lim` is absent (readings C15: a guard's condition is a declaration under the guards that enclose it, here `on`). `S` uses only itself, so it is the root.
- Today: with the record's form (the sub outside the `where` block, guarded `where depth > lim`), the recursion never unfolds and nothing is reported (task 10150 record, on task/10015). Probe `rec.ri` (run) prints `S.lim = 0` and no `S.child` cells.

```ri
structure def S {
    param depth : Int = 2
    param on : Bool = true
    where on {
        let lim = 0
        sub child = S(depth: depth - 1) where depth > lim
    }
}
```

| Path | Expect |
|---|---|
| S.child.depth | value 1 |
| S.child.child.depth | value 0 |
| S.child.child.lim | value 0 |
| S.child.child.child | absent |

### recursion-deeper-than-path-limit-fails-limit

- Tests: a guarded recursive sub made deeper than the depth setting allows gets the failure `limit` at the first path that is too deep, and what is above it is answered.
- Rule: Part 4 ("The limit on depth is the core's setting for the depth of a path"); contract, section on limits ("Exceeding one is a failure at that path"); readings B2 (`limit`); PRD D9(c)
- Provenance: design Part 4's recursion model, with a default deep enough to pass the setting; brief item 14.
- Needs: instance, guard
- Answer basis: traced, counting one step per sub level under the root: `Tree` is level 0, `Tree.left` 1, `Tree.left.left` 2, `Tree.left.left.left` 3, `Tree.left.left.left.left` 4. `depth` is 10, 9, 8, 7 down those levels, so every guard shown is true and recursion would go on. Levels 0 to 3 are answered; level 4 is too deep: `limit`. The rows are chosen so that counting every step of the path text after the root, member steps included (PRD D9(c)), gives the same answers: the three rows have 3, 3 and 4 steps.
- Note: not pinned, because the two counts differ there: the members of `Tree.left.left.left` (4 steps by the core's count) and every summary (a summary below the level-3 instance is one step deeper). Both wait on PRD D9(c).
- Settings: depth of a path = 3

```ri
structure def Tree {
    param depth : Int = 10
    sub left : Tree where depth > 0 {
        depth = self.depth - 1
    }
}
```

| Path | Expect |
|---|---|
| Tree.left.left.depth | value 8 |
| Tree.left.left.left | present |
| Tree.left.left.left.left | failure limit |


---

## One name under several guards (rule E)

### collection-and-sub-under-one-name-rejected

- Tests: A name declared as a collection under one guard and as a single sub under another is a compile error.
- Rule: rule E ("The declarations must give the same kind of node")
- Provenance: design Part 15.1 rule E; critic logic-3 R6. logic-3 R6. critic logic-1 (arms of different kinds).
- Needs: compile-error, guard, one-name, collection
- Answer basis: stated: rule E; readings C11 (same-name declarations under guards compile, except two that give different kinds of node, a collection and a single sub, which is a compile error).

```ri
structure def Big   { param w : Length = 5mm }
structure def Small { param w : Length = 2mm }
structure def Asm {
    param heavy : Bool = true
    sub s : List<Big> where heavy
    sub s : Small where not heavy
}
```

| Path | Expect |
|---|---|
| (compile) | error: `s` is declared as a collection and as a single sub |

### where-else-name-is-one-unguarded-member

- Tests: one name declared in both arms of a `where`/`else` is one member, readable without a guard, taking the arm whose guard holds, and undetermined in presence when the guard is undefined.
- Rule: rule E; Part 4 ("A name declared in every arm of a `where`/`else` ... is not a guarded member").
- Provenance: design Part 15.1 rule E; design Part 4; critic `language-2.md` B7(a) (report, not re-checked).
- Needs: instance, guard, one-name, compile-error
- Answer basis: traced. `on`: `c` true, first arm, 1 mm. `off`: `else` desugars to `where not c`, true, 2 mm, `r` = 4 mm. `unk`: both guards undefined, so presence undetermined and `r` reads something not there: undefined.
- Today: a warning "unguarded reference to guarded cell 'k'", and `r` undef in every instance (run, `e_else.ri`).

```ri
structure def S {
    param c : Bool
    where c { let k : Length = 1mm } else { let k : Length = 2mm }
    let r = k * 2
}
structure def Top {
    sub on : S { c = true }
    sub off : S { c = false }
    sub unk : S { c = undef }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Top.on.k | value 1 mm |
| Top.off.k | value 2 mm |
| Top.off.r | value 4 mm |
| Top.unk.k | presence-undetermined |
| Top.unk.r | value undefined |

### match-arm-subs-created-per-instance

- Tests: the arm of a declaration-level `match` whose case holds creates its sub, per instance, and a member that every arm's definition has reads from it.
- Rule: rule E; Part 4 (Today: "The arms of a `match` that declare subs are never created").
- Provenance: design Part 4; design Part 15.1 rule E; `crates/reify-eval/tests/fixtures/match_block_decls_bolt.ri` (same shape). logic-3 R4 (first half). language-1 L14 (second model); language-2 B7(d); claims-3 row 1.
- Needs: instance, guard, one-name, compile-error
- Answer basis: traced. The unguarded `let af = self.head.af` reads a name declared in every arm of a covering `match`, so it compiles (rule E; Part 4). `hex`: `head_type` Hex, so `head` is a `HexHead`, `af` = 10 mm. `sock`: `head` is a `SocketHead`, `af` = 8 mm, `depth` = 5 mm.
- Today: `head` is not created and `af` is undef in both instances (run, `match_arms.ri`).

```ri
enum HeadType { Hex, Socket }
structure def HexHead    { param af : Length = 10mm }
structure def SocketHead { param af : Length = 8mm   param depth : Length = 5mm }
structure def Bolt {
    param head_type : HeadType = HeadType.Hex
    match head_type {
        Hex => sub head : HexHead,
        Socket => sub head : SocketHead
    }
    let af = self.head.af
}
structure def Top {
    sub hex : Bolt
    sub sock : Bolt { head_type = HeadType.Socket }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Top.hex.head.af | value 10 mm |
| Top.hex.af | value 10 mm |
| Top.sock.af | value 8 mm |
| Top.sock.head.depth | value 5 mm |

### name-under-overlapping-guards-all-cases

- Tests: one name under several guards compiles, and is present only when one guard is true and every other is false; two true is a failure; all false is absent; anything else is presence undetermined.
- Rule: rule E (first and second bullets); decision 12 ("More than one declaration of a name present at once is a failure"); readings C11, B2, B4 and E28.
- Provenance: logic-3 R6 (one true and one undefined gave the true arm under draft 3; both undefined gave "absent"; taken: rule E); logic-1 doubts (one name under exclusive guards). design rule E; logic-3 R6. language-2 B7(c); logic-3 R6. language-1 L14 (first model); language-2 probe n3.ri.
- Needs: instance, guard, one-name, compile-error, summary
- Answer basis: traced. `a`: one true, one undefined: undetermined. `b`: `u` and `not u` both undefined: undetermined. `c`: two true: `failure ambiguous-declaration` (readings B2). `d`: both false: absent. `e`: one true, one false: present with `v` 1 mm. Summary (readings B4, E28): two undetermined presences, never counted as failed, and one failure. The compile row is readings C11: same-name declarations under guards compile whether or not the guards can be proved exclusive (spec 6.4's restriction to provably exclusive guards is to be amended); all five pairs give the same kind of node. Today's compiler accepts them too (run).
- Today: run `dupguard.ri`: two guarded `sub a` compile; no `a` is printed (a guarded sub that is not recursive is never created, task 8414).

```ri
structure def P {
    param v : Length = 1mm
}
structure def Asm {
    param yes : Bool = true
    param no : Bool = false
    param u : Bool
    sub a : P where yes {
        v = 1mm
    }
    sub a : P where u {
        v = 2mm
    }
    sub b : P where u {
        v = 1mm
    }
    sub b : P where not u {
        v = 2mm
    }
    sub c : P where yes {
        v = 1mm
    }
    sub c : P where yes {
        v = 2mm
    }
    sub d : P where no {
        v = 1mm
    }
    sub d : P where no {
        v = 2mm
    }
    sub e : P where yes {
        v = 1mm
    }
    sub e : P where no {
        v = 2mm
    }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.a | presence-undetermined |
| Asm.b | presence-undetermined |
| Asm.c | failure ambiguous-declaration |
| Asm.d | absent |
| Asm.e.v | value 1 mm |
| Asm/summary | summary violated 0 undetermined 2 failed 1 |

### let-and-sub-under-one-name-rejected

- Tests: a `let` and a `sub` declared under one name is a compile error.
- Rule: rule E (third bullet: "The declarations must give the same kind of node"); readings C11 (the exception: two declarations that give different kinds of node are a compile error).
- Provenance: logic-1 doubts ("One name under exclusive guards, with arms of different kinds (a `let` in one arm, a `sub` in the other). They cannot be one node with one recipe").
- Needs: compile-error, guard, one-name
- Answer basis: traced. A `let` is a cell and a `sub` an instance: different kinds of node, so rule E's third bullet rejects it. The bullet's only example is a collection against a single sub.

```ri
structure def P {
    param v : Length = 1mm
}
structure def Asm {
    param use : Bool = true
    sub p : P where use
    let p = 5mm where not use
}
```

| Path | Expect |
|---|---|
| (compile) | error: `p` is declared as a sub and as a let; declarations of one name must give the same kind of member |

---

### nested-where-else-arms-give-one-member

- Tests: one name declared under several guards takes the declaration whose guard is true when every other is false, through nested `where`/`else` and through the spec's desugared pair.
- Rule: rule E; Part 4; readings C11.
- Provenance: language-2 B7 (a), (b), (c) and its probes `n1.ri`, `n2.ri`; claims-3 row 1; language-1 L14. Taken (rule E replaces draft 1's "in order").
- Needs: instance, guard, one-name, constraint, compile-error
- Answer basis: traced. `K1`: `a` false, `b` true; the outer `where`/`else` has `k` in both arms, the inner one has `k` in both arms, so `k` is not a guarded member (applying Part 4's sentence at each level; the design does not say "arms nest" in words) and the constraint may read it: 2 mm, satisfied. `K2`: guard `n < 3` false, `n >= 3` true: 2 mm; the two declarations compile (readings C11). `K2` has no reader of `k`; a reader is entry `separate-wheres-read-without-guard`.
- Today: `K1.k = undef` and `INDETERMINATE K1#constraint[0]`; `K2.k = 2 mm` (run `alt_exclusive.ri`).

```ri
structure def K1 {
    param a : Bool = false
    param b : Bool = true
    where a { let k = 1mm } else { where b { let k = 2mm } else { let k = 3mm } }
    constraint k > 0mm
}
structure def K2 {
    param n : Int = 3
    let k = 1mm where n < 3
    let k = 2mm where n >= 3
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| K1.k | value 2 mm |
| K1/constraint[0] | verdict satisfied |
| K2.k | value 2 mm |

### separate-wheres-read-without-guard

- Tests: a name declared under separate `where` guards (not the arms of one `where`/`else`) is read by a declaration with no guard like any member: rule E chooses the declaration whose guard is true.
- Rule: rule E; readings C11; Leo's ruling of 2026-11-26 on the PRD's D13 (a read of a member that may be absent compiles wherever it is written), which amends decision 3b line 1 and Part 4.
- Provenance: language-2 B7 (c) (the spec's desugared form, which compiles and evaluates today). Until Leo ruled, the design's narrower exception made this read a compile error although `k` is there for every `n`. Renamed from `separate-wheres-leave-name-guarded`.
- Needs: compile-error, instance, guard, one-name
- Answer basis: traced. `n` is 3: the guard `n < 3` is false and `n >= 3` is true, so `k` is present with the second declaration, 2 mm (rule E: present with the declaration whose guard is true when every other guard is false). `r` is 4 mm.
- Today: compiles with "warning: unguarded reference to guarded cell 'k'"; `K.r = 4 mm` (run `alt_separate_read.ri`).

```ri
structure def K {
    param n : Int = 3
    let k = 1mm where n < 3
    let k = 2mm where n >= 3
    let r = k * 2
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| K.k | value 2 mm |
| K.r | value 4 mm |

### arm-only-member-read-across-instance

- Tests: what a read of a member that only one arm's definition has gives, from outside the instance that holds the `match`.
- Rule: rule E (last bullet); spec 6.4 (narrowing); readings C12.
- Provenance: language-1 L14 (`let d = self.head.socket_depth`); language-2 B7 (d). The design gives the run-time answer and hands the compile-time one to the spec's narrowing. Also, from the merged entries: design rule E; logic-3 R6. logic-3 R6.
- Needs: instance, guard, one-name, compile-error
- Answer basis: traced. Readings C12: a read from another instance of a member only some arms' definitions have compiles. At run time `s` is Socket, its `head` is a `SocketHead`: `ds` is 4 mm. `h` is Hex, and `HexHead` has no `socket_depth`: `dh` is undefined (rule E, last bullet).
- Today: compile errors "unknown member 'head'" (run `match_arms.ri`).

```ri
enum HeadType { Hex, Socket }
structure def HexHead    { param across : Length = 10mm }
structure def SocketHead { param across : Length = 8mm   param socket_depth : Length = 4mm }
structure def Bolt {
    param head_type : HeadType = HeadType.Hex
    match head_type {
        Hex => sub head : HexHead,
        Socket => sub head : SocketHead
    }
}
structure def Pair {
    sub h : Bolt
    sub s : Bolt { head_type = HeadType.Socket }
    let ds = self.s.head.socket_depth
    let dh = self.h.head.socket_depth
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Pair.ds | value 4 mm |
| Pair.dh | value undefined |

### unguarded-and-guarded-declaration-of-one-name

- Tests: a name declared once without a guard and once under a guard is the unguarded declaration while the guard is false, a failure naming both while the guard is true, and of undetermined presence while the guard is undefined.
- Rule: rule E; decision 12 ("More than one declaration of a name present at once is a failure"); readings C11, B2 and E28
- Provenance: task 10049 (its shape: a top-level `param body : Solid` shadowed by a guarded `param body : Solid`, here with a `Length` in place of the solid)
- Needs: instance, guard, one-name, compile-error
- Answer basis: traced.
  Compile: readings C11 says same-name declarations under guards compile whether or not the guards are exclusive (spec 6.4 to be amended); both are parameters, so `(compile)` is ok.
  Reading (not stated by rule E): an unguarded declaration is treated as one whose guard is always true. Rule E speaks of declarations under guards; this entry applies it to that reading.
  `off`: one declaration present, the other absent, so the unguarded one applies: 3 mm.
  `on`: two declarations present at once, so `failure ambiguous-declaration` (readings B2).
  `unk`: one present and one of undetermined presence, so presence undetermined.
- Today: in the `Solid` form it compiles with no diagnostic, and three resolvers choose different realizations (task 10049 record).

```ri
structure def S {
    param big : Bool = false
    param w : Length = 3mm
    where big { param w : Length = 5mm }
}
structure def Holder {
    sub off : S
    sub on : S { big = true }
    sub unk : S { big = undef }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Holder.off.w | value 3 mm |
| Holder.on.w | failure ambiguous-declaration |
| Holder.unk.w | presence-undetermined |

### one-name-with-failed-guard-is-blocked

- Tests: when one of the guards on a name declared twice is itself a failure, the name is blocked by that failure, not of undetermined presence, and the summary counts it as failed.
- Rule: readings E28 ("A guard that itself failed blocks the member"); readings C14 (a ring of `let`s is a run-time cycle); readings B2 and B4; rule E.
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N3 (rule E's "otherwise" against PRD 5.4's blocking rule); readings E28, which answers it: rule E's "otherwise" covers only an undefined guard.
- Needs: instance, guard, one-name, summary
- Answer basis: traced. `r1` and `r2` form a ring of `let`s in one definition: it compiles and each is `failure cycle` (readings C14). The second guard, `r1 > 0`, reads a failure, so its cell is `failure blocked` (readings B2). The first guard is true. A guard that failed blocks the member (readings E28), so the one node for `s` is `failure blocked`; it is not `presence-undetermined`, which rule E's text alone would give. Summary (readings B4): `r1`, `r2`, the second guard's cell and `s` are failures: failed 4. Nothing inside `s` is counted, and there is no verdict. The reading that treated the failed guard as undefined would give `undetermined 1 failed 3`.

```ri
structure def P { param v : Length = 1mm }
structure def Asm {
    param yes : Bool = true
    let r1 = r2 + 1
    let r2 = r1 + 1
    sub s : P where yes
    sub s : P where r1 > 0
}
```

| Path | Expect |
|---|---|
| Asm.r1 | failure cycle |
| Asm.s | failure blocked |
| Asm/summary | summary violated 0 undetermined 0 failed 4 |


---

## Collections

### bare-collection-reads-as-list-of-values

- Tests: A count may read a member of a sibling sub, and a bare reference to the collection is the list of its elements as values.
- Rule: Part 5 ("A count may read anything except its own elements"; "A bare collection is a list of its elements as values")
- Provenance: design Part 5; Part 13 (task 6604)
- Needs: collection, instance, value
- Answer basis: stated: Part 5 and Part 13.
- Today: a count that reads a sibling sub gives no elements (task 6604, design Part 13); a bare reference to a collection gives its first member's values (design Part 5).

```ri
structure def Spec { param n : Int = 3 }
structure def Vent { param w : Length = 2mm }
structure def Asm  { sub s : Spec   sub vents : List<Vent>   constraint vents.count == self.s.n   let k = vents }
```

| Path | Expect |
|---|---|
| Asm.vents[2] | present |
| Asm.vents[3] | absent |
| Asm.k | value [Vent{w: 2 mm}, Vent{w: 2 mm}, Vent{w: 2 mm}] |

### count-reading-own-elements-is-compile-error

- Tests: A count that reads its own elements is a compile error whose message says to read a prototype value.
- Rule: Part 5; decision 10
- Provenance: design Part 5; Part 14 decision 10 (model). decision 10's model, reduced to an Int. language-1 L13; language-2 B8.
- Needs: compile-error, collection
- Answer basis: stated: decision 10.
- Today: undefined, silently (design decision 10).

```ri
structure def Bolt { param rated_load : Force = 10kN }
structure def Rack {
    param load : Force = 35kN
    sub bolts : List<Bolt>
    constraint bolts.count == ceil(load / bolts[0].rated_load)
}
```

| Path | Expect |
|---|---|
| (compile) | error: the count of `bolts` reads its own elements; write `let proto = Bolt()` and read `proto.rated_load` |

### forall-block-gives-one-verdict-per-element

- Tests: A `forall` block that declares constraints gives one verdict per element, also when the count is an expression, while a quantifier inside an expression gives one verdict.
- Rule: Part 5; decision 12 (`forall`, ruled "Your rec LGTM")
- Provenance: design Part 5; Part 14 decision 12. examples/keyed_forall.ri (keyed form). language-1 L21.
- Needs: instance, collection, constraint, summary
- Answer basis: traced. Count 2 * 2 = 4 elements, each w = 2 mm. The count constraint is not a verdict and takes no position (decisions A), so the block is `constraint[0]` and the expression form `constraint[1]`. The block gives four verdicts `2mm > 5mm`, `constraint[0][0]` to `[0][3]`, all violated; the expression form gives one, violated. Five in all.
- Today: `count == n*2` gives no per-element verdicts (design Part 5).

```ri
structure def Vent { param w : Length = 2mm }
structure def Asm {
    param n : Int = 2
    sub vents : List<Vent>
    constraint vents.count == n * 2
    forall v in vents : constraint v.w > 5mm
    constraint forall v in vents: v.w > 5mm
}
```

| Path | Expect |
|---|---|
| Asm.vents[3] | present |
| Asm.vents[4] | absent |
| Asm/constraint[0][0] | verdict violated |
| Asm/constraint[0][3] | verdict violated |
| Asm/constraint[1] | verdict violated |
| Asm/summary | summary violated 5 undetermined 0 failed 0 |

### index-past-end-reads-undefined

- Tests: An index past the end of a collection reads something that is not there: undefined, a verdict over it is undetermined, and a guarded read gives the same verdict in either operand order.
- Rule: decision 3 (3b, line 5); Part 5 ("`xs[i]` with a computed `i`"); decision 12 (both operands are read)
- Provenance: design Part 14 decision 3b.5; Part 5 (`i < xs.count and xs[i].area > 0mm^2`); departs from spec 3.4. language-1 L20 and L1; language-2 B3.
- Needs: instance, collection, constraint, summary, eval-reads
- Answer basis: traced. The count constraint is not a verdict and takes no position (decisions A), so the three verdicts are `constraint[0]` to `[2]`. `x` and constraint[0] read `vents[7]`, not there: undefined, undetermined. Constraints [1] and [2]: `7 < 3` is false, and Kleene `false and undefined` is false in either order, so both are violated (not a failure, which the spec's 3.4 would give). `vents[2]` is the last element, at the default 2 mm; `vents[3]` is past the end.
- Today: undefined with no message (design Part 5).

```ri
structure def Vent { param w : Length = 2mm }
structure def Asm {
    sub vents : List<Vent>
    constraint vents.count == 3
    let x = vents[7].w
    constraint vents[7].w > 1mm
    constraint 7 < vents.count and vents[7].w > 1mm
    constraint vents[7].w > 1mm and 7 < vents.count
}
```

| Path | Expect |
|---|---|
| Asm.vents[2].w | value 2 mm |
| Asm.vents[3] | absent |
| Asm.x | value undefined |
| Asm/constraint[0] | verdict undetermined |
| Asm/constraint[1] | verdict violated |
| Asm/constraint[2] | verdict violated |
| Asm/summary | summary violated 2 undetermined 1 failed 0 |

### children-lists-present-subs-only

- Tests: `self.children` lists present subs only, not values held in cells, and is undefined when a sub's presence is undetermined.
- Rule: Part 5 ("A bare collection is a list of its elements as values"); decision 11; Part 7.4
- Provenance: design Part 5; Part 14 decision 11; Part 7.4 (a value is not listed by `self.children`)
- Needs: instance, guard, value
- Answer basis: stated: Part 5, "list present members only ... If a member's presence is undetermined, the list is undefined" (decisions E30). `x`: `f1` present, `f2` absent, `v` a value: 1. `y`: `f2` presence undetermined (decisions E28), so the list is undefined and its reader `n` is pinned undefined at its own path. No collection sub is declared, so the entry does not need the collections leaf.
- Today: these are lists of path strings that include absent subs (design decision 11).

```ri
structure def Fan  { param d : Length = 40mm }
structure def Case {
    param a : Bool = true
    param b : Bool = false
    sub f1 : Fan where a
    sub f2 : Fan where b
    let v = Fan()
    let n = self.children.count
}
structure def Top { sub x : Case   sub y : Case { b = undef } }
```

| Path | Expect |
|---|---|
| Top.x.n | value 1 |
| Top.y.f2 | presence-undetermined |
| Top.y.n | value undefined |

---

### one-unknown-per-keyed-entry

- Tests: `auto` written for each entry of a keyed collection gives one unknown per entry, owned by the declaring instance.
- Rule: decision 5 (for a collection's elements); Part 6.3 table (keyed entry); Part 5 (keys carry their arguments); path text for keys
- Provenance: design Part 14 decision 5 (model in words: "Per element: 6 and 3. One shared unknown: infeasible"), written here with keys because the list form waits for task 5482; Part 6.3 table
- Needs: instance, collection, unknown, solve-forced
- Answer basis: stated: decision 5, 6 and 3 per element. Trace: a = 2b and a + b = 9 mm give b = 3 mm, a = 6 mm, the only solution. Both unknowns are `Manifold`'s, the writer's (decision 5), and the two constraints are `Manifold`'s own, so they bear. The entry pins the parameters, not the unknowns' path text (that is `keyed-entry-autos-have-distinct-paths`).
- Today: both entries undefined and both constraints INDETERMINATE (run, `p3.ri`).
- Candidates: `Manifold/auto[vents["a"].w]`: 1 mm, 3 mm, 6 mm; `Manifold/auto[vents["b"].w]`: 1 mm, 6 mm, 3 mm. A reading that left the entries at `Vent`'s default would take 1 mm; one shared unknown has no feasible value. Only 6 mm with 3 mm satisfies both constraints, whatever order the pairs are tried in.

```ri
structure def Vent { param w : Length = 1mm }
structure def Manifold {
    sub vents : Keyed<Vent> {
        "a" => { w = auto }
        "b" => { w = auto }
    }
    constraint vents["a"].w == 2 * vents["b"].w
    constraint vents["a"].w + vents["b"].w == 9mm
}
```

| Path | Expect |
|---|---|
| Manifold.vents["a"].w | value 6 mm |
| Manifold.vents["b"].w | value 3 mm |

---

### solved-count-creates-elements-after-solve

- Tests: a collection whose count is a solved unknown declares that many elements, and a read of element 0 finds it.
- Rule: contract Example 4; Part 12 (Example 4); Part 13 row 4 (the solved count).
- Provenance: contract Part B Example 4; design Part 12 "Example 4"; design Part 13 row "A count that reads a sibling sub gives no elements (6604); a solved count gives none". `Vent` is added; the contract does not define it, and its default of 10 mm is what gives the contract's `a0` = 10 mm. design Part 5; Part 12 Example 4. second-design trace Example 4.
- Needs: instance, collection, unknown, solve-forced
- Answer basis: traced. `n ≥ 4` and `n ≤ 4` force `n` = 4. The count constraint is the count cell's source and bears on nothing (decisions A). The collection reads its count cell, which reads `n`: four elements, `[0]` to `[3]`. `[4]` is not declared, so absent. `a0` = `vents[0].area` = 10 mm. Every feasible value gives these answers and no objective is written, so the entry is forced, not `solve-real` (trace-structure, wrong tags).
- Today: the count is solved and no element is created; `a0` = undef (contract Example 4, run).
- Candidates: `Manifold.n`: 3, 5, 4. A reading in which the two bounds did not bear would take 3. Under 3 the first bound, and under 5 the second, is violated and depended on the trial; under 4 both are satisfied.

```ri
structure def Vent { param area : Length = 10mm }
structure def Manifold {
    param n : Int = auto
    sub vents : List<Vent>
    constraint n >= 4
    constraint n <= 4
    constraint vents.count == n
    let a0 = vents[0].area
}
```

| Path | Expect |
|---|---|
| Manifold.n | value 4 |
| Manifold.vents[3] | present |
| Manifold.vents[4] | absent |
| Manifold.a0 | value 10 mm |

### count-reads-sibling-sub-or-value

- Tests: a collection's count may read a sibling sub's member or a value made in a cell, and a read past the end is undefined, not a failure.
- Rule: Part 13 row 4; Part 5; decision 10 (the way out it names); decision 3 (an index out of range gives undefined).
- Provenance: design Part 13 row "A count that reads a sibling sub gives no elements (6604)"; design decision 10 (`let proto = Bolt()` and read `proto.rated_load`). task 6604. language-1 L13 (the way out).
- Needs: instance, collection, made, open
- Answer basis: traced. `spec.n` = 3: elements `[0]` to `[2]`, each with `area` 10 mm; `[3]` absent, and its reader `a3` is undefined (decisions B1). `Bolt()` makes `Asm.proto/made[0]`, and `proto.rated` opens its value: 3, so `bolts` has 4 elements.
- Today: the bolts are created; the vents are not, and `a2` = undef (run, `count_sib.ri`). The spec's 3.4 calls an out-of-bounds read a failure.

```ri
structure def Spec { param n : Int = 3 }
structure def Vent { param area : Length = 10mm }
structure def Bolt { param rated : Int = 3 }
structure def Asm {
    sub spec : Spec
    sub vents : List<Vent>
    constraint vents.count == spec.n
    let proto = Bolt()
    sub bolts : List<Bolt>
    constraint bolts.count == proto.rated + 1
    let a2 = vents[2].area
    let a3 = vents[3].area
}
```

| Path | Expect |
|---|---|
| Asm.vents[2].area | value 10 mm |
| Asm.vents[3] | absent |
| Asm.a2 | value 10 mm |
| Asm.a3 | value undefined |
| Asm.bolts[3] | present |
| Asm.bolts[4] | absent |

### undefined-count-makes-elements-presence-undetermined

- Tests: an undefined count makes the collection's elements undetermined, and a read of one is undefined.
- Rule: decision 12 ("An undefined count makes a collection's elements undetermined"); Part 5.
- Provenance: design decision 12 list; design Part 5. design Part 5.
- Needs: instance, collection, summary
- Answer basis: traced. `n` has no default and `Asm` is the root, so `n` is undefined; the count is undefined, so the collection is `presence-undetermined` (decisions E28), and so is every path below it (decisions B1); its reader `a0` is undefined. The summary counts the collection once as undetermined and nothing inside it (decisions B4, E28). The count constraint gives no verdict.
- Today: the count cell is undef, `a0` undef, with a note "Asm.n unbound" (run, `undefcount.ri`).

```ri
structure def Vent { param area : Length = 10mm }
structure def Asm {
    param n : Int
    sub vents : List<Vent>
    constraint vents.count == n
    let a0 = vents[0].area
}
```

| Path | Expect |
|---|---|
| Asm.vents | presence-undetermined |
| Asm.vents[0] | presence-undetermined |
| Asm.a0 | value undefined |
| Asm/summary | summary violated 0 undetermined 1 failed 0 |

### undefined-count-under-forall-block

- Tests: a `forall` block over a collection whose count is undefined is itself of undetermined presence, and the summary counts the collection and the block once each.
- Rule: decisions E28 ("A collection whose count is undefined is `presence-undetermined`, and so is a `forall` block over it; each counts once"); decisions B4; Part 5
- Provenance: trace-structure counter-example 6.
- Needs: instance, collection, constraint, summary
- Answer basis: traced. `n` is undefined at the root, so the count is undefined and `vents` is `presence-undetermined` (decisions E28). The count constraint gives no verdict and takes no position (decisions A), so the `forall` block is `constraint[0]`; it ranges over `vents`, so it is `presence-undetermined` too, with no per-element verdicts below it. Summary: the collection and the block, undetermined 2; nothing inside either is counted.
- Today: not probed.

```ri
structure def Vent { param w : Length = 2mm }
structure def Asm {
    param n : Int
    sub vents : List<Vent>
    constraint vents.count == n
    forall v in vents : constraint v.w > 1mm
}
```

| Path | Expect |
|---|---|
| Asm.vents | presence-undetermined |
| Asm/constraint[0] | presence-undetermined |
| Asm/summary | summary violated 0 undetermined 2 failed 0 |

### keyed-entries-have-own-unknowns

- Tests: each key of a keyed collection has its own arguments, so the set of unknowns differs per element.
- Rule: Part 5 ("each key's entry in the member table carries that key's arguments"); Part 6.2.
- Provenance: logic-1 L3 (taken: Part 5); logic-2 Part A row L3 (closed).
- Needs: instance, collection, constraint, summary, unknown, solve-forced
- Answer basis: traced. Element `"a"` is given `area`, so `gap` is its unknown and `gap == area / 2` forces 3 mm. Element `"b"` is given `gap`, so `area` is its unknown, forced to 4 mm. Each unknown is the element's own (`Vent`'s text holds the `auto`), and so is the constraint, which bears. One member table for both elements would give one of them the wrong unknowns (L1's two failure traces).
- Today: run `keyed.ri`: both elements print as `Vent { }`; the definition-level solve fails "infeasible".
- Candidates: `Manifold.vents["a"].gap`: 6 mm, 3 mm; `Manifold.vents["b"].area`: 2 mm, 4 mm. A reading in which the constraint did not bear would take 6 mm and 2 mm. Under each the constraint is violated and depended on the trial; under 3 mm and 4 mm it is satisfied.

```ri
structure def Vent {
    param area : Length = auto
    param gap : Length = auto
    constraint gap == area / 2
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "a" => { area = 6mm }
        "b" => { gap = 2mm }
    }
}
```

| Path | Expect |
|---|---|
| Manifold.vents["a"].gap | value 3 mm |
| Manifold.vents["b"].area | value 4 mm |
| Manifold/summary | summary violated 0 undetermined 0 failed 0 |

### counts-reading-each-others-elements-cycle

- Tests: two collections whose counts each read the other's element compile and are reported by the core as a cycle.
- Rule: Part 5 ("A count may read anything except its own elements"; only the direct case is a compile error); decision 10; decisions A (a count constraint is the source of the count cell `/count`) and B2.
- Provenance: logic-1 L19 (taken in Part 4's wording for guards; Part 5 states only the direct case).
- Needs: compile-error, instance, collection
- Answer basis: traced. `xs`'s count cell reads `ys[0]`, which needs `ys`'s count cell, which reads `xs[0]`, which needs `xs`'s count cell. Neither count reads its own elements, so the compile rule (leaf ι) does not fire. The failure is at the count cells (decisions B2). Both constraints are count constraints, so neither has a `constraint[k]` path. The core's cycle detection (core task 10558) is a prerequisite of leaf ε₁, so it is built before this entry runs.

```ri
structure def Bolt {
    param rated : Int = 2
}
structure def Asm {
    sub xs : List<Bolt>
    sub ys : List<Bolt>
    constraint xs.count == ys[0].rated
    constraint ys.count == xs[0].rated
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm/count[xs] | failure cycle |
| Asm/count[ys] | failure cycle |

---

### forall-constraints-bear-on-owner-auto

- Tests: the per-element constraints declared by a `forall` statement are members inside the owning instance, so they bear on its `auto` on every evaluation, with no population added only on the edit path.
- Rule: Part 5 (a `forall` block gives one constraint per element); Part 6.1; decision 12 (settled: the statement form declares one constraint per element)
- Provenance: task 9420 (its runtime `forall` half: cold evaluation never emits `forall` constraints for a collection whose count is not literal)
- Needs: instance, collection, constraint, summary, unknown, solve-forced
- Answer basis: traced.
  `bs` has two elements, each with `length` = 100 mm.
  The `forall` declares one constraint per element, `bs[i].length == t`. Both are inside `Asm`, which owns `t`, so both bear on it and t = 100 mm.
  If they did not bear, `t` would have no constraint at all. Both per-element verdicts are satisfied.
  The count constraint `bs.count == 2` has no `constraint[k]` (decisions A, last paragraph), so the `forall` block is `constraint[0]` and its elements are `/constraint[0][0]` and `/constraint[0][1]` (section A).
- Today: see task 9420's record (cold evaluation omits the runtime `forall` constraints that the edit path admits).
- Candidates: `Asm.t`: 50 mm, 100 mm. A reading in which the per-element constraints did not bear would take 50 mm, since `t` would then have no constraint. Under 50 mm both per-element verdicts are violated and depended on the trial; under 100 mm both are satisfied.

```ri
structure def Bar { param length : Length = 100mm }
structure def Asm {
    param t : Length = auto
    sub bs : List<Bar>
    constraint bs.count == 2
    forall b in bs: constraint b.length == t
}
```

| Path | Expect |
|---|---|
| Asm.t | value 100 mm |
| Asm/constraint[0][0] | verdict satisfied |
| Asm/constraint[0][1] | verdict satisfied |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### keyed-forall-verdicts-addressed-by-key

- Tests: a `forall` block over a keyed collection gives one verdict per entry, addressed by the entry's key.
- Rule: rule F (a per-element constraint of a `forall`); Part 5 (a `forall` block gives one constraint per element); decision 10 (an element's identity is its position or its key); decision 12 (`forall`, "Your rec LGTM")
- Provenance: `examples/keyed_forall.ri` (its form, with one entry made to fail); appendix A row "A constraint declared by a `forall` block".
- Needs: instance, collection, constraint, summary, paths
- Answer basis: traced. A keyed collection has no count constraint, so the block is `constraint[0]`, and its per-entry constraints are `/constraint[0]["key"]` (decisions A). `"intake"`: 6 mm > 4 mm, satisfied. `"exhaust"`: 3 mm > 4 mm, violated. Violated 1.
- Today: run `g15.ri`: `OK forall@v[0]`, `VIOLATED forall@v[1]` (positional ids, no key).

```ri
structure def Vent {
    param w : Length = 2mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { w = 6mm }
        "exhaust" => { w = 3mm }
    }
    forall v in vents: constraint v.w > 4mm
}
```

| Path | Expect |
|---|---|
| Manifold/constraint[0]["intake"] | verdict satisfied |
| Manifold/constraint[0]["exhaust"] | verdict violated |
| Manifold/summary | summary violated 1 undetermined 0 failed 0 |


---

## Structure values

### value-carries-type-parameters-and-decided-values

- Tests: A value carries its type's name, its parameters and every value decided at or below its instance, so a given and a solved parameter of equal value give equal values, and equal fields under two type names do not.
- Rule: decision 6; Part 7.1; decision 11; ruling 7 of `seat-context.md`
- Provenance: design Part 14 decision 6 (model); Part 7.1; contract Example 11 (`Plate{t: 3 mm}`). language-1 L21; claims-1 row 23; second-design Q5. logic-2 N12.
- Needs: instance, value, made, open, unknown, solve-real
- Answer basis: stated: decision 6 ("`Plate(t: 3mm)` equals a `Plate()` whose `t` was solved to 3 mm, and `A()` does not equal `B()`"). `set{c, d}` has one member because equal values are one member (Part 7.4). The nested rows: `o`'s value carries `inner.t`, so the opened value agrees with the made instance. Because this solve is deterministic, these two rows cannot tell the ruled rule from a value that carries only its own parameters.

```ri
structure def Plate { param t : Length = auto   constraint t >= 3mm   minimize t }
structure def A { param v : Length = 1mm }
structure def B { param v : Length = 1mm }
structure def Outer { sub inner : Plate }
structure def Top {
    let c = Plate()
    let d = Plate(t: 3mm)
    let k = set{c, d}.count
    let a = A()
    let b = B()
    let o = Outer()
}
```

| Path | Expect |
|---|---|
| Top.c | value Plate{t: 3 mm} |
| Top.d | value Plate{t: 3 mm} |
| Top.k | value 1 |
| Top.a | value A{v: 1 mm} |
| Top.b | value B{v: 1 mm} |
| Top.o/made[0].inner.t | value 3 mm |
| Top.o/open.inner.t | value 3 mm |

### failed-solve-inside-value-leaves-value-existing

- Tests: A solve that fails inside a value leaves the parameter undetermined, and the value and its made instance still exist.
- Rule: Part 7.1 ("A solve that fails inside a value"); contract section 8
- Provenance: design Part 7.1
- Needs: instance, value, made, open, constraint, unknown, solve-infeasible
- Answer basis: stated: Part 7.1, "leaves the parameter it was to decide undetermined. The value still exists". A failed solve is not a failure (readings B3): `t` is `value undefined` and the verdicts that read it are undetermined. Under the candidates (readings E35): at 2 mm `constraint[0]` (`t >= 3mm`) is violated and read the trial; at 3 mm `constraint[1]` (`t <= 2mm`) is; no candidate is taken. A solver that honoured only the upper bound would take 2 mm.
- Candidates: Top.c/made[0].t: 2 mm, 3 mm

```ri
structure def Plate { param t : Length = auto   constraint t >= 3mm   constraint t <= 2mm }
structure def Top   { let c = Plate()   let k = c.t }
```

| Path | Expect |
|---|---|
| Top.c/made[0] | present |
| Top.c/made[0].t | value undefined |
| Top.c/made[0]/constraint[0] | verdict undetermined |
| Top.k | value undefined |

### closed-value-auto-decided-inside-made-instance

- Tests: a structure value made by `Plate()` decides its own `auto` inside the made instance; a constraint of the holder on it is checked, not bearing; a sub of the same definition gives the same answers.
- Rule: contract Example 11 (option B, ruled); Part 7.1; Part 7.2; Part 12 (Example 11); decision 1.
- Provenance: contract Part B Example 11, lines (1), (2), (5), (6); design Part 12 "Example 11". second-design trace Example 11.
- Needs: instance, value, made, open, constraint, unknown, solve-real
- Answer basis: stated (design Part 12, Example 11: `t` decided inside as 3 mm; the value is `Plate{t: 3 mm}`; `c.t > 5mm` violated; `sub s : Plate` gives the same answers as `c`).
- Today: column A of the contract's table: `c.t` undefined and `Plate`'s constraint never checked; (2) indeterminate; `s.t` undefined.

```ri
structure def Plate {
    param t : Length = auto
    constraint t >= 3mm
    minimize t
    pub let area = t * 40mm
}
structure def Asm {
    let c = Plate()
    constraint c.t > 5mm
    sub s : Plate
    constraint self.s.t > 5mm
}
```

| Path | Expect |
|---|---|
| Asm.c/made[0].t | value 3 mm |
| Asm.c | value Plate { t: 3 mm } |
| Asm/constraint[0] | verdict violated |
| Asm.s.t | value 3 mm |
| Asm/constraint[1] | verdict violated |

### equal-values-and-subs-merge-in-sets

- Tests: structure values are equal by content, so a set merges equal values and equal subs; `self.children` lists present subs only, and is undefined when a sub's presence is undetermined.
- Rule: decision 11; Part 5 ("A bare collection is a list of its elements as values"); Part 7.1; Part 7.4.
- Provenance: design decision 11.
- Needs: instance, guard, value, made
- Answer basis: traced. `Peg()` and `Peg(w: 10mm)` carry type `Peg` and `w` = 10 mm: one member. `a` and `b` both have `w` = 10 mm: one member. In `off`, `fan` is absent: children are `a` and `b`, 2. In `unk`, `fan`'s presence is undetermined, so the list is undefined (readings E30). `self.children` and a set of values are leaf κ₁'s (`value`); the entry has no collection sub.
- Today: both sets have one member; `self.children` is a list of path strings that includes absent subs (design decision 11 and Part 5, report; `setv.ri` gives 1, 1 and 2 with no guarded sub, run).

```ri
structure def Peg { param w : Length = 10mm }
structure def Fan { param d : Length = 40mm }
structure def Pair {
    param with_fan : Bool = false
    sub a : Peg
    sub b : Peg { w = 10mm }
    sub fan : Fan where with_fan
    let n_vals = set{Peg(), Peg(w: 10mm)}.count
    let n_subs = set{self.a, self.b}.count
    let n_kids = self.children.count
}
structure def Top {
    sub off : Pair
    sub unk : Pair { with_fan = undef }
}
```

| Path | Expect |
|---|---|
| Top.off.n_vals | value 1 |
| Top.off.n_subs | value 1 |
| Top.off.n_kids | value 2 |
| Top.unk.n_kids | value undefined |

### opened-value-reproduces-guards

- Tests: an opened value reads its guards from the parameters the value carries, so a guarded member of a value is absent or present as it was at the made instance.
- Rule: Part 7.2 (reading any other member builds an instance from the value); Part 4 (a guard is read per instance); decision 3.
- Provenance: logic-3 holds-up ("An opened value reproduces every guard, except through the determinacy tests").
- Needs: instance, guard, value, made, open
- Answer basis: traced. `c` carries `needs_fan` false: at `Top.c/open` the guard is false, `fan` is absent, and `c.fan.d` is undefined. `c2` carries true: its fan is present with `d` 40 mm.

```ri
structure def Fan {
    param d : Length = 40mm
}
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
}
structure def Top {
    let c = Case()
    let c2 = Case(needs_fan: true)
    let f = c.fan.d
    let f2 = c2.fan.d
}
```

| Path | Expect |
|---|---|
| Top.c/open.fan | absent |
| Top.f | value undefined |
| Top.f2 | value 40 mm |

### children-of-value-instance-list-its-subs

- Tests: `self.children` lists the present subs of the instance that declares them, inside a value as inside a place.
- Rule: Part 1 rule 4; Part 5 (a bare collection, `self.children`); decision 11; decision 3a.
- Provenance: language-1 L17 (`Pair` inside `Rig`) and L18; language-2 L17 row. Taken: rule 4 now says a sub is listed by its declaring instance "wherever that instance is".
- Needs: instance, guard, value, made, open
- Answer basis: traced. `p` is a made `Pair` with `with_c` false: `a`, `b` present, `c` absent, so `n` is 2 and `k` reads it through `open`. `q` has `with_c` true: three present subs.
- Today: `Pair.n = 3` (the absent `c` is counted), `Rig.k = undef`, `Rig.q.n = undef` (run `children_value.ri`).

```ri
structure def Peg  { param w : Length = 10mm }
structure def Pair {
    param with_c : Bool = false
    sub a : Peg
    sub b : Peg
    sub c : Peg where with_c
    pub let n = self.children.count
}
structure def Rig { let p = Pair()   let k = p.n   sub q : Pair { with_c = true } }
```

| Path | Expect |
|---|---|
| Rig.k | value 2 |
| Rig.q.n | value 3 |
| Rig.q.c | present |
| Rig.p/made[0].c | absent |

### auto-argument-either-form-and-as-value

- Tests: an `auto` written as a sub's argument, in either form, is an unknown of the declaring instance that the declaring instance's constraints decide (the child's default is not used), and a sub read as a value is taken after that solve, so its fields and the members opened from it are the solved ones.
- Rule: Part 6.2; Part 6.3 (table, "a sub's argument, either form"); Part 7.2; decision 6; contract Example 11
- Provenance: task 6340; task 7065; task 6657 (its half without geometry); task 6926 (its solved-auto half); design Part 13 (task 6340 row). design Part 13 row (task 6340). design Part 7.2.
- Needs: instance, value, open, constraint, unknown, solve-forced
- Answer basis: traced.
  `bore = auto` on `b` is an unknown of `Holder`. `margin == 8mm` reads it through a `let`, so that constraint bears on it, and it is forced to 10 mm.
  `Bearing(bore: auto)` on `c` is a second unknown of `Holder`, forced to 12 mm. `Bearing`'s default of 5 mm is read by neither.
  `Holder.e` is `b`'s as-value after its solves: `Bearing{bore: 10 mm}`. `bore` is a parameter, and `Bearing` decides nothing of its own.
  `e.half` is read through `Holder.e/open`, which is opened from that value: 10 mm / 2 = 5 mm.
  The two unknowns have no names of their own: `Holder/auto[b.bore]` and `Holder/auto[c.bore]` (readings A). Under the candidates (readings E35), 5 mm, the child's default, comes first for each: at 5 mm `margin` is 3 mm and `constraint[0]` is violated, and `constraint[1]` is violated, each having read the trial, so 5 mm is rejected for each. 10 mm and 12 mm violate nothing. A reading that kept the child's default would take 5 mm.
  Neither `Bearing` has an unknown of its own, so the entry does not need `nested-solve`.
- Today: the value of `Holder.b` keeps `bore: undef` after the solve, while the member cell resolves (task 6340 record, reported, not re-verified). In the dogfood probe of task 7065, a derived member reads that stale value. The design (Part 7, run) says `let e = self.a` reads undefined after a solve.
- Candidates: Holder/auto[b.bore]: 5 mm, 10 mm; Holder/auto[c.bore]: 5 mm, 12 mm

```ri
structure def Bearing { param bore : Length = 5mm   pub let half = bore / 2 }
structure def Holder {
    sub b : Bearing { bore = auto }
    sub c = Bearing(bore: auto)
    let margin = self.b.bore - 2mm
    constraint margin == 8mm
    constraint self.c.bore == 12mm
    let e = self.b
    let eh = e.half
}
```

| Path | Expect |
|---|---|
| Holder.b.bore | value 10 mm |
| Holder.c.bore | value 12 mm |
| Holder.b/as-value | value Bearing{bore: 10 mm} |
| Holder.e | value Bearing{bore: 10 mm} |
| Holder.eh | value 5 mm |

### made-value-equals-equal-subs-as-value

- Tests: a sub and a constructor call of the same definition with equal arguments in one parent each give their own answer at their own path, and the value made equals the sub's as-value.
- Rule: Part 7.2; Part 7.4 (equality by content); decision 6; contract Example 11
- Provenance: task 9489 (with a scalar field in place of its geometry field `g: body`. The faithful geometry version is task 10611's)
- Needs: instance, value, made
- Answer basis: traced.
  `Plate.s_b` is a sub with g = 4 mm. `Plate.h_body` makes `Plate.h_body/made[0]` with g = 4 mm, and its value is `H2{g: 4 mm}`.
  The sub's as-value carries the same type and parameters, so it is equal by content. Equal content shares stored work and changes no answer.
- Today: `Plate.h_body` is undefined when the parent also has a sub of the same type, in the geometry form (task 9489 record).

```ri
structure def H2 { param g : Length }
structure def Plate {
    let body = 4mm
    sub s_b = H2(g: body)
    let h_body = H2(g: body)
}
```

| Path | Expect |
|---|---|
| Plate.s_b.g | value 4 mm |
| Plate.h_body | value H2{g: 4 mm} |
| Plate.h_body/made[0].g | value 4 mm |
| Plate.s_b/as-value | value H2{g: 4 mm} |

### value-carries-undetermined-presence-parameter-as-undefined

- Tests: an instance read as a value, when one of its parameters is of undetermined presence, is still a value: it carries that parameter as undefined, and nothing that reads it fails.
- Rule: readings E31; Part 7.1 ("The value still exists"); ruling 5 of `seat-context.md` (reading something that is not there gives undefined); readings E28 and B4.
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N1 and its gap G9; readings E31, which answers it.
- Needs: instance, guard, value, summary
- Answer basis: traced. `c` is given `u = undef`, so its guard `u` is undefined and `Top.c.rpm` is `presence-undetermined` (readings E28). `w` is present, 2 mm. `Top.e` reads `c` as a value: it carries `u` as undefined (its value is undefined), `w` as 2 mm and `rpm`, whose presence is undetermined, as undefined (readings E31). It is a value, not `failure blocked`. Summary (readings B4): one undetermined for `rpm`'s presence; `c`'s `/arguments` is satisfied, because the explicit `undef` does not make it undetermined (readings B5); nothing failed. A reading under which the value were blocked by `rpm` would give `failed 1` or more.
- Note: `undef` in the expected value marks a field whose value is undefined; it is the Reify literal for an undefined value. If the runner's comparison of values cannot express a field that is undefined, the lead should say how such a field is written; the summary row alone still tells this reading from the blocked one.

```ri
structure def Case {
    param u : Bool
    param w : Length = 2mm
    where u { param rpm : Int = 1200 }
}
structure def Top {
    sub c : Case { u = undef }
    let e = self.c
}
```

| Path | Expect |
|---|---|
| Top.c.rpm | presence-undetermined |
| Top.e | value Case{u: undef, w: 2 mm, rpm: undef} |
| Top/summary | summary violated 0 undetermined 1 failed 0 |


---

## Instances made by an expression (rule B)

### made-instance-follows-outer-trial-argument

- Tests: an instance made with an argument that is an unknown of the holder follows the outer solve, its own unknown being decided under each trial.
- Rule: contract Example 11 ("An instance built from something on trial"); Part 12 ("An argument on trial"); Part 6.1.
- Provenance: contract Part B Example 11; design Part 12 "An argument on trial". `Slot`'s body is supplied; neither source gives one.
- Needs: instance, value, made, open, constraint, unknown, solve-forced, nested-solve
- Answer basis: traced. `w = 8 mm` is forced. Under each trial the made `Slot` decides `depth = width / 2`; at the solution `depth` = 4 mm. `dd` reads `depth` through `Asm.c/open`, whose value carries the decided `depth` (ruling 7).
  Under the candidates (readings E35): the outer trial `w` = 6 mm comes first, and under it the made `Slot` takes `depth` = 3 mm (satisfied), but `w == 8mm` is violated and read the trial, so 6 mm is rejected. Under `w` = 8 mm the inner 3 mm violates `depth == width / 2`, which read the trial, and is rejected; 4 mm is taken. A made instance decided once, at the first outer trial, and not again would keep 3 mm.
- Today: unknown.
- Candidates: Asm.w: 6 mm, 8 mm; Asm.c/made[0].depth: 3 mm, 4 mm

```ri
structure def Slot {
    param width : Length
    param depth : Length = auto
    constraint depth == width / 2
}
structure def Asm {
    let w : Length = auto
    constraint w == 8mm
    let c = Slot(width: w)
    let dd = c.depth
}
```

| Path | Expect |
|---|---|
| Asm.w | value 8 mm |
| Asm.c/made[0].depth | value 4 mm |
| Asm.dd | value 4 mm |

### instances-made-via-function-and-lambda-counted

- Tests: instances made inside a user function, and inside a lambda applied by a built-in, are counted by the summary of the instance whose cell called them; making a lambda makes nothing.
- Rule: rule B (a unit of source may make instances if it holds "a call to a user function, a subexpression of function type"); decision 7.
- Provenance: logic-1 L13 (taken: "a call to any user function"); logic-2 N1(b) (taken: "a subexpression of function type"). language-1 L15.
- Needs: instance, value, made, open, summary
- Answer basis: traced. `d`'s unit calls `mk`: one `Plate` at 1 mm, violated. `ps`'s unit holds `f`, of function type: `generate` applies it for `i` 0 and 1, making `Plate`s at 0 mm and 1 mm, both violated. `f`'s own cell only makes a lambda. `Asm`'s summary reads `d`'s and `ps`'s lists.
- Today: run `made.ri`: `Asm.d = Plate { t: 0.001 m }`, `Asm.ps = [Plate { t: 0 m }, Plate { t: 0.001 m }]`; `check` reports one indeterminate verdict, `Plate#constraint[0]` at the definition's own missing default, and none of these three.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
fn mk() -> Plate { Plate(t: 1mm) }
structure def Asm {
    let d = mk()
    let f = |i : Int| Plate(t: i * 1mm)
    let ps = generate(2, f)
}
```

| Path | Expect |
|---|---|
| Asm.d/open.t | value 1 mm |
| Asm.f/made[0] | absent |
| Asm/summary | summary violated 3 undetermined 0 failed 0 |

### argument-made-instance-counted-once

- Tests: an instance made in a sub's argument is counted exactly once, by the instance the parameter cell belongs to, and gives the same count as the named form.
- Rule: rule B ("Exactly one summary reads a cell's list ... For a parameter cell evaluating an argument, the mark arrives with the argument").
- Provenance: logic-2 N1(a) (the uncounted violation; taken); logic-3 R7 (the double count under draft 3; taken: "exactly one summary"). critic logic-3 R7; logic-2 AsmI/AsmII.
- Needs: instance, value, made, constraint, summary, roots
- Answer basis: traced. `AsmI`: the cell `AsmI.d.p` evaluates `Plate(t: 1mm)` in `AsmI`'s scope and makes `AsmI.d.p/made[0]`, violated; the default `Plate(t: 5mm)` is never evaluated. `d`'s summary reads `p`'s list and `AsmI`'s reads `d`'s: once. `AsmII`: the instance is made at `AsmII.c/made[0]`; `d.p` holds the value and makes nothing. Both are roots.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Widget {
    param p : Plate = Plate(t: 5mm)
}
structure def AsmI {
    sub d = Widget(p: Plate(t: 1mm))
}
structure def AsmII {
    let c = Plate(t: 1mm)
    sub d = Widget(p: c)
}
```

| Path | Expect |
|---|---|
| (file) | roots AsmI, AsmII |
| AsmI.d.p/made[0].t | value 1 mm |
| AsmI.d.p/made[0]/constraint[0] | verdict violated |
| AsmI/summary | summary violated 1 undetermined 0 failed 0 |
| AsmII.c/made[0]/constraint[0] | verdict violated |
| AsmII/summary | summary violated 1 undetermined 0 failed 0 |

### instance-made-in-count-cell-counted

- Tests: an instance made in a cell that is not a member, here a collection's count cell, is counted.
- Rule: rule B (the rule speaks of a unit of source, not a member); Part 5; readings A (a count constraint is the source of the count cell and is not a verdict) and B4.
- Provenance: logic-2 N1(d) (taken: draft 3 said "cell", and draft 5 "unit of source").
- Needs: instance, collection, value, made, open, summary
- Answer basis: traced. The count cell `Asm/count[vents]` evaluates `Sizing(n: 2).n`: it makes a `Sizing` with `n` 2, whose `n >= 3` is violated, and gives 2. `vents` has elements 0 and 1. The count constraint is not a verdict and is not counted, so the one violation is the made `Sizing`'s; the summary reads the made list of the count cell, which is not a member (readings B4). `.n` is read through the opened value, whose verdicts are never counted.

```ri
structure def Vent {
    param area : Length = 1mm
}
structure def Sizing {
    param n : Int
    constraint n >= 3
}
structure def Asm {
    sub vents : List<Vent>
    constraint vents.count == Sizing(n: 2).n
}
```

| Path | Expect |
|---|---|
| Asm.vents[1] | present |
| Asm.vents[2] | absent |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### selecting-builtin-makes-no-instance

- Tests: a built-in that selects or passes on a structure value makes no instance, so a value made once and selected is counted once.
- Rule: rule B ("A built-in that only selects or passes a value along (`xs[0]`, `filter`, `max_by`) makes nothing").
- Provenance: logic-3 R8, second half (taken). `filter` with a lambda is refused today (run `made.ri`), so only indexing is modelled.
- Needs: instance, value, made, open, summary
- Answer basis: traced. `qs` makes two `Plate`s, at 1 mm (violated) and 4 mm. `qs[0]` is a value read: `first` makes nothing and reads `t` as a field.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Asm {
    let qs = [Plate(t: 1mm), Plate(t: 4mm)]
    let first = qs[0]
}
```

| Path | Expect |
|---|---|
| Asm.first/open.t | value 1 mm |
| Asm.first/made[0] | absent |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### two-calls-one-function-two-instances

- Tests: two calls of one function in one cell make two distinct instances and open two distinct values.
- Rule: rule F (a made instance is named "by the chain of call sites and iteration indices"; an opened value in mid-expression is `open` "with such a chain"); rule B; decision 12 (two declarations present at once is a failure, which this must not become).
- Provenance: logic-2 N14 (taken: rule F's chain of call sites, after contract 7.1).
- Needs: instance, value, made, open, summary, paths
- Answer basis: traced. Both constructor calls are the one call in `mk`'s body. Kept apart by the chain from `both`'s expression, they are two instances at 1 mm and 2 mm, both violated, and two opened values give 40 and 80 mm². Collapsed to one step, they would be two declarations at one path, a failure. The text of the two steps is rule F's to settle, so no made path is pinned.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
    pub let area = t * 40mm
}
fn mk(t : Length) -> Plate { Plate(t: t) }
structure def Asm {
    let both = mk(1mm).area + mk(2mm).area
}
```

| Path | Expect |
|---|---|
| Asm.both | value 120 mm^2 |
| Asm/summary | summary violated 2 undetermined 0 failed 0 |

---

### inline-or-wrapped-constructor-same-answer

- Tests: a constructor call written inline and the same call wrapped in a function give the same answer, because the made instance's constraints are inside the caller either way.
- Rule: Part 6.4 requirement 1; Part 6.1 ("It makes no difference how the instance holding the constraint came to exist ... one reached through a function").
- Provenance: logic-1 L5 (taken: 6.1 and 6.4 requirement 1); logic-2 Part A row L5 and holds-up (3 mm inline and wrapped). design 6.4 requirement 1. language-1 L7 and probe f1.ri. second-design Q6 and probe g3.ri; claims-1 row 1.
- Needs: instance, value, made, constraint, summary, unknown, roots, solve-real
- Answer basis: stated (Part 6.4, requirement 1). `u` is each root's unknown. `Plate`'s `t >= 3mm` reads `t` = `u` and is inside the root, so it bears; with `u >= 1mm` and `minimize u`, 3 mm. `Plate` is used by `AsmA`, so it is not a root.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
fn mk(t : Length) -> Plate { Plate(t: t) }
structure def AsmA {
    let u : Length = auto
    constraint u >= 1mm
    minimize u
    let d = Plate(t: u)
}
structure def AsmB {
    let u : Length = auto
    constraint u >= 1mm
    minimize u
    let d = mk(u)
}
```

| Path | Expect |
|---|---|
| (file) | roots AsmA, AsmB |
| AsmA.u | value 3 mm |
| AsmB.u | value 3 mm |
| AsmA.d/made[0]/constraint[0] | verdict satisfied |
| AsmB/summary | summary violated 0 undetermined 0 failed 0 |

### value-verdicts-counted-once-at-made-instance

- Tests: a value's verdicts are counted once, at its made instance, however many cells and subs read it; a value taken into a list, into another `let`, or read in mid-expression makes no new instance.
- Rule: Part 7.2 and 7.3; rule B; contract Example 11; Part 2 (summary); readings B4.
- Provenance: second-design Q5 and Q6 (probe `g3.ri`, cut); design Part 12 Example 11 ("one made instance, its verdicts counted once"); design 6.5 (`Widget { param p : Plate }`); language-1 L9 session 3 (the verdict half; the note's text is the session's). Taken. Also, from the merged entries: design Part 12 Example 11; logic-1 counting trace. design Part 12 Example 11. logic-1 holds-up (counting; Bracket(mat: Steel())).
- Needs: instance, value, made, open, constraint, summary
- Answer basis: traced. Made instances: `Asm.c/made[0]` (t 1 mm, violated) and `Asm.xs/made[0]` (t 5 mm, satisfied; `c` inside the list makes nothing, rule B). `e` holds `c`'s value; `c` opened is the made plate, area 1 × 40 mm²; `xs[0].area` is opened by the reading body, the same. `d` and `w` read `p.t` = 1 mm: violated each. Counted (readings B4): 1 + 2 = 3; every `/arguments` verdict is satisfied.
- Today: `Asm.a` 40 mm²; only the definitions' verdicts print, both INDETERMINATE (their parameters have no default) (run `values_once.ri`).

```ri
structure def Plate  { param t : Length   constraint t >= 3mm   pub let area = t * 40mm }
structure def Widget { param p : Plate   constraint p.t > 5mm }
structure def Asm {
    let c = Plate(t: 1mm)
    let e = c
    let xs = [c, Plate(t: 5mm)]
    let a = xs[0].area
    sub d = Widget(p: c)
    sub w = Widget(p: c)
}
```

| Path | Expect |
|---|---|
| Asm.c/made[0]/constraint[0] | verdict violated |
| Asm.xs/made[0]/constraint[0] | verdict satisfied |
| Asm.e/open.t | value 1 mm |
| Asm.a | value 40 mm^2 |
| Asm.d/constraint[0] | verdict violated |
| Asm.w/constraint[0] | verdict violated |
| Asm.c/open.area | value 40 mm^2 |
| Asm/summary | summary violated 3 undetermined 0 failed 0 |

### nested-and-listed-made-instances-counted-once

- Tests: an instance made inside another constructor call's argument, inside a list literal, or inside a sub's argument has its definition's constraint checked, and each such violation is counted exactly once.
- Rule: rule B; Part 7.3; decision 7
- Provenance: task 8322
- Needs: instance, value, made, summary
- Answer basis: traced.
  The cell `Root.g` evaluates `Thing(w: -3mm)` and then `Holder(...)`: two made instances, and the `Thing` is violated.
  The cell `Root.loads` makes two `Thing`s, one violated (-3 mm) and one satisfied (2 mm).
  The parameter cell `Root.h.t` evaluates its argument in `Root`'s scope and makes one `Thing`, which is violated. Rule B: exactly one summary reads that list.
  `Holder` has no constraint and reads no member of `t`, so nothing is opened. Violations: 1 + 1 + 1 = 3 (readings B4: a made instance's verdicts are counted once, at the made instance). Every argument is defined, so no `/arguments` verdict is undetermined.
  The made-instance paths are left unpinned: section A's `/made[chain]` text is not worked out here for a call nested in another call's argument or in a list literal.
- Today: per the task record (release binary, 2026-10-07), `Root.g = Holder { t: Thing { w: -0.003 m } }` exists but `Thing`'s constraint is not checked for it. Instances inside list values are not checked either.

```ri
structure def Thing { param w : Length = 1mm   constraint w > 0mm }
structure def Holder { param t : Thing }
structure def Root {
    let g = Holder(t: Thing(w: -3mm))
    let loads = [Thing(w: -3mm), Thing(w: 2mm)]
    sub h : Holder { t = Thing(w: -3mm) }
}
```

| Path | Expect |
|---|---|
| Root/summary | summary violated 3 undetermined 0 failed 0 |

### undefined-argument-constructor-versus-function-wrapper

- Tests: with an undefined argument, a constructor call written inline and the same call wrapped in a user function give the same answers: the function is entered, the instance is made, and its verdicts and their class are the same.
- Rule: PRD D10(d), ruled by Leo on 2026-11-26 (readings C19): a user function is entered whatever its arguments, and each operation inside propagates; design 6.4 requirement 1 ("Inline or wrapped, the same answer"). It amends spec 9.2.7 (`undef` propagates strictly through a function call).
- Provenance: readings C19; PRD D10(d). The draft recommended the spec's rule and Leo took the alternative.
- Needs: instance, value, made, open, constraint, summary, roots
- Answer basis: stated: PRD D10(d) as Leo ruled it. `u` is a root's parameter with no value: undefined, class partial. Inline, `Plate(t: u)` makes `AsmA.d/made[0]` with `t` undefined; its constraint and its verdict on arguments are undetermined, partial (PRD 5.4, "A cause across a constructor call"). Wrapped, `mk` is entered with `t` bound to the undefined argument, and the constructor call in its body makes `AsmB.d/made[0,0]` (the call site of `mk`, then the constructor call in its body; readings A) with the same parameter, the same two verdicts and the same class: the parameter of a function keeps the class of the argument it was bound to. `w` reads `d.w` through `open` and is 40 mm in both. `five` ignores its parameter, so `k` is 5 mm, where the spec's rule and today's engine give undefined. Summary of each root: the made instance's two verdicts, counted once.
- Today: run `g06.ri` (the first form of this entry): `AsmA.d = Plate { t: undef }`; `AsmB.d = undef` (because: `AsmB.u` unbound). Today's engine does not enter the function.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
    pub let w = 40mm
}
fn mk(t : Length) -> Plate { Plate(t: t) }
fn five(x : Length) -> Length { 5mm }
structure def AsmA {
    param u : Length
    let d = Plate(t: u)
    let w = d.w
}
structure def AsmB {
    param u : Length
    let d = mk(u)
    let w = d.w
    let k = five(u)
}
```

| Path | Expect |
|---|---|
| (file) | roots AsmA, AsmB |
| AsmA.d/made[0].t | value undefined |
| AsmA.d/made[0]/constraint[0] | undetermined-because partial |
| AsmA.d/made[0]/arguments | undetermined-because partial |
| AsmA.w | value 40 mm |
| AsmA/summary | summary violated 0 undetermined 2 failed 0 |
| AsmB.d/made[0,0].t | value undefined |
| AsmB.d/made[0,0]/constraint[0] | undetermined-because partial |
| AsmB.d/made[0,0]/arguments | undetermined-because partial |
| AsmB.w | value 40 mm |
| AsmB.k | value 5 mm |
| AsmB/summary | summary violated 0 undetermined 2 failed 0 |

### instance-made-in-guard-condition-counted

- Tests: an instance made in a guard's condition, a cell that is not a member, is counted by the summary of the instance that declares the guard.
- Rule: rule B (a unit of source may make instances, whatever the cell); readings B4 ("The summary reads every child its instance declares, the cells that are not members included (a guard's condition ...), and whatever those cells made").
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N6 and its gap G3; readings B4, which answers it.
- Needs: instance, guard, value, made, open, summary
- Answer basis: traced. The guard of `extra` is a cell of `Asm` (`Asm/guard[0]`). It evaluates `Sizing(n: 2).n`: it makes a `Sizing` with `n` 2, whose `n >= 3` is violated, and reads 2 through the opened value, so the guard is `2 > 1`, true, and `extra` is present. The made `Sizing`'s verdict is counted once, at the made instance, and `Asm`'s summary reaches it through the guard cell's made list (readings B4); the opened value's verdicts are never counted. A summary that read only members would give `violated 0`.

```ri
structure def Sizing { param n : Int   constraint n >= 3 }
structure def P { param v : Length = 1mm }
structure def Asm { sub extra : P where Sizing(n: 2).n > 1 }
```

| Path | Expect |
|---|---|
| Asm.extra | present |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |


---

## Members written in a sub's braces

### braces-constraint-bears-on-instance-auto

- Tests: A constraint written in a sub's braces is a member of that instance and bears on its own `auto`.
- Rule: decision 4 (braces half); decision 1; Part 4; ruling 3 of `seat-context.md`
- Provenance: design Part 14 decision 4 (braces model); Part 4. language-1 L5; language-2 C4. logic-2 Part C on decision 4; logic-3 holds-up (17.5 mm).
- Needs: instance, braces, constraint, summary, unknown, solve-real
- Answer basis: stated: decision 4, "the braces give 17.5 mm". Trace: the feasible range is (15 mm, 20 mm], middle 17.5 mm. All three verdicts (the braces' one and Motor's two bounds) hold at 17.5 mm, and `Rig.motor` has no arguments, so its `/arguments` verdict is satisfied and the summary counts nothing (readings B4, B5). The forced form of the same model is `braces-constraint-forces-instance-auto`.
- Today: the braces' constraint is dropped with no diagnostic, `Motor` is checked once at its definition, and `check` says "All constraints satisfied" (run, `p7.ri`).

```ri
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Rig   { sub motor : Motor { constraint shaft_d > 15mm } }
```

| Path | Expect |
|---|---|
| Rig.motor.shaft_d | value 17.5 mm |
| Rig.motor/braces/constraint[0] | verdict satisfied |
| Rig.motor/constraint[1] | verdict satisfied |
| Rig/summary | summary violated 0 undetermined 0 failed 0 |

### braces-conflict-unsatisfiable-or-violated

- Tests: A braces constraint that conflicts with the definition's own makes the instance's solve unsatisfiable when the value is being chosen, and is violated when the value is given.
- Rule: decision 4 ("A conflicting constraint is an error, unsatisfiable"); contract section 8 (a failed solve is a result)
- Provenance: design Part 14 decision 4 (both `Motor` variants). design decision 4 (Leo's question on a conflicting constraint); language-1 L5. claims-3 probe b1.ri; failing.md M6 and probe p/a4.ri; language-1 L5.
- Needs: instance, braces, constraint, summary, unknown, solve-infeasible
- Answer basis: stated: decision 4. `m1`: no value satisfies the constraints that bear, so the solve finds none; `shaft_d` is `value undefined` and nothing falls back to a default, so the braces' verdict and Motor's two bounds, which read it, are undetermined (readings B3). `m2`: `shaft_d` is given, so it is no unknown and nothing is chosen; 12 mm against `> 15mm` is violated, and Motor's bounds hold. Neither sub's `/arguments` verdict is undetermined: `m1` has no arguments and `m2`'s is defined (readings B5). The failed solve's own node belongs to the solve design (task 10610) and is not pinned; it is not counted as a failure (readings B3, B4). Summary: violated 1 (m2's braces), undetermined 3 (m1).
- Candidates: Rig.m1.shaft_d: 12mm, 20mm, 26mm. 12 mm and 20 mm meet Motor's bounds and not the braces (a reading in which the braces do not bear takes 12 mm); 26 mm meets the braces and not Motor's `<= 20mm` (a reading in which the definition's bounds do not bear at the instance takes it). Each candidate leaves a constraint that depended on the trial violated, so none is taken.

```ri
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Rig {
    sub m1 : Motor { constraint shaft_d > 25mm }
    sub m2 : Motor { shaft_d = 12mm   constraint shaft_d > 15mm }
}
```

| Path | Expect |
|---|---|
| Rig.m1.shaft_d | value undefined |
| Rig.m1/braces/constraint[0] | verdict undetermined |
| Rig.m2.shaft_d | value 12 mm |
| Rig.m2/braces/constraint[0] | verdict violated |
| Rig/summary | summary violated 1 undetermined 3 failed 0 |

### bare-name-assignment-reading-itself-is-cycle

- Tests: `t = t / 2` in the braces reads the instance's own `t`, which is a cycle and is reported.
- Rule: Part 4 ("Names inside the braces")
- Provenance: design Part 4. design Part 4; language-2 C4.
- Needs: compile-error, instance, braces
- Answer basis: stated: Part 4, "`t = t / 2` in the braces ... would read the plate's own, which is a cycle and is reported". The right-hand `t` is not a `let` of the braces and is a member `Plate` declares, so it is the instance's own (readings C20). That the source compiles and the cycle is reported at run time, at the parameter's cell, is readings C13. The call form reads the parent's `t`: `call-form-sub-argument-reads-parent-name`.
- Today: the right-hand `t` reads the parent's `t` (design Part 4).

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm   { param t : Length = 10mm   sub p : Plate { t = t / 2 } }
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.p.t | failure cycle |

---

### braces-let-constraint-and-guarded-assignment-compiled

- Tests: a guarded assignment, a constraint and a `let` written in a sub's braces are compiled as members of that instance.
- Rule: Part 13 row 10; Part 4 ("What is written in a sub's braces", "Names inside the braces"); decision 4 (braces half); seat-context ruling 3.
- Provenance: design Part 13 row "A `let`, a constraint or a `where` in a sub's braces is dropped". language-2 Part D; rule G's listed edge (an argument assigned under a guard in the braces).
- Needs: instance, braces, guard, constraint, summary
- Answer basis: traced. `thick` is not a `let` of the braces and not a member of `Plate`, so it is `Asm`'s (readings C20); its condition is the cell `Asm.p/braces/guard[0]`, false, so the argument is absent and `p.t` keeps its default 3 mm. In `q`, bare `t` is the plate's own: 3 mm > 4 mm violated. In `r`, `u` is the braces' own `let`, at `Asm.r/braces.u` (readings A), read by a constraint of the same braces, which C20 allows under either answer to who else may read it: 6 mm, and 6 mm > 7 mm violated. Nothing is solved, so both are verdicts. No argument is undefined, so every `/arguments` verdict is satisfied (readings B5).
- Today: the brace constraint is dropped and `check` says "All constraints satisfied"; the guard on the assignment is ignored, giving 6 mm with `thick` false (run, `braces2.ri`, `braces2f.ri`, `braces3.ri`).

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm {
    param thick : Bool = false
    sub p : Plate { t = 6mm where thick }
    sub q : Plate { constraint t > 4mm }
    sub r : Plate { let u = t * 2   constraint u > 7mm }
}
```

| Path | Expect |
|---|---|
| Asm.p/braces/guard[0] | value false |
| Asm.p.t | value 3 mm |
| Asm.q/braces/constraint[0] | verdict violated |
| Asm.r/braces.u | value 6 mm |
| Asm.r/braces/constraint[0] | verdict violated |
| Asm/summary | summary violated 2 undetermined 0 failed 0 |

### guarded-assignment-given-or-unknown-per-instance

- Tests: an argument written under a guard in a sub's braces is given when the guard holds, and otherwise the definition's own `auto` is an unknown of that instance.
- Rule: Part 6.2 ("The parameter's cell reads the guard first"); Part 4 (a guarded assignment is an argument present under that guard).
- Provenance: logic-1 L2 (taken: 6.2); logic-2 Part A row L2 and holds-up (traced 6 and 12; 3 and 6). design Part 4 and 6.2; critic logic-1 (the thick model). The tracer of 2026-11-26 (trace-braces-purposes-unknowns, wrong tags): `Plate`'s `minimize t` replaced by an upper bound in `b`'s braces, so that `b` is forced and the entry runs without a real solver.
- Needs: instance, braces, guard, constraint, summary, unknown, solve-forced
- Answer basis: traced. In `a`, `thick` holds, so `t` is given 6 mm and is not an unknown (design 6.2); `w` is `a`'s unknown and `w == 2 * t` forces 12 mm. In `b`, `thin` is false, so the argument is absent and `t` and `w` are both `b`'s unknowns; `Plate`'s `t >= 3mm` and the braces' `t <= 3mm` force 3 mm, and `w` is 6 mm. Every verdict holds; no argument is undefined (readings B5).
- Today: run `bracesguard.ri` (with `minimize t` in `Plate`, no braces constraint): `Asm.a` and `Asm.b` print with no `t` or `w`; the definition-level `Plate.t` prints about 5 m.
- Candidates: Asm.a.w: 6mm, 12mm. Asm.b.t: 6mm, 3mm. Asm.b.w: 12mm, 6mm. For `a`, 6 mm is the `w` a reading takes that ignores the guarded argument and decides `t` at 3 mm; under the given 6 mm it breaks `w == 2 * t`. For `b`, (6 mm, 12 mm) is what a reading takes that gives `t` the argument although its guard is false; it breaks the braces' `t <= 3mm`. Tried in order, or as every pair in order, the first taken is (3 mm, 6 mm).

```ri
structure def Plate {
    param t : Length = auto
    param w : Length = auto
    constraint t >= 3mm
    constraint w == 2 * t
}
structure def Asm {
    param thick : Bool = true
    param thin : Bool = false
    sub a : Plate {
        t = 6mm where thick
    }
    sub b : Plate {
        t = 6mm where thin
        constraint t <= 3mm
    }
}
```

| Path | Expect |
|---|---|
| Asm.a.t | value 6 mm |
| Asm.a.w | value 12 mm |
| Asm.b.t | value 3 mm |
| Asm.b.w | value 6 mm |
| Asm.b/braces/constraint[0] | verdict satisfied |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### guard-of-argument-reading-receivers-solve-left-open

- Tests: whether a guard on an argument that reads what the receiving instance's solve decides is a cycle, or is read through the solve's view.
- Rule: Part 4 ("An argument that depends on what it helps decide is a cycle"); contract section 8 (a solve reads its arguments before its view, and a guard through it); design 6.4 item 5; readings E27.
- Provenance: logic-2 N10, first half (logic-3 Part A row N10 "The cycle closed"). N10 also proposed that the compiler reject the direct case; draft 5 does not say so, so no compile outcome other than acceptance is pinned. Renamed 2026-11-26 from `guard-of-argument-reading-receivers-solve-cycles`, which pinned `failure cycle` on `Asm.wide` and `Asm.a.w`: the tracer (trace-braces-purposes-unknowns, wrong answers, low confidence) showed the cycle holds only if the guard of an argument is read before the view, and readings E27 gives that question to the solve design.
- Needs: compile-error, instance, braces, guard, unknown
- Answer basis: open for `Asm.wide`, `Asm.a.t` and `Asm.a.w`; owner task 10610 (readings E27). `wide` is a bare name in the braces that is no `let` of the braces and no member of `Plate`, so it is `Asm.wide` (readings C20), and `Asm.wide` reads `Asm.a.w`, which `a`'s solve decides. Read before the view, as part of the argument (design Part 4), the ring solve, guard, `wide`, `a.w`, solve is a `failure cycle` on each node. Read through the view (contract section 8, design 6.4 item 5), there is no ring, and two states are consistent: `t` 6 mm with `w` 12 mm, and `t` 3 mm with `w` 6 mm. The rows pinned are those no solve decides: the source compiles, and `a` has no guard of its own.
- Note: recommended in the source of this entry before 2026-11-26: `failure cycle` at `Asm.wide` and at `Asm.a.w`.

```ri
structure def Plate {
    param t : Length = auto
    param w : Length = auto
    constraint t >= 3mm
    constraint w == 2 * t
    minimize t
}
structure def Asm {
    let wide = self.a.w > 10mm
    sub a : Plate {
        t = 6mm where wide
    }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.a | present |

### undefined-guard-on-argument-gives-undefined-parameter

- Tests: a parameter whose argument's guard is undefined is undefined, and the instance's built-in verdict on its arguments is undetermined.
- Rule: Part 6.2; Part 4 (an undefined guard gives presence undetermined, said of members, not of arguments); readings E27 and B5.
- Provenance: logic-2 N10, second half ("Under an undefined guard the parameter is undefined, with the guard's cause, and is not an unknown", proposed); logic-3 Part A row N10 ("an undefined guard on an assignment still unstated"). Draft 5 does not say; settled in part by readings E27.
- Needs: compile-error, instance, braces, guard, constraint, summary, unknown
- Answer basis: traced for the guard, `t` and `/arguments`; open for the rest. `Asm` is a root and `thick` has no default, so `thick` is undefined, and so is the guard's cell `Asm.a/braces/guard[0]`. By readings E27 the parameter `t` is then `value undefined`, not an unknown. By readings B5 an argument whose own guard is undefined makes `/arguments` undetermined. Open: what `a`'s other unknown `w` and its solve do, and so the summary; owner task 10610 (readings E27). None of the pinned rows reads a solve, so the entry runs.

```ri
structure def Plate {
    param t : Length = auto
    param w : Length = auto
    constraint t >= 3mm
    constraint w == 2 * t
    minimize t
}
structure def Asm {
    param thick : Bool
    sub a : Plate {
        t = 6mm where thick
    }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.a | present |
| Asm.a/braces/guard[0] | value undefined |
| Asm.a.t | value undefined |
| Asm.a/arguments | verdict undetermined |

### braces-names-own-member-then-declarer

- Tests: in a sub's braces a bare name is the instance's own member first, then the declaring definition's; `self` is the parent.
- Rule: Part 4 ("Names inside the braces", ruled 2026-11-26); decision 4; readings C20 (first bullet).
- Provenance: design Part 4 (its `Plate`/`Asm` model, extended by one bare name the child lacks); language-1 L5 and language-2 C4 (name resolution was unsettled and blocked decision 4). Taken. Also, from the merged entries: design Part 4 model, the bound changed to 6 mm.
- Needs: instance, braces, constraint
- Answer basis: stated (Part 4: "`p.t` is 5 mm and the constraint reads the plate's `t`"). `self.t` is `Asm`'s 10 mm, so the argument is 5 mm. `clearance` is no `let` of the braces and not a member of `Plate`, so it is `Asm`'s, 4 mm (readings C20). Had bare `t` named `Asm`'s 10 mm, constraint[0] would be violated.
- Today: `Asm.p.t = 5 mm`; the braces constraints are dropped (run `braces_names.ri`).

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm {
    param t : Length = 10mm
    param clearance : Length = 4mm
    sub p : Plate { t = self.t / 2   constraint t <= 6mm   constraint t >= clearance }
}
```

| Path | Expect |
|---|---|
| Asm.p.t | value 5 mm |
| Asm.p/braces/constraint[0] | verdict satisfied |
| Asm.p/braces/constraint[1] | verdict satisfied |

### braces-let-read-from-parent-rejected

- Tests: a `let` written in a sub's braces can be read only by the other members written in the same braces; a read of it from the parent is a compile error.
- Rule: readings C20 (second bullet: who may read a braces `let`; PRD D10(f), ruled by Leo on 2026-11-26 as recommended); readings A (row "A `let` written in a sub's braces"); Part 4 (members written in the braces are members of the instance); seat-context ruling 3
- Provenance: appendix A; brief item 8. Renamed 2026-11-26 from `braces-let-member-readable-from-parent`, which pinned `Asm.s.zz` and `Asm.r` at 6 mm, and then from `braces-let-read-from-parent-left-open` when Leo ruled.
- Needs: compile-error, instance, braces
- Answer basis: stated: PRD D10(f) as Leo ruled it. The `let` is a member of the sub's instance that only the other members written in the same braces can read, so `self.s.zz` in `Asm` is a compile error. A value carries no braces, so `let e = self.s` could never have `e.zz`, and the sub and its value agree on what can be read.
- Today: run `g08a.ri`: "error: unknown member 'zz' on sub 's'" (the braces `let` is dropped).

```ri
structure def Plate {
    param t : Length = 3mm
}
structure def Asm {
    sub s : Plate {
        let zz = t * 2
    }
    let r = self.s.zz
}
```

| Path | Expect |
|---|---|
| (compile) | error: `zz` is a `let` written in the braces of `s` and can be read only there; write the `let` in `Asm` |

### braces-let-repeating-definition-name-rejected

- Tests: a `let` in a sub's braces that repeats a name the sub's definition declares is a compile error.
- Rule: readings C20 (third bullet); PRD D10(f) ("A name that repeats a member of the sub's definition is a compile error")
- Provenance: appendix A; brief item 8.
- Needs: compile-error, braces
- Answer basis: stated: readings C20.
- Today: run `g08b.ri`: accepted; the braces `let` is dropped and `Asm.s.zz = 0.006 m`, `Plate`'s.

```ri
structure def Plate {
    param t : Length = 3mm
    let zz = t * 2
}
structure def Asm {
    sub s : Plate {
        let zz = t * 3
    }
}
```

| Path | Expect |
|---|---|
| (compile) | error: `zz` is declared twice for `s`, by `Plate` and in the braces of `s`; the message names both |

### braces-constraint-forces-instance-auto

- Tests: a constraint in a sub's braces bears on the instance's own `auto`, shown with a forced value that needs no real solver.
- Rule: decision 4 (braces half); decision 1; Part 4; seat-context ruling 3.
- Provenance: the tracer of 2026-11-26 (trace-braces-purposes-unknowns, wrong tags, "coverage that waits and need not"): decision 4's model with the braces' bound at 20 mm; PRD boundary scenario S5.
- Needs: instance, braces, constraint, summary, unknown, solve-forced
- Answer basis: traced. `shaft_d` is `Motor`'s `auto`, owned by `Rig.motor`. Bearing, all inside `Rig.motor`: `Motor`'s two bounds and the braces' `>= 20mm`. The only value is 20 mm. Had the braces not borne, any value in [4 mm, 20 mm] would do and the first candidate, 12 mm, would be taken.
- Candidates: Rig.motor.shaft_d: 12mm, 25mm, 20mm. 12 mm meets `Motor`'s bounds and not the braces; 25 mm meets the braces and not `Motor`'s `<= 20mm`. Each breaks a constraint that depended on the trial, so 20 mm is the first taken.

```ri
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Rig   { sub motor : Motor { constraint shaft_d >= 20mm } }
```

| Path | Expect |
|---|---|
| Rig.motor.shaft_d | value 20 mm |
| Rig.motor/braces/constraint[0] | verdict satisfied |
| Rig.motor/constraint[1] | verdict satisfied |
| Rig/summary | summary violated 0 undetermined 0 failed 0 |

### braces-typed-let-auto-rejected

- Tests: a typed `let` with `auto` written in a sub's braces is a compile error that says to write it in the parent and pass it.
- Rule: readings C20 (fourth bullet); PRD section 5.7, table "What a sub's braces may hold" (row `let x : T = auto`).
- Provenance: the tracer's counter-example N1 (trace-braces-purposes-unknowns): rulings 3 and 4 give the `auto` two owners.
- Needs: compile-error, braces
- Answer basis: stated: readings C20. The `auto` is in `Asm`'s text (ruling 4) and the member would be `Asm.r`'s (ruling 3), so it has no single owner.

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm {
    sub r : Plate { let u : Length = auto   constraint u == t }
}
```

| Path | Expect |
|---|---|
| (compile) | error: `auto` in a `let` in the braces of `r`; write `let u : Length = auto` in `Asm` and pass it |

### purpose-line-in-braces-rejected

- Tests: a `purpose Name(args)` line written in a sub's braces is a compile error that says to write it in the parent with the sub as subject.
- Rule: readings C20 (fifth bullet); PRD decision D2 ("In a sub's braces the line is a compile error"); PRD section 5.7, table "What a sub's braces may hold".
- Provenance: the tracer's counter-example N4 (trace-braces-purposes-unknowns); critic-design F6.
- Needs: compile-error, braces, purpose, grammar-purpose
- Answer basis: stated: readings C20. Inside the braces `self` is `Holder`, so nothing there names `Holder.b`.
- Note: the member line `purpose robust(self)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.

```ri
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Bracket { param t : Length = 2mm }
structure def Holder {
    sub b : Bracket {
        purpose robust(self)
    }
}
```

| Path | Expect |
|---|---|
| (compile) | error: a purpose applied in the braces of `b`; write `purpose robust(self.b)` in `Holder` |

### braces-unguarded-read-of-guarded-member-is-undetermined

- Tests: a constraint in a sub's braces that reads, with no guard of its own, a member the sub's definition declares under a guard compiles and reads the instance's member, even when the parent has a member of that name; where the member is absent the constraint is undetermined.
- Rule: readings C20 (first and last bullets); Leo's ruling of 2026-11-26 on the PRD's D13 (a read of a member that may be absent compiles wherever it is written), which amends decision 3b line 1 and Part 4.
- Provenance: the tracer's counter-example N5 (trace-braces-purposes-unknowns), which found three possible outcomes. Readings C20 first chose a compile error; Leo's ruling makes it the run-time answer. Renamed from `braces-unguarded-read-of-guarded-member-rejected`.
- Needs: compile-error, instance, braces, guard, constraint, summary
- Answer basis: traced. Bare `rib` is no `let` of the braces and is a member `Plate` declares, present or not, so it is `Asm.p.rib` and not `Asm`'s 9 mm. `big` is false, so `Asm.p.rib` is absent and the constraint reads something that is not there: undetermined, other. Resolved to `Asm`'s 9 mm it would be satisfied, so the entry also pins the resolution.

```ri
structure def Plate {
    param big : Bool = false
    where big { param rib : Length = 2mm }
}
structure def Asm {
    param rib : Length = 9mm
    sub p : Plate { constraint rib > 1mm }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.p.rib | absent |
| Asm.p/braces/constraint[0] | undetermined-because other |
| Asm/summary | summary violated 0 undetermined 1 failed 0 |

### call-form-sub-argument-reads-parent-name

- Tests: in the call form of a sub an argument is the caller's expression, so a bare name in it is the parent's, where the same assignment in the braces reads the instance's own.
- Rule: readings C13; PRD section 5.4 ("In the call form of a sub ... an argument is the caller's expression"); PRD D13 item 20.
- Provenance: the tracer's counter-example N6 (trace-braces-purposes-unknowns). The braces form is `bare-name-assignment-reading-itself-is-cycle`.
- Needs: instance
- Answer basis: traced. `t / 2` is evaluated in `Asm`, whose `t` is 10 mm: 5 mm.

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm   { param t : Length = 10mm   sub p = Plate(t: t / 2) }
```

| Path | Expect |
|---|---|
| Asm.p.t | value 5 mm |

### required-parameter-given-only-under-guard-rejected

- Tests: a parameter with no default that a sub's braces give only under a guard is a compile error.
- Rule: readings C21; task 7874's rule (a required argument must be given).
- Provenance: the tracer's counter-example N7 (trace-braces-purposes-unknowns).
- Needs: compile-error, braces, guard, required-arguments
- Answer basis: stated: readings C21. With `long` false the argument is absent and `len` has no default; the compiler cannot decide `long`, so the guarded assignment alone does not give the argument. It waits for task 7874.

```ri
structure def Beam { param len : Length   constraint len >= 10mm }
structure def Asm  {
    param long : Bool = false
    sub b : Beam { len = 20mm where long }
}
```

| Path | Expect |
|---|---|
| (compile) | error: `len` has no default and the braces of `b` give it only under `long`; give it unguarded, or give `len` a default |

### braces-constraint-reads-parent-unknown

- Tests: a constraint in a sub's braces that reads the parent's unknown: the sub's solve takes the parent's trial value before it opens its view, so the two are decided with no cycle.
- Rule: Part 4 (names inside the braces); decision 1; contract O9; PRD section 5.6 (an instance's `problem` lists the names of the scope that wrote its braces that its constraints read; a solve makes those reads before it opens its view).
- Provenance: critic-design F3 (the `Rig` model with `let w`).
- Needs: instance, braces, constraint, summary, unknown, solve-forced, nested-solve
- Answer basis: traced. Bare `w` is no `let` of the braces and no member of `Motor`, so it is `Rig.w`, `Rig`'s unknown. `Rig`'s `w == 10mm` bears on it, and so does the braces' constraint, which is inside `Rig` and reads `w`: under a trial `w := x`, `m`'s solve reads `x` before its view and decides `shaft_d` = x + 5 mm, so the braces' constraint holds for any x up to 15 mm. `w` is 10 mm, and `shaft_d` 15 mm.
- Candidates: Rig.w: 5mm, 10mm. Rig.m.shaft_d: 10mm, 15mm. Under `w := 5mm`, `w == 10mm` is violated. Under `w := 10mm`, `shaft_d := 10mm` breaks the braces' constraint; 10 mm is what `m`'s solve takes in a reading in which the braces' constraint bears only on `Rig`'s `w` and not on `m`'s `shaft_d`. The first pair taken is (10 mm, 15 mm).

```ri
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Rig {
    let w : Length = auto
    constraint w == 10mm
    sub m : Motor { constraint shaft_d == w + 5mm }
}
```

| Path | Expect |
|---|---|
| Rig.w | value 10 mm |
| Rig.m.shaft_d | value 15 mm |
| Rig.m/braces/constraint[0] | verdict satisfied |
| Rig/summary | summary violated 0 undetermined 0 failed 0 |

### braces-constraint-reads-writer-scope

- Tests: a constraint in a sub's braces reads the parent through `self`; an argument that reads a parent's cell which reads the sub's own unknown is a cycle; the same dependence through a braces constraint is the solve design's.
- Rule: Part 4 (names inside the braces; "An argument that depends on what it helps decide is a cycle"); contract section 8 (a solve reads its arguments before its view); PRD section 5.6 (the names of the writer's scope that a constraint reads are read before the view).
- Provenance: the tracer's interaction I2 (trace-braces-purposes-unknowns); critic-design F3 (the companion case).
- Needs: instance, braces, constraint, unknown, solve-forced
- Answer basis: traced for `Rig` and `RigA`; open for `RigB`, owner task 10610. `Rig`: `d` is `Rig.m`'s; the braces' constraint reads `Rig.bore`, 15 mm, which no solve decides; with `d <= 15mm` the only value is 15 mm. `RigA`: `m`'s solve reads its argument `floor` before its view (contract section 8); `floor` is `self.half`, which reads `RigA.m.d`, which reads that solve: a ring, `failure cycle` on each node. `RigB`: the tracer, reading contract section 8 (only arguments before the view), reads `self.half` through the view: 5 mm under `d := 10mm`, and `d` is 10 mm. PRD section 5.6 has a solve read before its view the names of the writer's scope its constraints read, which `self.half` is; then `RigB` is a ring like `RigA`. The two documents disagree, and the solve design owns the answer.
- Note: recommended by the tracer: `RigB.m.d` 10 mm, `RigB.half` 5 mm. Under PRD section 5.6 as written: `failure cycle` at both.
- Candidates: Rig.m.d: 10mm, 15mm. RigA.m.d: 10mm. RigB.m.d: 5mm, 10mm. In `Rig`, 10 mm meets `Motor`'s bounds and breaks the braces' constraint, so 15 mm is the first taken. In `RigA` no candidate is tried: the solve meets the ring when it reads `floor`. `RigB`'s list is for the open rows.

```ri
structure def Motor {
    param d : Length = auto
    param floor : Length = 0mm
    constraint d >= floor
    constraint d <= 15mm
}
structure def Rig {
    param bore : Length = 15mm
    sub m : Motor { constraint d >= self.bore }
}
structure def RigA {
    let half = self.m.d / 2
    sub m : Motor { floor = self.half   constraint d == 10mm }
}
structure def RigB {
    let half = self.m.d / 2
    sub m : Motor { constraint d == 10mm   constraint d > self.half }
}
```

| Path | Expect |
|---|---|
| Rig.m.d | value 15 mm |
| Rig.m/braces/constraint[0] | verdict satisfied |
| RigA.m.floor | failure cycle |
| RigA.half | failure cycle |
| RigA.m.d | failure cycle |
| RigB.m.d | open |
| RigB.half | open |

### braces-constraint-reads-sibling-solved-value

- Tests: a constraint in a sub's braces may read a sibling's solved value, which is decided alone; two siblings whose braces constraints read each other's unknown are a cycle.
- Rule: decision 1; Part 6.1 ("Where this meaning has no answer"); contract section 6 (two solves inside each other's trials are a cycle); PRD section 5.6.
- Provenance: the tracer's interaction I3 (trace-braces-purposes-unknowns), here with forced values.
- Needs: instance, braces, constraint, unknown, solve-forced
- Answer basis: traced. In `Asm`, `b`'s braces force `b.shaft_d` to 4 mm, decided by `b` alone: `a`'s braces' constraint is outside `b` and does not bear on it. `a`'s braces' constraint reads `self.b.shaft_d`: 4 mm, so `a.shaft_d` is 8 mm. Read before `a`'s view (PRD section 5.6) or through it, the value is the same; through it the two solves nest, which needs core task 10563. In `Pair`, `a`'s solve reads `b.shaft_d`, whose solve reads `a.shaft_d`, which waits on `a`'s solve: a ring under either reading, `failure cycle` on each unknown. Neither constraint is inside the other's owner, so no joint group may form (contract section 8, ruled 2026-11-25).
- Candidates: Asm.b.shaft_d: 20mm, 4mm. Asm.a.shaft_d: 4mm, 8mm. Pair.a.shaft_d: 8mm. Pair.b.shaft_d: 4mm. 20 mm for `b` and 4 mm for `a` meet `Motor`'s bounds and break the braces' constraint, which a reading in which the braces do not bear would take. The first taken are 4 mm and 8 mm. In `Pair` no candidate is tried: each solve meets the ring before it can take one.

```ri
structure def Motor { param shaft_d : Length = auto   constraint shaft_d >= 4mm   constraint shaft_d <= 20mm }
structure def Asm {
    sub a : Motor { constraint shaft_d == self.b.shaft_d * 2 }
    sub b : Motor { constraint shaft_d <= 4mm }
}
structure def Pair {
    sub a : Motor { constraint shaft_d == self.b.shaft_d * 2 }
    sub b : Motor { constraint shaft_d == self.a.shaft_d / 2 }
}
```

| Path | Expect |
|---|---|
| Asm.b.shaft_d | value 4 mm |
| Asm.a.shaft_d | value 8 mm |
| Asm.a/braces/constraint[0] | verdict satisfied |
| Pair.a.shaft_d | failure cycle |
| Pair.b.shaft_d | failure cycle |

### braces-guard-reads-writer-unknown

- Tests: the guard of an assignment in a sub's braces may read an unknown of the parent; the parent's solve reads it through its view, and a constraint on the argument's effect bears on that unknown.
- Rule: Part 6.1 ("'Bears on' follows every chain of reads"); Part 4; PRD section 5.4 (the condition of a guard written in the braces is a cell of the sub's instance); contract section 8 (a guard is read through the view).
- Provenance: the tracer's interaction I4 (trace-braces-purposes-unknowns), with the bound on `n` widened so that the guard bears: `self.a.t == 6mm` is the only constraint that keeps `n` above 4 mm.
- Needs: instance, braces, guard, constraint, summary, unknown, solve-forced
- Answer basis: traced. `n` is `Asm`'s; `a` has no unknown and no solve. `Asm`'s three constraints are inside `Asm`, and `Asm/constraint[2]` reads `a.t`, which reads the guard `Asm.a/braces/guard[0]`, which reads `n`: it bears on `n`. Under `n := x` the guard holds exactly when x > 4 mm, and only then is `a.t` 6 mm. Every feasible `n` is in (4 mm, 5 mm], and each gives the rows pinned; `n` itself is not pinned. No cycle: `Asm`'s solve reads the guard through its view, and no argument of `Asm` is involved.
- Candidates: Asm.n: 3mm, 5mm. 3 mm meets `n >= 3mm` and `n <= 5mm` and leaves the guard false, so `a.t` is 3 mm and `Asm/constraint[2]` is violated; a reading in which a constraint reached only through a guard does not bear takes it. 5 mm is the first taken.

```ri
structure def Plate { param t : Length = 3mm }
structure def Asm {
    let n : Length = auto
    constraint n >= 3mm
    constraint n <= 5mm
    sub a : Plate { t = 6mm where n > 4mm }
    constraint self.a.t == 6mm
}
```

| Path | Expect |
|---|---|
| Asm.a/braces/guard[0] | value true |
| Asm.a.t | value 6 mm |
| Asm/constraint[2] | verdict satisfied |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### braces-read-of-private-let-rejected

- Tests: in a sub's braces a bare name that is a private `let` of the sub's definition is a compile error; it does not fall through to the parent's member of that name.
- Rule: spec 7.4 and 8.3; PRD D13 item 14, ruled by Leo on 2026-11-26; readings E38 (the braces are the declarer's text: the lead's reading of the ruling) and C20 (a bare name is the instance's own member before it is the declarer's).
- Provenance: Leo's ruling on the PRD's D13 item 14.
- Needs: compile-error, braces
- Answer basis: stated: readings E38. Bare `area` is no `let` of the braces and is a member `Plate` declares, so it is the plate's, and not `Asm`'s 9 mm. It is a `let` without `pub`, and the braces are written by `Asm`. If the name fell through to `Asm.area`, adding a private `let` to a definition would change what its users' braces mean.
- Today: run `priv2.ri`: compiles; the constraint in the braces is dropped.

```ri
structure def Plate {
    param t : Length = 3mm
    let area = t * 40mm
}
structure def Asm {
    param area : Length = 9mm
    sub p : Plate { constraint area > 100mm * 1mm }
}
```

| Path | Expect |
|---|---|
| (compile) | error: the constraint in the braces of `p` reads `area`, a `let` of `Plate` that is not `pub`; the parent's member is `self.area` |


---

## Purposes

### purpose-inside-owner-still-only-checks

- Tests: a purpose applied to the instance whose own `auto` it reads is checked against the result and never takes part in choosing it, even though it sits inside that instance by path.
- Rule: Part 10; seat-context ruling 1; decision 4 (purpose half, reversed 2026-11-26).
- Provenance: design Part 10 ("Its constraints are verdicts and nothing else"); design decision 4's purpose model, made forced: decision 4 uses `t ≤ 10mm` with `minimize t` (giving 1 mm); here `t ≤ 2mm` and no objective, so that the answer needs no default objective.
- Needs: instance, constraint, summary, unknown, solve-forced, purpose, grammar-purpose
- Answer basis: traced. `Bracket`'s own constraints give `t` ∈ [1 mm, 2 mm], both satisfied for any solution; every feasible value gives the rows pinned, which is what `solve-forced` asks (PRD section 5.6). The purpose `t ≥ 3 mm` is at `Bracket/purpose[0]`, inside the owner by path, and is left out of the problem by path (readings E34): violated for any solution. Had it borne, the problem would be unsatisfiable and both of `Bracket`'s verdicts undetermined.
- Today: no `.ri` file can apply a purpose (design Part 10).
- Note: the member line `purpose robust(self)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.
- Candidates: Bracket.t: 3mm, 1mm. 3 mm meets the purpose and breaks `t <= 2mm`; a reading in which only the purpose bore would take it, and a reading in which the purpose bears beside `Bracket`'s own finds no value. Under `t := 3mm` the definition's bound is violated and depended on the trial, so the first taken is 1 mm.

```ri
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Bracket {
    param t : Length = auto
    constraint t >= 1mm
    constraint t <= 2mm
    purpose robust(self)
}
```

| Path | Expect |
|---|---|
| Bracket/constraint[0] | verdict satisfied |
| Bracket/constraint[1] | verdict satisfied |
| Bracket/purpose[0]/constraint[0] | verdict violated |
| Bracket/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-applied-twice-gives-two-instances

- Tests: one purpose applied twice, to two subjects, gives two purpose instances with their own verdicts.
- Rule: Part 10 ("A purpose may be applied any number of times"); decision 12 (same rule).
- Provenance: design Part 10; design decision 12 list. claims-1 part D item 10; language-1 L6.
- Needs: instance, constraint, summary, purpose, grammar-purpose
- Answer basis: traced. `k` in `/purpose[k]` is the position among the applications, in source order (readings A). `a.t` = 2 mm < 4 mm: satisfied. `b.t` = 5 mm < 4 mm: violated. `Bracket`'s own `t > 1mm` holds on both.
- Today: a second activation on another subject is dropped with no message (design Part 10, run there).
- Note: the two member lines `purpose thin_enough(...)` need grammar-purpose (readings E33). With them taken out, the source parses on main.

```ri
structure def Bracket { param t : Length = 5mm   constraint t > 1mm }
purpose thin_enough(subject : Structure) { constraint subject.t < 4mm }
structure def Asm {
    sub a : Bracket { t = 2mm }
    sub b : Bracket
    purpose thin_enough(self.a)
    purpose thin_enough(self.b)
}
```

| Path | Expect |
|---|---|
| Asm/purpose[0]/constraint[0] | verdict satisfied |
| Asm/purpose[1]/constraint[0] | verdict violated |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### value-as-purpose-subject-is-compile-error

- Tests: a value held in a cell cannot be a purpose's subject.
- Rule: decision 9; Part 10; Part 7.4.
- Provenance: design decision 9; design Part 10 ("A value held in a cell is refused"). design Part 7.4 table. claims-1 row 8; claims-3 decision 9; second-design Q5 and Q9.
- Needs: compile-error, purpose, grammar-purpose
- Answer basis: stated (decision 9: "a value held in a cell cannot be named as a subject"). The message follows readings C18: a subject names an instance, `self` or a sub or element below it, and not a value held in a cell.
- Today: a purpose made active on a `let`-bound value gives an undetermined verdict (design decision 9, report).
- Note: the member line `purpose thin_enough(v)` needs grammar-purpose (readings E33). With it taken out, the source parses on main. Which parameters are subjects is PRD D10(e), ruled by Leo on 2026-11-26 as recommended (readings C18): `subject : Structure` is one.

```ri
structure def Bracket { param t : Length = 5mm }
purpose thin_enough(subject : Structure) { constraint subject.t < 4mm }
structure def Asm {
    let v = Bracket()
    purpose thin_enough(v)
}
```

| Path | Expect |
|---|---|
| (compile) | error: a purpose's subject must name an instance (`self`, or a sub or element below it), not a value held in a cell; write `sub v : Bracket` |

### purpose-parameter-of-kind-constraint-rejected

- Tests: a purpose declared with a parameter of kind Constraint is refused for now, by a compile error at the declaration.
- Rule: Part 15.2; Part 10.
- Provenance: design Part 15.2 ("A purpose whose subject is a constraint or a field stays refused").
- Needs: compile-error, purpose
- Answer basis: traced. Part 15.2 states the refusal; readings C17 places it at the purpose's declaration, as a compile error, so the declaration alone is enough to raise it. The Field case stays without a model: a module-level `field def` is the geometry and compute design's (task 10611).

```ri
purpose tight(c : Constraint) { constraint true }
```

| Path | Expect |
|---|---|
| (compile) | error: a purpose's parameter may not be of kind Constraint |

### absent-purpose-subject-gives-undetermined-verdicts

- Tests: an applied purpose whose subject is absent is still present, and its constraints are undetermined, with one subject or two.
- Rule: Part 10 ("A subject that is not there. The purpose's constraints then read something that is not there and are undetermined"); Part 4; readings B1, B4, B5, C16 and E33.
- Provenance: logic-1 L14 (two subjects); logic-2 N16 (one subject: "no home to report in"); logic-3 R17 (the two cases as different classes; a home of undetermined presence). Taken: draft 5 makes the purpose a member of the applying definition, so it has a place whatever its subject; "subject absent" as its own class was not kept. design Part 10. language-2 B12; language-1 L1 read 1.
- Needs: instance, guard, constraint, summary, purpose, grammar-purpose
- Answer basis: stated (Part 10) for the verdicts; traced for the summary. `c.p` is absent, so the host answers `absent` at `Asm.c.p` (readings B1), and `subject.t` and `a.t`, read by the purposes' constraints, are undefined; `thin_enough`'s and `pair`'s constraints are undetermined. The purposes are applied in `Asm`, across an instance, so the compile rule of readings C16 does not fire. A subject is a place, not an argument, so neither purpose has an undetermined `/arguments` verdict (readings B5); neither has value parameters. Summary (readings B4): the two undetermined purpose verdicts; `Asm.c.p` is absent, not of undetermined presence, so it adds nothing.
- Note: the member lines `purpose thin_enough(self.c.p)` and `purpose pair(self.c.p, self.c.q)` need grammar-purpose (readings E33). With them taken out, the source parses on main.

```ri
structure def Plate {
    param t : Length = 5mm
}
structure def Case {
    param use : Bool = false
    sub p : Plate where use
    sub q : Plate
}
purpose thin_enough(subject : Structure) {
    constraint subject.t < 4mm
}
purpose pair(a : Structure, b : Structure) {
    constraint a.t + b.t < 20mm
}
structure def Asm {
    sub c : Case
    purpose thin_enough(self.c.p)
    purpose pair(self.c.p, self.c.q)
}
```

| Path | Expect |
|---|---|
| Asm.c.p | absent |
| Asm/purpose[0]/constraint[0] | verdict undetermined |
| Asm/purpose[1]/constraint[0] | verdict undetermined |
| Asm/summary | summary violated 0 undetermined 2 failed 0 |

### guarded-own-sub-as-purpose-subject-undetermined-when-absent

- Tests: applying a purpose to a guarded sub of the applying definition, on a line with no guard of its own, compiles; where the sub is absent the purpose's verdicts are undetermined and not partial.
- Rule: Part 10 (an applied purpose is a member of the definition it is written in; "A subject that is not there. The purpose's constraints then read something that is not there and are undetermined"); readings C16; Leo's ruling of 2026-11-26 on the PRD's D13 (a read of a member that may be absent compiles wherever it is written), which amends decision 3b line 1 and Part 4.
- Provenance: logic-1 L14's model with the purpose applied in the definition that holds the guarded sub. A compile error until Leo ruled. Renamed from `guarded-own-sub-as-purpose-subject-needs-a-guard`.
- Needs: compile-error, instance, guard, purpose, grammar-purpose, constraint, summary
- Answer basis: traced. `use` is false, so `Asm.p` is absent. The application line has no guard, so the purpose instance is there, and its constraint reads `subject.t` on a subject that is not: undetermined, other. Written `purpose thin_enough(self.p) where use`, neither the sub nor the purpose instance is there (`guarded-purpose-application-follows-its-guard`).
- Note: the member line `purpose thin_enough(self.p)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.

```ri
structure def Plate {
    param t : Length = 5mm
}
purpose thin_enough(subject : Structure) {
    constraint subject.t < 4mm
}
structure def Asm {
    param use : Bool = false
    sub p : Plate where use
    purpose thin_enough(self.p)
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.p | absent |
| Asm/purpose[0]/constraint[0] | undetermined-because other |
| Asm/summary | summary violated 0 undetermined 1 failed 0 |

### objective-in-purpose-is-compile-error

- Tests: an objective inside a purpose is a compile error that says to write it in the definition.
- Rule: ruling 1; Part 10.
- Provenance: design Part 10 (Today: a purpose's `minimize` is stored and never read); language-1 L6 point 5; language-2 B12 and D2 ("members" includes a purpose's `minimize`). Taken. Also, from the merged entries: design Part 10. logic-2 N16 (Also).
- Needs: compile-error, purpose
- Answer basis: stated (Part 10: "An objective inside a purpose is a compile error that says to write it in the definition"); readings C17 places it at the declaration, so the declaration alone raises it. The `Bracket` of the earlier source, which nothing applied the purpose to, is left out.
- Today: accepted with no diagnostic; `Bracket.t = 5.5 mm` (run `purpose_obj.ri`, with a `Bracket` whose `t` is `auto` in [1 mm, 10 mm]).

```ri
purpose lean(subject : Structure) { minimize subject.t }
```

| Path | Expect |
|---|---|
| (compile) | error: an objective inside purpose `lean`; write it in the definition |

### purpose-only-checks-subjects-own-auto

- Tests: a purpose applied in source never takes part in deciding an `auto`: the subject's own solve gives the same value as with no purpose, and the purpose's constraint is a violated verdict counted in the applying instance's summary.
- Rule: Part 10 (ruled 2026-11-26: a purpose only checks); decision 4 (its purpose half, reversed); contract Example 9 as reworked in design Part 12
- Provenance: task 9420 (purpose half; its design note says the ruling answers "no"); design decision 4 (its purpose model, here with a forced `auto`). design decision 4 purpose model; Part 12 Example 9. language-1 L6; second-design Q9; contract Example 9. logic-1 L4; logic-3 holds-up. contract Example 9 as restated in design Part 12.
- Needs: instance, constraint, summary, unknown, solve-forced, purpose, grammar-purpose, roots
- Answer basis: traced.
  `t` is owned by `Holder.b`, and the purpose instance is at `Holder/purpose[0]`, outside that owner by path, so its constraint would not bear under design 6.1 even with no ruling on purposes; it is also left out of every problem by path (readings E34). So `Holder.b`'s solve sees only `t == 2mm`, and t = 2 mm.
  The purpose's constraint reads `subject.t` = 2 mm against 3 mm, so it is violated, and it is counted in `Holder`'s summary (readings B4).
  If the purpose bore on its subject's `auto`, as the ruling of 2026-11-25 had it, no value would satisfy `t == 2mm` and `t >= 3mm` together, and `t` would be `value undefined` (readings B3). The model tells that ruling from today's; it does not test the exclusion of a purpose inside the owner, which `purpose-inside-owner-still-only-checks` and `purpose-braces-and-use-site-auto-on-one-sub` do (tracer, contradiction C6).
  `Bracket` is used by `Holder`, and a purpose is not a structure definition, so `Holder` is the only root.
- Today: with a purpose active, a cold evaluation solves without the purpose's constraints and the edit path solves with them (task 9420 record).
- Note: the member line `purpose robust(self.b)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.
- Candidates: Holder.b.t: 3mm, 2mm. 3 mm meets the purpose and breaks `t == 2mm`, which depended on the trial; the first taken is 2 mm.

```ri
structure def Bracket { param t : Length = auto   constraint t == 2mm }
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Holder {
    sub b : Bracket
    purpose robust(self.b)
}
```

| Path | Expect |
|---|---|
| (file) | roots Holder |
| Holder.b.t | value 2 mm |
| Holder.b/constraint[0] | verdict satisfied |
| Holder/purpose[0]/constraint[0] | verdict violated |
| Holder/summary | summary violated 1 undetermined 0 failed 0 |

### guarded-purpose-application-follows-its-guard

- Tests: a purpose applied under a guard is absent, with nothing counted, when the guard is false, and present and counted when it holds.
- Rule: Part 4 ("`where` means presence for every member kind"); Part 10 (an applied purpose is a member of the definition it is written in); readings C16 and E33; readings A (a purpose applied under a guard keeps its position when it is absent)
- Provenance: readings C16 (the form it names, `purpose p(self.fan) where needs_fan`); brief item 7.
- Needs: instance, guard, constraint, summary, purpose, grammar-purpose, roots
- Answer basis: traced. `Fan` is used by both, so the roots are `CaseOff` and `CaseOn`. In `CaseOff`, `needs_fan` is false: `fan` is absent and so is the purpose instance, which keeps position 0; no verdict, nothing counted. In `CaseOn` it is true: `fan.d` is 40 mm, and `40mm < 30mm` is violated, counted in `CaseOn`'s summary (readings B4).
- Note: the two member lines `purpose quiet(self.fan) where needs_fan` need grammar-purpose (readings E33). With them taken out, the source parses on main. The same line without its guard compiles and is undetermined where the sub is absent (readings C16): `guarded-own-sub-as-purpose-subject-undetermined-when-absent`.

```ri
structure def Fan {
    param d : Length = 40mm
}
purpose quiet(subject : Structure) {
    constraint subject.d < 30mm
}
structure def CaseOff {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
    purpose quiet(self.fan) where needs_fan
}
structure def CaseOn {
    param needs_fan : Bool = true
    sub fan : Fan where needs_fan
    purpose quiet(self.fan) where needs_fan
}
```

| Path | Expect |
|---|---|
| (file) | roots CaseOff, CaseOn |
| CaseOff/purpose[0] | absent |
| CaseOff/summary | summary violated 0 undetermined 0 failed 0 |
| CaseOn/purpose[0]/constraint[0] | verdict violated |
| CaseOn/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-with-subject-and-value-parameter

- Tests: a purpose with a subject and a value parameter, applied with positional and with named arguments, binds the subject to a place and the value as an argument, and each application gives its own verdict.
- Rule: Part 10; readings C18 (a parameter whose declared type is a structure, a trait or `Structure` is a subject; any other is a value; PRD D10(e), ruled by Leo on 2026-11-26 as recommended); readings B5 (the value parameters of an applied purpose have an `/arguments` verdict); readings E33 (arguments as in a constructor call)
- Provenance: PRD D10(e); brief item 10.
- Needs: instance, constraint, summary, purpose, grammar-purpose
- Answer basis: traced. `subject : Structure` is a subject, bound to `Asm.b`; `limit : Length` is a value. `purpose[0]`: 5 mm < 4 mm, violated. `purpose[1]`: 5 mm < 6 mm, satisfied. `limit` is defined in both, so `purpose[0]`'s argument verdict is satisfied. Counted (readings B4): violated 1.
- Note: the two member lines `purpose thin_below(...)` need grammar-purpose (readings E33). With them taken out, the source parses on main. Which parameters are subjects is PRD D10(e) as ruled (readings C18): by the declared type; `self.b` names an instance and `4mm` and `6mm` are values.

```ri
structure def Bracket {
    param t : Length = 5mm
}
purpose thin_below(subject : Structure, limit : Length) {
    constraint subject.t < limit
}
structure def Asm {
    sub b : Bracket
    purpose thin_below(self.b, 4mm)
    purpose thin_below(subject: self.b, limit: 6mm)
}
```

| Path | Expect |
|---|---|
| Asm/purpose[0]/constraint[0] | verdict violated |
| Asm/purpose[1]/constraint[0] | verdict satisfied |
| Asm/purpose[0]/arguments | verdict satisfied |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-with-no-parameter-applied-empty

- Tests: a purpose with no parameter is applied as `purpose name()`, gives one purpose instance, and its verdict is counted.
- Rule: readings C18 ("A purpose with no parameter is applied as `purpose name()`"); PRD D10(e); Part 10
- Provenance: PRD D10(e); design Part 10 (4 of the 42 tracked purposes have no subject); the declaration form of `crates/reify-cli/tests/fixtures/purpose_zero_param.ri`.
- Needs: compile-error, instance, constraint, summary, purpose, grammar-purpose
- Answer basis: stated (readings C18) for the form; traced for the verdict: `2mm < 1mm` is false, violated, counted in `Asm`'s summary (readings B4).
- Note: the member line `purpose impossible()` needs grammar-purpose (readings E33). With it taken out, the source parses on main.

```ri
purpose impossible() {
    constraint 2mm < 1mm
}
structure def Asm {
    param t : Length = 3mm
    purpose impossible()
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm/purpose[0]/constraint[0] | verdict violated |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-body-applies-constraint-definition

- Tests: a purpose's body may apply a constraint definition with `constraint Name(args)`; the application compiles and gives the purpose's verdict, while the same definition applied in a structure is an ordinary constraint of it.
- Rule: Part 10 ("A `constraint def` gives it a name to reuse, and a purpose can call the same definition to check it"); PRD D13 item 10
- Provenance: design Part 10; PRD D13 item 10; the `constraint def` form of spec 4.1.3 and `examples/integration_full_v01.ri` (`InRange`).
- Needs: compile-error, instance, constraint, summary, purpose, grammar-purpose
- Answer basis: traced. In `Asm.b`, `AtMost(v: 5mm, hi: 8mm)` holds: `Asm.b/constraint[0]` satisfied. The purpose applied to `Asm.b` gives `AtMost(v: 5mm, hi: 4mm)`: violated. `AtMost`'s body has one predicate line, so each application is one verdict. Counted (readings B4): violated 1.
- Today: run `g11s.ri` (the application line taken out): "warning: unresolved function: AtMost" for the line in the purpose's body.
- Note: the member line `purpose thin(self.b)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.

```ri
constraint def AtMost {
    param v : Length
    param hi : Length
    v <= hi
}
structure def Bracket {
    param t : Length = 5mm
    constraint AtMost(v: t, hi: 8mm)
}
purpose thin(subject : Structure) {
    constraint AtMost(v: subject.t, hi: 4mm)
}
structure def Asm {
    sub b : Bracket
    purpose thin(self.b)
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.b/constraint[0] | verdict satisfied |
| Asm/purpose[0]/constraint[0] | verdict violated |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-braces-and-use-site-auto-on-one-sub

- Tests: on one sub with a use-site `auto`, a braces constraint and a purpose applied by the container: the container owns the `auto`, the braces constraint and the definition's own bear on it, and the purpose, inside the owner by path, only checks.
- Rule: seat-context rulings 1, 3 and 4; Part 6.1; Part 10; readings E34.
- Provenance: the tracer's interaction I1 (trace-braces-purposes-unknowns); the form decision 4 offers (`{ shaft_d = auto   constraint shaft_d > 15mm }`). It tests the exclusion of a purpose by path for a container that owns the unknown (tracer, contradiction C6).
- Needs: instance, braces, constraint, summary, unknown, solve-forced, purpose, grammar-purpose
- Answer basis: traced. The `auto` is written in `Holder`'s text, so `Holder` owns it, at `Holder/auto[b.t]`. `Bracket`'s `t >= 2mm` at `Holder.b` and the braces' `t <= 2mm`, a member of `Holder.b`, are inside `Holder` and bear: exactly 2 mm. The purpose instance is at `Holder/purpose[0]`, inside `Holder` by path, and is left out of the problem by path (readings E34): violated. Had it borne, no value. `Holder.b`'s argument is decided, so its `/arguments` verdict is satisfied (readings B5). Summary (readings B4): violated 1.
- Note: the member line `purpose robust(self.b)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.
- Candidates: Holder/auto[b.t]: 3mm, 1mm, 2mm. 3 mm meets the purpose and `Bracket`'s bound and breaks the braces' constraint (a reading in which the braces do not bear and the purpose does takes it); 1 mm meets the braces and breaks `Bracket`'s `t >= 2mm` (a reading in which the definition's constraints do not bear on a use-site `auto` takes it). The first taken is 2 mm.

```ri
structure def Bracket { param t : Length = 5mm   constraint t >= 2mm }
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Holder {
    sub b : Bracket { t = auto   constraint t <= 2mm }
    purpose robust(self.b)
}
```

| Path | Expect |
|---|---|
| Holder.b.t | value 2 mm |
| Holder.b/constraint[0] | verdict satisfied |
| Holder.b/braces/constraint[0] | verdict satisfied |
| Holder/purpose[0]/constraint[0] | verdict violated |
| Holder/summary | summary violated 1 undetermined 0 failed 0 |

### purpose-applied-in-made-definition-checked-per-instance

- Tests: a purpose applied with `self` inside a definition is checked on every instance of it, a sub and an instance a constructor call makes alike, and each verdict is counted once.
- Rule: design 7.4 (an applied purpose is the same on a value); Part 10; readings B4 (a made instance's verdicts are counted once, at the made instance); readings C18 (`self` is a subject).
- Provenance: the tracer's interaction I5 (trace-braces-purposes-unknowns). The tracer's contradiction C7 (decision 9's "place" against 7.4) is answered by readings C18: the subject is `self`, an instance, whether the instance is a sub or made.
- Needs: instance, constraint, summary, made, purpose, grammar-purpose
- Answer basis: traced. `Bracket` is used by `Asm`, so `Asm` is the root. `Asm.v/made[0]` and `Asm.s` are both `Bracket` instances with `t` 2 mm, and each declares its own purpose instance: 2 mm ≥ 3 mm is violated at each. The made instance's verdict is counted once, at the made instance. Summary: violated 2.
- Note: the member line `purpose robust(self)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.

```ri
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Bracket {
    param t : Length = 2mm
    purpose robust(self)
}
structure def Asm {
    let v = Bracket()
    sub s : Bracket
}
```

| Path | Expect |
|---|---|
| Asm.v/made[0]/purpose[0]/constraint[0] | verdict violated |
| Asm.s/purpose[0]/constraint[0] | verdict violated |
| Asm/summary | summary violated 2 undetermined 0 failed 0 |

### instance-made-in-purpose-constraint-does-not-bear

- Tests: an instance made by an expression inside a purpose's constraint is at or below the purpose instance, so its constraints never bear on an unknown, though they are inside the owner by path.
- Rule: seat-context ruling 1; readings E34 (the exclusion is by path); Part 6.1; PRD section 5.6 ("everything at or below a purpose instance" is left out of `problem`).
- Provenance: the tracer's counter-example N2 (trace-braces-purposes-unknowns), and gap G4.
- Needs: instance, constraint, summary, unknown, solve-forced, made, open, purpose, grammar-purpose
- Answer basis: traced. `t` is `Bracket`'s, and `t == 2mm` bears: 2 mm. The purpose's constraint makes a `Gauge` at `Bracket/purpose[0]/constraint[0]/made[0]`, whose `t >= 3mm` depends on `Bracket.t` and is inside `Bracket` by path. It is below the purpose instance, so it is left out of the problem (readings E34): `t` is decided as without the purpose. The made `Gauge`'s verdict is violated (2 mm ≥ 3 mm) and counted once, at the made instance (readings B4); the purpose's own constraint reads the opened value, 2 mm > 0 mm, satisfied. Summary: violated 1. Under design 6.1 alone the `Gauge` constraint would bear, no value would satisfy both, and `t` would be undefined.
- Note: the member line `purpose fits(self)` needs grammar-purpose (readings E33). With it taken out, the source parses on main.
- Candidates: Bracket.t: 3mm, 2mm. 3 mm meets the `Gauge` constraint and breaks `t == 2mm`; a reading in which the made instance's constraint bears rejects both and leaves `t` undefined. The first taken is 2 mm.

```ri
structure def Gauge { param t : Length   constraint t >= 3mm }
purpose fits(subject : Structure) { constraint Gauge(t: subject.t).t > 0mm }
structure def Bracket {
    param t : Length = auto
    constraint t == 2mm
    purpose fits(self)
}
```

| Path | Expect |
|---|---|
| Bracket.t | value 2 mm |
| Bracket/purpose[0]/constraint[0] | verdict satisfied |
| Bracket/purpose[0]/constraint[0]/made[0]/constraint[0] | verdict violated |
| Bracket/summary | summary violated 1 undetermined 0 failed 0 |

### constrained-ignores-purpose-constraint

- Tests: a purpose's constraint does not make `constrained(p)` true, so applying a purpose changes no value.
- Rule: readings B7 (`constrained(p)`; PRD D3, ruled by Leo on 2026-11-26 as recommended: a constraint the declarer writes in a sub's braces does not count, and neither does a purpose's); seat-context ruling 1.
- Provenance: the tracer's counter-example N3 (trace-braces-purposes-unknowns); critic-design F4.
- Needs: compile-error, instance, constraint, purpose, grammar-purpose, state-tests
- Answer basis: stated for `Bracket.w`: PRD D3 as Leo ruled it. No constraint written in `Bracket`'s definition reads `t` and a purpose's does not count, so `constrained(t)` is false and `w` is 1 mm. Traced for the rest: the source compiles, and the purpose's constraint reads `t` = 2 mm against 3 mm, violated. Under D3's reading before its revision (a present constraint inside the instance that holds `p`), the purpose's constraint, inside `Bracket` by path, would count, and `w` would be 5 mm with the purpose line and 1 mm without it.
- Note: the member line `purpose robust(self)` needs grammar-purpose (readings E33); with it taken out, the source parses on main.

```ri
purpose robust(subject : Structure) { constraint subject.t >= 3mm }
structure def Bracket {
    param t : Length = 2mm
    let w = if constrained(t) then 5mm else 1mm
    purpose robust(self)
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Bracket/purpose[0]/constraint[0] | verdict violated |
| Bracket.w | value 1 mm |

### purpose-read-of-private-let-rejected

- Tests: a purpose's body reads only the `pub` members of its subject: a read of a private `let` is a compile error at the purpose's declaration.
- Rule: spec 7.4 and 8.3; PRD D13 item 14, ruled by Leo on 2026-11-26; readings E38 (a purpose is another definition: the lead's reading of the ruling).
- Provenance: Leo's ruling on the PRD's D13 item 14.
- Needs: compile-error, purpose
- Answer basis: stated: readings E38. The subject's declared type is `Plate`, whose `area` is a `let` without `pub`. The purpose is declared and never applied; the error is in its body.
- Today: run `priv3.ri`: compiles.

```ri
structure def Plate {
    param t : Length = 3mm
    let area = t * 40mm
}
purpose big_enough(subject : Plate) { constraint subject.area > 100mm * 1mm }
structure def Asm { sub p : Plate }
```

| Path | Expect |
|---|---|
| (compile) | error: the purpose `big_enough` reads `area`, a `let` of `Plate` that is not `pub` |


---

## Unknowns and who owns them

### auto-owned-by-writer-sub-and-value-alike

- Tests: An `auto` belongs to the instance whose definition's text contains it, so a constraint above that instance only checks it, for a sub as for a value.
- Rule: decision 1; Part 6.1; ruling 4 of `seat-context.md`
- Provenance: design Part 14 decision 1 (model); Part 6.1. language-1 L10; claims-3 probes d1.ri and d1b.ri; second-design trace of contract Example 5. logic-2 Part C; logic-3 holds-up (55 mm, violated).
- Needs: instance, constraint, unknown, made, open, roots, solve-real
- Answer basis: stated: decision 1, "55 mm, and the constraint is violated" for both. Trace: the `auto` is in `Asm`'s text; `Beam`'s bounds are inside the `Asm` instance and bear; middle of [10, 100] is 55 mm. The forced form, which runs without a real solver, is `parent-constraint-only-checks-forced-child-auto`.
- Today: neither path compiles on the old binary (design decision 1).

```ri
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm  { sub a : Beam { len = auto } }
structure def TopS { sub asm : Asm      constraint self.asm.a.len == 70mm }
structure def TopV { let asm = Asm()    constraint asm.a.len == 70mm }
```

| Path | Expect |
|---|---|
| (file) | roots TopS, TopV |
| TopS.asm.a.len | value 55 mm |
| TopS/constraint[0] | verdict violated |
| TopV.asm/made[0].a.len | value 55 mm |
| TopV.asm/open.a.len | value 55 mm |
| TopV/constraint[0] | verdict violated |

### use-site-auto-borne-through-passed-value

- Tests: An `auto` written at a constructor call is borne on by every constraint inside the creator that reads the result, including those of subs the value is passed to.
- Rule: Part 6.1 ("'Bears on' follows every chain of reads"); decision 5; contract Example 11
- Provenance: contract Example 11 ("With `let c = Plate(t: auto)`, `sub d = Widget(c)` and `sub e = Fidget(c)`, there is one unknown, and the constraints inside `Widget` and `Fidget` both bear on it"); design Part 6.1
- Needs: instance, constraint, unknown, made, open, nested-solve, solve-real
- Answer basis: traced. One unknown, `Asm`'s, at `Asm/auto[c#0.t]`. Bearing: `t >= 3mm` (made instance), `p.t >= 4mm` (in `Asm.d`), `q.t <= 6mm` (in `Asm.e`). Range [4, 6], middle 5 mm.
- Candidates: Asm/auto[c#0.t]: 3mm, 7mm, 5mm. 3 mm meets the made instance's bound and breaks `Widget`'s (a reading in which the constraints of subs the value is passed to do not bear takes it); 7 mm breaks `Fidget`'s. The first taken is 5 mm, the pinned middle; the entry waits for task 10610 because that middle is the default objective's.

```ri
structure def Plate  { param t : Length   constraint t >= 3mm }
structure def Widget { param p : Plate    constraint p.t >= 4mm }
structure def Fidget { param q : Plate    constraint q.t <= 6mm }
structure def Asm    { let c = Plate(t: auto)   sub d = Widget(p: c)   sub e = Fidget(q: c) }
```

| Path | Expect |
|---|---|
| Asm.c/made[0].t | value 5 mm |
| Asm.d/constraint[0] | verdict satisfied |
| Asm.e/constraint[0] | verdict satisfied |

### solve-across-recursive-instance-left-open

- Tests: Whether the constraints of a recursive sub's levels bear on an unknown the root passes into it.
- Rule: 6.4 (third question); Part 4 (recursion is a guarded sub)
- Provenance: design Part 6.4, "Three questions", third; critic logic-1 (trace through `RecTree`); spec 8.9
- Needs: instance, guard, unknown
- Answer basis: open for `s` and every verdict, owner task 10610: rule 6.1 read plainly gives `s` = 40 mm (the third level needs s / 4 >= 10 mm); spec 8.9's "Resolution does not cross recursive instance boundaries" gives, on one critic's reading, 1 mm with the tree's constraints violated. The contract leaves it to the solve design. The structure does not depend on `s` and is pinned; no pinned row reads a solve, so the entry runs (readings F, last note). The braces hold argument assignments only, so the entry does not need `braces`.

```ri
structure def RecTree {
    param span : Length
    param depth : Int = 2
    constraint span >= 10mm
    sub child : RecTree where depth > 0 { span = self.span / 2   depth = self.depth - 1 }
}
structure def Top { let s : Length = auto   sub tree = RecTree(span: s)   minimize s }
```

| Path | Expect |
|---|---|
| Top.tree.child.child | present |
| Top.tree.child.child.depth | value 0 |
| Top.tree.child.child.child | absent |

### auto-in-constructor-argument-is-holders-unknown

- Tests: `auto` in the argument of a constructor call evaluated exactly once is an unknown of the instance whose member holds the call, and the callee's constraints bear on it.
- Rule: decision 5; Part 6.3 rule 1
- Provenance: design Part 14 decision 5 (model); Part 6.3 table; Part 12 Example 11 (`let d = Plate(t: auto)`). language-1 L11; second-design Q6.
- Needs: compile-error, instance, constraint, unknown, made, open, nested-solve, solve-real
- Answer basis: stated: decision 5, "5.5 mm". The unknown is `Asm`'s, at `Asm/auto[d#0.t]`; `Plate`'s `t >= 3mm` at the made instance and `Asm`'s `d.t <= 8mm` bear: [3 mm, 8 mm], middle 5.5 mm.
- Today: "argument 't' has type 'Real' but param 't' requires type 'Scalar[m]'" (run, `p6.ri`); for a plain number, a silent undefined (design Part 6.3).
- Candidates: Asm/auto[d#0.t]: 2mm, 9mm, 5.5mm. 2 mm breaks the made instance's bound (a reading in which the callee's constraints do not bear takes it); 9 mm breaks `Asm`'s (a reading in which the holder's constraint only checks takes it). The first taken is 5.5 mm, the pinned middle; the entry waits for task 10610 because that middle is the default objective's.

```ri
structure def Plate { param t : Length   constraint t >= 3mm }
structure def Asm   { let d = Plate(t: auto)   constraint d.t <= 8mm }
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.d/made[0].t | value 5.5 mm |
| Asm.d/made[0]/constraint[0] | verdict satisfied |
| Asm/constraint[0] | verdict satisfied |

### auto-in-function-body-is-compile-error

- Tests: `auto` in a constructor argument inside a function body is a compile error that says to write a typed `let` outside and pass it in.
- Rule: Part 6.3 rule 2; decision 5
- Provenance: design Part 6.3 table (lambda or function body; a quantifier's body by the same rule); contract Example 11. design 6.3 table; language-1 L11; claims-1 row 22.
- Needs: compile-error, unknown
- Answer basis: stated: Part 6.3.
- Today: a type error for a length, a silent undefined for a plain number (design Part 6.3, run).

```ri
structure def Plate { param t : Length   constraint t >= 3mm }
fn mk() -> Plate = Plate(t: auto)
structure def Asm { let d = mk() }
```

| Path | Expect |
|---|---|
| (compile) | error: `auto` inside a function body; write `let u : Length = auto` outside it and pass `u` in |

### auto-in-conditional-arm-is-compile-error

- Tests: `auto` in a constructor argument inside an arm of an `if` is a compile error that says to use `where` or a collection.
- Rule: Part 6.3 rule 2; decision 5
- Provenance: design Part 6.3 table (an arm of an `if` or a `match`). language-1 L11 point 3; second-design Q6.
- Needs: compile-error, unknown
- Answer basis: stated: Part 6.3.

```ri
structure def Plate { param t : Length   constraint t >= 3mm }
structure def Asm {
    param big : Bool = true
    let d = if big then Plate(t: auto) else Plate(t: 4mm)
}
```

| Path | Expect |
|---|---|
| (compile) | error: `auto` inside an arm of an `if`; put the member under `where`, or use a collection |

### untyped-let-auto-is-rejected

- Tests: `auto` as the value of a `let` with no type is rejected.
- Rule: Part 6.3 rule 1 and table (row "A typed `let`")
- Provenance: design Part 6.3 table ("The untyped form is rejected", unchanged by the design)
- Needs: compile-error, unknown
- Answer basis: stated: Part 6.3 table. The typed form, an unknown of the instance, is pinned by `absent-sub-unknown-left-out-of-solve`.

```ri
structure def Asm { let u = auto   constraint u >= 1mm }
```

| Path | Expect |
|---|---|
| (compile) | error: `let u = auto` needs a type, as in `let u : Length = auto` |

---

### use-site-autos-solved-with-child-bounds

- Tests: two `auto`s written at the use sites of two siblings are unknowns of the parent, decided together by the parent's constraints and both children's bounds, and the children's bounds are checked on the instances.
- Rule: contract Example 5; Part 12 (Example 5); Part 6.1; Part 6.2; decision 1.
- Provenance: contract Part B Example 5; design Part 12 "Example 5". second-design trace Example 5.
- Needs: instance, constraint, summary, unknown, solve-forced
- Answer basis: traced. Both unknowns belong to `Asm`, at `Asm/auto[a.len]` and `Asm/auto[b.len]`; `a.len + b.len = 120 mm` and `a.len − b.len = 20 mm` force 70 mm and 50 mm. Both lie in [10 mm, 100 mm], so all six constraints are satisfied. That the children's bounds bear, not just get checked, is shown by `child-bounds-make-use-site-solve-unsatisfiable`.
- Today: with the two constants changed to 200 mm and 100 mm the solve returned 150 mm against the bound of 100 mm, so `Beam`'s bounds were not applied to the instance (contract Example 5, run).
- Candidates: Asm/auto[a.len]: 60mm, 70mm. Asm/auto[b.len]: 60mm, 50mm. (60 mm, 60 mm) meets every constraint but `Asm/constraint[1]`, which a reading in which that constraint only checks would take. Tried as pairs in order, or as every combination in order, the first taken is (70 mm, 50 mm).

```ri
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Rod  { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm {
    sub a : Beam { len = auto }
    sub b : Rod  { len = auto }
    constraint self.a.len + self.b.len == 120mm
    constraint self.a.len - self.b.len == 20mm
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 70 mm |
| Asm.b.len | value 50 mm |
| Asm.a/constraint[1] | verdict satisfied |
| Asm/constraint[0] | verdict satisfied |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### child-bounds-make-use-site-solve-unsatisfiable

- Tests: a child's own bounds bear on an `auto` written at its use site, so a parent requirement beyond them makes the solve fail, its unknowns undetermined, and every verdict that reads them undetermined.
- Rule: contract Example 5; Part 6.1; decision 4 (a conflicting constraint is unsatisfiable); contract section 8 ("A failed solve is a stored result. Each unknown it owned is then undetermined").
- Provenance: the contract's run under Example 5 (constants 200 mm and 100 mm).
- Needs: instance, constraint, summary, unknown, solve-infeasible
- Answer basis: traced. The equalities force `a.len` = 150 mm, `b.len` = 50 mm; `Beam`'s `len ≤ 100 mm` is inside `Asm` and bears, so no point is feasible and any correct solver finds none. Each unknown it owned is `value undefined` (readings B3); every constraint reading one is undetermined: Asm's 2, Beam's 2, Rod's 2. Each sub's argument is an unknown left undefined by the failed solve, so each `/arguments` verdict is undetermined (readings B5). Summary by readings B4: 6 + 2 = 8 undetermined. The solve node's own path and outcome are the solve design's (task 10610) and are not pinned; a failed solve is not a failure (readings B3).
- Today: the solve returned 150 mm and `Beam`'s bound was not checked on the instance (contract Example 5, run).
- Candidates: Asm/auto[a.len]: 150mm, 100mm. Asm/auto[b.len]: 50mm, 100mm. (150 mm, 50 mm) meets both equalities and breaks `Beam`'s `len <= 100mm`; a reading in which the child's bounds do not bear at the instance takes it, which is the old engine's defect. (100 mm, 100 mm) breaks `Asm/constraint[1]`. As pairs in order or as every combination, each candidate breaks a constraint that depended on the trial, so none is taken.

```ri
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Rod  { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm {
    sub a : Beam { len = auto }
    sub b : Rod  { len = auto }
    constraint self.a.len + self.b.len == 200mm
    constraint self.a.len - self.b.len == 100mm
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value undefined |
| Asm.b.len | value undefined |
| Asm/constraint[0] | verdict undetermined |
| Asm.a/constraint[1] | verdict undetermined |
| Asm.a/arguments | verdict undetermined |
| Asm.b/arguments | verdict undetermined |
| Asm/summary | summary violated 0 undetermined 8 failed 0 |

### definition-autos-only-checked-by-parent

- Tests: an `auto` written in the definition is decided inside each instance from that instance's constraints alone; the parent's constraints are only checked against the results.
- Rule: contract Example 5 (last paragraph); Part 12 (Example 5); Part 6.1; decision 1.
- Provenance: contract Part B Example 5, "If `len = auto` is written in `Beam` itself"; design Part 12 "Example 5".
- Needs: instance, constraint, summary, unknown, solve-real
- Answer basis: traced, with the design's assumption that the default objective picks the middle of the feasible range: each `len` is 55 mm. 110 mm ≠ 120 mm: violated. 0 ≠ 20 mm: violated (this one holds for any solver that gives two identical problems the same answer).
- Today: that form gives the instances no `len` at all (contract Example 5, run).

```ri
structure def Beam { param len : Length = auto   constraint len >= 10mm   constraint len <= 100mm }
structure def Rod  { param len : Length = auto   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm {
    sub a : Beam
    sub b : Rod
    constraint self.a.len + self.b.len == 120mm
    constraint self.a.len - self.b.len == 20mm
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 55 mm |
| Asm.b.len | value 55 mm |
| Asm/constraint[0] | verdict violated |
| Asm/constraint[1] | verdict violated |
| Asm/summary | summary violated 2 undetermined 0 failed 0 |

### definition-objective-on-use-site-auto-open

- Tests: `Plate(t: auto)` in a `let` evaluated exactly once compiles and gives the holder an unknown on which `Plate`'s constraint and the holder's constraint both bear.
- Rule: contract Example 11, lines (3) and (4); Part 6.3; Part 12 (Example 11).
- Provenance: contract Part B Example 11, lines (3), (4); design Part 12 "Example 11".
- Needs: compile-error, unknown, made, nested-solve
- Answer basis: open. `d.t` is `Asm`'s unknown with `t ≥ 3 mm` and `t > 5 mm` bearing and nothing bounding it above; `Plate`'s `minimize t` does not reach an unknown moved to the use "unless the solve design says so" (Part 6.5). So the value of `Asm.d/made[0].t` and the verdict of `Asm/constraint[0]` depend on the solve design: open, owner task 10610 (readings E37). Only compilation is pinned, so the entry runs (readings F, last note).
- Today: form (3) does not compile (contract Example 11); a length `auto` in a constructor argument is a type error (design Part 6.3, run).

```ri
structure def Plate {
    param t : Length = auto
    constraint t >= 3mm
    minimize t
}
structure def Asm {
    let d = Plate(t: auto)
    constraint d.t > 5mm
}
```

| Path | Expect |
|---|---|
| (compile) | ok |

### child-auto-decided-per-instance

- Tests: a child's own `auto` is decided once per instance, from that instance's arguments.
- Rule: Part 13 row 2; Part 6.1; decision 1.
- Provenance: design Part 13 row "A child's own `auto` is never decided per instance (9604)". task 9604; task 7064 (its read half).
- Needs: instance, unknown, solve-forced
- Answer basis: traced. Each instance owns its `len`; `len = 2 × base` forces 2 mm for `a` and 4 mm for `b`. The row rests on the solve kinds; a forced model is decided by any solver, this crate's test solver included.
- Today: no `len` on either instance; `Beam.len` is solved once at the definition, 2 mm (run, `perinst.ri`).
- Candidates: Asm.a.len: 4mm, 2mm. Asm.b.len: 2mm, 4mm. 2 mm for `b` is the value decided once at the definition and shared, as today's engine does; 4 mm for `a` is `b`'s. Each breaks `len == base * 2` under its own instance's `base`, so the first taken are 2 mm and 4 mm.

```ri
structure def Beam {
    param base : Length = 1mm
    param len : Length = auto
    constraint len == base * 2
}
structure def Asm {
    sub a : Beam { base = 1mm }
    sub b : Beam { base = 2mm }
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 2 mm |
| Asm.b.len | value 4 mm |

### absent-sub-unknown-left-out-of-solve

- Tests: an `auto` written on a guarded sub is absent with the sub, and leaves the other unknowns of its owner solvable.
- Rule: Part 6.2 ("When the sub is absent, so is the unknown, and a solve leaves it out"); Part 6.4 requirement 4; decision 3.
- Provenance: logic-1 L8 (taken: 6.2 and 6.4); logic-2 holds-up (`x` = 20 mm). design 6.4 requirement 4; 6.2. `Beam`'s bounds changed on 2026-11-26 from 10 mm to 7 mm, so that the absent constraint, had it stayed in the problem, would make it infeasible and the entry would show what bears; the answers are unchanged.
- Needs: instance, guard, constraint, summary, unknown, solve-forced
- Answer basis: stated (Part 6.2 and 6.4 requirement 4). `use_b` is false, so `b`, its unknown `len` (at `Asm/auto[b.len]`) and the constraint under `where use_b` are absent; `x == 20mm` alone decides `x`. Had the absent sub and constraint stayed in the problem, `b.len` = 7 mm would need `x` = 14 mm, against `x == 20mm`: no value. The braces hold an argument assignment only, so the entry does not need `braces`.
- Today: run `l8.ri` (bounds of 10 mm): `Asm.x` about 20 mm, `Asm.b.len = undef` with W_UNDERDETERMINED.
- Candidates: Asm.x: 14mm, 20mm. 14 mm meets the absent constraint with `b.len` at 7 mm and breaks `x == 20mm`; the first taken is 20 mm, and under a reading that keeps the absent members in the problem none is taken.

```ri
structure def Beam {
    param len : Length
    constraint len >= 7mm
    constraint len <= 7mm
}
structure def Asm {
    param use_b : Bool = false
    let x : Length = auto
    constraint x == 20mm
    sub b : Beam where use_b {
        len = auto
    }
    constraint self.b.len == x / 2 where use_b
}
```

| Path | Expect |
|---|---|
| Asm.x | value 20 mm |
| Asm.b | absent |
| Asm/constraint[1] | absent |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### undetermined-presence-in-solve-left-open

- Tests: what a solve does with an unknown, and a constraint, whose presence is undetermined.
- Rule: Part 6.2; Part 6.4 (second question handed over); decision 3.
- Provenance: logic-2 N6; logic-3 R11. design 6.4, second question; logic-3 R11. Not taken: draft 5 hands the question to the solve design (6.4, "What a group does when a constraint that might bear on it is undetermined").
- Needs: instance, guard, unknown
- Answer basis: open for `x`, for `Asm/constraint[0]` and for the summary: is `x` solved from `x == 20mm` alone (20 mm, with the guarded constraint reported undetermined), or undetermined? Owner: task 10610. The presence rows are stated (Part 6.2, Part 4; readings E28): `use_b` has no default at the root, so it is undefined, and the guarded sub and constraint are of undetermined presence. No pinned row reads a solve, so the entry runs (readings F, last note).

```ri
structure def Beam {
    param len : Length
    constraint len == 7mm
}
structure def Asm {
    param use_b : Bool
    let x : Length = auto
    constraint x == 20mm
    sub b : Beam where use_b {
        len = auto
    }
    constraint self.b.len == x / 2 where use_b
}
```

| Path | Expect |
|---|---|
| Asm.b | presence-undetermined |
| Asm/constraint[1] | presence-undetermined |

### owner-constraint-bears-through-child-solve

- Tests: a constraint inside the owner bears on its unknown through a child's own solve, so the owner chooses the argument that makes the child's result meet it.
- Rule: decision 1; Part 6.1 ("'Bears on' follows every chain of reads ... through a child's own solve").
- Provenance: logic-1 L7 (taken: 6.1's transitive "bears on"; logic-2 Part A row L7 closed, `h` = 60 mm).
- Needs: instance, constraint, unknown, solve-forced, nested-solve
- Answer basis: traced. `a.len` (h) is `Asm`'s unknown, at `Asm/auto[a.len]`. `a`'s solve gives `k` = 100 mm − h for any h, under each trial of h (so the entry needs `nested-solve`; tracer, wrong tags). `Asm`'s constraint is inside `Asm` and reads `a.k`, so it bears on h: h = 60 mm is the only value. Then `k` is 40 mm and the constraint is satisfied. No note of 6.5 applies, since an unknown of `Asm` lies on the chain. The braces hold an argument assignment only, so the entry does not need `braces`.
- Candidates: Asm/auto[a.len]: 50mm, 60mm. Asm.a.k: 40mm, 50mm. Under h = 50 mm, `a`'s solve rejects 40 mm (it breaks `k == 100mm - len`) and takes 50 mm, and then `Asm/constraint[0]` is violated and depended on the trial, so 50 mm is rejected; a reading in which `Asm`'s constraint only checks takes it. Under h = 60 mm, `a` takes 40 mm and everything holds. The first taken are 60 mm and 40 mm.

```ri
structure def Beam {
    param len : Length
    param k : Length = auto
    constraint k == 100mm - len
}
structure def Asm {
    sub a : Beam {
        len = auto
    }
    constraint self.a.k == 40mm
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 60 mm |
| Asm.a.k | value 40 mm |
| Asm/constraint[0] | verdict satisfied |

### child-default-objective-shapes-parent-answer

- Tests: under ownership the parent sees the child's result as a function of the argument, so the child's own default objective shapes the parent's answer.
- Rule: Part 6.1 ("Each instance chooses its own unknowns from the constraints inside it, given its arguments ... The answers must be the same"); decision 1.
- Provenance: logic-2 N11, second model, and logic-2 doubts ("6.1's bilevel meaning makes an outer answer depend on an inner default objective ... deserves a model in decision 1"). Not taken as a model in the design.
- Needs: instance, constraint, unknown, nested-solve, solve-real
- Answer basis: traced. Given h = `a.len`, `a` decides `k` on [0, h] by the default objective: h/2. `Asm`'s `a.k == 30mm` bears on h: h = 60 mm, which meets `a.len <= 100mm`. One joint solve with the default objective over both would put h elsewhere in [30 mm, 100 mm]. The braces hold an argument assignment only, so the entry does not need `braces`.
- Candidates: Asm/auto[a.len]: 20mm, 120mm, 60mm. Asm.a.k: 30mm. Under h = 20 mm, `a`'s solve finds no `k` (30 mm breaks `k <= len`), so `Asm/constraint[0]` is undetermined because of the trial and 20 mm is rejected; a solver that rejects only a violated constraint takes it (critic-design F1). Under h = 120 mm, `a.len <= 100mm` is violated. The first taken is 60 mm with `k` 30 mm. A first-feasible test solver cannot model the child's default objective, so this list reproduces the pinned answer without testing it; the entry waits for task 10610.

```ri
structure def Beam {
    param len : Length
    param k : Length = auto
    constraint k >= 0mm
    constraint k <= len
}
structure def Asm {
    sub a : Beam {
        len = auto
    }
    constraint self.a.k == 30mm
    constraint self.a.len <= 100mm
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 60 mm |
| Asm.a.k | value 30 mm |

### sibling-or-self-result-arguments-cycle

- Tests: two siblings each given the other's own result, or one sub given its own result, is a cycle.
- Rule: decision 1 (its cost); Part 6.1 ("Where this meaning has no answer"); Part 4 (an argument that depends on what it helps decide).
- Provenance: logic-2 N11, first model (taken: 6.1 and decision 1's cost); logic-3 R12's companion rule (a solve reads only the arguments its constraints read: here `k == 100mm - len / 2` reads `len`). design decision 1 (cost, third bullet); Part 6.1; logic-2 trace of 66.7 mm.
- Needs: instance, unknown, roots, solve-forced
- Answer basis: stated (Part 6.1; decision 1, cost). Each solve reads, before it opens its view, the arguments its constraints read (contract section 8; PRD section 5.6). `a`'s solve reads `len`, which is `self.b.k`, which reads `b`'s solve, which reads `b.len` = `self.a.k`, which reads `a`'s solve. In `Loop`, `a`'s solve reads `len` = `self.a.k * 2`, which reads `a`'s solve. One solve over both would give 66.7 mm each in `Asm`. The braces hold argument assignments only, and `self` is the parent whatever the rule for bare names, so the entry does not need `braces`.
- Candidates: Loop.a.k: 50mm. Asm.a.k: 50mm. Asm.b.k: 75mm. None is tried: each solve meets the ring when it reads its argument, before its view. 50 mm for `Loop.a.k` is what a solver takes that reads the argument through its view (tracer, gap G5).

```ri
structure def Beam {
    param len : Length
    param k : Length = auto
    constraint k == 100mm - len / 2
}
structure def Asm {
    sub a : Beam {
        len = self.b.k
    }
    sub b : Beam {
        len = self.a.k
    }
}
structure def Loop {
    sub a : Beam {
        len = self.a.k * 2
    }
}
```

| Path | Expect |
|---|---|
| (file) | roots Asm, Loop |
| Asm.a.k | failure cycle |
| Asm.b.k | failure cycle |
| Loop.a.k | failure cycle |

### auto-inside-sub-argument-call-owned-by-writer

- Tests: an `auto` written in a constructor call inside a sub's argument belongs to the instance whose text contains it, so that instance's own constraints bear on it.
- Rule: Part 1 rule 3 and Part 6.1 (an `auto` belongs to the instance of the definition whose text contains it); Part 6.3; decision 5.
- Provenance: logic-2 N1, "Also (6.3)" ("could mean `d` ... or `root` ... Rule 3 says `root`"; taken: draft 5's rule).
- Needs: instance, constraint, unknown, made, open, nested-solve, solve-forced
- Answer basis: traced. The call is in `Asm`'s text, so the unknown is `Asm`'s, at `Asm/auto[d.p#0.t]` (PRD section 5.3: an `auto` in a call inside a sub's argument). `Plate`'s `t >= 3mm` (at `Asm.d.p/made[0]`) and `Asm`'s `d.p.t == 5mm` are both inside `Asm`: 5 mm. Had `d` owned it, `Asm`'s constraint would only check, and `t >= 3mm` alone would not fix `t`.
- Candidates: Asm/auto[d.p#0.t]: 3mm, 5mm. 3 mm meets `Plate`'s bound and breaks `Asm`'s constraint; a reading in which `d` or the made instance owns the `auto` takes it. The first taken is 5 mm.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Widget {
    param p : Plate
}
structure def Asm {
    sub d = Widget(p: Plate(t: auto))
    constraint self.d.p.t == 5mm
}
```

| Path | Expect |
|---|---|
| Asm.d.p/made[0].t | value 5 mm |
| Asm/constraint[0] | verdict satisfied |

---

### two-autos-in-one-expression-two-unknowns

- Tests: two `auto`s written in one expression are two unknowns of the instance, each decided from the constraints that bear on it.
- Rule: Part 6.3 (rule 1: "one unknown for each instance of that declaration"); decision 5; rule F (an unknown with no name of its own is named "with the chain of call sites when it is inside an expression").
- Provenance: logic-3 R14 ("Two unknowns in one expression get one step"; taken: rule F).
- Needs: instance, summary, unknown, made, open, nested-solve, solve-forced
- Answer basis: traced. Both calls are evaluated exactly once, so each `auto` is an unknown of `Asm`, at `Asm/auto[both#0.t]` and `Asm/auto[both#1.t]` (readings A). `Lo`'s bounds force the first to 1 mm and `Hi`'s the second to 5 mm. One shared unknown would be infeasible. Only the sum is pinned, so the entry does not pin path text and does not need `paths`.
- Candidates: Asm/auto[both#0.t]: 5mm, 1mm. Asm/auto[both#1.t]: 1mm, 5mm. (5 mm, 1 mm) is the pair with the two unknowns swapped, which a reading that numbers the two steps the other way takes; it breaks both made instances' bounds. Tried as pairs in order, or as every combination in order, the first taken is (1 mm, 5 mm).

```ri
structure def Lo {
    param t : Length
    constraint t >= 1mm
    constraint t <= 1mm
}
structure def Hi {
    param t : Length
    constraint t >= 5mm
    constraint t <= 5mm
}
structure def Asm {
    let both = Lo(t: auto).t + Hi(t: auto).t
}
```

| Path | Expect |
|---|---|
| Asm.both | value 6 mm |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### owner-unknown-found-where-child-feasible

- Tests: an owner's unknown passed to a child whose own constraints are feasible for one argument only is decided at that point, not left undetermined in silence.
- Rule: Part 6.4 requirement 2; decision 1 (cost: "A child that is feasible for only some arguments makes the parent's solve a search for where").
- Provenance: logic-1 L6 (taken: moved to 6.4 as a requirement on the solve design; logic-2 Part A row L6); logic-2 holds-up ("Model 3: 48 mm and 2 mm"); design 6.4 requirement 2. design 6.4 requirement 2; critic logic-1 (trace of the silent undetermined).
- Needs: instance, constraint, unknown, solve-forced, nested-solve
- Answer basis: stated (Part 6.4, requirement 2). `a.len` is `Asm`'s unknown, at `Asm/auto[a.len]`; `a.k` is `a`'s. `k == 2mm` and `len + k == 50mm` admit only `len` 48 mm. The braces hold an argument assignment only, so the entry does not need `braces`.
- Candidates: Asm/auto[a.len]: 40mm, 48mm. Asm.a.k: 10mm, 2mm. Under `len := 40mm`, `a`'s solve rejects 10 mm (`k == 2mm`) and 2 mm (`len + k == 50mm`), so `a.k` is undefined and `Beam`'s two verdicts are undetermined because of the trial: 40 mm is rejected (readings E35). A solver that rejects only a violated constraint takes it (critic-design F1). Under `len := 48mm`, `a` takes 2 mm and both hold. The first taken are 48 mm and 2 mm.

```ri
structure def Beam {
    param len : Length
    param k : Length = auto
    constraint k == 2mm
    constraint len + k == 50mm
}
structure def Asm {
    sub a : Beam {
        len = auto
    }
}
```

| Path | Expect |
|---|---|
| Asm.a.len | value 48 mm |
| Asm.a.k | value 2 mm |
| Asm.a/constraint[1] | verdict satisfied |

### pass-through-auto-lets-top-constraint-bear

- Tests: to choose a child's value from above, each level passes an `auto` parameter through, and then the top's constraint bears on it.
- Rule: decision 1 (its cost); Part 6.2; Part 4 (names in the braces).
- Provenance: language-1 L10 (the pass-through source); design decision 1 "Cost". Taken. Also, from the merged entries: design decision 1 (cost, first bullet).
- Needs: instance, braces, constraint, unknown, solve-forced
- Answer basis: traced. `a_len = auto` in `TopS`'s braces is `TopS`'s unknown, at `TopS/auto[asm.a_len]`; `Asm`'s own default `auto` is not used, since the use gives it. In `Asm`'s braces, bare `a_len` is no `let` of the braces and not a member of `Beam`, so it is `Asm`'s parameter (readings C20; this name resolution is why the entry needs `braces`). Bearing, all inside `TopS`: `a_len == 70mm` and `Beam`'s bounds at `TopS.asm.a`. Exactly 70 mm.
- Today: `TopS.asm.a_len = 70 mm` but `TopS.asm.a.len = undef` (run `own_pass.ri`).
- Candidates: TopS/auto[asm.a_len]: 50mm, 70mm. 50 mm meets `Beam`'s bounds and breaks `TopS`'s constraint; a reading in which `Asm` keeps its own `auto` and `TopS`'s constraint only checks takes it. The first taken is 70 mm.

```ri
structure def Beam { param len : Length = 50mm   constraint len >= 10mm   constraint len <= 100mm }
structure def Asm  { param a_len : Length = auto   sub a : Beam { len = a_len } }
structure def TopS { sub asm : Asm { a_len = auto }   constraint self.asm.a_len == 70mm }
```

| Path | Expect |
|---|---|
| TopS.asm.a.len | value 70 mm |
| TopS/constraint[0] | verdict satisfied |
| TopS.asm.a/constraint[1] | verdict satisfied |

### guarded-auto-and-constraint-bear-when-present

- Tests: an `auto` under a guard is an unknown only while its guard is true, and a constraint under a true guard bears on an unknown of the same instance, whether the unknown is guarded or not.
- Rule: Part 6.2 ("under the same guards"); Part 6.4 item 4; Part 4; decision 3
- Provenance: task 7489 (its probes 1 and 3. Probe 2 is in CG-20, which this draft's former compile-error entry merged into. Its guarded `x in C` domain is not modelled: I did not check the membership syntax)
- Needs: instance, guard, constraint, unknown, solve-forced
- Answer basis: traced.
  `Top.on`: g = 1.0 > 0, so `m` and its constraint are present. The constraint is inside the owner, so it bears, and `m` = 10 mm.
  `Top.off`: g = -1.0, so `m` and the constraint are absent. An absent unknown is left out of any solve (Part 6.4 item 4).
  `Top.k`: `m` is unguarded, and the guarded constraint is present with g = 1.0, so it bears, and `m` = 10 mm.
- Today: `GEP.m = undef` with an INDETERMINATE verdict, and `GK.m = undef` with "not touched by any constraint" (task 7489 record, release binary at 87b744b189).
- Candidates: Top.on.m: 5mm, 10mm. Top.k.m: 5mm, 10mm. 5 mm is what a reading takes in which the guarded constraint does not bear (task 7489's "not touched by any constraint"); it breaks `self.m == 10mm`, so the first taken is 10 mm for each.

```ri
structure def GEP {
    param g : Real = 1.0
    where g > 0.0 { param m : Length = auto   constraint self.m == 10mm }
}
structure def GK {
    param g : Real = 1.0
    param m : Length = auto
    where g > 0.0 { constraint self.m == 10mm }
}
structure def Top { sub on : GEP   sub off : GEP { g = -1.0 }   sub k : GK }
```

| Path | Expect |
|---|---|
| Top.on.m | value 10 mm |
| Top.on/constraint[0] | verdict satisfied |
| Top.off.m | absent |
| Top.off/constraint[0] | absent |
| Top.k.m | value 10 mm |

### parent-constraint-only-checks-forced-child-auto

- Tests: decision 1's negative half with a forced value: a constraint outside the instance that owns an `auto` is only checked against the result and cannot change it.
- Rule: decision 1; Part 6.1; seat-context ruling 4; contract section 8 ("A constraint outside it is checked against the result and cannot change it").
- Provenance: the tracer of 2026-11-26 (trace-braces-purposes-unknowns, wrong tags, "coverage that waits and need not"): decision 1's model with `Beam`'s bounds pinched to 55 mm, so that the value is forced. The same model with the middle of a range is `auto-owned-by-writer-sub-and-value-alike`.
- Needs: instance, constraint, summary, unknown, solve-forced
- Answer basis: traced. The `auto` is in `Asm`'s text, so `TopS.asm` owns it, at `TopS.asm/auto[a.len]`. `Beam`'s bounds at `TopS.asm.a` are inside the owner and force 55 mm. `TopS`'s constraint is outside the owner and is not part of its problem: violated. Had it borne, no value would satisfy it with `Beam`'s bounds, and `a.len` would be undefined with `TopS/constraint[0]` undetermined. Summary: violated 1.
- Candidates: TopS.asm/auto[a.len]: 70mm, 55mm. 70 mm meets `TopS`'s constraint and breaks `Beam`'s bounds, which depended on the trial; the first taken is 55 mm. `TopS`'s constraint is not read by the owner's solve.

```ri
structure def Beam { param len : Length = 50mm   constraint len >= 55mm   constraint len <= 55mm }
structure def Asm  { sub a : Beam { len = auto } }
structure def TopS { sub asm : Asm   constraint self.asm.a.len == 70mm }
```

| Path | Expect |
|---|---|
| TopS.asm.a.len | value 55 mm |
| TopS/constraint[0] | verdict violated |
| TopS/summary | summary violated 1 undetermined 0 failed 0 |

### trial-independent-violation-does-not-bear

- Tests: a constraint inside the owner whose verdict does not depend on the unknown does not bear on it: it is a violated verdict, and the unknown is decided by the constraints that do.
- Rule: Part 6.1 (a constraint bears only if its verdict depends on the unknown); readings E35; PRD section 5.6 ("A constraint that is violated whatever the trial does not bear").
- Provenance: the tracer's counter-example N8 (trace-braces-purposes-unknowns), and gap G6.
- Needs: instance, constraint, summary, unknown, solve-forced
- Answer basis: traced. `w` is a parameter with a default, so `w >= 2mm` is violated whatever `x` is, and does not bear. `x == 20mm` bears: 20 mm. Summary: violated 1. A solver that rejects a candidate for any violated constraint rejects every candidate and leaves `x` undefined.
- Candidates: Asm.x: 10mm, 20mm. Under 10 mm, `x == 20mm` is violated and depended on the trial. Under 20 mm, only `Asm/constraint[0]` is violated, and it did not depend on the trial, so 20 mm is taken.

```ri
structure def Asm {
    param w : Length = 1mm
    constraint w >= 2mm
    let x : Length = auto
    constraint x == 20mm
}
```

| Path | Expect |
|---|---|
| Asm.x | value 20 mm |
| Asm/constraint[0] | verdict violated |
| Asm/constraint[1] | verdict satisfied |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### guarded-sub-auto-controlled-by-owner-unknown-left-open

- Tests: what a solve does with an `auto` on a sub whose guard reads another unknown of the same owner.
- Rule: Part 6.2; contract section 8 ("Unknowns inside structure that another unknown of the same group controls" is the solve design's).
- Provenance: the tracer's interaction I6 (trace-braces-purposes-unknowns).
- Needs: compile-error, guard, unknown
- Answer basis: open for `Asm.x`, the presence of `Asm.b` and `Asm.b.len`; owner task 10610. Both unknowns are `Asm`'s; `b`'s `len`, at `Asm/auto[b.len]`, exists only in trials where `x > 15mm`. Only compilation is pinned, so the entry runs (readings F, last note).
- Note: traced by the tracer: `x` 20 mm, `b` present, `b.len` 10 mm; a test solver given the pair (20 mm, 10 mm) finds it.

```ri
structure def Beam { param len : Length   constraint len >= 10mm   constraint len <= 10mm }
structure def Asm {
    let x : Length = auto
    constraint x == 20mm
    sub b : Beam where x > 15mm { len = auto }
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.x | open |
| Asm.b | open |
| Asm.b.len | open |


---

## What makes a trial point infeasible (rule C)

### index-past-end-under-trial-infeasible

- Tests: a read past the end of a collection whose count is on trial makes that trial point infeasible, not a failure of the definition.
- Rule: rule C (first bullet; departs from spec 8.9's third category); Part 5 (an index out of range gives undefined); readings A (the count constraint `vents.count == n` has no `constraint[k]`, so `constraint[2]` is `vents[2].area >= 1mm`).
- Provenance: logic-1 L15 (taken: an index out of range gives undefined, decision 3; and rule C's first bullet).
- Needs: instance, collection, constraint, unknown, solve-real
- Answer basis: traced. Under the trial `n := 2`, `vents` has two elements and `vents[2].area` is undefined; that input depended on the trial's value, so the point is infeasible. Feasible: 3 to 6. `minimize n` gives 3. This needs a solver that tries integer values (Part 13: no kept solver does).
- Note: this entry does not separate rule C from a solver that accepts an undetermined verdict: its own `n >= 3` excludes every point at which `vents[2]` is past the end before the index matters, so its answer is 3 either way (tracer report, N4). Entry `index-past-end-on-trial-rejects-candidate` tests rule C's first bullet with the test solver.

```ri
structure def Vent {
    param area : Length = 2mm
}
structure def M {
    param n : Int = auto
    sub vents : List<Vent>
    constraint n >= 3
    constraint n <= 6
    constraint vents[2].area >= 1mm
    minimize n
    constraint vents.count == n
}
```

| Path | Expect |
|---|---|
| M.n | value 3 |
| M.vents[2].area | value 2 mm |
| M.vents[3] | absent |
| M/constraint[2] | verdict satisfied |

### trial-independent-undefined-input-is-model-fault

- Tests: a constraint that is undefined because of an input that does not depend on the trial is undetermined, as a fault of the model, and is not reported as a bound the unknown cannot meet.
- Rule: rule C (second bullet, and "How a solve tells them apart").
- Provenance: logic-2 N8 (proposed "depended on the substitution"; not closed in draft 3, logic-3 R10 first half); logic-3 R10 (taken: rule C decides by whether the undefined *input* depended on the trial).
- Needs: instance, guard, constraint, unknown
- Answer basis: traced for the verdict; open for `Top.x`. `fan`'s guard does not read `x`, so `fan` is absent at every trial and `self.c.fan.flow` is undefined with no dependence on the trial. The constraint is undetermined, and the solve must not fail with "could not be jointly satisfied" naming the bounds on `x`. Whether `x` is then solved from its bounds alone (3 mm) or undetermined is the solve design's (task 10610), so `Top.x` is not pinned.
- Note: both pinned rows hold whatever the solve does with `x`, decided or not, so no solve decides them and the entry does not carry `solve-real` (readings F, last paragraph; tracer report, wrong tags). It needs the unknown's cell (`unknown`), because without a kind for `solves` that cell is blocked and the verdict would be `failure blocked`. It carries no candidates: the test solver given none decides nothing, which leaves both rows as pinned.

```ri
structure def Fan {
    param flow : Length = 2mm
}
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
}
structure def Top {
    param x : Length = auto
    constraint x >= 1mm
    constraint x <= 5mm
    sub c : Case
    constraint self.c.fan.flow > x
}
```

| Path | Expect |
|---|---|
| Top.c.fan | absent |
| Top/constraint[2] | verdict undetermined |

### input-absent-at-every-trial-left-open

- Tests: what a solve concludes when a read is past the end at every trial because of the unknown's own bounds.
- Rule: rule C ("Open: what a solve concludes when an input is absent at every trial because of the unknown's bounds is the solve design's"); readings A (the count constraint has no `constraint[k]`) and B3.
- Provenance: logic-3 R10, second half (`vents[9]` with a count between 5 and 8). Not taken: left open in rule C.
- Needs: compile-error, collection, unknown
- Answer basis: open. Every trial is infeasible by rule C's first bullet, so the solve finds no value; the model's fault is the index. Is this a failed solve, written by readings B3 as `M.n | value undefined` with `M/constraint[2] | verdict undetermined` (needs `solve-infeasible`), or is the constraint reported as the model's fault? No expected value for `M.n`, `M.vents` or the verdicts. Owner: task 10610. The entry pins only the compile outcome, which no solve decides, so it does not carry `solve-real` (readings F, last paragraph).

```ri
structure def Vent {
    param area : Length = 2mm
}
structure def M {
    param n : Int = auto
    sub vents : List<Vent>
    constraint n >= 5
    constraint n <= 8
    constraint vents[9].area >= 1mm
    constraint vents.count == n
}
```

| Path | Expect |
|---|---|
| (compile) | ok |

---

### index-past-end-on-trial-rejects-candidate

- Tests: a trial point at which a constraint reads past the end of a collection whose count is on trial is rejected, because the undefined input depended on the trial; the test solver therefore takes the first count at which the element exists.
- Rule: rule C (first bullet); readings E35 (a candidate is rejected when a constraint that depended on the trial is undetermined); Part 5 (an index out of range gives undefined); readings A (a count constraint has no `constraint[k]`).
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N4, and its gap G6 (a solver that accepts an undetermined verdict never builds rule C).
- Needs: instance, collection, constraint, unknown, solve-forced
- Answer basis: traced. The verdict-bearing constraints are `n >= 1` (`constraint[0]`), `n <= 3` (`constraint[1]`) and `vents[2].area >= 1mm` (`constraint[2]`); `vents.count == n` is the count's source. At `n` = 1 and at `n` = 2, `vents[2]` is past the end, so `constraint[2]` is undetermined, and the count it read depended on the trial: both points are rejected (readings E35). At `n` = 3, `vents[2].area` is 2 mm and every constraint is satisfied. The only feasible value is 3, so the answer is forced. A solver that accepted an undetermined verdict would take 1.
- Candidates: M.n: 1, 2, 3

```ri
structure def Vent { param area : Length = 2mm }
structure def M {
    param n : Int = auto
    sub vents : List<Vent>
    constraint n >= 1
    constraint n <= 3
    constraint vents[2].area >= 1mm
    constraint vents.count == n
}
```

| Path | Expect |
|---|---|
| M.n | value 3 |
| M.vents[2].area | value 2 mm |
| M/constraint[2] | verdict satisfied |


---

## How the evaluator reads (rule A)

### cycle-through-and-operand-cannot-be-caught

- Tests: both operands of `and` are read, so a ring through an operand that could not change the answer is a cycle; a cycle is a failure that `determined` cannot catch.
- Rule: decision 12 (both operands read; a failure cannot be caught by an expression); rule A; Part 2; contract 7.7 ("every node on the cycle gets the failure cycle").
- Provenance: design decision 12 ("So a ring of dependencies through an operand that could not affect the answer is a cycle. Today the evaluator skips a right operand once the left decides"). The ring crosses two subs so that the compiler's check for cycles among `let`s does not catch it first (that check rejects a ring of `let`s in one definition today: run, `cyc.ri`).
- Needs: instance, eval-reads, compile-error
- Answer basis: traced. `a.inp` reads `b.out`, which reads `b.inp`, which reads `a.out`, which reads `a.inp`: a ring of running bodies, each a cycle. `z` reads a failure: blocked (readings B2); `determined` cannot catch it. The braces hold only argument assignments, so the entry does not need `braces`; `determined` is leaf ε₂'s, built with the first cells (PRD 5.5 and 5.8).
- Today: `a.inp` = false (right operand skipped) and `b.inp` = true (run, `xcyc.ri`).

```ri
structure def Node { param inp : Bool = false   pub let out : Bool = inp }
structure def S {
    sub a : Node { inp = false and self.b.out }
    sub b : Node { inp = not self.a.out }
    let z = if determined(self.a.out) then 1 else 2
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| S.a.inp | failure cycle |
| S.b.inp | failure cycle |
| S.z | failure blocked |

### determined-of-stand-in-declares-once

- Tests: a determinacy test given the stand-in for "not here yet" gives the stand-in, so a constructor call whose argument depends on it is declared once, with the final value.
- Rule: rule A (the stand-in is absorbing, "including a determinacy test"; a child is declared only from values with no stand-in).
- Provenance: logic-1 L9 (taken: draft 2's absorbing stand-in); logic-2 N4 (closed).
- Needs: instance, eval-reads, value, made, summary, roots
- Answer basis: traced. In `Asm`, pass 1 has `self.k` missing: the stand-in, so `determined` and the `if` give the stand-in and no instance is declared. Pass 2: 5 mm, one instance with `t` 5 mm, satisfied. In `AsmU`, `k` is undefined (a value, not a stand-in): `determined` is false, `t` 2 mm, violated. Both are roots; `Plate` is used.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Asm {
    param k : Length = 5mm
    let p = Plate(t: if determined(self.k) then self.k else 2mm)
}
structure def AsmU {
    param k : Length
    let p = Plate(t: if determined(self.k) then self.k else 2mm)
}
```

| Path | Expect |
|---|---|
| (file) | roots Asm, AsmU |
| Asm.p/made[0].t | value 5 mm |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |
| AsmU.p/made[0].t | value 2 mm |
| AsmU/summary | summary violated 1 undetermined 0 failed 0 |

### or-operand-constructor-call-makes-instance

- Tests: both operands of `or` are read, so a constructor call in an operand that cannot change the result still makes an instance whose verdicts count, an `auto` there is legal, and a call in an untaken `if` arm makes nothing.
- Rule: rule A ("Only the condition of an `if` or a `match` decides whether something is read"); decision 12; Part 6.3 ("Evaluated exactly once"); rule B.
- Provenance: logic-2 N3 model 2 (the "phantom child"; its proposed skip rule was not taken, reversed by logic-3 R1 and ruled in decision 12); logic-3 R9 (taken: the Words definition of "evaluated exactly once" excludes only lambdas, function bodies, quantifier bodies and `if`/`match` arms).
- Needs: instance, eval-reads, value, made, open, constraint, summary, unknown, solve-forced, nested-solve, compile-error
- Answer basis: traced. `f1`: `quick` is true, but the right operand is read, so `Plate(t: 1mm)` is made; its `t >= 3mm` is violated and counted. `f2`: the `else` arm is not taken, so nothing is made. `f3`: the `auto` is `Asm`'s unknown, at `Asm/auto[f3#0.t]` (readings A); `Plate`'s two constraints are inside `Asm` and force 3 mm. One violation in all.
  Under the candidates (readings E35): at 1 mm the made plate's `t >= 3mm` is violated and read the trial, so 1 mm is rejected; at 3 mm both its constraints are satisfied. The plate `f1` made is violated at every trial: it did not depend on the trial, so it does not bear and does not reject 3 mm; it stays one violated verdict. A solver that read only `Asm`'s own constraints would take 1 mm; one that let every violation reject would decide nothing.
- Today: logic-3 read-code `crates/reify-expr/src/lib.rs` (`and`/`or`/`implies` skip the right operand when the left decides), so `f1`'s instance would not be made; and constraints of values are not checked at all today (design 7.6).
- Candidates: Asm/auto[f3#0.t]: 1 mm, 3 mm

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
    constraint t <= 3mm
    pub let ok = t >= 3mm
}
structure def Asm {
    param quick : Bool = true
    let f1 = quick or Plate(t: 1mm).ok
    let f2 = if quick then true else Plate(t: 1mm).ok
    let f3 = quick or Plate(t: auto).ok
}
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Asm.f1 | value true |
| Asm.f1/made[0]/constraint[0] | verdict violated |
| Asm.f2/made[0] | absent |
| Asm.f3/made[0].t | value 3 mm |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### undefined-argument-instance-made-in-either-order

- Tests: a constructor call in either operand of `or`, given an argument that reads something absent, makes an instance with that parameter undefined whichever way round the operands are written.
- Rule: rule A; decision 3; decision 12 (an undefined argument is reported by a built-in verdict); Part 2; readings B4, B5 and C19.
- Provenance: logic-3 R2, second half (under draft 3's skip rule, operand order decided whether the `Plate` was made; taken: decision 12 reads both operands); logic-3 holds-up ("An undefined argument gives a made instance with that parameter undefined").
- Needs: instance, guard, eval-reads, value, made, open, constraint, summary
- Answer basis: traced. `fan` is absent, so `self.c.fan.d` is undefined, a value, not a stand-in: the `Plate` is made with `t` undefined, in both `g1` and `g2` (readings C19: a constructor call, unlike a user function, is made). Its `t >= 3mm` is undetermined, and so is its `/arguments` verdict (readings B5). `ok` is undefined; `undefined or true` is true. Summary (readings B4): two undetermined verdicts at each of the two made instances, undetermined 4.

```ri
structure def Fan {
    param d : Length = 40mm
}
structure def Case {
    param needs_fan : Bool = false
    sub fan : Fan where needs_fan
}
structure def Plate {
    param t : Length
    constraint t >= 3mm
    pub let ok = t >= 3mm
}
structure def Top {
    sub c : Case
    let g1 = Plate(t: self.c.fan.d).ok or not self.c.needs_fan
    let g2 = not self.c.needs_fan or Plate(t: self.c.fan.d).ok
}
```

| Path | Expect |
|---|---|
| Top.g1 | value true |
| Top.g2 | value true |
| Top.g1/made[0].t | value undefined |
| Top.g1/made[0]/constraint[0] | verdict undetermined |
| Top.g2/made[0]/constraint[0] | verdict undetermined |
| Top.g1/made[0]/arguments | verdict undetermined |
| Top.g2/made[0]/arguments | verdict undetermined |
| Top/summary | summary violated 0 undetermined 4 failed 0 |

### ring-through-unneeded-operand-is-cycle

- Tests: a ring of dependencies through an operand of `or`, or through an element of a quantifier, is a cycle even where that operand could not change the answer.
- Rule: rule A; decision 12 ("a ring of dependencies through an operand that could not affect the answer is a cycle").
- Provenance: logic-2 N3 model 1 (its skip rule, which gave true and true, was not taken); logic-3 R1 (taken: one round, every element of a quantifier read); logic-3 R2 first half (the ring swapped was a cycle under draft 3 and true unswapped).
- Needs: instance, eval-reads
- Answer basis: stated (decision 12). `x.v` reads `x.use` and `x.other`; `x.other` is `self.y.v` in `Top`; `y.v` reads `y.other`, which is `self.x.v`. The same ring runs through `forall`'s list in `Q`. The rings run across instances; within one definition today's compiler already rejects a let cycle ("circular let-binding dependency", run `letcycle2.ri`), which is why this model uses subs. The braces hold only argument assignments, so the entry does not need `braces`.
- Today: run `orring.ri`: `Top.x.v = true`, `Top.y.v = true`; run `forallring.ri`: `Top.x.v = false`, `Top.y.v = false`.
- Note: the quantifier half does not separate rule A from a quantifier that stops early: building the list literal `[false, other]` reads `other` before the quantifier runs, so a short-circuiting `forall` gives the same cycle (tracer report, N5). Entry `quantifier-later-element-read-is-cycle` separates them. The `or` half does.

```ri
structure def P {
    param use : Bool = true
    param other : Bool = false
    pub let v = use or other
}
structure def Q {
    param other : Bool = false
    pub let v = forall f in [false, other]: f
}
structure def Top {
    sub x : P {
        other = self.y.v
    }
    sub y : P {
        other = self.x.v
    }
    sub m : Q {
        other = self.n.v
    }
    sub n : Q {
        other = self.m.v
    }
}
```

| Path | Expect |
|---|---|
| Top.x.v | failure cycle |
| Top.y.v | failure cycle |
| Top.m.v | failure cycle |
| Top.n.v | failure cycle |

---

### stand-in-inside-container-declares-no-child

- Tests: a "not here yet" read held inside a list or an `Option`, or used as the index that selects an element, keeps a constructor call from declaring a child until the final values arrive, so each cell makes one instance, with the final argument.
- Rule: rule A ("absorbing: every operation given one returns one, including a determinacy test and anything that can hold a value"; "A child is declared only from values that contain no stand-in")
- Provenance: logic-1 L9 (the rule "slips through any operation that observes undefinedness", naming a `filter` predicate and the containers Map, Option and a closure's environment); logic-2 N4 (L9 closed for `determined` only). A lambda passed to `filter` is refused today (catalogue entry `selecting-builtin-makes-no-instance`), so the selection here is by an index and by `unwrap_or`.
- Needs: instance, eval-reads, value, made, summary
- Answer basis: traced.
  Pass 1: `self.k` and `self.i` are both missing and are asked for in one round. A list holding a stand-in, `some(...)` of one, an index that is one, and `unwrap_or` given one each give the stand-in (PRD 5.5: a node whose operand is pending is pending; a constructor call with a pending argument makes nothing). So neither cell declares a child.
  Pass 2: `[5mm, 2mm][0]` is 5 mm and `unwrap_or(some(5mm), 2mm)` is 5 mm. One `Plate` per cell, `t` 5 mm, satisfied.
  Had a stand-in been read as undefined, pass 1 would declare a `Plate` with `t` undefined at the same site as pass 2's: two undetermined verdicts or two declarations at one path, which the summary row would show.
- Today: run `g01.ri`: `Asm.p = Plate { t: 0.005 m }`, `Asm.q = Plate { t: 0.005 m }`.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Asm {
    param k : Length = 5mm
    param i : Int = 0
    let p = Plate(t: [self.k, 2mm][self.i])
    let q = Plate(t: unwrap_or(some(self.k), 2mm))
}
```

| Path | Expect |
|---|---|
| Asm.p/made[0].t | value 5 mm |
| Asm.q/made[0].t | value 5 mm |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |

### unwrap-or-default-read-makes-instance

- Tests: the default operand of `unwrap_or` is read whatever its first operand holds, so a constructor call in the default makes an instance whose verdict counts, though the result is the first operand's content.
- Rule: rule A ("Only the condition of an `if` or a `match` decides whether something is read"); decision 12 (confirmed 2026-11-26); rule B
- Provenance: logic-2 N3 (its smallest change listed "the value given to `unwrap_or`" among the positions to leave unevaluated on a stand-in; not taken: decision 12 reads everything outside an arm). The catalogue's `or-operand-constructor-call-makes-instance` is the same rule for `or`.
- Needs: instance, eval-reads, value, made, open, summary
- Answer basis: traced. `unwrap_or` is a call, not an `if` or a `match`, so both arguments are read. `Plate(t: 1mm)` is made, its `t >= 3mm` is violated and counted once, at the made instance (rule B; readings B4). `o` is `some(5mm)`, so `x` is 5 mm. The made instance's path is not pinned: section A's `/made[chain]` text is not worked out for a call nested in another call's argument (as in `nested-and-listed-made-instances-counted-once`).
- Today: the evaluator evaluates every argument before a recovery combinator runs (read-code `crates/reify-expr/src/lib.rs`, the `option_recovery::is_combinator` arm of `eval_expr`; kept code, not held). Run `g02.ri`: `Asm.x = 0.005 m`; `check` reports only `Plate#constraint[0]` at the definition's own missing default, INDETERMINATE, and not the made plate.
- Note: PRD 5.5 lists built-ins that select by the shape of a value (`map_or`, `map_err`, `flat_map` over nothing) as evaluating only what they select. Those apply a function; `unwrap_or`'s default is an ordinary argument expression, so it is not on that list. If the lead puts `unwrap_or` on it, the summary row becomes `violated 0`.

```ri
structure def Plate {
    param t : Length
    constraint t >= 3mm
}
structure def Asm {
    param o : Option<Length> = some(5mm)
    let x = unwrap_or(o, Plate(t: 1mm).t)
}
```

| Path | Expect |
|---|---|
| Asm.x | value 5 mm |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### quantifier-later-element-read-is-cycle

- Tests: every element of a quantifier is read, even after an earlier element has decided the result, so a ring through a later element's body is a cycle.
- Rule: rule A ("Only the condition of an `if` or a `match` decides whether something is read"); ruling 6 of `seat-context.md` ("every element of a quantifier ... are read"); decision 12; readings B2.
- Provenance: tracer report `critics/trace-presence-and-values.md`, counter-example N5 (the quantifier half of `ring-through-unneeded-operand-is-cycle` reads `other` when building its list literal, so it does not test this).
- Needs: instance, eval-reads
- Answer basis: traced. In `Q`, element 0 of the quantifier gives `false`, which decides `forall`; element 1 is read all the same, and there the condition `i == 0` is false, so its else arm reads `other`. In `Top`, `m.other` is `self.n.v` and `n.other` is `self.m.v` (`self` in the braces is the parent; the braces hold only argument assignments). So `Top.m.v` reads `Top.m.other`, which reads `Top.n.v`, which reads `Top.n.other`, which reads `Top.m.v`: a ring, and every node on it is `failure cycle` (readings B2). A quantifier that stopped at element 0 would give `false` for both and no cycle.

```ri
structure def Q {
    param other : Bool = false
    pub let v = forall i in [0, 1]: if i == 0 then false else other
}
structure def Top {
    sub m : Q { other = self.n.v }
    sub n : Q { other = self.m.v }
}
```

| Path | Expect |
|---|---|
| Top.m.v | failure cycle |
| Top.n.v | failure cycle |
| Top.m.other | failure cycle |


---

## The tests of state

### constrained-agrees-at-made-and-opened

- Tests: a determinacy predicate gives the same answer at a made instance and at its opened value.
- Rule: Part 15.2 ("What `partially_determined` and `constrained` mean"); decision 6 (a value carries nothing about how it was reached); readings B7 (PRD D3, ruled by Leo on 2026-11-26 as recommended).
- Provenance: logic-1 L10; logic-2 N13 (moved, precise: logic-3 Part A row N13). Not taken: Part 15.2 leaves it to a later ruling. design Part 15.2 (a critic's model).
- Needs: instance, made, open, unknown, solve-forced, state-tests
- Answer basis: stated: PRD D3 as Leo ruled it (readings B7). `constrained(t)` is true when some present constraint written in the definition of the instance that holds `t` can read `t`, and `Plate`'s two bounds read it: true at both paths. The design requires the two to agree in any case, which is pinned with `same-as`. `t` is the made instance's own unknown, forced to 3 mm. Today `constrained(x)` reads "the cell is `auto` or provisional" (logic-2 read-code `crates/reify-expr/src/lib.rs`, determinacy predicates). At `Asm.c/made[0]` `t` comes from a solve; at `Asm.c/open` it is given from the value, which cannot record how it was reached.
- Candidates: `Asm.c/made[0].t`: 4 mm, 3 mm. A reading in which `Plate`'s bounds did not bear would take 4 mm. Under 4 mm the bound `t <= 3mm` is violated and depended on the trial; under 3 mm both are satisfied.

```ri
structure def Plate {
    param t : Length = auto
    constraint t >= 3mm
    constraint t <= 3mm
    pub let via_solver = constrained(t)
}
structure def Asm {
    let c = Plate()
    let s = c.via_solver
}
```

| Path | Expect |
|---|---|
| Asm.c/made[0].t | value 3 mm |
| Asm.c/made[0].via_solver | value true |
| Asm.c/made[0].via_solver | same-as Asm.c/open.via_solver |
| Asm.s | value true |

### constrained-ignores-braces-sub-value-and-opened-agree

- Tests: `constrained(p)` gives one answer at a sub that is constrained only from its braces, at that sub read as a value, and at the opened value: the model of PRD decision D3.
- Rule: PRD D3 (ruled by Leo on 2026-11-26 as recommended); readings B7; decision 6 (a value carries nothing about how it was reached); seat-context ruling 7 (a structure value carries every value decided at or below its instance)
- Provenance: PRD D3 (the review's model, which showed that counting a constraint in the braces makes the sub and its value disagree); the brief of seat fix-structure.
- Needs: instance, braces, value, open, state-tests
- Answer basis: stated: PRD D3 as Leo ruled it (readings B7). A constraint that the declarer writes in a sub's braces does not count, and `Plate` itself has no constraint that reads `t`: false at every path. The design requires that the sub, the value and the opened value answer the same, which is pinned as well. `Asm.p.c` is the sub's own cell. `e` holds `p` read as a value, and `e.c` opens it at `Asm.e/open.c`; `x` reads that. A value carries no braces, so a reading that counted the braces constraint would make `Asm.p.c` true and `Asm.e/open.c` false, and the `same-as` rows would fail.
- Today: run `/tmp/instance-prd/fix-structure/d3.ri`: `Asm.p.c = false`, `Asm.x = false` (the braces constraint is dropped today).

```ri
structure def Plate {
    param t : Length = 3mm
    pub let c = constrained(t)
}
structure def Asm {
    sub p : Plate {
        constraint t >= 1mm
    }
    let e = self.p
    let x = e.c
}
```

| Path | Expect |
|---|---|
| Asm.p.c | value false |
| Asm.e/open.c | same-as Asm.p.c |
| Asm.x | same-as Asm.p.c |


---

## Paths and their text (rule F)

### path-text-name-index-key-resolves-instance-values

- Tests: the text form of a path through a member name, a list index and a key resolves to that instance's value, not the definition's default.
- Rule: rule F; Part 8; Part 13 row 8; decision 10 (an element's identity is its position or key).
- Provenance: design Part 8 (`Asm.thin.t`, `Asm.vents[2].area`, `Asm.vents["intake"].area`); design Part 13 row "The GUI cannot address an instance's parameter (7717)". The keyed collection is named `ducts` so that both kinds sit in one definition.
- Needs: instance, collection, paths
- Answer basis: stated (Part 8 gives the three spellings); values traced: `thin.t` is given 4 mm against `Plate`'s 3 mm; list elements take `Vent`'s default 1 mm; the "intake" entry gives 5 mm.
- Today: `Asm.thin.t` is refused as ambiguous (design Part 8, report).

```ri
structure def Plate { param t : Length = 3mm }
structure def Vent  { param area : Length = 1mm }
structure def Asm {
    sub thin : Plate { t = 4mm }
    sub vents : List<Vent>
    constraint vents.count == 3
    sub ducts : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
}
```

| Path | Expect |
|---|---|
| Asm.thin.t | value 4 mm |
| Asm.vents[2].area | value 1 mm |
| Asm.ducts["intake"].area | value 5 mm |

### constraint-braces-purpose-paths-by-position

- Tests: the text form of a path to a definition's constraint, a constraint in a sub's braces, and an applied purpose's constraint, each by position.
- Rule: rule F; Part 8; Part 10.
- Provenance: design Part 15.1 rule F ("A constraint, by its position in the member list. A member written in a sub's braces, by the sub and its position there ... an applied purpose, by its position"); the path text of decisions A.
- Needs: instance, constraint, braces, purpose, paths, grammar-purpose
- Answer basis: traced, with the path text of decisions A: `k` for an applied purpose counts the applications in the member table, so `thin_enough` is `purpose[0]` although it is `Asm`'s fourth member. The subject `self.p` is an unguarded sub, so the application compiles (decisions C16). `w` = 2 mm > 1 mm satisfied; `p.t` = 3 mm ≥ 1 mm satisfied; the braces' `t ≥ 5mm` violated; the purpose's `t < 4mm` satisfied. The verdicts differ so that a path sent to the wrong node shows.

```ri
structure def Plate { param t : Length = 3mm   constraint t >= 1mm }
purpose thin_enough(subject : Structure) { constraint subject.t < 4mm }
structure def Asm {
    param w : Length = 2mm
    sub p : Plate { constraint t >= 5mm }
    constraint w > 1mm
    purpose thin_enough(self.p)
}
```

| Path | Expect |
|---|---|
| Asm/constraint[0] | verdict satisfied |
| Asm.p/constraint[0] | verdict satisfied |
| Asm.p/braces/constraint[0] | verdict violated |
| Asm/purpose[0]/constraint[0] | verdict satisfied |

### made-open-as-value-paths

- Tests: the text form of a path to a made instance, a cell's opened value, and an instance read as a value.
- Rule: rule F; Part 8; Part 7.2; Part 7.3.
- Provenance: design Part 8 (`Asm.c/made[0].t`, `Asm.c/open.area`) and Words (`root/a/as-value`); the path text of decisions A. design Part 7.2; Part 8 path `Asm.c/open.area`.
- Needs: instance, value, constraint, summary, made, open, paths
- Answer basis: traced. `Plate()` makes `Asm.c/made[0]` with `t` = 2 mm, its constraint violated and counted once; `c.area` opens the value: 80 mm²; `s`'s value carries `t` = 4 mm and nothing derived.

```ri
structure def Plate { param t : Length = 2mm   constraint t >= 3mm   pub let area = t * 40mm }
structure def Asm {
    sub s : Plate { t = 4mm }
    let c = Plate()
    let a = c.area
    let e = self.s
}
```

| Path | Expect |
|---|---|
| Asm.c/made[0].t | value 2 mm |
| Asm.c/made[0]/constraint[0] | verdict violated |
| Asm.c/open.area | value 80 mm^2 |
| Asm.a | value 80 mm^2 |
| Asm.s/as-value | value Plate { t: 4 mm } |
| Asm.e | value Plate { t: 4 mm } |
| Asm/summary | summary violated 1 undetermined 0 failed 0 |

### unnamed-steps-name-their-nodes

- Tests: each kind of step that rule F lists without a name in the source names a node that holds the value below.
- Rule: rule F; Part 6.2 (an unknown with no name of its own); Part 7.2 (a value in mid-expression); rule B.
- Provenance: design Part 15.1 rule F.
- Needs: instance, guard, collection, made, open, paths, unknown, solve-forced
- Answer basis: traced, with the path text of decisions A and PRD 5.3. `a.len` is `Asm`'s unknown, `Asm/auto[a.len]`, forced to 20 mm. The `where` block's guard cell is true. The count cell is 2. `mk(3mm)` makes a `Plate` inside `mk`'s body with `t` = 3 mm; its chain is the call site of `mk` in `m`'s expression (0), then the constructor call in `mk`'s body (0): `Asm.m/made[0,0]`. The value is opened in mid-expression by the read `.area`, which is in `m`'s own expression (no call site lies between) and is the first member read of the unit: `Asm.m/open[0]`, 120 mm². The second element of `ps` is made with `t` = 5 mm: `Asm.ps/made[1]`. Not modelled: a placement's cell (`/at`, geometry, task 10611), a connector's sub, the solve design's fixed children, and a member table (`/defs[Name]`).
- Candidates: `Asm/auto[a.len]`: 50 mm, 20 mm. A reading in which `Asm`'s constraint did not bear would take 50 mm, `Beam`'s default. Under 50 mm the constraint is violated and depended on the trial; under 20 mm it is satisfied.

```ri
structure def Beam  { param len : Length = 50mm }
structure def Vent  { param area : Length = 1mm }
structure def Plate { param t : Length = 2mm   pub let area = t * 40mm }
fn mk(t : Length) -> Plate = Plate(t: t)
structure def Asm {
    param on : Bool = true
    sub a : Beam { len = auto }
    constraint self.a.len == 20mm
    where on { let g : Length = 1mm }
    sub vents : List<Vent>
    constraint vents.count == 2
    let m = mk(3mm).area
    let ps = [Plate(t: 4mm), Plate(t: 5mm)]
}
```

| Path | Expect |
|---|---|
| Asm/auto[a.len] | value 20 mm |
| Asm.a.len | value 20 mm |
| Asm/guard[0] | value true |
| Asm/count[vents] | value 2 |
| Asm.m/made[0,0].t | value 3 mm |
| Asm.m/open[0].area | value 120 mm^2 |
| Asm.m | value 120 mm^2 |
| Asm.ps/made[1].t | value 5 mm |

### path-text-round-trips-one-to-one

- Tests: the text form is one-to-one: printing a typed path and parsing the text gives the same path, and no two paths print alike, including a member named like a fixed child and a key that holds `.`, `/` and `]`.
- Rule: rule F ("that the text is one-to-one is a requirement to test"); Part 8 ("One module owns it, both ways"); decision 12 ("A path has one text form").
- Provenance: design Part 8; design Part 15.1 rule F. second-design Q7; claims-1 part B row on tasks 7717 and 8704.
- Needs: instance, collection, summary, paths
- Answer basis: stated (Part 8 and rule F state the requirement); values traced. Every path that every entry of the corpus pins must also round-trip. `Asm.summary` is the sub; `Asm/summary` is the fixed child. The dotted key is read in source as well as by path (`Asm.a`). A key's text inside the quotes is taken whole; how a `"` inside a key is escaped is open.

```ri
structure def Vent { param area : Length = 1mm }
structure def Asm {
    sub summary : Vent { area = 2mm }
    sub ducts : Keyed<Vent> {
        "a.b/c]" => { area = 3mm }
        "a" => { area = 4mm }
    }
    let a = ducts["a.b/c]"].area
}
```

| Path | Expect |
|---|---|
| Asm.summary.area | value 2 mm |
| Asm/summary | summary violated 0 undetermined 0 failed 0 |
| Asm.ducts["a.b/c]"].area | value 3 mm |
| Asm.ducts["a"].area | value 4 mm |
| Asm.a | value 3 mm |

### constraint-identity-is-its-position

- Tests: two constraints with the same text have two verdicts at two paths; a constraint inherited through a trait is checked on the instance, and is numbered before the definition's own.
- Rule: rule F; Part 8; decision 7; catalogue decision A (constraint numbering).
- Provenance: language-1 L15 (two constraints with the same text); failing.md M3 note (an inherited trait constraint's printed index is not its position among the definition's own). The design took L15's ordinal; the inherited ordering is the catalogue's decision A.
- Needs: roots, instance, constraint, summary, paths
- Answer basis: traced. `Plate` is unused, so a root; 1 mm fails both. Decision A: constraints inherited from traits come first, in trait order, then the definition's own. So `Asm.p/constraint[0]` is `Weighted`'s `mass > 0kg`, violated at −1 kg, and `constraint[1]` is `Plank`'s `t > 2mm`, violated at 1 mm. `/arguments` is satisfied. Summary: 2 violated.
- Today: `VIOLATED Plate#constraint[0]` and `[1]` (run `dup_constraint.ri`); for the trait, the inherited constraint prints as `Plank#constraint[0]` and the own one as `[1]`, both at the definition's defaults only (run `trait_index.ri`).

```ri
structure def Plate { param t : Length = 1mm   constraint t > 2mm   constraint t > 2mm }
trait Weighted { param mass : Mass = 1kg   constraint mass > 0kg }
structure def Plank : Weighted { param t : Length = 1mm   constraint t > 2mm }
structure def Asm { sub p : Plank { mass = 0kg - 1kg } }
```

| Path | Expect |
|---|---|
| (file) | roots Plate, Asm |
| Plate/constraint[0] | verdict violated |
| Plate/constraint[1] | verdict violated |
| Asm.p/constraint[0] | verdict violated |
| Asm.p/constraint[1] | verdict violated |
| Asm/summary | summary violated 2 undetermined 0 failed 0 |

### keyed-entry-autos-have-distinct-paths

- Tests: an `auto` written for each entry of a keyed collection is one unknown per entry, and the text of each unknown's path carries the entry's key, so two unknowns never print alike.
- Rule: rule F; decisions A (row "An unknown with no name of its own": "the member with its key for a keyed entry", `M/auto[vents["a"].w]`); decision 5; Part 6.3
- Provenance: trace-structure counter-example 3 (without the key, both unknowns print `M/auto[vents.w]`).
- Needs: instance, collection, paths, unknown, solve-forced
- Answer basis: traced. Both unknowns are `M`'s, the writer's (decision 5), one per key. `M`'s own constraints bear on them: the entry `"a"`'s is forced to 1 mm and `"b"`'s to 2 mm. The two paths differ only in the key. Different values make a path sent to the wrong unknown show.
- Candidates: `M/auto[vents["a"].w]`: 2 mm, 1 mm; `M/auto[vents["b"].w]`: 1 mm, 2 mm. A reading in which the constraints did not bear would take 2 mm and 1 mm. Only 1 mm with 2 mm satisfies both constraints, whatever order the pairs are tried in.

```ri
structure def Vent { param w : Length = 1mm }
structure def M {
    sub vents : Keyed<Vent> {
        "a" => { w = auto }
        "b" => { w = auto }
    }
    constraint vents["a"].w == 1mm
    constraint vents["b"].w == 2mm
}
```

| Path | Expect |
|---|---|
| M/auto[vents["a"].w] | value 1 mm |
| M/auto[vents["b"].w] | value 2 mm |

### braces-guard-and-definition-guard-distinct-paths

- Tests: a `where` block written in a sub's braces and one in the sub's definition are two guard cells of the same instance at two paths, `/braces/guard[k]` and `/guard[k]`, and a `let` in the braces is at `/braces.name`.
- Rule: rule F; decisions A (rows "The condition of a guard written in a sub's braces", "A guard's condition cell", "A `let` written in a sub's braces"); ruling 3 of `seat-context.md` (members written in the braces are members of the instance); decisions C20 (a bare name in the braces is the instance's own member first)
- Provenance: trace-structure counter-example 4 (numbered per list, both guards would print `Asm.p/guard[0]`); its two guards are made to differ so that a path sent to the wrong node shows.
- Needs: instance, guard, braces, paths
- Answer basis: traced. `thick` and `thin` in the braces are bare names that `Plate` declares, so they are `p`'s own members (decisions C20): `thick` true, `thin` false. Both guard cells belong to `Asm.p`. The definition's block is `Asm.p/guard[0]`, true, so `Asm.p.a` is 1 mm. The braces' block is `Asm.p/braces/guard[0]`, false, so the braces' `let b`, at `Asm.p/braces.b`, is absent. Nothing reads `b`, so who may read a braces `let` (decisions C20, pending Leo) does not bear on the rows.

```ri
structure def Plate {
    param thick : Bool = true
    param thin : Bool = false
    where thick { let a : Length = 1mm }
}
structure def Asm {
    sub p : Plate {
        where thin { let b : Length = 2mm }
    }
}
```

| Path | Expect |
|---|---|
| Asm.p/guard[0] | value true |
| Asm.p/braces/guard[0] | value false |
| Asm.p.a | value 1 mm |
| Asm.p/braces.b | absent |

### mid-expression-opens-numbered-by-member-read

- Tests: two values opened in one expression, neither following a call site, are two opened instances at two paths, numbered by the position of each member read among the unit's member reads, and the cell gets its value.
- Rule: rule F; decisions A (row "A cell's value opened as an instance": "opened in mid-expression, `/open[chain]`, the chain ending with the position of the member read among the unit's member reads"); PRD 5.3; Part 7.2
- Provenance: trace-structure counter-example 5 (with an empty chain, both opens would be declared at one step with different declarations, and by contract 7.5 the run of `q` would fail).
- Needs: instance, made, open, paths
- Answer basis: traced. `ps` holds two made plates, `Asm.ps/made[0]` (t = 1 mm) and `Asm.ps/made[1]` (t = 2 mm). In `q`, `ps[0].area` and `ps[1].area` each open a value in mid-expression; no call site lies between `q`'s expression and either read, so each chain is the read's position among `q`'s member reads: `Asm.q/open[0]` with area 40 mm², `Asm.q/open[1]` with area 80 mm². `q` = 120 mm².
- Today: run `/tmp/instance-prd/fix-structure/twoopen.ri`: `Asm.q = 0.00012000000000000002 m^2`.

```ri
structure def Plate {
    param t : Length = 2mm
    pub let area = t * 40mm
}
structure def Asm {
    let ps = [Plate(t: 1mm), Plate(t: 2mm)]
    let q = ps[0].area + ps[1].area
}
```

| Path | Expect |
|---|---|
| Asm.q/open[0].t | value 1 mm |
| Asm.q/open[1].t | value 2 mm |
| Asm.q | value 120 mm^2 |


---

## Edits

### argument-and-default-edits-match-fresh-evaluation

- Tests: after an edit to the definition's default (edit 1), and then an edit that adds an argument at one use (edit 2), each instance reads its own argument or the default, its `let` follows in the instance's scope, and every answer equals a fresh evaluation of the edited text.
- Rule: contract Examples 8 and 10; Part 12 (Examples 8 and 10); Part 13 row 9; decisions B9 (each edit applies to the text the one before it left).
- Provenance: contract Part B Examples 8 and 10; design Part 12 "Example 8", "Example 10"; design Part 13 row "A warm edit evaluates instance cells in the definition's scope (9818, 10081)". `Peg`'s body is dropped from Example 6's `Peg` so that no geometry is needed. The edits were reordered after trace-structure found that, applied in sequence, the earlier order hid the default edit from `b` (finding 1.2).
- Needs: instance, edits
- Answer basis: traced. Edit 1 changes `Peg`'s default to `12mm`: `b` has no argument, so `b.w` = 12 mm and `b.area` = 144 mm²; `a` has an argument and never reads the default (Part 12, Example 10), so `a.w` = 10 mm and `a.area` = 100 mm². Edit 2, applied to the text edit 1 left, replaces `sub b : Peg` by `sub b : Peg { w = 22mm }`: `b.w` = 22 mm, `b.area` = 484 mm², `a` untouched at 10 mm.
- Today: a warm edit evaluates instance cells in the definition's scope (tasks 9818, 10081; design Part 13, report).
- Edits:
  1. In `Peg`, replace `param w : Length = 10mm` by `param w : Length = 12mm`.
  2. In `Pair`, replace `sub b : Peg` by `sub b : Peg { w = 22mm }`.

```ri
structure def Peg { param w : Length = 10mm   let area = w * w }
structure def Pair {
    sub a : Peg { w = 10mm }
    sub b : Peg
}
```

| Path | Expect |
|---|---|
| Pair.b.w | value 10 mm |
| Pair.b.w (after edit 1) | value 12 mm |
| Pair.b.area (after edit 1) | value 144 mm^2 |
| Pair.a.area (after edit 1) | value 100 mm^2 |
| Pair.b.w (after edit 2) | value 22 mm |
| Pair.b.area (after edit 2) | value 484 mm^2 |
| Pair.a.w (after edit 2) | value 10 mm |

The gesture, its refusals and its preview are the session's (task 10551).


---

## Waiting on the geometry kinds

### constraint-verdicts-including-geometry-measure

- Tests: each constraint of a root is a verdict at its own path, one of which reads a geometry measure.
- Rule: contract Example 2; Part 2 (constraint).
- Provenance: contract Part B Example 2.
- Needs: instance, constraint, geometry
- Answer basis: traced. `t` = 5 mm ≥ 3 mm: satisfied. `box(40 mm, 40 mm, 5 mm)` has volume 8000 mm³ ≤ 10000 mm³: satisfied. Row 1 needs no geometry; row 2 does.
- Today: the contract marks it "checked".

```ri
structure def Plate {
    param t : Length = 5mm
    param w : Length = 40mm
    let body = box(w, w, t)
    constraint t >= 3mm
    constraint volume(body) <= 10000mm^3
}
```

| Path | Expect |
|---|---|
| Plate/constraint[0] | verdict satisfied |
| Plate/constraint[1] | verdict satisfied |

### purpose-conforms-is-measured-not-scalar

- Tests: a geometric `Conforms` with an explicit `actual`, written inside a purpose, gets the verdict it would get written in the definition (measured), never the satisfied verdict that the scalar predicate alone would give.
- Rule: Part 10 ("Its constraints are verdicts and nothing else"); Part 16 (for the geometry design); decisions E36
- Provenance: task 9452 (its purpose half. Its `forall` half follows from forall-constraints-bear-on-owner-auto, since per-element constraints are ordinary members); `examples/gdt_conformance_violated.ri` (the same `Conforms` written in a definition)
- Needs: instance, constraint, purpose, grammar-purpose, made, geometry
- Answer basis: traced. This holds on the condition that the geometry kinds (task 10611) measure a `Conforms` with an explicit `actual`, as today's engine does for `gdt_conformance_violated.ri`.
  A purpose's constraint is an ordinary verdict over its subject (Part 10). The measured deviation is 0.5 mm and the zone is 0.1 mm, so the verdict is violated.
  The scalar predicate alone (0.1 mm >= 0 mm) would be satisfied, which is what the record says happens today.
  If the geometry kinds cannot measure it, the expected verdict is undetermined. Either way it is never satisfied.
- Today: the language-level predicate gives a false "satisfied" for a `Conforms` injected by a purpose (task 9452 record).

```ri
structure def FlatnessCallout : GeometricTolerance {
    param tolerance_value : Length = 0mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.RFS
}
structure def Cube {
    pub let part = box(10mm, 10mm, 10mm)
    pub let measured = translate(part, 0.5mm, 0mm, 0mm)
}
purpose inspected(subject : Structure) {
    constraint Conforms(
        tolerance: FlatnessCallout(feature: subject.part, tolerance_value: 0.1mm),
        actual: subject.measured,
        measured_deviation: 0mm,
        feature_departure: 0mm
    )
}
structure def Holder {
    sub c : Cube
    purpose inspected(self.c)
}
```

| Path | Expect |
|---|---|
| Holder/purpose[0]/constraint[0] | verdict violated |

### purpose-evaluates-geometry-queries

- Tests: a purpose's constraint can evaluate a geometry query over its subject's geometry member, and over a geometry `let` of the purpose's own, like any constraint.
- Rule: Part 10; Part 2 (a cell is one unit of source evaluated in a scope); decisions E36
- Provenance: task 10273 (both of its measured shapes)
- Needs: instance, constraint, purpose, grammar-purpose, geometry
- Answer basis: traced for `purpose[0]`; open for `purpose[1]` and the summary.
  `Holder.p.b` is a 10 mm cube, whose volume is 1000 mm³ > 1 mm³, so purpose[0]'s constraint is satisfied. A volume needs the geometry kinds (task 10611).
  purpose[1]'s `let b` builds geometry inside the purpose's body. Decisions E36 does not pin whether the compile error for that shape (task 10037's purpose-body arm; task 10273's `PurposeGeometryUnrealizable`, by task 10037's amendment) survives, so its verdict and `Holder/summary` are open.
- Today: both shapes are INDETERMINATE: "operator undefined for these operand kinds: GeometryHandle" and "undefined inputs: purpose:review@P.__let_b" (task 10273 record, built at 23a721d574).
- Note: open question, owner task 10611: is a geometry `let` inside a purpose's body a compile error, or a cell of the purpose instance that is evaluated like any other? If it is evaluated, `Holder/purpose[1]/constraint[0]` is satisfied and `Holder/summary` is `summary violated 0 undetermined 0 failed 0`.

```ri
structure def P { param w : Length = 10mm   pub let b = box(w, w, w) }
purpose review(subject : Structure) { constraint volume(subject.b) > 1mm^3 }
purpose review_local(subject : Structure) {
    let b = box(subject.w, subject.w, subject.w)
    constraint volume(b) > 1mm^3
}
structure def Holder {
    sub p : P
    purpose review(self.p)
    purpose review_local(self.p)
}
```

| Path | Expect |
|---|---|
| Holder/purpose[0]/constraint[0] | verdict satisfied |
| Holder/purpose[1]/constraint[0] | open (owner 10611) |
| Holder/summary | open (owner 10611) |


---

## Other

### connect-end-on-value-is-compile-error

- Tests: an end of a `connect` that is a port of a value held in a cell is a compile error that says to write subs.
- Rule: decision 9; Part 10 ("An end must be a port of the declaring instance itself or of one of its subs or elements"); Part 7.4.
- Provenance: design decision 9's model. claims-1 row 8 (probe c1.ri compiles today); second-design Q5.
- Needs: compile-error, connect
- Answer basis: stated (decision 9: "This compiles today and would be a compile error that says to write subs"). The rejection is built by leaf ο, whose word is `connect` (decisions F; trace-structure, wrong tags).
- Today: compiles (design decision 9).

```ri
trait Flow {}
structure def Shaft { port end : out Flow {} }
structure def Hub   { port bore : in Flow {} }
structure def Asm {
    let s = Shaft()
    let h = Hub()
    connect s.end -> h.bore
}
```

| Path | Expect |
|---|---|
| (compile) | error: a connect end must be a port of this instance or of one of its subs or elements; `s` and `h` are values held in cells, write them as subs |

### let-cycle-fails-ring-blocks-readers

- Tests: a cycle between two `let`s of an instance fails only the cells on the ring, and a `let` or a child's argument that reads the ring is blocked by that failure, not part of the cycle.
- Rule: Part 2 (what a cell can return); Part 1 rule 2 (a parameter evaluates its argument in the writer's scope); contract 7.7 and the paragraph on failures as results; decisions C14 and B2
- Provenance: task 5819; task 8051
- Needs: compile-error, instance
- Answer basis: traced.
  Decisions C14: a ring of `let`s inside one definition compiles, so `(compile)` is ok.
  The core's cycle detection (core task 10558) is a prerequisite of leaf ε₁, which leaf ε₂ (the builder of `instance`) waits for, so the entry does not run before its answer can exist.
  The contract's 7.7 gives every node on the ring the failure "cycle"; its paragraph on failures gives a reader of a failed input "blocked".
  So `a` and `b` fail with cycle. `c` reads `a`, so it is blocked.
  `Parent.m.k.w` evaluates the argument `a` in `Mid`'s scope, so it is blocked too. `Parent.m.k` itself is present.
  Today the cycle is reported at evaluation (run).
- Today: probe `cyc.ri` (run) prints `circular let-binding dependency in template Mid: [a, b, c]` three times, and `Parent.m.k.w = undef`. Task 8051 is that `c` is named in the cycle.

```ri
structure def Kid { param w : Real = 1.0 }
structure def Mid {
    let a = b * 2.0
    let b = a * 2.0
    let c = a + 1.0
    sub k = Kid(w: a)
}
structure def Parent { sub m = Mid() }
```

| Path | Expect |
|---|---|
| (compile) | ok |
| Parent.m.a | failure cycle |
| Parent.m.b | failure cycle |
| Parent.m.c | failure blocked |
| Parent.m.k | present |
| Parent.m.k.w | failure blocked |

### unsupplied-member-kind-fails-others-answered

- Tests: a member whose kind nobody has supplied (a geometry member, with no geometry kinds) is the failure `no-kind-supplied`, what reads it is blocked, and the instance's other members are answered.
- Rule: PRD 5.4 ("A member whose row is empty answers `no-kind-supplied`, and whatever reads it is blocked. Nothing is skipped in silence"); PRD 12.1 rule 2; decisions B2 and B4
- Provenance: PRD boundary scenario G1; brief item 12.
- Needs: instance, constraint, summary
- Answer basis: stated (PRD 5.4, G1) for the failures; traced for the rest. `body` is a geometry member: `no-kind-supplied`. `constraint[1]` reads it: blocked. `half` is 5 mm; `constraint[0]` is 10 mm > 5 mm, satisfied. Failed (decisions B4, blocked included): `body` and `constraint[1]`, 2.
- Note: the entry holds only while the geometry entry of `member_kinds` is empty. Once task 10611 fills it, the runner must run this entry with that entry emptied, or the entry retires; the PRD does not yet say which.
- Today: run `g12.ri`: `Block.body = <Geometry: Block#realization[0]>`, `Block.half = 0.005 m`.

```ri
structure def Block {
    param w : Length = 10mm
    let body = box(w, w, w)
    let half = w / 2
    constraint w > 5mm
    constraint volume(body) > 1mm^3
}
```

| Path | Expect |
|---|---|
| Block.body | failure no-kind-supplied |
| Block.half | value 5 mm |
| Block/constraint[0] | verdict satisfied |
| Block/constraint[1] | failure blocked |
| Block/summary | summary violated 0 undetermined 0 failed 2 |


---

## Appendix: the readings the answers rest on

These are the PRD lead's readings of the design, made so that the catalogue is consistent. The PRD's section 7 put each to Leo, as a decision (D3, D10) or in its list of smaller choices (D13). **He ruled on 2026-11-26:** as recommended, except that a user function is entered whatever its arguments (item 19, PRD D10(d)) and that a read of a private `let` from another definition is a compile error (item 38, PRD D13 item 14). On a question he asked the same day he then ruled that a read of a member that may be absent compiles wherever it is written (items 15 and 16, and the last bullet of item 20). Where the two documents differ, the PRD is right. Revised 2026-11-26 after the review of the PRD and the tracing of the catalogue, and again after his rulings.

### A. Path text (the starting form of rule F)

Rule F is settled by building, so this text may change. Use it uniformly.

| Step | Text | Example |
|---|---|---|
| Root | the definition's name; the entry file's module is not written | `Asm` |
| Member by name | `.name` | `Asm.thin.t` |
| Element by position | `[n]` | `Asm.vents[2]` |
| Element by key | `["key"]` | `Asm.vents["intake"]` |
| Summary of an instance | `/summary` | `Asm/summary`, `Asm.b/summary` |
| An instance read as a value | `/as-value` | `Asm.a/as-value` |
| A member table | `/defs[Name]` on the root | `Asm/defs[Plate]` |
| A constraint | `/constraint[k]` | `Asm.b/constraint[0]` |
| A constraint declared by a `forall` block, one per element | `/constraint[k][i]`, or `/constraint[k]["key"]` for a keyed collection | `Asm/constraint[1][3]` |
| A constraint written in a sub's braces | `/braces/constraint[k]` on the sub | `Rig.motor/braces/constraint[0]` |
| A `let` written in a sub's braces | `/braces.name` on the sub | `Asm.s/braces.zz` |
| The condition of a guard written in a sub's braces | `/braces/guard[k]` on the sub | `Asm.p/braces/guard[0]` |
| An applied purpose, and its constraints | `/purpose[k]`, then `/constraint[j]` | `Asm/purpose[0]/constraint[0]` |
| The built-in verdict on an instance's arguments | `/arguments` | `Asm.b/arguments` |
| A guard's condition cell | `/guard[k]` on the declaring instance | `Case/guard[0]` |
| A collection's count cell | `/count[name]` on the declaring instance | `Asm/count[vents]` |
| A sub's placement cell | `/at[name]` on the declaring instance | `Asm/at[a]` |
| An unknown with no name of its own | `/auto[member.argument]` on the instance that owns it, the member with its key for a keyed entry; written inside an expression, `/auto[member#chain.argument]`; a placement, `/auto[name.at]`; a connector's parameter, `/auto[connect[k].parameter]`; the forms combine | `Asm/auto[a.len]`, `M/auto[vents["a"].w]`, `Asm/auto[both#1.t]` |
| A made instance | `/made[chain]` on the cell; the chain is the call sites and iteration indices from the cell's expression to the constructor call | `Asm.c/made[0]`, `Asm.c/made[0,2]` |
| The list of instances a cell made | `/made` on the cell | `Asm.c/made` |
| A cell's value opened as an instance | `/open`; opened in mid-expression, `/open[chain]`, the chain ending with the position of the member read among the unit's member reads | `Asm.c/open.area` |
| What an instance tells a solve; the solve design's nodes | `/problem`, `/solves` | `Asm.a/problem` |

`k` for a constraint is its position among the verdict-bearing constraints of the instance's member table: those inherited from traits first, in the order the traits are listed, then the definition's own in source order. `k` for an applied purpose is its position among the applications in the member table, in source order; a purpose applied under a guard keeps its position when it is absent. A count constraint (`xs.count == n`) is the source of the count cell and is **not** a verdict: it has no `constraint[k]` and is not counted by a summary.

### B. Expect forms, and what a summary counts

1. **An expectation is what the host is answered at that path, not what a reader would compute from it.** A path below an absent member answers `absent`. A path below a member of undetermined presence answers `presence-undetermined`. What a reader makes of either (an undefined value) is pinned at the reader's own path, for example a `let` that reads it.
2. **A failure is the answer at the path of the node that failed.** Write `failure <kind>` at the cell's, the member's or the instance's own path. Kinds: `cycle` (every node on the ring), `blocked` (a node that read a failed node), `limit`, `ambiguous-declaration` (rule E: two declarations of one name present at once), `type-argument-unresolved` (a sub whose `auto` type argument the compiler could not choose), `no-kind-supplied` (a member whose kind nobody supplies). "Presence undetermined" is a failure code of the instance kinds; in an expectation it is written `presence-undetermined`, without the word failure, and it is the one failure a reader turns into undefined.
3. **A failed solve is not a failure.** The unknown is `value undefined`, and a verdict that reads it is `verdict undetermined`. Tag such an entry `solve-infeasible`.
4. **`summary violated <n> undetermined <n> failed <n>`**, counted at or below the instance:
   - violated: violated verdicts, an applied purpose's included; a made instance's verdicts are counted once, at the made instance, and an opened value's never;
   - undetermined: undetermined verdicts, plus one for each member whose presence is undetermined (nothing inside such a member is counted, and it is never counted as failed);
   - failed: every cell, constraint or instance whose own answer is a failure, `blocked` included, whether or not a verdict reads it. So a ring of two `let`s that nothing reads is `failed 2`.
   The summary reads its instance's verdicts and cells, the cells that are not members included (a guard's condition, a count, a placement, an unknown with no name), and whatever those cells made. Its result has four groups (constraints; everything at or below a purpose instance; verdicts on arguments; cells, members and instances that failed or whose presence is undetermined). The three counts of an expectation are the sums over the groups, and an expectation may name a group.
5. **The built-in verdict on arguments** (`/arguments`): every instance has one; with no arguments it is satisfied. It is undetermined when the value of some argument is undefined and that argument is not the literal `undef`. An explicit `undef` does not make it undetermined. An argument that is an unknown left undefined by a failed solve makes it undetermined, and so does an argument whose own guard is undefined. When an argument's cell is a failure, the verdict is `failure blocked`. It never takes part in a solve.
6. **An argument given to a parameter that is absent** (the parameter is under a false guard): ruled by Leo as recommended (PRD D10(a)). The `/arguments` verdict is violated, and its message says to put the assignment under the same guard; it is undetermined with the guard's class when the parameter's presence is undetermined.
7. **`constrained(p)`**: ruled by Leo as recommended (PRD D3). True when some present constraint written in the text of the definition of the instance that holds `p`, or of a definition below it, can read `p` according to the source. A constraint a definition writes in the braces of its own sub counts; one that the declarer of the instance writes in its braces does not, and neither does a purpose's. Entries pin the value, and pin with `same-as` what the design requires: the sub, the made instance and the opened value answer the same.
8. **More forms.** `same-as <path>`: the two paths answer the same. `undetermined-because partial` or `undetermined-because other`, on the path of an undetermined verdict or of a member whose presence is undetermined: partial when every root cause is a root's parameter with no value or a literal `undef`; other for a read of something not there, an operation with no value, a failed solve.
9. **More bullets an entry may carry.** `- Settings: depth of a path = <n>` (a limit that changes answers). `- Candidates: <unknown's path>: <v1>, <v2>, ...` for each unknown the test solver decides; the lists of one owner are tried in step, the k-th value of each together; the list must hold at least one value that a wrong reading would take first, so that the entry shows what bears. `- Edits:` a numbered sequence; each edit applies to the text the one before it left.
10. **The default roots** are written on one row: `(file) | roots A, B`, compared as a set.

### C. Compile stage

11. **Same-name declarations under guards compile**, whether or not the guards can be proved exclusive (spec 6.4's restriction is to be amended). Rule E gives the run-time answers. The exception is two declarations that give different kinds of node (a collection and a single sub): a compile error.
12. **A read, from another instance, of a member that only some `match` arms' definitions have compiles.** At run time it is undefined where the chosen arm's definition lacks the member.
13. **`t = t / 2` in a sub's braces compiles** and is a run-time `failure cycle` at the parameter's cell (`Asm.p.t`). In the call form, `sub p = Plate(t: t / 2)`, ruled by Leo as recommended (PRD D10(g)): the argument is the caller's expression and reads the parent's `t`.
14. **A ring of `let`s inside one definition compiles** and is a run-time `failure cycle` on each cell of the ring, with `failure blocked` on what reads them.
15. **A read of a member that may be absent compiles wherever it is written: ruled by Leo on 2026-11-26 (the end of PRD D13); it amends the design's decision 3b line 1 and Part 4, and removes the spec's reference-safety rule (6.3, 8.10).** Where the member is absent the read is undefined, with the class other. A guard's condition is a declaration under the guards that enclose it, and not under the guard it states: a guard that reads a guarded member is undefined where that member is absent, and the member it guards is then of undetermined presence. Two things stay compile errors: a guard that reads the very member it guards, and a name that no configuration declares.
16. **Applying a purpose whose subject is a guarded member of the applying definition, on a line with no guard of its own, compiles** (item 15). Where the subject is absent the purpose instance is there and its verdicts are undetermined, other, as Part 10 says for a subject across an instance (`self.c.fan`). Written `purpose p(self.fan) where needs_fan`, the purpose instance is absent with its subject.
17. **A purpose declared with a parameter of kind Constraint or Field is a compile error at the declaration.** So is an objective, or an `auto`, inside a purpose, and a purpose line inside a purpose's body or a trait.
18. **Which parameters of a purpose are subjects: ruled by Leo as recommended (PRD D10(e)).** A parameter whose declared type is a structure, a trait or `Structure` is a subject, and its argument must name an instance: `self`, or a sub or an element with a literal index or key below it. A value held in a cell is refused. A whole collection is refused. Any other parameter is a value. A purpose with no parameter is applied as `purpose name()`.
19. **A user function called with an undefined argument is entered: ruled by Leo (PRD D10(d)); it amends the spec's 9.2.7.** Each operation inside propagates, so a function that does not read its argument answers as if it were defined, and a constructor call in the body makes its instance. `mk(u)` with `fn mk(t : Length) -> Plate { Plate(t: t) }` answers as `Plate(t: u)` does: a plate whose `t` is undefined, with its `/arguments` verdict undetermined and the same class. A parameter of a function keeps the class of the argument it was bound to, and whether that argument was the literal `undef`. A built-in called with an undefined argument is still not run.
20. **In a sub's braces:**
    - a bare name is first a `let` of the same braces, then a member the sub's definition declares, present or not, then a name of the declaring definition; `self` is the parent. A member found at the second step that is a `let` without `pub` is a compile error, and the name is not looked for further (item 38);
    - a `let` there, ruled by Leo as recommended (PRD D10(f)): only the other members written in the same braces can read it. A read of it from the parent (`self.s.zz`) is a compile error;
    - a `let` that repeats a name of the sub's definition is a compile error;
    - `let u : Length = auto` is a compile error (write it in the parent and pass it), and so is `auto` as a constructor argument inside a member written there;
    - a constraint in the braces of a list sub is a member of every element;
    - `purpose Name(args)` is a compile error (write the line in the parent, with the sub as its subject);
    - a constraint that reads, with no guard of its own, a member the sub's definition declares under a guard compiles (item 15) and is undetermined where the member is absent.
21. **A parameter with no default that is given only under a guard** (`len = 20mm where long`) is a compile error, with task 7874's rule. Tag `required-arguments`: it waits.

### D. Rule D (default roots)

22. A use inside a function body or a purpose body does not count.
23. A constructor call inside a lambda that is written in a member's expression counts: it is part of that member's source.
24. A definition that is the type of a member, written or inferred, counts as used, even when the value comes from a function. (`let b = mk_bracket()` uses `Bracket`.)
25. A generic definition with a type parameter that has no default is never a default root, the rule's last clause included. A file that holds nothing else has no root.
26. The fallback clause ("if that leaves no root ... every one of those is a root") appears unreachable otherwise. Keep at most one entry for it, marked `open`, saying so.

### E. Other

27. **The guard of a sub's argument is undefined** (`t = 6mm where thick`, `thick` undefined): the parameter is `value undefined` and the instance's `/arguments` verdict is undetermined. What the instance's other unknowns do, and whether an argument's guard is read before a solve's view or through it, are the solve design's: leave those paths open, owner 10610.
28. **Every guarded member, whatever its kind, is `presence-undetermined` under an undefined guard**, and counts one undetermined in its parent's summary. A collection whose count is undefined is `presence-undetermined`, and so is a `forall` block over it; each counts once. A guard that itself failed blocks the member (`failure blocked`).
29. **`present(x)`** is true, false, or `value undefined` when the presence of `x` is undetermined.
30. **`self.children`, `self.members`, `self.descendants`** list present members as values; if any member's presence is undetermined, the list is `value undefined`.
31. **A value read from an instance** leaves out absent parameters and carries a parameter of undetermined presence as undefined. The value still exists.
32. **A guarded assignment in a sub's braces** is written `t = 6mm where thick`. The block form `where thick { t = 6mm }` does not parse and is not used.
33. **A purpose is applied by the member line `purpose Name(args)`**, arguments as in a constructor call (positional or named).
34. **Nothing at or below a purpose instance is part of the problem of an owner outside that purpose instance**, by path: its constraints, those of a constraint definition it applies, and those of any instance its expressions make. An instance a purpose's expression makes still decides its own unknowns from its own constraints.
35. **The test solver** takes the first candidate under which no constraint that depended on the trial is violated, undetermined or failed. A constraint that is violated whatever the trial does not bear and stays a violated verdict.
36. **Geometry inside a purpose** (tasks 9452, 10273, 10037): keep the entries, needs `geometry`, owner 10611. Do not pin whether task 10037's compile error for an inline geometry query in a purpose's body survives: mark that path `open`.
37. **Objectives.** Whether an objective written in a definition reaches an unknown owned by another instance (contract Example 11 forms (3) and (4); `let d = Plate(t: auto)` with only lower bounds) is the solve design's: `open`, owner 10610.

38. **A `let` that is not `pub` can be read only in its own definition's text: ruled by Leo (PRD D13 item 14); it is the spec's 7.4 and 8.3, which today's compiler does not enforce.** A read of it from another definition is a compile error, through a sub (`self.p.area`) and through a value (`c.area`) alike. Parameters, subs and ports are read from outside as before. Two places where this is the lead's reading of the ruling: the braces of a sub are the declarer's text, so a bare name there that is a private `let` of the sub's definition is a compile error and does not fall through to the declarer's member of that name; and a purpose's body reads only the `pub` members of its subject. An expectation's path is not source: the runner reads a private `let` by path like any cell.
39. **An undefined parameter read back out of a value: ruled by Leo as recommended (PRD D10(h)).** Partial when the cell that holds the value made it itself, other when the value has flowed through another cell or a list. After the ruling on item 19 the lead reads "made it itself" as covering a constructor call inside a function that the cell called, so that the wrapped call answers as the inline one.

### F. The needs vocabulary

Each word belongs to exactly one leaf of the PRD, or to one other task. An entry carries the word of every leaf whose work its pinned answers need. An entry that expects a compile error carries `compile-error` and the word of the leaf that builds that rule.

| Word | The entry needs | Built by |
|---|---|---|
| `two-files` | a source made of several files | α |
| `paths` | to pin the text of a path itself (rule F) | β |
| `grammar-purpose` | a source that has a `purpose Name(args)` member line | γ₁ |
| `compile-error` | a compile outcome | δ₂ |
| `instance` | roots, subs, parameters, `let`s, arguments, `determined(...)` | ε₂ |
| `eval-reads` | rule A: what the evaluator reads, operand order, a cycle through an operand | ε₂ |
| `constraint`, `summary` | a verdict; a summary's counts, the verdict on arguments, the class of a cause | η |
| `guard` | a guard, presence, `present(...)`, a read of something not there | θ₁ |
| `one-name` | one name declared more than once, `where`/`else`, `match` arms | θ₂ |
| `collection` | a list or keyed sub, a count, a `forall` block, an index | ι |
| `value` | an instance read as a value, `self.children` and its kin, equality of values | κ₁ |
| `made` | a constructor call in an expression | κ₂ |
| `open` | a member read on a value | κ₃ |
| `braces` | a `let`, a constraint or a guard written in a sub's braces, or a name resolved by the braces rule. An argument assignment alone does not need it | λ |
| `unknown` | an `auto` anywhere; where `auto` may be written | μ₁ |
| `solve-forced` | a decided unknown where every feasible value gives the pinned answers | μ₁ |
| `solve-infeasible` | an unknown no value satisfies | μ₁ |
| `nested-solve` | an owner with an unknown that contains, at any depth, another instance with an unknown of its own; `auto` as a constructor argument | μ₂ |
| `purpose` | a purpose, declared or applied | ν |
| `roots` | the default roots of the file | ξ |
| `connect` | a `connect`, a port, a placement, a geometry member's source | ο |
| `generic` | a generic definition | π |
| `state-tests` | `constrained`, `partially_determined` | ρ |
| `edits` | a sequence of edits | σ |
| `solve-real` | an answer that depends on an objective, the default objective, how solves are grouped, or what a solver does with an undetermined constraint | task 10610; waits |
| `geometry` | a geometry kind | task 10611; waits |
| `session` | a surface | task 10551; waits |
| `indexed-subs` | the indexed form of a sub | task 5482; waits |
| `required-arguments` | the rule that a required argument must be given | task 7874; waits |

An entry that pins only rows no solve decides does not carry `solve-real`, even when its subject is a question for the solve design: it runs, and its open rows are simply not pinned.
