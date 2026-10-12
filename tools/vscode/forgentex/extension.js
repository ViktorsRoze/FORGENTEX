"use strict";

const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

const { registerProjectFilesystem } = require("./projectFs");

const FGX_LANGUAGES = [
  { language: "forgentex-fgx", extension: ".fgx" },
  { language: "forgentex-fgai", extension: ".fgai" },
  { language: "forgentex-fgp", extension: ".fgp" },
  { language: "forgentex-fgmap", extension: ".fgmap" }
];

// Syntaxes that `fgx check` and `fgx fmt` accept.
const SOURCE_LANGUAGES = [
  { language: "forgentex-fgx" },
  { language: "forgentex-fgai" }
];

const SOURCE_LANGUAGE_IDS = new Set(SOURCE_LANGUAGES.map((entry) => entry.language));

// Completion vocabulary, mirroring the grammars. `test/grammar-coverage.js`
// checks the grammars against the parser; these lists follow the same surface.
const HUMAN_COMPLETIONS = {
  keywords: [
    "module", "import", "export", "pub", "struct", "enum", "trait", "impl",
    "requires", "default", "fn", "unsafe", "const", "type", "let", "return",
    "require", "if", "else", "match", "case", "for", "in", "while", "break",
    "continue", "effects", "share", "foreign", "version", "input", "output",
    "sandbox", "code", "bind", "pipeline", "stage", "edge", "via", "test",
    "expect", "expect_diagnostic", "dep", "env", "mock", "true", "false"
  ],
  types: [
    "bool", "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64",
    "usize", "f32", "f64", "str", "void", "Vec", "Map", "Option", "Result",
    "Path", "DirEntry", "String"
  ],
  effects: [
    "alloc", "cpu", "cpu.render", "fs.read", "fs.write", "io.read", "io.write",
    "media.encode", "network.read", "network.write", "crypto.hash",
    "crypto.sign", "concurrency.task", "concurrency.channel", "gpu.render"
  ],
  bindings: ["read", "rw", "move", "update"]
};

const AGENT_COMPLETIONS = {
  keywords: [
    "M", "IMPORT", "EXPORT", "PUB", "S", "EN", "TR", "IM", "F", "UF", "CONST",
    "TYPE", "LET", "SET", "R", "REQ", "IF", "EI", "EL", "MT", "FOR", "WH",
    "BR", "CN", "SHARE", "CAP", "ENDCAP", "IN", "OUT", "SBX", "CODE", "BIND",
    "PIPELINE", "ENDPIPELINE", "STAGE", "EDGE", "VIA", "TEST", "XTEST",
    "EXPECT", "ASM", "OP", "true", "false"
  ],
  types: HUMAN_COMPLETIONS.types,
  effects: HUMAN_COMPLETIONS.effects,
  bindings: HUMAN_COMPLETIONS.bindings
};

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

  // Formatting and completion apply to the two source syntaxes only; patch and
  // project-map files are not reformatted by `fgx fmt`.
  for (const selector of SOURCE_LANGUAGES) {
    context.subscriptions.push(
      vscode.languages.registerDocumentFormattingEditProvider(selector, new ForgentexFormattingProvider()),
      vscode.languages.registerCompletionItemProvider(selector, new ForgentexCompletionProvider())
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

  // The project filesystem view: sections, their provenance, and the note
  // graph, for files that carry data and bytes beside their code.
  registerProjectFilesystem(context);

  registerAutomaticDiagnostics(context, diagnostics);

  // The debug adapter shim spawns the CLI, so it needs the same binary the
  // rest of the extension uses rather than whatever happens to be on PATH.
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("forgentex", {
      createDebugAdapterDescriptor() {
        const adapter = path.join(__dirname, "adapter.js");
        return new vscode.DebugAdapterExecutable(process.execPath, [adapter], {
          env: Object.assign({}, process.env, {
            FORGENTEX_FGX_PATH: config().get("fgxPath") || "fgx"
          })
        });
      }
    })
  );
}

