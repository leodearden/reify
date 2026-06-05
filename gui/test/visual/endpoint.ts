import * as net from 'node:net';

const DEFAULT_DEBUG_PORT = 3939;

export function resolveDebugPort(env: Record<string, string | undefined> = process.env): number {
  const raw = env['REIFY_DEBUG_PORT'];
  if (raw === undefined) return DEFAULT_DEBUG_PORT;
  const parsed = parseInt(raw, 10);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) return DEFAULT_DEBUG_PORT;
  return parsed;
}

export function debugUrlForPort(port: number): string {
  return `http://127.0.0.1:${port}/mcp`;
}

export function allocateFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      const port = typeof addr === 'object' && addr !== null ? addr.port : 0;
      server.close((err) => {
        if (err) reject(err);
        else resolve(port);
      });
    });
    server.on('error', reject);
  });
}
