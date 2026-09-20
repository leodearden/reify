import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { PropertyEditor } from '../panels/PropertyEditor';
import type { UnitLadderMap, ValueData } from '../types';
import { loadUnitPreference, saveUnitPreference } from '../stores/unitPreferences';
import { BASE_UNIT_LABELS, buildQuantityRe, NUMBER_RE } from '../stores/unitLadder';

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
    reason: overrides.reason,
    last_substantive_value: overrides.last_substantive_value,
    dimension: overrides.dimension,
    si_value: overrides.si_value,
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

    // BracketMount's rows should still be visible (selected group stays expanded).
    // Note: the breadcrumb header also shows "height" (leaf of the selected entity path),
    // so use getAllByText to handle both occurrences gracefully.
    expect(screen.getAllByText('height').length).toBeGreaterThan(0);
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
    // Leading '+' rejected: the numeric part of `buildQuantityRe` is `-?`
    // (minus-only), so '+10mm' fails even though the exponent group [eE][+-]?
    // does accept '+' (e.g., '1e+3mm' is valid). This matches the .ri grammar
    // which only defines unary minus for number literals.
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
  // (see tree-sitter-reify/grammar.js:692-699). The frontend's `buildQuantityRe`
  // enforces this stricter rule. The backend parse_value_string is more lenient
  // (accepts '5 mm') but that is an incidental bug, not a design choice.

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

  it('dual-guard: 1e999 passes NUMBER_RE but fails isFinite, proving both checks are necessary', () => {
    // Verify the regex alone would accept '1e999' — it's syntactically valid.
    // Built from the SINGLE definition of the numeric grammar (`NUMBER_RE` in
    // ../stores/unitLadder, task #6028) rather than a local re-declaration:
    // this test used to carry its own byte-for-byte copy.
    expect(NUMBER_RE.test('1e999')).toBe(true);
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

  it('the quantity regex accepts overflow strings but Number(group 1) reveals Infinity — documents the gap', () => {
    // Built from the SINGLE definition of the quantity grammar
    // (`buildQuantityRe` in ../stores/unitLadder, task #6028) rather than a
    // local re-declaration. This test used to carry its own inline copy of
    // both the regex and the unit alternation, which is precisely the drift
    // risk #6028 removes: the grammar now has one definition in the repo.
    const re = buildQuantityRe(BASE_UNIT_LABELS);
    // The regex happily accepts these — it has no numeric range check
    expect(re.test('1e999mm')).toBe(true);
    expect(re.test('-1e999deg')).toBe(true);
    expect(re.test('1e999m')).toBe(true);
    // But converting capture group 1 — the whole signed numeric literal —
    // reveals Infinity. This is why the `Number.isFinite` guard in
    // `isValidValue` is load-bearing and not redundant with the regex.
    const numeric = (v: string) => Number(re.exec(v)![1]);
    expect(Number.isFinite(numeric('1e999mm'))).toBe(false);
    expect(Number.isFinite(numeric('-1e999deg'))).toBe(false);
    expect(Number.isFinite(numeric('1e999m'))).toBe(false);
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

describe('PropertyEditor quantity literal acceptance with a live unit ladder (task #6028)', () => {
  /**
   * Five cells covering every branch of the per-cell alphabet resolution:
   * a Volume cell and a Density cell (both covered by the ladder map), a cell
   * whose dimension the map does NOT cover, a dimensionless cell, and — for the
   * coverage-conditional bare-number rule (task #5757 amend) — an uncovered
   * cell that DOES carry a unit badge.
   *
   * c5 is the shape of the real in-tree case, which c3 does not reach: 16
   * `param … : Money` declarations across `examples/*.ri` render with a `USD`
   * badge and an si_value, and `USD` is parseable by neither this panel's
   * alphabet nor the engine's composed index (it lives only in the compiler's
   * per-module `UnitRegistry`). c3 has no badge at all, so it cannot show that
   * a present-but-unparseable unit is still not enough to make the cell
   * expressible.
   */
  function tankValues(): Record<string, ValueData> {
    return {
      c1: makeValue({
        cell_id: 'c1',
        name: 'capacity',
        entity_path: 'Tank.capacity',
        value: '7045002.24',
        dimension: 'Volume',
        si_value: 0.00704500224,
      }),
      c2: makeValue({
        cell_id: 'c2',
        name: 'material_density',
        entity_path: 'Tank.material_density',
        value: '7.8',
        dimension: 'Density',
        si_value: 7800,
      }),
      c3: makeValue({
        cell_id: 'c3',
        name: 'preload',
        entity_path: 'Tank.preload',
        value: '12',
        dimension: 'Torque',
        si_value: 12,
      }),
      c4: makeValue({
        cell_id: 'c4',
        name: 'ratio',
        entity_path: 'Tank.ratio',
        value: '3',
      }),
      c5: makeValue({
        cell_id: 'c5',
        name: 'unit_cost',
        entity_path: 'Tank.unit_cost',
        value: '5',
        unit: 'USD',
        dimension: 'Money',
        si_value: 5,
      }),
    };
  }

  /** Type `literal` into `cellId` (default: the Volume cell) and report acceptance. */
  function typeInto(
    literal: string,
    unitLadders?: UnitLadderMap,
    cellId = 'c1',
  ): { accepted: boolean; onSetParam: ReturnType<typeof vi.fn> } {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={tankValues()}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={unitLadders}
      />
    ));
    const row = screen.getByTestId(`prop-row-${cellId}`);
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: literal } });
    fireEvent.keyDown(input, { key: 'Enter' });
    const invalid = input.hasAttribute('data-invalid');
    const called = onSetParam.mock.calls.length > 0;
    // The two signals must never disagree — an accepted literal is submitted
    // verbatim and clears data-invalid; a rejected one does neither.
    expect(called).toBe(!invalid);
    if (called) expect(onSetParam).toHaveBeenCalledWith(cellId, literal);
    return { accepted: called, onSetParam };
  }

  // The SAME curated ladders in both spellings. Every case below runs against
  // both, so this block is agnostic to whether task #5788's relabel has landed
  // — and its addendum L5 fixture sweep cannot turn these red.
  const LADDERS_BY_SPELLING: Array<[string, UnitLadderMap]> = [
    [
      'superscript spelling (pre-#5788)',
      {
        Volume: [
          { label: 'mm³', si_scale: 1e-9, is_default: true },
          { label: 'cm³', si_scale: 1e-6, is_default: false },
          { label: 'L', si_scale: 1e-3, is_default: false },
          { label: 'm³', si_scale: 1.0, is_default: false },
        ],
        Density: [
          { label: 'kg/m³', si_scale: 1.0, is_default: true },
          { label: 'g/cm³', si_scale: 1000.0, is_default: false },
        ],
      },
    ],
    [
      'ASCII spelling (post-#5788)',
      {
        Volume: [
          { label: 'mm^3', si_scale: 1e-9, is_default: true },
          { label: 'cm^3', si_scale: 1e-6, is_default: false },
          { label: 'L', si_scale: 1e-3, is_default: false },
          { label: 'm^3', si_scale: 1.0, is_default: false },
        ],
        Density: [
          { label: 'kg/m^3', si_scale: 1.0, is_default: true },
          { label: 'g/cm^3', si_scale: 1000.0, is_default: false },
        ],
      },
    ],
  ];

  describe.each(LADDERS_BY_SPELLING)('against the %s', (_spelling, ladders) => {
    // Before #6028 every one of these was rejected: the alphabet was five
    // hard-coded units. The cell's OWN dimension supplies the widening.
    it.each([
      ['5mm^3'],
      ['5cm^3'],
      ['5m^3'],
      ['5L'],
      // The baseline floor is still in the alphabet alongside the ladders.
      ['80mm'],
      ['90deg'],
    ])('accepts the ASCII-spelled literal %s on the Volume cell', (literal) => {
      expect(typeInto(literal, ladders).accepted).toBe(true);
    });

    it.each([
      ['7.8kg/m^3'],
      ['7.8g/cm^3'],
      ['80mm'],
      ['90deg'],
    ])('accepts the ASCII-spelled literal %s on the Density cell', (literal) => {
      expect(typeInto(literal, ladders, 'c2').accepted).toBe(true);
    });

    // PER-CELL SCOPING. The alphabet is the static floor plus just the ladder
    // the cell's own dimension advertises — not the union over all dimensions.
    // A cross-dimension literal is rejected inline, with the typed text kept
    // for correction, instead of being committed and then refused by the
    // backend on the worst-feedback path (typed text discarded, async toast).
    it.each([
      ['7.8kg/m^3', 'c1', 'a Density literal on the Volume cell'],
      ['7.8g/cm^3', 'c1', 'a Density literal on the Volume cell'],
      ['5mm^3', 'c2', 'a Volume literal on the Density cell'],
      ['5L', 'c2', 'a Volume literal on the Density cell'],
    ])('rejects %s on %s — %s', (literal, cellId) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(false);
    });

    // Superscript spellings have never been .ri-parseable, in either era — so
    // normalizing the ladder labels into the alphabet must not smuggle them in.
    it.each([
      ['5mm³', 'c1'],
      ['5cm³', 'c1'],
      ['5m³', 'c1'],
      ['7.8kg/m³', 'c2'],
    ])('still rejects the superscript-spelled literal %s', (literal, cellId) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(false);
    });

    // Widening the alphabet must not weaken any other rule.
    it.each([
      ['10xyz'],
      ['+10mm'],
      ['mm^3'],
      ['5 mm^3'],
      ['1e999L'],
      ['5kPa'],
    ])('still rejects %s', (literal) => {
      expect(typeInto(literal, ladders).accepted).toBe(false);
    });

    // The fallback branches. A cell whose dimension the ladder map does not
    // cover, and a cell with no dimension at all, have no ladder to scope to —
    // so they fall back to the union rather than narrowing to the bare floor.
    // Non-narrowing is the deliberate choice: it keeps the ladders-absent
    // behaviour byte-identical to pre-#6028 for every such cell.
    it.each([
      ['5mm^3', 'c3', 'an uncovered dimension'],
      ['7.8kg/m^3', 'c3', 'an uncovered dimension'],
      ['5mm^3', 'c4', 'no dimension'],
      ['7.8kg/m^3', 'c4', 'no dimension'],
      ['80mm', 'c3', 'an uncovered dimension'],
      ['80mm', 'c4', 'no dimension'],
    ])('falls back to the union for %s on cell %s, which has %s', (literal, cellId) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(true);
    });

    // ── task #5757: a bare number is not a valid literal for a DIMENSIONED cell ──
    //
    // `20` in a Volume cell is ambiguous — 20 what? — and the engine used to
    // resolve that ambiguity silently, reading it as 20 CUBIC METRES:
    // `parse_value_string` yields Value::Int, reify-eval's
    // `value_type_kind_matches` treats Int/Real as a dimension WILDCARD for
    // Type::Scalar, and `validate_param_override` then skips its dimension check
    // entirely because the value is not a Value::Scalar.
    //
    // The backend now refuses it (`parse_value_string_for_cell`). These cases
    // pin the INLINE mirror: rejecting here is what keeps the typed text on
    // screen under `data-invalid` for correction, instead of discarding it and
    // replacing it with an async error toast.
    it.each([
      ['20', 'c1', 'Volume'],
      ['7045002.24', 'c1', 'Volume'],
      ['-5', 'c1', 'Volume'],
      ['1e3', 'c1', 'Volume'],
      ['20', 'c2', 'Density'],
      ['7.8', 'c2', 'Density'],
    ])('rejects the bare number %s on cell %s, which is dimensioned — %s', (literal, cellId) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(false);
    });

    it.each([
      ['20'],
      ['7045002.24'],
      ['-5'],
      ['1e3'],
    ])('still accepts the bare number %s on the undimensioned cell', (literal) => {
      // The gate must not touch dimensionless cells — every ratio/count slider
      // in the panel is one. `typeInto` also asserts the value is submitted
      // VERBATIM, so this covers the accept side end-to-end.
      expect(typeInto(literal, ladders, 'c4').accepted).toBe(true);
    });

    // c3's dimension has NO curated ladder, so the gate does not fire there.
    // The rule keys on EXPRESSIBILITY, not dimensionedness: `20` in a Volume
    // cell is ambiguous because `20mm^3` and `20L` were both offered, but a
    // Torque cell's picker offers nothing and its alphabet admits nothing, so
    // refusing the bare number disambiguates nothing — it just removes the
    // row's last accepted input. The backend agrees (`dimension_requires_unit`
    // returns None for it), so refusing here would discard input the engine
    // would have taken.
    it.each([
      ['20', 'c3', 'Torque (no curated ladder)'],
      ['-5', 'c3', 'Torque (no curated ladder)'],
    ])('accepts the bare number %s on cell %s, whose dimension is inexpressible — %s', (literal, cellId) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(true);
    });

    // c5 — the real in-tree Money shape. A badge is not expressibility: the
    // cell displays `USD`, but no ladder carries it and neither alphabet admits
    // it, so the cell is uncovered exactly as c3 is.
    it.each([
      ['6'],
      ['0.5'],
      ['-2'],
    ])('accepts the bare number %s on the badged-but-uncovered Money cell', (literal) => {
      // `typeInto` also asserts the literal reaches `onSetParam` VERBATIM, so
      // this pins the submit side too — the panel must not helpfully append the
      // badge on the way out.
      expect(typeInto(literal, ladders, 'c5').accepted).toBe(true);
    });

    it.each([
      ['6USD'],
      ['5USD'],
    ])('still refuses %s inline on the Money cell — the engine cannot parse it either', (literal) => {
      // Matching the backend exactly is the whole contract: `USD` is reachable
      // only through the compiler's per-module `UnitRegistry`, which
      // `COMPOSED_UNIT_INDEX` excludes, so `parse_value_string` answers
      // `Cannot parse value '6USD'`. Admitting it here would re-open the
      // panel-accepts / engine-refuses gap task #5757 exists to close, just for
      // a different label.
      expect(typeInto(literal, ladders, 'c5').accepted).toBe(false);
    });

    it('seeds the badged-but-uncovered Money cell with the BARE magnitude', () => {
      // `editSeedUnitLabel`'s `?? val.unit` fallback must not be allowed to
      // pre-fill `5USD` here: that is text this very gate refuses, so focus+Enter
      // on an untouched row would set data-invalid and submit nothing — the
      // ergonomic hazard the #5757 amendment closed for covered cells. An
      // uncovered cell reaches the bare-magnitude branch before the label lookup
      // happens at all, and step-19's `isValidValue` re-check is the backstop
      // that keeps it true if that ordering ever changes.
      const onSetParam = vi.fn();
      render(() => (
        <PropertyEditor
          values={tankValues()}
          selectedEntity={null}
          onSetParameter={onSetParam}
          unitLadders={ladders}
        />
      ));
      const row = screen.getByTestId('prop-row-c5');
      const input = row.querySelector('input[type="text"]') as HTMLInputElement;

      fireEvent.focus(input);
      expect(input.value).toBe('5');

      // …so an untouched commit is a true no-op again.
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(input.hasAttribute('data-invalid')).toBe(false);
      expect(onSetParam).toHaveBeenCalledWith('c5', '5');
    });

    // Gating the bare-number branch must not weaken the QUANTITY branch beside
    // it: the widened per-cell alphabet, the cross-dimension narrowing, the
    // superscript refusal and the non-quantity rules are all unchanged. Their
    // primary coverage is the it.each blocks above; these are the specific
    // pairings where a mis-scoped gate would show up first.
    it.each([
      ['5L', 'c1', true],
      ['5mm^3', 'c1', true],
      ['80mm', 'c1', true],
      ['2kg/m^3', 'c2', true],
      // Cross-dimension is still rejected by the per-cell alphabet…
      ['2kg/m^3', 'c1', false],
      ['5L', 'c2', false],
      // …superscript input is still rejected…
      ['5mm³', 'c1', false],
      // …and the non-quantity rules still hold.
      ['10xyz', 'c1', false],
      ['5 mm^3', 'c1', false],
      ['1e999L', 'c1', false],
    ])('leaves the quantity branch unweakened: %s on %s → accepted=%s', (literal, cellId, expected) => {
      expect(typeInto(literal, ladders, cellId).accepted).toBe(expected);
    });
  });

  // The guard: the ladder-less path has not moved. The widening is scoped to
  // exactly what the backend advertises, and the static floor is intact — this
  // is what keeps every pre-existing validation test byte-identical.
  describe('without unitLadders (fetch not resolved / failed)', () => {
    it.each([['5mm^3'], ['5L'], ['7.8kg/m^3']])('rejects %s', (literal) => {
      expect(typeInto(literal, undefined).accepted).toBe(false);
    });

    it.each([['80mm'], ['90deg'], ['1.5m'], ['100cm'], ['1rad']])('accepts %s', (literal) => {
      expect(typeInto(literal, undefined).accepted).toBe(true);
    });
  });
});

