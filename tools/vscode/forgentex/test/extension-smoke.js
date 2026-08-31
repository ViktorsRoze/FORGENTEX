#!/usr/bin/env node
// Behavioural smoke test for the Forgentex VS Code extension.
//
// VS Code is not available here, so the `vscode` module is stubbed and the
// extension is driven directly. The `fgx` binary is real: diagnostics,
// formatting, and completion are exercised end to end against it.
//
// Run: node test/extension-smoke.js [path-to-fgx]

const Module = require("module");
const path = require("path");
const fs = require("fs");
const os = require("os");

const root = path.join(__dirname, "..");
const repoRoot = path.join(root, "..", "..", "..");
const fgxPath = process.argv[2]
  || path.join(repoRoot, "target", "release", process.platform === "win32" ? "fgx.exe" : "fgx");

let failures = 0;
function check(condition, label) {
  if (condition) {
    console.log(`ok ${label}`);
  } else {
    failures += 1;
    console.error(`FAIL ${label}`);
  }
}

// ---------------------------------------------------------------- vscode stub
const registered = {
  commands: [],
  symbolProviders: 0,
  hoverProviders: 0,
  formattingProviders: 0,
  completionProviders: 0,
  debugAdapters: [],
  listeners: {}
};
const published = new Map();

class Position {
  constructor(line, character) {
    this.line = line;
    this.character = character;
  }
}
class Range {
  constructor(a, b, c, d) {
    if (typeof a === "number") {
      this.start = new Position(a, b);
      this.end = new Position(c, d);
    } else {
      this.start = a;
      this.end = b;
    }
  }
}
class Diagnostic {
  constructor(range, message, severity) {
    this.range = range;
    this.message = message;
    this.severity = severity;
  }
}
class CompletionItem {
  constructor(label, kind) {
    this.label = label;
    this.kind = kind;
  }
}
class TextEdit {
  constructor(range, newText) {
    this.range = range;
    this.newText = newText;
  }
  static replace(range, newText) {
    return new TextEdit(range, newText);
  }
}

let settings = {
  fgxPath,
  defaultDeps: "types,effects,tests",
  languageServiceMode: "cli",
  checkOnType: true,
  checkDelayMs: 400
};

const vscodeStub = {
  Position,
  Range,
  Diagnostic,
  CompletionItem,
  TextEdit,
  DiagnosticSeverity: { Error: 0, Warning: 1 },
  CompletionItemKind: { Keyword: 0, TypeParameter: 1, Constant: 2, EnumMember: 3 },
  SymbolKind: { Function: 0, Struct: 1, Object: 2 },
  Uri: { file: (p) => ({ fsPath: p, toString: () => `file://${p}` }) },
  languages: {
    createDiagnosticCollection: () => ({
      set: (uri, items) => published.set(uri.toString(), items),
      delete: (uri) => published.delete(uri.toString()),
      dispose: () => {}
    }),
    registerDocumentSymbolProvider: () => { registered.symbolProviders += 1; return { dispose: () => {} }; },
    registerHoverProvider: () => { registered.hoverProviders += 1; return { dispose: () => {} }; },
    registerDocumentFormattingEditProvider: () => { registered.formattingProviders += 1; return { dispose: () => {} }; },
    registerCompletionItemProvider: () => { registered.completionProviders += 1; return { dispose: () => {} }; }
  },
  commands: {
    registerCommand: (name) => { registered.commands.push(name); return { dispose: () => {} }; }
  },
  debug: {
    registerDebugAdapterDescriptorFactory: (type, factory) => {
      registered.debugAdapters.push({ type, factory });
      return { dispose: () => {} };
    }
  },
  DebugAdapterExecutable: class {
    constructor(command, args, options) {
      this.command = command;
      this.args = args;
      this.options = options;
    }
  },
  workspace: {
    workspaceFolders: [{ uri: { fsPath: repoRoot } }],
    textDocuments: [],
    getConfiguration: () => ({ get: (key) => settings[key] }),
    onDidOpenTextDocument: (fn) => { registered.listeners.open = fn; return { dispose: () => {} }; },
    onDidSaveTextDocument: (fn) => { registered.listeners.save = fn; return { dispose: () => {} }; },
    onDidChangeTextDocument: (fn) => { registered.listeners.change = fn; return { dispose: () => {} }; },
    onDidCloseTextDocument: (fn) => { registered.listeners.close = fn; return { dispose: () => {} }; },
    openTextDocument: async (opts) => opts,
    findFiles: async () => []
  },
  window: {
    activeTextEditor: undefined,
    setStatusBarMessage: () => {},
    showErrorMessage: (message) => { console.error(`  (popup) ${message}`); },
    showInformationMessage: () => {},
    showTextDocument: async () => {},
    createWebviewPanel: () => ({ webview: { html: "" }, dispose: () => {} })
  },
  ViewColumn: { One: 1, Two: 2, Beside: 3 }
};

