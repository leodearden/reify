import { describe, it, expect } from 'vitest';
import { countErrorNodes } from './lezerProbeHelpers';

/**
 * ANTI-VACUITY GUARD. Most grammar assertions are of the form
 * `countErrorNodes(...) === 0`, and the corpus drift ledger counts a fixture
 * clean on the same test. If the helper ever silently degraded to always
 * returning 0 — a @lezer/lr change to `cursor.type.isError`, or
 * `tree.cursor()` gaining a default that skips anonymous/error nodes — every
 * grammar suite would stay green while covering nothing. This pins that the
 * helper can still report a non-zero count.
 */
describe('lezerProbeHelpers — countErrorNodes', () => {
  it('reports error nodes on input that cannot parse', () => {
    // Measured: 3 error nodes.
    expect(countErrorNodes('@@@ !!! ???')).toBeGreaterThan(0);
  });

  it('reports zero on input that parses', () => {
    expect(countErrorNodes('structure def Foo { }')).toBe(0);
  });
});
