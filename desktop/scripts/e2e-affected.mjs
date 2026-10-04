#!/usr/bin/env node
/**
 * Pick the desktop smoke specs a change can reach, and judge a run against the
 * checked-in known failures.
 *
 *   node scripts/e2e-affected.mjs [select] [base] [--repo <dir>] [--explain]
 *   node scripts/e2e-affected.mjs compare [--exit-codes a,b] [--require-core] <playwright-json-report>...
 *   node scripts/e2e-affected.mjs port
 *
 * `select` (the default) lists the files changed since `merge-base(base, HEAD)`
 * — committed, staged, unstaged and untracked — and prints either the single
 * word `FULL` or one Playwright filter per line (`tests/e2e/x.spec.ts`, or
 * `tests/e2e/x.spec.ts:94` for one test of the core set). `base` defaults to
 * the nearest main: of `main` and every `<remote>/main`, the one whose
 * merge-base with HEAD is newest — no remote name is written in here, and a
 * stale local `main` does not inflate the change set. `--repo <dir>` analyses another checkout — its change set,
 * its sources, its specs and its Playwright config. `--no-full` names the
 * global files but selects as though they had not changed, to show what the
 * rest of a change reaches. Reasons go to stderr, so stdout stays a clean
 * argument list.
 *
 * How a change reaches a spec:
 *
 *   - A global file (see GLOBAL below) prints FULL. `playwright.config.ts` is
 *     global unless its diff only registers or unregisters specs, in which
 *     case those specs are selected.
 *   - A changed smoke spec selects itself; a changed test helper selects every
 *     spec that imports it.
 *   - A changed source file contributes the `data-testid` values it renders
 *     (string literals, and the static head of a template literal). One that
 *     renders none — a hook, a model — contributes those of the nearest files
 *     importing it that do, up to IMPORT_DEPTH levels up. A spec is selected
 *     when it names one of them, or quotes a user-visible string literal of
 *     the changed file itself (specs that click "In 30 minutes" by text).
 *     A file reached only through its importers' ids is named on stderr: that
 *     path stops at the first renderer, so it can miss a spec.
 *   - The CORE set always runs: app boot, sending a message, opening a coding
 *     session.
 *
 * A source file that reaches no spec is named on stderr rather than passed over
 * in silence: the selection is a heuristic, and the full run before landing
 * (`just smoke`) is still the gate.
 *
 * `compare` reads a Playwright JSON report and sorts every failure into
 * known (listed in `tests/e2e/known-failures.json`) or new, and names known
 * failures that passed. It exits 1 when a failure is new, and also when the
 * run cannot be trusted: a top-level Playwright error (a spec that does not
 * load, a server that does not start), no test results at all, a Playwright
 * exit code (`--exit-codes`) that no failing test explains, or — with
 * `--require-core` — a core-set test that did not run.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ownDesktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
// The checkout being analysed; `--repo` points both at another one.
let desktop = ownDesktop;
let e2eDir = join(desktop, "tests/e2e");
const IMPORT_DEPTH = 2;
// A template-literal head shorter than this (`channel-`, `project-`) names a
// family, not a component, and would select half the suite.
const MIN_PREFIX = 10;

/** Repository-relative paths whose change can move any spec. */
const GLOBAL = [
  /^desktop\/tailwind\.config\.[cm]?[jt]s$/,
  /^desktop\/postcss\.config\.[cm]?[jt]s$/,
  /^desktop\/vite\.config\.[cm]?[jt]s$/,
  /^desktop\/index\.html$/,
  /^desktop\/package\.json$/,
  /^pnpm-lock\.yaml$/,
  /^desktop\/playwright\.config\.ts$/,
  /^desktop\/scripts\/e2e-preview-server\.py$/,
  /^desktop\/src\/main\.tsx$/,
  /^desktop\/src\/app\/App\.tsx$/,
  // The shell wraps nearly every spec but renders almost no ids of its own.
  /^desktop\/src\/app\/AppShell[^/]*\.tsx?$/,
  /^desktop\/src\/app\/router\.tsx$/,
  /^desktop\/src\/app\/routeTree\.gen\.ts$/,
  // Served as-is by the preview server (boot.css, fonts, icons).
  /^desktop\/public\//,
  /^desktop\/src\/shared\/styles\//,
  /^desktop\/src\/shared\/ui\/markdown(\.tsx|Utils\.ts|\/)/,
  // The whole mock bridge, not only its entry file.
  /^desktop\/src\/testing\//,
  /^desktop\/tests\/helpers\/bridge\.ts$/,
  /^desktop\/tests\/helpers\/previewOrigin\.ts$/,
  /^desktop\/tests\/fixtures\//,
];

