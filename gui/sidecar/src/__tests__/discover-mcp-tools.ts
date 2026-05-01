/**
 * Discovers the set of MCP tool names registered in the Rust tools source tree.
 *
 * Scans every `*.rs` file under `toolsDir` for `"reify_*"` string literals using
 * the pattern `/"(reify_[A-Za-z0-9_]+)"/g`.  This intentionally wider scan (vs. a
 * call-site regex) captures both the inline-literal form
 *   `registry.register("reify_get_source", ...)`
 * and the const-indirection form
 *   `const NAME: &str = "reify_qux"; ... registry.register(NAME, ...)`
 * as well as tool names with uppercase characters.
 *
 * Canonical contract: `crates/reify-mcp/tests/tools_tests.rs::EXPECTED_TOOLS`
 * That file pins the exact tool count and names; the floor assertion in
 * system-prompt.test.ts (`>= 16`) is derived from it.
 */

import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

/** Pattern matching any `"reify_*"` string literal in Rust source. */
const LITERAL_RE = /"(reify_[A-Za-z0-9_]+)"/g;

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
    for (const m of src.matchAll(LITERAL_RE)) {
      tools.add(m[1]);
    }
  }
  return tools;
}
