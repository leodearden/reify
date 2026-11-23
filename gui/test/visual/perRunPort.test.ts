import { describe, it, expect, vi } from 'vitest';
import { resolvePerRunPort } from './perRunPort.js';

describe.each(['REIFY_DEBUG_PORT', 'REIFY_VITE_PORT'] as const)('resolvePerRunPort(%s)', (name) => {
  const ALLOCATED = 40123;
  const allocator = () => vi.fn(async () => ALLOCATED);

  it('honours a valid value without allocating', async () => {
    const allocate = allocator();
    await expect(resolvePerRunPort(name, { [name]: '5173' }, allocate)).resolves.toBe(5173);
    expect(allocate).not.toHaveBeenCalled();
  });

  it('allocates a free port when unset', async () => {
    const allocate = allocator();
    await expect(resolvePerRunPort(name, {}, allocate)).resolves.toBe(ALLOCATED);
    expect(allocate).toHaveBeenCalledTimes(1);
  });

  it.each(['abc', '0', '65536', ' 5173', '5173x', ''])(
    'replaces invalid value %j with an allocated port',
    async (raw) => {
      const allocate = allocator();
      await expect(resolvePerRunPort(name, { [name]: raw }, allocate)).resolves.toBe(ALLOCATED);
      expect(allocate).toHaveBeenCalledTimes(1);
    },
  );

  it('reads only its own variable', async () => {
    const other = name === 'REIFY_DEBUG_PORT' ? 'REIFY_VITE_PORT' : 'REIFY_DEBUG_PORT';
    const allocate = allocator();
    await expect(resolvePerRunPort(name, { [other]: '5173' }, allocate)).resolves.toBe(ALLOCATED);
  });
});
