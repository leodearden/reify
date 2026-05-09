import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
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
  let server: PermissionServer;

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

    let capturedRequestId = '';
    server.onRequest((req) => {
      capturedRequestId = req.request_id;
    });

    const client = await connectClient(server.url());
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Bash', input: { command: 'ls' } },
    });

    // Wait for the request_id to be captured
    await new Promise<void>((resolve) => {
      const check = () => {
        if (capturedRequestId) resolve();
        else setTimeout(check, 10);
      };
      check();
    });

    server.decide(capturedRequestId, { behavior: 'allow' });

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

    let capturedRequestId = '';
    server.onRequest((req) => {
      capturedRequestId = req.request_id;
    });

    const client = await connectClient(server.url());
    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Write', input: { path: '/etc/passwd' } },
    });

    await new Promise<void>((resolve) => {
      const check = () => {
        if (capturedRequestId) resolve();
        else setTimeout(check, 10);
      };
      check();
    });

    server.decide(capturedRequestId, { behavior: 'deny', message: 'no' });

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

    let requestedToolName = '';
    server.onRequest((req) => {
      requestedToolName = req.tool_name;
    });

    server.setRemembered('Write');

    const client = await connectClient(server.url());

    // 'Bash' is not remembered, so onRequest should fire
    let capturedRequestId = '';
    server.onRequest((req) => {
      capturedRequestId = req.request_id;
      requestedToolName = req.tool_name;
    });

    const toolCallPromise = client.callTool({
      name: 'approve_tool',
      arguments: { tool_name: 'Bash', input: { command: 'ls' } },
    });

    await new Promise<void>((resolve) => {
      const check = () => {
        if (capturedRequestId) resolve();
        else setTimeout(check, 10);
      };
      check();
    });

    expect(requestedToolName).toBe('Bash');
    server.decide(capturedRequestId, { behavior: 'allow' });
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
    server.onRequest((req) => {
      requests.push(req);
    });

    const client = await connectClient(server.url());

    const p1 = client.callTool({ name: 'approve_tool', arguments: { tool_name: 'Write', input: { path: '/a' } } });
    const p2 = client.callTool({ name: 'approve_tool', arguments: { tool_name: 'Bash', input: { command: 'rm -rf' } } });

    // Wait for both requests to arrive
    await new Promise<void>((resolve) => {
      const check = () => {
        if (requests.length >= 2) resolve();
        else setTimeout(check, 10);
      };
      check();
    });

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
});
