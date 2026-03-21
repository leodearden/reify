import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { Splitter } from '../components/Splitter';

afterEach(() => {
  vi.restoreAllMocks();
});

describe('Splitter', () => {
  it('renders a draggable splitter bar with data-testid', () => {
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-left" />);
    expect(screen.getByTestId('splitter-left')).toBeTruthy();
  });

  it('applies horizontal orientation class', () => {
    render(() => <Splitter orientation="horizontal" onResize={vi.fn()} data-testid="splitter-h" />);
    const el = screen.getByTestId('splitter-h');
    expect(el.dataset.orientation).toBe('horizontal');
  });

  it('applies vertical orientation class', () => {
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-v" />);
    const el = screen.getByTestId('splitter-v');
    expect(el.dataset.orientation).toBe('vertical');
  });

  it('calls onResize with delta on mouse drag (vertical = left/right)', () => {
    const onResize = vi.fn();
    render(() => <Splitter orientation="vertical" onResize={onResize} data-testid="splitter-drag" />);
    const splitter = screen.getByTestId('splitter-drag');

    // Start drag at x=100
    fireEvent.mouseDown(splitter, { clientX: 100, clientY: 200 });
    // Move to x=130 (delta = +30)
    fireEvent.mouseMove(document, { clientX: 130, clientY: 200 });

    expect(onResize).toHaveBeenCalledWith(30);

    // Release
    fireEvent.mouseUp(document);
  });

  it('calls onResize with delta on mouse drag (horizontal = up/down)', () => {
    const onResize = vi.fn();
    render(() => <Splitter orientation="horizontal" onResize={onResize} data-testid="splitter-hdrag" />);
    const splitter = screen.getByTestId('splitter-hdrag');

    // Start drag at y=200
    fireEvent.mouseDown(splitter, { clientX: 100, clientY: 200 });
    // Move to y=250 (delta = +50)
    fireEvent.mouseMove(document, { clientX: 100, clientY: 250 });

    expect(onResize).toHaveBeenCalledWith(50);

    // Release
    fireEvent.mouseUp(document);
  });

  it('stops calling onResize after mouseup', () => {
    const onResize = vi.fn();
    render(() => <Splitter orientation="vertical" onResize={onResize} data-testid="splitter-stop" />);
    const splitter = screen.getByTestId('splitter-stop');

    fireEvent.mouseDown(splitter, { clientX: 100, clientY: 200 });
    fireEvent.mouseMove(document, { clientX: 120, clientY: 200 });
    expect(onResize).toHaveBeenCalledTimes(1);

    fireEvent.mouseUp(document);

    // Move after mouseup should NOT trigger onResize
    fireEvent.mouseMove(document, { clientX: 150, clientY: 200 });
    expect(onResize).toHaveBeenCalledTimes(1);
  });

  it('has appropriate cursor styling via data-orientation', () => {
    // Vertical splitter (between left/right columns) should allow col-resize
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-cursor" />);
    const el = screen.getByTestId('splitter-cursor');
    // The component should have data-orientation so CSS can style the cursor
    expect(el.dataset.orientation).toBe('vertical');
  });
});

describe('Splitter accessibility', () => {
  it('has role="separator"', () => {
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-a11y" />);
    const el = screen.getByTestId('splitter-a11y');
    expect(el.getAttribute('role')).toBe('separator');
  });

  it('has aria-orientation="vertical" for vertical splitter', () => {
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-v" />);
    const el = screen.getByTestId('splitter-v');
    expect(el.getAttribute('aria-orientation')).toBe('vertical');
  });

  it('has aria-orientation="horizontal" for horizontal splitter', () => {
    render(() => <Splitter orientation="horizontal" onResize={vi.fn()} data-testid="splitter-h" />);
    const el = screen.getByTestId('splitter-h');
    expect(el.getAttribute('aria-orientation')).toBe('horizontal');
  });

  it('has tabindex="0" for keyboard focusability', () => {
    render(() => <Splitter orientation="vertical" onResize={vi.fn()} data-testid="splitter-tab" />);
    const el = screen.getByTestId('splitter-tab');
    expect(el.getAttribute('tabindex')).toBe('0');
  });
});
