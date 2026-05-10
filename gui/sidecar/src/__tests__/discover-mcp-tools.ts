/**
 * Discovers the set of MCP tool names registered in the Rust tools source tree.
 *
 * Recursively scans every `*.rs` file under `toolsDir` (including nested
 * subdirectories) using two targeted patterns that
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
 * `// renamed from "reify_old_name"` or log/error strings. Line and block
 * comments are stripped before `REGISTER_IDENT_RE` is applied, so a
 * commented-out `// registry.register(NAME, ...)` line does not re-admit a
 * stale const.
 *
 * Per-file constraint — gating is per-file. The `REGISTER_IDENT_RE` pre-pass
 * only looks at `registry.register(IDENT, ...)` calls within the same `.rs`
 * file as the matching `CONST_DECL_RE`. A const declared in `mod.rs`/`consts.rs`
 * and registered via `registry.register(consts::NAME, ...)` from a sibling
 * `*.rs` file is silently dropped. The current Rust source tree does not split
 * const declarations across files, so this is a theoretical limitation today,
 * but the floor assertion (`>= 16`) would not catch a single missing tool.
 * Contract for future contributors: keep the const declaration and its
 * `registry.register(NAME, ...)` call in the same `.rs` file. See the
 * 'silently drops a const split across files (known per-file constraint)' test
 * in `discover-mcp-tools.test.ts` for the regression pin. Future-hardening
 * option: do a project-wide `REGISTER_IDENT_RE` pre-pass first, then filter
 * `CONST_DECL_RE` matches against the global set.
 *
 * Comment-strip caveat — the two regex passes applied before `REGISTER_IDENT_RE`
 * (`/\/\/.*$/gm` for line comments and `/\/\*[\s\S]*?\*\//g` for block comments)
 * have two known limitations:
 *   1. They do not respect Rust string literals: a value like
 *      `"https://example.com"` contains `//` which the line-comment regex treats
 *      as a comment start, silently truncating the rest of that line.  A
 *      `registry.register(NAME, ...)` call on the same line as such a literal
 *      could be incorrectly dropped from `registeredIdents`.
 *   2. They do not handle nested block comments: `\/* outer \/* inner *\/ still outer *\/`
 *      is closed at the first `*\/`, leaving ` still outer *\/` in the stripped
 *      source as unexpected plain text.
 * Neither condition arises in the current source tree, so discovery is unaffected
 * today.  Future contributors: avoid placing `registry.register(NAME, ...)` calls
 * on lines that also contain a `//`-bearing string literal, and avoid nested block
 * comments in tools files.  Optional hardening: replace the two `.replace()` calls
 * with a small state machine that honours `"..."`, `r"..."`, `r#"..."#`, line
 * comments, and nested block comments.
 *
 * Uppercase tool names are intentionally supported by `[A-Za-z0-9_]+` in both
 * patterns (the casing policy is enforced by the Rust layer; the TS discovery
 * layer stays casing-agnostic so it stays valid if the policy is ever relaxed).
 *
 * Canonical contract: `crates/reify-mcp/tests/tools_tests.rs::EXPECTED_TOOLS`
 * That file pins the exact tool count and names.
 */

import { readFileSync, readdirSync } from 'node:fs';
import type { Dirent } from 'node:fs';
import { join } from 'node:path';

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
  let entries: Dirent[];
  try {
    entries = readdirSync(toolsDir, { recursive: true, withFileTypes: true });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    throw new Error(
      `Cannot read MCP tools directory at ${toolsDir}: ${msg}. Update TOOLS_DIR if the workspace was reorganized.`,
    );
  }

  const tools = new Set<string>();
  for (const entry of entries.filter(e => e.isFile() && e.name.endsWith('.rs'))) {
    const src = readFileSync(join(entry.parentPath, entry.name), 'utf8');
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
      // Strip line and block comments before scanning for identifier-form
      // register calls so that `// registry.register(NAME, ...)` lines in
      // comments do not re-admit stale consts via registeredIdents.
      const srcNoComments = src
        .replace(/\/\/.*$/gm, '')
        .replace(/\/\*[\s\S]*?\*\//g, '');
      for (const m of srcNoComments.matchAll(REGISTER_IDENT_RE)) {
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
