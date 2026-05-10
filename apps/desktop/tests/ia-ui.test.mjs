import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const graphSource = readFileSync(
  new URL("../src/features/workspace/GraphWorkspace.tsx", import.meta.url),
  "utf8",
);
const iaSource = readFileSync(new URL("../IA.md", import.meta.url), "utf8");

test("desktop IA is the committed source of truth for the Knowledge workspace", () => {
  assert.match(iaSource, /Source와 Ask는 destination이 아니라 Knowledge workspace 안의 interaction surface/);
  assert.match(iaSource, /File attachment distinguishes Add to knowledge base from Ask only this time/);
});

test("app shell exposes only Knowledge and Settings as primary destinations", () => {
  assert.match(appSource, /type ActivePanel = "knowledge" \| "settings"/);
  assert.match(appSource, /label: "Knowledge"/);
  assert.doesNotMatch(appSource, /label: "Import"/);
  assert.doesNotMatch(appSource, /label: "Graph"/);
  assert.match(appSource, /Knowledge maintenance/);
});

test("Knowledge empty state gives first users add-file and prompt affordances", () => {
  assert.match(graphSource, /Your knowledge base is empty/);
  assert.match(graphSource, /Drop PDF, DOCX, or DOC files here/);
  assert.match(graphSource, /Add files or ask about your knowledge/);
  assert.doesNotMatch(graphSource, /Go to Import/);
  assert.doesNotMatch(graphSource, /compile-backed knowledge layer/);
});

test("Graph workspace has IA modes and source-file inspector actions", () => {
  for (const label of ["Graph", "Wiki", "Sources", "Claims", "Conflicts"]) {
    assert.match(graphSource, new RegExp(`>${label}<|${label}`));
  }
  assert.match(graphSource, /Open source detail/);
  assert.match(graphSource, /Open uploaded file/);
  assert.match(graphSource, /Reveal in Finder/);
});

test("bottom prompt composer supports attachment intent and source metadata", () => {
  assert.match(graphSource, /Ask or add files to this knowledge base/);
  assert.match(graphSource, /Add to knowledge base/);
  assert.match(graphSource, /Ask only this time/);
  assert.match(graphSource, /File description/);
  assert.match(graphSource, /source\.description|source metadata/);
});
