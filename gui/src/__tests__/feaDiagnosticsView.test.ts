/**
 * Tests for feaDiagnosticsView.ts (#2966, step-11/12).
 *
 * feaDiagnosticRows() is a pure view-model formatter that maps FeaDiagnosticInfo
 * variants to display rows for FeaDiagnosticsPanel.
 *
 * RED until gui/src/panels/feaDiagnosticsView.ts is implemented (step-12).
 */

import { describe, it, expect } from 'vitest';
import type { FeaDiagnosticInfo } from '../types';

import { feaDiagnosticRows } from '../panels/feaDiagnosticsView';
import type { FeaDiagnosticRow } from '../panels/feaDiagnosticsView';

// ─── fixtures ────────────────────────────────────────────────────────────────

const unconstrainedFull: FeaDiagnosticInfo = {
  kind: 'Unconstrained',
  rigid_body_modes: ['TranslationX', 'TranslationY', 'TranslationZ', 'RotationX', 'RotationY', 'RotationZ'],
};

const unconstrainedPartial: FeaDiagnosticInfo = {
  kind: 'Unconstrained',
  rigid_body_modes: ['TranslationX', 'RotationZ'],
};

const problemElements: FeaDiagnosticInfo = {
  kind: 'ProblemElements',
  ids: [5, 12, 99],
};

const problemElementsSingle: FeaDiagnosticInfo = {
  kind: 'ProblemElements',
  ids: [7],
};

const unresolvedSelector: FeaDiagnosticInfo = {
  kind: 'UnresolvedSelector',
  selector_path: 'Body.fea_load',
};

// ─── empty input ─────────────────────────────────────────────────────────────

describe('feaDiagnosticRows', () => {
  it('returns [] for an empty diagnostics list', () => {
    expect(feaDiagnosticRows([])).toEqual([]);
  });

  // ── order preservation ─────────────────────────────────────────────────────

  it('preserves input order', () => {
    const rows: FeaDiagnosticRow[] = feaDiagnosticRows([
      unconstrainedFull,
      problemElements,
      unresolvedSelector,
    ]);
    expect(rows).toHaveLength(3);
    expect(rows[0].kind).toBe('Unconstrained');
    expect(rows[1].kind).toBe('ProblemElements');
    expect(rows[2].kind).toBe('UnresolvedSelector');
  });

  // ── Unconstrained ──────────────────────────────────────────────────────────

  describe('Unconstrained variant', () => {
    it('kind is "Unconstrained"', () => {
      const [row] = feaDiagnosticRows([unconstrainedFull]);
      expect(row.kind).toBe('Unconstrained');
    });

    it('label describes unconstrained body / rigid-body mode', () => {
      const [row] = feaDiagnosticRows([unconstrainedFull]);
      // Label should convey that this is an unconstrained body
      expect(row.label).toBeTruthy();
      expect(row.label.toLowerCase()).toMatch(/unconstrained|rigid.?body/);
    });

    it('detail contains each rigid-body mode name', () => {
      const [row] = feaDiagnosticRows([unconstrainedFull]);
      expect(row.detail).toContain('TranslationX');
      expect(row.detail).toContain('TranslationY');
      expect(row.detail).toContain('TranslationZ');
      expect(row.detail).toContain('RotationX');
      expect(row.detail).toContain('RotationY');
      expect(row.detail).toContain('RotationZ');
    });

    it('detail contains the partial mode list when fewer than 6 modes', () => {
      const [row] = feaDiagnosticRows([unconstrainedPartial]);
      expect(row.detail).toContain('TranslationX');
      expect(row.detail).toContain('RotationZ');
      // Modes not in the list should NOT appear
      expect(row.detail).not.toContain('TranslationY');
    });
  });

  // ── ProblemElements ────────────────────────────────────────────────────────

  describe('ProblemElements variant', () => {
    it('kind is "ProblemElements"', () => {
      const [row] = feaDiagnosticRows([problemElements]);
      expect(row.kind).toBe('ProblemElements');
    });

    it('label includes the element count', () => {
      const [row] = feaDiagnosticRows([problemElements]);
      // 3 elements — label should mention "3"
      expect(row.label).toContain('3');
    });

    it('singular label for 1 element', () => {
      const [row] = feaDiagnosticRows([problemElementsSingle]);
      expect(row.label).toContain('1');
    });

    it('detail lists each element id', () => {
      const [row] = feaDiagnosticRows([problemElements]);
      expect(row.detail).toContain('5');
      expect(row.detail).toContain('12');
      expect(row.detail).toContain('99');
    });
  });

  // ── UnresolvedSelector ─────────────────────────────────────────────────────

  describe('UnresolvedSelector variant', () => {
    it('kind is "UnresolvedSelector"', () => {
      const [row] = feaDiagnosticRows([unresolvedSelector]);
      expect(row.kind).toBe('UnresolvedSelector');
    });

    it('label or detail contains the selector_path', () => {
      const [row] = feaDiagnosticRows([unresolvedSelector]);
      const combined = row.label + ' ' + row.detail;
      expect(combined).toContain('Body.fea_load');
    });
  });

  // ── multiple diagnostics ───────────────────────────────────────────────────

  it('maps each diagnostic independently in a mixed list', () => {
    const rows: FeaDiagnosticRow[] = feaDiagnosticRows([
      unconstrainedFull,
      problemElements,
    ]);
    expect(rows).toHaveLength(2);
    expect(rows[0].kind).toBe('Unconstrained');
    expect(rows[1].kind).toBe('ProblemElements');
  });
});
