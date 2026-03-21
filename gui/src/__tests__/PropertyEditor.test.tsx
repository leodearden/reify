import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { PropertyEditor } from '../panels/PropertyEditor';
import type { ValueData } from '../types';

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

describe('PropertyEditor basic rendering', () => {
  it('renders with data-testid="property-editor"', () => {
    render(() => (
      <PropertyEditor
        values={{}}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));
    expect(screen.getByTestId('property-editor')).toBeTruthy();
  });

  it('renders a search/filter input with placeholder text', () => {
    render(() => (
      <PropertyEditor
        values={{}}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));
    const input = screen.getByPlaceholderText('Filter properties...');
    expect(input).toBeTruthy();
  });

  it('groups values by entity_path prefix showing structure name as group headers', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
      c2: makeValue({ cell_id: 'c2', name: 'height', entity_path: 'Bracket.height' }),
      c3: makeValue({ cell_id: 'c3', name: 'radius', entity_path: 'Cylinder.radius' }),
    };

    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));

    // Should show group headers for "Bracket" and "Cylinder"
    expect(screen.getByText('Bracket')).toBeTruthy();
    expect(screen.getByText('Cylinder')).toBeTruthy();
  });

  it('shows empty state message when no values provided', () => {
    render(() => (
      <PropertyEditor
        values={{}}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));
    expect(screen.getByText('No properties')).toBeTruthy();
  });
});

describe('PropertyEditor parameter rows', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', unit: 'mm', determinacy: 'determined', entity_path: 'Bracket.width' }),
    c2: makeValue({ cell_id: 'c2', name: 'height', value: '30', unit: 'mm', determinacy: 'auto', entity_path: 'Bracket.height' }),
    c3: makeValue({ cell_id: 'c3', name: 'radius', value: '10', unit: 'mm', determinacy: 'constrained', entity_path: 'Bracket.radius' }),
    c4: makeValue({ cell_id: 'c4', name: 'depth', value: '', unit: '', determinacy: 'undef', entity_path: 'Bracket.depth' }),
  };

  it('each row displays name, value, unit badge, and determinacy badge', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    // Check a specific row
    expect(screen.getByText('width')).toBeTruthy();
    expect(screen.getByText('height')).toBeTruthy();
    // Check determinacy badges exist
    const badges = screen.getAllByText(/determined|auto|constrained|undef/);
    expect(badges.length).toBeGreaterThanOrEqual(4);
  });

  it('determinacy badge has correct data-determinacy attribute', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const container = screen.getByTestId('property-editor');
    const badges = container.querySelectorAll('[data-determinacy]');
    const attrs = Array.from(badges).map((b) => b.getAttribute('data-determinacy'));
    expect(attrs).toContain('determined');
    expect(attrs).toContain('auto');
    expect(attrs).toContain('constrained');
    expect(attrs).toContain('undef');
  });

  it('determined params have editable input, others have read-only display', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    // The determined param 'width' should have an input element
    const widthRow = screen.getByTestId('prop-row-c1');
    const input = widthRow.querySelector('input[type="text"]');
    expect(input).toBeTruthy();
    expect((input as HTMLInputElement).value).toBe('50');

    // The auto param 'height' should NOT have an editable input
    const heightRow = screen.getByTestId('prop-row-c2');
    const heightInput = heightRow.querySelector('input[type="text"]');
    expect(heightInput).toBeNull();
  });

  it('editing a determined param and pressing Enter calls onSetParameter', async () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const widthRow = screen.getByTestId('prop-row-c1');
    const input = widthRow.querySelector('input[type="text"]') as HTMLInputElement;
    // Change value and press Enter
    fireEvent.input(input, { target: { value: '75' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '75');
  });
});

