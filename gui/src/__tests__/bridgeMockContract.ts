/**
 * Pure helpers for the bridge-mock coverage guard: which bridge.ts runtime
 * exports the shared App factory neither mocks nor allowlists, which
 * allowlist entries have rotted, and which App-rendering test files do not
 * import the App harness for its side effect. The invariant they enforce, and
 * why a gap in it fails silently, are documented once in the consumer —
 * `bridgeMockCoverage.test.ts`. Not restated here.
 *
 * This file has no `.test.` segment, so vitest does not collect it as a suite
 * (same convention as `debugBridgeTestHelpers.ts`).
 */
import { dirname, relative } from 'node:path';

/**
 * Runtime exports covered by neither the factory nor the allowlist — i.e. the
 * gaps. Sorted, so a failure message reads as a stable list.
 *
 * Factory keys that are not runtime exports are ignored here; catching those is
 * the consumer's sanity check, which fails with a clearer message.
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

/** A test file's path and its source text, read once by the caller. */
export interface TestSource {
  path: string;
  source: string;
}

const DEFAULT_APP_IMPORT = /^import App\b[^;]*from '(\.\.\/)+App'/m;

/** The membership criterion: a default import of App. A named-only import renders nothing. */
export function defaultImportsApp(source: string): boolean {
  return DEFAULT_APP_IMPORT.test(source);
}

function importSpecifier(fromFile: string, module: string): string {
  const specifier = relative(dirname(fromFile), module);
  return specifier.startsWith('.') ? specifier : `./${specifier}`;
}

const sourceLines = (source: string) => source.split('\n').map((line) => line.trimEnd());

/** The bare side-effect import on a line of its own: the one import form a TS transform never drops. */
function sideEffectImports(lines: readonly string[], specifier: string): boolean {
  return lines.includes(`import '${specifier}';`);
}

/** Any import of `specifier`, bare or ending `from '<specifier>';` (named, multi-line, namespace or type-only). Errs toward inclusion. */
function importsModule(lines: readonly string[], specifier: string): boolean {
  return sideEffectImports(lines, specifier) || lines.some((line) => line.endsWith(` from '${specifier}';`));
}

/**
 * Paths of the files that default-import App or import anything from
 * `harnessModule` (an absolute path without extension), yet lack the bare
 * `import '<specifier>';` line. Sorted. Why only the bare form counts is
 * documented in `bridgeMockCoverage.test.ts`.
 */
export function harnessOffenders(files: readonly TestSource[], harnessModule: string): string[] {
  return files
    .filter(({ path, source }) => {
      const specifier = importSpecifier(path, harnessModule);
      const lines = sourceLines(source);
      const isMember = defaultImportsApp(source) || importsModule(lines, specifier);
      return isMember && !sideEffectImports(lines, specifier);
    })
    .map(({ path }) => path)
    .sort();
}
