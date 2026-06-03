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
    freshness: overrides.freshness ?? 'final',
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

describe('ConstraintPanel accessibility', () => {
  it('constraint list container has role="list"', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', expression: 'x > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const list = container.querySelector('[role="list"]');
    expect(list).toBeTruthy();
  });

  it('each constraint row has role="listitem"', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', expression: 'x > 0' }),
      n2: makeConstraint({ node_id: 'n2', expression: 'y > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const listitems = container.querySelectorAll('[role="listitem"]');
    expect(listitems.length).toBe(2);
  });

  it('each constraint row has tabindex="0" for keyboard focusability', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', expression: 'x > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const row = screen.getByTestId('constraint-row-n1');
    expect(row.getAttribute('tabindex')).toBe('0');
  });
});

describe('ConstraintPanel keyboard interaction', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
  };

  it('pressing Enter on a violated constraint row expands it', () => {
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

    // Initially not expanded
    expect(screen.queryByText('width = 50')).toBeNull();

    // Press Enter to expand
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(screen.getByText('width = 50')).toBeTruthy();
  });

  it('pressing Space on a violated constraint row expands it', () => {
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

    // Press Space to expand
    fireEvent.keyDown(row, { key: ' ' });
    expect(screen.getByText('width = 50')).toBeTruthy();
  });

  it('pressing Enter again collapses an expanded row', () => {
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
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(screen.getByText('width = 50')).toBeTruthy();

    // Collapse
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(screen.queryByText('width = 50')).toBeNull();
  });
});

describe('ConstraintPanel status badge aria-labels', () => {
  it('each status badge has aria-label matching its status', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', status: 'satisfied', expression: 'a > 0' }),
      n2: makeConstraint({ node_id: 'n2', status: 'violated', expression: 'b > 0' }),
      n3: makeConstraint({ node_id: 'n3', status: 'indeterminate', expression: 'c > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const badges = container.querySelectorAll('[data-status]');
    const ariaLabels = Array.from(badges).map((b) => b.getAttribute('aria-label'));
    expect(ariaLabels).toContain('satisfied');
    expect(ariaLabels).toContain('violated');
    expect(ariaLabels).toContain('indeterminate');
  });
});

describe('ConstraintPanel Ask Claude context menu', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
    c2: makeValue({ cell_id: 'c2', name: 'height', value: '30' }),
  };

  it('right-clicking a constraint row shows a context menu', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    expect(screen.getByTestId('constraint-context-menu')).toBeTruthy();
  });

  it('menu contains "Ask Claude about this constraint" option', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    expect(screen.getByText('Ask Claude about this constraint')).toBeTruthy();
  });

  it('clicking the option calls onAskClaude with context string', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1', 'c2'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    fireEvent.click(screen.getByText('Ask Claude about this constraint'));
    expect(onAskClaude).toHaveBeenCalledTimes(1);
    const contextStr = onAskClaude.mock.calls[0][0] as string;
    expect(contextStr).toContain('Constraint: width > 100');
    expect(contextStr).toContain('Status: violated');
    expect(contextStr).toContain('width=50');
    expect(contextStr).toContain('height=30');
  });

  it('menu closes after clicking the option', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    expect(screen.getByTestId('constraint-context-menu')).toBeTruthy();
    fireEvent.click(screen.getByText('Ask Claude about this constraint'));
    expect(screen.queryByTestId('constraint-context-menu')).toBeNull();
  });

  it('menu closes on click-outside', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    expect(screen.getByTestId('constraint-context-menu')).toBeTruthy();
    // Click outside the menu
    fireEvent.click(document.body);
    expect(screen.queryByTestId('constraint-context-menu')).toBeNull();
  });

  it('onAskClaude is optional — omitting it means no context menu on right-click', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'width > 100',
      parameter_ids: ['c1'],
    });
    // Render WITHOUT onAskClaude
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
      />
    ));
    const row = screen.getByTestId('constraint-row-n1');
    fireEvent.contextMenu(row);
    expect(screen.queryByTestId('constraint-context-menu')).toBeNull();
  });
});

describe('ConstraintPanel — status badge title', () => {
  it('each status badge has a non-empty, status-unique title', () => {
    const statuses = ['satisfied', 'violated', 'indeterminate'] as const;
    const constraints: Record<string, ConstraintData> = Object.fromEntries(
      statuses.map((s, i) => [`n${i + 1}`, makeConstraint({ node_id: `n${i + 1}`, status: s, expression: `x${i} > 0` })])
    );
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    const titles = statuses.map((s) => {
      const badge = container.querySelector(`[data-status="${s}"]`);
      expect(badge).toBeTruthy();
      return badge!.getAttribute('title') ?? '';
    });
    // All titles must be non-empty
    titles.forEach((t) => expect(t.length).toBeGreaterThan(0));
    // All titles must be distinct — each status gets its own human description
    expect(new Set(titles).size).toBe(statuses.length);
  });

  it('existing aria-label={status} is preserved on all badges', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', status: 'satisfied', expression: 'a > 0' }),
      n2: makeConstraint({ node_id: 'n2', status: 'violated', expression: 'b > 0' }),
      n3: makeConstraint({ node_id: 'n3', status: 'indeterminate', expression: 'c > 0' }),
    };
    render(() => <ConstraintPanel constraints={constraints} values={{}} />);
    const container = screen.getByTestId('constraint-panel');
    expect(container.querySelector('[data-status="satisfied"]')!.getAttribute('aria-label')).toBe('satisfied');
    expect(container.querySelector('[data-status="violated"]')!.getAttribute('aria-label')).toBe('violated');
    expect(container.querySelector('[data-status="indeterminate"]')!.getAttribute('aria-label')).toBe('indeterminate');
  });
});

describe('ConstraintPanel — visible actions affordance', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50' }),
  };

  it('when onAskClaude is provided, each row renders an actions button with correct testid and aria-label', () => {
    const constraints: Record<string, ConstraintData> = {
      n1: makeConstraint({ node_id: 'n1', status: 'violated', expression: 'x > 0' }),
      n2: makeConstraint({ node_id: 'n2', status: 'satisfied', expression: 'y > 0' }),
    };
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel constraints={constraints} values={values} onAskClaude={onAskClaude} />
    ));
    const btn1 = screen.getByTestId('constraint-actions-n1');
    expect(btn1).toBeTruthy();
    expect(btn1.getAttribute('aria-label')).toBe('Constraint actions');
    const btn2 = screen.getByTestId('constraint-actions-n2');
    expect(btn2).toBeTruthy();
    expect(btn2.getAttribute('aria-label')).toBe('Constraint actions');
  });

  it('clicking the actions button opens constraint-context-menu with Ask Claude option', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'x > 0',
      parameter_ids: ['c1'],
    });
    const onAskClaude = vi.fn();
    render(() => (
      <ConstraintPanel
        constraints={{ n1: constraint }}
        values={values}
        onAskClaude={onAskClaude}
      />
    ));
    const btn = screen.getByTestId('constraint-actions-n1');
    fireEvent.click(btn);
    expect(screen.getByTestId('constraint-context-menu')).toBeTruthy();
    expect(screen.getByText('Ask Claude about this constraint')).toBeTruthy();
  });

  it('when onAskClaude is omitted, actions button is NOT rendered', () => {
    const constraint = makeConstraint({
      node_id: 'n1',
      status: 'violated',
      expression: 'x > 0',
    });
    render(() => (
      <ConstraintPanel constraints={{ n1: constraint }} values={values} />
    ));
    expect(screen.queryByTestId('constraint-actions-n1')).toBeNull();
  });
});
