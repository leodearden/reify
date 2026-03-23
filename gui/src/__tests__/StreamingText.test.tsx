import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { StreamingText } from '../panels/chat/StreamingText';

describe('StreamingText', () => {
  it('renders text content with data-testid="streaming-text"', () => {
    render(() => <StreamingText text="Hello world" streaming={false} />);
    expect(screen.getByTestId('streaming-text')).toBeTruthy();
    expect(screen.getByTestId('streaming-text').textContent).toContain('Hello world');
  });

  it('shows blinking cursor with data-testid="streaming-cursor" when streaming=true', () => {
    render(() => <StreamingText text="Hello" streaming={true} />);
    expect(screen.getByTestId('streaming-cursor')).toBeTruthy();
  });

  it('hides cursor when streaming=false', () => {
    render(() => <StreamingText text="Hello" streaming={false} />);
    expect(screen.queryByTestId('streaming-cursor')).toBeNull();
  });

  it('applies markdown: bold text (**bold**) renders as <strong>', () => {
    render(() => <StreamingText text="This is **bold** text" streaming={false} />);
    const el = screen.getByTestId('streaming-text');
    const strong = el.querySelector('strong');
    expect(strong).toBeTruthy();
    expect(strong!.textContent).toBe('bold');
  });

  it('applies markdown: code block renders as <pre><code>', () => {
    render(() => <StreamingText text={'```\nconst x = 1;\n```'} streaming={false} />);
    const el = screen.getByTestId('streaming-text');
    const pre = el.querySelector('pre');
    const code = el.querySelector('code');
    expect(pre).toBeTruthy();
    expect(code).toBeTruthy();
  });

  it('renders inline code with <code> tags', () => {
    render(() => <StreamingText text="Use `foo()` here" streaming={false} />);
    const el = screen.getByTestId('streaming-text');
    const code = el.querySelector('code');
    expect(code).toBeTruthy();
    expect(code!.textContent).toBe('foo()');
  });

  it('empty text renders empty container without errors', () => {
    render(() => <StreamingText text="" streaming={false} />);
    expect(screen.getByTestId('streaming-text')).toBeTruthy();
  });
});
