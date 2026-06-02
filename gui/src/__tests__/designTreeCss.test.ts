import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const CSS_PATH = join(__dirname, '../panels/DesignTree.module.css');
const css = readFileSync(CSS_PATH, 'utf-8');

describe('DesignTree.module.css scrollbar contract', () => {
  // (1) REGRESSION GUARD — protects #3394: the bare .treeScroll rule must
  // retain `scrollbar-gutter: stable` (reserves a gutter for classic
  // scrollbars and prevents layout shift on overflow toggle).
  // The pattern uses `\s*\{` instead of `::-webkit-scrollbar` to match only
  // the plain `.treeScroll` selector, not the pseudo-element selectors.
  it('regression guard: .treeScroll still has scrollbar-gutter: stable (#3394)', () => {
    expect(css).toMatch(/\.treeScroll\s*\{[^}]*scrollbar-gutter:\s*stable/);
  });

  // (2) RED DRIVER — a `.treeScroll::-webkit-scrollbar` rule must declare an
  // explicit non-zero px width. This converts the WebKitGTK overlay scrollbar
  // into a classic, space-reserving scrollbar that cannot float over the eye-icons.
  it('declares .treeScroll::-webkit-scrollbar with an explicit non-zero px width', () => {
    const match = css.match(
      /\.treeScroll::-webkit-scrollbar\s*\{[^}]*width:\s*(\d+(?:\.\d+)?)px/
    );
    expect(match, 'expected .treeScroll::-webkit-scrollbar { width: <N>px } rule').not.toBeNull();
    const widthPx = parseFloat(match![1]);
    expect(widthPx, 'scrollbar width must be > 0 to force non-overlay rendering').toBeGreaterThan(0);
  });

  // (3) RED DRIVER — a `.treeScroll::-webkit-scrollbar-thumb` rule must set a
  // `background` using a `var(--reify-*` design token (so the visible scrollbar
  // follows the repo's theming convention rather than using a hardcoded colour).
  it('declares .treeScroll::-webkit-scrollbar-thumb background with a --reify-* token', () => {
    expect(css).toMatch(
      /\.treeScroll::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*var\(--reify-/
    );
  });
});
