"use strict";

const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

const FGX_LANGUAGES = [
  { language: "forgentex-fgx", extension: ".fgx" },
  { language: "forgentex-fgai", extension: ".fgai" },
  { language: "forgentex-fgp", extension: ".fgp" },
  { language: "forgentex-fgmap", extension: ".fgmap" }
];

const DECLARATION_PATTERNS = [
  {
    regex: /^\s*(?:pub\s+)?(unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)/,
    kind: vscode.SymbolKind.Function,
    prefix: "@function."
  },
  {
    regex: /^\s*struct\s+([A-Za-z_][A-Za-z0-9_]*)/,
    kind: vscode.SymbolKind.Struct,
    prefix: "@struct."
  },
  {
    regex: /^\s*(?:UF|F)\s+([A-Za-z_][A-Za-z0-9_]*)/,
    kind: vscode.SymbolKind.Function,
    prefix: "@function."
  },
  {
    regex: /^\s*S\s+([A-Za-z_][A-Za-z0-9_]*)/,
    kind: vscode.SymbolKind.Struct,
    prefix: "@struct."
  },
  {
    regex: /^\s*(?:function|struct):\s*([@A-Za-z0-9_.]+)/,
    kind: vscode.SymbolKind.Object,
    prefix: ""
  }
];

function activate(context) {
  const diagnostics = vscode.languages.createDiagnosticCollection("forgentex");
  context.subscriptions.push(diagnostics);

  for (const selector of FGX_LANGUAGES) {
    context.subscriptions.push(
      vscode.languages.registerDocumentSymbolProvider(selector, new ForgentexSymbolProvider()),
      vscode.languages.registerHoverProvider(selector, new ForgentexHoverProvider())
    );
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("forgentex.showHumanAgentViews", showHumanAgentViews),
    vscode.commands.registerCommand("forgentex.contextForSymbol", contextForSymbol),
    vscode.commands.registerCommand("forgentex.reviewPatch", (uri) => reviewPatch(context, uri)),
    vscode.commands.registerCommand("forgentex.applyPatchPreview", applyPatchPreview),
    vscode.commands.registerCommand("forgentex.runDiagnostics", () => runDiagnostics(diagnostics)),
    vscode.commands.registerCommand("forgentex.showUnsafeArchitectureSlices", showUnsafeArchitectureSlices)
  );
}

function deactivate() {}

class ForgentexSymbolProvider {
  provideDocumentSymbols(document) {
    const symbols = [];
    for (let index = 0; index < document.lineCount; index++) {
      const line = document.lineAt(index);
      const found = declarationForLine(line.text);
      if (!found) {
        continue;
      }
      const range = new vscode.Range(index, line.firstNonWhitespaceCharacterIndex, index, line.text.length);
      const symbol = new vscode.DocumentSymbol(found.name, found.nodeId, found.kind, range, range);
      symbols.push(symbol);
    }
    return symbols;
  }
}

class ForgentexHoverProvider {
  provideHover(document, position) {
    const line = document.lineAt(position.line);
    const found = declarationForLine(line.text);
    if (found) {
      return new vscode.Hover(`Stable node ID: \`${found.nodeId}\``);
    }

    const wordRange = document.getWordRangeAtPosition(position, /@?[A-Za-z_][A-Za-z0-9_.]*/);
    if (!wordRange) {
      return undefined;
    }
    const word = document.getText(wordRange);
    if (word.startsWith("@function.") || word.startsWith("@struct.")) {
      return new vscode.Hover(`Forgentex semantic target: \`${word}\``);
    }
    return undefined;
  }
}

