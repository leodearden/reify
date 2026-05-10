// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  DIAGNOSTICS_LINE_WRAP_KEY,
  DIAGNOSTICS_PANEL_SIZE_KEY,
  loadDiagnosticsLineWrap,
  saveDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  saveDiagnosticsPanelSize,
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
