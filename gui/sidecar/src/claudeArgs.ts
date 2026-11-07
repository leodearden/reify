import { allowedTools } from './tool-surface.js';

/** The part of the session config that the claude CLI argv depends on. */
export interface ClaudeArgsConfig {
  readonly model: string;
  readonly systemPrompt: string;
  /** Present only when the sidecar's reify-permission MCP server is running. */
  readonly permissionMcp?: { readonly url: string };
  /** Present only when the host's reify-debug MCP server is live. */
  readonly reifyDebugMcpUrl?: string;
}

const PERMISSION_PROMPT_TOOL = 'mcp__reify-permission__approve_tool';

/**
 * The claude CLI argv for one invocation: print mode with stream-json on both
 * stdin and stdout (partial messages included), resuming `sessionId` when set and
 * loading MCP servers from `mcpConfigPath` alone.
 */
export function buildClaudeArgs(
  config: ClaudeArgsConfig,
  sessionId: string | null,
  mcpConfigPath: string,
): string[] {
  return [
    '--print',
    '--verbose',
    '--output-format', 'stream-json',
    '--include-partial-messages',
    '--input-format', 'stream-json',
    '--model', config.model,
    '--system-prompt', config.systemPrompt,
    ...(sessionId !== null ? ['--resume', sessionId] : []),
    // --strict-mcp-config: load ONLY this file, never ~/.claude.json or a project
    // .mcp.json, which can define a foreign reify-debug server (the reify repo's does).
    '--mcp-config', mcpConfigPath, '--strict-mcp-config',
    ...(config.permissionMcp !== undefined ? ['--permission-prompt-tool', PERMISSION_PROMPT_TOOL] : []),
    // No per-call prompts, so the GUI never stalls on a missing permission UI (#3206):
    // --allowed-tools rejects anything off the list (strictly tighter than
    // --dangerously-skip-permissions), and bypassPermissions approves what is on it
    // without consulting the prompt tool above, which only a non-bypass mode would use.
    '--permission-mode', 'bypassPermissions',
    '--allowed-tools', allowedTools(config.reifyDebugMcpUrl),
  ];
}