async function showHumanAgentViews() {
  const document = activeDocument();
  if (!document) {
    return;
  }
  const source = document.uri.fsPath;
  const parsed = path.parse(source);
  const counterpartExtension = parsed.ext === ".fgai" ? ".fgx" : ".fgai";
  const counterpart = path.join(parsed.dir, `${parsed.name}${counterpartExtension}`);

  await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
  if (fs.existsSync(counterpart)) {
    const other = await vscode.workspace.openTextDocument(counterpart);
    await vscode.window.showTextDocument(other, vscode.ViewColumn.Two);
    return;
  }

  const targetMode = counterpartExtension === ".fgai" ? "fgai" : "fgx";
  const content = await runFgx(["fmt", source, "--to", targetMode], workspaceRootFor(source));
  const generated = await vscode.workspace.openTextDocument({
    content,
    language: targetMode === "fgai" ? "forgentex-fgai" : "forgentex-fgx"
  });
  await vscode.window.showTextDocument(generated, vscode.ViewColumn.Two);
}

async function contextForSymbol() {
  const suggested = symbolAtCursor() || "@function.mix_samples";
  const symbol = await vscode.window.showInputBox({
    prompt: "Forgentex symbol node ID",
    value: suggested
  });
  if (!symbol) {
    return;
  }
  const deps = config().get("defaultDeps") || "types,effects,tests";
  const output = await runFgx(["context", symbol, "--deps", deps, "--json"], workspaceRootForActive());
  await openOutputDocument(output, "json", `Forgentex context: ${symbol}`);
}

async function reviewPatch(context, uri) {
  const patch = await resolvePatchPath(uri);
  if (!patch) {
    return;
  }
  const args = ["review", patch, "--json"];
  const source = activeSourcePath();
  if (source) {
    args.push("--source", source);
  }
  const output = await runFgx(args, workspaceRootFor(patch));
  const parsed = parseJsonOrFallback(output);
  const panel = vscode.window.createWebviewPanel(
    "forgentexPatchReview",
    `Forgentex Patch Review: ${path.basename(patch)}`,
    vscode.ViewColumn.Beside,
    { enableScripts: true }
  );
  panel.webview.html = renderReviewHtml(parsed, output, source, patch);
  panel.webview.onDidReceiveMessage(async (message) => {
    if (message.command === "approvePreview") {
      await applyPatchPreview(vscode.Uri.file(patch), source);
    }
  }, undefined, context.subscriptions);
}

async function applyPatchPreview(uri, explicitSource) {
  const patch = await resolvePatchPath(uri);
  if (!patch) {
    return;
  }
  const source = explicitSource || activeSourcePath() || await pickSourceFile();
  if (!source) {
    return;
  }
  const output = await runFgx(["patch", source, patch, "--review"], workspaceRootFor(source));
  await openOutputDocument(output, "forgentex-fgx", `Forgentex patch preview: ${path.basename(patch)}`);
}

async function runDiagnostics(collection) {
  const document = activeDocument();
  if (!document) {
    return;
  }
  const source = document.uri.fsPath;
  const output = await runFgx(["check", source, "--json"], workspaceRootFor(source));
  const parsed = parseJsonOrFallback(output);
  const diagnostics = (parsed.diagnostics || []).map((item) => {
    const range = rangeForDiagnostic(document, item);
    const severity = item.severity === "warning"
      ? vscode.DiagnosticSeverity.Warning
      : vscode.DiagnosticSeverity.Error;
    const diagnostic = new vscode.Diagnostic(range, item.message || item.code || "Forgentex diagnostic", severity);
    diagnostic.code = item.code;
    diagnostic.source = "fgx";
    return diagnostic;
  });
  collection.set(document.uri, diagnostics);
  vscode.window.setStatusBarMessage(`Forgentex diagnostics: ${diagnostics.length}`, 4000);
}

async function showUnsafeArchitectureSlices() {
  const files = await vscode.workspace.findFiles("**/*.{fgx,fgai,fgmap}", "**/target/**");
  const entries = [];
  for (const uri of files) {
    const text = fs.readFileSync(uri.fsPath, "utf8");
    const lines = text.split(/\r?\n/);
    lines.forEach((line, index) => {
      if (isArchitectureLine(line)) {
        entries.push({
          label: `${path.basename(uri.fsPath)}:${index + 1}`,
          description: architectureTags(line).join(", "),
          detail: line.trim(),
          uri,
          line: index
        });
      }
    });
  }
  if (entries.length === 0) {
    vscode.window.showInformationMessage("No Forgentex unsafe or architecture-specific slices found.");
    return;
  }
  const picked = await vscode.window.showQuickPick(entries, {
    placeHolder: "Unsafe, assembly, ABI, x86_64, ARM/AArch64, and RISC-V slices"
  });
  if (!picked) {
    return;
  }
  const document = await vscode.workspace.openTextDocument(picked.uri);
  await vscode.window.showTextDocument(document, {
    selection: new vscode.Range(picked.line, 0, picked.line, 0)
  });
}

