/**
 * Unit pins for the constraint-verdict token extraction helper (task 6723).
 *
 * Every case below runs against SYNTHETIC Rust-source string literals — no
 * on-disk fixture — mirroring ./toolDefNames.test.ts.  That keeps the pattern
 * semantics pinned independently of whatever engine.rs happens to contain today.
 *
 * COVERAGE BOUNDARY.  The real cross-language drift guard — "the token the
 * engine emits is the token the frontend renders" — lives ONLY in
 * ./constraintVerdictParity.test.ts.  The single real-file touch in this file
 * is a path-resolution + non-vacuity smoke check: it asserts all three
 * variants still extract, so a future `Satisfaction` rename cannot quietly
 * hollow out the parity guard, but it deliberately does NOT assert the token
 * VALUES (that would duplicate the parity test's job here).
 */
import { describe, it, expect } from 'vitest';

import {
  extractVerdictTokens,
  readEngineSource,
  SATISFACTION_VARIANTS,
} from './constraintVerdictTokens';

describe('extractVerdictTokens', () => {
  it('extracts each variant from the single-helper form (post-6723 engine.rs)', () => {
    const src = `
      pub(crate) fn satisfaction_token(s: Satisfaction) -> &'static str {
          match s {
              Satisfaction::Satisfied => "satisfied",
              Satisfaction::Violated => "violated",
              Satisfaction::Indeterminate => "indeterminate",
          }
      }
    `;
    expect(Object.fromEntries(extractVerdictTokens(src))).toStrictEqual({
      Satisfied: 'satisfied',
      Violated: 'violated',
      Indeterminate: 'indeterminate',
    });
  });

  it('reports tokens VERBATIM, and collapses repeated arms that agree', () => {
    // The helper never normalises: casing is exactly what makes this defect
    // detectable, so folding it here would destroy the signal. And a variant may
    // legitimately be matched more than once — arms that AGREE collapse to one
    // entry, which is what keeps the ambiguity throw below specific to DRIFT.
    const src = `
      Satisfaction::Satisfied => "Satisfied",
      Satisfaction::Violated => "Violated",
      Satisfaction::Indeterminate => "Indeterminate",
      // … several hundred lines away …
      Satisfaction::Satisfied => "Satisfied",
    `;
    expect(Object.fromEntries(extractVerdictTokens(src))).toStrictEqual({
      Satisfied: 'Satisfied',
      Violated: 'Violated',
      Indeterminate: 'Indeterminate',
    });
  });

  it('spans arbitrary whitespace around `=>` (the `\\s*` in the pattern)', () => {
    expect(
      Object.fromEntries(
        extractVerdictTokens(
          'Satisfaction::Satisfied=>"a"\nSatisfaction::Violated\n    =>\n    "b"\nSatisfaction::Indeterminate => "c"',
        ),
      ),
    ).toStrictEqual({ Satisfied: 'a', Violated: 'b', Indeterminate: 'c' });
  });

  it('does NOT capture a `satisfaction_token(Satisfaction::Indeterminate)` guard call', () => {
    // The two Indeterminate GUARD comparisons in surface_geometry_derived_cells
    // route through the helper rather than a bare literal. They are calls, not
    // match arms, so they must contribute no token — otherwise a guard rewrite
    // could inject a phantom mapping.
    const src = `
      if surfaced_any && constraints.iter().any(|c| c.status == satisfaction_token(Satisfaction::Indeterminate)) {
          if c.status != satisfaction_token(Satisfaction::Indeterminate) { continue; }
      }
      Satisfaction::Satisfied => "satisfied",
      Satisfaction::Violated => "violated",
      Satisfaction::Indeterminate => "indeterminate",
    `;
    expect(extractVerdictTokens(src).get('Indeterminate')).toBe('indeterminate');
  });

  it('THROWS (never silently returns empty) when a variant is unmatched', () => {
    // A vacuous extraction is the failure mode that would let the parity guard
    // pass while asserting nothing at all.
    expect(() => extractVerdictTokens('')).toThrow(/found no token for/);
    expect(() =>
      extractVerdictTokens(`
        Satisfaction::Satisfied => "satisfied",
        Satisfaction::Violated => "violated",
      `),
    ).toThrow(/Satisfaction::Indeterminate/);
  });

  it('THROWS when one variant maps to two different tokens', () => {
    // Exactly the two-drifting-copies state task 6723 removed.
    const src = `
      Satisfaction::Satisfied => "Satisfied",
      Satisfaction::Violated => "violated",
      Satisfaction::Indeterminate => "indeterminate",
      Satisfaction::Satisfied => "satisfied",
    `;
    expect(() => extractVerdictTokens(src)).toThrow(/Ambiguous constraint-verdict extraction/);
  });
});

describe('readEngineSource', () => {
  it('resolves the real engine.rs and yields all three verdict tokens', () => {
    // Non-vacuity smoke check: pins the path math and proves the pattern still
    // matches the live producer. Token VALUES are asserted in
    // ./constraintVerdictParity.test.ts, not here.
    const tokens = extractVerdictTokens(readEngineSource());
    expect([...tokens.keys()].sort()).toStrictEqual([...SATISFACTION_VARIANTS].sort());
  });
});
