# /verify skill — reify-debug MCP reference

When verifying a GUI change against a live reify-gui session, consult the recipe in
[docs/debug-mcp-recipe.md](../debug-mcp-recipe.md) for:

- How to boot the debug server (`scripts/run-gui-dev.sh`, `REIFY_DEBUG_PORT`)
- The /verify tool sequence (open_file → wait_for_idle → store_state → get_diagnostics → screenshot)
- How to run the full e2e value-assertion suite (`npm --prefix gui run test:e2e`)
- In-band error handling for stuck renderer/engine conditions

The contract (coordinates, transport, error envelope) is in
[docs/debug-mcp-contract.md](../debug-mcp-contract.md).
