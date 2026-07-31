import { createFeaModeStore, type FeaModeStore } from './feaModeStore';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/**
 * A viewportId-keyed collection of `FeaModeStore` instances (#5670).
 *
 * `FeaModeStore` itself is unchanged — it is already a clean zero-arg
 * per-instance factory. What this fixes is *ownership*: before #5670 the store
 * was modeled as a singleton, and two competing singletons existed (one
 * created inside `DualViewport`, one App-scope for the MultiViewport branch).
 * Because there was only ever "the one store", downstream sites had to
 * hard-code pane-0-ness — the App pane-config carve-out, the single
 * `__REIFY_DEBUG__.feaMode` slot, and the debug bridge's document-wide
 * `fea-mode-channel-select` lookup. A registry lets every pane own its own
 * store, so those three sites become uniform lookups keyed by viewportId.
 */
export interface FeaModeStoreRegistry {
  /**
   * The LIVE backing record, keyed by viewportId.
   *
   * This is the record itself, not a snapshot: entries added by a later
   * `get()` are visible through a reference captured earlier. App relies on
   * that aliasing — it hands this record to
   * `registerDebugPanel('feaModes', registry.stores)` at mount, well before
   * the `panes` mapArray calls `get()` for each pane, and never re-registers.
   *
   * Treat as read-only. Mutate only via `get()`.
   */
  readonly stores: Record<string, FeaModeStore>;
  /**
   * Get-or-create the store for `viewportId`.
   *
   * Memoized per id: repeated calls return the same instance, so a pane's FEA
   * state (channel, palette, range, warp, active case) is stable for as long
   * as the registry lives.
   */
  get(viewportId: string): FeaModeStore;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create a viewportId-keyed FeaModeStore registry.
 *
 * A plain `Record` rather than a `Map`, deliberately: it is assigned straight
 * onto `ReifyDebugContext.feaModes` as a live reference, mirroring the
 * established `viewports?: Record<string, DebugViewport>` slot, and debug
 * consumers index it directly. It is also NOT a SolidJS store — the record is
 * a lookup table, not reactive state; each entry's own `createStore` supplies
 * all the reactivity any consumer needs.
 *
 * **Entries are never evicted.** A store outliving the pane that created it is
 * deliberate, and turns a latent bug into a feature: under the old
 * two-singleton design, flipping a file between single- and multi-pane layouts
 * handed control to a *different* store and silently reset FEA mode. A
 * persistent keyed entry makes that state survive the flip. A lingering entry
 * for an unmounted pane is inert to the debug bridge, which reaches a store
 * only through a rendered `<select>` that exists only while its pane is
 * mounted. The bound is the number of distinct viewportIds a session produces
 * (`design-main` plus `pane-N`), so there is no unbounded growth to manage.
 *
 * **Non-eviction spans documents, not just layout flips.** Keys are POSITIONAL
 * (`pane-N`), and opening a different file reuses them, so pane-2's store
 * carries the previous document's channel, palette, range and warp into the
 * next one — and, because `autoEnabledOnce` rides along, Viewport's one-shot
 * `tryAutoEnable` will not re-fire to pick a default channel for the new
 * document. That is chosen, not overlooked: it is exactly how `design-main`
 * behaved before AND after #5670 (both prior singletons outlived file-open
 * too), and #5670's remit is to make every pane behave like design-main rather
 * than to change what design-main does. Retention is also the useful default
 * for the common case — reopening or reloading a file mid-analysis keeps the
 * channel the user was looking at.
 *
 * The cost is real where documents differ: a retained channel need not exist
 * among the new document's scalar channels. The toolbar renders only channels
 * the mesh actually carries, so a stale selection shows as FEA mode enabled
 * with no matching channel rather than as wrong data. Revisit together with a
 * per-document reset (`FeaModeStore` has no `reset()` today) if that proves
 * confusing in practice; `feaModeStoreRegistry.test.ts` case (f) pins the
 * current contract so the choice cannot drift silently.
 */
export function createFeaModeStoreRegistry(): FeaModeStoreRegistry {
  const stores: Record<string, FeaModeStore> = {};

  function get(viewportId: string): FeaModeStore {
    let store = stores[viewportId];
    if (!store) {
      store = createFeaModeStore();
      stores[viewportId] = store;
    }
    return store;
  }

  return { stores, get };
}
