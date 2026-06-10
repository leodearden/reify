import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { DiagnosticsPanel } from '../panels/DiagnosticsPanel';
import type { DiagnosticInfo } from '../types';
import type { DiagnosticEntry } from '../panels/DiagnosticsPanel';
import { loadDiagnosticsLineWrap } from '../hooks/diagnosticsPanelPersistence';
import styles from '../panels/DiagnosticsPanel.module.css';

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

/** Minimal docked-panel render helper */
function renderDocked(opts: {
  collapsed?: boolean;
  height?: number;
  diagnostics?: DiagnosticEntry[];
  onToggleCollapsed?: () => void;
  onNavigate?: (d: DiagnosticEntry) => void;
} = {}) {
  const {
    collapsed = false,
    height = 160,
    diagnostics = [],
    onToggleCollapsed = vi.fn(),
    onNavigate = vi.fn(),
  } = opts;
  return render(() => (
    <DiagnosticsPanel
      collapsed={collapsed}
      height={height}
      diagnostics={diagnostics}
      onToggleCollapsed={onToggleCollapsed}
      onNavigate={onNavigate}
    />
  ));
}

describe('DiagnosticsPanel docked root', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('root always mounted: data-testid="diagnostics-panel" present when collapsed=true', () => {
    renderDocked({ collapsed: true });
    expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
  });

  it('root always mounted: data-testid="diagnostics-panel" present when collapsed=false', () => {
    renderDocked({ collapsed: false });
    expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
  });

  it('data-collapsed="true" when collapsed=true', () => {
    renderDocked({ collapsed: true });
    const root = screen.getByTestId('diagnostics-panel');
    expect(root.getAttribute('data-collapsed')).toBe('true');
  });

  it('data-collapsed="false" when collapsed=false', () => {
    renderDocked({ collapsed: false });
    const root = screen.getByTestId('diagnostics-panel');
    expect(root.getAttribute('data-collapsed')).toBe('false');
  });

  it('fold toggle: data-testid="diagnostics-fold-toggle" is always present', () => {
    renderDocked({ collapsed: true });
    expect(screen.getByTestId('diagnostics-fold-toggle')).toBeTruthy();
  });

  it('fold toggle: aria-expanded="false" when collapsed=true', () => {
    renderDocked({ collapsed: true });
    const btn = screen.getByTestId('diagnostics-fold-toggle');
    expect(btn.getAttribute('aria-expanded')).toBe('false');
  });

  it('fold toggle: aria-expanded="true" when collapsed=false', () => {
    renderDocked({ collapsed: false });
    const btn = screen.getByTestId('diagnostics-fold-toggle');
    expect(btn.getAttribute('aria-expanded')).toBe('true');
  });

  it('fold toggle: aria-label matches /diagnostic|toggle/i', () => {
    renderDocked({ collapsed: false });
    const btn = screen.getByTestId('diagnostics-fold-toggle');
    const label = btn.getAttribute('aria-label') ?? btn.textContent ?? '';
    expect(label).toMatch(/diagnostic|toggle/i);
  });

  it('fold toggle click invokes onToggleCollapsed', () => {
    const onToggleCollapsed = vi.fn();
    renderDocked({ onToggleCollapsed });
    fireEvent.click(screen.getByTestId('diagnostics-fold-toggle'));
    expect(onToggleCollapsed).toHaveBeenCalledTimes(1);
  });
});

describe('DiagnosticsPanel header', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('panel-title-diagnostics shows "Diagnostics (N)" — zero diagnostics', () => {
    renderDocked({ diagnostics: [] });
    const title = screen.getByTestId('panel-title-diagnostics');
    expect(title).toBeTruthy();
    expect(title.textContent).toMatch(/diagnostics\s*\(\s*0\s*\)/i);
  });

  it('panel-title-diagnostics count reflects diagnostics.length', () => {
    const diags: DiagnosticEntry[] = [
      { ...makeDiag('Error', { message: 'err1' }), source: 'compile' },
      { ...makeDiag('Warning', { message: 'warn1' }), source: 'tessellation' },
    ];
    renderDocked({ diagnostics: diags });
    const title = screen.getByTestId('panel-title-diagnostics');
    expect(title.textContent).toContain('2');
  });
});

