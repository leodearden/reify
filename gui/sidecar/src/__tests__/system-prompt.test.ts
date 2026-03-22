import { describe, it, expect } from 'vitest';
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
