"use strict";

// The project filesystem, shown as one.
//
// A Forgentex file can carry its data, its bytes, and a graph of remarks beside
// its code. Read as plain text that is a lot to hold in your head, so this
// renders it the way the file is actually organised: a tree of sections, each
// with its provenance, and every remark hanging under the thing it is about.
//
// Everything here reads through the `fgx` CLI rather than parsing the file in
// JavaScript. A second parser would be a second answer to "what is in this
// file", and the two would drift.

const cp = require("child_process");
const path = require("path");
const vscode = require("vscode");

const SECTION_HEADER =
  /^(?<indent>\s*)(?<kind>data|blob|note)\s+(?<name>[A-Za-z_][A-Za-z0-9_]*)\b/;

/// Section kinds, and how each should look in the tree.
const KIND_ICONS = {
  data: "table",
  blob: "file-binary",
  note: "comment",
};

function config() {
  return vscode.workspace.getConfiguration("forgentex");
}

function fgxPath() {
  return config().get("fgxPath") || "fgx";
}

/// Run the CLI and return stdout as text, or `null` when the call failed.
///
/// A file that does not parse is an ordinary state while someone is typing, so
/// a failure here empties the view rather than raising.
function readJson(args, cwd) {
  return new Promise((resolve) => {
    cp.execFile(
      fgxPath(),
      args,
      { cwd, maxBuffer: 64 * 1024 * 1024 },
      (error, stdout) => {
        if (error) {
          resolve(null);
          return;
        }
        try {
          resolve(JSON.parse(stdout));
        } catch (parseError) {
          resolve(null);
        }
      }
    );
  });
}

/// Read one payload as raw bytes. Blob payloads are binary, so stdout is taken
/// as a buffer rather than decoded as text.
function readBytes(args, cwd) {
  return new Promise((resolve, reject) => {
    cp.execFile(
      fgxPath(),
      args,
      { cwd, encoding: "buffer", maxBuffer: 256 * 1024 * 1024 },
      (error, stdout, stderr) => {
        if (error) {
          reject(new Error(String(stderr || error.message).trim()));
          return;
        }
        resolve(stdout);
      }
    );
  });
}

function isProjectFile(document) {
  return (
    document &&
    (document.languageId === "forgentex-fgx" ||
      document.languageId === "forgentex-fgai")
  );
}

function humanBytes(count) {
  if (count < 1024) return `${count} B`;
  if (count < 1024 * 1024) return `${(count / 1024).toFixed(1)} KB`;
  return `${(count / (1024 * 1024)).toFixed(1)} MB`;
}

/// One row in the tree.
class SectionItem extends vscode.TreeItem {
  constructor(entry, uri, children) {
    const isNote = entry.kind === "note";
    super(
      isNote ? entry.id.replace("@note.", "") : entry.name,
      children && children.length > 0
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.None
    );
    this.entry = entry;
    this.children = children || [];
    this.contextValue = `forgentex.${entry.kind}`;
    this.id = `${uri.toString()}#${entry.id}`;

    if (entry.kind === "data") {
      this.description = `${entry.shape} · ${entry.lines} lines`;
      this.iconPath = new vscode.ThemeIcon(KIND_ICONS.data);
    } else if (entry.kind === "blob") {
      const stored = entry.storage === "store" ? " · stored" : "";
      this.description = `${entry.media_type} · ${humanBytes(entry.bytes)}${stored}`;
      this.iconPath = new vscode.ThemeIcon(
        entry.media_type && entry.media_type.startsWith("image/")
          ? "file-media"
          : KIND_ICONS.blob
      );
    } else {
      const who = entry.agent ? "agent" : "human";
      this.description = `${who} · ${entry.status}`;
      // An open remark is something still asking for attention, so it carries
      // the colour that means exactly that; a resolved one recedes.
      this.iconPath = new vscode.ThemeIcon(
        entry.agent ? "hubot" : "account",
        new vscode.ThemeColor(
          entry.status === "open"
            ? "editorWarning.foreground"
            : "descriptionForeground"
        )
      );
    }
    this.tooltip = this.buildTooltip(entry);
    this.command = {
      command: "forgentex.fsReveal",
      title: "Reveal section",
      arguments: [uri, entry.id],
    };
  }