// Keep checked files current without a language server: check on open and on
// save always, and while typing when `checkOnType` is enabled. Typing is
// debounced and superseded so a burst of keystrokes runs one check.
function registerAutomaticDiagnostics(context, collection) {
  const timers = new Map();

  const schedule = (document, delay) => {
    if (!isSourceDocument(document)) {
      return;
    }
    const key = document.uri.toString();
    const pending = timers.get(key);
    if (pending) {
      clearTimeout(pending);
    }
    timers.set(key, setTimeout(() => {
      timers.delete(key);
      checkDocument(document, collection).catch(() => {});
    }, delay));
  };

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((document) => schedule(document, 0)),
    vscode.workspace.onDidSaveTextDocument((document) => schedule(document, 0)),
    vscode.workspace.onDidChangeTextDocument((event) => {
      if (config().get("checkOnType")) {
        schedule(event.document, Math.max(200, Number(config().get("checkDelayMs")) || 400));
      }
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      const key = document.uri.toString();
      const pending = timers.get(key);
      if (pending) {
        clearTimeout(pending);
        timers.delete(key);
      }
      collection.delete(document.uri);
    }),
    { dispose: () => {
      for (const timer of timers.values()) {
        clearTimeout(timer);
      }
      timers.clear();
    } }
  );

  for (const document of vscode.workspace.textDocuments) {
    schedule(document, 0);
  }
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
  const count = await checkDocument(document, collection, { quiet: false });
  if (count !== null) {
    vscode.window.setStatusBarMessage(`Forgentex diagnostics: ${count}`, 4000);
  }
}

function isSourceDocument(document) {
  return Boolean(document) && SOURCE_LANGUAGE_IDS.has(document.languageId);
}

// Check one document and publish its diagnostics. Returns the diagnostic count,
// or null when the check could not run.
//
// `fgx check` reads a path, so an unsaved or dirty buffer is mirrored into a
// scratch file first; otherwise the editor would report diagnostics for stale
// on-disk text while the user types.
async function checkDocument(document, collection, options) {
  const quiet = !options || options.quiet !== false;
  if (!isSourceDocument(document) || config().get("languageServiceMode") === "disabled") {
    return null;
  }

  const onDisk = !document.isUntitled && !document.isDirty;
  const target = onDisk ? document.uri.fsPath : mirrorToScratch(document);
  if (!target) {
    return null;
  }

  try {
    const cwd = workspaceRootFor(document.isUntitled ? undefined : document.uri.fsPath);
    const output = await runFgx(["check", target, "--json"], cwd, { quiet });
    const parsed = parseJsonOrFallback(output);
    const diagnostics = (parsed.diagnostics || []).map((item) => {
      // `forgentex.diagnostics.v1` names the code `error` and carries no
      // severity field; everything it reports blocks the checker.
      const code = item.error || item.code;
      const severity = item.severity === "warning"
        ? vscode.DiagnosticSeverity.Warning
        : vscode.DiagnosticSeverity.Error;
      let message = item.message || code || "Forgentex diagnostic";
      if (item.expected !== undefined || item.found !== undefined) {
        message += ` (expected ${item.expected}, found ${item.found})`;
      }
      const diagnostic = new vscode.Diagnostic(
        rangeForDiagnostic(document, item),
        message,
        severity
      );
      diagnostic.code = code;
      diagnostic.source = "fgx";
      return diagnostic;
    });
    collection.set(document.uri, diagnostics);
    return diagnostics.length;
  } catch (error) {
    // A failed check must not leave stale diagnostics implying the file is
    // still clean, or still broken, when neither is known.
    collection.delete(document.uri);
    return null;
  } finally {
    if (!onDisk && target) {
      removeScratch(target);
    }
  }
}

// Remove a scratch mirror synchronously. Fire-and-forget deletion can outlive
// the run that created it and leave the temp directory accumulating files.
function removeScratch(target) {
  try {
    fs.rmSync(target, { force: true });
  } catch (error) {
    // A scratch file that cannot be removed must not fail the surrounding
    // editor operation.
  }
}

// Mirror a dirty buffer into a scratch file that keeps the original extension,
// because `fgx` selects its syntax from the file extension.
function mirrorToScratch(document) {
  const suffix = document.languageId === "forgentex-fgai" ? ".fgai" : ".fgx";
  const name = `forgentex-check-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}${suffix}`;
  const target = path.join(require("os").tmpdir(), name);
  try {
    fs.writeFileSync(target, document.getText(), "utf8");
    return target;
  } catch (error) {
    return null;
  }
}

