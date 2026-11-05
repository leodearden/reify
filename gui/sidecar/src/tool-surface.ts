/**
 * What the claude child may reach, derived from whether the host's reify-debug
 * server is live (the host sets REIFY_DEBUG_MCP_URL only then).
 */

export const REIFY_DEBUG_MCP_URL_ENV = 'REIFY_DEBUG_MCP_URL';

/** The reify-debug MCP server's tool namespace. */
export const REIFY_DEBUG_TOOL_PREFIX = 'mcp__reify-debug__';

const BASE_TOOLS = 'Read Edit Write Bash Glob Grep';

/**
 * The live reify-debug endpoint the host passed, verbatim (its Rust
 * debug_endpoint_url is the only URL minter); undefined when absent or empty.
 */
export function resolveReifyDebugMcpUrl(
  env: Readonly<Record<string, string | undefined>>,
): string | undefined {
  return env[REIFY_DEBUG_MCP_URL_ENV] || undefined;
}

/** The --allowed-tools grant: base FS/shell tools, plus reify-debug only when live. */
export function allowedTools(reifyDebugMcpUrl: string | undefined): string {
  return reifyDebugMcpUrl === undefined ? BASE_TOOLS : `${BASE_TOOLS} ${REIFY_DEBUG_TOOL_PREFIX}*`;
}

export interface McpHttpServer {
  readonly type: 'http';
  readonly url: string;
}

/** The --mcp-config server map: each entry present only when its URL is. */
export function sidecarMcpServers(options: {
  reifyDebugMcpUrl?: string;
  permissionUrl?: string;
}): Record<string, McpHttpServer> {
  const servers: Record<string, McpHttpServer> = {};
  if (options.reifyDebugMcpUrl !== undefined) {
    servers['reify-debug'] = { type: 'http', url: options.reifyDebugMcpUrl };
  }
  if (options.permissionUrl !== undefined) {
    servers['reify-permission'] = { type: 'http', url: options.permissionUrl };
  }
  return servers;
}
