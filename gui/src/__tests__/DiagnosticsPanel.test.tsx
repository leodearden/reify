import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { DiagnosticsPanel } from '../panels/DiagnosticsPanel';
import type { DiagnosticInfo } from '../types';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';

function makeDiag(severity: 'Error' | 'Warning' | 'Info', overrides: Partial<DiagnosticInfo> = {}): DiagnosticInfo {
  return {
    file_path: 'test.ri',
    line: 5,
    column: 3,
    end_line: 5,
    end_column: 10,
    severity,
    message: 'test message',
    code: null,
    ...overrides,
  };
}

describe('DiagnosticsPanel', () => {
  it('renders nothing when open=false', () => {
    render(() => (
      <DiagnosticsPanel
        open={false}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    expect(document.querySelector('[data-testid="diagnostics-panel"]')).toBeNull();
  });

  it('renders panel with data-testid="diagnostics-panel" when open=true', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
  });

  it('renders heading with data-testid="panel-title-diagnostics" containing "Diagnostics"', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const title = screen.getByTestId('panel-title-diagnostics');
    expect(title).toBeTruthy();
    expect(title.textContent).toMatch(/diagnostics/i);
  });

  it('shows empty-state message when diagnostics is empty', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    // Some empty-state text should be visible
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).toMatch(/no diagnostics/i);
  });

  it('renders one row per diagnostic entry', () => {
    const diags = [
      makeDiag('Error', { file_path: 'main.ri', line: 10, message: 'import failed' }),
      makeDiag('Warning', { file_path: 'helper.ri', line: 3, message: "unknown port type 'Foo'" }),
    ];
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={diags}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const panel = screen.getByTestId('diagnostics-panel');
    const text = panel.textContent ?? '';
    // Both messages appear
    expect(text).toContain('import failed');
    expect(text).toContain("unknown port type 'Foo'");
    // Both severities appear
    expect(text).toMatch(/error/i);
    expect(text).toMatch(/warning/i);
    // Both file:line references appear
    expect(text).toContain('main.ri');
    expect(text).toContain('helper.ri');
    expect(text).toContain('10');
    expect(text).toContain('3');
  });

  it('clicking a diagnostic row invokes onNavigate with that diagnostic', () => {
    const diag = makeDiag('Warning', { file_path: 'helper.ri', line: 3, message: "unknown port type 'Foo'" });
    const onNavigate = vi.fn();
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[diag]}
        onClose={vi.fn()}
        onNavigate={onNavigate}
      />
    ));
    // Find the row by its message text
    const row = screen.getByText(/unknown port type/i).closest('[data-testid="diagnostic-row"]') as HTMLElement
      ?? screen.getByText(/unknown port type/i).parentElement as HTMLElement;
    fireEvent.click(row);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(diag);
  });

  it('Escape key invokes onClose (fired on document.body, matching real user behavior)', () => {
    const onClose = vi.fn();
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={onClose}
        onNavigate={vi.fn()}
      />
    ));
    // Fire on document.body to exercise the document-level listener; this
    // matches real user behavior where Escape is pressed without the overlay
    // div having focus (which it never does on open).
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('clicking the overlay (outside the dialog) invokes onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={onClose}
        onNavigate={vi.fn()}
      />
    ));
    const panel = screen.getByTestId('diagnostics-panel');
    // Click the overlay itself (the outermost element), not an inner element
    fireEvent.click(panel);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('renders a source chip per row with correct source label', () => {
    const compileEntry: DiagnosticEntry = { ...makeDiag('Error', { message: 'compile error' }), source: 'compile' };
    const tessEntry: DiagnosticEntry = { ...makeDiag('Warning', { message: 'tess warning' }), source: 'tessellation' };
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[compileEntry, tessEntry]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    // Find all source chips
    const chips = document.querySelectorAll('[data-testid="diagnostic-source-chip"]');
    expect(chips.length).toBe(2);
    const chipTexts = Array.from(chips).map((c) => c.textContent);
    expect(chipTexts).toContain('compile');
    expect(chipTexts).toContain('tessellation');
  });
});
