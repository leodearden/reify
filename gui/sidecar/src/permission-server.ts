import { createServer } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { randomUUID } from 'node:crypto';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import * as z from 'zod/v4';

export interface PermissionRequestEvent {
  request_id: string;
  tool_name: string;
  tool_input: Record<string, unknown>;
}

export interface PermissionDecisionResult {
  behavior: 'allow' | 'deny';
  message?: string;
  updatedInput?: Record<string, unknown>;
}

export interface PermissionServer {
  /** Start the HTTP listener on a random localhost port. */
  start(): Promise<void>;
  /** Close the HTTP listener. Idempotent. */
  stop(): Promise<void>;
  /** Return the MCP endpoint URL. Throws if start() has not been called. */
  url(): string;
  /**
   * Register a callback to be invoked when a permission request arrives.
   * Replacing the handler replaces the previous one (last-write-wins).
   * Pass `null` to clear the handler — useful when the registering session is
   * destroyed and the same PermissionServer may outlive it (1:1 lifetime is
   * the production norm in index.ts, but this sentinel keeps shared-server
   * reuse safe by construction).
   */
  onRequest(handler: ((req: PermissionRequestEvent) => void) | null): void;
  /**
   * Resolve a pending permission request.
   * No-op for unknown request_ids.
   */
  decide(requestId: string, decision: PermissionDecisionResult): void;
  /**
   * Mark a tool name as always-allowed for the lifetime of this server.
   * Future approve_tool calls for this tool_name will resolve immediately
   * without invoking the onRequest callback.
   * Any currently in-flight approve_tool calls for this tool_name are also
   * resolved immediately with `{ behavior: 'allow' }`.
   */
  setRemembered(toolName: string): void;
  /**
   * Cancel all pending permission requests by resolving them with `{ behavior: 'deny' }`.
   * Call this when clearing or destroying a session so that any suspended MCP HTTP handlers
   * are unblocked and resolver references can be garbage-collected.
   * Idempotent — safe to call when no requests are pending.
   */
  cancelAll(): void;
}

/**
 * Test-only helpers — not part of the production PermissionServer contract.
 * Accessed via the `__testHooks` property on the value returned by createPermissionServer().
 * Production callers typed as PermissionServer cannot reach this property.
 */
export interface PermissionServerTestHooks {
  /**
   * Resolves once pendingPromises.size >= n; rejects after timeoutMs (default 2000).
   * Event-driven: notified at the pendingPromises.set site, no polling loop.
   */
  awaitPending(n: number, timeoutMs?: number): Promise<void>;
}

/**
 * Create an in-process MCP HTTP server that exposes a single `approve_tool`
 * tool for Claude CLI's `--permission-prompt-tool` mechanism.
 *
 * Each HTTP request gets its own McpServer + StreamableHTTPServerTransport
 * pair (stateless pattern), but all share the closure-captured state:
 * `pendingPromises`, `rememberedTools`, and `onRequestHandler`.
 */