describe('DiagnosticsPanel body visibility (collapsed gating)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('collapsed=true: no filter-bar, no diagnostic-row, no line-wrap toggle', () => {
    const diag: DiagnosticEntry = { ...makeDiag('Error', { message: 'err' }), source: 'compile' };
    renderDocked({ collapsed: true, diagnostics: [diag] });
    expect(document.querySelector('[data-testid="diagnostic-row"]')).toBeNull();
    expect(document.querySelector('[data-testid="diagnostics-filter-source-compile"]')).toBeNull();
    expect(document.querySelector('[data-testid="diagnostics-line-wrap-toggle"]')).toBeNull();
  });

  it('collapsed=false: body present (diagnostic-row visible)', () => {
    const diag: DiagnosticEntry = { ...makeDiag('Error', { message: 'err' }), source: 'compile' };
    renderDocked({ collapsed: false, diagnostics: [diag] });
    expect(screen.getByTestId('diagnostic-row')).toBeTruthy();
  });

  it('collapsed=false: root element carries inline height matching height prop', () => {
    renderDocked({ collapsed: false, height: 200 });
    const root = screen.getByTestId('diagnostics-panel');
    // Height applied only when expanded
    expect(root.style.height).toBe('200px');
  });

  it('collapsed=true: root has no inline height (no visual space wasted)', () => {
    renderDocked({ collapsed: true, height: 200 });
    const root = screen.getByTestId('diagnostics-panel');
    expect(root.style.height).toBeFalsy();
  });
});

describe('DiagnosticsPanel', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('shows empty-state message when diagnostics is empty', () => {
    renderDocked({ collapsed: false, diagnostics: [] });
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).toMatch(/no diagnostics/i);
  });

  it('renders one row per diagnostic entry', () => {
    const diags: DiagnosticEntry[] = [
      { ...makeDiag('Error', { file_path: 'main.ri', line: 10, message: 'import failed' }), source: 'compile' },
      { ...makeDiag('Warning', { file_path: 'helper.ri', line: 3, message: "unknown port type 'Foo'" }), source: 'tessellation' },
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    const panel = screen.getByTestId('diagnostics-panel');
    const text = panel.textContent ?? '';
    expect(text).toContain('import failed');
    expect(text).toContain("unknown port type 'Foo'");
    expect(text).toMatch(/error/i);
    expect(text).toMatch(/warning/i);
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
        collapsed={false}
        height={160}
        diagnostics={[diag]}
        onToggleCollapsed={vi.fn()}
        onNavigate={onNavigate}
      />
    ));
    const row = screen.getByText(/unknown port type/i).closest('[data-testid="diagnostic-row"]') as HTMLElement
      ?? screen.getByText(/unknown port type/i).parentElement as HTMLElement;
    fireEvent.click(row);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(diag);
  });

  it('renders a source chip per row with correct source label', () => {
    const compileEntry: DiagnosticEntry = { ...makeDiag('Error', { message: 'compile error' }), source: 'compile' };
    const tessEntry: DiagnosticEntry = { ...makeDiag('Warning', { message: 'tess warning' }), source: 'tessellation' };
    renderDocked({ collapsed: false, diagnostics: [compileEntry, tessEntry] });
    const chips = document.querySelectorAll('[data-testid="diagnostic-source-chip"]');
    expect(chips.length).toBe(2);
    const chipTexts = Array.from(chips).map((c) => c.textContent);
    expect(chipTexts).toContain('compile');
    expect(chipTexts).toContain('tessellation');
  });

  it('renders a line-wrap checkbox by default (collapsed=false)', () => {
    renderDocked({ collapsed: false, diagnostics: [] });
    const checkbox = screen.getByTestId('diagnostics-line-wrap-toggle') as HTMLInputElement;
    expect(checkbox).toBeTruthy();
    expect(checkbox.checked).toBe(false);
  });

  it('toggling line-wrap adds lineWrapOn class to the panel root and persists to localStorage', () => {
    renderDocked({ collapsed: false, diagnostics: [] });
    const checkbox = screen.getByTestId('diagnostics-line-wrap-toggle') as HTMLInputElement;
    const root = screen.getByTestId('diagnostics-panel');

    // Initially no lineWrapOn class
    expect(root.className).not.toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBeNull();

    // Click to enable wrap
    fireEvent.click(checkbox);
    expect(root.className).toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBe(true);

    // Click again to disable
    fireEvent.click(checkbox);
    expect(root.className).not.toContain('lineWrapOn');
    expect(loadDiagnosticsLineWrap()).toBe(false);
  });

  it('list element has the list CSS class and root has no lineWrapOn class by default', () => {
    const diag: DiagnosticEntry = { ...makeDiag('Error', { message: 'oops' }), source: 'compile' };
    renderDocked({ collapsed: false, diagnostics: [diag] });
    const row = screen.getByTestId('diagnostic-row');
    const list = row.parentElement as HTMLElement;
    expect(list.classList.contains(styles.list)).toBe(true);

    const root = screen.getByTestId('diagnostics-panel');
    expect(root.className).not.toContain('lineWrapOn');
  });
});

