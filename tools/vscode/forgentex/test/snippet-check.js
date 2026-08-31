#!/usr/bin/env node
// Snippet validity check.
//
// A snippet that expands to unparseable source is worse than no snippet: the
// user gets a syntax error the moment they accept it. This expands every
// `.fgx` snippet to its placeholder defaults and runs `fgx check` on the
// result. Patch (`.fgp`) snippets are skipped: they are not Forgentex source.
//
// Run: node test/snippet-check.js [path-to-fgx]

const fs = require("fs");
const os = require("os");
const path = require("path");
const cp = require("child_process");

const root = path.join(__dirname, "..");
const repoRoot = path.join(root, "..", "..", "..");
const fgxPath = process.argv[2]
  || path.join(repoRoot, "target", "release", process.platform === "win32" ? "fgx.exe" : "fgx");

// Snippets that are not standalone Forgentex source.
const NOT_SOURCE = new Set(["Patch Operation"]);

// Snippets that only make sense inside a function body; wrap them in one.
const NEEDS_FUNCTION = new Set(["While Loop", "Match"]);

// Extra context some snippets need to type-check on their own.
const PRELUDE = {
  Match: "enum Name:\n    First\n    Second\n\n",
  "Inline Test": "fn call() -> i32:\n    return 0\n\n"
};

// Replace ${1:default} and ${1} with their default text, innermost first.
function expand(body) {
  let text = body.join("\n");
  for (let i = 0; i < 10; i++) {
    const next = text
      .replace(/\$\{\d+:([^{}]*)\}/g, "$1")
      .replace(/\$\{\d+\}/g, "")
      .replace(/\$\d+/g, "");
    if (next === text) {
      break;
    }
    text = next;
  }
  return text;
}

const snippets = JSON.parse(fs.readFileSync(path.join(root, "snippets", "forgentex.json"), "utf8"));
let failures = 0;

if (!fs.existsSync(fgxPath)) {
  console.error(`fgx binary not found at ${fgxPath}; build it with: cargo build --release --bin fgx`);
  process.exit(2);
}

for (const [name, snippet] of Object.entries(snippets)) {
  if (NOT_SOURCE.has(name)) {
    console.log(`skip ${name} (not Forgentex source)`);
    continue;
  }

  let source = expand(snippet.body);
  if (NEEDS_FUNCTION.has(name)) {
    source = source.split("\n").map((line) => `    ${line}`).join("\n");
    // Only declare the enum parameter for snippets whose prelude defines it,
    // or the wrapper itself would reference an unknown type.
    const params = /enum Name/.test(PRELUDE[name] || "")
      ? "limit: i32, value: Name"
      : "limit: i32";
    source = `fn snippet_host(${params}) -> i32:\n    let total: i32 = 0\n${source}\n    return total\n`;
  }
  source = (PRELUDE[name] || "") + source;
  if (!source.endsWith("\n")) {
    source += "\n";
  }

  const file = path.join(os.tmpdir(), `forgentex-snippet-${process.pid}-${name.replace(/\W+/g, "_")}.fgx`);
  fs.writeFileSync(file, source, "utf8");
  try {
    const out = cp.execFileSync(fgxPath, ["check", file, "--json"], { encoding: "utf8" });
    const parsed = JSON.parse(out);
    const diagnostics = parsed.diagnostics || [];
    // Only parse/shape errors matter here. Snippet placeholders are generic, so
    // a snippet is allowed to leave names undefined once expanded standalone.
    const structural = diagnostics.filter((d) =>
      /parse|syntax|indent|unexpected|expected/i.test(d.error || ""));
    if (structural.length > 0) {
      failures += 1;
      console.error(`FAIL ${name}: ${structural.map((d) => d.error).join(", ")}`);
      console.error(source.split("\n").map((l) => `    | ${l}`).join("\n"));
    } else {
      const note = diagnostics.length > 0 ? ` (${diagnostics.length} non-structural)` : "";
      console.log(`ok ${name} parses${note}`);
    }
  } catch (error) {
    failures += 1;
    console.error(`FAIL ${name}: fgx check errored: ${(error.stderr || error.message).toString().trim()}`);
    console.error(source.split("\n").map((l) => `    | ${l}`).join("\n"));
  } finally {
    try {
      fs.rmSync(file, { force: true });
    } catch (ignored) {
      // best effort
    }
  }
}

if (failures > 0) {
  console.error(`\n${failures} snippet(s) failed`);
  process.exit(1);
}
console.log("\nall snippets parse");