class ForgentexFormattingProvider {
  async provideDocumentFormattingEdits(document) {
    if (config().get("languageServiceMode") === "disabled") {
      return [];
    }
    const onDisk = !document.isUntitled && !document.isDirty;
    const target = onDisk ? document.uri.fsPath : mirrorToScratch(document);
    if (!target) {
      return [];
    }
    const to = document.languageId === "forgentex-fgai" ? "fgai" : "fgx";
    try {
      const formatted = await runFgx(
        ["fmt", target, "--to", to],
        workspaceRootFor(document.isUntitled ? undefined : document.uri.fsPath),
        { quiet: false }
      );
      if (!formatted || formatted === document.getText()) {
        return [];
      }
      const full = new vscode.Range(
        document.positionAt(0),
        document.positionAt(document.getText().length)
      );
      return [vscode.TextEdit.replace(full, formatted)];
    } catch (error) {
      return [];
    } finally {
      if (!onDisk && target) {
        removeScratch(target);
      }
    }
  }
}

class ForgentexCompletionProvider {
  provideCompletionItems(document, position) {
    const vocabulary = document.languageId === "forgentex-fgai"
      ? AGENT_COMPLETIONS
      : HUMAN_COMPLETIONS;
    const line = document.lineAt(position.line).text.slice(0, position.character);

    // Inside an `effects [...]` list only effect names are meaningful, and
    // after a `bind <name>` only the binding modes are.
    if (/effects\s*\[[^\]]*$/.test(line)) {
      return vocabulary.effects.map((name) =>
        completion(name, vscode.CompletionItemKind.Constant, "effect"));
    }
    if (/^\s*(?:bind|BIND)\s+[A-Za-z_][A-Za-z0-9_]*(?:@\d+)?\s+\w*$/.test(line)) {
      return vocabulary.bindings.map((name) =>
        completion(name, vscode.CompletionItemKind.EnumMember, "binding mode"));
    }

    return [
      ...vocabulary.keywords.map((name) =>
        completion(name, vscode.CompletionItemKind.Keyword, "keyword")),
      ...vocabulary.types.map((name) =>
        completion(name, vscode.CompletionItemKind.TypeParameter, "type")),
      ...vocabulary.effects.map((name) =>
        completion(name, vscode.CompletionItemKind.Constant, "effect"))
    ];
  }
}

function completion(label, kind, detail) {
  const item = new vscode.CompletionItem(label, kind);
  item.detail = `Forgentex ${detail}`;
  return item;
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

// `quiet` suppresses the error popup for background work such as check-on-type,
// where a missing binary or a failing check would otherwise raise a modal on
// every keystroke. Interactive commands leave it off and surface the message.
function runFgx(args, cwd, options) {
  if (config().get("languageServiceMode") === "disabled") {
    return Promise.reject(new Error("Forgentex CLI language service is disabled."));
  }
  const command = config().get("fgxPath") || "fgx";
  const quiet = Boolean(options && options.quiet);
  return new Promise((resolve, reject) => {
    cp.execFile(command, args, { cwd, maxBuffer: 32 * 1024 * 1024 }, (error, stdout, stderr) => {
      // `fgx check` exits non-zero when a file has diagnostics, and still
      // prints the JSON report. Treat parseable stdout as success so real
      // diagnostics are published instead of reported as a failed command.
      if (error) {
        if (stdout && stdout.trim().startsWith("{")) {
          resolve(stdout);
          return;
        }
        reject(new Error(stderr || error.message));
        return;
      }
      resolve(stdout);
    });
  }).catch((error) => {
    if (!quiet) {
      vscode.window.showErrorMessage(`Forgentex command failed: ${error.message}`);
    }
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
  deactivate,
  // Internals exercised by test/extension-smoke.js, which drives the extension
  // against the real fgx binary with a stubbed vscode module.
  __test: {
    checkDocument,
    mirrorToScratch,
    isSourceDocument,
    ForgentexFormattingProvider,
    ForgentexCompletionProvider,
    ForgentexSymbolProvider
  }
};