function declarationForLine(text) {
  for (const pattern of DECLARATION_PATTERNS) {
    const match = text.match(pattern.regex);
    if (!match) {
      continue;
    }
    const name = match[2] || match[1];
    const nodeId = name.startsWith("@") ? name : `${pattern.prefix}${name}`;
    return { name, nodeId, kind: pattern.kind };
  }
  return undefined;
}

function isArchitectureLine(line) {
  return /\b(?:unsafe|asm|ASM|ABI|x86_64|aarch64|arm64|armv7|riscv32|riscv64|risc-v|cpu\.|target\.)\b/i.test(line);
}

function architectureTags(line) {
  const tags = [];
  const checks = [
    ["unsafe", /\bunsafe\b/i],
    ["assembly", /\b(?:asm|ASM)\b/],
    ["abi", /\bABI\b/],
    ["x86_64", /\bx86_64\b/i],
    ["arm/aarch64", /\b(?:arm64|armv7|aarch64)\b/i],
    ["risc-v", /\b(?:riscv32|riscv64|risc-v)\b/i],
    ["effect", /\b(?:cpu|target)\.[A-Za-z0-9_]+\b/]
  ];
  for (const [tag, regex] of checks) {
    if (regex.test(line)) {
      tags.push(tag);
    }
  }
  return tags.length === 0 ? ["architecture"] : tags;
}

function rangeForDiagnostic(document, item) {
  const node = item.node_id || item.node || item.target || "";
  const symbol = node.replace(/^@(function|struct)\./, "");
  if (symbol) {
    for (let index = 0; index < document.lineCount; index++) {
      const line = document.lineAt(index);
      if (line.text.includes(symbol)) {
        return line.range;
      }
    }
  }
  return new vscode.Range(0, 0, 0, document.lineAt(0).text.length);
}

