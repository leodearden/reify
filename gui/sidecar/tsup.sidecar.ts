import { defineConfig } from 'tsup';

// The bundle is a Tauri externalBin (build-sidecar.sh copies it to src-tauri/sidecar/, and Tauri places
// it beside the GUI binary), so it runs where no node_modules is reachable: only Node built-ins may stay
// external. noExternal therefore lists every declared dependency that src/ imports. Nothing imports
// @anthropic-ai/claude-code (the claude CLI is spawned by name from PATH), so it is absent.
// It is also an extensionless file run by `node`, and Node 18 (Ubuntu 24.04's nodejs, which the deb depends
// on) loads such a file as CommonJS, so it must be CJS; node18 is the syntax floor. built-bundle.test.ts
// fails when a dependency is left external or the bundle does not load as CommonJS.
// clean empties outDir first, so after a format change build-sidecar.sh fails on its missing dist/cli.cjs
// instead of shipping a stale one.
export default defineConfig({
  entry: ['src/cli.ts'],
  format: ['cjs'],
  target: 'node18',
  clean: true,
  dts: false,
  outDir: 'dist',
  noExternal: ['@reify/shared-utils', '@modelcontextprotocol/sdk'],
});