export function createPermissionServer(): PermissionServer & { readonly __testHooks: PermissionServerTestHooks } {
  let port: number | null = null;
  let stopped = false;
  let onRequestHandler: ((req: PermissionRequestEvent) => void) | null = null;

  /** request_id → { resolver, toolName } for pending approve_tool calls.
   *  toolName is stored so setRemembered() can retroactively resolve in-flight
   *  requests and cancelAll() can drain all pending entries. */
  const pendingPromises = new Map<string, {
    resolve: (decision: PermissionDecisionResult) => void;
    toolName: string;
  }>();
  /** Tool names that are always approved without prompting */
  const rememberedTools = new Set<string>();

  /** Pending awaitPending callers waiting for pendingPromises.size to reach their threshold. */
  const waiters: Array<{ n: number; resolve: () => void; timer: ReturnType<typeof setTimeout> }> = [];

  function notifyWaiters(): void {
    for (let i = waiters.length - 1; i >= 0; i--) {
      const w = waiters[i];
      if (pendingPromises.size >= w.n) {
        clearTimeout(w.timer);
        waiters.splice(i, 1);
        w.resolve();
      }
    }
  }

  const httpServer = createServer(async (req: IncomingMessage, res: ServerResponse) => {
    // Parse the pathname from req.url so query strings and trailing slashes are
    // tolerated — Claude CLI is the sole client but strict equality is fragile.
    const pathname = req.url ? new URL(req.url, 'http://localhost').pathname : '';
    if (pathname !== '/mcp') {
      res.writeHead(404).end();
      return;
    }

    // Read the full request body before creating per-request server instances.
    let body: Buffer;
    try {
      body = await new Promise<Buffer>((resolve, reject) => {
        const chunks: Buffer[] = [];
        req.on('data', (chunk: Buffer) => chunks.push(chunk));
        req.on('end', () => resolve(Buffer.concat(chunks)));
        req.on('error', reject);
      });
    } catch (err) {
      console.error('[permission-server] Error reading request body:', err);
      if (!res.headersSent) res.writeHead(400).end('Bad request body');
      return;
    }

    let parsedBody: unknown;
    try {
      parsedBody = JSON.parse(body.toString());
    } catch {
      if (!res.headersSent) res.writeHead(400).end('Invalid JSON');
      return;
    }

    // Create a fresh McpServer + Transport per request (stateless pattern).
    // The tool handler captures the outer state via closure.
    const mcpServer = new McpServer({ name: 'reify-permission', version: '1.0.0' });

    mcpServer.tool(
      'approve_tool',
      'Permission prompt handler — invoked by Claude CLI for tool-use approval',
      {
        tool_name: z.string().describe('Name of the tool requesting permission'),
        input: z.record(z.string(), z.unknown()).describe('Tool input arguments'),
      },
      async ({ tool_name, input }: { tool_name: string; input: Record<string, unknown> }) => {
        // Short-circuit for remembered tools — no round-trip to the host.
        if (rememberedTools.has(tool_name)) {
          return {
            content: [{ type: 'text' as const, text: JSON.stringify({ behavior: 'allow' }) }],
          };
        }

        // Generate a unique correlation ID for this permission request.
        const requestId = randomUUID();

        // Await the host's decision, blocking the MCP tool call until decide() fires.
        // Store toolName alongside the resolver so setRemembered() and cancelAll()
        // can operate on in-flight entries without a separate lookup.
        const decision = await new Promise<PermissionDecisionResult>((resolve) => {
          pendingPromises.set(requestId, { resolve, toolName: tool_name });
          notifyWaiters();
          // Notify the host via the registered onRequest handler.
          onRequestHandler?.({ request_id: requestId, tool_name, tool_input: input });
        });

        return {
          content: [{ type: 'text' as const, text: JSON.stringify(decision) }],
        };
      }
    );

    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined });

    try {
      await mcpServer.connect(transport);
      // Register before handleRequest so client aborts mid-await still trigger
      // cleanup; `cleaned` makes the listener idempotent across the abort-then-
      // normal-close sequence.
      let cleaned = false;
      res.on('close', () => {
        if (cleaned) return;
        cleaned = true;
        transport.close();
        mcpServer.close();
      });
      await transport.handleRequest(req, res, parsedBody);
    } catch (err) {
      console.error('[permission-server] Error handling MCP request:', err);
      if (!res.headersSent) {
        res.writeHead(500).end();
      }
    }
  });

  return {
    async start(): Promise<void> {
      return new Promise<void>((resolve, reject) => {
        httpServer.once('error', reject);
        httpServer.listen(0, '127.0.0.1', () => {
          httpServer.removeListener('error', reject);
          const addr = httpServer.address();
          if (!addr || typeof addr === 'string') {
            reject(new Error('Unexpected server address format'));
            return;
          }
          port = addr.port;
          resolve();
        });
      });
    },

    async stop(): Promise<void> {
      if (stopped) return;
      stopped = true;
      return new Promise<void>((resolve, reject) => {
        httpServer.close((err) => {
          if (err) reject(err);
          else resolve();
        });
      });
    },

    url(): string {
      if (port === null) throw new Error('Permission server not started — call start() first');
      return `http://127.0.0.1:${port}/mcp`;
    },

    onRequest(handler: ((req: PermissionRequestEvent) => void) | null): void {
      onRequestHandler = handler;
    },

    decide(requestId: string, decision: PermissionDecisionResult): void {
      const entry = pendingPromises.get(requestId);
      if (entry) {
        pendingPromises.delete(requestId);
        entry.resolve(decision);
      }
      // Unknown request_id: silent no-op as specified.
    },

    setRemembered(toolName: string): void {
      rememberedTools.add(toolName);
      // Retroactively resolve any in-flight requests for this tool so the user
      // is not prompted again for a tool they just elected to always allow.
      for (const [reqId, entry] of pendingPromises) {
        if (entry.toolName === toolName) {
          pendingPromises.delete(reqId);
          entry.resolve({ behavior: 'allow' });
        }
      }
    },

    cancelAll(): void {
      // Resolve all pending entries with deny to unblock suspended HTTP handlers
      // and free resolver references. Safe to call when no entries are pending.
      for (const [reqId, entry] of pendingPromises) {
        pendingPromises.delete(reqId);
        entry.resolve({ behavior: 'deny' });
      }
    },

    __testHooks: {
      awaitPending(n: number, timeoutMs = 2000): Promise<void> {
        if (pendingPromises.size >= n) return Promise.resolve();
        return new Promise<void>((resolve, reject) => {
          const timer = setTimeout(() => {
            const idx = waiters.findIndex((w) => w.resolve === resolve);
            if (idx !== -1) waiters.splice(idx, 1);
            reject(new Error(
              `awaitPending(${n}): timed out after ${timeoutMs}ms (${pendingPromises.size} pending)`
            ));
          }, timeoutMs);
          waiters.push({ n, resolve, timer });
        });
      },
    },
  };
}
