import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
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
