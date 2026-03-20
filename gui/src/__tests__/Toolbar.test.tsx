import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { Toolbar } from '../panels/Toolbar';

describe('Toolbar', () => {
  it('renders with data-testid="toolbar"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} />);
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders an Export button with text "Export"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} />);
    expect(screen.getByText('Export')).toBeTruthy();
  });

  it('renders a Fit to View button with text "Fit to View"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} />);
    expect(screen.getByText('Fit to View')).toBeTruthy();
  });

  it('clicking Export calls onExport callback', () => {
    const onExport = vi.fn();
    render(() => <Toolbar onExport={onExport} onFitToView={vi.fn()} />);
    fireEvent.click(screen.getByText('Export'));
    expect(onExport).toHaveBeenCalledTimes(1);
  });

  it('clicking Fit to View calls onFitToView callback', () => {
    const onFitToView = vi.fn();
    render(() => <Toolbar onExport={vi.fn()} onFitToView={onFitToView} />);
    fireEvent.click(screen.getByText('Fit to View'));
    expect(onFitToView).toHaveBeenCalledTimes(1);
  });
});