/**
 * Task #5757 amendment: the bare-number gate on the LADDER-LESS path.
 *
 * `get_unit_ladders` is a one-shot best-effort fetch — `App.tsx` logs on
 * rejection, toasts "Unit ladders unavailable", and leaves `unitLadders()` as
 * `{}` — and there is a window between mount and resolution where it is
 * `undefined` too. The ENGINE has no such window: its `LADDER_COVERAGE` is built
 * in-process from the Rust-authored curated table and is always populated, so
 * `set_parameter` keeps refusing a bare number in a Length cell regardless.
 *
 * So a rule that read only the fetched map disagreed with the engine on exactly
 * this path, in the direction that hurts: the panel accepted `80`, `editSeed`
 * seeded it, and the engine answered "expects Length, got the bare number '80'"
 * behind an async toast that discards what the user typed. That is the failure
 * #5757 exists to remove, re-entered through the degraded path — and a
 * REGRESSION on it, since before the task the bare number committed fine.
 *
 * `acceptsBareNumber` therefore keeps gating the `BASE_UNIT_DIMENSIONS` floor
 * here, and this block pins the consequence that matters: a Length row is still
 * EDITABLE with the fetch failed. That is what makes the floor safe to gate —
 * `quantityUnitAlphabet` unions `BASE_UNIT_LABELS` in on every path, so the unit
 * the seed supplies is one this panel still accepts, and the Rust-side
 * `every_dimension_the_frontend_floor_gates_is_gated_here_too` pins that the
 * engine accepts it too.
 */
