import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/cli.ts'],
  format: ['esm'],
  dts: false,
  outDir: 'dist',
  noExternal: ['@reify/shared-utils'],
});
