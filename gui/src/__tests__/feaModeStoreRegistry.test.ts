/**
 * feaModeStoreRegistry — viewportId-keyed store-of-stores (#5670).
 *
 * The registry is the ownership fix for `FeaModeStore` having been modeled as
 * a singleton: two competing instances existed (one component-local to
 * DualViewport, one App-scope for the MultiViewport branch), so every
 * downstream consumer had to hard-code pane-0-ness. `createFeaModeStore()`
 * itself is untouched — only who owns the instances changes.
 *
 * Mirrors feaModeStore.test.ts's `withRoot` boilerplate-removal helper.
 */
import { describe, it, expect } from 'vitest';
import { createRoot } from 'solid-js';
import { createFeaModeStoreRegistry } from '../stores/feaModeStoreRegistry';

/**
 * Run `fn` inside a SolidJS root and dispose immediately after.
 * Removes repetitive createRoot boilerplate from each `it` block.
 */
function withRoot<T>(fn: () => T): T {
  let result!: T;
  createRoot((dispose) => {
    result = fn();
    dispose();
  });
  return result;
}

describe('feaModeStoreRegistry', () => {
  it('(a) get() mints a real FeaModeStore, not a stub', () => {
    withRoot(() => {
      const registry = createFeaModeStoreRegistry();
      const store = registry.get('design-main');
      expect(store).toBeDefined();
      expect(typeof store.setEnabled).toBe('function');
      expect(typeof store.setChannel).toBe('function');
      // Defaults come straight from createFeaModeStore() — the registry wraps
      // the existing factory rather than reimplementing its initial state.
      expect(store.state.enabled).toBe(false);
      expect(store.state.channel).toBe('vonMises');
    });
  });

  it('(b) get() is memoized per viewportId — the same id yields the SAME instance', () => {
    withRoot(() => {
      const registry = createFeaModeStoreRegistry();
      const first = registry.get('design-main');
      const second = registry.get('design-main');
      // Identity, not equality: a fresh instance per call would silently
      // discard whatever FEA state the pane's toolbar had already driven.
      expect(second).toBe(first);
    });
  });

  it('(c) distinct viewportIds yield distinct instances', () => {
    withRoot(() => {
      const registry = createFeaModeStoreRegistry();
      expect(registry.get('design-main')).not.toBe(registry.get('pane-1'));
    });
  });

  it('(d) per-viewport isolation — mutating one pane\'s store does not bleed into another', () => {
    withRoot(() => {
      const registry = createFeaModeStoreRegistry();
      registry.get('pane-1').setChannel('errorIndicator');
      registry.get('pane-1').setEnabled(true);

      expect(registry.get('pane-1').state.channel).toBe('errorIndicator');
      expect(registry.get('pane-1').state.enabled).toBe(true);
      // The whole point of keying: design-main is untouched.
      expect(registry.get('design-main').state.channel).toBe('vonMises');
      expect(registry.get('design-main').state.enabled).toBe(false);
    });
  });

  it('(e) registry.stores is the LIVE backing record, not a snapshot copy', () => {
    withRoot(() => {
      const registry = createFeaModeStoreRegistry();
      // Captured BEFORE the entry exists — this is exactly what
      // registerDebugPanel('feaModes', registry.stores) does at App mount,
      // long before the panes mapArray calls get() for each pane. If `stores`
      // were a copy, entries created later would be invisible through the
      // already-registered reference and ctx.feaModes would read empty.
      const live = registry.stores;
      expect(live['pane-2']).toBeUndefined();

      const created = registry.get('pane-2');

      expect(live['pane-2']).toBeDefined();
      expect(live['pane-2']).toBe(created);
    });
  });
});
