import { describe, it, expect, vi } from 'vitest';
import { flushMacrotasks, flushMicrotasks, deferred, withSuppressedRejections, withSuppressedRejectionsAndErrorSpy, withSuppressedRejectionsAndWarnSpy } from './test-utils';

describe('flushMacrotasks', () => {
  it('returns a Promise that resolves after yielding to the macrotask queue', async () => {
    const result = flushMacrotasks();
    expect(result).toBeInstanceOf(Promise);
    await result;
  });

  it('accepts an optional ms parameter for the timeout delay', async () => {
    const start = performance.now();
    await flushMacrotasks(50);
    const elapsed = performance.now() - start;
    // Should wait at least ~50ms (allow small margin)
    expect(elapsed).toBeGreaterThanOrEqual(40);
  });

  it('side effects scheduled via setTimeout(0) are visible after awaiting', async () => {
    let flag = false;
    setTimeout(() => { flag = true; }, 0);
    expect(flag).toBe(false);
    await flushMacrotasks();
    expect(flag).toBe(true);
  });

  it('Promise.resolve() microtask work is visible after awaiting', async () => {
    let flag = false;
    Promise.resolve().then(() => { flag = true; });
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
    const id = setTimeout(() => { flag = true; }, 0);
    await flushMicrotasks();
    // setTimeout(0) callback has NOT fired — only microtasks were flushed
    expect(flag).toBe(false);
    clearTimeout(id);
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

  it('returns an object with a reject property (function)', () => {
    const d = deferred<string>();
    expect(typeof d.reject).toBe('function');
  });

  it('calling reject(err) rejects the promise with that error', async () => {
    const d = deferred<number>();
    const err = new Error('test rejection');
    d.reject(err);
    await expect(d.promise).rejects.toThrow('test rejection');
  });

  it('respects generic type parameter', async () => {
    const d = deferred<{ name: string }>();
    const obj = { name: 'test' };
    d.resolve(obj);
    const result = await d.promise;
    expect(result).toEqual({ name: 'test' });
  });

  it('promise is initially pending (not immediately resolved)', async () => {
    const d = deferred<string>();
    const winner = await Promise.race([d.promise, Promise.resolve('sentinel')]);
    expect(winner).toBe('sentinel');
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

describe('withSuppressedRejectionsAndErrorSpy', () => {
  it('passes a mock function as errorSpy to the callback', async () => {
    let receivedSpy: unknown;
    await withSuppressedRejectionsAndErrorSpy(async (spy) => {
      receivedSpy = spy;
    });
    expect(vi.isMockFunction(receivedSpy)).toBe(true);
  });

  it('errorSpy is console.error spied on, with output suppressed during fn', async () => {
    await withSuppressedRejectionsAndErrorSpy(async (spy) => {
      // The spy wraps console.error — console.error === spy inside fn
      expect(spy).toBe(console.error);
      // Calling console.error (which === spy) records the call via the spy
      console.error('test message');
      expect(spy).toHaveBeenCalledWith('test message');
    });
  });

  it('restores console.error after fn resolves', async () => {
    const original = console.error;
    await withSuppressedRejectionsAndErrorSpy(async () => {
      // inside fn, console.error is mocked (not the original)
      expect(console.error).not.toBe(original);
    });
    // after fn resolves, console.error should be restored
    expect(console.error).toBe(original);
  });

  it('restores console.error after fn rejects', async () => {
    const original = console.error;
    await withSuppressedRejectionsAndErrorSpy(async () => {
      throw new Error('expected failure');
    }).catch(() => {});
    // even after rejection, console.error should be restored
    expect(console.error).toBe(original);
  });

  it('suppresses unhandled rejections by delegating to withSuppressedRejections', async () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    await withSuppressedRejectionsAndErrorSpy(async () => {
      // no-op fn, just verifying delegation
    });

    expect(addSpy.mock.calls.some(([event]) => event === 'unhandledrejection')).toBe(true);
    expect(removeSpy.mock.calls.some(([event]) => event === 'unhandledrejection')).toBe(true);
    addSpy.mockRestore();
    removeSpy.mockRestore();
  });

  it('re-throws the original error from fn', async () => {
    const original = new Error('propagated failure');
    await expect(
      withSuppressedRejectionsAndErrorSpy(async () => { throw original; }),
    ).rejects.toThrow(original);
  });
});

describe('withSuppressedRejectionsAndWarnSpy', () => {
  it('passes a mock function as warnSpy to the callback', async () => {
    let receivedSpy: unknown;
    await withSuppressedRejectionsAndWarnSpy(async (spy) => {
      receivedSpy = spy;
    });
    expect(vi.isMockFunction(receivedSpy)).toBe(true);
  });

  it('warnSpy is console.warn spied on, with output suppressed during fn', async () => {
    await withSuppressedRejectionsAndWarnSpy(async (spy) => {
      // The spy wraps console.warn — console.warn === spy inside fn
      expect(spy).toBe(console.warn);
      // Calling console.warn (which === spy) records the call via the spy
      console.warn('test message');
      expect(spy).toHaveBeenCalledWith('test message');
    });
  });

  it('restores console.warn after fn resolves', async () => {
    const original = console.warn;
    await withSuppressedRejectionsAndWarnSpy(async () => {
      // inside fn, console.warn is mocked (not the original)
      expect(console.warn).not.toBe(original);
    });
    // after fn resolves, console.warn should be restored
    expect(console.warn).toBe(original);
  });

  it('restores console.warn after fn rejects', async () => {
    const original = console.warn;
    await withSuppressedRejectionsAndWarnSpy(async () => {
      throw new Error('expected failure');
    }).catch(() => {});
    // even after rejection, console.warn should be restored
    expect(console.warn).toBe(original);
  });

  it('suppresses unhandled rejections by delegating to withSuppressedRejections', async () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    await withSuppressedRejectionsAndWarnSpy(async () => {
      // no-op fn, just verifying delegation
    });

    expect(addSpy.mock.calls.some(([event]) => event === 'unhandledrejection')).toBe(true);
    expect(removeSpy.mock.calls.some(([event]) => event === 'unhandledrejection')).toBe(true);
    addSpy.mockRestore();
    removeSpy.mockRestore();
  });

  it('re-throws the original error from fn', async () => {
    const original = new Error('propagated failure');
    await expect(
      withSuppressedRejectionsAndWarnSpy(async () => { throw original; }),
    ).rejects.toThrow(original);
  });
});
