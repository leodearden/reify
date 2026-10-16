import { describe, it, expect, vi } from 'vitest';
import { createSubscriptionSet, type SubscriptionRow, type Unsubscribe } from '../../app/subscriptionSet';
import { deferred } from '../test-utils';

describe('SubscriptionSet.track / disposeAll', () => {
  it('disposeAll calls every held handle exactly once, and a second disposeAll calls none again', async () => {
    const set = createSubscriptionSet();
    const a = vi.fn();
    const b = vi.fn();
    await set.track(Promise.resolve(a));
    await set.track(Promise.resolve(b));

    set.disposeAll();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);

    set.disposeAll();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
  });

  it('a handle that arrives after a disposeAll is disposed on arrival and never held', async () => {
    const set = createSubscriptionSet();
    const pending = deferred<Unsubscribe>();
    const handle = vi.fn();
    const tracked = set.track(pending.promise);

    set.disposeAll();
    expect(handle).not.toHaveBeenCalled();

    pending.resolve(handle);
    await tracked;
    expect(handle).toHaveBeenCalledTimes(1);

    set.disposeAll();
    expect(handle).toHaveBeenCalledTimes(1);
  });

  it('re-entry: after disposeAll then a fresh track, only the fresh generation is live', async () => {
    const set = createSubscriptionSet();
    const a = vi.fn();
    const b = vi.fn();
    await set.track(Promise.resolve(a));
    set.disposeAll();

    await set.track(Promise.resolve(b));
    expect(b).not.toHaveBeenCalled();

    set.disposeAll();
    expect(b).toHaveBeenCalledTimes(1);
    expect(a).toHaveBeenCalledTimes(1);
  });

  it('track rejects with the pending rejection and holds nothing', async () => {
    const set = createSubscriptionSet();
    const err = new Error('subscribe failed');

    await expect(set.track(Promise.reject(err))).rejects.toBe(err);
    expect(() => set.disposeAll()).not.toThrow();
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
      { subscribe: async () => firstHandle, onFailure: vi.fn() },
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
});
