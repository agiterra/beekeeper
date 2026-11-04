// node --test scripts/ci-gate-scope.test.mjs
// Git and Node only: this runs on the CI image, as root, from a shallow clone.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  ROOT,
  classifyPath,
  decide,
  fullLaneExpression,
  groups,
  hostLaneExpression,
  loadPolicy,
  pathPatterns,
  rangePaths,
  reverseClosure,
  reverseDependencyProblems,
  verifyProblems,
} from "./ci-gate-scope.mjs";

const policy = loadPolicy();
const SHA = "a".repeat(40);
const gateText = fs.readFileSync(
  path.join(ROOT, ".woodpecker/gate.yml"),
  "utf8",
);
const greenPush = {
  event: "push",
  branch: "main",
  prevEvent: "push",
  prevStatus: "success",
  prevSha: SHA,
};

// Real commits, by their actual file lists. Fixtures, because the CI clone is shallow.
const ADOPTION_351D62A4C = [
  "Cargo.lock",
  "crates/beekeeper-agent-management/Cargo.toml",
  "crates/beekeeper-agent-management/src/adopt_identity.rs",
  "crates/beekeeper-cli/Cargo.toml",
  "crates/beekeeper-cli/src/commands/host_manage.rs",
  "crates/beekeeper-host-core/src/agent_setup.rs",
  "crates/beekeeper-host/src/inventory.rs",
  "crates/beekeeper-session-provider/src/session_create.rs",
  "desktop/src-tauri/Cargo.lock",
  "desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.js",
  "docs/agent-host.md",
  "scripts/host-manage-adopt-check.py",
];

const MATRIX = {
  // Host
  "crates/beekeeper-host/src/inventory.rs": "host",
  "crates/beekeeper-host-core/src/agent_setup.rs": "host",
  "crates/beekeeper-cli/src/commands/host_manage.rs": "host",
  "crates/beekeeper-acp/src/base_prompt.md": "host",
  "desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.js": "host",
  "scripts/host-manage-check.py": "host",
  "conformance/acme-acp/acme-acp": "host",
  // Inert markdown
  "README.md": "docs",
  "TESTING.md": "docs",
  "docs/agent-host.md": "docs",
  "docs/nips/NIP-AE.md": "docs",
  // Dependency and gate inputs select full even under a Host path
  "Cargo.lock": "full",
  "Cargo.toml": "full",
  "desktop/src-tauri/Cargo.lock": "full",
  "desktop/src-tauri/Cargo.toml": "full",
  "crates/beekeeper-host/Cargo.toml": "full",
  "desktop/src-tauri/crates/beekeeper-menubar/Cargo.toml": "full",
  Justfile: "full",
  justfile: "full",
  "scripts/host-check.py": "full",
  "scripts/host-gate-policy.json": "full",
  ".woodpecker/gate.yml": "full",
  ".woodpecker/nightly.yml": "full",
  "scripts/ci-gate-scope.mjs": "full",
  "scripts/ci-image/Dockerfile": "full",
  "rust-toolchain.toml": "full",
  "bin/.node-24.15.0.pkg": "full",
  Dockerfile: "full",
  // Shared crates and other products
  "crates/beekeeper-core/src/kind.rs": "full",
  "crates/beekeeper-sdk/src/lib.rs": "full",
  "crates/beekeeper-persona/src/lib.rs": "full",
  "crates/beekeeper-ws-client/src/lib.rs": "full",
  "crates/beekeeper-workflow/src/lib.rs": "full",
  "crates/beekeeper-relay/src/main.rs": "full",
  "crates/beekeeper-dev-mcp/src/main.rs": "full",
  "crates/sprig/src/main.rs": "full",
  "migrations/0047_workflow_dispatch_queue.sql": "full",
  "schema/schema.sql": "full",
  "desktop/src/App.tsx": "full",
  "desktop/src-tauri/src/lib.rs": "full",
  "desktop/src-tauri/crates/beekeeper-tray/src/lib.rs": "full",
  "mobile/lib/main.dart": "full",
  "web/src/main.ts": "full",
  "deploy/autodeploy/autodeploy": "full",
  // Near misses and the unknown
  "crates/beekeeper-hostile/src/lib.rs": "full",
  "scripts/hostile.sh": "full",
  "scripts/run-tests.sh": "full",
  "docs/nips/NIP-MP.fixtures.json": "full",
  "crates/beekeeper-relay/README.md": "full",
  "desktop/README.md": "full",
  "docs.md/thing.rs": "full",
  "something-new/file": "full",
};

