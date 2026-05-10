import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const appSource = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const graphSource = readFileSync(
  new URL("../src/features/workspace/GraphWorkspace.tsx", import.meta.url),
  "utf8",
);
const previewSource = readFileSync(
  new URL("../src/features/workspace/buildWorkspacePreview.ts", import.meta.url),
  "utf8",
);
const stylesSource = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const iaSource = readFileSync(new URL("../IA.md", import.meta.url), "utf8");

test("desktop IA is the committed source of truth for the Knowledge workspace", () => {
  expect(iaSource).toMatch(/Source와 Ask는 destination이 아니라 Knowledge workspace 안의 interaction surface/);
  expect(iaSource).toMatch(/File attachment distinguishes Add to knowledge base from Ask only this time/);
});

test("app shell exposes only Knowledge and Settings as primary destinations", () => {
  expect(appSource).toMatch(/type ActivePanel = "knowledge" \| "settings"/);
  expect(appSource).toMatch(/label: "Knowledge"/);
  expect(appSource).not.toMatch(/label: "Import"/);
  expect(appSource).not.toMatch(/label: "Graph"/);
  expect(appSource).toMatch(/Knowledge maintenance/);
});

test("app shell exposes fixed window chrome independent of sidebar", () => {
  expect(appSource).toMatch(/fixed left-\[76px\] top-\[10px\]/);
  expect(appSource).toMatch(/fixed right-3 top-\[10px\]/);
  expect(appSource).toMatch(/native titlebar area stays empty/);
  expect(appSource).toMatch(/windowChromeButtonClass/);
  expect(appSource).not.toMatch(/className=\"size-7\"/);
});

test("desktop visual tokens follow DESIGN.md restraint", () => {
  expect(stylesSource).toMatch(/--primary: oklch\(0 0 0\)/);
  expect(stylesSource).not.toMatch(/0\.55 0\.28 300/);
  expect(stylesSource).not.toMatch(/fontsource-variable\/geist/);
  expect(graphSource).not.toMatch(/teal|gradient|shadow-\[/);
});

test("Knowledge empty state gives first users add-file and prompt affordances", () => {
  expect(graphSource).toMatch(/Your knowledge base is empty/);
  expect(graphSource).toMatch(/Drop PDF, DOCX, or DOC files here/);
  expect(graphSource).toMatch(/Add files or ask about your knowledge/);
  expect(graphSource).not.toMatch(/Go to Import/);
  expect(graphSource).not.toMatch(/compile|compiled|compiler/i);
  expect(previewSource).not.toMatch(/compile|compiled|compiler/i);
});

test("Graph workspace has IA modes and source-file inspector actions", () => {
  for (const label of ["Graph", "Wiki", "Sources", "Claims", "Conflicts"]) {
    expect(graphSource).toMatch(new RegExp(`>${label}<|${label}`));
  }
  expect(graphSource).toMatch(/Open source detail/);
  expect(graphSource).toMatch(/Original file/);
  expect(graphSource).toMatch(/Derived artifacts/);
  expect(graphSource).toMatch(/Open uploaded file/);
  expect(graphSource).toMatch(/Reveal in Finder/);
  expect(graphSource).toMatch(/Right inspector/);
  expect(graphSource).toMatch(/without leaving the graph/);
});

test("bottom prompt composer supports attachment intent and source metadata", () => {
  expect(graphSource).toMatch(/Ask or add files to this knowledge base/);
  expect(graphSource).toMatch(/Add to knowledge base/);
  expect(graphSource).toMatch(/Ask only this time/);
  expect(graphSource).toMatch(/File description/);
  expect(graphSource).toMatch(/\+ Attach files/);
  expect(graphSource).toMatch(/source\.description|source metadata/);
});

test("evidence is rendered as UI content instead of raw markdown", () => {
  expect(graphSource).toMatch(/formatEvidenceSnippet/);
  expect(graphSource).toMatch(/extractMarkdownImageLabel/);
  expect(graphSource).toMatch(/Page image:/);
});
