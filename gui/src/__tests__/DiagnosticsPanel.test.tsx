import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { DiagnosticsPanel } from '../panels/DiagnosticsPanel';
import type { DiagnosticInfo } from '../types';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';
import {
  loadDiagnosticsLineWrap,
  loadDiagnosticsPanelSize,
  saveDiagnosticsPanelSize,
} from '../hooks/diagnosticsPanelPersistence';
import styles from '../panels/DiagnosticsPanel.module.css';

// Stub ResizeObserver for jsdom (which doesn't support it).
// The global stub captures the last callback so per-test cases can
// invoke it directly to simulate a resize event.
//
// Design: capture the original and install the stub in beforeAll (same
// lifecycle point), restore in afterAll — prevents leaking this stub into other
// test files that run in the same vitest worker. capturedResizeCallback is
// reset to null in beforeEach so tests cannot observe state left by earlier tests.
let capturedResizeCallback: ResizeObserverCallback | null = null;
let ORIGINAL_RESIZE_OBSERVER: typeof ResizeObserver;
class StubResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor(cb: ResizeObserverCallback) {
    capturedResizeCallback = cb;
  }
}

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
  beforeAll(() => {
    ORIGINAL_RESIZE_OBSERVER = globalThis.ResizeObserver;
    globalThis.ResizeObserver = StubResizeObserver as unknown as typeof ResizeObserver;
  });
  afterAll(() => {
    globalThis.ResizeObserver = ORIGINAL_RESIZE_OBSERVER;
  });
  beforeEach(() => {
    localStorage.clear();
    capturedResizeCallback = null;
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
    const originalInnerWidth = window.innerWidth;
    // jsdom default innerWidth is 1024; set it to 1400 for this test only
    Object.defineProperty(window, 'innerWidth', { value: 1400, writable: true, configurable: true });
    try {
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
    } finally {
      Object.defineProperty(window, 'innerWidth', { value: originalInnerWidth, writable: true, configurable: true });
    }
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

  it('list element has the list CSS class and dialog has no lineWrapOn class by default', () => {
    const diag: DiagnosticEntry = { ...makeDiag('Error', { message: 'oops' }), source: 'compile' };
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[diag]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    // The row's parent is the list container
    const row = screen.getByTestId('diagnostic-row');
    const list = row.parentElement as HTMLElement;
    // Assert exact CSS-module class so substring matches on unrelated class
    // names (e.g. 'playlist') cannot produce a false positive.
    expect(list.classList.contains(styles.list)).toBe(true);

    // Without any click the dialog should NOT carry the lineWrapOn class
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    expect(dialog.className).not.toContain('lineWrapOn');
  });

  it('dialog has no inline overflow style; relies on .dialog class for single vertical scroll axis', () => {
    // The .dialog CSS class provides overflow-y: auto (vertical scroll for the
    // full dialog content) and .list provides overflow-x: auto (horizontal scroll
    // for long messages only). No inline overflow override must exist on the dialog
    // element — having both an inline overflow: auto AND the class-level overflow-y: auto
    // produces nested horizontal scrollbars for wide content.
    const diag: DiagnosticEntry = {
      ...makeDiag('Error', { message: 'x'.repeat(500) }),
      source: 'compile',
    };
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[diag]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
    // All three inline overflow properties must be absent — the .dialog class governs scrolling.
    expect(dialog.style.overflow).toBe('');
    expect(dialog.style.overflowX).toBe('');
    expect(dialog.style.overflowY).toBe('');
  });

  it('does not resize when diagnostics list changes mid-session', () => {
    // Set innerWidth to 1400 so the default for empty diags is ~480px and
    // a 500-char message would compute ~1260px — a clearly visible difference
    // if the memo incorrectly re-runs on diagnostics change.
    const originalInnerWidth = window.innerWidth;
    Object.defineProperty(window, 'innerWidth', { value: 1400, writable: true, configurable: true });
    try {
      const [diagnostics, setDiagnostics] = createSignal<DiagnosticEntry[]>([]);
      render(() => (
        <DiagnosticsPanel
          open={true}
          diagnostics={diagnostics()}
          onClose={vi.fn()}
          onNavigate={vi.fn()}
        />
      ));
      const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;
      const widthAfterMount = dialog.style.width;

      // Simulate a long message arriving mid-session (e.g. from a compile update).
      // The dialogSize memo must NOT re-evaluate — it should only track props.open.
      setDiagnostics([
        { ...makeDiag('Error', { message: 'x'.repeat(500) }), source: 'compile' },
      ]);

      // Width must be unchanged: the default was computed at mount and the
      // mid-session push must not reshape the dialog.
      expect(dialog.style.width).toBe(widthAfterMount);
    } finally {
      Object.defineProperty(window, 'innerWidth', { value: originalInnerWidth, writable: true, configurable: true });
    }
  });

  it('ResizeObserver: skips initial synchronous fire and persists subsequent user-driven resize', () => {
    // beforeEach resets capturedResizeCallback — no manual null assignment needed.
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const dialog = screen.getByTestId('diagnostics-dialog') as HTMLElement;

    // (i) First callback: browser's synchronous initial fire on observe().
    // Must NOT persist — persisting here would freeze the default size forever
    // and bypass computeDefaultDialogSize on every subsequent open.
    Object.defineProperty(dialog, 'offsetWidth', { value: 640, configurable: true });
    Object.defineProperty(dialog, 'offsetHeight', { value: 480, configurable: true });
    expect(capturedResizeCallback).not.toBeNull();
    capturedResizeCallback!([], {} as ResizeObserver);
    expect(loadDiagnosticsPanelSize()).toBeNull();

    // (ii) Second callback: real user-driven resize.  Must persist.
    Object.defineProperty(dialog, 'offsetWidth', { value: 1050, configurable: true });
    Object.defineProperty(dialog, 'offsetHeight', { value: 620, configurable: true });
    capturedResizeCallback!([], {} as ResizeObserver);
    expect(loadDiagnosticsPanelSize()).toEqual({ width: 1050, height: 620 });
  });
});

