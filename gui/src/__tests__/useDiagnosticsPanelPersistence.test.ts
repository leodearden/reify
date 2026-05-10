// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  DIAGNOSTICS_LINE_WRAP_KEY,
  DIAGNOSTICS_PANEL_SIZE_KEY,
  loadDiagnosticsLineWrap,
  saveDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  saveDiagnosticsPanelSize,
  computeDefaultDialogSize,
} from '../hooks/useDiagnosticsPanelPersistence';

describe('useDiagnosticsPanelPersistence', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  describe('loadDiagnosticsLineWrap', () => {
    it('returns null when localStorage is empty', () => {
      expect(loadDiagnosticsLineWrap()).toBeNull();
    });

    it('returns true when true is stored', () => {
      localStorage.setItem(DIAGNOSTICS_LINE_WRAP_KEY, JSON.stringify(true));
      expect(loadDiagnosticsLineWrap()).toBe(true);
    });

    it('returns false when false is stored', () => {
      localStorage.setItem(DIAGNOSTICS_LINE_WRAP_KEY, JSON.stringify(false));
      expect(loadDiagnosticsLineWrap()).toBe(false);
    });

    it('returns null for non-boolean JSON', () => {
      localStorage.setItem(DIAGNOSTICS_LINE_WRAP_KEY, JSON.stringify('yes'));
      expect(loadDiagnosticsLineWrap()).toBeNull();
    });

    it('returns null for corrupted JSON', () => {
      localStorage.setItem(DIAGNOSTICS_LINE_WRAP_KEY, '{broken json!!!');
      expect(loadDiagnosticsLineWrap()).toBeNull();
    });
  });

  describe('saveDiagnosticsLineWrap', () => {
    it('round-trips true', () => {
      saveDiagnosticsLineWrap(true);
      expect(loadDiagnosticsLineWrap()).toBe(true);
    });

    it('round-trips false', () => {
      saveDiagnosticsLineWrap(false);
      expect(loadDiagnosticsLineWrap()).toBe(false);
    });
  });

  describe('loadDiagnosticsPanelSize', () => {
    it('returns null when localStorage is empty', () => {
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });

    it('returns {width, height} when valid shape is stored', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify({ width: 900, height: 500 }));
      expect(loadDiagnosticsPanelSize()).toEqual({ width: 900, height: 500 });
    });

    it('returns null when width field is missing', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify({ height: 500 }));
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });

    it('returns null when height field is missing', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify({ width: 900 }));
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });

    it('returns null when width is not a number', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify({ width: '900', height: 500 }));
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });

    it('returns null when height is not a number', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, JSON.stringify({ width: 900, height: '500' }));
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });

    it('returns null for corrupted JSON', () => {
      localStorage.setItem(DIAGNOSTICS_PANEL_SIZE_KEY, '{broken json!!!');
      expect(loadDiagnosticsPanelSize()).toBeNull();
    });
  });

  describe('saveDiagnosticsPanelSize', () => {
    it('round-trips {width: 900, height: 500}', () => {
      saveDiagnosticsPanelSize({ width: 900, height: 500 });
      expect(loadDiagnosticsPanelSize()).toEqual({ width: 900, height: 500 });
    });
  });
});

describe('computeDefaultDialogSize', () => {
  it('returns at least minWidth for empty input (longestMessageChars: 0)', () => {
    const result = computeDefaultDialogSize({
      longestMessageChars: 0,
      viewportWidth: 1000,
      viewportHeight: 800,
    });
    expect(result.width).toBeGreaterThanOrEqual(480); // default minWidth
  });

  it('caps width at viewportWidth * maxFractionOfViewport for very long messages', () => {
    // 5000 chars × 8px + 80px chrome = 40080px, way more than 0.9 × 1000 = 900px
    const result = computeDefaultDialogSize({
      longestMessageChars: 5000,
      viewportWidth: 1000,
      viewportHeight: 800,
      monoCharPx: 8,
      chromePx: 80,
      maxFractionOfViewport: 0.9,
    });
    expect(result.width).toBe(900);
  });

  it('returns content-based width between min and cap for moderate message', () => {
    // 200 chars × 8px + 80px chrome = 1680px, viewport = 2000, cap = 0.9 × 2000 = 1800
    // 1680 is between minWidth (480) and 1800, so width = 1680
    const result = computeDefaultDialogSize({
      longestMessageChars: 200,
      viewportWidth: 2000,
      viewportHeight: 800,
      monoCharPx: 8,
      chromePx: 80,
      minWidth: 480,
      maxFractionOfViewport: 0.9,
    });
    expect(result.width).toBe(1680);
  });

  it('returns sensible default height capped at min(viewportHeight * 0.8, 600)', () => {
    // Large viewport: viewportHeight × 0.8 = 1200, but cap is 600 → height = 600
    const large = computeDefaultDialogSize({
      longestMessageChars: 0,
      viewportWidth: 1000,
      viewportHeight: 1000,
    });
    expect(large.height).toBe(600);

    // Small viewport: viewportHeight × 0.8 = 320, which is < 600 → height = 320
    const small = computeDefaultDialogSize({
      longestMessageChars: 0,
      viewportWidth: 1000,
      viewportHeight: 400,
    });
    expect(small.height).toBe(320);
  });
});