  buildTooltip(entry) {
    const lines = [`**${entry.id}**`, ""];
    if (entry.kind === "blob") {
      lines.push(`- type: \`${entry.media_type}\``);
      lines.push(`- size: ${humanBytes(entry.bytes)}`);
      lines.push(`- storage: ${entry.storage}`);
      if (entry.sha256) lines.push(`- sha256: \`${entry.sha256.slice(0, 16)}…\``);
    } else if (entry.kind === "data") {
      lines.push(`- shape: \`${entry.shape}\``);
      lines.push(`- ${entry.lines} lines`);
    } else {
      lines.push(`- on: \`${entry.on}\``);
      lines.push(`- by: ${entry.by}`);
      lines.push(`- status: ${entry.status}`);
    }
    if (entry.created) lines.push(`- created ${entry.created} by ${entry.created_by || "?"}`);
    if (entry.modified) lines.push(`- modified ${entry.modified} by ${entry.modified_by || "?"}`);
    const tooltip = new vscode.MarkdownString(lines.join("\n"));
    tooltip.supportThemeIcons = true;
    return tooltip;
  }
}

/// A heading row, so data, bytes, and remarks read as three groups.
class GroupItem extends vscode.TreeItem {
  constructor(label, icon, children) {
    super(label, vscode.TreeItemCollapsibleState.Expanded);
    this.iconPath = new vscode.ThemeIcon(icon);
    this.children = children;
    this.contextValue = "forgentex.group";
    this.description = `${children.length}`;
  }
}

class ProjectFsProvider {
  constructor() {
    this._onDidChangeTreeData = new vscode.EventEmitter();
    this.onDidChangeTreeData = this._onDidChangeTreeData.event;
    this.uri = null;
    this.sections = [];
    this.notes = [];
  }

  refresh() {
    this._onDidChangeTreeData.fire();
  }

  async load(document) {
    if (!isProjectFile(document)) {
      this.uri = null;
      this.sections = [];
      this.notes = [];
      this.refresh();
      return;
    }
    const file = document.uri.fsPath;
    const cwd = path.dirname(file);
    const [listing, notes] = await Promise.all([
      readJson(["fs", "ls", file, "--json"], cwd),
      readJson(["fs", "notes", file, "--json"], cwd),
    ]);
    this.uri = document.uri;
    this.sections = (listing && listing.sections) || [];
    this.notes = (notes && notes.notes) || [];
    // The view is only worth showing for a file that has sections.
    vscode.commands.executeCommand(
      "setContext",
      "forgentex.hasSections",
      this.sections.length > 0
    );
    this.refresh();
  }

  getTreeItem(element) {
    return element;
  }

  getChildren(element) {
    if (element) {
      return element.children || [];
    }
    if (!this.uri) {
      return [];
    }
    const payloads = this.sections.filter((entry) => entry.kind !== "note");
    const notesById = new Map();
    for (const note of this.notes) {
      const bucket = notesById.get(note.on) || [];
      bucket.push(note);
      notesById.set(note.on, bucket);
    }

    const groups = [];
    const data = payloads.filter((entry) => entry.kind === "data");
    const blobs = payloads.filter((entry) => entry.kind === "blob");
    if (data.length > 0) {
      groups.push(
        new GroupItem(
          "Data",
          "table",
          data.map(
            (entry) =>
              new SectionItem(
                entry,
                this.uri,
                // A remark hangs under the thing it is about, which is what
                // makes the annotations read as a graph rather than a list.
                (notesById.get(entry.id) || []).map(
                  (note) => new SectionItem(noteEntry(note), this.uri, [])
                )
              )
          )
        )
      );
    }
    if (blobs.length > 0) {
      groups.push(
        new GroupItem(
          "Bytes",
          "file-binary",
          blobs.map(
            (entry) =>
              new SectionItem(
                entry,
                this.uri,
                (notesById.get(entry.id) || []).map(
                  (note) => new SectionItem(noteEntry(note), this.uri, [])
                )
              )
          )
        )
      );
    }
    // Remarks about code, or about a node that is not a carried section, still
    // need somewhere to appear.
    const orphans = this.notes.filter(
      (note) => !payloads.some((entry) => entry.id === note.on)
    );
    if (orphans.length > 0) {
      groups.push(
        new GroupItem(
          "Notes",
          "comment-discussion",
          orphans.map((note) => new SectionItem(noteEntry(note), this.uri, []))
        )
      );
    }
    return groups;
  }
}

