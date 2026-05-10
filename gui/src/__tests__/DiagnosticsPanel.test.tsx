import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { DiagnosticsPanel } from '../panels/DiagnosticsPanel';
import type { DiagnosticInfo } from '../types';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';
import {
  loadDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  saveDiagnosticsPanelSize,
} from '../hooks/useDiagnosticsPanelPersistence';

// Stub ResizeObserver for jsdom (which doesn't support it).
// The global stub captures the last callback so per-test cases can
// invoke it directly to simulate a resize event.
let capturedResizeCallback: ResizeObserverCallback | null = null;
globalThis.ResizeObserver = class ResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor(cb: ResizeObserverCallback) {
    capturedResizeCallback = cb;
  }
};

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
  beforeEach(() => {
    localStorage.clear();
  });

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
    const diags: DiagnosticEntry[] = [
      { ...makeDiag('Error', { file_path: 'main.ri', line: 10, message: 'import failed' }), source: 'compile' },
      { ...makeDiag('Warning', { file_path: 'helper.ri', line: 3, message: "unknown port type 'Foo'" }), source: 'tessellation' },
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
    const diag: DiagnosticEntry = { ...makeDiag('Warning', { file_path: 'helper.ri', line: 3, message: "unknown port type 'Foo'" }), source: 'compile' };
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

  it('renders a line-wrap checkbox by default', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const checkbox = screen.getByTestId('diagnostics-line-wrap-toggle') as HTMLInputElement;
    expect(checkbox).toBeTruthy();
    expect(checkbox.checked).toBe(false);
  });

  it('toggling the line-wrap checkbox adds lineWrapOn class to the dialog and persists to localStorage', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const checkbox = screen.getByTestId('diagnostics-line-wrap-toggle') as HTMLInputElement;
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;

    // Initially no lineWrapOn class
    expect(dialog.className).not.toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBeNull();

    // Click to enable wrap
    fireEvent.click(checkbox);
    expect(dialog.className).toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBe(true);

    // Click again to disable
    fireEvent.click(checkbox);
    expect(dialog.className).not.toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBe(false);
  });

  it('applies persisted size as inline style on mount', () => {
    saveDiagnosticsPanelSize({ width: 1100, height: 640 });
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    expect(dialog.style.width).toBe('1100px');
    expect(dialog.style.height).toBe('640px');
  });

  it('uses computeDefaultDialogSize when no persisted size and longest message is wide', () => {
    // jsdom default innerWidth is 1024; set it to 1400
    Object.defineProperty(window, 'innerWidth', { value: 1400, writable: true, configurable: true });
    const longMessage = 'x'.repeat(500);
    const diags: DiagnosticEntry[] = [
      { ...makeDiag('Error', { message: longMessage }), source: 'compile' },
    ];
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={diags}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    const width = parseInt(dialog.style.width);
    expect(width).toBeGreaterThan(720);
    expect(width).toBeLessThanOrEqual(0.9 * 1400); // 1260
  });

  it('dialog has inline resize: both', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    expect(dialog.style.resize).toBe('both');
  });

  it('ResizeObserver callback persists current size to localStorage', () => {
    capturedResizeCallback = null;
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    // Stub offsetWidth/offsetHeight (JSDOM always reports 0 for layout)
    Object.defineProperty(dialog, 'offsetWidth', { value: 950, configurable: true });
    Object.defineProperty(dialog, 'offsetHeight', { value: 580, configurable: true });
    // Invoke the captured callback to simulate a resize event
    expect(capturedResizeCallback).not.toBeNull();
    capturedResizeCallback!([], {} as ResizeObserver);
    expect(loadDiagnosticsPanelSize()).toEqual({ width: 950, height: 580 });
  });
});
