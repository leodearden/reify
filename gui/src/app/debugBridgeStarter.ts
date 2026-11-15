/**
 * Start the debug bridge unless it is live or starting, so one debug-request
 * listener answers however many callers ask. Every caller awaits the one start
 * and sees its failure. The live handle joins the subscription set. A failed
 * start, or disposing the handle, lets the next call after that start afresh;
 * a handle a `disposeAll` overtook is disposed only when it arrives, so an
 * ensure issued before then shares the overtaken start.
 */
import type { SubscriptionSet, Unsubscribe } from './subscriptionSet';

export interface DebugBridgeStarterDeps {
  readonly subscriptions: Pick<SubscriptionSet, 'subscribeAll'>;
  readonly start: () => Promise<Unsubscribe>;
}

export function createDebugBridgeStarter({ subscriptions, start }: DebugBridgeStarterDeps): () => Promise<void> {
  let starting: Promise<void> | null = null;

  async function hold(): Promise<void> {
    const failures: unknown[] = [];
    await subscriptions.subscribeAll([
      {
        subscribe: async () => {
          const stop = await start();
          return () => {
            starting = null;
            stop();
          };
        },
        onFailure: (err) => failures.push(err),
      },
    ]);
    if (failures.length > 0) {
      starting = null;
      throw failures[0];
    }
  }

  return function ensure(): Promise<void> {
    starting ??= hold();
    return starting;
  };
}
