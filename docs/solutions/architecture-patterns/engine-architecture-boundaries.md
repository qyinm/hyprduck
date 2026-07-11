---
title: Engine architecture boundaries
date: 2026-06-01
category: architecture-patterns
module: Rust engine application/domain/adapter boundaries
problem_type: architecture_pattern
component: development_workflow
severity: high
applies_when:
  - "Rust engine responsibilities start accumulating in a single command, library, or transport module."
  - "MCP, desktop, CLI, and provider workflows need shared behavior without duplicating orchestration logic."
  - "Domain policy, persistence, parsing, provider clients, and process lookup need clear ownership boundaries."
related_components:
  - tooling
  - database
  - documentation
tags:
  - rust-engine
  - architecture-boundaries
  - application-layer
  - adapters
  - mcp-policy
  - graph-commit
  - import-lifecycle
  - desktop-dev
---

# Engine Architecture Boundaries

## Context

The Rust engine had absorbed too many responsibilities into broad modules: request handling, persistence, document parsing, graph patch validation, read-model projection, provider execution, process lookup, and desktop runtime assumptions were easy to couple accidentally. That made reviews harder because a tactical fix could hide a product boundary issue.

PR #48, `Refactor engine architecture boundaries`, moved the engine toward explicit application, domain, and adapter ownership. It also fixed review findings that proved why those boundaries mattered: MCP local path disclosure had to stay server-gated, graph patches had to preserve existing record refs, write commits had to be replay-safe, import lifecycle recovery had to distinguish citation readiness from graph readiness, and desktop dev had to resolve the unpackaged engine binary correctly.

Session history reinforced the same design pressure. Earlier SQLite and GraphQLite planning established DB-first persistence, GraphQLite as the current graph store, BrainEvent as the audit log, and hybrid retrieval as base architecture rather than a later add-on (session history). That direction requires explicit transaction and materialization boundaries instead of file-artifact or transport-owned orchestration.

## Guidance

Keep the engine layered by ownership, not by whichever command first needed the code:

```text
crates/etyma-engine/src/application/commands/
crates/etyma-engine/src/application/services/
crates/etyma-engine/src/domains/
crates/etyma-engine/src/adapters/
```

Use these boundaries when placing new code:

- `application/commands`: thin command handlers that translate typed requests into domain and service calls.
- `application/services`: orchestration that spans multiple domain or adapter operations.
- `domains/*`: product concepts and policy that should be independent of a specific transport or storage implementation.
- `adapters/*`: concrete document parsers, provider clients, persistence stores, and process or binary lookup.
- Root-level policy modules such as `graph_patch_policy.rs`, `graph_commit.rs`, and MCP `policy.rs`: narrow contracts whose audit and safety behavior should be obvious in review.

Do not introduce empty pattern scaffolding just because the architecture names a layer. PR #48 deliberately removed placeholder modules such as empty `ports`, `application::policies`, `application::workflows`, and `adapters::events`. Add traits, builders, visitors, or command abstractions only when there is a real variation point, repeated call shape, or test boundary that benefits from it.

Use review findings as boundary tests:

- MCP read tools redact local paths by default. Desktop paths may opt in through `includeLocalPaths`, but MCP server behavior must remain gated by explicit local-path disclosure policy.
- `graph_patch_apply` validates source and evidence scope before writing, then merges with existing graph records instead of overwriting record refs or provenance.
- `write_commit` and `write_commit_all` keep proposal state, memory events, and persistence ordering replay-safe.
- Import lifecycle code treats citation-ready evidence as usable even when graph materialization is pending, skipped, or retrying.
- Desktop development resolves the unpackaged `target/debug/etyma-engine` binary instead of assuming packaged app layout.

When adding behavior, ask which layer owns the durable invariant. Transport modules can expose a tool or IPC route, but they should not own evidence validation, graph commit semantics, provider persistence, or output packaging.

## Why This Matters

Etyma is an evidence compiler for coding agents. The product boundary depends on preserving provider route, local or hosted status, source refs, evidence refs, graph commit state, and local-path disclosure consistently across desktop and MCP surfaces.

DB-first storage increases the cost of hidden coupling. When GraphQLite holds current graph state and relational tables hold source, page, evidence, import job, and audit data, a mutation is not just a file write. It is a transaction across evidence validation, graph projection, read-model publication, and event history.

Clear boundaries also keep the product from drifting into generic pattern theater. The architecture is useful only when each layer owns concrete behavior that reviewers can test: command routing, orchestration, domain policy, adapter I/O, and explicit mutation contracts.

## When to Apply

- Refactoring broad engine command handlers or `lib.rs`-style modules.
- Adding or changing mutating MCP tools such as graph patch, write proposal, or write commit flows.
- Moving provider, parser, process lookup, persistence, or output packaging code.
- Changing import lifecycle states, citation readiness, graph readiness, or retry behavior.
- Fixing desktop startup behavior that depends on packaged versus unpackaged runtime layout.
- Introducing Builder, Visitor, Command, or trait-based seams only after a concrete variation point exists.

## Examples

Prefer this placement shape when splitting a command that has grown too large:

```text
application/commands/graph.rs
  -> request decoding and typed command dispatch

graph_patch_policy.rs
  -> source/evidence/relation/claim/wiki validation policy

graph_commit.rs
  -> graph materialization and record merge semantics

adapters/persistence/knowledge_store.rs
  -> concrete SQLite persistence and read-model queries
```

For desktop development, keep packaged and unpackaged runtime assumptions separate. The dev shell should resolve the local engine binary:

```javascript
function resolveEnginePath() {
  if (isPackaged()) {
    return path.join(process.resourcesPath, "etyma-engine");
  }

  return path.resolve(__dirname, "../../target/debug/etyma-engine");
}
```

For mutating MCP work, the useful review checklist is:

```text
1. Is the tool annotated as mutating and scoped to a named operation?
2. Are workspace, source, and evidence refs validated before any write?
3. Does persistence preserve existing record refs and provenance?
4. Is the local-path disclosure policy applied before returning agent-facing data?
5. Does the response tell downstream graph/wiki readers when caches or read models changed?
```

Useful verification targets for this architecture family:

```bash
cargo fmt
cargo test -p etyma-engine
cargo test -p etyma-cli --test mcp_server
bun test apps/desktop/src/features/workspace/materializedGraphSnapshot.test.ts
git diff --check
```

## Related

- [Agent graph patch MCP contract](agent-graph-patch-mcp-contract.md) documents the narrower agent-generated graph patch contract that sits inside this boundary.
- [MCP import graph readiness recovery](../integration-issues/mcp-import-graph-readiness-recovery.md) documents citation-ready versus graph-ready lifecycle recovery.
- [docs/ARCHITECTURE.md](../../ARCHITECTURE.md) summarizes the current app, engine, storage, and graph materialization boundaries.
- [docs/mcp.md](../../mcp.md) documents MCP read and mutation tools, including local-path disclosure and graph patch behavior.
- PR #48, `Refactor engine architecture boundaries`, merged as commit `950b789`.
