import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { FindUsesPanel } from '../panels/FindUsesPanel';
import type { ReferenceResult } from '../editor/references';

function makeResult(overrides: Partial<ReferenceResult> = {}): ReferenceResult {
  return {
    uri: 'file:///test.ri',
    line: 2,
    character: 4,
    endLine: 2,
    endCharacter: 9,
    preview: 'let foo = bar',
    ...overrides,
  };
}

describe('FindUsesPanel', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders nothing when open=false', () => {
    render(() => (
      <FindUsesPanel open={false} results={[]} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    expect(document.querySelector('[data-testid="find-uses-panel"]')).toBeNull();
  });

  it('renders the panel when open=true', () => {
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    expect(screen.getByTestId('find-uses-panel')).toBeTruthy();
  });

  it('renders a title containing the occurrence count', () => {
    const results = [
      makeResult({ line: 2, character: 4 }),
      makeResult({ line: 5, character: 8 }),
      makeResult({ line: 9, character: 0 }),
    ];
    render(() => (
      <FindUsesPanel open={true} results={results} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    const title = screen.getByTestId('panel-title-find-uses');
    expect(title.textContent).toMatch(/find uses/i);
    expect(title.textContent).toContain('3');
  });

  it('renders exactly N rows for N results, each showing a 1-based line:column label', () => {
    const results = [
      makeResult({ line: 2, character: 4 }),   // → 3:5
      makeResult({ line: 5, character: 8 }),   // → 6:9
      makeResult({ line: 9, character: 0 }),   // → 10:1
    ];
    render(() => (
      <FindUsesPanel open={true} results={results} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    const rows = document.querySelectorAll('[data-testid="find-use-row"]');
    expect(rows.length).toBe(3);
    // LSP is 0-based; rows display 1-based line:column (line+1 : character+1).
    const text = (screen.getByTestId('find-uses-panel').textContent ?? '');
    expect(text).toContain('3:5');
    expect(text).toContain('6:9');
    expect(text).toContain('10:1');
  });

  it('renders the best-effort preview text on a row when present', () => {
    const results = [makeResult({ line: 2, character: 4, preview: 'thickness = 5mm' })];
    render(() => (
      <FindUsesPanel open={true} results={results} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    expect(screen.getByTestId('find-uses-panel').textContent).toContain('thickness = 5mm');
  });

  it('clicking a row invokes onNavigate with that result', () => {
    const target = makeResult({ line: 5, character: 8, preview: 'use of foo' });
    const results = [makeResult({ line: 2, character: 4 }), target];
    const onNavigate = vi.fn();
    render(() => (
      <FindUsesPanel open={true} results={results} onClose={vi.fn()} onNavigate={onNavigate} />
    ));
    const rows = document.querySelectorAll('[data-testid="find-use-row"]');
    fireEvent.click(rows[1]);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate).toHaveBeenCalledWith(target);
  });

  it('shows an empty-state and zero rows when results is empty', () => {
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={vi.fn()} onNavigate={vi.fn()} />
    ));
    const panel = screen.getByTestId('find-uses-panel');
    expect(panel.textContent).toMatch(/no uses/i);
    expect(document.querySelectorAll('[data-testid="find-use-row"]').length).toBe(0);
  });

  it('clicking the footer close button invokes onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={onClose} onNavigate={vi.fn()} />
    ));
    fireEvent.click(screen.getByTestId('find-uses-close'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('clicking the header close button invokes onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={onClose} onNavigate={vi.fn()} />
    ));
    fireEvent.click(screen.getByTestId('find-uses-header-close'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('Escape key invokes onClose (document-level listener)', () => {
    const onClose = vi.fn();
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={onClose} onNavigate={vi.fn()} />
    ));
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('clicking the overlay (outside the dialog) invokes onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <FindUsesPanel open={true} results={[]} onClose={onClose} onNavigate={vi.fn()} />
    ));
    // Click the overlay itself (outermost element), not an inner element.
    fireEvent.click(screen.getByTestId('find-uses-panel'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
