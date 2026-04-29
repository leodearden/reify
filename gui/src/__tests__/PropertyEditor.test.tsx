import { describe, it, expect, vi, beforeEach } from 'vitest';
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
    freshness: overrides.freshness ?? 'final',
  };
}

/** Single editable (determined) param — shared fixture for most describe blocks. */
const EDITABLE_C1: Record<string, ValueData> = {
  c1: makeValue({ cell_id: 'c1', name: 'width', value: '50', determinacy: 'determined', entity_path: 'Bracket.width' }),
};

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
    fireEvent.focus(input);
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
  const values = EDITABLE_C1;

  it.each([
    ['75', '75', 'plain integer'],
    ['80mm', '80mm', 'quantity with unit'],
    ['  75 ', '75', 'whitespace-padded number'],
    [' 5mm ', '5mm', 'whitespace-padded quantity'],
    ['1e3', '1e3', 'scientific notation'],
    ['.5', '.5', 'leading-dot decimal'],
    ['-3', '-3', 'negative integer'],
    ['.5mm', '.5mm', 'leading-dot quantity'],
    ['1e3mm', '1e3mm', 'sci-notation quantity'],
    ['-10mm', '-10mm', 'negative quantity'],
    ['1e+3mm', '1e+3mm', 'explicit-plus exponent quantity'],
  ])("blur '%s' (%s) calls onSetParameter with '%s' and no data-invalid", (input, expected) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const el = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(el);
    fireEvent.input(el, { target: { value: input } });
    fireEvent.blur(el);
    expect(onSetParam).toHaveBeenCalledWith('c1', expected);
    // In tests the mock doesn't update the prop, so after editing ends the input shows
    // the original prop value '50'. In production the parent would update values and
    // the input would show '80mm'.
    expect(el.value).toBe('50');
    expect(el.hasAttribute('data-invalid')).toBe(false);
  });

  it.each([
    ['mm80', 'unit-first quantity'],
    ['0x10', 'hex lowercase'],
    ['0X10', 'hex uppercase'],
    ['0o10', 'octal lowercase'],
    ['0O10', 'octal uppercase'],
    ['0b10', 'binary lowercase'],
    ['0B10', 'binary uppercase'],
    ['+5', 'leading plus'],
    ['+0', 'leading plus zero'],
    ['   ', 'whitespace-only'],
  ])("blur '%s' (%s) does NOT call onSetParameter, reverts to '50', no data-invalid", (input) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const el = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(el);
    fireEvent.input(el, { target: { value: input } });
    fireEvent.blur(el);
    expect(onSetParam).not.toHaveBeenCalled();
    expect(el.value).toBe('50');
    expect(el.hasAttribute('data-invalid')).toBe(false);
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
  const values = EDITABLE_C1;

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
  const values = EDITABLE_C1;

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
  const values = EDITABLE_C1;

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

describe('PropertyEditor group header', () => {
  it('group header button has CSS class applied for styling (including user-select)', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const header = screen.getByText('Bracket');
    // The groupHeader class should be applied
    expect(header.className).toContain('groupHeader');
  });
});

describe('PropertyEditor validation - valid number', () => {
  const values = EDITABLE_C1;

  it.each([
    ['42.5', 'decimal'],
    ['-3', 'negative integer'],
    ['.5', 'leading-dot decimal'],
    ['-0.5', 'negative decimal'],
  ])("'%s' (%s) on Enter calls onSetParameter and input does NOT have data-invalid", (validNumber) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: validNumber } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', validNumber);
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });
});

describe('PropertyEditor input tooltip', () => {
  it('value input has title attribute showing the full value', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', value: '123.456', determinacy: 'determined', entity_path: 'Bracket.width' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.getAttribute('title')).toBe('123.456');
  });
});

describe('PropertyEditor validation - trailing non-numeric characters', () => {
  const values = EDITABLE_C1;

  it("'10mm' on Enter DOES call onSetParameter (quantity literal)", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '10mm' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '10mm');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });

  it("'1.5abc' on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1.5abc' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it("'1e3' (scientific notation) on Enter DOES call onSetParameter", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1e3' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '1e3');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });

  it("' 42 ' (whitespace-padded) on Enter submits trimmed '42'", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: ' 42 ' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '42');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });

  it("' 5mm ' (whitespace-padded quantity) on Enter submits trimmed '5mm'", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: ' 5mm ' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '5mm');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });

});

