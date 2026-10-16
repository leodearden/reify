/**
 * One owner for a group of event-subscription handles. Every held handle is
 * disposed exactly once, by the next `disposeAll`; a handle that arrives after
 * a `disposeAll` it was pending across is disposed on arrival and never held.
 */
export type Unsubscribe = () => void;

export interface SubscriptionRow {
  readonly subscribe: () => Promise<Unsubscribe>;
  readonly onFailure: (err: unknown) => void;
}

export interface SubscriptionSet {
  track(pending: Promise<Unsubscribe>): Promise<void>;
  subscribeAll(rows: readonly SubscriptionRow[]): Promise<boolean>;
  disposeAll(): void;
}

export function createSubscriptionSet(): SubscriptionSet {
  let generation = 0;
  let held: Unsubscribe[] = [];

  async function track(pending: Promise<Unsubscribe>): Promise<void> {
    const startedIn = generation;
    const unsubscribe = await pending;
    if (generation !== startedIn) {
      unsubscribe();
      return;
    }
    held.push(unsubscribe);
  }

  async function subscribeAll(rows: readonly SubscriptionRow[]): Promise<boolean> {
    const startedIn = generation;
    for (const row of rows) {
      try {
        await track(row.subscribe());
      } catch (err) {
        row.onFailure(err);
      }
      if (generation !== startedIn) return false;
    }
    return true;
  }

  function disposeAll(): void {
    generation++;
    const disposing = held;
    held = [];
    for (const unsubscribe of disposing) unsubscribe();
  }

  return { track, subscribeAll, disposeAll };
}
