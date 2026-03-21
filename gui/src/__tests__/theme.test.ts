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
