import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ContextPicker } from '../panels/chat/ContextPicker';

describe('ContextPicker', () => {
  it('renders [+ context] button with data-testid="context-picker-btn"', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={false}
        hasDiagnostics={false}
        hasViolatedConstraints={false}
        hasActiveFile={false}
      />
    ));
    expect(screen.getByTestId('context-picker-btn')).toBeTruthy();
  });

  it('clicking button opens dropdown with data-testid="context-picker-dropdown"', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={true}
        hasDiagnostics={true}
        hasViolatedConstraints={true}
        hasActiveFile={true}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    expect(screen.getByTestId('context-picker-dropdown')).toBeTruthy();
  });

  it('dropdown has 4 options: Current selection, Active diagnostics, Violated constraints, Current file', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={true}
        hasDiagnostics={true}
        hasViolatedConstraints={true}
        hasActiveFile={true}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    expect(screen.getByText('Current selection')).toBeTruthy();
    expect(screen.getByText('Active diagnostics')).toBeTruthy();
    expect(screen.getByText('Violated constraints')).toBeTruthy();
    expect(screen.getByText('Current file')).toBeTruthy();
  });

  it('clicking an option calls onAttach(type) with the context type string', () => {
    const onAttach = vi.fn();
    render(() => (
      <ContextPicker
        onAttach={onAttach}
        hasSelection={true}
        hasDiagnostics={true}
        hasViolatedConstraints={true}
        hasActiveFile={true}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    fireEvent.click(screen.getByText('Current selection'));
    expect(onAttach).toHaveBeenCalledWith('selection');
  });

  it('dropdown closes after selecting an option', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={true}
        hasDiagnostics={true}
        hasViolatedConstraints={true}
        hasActiveFile={true}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    expect(screen.getByTestId('context-picker-dropdown')).toBeTruthy();
    fireEvent.click(screen.getByText('Active diagnostics'));
    expect(screen.queryByTestId('context-picker-dropdown')).toBeNull();
  });

  it('dropdown closes on clicking outside', () => {
    render(() => (
      <div>
        <ContextPicker
          onAttach={() => {}}
          hasSelection={true}
          hasDiagnostics={true}
          hasViolatedConstraints={true}
          hasActiveFile={true}
        />
        <button data-testid="outside">Outside</button>
      </div>
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    expect(screen.getByTestId('context-picker-dropdown')).toBeTruthy();
    fireEvent.click(screen.getByTestId('outside'));
    expect(screen.queryByTestId('context-picker-dropdown')).toBeNull();
  });

  it('options can be disabled when not available', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={false}
        hasDiagnostics={false}
        hasViolatedConstraints={true}
        hasActiveFile={false}
      />
    ));
    fireEvent.click(screen.getByTestId('context-picker-btn'));
    const selectionOption = screen.getByText('Current selection');
    expect(selectionOption.closest('button')!.hasAttribute('disabled')).toBe(true);
    const diagnosticsOption = screen.getByText('Active diagnostics');
    expect(diagnosticsOption.closest('button')!.hasAttribute('disabled')).toBe(true);
    const constraintsOption = screen.getByText('Violated constraints');
    expect(constraintsOption.closest('button')!.hasAttribute('disabled')).toBe(false);
    const fileOption = screen.getByText('Current file');
    expect(fileOption.closest('button')!.hasAttribute('disabled')).toBe(true);
  });
});

describe('ContextPicker trigger button accessibility', () => {
  it('trigger button has aria-haspopup="menu"', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={false}
        hasDiagnostics={false}
        hasViolatedConstraints={false}
        hasActiveFile={false}
      />
    ));
    const btn = screen.getByTestId('context-picker-btn');
    expect(btn.getAttribute('aria-haspopup')).toBe('menu');
  });

  it('trigger button has aria-expanded="false" when dropdown is closed', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={false}
        hasDiagnostics={false}
        hasViolatedConstraints={false}
        hasActiveFile={false}
      />
    ));
    const btn = screen.getByTestId('context-picker-btn');
    expect(btn.getAttribute('aria-expanded')).toBe('false');
  });

  it('trigger button has aria-expanded="true" after clicking to open dropdown', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={true}
        hasDiagnostics={true}
        hasViolatedConstraints={false}
        hasActiveFile={false}
      />
    ));
    const btn = screen.getByTestId('context-picker-btn');
    fireEvent.click(btn);
    expect(btn.getAttribute('aria-expanded')).toBe('true');
  });

  it('trigger button has aria-label of "Attach context"', () => {
    render(() => (
      <ContextPicker
        onAttach={() => {}}
        hasSelection={false}
        hasDiagnostics={false}
        hasViolatedConstraints={false}
        hasActiveFile={false}
      />
    ));
    const btn = screen.getByTestId('context-picker-btn');
    expect(btn.getAttribute('aria-label')).toBe('Attach context');
  });
});
