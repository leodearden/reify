"""escalation_mcp.py — how a reify script reaches the escalation server's MCP.

A minimal streamable-HTTP MCP client (McpSession: initialize, then tools/call)
whose call_tool returns the tool's decoded JSON payload, or raises
EscalationError when the server answered but did not accept the call; and
escalation_url_from_mcp_config, which reads the endpoint a checkout declares in
its .mcp.json. Network failures surface as OSError / ValueError /
http.client.HTTPException, exactly as urllib raises them.
"""

import json
import urllib.request

MCP_PROTOCOL_VERSION = "2024-11-05"
MCP_TIMEOUT_SECONDS = 30


class EscalationError(Exception):
    """The escalation server answered, but did not accept the call."""


class McpConfigError(Exception):
    """A .mcp.json file does not declare a usable escalation endpoint."""


class McpSession:
    """A minimal streamable-HTTP MCP client: initialize, then tools/call.

    Keeps the server's mcp-session-id and sends it on every later post.
    Accepts both plain-JSON and SSE (`data:` line) reply bodies.
    """

    def __init__(self, url, client_name):
        self.url = url
        self.client_name = client_name
        self.session_id = None
        self.next_id = 0

    def open(self):
        self._post(
            self._request(
                "initialize",
                {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": self.client_name, "version": "1"},
                },
            )
        )
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call_tool(self, name, arguments):
        """The tool's decoded JSON payload; EscalationError if it was refused."""
        reply = self._post(self._request("tools/call", {"name": name, "arguments": arguments}))
        return tool_payload(reply, name)

    def _request(self, method, params):
        self.next_id += 1
        return {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}

    def _post(self, payload):
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session_id:
            headers["mcp-session-id"] = self.session_id
        request = urllib.request.Request(
            self.url, data=json.dumps(payload).encode(), headers=headers
        )
        with urllib.request.urlopen(request, timeout=MCP_TIMEOUT_SECONDS) as response:
            self.session_id = response.headers.get("mcp-session-id") or self.session_id
            body = response.read().decode("utf-8")
        return reply_message(body)


def reply_message(body):
    """The JSON-RPC message in a reply body, or None for an empty (202) reply."""
    data_lines = [line[len("data:"):].strip() for line in body.splitlines() if line.startswith("data:")]
    if data_lines:
        return json.loads(data_lines[-1])
    return json.loads(body) if body.strip() else None


def tool_payload(reply, tool_name):
    """The JSON object a tools/call reply carries; EscalationError if refused."""
    if reply is None:
        raise EscalationError("empty tools/call reply")
    if "error" in reply:
        raise EscalationError(f"JSON-RPC error: {reply['error']}")
    result = reply.get("result") or {}
    text = "".join(
        item.get("text", "") for item in result.get("content", []) if item.get("type") == "text"
    )
    if result.get("isError"):
        raise EscalationError(f"{tool_name} returned an error: {text}")
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as error:
        raise EscalationError(f"unparseable {tool_name} payload {text!r}: {error}") from error
    if not isinstance(payload, dict) or "error" in payload:
        raise EscalationError(f"{tool_name} refused the filing: {payload}")
    return payload


def escalation_url_from_mcp_config(path):
    """mcpServers.escalation.url of a .mcp.json-shaped file; McpConfigError otherwise."""
    try:
        with open(path, encoding="utf-8") as config_file:
            config = json.load(config_file)
    except (OSError, ValueError) as error:
        raise McpConfigError(f"cannot read MCP config {path}: {error}") from error
    url = config
    for key in ("mcpServers", "escalation", "url"):
        url = url.get(key) if isinstance(url, dict) else None
    if not isinstance(url, str) or not url:
        raise McpConfigError(f"MCP config {path} declares no mcpServers.escalation.url")
    return url
