/**
 * Normalizes a file path or file:// URI to a bare path for comparison.
 * Strips the "file://" scheme prefix so that paths and URIs can be compared
 * with a simple equality check.
 *
 * @example
 * normalizePath('file:///project/src/foo.ri') // → '/project/src/foo.ri'
 * normalizePath('/project/src/foo.ri')        // → '/project/src/foo.ri'
 */
export function normalizePath(p: string): string {
  if (p.startsWith('file://')) {
    return p.slice('file://'.length);
  }
  return p;
}

/**
 * Returns true if two file identifiers refer to the same file, normalizing
 * `file://` URI scheme vs bare path differences before comparison.
 *
 * The backend can emit paths as either bare paths (e.g. `/project/src/foo.ri`)
 * or `file://` URIs (e.g. `file:///project/src/foo.ri`). This utility strips
 * the scheme from both arguments before doing an exact equality check, so
 * cross-format comparisons work correctly.
 *
 * @example
 * isSameFile('/project/src/foo.ri', 'file:///project/src/foo.ri') // → true
 * isSameFile('/a/foo.ri', '/b/a/foo.ri')                          // → false
 */
export function isSameFile(a: string, b: string): boolean {
  if (a === b) return true; // fast path — identical strings always match
  return normalizePath(a) === normalizePath(b);
}