describe('PropertyEditor with the unit-ladder fetch failed (task #5757 amendment)', () => {
  const LADDER_LESS: Record<string, ValueData> = {
    len: makeValue({
      cell_id: 'len',
      name: 'width',
      entity_path: 'Bracket.width',
      value: '80',
      unit: 'mm',
      dimension: 'Length',
      si_value: 0.08,
    }),
    // Below the floor: the panel cannot describe Torque with no ladder data, so
    // the gate must stay open for it exactly as it does with ladders present.
    tor: makeValue({
      cell_id: 'tor',
      name: 'preload',
      entity_path: 'Bracket.preload',
      value: '12',
      unit: 'N·m',
      dimension: 'Torque',
      si_value: 12,
    }),
  };

  function inputFor(cellId: string) {
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={LADDER_LESS}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={undefined}
      />
    ));
    const row = screen.getByTestId(`prop-row-${cellId}`);
    return { input: row.querySelector('input[type="text"]') as HTMLInputElement, onSetParam };
  }

  it('seeds the Length row WITH ITS UNIT, so an untouched commit is still a no-op', () => {
    // There is no ladder to read a default rung from, so this is the one place
    // `editSeedUnitLabel`'s `?? val.unit` fallback is load-bearing. Without it
    // the seed would be the bare `80` — text this very gate now refuses — and
    // focus+Enter on an untouched row would set data-invalid and submit nothing.
    const { input, onSetParam } = inputFor('len');
    fireEvent.focus(input);
    expect(input.value).toBe('80mm');

    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('len', '80mm');
  });

  it('keeps the commonest edit working: change the digits, keep the unit', () => {
    const { input, onSetParam } = inputFor('len');
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '90mm' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('len', '90mm');
  });

  it('refuses a bare number on the Length row INLINE, matching the engine', () => {
    const { input, onSetParam } = inputFor('len');
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '90' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(input.hasAttribute('data-invalid')).toBe(true);
    expect(onSetParam).not.toHaveBeenCalled();
    // The whole point of mirroring the backend inline: the typed text is still
    // on screen to correct, rather than discarded behind an async toast.
    expect(input.value).toBe('90');
  });

  it('still accepts a bare number on a BELOW-FLOOR dimensioned row', () => {
    // Torque is not on the floor and has no ladder here, so nothing can express
    // a unit for it — refusing the bare number would remove the row's last
    // accepted input, and the engine would have taken it.
    const { input, onSetParam } = inputFor('tor');
    fireEvent.focus(input);
    expect(input.value).toBe('12');

    fireEvent.input(input, { target: { value: '15' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('tor', '15');
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

describe('PropertyEditor freshness badge title', () => {
  it('intermediate freshness badge has a title containing "intermediate"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'intermediate' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    const title = badge.getAttribute('title') ?? '';
    expect(title.toLowerCase()).toContain('intermediate');
  });

  it('pending freshness badge has a title containing "pending"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'pending' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    const title = badge.getAttribute('title') ?? '';
    expect(title.toLowerCase()).toContain('pending');
  });

  it('failed freshness badge has a title containing "failed"', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'failed' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    const title = badge.getAttribute('title') ?? '';
    expect(title.toLowerCase()).toContain('failed');
  });

  it('freshness badge aria-label is preserved unchanged (freshness intermediate)', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({ cell_id: 'c1', name: 'width', entity_path: 'Bracket.width', freshness: 'intermediate' }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const badge = screen.getByTestId('freshness-badge-c1');
    expect(badge.getAttribute('aria-label')).toBe('freshness intermediate');
  });
});

describe('PropertyEditor — selection breadcrumb header', () => {
  it('(a) with selectedEntity="Bracket.width" renders selection-breadcrumb with leaf "width"', () => {
    render(() => (
      <PropertyEditor
        values={EDITABLE_C1}
        selectedEntity="Bracket.width"
        onSetParameter={vi.fn()}
      />
    ));
    // Breadcrumb container must be present
    expect(screen.getByTestId('selection-breadcrumb')).toBeTruthy();
    // Leaf crumb must display the last segment
    const leaf = screen.getByTestId('breadcrumb-leaf');
    expect(leaf.textContent).toBe('width');
  });

  it('(b) with selectedEntity={null} shows "No selection" placeholder', () => {
    render(() => (
      <PropertyEditor
        values={{}}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));
    expect(screen.getByTestId('selection-breadcrumb')).toBeTruthy();
    expect(screen.getByText('No selection')).toBeTruthy();
  });

  it('(b) panel-title "Parameters" still renders alongside the breadcrumb', () => {
    render(() => (
      <PropertyEditor
        values={{}}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));
    // The existing "Parameters" title must remain
    const title = screen.getByText('Parameters');
    expect(title).toBeTruthy();
    // Breadcrumb should also be present
    expect(screen.getByTestId('selection-breadcrumb')).toBeTruthy();
  });
});

// --- undef-cause reason rendering (§4.4 ε, step-9/10) ---

describe('PropertyEditor undef-reason surface', () => {
  it('renders an undef-reason span for an unbound param and no span for a determined param', () => {
    const values: Record<string, ValueData> = {
      c_undef: makeValue({
        cell_id: 'c_undef',
        name: 'outer_d',
        determinacy: 'undetermined',
        reason: 'outer_d unbound',
      }),
      c_det: makeValue({
        cell_id: 'c_det',
        name: 'width',
        determinacy: 'determined',
      }),
    };

    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
      />
    ));

    // The undef param must have a reason span with the cause text.
    const reasonSpan = screen.getByTestId('undef-reason-c_undef');
    expect(reasonSpan).toBeTruthy();
    expect(reasonSpan.textContent).toBe('outer_d unbound');
    // The span title attribute enables hover tooltip.
    expect(reasonSpan.getAttribute('title')).toContain('outer_d unbound');

    // The determined param must have no such span.
    const detReason = screen.queryByTestId('undef-reason-c_det');
    expect(detReason).toBeNull();
  });
});

