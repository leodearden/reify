/**
 * Pure helpers for the bridge-mock coverage contract.
 *
 * INVARIANT ENFORCED (by the consumer, `bridgeMockCoverage.test.ts`):
 * every runtime export of `../bridge` is either a key of a target test file's
 * `vi.mock('../bridge', () => ({ ... }))` factory, or carries a documented
 * entry in that guard's `DELIBERATE_OMISSIONS` allowlist. The factory is a
 * NON-partial mock, so a missing key makes vitest throw
 * `No "X" export is defined on the "../bridge" mock` synchronously at property
 * access — which, depending on which `initApp` try/catch swallows it, surfaces
 * either as an stderr flood with every test still green, or as nothing at all.
 * Tasks 6035, 6039 and 6045 each fixed one name of this class by hand; this
 * module exists so the whole class is detected mechanically instead.
 *
 * Deliberately free of `node:fs` and of any vitest import: everything here is a
 * pure function over plain data, so the fragile half (source parsing) is
 * directly unit-testable with synthetic input — see `bridgeMockContract.test.ts`.
 *
 * This file has no `.test.` segment, so vitest does not collect it as a suite
 * (same convention as `debugBridgeTestHelpers.ts`).
 */

// One entry per quote style. A form with the closing paren included would be
// dead: `vi.mock("../bridge")` is a strict extension of `vi.mock("../bridge"`,
// so it can never be the earliest match.
const BRIDGE_MOCK_MARKERS = [
  `vi.mock('../bridge'`,
  `vi.mock("../bridge"`,
  'vi.mock(`../bridge`',
];

/** True for a character that may start a JS identifier key. */
function isIdentStart(ch: string): boolean {
  return /[A-Za-z_$]/.test(ch);
}

/** True for a character that may continue a JS identifier key. */
function isIdentPart(ch: string): boolean {
  return /[\w$]/.test(ch);
}

/**
 * Advance past whitespace and comments starting at `i`, returning the index of
 * the next significant character (or `source.length`).
 */
function skipTrivia(source: string, i: number): number {
  for (;;) {
    while (i < source.length && /\s/.test(source[i])) i += 1;
    if (source.startsWith('//', i)) {
      const nl = source.indexOf('\n', i);
      i = nl === -1 ? source.length : nl + 1;
      continue;
    }
    if (source.startsWith('/*', i)) {
      const end = source.indexOf('*/', i + 2);
      i = end === -1 ? source.length : end + 2;
      continue;
    }
    return i;
  }
}

/**
 * Advance past the string or template literal whose opening quote is at `i`,
 * returning the index just after its closing quote.
 */
function skipString(source: string, i: number): number {
  const quote = source[i];
  i += 1;
  while (i < source.length) {
    const ch = source[i];
    if (ch === '\\') {
      i += 2;
      continue;
    }
    if (ch === quote) return i + 1;
    i += 1;
  }
  return i;
}

/**
 * Extract the depth-1 keys of the `vi.mock('../bridge', () => ({ ... }))`
 * factory object literal in `source`, in source order, duplicates preserved.
 *
 * Deliberately a brace-depth walk with string/template/comment state tracking,
 * NOT an indentation regex (`/^\s{2}(\w+):/`). The indentation form happens to
 * work on both current targets, but it silently UNDER-reports the moment anyone
 * reformats — and under-reporting is the dangerous direction, because it makes
 * the guard vacuously pass. It also over-reports the inner keys of a nested
 * object-literal value, of which both factories have many
 * (`mockResolvedValue({ meshes: [], ... })`).
 *
 * Returns `[]` when no `../bridge` factory is present, so a mis-targeted path
 * trips the consumer's non-vacuity check rather than raising an opaque parser
 * error. Only the object literal that follows the marker is read, so
 * neighbouring `vi.mock` calls in the same file cannot bleed in.
 */
