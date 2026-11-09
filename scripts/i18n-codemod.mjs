#!/usr/bin/env node
// Wrap user-facing Chinese literals in t() so the UI can switch languages.
// This is a one-shot codemod; it is intentionally conservative about contexts
// where a string is data (object keys, switch cases, comparisons, types).
import ts from "typescript";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(ROOT, "src");
const I18N = join(SRC, "lib", "i18n.ts");
const HAN = /[\u3400-\u9fff]/;
const SKIP_FILES = new Set([I18N, join(SRC, "lib", "i18n.en.ts")]);
const SKIP_ATTRS = new Set([
  "className",
  "id",
  "role",
  "type",
  "name",
  "htmlFor",
  "key",
  "href",
  "src",
  "target",
  "rel",
  "value",
  "accept",
]);

const files = [];
async function walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "test" || entry.name === "__tests__") continue;
      await walk(full);
    } else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.endsWith(".d.ts")) {
      if (!SKIP_FILES.has(full)) files.push(full);
    }
  }
}

function isHan(text) {
  return HAN.test(text);
}

function normalize(text) {
  return text.replace(/\s+/g, " ").trim();
}

function hasAncestorT(node) {
  let current = node.parent;
  while (current) {
    if (ts.isCallExpression(current)) {
      const callee = current.expression;
      if (ts.isIdentifier(callee) && (callee.text === "t" || callee.text === "errorText")) {
        return true;
      }
    }
    current = current.parent;
  }
  return false;
}

function isTypePosition(node) {
  let current = node.parent;
  while (current) {
    if (
      ts.isTypeNode(current) ||
      ts.isInterfaceDeclaration(current) ||
      ts.isTypeAliasDeclaration(current)
    ) {
      return true;
    }
    if (ts.isStatement(current) || ts.isExpression(current)) return false;
    current = current.parent;
  }
  return false;
}

function shouldSkipLiteral(node) {
  if (!isHan(node.text)) return true;
  if (hasAncestorT(node) || isTypePosition(node)) return true;
  const parent = node.parent;
  if (!parent) return true;
  if (
    ts.isImportDeclaration(parent) ||
    ts.isExportDeclaration(parent) ||
    ts.isExternalModuleReference(parent) ||
    ts.isImportSpecifier(parent) ||
    ts.isExportSpecifier(parent) ||
    ts.isEnumMember(parent) ||
    ts.isCaseClause(parent) ||
    ts.isLabeledStatement(parent) ||
    ts.isPropertyAssignment(parent) && parent.name === node ||
    ts.isPropertyDeclaration(parent) && parent.name === node ||
    ts.isPropertySignature(parent) && parent.name === node ||
    ts.isMethodDeclaration(parent) && parent.name === node ||
    ts.isMethodSignature(parent) && parent.name === node ||
    ts.isGetAccessorDeclaration(parent) && parent.name === node ||
    ts.isSetAccessorDeclaration(parent) && parent.name === node ||
    ts.isQualifiedName(parent) ||
    ts.isJsxAttribute(parent)
  ) {
    return true;
  }
  if (ts.isElementAccessExpression(parent) && parent.argumentExpression === node) return true;
  if (ts.isBinaryExpression(parent)) {
    const op = parent.operatorToken.kind;
    const comparison =
      op === ts.SyntaxKind.EqualsEqualsEqualsToken ||
      op === ts.SyntaxKind.ExclamationEqualsEqualsToken ||
      op === ts.SyntaxKind.EqualsEqualsToken ||
      op === ts.SyntaxKind.ExclamationEqualsToken ||
      op === ts.SyntaxKind.LessThanToken ||
      op === ts.SyntaxKind.GreaterThanToken ||
      op === ts.SyntaxKind.LessThanEqualsToken ||
      op === ts.SyntaxKind.GreaterThanEqualsToken;
    if (comparison) return true;
  }
  if (ts.isCallExpression(parent)) {
    const callee = parent.expression;
    if (ts.isPropertyAccessExpression(callee) && callee.expression.getText() === "console") {
      return true;
    }
    if (ts.isIdentifier(callee) && callee.text === "require") return true;
  }
  return false;
}

function literalExpression(node, callee) {
  return `${callee}(${JSON.stringify(node.text)})`;
}

function expressionSource(node, callee) {
  if (ts.isTemplateExpression(node)) return templateExpression(node, callee) ?? node.getText();
  if (ts.isStringLiteralLike(node)) {
    return isHan(node.text) ? literalExpression(node, callee) : node.getText();
  }
  if (ts.isConditionalExpression(node)) {
    return `${expressionSource(node.condition, callee)} ? ${expressionSource(node.whenTrue, callee)} : ${expressionSource(node.whenFalse, callee)}`;
  }
  if (ts.isParenthesizedExpression(node)) return `(${expressionSource(node.expression, callee)})`;
  if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.PlusToken) {
    return `${expressionSource(node.left, callee)} + ${expressionSource(node.right, callee)}`;
  }
  return node.getText();
}

