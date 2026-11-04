#!/usr/bin/env node
// Compile .woodpecker/gate.yml with the real woodpecker-cli and read which steps it
// selected. The `when` blocks and depends_on are the file's own; only the bodies become
// `echo`, run by the CLI's local backend, so nothing here touches a service or a runner.
// That backend cannot run services, so each service compiles as a step under its own `when`.
//
// No CLI is a disclosed skip with exit 0, never a pass: the CI image has none.
// Usage: WOODPECKER_CLI=/path/to/woodpecker-cli scripts/test-ci-gate-config.mjs
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { ROOT } from "./ci-gate-scope.mjs";

const cli = process.env.WOODPECKER_CLI || "woodpecker-cli";
if (spawnSync(cli, ["--version"]).error) {
  console.log(
    `SKIPPED, nothing was compiled: no ${cli}. Set WOODPECKER_CLI to validate the workflow.`,
  );
  process.exit(0);
}

const SERVICES = ["postgres", "redis", "rustfs"];
const FULL = [
  "rust",
  "desktop",
  "deploy-scripts",
  "ci-scope-tests",
  "rust-agent",
];
const HOST = [
  "host-scope",
  "host-rust",
  "host-frontend",
  "host-compat",
  "host-gate",
];
const SHA = "a".repeat(40);
const HOST_FILE = "crates/beekeeper-host/src/inventory.rs";
const ADOPTION = [
  "Cargo.lock",
  "crates/beekeeper-cli/Cargo.toml",
  "crates/beekeeper-agent-management/Cargo.toml",
  "desktop/src-tauri/Cargo.lock",
  HOST_FILE,
  "docs/agent-host.md",
];
const manyFiles = Array.from(
  { length: 501 },
  (_, i) => `crates/beekeeper-host/src/f${i}.rs`,
);

const gate = fs.readFileSync(path.join(ROOT, ".woodpecker/gate.yml"), "utf8");

