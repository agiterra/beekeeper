#!/usr/bin/env node
// The remote gate's Host lane: docs/INTEGRATION.md § The Host lane.
//
// Node because the CI image has no Python and hermit pins Node. The list of what
// counts as Host is scripts/host-gate-policy.json, shared with scripts/host-check.py.
//
//   plan                 Re-derive the scope from git; exit 3 unless it is Host.
//   run <group>          Run one group of steps and write its receipt.
//   verify               Refuse unless every required receipt passed for this commit.
//   classify <base> <head>  Print the plan for a range. Read-only; exit 0.
//   expressions          Print the two `when.evaluate` expressions gate.yml must carry.
import { spawnSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const REFUSED = 3;
const SHA = /^[0-9a-f]{40}$/;

export function loadPolicy(root = ROOT) {
  return JSON.parse(
    fs.readFileSync(path.join(root, "scripts/host-gate-policy.json"), "utf8"),
  );
}

/** Markdown the workflow's own docs-only exclusion treats as inert: top level and docs/ only. */
const isInertMarkdown = (file) =>
  /^[^/]+\.md$/.test(file) ||
  (file.startsWith("docs/") && file.endsWith(".md"));

/** "docs" and "host" can share the Host lane. Anything else, including the unknown, is "full". */
export function classifyPath(policy, file) {
  const { remote } = policy;
  if (
    remote.full_files.includes(file) ||
    remote.full_basenames.includes(path.posix.basename(file))
  ) {
    return { lane: "full", reason: "gate or dependency input" };
  }
  if (isInertMarkdown(file)) return { lane: "docs", reason: "" };
  if (
    policy.in_scope_files.includes(file) ||
    policy.in_scope_prefixes.some((p) => file.startsWith(p))
  ) {
    return { lane: "host", reason: "" };
  }
  for (const [prefix, consumers] of Object.entries(policy.shared_fanout)) {
    if (file.startsWith(prefix))
      return {
        lane: "full",
        reason: `shared crate, also used by ${consumers}`,
      };
  }
  return { lane: "full", reason: "outside the Host product" };
}

/** The first-party paths and commits between two commits, both sides of a rename. Throws on any git error. */
export function rangePaths(cwd, base, head) {
  const git = (...args) => {
    const result = spawnSync("git", args, {
      cwd,
      encoding: "utf8",
      maxBuffer: 1 << 28,
    });
    if (result.status !== 0)
      throw new Error(`git ${args[0]}: ${(result.stderr || "failed").trim()}`);
    return result.stdout;
  };
  const resolve = (rev) =>
    git("rev-parse", "--verify", "--end-of-options", `${rev}^{commit}`).trim();
  const baseSha = resolve(base);
  const headSha = resolve(head);
  git("merge-base", "--is-ancestor", baseSha, headSha);
  const files = new Set();
  // The net diff is what the tree is; the per-commit log keeps a change-then-revert visible.
  const output = [
    git("diff", "--no-renames", "--name-only", "-z", baseSha, headSha, "--"),
    git(
      "log",
      "-m",
      "--no-renames",
      "--name-only",
      "-z",
      "--format=",
      `${baseSha}..${headSha}`,
      "--",
    ),
  ];
  for (const chunk of output) {
    for (const file of chunk.split(/[\0\n]/)) if (file) files.add(file);
  }
  return { files: [...files].sort(), baseSha, headSha };
}

/** Pure: given what CI says and what git says, is this a Host-lane tree? */
export function decide(
  policy,
  { event, branch, prevEvent, prevStatus, prevSha, paths, gitError },
) {
  const full = (reason) => ({ lane: "full", reasons: [reason] });
  if (event !== "push")
    return full(
      `event is ${event || "unknown"}: only a push to main can use the Host lane`,
    );
  if (branch !== "main") return full(`branch is ${branch || "unknown"}`);
  if (prevEvent !== "push")
    return full(`previous pipeline was ${prevEvent || "unknown"}, not a push`);
  if (prevStatus !== "success")
    return full(`previous pipeline is ${prevStatus || "unknown"}, not success`);
  if (!SHA.test(prevSha ?? ""))
    return full(`previous commit is ${prevSha ? "malformed" : "unknown"}`);
  if (gitError) return full(`range cannot be established: ${gitError}`);
  const outside = (paths ?? [])
    .map((file) => ({ file, ...classifyPath(policy, file) }))
    .filter(({ lane }) => lane === "full");
  if (outside.length)
    return {
      lane: "full",
      reasons: outside.map(({ file, reason }) => `${file}: ${reason}`),
    };
  return { lane: "host", reasons: [] };
}

const escapeRegex = (text) =>
  text.replace(/[.+*?^$()[\]{}|\\/]/g, (c) => (c === "/" ? "/" : `[${c}]`));

/** gate.yml's compile-time copy of classifyPath. Tests hold it equal across the tracked tree. */
export function pathPatterns(policy) {
  const { remote } = policy;
  const allow = [
    `(${policy.in_scope_prefixes.map(escapeRegex).join("|")})`,
    `(${policy.in_scope_files.map(escapeRegex).join("|")})$`,
    "[^/]+[.]md$",
    "docs/.*[.]md$",
  ];
  const deny = [
    `^(${remote.full_files.map(escapeRegex).join("|")})$`,
    `(^|/)(${remote.full_basenames.map(escapeRegex).join("|")})$`,
  ];
  return { allow: `^(${allow.join("|")})`, deny: deny.join("|") };
}

export function hostLaneExpression(policy) {
  const { allow, deny } = pathPatterns(policy);
  return [
    'CI_PIPELINE_EVENT == "push"',
    'CI_COMMIT_BRANCH == "main"',
    'CI_PREV_PIPELINE_EVENT == "push"',
    'CI_PREV_PIPELINE_STATUS == "success"',
    'CI_PIPELINE_FILES not in [nil, "", "[]"]',
    `all(fromJSON(CI_PIPELINE_FILES), # matches "${allow}" && not (# matches "${deny}"))`,
  ].join(" && ");
}

export const fullLaneExpression = (policy) =>
  `!(${hostLaneExpression(policy)})`;

/** Every workspace crate that reaches a root through path dependencies, roots excluded. */
export function reverseClosure(metadata, roots) {
  const closure = new Set(roots);
  for (let grew = true; grew; ) {
    grew = false;
    for (const pkg of metadata.packages) {
      if (closure.has(pkg.name)) continue;
      if (pkg.dependencies.some((dep) => dep.path && closure.has(dep.name))) {
        closure.add(pkg.name);
        grew = true;
      }
    }
  }
  for (const root of roots) closure.delete(root);
  return [...closure].sort();
}

/** Fails closed if a Host crate's blast radius outgrew what the lane tests. */
export function reverseDependencyProblems(policy, metadata) {
  const { remote } = policy;
  const known = new Set(metadata.packages.map((pkg) => pkg.name));
  const problems = remote.scope_crates
    .filter((name) => !known.has(name))
    .map((name) => `${name}: not in the workspace`);
  const tested = new Set([...policy.host_packages, ...remote.extra_packages]);
  for (const name of reverseClosure(metadata, remote.scope_crates)) {
    if (remote.relay_image_packages.includes(name))
      problems.push(`${name}: in the relay image, and depends on a Host crate`);
    else if (!tested.has(name))
      problems.push(
        `${name}: depends on a Host crate, and the Host lane does not test it`,
      );
  }
  return problems;
}

/** Each step is { name, argv, cwd?, tests?, quiet?, check? }. All Linux; none pretends to be host-check. */
export function groups(policy) {
  const packages = [
    ...policy.host_packages,
    ...policy.remote.extra_packages,
  ].flatMap((name) => ["-p", name]);
  const manifest = "desktop/src-tauri/Cargo.toml";
  const menubar = ["--manifest-path", manifest, "-p", "beekeeper-menubar"];
  const frontend = `${policy.menubar}/frontend`;
  const fromDesktop = frontend.replace(/^desktop\//, "");
  const biome = ["pnpm", "exec", "biome"];
  return {
    "host-rust": [
      {
        name: "lockfile matches the manifests",
        argv: ["cargo", "metadata", "--locked", "--format-version", "1"],
        quiet: true,
      },
      {
        name: "reverse dependencies are all tested",
        check: "reverse-dependencies",
      },
      {
        name: "fmt: workspace",
        argv: ["cargo", "fmt", "--all", "--", "--check"],
      },
      {
        name: "clippy: Host packages and dependents, all targets",
        argv: [
          "cargo",
          "clippy",
          ...packages,
          "--all-targets",
          "--",
          "-D",
          "warnings",
        ],
      },
      {
        name: "test: Host packages and dependents",
        argv: ["cargo", "test", ...packages, "--no-fail-fast"],
        tests: true,
      },
    ],
    "host-frontend": [
      {
        name: "frontend: format and lint",
        argv: [
          ...biome,
          "check",
          fromDesktop,
          "../scripts/host-manage-ui-check.mjs",
          "../scripts/host-manage-page-step.mjs",
          "../scripts/host-manage-ui-fixture-check.mjs",
        ],
        cwd: "desktop",
      },
      {
        name: "frontend: no undeclared variables",
        argv: [
          ...biome,
          "lint",
          "--only=correctness/noUndeclaredVariables",
          `${fromDesktop}/manage-host.js`,
          `${fromDesktop}/manage-host-view.js`,
        ],
        cwd: "desktop",
      },
      {
        name: "frontend: script parses",
        argv: ["node", "--check", `${frontend}/manage-host.js`],
      },
      {
        name: "frontend: view script parses",
        argv: ["node", "--check", `${frontend}/manage-host-view.js`],
      },
    ],
    "host-compat": [
      {
        name: "fmt: menu bar",
        argv: ["cargo", "fmt", ...menubar, "--", "--check"],
      },
      {
        name: "clippy: menu bar on Linux, all targets",
        argv: [
          "cargo",
          "clippy",
          ...menubar,
          "--all-targets",
          "--",
          "-D",
          "warnings",
        ],
      },
      {
        name: "test: menu bar on Linux",
        argv: ["cargo", "test", ...menubar],
        tests: true,
      },
      // The library, not the binary: main.rs calls run(), which is macOS-only, so the binary
      // has no Linux build. The library is what links the Host crates.
      {
        name: "compat: desktop library compiles against the Host libraries on Linux",
        argv: [
          "cargo",
          "check",
          "--manifest-path",
          manifest,
          "-p",
          "beekeeper-desktop",
          "--lib",
        ],
      },
      {
        name: "compat: desktop library tests compile on Linux",
        argv: [
          "cargo",
          "check",
          "--manifest-path",
          manifest,
          "-p",
          "beekeeper-desktop",
          "--lib",
          "--profile",
          "test",
        ],
      },
    ],
  };
}

/** Pure: everything `verify` refuses. An empty list is the only passing answer. */
export function verifyProblems(policy, { plan, receipts, commit }) {
  if (!plan) return ["no plan: host-scope did not record one"];
  const problems = [];
  if (plan.lane !== "host")
    problems.push(`plan selected ${plan.lane}, not host`);
  if (!commit || plan.head !== commit)
    problems.push(`plan is for ${plan.head}, this pipeline is ${commit}`);
  for (const [group, steps] of Object.entries(groups(policy))) {
    const receipt = receipts[group];
    if (!receipt) {
      problems.push(`${group}: did not run`);
      continue;
    }
    if (receipt.commit !== commit)
      problems.push(`${group}: receipt is for ${receipt.commit}`);
    for (const step of steps) {
      const ran = receipt.steps.find(({ name }) => name === step.name);
      if (!ran) problems.push(`${group}: "${step.name}" did not run`);
      else if (ran.code !== 0)
        problems.push(`${group}: "${step.name}" exited ${ran.code}`);
      else if (step.tests && !(ran.tests > 0))
        problems.push(`${group}: "${step.name}" ran no tests`);
    }
  }
  return problems;
}

const stateDir = () =>
  process.env.CI_GATE_STATE_DIR || path.join(ROOT, "..", ".ci-gate-state");

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

function writeJson(name, value) {
  fs.mkdirSync(stateDir(), { recursive: true });
  fs.writeFileSync(
    path.join(stateDir(), name),
    `${JSON.stringify(value, null, 2)}\n`,
  );
}

/** Every URL a test could read points at a closed port, the same as `just test-unit`. */
function unitEnv() {
  const script = `. ${JSON.stringify(path.join(ROOT, "scripts/lib/unit-env.sh"))}; env -0`;
  const result = spawnSync("bash", ["-c", script], {
    cwd: ROOT,
    maxBuffer: 1 << 26,
  });
  if (result.status !== 0) throw new Error("scripts/lib/unit-env.sh failed");
  const env = {};
  for (const item of result.stdout.toString().split("\0")) {
    const at = item.indexOf("=");
    if (at > 0) env[item.slice(0, at)] = item.slice(at + 1);
  }
  return env;
}

function runProcess(step, env) {
  return new Promise((resolve) => {
    const child = spawn(step.argv[0], step.argv.slice(1), {
      cwd: path.join(ROOT, step.cwd ?? "."),
      env,
      stdio: ["ignore", "pipe", "inherit"],
    });
    let tests = 0;
    let tail = "";
    child.stdout.on("data", (chunk) => {
      if (!step.quiet) process.stdout.write(chunk);
      if (!step.tests) return;
      const text = tail + chunk.toString();
      const lines = text.split("\n");
      tail = lines.pop();
      for (const line of lines) {
        const match = /^test result: ok\. (\d+) passed/.exec(line);
        if (match) tests += Number(match[1]);
      }
    });
    child.on("error", (error) => {
      console.error(`could not start ${step.argv[0]}: ${error.message}`);
      resolve({ code: 127, tests });
    });
    child.on("close", (code, signal) =>
      resolve({ code: code ?? (signal ? 128 : 1), tests }),
    );
  });
}

function runCheck(policy, step, env) {
  if (step.check !== "reverse-dependencies")
    throw new Error(`unknown check ${step.check}`);
  const result = spawnSync(
    "cargo",
    ["metadata", "--locked", "--no-deps", "--format-version", "1"],
    {
      cwd: ROOT,
      env,
      encoding: "utf8",
      maxBuffer: 1 << 28,
    },
  );
  if (result.status !== 0) {
    console.error(result.stderr);
    return { code: result.status ?? 1 };
  }
  const problems = reverseDependencyProblems(policy, JSON.parse(result.stdout));
  for (const problem of problems)
    console.error(`HOST_LANE_REVERSE_DEPENDENCY: ${problem}`);
  return { code: problems.length ? REFUSED : 0 };
}

async function run(policy, group) {
  const steps = groups(policy)[group];
  if (!steps) throw new Error(`unknown group ${group}`);
  const env = unitEnv();
  const receipt = { group, commit: process.env.CI_COMMIT_SHA, steps: [] };
  for (const step of steps) {
    const shown = step.argv ? `\n$ ${step.argv.join(" ")}` : "";
    console.log(`\n== ${step.name}${shown}`);
    const begun = Date.now();
    const { code, tests } = step.check
      ? runCheck(policy, step, env)
      : await runProcess(step, env);
    receipt.steps.push({
      name: step.name,
      code,
      tests,
      seconds: Math.round((Date.now() - begun) / 1000),
    });
    writeJson(`${group}.json`, receipt);
    if (code !== 0) return code;
  }
  return 0;
}

function plan(
  policy,
  env,
  base = env.CI_PREV_COMMIT_SHA,
  head = env.CI_COMMIT_SHA,
) {
  let paths = [];
  let gitError = "";
  let baseSha = "";
  try {
    if (!base || !head) throw new Error("no base or no head named");
    const range = rangePaths(ROOT, base, head);
    paths = range.files;
    baseSha = range.baseSha;
    head = range.headSha;
  } catch (error) {
    gitError = error.message;
  }
  const decision = decide(policy, {
    event: env.CI_PIPELINE_EVENT,
    branch: env.CI_COMMIT_BRANCH,
    prevEvent: env.CI_PREV_PIPELINE_EVENT,
    prevStatus: env.CI_PREV_PIPELINE_STATUS,
    prevSha: baseSha || base,
    paths,
    gitError,
  });
  return {
    ...decision,
    base: baseSha || base || null,
    head: head ?? null,
    paths,
  };
}

function printPlan(result) {
  console.log(
    `lane: ${result.lane}\nrange: ${result.base}..${result.head} (${result.paths.length} path(s))`,
  );
  for (const file of result.paths) console.log(`  ${file}`);
  for (const reason of result.reasons) console.log(`full because: ${reason}`);
}

async function main([command, ...args]) {
  const policy = loadPolicy();
  if (command === "expressions") {
    console.log(hostLaneExpression(policy));
    console.log(fullLaneExpression(policy));
    return 0;
  }
  if (command === "classify") {
    const [base, head] = args;
    printPlan(
      plan(
        policy,
        {
          CI_PIPELINE_EVENT: "push",
          CI_COMMIT_BRANCH: "main",
          CI_PREV_PIPELINE_EVENT: "push",
          CI_PREV_PIPELINE_STATUS: "success",
        },
        base,
        head,
      ),
    );
    return 0;
  }
  if (command === "plan") {
    const result = plan(policy, process.env);
    const checkedOut = spawnSync("git", ["rev-parse", "HEAD"], {
      cwd: ROOT,
      encoding: "utf8",
    }).stdout?.trim();
    if (result.lane === "host" && checkedOut !== result.head) {
      result.lane = "full";
      result.reasons = [
        `checked out ${checkedOut || "nothing"}, not the commit under test ${result.head}`,
      ];
    }
    printPlan(result);
    writeJson("plan.json", result);
    if (result.lane === "host") return 0;
    console.error(
      "HOST_LANE_REFUSED: this pipeline compiled the Host lane, but git does not vouch for it, so it fails rather than passes." +
        " Restart it or push again: its predecessor is now red, so that run compiles the full lane.",
    );
    return REFUSED;
  }
  if (command === "run") return run(policy, args[0]);
  if (command === "verify") {
    const receipts = {};
    for (const group of Object.keys(groups(policy)))
      receipts[group] = readJson(path.join(stateDir(), `${group}.json`));
    const problems = verifyProblems(policy, {
      plan: readJson(path.join(stateDir(), "plan.json")),
      receipts,
      commit: process.env.CI_COMMIT_SHA,
    });
    console.log("Host lane summary");
    for (const receipt of Object.values(receipts)) {
      for (const step of receipt?.steps ?? []) {
        const tests = step.tests ? `, ${step.tests} tests` : "";
        console.log(
          `  ${step.code === 0 ? "ok  " : "FAIL"} ${String(step.seconds).padStart(5)}s  ${step.name}${tests}`,
        );
      }
    }
    console.log(
      "  Not run by this lane: relay, database and auth integration, beekeeper-agent, desktop JS, web, mobile, macOS menu bar code.",
    );
    for (const problem of problems)
      console.error(`HOST_LANE_INCOMPLETE: ${problem}`);
    return problems.length ? REFUSED : 0;
  }
  console.error(
    "usage: ci-gate-scope.mjs plan | run <group> | verify | classify <base> <head> | expressions",
  );
  return 2;
}

if (
  process.argv[1] &&
  fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exitCode = await main(process.argv.slice(2));
}