test("the path matrix", () => {
  for (const [file, lane] of Object.entries(MATRIX)) {
    assert.equal(classifyPath(policy, file).lane, lane, file);
  }
});

test("a full selection says why", () => {
  assert.match(
    classifyPath(policy, "crates/beekeeper-core/src/kind.rs").reason,
    /shared crate/,
  );
  assert.match(
    classifyPath(policy, "web/src/main.ts").reason,
    /outside the Host product/,
  );
  assert.match(classifyPath(policy, "Cargo.lock").reason, /dependency input/);
});

test("the compile-time patterns agree with the classifier on the matrix and the tracked tree", () => {
  const { allow, deny } = pathPatterns(policy);
  const isHostLane = (file) =>
    new RegExp(allow).test(file) && !new RegExp(deny).test(file);
  const tracked = spawnSync("git", ["ls-files", "-z"], {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  })
    .stdout.split("\0")
    .filter(Boolean);
  assert.ok(tracked.length > 1000, "expected the whole tree");
  for (const file of [...Object.keys(MATRIX), ...tracked]) {
    assert.equal(
      isHostLane(file),
      classifyPath(policy, file).lane !== "full",
      file,
    );
  }
});

test("gate.yml carries the generated expressions verbatim, and they are exact negations", () => {
  const host = hostLaneExpression(policy);
  assert.equal(fullLaneExpression(policy), `!(${host})`);
  assert.ok(
    gateText.includes(`&host_lane >-\n    ${host}\n`),
    "host_lane drifted: re-run `ci-gate-scope.mjs expressions`",
  );
  assert.ok(
    gateText.includes(`&full_lane >-\n    !(${host})\n`),
    "full_lane drifted: re-run `ci-gate-scope.mjs expressions`",
  );
});

/** Top-level `services:`/`steps:` entries as { name, lines }; a deliberately small reader. */
function entries(section) {
  const lines = gateText.split("\n");
  const start = lines.indexOf(`${section}:`);
  assert.ok(start >= 0, section);
  const found = [];
  for (const line of lines.slice(start + 1)) {
    if (/^\S/.test(line)) break;
    const header = /^ {2}([a-z][a-z0-9-]*):$/.exec(line);
    if (header) found.push({ name: header[1], lines: [] });
    else found.at(-1)?.lines.push(line);
  }
  return found;
}
const body = (entry) => entry.lines.join("\n");

test("every service and step belongs to exactly one lane", () => {
  const all = [...entries("services"), ...entries("steps")];
  assert.ok(all.length >= 11);
  for (const entry of all) {
    const lanes = ["*host_lane", "*full_lane"].filter((lane) =>
      body(entry).includes(`evaluate: ${lane}`),
    );
    assert.equal(lanes.length, 1, entry.name);
  }
  for (const service of entries("services"))
    assert.ok(
      body(service).includes("*full_lane"),
      `${service.name} would start in the Host lane`,
    );
});

