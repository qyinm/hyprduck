const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const desktopRoot = path.resolve(__dirname, "..");

function readSource(relativePath, scriptKind) {
  const filePath = path.join(desktopRoot, relativePath);
  return ts.createSourceFile(
    filePath,
    fs.readFileSync(filePath, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    scriptKind,
  );
}

function propertyNameText(name) {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name)) {
    return name.text;
  }
  return null;
}

function collectDesktopCommandMapKeys() {
  const sourceFile = readSource("src/appTypes.ts", ts.ScriptKind.TS);
  const commands = new Set();

  function visit(node) {
    if (ts.isInterfaceDeclaration(node) && node.name.text === "DesktopCommandMap") {
      for (const member of node.members) {
        if (member.name) {
          const name = propertyNameText(member.name);
          if (name) {
            commands.add(name);
          }
        }
      }
      return;
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return commands;
}

function collectMainCommandCases() {
  const sourceFile = readSource("main.cjs", ts.ScriptKind.JS);
  const commands = new Set();

  function visit(node) {
    if (
      ts.isSwitchStatement(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === "command"
    ) {
      for (const clause of node.caseBlock.clauses) {
        if (
          ts.isCaseClause(clause) &&
          ts.isStringLiteralLike(clause.expression)
        ) {
          commands.add(clause.expression.text);
        }
      }
      return;
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return commands;
}

function collectWebPreviewHandlers() {
  const sourceFile = readSource("src/webPreviewApi.ts", ts.ScriptKind.TS);
  const commands = new Set();

  function visit(node) {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "handlers" &&
      node.initializer &&
      ts.isObjectLiteralExpression(node.initializer)
    ) {
      for (const property of node.initializer.properties) {
        if (
          (ts.isPropertyAssignment(property) || ts.isMethodDeclaration(property)) &&
          property.name
        ) {
          const name = propertyNameText(property.name);
          if (name) {
            commands.add(name);
          }
        }
      }
      return;
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return commands;
}

function sorted(values) {
  return [...values].sort((left, right) => left.localeCompare(right));
}

function difference(left, right) {
  return sorted(left).filter((value) => !right.has(value));
}

function assertSameContract(label, expected, actual) {
  const missing = difference(expected, actual);
  const extra = difference(actual, expected);
  if (missing.length === 0 && extra.length === 0) {
    return;
  }

  const details = [];
  if (missing.length > 0) {
    details.push(`missing from ${label}: ${missing.join(", ")}`);
  }
  if (extra.length > 0) {
    details.push(`extra in ${label}: ${extra.join(", ")}`);
  }
  throw new Error(details.join("\n"));
}

const typedCommands = collectDesktopCommandMapKeys();
assertSameContract("main.cjs IPC switch", typedCommands, collectMainCommandCases());
assertSameContract("web preview handlers", typedCommands, collectWebPreviewHandlers());

console.log(
  `Desktop command contract covers ${typedCommands.size} commands: ${sorted(typedCommands).join(", ")}`,
);
