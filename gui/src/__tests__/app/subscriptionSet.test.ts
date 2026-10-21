import { describe, it, expect, vi, afterEach } from 'vitest';
import { createSubscriptionSet, type SubscriptionRow, type Unsubscribe } from '../../app/subscriptionSet';
import { deferred } from '../test-utils';

function resolvedRow(handle: Unsubscribe): SubscriptionRow {
  return { subscribe: async () => handle, onFailure: vi.fn() };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('SubscriptionSet.disposeAll', () => {
  it('calls every held handle exactly once, and a second disposeAll calls none again', async () => {
    const set = createSubscriptionSet();
    const a = vi.fn();
    const b = vi.fn();
    await set.subscribeAll([resolvedRow(a), resolvedRow(b)]);

    set.disposeAll();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);

    set.disposeAll();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
  });

  it('a handle that throws while disposing is logged and does not stop the handles after it', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const set = createSubscriptionSet();
    const boom = new Error('unsubscribe failed');
    const throwing = vi.fn(() => {
      throw boom;
    });
    const after = vi.fn();
    await set.subscribeAll([resolvedRow(throwing), resolvedRow(after)]);

    expect(() => set.disposeAll()).not.toThrow();
    expect(throwing).toHaveBeenCalledTimes(1);
    expect(after).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalledWith(expect.any(String), boom);

    set.disposeAll();
    expect(throwing).toHaveBeenCalledTimes(1);
    expect(after).toHaveBeenCalledTimes(1);
  });
});

describe('SubscriptionSet.subscribeAll', () => {
  it('subscribes rows in order, routes each failure shape to its own row, and resolves true', async () => {
    const set = createSubscriptionSet();
    const order: string[] = [];
    const rejected = new Error('rejected');
    const thrown = new Error('thrown');
    const handles = { first: vi.fn(), middle: vi.fn(), last: vi.fn() };
    const failures = {
      first: vi.fn(),
      rejects: vi.fn(),
      middle: vi.fn(),
      throws: vi.fn(),
      last: vi.fn(),
    };
    const row = (id: keyof typeof failures, subscribe: () => Promise<Unsubscribe>): SubscriptionRow => ({
      subscribe: () => {
        order.push(id);
        return subscribe();
      },
      onFailure: failures[id],
    });
    const rows = [
      row('first', async () => handles.first),
      row('rejects', async () => {
        throw rejected;
      }),
      row('middle', async () => handles.middle),
      row('throws', () => {
        throw thrown;
      }),
      row('last', async () => handles.last),
    ];

    const completed = await set.subscribeAll(rows);

    expect(completed).toBe(true);
    expect(order).toEqual(['first', 'rejects', 'middle', 'throws', 'last']);
    expect(failures.rejects).toHaveBeenCalledTimes(1);
    expect(failures.rejects).toHaveBeenCalledWith(rejected);
    expect(failures.throws).toHaveBeenCalledTimes(1);
    expect(failures.throws).toHaveBeenCalledWith(thrown);
    expect(failures.first).not.toHaveBeenCalled();
    expect(failures.middle).not.toHaveBeenCalled();
    expect(failures.last).not.toHaveBeenCalled();
    for (const handle of Object.values(handles)) expect(handle).not.toHaveBeenCalled();

    set.disposeAll();
    for (const handle of Object.values(handles)) expect(handle).toHaveBeenCalledTimes(1);
  });

  it('a disposeAll while a row is pending disposes its late handle on arrival, skips later rows, and resolves false', async () => {
    const set = createSubscriptionSet();
    const firstHandle = vi.fn();
    const lateHandle = vi.fn();
    const pending = deferred<Unsubscribe>();
    const pendingSubscribe = vi.fn(() => pending.promise);
    const laterSubscribe = vi.fn(async () => vi.fn());
    const rows: SubscriptionRow[] = [
      resolvedRow(firstHandle),
      { subscribe: pendingSubscribe, onFailure: vi.fn() },
      { subscribe: laterSubscribe, onFailure: vi.fn() },
    ];

    const result = set.subscribeAll(rows);
    await vi.waitFor(() => expect(pendingSubscribe).toHaveBeenCalledTimes(1));
    expect(laterSubscribe).not.toHaveBeenCalled();

    set.disposeAll();
    expect(firstHandle).toHaveBeenCalledTimes(1);

    pending.resolve(lateHandle);
    expect(await result).toBe(false);
    expect(lateHandle).toHaveBeenCalledTimes(1);
    expect(laterSubscribe).not.toHaveBeenCalled();

    set.disposeAll();
    expect(firstHandle).toHaveBeenCalledTimes(1);
    expect(lateHandle).toHaveBeenCalledTimes(1);
  });

  it('a late handle that throws while disposing on arrival is logged, not reported as its row failing', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const set = createSubscriptionSet();
    const boom = new Error('unsubscribe failed');
    const lateHandle = vi.fn(() => {
      throw boom;
    });
    const pending = deferred<Unsubscribe>();
    const onFailure = vi.fn();
    const pendingSubscribe = vi.fn(() => pending.promise);

    const result = set.subscribeAll([{ subscribe: pendingSubscribe, onFailure }]);
    await vi.waitFor(() => expect(pendingSubscribe).toHaveBeenCalledTimes(1));
    set.disposeAll();
    pending.resolve(lateHandle);

    expect(await result).toBe(false);
    expect(lateHandle).toHaveBeenCalledTimes(1);
    expect(onFailure).not.toHaveBeenCalled();
    expect(consoleError).toHaveBeenCalledWith(expect.any(String), boom);
  });

  it('re-entry: a pass overlapping a disposeAll and a fresh pass leaves only the fresh pass live', async () => {
    const set = createSubscriptionSet();
    const staleHeld = vi.fn();
    const staleLate = vi.fn();
    const fresh = vi.fn();
    const pending = deferred<Unsubscribe>();
    const pendingSubscribe = vi.fn(() => pending.promise);

    const stalePass = set.subscribeAll([
      resolvedRow(staleHeld),
      { subscribe: pendingSubscribe, onFailure: vi.fn() },
    ]);
    await vi.waitFor(() => expect(pendingSubscribe).toHaveBeenCalledTimes(1));

    set.disposeAll();
    expect(await set.subscribeAll([resolvedRow(fresh)])).toBe(true);

    pending.resolve(staleLate);
    expect(await stalePass).toBe(false);
    expect(staleHeld).toHaveBeenCalledTimes(1);
    expect(staleLate).toHaveBeenCalledTimes(1);
    expect(fresh).not.toHaveBeenCalled();

    set.disposeAll();
    expect(fresh).toHaveBeenCalledTimes(1);
    expect(staleHeld).toHaveBeenCalledTimes(1);
    expect(staleLate).toHaveBeenCalledTimes(1);
  });
});
