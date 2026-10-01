import { describe, it, expect, vi } from 'vitest';
import { resolveDebugPort, debugUrlForPort, resolveVitePort } from './endpoint.js';

describe('resolveDebugPort', () => {
  it('returns the port from REIFY_DEBUG_PORT when valid', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: '4500' })).toBe(4500);
  });

  it('returns 3939 when REIFY_DEBUG_PORT is unset', () => {
    expect(resolveDebugPort({})).toBe(3939);
  });

  it('returns 3939 when REIFY_DEBUG_PORT is invalid', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: 'bad' })).toBe(3939);
  });

  it('returns 3939 when REIFY_DEBUG_PORT is 0', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: '0' })).toBe(3939);
  });

  // Strict digits-only: whitespace-padded and trailing-garbage values must
  // fall back just like the Rust parse_debug_port (uses str::parse::<u32>()).
  it('returns 3939 when REIFY_DEBUG_PORT has leading whitespace', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: ' 4500' })).toBe(3939);
  });

  it('returns 3939 when REIFY_DEBUG_PORT has trailing whitespace', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: '4500 ' })).toBe(3939);
  });

  it('returns 3939 when REIFY_DEBUG_PORT has trailing non-digit chars', () => {
    expect(resolveDebugPort({ REIFY_DEBUG_PORT: '4500abc' })).toBe(3939);
  });
});

describe('debugUrlForPort', () => {
  it('formats port 4500 correctly', () => {
    expect(debugUrlForPort(4500)).toBe('http://127.0.0.1:4500/mcp');
  });

  it('formats port 3939 correctly', () => {
    expect(debugUrlForPort(3939)).toBe('http://127.0.0.1:3939/mcp');
  });
});

describe('resolveVitePort', () => {
  const ALLOCATED = 40123;
  const allocator = () => vi.fn(async () => ALLOCATED);

  it('honours a valid REIFY_VITE_PORT without allocating', async () => {
    const allocate = allocator();
    await expect(resolveVitePort({ REIFY_VITE_PORT: '5173' }, allocate)).resolves.toBe(5173);
    expect(allocate).not.toHaveBeenCalled();
  });

  it('allocates a free port when REIFY_VITE_PORT is unset', async () => {
    const allocate = allocator();
    await expect(resolveVitePort({}, allocate)).resolves.toBe(ALLOCATED);
    expect(allocate).toHaveBeenCalledTimes(1);
  });

  it.each(['abc', '0', '65536', ' 5173', '5173x', ''])(
    'replaces invalid REIFY_VITE_PORT %j with an allocated port',
    async (raw) => {
      const allocate = allocator();
      await expect(resolveVitePort({ REIFY_VITE_PORT: raw }, allocate)).resolves.toBe(ALLOCATED);
      expect(allocate).toHaveBeenCalledTimes(1);
    },
  );
});