export function extractBridgeFactoryKeys(source: string): string[] {
  let markerAt = -1;
  for (const marker of BRIDGE_MOCK_MARKERS) {
    const at = source.indexOf(marker);
    if (at !== -1 && (markerAt === -1 || at < markerAt)) markerAt = at;
  }
  if (markerAt === -1) return [];

  // Find the `{` that opens the factory's returned object literal. Everything
  // between the marker and it is `'../bridge', () => (` plus trivia.
  let i = markerAt + `vi.mock(`.length;
  let open = -1;
  while (i < source.length) {
    const ch = source[i];
    if (ch === '"' || ch === "'" || ch === '`') {
      i = skipString(source, i);
      continue;
    }
    if (ch === '/' && (source.startsWith('//', i) || source.startsWith('/*', i))) {
      i = skipTrivia(source, i);
      continue;
    }
    if (ch === '{') {
      open = i;
      break;
    }
    i += 1;
  }
  if (open === -1) return [];

  const keys: string[] = [];
  let depth = 0; // nesting relative to the inside of the factory object
  let expectKey = true; // at an entry position: just after `{` or a depth-0 `,`
  i = open + 1;

  while (i < source.length) {
    const ch = source[i];

    if (ch === '"' || ch === "'" || ch === '`') {
      // A quoted key is only a key at an entry position AND when followed by `:`.
      const after = skipString(source, i);
      if (expectKey && depth === 0) {
        const next = skipTrivia(source, after);
        if (source[next] === ':') {
          keys.push(source.slice(i + 1, after - 1));
          i = next + 1;
          expectKey = false;
          continue;
        }
        expectKey = false;
      }
      i = after;
      continue;
    }

    if (ch === '/' && (source.startsWith('//', i) || source.startsWith('/*', i))) {
      i = skipTrivia(source, i);
      continue;
    }

    if (ch === '{' || ch === '[' || ch === '(') {
      depth += 1;
      expectKey = false;
      i += 1;
      continue;
    }

    if (ch === '}' || ch === ']' || ch === ')') {
      if (ch === '}' && depth === 0) return keys; // factory object closed
      depth -= 1;
      i += 1;
      continue;
    }

    if (ch === ',' && depth === 0) {
      expectKey = true;
      i += 1;
      continue;
    }

    if (/\s/.test(ch)) {
      i += 1;
      continue;
    }

    if (expectKey && depth === 0 && isIdentStart(ch)) {
      let end = i + 1;
      while (end < source.length && isIdentPart(source[end])) end += 1;
      const next = skipTrivia(source, end);
      if (source[next] === ':') {
        keys.push(source.slice(i, end));
        i = next + 1;
        expectKey = false;
        continue;
      }
      // Not a key (a spread's identifier, a shorthand entry, ...).
      expectKey = false;
      i = end;
      continue;
    }

    expectKey = false;
    i += 1;
  }

  return keys;
}

/**
 * Runtime exports covered by neither the factory nor the allowlist — i.e. the
 * gaps. Sorted, so a failure message reads as a stable list.
 *
 * Factory keys that are not runtime exports are ignored here; catching those is
 * the consumer's extraction-sanity check, which fails with a clearer message.
 */
export function missingFactoryKeys(
  runtimeExports: string[],
  factoryKeys: string[],
  omissions: Record<string, string>,
): string[] {
  const covered = new Set(factoryKeys);
  const allowed = new Set(Object.keys(omissions));
  return [...new Set(runtimeExports.filter((n) => !covered.has(n) && !allowed.has(n)))].sort();
}

/**
 * Allowlist entries that have rotted, either because `bridge.ts` no longer
 * exports the name, or because the factory in fact mocks it. Sorted union of
 * the two conditions, without duplicates.
 *
 * Without this the allowlist decays into a rubber stamp: entries whose stated
 * reason stopped being true keep suppressing checks nobody re-reads.
 */
export function staleOmissions(
  runtimeExports: string[],
  factoryKeys: string[],
  omissions: Record<string, string>,
): string[] {
  const exported = new Set(runtimeExports);
  const covered = new Set(factoryKeys);
  return Object.keys(omissions)
    .filter((n) => !exported.has(n) || covered.has(n))
    .sort();
}