describe('DiagnosticsPanel source/severity filtering', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  function makeCompileError(message: string): DiagnosticEntry {
    return { ...makeDiag('Error', { message }), source: 'compile' };
  }
  function makeTessWarning(message: string): DiagnosticEntry {
    return { ...makeDiag('Warning', { message }), source: 'tessellation' };
  }

  it('filter controls render when diagnostics are present: source toggles + severity toggles', () => {
    const diags: DiagnosticEntry[] = [
      makeCompileError('compA'),
      makeTessWarning('tessB'),
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    expect(screen.getByTestId('diagnostics-filter-source-compile')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-filter-source-tessellation')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-filter-severity-Error')).toBeTruthy();
    expect(screen.getByTestId('diagnostics-filter-severity-Warning')).toBeTruthy();
  });

  it('by default both messages are visible', () => {
    const diags: DiagnosticEntry[] = [
      makeCompileError('compA'),
      makeTessWarning('tessB'),
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).toContain('compA');
    expect(panel.textContent).toContain('tessB');
  });

  it('clicking tessellation source toggle hides tessellation entries, keeps compile entries', () => {
    const diags: DiagnosticEntry[] = [
      makeCompileError('compA'),
      makeTessWarning('tessB'),
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    fireEvent.click(screen.getByTestId('diagnostics-filter-source-tessellation'));
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).toContain('compA');
    expect(panel.textContent).not.toContain('tessB');
  });

  it('clicking Error severity toggle hides Error entries, keeps Warning entries', () => {
    const diags: DiagnosticEntry[] = [
      makeCompileError('compA'),
      makeTessWarning('tessB'),
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    fireEvent.click(screen.getByTestId('diagnostics-filter-severity-Error'));
    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).not.toContain('compA');
    expect(panel.textContent).toContain('tessB');
  });

  it('filter controls are NOT rendered when diagnostics list is empty', () => {
    renderDocked({ collapsed: false, diagnostics: [] });
    expect(screen.queryByTestId('diagnostics-filter-source-compile')).toBeNull();
    expect(screen.queryByTestId('diagnostics-filter-severity-Error')).toBeNull();
  });
});

