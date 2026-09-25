/**
 * The `CSS.escape` stub and arm table for `escapeAttrValue`'s two branches
 * (`gui/src/debug/bridge.ts`), shared so there is ONE home for the stub's
 * correctness.
 *
 * It lives here rather than inline in the suite that first needed it because
 * `escapeAttrValue` now serves five call sites in bridge.ts, and the next suite
 * that wants to exercise the `CSS.escape` arm would otherwise copy this
 * ~35-line algorithm. Two copies can then disagree about, say, the leading-`-`
 * or second-char-digit branch while both suites stay green — a divergence no
 * test could see. Import it; do not re-implement it (task #6178 review
 * amendment).
 */
import { vi } from 'vitest';

/**
 * CSSOM's "serialize an identifier" algorithm, standing in for the `CSS.escape`
 * jsdom does not provide (https://drafts.csswg.org/cssom/#serialize-an-identifier).
 *
 * It exists only to STUB the missing global. `escapeAttrValue`'s production arm
 * IS `CSS.escape`, and jsdom exposes no global `CSS` at all (asserted by the
 * `jsdom exposes no global CSS` case in debugBridge.test.tsx rather than
 * assumed), so without a stub the one arm every real webview takes would have
 * zero coverage anywhere in the suite — leaving the hand-rolled `["\\]`
 * fallback, which production never reaches, as the only branch any test
 * discriminated on.
 *
 * Faithful in exactly the way that matters here: it emits the hex escape
 * (`\31 `) and escaped space (`\ `) the real implementation does. Those are
 * IDENTIFIER escapes, and the property under test is that they still survive
 * being placed inside a DOUBLE-QUOTED attribute value.
 */
export function cssEscapePolyfill(value: string): string {
  const s = String(value);
  let out = '';
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    const ch = s.charAt(i);
    if (c === 0x0000) {
      out += '\uFFFD';
    } else if (
      (c >= 0x0001 && c <= 0x001f) ||
      c === 0x007f ||
      (i === 0 && c >= 0x0030 && c <= 0x0039) ||
      (i === 1 && c >= 0x0030 && c <= 0x0039 && s.charCodeAt(0) === 0x002d)
    ) {
      out += `\\${c.toString(16)} `;
    } else if (i === 0 && c === 0x002d && s.length === 1) {
      out += `\\${ch}`;
    } else if (
      c >= 0x0080 ||
      c === 0x002d ||
      c === 0x005f ||
      (c >= 0x0030 && c <= 0x0039) ||
      (c >= 0x0041 && c <= 0x005a) ||
      (c >= 0x0061 && c <= 0x007a)
    ) {
      out += ch;
    } else {
      out += `\\${ch}`;
    }
  }
  return out;
}

/**
 * The two arms of `escapeAttrValue`, each forced deterministically rather than
 * inferred from the environment: `vi.stubGlobal('CSS', undefined)` guarantees
 * the fallback arm even if a future jsdom starts shipping a partial `CSS`, and
 * the polyfill stub guarantees the production arm even though jsdom ships none.
 *
 * Each row's `install()` must run INSIDE the test (or its `beforeEach`), and
 * the caller is responsible for `vi.unstubAllGlobals()` in teardown.
 */
export const ESCAPE_ARMS = [
  { name: 'fallback', install: () => vi.stubGlobal('CSS', undefined) },
  { name: 'CSS.escape', install: () => vi.stubGlobal('CSS', { escape: cssEscapePolyfill }) },
] as const;
