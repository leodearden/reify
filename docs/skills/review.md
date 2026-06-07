# /review skill — reify-debug MCP reference

When reviewing a GUI change for layout/diagnostic regressions against a live reify-gui
session, consult the recipe in [docs/debug-mcp-recipe.md](../debug-mcp-recipe.md) for:

- How to boot the debug server (`scripts/run-gui-dev.sh`, `REIFY_DEBUG_PORT`)
- The /review tool sequence (load_fixture → wait_for_idle → ui_outline → get_layout_metrics → list_console_errors → screenshot)
- Per-element capture with `element_screenshot({testId: '…'})`
- In-band error handling for stuck renderer/engine conditions

The contract (coordinates, transport, error envelope) is in
[docs/debug-mcp-contract.md](../debug-mcp-contract.md).
