# PRD: Specialization-Scope Validation

## Goal

In specialization bodies (sub-entity instantiation blocks like `sub motor : ElectricMotor { ... }`), reject `param`, `port`, and `sub` declarations with a clear diagnostic, per spec §8.7.

## Background

- Spec §8.7 (lines 1531-1561): a specialization scope is the body of a `sub` instantiation. It can configure an existing definition but cannot extend its schema.
- Permitted in a specialization body: parameter assignments (`thickness = 3mm`), `constraint`, `let`, `connect`, `where` guards.
- Forbidden: new `param`, `port`, or `sub` declarations.
- Today these may parse silently or produce confusing downstream errors. We want a single dedicated validation pass with a precise diagnostic.

## Scope

- A validation pass over each specialization scope body that walks declarations and rejects any of the three forbidden member kinds.
- New diagnostic code (e.g. `E_SPECIALIZATION_FORBIDDEN_DECL`) naming the forbidden kind and pointing at the offending declaration's span.
- Applies anywhere a specialization scope appears: top-level `sub` blocks, nested `sub` blocks, `match`-arm `sub` blocks (the `match` desugars to per-arm `sub` decls — each arm's `{...}` is a specialization scope), `forall ... : connect`/`constraint` desugarings if they generate specialization scopes.
- Permitted forms (assignments, constraints, lets, connects, where guards) are unaffected.

## Out of scope

- Auto-fix / migration tooling.
- Allowing limited `param` extensions via traits (separate path: define a refining structure).
- Lifting the restriction for `let` bindings (already permitted).

## Acceptance criteria

1. `sub motor : ElectricMotor { param x : Length }` — error E_SPECIALIZATION_FORBIDDEN_DECL identifying `param`.
2. `sub motor : ElectricMotor { port p : MechanicalPort }` — error identifying `port`.
3. `sub motor : ElectricMotor { sub child : Foo }` — error identifying `sub`.
4. `sub motor : ElectricMotor { thickness = 3mm; constraint thickness > 1mm; let m = thickness * 2 }` — no error.
5. `where`-guarded forms inside specializations work: `sub motor : ElectricMotor { thickness = 3mm where high_torque }`.
6. Diagnostic span points at the forbidden keyword + name, not at the entire body block.
7. The validator runs after parsing; no parse-error / panic surfaces if the user attempts a forbidden form.

## Task breakdown

1. Identify the specialization-scope AST node (or distinguishing context flag) and add a single-pass validator that traverses its members.
2. Implement the rejection rule + diagnostic code; format messages with the offending kind name.
3. Tests: each forbidden kind in isolation, all three combined, permitted-only bodies (negative), nested `sub` whose own body has a forbidden decl, `match`-arm `sub` body with forbidden decl.
4. LSP integration: ensure the diagnostic surfaces via the standard diagnostics channel.
