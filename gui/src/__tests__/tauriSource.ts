/**
 * Reading a `gui/src-tauri/src/*.rs` file from a frontend test.
 *
 * Cross-language guards need the producer's real bytes: `toolDefNames.ts`'s
 * `readDebugServerSource` (ToolDef names) and `constraintVerdictTokens.ts`'s
 * `readEngineSource` (constraint-verdict tokens). One read, one path
 * computation — toolDefNames.ts's own header records the incident that makes
 * that worth centralising: two copies of a source read had drifted into two
 * different path computations.
 *
 * The `fileURLToPath` + `path.dirname` + `path.resolve` idiom is the one proven
 * in gui/test/visual/paths.ts, which documents why naive URL-relative `..` math
 * over-shoots. Segment count here:
 *
 *   <gui>/src/__tests__/tauriSource.ts
 *              ^^^^^^^^   (dirname = __tests__/)
 *         ^^^               (..     = src/)
 *   ^^^^^                   (..     = gui/)
 *
 * Vitest-free by construction, like its siblings here: a plain read, with every
 * `expect` in the importing `.test.ts` files. No `.test.` segment, so vitest's
 * default include does not collect it as a suite.
 *
 * `readDebugServerSource` still carries its own copy of the idiom — toolDefNames.ts
 * is outside task 6723's locked scope, and repointing it here is filed as
 * follow-up work.
 */
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Read `gui/src-tauri/src/<fileName>` as UTF-8. */
export function readTauriSrc(fileName: string): string {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(path.resolve(dir, '..', '..', 'src-tauri', 'src', fileName), 'utf-8');
}