describe('PropertyEditor quantity literal acceptance', () => {
  const values = EDITABLE_C1;

  it.each([
    ['80mm'],
    ['90deg'],
    ['1.5m'],
    ['100cm'],
    ['1rad'],
    ['-10mm'],
    ['1e3mm'],
    ['1e+3mm'],
    ['1.5e-2deg'],
    ['.5mm'],
    ['.25deg'],
  ])("'%s' on Enter DOES call onSetParameter", (qtyLiteral) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: qtyLiteral } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', qtyLiteral);
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });

  it.each([
    ['10xyz'],
    ['mm80'],
    // Leading '+' rejected: QUANTITY_RE uses ^-? (minus-only), so '+10mm' fails even
    // though the exponent group [eE][+-]? does accept '+' (e.g., '1e+3mm' is valid).
    // This matches the .ri grammar which only defines unary minus for number literals.
    ['+10mm'],
    ['mm'],
    ['deg'],
  ])("'%s' on Enter does NOT call onSetParameter", (invalidLiteral) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: invalidLiteral } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it("'10' (plain number, no unit) on Enter DOES call onSetParameter", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '10' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '10');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });
});

describe('Design decision: whitespace between number and unit is rejected', () => {
  // The .ri grammar uses token.immediate to forbid whitespace between number and unit
  // (see tree-sitter-reify/grammar.js:692-699). The frontend QUANTITY_RE enforces this
  // stricter rule. The backend parse_value_string is more lenient (accepts '5 mm') but
  // that is an incidental bug, not a design choice.

  const values = EDITABLE_C1;

  it.each([
    ['5 mm', 'single space'],
    ['5  mm', 'double space'],
    ['5\tmm', 'tab'],
    [' 5 mm ', 'leading + trailing + internal whitespace'],
  ])("'%s' (%s) on Enter does NOT call onSetParameter", (invalidLiteral) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: invalidLiteral } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });
});

describe('PropertyEditor validation - Infinity rejection', () => {
  const values = EDITABLE_C1;

  it("'Infinity' on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: 'Infinity' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it("'-Infinity' on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '-Infinity' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it("'1e999' (overflows to Infinity) on Enter does NOT call onSetParameter", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1e999' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it('dual-guard: 1e999 passes NUM_RE but fails isFinite, proving both checks are necessary', () => {
    // Verify the regex alone would accept '1e999' — it's syntactically valid
    const NUM_RE = /^-?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
    expect(NUM_RE.test('1e999')).toBe(true);
    // But Number('1e999') overflows to Infinity, which isFinite rejects
    expect(Number.isFinite(Number('1e999'))).toBe(false);

    // Confirm the component correctly rejects it
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1e999' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
  });

  it("'1e999mm' (quantity overflow) on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1e999mm' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it("'-1e999deg' (negative quantity overflow) on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '-1e999deg' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });
});

describe('PropertyEditor validation - valid sci-notation quantities still accepted', () => {
  const values = EDITABLE_C1;

  it.each([
    ['1e2mm', 'scientific notation + mm'],
    ['-3.14rad', 'negative decimal + rad'],
    ['0.5cm', 'decimal fraction + cm'],
    ['100deg', 'integer + deg'],
  ])("'%s' (%s) on Enter DOES call onSetParameter", (quantity) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: quantity } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', quantity);
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });
});

