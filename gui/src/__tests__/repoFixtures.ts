import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Repo-relative file access for test files that read committed sources
 * (`.ri` fixtures, the grammar file) rather than inline snippets.
 */

/** Repo root, three levels up from `gui/src/__tests__/`. */
export const REPO_ROOT = join(__dirname, '../../..');

/** Read a committed file by its repo-relative path. */
export function readFixture(relPath: string): string {
  return readFileSync(join(REPO_ROOT, relPath), 'utf-8');
}
