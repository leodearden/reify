import { describe, it, expect, vi } from 'vitest';
import { flushMicrotasks } from './test-utils';

describe('flushMicrotasks', () => {
  it('returns a Promise that resolves after yielding to the microtask queue', async () => {
    const result = flushMicrotasks();
    expect(result).toBeInstanceOf(Promise);
    await result;
  });

  it('side effects scheduled via setTimeout(0) are visible after awaiting', async () => {
    let flag = false;
    setTimeout(() => { flag = true; }, 0);
    expect(flag).toBe(false);
    await flushMicrotasks();
    expect(flag).toBe(true);
  });
});