function renderReviewHtml(parsed, rawOutput, source, patch) {
  const escapedRaw = escapeHtml(rawOutput);
  const review = parsed.patch_review || parsed.review || parsed;
  const effectChanges = asList(review.effect_changes || review.effects || []);
  const abiChanges = asList(review.abi_changes || review.abi || []);
  const unsafeChanges = asList(review.unsafe || review.unsafe_changes || []);
  const summary = review.summary || parsed.summary || "Forgentex semantic patch review";
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>
    body { font-family: var(--vscode-font-family); padding: 20px; color: var(--vscode-editor-foreground); }
    h1 { font-size: 20px; margin: 0 0 12px; }
    h2 { font-size: 14px; margin-top: 20px; text-transform: uppercase; letter-spacing: 0; }
    code, pre { font-family: var(--vscode-editor-font-family); }
    pre { background: var(--vscode-textCodeBlock-background); padding: 12px; overflow: auto; }
    .meta { color: var(--vscode-descriptionForeground); margin-bottom: 16px; }
    .badge { display: inline-block; border: 1px solid var(--vscode-contrastBorder); padding: 2px 6px; margin: 2px 4px 2px 0; border-radius: 4px; }
    .danger { color: var(--vscode-testing-iconFailed); }
    button { color: var(--vscode-button-foreground); background: var(--vscode-button-background); border: 0; padding: 6px 10px; cursor: pointer; }
  </style>
</head>
<body>
  <h1>Forgentex Patch Review</h1>
  <div class="meta">Patch: <code>${escapeHtml(patch || "")}</code><br>Source: <code>${escapeHtml(source || "active source not supplied")}</code></div>
  <p>${escapeHtml(summary)}</p>
  <h2>Effect Changes</h2>
  ${renderBadges(effectChanges)}
  <h2>ABI Changes</h2>
  ${renderBadges(abiChanges)}
  <h2 class="danger">Unsafe / Assembly</h2>
  ${renderBadges(unsafeChanges)}
  <h2>Raw Review JSON</h2>
  <pre>${escapedRaw}</pre>
  <button id="approve">Approve Patch Preview</button>
  <script>
    const vscode = acquireVsCodeApi();
    document.getElementById("approve").addEventListener("click", () => vscode.postMessage({ command: "approvePreview" }));
  </script>
</body>
</html>`;
}

function renderBadges(values) {
  if (values.length === 0) {
    return "<p>No changes reported.</p>";
  }
  return `<p>${values.map((value) => `<span class="badge">${escapeHtml(String(value))}</span>`).join("")}</p>`;
}

function asList(value) {
  if (Array.isArray(value)) {
    return value;
  }
  if (value && typeof value === "object") {
    return Object.entries(value).map(([key, entry]) => `${key}: ${JSON.stringify(entry)}`);
  }
  if (value) {
    return [value];
  }
  return [];
}

function activeDocument() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.isUntitled) {
    vscode.window.showWarningMessage("Open a Forgentex file first.");
    return undefined;
  }
  return editor.document;
}

function activeSourcePath() {
  const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  if (!document || document.isUntitled) {
    return undefined;
  }
  if ([".fgx", ".fgai"].includes(path.extname(document.uri.fsPath))) {
    return document.uri.fsPath;
  }
  return undefined;
}

async function resolvePatchPath(uri) {
  if (uri && uri.fsPath && path.extname(uri.fsPath) === ".fgp") {
    return uri.fsPath;
  }
  const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  if (document && !document.isUntitled && path.extname(document.uri.fsPath) === ".fgp") {
    return document.uri.fsPath;
  }
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    filters: { "Forgentex patches": ["fgp"] }
  });
  return picked && picked[0] && picked[0].fsPath;
}

async function pickSourceFile() {
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    filters: { "Forgentex sources": ["fgx", "fgai"] }
  });
  return picked && picked[0] && picked[0].fsPath;
}

function symbolAtCursor() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return undefined;
  }
  const range = editor.document.getWordRangeAtPosition(editor.selection.active, /@?[A-Za-z_][A-Za-z0-9_.]*/);
  if (!range) {
    return undefined;
  }
  const word = editor.document.getText(range);
  if (word.startsWith("@function.") || word.startsWith("@struct.")) {
    return word;
  }
  const line = editor.document.lineAt(editor.selection.active.line);
  const declaration = declarationForLine(line.text);
  return declaration && declaration.nodeId;
}

function config() {
  return vscode.workspace.getConfiguration("forgentex");
}

function workspaceRootForActive() {
  const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  return workspaceRootFor(document && !document.isUntitled ? document.uri.fsPath : undefined);
}

function workspaceRootFor(file) {
  if (vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0) {
    return vscode.workspace.workspaceFolders[0].uri.fsPath;
  }
  return file ? path.dirname(file) : process.cwd();
}

function runFgx(args, cwd) {
  if (config().get("languageServiceMode") === "disabled") {
    return Promise.reject(new Error("Forgentex CLI language service is disabled."));
  }
  const command = config().get("fgxPath") || "fgx";
  return new Promise((resolve, reject) => {
    cp.execFile(command, args, { cwd }, (error, stdout, stderr) => {
      if (error) {
        reject(new Error(stderr || error.message));
        return;
      }
      resolve(stdout);
    });
  }).catch((error) => {
    vscode.window.showErrorMessage(`Forgentex command failed: ${error.message}`);
    throw error;
  });
}

async function openOutputDocument(content, language, title) {
  const document = await vscode.workspace.openTextDocument({ content, language });
  await vscode.window.showTextDocument(document, vscode.ViewColumn.Beside);
  vscode.window.setStatusBarMessage(title, 4000);
}

function parseJsonOrFallback(value) {
  try {
    return JSON.parse(value);
  } catch (_error) {
    return { summary: "Raw Forgentex output", raw: value };
  }
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

module.exports = {
  activate,
  deactivate
};
