/**
 * Unit pins for the constraint-verdict token extraction helper (task 6723).
 *
 * Every case below runs against SYNTHETIC Rust-source string literals — no
 * on-disk fixture — mirroring ./toolDefNames.test.ts.  That keeps the pattern
 * semantics pinned independently of whatever engine.rs happens to contain
 * today, and in particular pins BOTH the pre-6723 two-match-block shape and
 * the post-6723 single-`satisfaction_token`-helper shape, so the extraction is
 * proven to survive its own refactor.
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

  it('extracts from the two-duplicated-match-block form (pre-6723 engine.rs)', () => {
    // The exact shape build_constraints and surface_geometry_derived_cells
    // carried before this task collapsed them: two byte-identical blocks.
    const src = `
      let status = match entry.satisfaction {
          Satisfaction::Satisfied => "Satisfied",
          Satisfaction::Violated => "Violated",
          Satisfaction::Indeterminate => "Indeterminate",
      };
      // … several hundred lines away …
      c.status = match new_sat {
          Satisfaction::Satisfied => "Satisfied",
          Satisfaction::Violated => "Violated",
          Satisfaction::Indeterminate => "Indeterminate",
      }
      .to_string();
    `;
    // Two agreeing copies collapse to one entry per variant — not an error.
    expect(Object.fromEntries(extractVerdictTokens(src))).toStrictEqual({
      Satisfied: 'Satisfied',
      Violated: 'Violated',
      Indeterminate: 'Indeterminate',
    });
  });

  it('reports the tokens verbatim — PascalCase and lower-case alike', () => {
    // The helper never normalises. Casing is exactly what makes this defect
    // detectable, so folding it here would destroy the signal.
    const pascal = `
      Satisfaction::Satisfied => "Satisfied",
      Satisfaction::Violated => "Violated",
      Satisfaction::Indeterminate => "Indeterminate",
    `;
    expect(extractVerdictTokens(pascal).get('Satisfied')).toBe('Satisfied');

    const lower = `
      Satisfaction::Satisfied => "satisfied",
      Satisfaction::Violated => "violated",
      Satisfaction::Indeterminate => "indeterminate",
    `;
    expect(extractVerdictTokens(lower).get('Satisfied')).toBe('satisfied');
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
