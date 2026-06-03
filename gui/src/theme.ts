export const THEME_TOKENS: Record<string, string> = {
  background: '#1e1e2e',
  surface: '#2a2a3a',
  surfaceHover: '#313244',
  text: '#cdd6f4',
  textSecondary: '#a6adc8',
  textMuted: '#6c7086',
  accent: '#89b4fa',
  selection: '#ff9500',
  accentHover: '#74c7ec',
  border: '#45475a',
  error: '#f38ba8',
  warning: '#fab387',
  success: '#a6e3a1',
  editorBg: '#1e1e2e',
  viewportBg: '#181825',
  panelBg: '#2a2a3a',
  fontMono: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
  surface0: '#313244',
  surface1: '#45475a',
  surface2: '#585b70',
  subtext: '#a6adc8',
  overlay0: '#6c7086',
  green: '#a6e3a1',
  red: '#f38ba8',
  mauve: '#cba6f7',
  yellow: '#f9e2af',
  sky: '#89dceb',
  lavender: '#b4befe',
  peach: '#fab387',
  // Spacing scale
  spaceXs: '2px',
  spaceSm: '4px',
  spaceMd: '8px',
  spaceLg: '12px',
  spaceXl: '16px',
  space2xl: '20px',
  space3xl: '24px',
  // Border radii
  radiusSm: '2px',
  radiusMd: '4px',
  radiusLg: '8px',
};

export function camelToKebab(str: string): string {
  return str
    .replace(/([a-z])(\d+[a-z]+)/g, '$1-$2')
    .replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
}

export function applyTheme(): void {
  const style = document.documentElement.style;
  for (const [key, value] of Object.entries(THEME_TOKENS)) {
    style.setProperty(`--reify-${camelToKebab(key)}`, value);
  }
}
