#!/usr/bin/env node
// Grammar coverage check for the Forgentex VS Code extension.
//
// The TextMate grammars are hand-written, so they drift from the language as
// keywords are added. This asserts that every keyword the parser recognises is
// matched by at least one grammar pattern, that every pattern's regex compiles,
// and that the manifest's contributions resolve to files that exist.
//
// Oniguruma (what VS Code actually runs) is a superset of JavaScript RegExp for
// the constructs used here, so compiling with RegExp is a valid approximation.
//
// Run: node test/grammar-coverage.js

const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
let failures = 0;

function fail(message) {
  failures += 1;
  console.error(`FAIL ${message}`);
}

function readJson(relative) {
  return JSON.parse(fs.readFileSync(path.join(root, relative), "utf8"));
}

// Keywords accepted by src/parser.rs. Keep in sync when the language grows;
// the human list is the `.fgx` surface and the agent list is the `.fgai` one.
const HUMAN_KEYWORDS = [
  "asm", "bind", "break", "case", "clobber", "code", "const", "continue",
  "dep", "edge", "effects", "else", "enum", "env", "expect",
  "expect_diagnostic", "export", "false", "fn", "for", "foreign", "if",
  "impl", "import", "in", "input", "let", "match", "mock", "module", "op",
  "out", "output", "pipeline", "pub", "require", "requires", "return",
  "sandbox", "share", "stage", "struct", "test", "trait", "true", "type",
  "unsafe", "version", "via", "while",
  // Carried sections: the project filesystem inside the file.
  "data", "blob", "note", "content", "inline", "store", "created", "modified",
  "sha256", "bytes", "status", "reply-to",
];

const AGENT_KEYWORDS = [
  "A", "ASM", "BIND", "BR", "CAP", "CAPREF", "CLOBBER", "CN", "CODE", "CONST",
  "DEP", "EDGE", "EI", "EL", "EN", "ENDCAP", "ENDPIPELINE", "EXPECT",
  "EXPECT_DIAGNOSTIC", "EXPORT", "F", "FOR", "I", "IF", "IM", "IMPORT", "IN",
  "LET", "M", "MOCK", "MT", "OP", "OUT", "PIPELINE", "PUB", "R", "REQ", "S",
  "SBX", "SET", "SHARE", "STAGE", "TEST", "TR", "TYPE", "UF", "VIA", "WH",
  "XTEST",
  // Carried sections in the dense view.
  "DATA", "ENDDATA", "BODY", "BLOB", "ENDBLOB", "B64", "SIZE", "HASH", "STORE",
  "NOTE", "ENDNOTE", "TXT", "CRT", "MOD", "BY", "ON", "RE", "ST",
];

const PRIMITIVE_TYPES = [
  "bool", "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64",
  "usize", "f32", "f64", "str", "void",
];

function collectRegexes(grammar, file) {
  const found = [];
  const walk = (node) => {
    if (!node || typeof node !== "object") {
      return;
    }
    for (const key of ["match", "begin", "end"]) {
      if (typeof node[key] === "string") {
        try {
          found.push(new RegExp(node[key]));
        } catch (error) {
          fail(`${file}: ${key} does not compile: ${node[key]} (${error.message})`);
        }
      }
    }
    for (const value of Object.values(node)) {
      walk(value);
    }
  };
  walk(grammar);
  return found;
}

function checkCoverage(file, keywords, label) {
  const grammar = JSON.parse(fs.readFileSync(path.join(root, "syntaxes", file), "utf8"));
  const regexes = collectRegexes(grammar, file);
  const missing = keywords.filter((keyword) => !regexes.some((re) => {
    re.lastIndex = 0;
    return re.test(keyword);
  }));
  if (missing.length > 0) {
    fail(`${file}: ${label} not highlighted: ${missing.join(", ")}`);
  } else {
    console.log(`ok ${file}: all ${keywords.length} ${label} highlighted (${regexes.length} patterns)`);
  }
}

// 1. Every declared contribution must point at a file that exists.
const manifest = readJson("package.json");
const languageIds = new Set(manifest.contributes.languages.map((entry) => entry.id));
for (const entry of manifest.contributes.languages) {
  if (!fs.existsSync(path.join(root, entry.configuration))) {
    fail(`package.json: missing language configuration ${entry.configuration}`);
  }
}
for (const entry of manifest.contributes.grammars) {
  if (!fs.existsSync(path.join(root, entry.path))) {
    fail(`package.json: missing grammar ${entry.path}`);
  }
  if (!languageIds.has(entry.language)) {
    fail(`package.json: grammar references unknown language ${entry.language}`);
  }
}
for (const entry of manifest.contributes.snippets || []) {
  if (!fs.existsSync(path.join(root, entry.path))) {
    fail(`package.json: missing snippet file ${entry.path}`);
  }
}
if (!fs.existsSync(path.join(root, manifest.main))) {
  fail(`package.json: missing main ${manifest.main}`);
}

/// Local modules required by `entry`, as relative paths with a .js suffix.
function moduleRequires(entry) {
  const text = fs.readFileSync(path.join(root, entry), "utf8");
  return [...text.matchAll(/require\(\s*"\.\/([^"]+)"\s*\)/g)]
    .map((match) => (match[1].endsWith(".js") ? match[1] : `${match[1]}.js`))
    .filter((file) => fs.existsSync(path.join(root, file)));
}

// 2. Every contributed command must be registered in the extension, and every
//    registered command must be contributed, or the palette and code disagree.
// The entry point plus the modules it pulls in: a command registered in a
// sibling module is still registered, and splitting the extension across files
// must not weaken this check.
const source = [manifest.main, ...moduleRequires(manifest.main)]
  .map((file) => fs.readFileSync(path.join(root, file), "utf8"))
  .join("\n");
const declared = manifest.contributes.commands.map((entry) => entry.command);
const registered = [...source.matchAll(/registerCommand\(\s*"([^"]+)"/g)].map((m) => m[1]);
for (const command of declared) {
  if (!registered.includes(command)) {
    fail(`package.json declares ${command} but extension.js never registers it`);
  }
}
for (const command of registered) {
  if (!declared.includes(command)) {
    fail(`extension.js registers ${command} but package.json never declares it`);
  }
}

// 3. Every configuration key the extension reads must be declared.
const declaredConfig = Object.keys(manifest.contributes.configuration.properties);
const readConfig = [...source.matchAll(/config\(\)\.get\(\s*"([^"]+)"/g)].map((m) => `forgentex.${m[1]}`);
for (const key of new Set(readConfig)) {
  if (!declaredConfig.includes(key)) {
    fail(`extension.js reads undeclared configuration ${key}`);
  }
}

// 4. Grammar keyword coverage.
checkCoverage("fgx.tmLanguage.json", HUMAN_KEYWORDS, "human keywords");
checkCoverage("fgx.tmLanguage.json", PRIMITIVE_TYPES, "primitive types");
checkCoverage("fgai.tmLanguage.json", AGENT_KEYWORDS, "agent keywords");
checkCoverage("fgai.tmLanguage.json", PRIMITIVE_TYPES, "primitive types");

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log("\nall extension checks passed");