function noteEntry(note) {
  return Object.assign({ kind: "note", name: note.id }, note);
}

/// Put the cursor on a section's header line.
async function revealSection(uri, id) {
  const document = await vscode.workspace.openTextDocument(uri);
  const editor = await vscode.window.showTextDocument(document, { preview: false });
  const name = id.replace(/^@(data|blob|note)\./, "");
  for (let line = 0; line < document.lineCount; line += 1) {
    const text = document.lineAt(line).text;
    const match = SECTION_HEADER.exec(text);
    const dense = /^(DATA|BLOB|NOTE)\s+([A-Za-z_][A-Za-z0-9_]*)\b/.exec(text);
    const found = (match && match.groups.name === name) || (dense && dense[2] === name);
    if (found) {
      const position = new vscode.Position(line, 0);
      editor.selection = new vscode.Selection(position, position);
      editor.revealRange(
        new vscode.Range(position, position),
        vscode.TextEditorRevealType.InCenter
      );
      return;
    }
  }
}

/// Show a blob for what it is: an image as an image, audio and video in a
/// player, anything else as the head of its bytes.
async function previewBlob(uriOrItem, maybeId) {
  const { uri, id } = resolveTarget(uriOrItem, maybeId);
  if (!uri) {
    return;
  }
  let bytes;
  try {
    bytes = await readBytes(
      ["fs", "cat", uri.fsPath, "--tag", id],
      path.dirname(uri.fsPath)
    );
  } catch (error) {
    vscode.window.showErrorMessage(`Forgentex: ${error.message}`);
    return;
  }
  const listing = await readJson(["fs", "ls", uri.fsPath, "--json"], path.dirname(uri.fsPath));
  const entry = ((listing && listing.sections) || []).find((item) => item.id === id) || {};
  const mediaType = entry.media_type || "application/octet-stream";
  const panel = vscode.window.createWebviewPanel(
    "forgentex.blobPreview",
    `${id}`,
    vscode.ViewColumn.Beside,
    { enableScripts: false }
  );
  panel.webview.html = blobPreviewHtml(id, mediaType, bytes);
}

function resolveTarget(uriOrItem, maybeId) {
  if (uriOrItem && uriOrItem.entry) {
    return { uri: uriOrItem.command.arguments[0], id: uriOrItem.entry.id };
  }
  if (uriOrItem instanceof vscode.Uri) {
    return { uri: uriOrItem, id: maybeId };
  }
  const editor = vscode.window.activeTextEditor;
  return { uri: editor ? editor.document.uri : null, id: maybeId };
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (ch) => `&#${ch.charCodeAt(0)};`);
}

function blobPreviewHtml(id, mediaType, bytes) {
  const base64 = bytes.toString("base64");
  const source = `data:${mediaType};base64,${base64}`;
  let body;
  if (mediaType.startsWith("image/")) {
    body = `<img src="${source}" alt="${escapeHtml(id)}">`;
  } else if (mediaType.startsWith("audio/")) {
    body = `<audio controls src="${source}"></audio>`;
  } else if (mediaType.startsWith("video/")) {
    body = `<video controls src="${source}"></video>`;
  } else if (mediaType.startsWith("text/") || mediaType.endsWith("json") || mediaType.endsWith("xml")) {
    body = `<pre>${escapeHtml(bytes.toString("utf8").slice(0, 100000))}</pre>`;
  } else {
    // Not something to render: show the first bytes as hex, which is what a
    // reader actually wants from an opaque payload.
    const head = bytes.subarray(0, 512);
    const rows = [];
    for (let offset = 0; offset < head.length; offset += 16) {
      const slice = head.subarray(offset, offset + 16);
      const hex = [...slice].map((b) => b.toString(16).padStart(2, "0")).join(" ");
      const text = [...slice]
        .map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : "."))
        .join("");
      rows.push(
        `${offset.toString(16).padStart(8, "0")}  ${hex.padEnd(47)}  ${escapeHtml(text)}`
      );
    }
    body = `<pre>${rows.join("\n")}</pre>`;
  }
  return `<!DOCTYPE html>
<html><head><meta charset="utf-8">
<style>
  body { font-family: var(--vscode-font-family); color: var(--vscode-foreground);
         background: var(--vscode-editor-background); padding: 16px; }
  h1 { font-size: 13px; font-weight: 600; margin: 0 0 4px; }
  .meta { font-size: 12px; color: var(--vscode-descriptionForeground); margin-bottom: 16px; }
  img, video { max-width: 100%; border: 1px solid var(--vscode-panel-border);
               border-radius: 4px; background: var(--vscode-editorWidget-background); }
  pre { font-family: var(--vscode-editor-font-family); font-size: 12px;
        background: var(--vscode-textCodeBlock-background); padding: 12px;
        border-radius: 4px; overflow-x: auto; }
</style></head>
<body>
  <h1>${escapeHtml(id)}</h1>
  <div class="meta">${escapeHtml(mediaType)} · ${humanBytes(bytes.length)}</div>
  ${body}
</body></html>`;
}