test("nothing is allowed to fail quietly, and no lane depends across lanes", () => {
  assert.doesNotMatch(gateText.replace(/^\s*#.*$/gm, ""), /failure:\s*ignore/);
  const steps = entries("steps");
  const laneOf = new Map(
    steps.map((step) => [
      step.name,
      body(step).includes("*host_lane") ? "host" : "full",
    ]),
  );
  for (const step of steps) {
    const match = /depends_on: \[(.*)\]/.exec(body(step));
    assert.ok(match, `${step.name} has an explicit depends_on`);
    for (const dep of match[1]
      .split(",")
      .map((d) => d.trim())
      .filter(Boolean)) {
      assert.equal(
        laneOf.get(dep),
        laneOf.get(step.name),
        `${step.name} -> ${dep}`,
      );
    }
  }
});

test("the Host lane holds no Desktop JS, web, mobile or relay step, and the final step needs them all", () => {
  const host = entries("steps").filter((step) =>
    body(step).includes("*host_lane"),
  );
  const text = host.map(body).join("\n") + JSON.stringify(groups(policy));
  for (const forbidden of [
    "desktop-test",
    "desktop-check",
    "beekeeper-relay",
    "beekeeper-db",
    "flutter",
    "test:e2e",
    "--workspace",
    "just ci",
  ]) {
    assert.ok(!text.includes(forbidden), `the Host lane reaches ${forbidden}`);
  }
  const final = host.find((step) => step.name === "host-gate");
  assert.match(body(final), /status: \[success, failure\]/);
  for (const step of host.filter((s) => s !== final)) {
    assert.ok(
      body(final).includes(step.name),
      `host-gate must wait for ${step.name}`,
    );
  }
  for (const group of Object.keys(groups(policy))) {
    assert.ok(
      host.some((step) =>
        body(step).includes(`ci-gate-scope.mjs run ${group}`),
      ),
      group,
    );
  }
});

test("the Host lane keeps Host tests and the Desktop Rust compile", () => {
  const all = Object.values(groups(policy)).flat();
  const named = (text) => all.find((step) => step.name.includes(text));
  const hostTests = named("test: Host packages");
  assert.ok(hostTests.tests, "must require a test count");
  for (const name of [
    ...policy.host_packages,
    ...policy.remote.extra_packages,
  ]) {
    assert.ok(hostTests.argv.includes(name), name);
    assert.ok(named("clippy: Host packages").argv.includes(name), name);
  }
  const compat = named("compat: desktop library compiles");
  assert.ok(
    compat.argv.includes("beekeeper-desktop") && compat.argv.includes("--lib"),
  );
  assert.ok(named("compat: desktop library tests").argv.includes("test"));
});

test("the decision: only a green push predecessor and a Host range select Host", () => {
  const paths = [
    "crates/beekeeper-host/src/inventory.rs",
    "docs/agent-host.md",
  ];
  assert.equal(decide(policy, { ...greenPush, paths }).lane, "host");
  const refused = {
    "red predecessor": { prevStatus: "failure" },
    "killed predecessor": { prevStatus: "killed" },
    "running predecessor": { prevStatus: "running" },
    "unknown predecessor": { prevStatus: undefined },
    "nightly predecessor": { prevEvent: "cron" },
    "PR predecessor": { prevEvent: "pull_request" },
    "pull request": { event: "pull_request" },
    "another branch": { branch: "work/x" },
    "missing base": { prevSha: undefined },
    "malformed base": { prevSha: "abc" },
    "git failure": { gitError: "bad object" },
    "a path outside Host": { paths: [...paths, "web/src/main.ts"] },
    "a lockfile": { paths: [...paths, "Cargo.lock"] },
  };
  for (const [name, change] of Object.entries(refused)) {
    const plan = decide(policy, { ...greenPush, paths, ...change });
    assert.equal(plan.lane, "full", name);
    assert.ok(plan.reasons.length > 0, name);
  }
});

test("plan examples from real commits", () => {
  const adoption = decide(policy, { ...greenPush, paths: ADOPTION_351D62A4C });
  assert.equal(
    adoption.lane,
    "full",
    "351d62a4c moves both lockfiles and two manifests",
  );
  assert.deepEqual(adoption.reasons.map((r) => r.split(":")[0]).sort(), [
    "Cargo.lock",
    "crates/beekeeper-agent-management/Cargo.toml",
    "crates/beekeeper-cli/Cargo.toml",
    "desktop/src-tauri/Cargo.lock",
  ]);
  const withoutInputs = ADOPTION_351D62A4C.filter(
    (f) => !f.endsWith("Cargo.lock") && !f.endsWith("Cargo.toml"),
  );
  assert.equal(
    decide(policy, { ...greenPush, paths: withoutInputs }).lane,
    "host",
  );
  assert.equal(
    decide(policy, {
      ...greenPush,
      paths: ["crates/beekeeper-relay/src/main.rs"],
    }).lane,
    "full",
  );
  assert.equal(
    decide(policy, {
      ...greenPush,
      paths: ["crates/beekeeper-core/src/kind.rs"],
    }).lane,
    "full",
  );
});

function repo() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ci-gate-scope-"));
  const git = (...args) =>
    execFileSync("git", args, {
      cwd: dir,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.name", "scope test");
  git("config", "user.email", "scope@invalid");
  const write = (file, text = "x\n") => {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true });
    fs.writeFileSync(path.join(dir, file), text);
  };
  const commit = (message = "c") => {
    git("add", "-A");
    git("commit", "-qm", message, "--allow-empty");
    return git("rev-parse", "HEAD");
  };
  return {
    dir,
    git,
    write,
    commit,
    done: () => fs.rmSync(dir, { recursive: true, force: true }),
  };
}

const lane = (paths) => decide(policy, { ...greenPush, paths }).lane;

test("range: a commit outside Host cannot hide behind a Host commit at the tip", () => {
  const r = repo();
  try {
    r.write("crates/beekeeper-relay/src/main.rs");
    const base = r.commit();
    r.write("crates/beekeeper-relay/src/main.rs", "changed\n");
    r.commit();
    r.write("crates/beekeeper-host/src/lib.rs");
    const head = r.commit();
    const { files } = rangePaths(r.dir, base, head);
    assert.deepEqual(files, [
      "crates/beekeeper-host/src/lib.rs",
      "crates/beekeeper-relay/src/main.rs",
    ]);
    assert.equal(lane(files), "full");
  } finally {
    r.done();
  }
});

test("range: a change that was reverted inside the range still selects full", () => {
  const r = repo();
  try {
    r.write("web/a.ts");
    r.write("crates/beekeeper-host/src/lib.rs");
    const base = r.commit();
    r.write("web/a.ts", "changed\n");
    r.commit();
    r.write("web/a.ts");
    const head = r.commit();
    assert.equal(lane(rangePaths(r.dir, base, head).files), "full");
  } finally {
    r.done();
  }
});

test("range: both sides of a rename count, in either direction", () => {
  const r = repo();
  try {
    r.write(
      "migrations/0012.sql",
      "same content, long enough to be detected as a rename\n",
    );
    const base = r.commit();
    fs.mkdirSync(path.join(r.dir, "scripts"));
    fs.mkdirSync(path.join(r.dir, "web"));
    r.git("mv", "migrations/0012.sql", "scripts/host-moved.sql");
    const head = r.commit();
    const { files } = rangePaths(r.dir, base, head);
    assert.deepEqual(files, ["migrations/0012.sql", "scripts/host-moved.sql"]);
    assert.equal(lane(files), "full");
    r.git("mv", "scripts/host-moved.sql", "web/moved.sql");
    const next = r.commit();
    assert.equal(lane(rangePaths(r.dir, head, next).files), "full");
  } finally {
    r.done();
  }
});

test("range: a deleted file counts", () => {
  const r = repo();
  try {
    r.write("web/gone.ts");
    const base = r.commit();
    fs.rmSync(path.join(r.dir, "web/gone.ts"));
    const head = r.commit();
    assert.deepEqual(rangePaths(r.dir, base, head).files, ["web/gone.ts"]);
  } finally {
    r.done();
  }
});

test("range: a merge shows what either side brought", () => {
  const r = repo();
  try {
    r.write("crates/beekeeper-host/src/lib.rs");
    const base = r.commit();
    r.git("checkout", "-qb", "side");
    r.write("web/side.ts");
    r.commit();
    r.git("checkout", "-q", "main");
    r.write("crates/beekeeper-host/src/other.rs");
    r.commit();
    r.git("merge", "-q", "--no-ff", "--no-edit", "side");
    assert.equal(
      lane(rangePaths(r.dir, base, r.git("rev-parse", "HEAD")).files),
      "full",
    );
  } finally {
    r.done();
  }
});

test("range: a filename with spaces survives", () => {
  const r = repo();
  try {
    r.write("crates/beekeeper-host/a.rs");
    const base = r.commit();
    r.write("web/with space.ts");
    const head = r.commit();
    assert.deepEqual(rangePaths(r.dir, base, head).files, [
      "web/with space.ts",
    ]);
  } finally {
    r.done();
  }
});

test("range: restarting the same commit is an empty, vouched range", () => {
  const r = repo();
  try {
    const head = r.commit();
    assert.deepEqual(rangePaths(r.dir, head, head).files, []);
  } finally {
    r.done();
  }
});

test("range: every base it cannot establish throws", () => {
  const r = repo();
  try {
    r.write("a");
    const first = r.commit();
    r.write("b");
    const second = r.commit();
    r.git("checkout", "-qb", "sibling", first);
    r.write("c");
    const sibling = r.commit();
    assert.throws(
      () => rangePaths(r.dir, "0".repeat(40), second),
      /git rev-parse/,
      "absent",
    );
    assert.throws(
      () => rangePaths(r.dir, sibling, second),
      /git merge-base/,
      "force-pushed: not an ancestor",
    );
    assert.throws(
      () => rangePaths(r.dir, "--output=x", second),
      /git rev-parse/,
      "an option is not a revision",
    );
    assert.throws(
      () => rangePaths(r.dir, "a", second),
      /git rev-parse/,
      "a path is not a commit",
    );
  } finally {
    r.done();
  }
});

test("range: a shallow clone that cut off the base fails closed", () => {
  const r = repo();
  const clone = fs.mkdtempSync(path.join(os.tmpdir(), "ci-gate-shallow-"));
  try {
    r.write("a");
    const base = r.commit();
    r.write("web/hidden.ts");
    const hidden = r.commit();
    r.write("crates/beekeeper-host/src/lib.rs");
    const head = r.commit();
    execFileSync(
      "git",
      ["clone", "-q", "--depth", "1", `file://${r.dir}`, clone],
      { stdio: "ignore" },
    );
    assert.throws(() => rangePaths(clone, base, head), /git rev-parse/);
    const deeper = path.join(clone, "two");
    execFileSync(
      "git",
      ["clone", "-q", "--depth", "2", `file://${r.dir}`, deeper],
      { stdio: "ignore" },
    );
    assert.throws(
      () => rangePaths(deeper, base, head),
      /git rev-parse/,
      "base just past the cut",
    );
    assert.equal(
      lane(rangePaths(deeper, hidden, head).files),
      "host",
      "a base inside the depth works",
    );
  } finally {
    r.done();
    fs.rmSync(clone, { recursive: true, force: true });
  }
});

const workspace = (...edges) => {
  const names = new Set(edges.flat());
  return {
    packages: [...names].map((name) => ({
      name,
      dependencies: edges
        .filter(([from]) => from === name)
        .map(([, to]) => ({ name: to, path: `/x/${to}` })),
    })),
  };
};

test("reverse dependencies: derived, transitive, and path-only", () => {
  const graph = workspace(["a", "root"], ["b", "a"], ["c", "other"]);
  graph.packages.push({ name: "d", dependencies: [{ name: "root" }] });
  assert.deepEqual(reverseClosure(graph, ["root"]), ["a", "b"]);
});

test("reverse dependencies: an untested dependent or a relay image crate refuses", () => {
  const scoped = {
    ...policy,
    remote: { ...policy.remote, scope_crates: ["root"] },
  };
  const tested = workspace(["sprig", "root"]);
  assert.deepEqual(reverseDependencyProblems(scoped, tested), []);
  assert.match(
    reverseDependencyProblems(scoped, workspace(["new-crate", "root"]))[0],
    /does not test it/,
  );
  assert.match(
    reverseDependencyProblems(
      scoped,
      workspace(["beekeeper-relay", "root"]),
    )[0],
    /relay image/,
  );
  assert.match(
    reverseDependencyProblems(scoped, { packages: [] })[0],
    /not in the workspace/,
  );
});

function passingReceipts(commit) {
  const receipts = {};
  for (const [group, steps] of Object.entries(groups(policy))) {
    receipts[group] = {
      group,
      commit,
      steps: steps.map((step) => ({
        name: step.name,
        code: 0,
        tests: step.tests ? 7 : 0,
        seconds: 1,
      })),
    };
  }
  return receipts;
}

test("verify: passes only on complete, passing receipts for this commit", () => {
  const plan = { lane: "host", head: SHA };
  assert.deepEqual(
    verifyProblems(policy, {
      plan,
      receipts: passingReceipts(SHA),
      commit: SHA,
    }),
    [],
  );
});

test("verify: every selected failure and every unrun step is a refusal, never a green", () => {
  const plan = { lane: "host", head: SHA };
  const refuses = (mutate, pattern, overrides = {}) => {
    const receipts = passingReceipts(SHA);
    mutate(receipts);
    const problems = verifyProblems(policy, {
      plan,
      receipts,
      commit: SHA,
      ...overrides,
    });
    assert.ok(
      problems.some((p) => pattern.test(p)),
      `${pattern}: ${JSON.stringify(problems)}`,
    );
  };
  refuses((r) => {
    r["host-rust"] = null;
  }, /host-rust: did not run/);
  refuses((r) => {
    r["host-compat"] = null;
  }, /host-compat: did not run/);
  refuses((r) => {
    r["host-frontend"].steps[0].code = 1;
  }, /exited 1/);
  refuses((r) => {
    r["host-rust"].steps.pop();
  }, /did not run/);
  refuses((r) => {
    r["host-rust"].steps.find((s) => s.tests).tests = 0;
  }, /ran no tests/);
  refuses((r) => {
    r["host-compat"].steps.find((s) => s.tests).tests = undefined;
  }, /ran no tests/);
  refuses((r) => {
    r["host-rust"].commit = "b".repeat(40);
  }, /receipt is for/);
  refuses(() => {}, /no plan/, { plan: null });
  refuses(() => {}, /plan selected full/, {
    plan: { lane: "full", head: SHA },
  });
  refuses(() => {}, /plan is for/, {
    plan: { lane: "host", head: "b".repeat(40) },
  });
  refuses(() => {}, /plan is for/, { commit: undefined });
});

test("the local gate reads the same list", () => {
  const source = fs.readFileSync(
    path.join(ROOT, "scripts/host-check.py"),
    "utf8",
  );
  assert.match(source, /scripts\/host-gate-policy\.json/);
  assert.doesNotMatch(
    source,
    /"crates\/beekeeper-host-core\/"/,
    "a second copy of the list",
  );
});
