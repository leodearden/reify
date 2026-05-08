// @vitest-environment node
/**
 * Config-value regression test for task 3185.
 *
 * The GUI vitest suite was flaking under cargo-concurrency load because the
 * default 5 000 ms testTimeout and 10 000 ms hookTimeout were too tight.
 * Commits 085a9b77f6 / 236a759bf6 / 93a97c6102 worked around this by adding
 * per-test overrides in colormap.test.ts (and potentially other files).
 * Task 3185 centralises those overrides into gui/vitest.config.ts so future
 * test files don't need to repeat the pattern.
 *
 * These assertions exist specifically to lock in that fix: a future PR that
 * tightens the timeouts back to vitest defaults would fail here and surface
 * the regression intentionally (esc-2915-17 / esc-3061-3 precedent).
 */

import config from '../../vitest.config';

describe('gui vitest config — cargo-concurrency flake guard (task 3185)', () => {
  it('testTimeout is at least 15 000 ms so per-test overrides are not needed', () => {
    // Lock-in: testTimeout must be ≥ 15 000 ms.
    // This is the value previously applied per-test in colormap.test.ts (task 3185).
    // Reducing it below 15 000 ms would re-introduce cargo-concurrency flakes.
    expect((config.test as { testTimeout?: number }).testTimeout).toBeGreaterThanOrEqual(15_000);
  });

  it('hookTimeout is at least 30 000 ms so the cold barrel beforeAll does not timeout', () => {
    // Lock-in: hookTimeout must be ≥ 30 000 ms.
    // vitest's beforeAll runs under hookTimeout (default 10 000 ms), not testTimeout.
    // The colormap.test.ts beforeAll used }, 30_000) to cover the cold barrel
    // module import (~2.3 s + scheduling slack). Setting this globally lets us
    // remove that per-hook override cleanly (task 3185).
    expect((config.test as { hookTimeout?: number }).hookTimeout).toBeGreaterThanOrEqual(30_000);
  });
});
