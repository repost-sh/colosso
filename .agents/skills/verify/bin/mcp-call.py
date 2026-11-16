"""One MCP tool call on a test Koyo through `koyo mcp` over stdio, for the tools `koyo app`
doesn't have (terminal_*). Run by `koyo-verify mcp`, which sets the environment:

    python3 -I mcp-call.py <koyo binary> <client name> <tool | --list> ['{json}']

Prints the call's structured result as JSON (an image's base64 left out), exit 1 on an error.
"""

import json
import subprocess
import sys


def main():
    binary, client, tool = sys.argv[1], sys.argv[2], sys.argv[3]
    arguments = json.loads(sys.argv[4]) if len(sys.argv) > 4 and sys.argv[4] else {}
    relay = subprocess.Popen(
        [binary, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    def send(message):
        relay.stdin.write(json.dumps(message) + "\n")
        relay.stdin.flush()

    def answer(wanted):
        while True:
            line = relay.stdout.readline()
            if not line:
                sys.exit("koyo mcp closed: " + relay.stderr.read())
            message = json.loads(line)
            if message.get("id") == wanted:
                return message

    try:
        send({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": client, "version": "1"},
            },
        })
        answer(1)
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        if tool == "--list":
            send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
            for found in answer(2)["result"]["tools"]:
                print(found["name"])
            return 0
        send({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        })
        reply = answer(2)
    finally:
        relay.stdin.close()
        relay.terminate()
    if "error" in reply:
        print(json.dumps(reply["error"], indent=2))
        return 1
    result = reply["result"]
    shown = result.get("structuredContent")
    if shown is None:
        shown = [
            {"image": block.get("mimeType")} if block.get("type") == "image" else block.get("text")
            for block in result.get("content", [])
        ]
    print(json.dumps(shown, indent=2))
    return 1 if result.get("isError") else 0


if __name__ == "__main__":
    sys.exit(main())
