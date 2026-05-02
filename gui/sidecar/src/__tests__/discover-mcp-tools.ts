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
 *   2. `const NAME` declaration (const-indirection form):
 *        `const NAME: &str = "reify_qux"; ... registry.register(NAME, ...)`
 *      matched by `const\s+\w+\s*:\s*&\s*str\s*=\s*"(reify_[A-Za-z0-9_]+)"`
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
 * Captures the string value so that `registry.register(NAME, ...)` patterns are
 * still discovered even though the literal appears in the declaration, not the call.
 */
const CONST_DECL_RE = /const\s+\w+\s*:\s*&\s*str\s*=\s*"(reify_[A-Za-z0-9_]+)"/g;

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
    for (const m of src.matchAll(CONST_DECL_RE)) {
      tools.add(m[1]);
    }
  }
  return tools;
}
