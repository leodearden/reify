import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { DesignTreeContextMenu } from '../panels/DesignTreeContextMenu';
import type { MenuAction } from '../panels/DesignTreeContextMenu';

describe('DesignTreeContextMenu', () => {
  const defaultProps = {
    entityPath: 'Root.A',
    x: 100,
    y: 200,
    onAction: vi.fn<[MenuAction, string], void>(),
  };

  it('renders with data-testid="design-tree-context-menu"', () => {
    render(() => <DesignTreeContextMenu {...defaultProps} />);
    expect(screen.getByTestId('design-tree-context-menu')).toBeTruthy();
  });

  it('renders 6 action buttons in correct order', () => {
    render(() => <DesignTreeContextMenu {...defaultProps} />);
    const menu = screen.getByTestId('design-tree-context-menu');
    const buttons = menu.querySelectorAll('button');
    expect(buttons).toHaveLength(6);
    expect(buttons[0].textContent).toContain('Show this and children');
    expect(buttons[1].textContent).toContain('Ghost this and children');
    expect(buttons[2].textContent).toContain('Hide this and children');
    expect(buttons[3].textContent).toContain('Show only this');
    expect(buttons[4].textContent).toContain('Reset to default');
    expect(buttons[5].textContent).toContain('Show only this (no cascade)');
  });

  it('renders a separator between "Reset to default" and "Show only this (no cascade)"', () => {
    render(() => <DesignTreeContextMenu {...defaultProps} />);
    const menu = screen.getByTestId('design-tree-context-menu');
    const hr = menu.querySelector('hr');
    expect(hr).toBeTruthy();
  });

  it('positioned at supplied x/y via inline style', () => {
    render(() => <DesignTreeContextMenu {...defaultProps} x={150} y={300} />);
    const menu = screen.getByTestId('design-tree-context-menu');
    expect((menu as HTMLElement).style.left).toBe('150px');
    expect((menu as HTMLElement).style.top).toBe('300px');
  });

  it('clicking "Show this and children" calls onAction with show-cascade and entityPath', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[0]);
    expect(onAction).toHaveBeenCalledWith('show-cascade', 'Root.A');
  });

  it('clicking "Ghost this and children" calls onAction with ghost-cascade', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[1]);
    expect(onAction).toHaveBeenCalledWith('ghost-cascade', 'Root.A');
  });

  it('clicking "Hide this and children" calls onAction with hide-cascade', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[2]);
    expect(onAction).toHaveBeenCalledWith('hide-cascade', 'Root.A');
  });

  it('clicking "Show only this" calls onAction with show-only', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[3]);
    expect(onAction).toHaveBeenCalledWith('show-only', 'Root.A');
  });

  it('clicking "Reset to default" calls onAction with reset', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[4]);
    expect(onAction).toHaveBeenCalledWith('reset', 'Root.A');
  });

  it('clicking "Show only this (no cascade)" calls onAction with show-only-no-cascade', () => {
    const onAction = vi.fn<[MenuAction, string], void>();
    render(() => <DesignTreeContextMenu {...defaultProps} onAction={onAction} />);
    const buttons = screen.getByTestId('design-tree-context-menu').querySelectorAll('button');
    fireEvent.click(buttons[5]);
    expect(onAction).toHaveBeenCalledWith('show-only-no-cascade', 'Root.A');
  });
});