/**
 * Always run. Titles, not line numbers, so an edit above a test cannot quietly
 * point the core set at its neighbour. `title: null` takes the whole file.
 */
const CORE = [
  { spec: "boot-splash.spec.ts", title: null, why: "app boot" },
  {
    spec: "smoke.spec.ts",
    title: "loads the app shell with mocked channels",
    why: "app boot",
  },
  {
    spec: "messaging.spec.ts",
    title: "send a message and see it in timeline",
    why: "send a message",
  },
  {
    spec: "coding-sessions.spec.ts",
    title:
      "a seeded signed session is discoverable, opens, and renders its turn",
    why: "open a coding session",
  },
  {
    spec: "coding-sessions.spec.ts",
    title: "a legacy session stays usable through the unified composer",
    why: "send into a coding session",
  },
];

const SOURCE_EXT = /\.(tsx?|mjs|jsx?)$/;
const UNIT_TEST = /\.test\.m?[jt]sx?$/;

function git(cwd, args) {
  return execFileSync("git", ["-C", cwd, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function lines(text) {
  return text.split("\n").filter(Boolean);
}

/** The nearest main to HEAD (see the header), as a ref name. */
export function nearestMain(repo) {
  const refs = lines(
    git(repo, [
      "for-each-ref",
      "--format=%(refname:short)",
      "refs/heads/main",
      "refs/remotes/*/main",
    ]),
  );
  let best = null;
  let bestCount = -1;
  for (const ref of refs) {
    try {
      const mb = git(repo, ["merge-base", ref, "HEAD"]).trim();
      const count = Number(git(repo, ["rev-list", "--count", mb]).trim());
      if (count > bestCount) {
        best = ref;
        bestCount = count;
      }
    } catch {
      // No common history with HEAD: not a candidate.
    }
  }
  return best ?? "main";
}

/** Changed paths, repository-relative, including untracked files. */
export function changedFiles(repo, base) {
  const mergeBase = git(repo, ["merge-base", base, "HEAD"]).trim();
  const tracked = lines(git(repo, ["diff", "--name-only", mergeBase]));
  const untracked = lines(
    git(repo, ["ls-files", "--others", "--exclude-standard"]),
  );
  return { mergeBase, files: [...new Set([...tracked, ...untracked])] };
}

/** Smoke-project spec names (basenames) from `playwright.config.ts`. */
export function smokeSpecs(configText) {
  const out = new Set();
  // Every project whose name starts with "smoke" owns the `"**/x"` entries up
  // to the next `name:`.
  const blocks = configText.split(/\bname:\s*"/).slice(1);
  for (const block of blocks) {
    if (!block.startsWith("smoke")) continue;
    for (const m of block.matchAll(/"\*\*\/([^"]+)"/g)) out.add(m[1]);
  }
  return out;
}

/**
 * The `data-testid` values a source file renders: exact literals, and the
 * static head of template literals (`agent-${id}` gives the prefix `agent-`).
 */
export function testIdsIn(text) {
  const exact = new Set();
  const prefixes = new Set();
  const addValue = (raw) => {
    for (const m of raw.matchAll(/"([^"\n]+)"|'([^'\n]+)'|`([^`\n]*)`/g)) {
      if (m[3] !== undefined) {
        const head = m[3].split("${")[0];
        if (m[3].includes("${")) {
          if (head.length >= 4) prefixes.add(head);
        } else if (head) exact.add(head);
      } else exact.add(m[1] ?? m[2]);
    }
  };
  // data-testid="x" | data-testid='x'
  for (const m of text.matchAll(/data-testid=("[^"\n]*"|'[^'\n]*')/g)) {
    addValue(m[1]);
  }
  // data-testid={...}: every literal inside the braces (ternaries included).
  for (const m of text.matchAll(/data-testid=\{/g)) {
    addValue(balancedBraces(text, m.index + m[0].length - 1));
  }
  // testId="x", listTestId: "x", "data-testid": "x", dataTestId={`x-${y}`}
  const prop = /(?:\b[A-Za-z]*[tT]est[iI]d\b|"data-testid")\s*[:=]\s*/g;
  for (const m of text.matchAll(prop)) {
    const at = m.index + m[0].length;
    if (text[at] === "{") addValue(balancedBraces(text, at));
    else {
      const q = /^("[^"\n]*"|'[^'\n]*'|`[^`\n]*`)/.exec(
        text.slice(at, at + 300),
      );
      if (q) addValue(q[1]);
    }
  }
  return { exact, prefixes };
}

/**
 * User-visible string literals a source file holds: at least 8 characters,
 * containing a space and a letter, and not a path, class list or selector.
 * A spec that finds an element by its text quotes one of these.
 */
export function textLiteralsIn(text) {
  const out = new Set();
  for (const m of text.matchAll(/"([^"\n]{8,80})"|'([^'\n]{8,80})'/g)) {
    const s = m[1] ?? m[2];
    if (!/ /.test(s) || !/[A-Za-z]/.test(s)) continue;
    if (/[{}<>=/\\]|^[a-z0-9:_-]+( [a-z0-9:_[\]./-]+)+$/.test(s)) continue;
    out.add(s);
  }
  return out;
}

function balancedBraces(text, open) {
  let depth = 0;
  for (let i = open; i < text.length && i < open + 2000; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}" && --depth === 0) return text.slice(open, i + 1);
  }
  return text.slice(open, open + 200);
}

/** Every literal and template head a spec mentions — its candidate ids. */
export function specStrings(text) {
  const exact = new Set();
  const heads = new Set();
  for (const m of text.matchAll(/"([^"\n]{2,})"|'([^'\n]{2,})'|`([^`]*)`/g)) {
    if (m[3] !== undefined) {
      const head = m[3].split("${")[0];
      if (m[3].includes("${")) {
        if (head.length >= 4) heads.add(head);
      } else exact.add(head);
      // A `[data-testid="x"]` selector inside a template literal.
      for (const q of m[3].matchAll(/data-testid[\^$*]?=["']([^"']+)["']/g)) {
        exact.add(q[1]);
      }
    } else {
      const s = m[1] ?? m[2];
      exact.add(s);
      for (const q of s.matchAll(/data-testid[\^$*]?=['"]?([^'"\]]+)/g)) {
        exact.add(q[1]);
      }
    }
  }
  return { exact, heads };
}

/** Does a spec's strings name any of these ids? Returns the first hit. */
export function specNames(spec, ids) {
  for (const t of ids.texts ?? []) if (spec.exact.has(t)) return `"${t}"`;
  for (const id of ids.exact) {
    if (spec.exact.has(id)) return id;
    for (const head of spec.heads) {
      if (head.length >= MIN_PREFIX && id.startsWith(head)) return `${head}…`;
    }
  }
  for (const prefix of ids.prefixes) {
    if (prefix.length < MIN_PREFIX) continue;
    for (const s of spec.exact) if (s.startsWith(prefix)) return `${prefix}…`;
    for (const head of spec.heads) {
      if (
        head.length >= MIN_PREFIX &&
        (head.startsWith(prefix) || prefix.startsWith(head))
      ) {
        return `${prefix}…`;
      }
    }
  }
  return null;
}

/** Module specifiers a source file imports, resolved to desktop-relative paths. */
function importsOf(fileAbs, text) {
  const out = [];
  for (const m of text.matchAll(
    /(?:from\s+|import\s*\(\s*|import\s+)["']([^"']+)["']/g,
  )) {
    const spec = m[1];
    let base;
    if (spec.startsWith("@/")) base = join(desktop, "src", spec.slice(2));
    else if (spec.startsWith(".")) base = resolve(dirname(fileAbs), spec);
    else continue;
    const hit = resolveModule(base);
    if (hit) out.push(relative(desktop, hit));
  }
  return out;
}

function resolveModule(base) {
  const stripped = base.replace(/\.(m?js|jsx)$/, "");
  const candidates = [
    base,
    ...[".ts", ".tsx", ".mjs", ".js", ".css"].map((e) => stripped + e),
    ...["index.ts", "index.tsx"].map((f) => join(base, f)),
  ];
  return candidates.find((c) => existsSync(c) && statSync(c).isFile()) ?? null;
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) walk(p, out);
    else if (SOURCE_EXT.test(entry.name) && !UNIT_TEST.test(entry.name)) {
      out.push(p);
    }
  }
  return out;
}

/** desktop-relative path → desktop-relative paths that import it. */
function reverseImports() {
  const importers = new Map();
  for (const abs of walk(join(desktop, "src"))) {
    const text = readFileSync(abs, "utf8");
    for (const dep of importsOf(abs, text)) {
      if (!importers.has(dep)) importers.set(dep, new Set());
      importers.get(dep).add(relative(desktop, abs));
    }
  }
  return importers;
}

function idsOf(rel) {
  const abs = join(desktop, rel);
  // A file deleted by the change renders nothing any more.
  if (!existsSync(abs)) return { exact: new Set(), prefixes: new Set() };
  return testIdsIn(readFileSync(abs, "utf8"));
}

/**
 * The ids a change to `rel` can move: its own, or — for a file that renders
 * none (a hook, a model, a style) — those of the nearest files importing it
 * that do, climbing at most IMPORT_DEPTH levels. Stopping at the first
 * renderer keeps a shared helper from selecting everything above it.
 */
export function nearestRenderedIds(rel, importers) {
  const ids = { exact: new Set(), prefixes: new Set(), viaImporters: false };
  const seen = new Set([rel]);
  let frontier = [rel];
  for (let d = 0; d <= IMPORT_DEPTH && frontier.length > 0; d++) {
    const next = [];
    for (const r of frontier) {
      const found = idsOf(r);
      if (found.exact.size + found.prefixes.size > 0) {
        if (d > 0) ids.viaImporters = true;
        for (const x of found.exact) ids.exact.add(x);
        for (const x of found.prefixes) ids.prefixes.add(x);
        continue;
      }
      for (const up of importers.get(r) ?? []) {
        if (!seen.has(up)) {
          seen.add(up);
          next.push(up);
        }
      }
    }
    frontier = next;
  }
  return ids;
}

/**
 * The line of the `test(` call whose title this is — Playwright's `file:line`
 * filter matches the call's line, which is not the title's when a formatter
 * wraps the title onto the next one.
 */
export function lineOfTitle(text, title) {
  const quoted = [`"${title}"`, `'${title}'`, `\`${title}\``];
  const rows = text.split("\n");
  for (let i = 0; i < rows.length; i++) {
    if (!quoted.some((q) => rows[i].includes(q))) continue;
    for (let j = i; j >= Math.max(0, i - 3); j--) {
      if (/\btest(\.(only|fixme|skip|fail|slow))?\s*\(/.test(rows[j])) {
        return j + 1;
      }
    }
    return i + 1;
  }
  return null;
}

/**
 * The specs a `playwright.config.ts` diff registers or unregisters, or null
 * when the diff touches anything else (a comment or blank line is neutral).
 */
export function registrationOnly(diffText) {
  const specs = [];
  for (const row of diffText.split("\n")) {
    if (/^(\+\+\+|---|@@|diff |index )/.test(row)) continue;
    if (!/^[+-]/.test(row)) continue;
    const body = row.slice(1).trim();
    if (body === "" || body.startsWith("//")) continue;
    const m = /^"\*\*\/([^"]+\.ts)",?$/.exec(body);
    if (!m) return null;
    if (row.startsWith("+")) specs.push(m[1]);
  }
  return specs;
}

export function select({ repo, base, explain, noFull }) {
  const log = (s) => process.stderr.write(`${s}\n`);
  const { mergeBase, files } = changedFiles(repo, base);
  log(
    `e2e-affected: ${files.length} changed file(s) in ${repo} since ${mergeBase.slice(0, 9)} (merge-base with ${base})`,
  );

  const registered = [];
  const global = files.filter((f) => {
    if (UNIT_TEST.test(f) || !GLOBAL.some((re) => re.test(f))) return false;
    if (f !== "desktop/playwright.config.ts") return true;
    const only = registrationOnly(git(repo, ["diff", mergeBase, "--", f]));
    if (only === null) return true;
    registered.push(...only);
    return false;
  });
  if (global.length > 0) {
    for (const f of global)
      log(`  ${noFull ? "global (ignored)" : "FULL ←"} ${f}`);
    if (!noFull) return { full: true, filters: [] };
  }

  const config = readFileSync(join(desktop, "playwright.config.ts"), "utf8");
  const smoke = smokeSpecs(config);
  const specTexts = new Map();
  for (const name of readdirSync(e2eDir)) {
    if (name.endsWith(".spec.ts")) {
      specTexts.set(name, readFileSync(join(e2eDir, name), "utf8"));
    }
  }
  const specIndex = new Map(
    [...specTexts].map(([n, t]) => [n, specStrings(t)]),
  );

  const selected = new Map(); // spec → reason
  const pick = (spec, why) => {
    if (!selected.has(spec)) selected.set(spec, why);
  };
  const unreached = [];
  const viaImporters = [];
  const offSmoke = new Map();
  let importers = null;
  for (const spec of registered) {
    pick(spec, "registered in desktop/playwright.config.ts");
  }

  for (const f of files) {
    if (!f.startsWith("desktop/") || global.includes(f)) continue;
    const rel = f.slice("desktop/".length);
    if (rel.startsWith("tests/e2e/") && rel.endsWith(".spec.ts")) {
      pick(rel.slice("tests/e2e/".length), `changed: ${f}`);
      continue;
    }
    if (rel.startsWith("tests/helpers/") || rel.startsWith("tests/e2e/")) {
      let n = 0;
      for (const stem of helperClosure(rel)) {
        const re = new RegExp(`["'][./]*[^"']*/${stem}(\\.[a-z]+)?["']`);
        for (const [name, text] of specTexts) {
          if (re.test(text)) {
            pick(name, `imports ${f}`);
            n++;
          }
        }
      }
      if (n === 0) unreached.push(f);
      continue;
    }
    if (!rel.startsWith("src/") || UNIT_TEST.test(rel)) continue;
    if (!SOURCE_EXT.test(rel) && !rel.endsWith(".css")) {
      // JSON, images, SVG: not traced. Say so rather than pass over them.
      unreached.push(`${f} (not a source file; not traced)`);
      continue;
    }

    importers ??= reverseImports();
    const ids = nearestRenderedIds(rel, importers);
    if (existsSync(join(desktop, rel))) {
      ids.texts = textLiteralsIn(readFileSync(join(desktop, rel), "utf8"));
    }
    if (ids.viaImporters) viaImporters.push(f);
    let n = 0;
    for (const [name, index] of specIndex) {
      const hit = specNames(index, ids);
      if (!hit) continue;
      n++;
      if (smoke.has(name)) pick(name, `${f} → ${hit}`);
      else offSmoke.set(name, `${f} → ${hit}`);
    }
    if (n === 0) unreached.push(f);
  }

  const filters = [];
  const whole = new Set();
  for (const [name] of selected) {
    if (smoke.has(name)) {
      whole.add(name);
      filters.push(`tests/e2e/${name}`);
    } else offSmoke.set(name, selected.get(name));
  }
  const byChange = whole.size;
  for (const core of CORE) {
    if (whole.has(core.spec)) continue;
    if (core.title === null) {
      whole.add(core.spec);
      filters.push(`tests/e2e/${core.spec}`);
      continue;
    }
    const line = lineOfTitle(specTexts.get(core.spec) ?? "", core.title);
    if (line === null) {
      log(`  core: "${core.title}" is no longer in ${core.spec}; whole file`);
      whole.add(core.spec);
      filters.push(`tests/e2e/${core.spec}`);
    } else filters.push(`tests/e2e/${core.spec}:${line}`);
  }

  log(
    `  ${byChange} spec file(s) by change + core set (${CORE.length} entries); ${smoke.size} smoke specs in all`,
  );
  if (explain) {
    for (const [name, why] of selected) log(`  ${name} ← ${why}`);
  }
  for (const [name, why] of offSmoke) {
    log(
      `  not run (not a smoke spec — integration needs a relay): ${name} ← ${why}`,
    );
  }
  for (const f of unreached) log(`  no spec reaches ${f}`);
  for (const f of viaImporters) {
    log(`  reached only through its importers' ids (may miss specs): ${f}`);
  }
  return { full: false, filters };
}

/**
 * Test helpers a changed helper reaches: itself, and every helper that
 * imports it, transitively. Returns file stems.
 */
function helperClosure(rel) {
  const dir = join(desktop, "tests/helpers");
  const stemOf = (p) =>
    p
      .split("/")
      .pop()
      .replace(/\.[^.]+$/, "");
  const texts = existsSync(dir)
    ? readdirSync(dir)
        .filter((n) => SOURCE_EXT.test(n))
        .map((n) => [stemOf(n), readFileSync(join(dir, n), "utf8")])
    : [];
  const out = new Set([stemOf(rel)]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const [stem, text] of texts) {
      if (out.has(stem)) continue;
      for (const s of out) {
        if (new RegExp(`["']\\.{1,2}/[^"']*?${s}(\\.[a-z]+)?["']`).test(text)) {
          out.add(stem);
          grew = true;
          break;
        }
      }
    }
  }
  return out;
}

