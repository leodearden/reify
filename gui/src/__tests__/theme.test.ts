import { describe, it, expect } from 'vitest';
import { THEME_TOKENS, applyTheme } from '../theme';

describe('THEME_TOKENS', () => {
  it('has all required color keys', () => {
    const requiredKeys = [
      'background', 'surface', 'text', 'accent', 'border',
      'error', 'warning', 'success', 'textMuted', 'fontMono',
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

  it('all color tokens are valid hex', () => {
    for (const [key, value] of Object.entries(THEME_TOKENS)) {
      if (key === 'fontMono') continue;
      expect(value, `${key} should be valid hex color`).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('applyTheme is callable function', () => {
    expect(typeof applyTheme).toBe('function');
  });
});
