import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { ExportDialog } from '../panels/ExportDialog';

describe('ExportDialog', () => {
  it('renders modal with data-testid="export-dialog" when open=true', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
  });

  it('is not in DOM when open=false', () => {
    render(() => (
      <ExportDialog open={false} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    expect(screen.queryByTestId('export-dialog')).toBeNull();
  });

  it('format selector has STEP/STL/3MF options', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const select = screen.getByTestId('export-dialog').querySelector('select')!;
    const options = Array.from(select.querySelectorAll('option'));
    const values = options.map((o) => o.value);
    expect(values).toContain('step');
    expect(values).toContain('stl');
    expect(values).toContain('3mf');
  });

  it('default selected format is "step"', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const select = screen.getByTestId('export-dialog').querySelector('select') as HTMLSelectElement;
    expect(select.value).toBe('step');
  });

  it('clicking Export button calls onExport(selectedFormat)', () => {
    const onExport = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={onExport} onClose={vi.fn()} />
    ));
    fireEvent.click(screen.getByText('Export'));
    expect(onExport).toHaveBeenCalledWith('step');
  });

  it('clicking Cancel calls onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={onClose} />
    ));
    fireEvent.click(screen.getByText('Cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('changing format selector updates selection', () => {
    const onExport = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={onExport} onClose={vi.fn()} />
    ));
    const select = screen.getByTestId('export-dialog').querySelector('select')!;
    fireEvent.change(select, { target: { value: 'stl' } });
    fireEvent.click(screen.getByText('Export'));
    expect(onExport).toHaveBeenCalledWith('stl');
  });

  it('shows progress indicator when exporting=true prop set', () => {
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    expect(screen.getByTestId('export-progress')).toBeTruthy();
  });

  it('Export button is disabled when exporting=true', () => {
    const onExport = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={onExport} onClose={vi.fn()} />
    ));
    const exportBtn = screen.getByText('Export') as HTMLButtonElement;
    expect(exportBtn.disabled).toBe(true);
    fireEvent.click(exportBtn);
    expect(onExport).not.toHaveBeenCalled();
  });

  it('Cancel button is disabled when exporting=true', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={vi.fn()} onClose={onClose} />
    ));
    const cancelBtn = screen.getByText('Cancel') as HTMLButtonElement;
    expect(cancelBtn.disabled).toBe(true);
    fireEvent.click(cancelBtn);
    expect(onClose).not.toHaveBeenCalled();
  });

  it('Export button is enabled when exporting=false', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const exportBtn = screen.getByText('Export') as HTMLButtonElement;
    expect(exportBtn.disabled).toBe(false);
  });

  it('format selector is disabled when exporting=true', () => {
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const select = screen.getByTestId('export-dialog').querySelector('select') as HTMLSelectElement;
    expect(select.disabled).toBe(true);
  });

  // ── ARIA attributes (E-4) ──────────────────────────────────────────

  it('inner dialog div has role="dialog"', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const dialog = screen.getByRole('dialog');
    expect(dialog).toBeTruthy();
  });

  it('inner dialog div has aria-modal="true"', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });

  it('inner dialog div has aria-labelledby matching the title element id', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const dialog = screen.getByRole('dialog');
    const labelledBy = dialog.getAttribute('aria-labelledby');
    expect(labelledBy).toBeTruthy();
    const title = document.getElementById(labelledBy!);
    expect(title).toBeTruthy();
    expect(title!.textContent).toBe('Export Geometry');
  });

  // ── Label-select association (E-5) ─────────────────────────────────

  it('label for attribute matches the select element id', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const label = screen.getByText('Format');
    const forAttr = label.getAttribute('for');
    expect(forAttr).toBeTruthy();
    const select = document.getElementById(forAttr!) as HTMLSelectElement;
    expect(select).toBeTruthy();
    expect(select.tagName).toBe('SELECT');
  });

  // ── Escape key closes dialog (E-2) ────────────────────────────────

  it('pressing Escape calls onClose when not exporting', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={onClose} />
    ));
    const overlay = screen.getByTestId('export-dialog');
    fireEvent.keyDown(overlay, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('pressing Escape does NOT call onClose when exporting', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={vi.fn()} onClose={onClose} />
    ));
    const overlay = screen.getByTestId('export-dialog');
    fireEvent.keyDown(overlay, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();
  });

  // ── Overlay click dismiss (E-3) ───────────────────────────────────

  it('clicking overlay background calls onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={onClose} />
    ));
    const overlay = screen.getByTestId('export-dialog');
    fireEvent.click(overlay);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('clicking inside dialog does NOT call onClose', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={onClose} />
    ));
    const dialog = screen.getByRole('dialog');
    fireEvent.click(dialog);
    expect(onClose).not.toHaveBeenCalled();
  });

  it('clicking overlay does NOT call onClose when exporting', () => {
    const onClose = vi.fn();
    render(() => (
      <ExportDialog open={true} exporting={true} onExport={vi.fn()} onClose={onClose} />
    ));
    const overlay = screen.getByTestId('export-dialog');
    fireEvent.click(overlay);
    expect(onClose).not.toHaveBeenCalled();
  });

  // ── Focus trap (E-1) ──────────────────────────────────────────────

  it('focuses the first focusable element when dialog opens', async () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    // The first focusable element should be the select
    const select = document.getElementById('export-format-select');
    expect(document.activeElement).toBe(select);
  });

  it('Tab on last focusable element cycles to first', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const exportBtn = screen.getByText('Export');
    exportBtn.focus();
    fireEvent.keyDown(exportBtn, { key: 'Tab' });
    const select = document.getElementById('export-format-select');
    expect(document.activeElement).toBe(select);
  });

  it('Shift+Tab on first focusable element cycles to last', () => {
    render(() => (
      <ExportDialog open={true} exporting={false} onExport={vi.fn()} onClose={vi.fn()} />
    ));
    const select = document.getElementById('export-format-select')!;
    select.focus();
    fireEvent.keyDown(select, { key: 'Tab', shiftKey: true });
    const exportBtn = screen.getByText('Export');
    expect(document.activeElement).toBe(exportBtn);
  });
});
