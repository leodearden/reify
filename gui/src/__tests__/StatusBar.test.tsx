import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { StatusBar } from '../panels/StatusBar';
import type { EvaluationStatus, MeshData, ConstraintData } from '../types';

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