/// Put every payload on disk, for the tools that need real files.
async function extractPayloads(uriOrItem) {
  const { uri } = resolveTarget(uriOrItem);
  if (!uri) {
    vscode.window.showWarningMessage("Forgentex: open a project file first.");
    return;
  }
  const picked = await vscode.window.showOpenDialog({
    canSelectFolders: true,
    canSelectFiles: false,
    openLabel: "Extract here",
    defaultUri: vscode.Uri.file(path.dirname(uri.fsPath)),
  });
  if (!picked || picked.length === 0) {
    return;
  }
  const out = picked[0].fsPath;
  const report = await readJson(
    ["fs", "extract", uri.fsPath, "--out", out, "--json"],
    path.dirname(uri.fsPath)
  );
  if (!report) {
    vscode.window.showErrorMessage("Forgentex: extract failed; run fgx check on the file.");
    return;
  }
  const count = (report.written || []).length;
  const open = await vscode.window.showInformationMessage(
    `Forgentex: extracted ${count} payload${count === 1 ? "" : "s"} to ${out}.`,
    "Open folder"
  );
  if (open) {
    vscode.commands.executeCommand("revealFileInOS", vscode.Uri.file(out));
  }
}

/// Bring a file on disk into the project.
async function addPayload(uriOrItem) {
  const { uri } = resolveTarget(uriOrItem);
  if (!uri) {
    vscode.window.showWarningMessage("Forgentex: open a project file first.");
    return;
  }
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    openLabel: "Add to project",
    defaultUri: vscode.Uri.file(path.dirname(uri.fsPath)),
  });
  if (!picked || picked.length === 0) {
    return;
  }
  const source = picked[0].fsPath;
  const suggested = path.basename(source, path.extname(source)).replace(/[^A-Za-z0-9_]/g, "_");
  const name = await vscode.window.showInputBox({
    prompt: "Section name",
    value: suggested,
    validateInput: (value) =>
      /^[A-Za-z_][A-Za-z0-9_]*$/.test(value)
        ? null
        : "A section name is a letter or underscore followed by letters, digits, or underscores.",
  });
  if (!name) {
    return;
  }
  const author = config().get("author");
  const args = ["fs", "add", uri.fsPath, "--from", source, "--tag", name];
  if (author) {
    args.push("--by", author);
  }
  await new Promise((resolve) => {
    cp.execFile(
      fgxPath(),
      args,
      { cwd: path.dirname(uri.fsPath) },
      (error, stdout, stderr) => {
        if (error) {
          vscode.window.showErrorMessage(
            `Forgentex: ${String(stderr || error.message).trim()}`
          );
        } else {
          vscode.window.showInformationMessage(`Forgentex: ${String(stdout).trim()}`);
        }
        resolve();
      }
    );
  });
}

/// Lenses on a section header: what is said about it, and what can be done
/// with it, without hunting through a menu.
class SectionLensProvider {
  constructor(provider) {
    this.provider = provider;
  }

