// The MCP SDK, whose types the Agent SDK's import, names the DOM's global `HeadersInit`, which
// Bun's types keep in their `Bun` namespace. This names Bun's globally, so the host's check,
// which reads the libraries' types too, finds it.

declare global {
  type HeadersInit = Bun.HeadersInit;
}

export {};