const originalResolve = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === "vscode") {
    return "vscode";
  }
  return originalResolve.call(this, request, ...rest);
};
require.cache.vscode = { id: "vscode", filename: "vscode", loaded: true, exports: vscodeStub };

const extension = require(path.join(root, "extension.js"));

// ------------------------------------------------------------------ documents
function makeDocument(text, languageId, filePath) {
  const lines = text.split("\n");
  return {
    languageId,
    isUntitled: !filePath,
    isDirty: !filePath,
    uri: vscodeStub.Uri.file(filePath || `/virtual/buffer.${languageId === "forgentex-fgai" ? "fgai" : "fgx"}`),
    lineCount: lines.length,
    getText: () => text,
    lineAt: (i) => ({ text: lines[i], range: new Range(i, 0, i, lines[i].length) }),
    positionAt: (offset) => {
      const before = text.slice(0, offset).split("\n");
      return new Position(before.length - 1, before[before.length - 1].length);
    }
  };
}

async function main() {
  if (!fs.existsSync(fgxPath)) {
    console.error(`fgx binary not found at ${fgxPath}; build it with: cargo build --release --bin fgx`);
    process.exit(2);
  }

  // 1. activate() must wire every provider and command.
  const context = { subscriptions: [] };
  extension.activate(context);
  check(registered.commands.length === 6, `activate registers 6 commands (got ${registered.commands.length})`);
  check(registered.symbolProviders === 4, `symbol providers for 4 languages (got ${registered.symbolProviders})`);
  check(registered.hoverProviders === 4, `hover providers for 4 languages (got ${registered.hoverProviders})`);
  check(registered.formattingProviders === 2, `formatting providers for 2 source languages (got ${registered.formattingProviders})`);
  check(registered.completionProviders === 2, `completion providers for 2 source languages (got ${registered.completionProviders})`);
  check(
    ["open", "save", "change", "close"].every((k) => typeof registered.listeners[k] === "function"),
    "automatic diagnostics subscribe to open/save/change/close"
  );

  // The debug adapter shim must run the same binary the extension checks with,
  // not whatever `fgx` happens to be on PATH.
  check(
    registered.debugAdapters.length === 1 && registered.debugAdapters[0].type === "forgentex",
    "a forgentex debug adapter factory is registered"
  );
  const descriptor = registered.debugAdapters[0].factory.createDebugAdapterDescriptor();
  check(
    descriptor.args.length === 1 && descriptor.args[0].endsWith("adapter.js"),
    `adapter descriptor runs adapter.js (got ${descriptor.args})`
  );
  check(
    descriptor.options.env.FORGENTEX_FGX_PATH === fgxPath,
    "adapter inherits the configured fgx path"
  );

  // 2. A dirty buffer with a type error must produce a diagnostic carrying the
  //    real code. `forgentex.diagnostics.v1` names that field `error`, so this
  //    fails if the extension reads `code` instead.
  const collection = {
    set: (uri, items) => published.set(uri.toString(), items),
    delete: (uri) => published.delete(uri.toString())
  };
  const broken = makeDocument('fn bad() -> i32:\n    return "x"\n', "forgentex-fgx", null);
  const count = await extension.__test.checkDocument(broken, collection, { quiet: true });
  const items = published.get(broken.uri.toString()) || [];
  check(count === 1, `dirty buffer yields 1 diagnostic (got ${count})`);
  check(items.length === 1 && items[0].code === "return_type_mismatch",
    `diagnostic code is return_type_mismatch (got ${items[0] && items[0].code})`);
  check(items.length === 1 && /expected i32, found str/.test(items[0].message),
    `diagnostic message carries expected/found (got ${items[0] && items[0].message})`);

  // 3. A clean on-disk file must publish an empty list, not a stale one.
  const cleanPath = path.join(repoRoot, "examples", "public_0_1", "portable_core.fgx");
  const clean = makeDocument(fs.readFileSync(cleanPath, "utf8"), "forgentex-fgx", cleanPath);
  const cleanCount = await extension.__test.checkDocument(clean, collection, { quiet: true });
  check(cleanCount === 0, `clean file yields 0 diagnostics (got ${cleanCount})`);

  // 4. Non-source documents must be ignored rather than sent to `fgx check`.
  const patch = makeDocument("OP REPLACE\n", "forgentex-fgp", null);
  check(await extension.__test.checkDocument(patch, collection, { quiet: true }) === null,
    "patch documents are not checked");

  // 5. Formatting must round-trip real source through `fgx fmt`.
  const formatter = new extension.__test.ForgentexFormattingProvider();
  const messy = makeDocument("fn  hello()  -> i32:\n    return   42\n", "forgentex-fgx", null);
  const edits = await formatter.provideDocumentFormattingEdits(messy);
  check(Array.isArray(edits) && edits.length === 1 && /fn hello\(\) -> i32/.test(edits[0].newText),
    "formatting normalises spacing via fgx fmt");

  // 6. Completion must be context sensitive.
  const completer = new extension.__test.ForgentexCompletionProvider();
  const effectsDoc = makeDocument("fn f() -> i32 effects [", "forgentex-fgx", null);
  const inEffects = completer.provideCompletionItems(effectsDoc, new Position(0, 23));
  check(inEffects.every((i) => i.detail === "Forgentex effect") && inEffects.some((i) => i.label === "fs.read"),
    "completion inside effects[...] offers only effects");

  const bindDoc = makeDocument("    bind samples ", "forgentex-fgx", null);
  const inBind = completer.provideCompletionItems(bindDoc, new Position(0, 17));
  check(inBind.length === 4 && inBind.map((i) => i.label).join(",") === "read,rw,move,update",
    `completion after bind offers the 4 modes (got ${inBind.map((i) => i.label).join(",")})`);

  const general = completer.provideCompletionItems(makeDocument("fo", "forgentex-fgx", null), new Position(0, 2));
  check(general.some((i) => i.label === "foreign") && general.some((i) => i.label === "Vec"),
    "general completion offers keywords and types");

  const agent = completer.provideCompletionItems(makeDocument("W", "forgentex-fgai", null), new Position(0, 1));
  check(agent.some((i) => i.label === "WH") && !agent.some((i) => i.label === "while"),
    "agent completion offers dense tokens, not human keywords");

  // 7. Disabling the language service must stop all CLI work.
  settings = { ...settings, languageServiceMode: "disabled" };
  check(await extension.__test.checkDocument(broken, collection, { quiet: true }) === null,
    "languageServiceMode=disabled suppresses checking");
  settings = { ...settings, languageServiceMode: "cli" };

  // 8. No scratch mirror files may survive a run.
  const leaked = fs.readdirSync(os.tmpdir()).filter((f) => f.startsWith("forgentex-check-"));
  check(leaked.length === 0, `no scratch files leaked (found ${leaked.length})`);

  if (failures > 0) {
    console.error(`\n${failures} check(s) failed`);
    process.exit(1);
  }
  console.log("\nall extension smoke checks passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
