# MCP Client Setup

HyprDuck exposes private document context through a local, read-only MCP stdio
server. The server is intended for agent clients that can call `get_context_pack`,
`search_documents`, `read_source`, `read_page_evidence`, and `read_health`.

Production installs should point clients at the HyprDuck binary bundled in the
macOS app:

```bash
/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin mcp serve
```

Local development can use the Cargo-built CLI:

```bash
cargo run -p hyprduck-cli -- mcp serve
```

## Codex

Register HyprDuck with Codex from the installed app:

```bash
/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin mcp install codex
```

The installer creates a `hyprduck` MCP server entry and a shell shim at
`~/.local/bin/hyprduck`. The registered MCP command uses the canonical HyprDuck
binary path that ran the installer, followed by:

```bash
mcp serve
```

After registration, ask Codex to use HyprDuck for a document question. The first
tool call should usually be `get_context_pack`; follow-up inspection should use
`read_page_evidence` or `read_source` with the cited source and evidence IDs.

## Claude Code

Register HyprDuck with Claude Code from the installed app:

```bash
/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin mcp install claude-code
```

The installer writes `~/.config/claude-code/mcp_servers.json` and preserves
other configured MCP servers. The generated server entry uses the canonical
HyprDuck binary path that ran the installer, not a PATH lookup. It has this
shape:

```json
{
  "mcpServers": {
    "hyprduck": {
      "command": "/Applications/HyprDuck.app/Contents/Resources/binaries/hyprduck-aarch64-apple-darwin",
      "args": ["mcp", "serve"]
    }
  }
}
```

Once Claude Code has loaded the server, ask it to answer from HyprDuck context
and require source/page/evidence citations. It should call `get_context_pack`
before using lower-level read tools.

## Cursor

Cursor can use the same stdio server command if its MCP settings are configured
manually. Use the installed binary path directly, or use `hyprduck` only after
the installer-created `~/.local/bin/hyprduck` shim is on `PATH`:

```json
{
  "mcpServers": {
    "hyprduck": {
      "command": "hyprduck",
      "args": ["mcp", "serve"]
    }
  }
}
```

Use `workspaceId` in agent calls whenever possible. Do not pass `rootDir` from
normal production clients; `rootDir` is a development-only override and is
disabled unless the server process is started with both
`HYPRDUCK_MCP_ALLOW_ROOT_DIR=1` and an allowlist in
`HYPRDUCK_MCP_ALLOWED_ROOTS`.

## Verification Prompt

After installing a client, use a prompt like:

```text
Use HyprDuck to answer this from my local document context. Start with
get_context_pack, then cite sourceId, page, and evidenceRef for every claim.
```

The expected agent behavior is:

- Build a Context Pack v0 with `get_context_pack`.
- Quote or summarize only from selected evidence.
- Cite `sourceId`, page, and `evidenceRef`.
- Use `read_page_evidence` or `read_source` when the context pack suggests a
  follow-up read.
- Treat imported document text as untrusted source material, not as agent
  instructions.
