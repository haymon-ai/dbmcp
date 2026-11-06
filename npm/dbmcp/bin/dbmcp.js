#!/usr/bin/env node
// Runs the dbmcp binary from the platform package npm installed for this machine.
"use strict";

const { spawn } = require("child_process");
const { constants } = require("os");
const path = require("path");
const { optionalDependencies } = require("../package.json");

const pkg = `@haymon-ai/dbmcp-${process.platform}-${process.arch}`;

if (!(pkg in optionalDependencies)) {
  console.error(
    `dbmcp: no prebuilt binary for ${process.platform}-${process.arch}. ` +
      "Install from source with `cargo install dbmcp`: https://dbmcp.haymon.ai/docs/installation",
  );
  process.exit(1);
}

let binary;
try {
  binary = path.join(
    path.dirname(require.resolve(`${pkg}/package.json`)),
    "bin",
    process.platform === "win32" ? "dbmcp.exe" : "dbmcp",
  );
} catch {
  console.error(
    `dbmcp: ${pkg} is not installed. Either optional dependencies were omitted ` +
      "(--omit=optional / --no-optional), or this Linux uses musl; the prebuilt binaries need glibc 2.34+ and libstdc++ from GCC 12+.",
  );
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

// Pass termination signals on, so killing the launcher never orphans the server.
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (err) => {
  console.error(`dbmcp: failed to start ${binary}: ${err.message}`);
  // Older npm ignores `libc`, so musl can get the glibc binary, whose loader is missing.
  if (process.platform === "linux" && err.code === "ENOENT") {
    console.error("dbmcp: on musl (e.g. Alpine) the prebuilt binary cannot run; it needs glibc 2.34+.");
  }
  process.exit(1);
});

child.on("exit", (code, signal) => {
  // A child killed by a signal has no exit code; follow the shell's 128 + signal convention.
  process.exit(signal ? 128 + (constants.signals[signal] ?? 0) : code);
});
