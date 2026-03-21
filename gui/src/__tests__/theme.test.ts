import { describe, it, expect } from 'vitest';
import { THEME_TOKENS, applyTheme } from '../theme';

describe('THEME_TOKENS', () => {
  it('has all required color keys', () => {
    const requiredKeys = [
      'background', 'surface', 'text', 'accent', 'border',
      'error', 'warning', 'success', 'textMuted', 'fontMono',
      'surface0', 'surface1', 'surface2', 'subtext', 'overlay0', 'green', 'red',
    ];
    for (const key of requiredKeys) {
      expect(THEME_TOKENS[key], `missing key: ${key}`).toBeTruthy();
    }
  });

  it('has correct Catppuccin Mocha values', () => {
    expect(THEME_TOKENS.background).toBe('#1e1e2e');
    expect(THEME_TOKENS.surface).toBe('#2a2a3a');
    expect(THEME_TOKENS.text).toBe('#cdd6f4');
    expect(THEME_TOKENS.accent).toBe('#89b4fa');
    expect(THEME_TOKENS.border).toBe('#45475a');
  });

  it('has correct Catppuccin Mocha values for surface/accent tokens', () => {
    expect(THEME_TOKENS.surface0).toBe('#313244');
    expect(THEME_TOKENS.surface1).toBe('#45475a');
    expect(THEME_TOKENS.surface2).toBe('#585b70');
    expect(THEME_TOKENS.subtext).toBe('#a6adc8');
    expect(THEME_TOKENS.overlay0).toBe('#6c7086');
    expect(THEME_TOKENS.green).toBe('#a6e3a1');
    expect(THEME_TOKENS.red).toBe('#f38ba8');
  });

  it('has correct spacing token values', () => {
    expect(THEME_TOKENS.spaceXs).toBe('2px');
    expect(THEME_TOKENS.spaceSm).toBe('4px');
    expect(THEME_TOKENS.spaceMd).toBe('8px');
    expect(THEME_TOKENS.spaceLg).toBe('12px');
    expect(THEME_TOKENS.spaceXl).toBe('16px');
    expect(THEME_TOKENS.space2xl).toBe('20px');
    expect(THEME_TOKENS.space3xl).toBe('24px');
  });

  it('has correct radius token values', () => {
    expect(THEME_TOKENS.radiusSm).toBe('2px');
    expect(THEME_TOKENS.radiusMd).toBe('4px');
    expect(THEME_TOKENS.radiusLg).toBe('8px');
  });

  it('all color tokens are valid hex', () => {
    for (const [key, value] of Object.entries(THEME_TOKENS)) {
      if (key.startsWith('font') || key.startsWith('space') || key.startsWith('radius')) continue;
      expect(value, `${key} should be valid hex color`).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('applyTheme is callable function', () => {
    expect(typeof applyTheme).toBe('function');
  });

  it('every CSS variable reference has a THEME_TOKENS entry', () => {
    // Hardcoded list of all unique --reify-* variable names found in CSS files.
    // When adding a new --reify-* CSS variable reference, add it here too.
    const cssVariableNames = [
      'background', 'surface', 'surface-hover', 'text', 'text-secondary',
      'text-muted', 'accent', 'accent-hover', 'border', 'error', 'warning',
      'success', 'editor-bg', 'viewport-bg', 'font-mono',
      'surface0', 'surface1', 'surface2', 'subtext', 'overlay0', 'green', 'red',
    ];

    function kebabToCamel(str: string): string {
      return str.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
    }

    for (const name of cssVariableNames) {
      const camelKey = kebabToCamel(name);
      expect(
        THEME_TOKENS[camelKey],
        `CSS uses --reify-${name} but THEME_TOKENS is missing key '${camelKey}'`,
      ).toBeTruthy();
    }
  });

  it('applyTheme sets all tokens as CSS custom properties', () => {
    applyTheme();
    const style = document.documentElement.style;

    function camelToKebab(str: string): string {
      return str.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
    }

    for (const [key, value] of Object.entries(THEME_TOKENS)) {
      const cssVar = `--reify-${camelToKebab(key)}`;
      expect(
        style.getPropertyValue(cssVar),
        `CSS variable ${cssVar} should be set to ${value}`,
      ).toBe(value);
    }
  });

  it('applyTheme works synchronously without requiring a component mount', () => {
    // Clear all CSS variables first
    const style = document.documentElement.style;
    for (let i = style.length - 1; i >= 0; i--) {
      const prop = style.item(i);
      if (prop.startsWith('--reify-')) {
        style.removeProperty(prop);
      }
    }

    // Verify cleared
    expect(style.getPropertyValue('--reify-background')).toBe('');

    // Call applyTheme synchronously — CSS variables must be immediately available
    applyTheme();

    // Verify synchronously available (no await, no onMount needed)
    expect(style.getPropertyValue('--reify-background')).toBe('#1e1e2e');
    expect(style.getPropertyValue('--reify-accent')).toBe('#89b4fa');
    expect(style.getPropertyValue('--reify-surface0')).toBe('#313244');
  });
});