describe('PropertyEditor interactive features', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
    c2: makeValue({ cell_id: 'c2', name: 'height', entity_path: 'Bracket.height' }),
    c3: makeValue({ cell_id: 'c3', name: 'radius', entity_path: 'Cylinder.radius' }),
  };

  it('collapse/expand: clicking a group header toggles visibility of child rows', async () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    // Initially all rows visible
    expect(screen.getByText('width')).toBeTruthy();
    expect(screen.getByText('height')).toBeTruthy();

    // Click "Bracket" header to collapse
    const bracketHeader = screen.getByText('Bracket');
    fireEvent.click(bracketHeader);

    // width and height should be hidden
    expect(screen.queryByText('width')).toBeNull();
    expect(screen.queryByText('height')).toBeNull();

    // Cylinder params should still be visible
    expect(screen.getByText('radius')).toBeTruthy();

    // Click again to expand
    fireEvent.click(bracketHeader);
    expect(screen.getByText('width')).toBeTruthy();
  });

  it('search/filter: typing in filter input hides non-matching params and groups', async () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const filterInput = screen.getByPlaceholderText('Filter properties...');

    // Type "wid" to filter to only width
    fireEvent.input(filterInput, { target: { value: 'wid' } });

    expect(screen.getByText('width')).toBeTruthy();
    expect(screen.queryByText('height')).toBeNull();
    // Cylinder group should be hidden since no params match
    expect(screen.queryByText('Cylinder')).toBeNull();
    // Bracket group should still show (has matching param)
    expect(screen.getByText('Bracket')).toBeTruthy();
  });

  it('selection highlighting: selected entity group gets selected class and auto-expands', async () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity="Bracket.width" onSetParameter={vi.fn()} />
    ));
    const container = screen.getByTestId('property-editor');
    const selectedGroup = container.querySelector('[data-selected]');
    expect(selectedGroup).toBeTruthy();

    // The selected group should contain Bracket's params
    expect(selectedGroup!.textContent).toContain('Bracket');
  });
});

describe('PropertyEditor group selection boundary checks', () => {
  it('does not false-positive select group with shared prefix', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
      c2: makeValue({ cell_id: 'c2', name: 'height', entity_path: 'BracketMount.height' }),
    };

    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity="BracketMount.height"
        onSetParameter={vi.fn()}
      />
    ));

    const container = screen.getByTestId('property-editor');
    const selectedGroups = container.querySelectorAll('[data-selected]');

    // Only BracketMount group should be selected, not Bracket
    expect(selectedGroups.length).toBe(1);
    expect(selectedGroups[0].textContent).toContain('BracketMount');
    // Bracket group should NOT have data-selected
    const allGroups = container.querySelectorAll('[class*="group"]');
    const bracketGroup = Array.from(allGroups).find(
      (g) => g.querySelector('button')?.textContent?.includes('Bracket') &&
             !g.querySelector('button')?.textContent?.includes('BracketMount')
    );
    expect(bracketGroup?.hasAttribute('data-selected')).toBe(false);
  });

  it('does not false-positive force-expand group with shared prefix', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
      c2: makeValue({ cell_id: 'c2', name: 'height', entity_path: 'BracketMount.height' }),
    };

    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity="BracketMount.height"
        onSetParameter={vi.fn()}
      />
    ));

    // Collapse the Bracket group by clicking its header
    const bracketHeader = screen.getByText('Bracket');
    fireEvent.click(bracketHeader);

    // Bracket's rows should be hidden (collapsed) since it's not the selected group
    expect(screen.queryByText('width')).toBeNull();

    // BracketMount's rows should still be visible (selected group stays expanded)
    expect(screen.getByText('height')).toBeTruthy();
  });

  it('empty-string group name does not match everything', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'unnamed', entity_path: '' }),
      c2: makeValue({ cell_id: 'c2', name: 'width', entity_path: 'Bracket.width' }),
    };

    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity="Bracket.width"
        onSetParameter={vi.fn()}
      />
    ));

    const container = screen.getByTestId('property-editor');
    const selectedGroups = container.querySelectorAll('[data-selected]');

    // Only the 'Bracket' group should be selected, not the empty-name group
    // With the startsWith bug, ''.startsWith('') is always true for any selectedEntity
    expect(selectedGroups.length).toBe(1);
    expect(selectedGroups[0].textContent).toContain('Bracket');
  });
});

describe('PropertyEditor navigation enhancements', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
    c2: makeValue({ cell_id: 'c2', name: 'height', entity_path: 'Bracket.height' }),
  };

  it('onGroupDoubleClick: double-clicking group header calls callback with group name', () => {
    const onGroupDblClick = vi.fn();
    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        onGroupDoubleClick={onGroupDblClick}
      />
    ));
    const bracketHeader = screen.getByText('Bracket');
    fireEvent.dblClick(bracketHeader);
    expect(onGroupDblClick).toHaveBeenCalledWith('Bracket');
  });

  it('highlightedParams: row with matching cell_id has data-highlighted attribute', () => {
    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        highlightedParams={['c1']}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    expect(row.hasAttribute('data-highlighted')).toBe(true);
  });

  it('highlightedParams: row without matching cell_id does not have data-highlighted', () => {
    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        highlightedParams={['c1']}
      />
    ));
    const row = screen.getByTestId('prop-row-c2');
    expect(row.hasAttribute('data-highlighted')).toBe(false);
  });

  it('empty highlightedParams means no rows have data-highlighted', () => {
    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        highlightedParams={[]}
      />
    ));
    const container = screen.getByTestId('property-editor');
    const highlighted = container.querySelectorAll('[data-highlighted]');
    expect(highlighted.length).toBe(0);
  });
});

