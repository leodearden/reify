import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { PermissionPrompt } from '../panels/chat/PermissionPrompt';

describe('PermissionPrompt', () => {
  const defaultProps = {
    toolName: 'Write',
    toolInput: { path: '/tmp/x', content: 'hello' },
    onDecide: vi.fn(),
  };

  it('renders data-testid="permission-prompt"', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    expect(screen.getByTestId('permission-prompt')).toBeTruthy();
  });

  it('displays the tool name', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    expect(screen.getByTestId('permission-prompt').textContent).toContain('Write');
  });

  it('displays a JSON summary of tool_input', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    const text = screen.getByTestId('permission-prompt').textContent;
    // JSON.stringify of the input should appear somewhere in the component
    expect(text).toContain('/tmp/x');
  });

  it('renders Allow button with data-testid="permission-allow"', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    expect(screen.getByTestId('permission-allow')).toBeTruthy();
  });

  it('renders Deny button with data-testid="permission-deny"', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    expect(screen.getByTestId('permission-deny')).toBeTruthy();
  });

  it('renders "Always allow this tool" button with data-testid="permission-allow-always"', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    expect(screen.getByTestId('permission-allow-always')).toBeTruthy();
  });

  it('clicking Allow calls onDecide with { behavior: "allow" }', () => {
    const onDecide = vi.fn();
    render(() => <PermissionPrompt {...defaultProps} onDecide={onDecide} />);
    fireEvent.click(screen.getByTestId('permission-allow'));
    expect(onDecide).toHaveBeenCalledOnce();
    expect(onDecide).toHaveBeenCalledWith({ behavior: 'allow' });
  });

  it('clicking Deny calls onDecide with { behavior: "deny" }', () => {
    const onDecide = vi.fn();
    render(() => <PermissionPrompt {...defaultProps} onDecide={onDecide} />);
    fireEvent.click(screen.getByTestId('permission-deny'));
    expect(onDecide).toHaveBeenCalledOnce();
    expect(onDecide).toHaveBeenCalledWith({ behavior: 'deny' });
  });

  it('clicking "Always allow" calls onDecide with { behavior: "allow", remember: true }', () => {
    const onDecide = vi.fn();
    render(() => <PermissionPrompt {...defaultProps} onDecide={onDecide} />);
    fireEvent.click(screen.getByTestId('permission-allow-always'));
    expect(onDecide).toHaveBeenCalledOnce();
    expect(onDecide).toHaveBeenCalledWith({ behavior: 'allow', remember: true });
  });

  it('renders three action buttons total', () => {
    render(() => <PermissionPrompt {...defaultProps} />);
    const allow = screen.getByTestId('permission-allow');
    const deny = screen.getByTestId('permission-deny');
    const always = screen.getByTestId('permission-allow-always');
    expect(allow.tagName.toLowerCase()).toBe('button');
    expect(deny.tagName.toLowerCase()).toBe('button');
    expect(always.tagName.toLowerCase()).toBe('button');
  });

  it('works with empty toolInput', () => {
    const onDecide = vi.fn();
    render(() => (
      <PermissionPrompt toolName="Bash" toolInput={{}} onDecide={onDecide} />
    ));
    expect(screen.getByTestId('permission-prompt')).toBeTruthy();
    fireEvent.click(screen.getByTestId('permission-allow'));
    expect(onDecide).toHaveBeenCalledWith({ behavior: 'allow' });
  });
});
