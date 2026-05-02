import { describe, it, expect, beforeAll } from 'vitest';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { SYSTEM_PROMPT, buildSystemPrompt } from '../system-prompt.js';
import { discoverRegisteredTools } from './discover-mcp-tools.js';

describe('SYSTEM_PROMPT', () => {
  it('is under 3000 tokens (estimated as chars/4 < 12000)', () => {
    expect(SYSTEM_PROMPT.length).toBeLessThan(12000);
  });

  it('contains key language keywords', () => {
    const keywords = ['structure', 'param', 'let', 'auto', 'constraint'];
    for (const keyword of keywords) {
      expect(SYSTEM_PROMPT).toContain(keyword);
    }
  });

  it('mentions MCP tools', () => {
    const tools = ['reify_get_source', 'reify_get_diagnostics', 'reify_language_reference'];
    for (const tool of tools) {
      expect(SYSTEM_PROMPT).toContain(tool);
    }
  });

  it('mentions the Reify GUI context', () => {
    expect(SYSTEM_PROMPT.toLowerCase()).toContain('reify');
  });
});

describe('buildSystemPrompt', () => {
  it('returns the prompt string', () => {
    const prompt = buildSystemPrompt();
    expect(typeof prompt).toBe('string');
    expect(prompt.length).toBeGreaterThan(0);
    expect(prompt).toContain('structure');
  });

  it('includes working directory when provided', () => {
    const prompt = buildSystemPrompt({ workingDirectory: '/home/user/project' });
    expect(prompt).toContain('/home/user/project');
  });
});

describe('SYSTEM_PROMPT MCP tool registry alignment', () => {
  // Discovers registered tool names from Rust source files at test runtime using
  // discoverRegisteredTools(), which scans "reify_*" string literals across all *.rs
  // files in TOOLS_DIR.  This covers:
  //   - inline call site:    registry.register("reify_get_source", ...)
  //   - const indirection:   const NAME: &str = "reify_qux"; ... registry.register(NAME, ...)
  //   - any casing:          "reify_GetSource" is captured by [A-Za-z0-9_]+
  //
  // Canonical Rust contract: crates/reify-mcp/tests/tools_tests.rs::EXPECTED_TOOLS
  // That file pins the exact tool count and names; the floor assertion below (>= 16)
  // is derived from its current count.  Update the floor when EXPECTED_TOOLS grows.
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const TOOLS_DIR = resolve(__dirname, '../../../../crates/reify-mcp/src/tools');

  let registeredTools: Set<string>;

  beforeAll(() => {
    // Wrapped in beforeAll so that a missing/moved TOOLS_DIR surfaces as a focused
    // test failure (with the resolved path in the message) rather than a collection
    // crash that obscures which assertion actually failed.
    try {
      registeredTools = discoverRegisteredTools(TOOLS_DIR);
    } catch (err) {
      throw new Error(
        `Failed to discover MCP tools at ${TOOLS_DIR}: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  });

  it('parses at least one registered tool from the Rust source', () => {
    expect(registeredTools.size).toBeGreaterThan(0);
  });

  it('every reify_* token in SYSTEM_PROMPT resolves to a registered tool', () => {
    // Widened to [A-Za-z0-9_]+ so uppercase tool references in the prompt are also detected.
    const advertised = new Set([...SYSTEM_PROMPT.matchAll(/reify_[A-Za-z0-9_]+/g)].map(m => m[0]));
    const missing = [...advertised].filter(name => !registeredTools.has(name));
    expect(
      missing,
      `SYSTEM_PROMPT advertises tools not in the MCP registry: ${missing.join(', ')}. Registered tools: ${[...registeredTools].sort().join(', ')}`,
    ).toEqual([]);
  });

  it('discovers at least 16 registered tools (matches EXPECTED_TOOLS floor in crates/reify-mcp/tests/tools_tests.rs)', () => {
    // Floor pinned to the canonical EXPECTED_TOOLS count (16) in crates/reify-mcp/tests/tools_tests.rs.
    // Fails loudly if discovery starts under-counting (e.g. TOOLS_DIR drifted, regex broken).
    expect(registeredTools.size).toBeGreaterThanOrEqual(16);
  });
});
