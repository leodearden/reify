import { describe, it, expect } from 'vitest';
import { SYSTEM_PROMPT, buildSystemPrompt } from '../system-prompt.js';
import { ALLOWED_TOOLS } from '../session.js';

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

  it('mentions the real, grantable tool surface', () => {
    expect(SYSTEM_PROMPT).toContain('mcp__reify-debug__');
    expect(SYSTEM_PROMPT).toContain('get_diagnostics');
    expect(SYSTEM_PROMPT).toMatch(/\bWrite\b/);
    expect(SYSTEM_PROMPT).toMatch(/\bEdit\b/);
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

describe('SYSTEM_PROMPT advertises only grantable tools', () => {
  // ALLOWED_TOOLS (session.ts) is the sidecar's true grantable surface: invokeSdk
  // wires only the reify-debug MCP server into --mcp-config and runs the Claude CLI
  // child under --permission-mode bypassPermissions --allowed-tools ALLOWED_TOOLS, so
  // a tool absent from ALLOWED_TOOLS can never actually be invoked regardless of what
  // the prompt prose claims. Parsed into an exact base-tool set plus wildcard prefixes
  // so this test tracks the real allowlist automatically if it ever changes.
  const tokens = ALLOWED_TOOLS.split(/\s+/).filter(Boolean);
  const exact = new Set(tokens.filter(t => !t.endsWith('*')));
  const prefixes = tokens.filter(t => t.endsWith('*')).map(t => t.slice(0, -1));

  function isGrantable(tool: string): boolean {
    return exact.has(tool) || prefixes.some(p => tool.startsWith(p));
  }

  it('every MCP/reify tool named in SYSTEM_PROMPT is grantable', () => {
    // These two lexical shapes capture the drift class this test guards against:
    // bare reify_* fictions and any non-reify-debug MCP reference. Base FS tools
    // (Read/Write/Edit) are grantable by definition and lexically indistinguishable
    // from prose verbs, so they are covered by ALLOWED_TOOLS membership, not extraction.
    const named = new Set([
      ...[...SYSTEM_PROMPT.matchAll(/mcp__[A-Za-z0-9_-]+/g)].map(m => m[0]),
      ...[...SYSTEM_PROMPT.matchAll(/\breify_[A-Za-z0-9_]+/g)].map(m => m[0]),
    ]);
    const ungrantable = [...named].filter(t => !isGrantable(t));
    expect(
      ungrantable,
      `SYSTEM_PROMPT advertises tools the sidecar can never grant: ${ungrantable.join(', ')}. Grantable: ${ALLOWED_TOOLS}`,
    ).toEqual([]);
  });

  it('no reify_* fiction survives', () => {
    expect(SYSTEM_PROMPT.match(/\breify_[A-Za-z0-9_]+/g)).toBeNull();
    expect(SYSTEM_PROMPT).not.toContain('reify_set_parameter');
    expect(SYSTEM_PROMPT).not.toContain('reify_update_source');
  });
});
