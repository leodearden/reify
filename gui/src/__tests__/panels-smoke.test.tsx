import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { PropertyEditor, ConstraintPanel, Toolbar, StatusBar } from '../panels';

describe('panels smoke integration', () => {
  it('all four components mount and have expected data-testid attributes', () => {
    render(() => (
      <div>
        <PropertyEditor
          values={{}}
          selectedEntity={null}
          onSetParameter={vi.fn()}
        />
        <ConstraintPanel constraints={{}} values={{}} />
        <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} />
        <StatusBar
          evalStatus={{ phase: 'idle' }}
          meshes={{}}
          constraints={{}}
        />
      </div>
    ));

    expect(screen.getByTestId('property-editor')).toBeTruthy();
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
    expect(screen.getByTestId('toolbar')).toBeTruthy();
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });
});
