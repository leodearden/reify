import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { StatusBar } from '../panels/StatusBar';
import type { EvaluationStatus, MeshData, ConstraintData, DiagnosticInfo } from '../types';

function makeMesh(entityPath: string, numTriangles: number): MeshData {
  return {
    entity_path: entityPath,
    vertices: new Float32Array(numTriangles * 9), // 3 verts * 3 coords
    indices: new Uint32Array(numTriangles * 3),    // 3 indices per triangle
    normals: null,
  };
}

function makeConstraint(nodeId: string, status: string): ConstraintData {
  return {
    node_id: nodeId,
    expression: 'x > 0',
    status,
    label: null,
    parameter_ids: [],
  };
}

describe('StatusBar', () => {
  it('renders with data-testid="status-bar"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('displays evaluation phase text from evalStatus prop', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'evaluating' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    expect(screen.getByText(/evaluating/i)).toBeTruthy();
  });

  it('displays triangle count computed from meshes', () => {
    const meshes: Record<string, MeshData> = {
      m1: makeMesh('Bracket', 100),  // 100 triangles = 300 indices
      m2: makeMesh('Cylinder', 50),  // 50 triangles = 150 indices
    };
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={meshes}
        constraints={{}}
      />
    ));
    // Total: 150 triangles
    expect(screen.getByText(/150/)).toBeTruthy();
  });

  it('displays constraint summary counts', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint('n1', 'satisfied'),
      n2: makeConstraint('n2', 'satisfied'),
      n3: makeConstraint('n3', 'violated'),
      n4: makeConstraint('n4', 'indeterminate'),
    };
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={constraints}
      />
    ));
    // Should show counts: 2 satisfied, 1 violated, 1 indeterminate
    const container = screen.getByTestId('status-bar');
    const text = container.textContent || '';
    expect(text).toContain('2');
    expect(text).toContain('1');
  });
});

describe('StatusBar accessibility', () => {
  it('container has role="status" for screen readers', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    const el = screen.getByTestId('status-bar');
    expect(el.getAttribute('role')).toBe('status');
  });

  it('container has aria-live="polite" for live region updates', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    const el = screen.getByTestId('status-bar');
    expect(el.getAttribute('aria-live')).toBe('polite');
  });
});

describe('StatusBar tessellation diagnostics', () => {
  function makeDiag(severity: string, message = 'test error'): DiagnosticInfo {
    return {
      file_path: '<unknown>',
      line: 1, column: 1, end_line: 1, end_column: 1,
      severity,
      message,
      code: null,
    };
  }

  it('absent tessellationDiagnostics prop: no error badge rendered and phase label unchanged', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    expect(screen.queryByTestId('tessellation-errors')).toBeNull();
    expect(screen.getByText(/idle/i)).toBeTruthy();
  });

  it('empty tessellationDiagnostics prop: no error badge rendered', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[]}
      />
    ));
    expect(screen.queryByTestId('tessellation-errors')).toBeNull();
  });

  it('one Error diagnostic: badge with count 1 is visible and data-has-errors="true"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error')]}
      />
    ));
    const badge = screen.getByTestId('tessellation-errors');
    expect(badge).toBeTruthy();
    expect(badge.getAttribute('data-has-errors')).toBe('true');
    // Error count badge should contain "1"
    expect(badge.textContent).toContain('1');
  });

  it('mixed Error and Warning diagnostics: both counts render', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error'), makeDiag('Warning')]}
      />
    ));
    // Assert each badge separately so a missing badge fails the test.
    // Use exact string matching so a regression "1 errors" would be caught
    // (the regex /1 error/i would silently pass for "1 errors" as a substring match).
    expect(screen.getByText('1 error')).toBeTruthy();
    expect(screen.getByText('1 warning')).toBeTruthy();
  });

  it('asymmetric counts: pluralisation is correct for 2 errors and 1 warning', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error'), makeDiag('Error'), makeDiag('Warning')]}
      />
    ));
    expect(screen.getByText('2 errors')).toBeTruthy();
    expect(screen.getByText('1 warning')).toBeTruthy();
  });

  it('zero meshes and zero errors: shows "No geometry" label', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[]}
      />
    ));
    expect(screen.getByText(/no geometry/i)).toBeTruthy();
  });

  it('zero meshes and at least one error: shows "Tessellation error" label', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error')]}
      />
    ));
    expect(screen.getByText(/tessellation error/i)).toBeTruthy();
  });

  it('clicking the tessellation-errors badge invokes onToggleDiagnostics exactly once', () => {
    const onToggle = vi.fn();
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error')]}
        onToggleDiagnostics={onToggle}
      />
    ));
    fireEvent.click(screen.getByTestId('tessellation-errors'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('tessellation-errors button has type="button" to prevent accidental form submission', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error')]}
      />
    ));
    const badge = screen.getByTestId('tessellation-errors');
    expect(badge.getAttribute('type')).toBe('button');
  });

  it('tessellation badge aria-label identifies pipeline and count (not merged total)', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error')]}
        compileDiagnostics={[makeDiag('Warning'), makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('tessellation-errors');
    const label = badge.getAttribute('aria-label') ?? '';
    // Shows only tessellation count so SR users can distinguish it from the compile button
    expect(label).toBe('Show 1 tessellation diagnostic');
  });
});

