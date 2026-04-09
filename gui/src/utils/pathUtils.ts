/**
 * Normalizes a file path or file:// URI to a bare decoded OS path.
 * Strips the "file://" scheme prefix and applies selective percent-decoding:
 * all percent-sequences are decoded to produce a valid OS path (e.g. %20 → space,
 * %E4%BD%A0 → 你), EXCEPT %2F (preserves path structure — decoding to '/' would
 * introduce spurious path separators) and %00 (null byte safety).
 *
 * This matches the downstream contract at Editor.tsx:81-83 where the result is
 * passed to bridgeOpenFile → Tauri open_file → Rust std::fs::read_to_string,
 * which expects a decoded OS path.
 *
 * @example
 * normalizePath('file:///project/src/foo.ri')           // → '/project/src/foo.ri'
 * normalizePath('/project/src/foo.ri')                  // → '/project/src/foo.ri'
 * normalizePath('file:///project/src/hello%20world.ri') // → '/project/src/hello world.ri'
 * normalizePath('file:///path/foo%2Fbar.ri')            // → '/path/foo%2Fbar.ri'
 */
export function normalizePath(p: string): string {
  if (p.startsWith('file://')) {
    try {
      const pathname = new URL(p).pathname;
      // Selectively decode percent-sequences to produce a valid OS path.
      // Groups consecutive allowed sequences so that multi-byte UTF-8 sequences
      // (e.g. %E4%BD%A0 → 你) are decoded as a unit. Excludes %2F (path separator —
      // would corrupt path structure) and %00 (null byte — security concern).
      return pathname.replace(
        /((?:%(?!2[Ff]|00)[0-9A-Fa-f]{2})+)/g,
        m => decodeURIComponent(m)
      );
    } catch {
      return p.slice('file://'.length);
    }
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
