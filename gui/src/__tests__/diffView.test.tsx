import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { DiffView } from '../panels/chat/DiffView';

describe('DiffView', () => {
  it('renders with data-testid="diff-view"', () => {
    render(() => <DiffView before="line1" after="line2" />);
    expect(screen.getByTestId('diff-view')).toBeTruthy();
  });

  it('addition lines have data-diff="add"', () => {
    render(() => <DiffView before="" after="new line" />);
    const diffView = screen.getByTestId('diff-view');
    const addLines = diffView.querySelectorAll('[data-diff="add"]');
    expect(addLines.length).toBeGreaterThan(0);
  });

  it('deletion lines have data-diff="remove"', () => {
    render(() => <DiffView before="old line" after="" />);
    const diffView = screen.getByTestId('diff-view');
    const removeLines = diffView.querySelectorAll('[data-diff="remove"]');
    expect(removeLines.length).toBeGreaterThan(0);
  });

  it('context lines have data-diff="context"', () => {
    render(() => <DiffView before="same\nchanged" after="same\nnew" />);
    const diffView = screen.getByTestId('diff-view');
    const contextLines = diffView.querySelectorAll('[data-diff="context"]');
    expect(contextLines.length).toBeGreaterThan(0);
  });

  it('starts expanded by default', () => {
    render(() => <DiffView before="a" after="b" />);
    const diffView = screen.getByTestId('diff-view');
    const content = diffView.querySelector('[data-testid="diff-content"]');
    expect(content).toBeTruthy();
  });

  it('clicking header collapses the diff content', () => {
    render(() => <DiffView before="a" after="b" />);
    const header = screen.getByTestId('diff-header');
    fireEvent.click(header);
    const content = screen.getByTestId('diff-view').querySelector('[data-testid="diff-content"]');
    expect(content).toBeNull();
  });

  it('clicking again re-expands', () => {
    render(() => <DiffView before="a" after="b" />);
    const header = screen.getByTestId('diff-header');
    fireEvent.click(header);
    fireEvent.click(header);
    const content = screen.getByTestId('diff-view').querySelector('[data-testid="diff-content"]');
    expect(content).toBeTruthy();
  });

  it('when before===after shows "No changes" text', () => {
    render(() => <DiffView before="same content" after="same content" />);
    expect(screen.getByText('No changes')).toBeTruthy();
  });

  it('line numbers are displayed', () => {
    render(() => <DiffView before="line1\nline2" after="line1\nline3" />);
    const diffView = screen.getByTestId('diff-view');
    const lineNums = diffView.querySelectorAll('[data-testid="line-number"]');
    expect(lineNums.length).toBeGreaterThan(0);
  });
});
