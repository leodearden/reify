# Tensor static shape: carry M×N in the type (DEFERRED STUB)

**Milestone:** v0_6 · **Status:** **deferred stub** — bookmark task tracks activation · **Date:** 2026-06-10

## Problem

`Type::Tensor` carries a single `n`: the row count of an M×N matrix is discarded at the type layer (`crates/reify-compiler/src/math_signatures.rs` — documented there as limitation D5, noted at the 4179/4182 signature work). Consequences:

- Shape-mismatched tensor expressions (e.g. 3×3 vs 2×3 in the same arithmetic) are not statically rejectable; they surface as runtime Undef or wrong-shaped results.
- `matrix([[…]])` construction can't be checked against a declared `Tensor<2,3,Q>` member beyond column count.
- The type-hygiene PRD's relational-operand guard (`docs/prds/v0_6/type-hygiene.md` §7.1) rejects tensor comparisons by *kind*; with full static shape it could also pin shape in fixits and in any future structural-equality widening (decision 3 there names that widening as revisitable).

## Direction sketch (not ratified — full /prd session decides)

Extend `Type::Tensor` to carry both extents (`rank`, `rows`, `cols` or a dims smallvec), thread through: `math_signatures.rs` result rows (`matrix`/`identity`/`transpose`/`inverse` shape algebra), `implicitly_converts_to` tensor rules (the Rule-2c family, `type_compat.rs`), `matrix()` arg checking, conformance member matching, LSP hover. Value layer (`Value::Tensor`) already knows its real shape — this is type-layer-only surgery. Expect wide but mechanical churn in reify-compiler tests.

## Why deferred

Type-system surgery with broad mechanical blast radius and **no urgent consumer**: the hygiene guard only needs kind-level rejection; FEA/dynamics surfaces fix their shapes by construction. Activation trigger: a consumer that needs static shape rejection (structural tensor equality, shape-polymorphic stdlib fns, or a recurring class of runtime shape bugs).

## Activation protocol

Run `/prd` author mode against this stub; the bookmark task (filed at type-hygiene decompose, deferred per the bookmark-task pattern) is the tracking handle. Do not implement from this stub.
