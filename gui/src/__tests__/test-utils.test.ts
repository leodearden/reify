import { describe, it, expect, vi } from 'vitest';
import { flushMacrotasks, flushMicrotasks, deferred, withSuppressedRejections } from './test-utils';

describe('flushMacrotasks', () => {
  it('returns a Promise that resolves after yielding to the macrotask queue', async () => {
    const result = flushMacrotasks();
    expect(result).toBeInstanceOf(Promise);
    await result;
  });

  it('side effects scheduled via setTimeout(0) are visible after awaiting', async () => {
    let flag = false;
    setTimeout(() => { flag = true; }, 0);
    expect(flag).toBe(false);
    await flushMacrotasks();
    expect(flag).toBe(true);
  });
});

describe('flushMicrotasks', () => {
  it('returns a Promise', async () => {
    const result = flushMicrotasks();
    expect(result).toBeInstanceOf(Promise);
    await result;
  });

  it('does not yield to the macrotask queue (setTimeout callbacks not yet fired)', async () => {
    let flag = false;
    setTimeout(() => { flag = true; }, 0);
    await flushMicrotasks();
    // setTimeout(0) callback has NOT fired — only microtasks were flushed
    expect(flag).toBe(false);
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

describe('withSuppressedRejections', () => {
  it('registers unhandledrejection handler before calling fn', async () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    let handlerRegisteredBeforeCall = false;

    await withSuppressedRejections(async () => {
      handlerRegisteredBeforeCall = addSpy.mock.calls.some(
        ([event]) => event === 'unhandledrejection',
      );
    });

    expect(handlerRegisteredBeforeCall).toBe(true);
    addSpy.mockRestore();
  });

  it('removes handler after fn resolves', async () => {
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    await withSuppressedRejections(async () => {
      // fn completes successfully
    });

    const removedRejection = removeSpy.mock.calls.some(
      ([event]) => event === 'unhandledrejection',
    );
    expect(removedRejection).toBe(true);
    removeSpy.mockRestore();
  });

  it('removes handler after fn rejects', async () => {
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    await withSuppressedRejections(async () => {
      throw new Error('boom');
    }).catch(() => {}); // swallow so the test doesn't fail

    const removedRejection = removeSpy.mock.calls.some(
      ([event]) => event === 'unhandledrejection',
    );
    expect(removedRejection).toBe(true);
    removeSpy.mockRestore();
  });

  it('re-throws the original error from fn', async () => {
    const original = new Error('original failure');

    await expect(
      withSuppressedRejections(async () => { throw original; }),
    ).rejects.toThrow(original);
  });
});
