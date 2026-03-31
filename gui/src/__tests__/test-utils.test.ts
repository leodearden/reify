import { describe, it, expect, vi } from 'vitest';
import { flushMicrotasks, deferred } from './test-utils';

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

describe('deferred', () => {
  it('returns an object with promise and resolve properties', () => {
    const d = deferred<string>();
    expect(d.promise).toBeInstanceOf(Promise);
    expect(typeof d.resolve).toBe('function');
  });

  it('calling resolve(value) resolves the promise to that value', async () => {
    const d = deferred<number>();
    d.resolve(42);
    const result = await d.promise;
    expect(result).toBe(42);
  });

  it('respects generic type parameter', async () => {
    const d = deferred<{ name: string }>();
    const obj = { name: 'test' };
    d.resolve(obj);
    const result = await d.promise;
    expect(result).toEqual({ name: 'test' });
  });
});
