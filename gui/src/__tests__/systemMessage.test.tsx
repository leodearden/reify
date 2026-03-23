import { describe, it, expect } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import { SystemMessage } from '../panels/chat/SystemMessage';

describe('SystemMessage', () => {
  it('renders with data-testid="system-message"', () => {
    render(() => <SystemMessage errorType="unknown" text="Something happened" />);
    expect(screen.getByTestId('system-message')).toBeTruthy();
  });

  it('displays message text', () => {
    render(() => <SystemMessage errorType="unknown" text="Something went wrong" />);
    expect(screen.getByText('Something went wrong')).toBeTruthy();
  });

  it('auth errors have data-error-type="auth" and show "claude login" text', () => {
    render(() => (
      <SystemMessage
        errorType="auth"
        text="Authentication required. Run `claude login` in your terminal."
      />
    ));
    const el = screen.getByTestId('system-message');
    expect(el.getAttribute('data-error-type')).toBe('auth');
    expect(screen.getByText(/claude login/)).toBeTruthy();
  });

  it('rate-limit errors have data-error-type="rate-limit"', () => {
    render(() => (
      <SystemMessage errorType="rate-limit" text="Rate limited. Please wait and try again." />
    ));
    const el = screen.getByTestId('system-message');
    expect(el.getAttribute('data-error-type')).toBe('rate-limit');
  });

  it('network errors have data-error-type="network"', () => {
    render(() => (
      <SystemMessage errorType="network" text="Connection failed. Check your network." />
    ));
    const el = screen.getByTestId('system-message');
    expect(el.getAttribute('data-error-type')).toBe('network');
  });

  it('sidecar errors have data-error-type="sidecar" and show "restart" text', () => {
    render(() => (
      <SystemMessage
        errorType="sidecar"
        text="Claude session disconnected. Click to restart."
      />
    ));
    const el = screen.getByTestId('system-message');
    expect(el.getAttribute('data-error-type')).toBe('sidecar');
    expect(screen.getByText(/restart/)).toBeTruthy();
  });

  it('unknown errors display the raw message', () => {
    render(() => <SystemMessage errorType="unknown" text="Unexpected: foobar glitch" />);
    expect(screen.getByText('Unexpected: foobar glitch')).toBeTruthy();
  });
});
