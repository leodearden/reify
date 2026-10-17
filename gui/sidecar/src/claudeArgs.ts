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
    // Always bypass per-call permission prompts using an explicit allowlist, so the GUI
    // never stalls silently on a missing permission UI (task #3206). The allowlist is
    // strictly tighter than --dangerously-skip-permissions (which allows all tools).
    //
    // Precedence: bypassPermissions auto-approves every allowed call, so the
    // reify-permission prompt tool is NOT consulted in this mode. The flags act on
    // different axes: --allowed-tools rejects any tool outside the list outright;
    // bypassPermissions means an allowed call needs no confirmation. The permission MCP
    // would only become active under a future non-bypass permission mode.
    '--permission-mode', 'bypassPermissions',
    '--allowed-tools', allowedTools(config.reifyDebugMcpUrl),
  ];
}
