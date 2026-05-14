# PRD: Deep Dot-Chain Warning

## Goal

Warn at compile time when a member-access chain `a.b.c.d.e` exceeds a configurable depth threshold, per spec §5.7. Default threshold for v0.1: **4 levels** (i.e., `a.b.c.d` is OK; `a.b.c.d.e` warns).

## Background

- Spec §5.7 (line 1077-1087): "Dot-notation. Chained access resolves through the containment tree. Unlimited dot-chain depth permitted; compiler warns on deep chains (threshold configurable, suggested default: 3-4 levels)."
- Rationale: deep chains are usually a Law-of-Demeter smell — the design has reached too far across the containment tree. Catching them early nudges designers to introduce intermediate `let`s or to push computation closer to the data.

## Scope

- Single-pass syntactic check during AST validation (post-parse, pre-typecheck is fine; no semantic info needed).
- Count the number of `MemberAccess` nodes in a left-to-right chain, where each node's left-hand side is itself a `MemberAccess` or a bare identifier.
- New diagnostic code (e.g. `DeepDotChain`) reporting full chain text and span.
- Threshold configurable but with a hardcoded v0.1 default of 4 (chains of length > 4 warn).
- Method-call chains (`x.foo().bar().baz()`) are out-of-scope unless the spec extends — v0.1 only counts pure member-access (`.field`) hops. Document this explicitly so we don't surprise users with method-call lint noise.

## Out of scope

- Cross-file or design-tree-wide chain analysis.
- Auto-fix / refactor suggestion (could be a future LSP code action).
- Per-project threshold override (config knob can come post-v0.1).
- Method-call chains.

## Acceptance criteria

1. `a.b.c.d` (4 hops) does not warn.
2. `a.b.c.d.e` (5 hops) emits DeepDotChain with the full chain text and span.
3. `a.b.foo().c.d` (mixed call+access) does not trip the lint in v0.1 (out-of-scope).
4. Indexing in the middle (`a.b[0].c.d.e`) — count `.field` hops only; treat the indexed expression as a fresh chain root so `a.b[0]` is hop-1.
5. Test coverage: at-threshold (no warn), one-over-threshold (warn), deeply nested chains in let bodies and constraint bodies, mixed indexing/method calls (no false-positive).

## Task breakdown

1. Implement chain-depth counter on AST `MemberAccess` walker.
2. Wire diagnostic code + format + LSP path.
3. Tests: threshold boundary, mixed expression forms, multi-chain expressions in same line.

## Note on AC #3 (mixed call+access) — GR-040, 2026-05-14

The "mixed call+access" form `a.b.foo().c.d` referenced in AC #3 is not currently expressible in Reify source: `ExprKind::FunctionCall { name: String, args: Vec<Expr> }` takes a bare identifier as callee, so `a.foo()` doesn't parse. The lint passes vacuously on any "mixed call+access" input by construction — there are no inputs to assess. AC #3 remains correct as written ("does not trip the lint"), but the framing assumes a syntax that doesn't exist.

If method-call syntax ever lands (UFCS sugar `a.foo(b) ⇔ foo(a, b)`, full method dispatch, or namespace-qualified calls `mod::foo()`), this lint's `MemberAccess`-only walker will need to be re-walked with real test cases to confirm chain counting still works across method-call segments. The minimal-cost option if Reify ever wants method-call syntax is UFCS sugar — additive, no method-resolution rules, gives readability wins on transform/builder chains without committing to OO dispatch semantics.

See `docs/architecture-audit/gap-register.md` GR-040 for the audit-trail entry.
