import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
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
