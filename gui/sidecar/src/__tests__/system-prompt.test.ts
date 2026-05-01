import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { SYSTEM_PROMPT, buildSystemPrompt } from '../system-prompt.js';

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
  // Parse registered tool names from Rust source files at test runtime.
  // Scans all *.rs files in crates/reify-mcp/src/tools/ so new files are automatically included.
  // The canonical Rust counterpart is: crates/reify-mcp/tests/tools_tests.rs EXPECTED_TOOLS.
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const TOOLS_DIR = resolve(__dirname, '../../../../crates/reify-mcp/src/tools');
  const REGISTER_RE = /registry\.register\s*\(\s*"(reify_[a-z0-9_]+)"/g;

  const registeredTools = new Set<string>();
  for (const file of readdirSync(TOOLS_DIR).filter(f => f.endsWith('.rs'))) {
    const src = readFileSync(resolve(TOOLS_DIR, file), 'utf8');
    for (const m of src.matchAll(REGISTER_RE)) registeredTools.add(m[1]);
  }

  it('parses at least one registered tool from the Rust source', () => {
    expect(registeredTools.size).toBeGreaterThan(0);
  });

  it('every reify_* token in SYSTEM_PROMPT resolves to a registered tool', () => {
    const advertised = new Set([...SYSTEM_PROMPT.matchAll(/reify_[a-z0-9_]+/g)].map(m => m[0]));
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
