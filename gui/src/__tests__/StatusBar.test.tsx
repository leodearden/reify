import { describe, it, expect } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
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
