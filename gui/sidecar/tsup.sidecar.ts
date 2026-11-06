import { defineConfig } from 'tsup';

// The bundle is a Tauri externalBin (build-sidecar.sh copies it to src-tauri/sidecar/, and Tauri places
// it beside the GUI binary), so it runs where no node_modules is reachable: only Node built-ins may stay
// external. noExternal therefore lists every declared dependency that src/ imports. Nothing imports
// @anthropic-ai/claude-code (the claude CLI is spawned by name from PATH), so it is absent.
// src/__tests__/built-bundle.test.ts fails when a dependency is left external.
export default defineConfig({
  entry: ['src/cli.ts'],
  format: ['esm'],
  dts: false,
  outDir: 'dist',
  noExternal: ['@reify/shared-utils', '@modelcontextprotocol/sdk'],
});
