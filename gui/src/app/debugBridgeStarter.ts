/**
 * Start the debug bridge unless it is live or starting, so one debug-request
 * listener answers however many callers ask. Every caller awaits the one start
 * and sees its failure. The live handle joins the subscription set; a failed
 * start, or disposing the handle (including a `disposeAll` that overtakes a
 * pending start), lets the next call start afresh.
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