// --- Demand-pruned (Pending) cell displays its last-substantive value (§8 γ #4739) ---
//
// step-15 (RED until step-16): a Pending cell — whose body was demand-pruned by a
// warm selective build — must DISPLAY its last **substantive** (prior good) value,
// not the current un-recomputed one (arch §8 prune-safety scenario 3: "the
// displayed number equals the last good value"), while the ⚠ pending freshness
// badge stays visible. A Final cell is unaffected and shows its current value.
//
// RED today: `types.ts` lacks `last_substantive_value` and `PropertyEditor` renders
// `val.value`, so the pending cell shows the stale '99' instead of '42'. GREEN
// after step-16 adds the field and the prior-value display fallback.
describe('PropertyEditor pending last-substantive value (#4739 γ)', () => {
  it('a pending cell displays last_substantive_value instead of the stale current value, with the pending badge still shown', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({
        cell_id: 'c1',
        name: 'width',
        entity_path: 'Bracket.width',
        determinacy: 'determined',
        value: '99', // stale current value (un-recomputed under prune)
        freshness: 'pending',
        last_substantive_value: '42', // last good value
      }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    // The displayed (input) value is the prior good value, not the stale current one.
    const input = screen
      .getByTestId('prop-row-c1')
      .querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('42');
    // The title (hover) mirrors the displayed value.
    expect(input.getAttribute('title')).toBe('42');
    // The ⚠ pending freshness badge is still present alongside the prior value.
    const badge = screen.getByTestId('freshness-badge-c1');
    expect(badge.getAttribute('data-freshness')).toBe('pending');
  });

  it('a final cell is unaffected and displays its current value', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({
        cell_id: 'c1',
        name: 'width',
        entity_path: 'Bracket.width',
        determinacy: 'determined',
        value: '50',
        freshness: 'final',
      }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const input = screen
      .getByTestId('prop-row-c1')
      .querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('50');
    expect(screen.queryByTestId('freshness-badge-c1')).toBeNull();
  });

  it('a pending non-determined (readonly) cell displays last_substantive_value in the readonly span', () => {
    const values: Record<string, ValueData> = {
      c1: makeValue({
        cell_id: 'c1',
        name: 'width',
        entity_path: 'Bracket.width',
        determinacy: 'undef', // renders the readonly fallback span, not an input
        value: '99',
        freshness: 'pending',
        last_substantive_value: '42',
      }),
    };
    render(() => (
      <PropertyEditor values={values} selectedEntity={null} onSetParameter={vi.fn()} />
    ));
    const row = screen.getByTestId('prop-row-c1');
    expect(row.querySelector('input[type="text"]')).toBeNull();
    // The readonly span shows the prior good value, not the stale current one.
    expect(row.textContent).toContain('42');
    expect(row.textContent).not.toContain('99');
  });
});

