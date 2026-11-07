/**
 * Pure helpers for the bridge-mock coverage contract. The invariant they exist
 * to enforce, and why a gap in it fails silently, are documented once in the
 * consumer — `bridgeMockCoverage.test.ts`. Not restated here.
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
 * Vitest methods that install a PERSISTENT implementation on a mock — one that
 * outlives the test that set it, because `vi.clearAllMocks()` (what both
 * targets' `beforeEach` calls) resets call history only, not implementations.
 *
 * The `*Once` variants are deliberately absent: they consume themselves, so
 * they cannot leak. `mockClear` / `mockReset` likewise install nothing.
 */
const PERSISTENT_SETTERS = new Set([
  'mockImplementation',
  'mockResolvedValue',
  'mockRejectedValue',
  'mockReturnValue',
]);

/**
 * Matches both override spellings the targets use:
 *   `vi.mocked(bridge.NAME).mockX(`  and  `vi.mocked((bridge as any).NAME).mockX(`
 */
const MOCK_SETTER_RE =
  /vi\.mocked\(\s*\(?\s*bridge(?:\s+as\s+[A-Za-z_$][\w$]*)?\s*\)?\s*\.\s*([A-Za-z_$][\w$]*)\s*\)\s*\.\s*([A-Za-z_$][\w$]*)/g;

export interface BridgeMockOverrides {
  /** Names given a persistent implementation inside some `beforeEach` block. */
  restored: string[];
  /** Names given a persistent implementation outside every `beforeEach` block. */
  overridden: string[];
}

/** Spans of every `beforeEach(...)` callback body in `source`, as [open, close]. */
function beforeEachSpans(source: string): Array<[number, number]> {
  const spans: Array<[number, number]> = [];
  for (let at = source.indexOf('beforeEach('); at !== -1; at = source.indexOf('beforeEach(', at + 1)) {
    let i = source.indexOf('{', at);
    if (i === -1) break;
    let depth = 0;
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
      if (ch === '{') depth += 1;
      else if (ch === '}') {
        depth -= 1;
        if (depth === 0) break;
      }
      i += 1;
    }
    spans.push([source.indexOf('{', at), i]);
  }
  return spans;
}

/**
 * Split a target file's persistent bridge-mock implementation assignments into
 * those a `beforeEach` re-establishes and those only a test body installs.
 *
 * This covers the truth table's third row, which the factory-key checks cannot
 * see: a key present in the factory but absent from `beforeEach` is green, yet
 * any test that overrides it leaks that override into every later test.
 *
 * DELIBERATELY FILE-LEVEL, not describe-scoped: a restore in a nested
 * `describe`'s `beforeEach` only protects tests in that describe, but is
 * counted here as restoring the name file-wide. That makes the derived check
 * UNDER-report (a missed leak stays silent) rather than false-alarm, which is
 * the right direction for a guard nobody should have to argue with. Both sets
 * come from the same regex, so a parse that breaks empties BOTH — assert a
 * floor on `restored` to keep the consumer's check non-vacuous.
 */
export function extractBridgeMockOverrides(source: string): BridgeMockOverrides {
  const spans = beforeEachSpans(source);
  const restored = new Set<string>();
  const overridden = new Set<string>();

  MOCK_SETTER_RE.lastIndex = 0;
  for (let m = MOCK_SETTER_RE.exec(source); m !== null; m = MOCK_SETTER_RE.exec(source)) {
    const [, name, setter] = m;
    if (!PERSISTENT_SETTERS.has(setter)) continue;
    const inBeforeEach = spans.some(([open, close]) => m!.index > open && m!.index < close);
    (inBeforeEach ? restored : overridden).add(name);
  }

  return { restored: [...restored].sort(), overridden: [...overridden].sort() };
}

/**
 * Names some test persistently overrides that no `beforeEach` restores — the
 * leak surface. Sorted.
 */
export function unrestoredOverrides({ restored, overridden }: BridgeMockOverrides): string[] {
  const safe = new Set(restored);
  return overridden.filter((n) => !safe.has(n)).sort();
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
