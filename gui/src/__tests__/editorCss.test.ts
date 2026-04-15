import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const CSS_PATH = join(__dirname, '../editor/Editor.module.css');
const css = readFileSync(CSS_PATH, 'utf-8');

describe('Editor.module.css hover tooltip', () => {
  it('uses --reify-text CSS variable for color instead of hardcoded hex', () => {
    expect(css).toContain('var(--reify-text');
    expect(css).not.toContain('color: #e0e0e0');
  });

  it('uses --reify-border CSS variable for border instead of hardcoded #444', () => {
    expect(css).toContain('var(--reify-border');
    expect(css).not.toContain('#444');
  });

  it('uses --reify-surface CSS variable for background instead of hardcoded color', () => {
    expect(css).toContain('var(--reify-surface');
  });
});