/** Keeps top-level when/variables and each entry's when + depends_on; replaces the rest. */
function fixture(text) {
  const head = ["skip_clone: true"];
  const entries = [];
  let section = null;
  let keep = false;
  for (const line of text.split("\n")) {
    if (/^\s*#/.test(line) || !line.trim()) continue;
    const top = /^([a-z_]+):/.exec(line);
    if (top) {
      section = top[1];
      if (section === "when" || section === "variables") head.push(line);
      keep = false;
      continue;
    }
    if (section === "when" || section === "variables") {
      head.push(line);
    } else if (section === "services" || section === "steps") {
      const entry = /^ {2}([a-z][a-z0-9-]*):$/.exec(line);
      if (entry) {
        entries.push(
          line,
          "    image: bash",
          `    commands: ["echo RAN=${entry[1]}"]`,
        );
        if (section === "services") entries.push("    depends_on: []");
        keep = false;
        continue;
      }
      const key = /^ {4}([a-z_]+):/.exec(line);
      if (key) keep = key[1] === "when" || key[1] === "depends_on";
      if (keep) entries.push(line);
    }
  }
  return `${[...head, "steps:", ...entries].join("\n")}\n`;
}

function compile({ event, branch = "main", prevEvent, prevStatus, files }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ci-gate-config-"));
  try {
    const file = path.join(dir, "gate.yml");
    fs.writeFileSync(file, fixture(gate));
    const args = [
      "exec",
      "--backend-engine",
      "local",
      "--pipeline-event",
      event,
      "--commit-branch",
      branch,
      "--commit-sha",
      SHA,
      "--repo",
      "agiterra/beekeeper",
    ];
    if (prevEvent) args.push("--prev-pipeline-event", prevEvent);
    if (prevStatus) args.push("--prev-pipeline-status", prevStatus);
    if (files) args.push("--pipeline-changed-files", JSON.stringify(files));
    args.push(file);
    const env = { ...process.env, HOME: dir };
    for (const key of Object.keys(env))
      if (key.startsWith("CI_") || key.startsWith("WOODPECKER_"))
        delete env[key];
    const result = spawnSync(cli, args, {
      cwd: dir,
      env,
      encoding: "utf8",
      maxBuffer: 1 << 26,
    });
    const output = `${result.stdout}\n${result.stderr}`;
    return {
      status: result.status,
      output,
      ran: [
        ...new Set([...output.matchAll(/RAN=([a-z0-9-]+)/g)].map((m) => m[1])),
      ],
    };
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

const greenPush = { event: "push", prevEvent: "push", prevStatus: "success" };
const cases = [
  [
    "Host commit on a green push",
    { ...greenPush, files: [HOST_FILE, "docs/agent-host.md"] },
    "host",
  ],
  [
    "menu bar frontend on a green push",
    {
      ...greenPush,
      files: [
        "desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.js",
      ],
    },
    "host",
  ],
  [
    "Host commit on a red push",
    { ...greenPush, prevStatus: "failure", files: [HOST_FILE] },
    "full",
  ],
  [
    "Host commit on a killed push",
    { ...greenPush, prevStatus: "killed", files: [HOST_FILE] },
    "full",
  ],
  [
    "Host commit on a running push",
    { ...greenPush, prevStatus: "running", files: [HOST_FILE] },
    "full",
  ],
  [
    "Host commit after a green nightly",
    { ...greenPush, prevEvent: "cron", files: [HOST_FILE] },
    "full",
  ],
  [
    "Host commit after a green PR pipeline",
    { ...greenPush, prevEvent: "pull_request", files: [HOST_FILE] },
    "full",
  ],
  [
    "Host commit with no previous pipeline",
    { event: "push", files: [HOST_FILE] },
    "full",
  ],
  [
    "adoption 351d62a4c on a green push",
    { ...greenPush, files: ADOPTION },
    "full",
  ],
  [
    "relay change",
    { ...greenPush, files: ["crates/beekeeper-relay/src/main.rs"] },
    "full",
  ],
  [
    "shared crate",
    { ...greenPush, files: ["crates/beekeeper-core/src/kind.rs"] },
    "full",
  ],
  [
    "Host commit carrying the workflow",
    { ...greenPush, files: [HOST_FILE, ".woodpecker/gate.yml"] },
    "full",
  ],
  [
    "unknown path",
    { ...greenPush, files: [HOST_FILE, "something-new/file"] },
    "full",
  ],
  ["no file list", { ...greenPush }, "full"],
  [
    "over Woodpecker's 500-file limit",
    { ...greenPush, files: manyFiles },
    "full",
  ],
  [
    "pull request of Host files",
    {
      event: "pull_request",
      prevEvent: "push",
      prevStatus: "success",
      files: [HOST_FILE],
    },
    "full",
  ],
];

let failures = 0;
for (const [name, input, lane] of cases) {
  const { status, ran, output } = compile(input);
  const expected = lane === "host" ? HOST : [...SERVICES, ...FULL];
  const missing = expected.filter((step) => !ran.includes(step));
  const extra = ran.filter((step) => !expected.includes(step));
  const pass = status === 0 && missing.length === 0 && extra.length === 0;
  console.log(`${pass ? "ok  " : "FAIL"} ${lane.padEnd(4)} ${name}`);
  if (!pass) {
    failures += 1;
    console.log(
      `     exit ${status}; ran [${ran}]; missing [${missing}]; extra [${extra}]`,
    );
    console.log(output.split("\n").slice(-12).join("\n"));
  }
}

// The workflow's docs-only exclusion still means no pipeline: nothing compiles.
const docs = compile({
  ...greenPush,
  files: ["README.md", "docs/agent-host.md"],
});
const docsPass =
  docs.ran.length === 0 && docs.output.includes("all filtered out");
console.log(
  `${docsPass ? "ok  " : "FAIL"} none documentation-only push compiles nothing`,
);
if (!docsPass) {
  failures += 1;
  console.log(`     exit ${docs.status}; ran [${docs.ran}]\n${docs.output}`);
}

const lint = spawnSync(
  cli,
  ["lint", "--strict", path.join(ROOT, ".woodpecker/gate.yml")],
  { encoding: "utf8" },
);
console.log(`${lint.status === 0 ? "ok  " : "FAIL"} lint --strict`);
if (lint.status !== 0) {
  failures += 1;
  console.log(lint.stdout + lint.stderr);
}

console.log(
  failures
    ? `${failures} case(s) failed`
    : `${cases.length + 2} cases passed, compiled by ${spawnSync(cli, ["--version"], { encoding: "utf8" }).stdout.trim()}`,
);
process.exit(failures ? 1 : 0);
