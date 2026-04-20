import { describe, it, expect, vi } from 'vitest';
import { flushMacrotasks, flushMicrotasks, deferred, withSuppressedRejections, withSuppressedRejectionsAndErrorSpy, withSuppressedRejectionsAndWarnSpy, expectNoUnhandledRejections, median, formatPerfSamples, makeNode, makeTree, makeTreeWithTwoSubtrees, makeTreeWithGeometryA } from './test-utils';

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

  it('calling reject() with no argument rejects the promise with undefined', async () => {
    const d = deferred<number>();
    d.reject();
    await expect(d.promise).rejects.toBeUndefined();
  });

  it('calling reject(\'string reason\') rejects the promise with the string primitive', async () => {
    const d = deferred<number>();
    d.reject('string reason');
    await expect(d.promise).rejects.toBe('string reason');
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

describe('expectNoUnhandledRejections', () => {
  it('registers unhandledrejection handler before calling fn', async () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    let handlerRegisteredBeforeCall = false;

    await expectNoUnhandledRejections(async () => {
      handlerRegisteredBeforeCall = addSpy.mock.calls.some(
        ([event]) => event === 'unhandledrejection',
      );
    });

    expect(handlerRegisteredBeforeCall).toBe(true);
    addSpy.mockRestore();
  });

  it('removes handler after fn resolves', async () => {
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    await expectNoUnhandledRejections(async () => {
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

    await expectNoUnhandledRejections(async () => {
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
      expectNoUnhandledRejections(async () => { throw original; }),
    ).rejects.toThrow(original);
  });

  it('fails the test if an unhandledrejection fires during fn', async () => {
    await expect(
      expectNoUnhandledRejections(async () => {
        // Simulate an unhandledrejection event firing during fn
        window.dispatchEvent(new Event('unhandledrejection'));
      }),
    ).rejects.toThrow(/expected.*not.*called/i);
  });
});

describe('makeNode', () => {
  it('returns an EntityTreeNode with default fields when only entity_path is supplied', () => {
    const node = makeNode({ entity_path: 'Root.A' });
    expect(node.kind).toBe('structure');
    expect(node.type_name).toBeNull();
    expect(node.has_mesh).toBe(false);
    expect(node.trait_geometry).toBe(false);
    expect(node.children).toEqual([]);
  });

  it('sets entity_path from the overrides argument', () => {
    const node = makeNode({ entity_path: 'Root.A' });
    expect(node.entity_path).toBe('Root.A');
  });

  it('override fields win over defaults (kind, trait_geometry, type_name, has_mesh)', () => {
    const node = makeNode({ entity_path: 'Root.A', kind: 'param', trait_geometry: true, type_name: 'Bracket', has_mesh: true });
    expect(node.kind).toBe('param');
    expect(node.trait_geometry).toBe(true);
    expect(node.type_name).toBe('Bracket');
    expect(node.has_mesh).toBe(true);
  });

  it('passing children in overrides replaces the default empty array', () => {
    const child = makeNode({ entity_path: 'Root.A.a1' });
    const parent = makeNode({ entity_path: 'Root.A', children: [child] });
    expect(parent.children).toHaveLength(1);
    expect(parent.children[0].entity_path).toBe('Root.A.a1');
  });
});

describe('makeTree', () => {
  it('returns the canonical Root{A{a1,a2},B} tree structure', () => {
    expect(makeTree()).toEqual([
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
              makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ]);
  });
});

describe('makeTreeWithTwoSubtrees', () => {
  it('returns Root{A{a1,a2},B{b1,b2}} with both A and B having two param children', () => {
    expect(makeTreeWithTwoSubtrees()).toEqual([
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
              makeNode({ entity_path: 'Root.A.a2', kind: 'param' }),
            ],
          }),
          makeNode({
            entity_path: 'Root.B',
            kind: 'structure',
            children: [
              makeNode({ entity_path: 'Root.B.b1', kind: 'param' }),
              makeNode({ entity_path: 'Root.B.b2', kind: 'param' }),
            ],
          }),
        ],
      }),
    ]);
  });
});

describe('makeTreeWithGeometryA', () => {
  it('returns Root{A(trait_geometry=true){a1},B} with A carrying trait_geometry', () => {
    expect(makeTreeWithGeometryA()).toEqual([
      makeNode({
        entity_path: 'Root',
        kind: 'structure',
        children: [
          makeNode({
            entity_path: 'Root.A',
            kind: 'structure',
            trait_geometry: true,
            children: [
              makeNode({ entity_path: 'Root.A.a1', kind: 'param' }),
            ],
          }),
          makeNode({ entity_path: 'Root.B', kind: 'structure' }),
        ],
      }),
    ]);
  });
});

describe('median', () => {
  it('throws on an empty array', () => {
    expect(() => median([])).toThrow(/empty/i);
  });

  it('throws when the input contains NaN', () => {
    expect(() => median([1, NaN, 3])).toThrow(/non-finite/i);
  });

  it('throws when the input contains +Infinity', () => {
    expect(() => median([1, Infinity, 3])).toThrow(/non-finite/i);
  });

  it('throws when the input contains -Infinity', () => {
    expect(() => median([1, -Infinity, 3])).toThrow(/non-finite/i);
  });

  it('returns the single element for a one-element array', () => {
    expect(median([42])).toBe(42);
  });

  it('returns the middle element for an odd-length sorted array', () => {
    expect(median([1, 2, 3])).toBe(2);
  });

  it('returns the average of the two middle elements for an even-length sorted array', () => {
    expect(median([1, 2, 3, 4])).toBe(2.5);
  });

  it('sorts internally before computing (unsorted input)', () => {
    expect(median([3, 1, 2])).toBe(2);
  });

  it('does not mutate the caller\'s input array', () => {
    const input = [3, 1, 2];
    const copy = [...input];
    median(input);
    expect(input).toEqual(copy);
  });
});

describe('formatPerfSamples', () => {
  it('formats the median field to 2 decimal places', () => {
    // median of [1.234, 5.678, 9.012] is 5.678 → "5.68"
    const result = formatPerfSamples([1.234, 5.678, 9.012]);
    expect(result).toContain('median=5.68ms');
  });

  it('rounds sample values to 2 decimal places in the samples array', () => {
    const result = formatPerfSamples([1.234567]);
    expect(result).toContain('samples=[1.23]');
  });

  it('drops trailing zeros in the samples array (1.2 not 1.20)', () => {
    // +v.toFixed(2) coerces "1.20" back to the number 1.2 before JSON.stringify,
    // so the serialised form is [1.2] (not [1.20] with a trailing zero).
    // Note: the scalar median/min/max fields still show "1.20" via .toFixed(2)
    // for consistent decimal alignment — the trailing-zero drop applies only to
    // the samples array where JSON.stringify serialises the coerced number.
    const result = formatPerfSamples([1.2]);
    expect(result).toContain('samples=[1.2]');
    expect(result).not.toContain('samples=[1.20]');
  });

  it('formats min and max fields with ms unit rounded to 2 decimal places', () => {
    const result = formatPerfSamples([1.234, 5.678, 9.012]);
    expect(result).toContain('min=1.23ms');
    expect(result).toContain('max=9.01ms');
  });

  it('propagates the median() non-finite guard', () => {
    expect(() => formatPerfSamples([1, Infinity])).toThrow(/non-finite/i);
  });

  it('throws on empty input', () => {
    expect(() => formatPerfSamples([])).toThrow(/empty/i);
  });
});
