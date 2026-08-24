import { execFileSync } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";

function git(args, cwd, options = {}) {
  // Git hooks export repository-local GIT_* variables. Child commands that
  // intentionally target `cwd` must not be redirected back to the hook's repo.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith("GIT_")),
  );
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
    env,
    ...options,
  });
}

function toPosixPath(relativePath) {
  return relativePath.split(path.sep).join("/");
}

export function countLines(content) {
  if (content.length === 0) {
    return 0;
  }
  return content.split(/\r?\n/).length;
}

export function allowedLineCount(baseLines, maxLines) {
  return baseLines == null || baseLines <= maxLines ? maxLines : baseLines;
}

export function evaluateFileSize({ baseLines, candidateLines, maxLines }) {
  const limit = allowedLineCount(baseLines, maxLines);
  return { limit, violates: candidateLines > limit };
}

function findRule(rules, relativePath) {
  return rules.find((rule) => relativePath.startsWith(`${rule.root}/`));
}

export function resolveBaseRef(repoRoot, env = process.env) {
  if (env.CHECK_FILE_SIZES_BASE) {
    return env.CHECK_FILE_SIZES_BASE;
  }

  if (env.GITHUB_ACTIONS === "true") {
    return "HEAD^1";
  }

  // Candidates in preference order, not a single hard-coded ref. `origin/main`
  // alone broke the moment `origin` was repointed at the relay's own git
  // hosting and GitHub moved to `upstream`: the ratchet stopped resolving on
  // every branch at once, which also blocks every push, since pre-push runs it.
  // Whatever `main` actually tracks is the honest base, so ask git rather than
  // assume a remote name.
  const candidates = [];
  try {
    candidates.push(
      git(["rev-parse", "--abbrev-ref", "main@{upstream}"], repoRoot).trim(),
    );
  } catch {
    // `main` may have no upstream configured, or may not exist in this
    // checkout. Fall through to the conventional names.
  }
  candidates.push("origin/main", "upstream/main");

  const head = git(["rev-parse", "HEAD"], repoRoot).trim();
  let lastError;
  for (const ref of candidates) {
    if (!ref) continue;
    try {
      const mergeBase = git(["merge-base", ref, "HEAD"], repoRoot).trim();
      return mergeBase === head ? "HEAD" : mergeBase;
    } catch (error) {
      lastError = error;
    }
  }

  throw new Error(
    `Could not resolve the file-size base from any of: ${candidates.join(", ")}. Fetch one of them, point main at a tracking branch, or set CHECK_FILE_SIZES_BASE to an explicit commit.`,
    { cause: lastError },
  );
}

export function parseChangedFiles(output) {
  const fields = output.split("\0");
  const changes = [];

  for (let index = 0; index < fields.length - 1; ) {
    const status = fields[index++];
    if (status.startsWith("R") || status.startsWith("C")) {
      changes.push({
        status: status[0],
        oldPath: fields[index++],
        path: fields[index++],
      });
    } else {
      changes.push({ status: status[0], path: fields[index++] });
    }
  }

  return changes;
}

function changedProjectFiles({ repoRoot, projectRelative, baseRef }) {
  const output = git(
    ["diff", "--name-status", "-z", "-M", baseRef, "--", projectRelative],
    repoRoot,
  );
  const changes = parseChangedFiles(output);
  const trackedPaths = new Set(changes.map((change) => change.path));
  const untracked = git(
    ["ls-files", "--others", "--exclude-standard", "-z", "--", projectRelative],
    repoRoot,
  )
    .split("\0")
    .filter(Boolean);

  for (const filePath of untracked) {
    if (!trackedPaths.has(filePath)) {
      changes.push({ status: "A", path: filePath });
    }
  }
  return changes;
}

function readBaseFile(repoRoot, baseRef, filePath) {
  return git(["show", `${baseRef}:${filePath}`], repoRoot, {
    encoding: null,
  }).toString("utf8");
}

export async function runFileSizeCheck({ projectRoot, rules, label }) {
  // Every governed project is a direct child of the repository root. Derive
  // these paths without Git so hook-provided repository environment variables
  // cannot collapse the project pathspec to an empty string.
  const repoRoot = path.dirname(projectRoot);
  const projectRelative = toPosixPath(path.basename(projectRoot));
  const baseRef = resolveBaseRef(repoRoot);

  // Fail clearly instead of silently turning a missing/shallow base into a pass.
  git(["cat-file", "-e", `${baseRef}^{commit}`], repoRoot);

  const violations = [];
  for (const change of changedProjectFiles({
    repoRoot,
    projectRelative,
    baseRef,
  })) {
    if (change.status === "D") continue;

    const relativePath = toPosixPath(
      path.relative(projectRelative, change.path),
    );
    const rule = findRule(rules, relativePath);
    if (!rule || !rule.extensions.has(path.extname(relativePath))) continue;

    const candidatePath = path.join(repoRoot, change.path);
    const candidateLines = countLines(await fs.readFile(candidatePath, "utf8"));
    const basePath = change.oldPath ?? change.path;
    const baseContent =
      change.status === "A" ? null : readBaseFile(repoRoot, baseRef, basePath);
    const baseLines = baseContent == null ? null : countLines(baseContent);
    const result = evaluateFileSize({
      baseLines,
      candidateLines,
      maxLines: rule.maxLines,
    });

    if (result.violates) {
      violations.push({
        relativePath,
        baseLines,
        candidateLines,
        limit: result.limit,
      });
    }
  }

  if (violations.length === 0) return;

  console.error(`${label} file size ratchet failed (base ${baseRef}):`);
  for (const violation of violations) {
    const before = violation.baseLines == null ? "new" : violation.baseLines;
    const delta =
      violation.baseLines == null
        ? ""
        : ` (${violation.candidateLines - violation.baseLines >= 0 ? "+" : ""}${violation.candidateLines - violation.baseLines})`;
    console.error(
      `- ${violation.relativePath}: ${before} -> ${violation.candidateLines}${delta} lines (allowed ${violation.limit})`,
    );
  }
  console.error(
    "Keep new files at or below the limit; files already over it may not grow.",
  );
  process.exitCode = 1;
}
