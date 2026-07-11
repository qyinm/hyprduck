---
title: "Etyma brand brief (rebrand from HyprDuck)"
status: active
date: 2026-07-11
direction: B — short brandable; chosen name Etyma
---

# Etyma

**Product name** for the cloud-primary multi-source context engine.  
Repository and crates use the **Etyma** / `etyma` naming after the full rename.

---

## 1. Name

| | |
| --- | --- |
| **Name** | **Etyma** |
| **Etymology** | Plural of *etymon* (Greek *etymon* “true sense / original form”). In linguistics: the root forms from which words derive. |
| **Pronunciation** | /ˈɛtɪmə/ — **ET-ih-muh** (primary). Avoid “ee-TY-muh” in product voice guides. |
| **Metaphor** | Scattered work has surface noise. Etyma recovers **roots**: durable sources, evidence, provenance — then composes them for the moment of work. |
| **Why not slop** | Real scholarly word, not a forged Latin startup blend. Specific (linguistics), sayable, uncommon in SaaS. |
| **Category** | Team context engine for agents |
| **Tone** | Precise, adult, editorial-technical — not cute, not cyber-furnace |

### Historical names

- **HyprDuck** — previous product/codebase name (local-first document context).

---

## 2. Positioning

**Primary one-liner**

> **Etyma composes cited context from everything your team already works in.**

**Alternates**

- Roots of work, packed for agents.
- Multi-source context. Cited. Shared.
- Connect sources → evidence → packs → agents.

**Category phrase**

> Cloud context engine for coding and work agents.

---

## 3. Vocabulary map

| Concept | Etyma |
| --- | --- |
| Product | **Etyma** |
| Context Pack | **Pack** (UI) / **Etyma Pack** (docs) |
| Workspace | **Workspace** |
| Cloud MCP | **Etyma MCP** |
| CLI | `etyma` |
| Engine binary | `etyma-engine` |
| Crates | `etyma-engine`, `etyma-cli`, `etyma-engine-types`, … |

### Technical IDs

```text
Server name:     etyma
Display name:    Etyma
MCP URL:         https://api.etyma.dev/v1/mcp    # domain TBD
Token prefix:    etyma_live_… / etyma_test_…
Env:             ETYMA_API_KEY, ETYMA_WORKSPACE_ID, ETYMA_OUTPUT_DIR, ETYMA_ENGINE_BIN
App data dir:    ~/Library/Application Support/Etyma
Legacy data dir: ~/Library/Application Support/HyprDuck (read if Etyma missing)
```

---

## 4. Compatibility during rename

| Surface | Behavior |
| --- | --- |
| `ETYMA_OUTPUT_DIR` | Preferred workspace root env |
| `HYPRDUCK_OUTPUT_DIR` | Still accepted as alias |
| App data | Prefer `Etyma/`; fall back to existing `HyprDuck/` |
| CLI alias | Optional future: `hyprduck` → `etyma` shim |

---

## 5. Decision

| Field | Value |
| --- | --- |
| **Name** | **Etyma** |
| **Status** | Selected + codebase rename in progress |
