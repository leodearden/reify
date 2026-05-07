/**
 * Repository-root resolution helpers for the visual regression harness.
 *
 * Why this module exists:
 *   run.ts originally used `new URL("../../../..", import.meta.url).pathname`
 *   (4 segments) to compute REPO_ROOT. However, URL relative resolution first
 *   strips the filename from the base URL's path (leaving the directory), then
 *   applies each `..` segment to that directory. From gui/test/visual/, going
 *   up 4 levels over-shoots by one, landing at the *parent* of the repo root
 *   (e.g. /home/leo/src/ instead of /home/leo/src/reify/).
 *
 *   The correct approach uses `fileURLToPath` + `path.dirname` to get the
 *   containing directory explicitly, then resolves 3 levels up ("visual →
 *   test → gui → reify").
 */

import { fileURLToPath } from "node:url";
import * as path from "node:path";
import * as fs from "node:fs";

/**
 * Given the module URL of a file in gui/test/visual/<name>.ts, walk 3 parent
 * directories to arrive at the reify repository root.
 *
 * Layout:
 *   <repoRoot>/gui/test/visual/<file>.ts
 *                     ^^^^   (dirname = visual/)
 *              ^^^^          (..      = test/)
 *         ^^^^               (..      = gui/)
 *    ^^^^                    (..      = repoRoot)
 */
export function resolveRepoRoot(moduleUrl: string): string {
  const filePath = fileURLToPath(moduleUrl);
  const dir = path.dirname(filePath);
  return path.resolve(dir, "..", "..", "..");
}

/**
 * Throw a self-explanatory error if `repoRoot` does not look like the reify
 * repository root (i.e. `<repoRoot>/gui/package.json` is absent).
 *
 * This provides an early-failure smoke check so a future regression in the
 * REPO_ROOT path math surfaces as a clear message rather than a cryptic
 * "fixture not found" error downstream.
 */
export function assertRepoRootStructure(repoRoot: string): void {
  const guiPkg = path.join(repoRoot, "gui", "package.json");
  if (!fs.existsSync(guiPkg)) {
    throw new Error(
      `Resolved REPO_ROOT does not look like the reify repo: missing ${repoRoot}/gui/package.json`,
    );
  }
}