describe('DiagnosticsPanel repeated-warning collapse (grouping)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  function makeIdenticalTessWarning(message: string): DiagnosticEntry {
    return {
      ...makeDiag('Warning', { file_path: 'model.ri', line: 42, column: 7, message }),
      source: 'tessellation',
    };
  }

  it('three IDENTICAL entries collapse to one row with a repeat-count badge showing "3"', () => {
    const repeatedDiag = makeIdenticalTessWarning('repeated warning');
    const diags: DiagnosticEntry[] = [repeatedDiag, repeatedDiag, repeatedDiag];
    renderDocked({ collapsed: false, diagnostics: diags });
    const rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(1);

    const badge = screen.getByTestId('diagnostic-repeat-count');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toContain('3');
  });

  it('a group toggle data-testid="diagnostics-group-toggle" exists', () => {
    const repeatedDiag = makeIdenticalTessWarning('repeated warning');
    renderDocked({ collapsed: false, diagnostics: [repeatedDiag, repeatedDiag, repeatedDiag] });
    expect(screen.getByTestId('diagnostics-group-toggle')).toBeTruthy();
  });

  it('toggling group-toggle OFF expands to three rows with no repeat-count badge', () => {
    const repeatedDiag = makeIdenticalTessWarning('repeated warning');
    const diags: DiagnosticEntry[] = [repeatedDiag, repeatedDiag, repeatedDiag];
    renderDocked({ collapsed: false, diagnostics: diags });
    expect(document.querySelectorAll('[data-testid="diagnostic-row"]').length).toBe(1);

    fireEvent.click(screen.getByTestId('diagnostics-group-toggle'));

    const rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(3);
    expect(screen.queryByTestId('diagnostic-repeat-count')).toBeNull();
  });

  it('clicking the collapsed row invokes onNavigate with the first representative entry', () => {
    const first: DiagnosticEntry = { ...makeDiag('Warning', { file_path: 'model.ri', line: 42, column: 7, message: 'repeated warning' }), source: 'tessellation' };
    const second: DiagnosticEntry = { ...makeDiag('Warning', { file_path: 'model.ri', line: 42, column: 7, message: 'repeated warning' }), source: 'tessellation' };
    const third: DiagnosticEntry = { ...makeDiag('Warning', { file_path: 'model.ri', line: 42, column: 7, message: 'repeated warning' }), source: 'tessellation' };
    const onNavigate = vi.fn();
    render(() => (
      <DiagnosticsPanel
        collapsed={false}
        height={160}
        diagnostics={[first, second, third]}
        onToggleCollapsed={vi.fn()}
        onNavigate={onNavigate}
      />
    ));
    const row = screen.getByTestId('diagnostic-row');
    fireEvent.click(row);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(first);
  });

  it('regression: two DISTINCT entries render two rows, neither has a repeat-count badge', () => {
    const diags: DiagnosticEntry[] = [
      { ...makeDiag('Error', { message: 'distinct error A' }), source: 'compile' },
      { ...makeDiag('Warning', { message: 'distinct warning B' }), source: 'tessellation' },
    ];
    renderDocked({ collapsed: false, diagnostics: diags });
    const rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(2);
    expect(document.querySelectorAll('[data-testid="diagnostic-repeat-count"]').length).toBe(0);
  });
});

describe('DiagnosticsPanel filter+group interaction', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('grouped rows survive source toggle and repeat-count recomputes correctly', () => {
    const sameCompileError: DiagnosticEntry = {
      ...makeDiag('Error', { file_path: 'x.ri', line: 1, column: 1, message: 'dup compile error' }),
      source: 'compile',
    };
    const tessWarning: DiagnosticEntry = {
      ...makeDiag('Warning', { file_path: 'y.ri', line: 2, column: 1, message: 'tess warning' }),
      source: 'tessellation',
    };

    renderDocked({ collapsed: false, diagnostics: [sameCompileError, sameCompileError, tessWarning] });

    let rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(2);

    fireEvent.click(screen.getByTestId('diagnostics-filter-source-tessellation'));
    rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(1);

    const badge = screen.getByTestId('diagnostic-repeat-count');
    expect(badge.textContent).toContain('2');
  });

  it('filtering everything out shows a "no diagnostics match" message instead of an empty list', () => {
    const diag: DiagnosticEntry = {
      ...makeDiag('Error', { message: 'only compile error' }),
      source: 'compile',
    };
    renderDocked({ collapsed: false, diagnostics: [diag] });

    fireEvent.click(screen.getByTestId('diagnostics-filter-source-compile'));

    const panel = screen.getByTestId('diagnostics-panel');
    expect(panel.textContent).toMatch(/no diagnostics match/i);
    expect(document.querySelectorAll('[data-testid="diagnostic-row"]').length).toBe(0);
  });

  it('Info severity chip does not render when no Info diagnostics are present', () => {
    const warningOnly: DiagnosticEntry = {
      ...makeDiag('Warning', { message: 'warning only' }),
      source: 'compile',
    };
    renderDocked({ collapsed: false, diagnostics: [warningOnly] });
    expect(screen.queryByTestId('diagnostics-filter-severity-Info')).toBeNull();
  });

  it('Info severity chip renders when Info diagnostics are present', () => {
    const entries: DiagnosticEntry[] = [
      { ...makeDiag('Warning', { message: 'warning' }), source: 'compile' },
      { ...makeDiag('Info', { message: 'info note' }), source: 'compile' },
    ];
    renderDocked({ collapsed: false, diagnostics: entries });
    expect(screen.getByTestId('diagnostics-filter-severity-Info')).toBeTruthy();
  });
});

