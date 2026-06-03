import { describe, it, expect } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { SelectionBreadcrumb } from '../panels/SelectionBreadcrumb';

describe('SelectionBreadcrumb', () => {
  it('(a) renders root container with data-testid="selection-breadcrumb"', () => {
    render(() => <SelectionBreadcrumb path="Printer.motion.head_block" />);
    expect(screen.getByTestId('selection-breadcrumb')).toBeTruthy();
  });

  it('(a) renders one crumb per dot-segment', () => {
    render(() => <SelectionBreadcrumb path="Printer.motion.head_block" />);
    expect(screen.getByText('Printer')).toBeTruthy();
    expect(screen.getByText('motion')).toBeTruthy();
    expect(screen.getByText('head_block')).toBeTruthy();
  });

  it('(a) renders separators between crumbs (at least one ›)', () => {
    render(() => <SelectionBreadcrumb path="Printer.motion.head_block" />);
    const container = screen.getByTestId('selection-breadcrumb');
    expect(container.textContent).toContain('›');
  });

  it('(b) leaf segment carries data-leaf="true"', () => {
    render(() => <SelectionBreadcrumb path="Printer.motion.head_block" />);
    const leaf = screen.getByTestId('breadcrumb-leaf');
    expect(leaf.getAttribute('data-leaf')).toBe('true');
    expect(leaf.textContent).toBe('head_block');
  });

  it('(b) non-leaf segments do NOT carry data-leaf="true"', () => {
    render(() => <SelectionBreadcrumb path="Printer.motion.head_block" />);
    // Query all elements with data-leaf and filter non-leaf
    const container = screen.getByTestId('selection-breadcrumb');
    const allLeafs = container.querySelectorAll('[data-leaf="true"]');
    expect(allLeafs.length).toBe(1); // only the last segment
  });

  it('(c) realization path keeps "#realization[0]" suffix on the leaf crumb', () => {
    render(() => (
      <SelectionBreadcrumb path="Printer.motion.head_block#realization[0]" />
    ));
    const leaf = screen.getByTestId('breadcrumb-leaf');
    // leaf text must include the realization suffix (not split on '#')
    expect(leaf.textContent).toBe('head_block#realization[0]');
    // ancestor segments are unchanged
    expect(screen.getByText('Printer')).toBeTruthy();
    expect(screen.getByText('motion')).toBeTruthy();
  });

  it('(c) only three crumbs for a three-segment realization path', () => {
    render(() => (
      <SelectionBreadcrumb path="Printer.motion.head_block#realization[0]" />
    ));
    const container = screen.getByTestId('selection-breadcrumb');
    // Count ALL crumb elements — non-leaf crumbs use breadcrumb-crumb-N testids,
    // the leaf uses breadcrumb-leaf; together they must equal the number of segments.
    const crumbs = container.querySelectorAll('[data-testid^="breadcrumb-crumb"], [data-testid="breadcrumb-leaf"]');
    expect(crumbs.length).toBe(3);
  });

  it('(d) path=null renders "No selection" placeholder and no crumbs', () => {
    render(() => <SelectionBreadcrumb path={null} />);
    expect(screen.getByTestId('selection-breadcrumb')).toBeTruthy();
    expect(screen.getByText('No selection')).toBeTruthy();
    const container = screen.getByTestId('selection-breadcrumb');
    const crumbs = container.querySelectorAll('[data-testid^="breadcrumb-crumb"]');
    expect(crumbs.length).toBe(0);
  });
});
