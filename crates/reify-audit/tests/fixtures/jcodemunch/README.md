# jcodemunch fixture provenance

Real-wire JSON-RPC tool responses captured from `jcodemunch-mcp serve` during the
L-SERVE spike (task 4102).  Consumed by L-CLIENT's decode boundary test to validate
the Rust client without a live serve.

## Capture details

| Field            | Value                                                      |
|------------------|------------------------------------------------------------|
| serve version    | v1.108.27 (`git+https://github.com/jgravelle/jcodemunch-mcp.git@v1.108.27`) |
| python           | 3.12 (via `uvx --python 3.12`)                             |
| transport        | streamable-HTTP, `http://127.0.0.1:8901/mcp`               |
| repo id          | `leodearden/reify`                                         |
| storage path     | `~/.code-index` (schema v16, maintained by jcodemunch-watcher.service) |
| commit range     | `00f56f1a20be3a66a0797663506280be4db9ccf3..27b212c61cfe86bf57055d769921805e34d8b467` |
| capture date     | 2026-05-30T21:19:24Z                                       |

## Fixture formats

Three tools return MUNCH-encoded responses; two `get_layer_violations` fixtures are plain JSON.

**Shape distinction**: MUNCH fixtures are the raw JSON-RPC result envelope (the decoder's
input — `content[0].text` holds the MUNCH-encoded text); the `get_layer_violations` fixtures
are post-decode payloads (the inner JSON object already extracted from the tool text response).
A consumer test must not assume uniform shape across fixture files.

| File                                   | Format     | Top-level key  | Notes                                                               |
|----------------------------------------|------------|----------------|---------------------------------------------------------------------|
| `get_changed_symbols.json`             | MUNCH      | `content`      | JSON-RPC result object; `content[0].text` is MUNCH/1 encoded       |
| `get_dead_code_v2.json`                | MUNCH      | `content`      | Same format                                                         |
| `get_untested_symbols.json`            | MUNCH      | `content`      | Same format                                                         |
| `get_layer_violations.json`            | plain JSON | `violations`   | Post-decode payload; empty array (clean repo at capture time)       |
| `get_layer_violations_populated.json`  | plain JSON | `violations`   | Synthetic fixture; one representative violation record (non-empty decode path) |

MUNCH format: `#MUNCH/1 tool=<name> enc=gen1` header followed by `@N=<string>` reference
definitions and compact symbol records.  L-CLIENT must decode these into structured symbol
data.

> **Non-empty decode coverage**: `get_layer_violations.json` captured an empty `violations`
> array (the reify corpus had no violations under the minimal inline rule at capture time).
> `get_layer_violations_populated.json` is a hand-authored companion fixture containing one
> synthetic violation record (`from`/`to`/`from_symbol`/`to_symbol`/`allowed`/`rule_index`)
> so that L-CLIENT's decode boundary test can exercise the populated array path.  If future
> L-CLIENT tests require a live-captured populated fixture, re-run the capture against a repo
> with known violations and replace this file.

> **Follow-up (L-CLIENT scope)**: the existing `call_tool()` in `fused_memory_client.rs`
> parses `content[0].text` as JSON directly.  MUNCH-encoded text is not JSON, so MUNCH
> decoding is not yet wired in the client.  L-CLIENT should add MUNCH decode logic before
> consuming the three MUNCH fixtures above.

## Reproduce

Start the serve (see `docs/architecture-audit/jcodemunch-serve-activation.md`), then:

```bash
SERVE_URL="http://127.0.0.1:8901/mcp"
SESSION_ID=$(curl -s -D - -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"capture","version":"0.1"}}}' \
  | grep -i '^mcp-session-id:' | sed 's/.*: //' | tr -d '\r')

# Send notifications/initialized
curl -s -o /dev/null -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'

# get_changed_symbols (save .result as fixture)
curl -s -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_changed_symbols","arguments":{"repo":"leodearden/reify","since_sha":"00f56f1a20be3a66a0797663506280be4db9ccf3","until_sha":"27b212c61cfe86bf57055d769921805e34d8b467"}}}' \
  | jq '.result' > crates/reify-audit/tests/fixtures/jcodemunch/get_changed_symbols.json

# get_dead_code_v2 (save .result as fixture)
curl -s -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_dead_code_v2","arguments":{"repo":"leodearden/reify","min_confidence":0.5}}}' \
  | jq '.result' > crates/reify-audit/tests/fixtures/jcodemunch/get_dead_code_v2.json

# get_untested_symbols (save .result as fixture)
curl -s -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_untested_symbols","arguments":{"repo":"leodearden/reify","min_confidence":0.5}}}' \
  | jq '.result' > crates/reify-audit/tests/fixtures/jcodemunch/get_untested_symbols.json

# get_layer_violations — minimal inline rule; save the text content (plain JSON)
curl -s -X POST "$SERVE_URL" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -H "mcp-session-id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_layer_violations","arguments":{"repo":"leodearden/reify","rules":[{"from":"crates/reify-cli","to":"crates/reify-ir","allowed":true}]}}}' \
  | jq -r '.result.content[0].text' | jq '.' \
  > crates/reify-audit/tests/fixtures/jcodemunch/get_layer_violations.json
```

## Offline validation

```bash
bash scripts/check-jcodemunch-fixtures.sh
```
