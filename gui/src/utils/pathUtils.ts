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
    const stripped = p.slice('file://'.length);
    try {
      return decodeURIComponent(stripped);
    } catch {
      return stripped;
    }
  }
  return p;
}

/**
 * Returns a canonical form of `path` suitable for use as a document-identity key
 * in the editor store.
 *
 * Steps applied in order:
 * 1. Strips `file://` prefix and decodes percent-encoding via {@link normalizePath}.
 * 2. Collapses repeated `/` separators.
 * 3. Resolves `.` (current-dir) and `..` (parent-dir) segments.
 * 4. Removes a trailing `/` on non-root paths.
 *
 * **Limitation**: pure-TS code running in the webview cannot perform a true
 * `realpath(3)` syscall, so symlinks and relative-to-absolute resolution are
 * NOT handled here.  The Rust backend canonicalises those cases before emitting
 * any path over IPC; this function provides defence-in-depth for residual `.`/
 * `..` segments and `file://`-scheme variations that the backend may not strip.
 *
 * Relative paths (no leading `/`) are returned unchanged — the function cannot
 * know the process CWD from inside the browser sandbox.
 *
 * @example
 * canonicalizeKey('file:///a/./b/foo.ri')    // → '/a/b/foo.ri'
 * canonicalizeKey('/a/b/../foo.ri')            // → '/a/foo.ri'
 * canonicalizeKey('/a//b///foo.ri')            // → '/a/b/foo.ri'
 * canonicalizeKey('relative/foo.ri')           // → 'relative/foo.ri'
 */
export function canonicalizeKey(path: string): string {
  // Step 1: strip file:// and decode percent-encoding
  let p = normalizePath(path);

  // Only apply segment resolution to absolute paths (starts with '/')
  if (!p.startsWith('/')) {
    return p;
  }

  // Step 2–3: split on '/', collapse repeated slashes, resolve . and ..
  const segments: string[] = [];
  for (const seg of p.split('/')) {
    if (seg === '' || seg === '.') {
      // empty segment (from leading/repeated '/') or current-dir: skip
      continue;
    }
    if (seg === '..') {
      // parent-dir: pop last segment (don't go above root)
      if (segments.length > 0) {
        segments.pop();
      }
    } else {
      segments.push(seg);
    }
  }

  // Step 4: rebuild — always absolute, strip trailing slash (root stays '/')
  const result = '/' + segments.join('/');
  return result;
}

/**
 * Convert a bare file-system path to a `file://` URI for use with the LSP server.
 *
 * - An already-`file://`-prefixed string is returned unchanged.
 * - An absolute path (`/a/b.ri`) becomes `file:///a/b.ri`.
 * - A relative path (no leading `/`) gets a slash inserted: `b.ri` → `file:///b.ri`.
 *
 * This is extracted from the private `pathToUri` closure in Editor.tsx so that
 * `bridge.ts` LSP probe handlers can derive the byte-identical URI that the editor
 * registered with the LSP via `lspClient.didOpen`.
 *
 * @example
 * pathToUri('/a/b.ri')          // → 'file:///a/b.ri'
 * pathToUri('file:///a/b.ri')   // → 'file:///a/b.ri' (unchanged)
 * pathToUri('b.ri')             // → 'file:///b.ri'
 */
export function pathToUri(path: string): string {
  if (path.startsWith('file://')) return path;
  return `file://${path.startsWith('/') ? '' : '/'}${path}`;
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
