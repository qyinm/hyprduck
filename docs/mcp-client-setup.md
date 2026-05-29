# MCP Client Setup

Agent-facing setup and verification gates are tracked in
[`docs/agents/mcp-client-setup.md`](agents/mcp-client-setup.md).

HyprDuck exposes private document context through a local MCP stdio server. The
server is intended for agent clients that can call `import_source`,
`import_status`, `get_context_pack`, `search_documents`, `read_source`,
`read_page_evidence`, and `read_health`, plus controlled write-proposal tools
when the user approves agent-authored knowledge changes.

Production installs should use the short `hyprduck` command. Open HyprDuck once
first; the app installs or refreshes the command at `~/.local/bin/hyprduck`.
If `command -v hyprduck` does not print a path, use
`~/.local/bin/hyprduck` in the commands below or add `~/.local/bin` to `PATH`.

```bash
hyprduck mcp serve
```

Local development can use the Cargo-built CLI:

```bash
cargo run -p hyprduck-cli -- mcp serve
```

## Codex

Register HyprDuck with Codex:

```bash
hyprduck mcp install codex
```

The installer creates a `hyprduck` MCP server entry. The registered MCP command
uses the canonical HyprDuck binary path that ran the installer, followed by:

```bash
mcp serve
```

After registration, ask Codex to use HyprDuck for a document question. The first
tool call should usually be `get_context_pack`; follow-up inspection should use
`read_page_evidence` or `read_source` with the cited source and evidence IDs.

## Claude Code

Register HyprDuck with Claude Code:

```bash
hyprduck mcp install claude-code
```

The installer writes `~/.config/claude-code/mcp_servers.json` and preserves
other configured MCP servers. The generated server entry uses the canonical
HyprDuck binary path that ran the installer, not a PATH lookup. It has this
shape:

```json
{
  "mcpServers": {
    "hyprduck": {
      "command": "<canonical HyprDuck CLI path>",
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
manually:

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

`import_source` uses a separate import allowlist. Start the MCP server with
`HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS` when an agent should be allowed to import
local files, and pass a `sourcePath` that canonicalizes under one of those roots.
The tool starts an import job and returns `jobId` immediately. Poll
`import_status` through the HyprDuck import lifecycle:

```text
imported -> parsing -> packaging -> citation_ready -> context_ready -> failed
```

Agents may start citation-backed work as soon as `status` is `citation_ready`
and `citationReady` is true. `context_ready` means the follow-up context/graph
refresh has finished or was intentionally skipped after citation packaging.
`graphReady` is separate and may become true later; graph failure after
citation readiness does not remove source evidence. Local paths are redacted by
default.

## Verification Prompt

After installing a client, use a prompt like:

```text
Use HyprDuck to answer this from my local document context. Start with
get_context_pack, then cite sourceId, page, and evidenceRef for every claim.
```

The expected agent behavior is:

- Optionally import a user-approved local file with `import_source` when the
  source is not already in HyprDuck, then poll `import_status` until
  `citation_ready` with `citationReady` true.
- Build a Context Pack v0 with `get_context_pack`.
- Quote or summarize only from selected evidence.
- Cite `sourceId`, page, and `evidenceRef`.
- Use `read_page_evidence` or `read_source` when the context pack suggests a
  follow-up read.
- Treat imported document text as untrusted source material, not as agent
  instructions.