describe('StatusBar compile diagnostics', () => {
  function makeDiag(severity: 'Error' | 'Warning' | 'Info', message = 'test'): DiagnosticInfo {
    return {
      file_path: 'test.ri',
      line: 1, column: 1, end_line: 1, end_column: 1,
      severity,
      message,
      code: null,
    };
  }

  it('absent compileDiagnostics prop: no diagnostics-count badge rendered', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    expect(screen.queryByTestId('diagnostics-count')).toBeNull();
  });

  it('empty compileDiagnostics array: no diagnostics-count badge rendered', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[]}
      />
    ));
    expect(screen.queryByTestId('diagnostics-count')).toBeNull();
  });

  it('one Warning diagnostic: badge with "1 warning" is visible', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toMatch(/1 warning/i);
  });

  it('one Error and one Warning: both counts visible in the badge', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Error'), makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toMatch(/1 error/i);
    expect(badge.textContent).toMatch(/1 warning/i);
  });

  it('clicking the badge invokes onToggleDiagnostics exactly once', () => {
    const onToggle = vi.fn();
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
        onToggleDiagnostics={onToggle}
      />
    ));
    fireEvent.click(screen.getByTestId('diagnostics-count'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('badge has aria-label mentioning "diagnostics" for screen readers', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    const label = badge.getAttribute('aria-label') ?? '';
    expect(label).toMatch(/diagnostic/i);
  });

  it('diagnostics-count button has type="button" to prevent accidental form submission', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    expect(badge.getAttribute('type')).toBe('button');
  });

  it('compile badge aria-label uses singular when count is 1', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    const label = badge.getAttribute('aria-label') ?? '';
    expect(label).toBe('Show 1 compile diagnostic');
  });

  it('compile badge aria-label identifies pipeline and count (not merged total)', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Error'), makeDiag('Warning')]}
        tessellationDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    const label = badge.getAttribute('aria-label') ?? '';
    // Shows only compile count so SR users can distinguish it from the tessellation button
    expect(label).toBe('Show 2 compile diagnostics');
  });
});

describe('StatusBar Claude status indicator', () => {
  it('when claudeStatus prop provided, renders section with data-testid="claude-status"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="idle"
      />
    ));
    expect(screen.getByTestId('claude-status')).toBeTruthy();
  });

  it('shows "Claude: idle" when claudeStatus="idle"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="idle"
      />
    ));
    const el = screen.getByTestId('claude-status');
    expect(el.textContent).toContain('Claude:');
    expect(el.textContent).toContain('idle');
  });

  it('shows "Claude: thinking..." when claudeStatus="thinking"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="thinking"
      />
    ));
    const el = screen.getByTestId('claude-status');
    expect(el.textContent).toContain('thinking...');
  });

  it('shows "Claude: calling tool..." when claudeStatus="tool-calling"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="tool-calling"
      />
    ));
    const el = screen.getByTestId('claude-status');
    expect(el.textContent).toContain('calling tool...');
  });

  it('shows "Claude: responding..." when claudeStatus="responding"', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="responding"
      />
    ));
    const el = screen.getByTestId('claude-status');
    expect(el.textContent).toContain('responding...');
  });

  it('clicking the indicator calls onToggleChat callback', () => {
    const onToggle = vi.fn();
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        claudeStatus="idle"
        onToggleChat={onToggle}
      />
    ));
    fireEvent.click(screen.getByTestId('claude-status'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('without claudeStatus prop, no claude-status section rendered', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
      />
    ));
    expect(screen.queryByTestId('claude-status')).toBeNull();
  });
});

