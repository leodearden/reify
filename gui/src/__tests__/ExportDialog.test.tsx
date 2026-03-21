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
});
