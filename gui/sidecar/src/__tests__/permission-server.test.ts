import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { createPermissionServer } from '../permission-server.js';
import type { PermissionServer } from '../permission-server.js';

/**
 * Helper: connect an MCP SDK client to the permission server.
 */
async function connectClient(serverUrl: string): Promise<Client> {
  const client = new Client({ name: 'test-client', version: '1.0.0' }, { capabilities: {} });
  const transport = new StreamableHTTPClientTransport(new URL(serverUrl));
  await client.connect(transport);
  return client;
}

describe('createPermissionServer', () => {
  let server: ReturnType<typeof createPermissionServer>;

  afterEach(async () => {
    await server.stop().catch(() => {});
  });

  // (a) start() binds an HTTP listener and url() returns http://127.0.0.1:<port>/mcp
  it('start() binds on a random localhost port and url() returns http://127.0.0.1:<port>/mcp', async () => {
    server = createPermissionServer();
    await server.start();

    const url = server.url();
    expect(url).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/mcp$/);

    const port = parseInt(new URL(url).port, 10);
    expect(port).toBeGreaterThan(0);
    expect(port).toBeLessThanOrEqual(65535);
  });

  // (b) stop() closes the listener so the port is reusable
  it('stop() closes the HTTP listener', async () => {
    server = createPermissionServer();
    await server.start();
    const url = server.url();

    await server.stop();

    // After stop(), connecting should fail
    const port = parseInt(new URL(url).port, 10);
    await expect(
      fetch(`http://127.0.0.1:${port}/mcp`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'test', version: '0' } } }),
      })
    ).rejects.toThrow();
  });

  // (c) calling approve_tool invokes the onRequest callback and awaits decide()
  it('approve_tool invokes onRequest callback and blocks until decide() is called', async () => {
    server = createPermissionServer();
    await server.start();

    const requestPromise = new Promise<{ request_id: string; tool_name: string; tool_input: Record<string, unknown> }>((resolve) => {
      server.onRequest((req) => resolve(req));
    });

    const client = await connectClient(server.url());

    // Start the tool call but don't await it yet — it should block waiting for decide()
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Write', input: { path: '/tmp/x' } },
    });

    // Wait for the onRequest callback to fire
    const req = await requestPromise;
    expect(req.tool_name).toBe('Write');
    expect(req.tool_input).toEqual({ path: '/tmp/x' });
    expect(typeof req.request_id).toBe('string');
    expect(req.request_id.length).toBeGreaterThan(0);

    // Now resolve the pending call
    server.decide(req.request_id, { behavior: 'allow' });

    // The tool call should now resolve
    const result = await toolCallPromise;
    expect(result).toBeDefined();

    await client.close();
  });

  // (d) decide() with allow resolves; with deny the tool returns a deny decision
  it('decide() with { behavior: "allow" } resolves the tool call with allow', async () => {
    server = createPermissionServer();
    await server.start();

    const requestIdPromise = new Promise<string>((resolve) => {
      server.onRequest((req) => resolve(req.request_id));
    });

    const client = await connectClient(server.url());
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Bash', input: { command: 'ls' } },
    });

    server.decide(await requestIdPromise, { behavior: 'allow' });

    const result = await toolCallPromise;
    // Result content should contain behavior: 'allow'
    const content = result.content as Array<{ type: string; text: string }>;
    const parsed = JSON.parse(content[0].text);
    expect(parsed.behavior).toBe('allow');

    await client.close();
  });

  it('decide() with { behavior: "deny", message: "no" } resolves the tool call with deny', async () => {
    server = createPermissionServer();
    await server.start();

    const requestIdPromise = new Promise<string>((resolve) => {
      server.onRequest((req) => resolve(req.request_id));
    });

    const client = await connectClient(server.url());
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Write', input: { path: '/etc/passwd' } },
    });

    server.decide(await requestIdPromise, { behavior: 'deny', message: 'no' });

    const result = await toolCallPromise;
    const content = result.content as Array<{ type: string; text: string }>;
    const parsed = JSON.parse(content[0].text);
    expect(parsed.behavior).toBe('deny');
    expect(parsed.message).toBe('no');

    await client.close();
  });

  // (e) setRemembered(tool_name) short-circuits without invoking onRequest
  it('setRemembered(tool_name) short-circuits future calls without invoking onRequest', async () => {
    server = createPermissionServer();
    await server.start();

    let onRequestCalled = false;
    server.onRequest(() => {
      onRequestCalled = true;
    });

    server.setRemembered('Write');

    const client = await connectClient(server.url());
    const result = await client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Write', input: { path: '/tmp/x' } },
    });

    expect(onRequestCalled).toBe(false);
    const content = result.content as Array<{ type: string; text: string }>;
    const parsed = JSON.parse(content[0].text);
    expect(parsed.behavior).toBe('allow');

    await client.close();
  });

  it('setRemembered does not affect other tool names', async () => {
    server = createPermissionServer();
    await server.start();

    server.setRemembered('Write');

    const client = await connectClient(server.url());

    // 'Bash' is not remembered, so onRequest should fire
    const requestPromise = new Promise<{ request_id: string; tool_name: string }>((resolve) => {
      server.onRequest((req) => resolve({ request_id: req.request_id, tool_name: req.tool_name }));
    });

    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Bash', input: { command: 'ls' } },
    });

    const { request_id, tool_name } = await requestPromise;
    expect(tool_name).toBe('Bash');
    server.decide(request_id, { behavior: 'allow' });
    await toolCallPromise;

    await client.close();
  });

  // (f) decide() for unknown request_id is a no-op and does not throw
  it('decide() for unknown request_id is a no-op and does not throw', () => {
    server = createPermissionServer();
    // Even without calling start(), decide() for unknown id should not throw
    expect(() => server.decide('nonexistent-id', { behavior: 'allow' })).not.toThrow();
  });

  it('decide() for unknown request_id after start() is a no-op and does not throw', async () => {
    server = createPermissionServer();
    await server.start();
    expect(() => server.decide('unknown-request-id', { behavior: 'deny' })).not.toThrow();
  });

  // Additional: multiple concurrent pending requests are handled independently
  it('handles multiple concurrent pending requests independently', async () => {
    server = createPermissionServer();
    await server.start();

    const requests: Array<{ request_id: string; tool_name: string; tool_input: Record<string, unknown> }> = [];
    const twoRequestsReady = new Promise<void>((resolve) => {
      server.onRequest((req) => {
        requests.push(req);
        if (requests.length >= 2) resolve();
      });
    });

    const client = await connectClient(server.url());

    const p1 = client.callTool({ name: 'approve_tool', arguments: { tool_name: 'Write', input: { path: '/a' } } });
    const p2 = client.callTool({ name: 'approve_tool', arguments: { tool_name: 'Bash', input: { command: 'rm -rf' } } });

    // Wait for both requests to arrive
    await twoRequestsReady;

    // Resolve in reverse order to prove independence
    server.decide(requests[1].request_id, { behavior: 'deny', message: 'no bash' });
    server.decide(requests[0].request_id, { behavior: 'allow' });

    const [r1, r2] = await Promise.all([p1, p2]);

    const content1 = r1.content as Array<{ type: string; text: string }>;
    const content2 = r2.content as Array<{ type: string; text: string }>;
    const parsed1 = JSON.parse(content1[0].text);
    const parsed2 = JSON.parse(content2[0].text);

    expect(parsed1.behavior).toBe('allow');
    expect(parsed2.behavior).toBe('deny');

    await client.close();
  });

  // stop() is idempotent
  it('stop() is idempotent — calling it twice does not throw', async () => {
    server = createPermissionServer();
    await server.start();
    await server.stop();
    await expect(server.stop()).resolves.not.toThrow();
  });

  // onRequest(null) sentinel: clears the registered handler.
  // The TypeScript signature on the unfixed interface rejects null — tsc --noEmit fails.
  // At runtime (esbuild strips types) the behaviour is correct in both versions.
  //
  // The test uses an unremembered tool so the handler would ordinarily be consulted —
  // making `handlerCalls === 0` a meaningful assertion that the handler was actually cleared.

  it('onRequest(null) clears the handler so unremembered tool calls leave the prior handler uninvoked', async () => {
    server = createPermissionServer();
    await server.start();

    let handlerCalls = 0;
    server.onRequest(() => { handlerCalls++; });
    server.onRequest(null); // clear the handler — TypeScript rejects this on unfixed signature

    const client = await connectClient(server.url());

    // Start a tools/call for an unremembered tool — do NOT await.
    // With the handler cleared, no callback fires, so the call blocks indefinitely.
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Bash', input: { command: 'ls' } },
    });

    // Wait deterministically for the request to reach pending-await state.
    // __testHooks.awaitPending(1) resolves as soon as pendingPromises.size >= 1
    // via an event-driven waiter list notified at the pendingPromises.set site;
    // setTimeout-based timeout rejects after 2 s — no polling, no Date.now().
    await server.__testHooks.awaitPending(1, 2000);

    // The cleared handler must not have been invoked during the blocking wait.
    expect(handlerCalls).toBe(0);

    // Unblock the pending call so the test and server can clean up.
    server.cancelAll();
    const result = await toolCallPromise;

    // cancelAll() resolves all pending entries with deny.
    const content = result.content as Array<{ type: string; text: string }>;
    expect(JSON.parse(content[0].text).behavior).toBe('deny');

    await client.close();
  });

  // close-listener race: listener must be registered BEFORE handleRequest so that
  // a socket-close mid-await (e.g. client abort) still triggers transport/server cleanup.
  it('registers close-listener before awaiting handleRequest so socket-close mid-request still triggers cleanup', async () => {
    server = createPermissionServer();

    // Use a promise to deterministically detect when the server-side approve_tool handler
    // has entered its blocking await (i.e., handleRequest() is suspended). The onRequest
    // callback fires synchronously inside the Promise executor that creates the pending
    // decision entry — so by the time `serverPending` resolves and our test resumes, the
    // tool handler is guaranteed to be suspended and handleRequest() is in-flight.
    let signalPending!: () => void;
    const serverPending = new Promise<void>((resolve) => { signalPending = resolve; });
    server.onRequest(() => { signalPending(); });
    await server.start();

    const mcpCloseSpy = vi.spyOn(McpServer.prototype, 'close');
    const transportCloseSpy = vi.spyOn(StreamableHTTPServerTransport.prototype, 'close');

    try {
      const ac = new AbortController();

      // Send a tools/call that blocks on the server side: approve_tool awaits decide()
      // so handleRequest() stays in-flight for the entire duration of our abort window.
      const fetchPromise = fetch(server.url(), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Accept': 'application/json, text/event-stream',
        },
        body: JSON.stringify({
          jsonrpc: '2.0',
          id: 1,
          method: 'tools/call',
          params: {
            name: 'approve_tool',
            arguments: { tool_name: 'Write', input: {} },
          },
        }),
        signal: ac.signal,
      });

      // Wait deterministically until the server's onRequest callback has fired —
      // at that point handleRequest() is guaranteed to be suspended in the decision await.
      // This replaces the previous fixed-delay approach which was flaky on slow CI.
      await serverPending;

      // Abort — this closes the client-side socket, which propagates to the server's
      // res and fires the 'close' event while handleRequest() is still awaiting.
      ac.abort();
      await fetchPromise.catch(() => {}); // AbortError is expected

      // Allow the close event to propagate through Node's event loop.
      await new Promise((r) => setTimeout(r, 100));

      // With the fix: the close listener is registered before handleRequest(), so it
      // catches the close event and calls transport.close() + mcpServer.close().
      // With the bug: the listener is registered after handleRequest() — already too late.
      expect(mcpCloseSpy).toHaveBeenCalled();
      expect(transportCloseSpy).toHaveBeenCalled();
    } finally {
      mcpCloseSpy.mockRestore();
      transportCloseSpy.mockRestore();
      // Drain pending permission requests so the server can shut down cleanly in afterEach.
      server.cancelAll();
    }
  });

  // success-path cleanup: each completed approve_tool call must close its per-request
  // transport + mcpServer. With `cleaned = true` after handleRequest() the listener is
  // short-circuited on the success path and both spies stay at 0 — proving the leak.
  it('runs cleanup on the success path: each completed approve_tool call closes its per-request transport+mcpServer', async () => {
    server = createPermissionServer();
    await server.start();

    const mcpCloseSpy = vi.spyOn(McpServer.prototype, 'close');
    const transportCloseSpy = vi.spyOn(StreamableHTTPServerTransport.prototype, 'close');

    const client = await connectClient(server.url());

    try {
      const N = 10;
      for (let i = 0; i < N; i++) {
        // Register a one-shot handler per iteration — resolves as soon as the
        // server fires onRequest, without a setTimeout busy-poll. onRequest is
        // last-write-wins, so each iteration cleanly replaces the previous one.
        const requestIdPromise = new Promise<string>((resolve) => {
          server.onRequest((req) => resolve(req.request_id));
        });

        const toolCallPromise = client.callTool({
          name: 'approve_tool',
          arguments: { tool_name: 'Write', input: { i } },
        });

        server.decide(await requestIdPromise, { behavior: 'allow' });
        await toolCallPromise;
      }

      // Poll until all N close() calls have fired, with a 2 s budget — more
      // robust than a fixed sleep on loaded CI where event-loop queuing can
      // delay post-response 'close' events beyond any fixed wall-clock constant.
      const deadline = Date.now() + 2000;
      while (mcpCloseSpy.mock.calls.length < N && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 10));
      }

      // Each of the N requests must have triggered cleanup via the 'close' listener.
      expect(mcpCloseSpy.mock.calls.length).toBeGreaterThanOrEqual(N);
      expect(transportCloseSpy.mock.calls.length).toBeGreaterThanOrEqual(N);
    } finally {
      mcpCloseSpy.mockRestore();
      transportCloseSpy.mockRestore();
      await client.close();
    }
  });
});