describe('PropertyEditor per-cell unit picker (task #5199)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  const VOLUME_LADDER: UnitLadderMap = {
    Volume: [
      { label: 'mm^3', si_scale: 1e-9, is_default: true },
      { label: 'cm^3', si_scale: 1e-6, is_default: false },
      { label: 'L', si_scale: 1e-3, is_default: false },
      { label: 'm^3', si_scale: 1.0, is_default: false },
    ],
  };

  function capacityValues(overrides: Partial<ValueData> = {}): Record<string, ValueData> {
    return {
      c1: makeValue({
        cell_id: 'c1',
        name: 'capacity',
        entity_path: 'Tank.capacity',
        value: '7045002.24',
        unit: 'mm^3',
        dimension: 'Volume',
        si_value: 0.00704500224,
        ...overrides,
      }),
    };
  }

  it('(a) a Volume cell with si_value renders a unit-picker select with the ladder option labels', () => {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    const labels = Array.from(select.options).map((o) => o.value);
    expect(labels).toEqual(['mm^3', 'cm^3', 'L', 'm^3']);
  });

  it('(b) selecting "L" shows the converted displayed value and label, with no duplicate canonical number', () => {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'L' } });
    expect(select.value).toBe('L');
    // The PRIMARY value field itself must track the picked unit (task #5199
    // amend: previously only the badge's secondary number reflected the
    // pick, so the row showed '7045002.24' in the field and '7.04500224 L'
    // in the badge for the same cell).
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('7.04500224');
    // The badge/select must carry ONLY the unit label — no second numeric
    // value living alongside the select.
    const badge = select.parentElement as HTMLElement;
    expect(badge.textContent).toBe(select.textContent);
  });

  it('(c) a cell whose unit is pre-persisted renders that unit + converted value on first render', () => {
    saveUnitPreference('c1', 'L');
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    expect(select.value).toBe('L');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('7.04500224');
  });

  it('(d) changing the select persists the choice via saveUnitPreference', () => {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'L' } });
    expect(loadUnitPreference('c1')).toBe('L');
  });

  it('(e) the default-unit selection shows the backend value verbatim', () => {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    expect(select.value).toBe('mm^3');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('7045002.24');
  });

  it('(f) a cell without si_value/dimension still renders the plain static badge, no select', () => {
    render(() => (
      <PropertyEditor
        values={EDITABLE_C1}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    expect(screen.queryByTestId('unit-select-c1')).toBeNull();
    expect(screen.getByTestId('prop-row-c1').textContent).toContain('mm');
  });

  it('(g) starting to edit while a non-default unit is picked seeds the edit buffer with the canonical value AND unit', () => {
    // Guards the fix that came with driving the primary field from the
    // picker: editing must stay anchored to the canonical backend magnitude
    // so an unmodified commit does not silently rewrite the parameter by the
    // picked unit's conversion factor (task #5199 amend) — and, since the
    // #5757 amendment, must seed the canonical UNIT alongside it so that
    // unmodified commit is still ACCEPTED (see `editSeed` in PropertyEditor).
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'L' } });
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('7.04500224');

    // Focus reseeds to the CANONICAL magnitude — and, since task #5757, carries
    // the canonical UNIT with it. Both halves matter: the magnitude is what
    // stops the picked 'L' rescaling the value, and the unit is what keeps the
    // seed a literal this panel will actually accept, now that a bare number is
    // refused at a dimensioned position (PRD ratified decision (1), §6 row 16).
    fireEvent.focus(input);
    expect(input.value).toBe('7045002.24mm^3');

    // So an unmodified commit is a TRUE NO-OP again: it submits verbatim, and
    // the value it submits is the canonical one. Before the #5199 fix this
    // submitted the picker-converted `7.04500224` as if it were mm³ — a 1000×
    // error; before the #5757 amendment it submitted nothing at all, because
    // the panel was seeding text its own validator refused.
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('c1', '7045002.24mm^3');
  });

  it('(g2) editing only the DIGITS of a dimensioned cell keeps the seeded unit', () => {
    // The most common edit in the panel by far, and the one the #5757
    // bare-number gate would have broken without the unit-bearing seed: click
    // in, change the number, commit. The user never retypes the unit, so it has
    // to already be there — and the committed literal has to carry it, or the
    // engine refuses the very edit the panel just accepted.
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;

    fireEvent.focus(input);
    expect(input.value).toBe('7045002.24mm^3');

    // Simulate editing just the digits: the unit the seed supplied is still
    // there, and only the magnitude changed.
    fireEvent.input(input, { target: { value: '90mm^3' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('c1', '90mm^3');
  });

  it('(g3) a cell whose dimension has no curated ladder seeds the bare magnitude, and commits it', () => {
    // No longer a degradation — a consistency. An uncovered dimension has no
    // label this gate accepts (`val.unit` here is a composed base-SI spelling,
    // `kg·m^2·s^-2`, that neither this gate nor `parse_value_string` reads), so
    // the seed is the bare magnitude; and since the bare-number rule keys on
    // EXPRESSIBILITY, that seed is also a literal the panel and the engine both
    // accept. Focus+Enter on an untouched row is a no-op here for the same
    // reason it is on a covered cell: the seed is valid.
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={{
          c9: makeValue({
            cell_id: 'c9',
            name: 'preload',
            entity_path: 'Tank.preload',
            value: '12',
            unit: 'kg·m^2·s^-2',
            dimension: 'Torque',
            si_value: 12,
          }),
        }}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c9');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;

    fireEvent.focus(input);
    expect(input.value).toBe('12');

    fireEvent.keyDown(input, { key: 'Enter' });
    expect(input.hasAttribute('data-invalid')).toBe(false);
    expect(onSetParam).toHaveBeenCalledWith('c9', '12');
  });

  it('(h) focusing a cell while a non-default unit is picked shows an explicit "editing in <default>" hint', () => {
    // Reviewer finding (task #5199 amend, robustness): the edit buffer
    // silently reseeds to the canonical magnitude on focus while the
    // <select> keeps reading the picked unit — this hint makes that switch
    // explicit instead of leaving it silent.
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;

    // No hint before a non-default unit is even picked.
    expect(screen.queryByTestId('unit-edit-hint-c1')).toBeNull();

    fireEvent.change(select, { target: { value: 'L' } });
    // Still no hint at rest — only while actually editing.
    expect(screen.queryByTestId('unit-edit-hint-c1')).toBeNull();

    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    expect(screen.getByTestId('unit-edit-hint-c1').textContent).toContain('mm^3');

    // Committing ends the edit and the hint disappears again. The literal must
    // carry a unit: since task #5757 a bare magnitude is refused for a
    // dimensioned cell, and a refused commit keeps the row in edit mode (that
    // is the point of the inline `data-invalid` path — the typed text stays on
    // screen for correction), so the hint would correctly still be showing.
    fireEvent.input(input, { target: { value: '7045002.24mm^3' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.queryByTestId('unit-edit-hint-c1')).toBeNull();
  });

  it('(h2) no edit-hint appears when editing while the default unit is selected', () => {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    expect(screen.queryByTestId('unit-edit-hint-c1')).toBeNull();
  });

  it('(h3) the edit-hint is styled distinctly from a plain unit badge so it is not mistaken for one', () => {
    // Reviewer finding (task #5199 amend, robustness): the hint mitigating
    // the canonical-vs-picked-unit footgun previously reused the plain
    // `.unitBadge` style, making it "a small badge that's easy to miss".
    // Pin that it now carries its own, more visually weighty class.
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'L' } });
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);

    const hint = screen.getByTestId('unit-edit-hint-c1');
    expect(hint.className).toContain('unitEditHint');
    expect(hint.className).not.toContain('unitBadge');
  });

  it('(i) typing a NEW value while a non-default unit is picked commits the raw typed literal, uninterpreted by the picked unit', () => {
    // Reviewer finding (task #5199 amend, robustness): test (g) only covers
    // the no-op focus+Enter case. This covers the actually-lossy case: the
    // user types a fresh value while "L" is selected. onSetParameter must
    // receive exactly what was typed (submission is never silently
    // converted by the picked unit) — documented here so a future change to
    // this contract cannot land unnoticed.
    //
    // Task #5757 changes only the GRAMMAR of what may be typed, not this
    // contract: a dimensioned cell needs a unit, so the fresh value is
    // `9999mm^3` rather than a bare `9999`. The point still stands and is
    // arguably sharper — `9999mm^3` is submitted verbatim, NOT reinterpreted
    // as 9999 litres because the picker happens to read "L".
    const onSetParam = vi.fn();
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={onSetParam}
        unitLadders={VOLUME_LADDER}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    const select = screen.getByTestId('unit-select-c1') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'L' } });

    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.input(input, { target: { value: '9999mm^3' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(onSetParam).toHaveBeenCalledWith('c1', '9999mm^3');

    // The bare spelling is what the gate refuses, and refusing it is what
    // stops the engine reading `9999` as 9999 CUBIC METRES.
    fireEvent.input(input, { target: { value: '9999' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSetParam).toHaveBeenCalledTimes(1);
    expect(input.hasAttribute('data-invalid')).toBe(true);
  });

  it('(j) a demand-pruned cell showing last_substantive_value suppresses the picker entirely', () => {
    // Reviewer finding (task #5199 amend, correctness): the picker's
    // non-default conversion reads the LIVE (possibly stale) si_value, which
    // is a different source of truth than the last-good value shown at the
    // default unit — converting it would flip the displayed magnitude
    // between the last-good and stale numbers across a single unit change.
    // The picker must not appear at all for such cells.
    const values = capacityValues({
      freshness: 'pending',
      last_substantive_value: '42',
    });
    render(() => (
      <PropertyEditor
        values={values}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));
    expect(screen.queryByTestId('unit-select-c1')).toBeNull();
    const row = screen.getByTestId('prop-row-c1');
    // The value lives in the input's `value` property, not row.textContent
    // (an <input>'s value is never part of its element's text content).
    const input = row.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input.value).toBe('42');
    expect(row.textContent).toContain('mm^3');
  });

  it('(k) a cell removed from props.values (e.g. file reload) has its unit preference pruned, both persisted and in-memory', () => {
    // Reviewer finding (task #5199 amend, resource_cleanup): both the
    // in-memory `selectedUnits` record and the persisted localStorage blob
    // previously accumulated one entry per cell_id ever seen and were never
    // pruned when a cell disappeared. Guard that a cell's preference is
    // dropped from BOTH places once it's no longer in props.values — so if
    // the same cell_id later reappears (e.g. the file is reloaded again) it
    // does not resurrect a stale pick from before it disappeared.
    const [values, setValues] = createSignal<Record<string, ValueData>>(capacityValues());
    render(() => (
      <PropertyEditor
        values={values()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={VOLUME_LADDER}
      />
    ));

    fireEvent.change(screen.getByTestId('unit-select-c1'), { target: { value: 'L' } });
    expect(loadUnitPreference('c1')).toBe('L');

    // Simulate a file reload: c1 disappears, replaced by an unrelated cell.
    setValues({
      c2: makeValue({ cell_id: 'c2', name: 'other', entity_path: 'Other.other' }),
    });
    expect(screen.queryByTestId('prop-row-c1')).toBeNull();
    expect(loadUnitPreference('c1')).toBeNull();

    // c1 reappears — it must NOT remember the pruned 'L' choice; it falls
    // back to the ladder default rather than resurrecting the stale pick.
    setValues(capacityValues());
    expect((screen.getByTestId('unit-select-c1') as HTMLSelectElement).value).toBe('mm^3');
  });
});

describe('PropertyEditor persisted unit preference survives the curated-label relabel (task #6028)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  const SUPERSCRIPT_VOLUME: UnitLadderMap = {
    Volume: [
      { label: 'mm³', si_scale: 1e-9, is_default: true },
      { label: 'cm³', si_scale: 1e-6, is_default: false },
      { label: 'L', si_scale: 1e-3, is_default: false },
      { label: 'm³', si_scale: 1.0, is_default: false },
    ],
  };

  const ASCII_VOLUME: UnitLadderMap = {
    Volume: [
      { label: 'mm^3', si_scale: 1e-9, is_default: true },
      { label: 'cm^3', si_scale: 1e-6, is_default: false },
      { label: 'L', si_scale: 1e-3, is_default: false },
      { label: 'm^3', si_scale: 1.0, is_default: false },
    ],
  };

  function capacityValues(): Record<string, ValueData> {
    return {
      c1: makeValue({
        cell_id: 'c1',
        name: 'capacity',
        entity_path: 'Tank.capacity',
        value: '7045002.24',
        unit: 'mm³',
        dimension: 'Volume',
        si_value: 0.00704500224,
      }),
    };
  }

  function renderWith(ladders: UnitLadderMap) {
    render(() => (
      <PropertyEditor
        values={capacityValues()}
        selectedEntity={null}
        onSetParameter={vi.fn()}
        unitLadders={ladders}
      />
    ));
    const row = screen.getByTestId('prop-row-c1');
    return {
      select: screen.getByTestId('unit-select-c1') as HTMLSelectElement,
      input: row.querySelector('input[type="text"]') as HTMLInputElement,
    };
  }

  // The default rung (mm^3) displays the backend value verbatim; the m^3 rung
  // displays the converted magnitude. Asserting the INPUT as well as the
  // select proves the rung IDENTITY survived, not merely the label text.
  const DEFAULT_RUNG_MAGNITUDE = '7045002.24';
  const M3_RUNG_MAGNITUDE = '0.00704500224';

  it('(a) a preference stored in the superscript spelling resolves against an ASCII ladder', () => {
    // The upgrade path: the user picked m³ before #5788 relabelled the curated
    // table, then #5788 lands. Without normalization the lookup misses and the
    // cell silently snaps back to the mm^3 default.
    saveUnitPreference('c1', 'm³');
    const { select, input } = renderWith(ASCII_VOLUME);
    expect(select.value).toBe('m^3');
    expect(input.value).toBe(M3_RUNG_MAGNITUDE);
  });

  it('(b) a preference stored in the ASCII spelling resolves against a superscript ladder', () => {
    // The mirror — a downgrade/rollback. This direction is what makes the fix
    // order-independent w.r.t. #5788: it holds whichever task lands first.
    saveUnitPreference('c1', 'm^3');
    const { select, input } = renderWith(SUPERSCRIPT_VOLUME);
    expect(select.value).toBe('m³');
    expect(input.value).toBe(M3_RUNG_MAGNITUDE);
  });

  it('(c) an exact match still wins — unchanged behaviour', () => {
    saveUnitPreference('c1', 'cm³');
    const { select, input } = renderWith(SUPERSCRIPT_VOLUME);
    expect(select.value).toBe('cm³');
    expect(input.value).toBe('7045.00224');
  });

  it.each([
    ['the superscript ladder', SUPERSCRIPT_VOLUME, 'mm³'],
    ['the ASCII ladder', ASCII_VOLUME, 'mm^3'],
  ])(
    '(d) a genuinely unknown label still falls back to the is_default rung against %s',
    (_desc, ladders, defaultLabel) => {
      // The normalized fallback must not have become an accept-anything.
      saveUnitPreference('c1', 'furlong');
      const { select, input } = renderWith(ladders);
      expect(select.value).toBe(defaultLabel);
      expect(input.value).toBe(DEFAULT_RUNG_MAGNITUDE);
    },
  );

  it.each([
    ['m³', ASCII_VOLUME],
    ['m^3', SUPERSCRIPT_VOLUME],
    ['furlong', SUPERSCRIPT_VOLUME],
  ])('leaves the stored spelling %s verbatim in localStorage', (stored, ladders) => {
    // This is resolution-time normalization, NOT a storage rewrite. Rewriting
    // the persisted blob would be order-dependent: landing before #5788, a
    // stored-form rewrite would itself break a preference that works today.
    saveUnitPreference('c1', stored);
    renderWith(ladders);
    expect(loadUnitPreference('c1')).toBe(stored);
  });

  it('resolves against a ladder carrying a non-string label without throwing', () => {
    // `UnitOption.label: string` is a claim about the backend's serde shape,
    // not a runtime guarantee — this data crosses the `get_unit_ladders` IPC
    // boundary. The exact-equality attempt tolerated a malformed entry by
    // simply missing it; the normalized attempt must not turn that silent miss
    // into a render-time crash.
    const malformed = {
      Volume: [
        { label: 'mm³', si_scale: 1e-9, is_default: true },
        { label: undefined, si_scale: 1e-6, is_default: false },
        { label: 'm³', si_scale: 1.0, is_default: false },
      ],
    } as unknown as UnitLadderMap;
    saveUnitPreference('c1', 'm^3');
    const { select, input } = renderWith(malformed);
    expect(select.value).toBe('m³');
    expect(input.value).toBe(M3_RUNG_MAGNITUDE);
  });
});
