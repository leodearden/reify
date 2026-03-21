import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ConstraintPanel } from '../panels/ConstraintPanel';
import type { ConstraintData, ValueData } from '../types';

function makeConstraint(overrides: Partial<ConstraintData> & { node_id: string }): ConstraintData {
  return {
    node_id: overrides.node_id,
    expression: overrides.expression ?? 'x > 0',
    status: overrides.status ?? 'satisfied',
    label: overrides.label ?? null,
    parameter_ids: overrides.parameter_ids ?? [],
  };
}

function makeValue(overrides: Partial<ValueData> & { cell_id: string }): ValueData {
  return {
    cell_id: overrides.cell_id,
    name: overrides.name ?? 'param',
    value: overrides.value ?? '10',
    unit: overrides.unit ?? 'mm',
    determinacy: overrides.determinacy ?? 'determined',
    entity_path: overrides.entity_path ?? 'Bracket.param',
    kind: overrides.kind ?? 'Param',
  };
}

describe('ConstraintPanel basic rendering', () => {
  it('renders with data-testid="constraint-panel"', () => {
    render(() => <ConstraintPanel constraints={{}} values={{}} />);
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
  });

  it('renders flat list of constraints', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', expression: 'width > 10' }),
      n2: makeConstraint({ node_id: 'n2', expression: 'height < 100' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    expect(screen.getByText('width > 10')).toBeTruthy();
    expect(screen.getByText('height < 100')).toBeTruthy();
  });

  it('each constraint row shows expression text and status badge', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', expression: 'x > 0', status: 'satisfied' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    expect(screen.getByText('x > 0')).toBeTruthy();
    const container = screen.getByTestId('constraint-panel');
    const badge = container.querySelector('[data-status="satisfied"]');
    expect(badge).toBeTruthy();
  });

  it('status badges have correct data-status attributes', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', status: 'satisfied', expression: 'a > 0' }),
      n2: makeConstraint({ node_id: 'n2', status: 'violated', expression: 'b > 0' }),
      n3: makeConstraint({ node_id: 'n3', status: 'indeterminate', expression: 'c > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const badges = container.querySelectorAll('[data-status]');
    const statuses = Array.from(badges).map((b) => b.getAttribute('data-status'));
    expect(statuses).toContain('satisfied');
    expect(statuses).toContain('violated');
    expect(statuses).toContain('indeterminate');
  });

  it('constraints are sorted: violated first, then indeterminate, then satisfied', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', status: 'satisfied', expression: 'sat-expr' }),
      n2: makeConstraint({ node_id: 'n2', status: 'violated', expression: 'viol-expr' }),
      n3: makeConstraint({ node_id: 'n3', status: 'indeterminate', expression: 'indet-expr' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const rows = container.querySelectorAll('[data-testid^="constraint-row-"]');
    const expressions = Array.from(rows).map((r) => r.textContent);
    // violated first, then indeterminate, then satisfied
    expect(expressions[0]).toContain('viol-expr');
    expect(expressions[1]).toContain('indet-expr');
    expect(expressions[2]).toContain('sat-expr');
  });

  it('shows empty state message when no constraints', () => {
    render(() => <ConstraintPanel constraints={{}} values={{}} />);
    expect(screen.getByText('No constraints')).toBeTruthy();
  });
});

describe('ConstraintPanel expansion', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
    c2: makeValue({ cell_id: 'c2', name: 'height', value: '30' }),
  };

  it('clicking a violated constraint expands it to show contributing parameters', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({
        node_id: 'n1',
        status: 'violated',
        expression: 'width > 100',
        parameter_ids: ['c1', 'c2'],
      }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={values} />);

    // Initially no param details visible
    expect(screen.queryByText('width = 50')).toBeNull();

    // Click to expand
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.click(row);

    // Now contributing params should be visible
    expect(screen.getByText('width = 50')).toBeTruthy();
    expect(screen.getByText('height = 30')).toBeTruthy();
  });

  it('clicking again collapses it', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({
        node_id: 'n1',
        status: 'violated',
        expression: 'width > 100',
        parameter_ids: ['c1'],
      }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={values} />);
    const row = screen.getByTestId('constraint-row-n1');

    // Expand
    fireEvent.click(row);
    expect(screen.getByText('width = 50')).toBeTruthy();

    // Collapse
    fireEvent.click(row);
    expect(screen.queryByText('width = 50')).toBeNull();
  });

  it('satisfied constraints are not expandable (no expand indicator)', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({
        node_id: 'n1',
        status: 'satisfied',
        expression: 'width > 0',
        parameter_ids: ['c1'],
      }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={values} />);
    const row = screen.getByTestId('constraint-row-n1');

    // Click should not expand
    fireEvent.click(row);
    expect(screen.queryByText('width = 50')).toBeNull();
  });

  it('expanded violated constraint shows each contributing parameter as "name = value"', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({
        node_id: 'n1',
        status: 'violated',
        expression: 'width + height > 200',
        parameter_ids: ['c1', 'c2'],
      }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={values} />);

    fireEvent.click(screen.getByTestId('constraint-row-n1'));
    expect(screen.getByText('width = 50')).toBeTruthy();
    expect(screen.getByText('height = 30')).toBeTruthy();
  });
});

describe('ConstraintPanel onConstraintSelect', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
  };

  it('clicking a constraint row calls onConstraintSelect with the ConstraintData object', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onSelect = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onConstraintSelect={onSelect}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.click(row);
    expect(onSelect).toHaveBeenCalledWith(constraint);
  });

  it('expand/collapse still works alongside onConstraintSelect', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onSelect = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onConstraintSelect={onSelect}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');

    // Click expands AND calls onConstraintSelect
    fireEvent.click(row);
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.getByText('width = 50')).toBeTruthy();

    // Click again collapses AND calls onConstraintSelect again
    fireEvent.click(row);
    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(screen.queryByText('width = 50')).toBeNull();
  });

  it('onConstraintSelect is optional — omitting it does not break existing behavior', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    // Render WITHOUT onConstraintSelect
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    // Should not throw
    fireEvent.click(row);
    expect(screen.getByText('width = 50')).toBeTruthy();
  });
});