describe('DiagnosticsPanel header close button', () => {
  beforeAll(() => {
    globalThis.ResizeObserver = StubResizeObserver as unknown as typeof ResizeObserver;
  });
  beforeEach(() => {
    localStorage.clear();
    capturedResizeCallback = null;
  });

  it('when open, diagnostics-header-close element exists', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    expect(screen.getByTestId('diagnostics-header-close')).toBeTruthy();
  });

  it('diagnostics-header-close is in the header region (before the list container in DOM order)', () => {
    const diag: DiagnosticEntry = { ...makeDiag('Error', { message: 'some error' }), source: 'compile' };
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[diag]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const headerClose = screen.getByTestId('diagnostics-header-close');
    const list = document.querySelector(`.${styles.list}`) as HTMLElement;

    expect(headerClose).toBeTruthy();
    expect(list).toBeTruthy();

    // The header close button must NOT be inside the .list container
    expect(list.contains(headerClose)).toBe(false);

    // The header close button must come before the .list in DOM order
    const position = headerClose.compareDocumentPosition(list);
    // Node.DOCUMENT_POSITION_FOLLOWING = 4 — list comes after headerClose
    expect(position & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('clicking diagnostics-header-close invokes onClose exactly once', () => {
    const onClose = vi.fn();
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={onClose}
        onNavigate={vi.fn()}
      />
    ));
    fireEvent.click(screen.getByTestId('diagnostics-header-close'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('diagnostics-header-close has aria-label matching /close/i', () => {
    render(() => (
      <DiagnosticsPanel
        open={true}
        diagnostics={[]}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    const btn = screen.getByTestId('diagnostics-header-close');
    const label = btn.getAttribute('aria-label') ?? '';
    expect(label).toMatch(/close/i);
  });
});
