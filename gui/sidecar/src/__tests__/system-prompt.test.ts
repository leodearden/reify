import { describe, it, expect } from 'vitest';
import { buildSystemPrompt } from '../system-prompt.js';

// Grantability of every tool the prompt names, over both variants, is guarded
// by the gate-run gui/src/__tests__/sidecarReifyDebugGating.test.ts.

const LIVE = buildSystemPrompt({ reifyDebugLive: true });
const NOT_LIVE = buildSystemPrompt({ reifyDebugLive: false });

describe('buildSystemPrompt', () => {
  it('is under 3000 tokens (estimated as chars/4 < 12000) even with the reify-debug section', () => {
    expect(LIVE.length).toBeLessThan(12000);
  });

  it('contains key language keywords in both variants', () => {
    const keywords = ['structure', 'param', 'let', 'auto', 'constraint'];
    for (const prompt of [LIVE, NOT_LIVE]) {
      for (const keyword of keywords) {
        expect(prompt).toContain(keyword);
      }
    }
  });

  it('describes the reify-debug tools only when the server is live', () => {
    expect(LIVE).toContain('mcp__reify-debug__');
    expect(LIVE).toContain('get_diagnostics');
    expect(NOT_LIVE).not.toContain('mcp__reify-debug__');
    expect(NOT_LIVE).not.toContain('get_diagnostics');
  });

  it('always names the file tools', () => {
    for (const prompt of [LIVE, NOT_LIVE]) {
      expect(prompt).toMatch(/\bWrite\b/);
      expect(prompt).toMatch(/\bEdit\b/);
    }
  });

  it('mentions the Reify GUI context', () => {
    expect(NOT_LIVE.toLowerCase()).toContain('reify');
  });

  it('includes working directory when provided', () => {
    const prompt = buildSystemPrompt({ reifyDebugLive: false, workingDirectory: '/home/user/project' });
    expect(prompt).toContain('/home/user/project');
  });

  it('names no bare reify_* tool (only the mcp__reify-debug__ namespaced form is callable)', () => {
    for (const prompt of [LIVE, NOT_LIVE]) {
      expect(prompt.match(/\breify_[A-Za-z0-9_]+/g)).toBeNull();
    }
  });
});
