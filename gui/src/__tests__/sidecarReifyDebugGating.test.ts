/**
 * The in-app assistant reaches reify-debug only when the host's own debug server
 * is live (ruling esc-7049-7, task 7816); this lives in the gui suite because
 * the merge gate runs only typecheck for gui/sidecar, never its vitest suite.
 */
import { describe, it, expect } from 'vitest';
import {
  allowedTools,
  resolveReifyDebugMcpUrl,
  sidecarMcpServers,
} from '../../sidecar/src/tool-surface';
import { ADVERTISED_DEBUG_TOOL_NAMES, buildSystemPrompt } from '../../sidecar/src/system-prompt';

const LIVE_URL = 'http://127.0.0.1:4500/mcp';
const PERMISSION_URL = 'http://127.0.0.1:4600/mcp';
const BASE_TOOLS = ['Read', 'Edit', 'Write', 'Bash', 'Glob', 'Grep'];

function tokens(grant: string): string[] {
  return grant.split(/\s+/).filter(Boolean);
}

describe('resolveReifyDebugMcpUrl', () => {
  it('is undefined unless the host passed a non-empty REIFY_DEBUG_MCP_URL', () => {
    expect(resolveReifyDebugMcpUrl({})).toBeUndefined();
    expect(resolveReifyDebugMcpUrl({ REIFY_DEBUG_MCP_URL: '' })).toBeUndefined();
    expect(resolveReifyDebugMcpUrl({ REIFY_DEBUG_MCP_URL: LIVE_URL })).toBe(LIVE_URL);
  });

  it('never infers a live server from the debug flag or port', () => {
    expect(resolveReifyDebugMcpUrl({ REIFY_DEBUG: '1', REIFY_DEBUG_PORT: '4500' })).toBeUndefined();
  });
});

describe('allowedTools', () => {
  it('grants only the base tools when reify-debug is not live', () => {
    const granted = tokens(allowedTools(undefined));
    expect(granted).toStrictEqual(BASE_TOOLS);
    expect(granted.filter((t) => t.startsWith('mcp__'))).toStrictEqual([]);
  });

  it('adds the reify-debug namespace when it is live', () => {
    expect(tokens(allowedTools(LIVE_URL))).toStrictEqual([...BASE_TOOLS, 'mcp__reify-debug__*']);
  });
});

describe('sidecarMcpServers', () => {
  it('registers reify-debug only with a live URL', () => {
    expect(
      sidecarMcpServers({ reifyDebugMcpUrl: undefined, permissionUrl: PERMISSION_URL }),
    ).toStrictEqual({ 'reify-permission': { type: 'http', url: PERMISSION_URL } });

    expect(
      sidecarMcpServers({ reifyDebugMcpUrl: LIVE_URL, permissionUrl: PERMISSION_URL }),
    ).toStrictEqual({
      'reify-debug': { type: 'http', url: LIVE_URL },
      'reify-permission': { type: 'http', url: PERMISSION_URL },
    });

    expect(sidecarMcpServers({})).toStrictEqual({});
  });
});

describe('buildSystemPrompt when reify-debug is not live', () => {
  const prompt = buildSystemPrompt({ reifyDebugLive: false });

  it('advertises no reify-debug surface', () => {
    expect(prompt).not.toContain('mcp__');
    expect(prompt).not.toContain('reify-debug');
    const named = ADVERTISED_DEBUG_TOOL_NAMES.filter((name) =>
      new RegExp(`\\b${name}\\b`).test(prompt),
    );
    expect(named, 'reify-debug tools named by the not-live prompt').toStrictEqual([]);
  });

  it('still briefs the language and the file tools', () => {
    for (const tool of ['Read', 'Write', 'Edit']) {
      expect(prompt).toMatch(new RegExp(`\\b${tool}\\b`));
    }
    for (const keyword of ['structure', 'param', 'constraint']) {
      expect(prompt).toContain(keyword);
    }
  });
});

describe('every tool the prompt names is grantable', () => {
  function isGrantable(tool: string, grant: string): boolean {
    return tokens(grant).some((t) => (t.endsWith('*') ? tool.startsWith(t.slice(0, -1)) : t === tool));
  }

  for (const reifyDebugLive of [true, false]) {
    it(`reifyDebugLive: ${reifyDebugLive}`, () => {
      const prompt = buildSystemPrompt({ reifyDebugLive });
      const grant = allowedTools(reifyDebugLive ? LIVE_URL : undefined);
      const named = new Set([
        ...[...prompt.matchAll(/mcp__[A-Za-z0-9_-]+/g)].map((m) => m[0]),
        ...[...prompt.matchAll(/\breify_[A-Za-z0-9_]+/g)].map((m) => m[0]),
      ]);
      const ungrantable = [...named].filter((t) => !isGrantable(t, grant));
      expect(ungrantable, `prompt names tools the grant "${grant}" never allows`).toStrictEqual([]);
    });
  }
});
