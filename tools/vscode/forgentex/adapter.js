#!/usr/bin/env node
"use strict";

// Debug adapter shim for the Forgentex VS Code extension.
//
// VS Code launches this and speaks DAP to it over stdio. All it does is start
// `fgx dap` and connect the pipes: the adapter itself lives in the CLI, so the
// editor and the terminal debug through exactly the same code path.
//
// The binary is resolved from FORGENTEX_FGX_PATH when set, so a workspace that
// configures `forgentex.fgxPath` debugs with the same binary it checks with.

const cp = require("child_process");

const command = process.env.FORGENTEX_FGX_PATH || "fgx";
const child = cp.spawn(command, ["dap"], { stdio: ["pipe", "pipe", "pipe"] });

child.on("error", (error) => {
  // Surface the cause on stderr; VS Code shows it in the Debug Console rather
  // than silently failing to start a session.
  process.stderr.write(
    `Forgentex debug adapter could not start '${command}': ${error.message}\n` +
      "Set forgentex.fgxPath, or put fgx on PATH.\n"
  );
  process.exit(1);
});

process.stdin.pipe(child.stdin);
child.stdout.pipe(process.stdout);
child.stderr.pipe(process.stderr);

child.on("exit", (code) => process.exit(code === null ? 1 : code));