function titlePathOf(suiteTitles, spec) {
  return [...suiteTitles, spec.title].join(" › ");
}

/** Every test outcome in a Playwright JSON report: {spec, title, line, status}. */
export function outcomes(report) {
  const out = [];
  const visit = (suite, titles, file) => {
    const f = suite.file ?? file;
    const here = file === undefined ? [] : [...titles, suite.title];
    for (const spec of suite.specs ?? []) {
      for (const t of spec.tests ?? []) {
        out.push({
          spec: (spec.file ?? f).split("/").pop(),
          title: titlePathOf(here, spec),
          line: spec.line,
          status: t.status, // expected | unexpected | flaky | skipped
        });
      }
    }
    for (const child of suite.suites ?? []) visit(child, here, f);
  };
  for (const s of report.suites ?? []) visit(s, [], undefined);
  return out;
}

/**
 * Why a run cannot be judged by its failures alone, or [] when it can.
 * `exitCodes[i]` is Playwright's exit code for `reports[i]`.
 */
export function untrustworthy(reports, exitCodes = [], core = null) {
  const why = [];
  let total = 0;
  reports.forEach((report, i) => {
    for (const e of report.errors ?? []) {
      const msg = (e.message ?? e.value ?? JSON.stringify(e)).split("\n")[0];
      why.push(`report ${i + 1}: Playwright error: ${msg}`);
    }
    const res = outcomes(report);
    total += res.length;
    const code = exitCodes[i];
    if (
      code !== undefined &&
      code !== 0 &&
      !res.some((r) => r.status === "unexpected")
    ) {
      why.push(
        `report ${i + 1}: Playwright exited ${code} with no failing test to explain it`,
      );
    }
  });
  if (total === 0) why.push("no test ran in any pass");
  if (core) {
    const all = reports.flatMap((r) => outcomes(r));
    for (const c of core) {
      const ran = all.some(
        (r) =>
          r.spec === c.spec &&
          r.status !== "skipped" &&
          (c.title === null || r.title.split(" › ").pop() === c.title),
      );
      if (!ran) {
        why.push(
          `core test did not run: ${c.spec}${c.title ? ` › ${c.title}` : ""}`,
        );
      }
    }
  }
  return why;
}

