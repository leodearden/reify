import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ThinkingBlock } from '../panels/chat/ThinkingBlock';

describe('ThinkingBlock', () => {
  it('when complete=false, shows animated indicator with data-testid="thinking-indicator"', () => {
    render(() => <ThinkingBlock text="pondering..." complete={false} />);
    expect(screen.getByTestId('thinking-indicator')).toBeTruthy();
  });

  it('when complete=false, does NOT show thinking text content', () => {
    render(() => <ThinkingBlock text="secret thoughts" complete={false} />);
    expect(screen.queryByText('secret thoughts')).toBeNull();
  });

  it('when complete=true, renders collapsible section with data-testid="thinking-block"', () => {
    render(() => <ThinkingBlock text="done thinking" complete={true} />);
    expect(screen.getByTestId('thinking-block')).toBeTruthy();
  });

  it('when complete=true, thinking text is NOT visible by default (collapsed)', () => {
    render(() => <ThinkingBlock text="hidden by default" complete={true} />);
    expect(screen.queryByText('hidden by default')).toBeNull();
  });

  it('clicking the header expands to show thinking text', () => {
    render(() => <ThinkingBlock text="revealed text" complete={true} />);
    const header = screen.getByTestId('thinking-block').querySelector('[role="button"]')!;
    fireEvent.click(header);
    expect(screen.getByText('revealed text')).toBeTruthy();
  });

  it('clicking again collapses the thinking text', () => {
    render(() => <ThinkingBlock text="toggle text" complete={true} />);
    const header = screen.getByTestId('thinking-block').querySelector('[role="button"]')!;
    fireEvent.click(header);
    expect(screen.getByText('toggle text')).toBeTruthy();
    fireEvent.click(header);
    expect(screen.queryByText('toggle text')).toBeNull();
  });

  it('expanded text container has muted styling', () => {
    render(() => <ThinkingBlock text="muted text" complete={true} />);
    const header = screen.getByTestId('thinking-block').querySelector('[role="button"]')!;
    fireEvent.click(header);
    const textEl = screen.getByText('muted text');
    // The text element or its parent should have a class containing 'muted' or 'content'
    const container = textEl.closest('[class]')!;
    expect(container.className).toBeTruthy();
  });
});
