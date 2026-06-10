import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@solidjs/testing-library';
import {
  PropertyEditor,
  ConstraintPanel,
  Toolbar,
  StatusBar,
  Toast,
  FileBrowser,
  ExportDialog,
  ReloadPrompt,
  ChatPanel,
  MenuBar,
  DiagnosticsPanel,
  AutoResolvePanel,
} from '../panels';
import { createClaudeStore } from '../stores';

describe('panels smoke integration', () => {
  it('all four original components mount and have expected data-testid attributes', () => {
    render(() => (
      <div>
        <PropertyEditor
          values={{}}
          selectedEntity={null}
          onSetParameter={vi.fn()}
        />
        <ConstraintPanel constraints={{}} values={{}} />
        <Toolbar onExport={vi.fn()} onFitToView={vi.fn()} />
        <StatusBar
          evalStatus={{ phase: 'idle' }}
          meshes={{}}
          constraints={{}}
        />
      </div>
    ));

    expect(screen.getByTestId('property-editor')).toBeTruthy();
    expect(screen.getByTestId('constraint-panel')).toBeTruthy();
    expect(screen.getByTestId('toolbar')).toBeTruthy();
    expect(screen.getByTestId('status-bar')).toBeTruthy();
  });

  it('new components mount and have expected data-testid attributes', () => {
    render(() => (
      <div>
        <Toast message="Test toast" type="info" onDismiss={vi.fn()} />
        <FileBrowser files={[]} activeFile={null} onFileClick={vi.fn()} />
        <ExportDialog
          open={true}
          exporting={false}
          onExport={vi.fn()}
          onClose={vi.fn()}
        />
        <ReloadPrompt
          filePaths={["/test/file.ri"]}
          onReload={vi.fn()}
          onDismiss={vi.fn()}
        />
      </div>
    ));

    expect(screen.getByTestId('toast')).toBeTruthy();
    expect(screen.getByTestId('file-browser')).toBeTruthy();
    expect(screen.getByTestId('export-dialog')).toBeTruthy();
    expect(screen.getByTestId('reload-prompt')).toBeTruthy();
  });

  it('ChatPanel mounts with expected data-testid', () => {
    const store = createClaudeStore({ onSend: vi.fn(), onAbort: vi.fn(), onPermissionDecision: vi.fn() });
    render(() => <ChatPanel store={store} />);
    expect(screen.getByTestId('chat-panel')).toBeTruthy();
  });

  it('MenuBar mounts with data-testid="menu-bar"', () => {
    render(() => <MenuBar />);
    expect(screen.getByTestId('menu-bar')).toBeTruthy();
  });

  it('DiagnosticsPanel collapsed=false renders diagnostics-panel and panel-title-diagnostics', () => {
    render(() => (
      <DiagnosticsPanel
        collapsed={false}
        height={160}
        diagnostics={[]}
        onToggleCollapsed={vi.fn()}
        onNavigate={vi.fn()}
      />
    ));
    expect(screen.getByTestId('diagnostics-panel')).toBeTruthy();
    expect(screen.getByTestId('panel-title-diagnostics')).toBeTruthy();
  });

  it('AutoResolvePanel mounts via barrel import with data-testid="auto-resolve-panel"', () => {
    render(() => (
      <AutoResolvePanel state={{ active: false, iterations: [] }} />
    ));
    expect(screen.getByTestId('auto-resolve-panel')).toBeTruthy();
  });
});
