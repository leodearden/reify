import { describe, it, expect } from 'vitest';
import { buildClaudeArgs } from '../claudeArgs.js';
import { allowedTools } from '../tool-surface.js';

const MCP_CONFIG_PATH = '/tmp/x/mcp-config.json';
const MINIMAL = { model: 'm', systemPrompt: 'p' };
const PERMISSION_MCP = { url: 'http://127.0.0.1:1/mcp' };

function valueAfter(args: string[], flag: string): string | undefined {
  const idx = args.indexOf(flag);
  return idx === -1 ? undefined : args[idx + 1];
}

describe('buildClaudeArgs', () => {
  it('builds the exact argv for a minimal config', () => {
    expect(buildClaudeArgs(MINIMAL, null, MCP_CONFIG_PATH)).toEqual([
      '--print',
      '--verbose',
      '--output-format', 'stream-json',
      '--include-partial-messages',
      '--input-format', 'stream-json',
      '--model', 'm',
      '--system-prompt', 'p',
      '--mcp-config', MCP_CONFIG_PATH,
      '--strict-mcp-config',
      '--permission-mode', 'bypassPermissions',
      '--allowed-tools', 'Read Edit Write Bash Glob Grep',
    ]);
  });

  it('resumes the given session right after the system prompt', () => {
    const args = buildClaudeArgs(MINIMAL, 'sess-abc', MCP_CONFIG_PATH);
    const promptIdx = args.indexOf('--system-prompt');
    expect(args.slice(promptIdx + 2, promptIdx + 4)).toEqual(['--resume', 'sess-abc']);
  });

  it('does not resume without a session id', () => {
    expect(buildClaudeArgs(MINIMAL, null, MCP_CONFIG_PATH)).not.toContain('--resume');
  });

  it('wires the permission prompt tool between the MCP config and the permission mode', () => {
    const args = buildClaudeArgs({ ...MINIMAL, permissionMcp: PERMISSION_MCP }, null, MCP_CONFIG_PATH);
    const toolIdx = args.indexOf('--permission-prompt-tool');
    expect(args[toolIdx + 1]).toBe('mcp__reify-permission__approve_tool');
    expect(toolIdx).toBeGreaterThan(args.indexOf('--strict-mcp-config'));
    expect(toolIdx).toBeLessThan(args.indexOf('--permission-mode'));
  });

  it('omits the permission prompt tool without a permission MCP', () => {
    expect(buildClaudeArgs(MINIMAL, null, MCP_CONFIG_PATH)).not.toContain('--permission-prompt-tool');
  });

  it('grants reify-debug tools only when its endpoint is live', () => {
    const url = 'http://127.0.0.1:4500/mcp';
    const live = buildClaudeArgs({ ...MINIMAL, reifyDebugMcpUrl: url }, null, MCP_CONFIG_PATH);
    expect(valueAfter(live, '--allowed-tools')).toBe(allowedTools(url));
    expect(valueAfter(live, '--allowed-tools')).toMatch(/mcp__reify-debug__\*$/);

    const dark = buildClaudeArgs(MINIMAL, null, MCP_CONFIG_PATH);
    expect(valueAfter(dark, '--allowed-tools')).toBe(allowedTools(undefined));
  });

  it('never skips permissions wholesale', () => {
    for (const config of [MINIMAL, { ...MINIMAL, permissionMcp: PERMISSION_MCP }]) {
      expect(buildClaudeArgs(config, null, MCP_CONFIG_PATH)).not.toContain('--dangerously-skip-permissions');
    }
  });

  it('is pure: accepts a frozen config and returns a fresh, equal array per call', () => {
    const config = Object.freeze({ ...MINIMAL, permissionMcp: Object.freeze({ ...PERMISSION_MCP }) });
    const first = buildClaudeArgs(config, 'sess-1', MCP_CONFIG_PATH);
    const second = buildClaudeArgs(config, 'sess-1', MCP_CONFIG_PATH);
    expect(second).toEqual(first);
    expect(second).not.toBe(first);

    first.push('--mutated');
    expect(buildClaudeArgs(config, 'sess-1', MCP_CONFIG_PATH)).toEqual(second);
  });
});
