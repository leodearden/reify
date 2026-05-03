/**
 * Discovers the set of MCP tool names registered in the Rust tools source tree.
 *
 * Scans every `*.rs` file under `toolsDir` using two targeted patterns that
 * together cover both registration forms without admitting comment/error-string
 * false positives:
 *
 *   1. Inline call-site literal:
 *        `registry.register("reify_get_source", ...)`
 *      matched by `registry\.register\s*\(\s*"(reify_[A-Za-z0-9_]+)"`
 *
 *   2. `const NAME` declaration (const-indirection form), GATED on usage:
 *        `const NAME: &str = "reify_qux"; ... registry.register(NAME, ...)`
 *      The const declaration is matched by
 *        `const\s+(\w+)\s*:\s*&\s*str\s*=\s*"(reify_[A-Za-z0-9_]+)"`
 *      but the value is only collected when the captured NAME also appears as
 *      the first (identifier) argument to a `registry.register(NAME, ...)` call
 *      in the same file — detected by `REGISTER_IDENT_RE`. This ensures that
 *      stale, deprecated, or test-only const declarations do not leak into the
 *      discovered set without an active registration call backing them.
 *
 * Using narrower call-site and declaration patterns (rather than every string
 * literal in the file) avoids false positives from comments such as
 * `// renamed from "reify_old_name"` or log/error strings.
 *
 * Uppercase tool names are intentionally supported by `[A-Za-z0-9_]+` in both
 * patterns (the casing policy is enforced by the Rust layer; the TS discovery
 * layer stays casing-agnostic so it stays valid if the policy is ever relaxed).
 *
 * Canonical contract: `crates/reify-mcp/tests/tools_tests.rs::EXPECTED_TOOLS`
 * That file pins the exact tool count and names; the floor assertion in
 * system-prompt.test.ts (`>= 16`) is derived from it.
 */

import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

/**
 * Matches inline `registry.register("reify_*", ...)` call-site literals.
 * Deliberately excludes string literals that appear only in comments or log messages.
 */
const REGISTER_LITERAL_RE = /registry\.register\s*\(\s*"(reify_[A-Za-z0-9_]+)"/g;

/**
 * Matches `const NAME: &str = "reify_*"` declarations (const-indirection form).
 * Group 1 captures the const NAME (identifier); group 2 captures the string value.
 * A value is only added to the discovered set when NAME is also present in
 * `registeredIdents` (i.e. it appears as the first argument of a
 * `registry.register(NAME, ...)` call — see `REGISTER_IDENT_RE` below).
 */
const CONST_DECL_RE = /const\s+(\w+)\s*:\s*&\s*str\s*=\s*"(reify_[A-Za-z0-9_]+)"/g;

/**
 * Matches the identifier form of `registry.register(IDENT, ...)` where the
 * first argument is a Rust identifier rather than a string literal.
 * Rust identifiers start with `[A-Za-z_]`, which naturally excludes string
 * literals (starting with `"`), so no negative lookahead is needed.
 * Captures group 1: the final identifier name.
 *
 * The optional non-capturing prefix `(?:\w+::)*` allows path-qualified forms
 * such as `registry.register(self::NAME, ...)`, `registry.register(crate::NAME, ...)`,
 * or `registry.register(MyMod::NAME, ...)`.  The captured group is always the
 * trailing simple identifier, which is what `CONST_DECL_RE` captures in group 1.
 *
 * Note: This may also capture unrelated identifiers (e.g. helper-function names
 * like `some_func` in `registry.register(some_func(), ...)`); those false-positive
 * identifiers are harmless because they will not match any `CONST_DECL_RE` entry.
 */
const REGISTER_IDENT_RE = /registry\.register\s*\(\s*(?:\w+::)*([A-Za-z_]\w*)\s*,/g;

/**
 * Discover all MCP tool names registered in the Rust tools source tree.
 *
 * @param toolsDir  Absolute path to the directory containing `*.rs` tool
 *                  registration files (e.g. `crates/reify-mcp/src/tools`).
 * @returns         A `Set<string>` of every `reify_*` tool name found.
 * @throws          `Error` with the resolved path in the message when
 *                  `toolsDir` cannot be read (e.g. after a workspace reorg).
 */
export function discoverRegisteredTools(toolsDir: string): Set<string> {
  let entries: string[];
  try {
    entries = readdirSync(toolsDir);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    throw new Error(
      `Cannot read MCP tools directory at ${toolsDir}: ${msg}. Update TOOLS_DIR if the workspace was reorganized.`,
    );
  }

  const tools = new Set<string>();
  for (const file of entries.filter(f => f.endsWith('.rs'))) {
    const src = readFileSync(resolve(toolsDir, file), 'utf8');
    // Collect inline call-site literals: registry.register("reify_*", ...)
    for (const m of src.matchAll(REGISTER_LITERAL_RE)) {
      tools.add(m[1]);
    }
    // Collect const declarations: const NAME: &str = "reify_*"
    // m[1] = const NAME (identifier), m[2] = the reify_* string value.
    // We gather matches first so we can skip the REGISTER_IDENT_RE pre-pass
    // entirely when the file contains no const declarations (the common case).
    const constMatches = [...src.matchAll(CONST_DECL_RE)];
    if (constMatches.length > 0) {
      // Build a set of identifiers used as the first arg in registry.register(IDENT, ...)
      // calls in this file.  Only const declarations whose NAME appears here are added
      // to `tools` — this gates out stale/test-only consts that are never actually wired
      // into the registry.
      const registeredIdents = new Set<string>();
      for (const m of src.matchAll(REGISTER_IDENT_RE)) {
        registeredIdents.add(m[1]);
      }
      // Only include the value when NAME is in registeredIdents (i.e. is actively
      // passed to registry.register in this file).
      for (const m of constMatches) {
        if (registeredIdents.has(m[1])) {
          tools.add(m[2]);
        }
      }
    }
  }
  return tools;
}
