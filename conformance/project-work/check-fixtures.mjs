#!/usr/bin/env node
// Fixture well-formedness check for conformance/project-work.
//
// This is NOT the contract's validator and NOT the fold. Those are lane W1's,
// in buzz-core, and these fixtures are what they bind to. This script only
// asserts that the fixtures themselves are well formed — every file parses,
// every id has the right shape and length, every tag agrees with its content,
// every sequence's expected output refers to events that exist — so a later
// edit cannot silently break them.
//
// Run: node conformance/project-work/check-fixtures.mjs
// Wired into no CI recipe on purpose (W0 owns this directory alone).

import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIX = join(HERE, "fixtures");

const SCHEMA = "buzz-project-work/v1";
const KIND = 44249;
const MAX_PLAN_BYTES = 65536;
const MAX_CRITERIA = 64;
const MAX_CONTENT_BYTES = 16384;
const SLUG = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/;
const HEX64 = /^[0-9a-f]{64}$/;
const COMMIT = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const COORD = /^30621:[0-9a-f]{64}:[A-Za-z0-9._-]{1,128}$/;
const REPO_COORD = /^30617:[0-9a-f]{64}:[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;
const KIND_GIT_REPO_STATE = 30618;
const TYPES = ["work.declared", "work.assignment_bound", "work.evidence_bound"];
const EVIDENCE_KINDS = ["report", "verdict", "action_result", "ref_observation"];
const PLAN_KEYS = [
  "schema", "id", "status", "title",
  "code_repository", "delivery_ref", "criteria", "retired_criteria",
];

const failures = [];
const fail = (where, message) => failures.push(`${where}: ${message}`);
const check = (cond, where, message) => {
  if (!cond) fail(where, message);
  return cond;
};

// ---------------------------------------------------------------- plan files
// Deliberately line-based, not a YAML parser: the parser is W1's, and a second
// parser here would be a second opinion nobody asked for.
function readPlan(path) {
  const text = readFileSync(path, "utf8");
  const bytes = Buffer.byteLength(text);
  const lines = text.split("\n");
  if (lines[0] !== "---") return { bytes, error: "no opening --- on line 1" };
  const end = lines.indexOf("---", 1);
  if (end < 0) return { bytes, error: "frontmatter is never closed" };
  const fm = lines.slice(1, end);
  const topKeys = [];
  const criteria = [];
  const refusals = [];
  let retired = null;
  let current = null;
  for (const line of fm) {
    if (/^\s*#/.test(line)) {
      const m = line.match(/#\s*REFUSED:\s*(.+)$/);
      if (m) refusals.push(m[1].trim());
      continue;
    }
    const top = line.match(/^([a-z_]+):(.*)$/);
    if (top) {
      topKeys.push(top[1]);
      if (top[1] === "retired_criteria") retired = top[2].trim();
      current = null;
      continue;
    }
    const id = line.match(/^\s+-\s+id:\s*(.*)$/);
    if (id) {
      current = { id: id[1].trim(), accept: null, proof: null };
      criteria.push(current);
      continue;
    }
    const accept = line.match(/^\s+accept:\s*(.*)$/);
    if (accept && current) current.accept = accept[1].trim();
    const proof = line.match(/^\s+proof:\s*(.*)$/);
    if (proof && current) current.proof = proof[1].trim();
  }
  return { text, bytes, topKeys, criteria, retired, refusals };
}

function checkValidPlan(path) {
  const where = `plans/valid/${path.split("/").pop()}`;
  const plan = readPlan(path);
  if (plan.error) return fail(where, plan.error);
  check(plan.bytes <= MAX_PLAN_BYTES, where, `${plan.bytes} bytes exceeds the ${MAX_PLAN_BYTES}-byte ceiling`);
  for (const key of PLAN_KEYS) {
    check(plan.topKeys.includes(key), where, `missing frontmatter key "${key}"`);
  }
  for (const key of plan.topKeys) {
    check(PLAN_KEYS.includes(key), where, `unknown frontmatter key "${key}"`);
  }
  check(plan.criteria.length >= 1, where, "no criteria");
  check(plan.criteria.length <= MAX_CRITERIA, where, `${plan.criteria.length} criteria exceeds ${MAX_CRITERIA}`);
  check(plan.refusals.length === 0, where, "a valid plan must carry no REFUSED marker");
  const seen = new Set();
  const retired = (plan.retired ?? "[]").replace(/[[\]]/g, "").split(",").map((s) => s.trim()).filter(Boolean);
  for (const c of plan.criteria) {
    check(SLUG.test(c.id), where, `criterion id "${c.id}" is not slug grammar`);
    check(Buffer.byteLength(c.id) <= 64, where, `criterion id "${c.id}" exceeds 64 bytes`);
    check(!seen.has(c.id), where, `duplicate criterion id "${c.id}"`);
    seen.add(c.id);
    check(!retired.includes(c.id), where, `criterion id "${c.id}" is both active and retired`);
    check(c.accept !== null && c.accept !== "" && !/^"\s*"$/.test(c.accept), where, `criterion "${c.id}" has empty accept text`);
    check(c.proof !== null, where, `criterion "${c.id}" has no proof`);
    if (c.proof) {
      const ok =
        c.proof === "{kind: review}" ||
        c.proof === "{kind: git-ref}" ||
        /^\{kind: action, name: [a-z0-9-]+, step: [a-z0-9-]+\}$/.test(c.proof);
      check(ok, where, `criterion "${c.id}" has a proof that is not one of the three forms: ${c.proof}`);
    }
  }
  return seen;
}

function checkInvalidPlan(path) {
  const name = path.split("/").pop();
  const where = `plans/invalid/${name}`;
  const plan = readPlan(path);
  if (plan.error) return fail(where, plan.error);
  check(plan.refusals.length === 1, where, "an invalid plan must carry exactly one # REFUSED: marker naming its reason");
  // Assert the defect is still present, so nobody repairs a fixture by hand
  // and leaves the refusal marker lying about what the file is.
  const ids = plan.criteria.map((c) => c.id);
  const retired = (plan.retired ?? "[]").replace(/[[\]]/g, "").split(",").map((s) => s.trim()).filter(Boolean);
  switch (name) {
    case "duplicate-id.md":
      check(new Set(ids).size < ids.length, where, "no duplicate criterion id present");
      break;
    case "recycled-retired-id.md":
      check(ids.some((id) => retired.includes(id)), where, "no active/retired overlap present");
      break;
    case "unknown-key.md":
      check(plan.topKeys.some((k) => !PLAN_KEYS.includes(k)), where, "no unknown frontmatter key present");
      break;
    case "empty-accept.md":
      check(plan.criteria.some((c) => c.accept === null || /^"?\s*"?$/.test(c.accept)), where, "no empty accept present");
      break;
    case "absolute-path-in-action-name.md":
      check(plan.criteria.some((c) => (c.proof ?? "").includes("/")), where, "no path-shaped action name present");
      break;
    case "oversize.md":
      check(plan.bytes > MAX_PLAN_BYTES, where, `${plan.bytes} bytes does not exceed the ceiling`);
      break;
    default:
      fail(where, "unknown invalid-plan fixture: add its defect assertion to check-fixtures.mjs");
  }
}

// -------------------------------------------------------------- work records
const REASON_CODES = [
  "evidence_unavailable", "wrong_signer", "wrong_run_or_hash", "action_failed",
  "dirty_revision", "revision_mismatch", "not_approving",
  "report_not_canonical", "disposition_not_canonical",
  "bound_to_superseded_declaration", "ref_observation_superseded", "plan_unreadable",
];
const STATE_REASON_CODES = ["superseded", "conflict", "goal_changed"];
const COVERAGE_REASON_CODES = [
  "criteria_not_covered", "mixed_artifacts", "conflict", "superseded", "plan_unavailable",
];
// `<30617 coordinate>@<commit>#<action name>`: a definition never loses the
// repository and plan commit it was compiled at.
const ACTION_DEF_KEY =
  /^30617:[0-9a-f]{64}:[a-z0-9-]+@(?:[0-9a-f]{40}|[0-9a-f]{64})#[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/;
const EXCLUSION_CODES = ["signer_not_may_lead", "goal_ref_not_a_goal"];

const KEYS = {
  "work.declared": ["workId", "goalRef", "decisionRef", "responsibleActor", "planRef", "supersedes"],
  "work.assignment_bound": ["declarationRef", "criterionIds", "assignmentRef", "replacesBinding"],
  "work.evidence_bound": ["declarationRef", "criterionIds", "artifactCommit", "evidenceRefs", "completionRef"],
};

function checkEvent(event, where) {
  const before = failures.length;
  check(event && typeof event === "object", where, "not an object");
  if (!event || typeof event !== "object") return false;
  check(HEX64.test(event.id ?? ""), where, "id is not 64 lowercase hex");
  check(HEX64.test(event.pubkey ?? ""), where, "pubkey is not 64 lowercase hex");
  check(Number.isInteger(event.created_at), where, "created_at is not an integer");
  check(event.kind === KIND, where, `kind is ${event.kind}, not ${KIND}`);
  check(Buffer.byteLength(event.content ?? "") <= MAX_CONTENT_BYTES, where, "content exceeds 16 KiB");
  let p;
  try {
    p = JSON.parse(event.content);
  } catch (e) {
    fail(where, `content is not JSON: ${e.message}`);
    return false;
  }
  check(p.schema === SCHEMA, where, `schema is "${p.schema}"`);
  check(UUID.test(p.sessionRef ?? ""), where, "sessionRef is not a uuid");
  check(HEX64.test(p.genesisRef ?? ""), where, "genesisRef is not 64 hex");
  check(COORD.test(p.projectRef ?? ""), where, `projectRef is not a canonical coordinate: ${p.projectRef}`);
  check(TYPES.includes(p.type), where, `type is "${p.type}"`);
  check(Object.keys(p).length === 6, where, `payload has ${Object.keys(p).length} keys, not 6`);

  const tags = event.tags ?? [];
  check(tags.length === 6, where, `${tags.length} tags, not 6`);
  check(tags.every((t) => Array.isArray(t) && t.length === 2), where, "a tag is not a two-field tag");
  const expect = [
    ["h", UUID],
    ["d", p.sessionRef],
    ["a", p.projectRef],
    ["pwk-v", SCHEMA],
    ["pwk-genesis", p.genesisRef],
    ["pwk-type", p.type],
  ];
  expect.forEach(([name, want], i) => {
    const tag = tags[i];
    if (!Array.isArray(tag) || tag.length !== 2) return;
    check(tag[0] === name, where, `tag ${i} is "${tag[0]}", expected "${name}"`);
    if (want instanceof RegExp) check(want.test(tag[1]), where, `tag "${name}" value is malformed`);
    else check(tag[1] === want, where, `tag "${name}" does not match content (${tag[1]} vs ${want})`);
  });

  const body = p.body;
  if (body && typeof body === "object" && KEYS[p.type]) {
    const want = KEYS[p.type];
    check(Object.keys(body).length === want.length, where, `body has keys [${Object.keys(body)}], expected [${want}]`);
    for (const k of want) check(k in body, where, `body is missing "${k}" (nullable keys are written as null, never absent)`);
  } else {
    fail(where, "body is not an object");
    return false;
  }
  if (p.type === "work.declared") {
    check(UUID.test(body.workId ?? ""), where, "workId is not a uuid");
    check(HEX64.test(body.goalRef ?? ""), where, "goalRef is not 64 hex");
    check(body.decisionRef === null || HEX64.test(body.decisionRef ?? ""), where, "decisionRef must be null or 64 hex");
    check(HEX64.test(body.responsibleActor ?? ""), where, "responsibleActor is not 64 hex");
    const r = body.planRef ?? {};
    check(Object.keys(r).length === 3, where, "planRef must have exactly repository, commit, path");
    check(REPO_COORD.test(r.repository ?? ""), where, `planRef.repository is not a full 30617 coordinate: ${r.repository}`);
    check(COMMIT.test(r.commit ?? ""), where, `planRef.commit is not a full 40/64 hex commit: ${r.commit}`);
    check(typeof r.path === "string" && r.path.startsWith("plans/") && !r.path.includes(".."), where, `planRef.path is not a plans/ path: ${r.path}`);
    check(Array.isArray(body.supersedes) && body.supersedes.every((x) => HEX64.test(x)), where, "supersedes is not a list of event ids");
    check((body.supersedes ?? []).length <= 8, where, "supersedes exceeds 8 entries");
    check(!(body.supersedes ?? []).includes(event.id), where, "a declaration cannot supersede its own event id");
  } else {
    check(HEX64.test(body.declarationRef ?? ""), where, "declarationRef is not 64 hex");
    const ids = body.criterionIds;
    check(Array.isArray(ids) && ids.length >= 1, where, "criterionIds must name at least one criterion");
    if (Array.isArray(ids)) {
      check(ids.length <= MAX_CRITERIA, where, `criterionIds exceeds ${MAX_CRITERIA}`);
      check(new Set(ids).size === ids.length, where, "criterionIds repeats an id");
      for (const id of ids) check(SLUG.test(id), where, `criterion id "${id}" is not slug grammar`);
    }
  }
  if (p.type === "work.assignment_bound") {
    check(HEX64.test(body.assignmentRef ?? ""), where, "assignmentRef is not 64 hex");
    check(body.replacesBinding === null || HEX64.test(body.replacesBinding ?? ""), where, "replacesBinding must be null or 64 hex");
  }
  if (p.type === "work.evidence_bound") {
    check(COMMIT.test(body.artifactCommit ?? ""), where, `artifactCommit is not 40/64 hex: ${body.artifactCommit}`);
    const refs = body.evidenceRefs;
    check(Array.isArray(refs) && refs.length >= 1, where, "evidenceRefs must carry at least one reference");
    check(!Array.isArray(refs) || refs.length <= 32, where, "evidenceRefs exceeds 32 entries");
    for (const r of refs ?? []) {
      check(r && Object.keys(r).length === 2, where, "an evidence ref must be exactly {kind, eventId}");
      check(EVIDENCE_KINDS.includes(r?.kind), where, `evidence kind "${r?.kind}" is not one of ${EVIDENCE_KINDS.join("|")}`);
      check(HEX64.test(r?.eventId ?? ""), where, "an evidence eventId is not 64 hex");
    }
    check(body.completionRef === null || HEX64.test(body.completionRef ?? ""), where, "completionRef must be null or 64 hex");
  }
  return failures.length === before;
}

function checkRecords() {
  for (const name of readdirSync(join(FIX, "records/valid")).sort()) {
    const where = `records/valid/${name}`;
    const event = JSON.parse(readFileSync(join(FIX, "records/valid", name), "utf8"));
    checkEvent(event, where);
  }
  for (const name of readdirSync(join(FIX, "records/invalid")).sort()) {
    const where = `records/invalid/${name}`;
    const file = JSON.parse(readFileSync(join(FIX, "records/invalid", name), "utf8"));
    if (!check(typeof file.refusal === "string" && file.refusal.length > 0, where, "no refusal reason")) continue;
    if (!check(file.event && typeof file.event === "object", where, "no event")) continue;
    // The shape checks above must reject it: a refusal fixture that passes is
    // a fixture that stopped testing anything.
    const before = failures.length;
    checkEvent(file.event, where);
    if (failures.length === before) fail(where, "this refusal fixture passes every shape check — it no longer refuses anything");
    else failures.length = before;
  }
}

// ------------------------------------------------------------------ sequences
const DECLARATION_KEYS = [
  "workId", "declarationRef", "planRef", "state", "supersedes", "supersededBy",
  "stateReasonCode", "stateReason", "planResolved", "candidateArtifact",
  "artifactCommits", "criteria", "coverageComplete", "coverageReasonCode",
  "coverageReason",
].join(",");
const CRITERION_KEYS = [
  "criterionId", "proof", "status", "assignmentRefs", "evidence",
  "artifactCommit", "reasonCode", "reason",
].join(",");
const keysOf = (o) => Object.keys(o).join(",");
const short = (id) => `${id.slice(0, 8)}…`;

// ------------------------------------------------- the branch reason templates
// § (c) "The exact reason strings" carries one row per *branch* — one code path
// producing one sentence — not one row per code. A code with four ways to fail
// gets four rows, because forcing four facts into one sentence is how
// `wrong_run_or_hash` came to claim a hash mismatch for an unsigned echo (A7.4).
//
// Every placeholder has a shape, so a template is a regex and "matches exactly"
// means exactly: anchored, whole string, no leftovers.
const PLACEHOLDERS = {
  id: "[0-9a-f]{8}…",
  sha: "[0-9a-f]{12}…",
  code: "-?[0-9]+",
  decision: "[a-z][a-z-]*",
  subtype: "[a-z][a-z-]*",
  action: "[a-z0-9][a-z0-9-]*",
  step: "[a-z0-9][a-z0-9-]*",
  repo: "[A-Za-z0-9._-]+",
  branch: "[A-Za-z0-9._/-]+",
};
const TEMPLATE_MARKER = "<!-- check-fixtures: reason-templates -->";

function templateRegex(template, where) {
  let pattern = "^";
  for (const [index, part] of template.split(/[<>]/).entries()) {
    if (index % 2 === 0) {
      pattern += part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      continue;
    }
    const shape = PLACEHOLDERS[part];
    if (!shape) {
      fail(where, `template placeholder <${part}> has no declared shape in check-fixtures.mjs`);
      return null;
    }
    pattern += `(?:${shape})`;
  }
  return new RegExp(`${pattern}$`);
}

/// Parse the README's branch table: `| reasonCode | branch | reason |`.
function readReasonTemplates() {
  const where = "README.md § (c) reason templates";
  const text = readFileSync(join(HERE, "README.md"), "utf8");
  const at = text.indexOf(TEMPLATE_MARKER);
  if (!check(at >= 0, where, `no ${TEMPLATE_MARKER} in README.md`)) return [];
  const rows = [];
  for (const line of text.slice(at).split("\n").slice(1)) {
    if (!line.startsWith("|")) {
      if (rows.length > 0) break;
      continue;
    }
    const cells = line.replace(/^\|/, "").replace(/\|\s*$/, "").split("|").map((c) => c.trim());
    if (cells.length !== 3 || /^-+$/.test(cells[0]) || cells[0] === "`reasonCode`") continue;
    const code = cells[0].replace(/`/g, "");
    const branch = cells[1].replace(/`/g, "");
    const template = cells[2].replace(/^`|`$/g, "");
    check(REASON_CODES.includes(code), where, `template row names unknown reasonCode "${code}"`);
    check(/^[a-z0-9-]+$/.test(branch), where, `branch name "${branch}" is not a slug`);
    const regex = templateRegex(template, where);
    if (regex) rows.push({ code, branch, template, regex });
  }
  check(rows.length > 0, where, "the reason-template table is empty");
  const ids = rows.map((r) => `${r.code}/${r.branch}`);
  check(new Set(ids).size === ids.length, where, "two template rows name the same code and branch");
  return rows;
}

const TEMPLATES = readReasonTemplates();
// `${code}/${branch}` -> the sequence that first produced it.
const exercisedBranches = new Map();

/// Every rule a coverage document must satisfy on its own. `ctx` supplies the
/// sequence's events and inputs when there are any; the README's embedded
/// example is checked without one, so the two cannot drift apart in key names
/// or spellings.
function checkFold(fold, where, planCriteria, ctx) {
  const projection = ctx?.inputs?.teamProjection ?? { includedEventIds: [], assignments: {} };
    check(fold.schema === "buzz-project-work-coverage/v1", where, "expected-fold schema is wrong");
    check(Array.isArray(fold.declarations), where, "declarations is not an array");
    check(fold.declarations.length > 0 || fold.excluded.length > 0, where, "an expected fold with no declarations must say what it excluded");
    for (const d of fold.declarations) {
      if (ctx) check(ctx.ids.has(d.declarationRef), where, `expected fold names declaration ${d.declarationRef} which is not in events.json`);
      check(keysOf(d) === DECLARATION_KEYS, where, `declaration ${d.declarationRef} has keys [${Object.keys(d)}], expected [${DECLARATION_KEYS}]`);
      check(["head", "superseded", "stale", "conflict"].includes(d.state), where, `unknown declaration state "${d.state}"`);
      check(Array.isArray(d.supersedes) && d.supersedes.every((x) => HEX64.test(x)), where, "supersedes is not a list of event ids");
      check(Array.isArray(d.supersededBy) && d.supersededBy.every((x) => HEX64.test(x)), where, "supersededBy is not a list of event ids");
      // `supersedes` is reported as recorded, whatever the state; `supersededBy`
      // is its derived inverse over the projected set.
      for (const older of d.supersedes) {
        const target = fold.declarations.find((x) => x.declarationRef === older);
        if (target) {
          check(target.supersededBy.includes(d.declarationRef), where,
            `${short(d.declarationRef)} supersedes ${short(older)}, which does not list it in supersededBy`);
        }
      }
      for (const newer of d.supersededBy) {
        const source = fold.declarations.find((x) => x.declarationRef === newer);
        if (source) {
          check(source.supersedes.includes(d.declarationRef), where,
            `${short(d.declarationRef)} claims to be superseded by ${short(newer)}, whose recorded supersedes does not name it`);
        }
      }
      check((d.state === "head") === (d.stateReason === null), where, `a ${d.state} declaration must state its reason (and a head must not)`);
      const covering = d.criteria.filter((c) => c.status === "covered");
      const commits = [...new Set(covering.map((c) => c.artifactCommit))].sort();
      check(JSON.stringify(commits) === JSON.stringify([...d.artifactCommits].sort()), where, `artifactCommits ${d.artifactCommits} does not list the covering evidence's commits ${commits}`);
      // There is no candidate until delivery is observed: a plan with a
      // `git-ref` criterion that is not covered has `candidateArtifact: null`,
      // however many other criteria are covered.
      const gitRefRow = d.criteria.find((c) => c.proof?.kind === "git-ref");
      if (gitRefRow && gitRefRow.status !== "covered") {
        check(d.candidateArtifact === null, where,
          `candidateArtifact is set while the git-ref criterion "${gitRefRow.criterionId}" is ${gitRefRow.status}: there is no candidate until delivery is observed`);
      } else if (covering.length > 0) {
        check(d.candidateArtifact !== null, where, "covered criteria with no candidate artifact");
        if (gitRefRow) check(d.candidateArtifact === gitRefRow.artifactCommit, where, "the candidate is not the git-ref evidence's commit");
      }
      if (commits.length > 1) {
        check(d.coverageReasonCode === "mixed_artifacts", where, "covering evidence at several commits must read mixed_artifacts");
        check(d.coverageComplete === false, where, "mixed artifacts cannot be complete coverage");
      }
      check(typeof d.coverageComplete === "boolean", where, "coverageComplete is not a boolean");
      check(d.candidateArtifact === null || COMMIT.test(d.candidateArtifact), where, "candidateArtifact is neither null nor a 40/64-hex commit");
      check(Array.isArray(d.artifactCommits) && d.artifactCommits.every((c) => COMMIT.test(c)), where, "artifactCommits is not a list of commits");
      check(d.coverageReasonCode === null || COVERAGE_REASON_CODES.includes(d.coverageReasonCode), where, `unknown coverageReasonCode "${d.coverageReasonCode}"`);
      check((d.coverageReasonCode === null) === (d.coverageReason === null), where, "coverageReasonCode and coverageReason must agree about being absent");
      check(d.coverageComplete === (d.coverageReasonCode === null), where, "an incomplete declaration must name why, and a complete one must not");
      check(d.stateReasonCode === null || STATE_REASON_CODES.includes(d.stateReasonCode), where, `unknown stateReasonCode "${d.stateReasonCode}"`);
      check((d.stateReasonCode === null) === (d.stateReason === null), where, "stateReasonCode and stateReason must agree about being absent");
      check(!("missionTerminal" in d) && !("mission" in fold), where, "coverage output must not carry a mission terminal field: two questions, never merged");
      if ((d.state === "head" || d.state === "stale") && d.planResolved) {
        check(d.criteria.length > 0, where, "a head declaration must project its criteria");
      } else if (d.state === "head" || d.state === "stale") {
        // Finding 5: no criterion rows must never read as nothing to do. An
        // unresolved plan may still project the criteria its *bindings* named
        // — every one `unknown`/`plan_unreadable` with no proof form, because
        // the fold knows the id was claimed and not what it requires — and the
        // declaration still says `plan_unavailable` either way.
        check(d.coverageReasonCode === "plan_unavailable", where,
          "a head whose plan blob is unavailable must say plan_unavailable at the declaration level");
        check(d.coverageComplete === false, where, "an un-evaluable declaration is never complete");
        for (const c of d.criteria) {
          check(c.proof === null, where,
            `criterion "${c.criterionId}" carries a proof form while the plan blob is unavailable: the fold cannot know it`);
          check(c.status === "unknown" && c.reasonCode === "plan_unreadable", where,
            `criterion "${c.criterionId}" under an unreadable plan is ${c.status}/${c.reasonCode}, not unknown/plan_unreadable`);
          check(c.assignmentRefs.length === 0 && c.evidence.length === 0 && c.artifactCommit === null, where,
            `criterion "${c.criterionId}" under an unreadable plan reports bindings it cannot evaluate`);
        }
      } else {
        check(d.criteria.length === 0, where, `a ${d.state} declaration projects no criteria`);
      }
      for (const c of d.criteria) {
        check(keysOf(c) === CRITERION_KEYS, where, `criterion ${c.criterionId} has keys [${Object.keys(c)}], expected [${CRITERION_KEYS}]`);
        check(SLUG.test(c.criterionId), where, `criterion id "${c.criterionId}" is not slug grammar`);
        check(Array.isArray(c.assignmentRefs) && c.assignmentRefs.every((x) => HEX64.test(x)), where, `criterion "${c.criterionId}" has a malformed assignmentRefs`);
        check(planCriteria.has(c.criterionId), where, `criterion "${c.criterionId}" is not in the plan fixture`);
        check(["open", "covered", "stale", "unknown"].includes(c.status), where, `unknown criterion status "${c.status}"`);
        check(c.status !== "covered" || c.evidence.length > 0, where, `criterion "${c.criterionId}" is covered with no evidence`);
        check(c.reasonCode === null || REASON_CODES.includes(c.reasonCode), where, `criterion "${c.criterionId}" has unknown reasonCode "${c.reasonCode}"`);
        check((c.reasonCode === null) === (c.reason === null), where, `criterion "${c.criterionId}": reasonCode and reason must agree about being absent`);
        check(c.status !== "covered" || c.reasonCode === null, where, `criterion "${c.criterionId}" is covered and still names a reason`);
        check(c.status === "covered" || c.evidence.length === 0 || c.reasonCode !== null, where, `criterion "${c.criterionId}" is ${c.status} with evidence and no reason: say which predicate failed`);
        check(c.status !== "unknown" || c.reasonCode !== null, where, `criterion "${c.criterionId}" is unknown with no reason`);
        if (c.reasonCode !== null && c.reason !== null) {
          const candidates = TEMPLATES.filter((t) => t.code === c.reasonCode);
          const hit = candidates.filter((t) => t.regex.test(c.reason));
          if (check(hit.length > 0, where,
            `criterion "${c.criterionId}": ${c.reasonCode} reason ${JSON.stringify(c.reason)} follows none of the ${candidates.length} template(s) in the README's table`)) {
            check(hit.length === 1, where,
              `criterion "${c.criterionId}": ${c.reasonCode} reason ${JSON.stringify(c.reason)} matches ${hit.length} templates (${hit.map((t) => t.branch).join(", ")}); a branch's sentence must identify its branch`);
            for (const t of hit) {
              if (!exercisedBranches.has(`${t.code}/${t.branch}`)) {
                exercisedBranches.set(`${t.code}/${t.branch}`, where);
              }
            }
          }
        }
        check(c.status !== "covered" || COMMIT.test(c.artifactCommit ?? ""), where, `criterion "${c.criterionId}" is covered with no artifact commit`);
        for (const e of c.evidence) {
          check(EVIDENCE_KINDS.includes(e.kind) && HEX64.test(e.eventId), where, "malformed evidence ref in expected fold");
          const known = !ctx ? true : e.kind === "ref_observation"
            ? (ctx.inputs.refStates ?? []).some((r) => r.id === e.eventId)
            : Boolean((ctx.inputs.evidence ?? {})[e.eventId]);
          if (ctx && !known) {
            check(c.status === "unknown" && c.reasonCode === "evidence_unavailable", where,
              `criterion "${c.criterionId}" names evidence ${e.eventId} that no input supplies, so it must be unknown/evidence_unavailable`);
          }
          if (ctx && known && e.kind !== "ref_observation") {
            check((ctx.inputs.evidence[e.eventId].kind) === e.kind, where, `evidence ${e.eventId} is bound as ${e.kind} but supplied as ${ctx.inputs.evidence[e.eventId].kind}`);
          }
          // A7.4: coverage is a POSITIVE proof. An empty canonical projection
          // establishes nothing, so it can never leave a criterion covered —
          // "empty" is unproved, never permission to omit the predicate.
          if (ctx && known && c.status === "covered" && (e.kind === "verdict" || e.kind === "report")) {
            check(projection.includedEventIds.length > 0, where,
              `criterion "${c.criterionId}" is covered while the team projection includes no records at all: empty is unproved`);
          }
          if (ctx && known && c.status === "covered" && e.kind === "verdict") {
            const f = ctx.inputs.evidence[e.eventId];
            check(["approve", "approve-with-notes"].includes(f.decision), where, `criterion "${c.criterionId}" is covered by a ${f.decision} disposition`);
            check(ctx.mayLead(f.signer), where, `criterion "${c.criterionId}" is covered by a verdict from an actor who does not satisfy may_lead`);
            // Finding 6: only the canonical team projection's records count.
            check(projection.includedEventIds.includes(f.eventId), where,
              `criterion "${c.criterionId}" is covered by disposition ${short(f.eventId)}, which the team projection excludes`);
          }
          if (ctx && known && c.status === "covered" && e.kind === "report") {
            const f = ctx.inputs.evidence[e.eventId];
            check(f.headSha === c.artifactCommit, where, `criterion "${c.criterionId}" is covered by a report about another revision`);
            check(projection.includedEventIds.includes(f.eventId), where,
              `criterion "${c.criterionId}" is covered by report ${short(f.eventId)}, which the team projection excludes`);
            const asg = (projection.assignments ?? {})[f.assignmentRef];
            check(Boolean(asg) && asg.assigneeActor === f.signer, where,
              `criterion "${c.criterionId}" is covered by a report signed by ${short(f.signer)}, who is not assignment ${short(f.assignmentRef)}'s assignee`);
            check(c.assignmentRefs.includes(f.assignmentRef), where,
              `criterion "${c.criterionId}" is covered by a report on assignment ${short(f.assignmentRef)}, which is not bound to it under this declaration`);
          }
          if (ctx && known && c.status === "covered" && e.kind === "action_result") {
            const f = ctx.inputs.evidence[e.eventId];
            // Finding 7: only the definitions compiled at THIS declaration's
            // repository and plan commit may satisfy its action criteria.
            const key = `${d.planRef.repository}@${d.planRef.commit}#${f.actionName}`;
            const def = (ctx.inputs.actionDefinitions ?? {})[key];
            check(Boolean(def) && def.definitionHash === f.definitionHash, where, `criterion "${c.criterionId}" is covered by a run of another definition than the one compiled at ${d.planRef.commit.slice(0, 12)}…`);
            check(Boolean(def) && def.steps.includes(f.stepId), where, `criterion "${c.criterionId}" is covered by a step the definition does not define`);
            check(f.echoSigner === ctx.inputs.relaySelfKey, where,
              `criterion "${c.criterionId}" is covered by a host result the relay did not echo`);
            check(f.exitCode === 0 && f.dirty === false && f.checkout.dirtyBefore === false, where, `criterion "${c.criterionId}" is covered by a failed or dirty run`);
            check(f.checkout.sha === c.artifactCommit, where, `criterion "${c.criterionId}" is covered by a run on another commit`);
          }
        }
      }
      if (d.coverageComplete) {
        check(d.state === "head", where, "coverageComplete on a declaration that is not the head");
        check(d.criteria.every((c) => c.status === "covered"), where, "coverageComplete with an uncovered criterion");
        check(fold.conflicts.length === 0, where, "coverageComplete while a conflict is recorded");
      }
    }
    for (const x of fold.excluded ?? []) {
      if (ctx) check(ctx.ids.has(x.eventId), where, `excluded event ${x.eventId} is not in events.json`);
      check(EXCLUSION_CODES.includes(x.code), where, `unknown exclusion code "${x.code}"`);
    }
    for (const c of fold.conflicts ?? []) {
      check(Array.isArray(c.heads) && c.heads.length >= 2, where, "a conflict must name at least two heads");
      for (const h of c.heads) if (ctx) check(ctx.ids.has(h), where, `conflict head ${h} is not in events.json`);
      const conflicted = fold.declarations.filter((d) => d.workId === c.workId && d.state === "conflict").map((d) => d.declarationRef).sort();
      check(JSON.stringify(conflicted) === JSON.stringify([...c.heads].sort()), where,
        "the conflict's heads must be exactly the declarations projected as conflict: a head is a maximal declaration, not a direct sibling");
    }
}

function checkSequences(planCriteria) {
  for (const name of readdirSync(join(FIX, "sequences")).sort()) {
    const dir = join(FIX, "sequences", name);
    const where = `sequences/${name}`;
    const events = JSON.parse(readFileSync(join(dir, "events.json"), "utf8"));
    const fold = JSON.parse(readFileSync(join(dir, "expected-fold.json"), "utf8"));
    const inputs = JSON.parse(readFileSync(join(dir, "inputs.json"), "utf8"));
    check(Array.isArray(events) && events.length > 0, where, "events.json is not a non-empty array");
    const ids = new Set();
    const times = new Set();
    for (const [i, event] of events.entries()) {
      checkEvent(event, `${where}/events[${i}]`);
      check(Number.isInteger(event.created_at), `${where}/events[${i}]`, "no fixed timestamp");
      times.add(`${event.id}:${event.created_at}`);
      ids.add(event.id);
    }
    check(times.size === events.length, where, "two events share an id and timestamp");
    const auth = inputs.authority ?? {};
    check(HEX64.test(auth.founderPubkey ?? ""), where, "authority.founderPubkey is not 64 hex");
    for (const seat of auth.activeSeats ?? []) {
      check(HEX64.test(seat.actorPubkey ?? ""), where, "an active seat actorPubkey is not 64 hex");
      check(typeof seat.role === "string" && seat.role.length > 0, where, "an active seat has no role");
    }
    for (const g of auth.activeGrants ?? []) {
      check(HEX64.test(g.actorPubkey ?? ""), where, "a grant actorPubkey is not 64 hex");
      check(HEX64.test(g.grantEventRef ?? ""), where, "a grant grantEventRef is not 64 hex");
      check(typeof g.maySteer === "boolean", where, "a grant does not say whether it may steer");
    }
    const mayLead = (pubkey) =>
      pubkey === auth.founderPubkey ||
      (auth.activeSeats ?? []).some((x) => x.actorPubkey === pubkey && x.role === "lead") ||
      (auth.activeGrants ?? []).some((x) => x.actorPubkey === pubkey && x.maySteer);
    for (const [key, def] of Object.entries(inputs.actionDefinitions ?? {})) {
      check(ACTION_DEF_KEY.test(key), where, `action definition key "${key}" is not <30617 coordinate>@<commit>#<action>`);
      check(HEX64.test(def.definitionHash ?? ""), where, `actionDefinitions["${key}"].definitionHash is not 64 hex`);
      check(Array.isArray(def.steps) && def.steps.length > 0 && def.steps.every((x) => SLUG.test(x)), where, `actionDefinitions["${key}"].steps is not a list of step slugs`);
    }
    const tp = inputs.teamProjection ?? {};
    check(Array.isArray(tp.includedEventIds) && tp.includedEventIds.every((x) => HEX64.test(x)), where, "teamProjection.includedEventIds is not a list of event ids");
    check(tp.assignments && typeof tp.assignments === "object", where, "teamProjection.assignments is missing");
    for (const [asg, row] of Object.entries(tp.assignments ?? {})) {
      check(HEX64.test(asg), where, "a teamProjection assignment key is not an event id");
      check(HEX64.test(row.assigneeActor ?? ""), where, `assignment ${asg} names no assignee`);
      check(typeof row.assigneeRole === "string" && row.assigneeRole.length > 0, where, `assignment ${asg} names no role`);
    }
    // A report naming an assignment the projection does not carry is a
    // deliberate NEGATIVE input (A7.4, R5 counterexample 1) — empty is
    // unproved, so the sequence must exist and must not cover anything with
    // it. The constraint is on the expected fold, in `checkFold`, not here.
    for (const [key, fact] of Object.entries(inputs.evidence ?? {})) {
      check(key === fact.eventId, where, `evidence is keyed ${key} but the fact names ${fact.eventId}`);
      check(HEX64.test(fact.eventId ?? ""), where, "an evidence fact id is not 64 hex");
      check(EVIDENCE_KINDS.includes(fact.kind), where, `evidence fact kind "${fact.kind}" is not closed-set`);
      if (fact.kind === "report") {
        check(HEX64.test(fact.signer ?? ""), where, "a report fact has no signer");
        check(HEX64.test(fact.assignmentRef ?? ""), where, "a report fact has no assignmentRef");
        check(COMMIT.test(fact.headSha ?? ""), where, "a report fact has no 40/64-hex headSha");
      }
      if (fact.kind === "verdict") {
        check(HEX64.test(fact.signer ?? ""), where, "a verdict fact has no signer");
        check(["disposition", "refutation"].includes(fact.subtype), where, "a verdict fact has no closed subtype");
        check(typeof fact.decision === "string" && fact.decision.length > 0, where, "a verdict fact has no decision");
        check(HEX64.test(fact.assignmentRef ?? "") && HEX64.test(fact.reportRef ?? ""), where, "a verdict fact does not name its assignment and report");
      }
      if (fact.kind === "action_result") {
        check(HEX64.test(fact.exitedEventId ?? ""), where, "an action result carries no relay 46014 echo id");
        check(HEX64.test(fact.resultSigner ?? ""), where, "an action result has no result signer");
        // An echo the relay did not sign is a negative input with its own
        // branch (`wrong_run_or_hash/echo-signer`), so this only asserts the
        // field is an event signer at all; `checkFold` forbids covering with it.
        check(HEX64.test(fact.echoSigner ?? ""), where, "an action result's echo signer is not 64 hex");
        check(HEX64.test(fact.definitionHash ?? ""), where, "an action result has no 64-hex definitionHash");
        check(UUID.test(fact.runId ?? ""), where, "an action result has no run uuid");
        check(SLUG.test(fact.stepId ?? ""), where, "an action result has no step id");
        check(Number.isInteger(fact.exitCode), where, "an action result has no integer exit code");
        check(typeof fact.dirty === "boolean", where, "an action result does not say whether the tree ended dirty");
        check(COMMIT.test(fact.checkout?.sha ?? ""), where, "an action result's checkout names no commit");
        check(typeof fact.checkout?.dirtyBefore === "boolean", where, "an action result does not say whether the tree started dirty");
      }
    }
    check(HEX64.test(inputs.currentGoalRef ?? ""), where, "inputs.currentGoalRef is not a 64-hex goal event id");
    // `null` is a supplied fact, not an omission: the caller could not
    // establish the relay's self key, and `relay-self-key-absent` pins what
    // a git-ref criterion reads then.
    check(inputs.relaySelfKey === null || HEX64.test(inputs.relaySelfKey ?? ""), where,
      "inputs.relaySelfKey is neither null nor 64 hex");
    check("relaySelfKey" in inputs, where, "inputs omits relaySelfKey; nullable keys are written as null");
    check(Array.isArray(inputs.goalEvents) && inputs.goalEvents.every((g) => HEX64.test(g)), where, "inputs.goalEvents is not a list of 64-hex goal ids");
    for (const rs of inputs.refStates ?? []) {
      check(rs.kind === KIND_GIT_REPO_STATE, where, `a ref state is kind ${rs.kind}, not ${KIND_GIT_REPO_STATE}`);
      check(HEX64.test(rs.pubkey ?? ""), where, "a ref state has no 64-hex signer");
      if (inputs.relaySelfKey !== null) {
        check(rs.pubkey === inputs.relaySelfKey, where, "a ref state is not signed by the relay's self key");
      }
      check(HEX64.test(rs.id ?? ""), where, "a ref state id is not 64 hex");
      check(Number.isInteger(rs.created_at), where, "a ref state has no fixed timestamp");
      const d = (rs.tags ?? []).find((t) => t[0] === "d");
      check(Boolean(d), where, "a ref state carries no d tag naming its repository");
      const heads = (rs.tags ?? []).filter((t) => t[0].startsWith("refs/heads/"));
      check(heads.length >= 1, where, "a ref state names no branch");
      for (const h of heads) check(COMMIT.test(h[1]), where, `ref ${h[0]} does not name a 40/64-hex commit`);
    }
    for (const [key, rel] of Object.entries(inputs.planBlobs ?? {})) {
      check(/^30617:[0-9a-f]{64}:[a-z0-9-]+@(?:[0-9a-f]{40}|[0-9a-f]{64}):plans\/[a-z0-9-]+\.md$/.test(key), where, `plan blob key is malformed: ${key}`);
      check(existsSync(resolve(dir, rel)), where, `plan blob fixture is missing: ${rel}`);
    }
    checkFold(fold, where, planCriteria, { ids, inputs, mayLead, events });

    // Derived from the events, not from the expected file: `supersedes` must
    // be what the record carries, and `assignmentRefs` must list every valid
    // assignment binding for that criterion under that declaration — with or
    // without evidence, because "who owes this" is what an unfinished
    // criterion is for.
    const records = events.map((e) => ({ id: e.id, pubkey: e.pubkey, body: JSON.parse(e.content) }));
    const valid = (r) => mayLead(r.pubkey);
    for (const d of fold.declarations) {
      const record = records.find((r) => r.id === d.declarationRef);
      if (record) {
        check(JSON.stringify(d.supersedes) === JSON.stringify(record.body.body.supersedes), where,
          `declaration ${short(d.declarationRef)} projects supersedes ${JSON.stringify(d.supersedes)} but its event recorded ${JSON.stringify(record.body.body.supersedes)}`);
      }
      // An unresolved plan projects no bindings on its rows at all (checked in
      // `checkFold`), so there is nothing to derive here.
      for (const c of d.planResolved ? d.criteria : []) {
        const expected = [...new Set(records
          .filter((r) => r.body.type === "work.assignment_bound" && valid(r)
            && r.body.body.declarationRef === d.declarationRef
            && r.body.body.criterionIds.includes(c.criterionId))
          .map((r) => r.body.body.assignmentRef))].sort();
        check(JSON.stringify([...c.assignmentRefs].sort()) === JSON.stringify(expected), where,
          `criterion "${c.criterionId}" under ${short(d.declarationRef)} projects assignmentRefs ${JSON.stringify(c.assignmentRefs)}, but the events bind ${JSON.stringify(expected)}`);
      }
    }
  }
}

// --------------------------------------------------- the README's own example
// § (c) shows a coverage document. It is checked by the same rules as every
// sequence, so the normative text and the fixtures cannot drift apart in key
// names or in status and reason spellings.
function checkReadmeExample(planCriteria) {
  const where = "README.md § (c) example";
  const text = readFileSync(join(HERE, "README.md"), "utf8");
  const marker = "<!-- check-fixtures: fold-example -->";
  const at = text.indexOf(marker);
  if (!check(at >= 0, where, `no ${marker} in README.md`)) return;
  const open = text.indexOf("```json", at);
  const close = text.indexOf("```", open + 7);
  if (!check(open >= 0 && close > open, where, "the marker is not followed by a json block")) return;
  let fold;
  try {
    fold = JSON.parse(text.slice(open + 7, close));
  } catch (e) {
    return fail(where, `the example is not JSON: ${e.message}`);
  }
  checkFold(fold, where, planCriteria, null);
}

// ------------------------------------------------- coverage of the oracle itself
// An oracle that names a failure nothing demonstrates is a promise, not a test.
// Two directions, both required (A7.4): every reason CODE the contract defines
// is produced by some sequence, and every BRANCH template in the README's table
// is produced by some sequence. The second is the stronger one — it is what
// stops a code path from borrowing another branch's sentence.
function checkOracleCoverage() {
  const where = "oracle coverage";
  for (const code of REASON_CODES) {
    const branches = TEMPLATES.filter((t) => t.code === code);
    if (!check(branches.length > 0, where,
      `reasonCode "${code}" has no template row in the README's table`)) continue;
    check(branches.some((t) => exercisedBranches.has(`${code}/${t.branch}`)), where,
      `no sequence exercises reasonCode "${code}": add one rather than documenting a reason nothing produces`);
  }
  for (const t of TEMPLATES) {
    check(exercisedBranches.has(`${t.code}/${t.branch}`), where,
      `no sequence exercises the template "${t.code}/${t.branch}": every branch the contract spells out needs a fixture`);
  }
}

// ---------------------------------------------------------------------- main
const planCriteria = checkValidPlan(join(FIX, "plans/valid/kettle.md")) ?? new Set();
for (const name of readdirSync(join(FIX, "plans/invalid")).sort()) {
  checkInvalidPlan(join(FIX, "plans/invalid", name));
}
checkRecords();
checkSequences(planCriteria);
checkReadmeExample(planCriteria);
checkOracleCoverage();

if (failures.length > 0) {
  console.error(`conformance/project-work: ${failures.length} fixture problem(s)`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log("conformance/project-work: fixtures are well formed");
