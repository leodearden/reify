// The stub components the App harness installs for '../viewport',
// '../editor/Editor' and '../editor/FileTabs', and the props they record.
// Evaluated inside those mocks' own resolution, so its only runtime import is
// vitest. Only this module assigns the captured slots; importers read them as
// live bindings, and primeAppHarness resets them before every test.
import { vi, type Mock } from 'vitest';
import type { ReferenceResult } from '../../editor/references';
import type * as EditorModule from '../../editor/Editor';
import type * as FileTabsModule from '../../editor/FileTabs';
import type * as ViewportModule from '../../viewport';

export let capturedViewportProps: any = {};
export let capturedDualViewportProps: any = {};
export let capturedMultiViewportProps: any = {};
export let mockViewportFitToView: Mock;
export let mockFlyToEntity: Mock;
export let capturedEditorStore: any = null;
export let capturedEditorOnOpen: (() => void) | undefined = undefined;
export let capturedEditorScrollToLocation: (() => any) | undefined = undefined;
export let capturedEditorCompileDiagnostics: any = undefined;
export let capturedEditorLiveContentRef: ((getter: () => string | null) => void) | undefined = undefined;
export let capturedEditorOnShowReferences: ((results: ReferenceResult[]) => void) | undefined = undefined;

export function resetRenderStubCaptures(): void {
  capturedViewportProps = {};
  capturedDualViewportProps = {};
  capturedMultiViewportProps = {};
  mockViewportFitToView = vi.fn();
  mockFlyToEntity = vi.fn();
  capturedEditorStore = null;
  capturedEditorOnOpen = undefined;
  capturedEditorScrollToLocation = undefined;
  capturedEditorCompileDiagnostics = undefined;
  capturedEditorLiveContentRef = undefined;
  capturedEditorOnShowReferences = undefined;
}

resetRenderStubCaptures();

function stubElement(testId: string, text: string): HTMLElement {
  const el = document.createElement('div');
  el.setAttribute('data-testid', testId);
  el.textContent = text;
  return el;
}

function handOutViewRefs(props: any): void {
  if (props.flyToEntityRef) {
    props.flyToEntityRef(mockFlyToEntity);
  }
  if (props.fitToViewRef) {
    props.fitToViewRef(mockViewportFitToView);
  }
}

export function ViewportStub(props: any): HTMLElement {
  capturedViewportProps = props;
  handOutViewRefs(props);
  return stubElement('viewport-container', 'Viewport Mock');
}

export function DualViewportStub(props: any): HTMLElement {
  capturedDualViewportProps = props;
  handOutViewRefs(props);
  return stubElement('dual-viewport', 'DualViewport Mock');
}

export function MultiViewportStub(props: any): HTMLElement {
  capturedMultiViewportProps = props;
  return stubElement('multi-viewport', 'MultiViewport Mock');
}

export function EditorStub(props: any): HTMLElement {
  capturedEditorStore = props.store;
  capturedEditorOnOpen = props.onOpen;
  capturedEditorScrollToLocation = props.scrollToLocation;
  capturedEditorCompileDiagnostics = props.compileDiagnostics;
  capturedEditorLiveContentRef = props.liveContentRef;
  capturedEditorOnShowReferences = props.onShowReferences;
  return stubElement('editor-container', 'Editor Mock');
}

export function FileTabsStub(_props: unknown): HTMLElement {
  return stubElement('file-tabs', 'FileTabs Mock');
}

export const viewportStub = () =>
  ({
    Viewport: ViewportStub,
    DualViewport: DualViewportStub,
    MultiViewport: MultiViewportStub,
  }) satisfies Partial<Record<keyof typeof ViewportModule, unknown>>;

export const editorStub = () => ({ Editor: EditorStub }) satisfies Partial<Record<keyof typeof EditorModule, unknown>>;

export const fileTabsStub = () => ({ FileTabs: FileTabsStub }) satisfies Partial<Record<keyof typeof FileTabsModule, unknown>>;
