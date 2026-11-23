import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@solidjs/testing-library';
import { Toolbar } from '../panels/Toolbar';

describe('Toolbar', () => {
  it('renders with data-testid="toolbar"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    expect(screen.getByTestId('toolbar')).toBeTruthy();
  });

  it('renders an Export button with text "Export"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    expect(screen.getByText('Export')).toBeTruthy();
  });

  it('renders a Fit to View button with text "Fit to View"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    expect(screen.getByText('Fit to View')).toBeTruthy();
  });

  it('clicking Export calls onExport callback', () => {
    const onExport = vi.fn();
    render(() => <Toolbar onExport={onExport} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    fireEvent.click(screen.getByText('Export'));
    expect(onExport).toHaveBeenCalledTimes(1);
  });

  it('clicking Fit to View calls onFitToView callback', () => {
    const onFitToView = vi.fn();
    render(() => <Toolbar onExport={vi.fn()} onFitToView={onFitToView} edges={false} onToggleEdges={vi.fn()} />);
    fireEvent.click(screen.getByText('Fit to View'));
    expect(onFitToView).toHaveBeenCalledTimes(1);
  });
});

describe('Toolbar Claude button', () => {
  it('renders a button with text "Claude" when onToggleChatPanel prop is provided', () => {
    render(() => (
      <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} onToggleChatPanel={vi.fn()} />
    ));
    expect(screen.getByText('Claude')).toBeTruthy();
  });

  it('clicking Claude button calls onToggleChatPanel callback', () => {
    const onToggleChatPanel = vi.fn();
    render(() => (
      <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} onToggleChatPanel={onToggleChatPanel} />
    ));
    fireEvent.click(screen.getByText('Claude'));
    expect(onToggleChatPanel).toHaveBeenCalledTimes(1);
  });
});

describe('Toolbar accessibility', () => {
  it('container has role="toolbar"', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    const el = screen.getByTestId('toolbar');
    expect(el.getAttribute('role')).toBe('toolbar');
  });
});

describe('Toolbar edges toggle', () => {
  it('renders an "Edges" button', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    expect(screen.getByTestId('toolbar-edges-toggle').textContent).toBe('Edges');
  });

  it('reports the edges state through aria-pressed', () => {
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={vi.fn()} />);
    expect(screen.getByTestId('toolbar-edges-toggle').getAttribute('aria-pressed')).toBe('false');
    cleanup();

    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={true} onToggleEdges={vi.fn()} />);
    expect(screen.getByTestId('toolbar-edges-toggle').getAttribute('aria-pressed')).toBe('true');
  });

  it('clicking it calls onToggleEdges once', () => {
    const onToggleEdges = vi.fn();
    render(() => <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} edges={false} onToggleEdges={onToggleEdges} />);
    fireEvent.click(screen.getByTestId('toolbar-edges-toggle'));
    expect(onToggleEdges).toHaveBeenCalledTimes(1);
  });
});
