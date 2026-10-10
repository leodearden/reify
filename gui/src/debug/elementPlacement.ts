/**
 * Where an element sits and whether a pointer aimed at its centre reaches it.
 *
 * DOM only. `bounds` is the element's unclipped layout rect, `visible` its own
 * computed display/visibility plus a non-zero width, and `hitTestable` asks the
 * same `document.elementFromPoint` question the coordinate tools (click_at,
 * hover, drag) ask. Contract: docs/debug-mcp-contract.md §3 "Clipping and
 * hit-testability".
 */

export interface ElementPlacement {
  readonly bounds: { x: number; y: number; width: number; height: number };
  readonly visible: boolean;
  readonly hitTestable: boolean;
}

export function describePlacement(el: Element): ElementPlacement {
  const rect = el.getBoundingClientRect();
  const style = window.getComputedStyle(el);
  const visible = style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0;
  return {
    bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    visible,
    hitTestable: visible && centreHitLandsOn(el, rect),
  };
}

/** A hit on a descendant counts: the coordinate tools' events bubble up to `el`. */
function centreHitLandsOn(el: Element, rect: DOMRect): boolean {
  const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
  return hit !== null && el.contains(hit);
}