export function compare(report, known) {
  const key = (x) => `${x.spec}\u0000${x.title}`;
  const knownKeys = new Map(known.map((k) => [key(k), k]));
  const results = outcomes(report);
  const failed = results.filter((r) => r.status === "unexpected");
  const fresh = failed.filter((r) => !knownKeys.has(key(r)));
  const stillKnown = failed.filter((r) => knownKeys.has(key(r)));
  const ran = new Map(results.map((r) => [key(r), r]));
  const nowPassing = known.filter((k) => {
    const r = ran.get(key(k));
    return r && (r.status === "expected" || r.status === "flaky");
  });
  const flaky = results.filter((r) => r.status === "flaky");
  return { results, fresh, stillKnown, nowPassing, flaky };
}

function main(argv) {
  const args = [...argv];
  let mode = "select";
  if (["select", "compare", "port"].includes(args[0])) mode = args.shift();
  const flag = (name) => {
    const i = args.indexOf(name);
    if (i === -1) return null;
    const [, v] = args.splice(i, 2);
    return v;
  };
  const has = (name) => {
    const i = args.indexOf(name);
    if (i === -1) return false;
    args.splice(i, 1);
    return true;
  };

  if (mode === "compare") {
    const codes = flag("--exit-codes");
    const requireCore = has("--require-core");
    if (args.length === 0) {
      console.error(
        "usage: e2e-affected.mjs compare [--exit-codes a,b] [--require-core] <playwright-json-report>...",
      );
      return 2;
    }
    // One report per pass (`smoke`, then `smoke-serial`); a missing or empty
    // one is a pass that never ran, which is not a pass.
    const report = { suites: [] };
    const reports = [];
    for (const path of args) {
      if (!existsSync(path) || statSync(path).size === 0) {
        console.error(`e2e-affected: no Playwright report at ${path}`);
        return 2;
      }
      const one = JSON.parse(readFileSync(path, "utf8"));
      reports.push(one);
      report.suites.push(...(one.suites ?? []));
    }
    const exitCodes = codes === null ? [] : codes.split(",").map(Number);
    const known = JSON.parse(
      readFileSync(join(ownDesktop, "tests/e2e/known-failures.json"), "utf8"),
    ).failures;
    const r = compare(report, known);
    const where = (x) => `${x.spec}:${x.line} › ${x.title}`;
    const counts = { expected: 0, unexpected: 0, flaky: 0, skipped: 0 };
    for (const x of r.results) counts[x.status] = (counts[x.status] ?? 0) + 1;
    console.log(
      `e2e: ${counts.expected} passed, ${counts.unexpected} failed (${r.stillKnown.length} known, ${r.fresh.length} new), ${counts.flaky} flaky, ${counts.skipped} skipped`,
    );
    for (const x of r.fresh) console.log(`  NEW failure      ${where(x)}`);
    for (const x of r.stillKnown) {
      const k = known.find((k) => k.spec === x.spec && k.title === x.title);
      console.log(`  known failure    ${where(x)}  [ledger ${k.ledger}]`);
    }
    for (const x of r.flaky)
      console.log(`  flaky (passed on retry) ${where(x)}`);
    for (const k of r.nowPassing.filter((k) => k.intermittent)) {
      console.log(
        `  intermittent known failure passed this run: ${k.spec} › ${k.title}`,
      );
    }
    for (const k of r.nowPassing.filter((k) => !k.intermittent)) {
      console.log(
        `  known failure PASSED — re-verify and drop it from known-failures.json: ${k.spec} › ${k.title}`,
      );
    }
    const doubts = untrustworthy(reports, exitCodes, requireCore ? CORE : null);
    for (const d of doubts) console.log(`  RUN NOT TRUSTED  ${d}`);
    return r.fresh.length > 0 || doubts.length > 0 ? 1 : 0;
  }

  if (mode === "port") {
    return import("../tests/helpers/previewOrigin.ts").then((m) => {
      console.log(m.PREVIEW_PORT);
      return 0;
    });
  }

  const repo = resolve(flag("--repo") ?? join(ownDesktop, ".."));
  desktop = join(repo, "desktop");
  e2eDir = join(desktop, "tests/e2e");
  const explain = has("--explain");
  const noFull = has("--no-full");
  const base = args[0] ?? nearestMain(repo);
  const { full, filters } = select({ repo, base, explain, noFull });
  if (full) console.log("FULL");
  else for (const f of filters) console.log(f);
  return 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  Promise.resolve(main(process.argv.slice(2))).then((code) => {
    process.exitCode = code;
  });
}