describe('PropertyEditor validation - quantity overflow rejection', () => {
  const values = EDITABLE_C1;

  it('QUANTITY_RE accepts overflow strings but Number(strip) reveals Infinity — documents the gap', () => {
    const QUANTITY_RE = /^-?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?(mm|cm|deg|rad|m)$/;
    // The regex happily accepts these — it has no numeric range check
    expect(QUANTITY_RE.test('1e999mm')).toBe(true);
    expect(QUANTITY_RE.test('-1e999deg')).toBe(true);
    expect(QUANTITY_RE.test('1e999m')).toBe(true);
    // But stripping the unit and converting via Number() reveals Infinity
    const strip = (v: string) => Number(v.replace(/(mm|cm|deg|rad|m)$/, ''));
    expect(Number.isFinite(strip('1e999mm'))).toBe(false);
    expect(Number.isFinite(strip('-1e999deg'))).toBe(false);
    expect(Number.isFinite(strip('1e999m'))).toBe(false);
  });

  it("'1e999m' (overflow m) on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '1e999m' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });
});

describe('PropertyEditor data-invalid recovery', () => {
  const values = EDITABLE_C1;
  let input: HTMLInputElement;
  let onSetParam: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const el = row.querySelector('input[type="text"]');
    if (!el) throw new Error('text input not found in prop-row-c1');
    input = el as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: 'abc' } });
    fireEvent.keyDown(input, { key: 'Enter' });
  });

  it('Escape reverts value and clears data-invalid', () => {
    // Precondition: data-invalid should be set after invalid Enter
    expect(input.hasAttribute('data-invalid')).toBe(true);
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(input.value).toBe('50');
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).not.toHaveBeenCalled();
  });

  it('blur reverts value and clears data-invalid', () => {
    // Precondition: data-invalid should be set after invalid Enter
    expect(input.hasAttribute('data-invalid')).toBe(true);
    // Typed value is preserved in editing state until blur reverts it
    expect(input.value).toBe('abc');
    fireEvent.blur(input);
    expect(input.value).toBe('50');
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).not.toHaveBeenCalled();
  });

  it('Escape then valid value + Enter calls onSetParam and clears data-invalid', () => {
    // Precondition: data-invalid should be set from beforeEach
    expect(input.hasAttribute('data-invalid')).toBe(true);
    // Step 1: Escape to recover from invalid state
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(input.value).toBe('50');
    expect(input.hasAttribute('data-invalid')).toBe(false);
    // Step 2: Enter a valid new value and submit
    fireEvent.input(input, { target: { value: '75' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledWith('c1', '75');
    expect(input.hasAttribute('data-invalid')).toBe(false);
  });
});

describe('PropertyEditor validation - hex/octal/binary/leading-plus rejection', () => {
  const values = EDITABLE_C1;

  it.each([
    ['0x10', 'hex lowercase'],
    ['0X10', 'hex uppercase'],
    ['0o10', 'octal lowercase'],
    ['0O10', 'octal uppercase'],
    ['0b10', 'binary lowercase'],
    ['0B10', 'binary uppercase'],
    ['+5', 'leading plus'],
    ['+0', 'leading plus zero'],
    ['+5.5', 'leading plus decimal'],
    ['+.5', 'leading plus leading-dot'],
    ['+1e3', 'leading plus scientific'],
  ])("'%s' (%s) on Enter does NOT call onSetParameter and sets data-invalid", (invalidLiteral) => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: invalidLiteral } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
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

describe('PropertyEditor whitespace-only input rejection', () => {
  const values = EDITABLE_C1;

  it("whitespace-only '   ' on Enter does NOT call onSetParameter and sets data-invalid", () => {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={onSetParam} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '   ' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).not.toHaveBeenCalled();
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

});
describe('PropertyEditor freshness badge', () => {
  it('final freshness renders no freshness badge', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'final' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    expect(screen.queryByTestId('freshness-badge-c1')).toBeNull();
  });

  it('intermediate freshness renders badge with data-freshness="intermediate"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'intermediate' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    expect(badge).toBeTruthy();
    expect(badge.getAttribute('data-freshness')).toBe('intermediate');
    expect(badge.getAttribute('aria-label')).toBe('freshness intermediate');
  });

  it('pending freshness renders badge with data-freshness="pending"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'pending' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    expect(badge).toBeTruthy();
    expect(badge.getAttribute('data-freshness')).toBe('pending');
    expect(badge.getAttribute('aria-label')).toBe('freshness pending');
  });

  it('failed freshness renders badge with data-freshness="failed"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'failed' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    expect(badge).toBeTruthy();
    expect(badge.getAttribute('data-freshness')).toBe('failed');
    expect(badge.getAttribute('aria-label')).toBe('freshness failed');
  });

  it('freshness badge and determinacy badge are both visible simultaneously', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'failed', determinacy: 'determined' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const freshnessBadge = screen.getByTestId('freshness-badge-c1');
    expect(freshnessBadge.getAttribute('data-freshness')).toBe('failed');
    const container = screen.getByTestId('property-editor');
    const determinacyBadge = container.querySelector('[data-determinacy="determined"]');
    expect(determinacyBadge).toBeTruthy();
  });
});