describe('DiagnosticsPanel span-less interactivity (β/4402)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  // Three diagnostics with DISTINCT messages so grouping-ON doesn't collapse them.
  // A = span-less (has_location: false), B = line-tied (has_location: true),
  // C = legacy/absent (has_location omitted).
  function makeRowSet() {
    const diagA: DiagnosticEntry = {
      ...makeDiag('Error', { message: 'span-less error A', has_location: false }),
      source: 'compile',
    };
    const diagB: DiagnosticEntry = {
      ...makeDiag('Warning', { message: 'line-tied warning B', has_location: true }),
      source: 'compile',
    };
    const diagC: DiagnosticEntry = {
      ...makeDiag('Info', { message: 'legacy info C' /* has_location omitted */ }),
      source: 'compile',
    };
    return { diagA, diagB, diagC };
  }

  it('all three rows expose data-testid="diagnostic-row"', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC] });
    const rows = document.querySelectorAll('[data-testid="diagnostic-row"]');
    expect(rows.length).toBe(3);
  });

  it('row A (has_location:false) has no role and no tabindex', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC] });
    const rowA = screen.getByText(/span-less error A/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(rowA).toBeTruthy();
    expect(rowA.getAttribute('role')).toBeNull();
    expect(rowA.getAttribute('tabindex')).toBeNull();
  });

  it('row A (has_location:false) click does NOT call onNavigate', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    const onNavigate = vi.fn();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC], onNavigate });
    const rowA = screen.getByText(/span-less error A/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    fireEvent.click(rowA);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('row A (has_location:false) keyDown Enter does NOT call onNavigate', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    const onNavigate = vi.fn();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC], onNavigate });
    const rowA = screen.getByText(/span-less error A/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    fireEvent.keyDown(rowA, { key: 'Enter' });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('row A (has_location:false) keyDown Space does NOT call onNavigate', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    const onNavigate = vi.fn();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC], onNavigate });
    const rowA = screen.getByText(/span-less error A/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    fireEvent.keyDown(rowA, { key: ' ' });
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it('row B (has_location:true) has role="button" and tabindex="0"', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC] });
    const rowB = screen.getByText(/line-tied warning B/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(rowB).toBeTruthy();
    expect(rowB.getAttribute('role')).toBe('button');
    expect(rowB.getAttribute('tabindex')).toBe('0');
  });

  it('row B (has_location:true) click calls onNavigate with diagB', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    const onNavigate = vi.fn();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC], onNavigate });
    const rowB = screen.getByText(/line-tied warning B/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    fireEvent.click(rowB);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(diagB);
  });

  it('row C (has_location omitted — back-compat) has role="button" and tabindex="0"', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC] });
    const rowC = screen.getByText(/legacy info C/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    expect(rowC).toBeTruthy();
    expect(rowC.getAttribute('role')).toBe('button');
    expect(rowC.getAttribute('tabindex')).toBe('0');
  });

  it('row C (has_location omitted — back-compat) click calls onNavigate with diagC', () => {
    const { diagA, diagB, diagC } = makeRowSet();
    const onNavigate = vi.fn();
    renderDocked({ collapsed: false, diagnostics: [diagA, diagB, diagC], onNavigate });
    const rowC = screen.getByText(/legacy info C/).closest('[data-testid="diagnostic-row"]') as HTMLElement;
    fireEvent.click(rowC);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(diagC);
  });
});
