import { describe, it, expect, vi, afterEach } from 'vitest';
import { createDebugBridgeStarter } from '../../app/debugBridgeStarter';
import { createSubscriptionSet, type Unsubscribe } from '../../app/subscriptionSet';
import { deferred, flushMacrotasks } from '../test-utils';

function setup() {
  const subscriptions = createSubscriptionSet();
  const starts: { resolve(handle: Unsubscribe): void; reject(err: unknown): void }[] = [];
  const start = vi.fn(() => {
    const pending = deferred<Unsubscribe>();
    starts.push(pending);
    return pending.promise;
  });
  const ensure = createDebugBridgeStarter({ subscriptions, start });
  return { subscriptions, start, starts, ensure };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('createDebugBridgeStarter', () => {
  it('concurrent ensures share one start, and every caller settles only once it is live', async () => {
    const { subscriptions, start, starts, ensure } = setup();
    const settled = { first: false, second: false };
    const first = ensure().then(() => {
      settled.first = true;
    });
    const second = ensure().then(() => {
      settled.second = true;
    });

    expect(start).toHaveBeenCalledTimes(1);
    await flushMacrotasks();
    expect(settled).toEqual({ first: false, second: false });

    const stop = vi.fn();
    starts[0].resolve(stop);
    await Promise.all([first, second]);
    expect(settled).toEqual({ first: true, second: true });
    expect(start).toHaveBeenCalledTimes(1);
    expect(stop).not.toHaveBeenCalled();

    subscriptions.disposeAll();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it('an ensure while the bridge is live starts nothing', async () => {
    const { start, starts, ensure } = setup();
    const live = ensure();
    starts[0].resolve(vi.fn());
    await live;

    await expect(ensure()).resolves.toBeUndefined();
    expect(start).toHaveBeenCalledTimes(1);
  });

  it('a failed start rejects every waiting caller, and the next ensure starts afresh', async () => {
    const { subscriptions, start, starts, ensure } = setup();
    const boom = new Error('listen failed');
    const first = ensure();
    const second = ensure();

    starts[0].reject(boom);
    await expect(first).rejects.toBe(boom);
    await expect(second).rejects.toBe(boom);

    const third = ensure();
    expect(start).toHaveBeenCalledTimes(2);
    const stop2 = vi.fn();
    starts[1].resolve(stop2);
    await third;

    subscriptions.disposeAll();
    expect(stop2).toHaveBeenCalledTimes(1);
  });

  it('disposing the held handle stops the bridge, and the next ensure starts afresh', async () => {
    const { subscriptions, start, starts, ensure } = setup();
    const stop1 = vi.fn();
    const firstLive = ensure();
    starts[0].resolve(stop1);
    await firstLive;

    subscriptions.disposeAll();
    expect(stop1).toHaveBeenCalledTimes(1);

    const secondLive = ensure();
    expect(start).toHaveBeenCalledTimes(2);
    const stop2 = vi.fn();
    starts[1].resolve(stop2);
    await secondLive;

    subscriptions.disposeAll();
    expect(stop1).toHaveBeenCalledTimes(1);
    expect(stop2).toHaveBeenCalledTimes(1);
  });

  it('a disposeAll while the start is pending stops the late handle on arrival, and the next ensure starts afresh', async () => {
    const { subscriptions, start, starts, ensure } = setup();
    const stop1 = vi.fn();
    const pending = ensure();

    subscriptions.disposeAll();
    starts[0].resolve(stop1);
    await expect(pending).resolves.toBeUndefined();
    expect(stop1).toHaveBeenCalledTimes(1);

    void ensure();
    expect(start).toHaveBeenCalledTimes(2);
  });

  it('a start that a disposeAll overtakes and that then fails rejects its caller, and the next ensure starts afresh', async () => {
    const { subscriptions, start, starts, ensure } = setup();
    const boom = new Error('listen failed');
    const pending = ensure();

    subscriptions.disposeAll();
    starts[0].reject(boom);
    await expect(pending).rejects.toBe(boom);

    void ensure();
    expect(start).toHaveBeenCalledTimes(2);
  });

  it('a stop that throws while disposing still lets the next ensure start afresh', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const { subscriptions, start, starts, ensure } = setup();
    const boom = new Error('stop failed');
    const live = ensure();
    starts[0].resolve(() => {
      throw boom;
    });
    await live;

    expect(() => subscriptions.disposeAll()).not.toThrow();

    void ensure();
    expect(start).toHaveBeenCalledTimes(2);
  });
});