  provideCodeLenses(document) {
    if (!isProjectFile(document)) {
      return [];
    }
    const lenses = [];
    const notes = this.provider.uri &&
      this.provider.uri.toString() === document.uri.toString()
      ? this.provider.notes
      : [];
    for (let line = 0; line < document.lineCount; line += 1) {
      const text = document.lineAt(line).text;
      const match = SECTION_HEADER.exec(text) ||
        /^(?<kind>DATA|BLOB|NOTE)\s+(?<name>[A-Za-z_][A-Za-z0-9_]*)\b/.exec(text);
      if (!match) {
        continue;
      }
      const kind = match.groups.kind.toLowerCase();
      if (kind === "note") {
        continue;
      }
      const id = `@${kind}.${match.groups.name}`;
      const range = new vscode.Range(line, 0, line, text.length);
      const about = notes.filter((note) => note.on === id);
      if (about.length > 0) {
        const open = about.filter((note) => note.status === "open").length;
        const agents = about.filter((note) => note.agent).length;
        lenses.push(
          new vscode.CodeLens(range, {
            title: `$(comment) ${about.length} note${about.length === 1 ? "" : "s"}` +
              (open > 0 ? ` · ${open} open` : "") +
              (agents > 0 ? ` · ${agents} from agents` : ""),
            command: "forgentex.fsShowNotes",
            arguments: [document.uri, id],
          })
        );
      }
      if (kind === "blob") {
        lenses.push(
          new vscode.CodeLens(range, {
            title: "$(eye) Preview",
            command: "forgentex.fsPreviewBlob",
            arguments: [document.uri, id],
          })
        );
      }
    }
    return lenses;
  }
}

/// Everything said about one section, as a readable thread.
async function showNotes(uri, id) {
  const target = uri instanceof vscode.Uri ? uri : (vscode.window.activeTextEditor || {}).document?.uri;
  if (!target) {
    return;
  }
  const report = await readJson(
    ["fs", "notes", target.fsPath, "--on", id, "--json"],
    path.dirname(target.fsPath)
  );
  const notes = (report && report.notes) || [];
  const lines = [`# Notes on ${id}`, ""];
  for (const note of notes) {
    const who = note.agent ? `🤖 ${note.by}` : `👤 ${note.by}`;
    lines.push(`### ${who} · ${note.status}${note.created ? ` · ${note.created}` : ""}`);
    if (note.reply_to) {
      lines.push(`_in reply to ${note.reply_to}_`);
    }
    lines.push("");
    lines.push(note.text);
    lines.push("");
  }
  if (notes.length === 0) {
    lines.push("_No notes yet._");
  }
  const document = await vscode.workspace.openTextDocument({
    content: lines.join("\n"),
    language: "markdown",
  });
  await vscode.window.showTextDocument(document, { preview: true, viewColumn: vscode.ViewColumn.Beside });
}

function registerProjectFilesystem(context) {
  const provider = new ProjectFsProvider();
  const view = vscode.window.createTreeView("forgentex.projectFilesystem", {
    treeDataProvider: provider,
    showCollapseAll: true,
  });
  context.subscriptions.push(view);

  const reload = () => provider.load(vscode.window.activeTextEditor?.document);
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(reload),
    // Sections are read through the CLI, which reads the file on disk, so the
    // view follows saves rather than keystrokes.
    vscode.workspace.onDidSaveTextDocument((document) => {
      if (isProjectFile(document)) {
        provider.load(document);
      }
    }),
    vscode.commands.registerCommand("forgentex.fsRefresh", reload),
    vscode.commands.registerCommand("forgentex.fsReveal", revealSection),
    vscode.commands.registerCommand("forgentex.fsPreviewBlob", previewBlob),
    vscode.commands.registerCommand("forgentex.fsExtract", extractPayloads),
    vscode.commands.registerCommand("forgentex.fsAdd", addPayload),
    vscode.commands.registerCommand("forgentex.fsShowNotes", showNotes)
  );

  for (const language of ["forgentex-fgx", "forgentex-fgai"]) {
    context.subscriptions.push(
      vscode.languages.registerCodeLensProvider(
        { language },
        new SectionLensProvider(provider)
      )
    );
  }

  reload();
  return provider;
}

module.exports = {
  registerProjectFilesystem,
  // Exposed for the smoke test, which drives the view against the real CLI.
  __test: { ProjectFsProvider, blobPreviewHtml, humanBytes, SECTION_HEADER },
};
