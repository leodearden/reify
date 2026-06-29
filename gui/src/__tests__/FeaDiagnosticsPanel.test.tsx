/**
 * Tests for FeaDiagnosticsPanel (#2966, step-13/14).
 *
 * Asserts:
 *   - One row per diagnostic showing formatted label/detail (via feaDiagnosticRows)
 *   - Clicking a row invokes props.onFocusDiagnostic with that diagnostic
 *   - Enter/Space keydown on a row invokes props.onFocusDiagnostic
 *   - Empty list renders empty/placeholder state (no rows)
 *
 * RED until gui/src/panels/FeaDiagnosticsPanel.tsx is created (step-14).
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import type { FeaDiagnosticInfo } from '../types';

// @ts-expect-error — module absent until step-14
import { FeaDiagnosticsPanel } from '../panels/FeaDiagnosticsPanel';

// ─── fixtures ────────────────────────────────────────────────────────────────

const unconstrainedDiag: FeaDiagnosticInfo = {
  kind: 'Unconstrained',
  rigid_body_modes: ['TranslationX', 'TranslationY', 'TranslationZ'],
};

const problemElementsDiag: FeaDiagnosticInfo = {
  kind: 'ProblemElements',
  ids: [5, 12, 99],
};

const unresolvedSelectorDiag: FeaDiagnosticInfo = {
  kind: 'UnresolvedSelector',
  selector_path: 'Body.fea_load',
};

// ─── render helper ───────────────────────────────────────────────────────────

function renderPanel(opts: {
  diagnostics?: FeaDiagnosticInfo[];
  onFocusDiagnostic?: (d: FeaDiagnosticInfo) => void;
} = {}) {
  const {
    diagnostics = [],
    onFocusDiagnostic = vi.fn(),
  } = opts;
  return render(() => (
    <FeaDiagnosticsPanel
      diagnostics={diagnostics}
      onFocusDiagnostic={onFocusDiagnostic}
    />
  ));
}

// ─── empty state ─────────────────────────────────────────────────────────────

describe('FeaDiagnosticsPanel — empty state', () => {
  it('renders the panel container when diagnostics is empty', () => {
    renderPanel({ diagnostics: [] });
    expect(screen.getByTestId('fea-diagnostics-panel')).toBeTruthy();
  });

  it('no rows rendered when diagnostics is empty', () => {
    renderPanel({ diagnostics: [] });
    expect(document.querySelectorAll('[data-testid="fea-diagnostic-row"]').length).toBe(0);
  });

  it('shows an empty/placeholder state message when diagnostics is empty', () => {
    renderPanel({ diagnostics: [] });
    const panel = screen.getByTestId('fea-diagnostics-panel');
    // Should show some placeholder text — no strict wording requirement
    expect(panel.textContent?.trim().length).toBeGreaterThan(0);
  });
});

// ─── row rendering ───────────────────────────────────────────────────────────

describe('FeaDiagnosticsPanel — row rendering', () => {
  it('renders one row per diagnostic', () => {
    renderPanel({ diagnostics: [unconstrainedDiag, problemElementsDiag, unresolvedSelectorDiag] });
    const rows = document.querySelectorAll('[data-testid="fea-diagnostic-row"]');
    expect(rows.length).toBe(3);
  });

  it('Unconstrained row shows a label containing "unconstrained" or "rigid"', () => {
    renderPanel({ diagnostics: [unconstrainedDiag] });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    expect(row.textContent?.toLowerCase()).toMatch(/unconstrained|rigid/);
  });

  it('Unconstrained row detail contains the DOF mode names', () => {
    renderPanel({ diagnostics: [unconstrainedDiag] });
    const panel = screen.getByTestId('fea-diagnostics-panel');
    const text = panel.textContent ?? '';
    expect(text).toContain('TranslationX');
    expect(text).toContain('TranslationY');
    expect(text).toContain('TranslationZ');
  });

  it('ProblemElements row shows the element count', () => {
    renderPanel({ diagnostics: [problemElementsDiag] });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    expect(row.textContent).toContain('3');
  });

  it('ProblemElements row detail contains element ids', () => {
    renderPanel({ diagnostics: [problemElementsDiag] });
    const panel = screen.getByTestId('fea-diagnostics-panel');
    const text = panel.textContent ?? '';
    expect(text).toContain('5');
    expect(text).toContain('12');
    expect(text).toContain('99');
  });

  it('UnresolvedSelector row contains the selector_path', () => {
    renderPanel({ diagnostics: [unresolvedSelectorDiag] });
    const panel = screen.getByTestId('fea-diagnostics-panel');
    expect(panel.textContent).toContain('Body.fea_load');
  });

  it('preserves input order across mixed variants', () => {
    renderPanel({ diagnostics: [unconstrainedDiag, problemElementsDiag, unresolvedSelectorDiag] });
    const rows = document.querySelectorAll('[data-testid="fea-diagnostic-row"]');
    expect(rows.length).toBe(3);
    // First row should be about Unconstrained
    expect(rows[0].textContent?.toLowerCase()).toMatch(/unconstrained|rigid/);
    // Third row should contain the selector path
    expect(rows[2].textContent).toContain('Body.fea_load');
  });
});

// ─── click interaction ────────────────────────────────────────────────────────

describe('FeaDiagnosticsPanel — click interaction', () => {
  it('clicking a row invokes onFocusDiagnostic with that diagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [unconstrainedDiag], onFocusDiagnostic });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    fireEvent.click(row);
    expect(onFocusDiagnostic).toHaveBeenCalledTimes(1);
    expect(onFocusDiagnostic).toHaveBeenCalledWith(unconstrainedDiag);
  });

  it('clicking ProblemElements row invokes onFocusDiagnostic with that diagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [problemElementsDiag], onFocusDiagnostic });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    fireEvent.click(row);
    expect(onFocusDiagnostic).toHaveBeenCalledTimes(1);
    expect(onFocusDiagnostic).toHaveBeenCalledWith(problemElementsDiag);
  });

  it('clicking the correct row in a multi-row panel invokes with the correct diagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [unconstrainedDiag, problemElementsDiag], onFocusDiagnostic });
    const rows = document.querySelectorAll('[data-testid="fea-diagnostic-row"]');
    fireEvent.click(rows[1] as HTMLElement);
    expect(onFocusDiagnostic).toHaveBeenCalledTimes(1);
    expect(onFocusDiagnostic).toHaveBeenCalledWith(problemElementsDiag);
  });
});

// ─── keyboard interaction ─────────────────────────────────────────────────────

describe('FeaDiagnosticsPanel — keyboard interaction', () => {
  it('Enter keydown on a row invokes onFocusDiagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [unconstrainedDiag], onFocusDiagnostic });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(onFocusDiagnostic).toHaveBeenCalledTimes(1);
    expect(onFocusDiagnostic).toHaveBeenCalledWith(unconstrainedDiag);
  });

  it('Space keydown on a row invokes onFocusDiagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [unconstrainedDiag], onFocusDiagnostic });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    fireEvent.keyDown(row, { key: ' ' });
    expect(onFocusDiagnostic).toHaveBeenCalledTimes(1);
    expect(onFocusDiagnostic).toHaveBeenCalledWith(unconstrainedDiag);
  });

  it('row has role="button" for accessibility', () => {
    renderPanel({ diagnostics: [unconstrainedDiag] });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    expect(row.getAttribute('role')).toBe('button');
  });

  it('row has tabindex="0" for keyboard focus', () => {
    renderPanel({ diagnostics: [unconstrainedDiag] });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    expect(row.getAttribute('tabindex')).toBe('0');
  });

  it('other keys (e.g. Escape) do NOT invoke onFocusDiagnostic', () => {
    const onFocusDiagnostic = vi.fn();
    renderPanel({ diagnostics: [unconstrainedDiag], onFocusDiagnostic });
    const row = document.querySelector('[data-testid="fea-diagnostic-row"]') as HTMLElement;
    fireEvent.keyDown(row, { key: 'Escape' });
    expect(onFocusDiagnostic).not.toHaveBeenCalled();
  });
});
