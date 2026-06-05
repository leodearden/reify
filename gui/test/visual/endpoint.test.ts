import { describe, it, expect } from 'vitest';
import { resolveDebugPort, debugUrlForPort } from './endpoint.js';

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
});

describe('debugUrlForPort', () => {
  it('formats port 4500 correctly', () => {
    expect(debugUrlForPort(4500)).toBe('http://127.0.0.1:4500/mcp');
  });

  it('formats port 3939 correctly', () => {
    expect(debugUrlForPort(3939)).toBe('http://127.0.0.1:3939/mcp');
  });
});