describe('StatusBar constraint pill accessible labels', () => {
  it('each constraint pill has aria-label and title with count and status', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint('n1', 'satisfied'),
      n2: makeConstraint('n2', 'satisfied'),
      n3: makeConstraint('n3', 'violated'),
      n4: makeConstraint('n4', 'indeterminate'),
      n5: makeConstraint('n5', 'indeterminate'),
      n6: makeConstraint('n6', 'indeterminate'),
    };
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={constraints}
      />
    ));
    const satisfiedPill = document.querySelector('[data-status="satisfied"]') as HTMLElement;
    const violatedPill = document.querySelector('[data-status="violated"]') as HTMLElement;
    const indeterminatePill = document.querySelector('[data-status="indeterminate"]') as HTMLElement;

    expect(satisfiedPill).toBeTruthy();
    expect(violatedPill).toBeTruthy();
    expect(indeterminatePill).toBeTruthy();

    // aria-label and title must contain count + status word
    expect(satisfiedPill.getAttribute('aria-label')).toMatch(/2\s*satisfied/i);
    expect(satisfiedPill.getAttribute('title')).toMatch(/2\s*satisfied/i);

    expect(violatedPill.getAttribute('aria-label')).toMatch(/1\s*violated/i);
    expect(violatedPill.getAttribute('title')).toMatch(/1\s*violated/i);

    expect(indeterminatePill.getAttribute('aria-label')).toMatch(/3\s*indeterminate/i);
    expect(indeterminatePill.getAttribute('title')).toMatch(/3\s*indeterminate/i);
  });
});

describe('StatusBar merged diagnostics rendering', () => {
  function makeDiag(severity: string, message = 'test error'): DiagnosticInfo {
    return {
      file_path: '<unknown>',
      line: 1, column: 1, end_line: 1, end_column: 1,
      severity,
      message,
      code: null,
    };
  }

  it('when both compile and tessellation diagnostics arrays are non-empty, both badges render', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Error', 'tess boom')]}
        compileDiagnostics={[makeDiag('Warning', 'compile warn')]}
      />
    ));
    const tessBadge = screen.getByTestId('tessellation-errors');
    const compileBadge = screen.getByTestId('diagnostics-count');
    expect(tessBadge).toBeTruthy();
    expect(compileBadge).toBeTruthy();
    // Each badge must carry the right summary — a regression that swaps the two
    // badge summaries would still render both elements but fail here.
    expect(tessBadge.textContent).toContain('1 error');
    expect(compileBadge.textContent).toContain('1 warning');
    expect(tessBadge.getAttribute('aria-label')).toContain('tessellation');
    expect(compileBadge.getAttribute('aria-label')).toContain('compile');
  });
});

describe('StatusBar pipeline labels and diagnostics total', () => {
  function makeDiag(severity: string, message = 'test'): DiagnosticInfo {
    return {
      file_path: '<unknown>',
      line: 1, column: 1, end_line: 1, end_column: 1,
      severity,
      message,
      code: null,
    };
  }

  it('tessellation-errors button visible textContent contains pipeline label /tessellation/i', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('tessellation-errors');
    expect(badge.textContent).toMatch(/tessellation/i);
  });

  it('diagnostics-count button visible textContent contains pipeline label /compile/i', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    const badge = screen.getByTestId('diagnostics-count');
    expect(badge.textContent).toMatch(/compile/i);
  });

  it('diagnostics-total renders when BOTH arrays are non-empty and shows combined total', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Warning')]}
        compileDiagnostics={[makeDiag('Error'), makeDiag('Warning')]}
      />
    ));
    const total = screen.getByTestId('diagnostics-total');
    expect(total).toBeTruthy();
    // Combined total is 1 + 2 = 3
    expect(total.textContent).toContain('3');
    expect(total.getAttribute('aria-label')).toMatch(/diagnostic/i);
  });

  it('diagnostics-total is NOT rendered when only tessellation has diagnostics', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        tessellationDiagnostics={[makeDiag('Warning')]}
      />
    ));
    expect(screen.queryByTestId('diagnostics-total')).toBeNull();
  });

  it('diagnostics-total is NOT rendered when only compile has diagnostics', () => {
    render(() => (
      <StatusBar
        evalStatus={{ phase: 'idle' }}
        meshes={{}}
        constraints={{}}
        compileDiagnostics={[makeDiag('Warning')]}
      />
    ));
    expect(screen.queryByTestId('diagnostics-total')).toBeNull();
  });
});