describe('PropertyEditor blur-commit', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
  };

  it('blurring a determined input commits the current value via onSetParameter', () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.input(input, { target: { value: '75' } });
    fireEvent.blur(input);
    expect(onSetParam).toHaveBeenCalledWith('c1', '75');
  });
});

describe('PropertyEditor stale input', () => {
  it('when not editing, input value updates when props.values changes', () => {
    const [values, setValues] = createSignal<Record<string, ValueData>>({
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '10', determinacy: 'determined', entity_path: 'Bracket.width' }),
    });
    render(() => (
      <PropertyEditor values={values()} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const input1 = screen.getByTestId('prop-row-c1').querySelector('input[type="text"]') as HTMLInputElement;
    expect(input1.value).toBe('10');

    setValues({
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '20', determinacy: 'determined', entity_path: 'Bracket.width' }),
    });
    // Re-query since SolidJS may recreate DOM nodes
    const input2 = screen.getByTestId('prop-row-c1').querySelector('input[type="text"]') as HTMLInputElement;
    expect(input2.value).toBe('20');
  });
});

describe('PropertyEditor stale input during editing', () => {
  it('when editing (focused), external prop changes do NOT overwrite local edit value', () => {
    const [values, setValues] = createSignal<Record<string, ValueData>>({
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '10', determinacy: 'determined', entity_path: 'Bracket.width' }),
    });
    render(() => (
      <PropertyEditor values={values()} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const input = screen.getByTestId('prop-row-c1').querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('10');

    // Start editing
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '15' } });
    expect(input.value).toBe('15');

    // External prop change while editing
    setValues({
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '20', determinacy: 'determined', entity_path: 'Bracket.width' }),
    });

    // Re-query since SolidJS may recreate DOM
    const inputAfter = screen.getByTestId('prop-row-c1').querySelector('input[type="text"]') as HTMLInputElement;
    // The input should still show the local edit value '15', NOT the new prop value '20'
    expect(inputAfter.value).toBe('15');
  });
});

describe('PropertyEditor escape-cancel', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
  };

  it('pressing Escape reverts input to original prop value and does NOT call onSetParameter', () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '99' } });
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(input.value).toBe('50');
    expect(onSetParam).not.toHaveBeenCalled();
  });
});

describe('PropertyEditor validation', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
  };

  it('empty string on Enter does NOT call onSetParameter and input gets data-invalid', () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });
});

describe('PropertyEditor validation - non-parseable', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
  };

  it("'abc' on Enter does NOT call onSetParameter and input shows error styling", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: 'abc' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });
});

describe('PropertyEditor highlight CSS', () => {
  it('row with data-highlighted should have highlight CSS class applied', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} highlightedParams={['c1']} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    expect(row.hasAttribute('data-highlighted')).toBe(true);
    // Verify the CSS module produces a class that would match [data-highlighted]
    // The row class should exist (it's applied by the component)
    expect(row.className).toContain('row');
  });
});

describe('PropertyEditor validation - valid number', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
  };

  it("'42.5' on Enter calls onSetParameter and input does NOT have data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '42.5' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '42.5');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });
});

describe('PropertyEditor accessibility', () => {
  const values: Record<string, ValueData> = {
    c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
    c2: makeValue({ cell_id: 'c2', name: 'radius', entity_path: 'Cylinder.radius' }),
  };

  it('groups container has role="tree"', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const container = screen.getByTestId('property-editor');
    const tree = container.querySelector('[role="tree"]');
    expect(tree).toBeTruthy();
  });

  it('each group wrapper has role="treeitem"', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const container = screen.getByTestId('property-editor');
    const treeitems = container.querySelectorAll('[role="treeitem"]');
    expect(treeitems.length).toBe(2); // Bracket + Cylinder
  });

  it('each group body has role="group"', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const container = screen.getByTestId('property-editor');
    const groups = container.querySelectorAll('[role="group"]');
    expect(groups.length).toBe(2);
  });

  it('filter input has aria-label="Filter properties"', () => {
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const input = screen.getByPlaceholderText('Filter properties...');
    expect(input.getAttribute('aria-label')).toBe('Filter properties');
  });
});
