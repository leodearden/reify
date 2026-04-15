import { EditorView } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';
import { THEME_TOKENS } from '../theme';

/**
 * Raw CSS spec object for the CodeMirror dark theme.
 * Exported separately so tests can inspect cursor width, colors, and gutter
 * styling without relying on DOM rendering.
 */
export const editorThemeSpec: Record<string, Record<string, string>> = {
  '.cm-cursor, .cm-dropCursor': {
    borderLeftWidth: '2px',
    borderLeftColor: THEME_TOKENS.text,
  },
  '.cm-gutters': {
    backgroundColor: THEME_TOKENS.viewportBg,
    color: THEME_TOKENS.overlay0,
    border: 'none',
  },
  '.cm-activeLineGutter': {
    backgroundColor: THEME_TOKENS.surfaceHover,
    color: THEME_TOKENS.text,
  },
  // NOTE: The '+ hex' suffix pattern appends a two-digit alpha channel to produce
  // 8-digit hex colors (#rrggbbaa). This works because all THEME_TOKENS values are
  // guaranteed to be 6-digit hex strings — do not change tokens to rgb()/hsl() form.
  '.cm-activeLine': {
    backgroundColor: THEME_TOKENS.surface0 + '40',
  },
  '&.cm-focused .cm-cursor': {
    borderLeftColor: THEME_TOKENS.text,
  },
  '.cm-selectionBackground': {
    backgroundColor: THEME_TOKENS.accent + '33',
  },
  '&.cm-focused .cm-selectionBackground': {
    backgroundColor: THEME_TOKENS.accent + '40',
  },
  '.cm-content': {
    caretColor: THEME_TOKENS.text,
  },
};

/**
 * Dark CodeMirror base theme: cursor, gutters, selection, active line.
 */
export const reifyEditorTheme = EditorView.theme(editorThemeSpec, { dark: true });

/**
 * Catppuccin Mocha syntax highlighting for CodeMirror.
 * Uses THEME_TOKENS hex values directly (not CSS variables) because
 * HighlightStyle generates CSS class rules at module init time.
 */
export const reifyHighlightStyle = HighlightStyle.define([
  { tag: t.keyword, color: THEME_TOKENS.mauve },
  { tag: t.number, color: THEME_TOKENS.peach },
  { tag: t.string, color: THEME_TOKENS.green },
  { tag: t.bool, color: THEME_TOKENS.peach },
  { tag: t.variableName, color: THEME_TOKENS.text },
  { tag: t.operator, color: THEME_TOKENS.sky },
  { tag: t.lineComment, color: THEME_TOKENS.overlay0, fontStyle: 'italic' },
  { tag: t.blockComment, color: THEME_TOKENS.overlay0, fontStyle: 'italic' },
  { tag: t.paren, color: THEME_TOKENS.subtext },
  { tag: t.brace, color: THEME_TOKENS.subtext },
]);

/**
 * Combined extension: base theme + syntax highlighting.
 * Import and add to the CodeMirror extensions array in Editor.tsx.
 */
export const reifyHighlighting = syntaxHighlighting(reifyHighlightStyle);