function templateExpression(node, callee) {
  const quasis = node.templateSpans.map((span) => span.literal.text);
  const expressions = node.templateSpans.map((span) => expressionSource(span.expression, callee));
  let pattern = node.head.text;
  quasis.forEach((text, index) => {
    pattern += `{${index}}${text}`;
  });
  if (!isHan(pattern)) return null;
  return `${callee}(${JSON.stringify(normalize(pattern))}, [${expressions.join(", ")}])`;
}

function hasLocalTBinding(source) {
  let found = false;
  function visit(node) {
    if (found) return;
    if (ts.isParameter(node) && ts.isIdentifier(node.name) && node.name.text === "t") found = true;
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === "t") found = true;
    if (ts.isFunctionDeclaration(node) && node.name?.text === "t") found = true;
    if (ts.isBindingElement(node) && ts.isIdentifier(node.name) && node.name.text === "t") found = true;
    ts.forEachChild(node, visit);
  }
  visit(source);
  return found;
}

function collectForFile(file, text) {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.ES2022, true, ts.ScriptKind.TSX);
  const edits = [];
  const callee = hasLocalTBinding(source) ? "tr" : "t";

  function visit(node) {
    if (ts.isJsxText(node)) {
      const value = normalize(node.text);
      if (value && isHan(value)) {
        edits.push({ start: node.getStart(source), end: node.getEnd(), text: `{${callee}(${JSON.stringify(value)})}` });
      }
    } else if (ts.isJsxAttribute(node)) {
      const name = node.name.getText(source);
      const init = node.initializer;
      if (!SKIP_ATTRS.has(name) && init && ts.isStringLiteral(init) && isHan(init.text)) {
        edits.push({
          start: init.getStart(source),
          end: init.getEnd(),
          text: `{${callee}(${JSON.stringify(init.text)})}`,
        });
      }
    } else if (ts.isTemplateExpression(node)) {
      const replacement = templateExpression(node, callee);
      if (replacement) {
        edits.push({ start: node.getStart(source), end: node.getEnd(), text: replacement });
      }
    } else if (ts.isNoSubstitutionTemplateLiteral(node)) {
      if (!shouldSkipLiteral(node)) {
        edits.push({ start: node.getStart(source), end: node.getEnd(), text: literalExpression(node, callee) });
      }
    } else if (ts.isStringLiteral(node)) {
      if (!shouldSkipLiteral(node)) {
        edits.push({ start: node.getStart(source), end: node.getEnd(), text: literalExpression(node, callee) });
      }
    }
    ts.forEachChild(node, visit);
  }

  visit(source);
  if (!edits.length) return { text, changed: false, count: 0 };

  // Keep only outermost edits: a template replacement already covers any
  // nested literals, and applying overlapping ranges would corrupt offsets.
  edits.sort((a, b) => a.start - b.start || b.end - a.end);
  const outer = [];
  for (const edit of edits) {
    const last = outer[outer.length - 1];
    if (last && edit.start >= last.start && edit.end <= last.end) continue;
    outer.push(edit);
  }
  edits.length = 0;
  edits.push(...outer);

  // Insert the import after the last import declaration.
  const imports = source.statements.filter(ts.isImportDeclaration);
  const insertAt = imports.length ? imports[imports.length - 1].getEnd() : 0;
  const rel = relative(dirname(file), join(SRC, "lib", "i18n"))
    .split(sep)
    .join("/")
    .replace(/\.ts$/, "");
  const spec = rel.startsWith(".") ? rel : `./${rel}`;
  const importText =
    callee === "t"
      ? `\nimport { t } from ${JSON.stringify(spec)};\n`
      : `\nimport { t as tr } from ${JSON.stringify(spec)};\n`;
  edits.push({ start: insertAt, end: insertAt, text: importText });

  edits.sort((a, b) => b.start - a.start || b.end - a.end);
  let output = text;
  for (const edit of edits) {
    output = output.slice(0, edit.start) + edit.text + output.slice(edit.end);
  }
  return { text: output, changed: true, count: edits.length - 1 };
}

await walk(SRC);
let changedFiles = 0;
let total = 0;
for (const file of files) {
  const input = await readFile(file, "utf8");
  if (!isHan(input)) continue;
  const result = collectForFile(file, input);
  if (result.changed) {
    await writeFile(file, result.text, "utf8");
    changedFiles += 1;
    total += result.count;
    console.log(`${relative(ROOT, file)}: ${result.count}`);
  }
}
console.log(`wrapped ${total} strings in ${changedFiles} files`);
